//! Diagram-type sniffer. Looks at the first non-blank, non-comment
//! token of the mermaid source and dispatches.
//!
//! Per D-fb4ebb §1: `flowchart` and `graph` are synonyms (the mermaid
//! project does the same); `sequenceDiagram` is recognised but routed
//! to the sibling Work; the explicit out-of-subset list maps to typed
//! `UnsupportedDiagramType` errors; anything else is
//! `UnknownDiagramType`.

use super::MermaidError;

/// What the sniffer produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagramType {
    /// `flowchart` / `graph` — handled by the flowchart sub-renderer.
    Flowchart,
    /// `sequenceDiagram` — handled by the sequence sub-renderer
    /// (W-4c3f16).
    SequenceDiagram,
    /// Recognised mermaid diagram type that this v1 build does not
    /// render. The caller turns this into a
    /// [`MermaidError::UnsupportedDiagramType`] with the contained name.
    Unsupported(String),
}

/// The exhaustive list of recognised-but-unsupported diagram tokens
/// pinned by D-fb4ebb §1.
const UNSUPPORTED: &[&str] = &[
    "gantt",
    "classDiagram",
    "classDiagram-v2",
    "stateDiagram",
    "stateDiagram-v2",
    "erDiagram",
    "journey",
    "pie",
    "mindmap",
    "quadrantChart",
    "requirementDiagram",
    "gitGraph",
    "C4Context",
    "C4Container",
    "C4Component",
    "timeline",
    "sankey-beta",
    "xychart-beta",
    "block-beta",
];

/// Pull the first non-blank, non-comment token. Mermaid comments start
/// with `%%`. A "token" here is the run of non-whitespace bytes at the
/// start of the first significant line.
pub fn sniff_diagram_type(src: &str) -> Result<DiagramType, MermaidError> {
    let token = first_significant_token(src);
    match token {
        None => Err(MermaidError::UnknownDiagramType {
            token: String::new(),
        }),
        Some(t) => {
            if t == "flowchart" || t == "graph" {
                Ok(DiagramType::Flowchart)
            } else if t == "sequenceDiagram" {
                Ok(DiagramType::SequenceDiagram)
            } else if UNSUPPORTED.iter().any(|u| *u == t) {
                Ok(DiagramType::Unsupported(t.to_string()))
            } else {
                Err(MermaidError::UnknownDiagramType {
                    token: t.to_string(),
                })
            }
        }
    }
}

fn first_significant_token(src: &str) -> Option<&str> {
    for line in src.lines() {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("%%") {
            continue;
        }
        // Token = run of non-whitespace from the start.
        let end = trimmed
            .find(|c: char| c.is_whitespace())
            .unwrap_or(trimmed.len());
        return Some(&trimmed[..end]);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flowchart_keyword() {
        assert_eq!(
            sniff_diagram_type("flowchart TD\nA-->B").unwrap(),
            DiagramType::Flowchart
        );
    }

    #[test]
    fn graph_synonym() {
        assert_eq!(
            sniff_diagram_type("graph LR\nA-->B").unwrap(),
            DiagramType::Flowchart
        );
    }

    #[test]
    fn skips_blank_and_comment_lines() {
        let src = "\n\n%% leading comment\n%% another\nflowchart TD\nA-->B";
        assert_eq!(sniff_diagram_type(src).unwrap(), DiagramType::Flowchart);
    }

    #[test]
    fn sequence_diagram_recognised() {
        assert_eq!(
            sniff_diagram_type("sequenceDiagram\n  A->>B: m").unwrap(),
            DiagramType::SequenceDiagram
        );
    }

    #[test]
    fn gantt_unsupported() {
        match sniff_diagram_type("gantt\n  title").unwrap() {
            DiagramType::Unsupported(n) => assert_eq!(n, "gantt"),
            _ => panic!(),
        }
    }

    #[test]
    fn unknown_token_errors() {
        let err = sniff_diagram_type("blargh\n  x").unwrap_err();
        match err {
            MermaidError::UnknownDiagramType { token } => assert_eq!(token, "blargh"),
            _ => panic!(),
        }
    }

    #[test]
    fn empty_input_errors() {
        assert!(matches!(
            sniff_diagram_type(""),
            Err(MermaidError::UnknownDiagramType { .. })
        ));
    }

    #[test]
    fn whitespace_only_errors() {
        assert!(matches!(
            sniff_diagram_type("   \n\t\n%% only comment"),
            Err(MermaidError::UnknownDiagramType { .. })
        ));
    }

    #[test]
    fn token_terminates_at_whitespace_not_punctuation() {
        // `flowchart-extra` is not the keyword; it should be unknown.
        assert!(matches!(
            sniff_diagram_type("flowchart-extra TD\nA-->B"),
            Err(MermaidError::UnknownDiagramType { .. })
        ));
    }
}
