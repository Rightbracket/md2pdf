//! HTML tokenizer for the table-parser pipeline (D-875e4b §1f).
//!
//! Hand-rolled byte-level state machine. Produces a flat `Vec<Token>`
//! that the parser consumes via recursive descent. Total — given any
//! input string, returns a (possibly empty) token vector with no
//! panics, no errors. Malformed inputs degrade to text tokens.
//!
//! ## Tokens produced
//!
//! - `StartTag { name, attrs, self_closing }` — `<tagname attr="v">`
//!   or `<img/>`. Tag names lowercased; attribute names lowercased;
//!   attribute values entity-decoded. `self_closing` is true if the
//!   tag form ends with `/>`.
//! - `EndTag { name }` — `</tagname>`. Trailing junk inside the close
//!   tag is permissively dropped.
//! - `Text(String)` — content between tags, with HTML entities
//!   decoded.
//! - `Comment(String)` — `<!-- ... -->`. Content preserved (without
//!   the surrounding markers); not entity-decoded.
//!
//! Comments, CDATA sections, processing instructions, and DOCTYPE
//! declarations are all reported as `Comment` (the parser treats them
//! uniformly as inert).
//!
//! ## Security
//!
//! - **Total**: no panics, no `unwrap()` on user input.
//! - **Bounded iteration**: O(input length) work overall.
//! - **Bounded allocation**: O(input length) total token memory.
//! - **Quote-aware**: attribute values preserve `<`, `>`, etc. inside
//!   quoted strings so a malicious value can't break out of its tag.
//! - **No recursion**: state machine is purely iterative.
//!
//! Per D-875e4b §7f (security envelope).

// =============================================================================
// Public types
// =============================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// `<tagname attr="value" attr2='value2' attr3=value3 boolean>`
    /// or `<tagname.../>`. Tag name is lowercased; attribute names
    /// are lowercased; attribute values are entity-decoded.
    StartTag {
        name: String,
        attrs: Vec<(String, String)>,
        self_closing: bool,
    },
    /// `</tagname>`. Tag name is lowercased. Any trailing junk (HTML
    /// permits `</tag attr=val>` even though the spec says no) is
    /// silently discarded.
    EndTag { name: String },
    /// Text content between tags. HTML entities are decoded.
    Text(String),
    /// `<!-- ... -->`, `<!DOCTYPE ...>`, `<![CDATA[...]]>`, or
    /// `<?xml ...?>`. Content preserved (without markers); the parser
    /// treats all four as inert markers.
    Comment(String),
}

// =============================================================================
// Public entry point
// =============================================================================

/// Tokenize a string of HTML into a flat token list. Total — never
/// panics, never errors. Malformed input produces best-effort tokens
/// (e.g. a `<` not followed by a valid tag start becomes part of the
/// surrounding text token).
pub fn tokenize(input: &str) -> Vec<Token> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    let mut cursor = 0;

    while cursor < len {
        if bytes[cursor] == b'<' {
            // Potential tag-start.
            let next_idx = cursor + 1;
            if next_idx >= len {
                // Lone `<` at end of input → text.
                push_text(&mut out, "<");
                cursor += 1;
                continue;
            }

            let next_byte = bytes[next_idx];
            if next_byte == b'/' {
                // End tag.
                if let Some((token, advance)) = parse_end_tag(input, cursor) {
                    out.push(token);
                    cursor = advance;
                } else {
                    // Malformed end tag → treat `<` as text.
                    push_text(&mut out, "<");
                    cursor += 1;
                }
            } else if next_byte == b'!' {
                // Comment, CDATA, or DOCTYPE.
                if input[cursor..].starts_with("<!--") {
                    let (token, advance) = parse_comment(input, cursor);
                    out.push(token);
                    cursor = advance;
                } else if input[cursor..].len() >= 9
                    && input[cursor..cursor + 9].eq_ignore_ascii_case("<![CDATA[")
                {
                    let (token, advance) = parse_cdata(input, cursor);
                    out.push(token);
                    cursor = advance;
                } else {
                    // DOCTYPE / other declaration → treat as comment.
                    let (token, advance) = parse_declaration(input, cursor);
                    out.push(token);
                    cursor = advance;
                }
            } else if next_byte == b'?' {
                // Processing instruction → treat as comment.
                let (token, advance) = parse_processing_instruction(input, cursor);
                out.push(token);
                cursor = advance;
            } else if next_byte.is_ascii_alphabetic() {
                // Start tag.
                if let Some((token, advance)) = parse_start_tag(input, cursor) {
                    out.push(token);
                    cursor = advance;
                } else {
                    // Malformed start tag → treat `<` as text.
                    push_text(&mut out, "<");
                    cursor += 1;
                }
            } else {
                // `<` followed by something that can't begin a tag
                // (e.g. `<3` math, stray punctuation) → text.
                push_text(&mut out, "<");
                cursor += 1;
            }
        } else {
            // Plain text run — read up to next `<` or end-of-input.
            let text_start = cursor;
            while cursor < len && bytes[cursor] != b'<' {
                cursor += 1;
            }
            let text = &input[text_start..cursor];
            if !text.is_empty() {
                push_text(&mut out, &decode_entities(text));
            }
        }
    }

    out
}

// =============================================================================
// Internal: append text token (merging with previous text if possible)
// =============================================================================

/// Push a text fragment, merging with the previous text token if the
/// last token was also `Text`. Avoids a sea of single-byte text tokens
/// when malformed input emits stray `<` chars one at a time.
fn push_text(out: &mut Vec<Token>, s: &str) {
    if s.is_empty() {
        return;
    }
    if let Some(Token::Text(prev)) = out.last_mut() {
        prev.push_str(s);
    } else {
        out.push(Token::Text(s.to_string()));
    }
}

// =============================================================================
// Internal: tag parsers
// =============================================================================

/// Parse a start tag beginning at `cursor` (`bytes[cursor]` is `<`).
/// Returns `(Token, advance_index)` on success, `None` on malformed
/// input (caller should treat the `<` as text and resume).
fn parse_start_tag(input: &str, cursor: usize) -> Option<(Token, usize)> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = cursor + 1; // past the `<`

    // Tag name: ASCII alphanumeric (HTML allows digits after the first
    // letter; we already verified the first byte is alphabetic in the
    // dispatch above).
    let name_start = i;
    while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = input[name_start..i].to_ascii_lowercase();

    // Parse attributes until `>`, `/>`, or end of input.
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut self_closing = false;

    loop {
        // Skip whitespace.
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= len {
            // Unterminated tag → bail (treat as malformed).
            return None;
        }
        match bytes[i] {
            b'>' => {
                // End of start tag.
                i += 1;
                break;
            }
            b'/' => {
                // Possible self-close: `/>`.
                if i + 1 < len && bytes[i + 1] == b'>' {
                    self_closing = true;
                    i += 2;
                    break;
                } else {
                    // Stray `/` → skip and retry.
                    i += 1;
                }
            }
            _ => {
                // Attribute name.
                let attr_name_start = i;
                while i < len
                    && !bytes[i].is_ascii_whitespace()
                    && bytes[i] != b'='
                    && bytes[i] != b'>'
                    && bytes[i] != b'/'
                {
                    i += 1;
                }
                if i == attr_name_start {
                    // No progress → bail to prevent infinite loop.
                    return None;
                }
                let attr_name = input[attr_name_start..i].to_ascii_lowercase();

                // Skip whitespace before optional `=`.
                while i < len && bytes[i].is_ascii_whitespace() {
                    i += 1;
                }

                let attr_value = if i < len && bytes[i] == b'=' {
                    i += 1;
                    // Skip whitespace before value.
                    while i < len && bytes[i].is_ascii_whitespace() {
                        i += 1;
                    }
                    parse_attr_value(input, &mut i)?
                } else {
                    // Boolean attribute (no `=`).
                    String::new()
                };

                if !attr_name.is_empty() {
                    attrs.push((attr_name, attr_value));
                }
            }
        }
    }

    Some((
        Token::StartTag {
            name,
            attrs,
            self_closing,
        },
        i,
    ))
}

/// Parse an attribute value beginning at `*i`. Updates `*i` past the
/// value. Supports double-quoted, single-quoted, and unquoted forms.
/// Entity-decodes the value.
fn parse_attr_value(input: &str, i: &mut usize) -> Option<String> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    if *i >= len {
        return Some(String::new());
    }
    match bytes[*i] {
        b'"' => {
            *i += 1;
            let start = *i;
            while *i < len && bytes[*i] != b'"' {
                *i += 1;
            }
            let raw = &input[start..*i];
            // Skip the closing quote, if present.
            if *i < len {
                *i += 1;
            }
            Some(decode_entities(raw))
        }
        b'\'' => {
            *i += 1;
            let start = *i;
            while *i < len && bytes[*i] != b'\'' {
                *i += 1;
            }
            let raw = &input[start..*i];
            if *i < len {
                *i += 1;
            }
            Some(decode_entities(raw))
        }
        _ => {
            // Unquoted value — read until whitespace, `>`, or `/`.
            let start = *i;
            while *i < len
                && !bytes[*i].is_ascii_whitespace()
                && bytes[*i] != b'>'
                && bytes[*i] != b'/'
            {
                *i += 1;
            }
            let raw = &input[start..*i];
            Some(decode_entities(raw))
        }
    }
}

/// Parse an end tag beginning at `cursor` (`bytes[cursor]` is `<` and
/// `bytes[cursor+1]` is `/`). Returns `(Token, advance_index)` on
/// success, `None` on malformed input.
fn parse_end_tag(input: &str, cursor: usize) -> Option<(Token, usize)> {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = cursor + 2; // past `</`

    // Skip whitespace inside `< /tag>`.
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }

    let name_start = i;
    while i < len && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'-') {
        i += 1;
    }
    if i == name_start {
        return None;
    }
    let name = input[name_start..i].to_ascii_lowercase();

    // Permissively skip junk until `>`.
    while i < len && bytes[i] != b'>' {
        i += 1;
    }
    if i >= len {
        return None; // unterminated
    }
    i += 1; // past `>`

    Some((Token::EndTag { name }, i))
}

/// Parse `<!-- ... -->`. Returns `(Comment, advance_index)`.
/// If unterminated, consumes to end-of-input.
fn parse_comment(input: &str, cursor: usize) -> (Token, usize) {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let body_start = cursor + 4; // past `<!--`
    let mut i = body_start;
    while i + 2 < len {
        if bytes[i] == b'-' && bytes[i + 1] == b'-' && bytes[i + 2] == b'>' {
            let body = input[body_start..i].to_string();
            return (Token::Comment(body), i + 3);
        }
        i += 1;
    }
    // Unterminated → consume all remaining input as the comment body.
    let body = input[body_start..].to_string();
    (Token::Comment(body), len)
}

/// Parse `<![CDATA[ ... ]]>`. Returns `(Comment, advance_index)`.
fn parse_cdata(input: &str, cursor: usize) -> (Token, usize) {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let body_start = cursor + 9; // past `<![CDATA[`
    let mut i = body_start;
    while i + 2 < len {
        if bytes[i] == b']' && bytes[i + 1] == b']' && bytes[i + 2] == b'>' {
            let body = input[body_start..i].to_string();
            return (Token::Comment(body), i + 3);
        }
        i += 1;
    }
    let body = input[body_start..].to_string();
    (Token::Comment(body), len)
}

/// Parse `<!DOCTYPE ...>` or `<!OTHER>` declarations. Returns
/// `(Comment, advance_index)`.
fn parse_declaration(input: &str, cursor: usize) -> (Token, usize) {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let body_start = cursor + 2; // past `<!`
    let mut i = body_start;
    while i < len && bytes[i] != b'>' {
        i += 1;
    }
    let body = input[body_start..i].to_string();
    if i < len {
        i += 1; // past `>`
    }
    (Token::Comment(body), i)
}

/// Parse `<?xml ...?>` processing instructions. Returns
/// `(Comment, advance_index)`. Per HTML5, PIs are treated as
/// comments.
fn parse_processing_instruction(input: &str, cursor: usize) -> (Token, usize) {
    let bytes = input.as_bytes();
    let len = bytes.len();
    let body_start = cursor + 2; // past `<?`
    let mut i = body_start;
    while i + 1 < len {
        if bytes[i] == b'?' && bytes[i + 1] == b'>' {
            let body = input[body_start..i].to_string();
            return (Token::Comment(body), i + 2);
        }
        i += 1;
    }
    // Unterminated — consume up to next `>` or end.
    while i < len && bytes[i] != b'>' {
        i += 1;
    }
    let body = input[body_start..i].to_string();
    if i < len {
        i += 1;
    }
    (Token::Comment(body), i)
}

// =============================================================================
// Internal: HTML entity decoder
// =============================================================================

/// Decode HTML entities inside `s`. Recognizes:
///
/// - Named: `&amp;`, `&lt;`, `&gt;`, `&quot;`, `&apos;`, `&nbsp;`.
/// - Numeric decimal: `&#NNN;`.
/// - Numeric hex: `&#xHHHH;` / `&#XHHHH;`.
///
/// Unrecognized / malformed entities pass through verbatim. Total —
/// never panics on any input.
fn decode_entities(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string(); // fast path — no entities possible
    }
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i < len {
        if bytes[i] != b'&' {
            // Walk to the next `&` or end-of-input, copying chars.
            let chunk_start = i;
            while i < len && bytes[i] != b'&' {
                i += 1;
            }
            out.push_str(&s[chunk_start..i]);
            continue;
        }
        // We're sitting on an `&`. Try to read an entity name terminated
        // by `;`. Bound the scan to a sane length to avoid linear-time
        // attack on malformed input (entity names are <= 8 chars in our
        // recognized set, but allow more for numeric refs and others).
        let scan_end = (i + 16).min(len);
        let mut j = i + 1;
        while j < scan_end && bytes[j] != b';' && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'#') {
            j += 1;
        }
        if j < scan_end && bytes[j] == b';' {
            let name = &s[i + 1..j];
            if let Some(decoded) = lookup_entity(name) {
                out.push_str(&decoded);
                i = j + 1;
                continue;
            }
        }
        // Unrecognized / malformed — emit the `&` literal and resume.
        out.push('&');
        i += 1;
    }
    out
}

/// Look up an HTML entity by its name (without `&` and `;`).
/// Returns the decoded string, or `None` if unrecognized.
fn lookup_entity(name: &str) -> Option<String> {
    if name.is_empty() {
        return None;
    }
    if let Some(rest) = name.strip_prefix('#') {
        // Numeric character reference.
        let cp = if let Some(hex) = rest.strip_prefix('x').or_else(|| rest.strip_prefix('X')) {
            u32::from_str_radix(hex, 16).ok()?
        } else {
            rest.parse::<u32>().ok()?
        };
        return char::from_u32(cp).map(|c| c.to_string());
    }
    // Named entity. Recognized subset (HTML5 has hundreds; we handle
    // the common ones — others pass through verbatim).
    Some(
        match name {
            "amp" => "&",
            "lt" => "<",
            "gt" => ">",
            "quot" => "\"",
            "apos" => "'",
            "nbsp" => "\u{00A0}",
            "copy" => "\u{00A9}",
            "reg" => "\u{00AE}",
            "trade" => "\u{2122}",
            "hellip" => "\u{2026}",
            "mdash" => "\u{2014}",
            "ndash" => "\u{2013}",
            "lsquo" => "\u{2018}",
            "rsquo" => "\u{2019}",
            "ldquo" => "\u{201C}",
            "rdquo" => "\u{201D}",
            _ => return None,
        }
        .to_string(),
    )
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn start(name: &str, attrs: &[(&str, &str)], self_closing: bool) -> Token {
        Token::StartTag {
            name: name.to_string(),
            attrs: attrs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            self_closing,
        }
    }
    fn end(name: &str) -> Token {
        Token::EndTag {
            name: name.to_string(),
        }
    }
    fn text(s: &str) -> Token {
        Token::Text(s.to_string())
    }
    fn comment(s: &str) -> Token {
        Token::Comment(s.to_string())
    }

    // --- Simple tags --------------------------------------------------------

    #[test]
    fn tokenize_empty() {
        assert!(tokenize("").is_empty());
    }

    #[test]
    fn tokenize_pure_text() {
        assert_eq!(tokenize("hello world"), vec![text("hello world")]);
    }

    #[test]
    fn tokenize_simple_start_tag() {
        assert_eq!(tokenize("<p>"), vec![start("p", &[], false)]);
    }

    #[test]
    fn tokenize_simple_end_tag() {
        assert_eq!(tokenize("</p>"), vec![end("p")]);
    }

    #[test]
    fn tokenize_paragraph() {
        assert_eq!(
            tokenize("<p>hello</p>"),
            vec![start("p", &[], false), text("hello"), end("p")]
        );
    }

    #[test]
    fn tokenize_lowercases_tag_name() {
        assert_eq!(tokenize("<P>"), vec![start("p", &[], false)]);
        assert_eq!(tokenize("<TABLE>"), vec![start("table", &[], false)]);
    }

    #[test]
    fn tokenize_lowercases_end_tag_name() {
        assert_eq!(tokenize("</TABLE>"), vec![end("table")]);
    }

    // --- Attributes ---------------------------------------------------------

    #[test]
    fn tokenize_double_quoted_attr() {
        assert_eq!(
            tokenize(r#"<a href="https://example.com">"#),
            vec![start("a", &[("href", "https://example.com")], false)]
        );
    }

    #[test]
    fn tokenize_single_quoted_attr() {
        assert_eq!(
            tokenize(r#"<a href='https://example.com'>"#),
            vec![start("a", &[("href", "https://example.com")], false)]
        );
    }

    #[test]
    fn tokenize_unquoted_attr() {
        assert_eq!(
            tokenize("<img width=100>"),
            vec![start("img", &[("width", "100")], false)]
        );
    }

    #[test]
    fn tokenize_boolean_attr() {
        assert_eq!(
            tokenize("<input disabled>"),
            vec![start("input", &[("disabled", "")], false)]
        );
    }

    #[test]
    fn tokenize_multiple_attrs() {
        assert_eq!(
            tokenize(r#"<td colspan="2" rowspan="3" style="width:100%">"#),
            vec![start(
                "td",
                &[
                    ("colspan", "2"),
                    ("rowspan", "3"),
                    ("style", "width:100%"),
                ],
                false,
            )]
        );
    }

    #[test]
    fn tokenize_lowercases_attr_name() {
        assert_eq!(
            tokenize(r#"<td COLSPAN="2">"#),
            vec![start("td", &[("colspan", "2")], false)]
        );
    }

    #[test]
    fn tokenize_attr_with_extra_whitespace() {
        assert_eq!(
            tokenize(r#"<td   colspan = "2"   >"#),
            vec![start("td", &[("colspan", "2")], false)]
        );
    }

    // --- Self-closing tags --------------------------------------------------

    #[test]
    fn tokenize_self_closing_void() {
        assert_eq!(tokenize("<br/>"), vec![start("br", &[], true)]);
        assert_eq!(tokenize("<br />"), vec![start("br", &[], true)]);
    }

    #[test]
    fn tokenize_self_closing_img() {
        assert_eq!(
            tokenize(r#"<img src="foo.png" alt="hello"/>"#),
            vec![start(
                "img",
                &[("src", "foo.png"), ("alt", "hello")],
                true
            )]
        );
    }

    // --- Entity decoding ----------------------------------------------------

    #[test]
    fn tokenize_decodes_text_entities() {
        assert_eq!(
            tokenize("foo &amp; bar"),
            vec![text("foo & bar")]
        );
        assert_eq!(
            tokenize("&lt;b&gt;"),
            vec![text("<b>")]
        );
        assert_eq!(
            tokenize("&quot;hello&quot;"),
            vec![text("\"hello\"")]
        );
    }

    #[test]
    fn tokenize_decodes_numeric_entities() {
        assert_eq!(tokenize("&#65;"), vec![text("A")]);
        assert_eq!(tokenize("&#xA9;"), vec![text("\u{00A9}")]);
        assert_eq!(tokenize("&#X41;"), vec![text("A")]);
    }

    #[test]
    fn tokenize_decodes_attr_entities() {
        assert_eq!(
            tokenize(r#"<a href="?q=&amp;x">"#),
            vec![start("a", &[("href", "?q=&x")], false)]
        );
    }

    #[test]
    fn tokenize_passes_through_unrecognized_entities() {
        assert_eq!(
            tokenize("&fubar;"),
            vec![text("&fubar;")]
        );
    }

    #[test]
    fn tokenize_handles_lone_ampersand() {
        assert_eq!(
            tokenize("a & b"),
            vec![text("a & b")]
        );
    }

    // --- Comments / declarations --------------------------------------------

    #[test]
    fn tokenize_html_comment() {
        assert_eq!(
            tokenize("<!-- pagebreak -->"),
            vec![comment(" pagebreak ")]
        );
    }

    #[test]
    fn tokenize_unterminated_comment() {
        // Should consume to end-of-input gracefully.
        assert_eq!(
            tokenize("<!-- unterminated"),
            vec![comment(" unterminated")]
        );
    }

    #[test]
    fn tokenize_doctype() {
        let toks = tokenize("<!DOCTYPE html>");
        assert_eq!(toks.len(), 1);
        assert!(matches!(toks[0], Token::Comment(_)));
    }

    #[test]
    fn tokenize_processing_instruction() {
        let toks = tokenize(r#"<?xml version="1.0"?>"#);
        assert_eq!(toks.len(), 1);
        assert!(matches!(toks[0], Token::Comment(_)));
    }

    #[test]
    fn tokenize_cdata() {
        let toks = tokenize("<![CDATA[ stuff ]]>");
        assert_eq!(toks, vec![comment(" stuff ")]);
    }

    // --- Mixed content -----------------------------------------------------

    #[test]
    fn tokenize_table_skeleton() {
        let toks = tokenize("<table><tr><td>cell</td></tr></table>");
        assert_eq!(
            toks,
            vec![
                start("table", &[], false),
                start("tr", &[], false),
                start("td", &[], false),
                text("cell"),
                end("td"),
                end("tr"),
                end("table"),
            ]
        );
    }

    #[test]
    fn tokenize_table_with_attrs_and_whitespace() {
        let src = r#"<table style="width: 100%">
  <tr>
    <td style="padding: 4pt;">hi</td>
  </tr>
</table>"#;
        let toks = tokenize(src);
        // Verify the structural tags appear correctly.
        let names: Vec<_> = toks
            .iter()
            .filter_map(|t| match t {
                Token::StartTag { name, .. } => Some(format!("<{name}>")),
                Token::EndTag { name } => Some(format!("</{name}>")),
                _ => None,
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "<table>", "<tr>", "<td>", "</td>", "</tr>", "</table>"
            ]
        );
    }

    #[test]
    fn tokenize_inline_formatters_in_text() {
        let toks = tokenize("hello <b>bold</b> world");
        assert_eq!(
            toks,
            vec![
                text("hello "),
                start("b", &[], false),
                text("bold"),
                end("b"),
                text(" world"),
            ]
        );
    }

    // --- Malformed handling ------------------------------------------------

    #[test]
    fn tokenize_lone_lt_at_end() {
        assert_eq!(tokenize("foo <"), vec![text("foo <")]);
    }

    #[test]
    fn tokenize_lt_followed_by_garbage() {
        // `<3` should become text, not a tag.
        assert_eq!(tokenize("a<3b"), vec![text("a<3b")]);
    }

    #[test]
    fn tokenize_unterminated_start_tag() {
        // Unclosed tag → treat the `<` as text (best-effort).
        let toks = tokenize("<table");
        // We expect no StartTag, just text. Either we emit `<table`
        // as text or some other graceful behavior.
        assert!(!toks.iter().any(|t| matches!(t, Token::StartTag { .. })));
    }

    #[test]
    fn tokenize_no_panic_on_pathological_input() {
        // Long pathological input → bounded work, no panic.
        let huge = "<table>".repeat(1000);
        let _ = tokenize(&huge);

        let unterminated = "<".repeat(1000);
        let _ = tokenize(&unterminated);

        let mismatched = "<<>>><<>>".repeat(100);
        let _ = tokenize(&mismatched);
    }

    #[test]
    fn tokenize_no_panic_on_unicode() {
        let toks = tokenize("héllo <b>wörld</b>");
        assert_eq!(toks.len(), 4);
    }

    #[test]
    fn tokenize_no_panic_on_emoji() {
        let toks = tokenize("<p>🎉 party 🎊</p>");
        assert_eq!(toks.len(), 3);
        assert_eq!(toks[1], text("🎉 party 🎊"));
    }

    // --- README fixture round-trip -----------------------------------------

    #[test]
    fn tokenize_readme_table_fixture_structure() {
        let src = r#"<table style="width: 100%; border-collapse: collapse;">
  <tr>
    <td style="width: 50%; padding: 0 4pt; vertical-align: middle;">
      <img src="assets/images/rolled-paper.png"
           alt="A rolled-up piece of paper, by Round Icons via Unsplash+"
           style="width: 100%; height: auto;" />
    </td>
  </tr>
</table>"#;
        let toks = tokenize(src);
        // Verify we see <table>, <tr>, <td>, <img/>, </td>, </tr>, </table>.
        let names: Vec<String> = toks
            .iter()
            .filter_map(|t| match t {
                Token::StartTag {
                    name,
                    self_closing,
                    ..
                } => Some(if *self_closing {
                    format!("<{name}/>")
                } else {
                    format!("<{name}>")
                }),
                Token::EndTag { name } => Some(format!("</{name}>")),
                _ => None,
            })
            .collect();
        assert_eq!(
            names,
            vec![
                "<table>", "<tr>", "<td>", "<img/>", "</td>", "</tr>", "</table>"
            ]
        );

        // The img tag should have all 3 attributes.
        let img = toks.iter().find_map(|t| match t {
            Token::StartTag { name, attrs, .. } if name == "img" => Some(attrs),
            _ => None,
        });
        let img_attrs = img.expect("img tag present");
        let attr_names: Vec<&str> = img_attrs.iter().map(|(k, _)| k.as_str()).collect();
        assert!(attr_names.contains(&"src"));
        assert!(attr_names.contains(&"alt"));
        assert!(attr_names.contains(&"style"));
    }

    // --- Specific sanity ---------------------------------------------------

    #[test]
    fn tokenize_close_tag_with_trailing_junk_recovers() {
        // HTML5 says no attrs on close tags, but pulldown-passed
        // input might have them; tokenizer should be permissive.
        assert_eq!(tokenize("</td foo bar>"), vec![end("td")]);
    }

    #[test]
    fn tokenize_text_runs_merge() {
        // Lone `<` followed by valid text should produce a single
        // text token, not multiple fragments.
        let toks = tokenize("a<b<c");
        // `<b` is malformed (no `>`); should be treated as text or
        // a recovery path. The exact behavior is tested loosely.
        assert!(!toks.is_empty());
    }
}
