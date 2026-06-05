//! Recognized inline-HTML element classifier (D-875e4b §2h v2 +
//! U-ad8c6c v2).
//!
//! This module provides:
//!
//! - `RecognizedInlineFormatter` — the inline-formatter axis we
//!   recognize structurally (Bold = `<b>`/`<strong>`, Italic =
//!   `<i>`/`<em>`).
//!
//! - `InlineHtmlKind` — the classification result for a single
//!   `Event::InlineHtml(s)` payload (or for one inline-position tag
//!   inside HTML-table cell content).
//!
//! - `try_classify_inline_html(s)` — the single source of truth used
//!   by BOTH the outside-cell streaming Typst-markup path
//!   (`emitter.rs`'s new `Event::InlineHtml` handler) AND the
//!   inside-cell IR-build path (`html::parser` walking tokenized cell
//!   content). Both paths share the same recognition rules:
//!   case-insensitive, attribute-tolerant, fixed 5-element subset.
//!
//! ## Recognition rules (per D-875e4b §2h.1)
//!
//! - Tag-name match is **case-insensitive** (`<B>`, `<Strong>`, `<EM>`
//!   all match).
//! - **Attributes are tolerated and ignored** on all recognized
//!   elements per U-ad8c6c "Other attributes ... have no rendering
//!   effect".
//! - **Self-closing forms.** `<br>`, `<br/>`, `<br />` all classify as
//!   `SelfClosingBr`. `<b/>`, `<strong/>`, `<i/>`, `<em/>` classify as
//!   `Unrecognized` (these elements are not legitimately self-closing
//!   in HTML; treat as malformed).
//! - **Malformed forms** (unterminated tag, weird chars in name): →
//!   `Unrecognized`.
//! - **Comments / CDATA / processing instructions / DOCTYPEs** (`<!-- foo -->`,
//!   `<![CDATA[...]]>`, `<?xml ?>`, `<!DOCTYPE>`): → `Unrecognized`.
//!
//! ## Security
//!
//! The classifier walks the input byte-by-byte with bounded iteration
//! (≤ input length). No allocation beyond a single tag-name lowercased
//! `String` (≤ 8 chars in practice). No recursion, no panics, no
//! `unwrap()` on user input. Per D-875e4b §7f.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecognizedInlineFormatter {
    /// `<b>` or `<strong>`. Typst markup: `*…*`.
    Bold,
    /// `<i>` or `<em>`. Typst markup: `_…_`.
    Italic,
}

impl RecognizedInlineFormatter {
    /// Typst markup-marker for an opening recognized inline.
    pub fn typst_open(self) -> &'static str {
        match self {
            Self::Bold => "*",
            Self::Italic => "_",
        }
    }
    /// Typst markup-marker for a closing recognized inline.
    pub fn typst_close(self) -> &'static str {
        match self {
            Self::Bold => "*",
            Self::Italic => "_",
        }
    }
}

/// Result of classifying a single inline-position HTML tag string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InlineHtmlKind {
    /// Recognized open tag (`<b>`, `<strong>`, `<i>`, `<em>` —
    /// case-insensitive, attribute-tolerant).
    OpenTag(RecognizedInlineFormatter),
    /// Recognized close tag (`</b>`, `</strong>`, `</i>`, `</em>` —
    /// case-insensitive; permissive about attribute-shaped trailing
    /// junk per HTML5 lenient parsing of close tags).
    CloseTag(RecognizedInlineFormatter),
    /// Recognized line-break: `<br>`, `<br/>`, `<br />` (any case;
    /// `<BR>`, `<Br />` etc. all match). The void-element nature of
    /// `<br>` means we accept any of these forms as equivalent.
    SelfClosingBr,
    /// Anything not matching the recognized subset. Includes
    /// non-recognized element names (`<u>`, `<span>`, `<font>`),
    /// malformed tags, comments, CDATA, processing instructions, and
    /// the (HTML-illegal but possible) self-closing forms of
    /// non-void recognized elements (`<b/>`, `<i/>`, etc.).
    Unrecognized,
}

/// Classify one inline-position HTML tag string against the recognized
/// 5-element subset (`<b>`, `<strong>`, `<i>`, `<em>`, `<br>`).
///
/// Returns one of:
/// - `OpenTag(formatter)` for `<b>`, `<strong>`, `<i>`, `<em>`
///   (without trailing self-close slash).
/// - `CloseTag(formatter)` for `</b>`, `</strong>`, `</i>`, `</em>`.
/// - `SelfClosingBr` for `<br>`, `<br/>`, `<br />` (any case).
/// - `Unrecognized` for everything else (other tag names, malformed
///   tags, comments, CDATA, etc.).
///
/// Case-insensitive on tag names; attribute-tolerant.
pub fn try_classify_inline_html(s: &str) -> InlineHtmlKind {
    // Strip outer whitespace.
    let s = s.trim();

    // Must be a full tag: `<...>`. Empty / unterminated / non-tag input
    // is unrecognized.
    if !s.starts_with('<') || !s.ends_with('>') || s.len() < 3 {
        return InlineHtmlKind::Unrecognized;
    }

    // Strip outer `<` and `>`.
    let inner = &s[1..s.len() - 1];
    let inner_bytes = inner.as_bytes();

    // Cursor over `inner`.
    let mut i: usize = 0;

    // Skip leading whitespace inside the brackets.
    while i < inner_bytes.len() && inner_bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    if i >= inner_bytes.len() {
        return InlineHtmlKind::Unrecognized;
    }

    // Detect end-tag form: leading `/`.
    let is_end = inner_bytes[i] == b'/';
    if is_end {
        i += 1;
        // Skip whitespace after `/`.
        while i < inner_bytes.len() && inner_bytes[i].is_ascii_whitespace() {
            i += 1;
        }
    }

    // Read the tag name (ASCII alphabetic chars only; HTML5 also
    // permits digits in tag names but our recognized set is purely
    // alphabetic so being strict is fine).
    let name_start = i;
    while i < inner_bytes.len() && inner_bytes[i].is_ascii_alphabetic() {
        i += 1;
    }
    if i == name_start {
        // No alphabetic tag name → not a normal HTML tag (could be a
        // comment, CDATA, processing instruction, DOCTYPE, or just
        // garbage). All unrecognized.
        return InlineHtmlKind::Unrecognized;
    }
    let name_lower = inner[name_start..i].to_ascii_lowercase();

    // The next character must be a valid tag-boundary char: whitespace,
    // `/` (preceding `>`), or end-of-inner. If it's something else
    // (e.g. a digit immediately after the alpha name), the tag has a
    // malformed name — treat as unrecognized.
    if i < inner_bytes.len() {
        let next = inner_bytes[i];
        if !next.is_ascii_whitespace() && next != b'/' {
            return InlineHtmlKind::Unrecognized;
        }
    }

    // Determine self-closing form: trailing `/` before the (already
    // stripped) `>`, after trimming any trailing whitespace.
    let rest = &inner[i..];
    let trimmed_rest = rest.trim_end();
    let self_closing = trimmed_rest.ends_with('/');

    // Match tag name against the recognized 5-element subset.
    match name_lower.as_str() {
        "br" => {
            // `</br>` is malformed (br is void; it has no end tag).
            if is_end {
                InlineHtmlKind::Unrecognized
            } else {
                // <br>, <br/>, <br />, <br aria-hidden="true"/> all
                // classify as SelfClosingBr.
                InlineHtmlKind::SelfClosingBr
            }
        }
        "b" | "strong" => {
            if is_end {
                InlineHtmlKind::CloseTag(RecognizedInlineFormatter::Bold)
            } else if self_closing {
                // <b/>, <strong/> are not legitimate self-closing
                // forms. Treat as malformed per Decision §2h.1.
                InlineHtmlKind::Unrecognized
            } else {
                InlineHtmlKind::OpenTag(RecognizedInlineFormatter::Bold)
            }
        }
        "i" | "em" => {
            if is_end {
                InlineHtmlKind::CloseTag(RecognizedInlineFormatter::Italic)
            } else if self_closing {
                InlineHtmlKind::Unrecognized
            } else {
                InlineHtmlKind::OpenTag(RecognizedInlineFormatter::Italic)
            }
        }
        _ => InlineHtmlKind::Unrecognized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_b() -> InlineHtmlKind {
        InlineHtmlKind::OpenTag(RecognizedInlineFormatter::Bold)
    }
    fn close_b() -> InlineHtmlKind {
        InlineHtmlKind::CloseTag(RecognizedInlineFormatter::Bold)
    }
    fn open_i() -> InlineHtmlKind {
        InlineHtmlKind::OpenTag(RecognizedInlineFormatter::Italic)
    }
    fn close_i() -> InlineHtmlKind {
        InlineHtmlKind::CloseTag(RecognizedInlineFormatter::Italic)
    }

    // --- Recognized open tags ------------------------------------------------

    #[test]
    fn recognizes_open_b() {
        assert_eq!(try_classify_inline_html("<b>"), open_b());
    }

    #[test]
    fn recognizes_open_strong_as_bold() {
        assert_eq!(try_classify_inline_html("<strong>"), open_b());
    }

    #[test]
    fn recognizes_open_i_as_italic() {
        assert_eq!(try_classify_inline_html("<i>"), open_i());
    }

    #[test]
    fn recognizes_open_em_as_italic() {
        assert_eq!(try_classify_inline_html("<em>"), open_i());
    }

    // --- Recognized close tags -----------------------------------------------

    #[test]
    fn recognizes_close_b() {
        assert_eq!(try_classify_inline_html("</b>"), close_b());
    }

    #[test]
    fn recognizes_close_strong() {
        assert_eq!(try_classify_inline_html("</strong>"), close_b());
    }

    #[test]
    fn recognizes_close_i() {
        assert_eq!(try_classify_inline_html("</i>"), close_i());
    }

    #[test]
    fn recognizes_close_em() {
        assert_eq!(try_classify_inline_html("</em>"), close_i());
    }

    // --- <br> variants -------------------------------------------------------

    #[test]
    fn recognizes_br_void() {
        assert_eq!(try_classify_inline_html("<br>"), InlineHtmlKind::SelfClosingBr);
    }

    #[test]
    fn recognizes_br_self_closed_no_space() {
        assert_eq!(try_classify_inline_html("<br/>"), InlineHtmlKind::SelfClosingBr);
    }

    #[test]
    fn recognizes_br_self_closed_with_space() {
        assert_eq!(
            try_classify_inline_html("<br />"),
            InlineHtmlKind::SelfClosingBr
        );
    }

    #[test]
    fn rejects_close_br() {
        // </br> is malformed (br is void).
        assert_eq!(
            try_classify_inline_html("</br>"),
            InlineHtmlKind::Unrecognized
        );
    }

    // --- Case-insensitivity --------------------------------------------------

    #[test]
    fn case_insensitive_uppercase_b() {
        assert_eq!(try_classify_inline_html("<B>"), open_b());
    }

    #[test]
    fn case_insensitive_mixed_case_strong() {
        assert_eq!(try_classify_inline_html("<Strong>"), open_b());
    }

    #[test]
    fn case_insensitive_uppercase_em() {
        assert_eq!(try_classify_inline_html("<EM>"), open_i());
    }

    #[test]
    fn case_insensitive_uppercase_br() {
        assert_eq!(
            try_classify_inline_html("<BR>"),
            InlineHtmlKind::SelfClosingBr
        );
    }

    #[test]
    fn case_insensitive_close_strong() {
        assert_eq!(try_classify_inline_html("</STRONG>"), close_b());
    }

    // --- Attribute tolerance -------------------------------------------------

    #[test]
    fn tolerates_class_attr_on_open_b() {
        assert_eq!(try_classify_inline_html("<b class=\"warning\">"), open_b());
    }

    #[test]
    fn tolerates_id_attr_on_open_i() {
        assert_eq!(try_classify_inline_html("<i id=\"foo\">"), open_i());
    }

    #[test]
    fn tolerates_data_attr_on_strong() {
        assert_eq!(
            try_classify_inline_html("<strong data-x=\"y\">"),
            open_b()
        );
    }

    #[test]
    fn tolerates_aria_attr_on_br() {
        assert_eq!(
            try_classify_inline_html("<br aria-hidden=\"true\">"),
            InlineHtmlKind::SelfClosingBr
        );
    }

    #[test]
    fn tolerates_aria_attr_self_closing_br() {
        assert_eq!(
            try_classify_inline_html("<br aria-hidden=\"true\"/>"),
            InlineHtmlKind::SelfClosingBr
        );
    }

    #[test]
    fn tolerates_extra_attr_on_close_tag() {
        // Per Decision §6k, attribute-shaped junk on a close tag is
        // tolerated (HTML5 says nothing should follow the name in a
        // close tag, but pulldown is permissive).
        assert_eq!(try_classify_inline_html("</b extra>"), close_b());
    }

    // --- Whitespace tolerance ------------------------------------------------

    #[test]
    fn tolerates_outer_whitespace() {
        assert_eq!(try_classify_inline_html("  <b>  "), open_b());
        assert_eq!(try_classify_inline_html("\n<b>\n"), open_b());
    }

    #[test]
    fn tolerates_inner_leading_whitespace() {
        assert_eq!(try_classify_inline_html("< b>"), open_b());
    }

    #[test]
    fn tolerates_whitespace_before_close_slash() {
        assert_eq!(try_classify_inline_html("< /b>"), close_b());
    }

    // --- Unrecognized ---------------------------------------------------------

    #[test]
    fn unrecognized_self_closing_b() {
        // <b/>, <strong/>, <i/>, <em/> are not legitimately
        // self-closing; per Decision §2h.1 → Unrecognized.
        assert_eq!(
            try_classify_inline_html("<b/>"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_self_closing_em() {
        assert_eq!(
            try_classify_inline_html("<em/>"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_u_tag() {
        assert_eq!(
            try_classify_inline_html("<u>"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_span_tag() {
        assert_eq!(
            try_classify_inline_html("<span>"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_font_tag() {
        assert_eq!(
            try_classify_inline_html("<font color=\"red\">"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_html_comment() {
        assert_eq!(
            try_classify_inline_html("<!-- foo -->"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_doctype() {
        assert_eq!(
            try_classify_inline_html("<!DOCTYPE html>"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_processing_instruction() {
        assert_eq!(
            try_classify_inline_html("<?xml version=\"1.0\"?>"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_unterminated_tag() {
        assert_eq!(
            try_classify_inline_html("<b"),
            InlineHtmlKind::Unrecognized
        );
        assert_eq!(
            try_classify_inline_html("b>"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_empty_tag() {
        assert_eq!(
            try_classify_inline_html("<>"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_empty_string() {
        assert_eq!(
            try_classify_inline_html(""),
            InlineHtmlKind::Unrecognized
        );
        assert_eq!(
            try_classify_inline_html("   "),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_garbled_name() {
        // Digits intermixed with the tag name → not a clean alpha
        // name → Unrecognized.
        assert_eq!(
            try_classify_inline_html("<b3>"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_plain_text() {
        assert_eq!(
            try_classify_inline_html("hello"),
            InlineHtmlKind::Unrecognized
        );
    }

    #[test]
    fn unrecognized_table_tag() {
        // table/tr/td/th/thead/tbody/img are NOT classified as inline
        // formatters even though the parser handles them; this
        // classifier is for the outside-cell inline path only.
        assert_eq!(
            try_classify_inline_html("<table>"),
            InlineHtmlKind::Unrecognized
        );
        assert_eq!(
            try_classify_inline_html("<td>"),
            InlineHtmlKind::Unrecognized
        );
    }

    // --- Typst markers --------------------------------------------------------

    #[test]
    fn typst_markers_for_bold() {
        assert_eq!(RecognizedInlineFormatter::Bold.typst_open(), "*");
        assert_eq!(RecognizedInlineFormatter::Bold.typst_close(), "*");
    }

    #[test]
    fn typst_markers_for_italic() {
        assert_eq!(RecognizedInlineFormatter::Italic.typst_open(), "_");
        assert_eq!(RecognizedInlineFormatter::Italic.typst_close(), "_");
    }
}
