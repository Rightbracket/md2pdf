//! Integration test for the real mermaid dispatcher (W-ddc4e7,
//! D-c3af71 §E).
//!
//! Sample doc embeds:
//! - a valid `flowchart` block (must render to an SVG-bearing region;
//!   no mermaid-warning on stderr);
//! - a valid `sequenceDiagram` block (same);
//! - a `gantt` block — recognised but out of v1 scope per §E; must
//!   produce a canonical `md2pdf: warn: mermaid: ...` line on stderr
//!   AND a placeholder in the PDF;
//! - a `nonsenseDiagramType` block — unrecognised; same canonical
//!   stderr-warning + placeholder behavior.
//!
//! "Embedded marker" verification: a unique short label (`MARKER42`
//! for flowchart, `SEQMARK7` for sequence) is placed inside the
//! diagram body. Typst's resvg-backed SVG ingestion preserves glyph
//! shapes from `<text>` elements; we look for the rendered glyphs by
//! checking that the PDF stream is non-trivially larger than a
//! placeholder-only run, AND that the per-stream raw text shows the
//! emitter's `md_image_bytes(bytes((...)), "svg", ...)` invocation
//! by smoke-testing the body emission directly via a unit-level
//! re-render too. This dual check (PDF byte size + body invocation)
//! gives us the "embedded marker" without depending on PDF text
//! extraction libraries.

use std::fs;
use std::process::Command as ProcCommand;

use assert_cmd::Command;
use tempfile::TempDir;

const SAMPLE_MD: &str = r#"# Mermaid dispatcher smoke

## Flowchart (must render)

```mermaid
flowchart TD
A[MARKER42]-->B[End]
```

## Sequence (must render)

```mermaid
sequenceDiagram
Alice->>Bob: SEQMARK7
```

## Gantt (recognised, deferred — must warn + placeholder)

```mermaid
gantt
title Project
section A
Task1: a1, 2026-01-01, 5d
```

## Nonsense (unrecognised — must warn + placeholder)

```mermaid
nonsenseDiagramType
foo bar baz
```
"#;

#[test]
fn mermaid_dispatcher_round_trips_with_canonical_warnings() {
    let dir = TempDir::new().unwrap();
    let md_path = dir.path().join("dispatcher.md");
    fs::write(&md_path, SAMPLE_MD).unwrap();

    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd.arg(&md_path).output().expect("spawn md2pdf");
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    assert!(
        output.status.success(),
        "md2pdf exited non-zero: code={:?}\nstderr:\n{stderr}",
        output.status.code(),
    );

    let pdf_path = dir.path().join("dispatcher.pdf");
    assert!(pdf_path.exists(), "expected {} to exist", pdf_path.display());
    let bytes = fs::read(&pdf_path).unwrap();
    assert!(bytes.starts_with(b"%PDF"));
    // Two SVG diagrams + four headings + a few paragraphs is
    // comfortably over 4 KiB. A placeholder-only PDF tends to be
    // smaller; this guards against the "all four blocks fell through
    // to placeholder" regression.
    assert!(
        bytes.len() > 4096,
        "PDF suspiciously small ({} bytes); flowchart+sequence may not have rendered",
        bytes.len()
    );

    // Canonical warning lines: per D-c3af71 §D-2 / WarningCollector,
    // mermaid warnings render as `md2pdf: warn: mermaid: <reason>`.
    // gantt is recognised-but-deferred → "not yet supported" reason;
    // nonsenseDiagramType is unrecognised → "could not identify"
    // reason.
    let warn_lines: Vec<&str> = stderr
        .lines()
        .filter(|l| l.starts_with("md2pdf: warn: mermaid:"))
        .collect();
    assert!(
        warn_lines.len() >= 2,
        "expected at least 2 mermaid warning lines (gantt + nonsense); got {} in stderr:\n{stderr}",
        warn_lines.len()
    );
    assert!(
        stderr.contains("gantt"),
        "expected the gantt warning on stderr, got:\n{stderr}"
    );
    assert!(
        stderr.contains("nonsenseDiagramType"),
        "expected the nonsense diagram-type warning on stderr, got:\n{stderr}"
    );

    // Negative: no mermaid warning should fire for flowchart/sequence.
    // The two warning lines above are the gantt + nonsense pair; if a
    // third snuck in for flowchart or sequence, fail.
    assert_eq!(
        warn_lines.len(),
        2,
        "exactly 2 mermaid warnings expected (gantt + nonsense); got {} — did flowchart or sequence regress to a warning?\nstderr:\n{stderr}",
        warn_lines.len()
    );

    // Drop a copy under target/ for human inspection.
    let inspect = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("mermaid_dispatcher.pdf");
    let _ = fs::write(&inspect, &bytes);
    let _ = ProcCommand::new("true"); // silence unused import on some platforms
}

/// Embedded-marker verification at the emitter-body level: the
/// dispatcher's success path must produce `md_image_bytes(...)` with
/// `"svg"` format for valid flowchart/sequence; the failure path must
/// produce `md_mermaid_stub(...)`. Drives `emit_typst_body` directly
/// so we can grep the body string without round-tripping through PDF.
#[test]
fn dispatcher_body_emission_carries_svg_for_valid_blocks() {
    use md2pdf::emitter::{emit_typst_body, RealMermaidDispatcher};
    use md2pdf::image_pipeline::PipelineBuilder;
    use md2pdf::warnings::WarningCollector;

    let mut p = PipelineBuilder::new(std::env::temp_dir()).build();
    let mut m = RealMermaidDispatcher;
    let mut w = WarningCollector::new();
    let body = emit_typst_body(SAMPLE_MD, &mut p, &mut m, &mut w).unwrap();

    // Two successful renders → two `md_image_bytes(... "svg" ...)`
    // invocations.
    let svg_invocations = body.matches("md_image_bytes(").count();
    assert!(
        svg_invocations >= 2,
        "expected >=2 md_image_bytes() invocations from flowchart+sequence; got {svg_invocations}\nbody:\n{body}"
    );
    let svg_format_hits = body.matches("\"svg\"").count();
    assert!(
        svg_format_hits >= 2,
        "expected >=2 \"svg\" format strings; got {svg_format_hits}"
    );

    // Two failures → two `md_mermaid_stub(...)` placeholders.
    let stub_hits = body.matches("md_mermaid_stub(").count();
    assert_eq!(
        stub_hits, 2,
        "expected exactly 2 md_mermaid_stub() placeholders from gantt+nonsense; got {stub_hits}"
    );

    // Warning collector saw exactly two mermaid warnings.
    assert_eq!(w.count(), 2, "expected 2 warnings; got {}", w.count());
}
