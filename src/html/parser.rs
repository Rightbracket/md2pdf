//! HTML table parser (D-875e4b §1e–§1g, U-ad8c6c).
//!
//! Hand-rolled recursive-descent parser that converts a tokenized HTML
//! string into the `HtmlTable` IR. The IR is then emitted to Typst
//! by `crate::html::typst_emit`.
//!
//! ## Public surface
//!
//! - `try_parse_html_table(raw: &str) -> ParseOutcome` — the entry
//!   point called by the emitter at HtmlBlock-end. Returns one of
//!   three outcomes; the caller (`emitter.rs`) decides whether to
//!   emit the IR, fire a warning + fall through, or fall through
//!   silently.
//!
//! - `HtmlTable`, `Row`, `Cell`, `CellKind`, `CellContent`, `Inline`
//!   — the IR types. Cell-level CSS lives in `crate::html::css`.
//!
//! ## Structural strictness vs cell-content leniency
//!
//! - **Structural positions** (`<table>`, `<thead>`, `<tbody>`,
//!   `<tr>`): recognized children are `thead`, `tbody`, `tr`, `td`,
//!   `th`. Whitespace text and comments are skipped. **Anything else
//!   at a structural position triggers `ParseFailed`** per D-875e4b
//!   §1e ("Unrecognized child elements at structural positions").
//!
//! - **Cell-content positions** (`<td>`, `<th>`): inline content
//!   parsed leniently. Recognized inline elements (`<b>`, `<i>`,
//!   `<strong>`, `<em>`, `<br>`, `<img>`) produce `Inline` nodes.
//!   Unrecognized inline elements pass through transparently — their
//!   children fold into the parent content. Mismatched/unclosed
//!   inline formatters auto-close at cell end (matches the
//!   outside-cell streaming behavior of D-875e4b §2h). The single
//!   exception per D-875e4b §6e: a nested `<table>` inside `<td>`
//!   has its source content emitted as escaped text.
//!
//! ## Bounded recursion
//!
//! No recursion. The parser is purely iterative: a `TokenCursor`
//! walks the token slice with explicit stack-based frame management
//! for cell content. Per D-875e4b §7f.

use crate::html::css::{
    parse_img_style_attribute, parse_style_attribute, CellStyle, ImgStyle,
};
use crate::html::tokenizer::{tokenize, Token};

// =============================================================================
// IR types
// =============================================================================

/// The parsed HTML table.
#[derive(Debug, Clone, PartialEq)]
pub struct HtmlTable {
    /// Table-level styles (border-collapse, border, background-color,
    /// width). Cell-level properties (padding, vertical-align, etc.)
    /// are populated only on `Cell::style`, not here.
    pub style: CellStyle,
    /// Rows from `<thead>` blocks, in source order. Empty if no
    /// `<thead>` present.
    pub head_rows: Vec<Row>,
    /// Rows from `<tbody>` blocks AND any direct `<tr>` children of
    /// `<table>`, in source order.
    pub body_rows: Vec<Row>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub cells: Vec<Cell>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub kind: CellKind,
    /// `colspan` HTML attribute. Default 1; values < 1 clamped to 1.
    pub colspan: usize,
    /// `rowspan` HTML attribute. Default 1; values < 1 clamped to 1.
    pub rowspan: usize,
    /// Per-cell `style="..."` declarations.
    pub style: CellStyle,
    /// Cell body — mixed inline content.
    pub content: CellContent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    /// `<td>` body cell.
    Td,
    /// `<th>` header cell. Theme emit may bold the content.
    Th,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CellContent {
    /// Mixed inline content (text, recognized formatters, images,
    /// nested-table source fallbacks).
    Inlines(Vec<Inline>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Inline {
    /// Plain text. Already entity-decoded by the tokenizer; the Typst
    /// emitter is responsible for escaping for Typst markup.
    Text(String),
    /// `<b>` or `<strong>` body — recursive inlines.
    Bold(Vec<Inline>),
    /// `<i>` or `<em>` body — recursive inlines.
    Italic(Vec<Inline>),
    /// `<br>` self-closing line break.
    LineBreak,
    /// `<img src="..." alt="..." style="..."/>` — image reference.
    Image {
        src: String,
        alt: String,
        style: Option<ImgStyle>,
    },
}

// =============================================================================
// Parse outcome
// =============================================================================

/// The three outcomes of `try_parse_html_table`.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseOutcome {
    /// Input doesn't look like an HTML table at all (doesn't start
    /// with `<table` after trimming). The caller should fall through
    /// to the existing `md_inline_html` path silently — this is NOT
    /// an error.
    NotATable,
    /// Successful parse. The caller should emit the IR via
    /// `crate::html::typst_emit`.
    Parsed(HtmlTable),
    /// Input started with `<table` but failed to parse structurally.
    /// The caller should fire a `WarningSource::Emitter` warning
    /// containing the reason string and fall through to the existing
    /// `md_inline_html` raw pass-through.
    ParseFailed(String),
}

// =============================================================================
// Public entry point
// =============================================================================

/// Parse the trimmed raw text of an HtmlBlock. Returns the appropriate
/// `ParseOutcome` per the function's contract above.
pub fn try_parse_html_table(raw: &str) -> ParseOutcome {
    let trimmed = raw.trim();
    if !looks_like_table_open(trimmed) {
        return ParseOutcome::NotATable;
    }

    let tokens = tokenize(trimmed);
    let mut cursor = TokenCursor::new(&tokens);

    // Skip leading comments (we already verified the first non-comment
    // token is `<table>` via looks_like_table_open).
    skip_inert(&mut cursor);

    // Expect the opening <table> tag.
    let (table_attrs, _self_closing) = match cursor.peek() {
        Some(Token::StartTag { name, attrs, self_closing }) if name == "table" => {
            let attrs = attrs.clone();
            let sc = *self_closing;
            cursor.advance();
            (attrs, sc)
        }
        _ => {
            return ParseOutcome::ParseFailed(
                "expected <table> at start (after stripping leading comments)"
                    .to_string(),
            );
        }
    };

    let table_style = style_from_attrs(&table_attrs);

    let mut head_rows: Vec<Row> = Vec::new();
    let mut body_rows: Vec<Row> = Vec::new();
    let mut saw_table_close = false;

    // Parse table contents.
    while let Some(token) = cursor.peek().cloned() {
        match &token {
            Token::Comment(_) => {
                cursor.advance();
            }
            Token::Text(s) if is_whitespace_only(s) => {
                cursor.advance();
            }
            Token::Text(_) => {
                return ParseOutcome::ParseFailed(
                    "non-whitespace text directly inside <table>".to_string(),
                );
            }
            Token::StartTag {
                name, self_closing, ..
            } => match name.as_str() {
                "thead" => {
                    if *self_closing {
                        cursor.advance();
                        continue;
                    }
                    cursor.advance();
                    match parse_section(&mut cursor, "thead") {
                        Ok(rows) => head_rows.extend(rows),
                        Err(e) => return ParseOutcome::ParseFailed(e),
                    }
                }
                "tbody" => {
                    if *self_closing {
                        cursor.advance();
                        continue;
                    }
                    cursor.advance();
                    match parse_section(&mut cursor, "tbody") {
                        Ok(rows) => body_rows.extend(rows),
                        Err(e) => return ParseOutcome::ParseFailed(e),
                    }
                }
                "tr" => {
                    if *self_closing {
                        cursor.advance();
                        continue;
                    }
                    cursor.advance();
                    match parse_row(&mut cursor) {
                        Ok(row) => body_rows.push(row),
                        Err(e) => return ParseOutcome::ParseFailed(e),
                    }
                }
                other => {
                    return ParseOutcome::ParseFailed(format!(
                        "unrecognized element <{other}> at table-level (expected thead/tbody/tr)"
                    ));
                }
            },
            Token::EndTag { name } if name == "table" => {
                cursor.advance();
                saw_table_close = true;
                break;
            }
            Token::EndTag { name } => {
                return ParseOutcome::ParseFailed(format!(
                    "unexpected </{name}> at table-level"
                ));
            }
        }
    }

    if !saw_table_close {
        return ParseOutcome::ParseFailed("unterminated <table> (no </table>)".to_string());
    }

    // After </table>, only whitespace/comments allowed.
    while let Some(token) = cursor.peek() {
        match token {
            Token::Comment(_) => {
                cursor.advance();
            }
            Token::Text(s) if is_whitespace_only(s) => {
                cursor.advance();
            }
            _ => {
                return ParseOutcome::ParseFailed(
                    "trailing content after </table>".to_string(),
                );
            }
        }
    }

    ParseOutcome::Parsed(HtmlTable {
        style: table_style,
        head_rows,
        body_rows,
    })
}

// =============================================================================
// Internal: token cursor
// =============================================================================

/// Stateful cursor over a token slice.
struct TokenCursor<'a> {
    tokens: &'a [Token],
    idx: usize,
}

impl<'a> TokenCursor<'a> {
    fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, idx: 0 }
    }
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.idx)
    }
    fn advance(&mut self) {
        if self.idx < self.tokens.len() {
            self.idx += 1;
        }
    }
}

// =============================================================================
// Internal: prefix check
// =============================================================================

/// Quick pre-check: does the trimmed input look like it starts with a
/// `<table` open tag (case-insensitive), possibly with leading
/// whitespace text or HTML comments before it? Only checks structure;
/// the actual parse is what determines validity.
///
/// Returns `false` for inputs that clearly aren't tables (e.g. `<div>`,
/// `<!-- pagebreak -->` on its own with no table). The pagebreak path
/// in the emitter dispatches before this function is called, so we
/// don't need to worry about pagebreak-only blocks here — but we DO
/// want to accept tables that happen to have a leading comment.
fn looks_like_table_open(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    loop {
        // Skip leading whitespace.
        while i < len && bytes[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= len {
            return false;
        }
        // Skip a leading comment if present.
        if i + 4 <= len && &raw[i..i + 4] == "<!--" {
            // Find `-->` end; if missing, bail.
            if let Some(end) = raw[i + 4..].find("-->") {
                i = i + 4 + end + 3;
                continue;
            } else {
                return false;
            }
        }
        // Check `<table` case-insensitively.
        if i + 6 > len {
            return false;
        }
        if !raw[i..i + 6].eq_ignore_ascii_case("<table") {
            return false;
        }
        // The character after must be whitespace, `>`, or `/` to
        // disambiguate from `<tablesomething>`.
        if i + 6 == len {
            return false;
        }
        let next = bytes[i + 6];
        return next.is_ascii_whitespace() || next == b'>' || next == b'/';
    }
}

// =============================================================================
// Internal: section parser (<thead>, <tbody>)
// =============================================================================

/// Parse the contents of a `<thead>` or `<tbody>` block. Returns the
/// rows it contains. Cursor is positioned just after the opening tag;
/// returns with cursor positioned just after the closing tag.
fn parse_section(cursor: &mut TokenCursor, section_name: &str) -> Result<Vec<Row>, String> {
    let mut rows = Vec::new();
    loop {
        let Some(token) = cursor.peek().cloned() else {
            return Err(format!("unterminated <{section_name}>"));
        };
        match &token {
            Token::Comment(_) => {
                cursor.advance();
            }
            Token::Text(s) if is_whitespace_only(s) => {
                cursor.advance();
            }
            Token::Text(_) => {
                return Err(format!(
                    "non-whitespace text directly inside <{section_name}>"
                ));
            }
            Token::StartTag {
                name, self_closing, ..
            } => match name.as_str() {
                "tr" => {
                    if *self_closing {
                        cursor.advance();
                        continue;
                    }
                    cursor.advance();
                    rows.push(parse_row(cursor)?);
                }
                other => {
                    return Err(format!(
                        "unrecognized element <{other}> at <{section_name}>-level (expected tr)"
                    ));
                }
            },
            Token::EndTag { name } if name == section_name => {
                cursor.advance();
                return Ok(rows);
            }
            Token::EndTag { name } => {
                return Err(format!(
                    "unexpected </{name}> inside <{section_name}>"
                ));
            }
        }
    }
}

// =============================================================================
// Internal: row parser
// =============================================================================

/// Parse a `<tr>...</tr>` body. Cursor positioned just after the
/// opening `<tr>`; returns with cursor positioned just after `</tr>`.
fn parse_row(cursor: &mut TokenCursor) -> Result<Row, String> {
    let mut cells = Vec::new();
    loop {
        let Some(token) = cursor.peek().cloned() else {
            return Err("unterminated <tr>".to_string());
        };
        match &token {
            Token::Comment(_) => {
                cursor.advance();
            }
            Token::Text(s) if is_whitespace_only(s) => {
                cursor.advance();
            }
            Token::Text(_) => {
                return Err("non-whitespace text directly inside <tr>".to_string());
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } => match name.as_str() {
                "td" | "th" => {
                    let kind = if name == "th" {
                        CellKind::Th
                    } else {
                        CellKind::Td
                    };
                    let attrs = attrs.clone();
                    let sc = *self_closing;
                    cursor.advance();
                    cells.push(parse_cell(cursor, kind, &attrs, sc, name)?);
                }
                other => {
                    return Err(format!(
                        "unrecognized element <{other}> at <tr>-level (expected td/th)"
                    ));
                }
            },
            Token::EndTag { name } if name == "tr" => {
                cursor.advance();
                return Ok(Row { cells });
            }
            Token::EndTag { name } => {
                return Err(format!("unexpected </{name}> inside <tr>"));
            }
        }
    }
}

// =============================================================================
// Internal: cell parser
// =============================================================================

/// Parse the content of a `<td>` or `<th>`. Cursor positioned just
/// after the opening tag; returns with cursor positioned just after
/// the matching close (or at the structural boundary if unclosed).
fn parse_cell(
    cursor: &mut TokenCursor,
    kind: CellKind,
    attrs: &[(String, String)],
    self_closing: bool,
    open_name: &str,
) -> Result<Cell, String> {
    let colspan = attr_lookup(attrs, "colspan")
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1);
    let rowspan = attr_lookup(attrs, "rowspan")
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(1);
    let style = style_from_attrs(attrs);

    if self_closing {
        return Ok(Cell {
            kind,
            colspan,
            rowspan,
            style,
            content: CellContent::Inlines(Vec::new()),
        });
    }

    // Parse cell content using a stack-based inline-formatter parser.
    let inlines = parse_cell_inlines(cursor, open_name)?;

    Ok(Cell {
        kind,
        colspan,
        rowspan,
        style,
        content: CellContent::Inlines(inlines),
    })
}

/// Parse inline content of a cell until we hit the closing `</td>` or
/// `</th>` (matching `open_name`). Uses a stack to track open inline
/// formatters and auto-closes any dangling formatters at cell end
/// (matching the outside-cell behavior of D-875e4b §2h).
///
/// Mismatched close tags pop intermediate frames (browser-like
/// behavior). Unmatched close tags are silently ignored. Unrecognized
/// tags pass through transparently — their children fold into the
/// current frame's content.
fn parse_cell_inlines(
    cursor: &mut TokenCursor,
    open_name: &str,
) -> Result<Vec<Inline>, String> {
    /// Stack frame for a currently-open inline formatter.
    #[derive(Debug)]
    enum Frame {
        Cell,
        Bold,
        Italic,
    }
    impl Frame {
        fn close_name_match(&self, name: &str) -> bool {
            match self {
                Frame::Cell => false,
                Frame::Bold => name == "b" || name == "strong",
                Frame::Italic => name == "i" || name == "em",
            }
        }
    }

    let mut frames: Vec<Frame> = vec![Frame::Cell];
    let mut bodies: Vec<Vec<Inline>> = vec![Vec::new()];

    fn pop_and_wrap(frames: &mut Vec<Frame>, bodies: &mut Vec<Vec<Inline>>) {
        let body = bodies.pop().expect("frames/bodies length invariant");
        let frame = frames.pop().expect("frames/bodies length invariant");
        let wrapped = match frame {
            Frame::Cell => return, // Cell is the outer frame; never wrap.
            Frame::Bold => Inline::Bold(body),
            Frame::Italic => Inline::Italic(body),
        };
        bodies
            .last_mut()
            .expect("at least the cell frame remains")
            .push(wrapped);
    }

    fn append_text(bodies: &mut [Vec<Inline>], s: String) {
        if s.is_empty() {
            return;
        }
        let body = bodies.last_mut().expect("bodies non-empty");
        // Merge into trailing Text node if possible.
        if let Some(Inline::Text(prev)) = body.last_mut() {
            prev.push_str(&s);
        } else {
            body.push(Inline::Text(s));
        }
    }

    let mut saw_close = false;

    while let Some(token) = cursor.peek().cloned() {
        match &token {
            Token::Comment(_) => {
                cursor.advance();
            }
            Token::Text(s) => {
                append_text(&mut bodies, s.clone());
                cursor.advance();
            }
            Token::StartTag {
                name,
                attrs,
                self_closing,
            } => match name.as_str() {
                "b" | "strong" if !*self_closing => {
                    cursor.advance();
                    frames.push(Frame::Bold);
                    bodies.push(Vec::new());
                }
                "i" | "em" if !*self_closing => {
                    cursor.advance();
                    frames.push(Frame::Italic);
                    bodies.push(Vec::new());
                }
                "br" => {
                    cursor.advance();
                    bodies.last_mut().unwrap().push(Inline::LineBreak);
                }
                "img" => {
                    let inline = build_img(attrs);
                    bodies.last_mut().unwrap().push(inline);
                    cursor.advance();
                }
                "table" => {
                    // Nested table — render its source content as
                    // text per D-875e4b §6e. Consume tokens through
                    // the matching </table> and append a flattened
                    // text representation.
                    let txt = collect_nested_table_as_text(cursor);
                    append_text(&mut bodies, txt);
                }
                _ => {
                    // Unrecognized tag — transparent passthrough.
                    // Children fold into the current parent frame's
                    // content. We just skip the start tag itself.
                    cursor.advance();
                    // (No frame push — unrecognized tags don't create
                    // their own frame. Their close tag, if any, is
                    // handled by the EndTag arm below as an unmatched
                    // close → silently ignored.)
                }
            },
            Token::EndTag { name } if name == open_name => {
                cursor.advance();
                saw_close = true;
                break;
            }
            Token::EndTag { name } => {
                // Look for a matching frame on the stack. If found,
                // auto-close any frames above it.
                let matching_idx = frames
                    .iter()
                    .rposition(|f| f.close_name_match(name));
                match matching_idx {
                    Some(idx) => {
                        // Close frames idx+1..len() in order, then idx.
                        while frames.len() > idx {
                            pop_and_wrap(&mut frames, &mut bodies);
                            if matches!(frames.last(), Some(Frame::Cell))
                                && frames.len() == 1
                            {
                                // Cell frame is at index 0; we can
                                // stop popping when we've popped down
                                // to it.
                                break;
                            }
                        }
                        cursor.advance();
                    }
                    None => {
                        // Unmatched close — silently ignore.
                        cursor.advance();
                    }
                }
            }
        }
    }

    if !saw_close {
        return Err(format!("unterminated <{open_name}>"));
    }

    // Drain any dangling open formatters at cell end.
    while frames.len() > 1 {
        pop_and_wrap(&mut frames, &mut bodies);
    }

    debug_assert_eq!(frames.len(), 1);
    debug_assert_eq!(bodies.len(), 1);
    Ok(bodies.pop().unwrap())
}

// =============================================================================
// Internal: nested-table fallback (D-875e4b §6e)
// =============================================================================

/// Consume tokens starting from a nested `<table>` start tag, through
/// the matching `</table>`, and return a flat text representation
/// suitable for embedding in the parent cell as `Inline::Text`.
///
/// Per D-875e4b §6e: nested-table source rendered as escaped Typst
/// text. We achieve the spirit by serializing the consumed tokens
/// back to a readable HTML-like string. Exact-byte source preservation
/// is not required.
///
/// On entry, the cursor points at the nested `<table>` StartTag. On
/// return, the cursor is positioned past the matching `</table>`.
fn collect_nested_table_as_text(cursor: &mut TokenCursor) -> String {
    let mut depth: i32 = 0;
    let mut out = String::new();
    while let Some(token) = cursor.peek().cloned() {
        match &token {
            Token::StartTag {
                name,
                self_closing,
                ..
            } if name == "table" => {
                if !*self_closing {
                    depth += 1;
                }
                out.push_str(&serialize_token(&token));
                cursor.advance();
            }
            Token::EndTag { name } if name == "table" => {
                depth -= 1;
                out.push_str(&serialize_token(&token));
                cursor.advance();
                if depth <= 0 {
                    break;
                }
            }
            _ => {
                out.push_str(&serialize_token(&token));
                cursor.advance();
            }
        }
    }
    out
}

/// Serialize a single token back to an HTML-like string. Used only
/// for nested-table source fallback per D-875e4b §6e.
fn serialize_token(t: &Token) -> String {
    match t {
        Token::StartTag {
            name,
            attrs,
            self_closing,
        } => {
            let mut s = format!("<{name}");
            for (k, v) in attrs {
                if v.is_empty() {
                    s.push_str(&format!(" {k}"));
                } else {
                    s.push_str(&format!(" {k}=\"{v}\""));
                }
            }
            s.push_str(if *self_closing { "/>" } else { ">" });
            s
        }
        Token::EndTag { name } => format!("</{name}>"),
        Token::Text(s) => s.clone(),
        Token::Comment(s) => format!("<!--{s}-->"),
    }
}

// =============================================================================
// Internal: image-tag → IR
// =============================================================================

fn build_img(attrs: &[(String, String)]) -> Inline {
    let src = attr_lookup(attrs, "src").unwrap_or_default();
    let alt = attr_lookup(attrs, "alt").unwrap_or_default();
    let style = attr_lookup(attrs, "style").map(|s| parse_img_style_attribute(&s));
    Inline::Image { src, alt, style }
}

// =============================================================================
// Internal: helpers
// =============================================================================

fn skip_inert(cursor: &mut TokenCursor) {
    while let Some(token) = cursor.peek() {
        match token {
            Token::Comment(_) => cursor.advance(),
            Token::Text(s) if is_whitespace_only(s) => cursor.advance(),
            _ => break,
        }
    }
}

fn is_whitespace_only(s: &str) -> bool {
    s.bytes().all(|b| b.is_ascii_whitespace())
}

fn attr_lookup(attrs: &[(String, String)], name: &str) -> Option<String> {
    attrs.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone())
}

fn style_from_attrs(attrs: &[(String, String)]) -> CellStyle {
    attr_lookup(attrs, "style")
        .map(|s| parse_style_attribute(&s))
        .unwrap_or_default()
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::css::CssLength;

    fn parsed(outcome: ParseOutcome) -> HtmlTable {
        match outcome {
            ParseOutcome::Parsed(t) => t,
            other => panic!("expected Parsed, got {other:?}"),
        }
    }

    fn parse_failed(outcome: ParseOutcome) -> String {
        match outcome {
            ParseOutcome::ParseFailed(s) => s,
            other => panic!("expected ParseFailed, got {other:?}"),
        }
    }

    // --- NotATable ---------------------------------------------------------

    #[test]
    fn not_a_table_for_arbitrary_html() {
        assert_eq!(
            try_parse_html_table("<div>foo</div>"),
            ParseOutcome::NotATable
        );
        assert_eq!(
            try_parse_html_table("<p>not a table</p>"),
            ParseOutcome::NotATable
        );
    }

    #[test]
    fn not_a_table_for_pagebreak_comment() {
        // (The emitter dispatches pagebreak before this function;
        // even so, this should report NotATable.)
        assert_eq!(
            try_parse_html_table("<!-- pagebreak -->"),
            ParseOutcome::NotATable
        );
    }

    #[test]
    fn not_a_table_for_empty() {
        assert_eq!(try_parse_html_table(""), ParseOutcome::NotATable);
        assert_eq!(try_parse_html_table("   \n"), ParseOutcome::NotATable);
    }

    #[test]
    fn not_a_table_for_table_inside_div() {
        // `<div><table>...` doesn't start with `<table>`.
        assert_eq!(
            try_parse_html_table("<div><table><tr><td>x</td></tr></table></div>"),
            ParseOutcome::NotATable
        );
    }

    // --- Simple table ------------------------------------------------------

    #[test]
    fn parses_minimal_table() {
        let t = parsed(try_parse_html_table(
            "<table><tr><td>cell</td></tr></table>",
        ));
        assert_eq!(t.body_rows.len(), 1);
        assert_eq!(t.body_rows[0].cells.len(), 1);
        assert_eq!(t.body_rows[0].cells[0].kind, CellKind::Td);
        assert_eq!(t.body_rows[0].cells[0].colspan, 1);
        assert_eq!(t.body_rows[0].cells[0].rowspan, 1);
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        assert_eq!(*inls, vec![Inline::Text("cell".to_string())]);
    }

    #[test]
    fn parses_uppercase_table_tag() {
        let t = parsed(try_parse_html_table(
            "<TABLE><TR><TD>cell</TD></TR></TABLE>",
        ));
        assert_eq!(t.body_rows.len(), 1);
        assert_eq!(t.body_rows[0].cells.len(), 1);
    }

    #[test]
    fn parses_thead_and_tbody() {
        let src = "<table>\
            <thead><tr><th>H1</th><th>H2</th></tr></thead>\
            <tbody><tr><td>D1</td><td>D2</td></tr></tbody>\
            </table>";
        let t = parsed(try_parse_html_table(src));
        assert_eq!(t.head_rows.len(), 1);
        assert_eq!(t.head_rows[0].cells.len(), 2);
        assert_eq!(t.head_rows[0].cells[0].kind, CellKind::Th);
        assert_eq!(t.body_rows.len(), 1);
        assert_eq!(t.body_rows[0].cells.len(), 2);
        assert_eq!(t.body_rows[0].cells[0].kind, CellKind::Td);
    }

    #[test]
    fn parses_direct_tr_inside_table() {
        // <table><tr>...</tr></table> with no <tbody> wrapper.
        let t = parsed(try_parse_html_table(
            "<table><tr><td>a</td><td>b</td></tr></table>",
        ));
        assert_eq!(t.body_rows.len(), 1);
        assert_eq!(t.body_rows[0].cells.len(), 2);
    }

    #[test]
    fn skips_whitespace_between_rows() {
        let src = "<table>\n  <tr>\n    <td>x</td>\n  </tr>\n</table>";
        let t = parsed(try_parse_html_table(src));
        assert_eq!(t.body_rows.len(), 1);
        assert_eq!(t.body_rows[0].cells.len(), 1);
    }

    #[test]
    fn skips_comments_inside_table() {
        let src = "<table>\
            <!-- a comment -->\
            <tr><!-- inside row --><td>x</td></tr>\
            </table>";
        let t = parsed(try_parse_html_table(src));
        assert_eq!(t.body_rows.len(), 1);
        assert_eq!(t.body_rows[0].cells.len(), 1);
    }

    // --- colspan / rowspan -------------------------------------------------

    #[test]
    fn parses_colspan() {
        let t = parsed(try_parse_html_table(
            r#"<table><tr><td colspan="3">spans 3</td></tr></table>"#,
        ));
        assert_eq!(t.body_rows[0].cells[0].colspan, 3);
        assert_eq!(t.body_rows[0].cells[0].rowspan, 1);
    }

    #[test]
    fn parses_rowspan() {
        let t = parsed(try_parse_html_table(
            r#"<table><tr><td rowspan="2">spans 2</td></tr></table>"#,
        ));
        assert_eq!(t.body_rows[0].cells[0].rowspan, 2);
        assert_eq!(t.body_rows[0].cells[0].colspan, 1);
    }

    #[test]
    fn malformed_colspan_clamps_to_one() {
        // Negative or zero / non-numeric colspan → defaults to 1.
        let t = parsed(try_parse_html_table(
            r#"<table><tr><td colspan="0">x</td></tr></table>"#,
        ));
        assert_eq!(t.body_rows[0].cells[0].colspan, 1);

        let t = parsed(try_parse_html_table(
            r#"<table><tr><td colspan="abc">x</td></tr></table>"#,
        ));
        assert_eq!(t.body_rows[0].cells[0].colspan, 1);
    }

    // --- Style attribute ----------------------------------------------------

    #[test]
    fn parses_table_style() {
        let t = parsed(try_parse_html_table(
            r#"<table style="width: 100%; border-collapse: collapse"><tr><td>x</td></tr></table>"#,
        ));
        assert_eq!(t.style.width, Some(CssLength::Percent(100.0)));
        assert!(t.style.border_collapse.is_some());
    }

    #[test]
    fn parses_cell_style() {
        let t = parsed(try_parse_html_table(
            r#"<table><tr><td style="width: 50%; padding: 4pt; text-align: center">x</td></tr></table>"#,
        ));
        let cell = &t.body_rows[0].cells[0];
        assert_eq!(cell.style.width, Some(CssLength::Percent(50.0)));
        assert!(cell.style.padding.is_some());
        assert!(cell.style.text_align.is_some());
    }

    // --- Inline content -----------------------------------------------------

    #[test]
    fn parses_bold_in_cell() {
        let t = parsed(try_parse_html_table(
            "<table><tr><td>before <b>bold</b> after</td></tr></table>",
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        assert_eq!(inls.len(), 3);
        assert_eq!(inls[0], Inline::Text("before ".to_string()));
        assert_eq!(inls[1], Inline::Bold(vec![Inline::Text("bold".to_string())]));
        assert_eq!(inls[2], Inline::Text(" after".to_string()));
    }

    #[test]
    fn parses_strong_as_bold() {
        let t = parsed(try_parse_html_table(
            "<table><tr><td><strong>x</strong></td></tr></table>",
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        assert_eq!(inls[0], Inline::Bold(vec![Inline::Text("x".to_string())]));
    }

    #[test]
    fn parses_em_as_italic() {
        let t = parsed(try_parse_html_table(
            "<table><tr><td><em>x</em></td></tr></table>",
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        assert_eq!(inls[0], Inline::Italic(vec![Inline::Text("x".to_string())]));
    }

    #[test]
    fn parses_nested_inlines() {
        // <b><i>foo</i></b> → Bold([Italic([Text("foo")])])
        let t = parsed(try_parse_html_table(
            "<table><tr><td><b><i>foo</i></b></td></tr></table>",
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        assert_eq!(
            inls[0],
            Inline::Bold(vec![Inline::Italic(vec![Inline::Text("foo".to_string())])])
        );
    }

    #[test]
    fn parses_br_as_linebreak() {
        let t = parsed(try_parse_html_table(
            "<table><tr><td>a<br/>b<br>c</td></tr></table>",
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        assert_eq!(inls.len(), 5);
        assert_eq!(inls[0], Inline::Text("a".to_string()));
        assert_eq!(inls[1], Inline::LineBreak);
        assert_eq!(inls[2], Inline::Text("b".to_string()));
        assert_eq!(inls[3], Inline::LineBreak);
        assert_eq!(inls[4], Inline::Text("c".to_string()));
    }

    #[test]
    fn parses_img_in_cell() {
        let t = parsed(try_parse_html_table(
            r#"<table><tr><td><img src="foo.png" alt="hello" style="width:50%"/></td></tr></table>"#,
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        match &inls[0] {
            Inline::Image { src, alt, style } => {
                assert_eq!(src, "foo.png");
                assert_eq!(alt, "hello");
                let style = style.as_ref().expect("style present");
                assert_eq!(style.width, Some(CssLength::Percent(50.0)));
            }
            other => panic!("expected Image, got {other:?}"),
        }
    }

    #[test]
    fn parses_unclosed_inline_drains_at_cell_end() {
        // <b>foo (no </b>) → Bold(Text("foo")) auto-closed at </td>.
        let t = parsed(try_parse_html_table(
            "<table><tr><td><b>foo</td></tr></table>",
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        assert_eq!(
            inls,
            &vec![Inline::Bold(vec![Inline::Text("foo".to_string())])]
        );
    }

    #[test]
    fn parses_mismatched_close_via_auto_close() {
        // <b><i>foo</b></i> — the </b> auto-closes <i> first.
        // Net: Bold(Italic(Text("foo"))).
        let t = parsed(try_parse_html_table(
            "<table><tr><td><b><i>foo</b></i></td></tr></table>",
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        // </i> after auto-close is unmatched → ignored. Result is
        // Bold([Italic([Text("foo")])]).
        assert_eq!(
            inls,
            &vec![Inline::Bold(vec![Inline::Italic(vec![Inline::Text(
                "foo".to_string()
            )])])]
        );
    }

    #[test]
    fn parses_unmatched_close_is_silently_ignored() {
        // </b> with no prior <b> → silently dropped.
        let t = parsed(try_parse_html_table(
            "<table><tr><td>foo</b>bar</td></tr></table>",
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        assert_eq!(
            inls,
            &vec![Inline::Text("foobar".to_string())]
        );
    }

    #[test]
    fn unrecognized_inline_tag_passes_through() {
        // <u>strange</u> — `<u>` is unrecognized; children fold up.
        let t = parsed(try_parse_html_table(
            "<table><tr><td>foo <u>strange</u> bar</td></tr></table>",
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        // Tags vanish; text content survives.
        assert_eq!(inls, &vec![Inline::Text("foo strange bar".to_string())]);
    }

    // --- Nested table fallback ---------------------------------------------

    #[test]
    fn nested_table_renders_as_text() {
        let t = parsed(try_parse_html_table(
            "<table><tr><td>before<table><tr><td>inner</td></tr></table>after</td></tr></table>",
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        // Should produce a single Text inline (or merged Text inlines)
        // containing both "before", the serialized nested-table source,
        // and "after".
        let merged = inls
            .iter()
            .map(|i| match i {
                Inline::Text(s) => s.clone(),
                _ => panic!("expected only Text, got {i:?}"),
            })
            .collect::<String>();
        assert!(merged.starts_with("before"));
        assert!(merged.contains("<table>"));
        assert!(merged.contains("inner"));
        assert!(merged.contains("</table>"));
        assert!(merged.ends_with("after"));
    }

    // --- Strict structural failures ----------------------------------------

    #[test]
    fn parse_failed_unrecognized_at_table_level() {
        let outcome = try_parse_html_table(
            "<table><caption>title</caption><tr><td>x</td></tr></table>",
        );
        let reason = parse_failed(outcome);
        assert!(reason.contains("caption") || reason.contains("table-level"));
    }

    #[test]
    fn parse_failed_div_at_table_level() {
        let outcome = try_parse_html_table(
            "<table><div>oops</div><tr><td>x</td></tr></table>",
        );
        assert!(matches!(outcome, ParseOutcome::ParseFailed(_)));
    }

    #[test]
    fn parse_failed_unrecognized_at_tr_level() {
        let outcome = try_parse_html_table(
            "<table><tr><div>oops</div></tr></table>",
        );
        assert!(matches!(outcome, ParseOutcome::ParseFailed(_)));
    }

    #[test]
    fn parse_failed_unterminated_table() {
        let outcome = try_parse_html_table("<table><tr><td>x</td></tr>");
        assert!(matches!(outcome, ParseOutcome::ParseFailed(_)));
    }

    #[test]
    fn parse_failed_unterminated_cell() {
        let outcome = try_parse_html_table("<table><tr><td>x</tr></table>");
        // Missing </td>; structural failure.
        assert!(matches!(outcome, ParseOutcome::ParseFailed(_)));
    }

    #[test]
    fn parse_failed_text_at_table_level() {
        let outcome = try_parse_html_table(
            "<table>oops<tr><td>x</td></tr></table>",
        );
        assert!(matches!(outcome, ParseOutcome::ParseFailed(_)));
    }

    #[test]
    fn parse_failed_trailing_content() {
        let outcome = try_parse_html_table(
            "<table><tr><td>x</td></tr></table>extra",
        );
        assert!(matches!(outcome, ParseOutcome::ParseFailed(_)));
    }

    // --- Whitespace + comment leading ---------------------------------------

    #[test]
    fn allows_leading_whitespace_and_comments() {
        let outcome = try_parse_html_table(
            "  <!-- hello -->\n<table><tr><td>x</td></tr></table>",
        );
        assert!(matches!(outcome, ParseOutcome::Parsed(_)));
    }

    // --- README fixture -----------------------------------------------------

    #[test]
    fn parses_readme_fixture_structure() {
        let src = r#"<table style="width: 100%; border-collapse: collapse;">
  <tr>
    <td style="width: 50%; padding: 0 4pt; vertical-align: middle;">
      <img src="assets/images/rolled-paper.png"
           alt="A rolled-up piece of paper, by Round Icons via Unsplash+"
           style="width: 100%; height: auto;" />
    </td>
    <td style="width: 20%; padding: 0 8pt; vertical-align: middle; text-align: center;">
      One scroll on the left, one workstation on the right, and a
      narrow column of prose wedged in between to prove that mixed
      image/text rows survive the trip through the renderer.
    </td>
    <td style="width: 30%; padding: 0 4pt; vertical-align: middle;">
      <img src="https://example.com/foo.png"
           alt="vector illustration"
           style="width: 100%; height: auto;" />
    </td>
  </tr>
</table>"#;
        let t = parsed(try_parse_html_table(src));
        assert_eq!(t.body_rows.len(), 1);
        assert_eq!(t.body_rows[0].cells.len(), 3);

        let cell0 = &t.body_rows[0].cells[0];
        assert_eq!(cell0.style.width, Some(CssLength::Percent(50.0)));
        let CellContent::Inlines(inls) = &cell0.content;
        // Should contain an Image (possibly wrapped by surrounding
        // whitespace text).
        assert!(inls.iter().any(|i| matches!(i, Inline::Image { .. })));

        let cell1 = &t.body_rows[0].cells[1];
        assert_eq!(cell1.style.width, Some(CssLength::Percent(20.0)));

        let cell2 = &t.body_rows[0].cells[2];
        assert_eq!(cell2.style.width, Some(CssLength::Percent(30.0)));
    }

    // --- Robustness ---------------------------------------------------------

    #[test]
    fn does_not_panic_on_pathological_input() {
        let huge = "<table>".repeat(1000);
        let _ = try_parse_html_table(&huge);

        let nested = format!(
            "<table>{}<tr><td>x</td></tr>{}</table>",
            "<table>".repeat(50),
            "</table>".repeat(50)
        );
        // Nested tables are at table-level (structural position
        // inside <table>) — wait, they're outside the cells in this
        // construction, so they're at structural position. ParseFailed.
        let _ = try_parse_html_table(&nested);
    }

    #[test]
    fn does_not_panic_on_unicode_content() {
        let outcome = try_parse_html_table(
            "<table><tr><td>héllo 世界 🎉</td></tr></table>",
        );
        let t = parsed(outcome);
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        assert_eq!(inls[0], Inline::Text("héllo 世界 🎉".to_string()));
    }

    #[test]
    fn does_not_panic_on_deeply_nested_inlines() {
        let depth = 200;
        let mut src = "<table><tr><td>".to_string();
        for _ in 0..depth {
            src.push_str("<b>");
        }
        src.push_str("X");
        for _ in 0..depth {
            src.push_str("</b>");
        }
        src.push_str("</td></tr></table>");
        // Should parse successfully without stack overflow (we're
        // iterative, so no stack growth).
        let _ = try_parse_html_table(&src);
    }

    // --- Image style parsing inside cells ----------------------------------

    #[test]
    fn img_with_no_style_has_no_style() {
        let t = parsed(try_parse_html_table(
            r#"<table><tr><td><img src="foo.png" alt="x"/></td></tr></table>"#,
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        match &inls[0] {
            Inline::Image { src, alt, style } => {
                assert_eq!(src, "foo.png");
                assert_eq!(alt, "x");
                assert!(style.is_none());
            }
            _ => panic!("expected Image"),
        }
    }

    #[test]
    fn img_height_auto_present_but_none() {
        let t = parsed(try_parse_html_table(
            r#"<table><tr><td><img src="foo.png" style="width:100%; height:auto"/></td></tr></table>"#,
        ));
        let CellContent::Inlines(inls) = &t.body_rows[0].cells[0].content;
        match &inls[0] {
            Inline::Image { style, .. } => {
                let style = style.as_ref().expect("style present");
                assert_eq!(style.width, Some(CssLength::Percent(100.0)));
                assert!(style.height.is_none()); // auto = None
            }
            _ => panic!("expected Image"),
        }
    }
}
