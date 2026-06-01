//! Mermaid diagram sub-renderer.
//!
//! Public surface for W-3686a9 (flowchart) and the sibling sequence-diagram
//! Work. The architecture follows D-fb4ebb §1:
//!
//! ```text
//!   mermaid src
//!     → diagram-type sniff (first non-blank, non-comment token)
//!     → per-type recursive-descent parser → typed IR
//!     → layout → coord-annotated IR
//!     → constrained-subset SVG bytes
//!     → handed to Typst's image element (resvg ingestion)
//! ```
//!
//! The flowchart implementation lives in [`flowchart`]. The sequence-diagram
//! sub-renderer is a sibling Work item. Out-of-subset diagram types
//! (`gantt`, `classDiagram`, …) are recognised by the type sniffer and
//! returned as [`MermaidError::UnsupportedDiagramType`] — never silently
//! rendered, never silently dropped (D-fb4ebb §1).
//!
//! ## Hostile-input safety (W-3686a9 done_definition)
//!
//! All caps below are enforced at parse time. Crossing one is a typed
//! error, not a panic. The caps are deliberately well above any
//! plausible hand-authored mermaid block and well below anything that
//! would degrade memory/CPU.
//!
//! - `MAX_INPUT_BYTES`: 256 KiB
//! - `MAX_LINES`: 5_000
//! - `MAX_NODES`: 500
//! - `MAX_EDGES`: 2_000
//! - `MAX_SUBGRAPH_DEPTH`: 16
//! - `MAX_LABEL_LEN`: 1_024 chars
//! - `MAX_TOKENS_PER_LINE`: 256
//!
//! ## Out-of-subset behavior
//!
//! Diagram types we recognise but do not implement in v1 produce
//! [`MermaidError::UnsupportedDiagramType`]; tokens we cannot identify
//! at all produce [`MermaidError::UnknownDiagramType`]. Either is a
//! build error (exit code 5 per D-fb4ebb §3); the caller maps the error
//! to [`crate::error::Md2PdfError::Mermaid`].

pub mod diagram_type;
pub mod flowchart;
pub mod sequence;
pub mod svg_buf;

use std::fmt;

pub use diagram_type::{sniff_diagram_type, DiagramType};

// -- caps -----------------------------------------------------------------

/// Hard cap on input bytes per mermaid block. 256 KiB is ~5× the
/// largest hand-authored mermaid block we found in the wild and ~1/80
/// the default OS process stack.
pub const MAX_INPUT_BYTES: usize = 256 * 1024;
pub const MAX_LINES: usize = 5_000;
pub const MAX_NODES: usize = 500;
pub const MAX_EDGES: usize = 2_000;
pub const MAX_SUBGRAPH_DEPTH: usize = 16;
pub const MAX_LABEL_LEN: usize = 1_024;
pub const MAX_TOKENS_PER_LINE: usize = 256;

// Sequence-diagram-specific caps (W-4c3f16). Same justification as the
// flowchart caps: well above any plausible hand-authored diagram, well
// below anything that degrades memory/CPU. Crossing one is a typed
// `MermaidError`, never a panic.
pub const MAX_ACTORS: usize = 256;
pub const MAX_EVENTS: usize = 4_096;
pub const MAX_GROUP_DEPTH: usize = 8;
pub const MAX_LINE_BYTES: usize = 4_096;

// -- error type -----------------------------------------------------------

/// Typed errors from the mermaid sub-renderer. Each variant maps to a
/// human-readable message naming the unsupported feature so authors can
/// fix Markdown source without spelunking through stack traces. Per
/// D-fb4ebb §3 these all map to exit code 5.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidError {
    /// Recognised diagram type that v1 does not implement (e.g.
    /// `gantt`, `classDiagram`).
    UnsupportedDiagramType { ty: String },
    /// Leading token did not match any known mermaid diagram keyword.
    UnknownDiagramType { token: String },
    /// Input exceeds a static cap. The cap name is included so authors
    /// know which knob tripped.
    InputTooLarge { cap: &'static str, limit: usize },
    /// Recoverable parse error pinned to a 1-indexed line number with a
    /// short reason.
    ParseError { line: usize, reason: String },
    /// Layout pass refused to lay out the IR (e.g. exceeded node cap
    /// after subgraph expansion).
    LayoutError { reason: String },
}

impl fmt::Display for MermaidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MermaidError::UnsupportedDiagramType { ty } => {
                write!(
                    f,
                    "mermaid: diagram type '{}' is not yet supported in md2pdf v1",
                    ty
                )
            }
            MermaidError::UnknownDiagramType { token } => {
                write!(
                    f,
                    "mermaid: could not identify diagram type from leading token '{}'",
                    token
                )
            }
            MermaidError::InputTooLarge { cap, limit } => {
                write!(f, "mermaid: input rejected: {} > {}", cap, limit)
            }
            MermaidError::ParseError { line, reason } => {
                write!(f, "mermaid: parse error at line {}: {}", line, reason)
            }
            MermaidError::LayoutError { reason } => {
                write!(f, "mermaid: layout error: {}", reason)
            }
        }
    }
}

impl std::error::Error for MermaidError {}

// -- public render entry point -------------------------------------------

/// Render a mermaid block to SVG bytes.
///
/// Top-level dispatch: sniff the diagram type, route to the matching
/// per-type sub-renderer (currently only flowchart), or emit a typed
/// error for out-of-subset / unknown types.
pub fn render(src: &str) -> Result<Vec<u8>, MermaidError> {
    if src.len() > MAX_INPUT_BYTES {
        return Err(MermaidError::InputTooLarge {
            cap: "MAX_INPUT_BYTES",
            limit: MAX_INPUT_BYTES,
        });
    }
    match sniff_diagram_type(src)? {
        DiagramType::Flowchart => flowchart::render(src),
        DiagramType::SequenceDiagram => sequence::render(src),
        DiagramType::Unsupported(name) => {
            Err(MermaidError::UnsupportedDiagramType { ty: name })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_type_returns_named_error() {
        let err = render("gantt\n  title G").unwrap_err();
        assert!(matches!(err, MermaidError::UnsupportedDiagramType { ref ty } if ty == "gantt"));
        let msg = err.to_string();
        assert!(msg.contains("gantt"));
        assert!(msg.contains("not yet supported"));
    }

    #[test]
    fn unknown_type_returns_unknown_error() {
        let err = render("blarghDiagram\n  x").unwrap_err();
        assert!(matches!(err, MermaidError::UnknownDiagramType { .. }));
    }

    #[test]
    fn empty_input_unknown() {
        let err = render("").unwrap_err();
        assert!(matches!(err, MermaidError::UnknownDiagramType { .. }));
    }

    #[test]
    fn input_too_large_returns_typed_error() {
        let big = "a".repeat(MAX_INPUT_BYTES + 1);
        let err = render(&big).unwrap_err();
        assert!(matches!(err, MermaidError::InputTooLarge { .. }));
    }

    #[test]
    fn flowchart_keyword_dispatches_to_flowchart() {
        let svg = render("flowchart TD\nA-->B").unwrap();
        let s = std::str::from_utf8(&svg).unwrap();
        assert!(s.starts_with("<?xml") || s.starts_with("<svg"));
        assert!(s.contains("<svg"));
    }

    #[test]
    fn graph_keyword_is_flowchart_synonym() {
        let svg = render("graph LR\nA-->B").unwrap();
        let s = std::str::from_utf8(&svg).unwrap();
        assert!(s.contains("<svg"));
    }
}
