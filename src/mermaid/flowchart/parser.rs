//! Recursive-descent parser for the v1 mermaid flowchart subset.
//!
//! The parser is deliberately small and line-oriented: mermaid
//! flowcharts are statements separated by newlines (or `;`), and almost
//! every construct fits on one line. A single line is tokenised and
//! parsed as a "chain" of nodes interleaved with edge operators
//! (`A --> B --> C`), or as a control statement (header `flowchart TD`,
//! `subgraph name`, `end`, comment `%% …`).
//!
//! Subset (W-3686a9):
//!
//! - Header: `flowchart [TD|TB|BT|LR|RL]` or `graph [...]`.
//! - Comments: lines starting with `%%`.
//! - Subgraph: `subgraph name [optional title]` / `end` (nestable up
//!   to [`MAX_SUBGRAPH_DEPTH`]).
//! - Node refs and decls in a chain: `id`, `id[Label]`, `id(Label)`,
//!   `id([Label])`, `id[[Label]]`, `id[(Label)]`, `id((Label))`,
//!   `id{Label}`. Labels may be quoted (`"…"`); inside quotes, escape
//!   the closing `"` with `\"`.
//! - Edges in a chain: `-->`, `---`, `-.->`, `==>`, with optional pipe
//!   labels `-->|label|`, and the inline-label form `-- text -->`.
//!
//! Out-of-subset constructs (click handlers, classDef styling, complex
//! curve syntaxes, Markdown-in-node-text) error with a typed
//! [`MermaidError::ParseError`] naming the unsupported feature.
//!
//! ## Hostile-input safety
//!
//! Every loop in this module is bounded by a static cap from
//! [`super::super`] (the `mermaid::` module-level constants). The
//! parser is panic-free for any UTF-8 input bounded by
//! [`MAX_INPUT_BYTES`]; bytes beyond that are rejected upstream in
//! [`crate::mermaid::render`]. Per-line tokenisation is bounded by
//! [`MAX_TOKENS_PER_LINE`]. Subgraph depth is hard-capped.

use super::super::{
    MermaidError, MAX_EDGES, MAX_LABEL_LEN, MAX_LINES, MAX_NODES, MAX_SUBGRAPH_DEPTH,
    MAX_TOKENS_PER_LINE,
};
use super::ir::{Direction, Edge, EdgeKind, Flowchart, Node, NodeIdx, NodeShape, Subgraph};

// -- public entry ---------------------------------------------------------

/// Parse the full mermaid source into a [`Flowchart`] IR.
///
/// Caller has already validated `src.len() <= MAX_INPUT_BYTES` and run
/// the diagram-type sniffer; this function trusts the leading token is
/// `flowchart` or `graph`.
pub fn parse(src: &str) -> Result<Flowchart, MermaidError> {
    let mut p = Parser::new(src)?;
    p.parse_all()?;
    Ok(p.fc)
}

// -- internals ------------------------------------------------------------

struct Parser<'a> {
    lines: Vec<(usize, &'a str)>, // (1-indexed line number, line text)
    cursor: usize,
    fc: Flowchart,
    /// Stack of in-progress subgraph indices into `fc.subgraphs`.
    subgraph_stack: Vec<usize>,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Result<Self, MermaidError> {
        let mut lines = Vec::new();
        for (i, raw) in src.lines().enumerate() {
            // Lines beyond the cap are rejected.
            if i >= MAX_LINES {
                return Err(MermaidError::InputTooLarge {
                    cap: "MAX_LINES",
                    limit: MAX_LINES,
                });
            }
            // Mermaid allows `;` as statement separator inside a line.
            // We split each top-level line on `;` (respecting quotes
            // and bracket nesting), giving each piece its own logical
            // line number = source line.
            for piece in split_top_level_semicolons(raw) {
                lines.push((i + 1, piece));
            }
        }
        Ok(Parser {
            lines,
            cursor: 0,
            fc: Flowchart::default(),
            subgraph_stack: Vec::new(),
        })
    }

    fn parse_all(&mut self) -> Result<(), MermaidError> {
        // 1. Header.
        let (lineno, header) = self.next_significant().ok_or(MermaidError::ParseError {
            line: 0,
            reason: "empty diagram".into(),
        })?;
        self.parse_header(lineno, header)?;

        // 2. Body.
        while let Some((lineno, line)) = self.next_significant() {
            self.parse_body_line(lineno, line)?;
        }

        // 3. Unclosed subgraphs are an error — naming the unclosed name
        //    so the author can find it.
        if let Some(&top) = self.subgraph_stack.last() {
            let name = self.fc.subgraphs[top].id.clone();
            return Err(MermaidError::ParseError {
                line: 0,
                reason: format!("unclosed subgraph '{}'", name),
            });
        }
        Ok(())
    }

    /// Consume the next non-blank, non-comment line.
    fn next_significant(&mut self) -> Option<(usize, &'a str)> {
        while self.cursor < self.lines.len() {
            let (n, l) = self.lines[self.cursor];
            self.cursor += 1;
            let t = l.trim();
            if t.is_empty() || t.starts_with("%%") {
                continue;
            }
            return Some((n, l));
        }
        None
    }

    fn parse_header(&mut self, lineno: usize, line: &str) -> Result<(), MermaidError> {
        let trimmed = line.trim();
        let mut parts = trimmed.split_whitespace();
        let kw = parts.next().unwrap_or("");
        if kw != "flowchart" && kw != "graph" {
            return Err(MermaidError::ParseError {
                line: lineno,
                reason: format!("expected 'flowchart' or 'graph', got '{}'", kw),
            });
        }
        if let Some(dir_tok) = parts.next() {
            // Mermaid permits `flowchart-direction:LR` curve syntax;
            // we only support the simple `flowchart LR` form for v1.
            self.fc.direction = parse_direction(dir_tok).ok_or_else(|| {
                MermaidError::ParseError {
                    line: lineno,
                    reason: format!("unrecognised direction '{}' (expected TD/TB/BT/LR/RL)", dir_tok),
                }
            })?;
            // Trailing tokens after a valid direction are silently
            // accepted (mermaid does the same).
        }
        Ok(())
    }

    fn parse_body_line(&mut self, lineno: usize, line: &str) -> Result<(), MermaidError> {
        let trimmed = line.trim();

        // Subgraph open.
        if let Some(rest) = strip_keyword(trimmed, "subgraph") {
            return self.open_subgraph(lineno, rest);
        }
        // Subgraph close.
        if trimmed == "end" {
            return self.close_subgraph(lineno);
        }
        // Out-of-subset feature flags. Cheap rejection on the common
        // mermaid extensions we explicitly defer.
        for forbidden in &["click ", "classDef ", "class ", "linkStyle ", "style "] {
            if trimmed.starts_with(forbidden) {
                let f = forbidden.trim_end();
                return Err(MermaidError::ParseError {
                    line: lineno,
                    reason: format!("'{}' is not supported in v1", f),
                });
            }
        }

        // Otherwise: a chain of nodes and edges.
        self.parse_chain(lineno, trimmed)
    }

    // -- subgraph control --------------------------------------------------

    fn open_subgraph(&mut self, lineno: usize, rest: &str) -> Result<(), MermaidError> {
        if self.subgraph_stack.len() >= MAX_SUBGRAPH_DEPTH {
            return Err(MermaidError::InputTooLarge {
                cap: "MAX_SUBGRAPH_DEPTH",
                limit: MAX_SUBGRAPH_DEPTH,
            });
        }
        // `subgraph name` or `subgraph name [Title]` or
        // `subgraph "Quoted title"`.
        let (id, title) = parse_subgraph_header(rest).ok_or_else(|| MermaidError::ParseError {
            line: lineno,
            reason: "subgraph requires a name".into(),
        })?;
        let depth = self.subgraph_stack.len();
        let new_idx = self.fc.subgraphs.len();
        self.fc.subgraphs.push(Subgraph {
            id,
            title,
            members: Vec::new(),
            children: Vec::new(),
            depth,
            layout: None,
        });
        if let Some(&parent) = self.subgraph_stack.last() {
            self.fc.subgraphs[parent].children.push(new_idx);
        } else {
            self.fc.top_level_subgraphs.push(new_idx);
        }
        self.subgraph_stack.push(new_idx);
        Ok(())
    }

    fn close_subgraph(&mut self, lineno: usize) -> Result<(), MermaidError> {
        if self.subgraph_stack.pop().is_none() {
            return Err(MermaidError::ParseError {
                line: lineno,
                reason: "unexpected 'end' (no open subgraph)".into(),
            });
        }
        Ok(())
    }

    // -- chains ------------------------------------------------------------

    fn parse_chain(&mut self, lineno: usize, line: &str) -> Result<(), MermaidError> {
        let tokens = tokenize_line(line, lineno)?;
        if tokens.is_empty() {
            return Ok(());
        }

        // A chain is: NodeRef (Edge NodeRef)*. The simplest line is a
        // single node declaration with no edges.
        let mut i = 0;
        let mut prev_node: Option<NodeIdx> = None;
        let mut pending_edge: Option<PendingEdge> = None;

        while i < tokens.len() {
            match &tokens[i] {
                Tok::Node { id, label, shape } => {
                    let idx = self.upsert_node(id, label.clone(), *shape, lineno)?;
                    if let Some(pe) = pending_edge.take() {
                        let from = prev_node.ok_or(MermaidError::ParseError {
                            line: lineno,
                            reason: "edge has no left-hand node".into(),
                        })?;
                        self.add_edge(from, idx, pe.kind, pe.label, lineno)?;
                    }
                    prev_node = Some(idx);
                    i += 1;
                }
                Tok::Edge { kind, label } => {
                    if prev_node.is_none() {
                        return Err(MermaidError::ParseError {
                            line: lineno,
                            reason: "edge appears before any node".into(),
                        });
                    }
                    if pending_edge.is_some() {
                        return Err(MermaidError::ParseError {
                            line: lineno,
                            reason: "two edges in a row with no node between".into(),
                        });
                    }
                    pending_edge = Some(PendingEdge {
                        kind: *kind,
                        label: label.clone(),
                    });
                    i += 1;
                }
            }
        }

        if pending_edge.is_some() {
            return Err(MermaidError::ParseError {
                line: lineno,
                reason: "trailing edge with no right-hand node".into(),
            });
        }
        Ok(())
    }

    fn upsert_node(
        &mut self,
        id: &str,
        label: Option<String>,
        shape: Option<NodeShape>,
        lineno: usize,
    ) -> Result<NodeIdx, MermaidError> {
        if let Some(existing) = self.fc.find_node(id) {
            // Re-declaration: if the new occurrence carries a shape /
            // label, it overrides the previous (mermaid's behaviour).
            if let Some(lbl) = label {
                self.fc.nodes[existing.0].label = lbl;
            }
            if let Some(sh) = shape {
                self.fc.nodes[existing.0].shape = sh;
            }
            self.attach_to_current_subgraph(existing);
            return Ok(existing);
        }
        if self.fc.nodes.len() >= MAX_NODES {
            return Err(MermaidError::InputTooLarge {
                cap: "MAX_NODES",
                limit: MAX_NODES,
            });
        }
        let _ = lineno; // present for future error context
        let idx = NodeIdx(self.fc.nodes.len());
        self.fc.nodes.push(Node {
            id: id.to_string(),
            label: label.unwrap_or_else(|| id.to_string()),
            shape: shape.unwrap_or(NodeShape::Rectangle),
            layout: None,
        });
        self.attach_to_current_subgraph(idx);
        Ok(idx)
    }

    fn attach_to_current_subgraph(&mut self, idx: NodeIdx) {
        if let Some(&top) = self.subgraph_stack.last() {
            // De-dup membership.
            let members = &mut self.fc.subgraphs[top].members;
            if !members.iter().any(|m| *m == idx) {
                members.push(idx);
            }
        }
    }

    fn add_edge(
        &mut self,
        from: NodeIdx,
        to: NodeIdx,
        kind: EdgeKind,
        label: Option<String>,
        _lineno: usize,
    ) -> Result<(), MermaidError> {
        if self.fc.edges.len() >= MAX_EDGES {
            return Err(MermaidError::InputTooLarge {
                cap: "MAX_EDGES",
                limit: MAX_EDGES,
            });
        }
        self.fc.edges.push(Edge {
            from,
            to,
            kind,
            label,
        });
        Ok(())
    }
}

/// In-progress edge waiting for its right-hand node.
struct PendingEdge {
    kind: EdgeKind,
    label: Option<String>,
}

// -- token types ----------------------------------------------------------

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Node {
        id: String,
        label: Option<String>,
        shape: Option<NodeShape>,
    },
    Edge {
        kind: EdgeKind,
        label: Option<String>,
    },
}

// -- helpers --------------------------------------------------------------

/// Strip a leading keyword followed by whitespace; return the rest of
/// the line if matched, else `None`. Case-sensitive.
fn strip_keyword<'a>(s: &'a str, kw: &str) -> Option<&'a str> {
    if s.len() < kw.len() {
        return None;
    }
    if !s.starts_with(kw) {
        return None;
    }
    let rest = &s[kw.len()..];
    if rest.is_empty() {
        return Some("");
    }
    let first = rest.chars().next().unwrap();
    if !first.is_whitespace() {
        return None;
    }
    Some(rest.trim_start())
}

fn parse_direction(s: &str) -> Option<Direction> {
    match s {
        "TD" | "TB" => Some(Direction::TopDown),
        "BT" => Some(Direction::BottomTop),
        "LR" => Some(Direction::LeftRight),
        "RL" => Some(Direction::RightLeft),
        _ => None,
    }
}

/// Parse a subgraph header tail (everything after the `subgraph`
/// keyword). Accepts:
///   - `id`
///   - `id [Title in brackets]`
///   - `"Quoted name as both id and title"`
fn parse_subgraph_header(rest: &str) -> Option<(String, Option<String>)> {
    let r = rest.trim();
    if r.is_empty() {
        return None;
    }
    if r.starts_with('"') {
        // Quoted: "Title". Use the title text as the id.
        let inner = strip_quoted(r)?;
        return Some((inner.clone(), Some(inner)));
    }
    // Unquoted: first whitespace-delimited token is the id; remainder is
    // optional title (with optional surrounding `[ ... ]`).
    let mut iter = r.splitn(2, char::is_whitespace);
    let id = iter.next()?.to_string();
    let title = iter.next().map(|t| {
        let t = t.trim();
        if t.starts_with('[') && t.ends_with(']') && t.len() >= 2 {
            t[1..t.len() - 1].trim().to_string()
        } else {
            t.to_string()
        }
    });
    Some((id, title))
}

/// Strip surrounding double-quotes; honor `\"` as an escaped quote.
/// Returns the inner text, or None if the string is malformed (no
/// closing quote).
fn strip_quoted(s: &str) -> Option<String> {
    if !s.starts_with('"') {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 1;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'\\' && i + 1 < bytes.len() && bytes[i + 1] == b'"' {
            out.push('"');
            i += 2;
            continue;
        }
        if b == b'"' {
            // Closing quote.
            return Some(out);
        }
        // Push as a UTF-8 codepoint properly.
        // Find the codepoint length.
        let cp_len = utf8_char_len(bytes[i]);
        if i + cp_len > bytes.len() {
            return None;
        }
        match std::str::from_utf8(&bytes[i..i + cp_len]) {
            Ok(s) => out.push_str(s),
            Err(_) => return None,
        }
        i += cp_len;
    }
    None
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0x80 {
        1
    } else if b < 0xC0 {
        1 // invalid continuation, treat as single byte
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

/// Split on `;` only when not inside quotes or any bracket pair. Used
/// to permit `A --> B; C --> D` on a single source line.
fn split_top_level_semicolons(line: &str) -> Vec<&str> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut start = 0;
    let mut depth_round = 0i32;
    let mut depth_square = 0i32;
    let mut depth_curly = 0i32;
    let mut in_quote = false;
    let mut prev = 0u8;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if in_quote {
            if b == b'"' && prev != b'\\' {
                in_quote = false;
            }
        } else {
            match b {
                b'"' => in_quote = true,
                b'(' => depth_round += 1,
                b')' => depth_round -= 1,
                b'[' => depth_square += 1,
                b']' => depth_square -= 1,
                b'{' => depth_curly += 1,
                b'}' => depth_curly -= 1,
                b';' if depth_round == 0 && depth_square == 0 && depth_curly == 0 => {
                    out.push(&line[start..i]);
                    start = i + 1;
                }
                _ => {}
            }
        }
        prev = b;
        i += 1;
    }
    out.push(&line[start..]);
    out
}

// -- per-line tokenizer ---------------------------------------------------

/// Tokenize one logical (semicolon-split) line into Node/Edge tokens.
///
/// The tokenizer scans left-to-right. At each position it tries, in
/// order: edge operator (longest-match), then node ref/decl. Whitespace
/// is skipped.
fn tokenize_line(line: &str, lineno: usize) -> Result<Vec<Tok>, MermaidError> {
    let bytes = line.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let n = bytes.len();
    let mut count = 0usize;
    while i < n {
        // Defensive cap.
        if count >= MAX_TOKENS_PER_LINE {
            return Err(MermaidError::InputTooLarge {
                cap: "MAX_TOKENS_PER_LINE",
                limit: MAX_TOKENS_PER_LINE,
            });
        }
        // Skip ASCII whitespace.
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }

        // Edge operator?
        if let Some((tok, consumed)) = try_edge(bytes, i, lineno)? {
            out.push(tok);
            i += consumed;
            count += 1;
            continue;
        }

        // Node ref / decl.
        let (tok, consumed) = parse_node_token(bytes, i, lineno)?;
        out.push(tok);
        i += consumed;
        count += 1;
    }
    Ok(out)
}

/// Try to match an edge operator at position `i`. Returns the parsed
/// edge token and the number of bytes consumed, or `Ok(None)` if no
/// edge starts here.
///
/// Recognised edges (longest-match wins):
///   `==>`, `==`+
///   `-->`, `---`, `----`+ (extra dashes are truncated to `---`/`-->`)
///   `-.->`, `-.-`
///   `-- text -->`, `-- text ---`
///   `-->|label|`, `--->|label|`, `==>|label|`, `-.->|label|`
fn try_edge(bytes: &[u8], i: usize, lineno: usize) -> Result<Option<(Tok, usize)>, MermaidError> {
    // The leading char of any supported edge is `-` or `=`.
    if bytes.get(i) != Some(&b'-') && bytes.get(i) != Some(&b'=') {
        return Ok(None);
    }
    let lead = bytes[i];
    // Scan ahead the edge's "shaft" plus optional inline label and
    // optional pipe label.
    //
    // Implementation: parse a "shaft segment" of `--…->` / `--…--` /
    // `==…=>` / `-.…->`. We only accept simple forms:
    //   - run of >=2 leading `-` or `=`
    //   - optional `.` (one) for dashed
    //   - run of `-` or `=` matching the leading char
    //   - terminator `>` (arrow) or matching char (line)
    //   - or "inline label" form: `-- text -->` (text between two dash
    //     runs that ends with `>` or just `--`).
    //   - or pipe label form: append `|label|` after the shaft.

    // Try inline-label form first because it's the most specific:
    // `-- TEXT -->` or `-- TEXT --`.
    if lead == b'-' {
        if let Some((tok, consumed)) = try_inline_label_edge(bytes, i, lineno)? {
            return Ok(Some((tok, consumed)));
        }
    }

    // Otherwise: scan a contiguous shaft.
    let (kind, mut consumed) = match parse_shaft(bytes, i) {
        Some(x) => x,
        None => return Ok(None),
    };
    // Optional pipe label.
    let mut label: Option<String> = None;
    if bytes.get(i + consumed) == Some(&b'|') {
        let (lbl, lbl_len) = parse_pipe_label(bytes, i + consumed, lineno)?;
        label = Some(lbl);
        consumed += lbl_len;
    }
    Ok(Some((Tok::Edge { kind, label }, consumed)))
}

/// Parse a contiguous edge "shaft" — the dash/equal/dot run that
/// terminates in `>` (arrow) or repeats (line). Returns the [`EdgeKind`]
/// and number of bytes consumed.
fn parse_shaft(bytes: &[u8], i: usize) -> Option<(EdgeKind, usize)> {
    let n = bytes.len();
    if i >= n {
        return None;
    }
    let first = bytes[i];
    if first != b'-' && first != b'=' {
        return None;
    }
    let mut p = i;
    // Dotted form: starts with `-.`
    if first == b'-' && bytes.get(p + 1) == Some(&b'.') {
        // -.->  or  -.-   (we only require: '-', '.', '-', then '>' or end-of-edge)
        p += 2; // past '-.'
        // Optional more dots? Mermaid uses `-.->` and `-.-`.
        // Then a single `-`.
        if bytes.get(p) != Some(&b'-') {
            return None;
        }
        p += 1; // consumed '-.-'
        if bytes.get(p) == Some(&b'>') {
            p += 1;
            return Some((EdgeKind::Dashed, p - i));
        }
        // `-.-` without trailing `>` is treated as a dashed line (no arrow).
        return Some((EdgeKind::Dashed, p - i));
    }
    // Solid / thick: run of `-` (>=2) or `=` (>=2), then `>` or one
    // more of the same to make a line.
    if first == b'-' {
        let mut dashes = 0;
        while bytes.get(p) == Some(&b'-') {
            dashes += 1;
            p += 1;
        }
        if dashes < 2 {
            return None;
        }
        if bytes.get(p) == Some(&b'>') {
            p += 1;
            return Some((EdgeKind::Arrow, p - i));
        }
        // No `>` → line edge `---`.
        return Some((EdgeKind::Line, p - i));
    }
    if first == b'=' {
        let mut eq = 0;
        while bytes.get(p) == Some(&b'=') {
            eq += 1;
            p += 1;
        }
        if eq < 2 {
            return None;
        }
        if bytes.get(p) == Some(&b'>') {
            p += 1;
            return Some((EdgeKind::Thick, p - i));
        }
        return Some((EdgeKind::Thick, p - i));
    }
    None
}

/// Parse the `|label|` suffix on an edge. Returns (label_text,
/// bytes_consumed_including_pipes).
fn parse_pipe_label(
    bytes: &[u8],
    i: usize,
    lineno: usize,
) -> Result<(String, usize), MermaidError> {
    debug_assert_eq!(bytes.get(i), Some(&b'|'));
    let n = bytes.len();
    let mut p = i + 1;
    while p < n {
        if bytes[p] == b'|' {
            // Slice [i+1 .. p) is the label text.
            let raw = std::str::from_utf8(&bytes[i + 1..p]).map_err(|_| {
                MermaidError::ParseError {
                    line: lineno,
                    reason: "edge label is not valid UTF-8".into(),
                }
            })?;
            let label = unquote_if_needed(raw.trim());
            if label.chars().count() > MAX_LABEL_LEN {
                return Err(MermaidError::InputTooLarge {
                    cap: "MAX_LABEL_LEN",
                    limit: MAX_LABEL_LEN,
                });
            }
            return Ok((label, p - i + 1));
        }
        p += 1;
    }
    Err(MermaidError::ParseError {
        line: lineno,
        reason: "unclosed edge label '|'".into(),
    })
}

/// Try to match the inline-label edge form `-- text -->` /
/// `-- text ---`. The specific shape is: the edge starts with `--`,
/// has at least one space, runs label text, then a space and a closing
/// `-->` or `---` (or `-.->` / `==>` for symmetry — but mermaid
/// canonically only recognises this on the dash/arrow forms).
///
/// Returns Some((edge_token, bytes_consumed)) if matched.
fn try_inline_label_edge(
    bytes: &[u8],
    i: usize,
    lineno: usize,
) -> Result<Option<(Tok, usize)>, MermaidError> {
    // Must start with at least `-- ` (two dashes and a space).
    if bytes.get(i) != Some(&b'-') || bytes.get(i + 1) != Some(&b'-') {
        return Ok(None);
    }
    if bytes.get(i + 2) != Some(&b' ') {
        return Ok(None);
    }
    // Find the end of the inline-label form by searching for the next
    // ` -- ` / ` --> ` / ` ---` boundary. We bound the search to a
    // reasonable label length to keep this O(n).
    let n = bytes.len();
    let max_search = n.min(i + 2 + MAX_LABEL_LEN * 4 + 8);
    let mut p = i + 3; // past "-- "
    // We scan for either:
    //   "--> "  arrow terminator
    //   "---"   line terminator (followed by non-`-` or end-of-line)
    //
    // Prefer the longest shaft we can match.
    while p < max_search {
        // Look for a space followed by "--" — that's the boundary
        // between label text and the closing shaft.
        if bytes[p] == b' ' && bytes.get(p + 1) == Some(&b'-') && bytes.get(p + 2) == Some(&b'-') {
            // Closing shaft starts at p+1.
            let shaft_start = p + 1;
            // Match closing shaft.
            let (kind, cons) = parse_shaft(bytes, shaft_start).ok_or_else(|| {
                MermaidError::ParseError {
                    line: lineno,
                    reason: "malformed edge after inline label".into(),
                }
            })?;
            // Label text is [i+3 .. p).
            let raw_label = std::str::from_utf8(&bytes[i + 3..p]).map_err(|_| {
                MermaidError::ParseError {
                    line: lineno,
                    reason: "edge label is not valid UTF-8".into(),
                }
            })?;
            let label_text = unquote_if_needed(raw_label.trim());
            if label_text.chars().count() > MAX_LABEL_LEN {
                return Err(MermaidError::InputTooLarge {
                    cap: "MAX_LABEL_LEN",
                    limit: MAX_LABEL_LEN,
                });
            }
            let total = (shaft_start - i) + cons;
            return Ok(Some((
                Tok::Edge {
                    kind,
                    label: Some(label_text),
                },
                total,
            )));
        }
        p += 1;
    }
    // No close → not an inline-label edge; let the simple-shaft parser
    // try.
    Ok(None)
}

/// Strip a single pair of surrounding `"…"` if present; otherwise
/// return the input unchanged.
fn unquote_if_needed(s: &str) -> String {
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        // Honour `\"` escapes inside.
        if let Some(unq) = strip_quoted(s) {
            return unq;
        }
    }
    s.to_string()
}

// -- node tokens ----------------------------------------------------------

/// Recognised set of brace pairs that introduce a node-shape decl.
/// Kept as a table of (open, close, shape).
const SHAPE_BRACES: &[(&[u8], &[u8], NodeShape)] = &[
    (b"([", b"])", NodeShape::Stadium),
    (b"[[", b"]]", NodeShape::Subroutine),
    (b"[(", b")]", NodeShape::Cylinder),
    (b"((", b"))", NodeShape::Circle),
    (b"[", b"]", NodeShape::Rectangle),
    (b"(", b")", NodeShape::RoundedRectangle),
    (b"{", b"}", NodeShape::Diamond),
];

/// Parse one node token starting at `i`. Returns the token and the
/// number of bytes consumed.
fn parse_node_token(
    bytes: &[u8],
    i: usize,
    lineno: usize,
) -> Result<(Tok, usize), MermaidError> {
    // Identifier first.
    let (id, id_len) = parse_identifier(bytes, i, lineno)?;
    let p = i + id_len;

    // Optional shape decl.
    for (open, close, shape) in SHAPE_BRACES {
        if bytes[p..].starts_with(open) {
            // Find matching `close`. The label may contain quoted
            // strings; we honor them so a label like `"a]b"` doesn't
            // close early.
            let label_start = p + open.len();
            let close_at = find_close(bytes, label_start, close, lineno)?;
            let raw = std::str::from_utf8(&bytes[label_start..close_at]).map_err(|_| {
                MermaidError::ParseError {
                    line: lineno,
                    reason: "node label is not valid UTF-8".into(),
                }
            })?;
            let label = unquote_if_needed(raw.trim());
            if label.chars().count() > MAX_LABEL_LEN {
                return Err(MermaidError::InputTooLarge {
                    cap: "MAX_LABEL_LEN",
                    limit: MAX_LABEL_LEN,
                });
            }
            let consumed = (close_at - i) + close.len();
            return Ok((
                Tok::Node {
                    id: id.to_string(),
                    label: Some(label),
                    shape: Some(*shape),
                },
                consumed,
            ));
        }
    }

    // Bare reference.
    Ok((
        Tok::Node {
            id: id.to_string(),
            label: None,
            shape: None,
        },
        id_len,
    ))
}

/// Identifier characters are `[A-Za-z0-9_]+`. We additionally accept
/// `-` not followed by `>` to match mermaid's permissive ids in
/// limited cases, but reject ids that would conflict with edge tokens.
fn parse_identifier<'a>(
    bytes: &'a [u8],
    i: usize,
    lineno: usize,
) -> Result<(&'a str, usize), MermaidError> {
    let n = bytes.len();
    if i >= n {
        return Err(MermaidError::ParseError {
            line: lineno,
            reason: "expected node identifier".into(),
        });
    }
    let first = bytes[i];
    let is_ident_start = first.is_ascii_alphabetic() || first == b'_';
    if !is_ident_start {
        return Err(MermaidError::ParseError {
            line: lineno,
            reason: format!("unexpected character '{}' at column {}", first as char, i + 1),
        });
    }
    let mut p = i + 1;
    while p < n {
        let c = bytes[p];
        if c.is_ascii_alphanumeric() || c == b'_' {
            p += 1;
            continue;
        }
        break;
    }
    let s = std::str::from_utf8(&bytes[i..p]).map_err(|_| MermaidError::ParseError {
        line: lineno,
        reason: "identifier not valid UTF-8".into(),
    })?;
    Ok((s, p - i))
}

/// Find the index in `bytes` (>= `start`) where the matching `close`
/// sequence begins. Honours quotes inside the bracket region: a
/// `"…"` quoted segment is skipped so its contents cannot prematurely
/// match `close`.
fn find_close(
    bytes: &[u8],
    start: usize,
    close: &[u8],
    lineno: usize,
) -> Result<usize, MermaidError> {
    let n = bytes.len();
    let mut p = start;
    let mut in_quote = false;
    let mut prev = 0u8;
    while p < n {
        let b = bytes[p];
        if in_quote {
            if b == b'"' && prev != b'\\' {
                in_quote = false;
            }
            prev = b;
            p += 1;
            continue;
        }
        if b == b'"' {
            in_quote = true;
            prev = b;
            p += 1;
            continue;
        }
        if bytes[p..].starts_with(close) {
            return Ok(p);
        }
        prev = b;
        p += 1;
    }
    Err(MermaidError::ParseError {
        line: lineno,
        reason: format!(
            "unclosed node label (expected '{}')",
            String::from_utf8_lossy(close)
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(src: &str) -> Flowchart {
        parse(src).unwrap_or_else(|e| panic!("parse failed: {} for src={:?}", e, src))
    }

    #[test]
    fn header_td_default() {
        let fc = ok("flowchart TD\nA-->B");
        assert_eq!(fc.direction, Direction::TopDown);
    }
    #[test]
    fn header_tb_synonym() {
        assert_eq!(ok("flowchart TB\nA-->B").direction, Direction::TopDown);
    }
    #[test]
    fn header_directions() {
        for (s, d) in [
            ("LR", Direction::LeftRight),
            ("RL", Direction::RightLeft),
            ("BT", Direction::BottomTop),
        ] {
            let fc = ok(&format!("flowchart {}\nA-->B", s));
            assert_eq!(fc.direction, d, "{}", s);
        }
    }
    #[test]
    fn graph_synonym_for_flowchart() {
        let fc = ok("graph LR\nA-->B");
        assert_eq!(fc.direction, Direction::LeftRight);
    }
    #[test]
    fn header_no_direction_defaults_to_top_down() {
        assert_eq!(ok("flowchart\nA-->B").direction, Direction::TopDown);
    }
    #[test]
    fn header_unknown_direction_errors() {
        assert!(matches!(
            parse("flowchart NW\nA-->B"),
            Err(MermaidError::ParseError { .. })
        ));
    }

    #[test]
    fn simple_two_node_arrow() {
        let fc = ok("flowchart TD\nA-->B");
        assert_eq!(fc.nodes.len(), 2);
        assert_eq!(fc.edges.len(), 1);
        assert_eq!(fc.edges[0].kind, EdgeKind::Arrow);
        assert_eq!(fc.nodes[0].id, "A");
        assert_eq!(fc.nodes[1].id, "B");
    }

    #[test]
    fn chain_creates_consecutive_edges() {
        let fc = ok("flowchart TD\nA --> B --> C");
        assert_eq!(fc.nodes.len(), 3);
        assert_eq!(fc.edges.len(), 2);
        assert_eq!(fc.edges[0].from, NodeIdx(0));
        assert_eq!(fc.edges[0].to, NodeIdx(1));
        assert_eq!(fc.edges[1].from, NodeIdx(1));
        assert_eq!(fc.edges[1].to, NodeIdx(2));
    }

    #[test]
    fn node_shapes_all_seven() {
        let src = "\
flowchart TD
A[Rect]
B(Round)
C([Stadium])
D[[Sub]]
E[(Cyl)]
F((Circ))
G{Diam}
A-->B-->C-->D-->E-->F-->G
";
        let fc = ok(src);
        assert_eq!(fc.nodes.len(), 7);
        let shapes: Vec<NodeShape> = fc.nodes.iter().map(|n| n.shape).collect();
        assert_eq!(
            shapes,
            vec![
                NodeShape::Rectangle,
                NodeShape::RoundedRectangle,
                NodeShape::Stadium,
                NodeShape::Subroutine,
                NodeShape::Cylinder,
                NodeShape::Circle,
                NodeShape::Diamond,
            ]
        );
        assert_eq!(fc.nodes[0].label, "Rect");
        assert_eq!(fc.nodes[6].label, "Diam");
    }

    #[test]
    fn edge_kinds() {
        let fc = ok("flowchart TD\nA-->B\nB---C\nC-.->D\nD==>E");
        let kinds: Vec<EdgeKind> = fc.edges.iter().map(|e| e.kind).collect();
        assert_eq!(
            kinds,
            vec![EdgeKind::Arrow, EdgeKind::Line, EdgeKind::Dashed, EdgeKind::Thick]
        );
    }

    #[test]
    fn pipe_label_on_arrow() {
        let fc = ok("flowchart TD\nA-->|yes|B");
        assert_eq!(fc.edges[0].label.as_deref(), Some("yes"));
    }

    #[test]
    fn inline_label_text_on_arrow() {
        let fc = ok("flowchart TD\nA -- text --> B");
        assert_eq!(fc.edges[0].kind, EdgeKind::Arrow);
        assert_eq!(fc.edges[0].label.as_deref(), Some("text"));
    }

    #[test]
    fn inline_label_on_line() {
        let fc = ok("flowchart TD\nA -- maybe --- B");
        assert_eq!(fc.edges[0].kind, EdgeKind::Line);
        assert_eq!(fc.edges[0].label.as_deref(), Some("maybe"));
    }

    #[test]
    fn quoted_label_with_special_chars() {
        let fc = ok("flowchart TD\nA[\"Hello, world!\"]-->B");
        assert_eq!(fc.nodes[0].label, "Hello, world!");
    }

    #[test]
    fn quoted_label_keeps_brackets_inside() {
        let fc = ok("flowchart TD\nA[\"a]b\"]-->B");
        assert_eq!(fc.nodes[0].label, "a]b");
    }

    #[test]
    fn semicolon_separates_statements() {
        let fc = ok("flowchart TD\nA-->B; B-->C");
        assert_eq!(fc.edges.len(), 2);
    }

    #[test]
    fn comments_skipped() {
        let fc = ok("%% leading\nflowchart TD\n%% body\nA-->B\n%% trailing");
        assert_eq!(fc.nodes.len(), 2);
    }

    #[test]
    fn redeclaration_overrides_label_and_shape() {
        let fc = ok("flowchart TD\nA --> B\nA[Renamed]");
        assert_eq!(fc.nodes[0].id, "A");
        assert_eq!(fc.nodes[0].label, "Renamed");
    }

    #[test]
    fn subgraph_membership_recorded() {
        let src = "\
flowchart TD
subgraph s1 [Group]
A --> B
end
B --> C
";
        let fc = ok(src);
        assert_eq!(fc.subgraphs.len(), 1);
        assert_eq!(fc.subgraphs[0].id, "s1");
        assert_eq!(fc.subgraphs[0].title.as_deref(), Some("Group"));
        assert_eq!(fc.subgraphs[0].members.len(), 2);
        assert_eq!(fc.subgraphs[0].depth, 0);
    }

    #[test]
    fn nested_subgraphs() {
        let src = "\
flowchart TD
subgraph outer
  A
  subgraph inner
    B
    C --> B
  end
  A --> B
end
";
        let fc = ok(src);
        assert_eq!(fc.subgraphs.len(), 2);
        assert_eq!(fc.subgraphs[0].id, "outer");
        assert_eq!(fc.subgraphs[0].depth, 0);
        assert_eq!(fc.subgraphs[1].id, "inner");
        assert_eq!(fc.subgraphs[1].depth, 1);
        assert_eq!(fc.subgraphs[0].children, vec![1]);
        assert_eq!(fc.top_level_subgraphs, vec![0]);
    }

    #[test]
    fn unclosed_subgraph_errors() {
        let err = parse("flowchart TD\nsubgraph s1\nA --> B").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { .. }));
    }

    #[test]
    fn unexpected_end_errors() {
        let err = parse("flowchart TD\nend").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { .. }));
    }

    #[test]
    fn click_handler_rejected_with_named_error() {
        let err = parse("flowchart TD\nA --> B\nclick A \"http://example.com\"").unwrap_err();
        match err {
            MermaidError::ParseError { reason, .. } => assert!(reason.contains("click")),
            _ => panic!(),
        }
    }

    #[test]
    fn classdef_rejected_with_named_error() {
        let err = parse("flowchart TD\nclassDef foo fill:#f9f").unwrap_err();
        match err {
            MermaidError::ParseError { reason, .. } => assert!(reason.contains("classDef")),
            _ => panic!(),
        }
    }

    #[test]
    fn unclosed_node_label_errors() {
        let err = parse("flowchart TD\nA[Unclosed --> B").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { .. }));
    }

    #[test]
    fn unclosed_pipe_label_errors() {
        let err = parse("flowchart TD\nA -->|never B").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { .. }));
    }

    #[test]
    fn trailing_edge_errors() {
        let err = parse("flowchart TD\nA -->").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { .. }));
    }

    #[test]
    fn leading_edge_errors() {
        let err = parse("flowchart TD\n--> B").unwrap_err();
        assert!(matches!(err, MermaidError::ParseError { .. }));
    }

    #[test]
    fn many_nodes_under_cap_ok() {
        let mut s = String::from("flowchart TD\n");
        for i in 0..200 {
            s.push_str(&format!("n{} --> n{}\n", i, i + 1));
        }
        let fc = ok(&s);
        assert_eq!(fc.nodes.len(), 201);
    }

    #[test]
    fn node_cap_enforced() {
        let mut s = String::from("flowchart TD\n");
        for i in 0..(MAX_NODES + 5) {
            s.push_str(&format!("n{}\n", i));
        }
        let err = parse(&s).unwrap_err();
        assert!(matches!(err, MermaidError::InputTooLarge { cap, .. } if cap == "MAX_NODES"));
    }

    #[test]
    fn edge_cap_enforced() {
        // Each chain `n0 --> n1 --> n2 ...` produces edges. Use 2 nodes
        // and many redeclarations to ramp edges fast.
        let mut s = String::from("flowchart TD\n");
        for _ in 0..(MAX_EDGES + 5) {
            s.push_str("A --> B\n");
        }
        let err = parse(&s).unwrap_err();
        assert!(matches!(err, MermaidError::InputTooLarge { cap, .. } if cap == "MAX_EDGES"));
    }

    #[test]
    fn subgraph_depth_cap_enforced() {
        let mut s = String::from("flowchart TD\n");
        for i in 0..(MAX_SUBGRAPH_DEPTH + 2) {
            s.push_str(&format!("subgraph s{}\n", i));
        }
        let err = parse(&s).unwrap_err();
        assert!(matches!(
            err,
            MermaidError::InputTooLarge { cap, .. } if cap == "MAX_SUBGRAPH_DEPTH"
        ));
    }

    #[test]
    fn line_cap_enforced() {
        let mut s = String::from("flowchart TD\n");
        for _ in 0..(MAX_LINES + 2) {
            s.push('\n');
        }
        let err = parse(&s).unwrap_err();
        assert!(matches!(err, MermaidError::InputTooLarge { cap, .. } if cap == "MAX_LINES"));
    }

    #[test]
    fn invalid_utf8_in_label_passthrough_safe() {
        // We can only feed valid UTF-8 strings to parse, but we can
        // verify the parser accepts non-ASCII labels.
        let fc = ok("flowchart TD\nA[\"Привет ✓\"]-->B");
        assert_eq!(fc.nodes[0].label, "Привет ✓");
    }
}
