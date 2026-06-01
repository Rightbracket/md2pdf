//! Flowchart sub-renderer: parser → layout → SVG emit.
//!
//! Public entry point: [`render`]. Modules are public-in-crate so the
//! sibling sequence-diagram Work can reuse the pieces it needs (shared
//! IR conventions, label-quoting helpers) without re-implementing them.

pub mod emit;
pub mod ir;
pub mod layout;
pub mod parser;

use super::MermaidError;

/// Render a mermaid flowchart-source string to SVG bytes.
///
/// The leading diagram-type token must already be `flowchart` or
/// `graph`; that dispatch happens in [`crate::mermaid::render`]. This
/// function trusts the caller and reports parse errors against the
/// flowchart grammar.
pub fn render(src: &str) -> Result<Vec<u8>, MermaidError> {
    let mut fc = parser::parse(src)?;
    layout::layout(&mut fc)?;
    Ok(emit::emit(&fc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_to_end_simple() {
        let svg = render("flowchart TD\nA[Start]-->B[End]").unwrap();
        let s = std::str::from_utf8(&svg).unwrap();
        assert!(s.contains("<svg"));
        assert!(s.contains("Start"));
        assert!(s.contains("End"));
    }
}
