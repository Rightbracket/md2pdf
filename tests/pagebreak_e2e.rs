//! Pagebreak directive end-to-end tests (W-pagebreak per D-875e4b §1d, §2a, §7a).
//!
//! Two layers:
//! 1. **Library-level Typst-emit verification** (`emit_typst_body` →
//!    inspect the generated Typst source for `#pagebreak()` placement).
//!    Cheap, deterministic, runs in milliseconds.
//! 2. **Binary-level PDF round-trip** (`md2pdf README.md` → assert exit
//!    0 + non-trivial PDF written). Confirms the end-to-end pipeline
//!    composes (pagebreak emission → Typst compile → PDF export).
//!
//! Per `done_definition #7`: README.md line 211 fixture must round-trip
//! to a single `#pagebreak()` in the emitted Typst source.
//!
//! Per `done_definition #8`: `tests/fixtures/pagebreak_edge_cases.md`
//! must drive the four U-976c35 edge cases (leading/trailing/back-to-back/
//! mid-document) plus the D-875e4b §6c parent-context fall-throughs
//! (fenced code, blockquote).

use std::fs;

use assert_cmd::Command;
use md2pdf::emitter::{StubMermaidDispatcher, emit_typst_body};
use md2pdf::image_pipeline::PipelineBuilder;
use md2pdf::warnings::WarningCollector;
use tempfile::TempDir;

const README_FIXTURE: &str = include_str!("../README.md");
const EDGE_CASES_FIXTURE: &str =
    include_str!("fixtures/pagebreak_edge_cases.md");

/// Convenience wrapper: drive `emit_typst_body` over a Markdown source
/// using a stubbed mermaid dispatcher, scratch image-pipeline, and a
/// silenced warning collector. Returns the emitted Typst body string.
fn emit_for(md: &str) -> (String, WarningCollector) {
    let tmp = std::env::temp_dir();
    let mut p = PipelineBuilder::new(tmp).build();
    let mut m = StubMermaidDispatcher;
    let mut w = WarningCollector::new();
    let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
    (out, w)
}

/// Count standalone `#pagebreak()` emissions in `out` — i.e. the
/// directive emitted by `flush_pending_pagebreaks` (`#pagebreak()\n\n`),
/// distinguishable from any `#pagebreak()` substring that happens to
/// appear inside Markdown prose (e.g. a fixture explaining the
/// directive — those would be wrapped in `md_inline_code(...)` and
/// not preceded/followed by raw newlines).
///
/// The flush form is `#pagebreak()\n\n`, always with two trailing
/// newlines and either a leading newline or sitting at the start of
/// the buffer. We match `#pagebreak()\n\n` to count emits exactly.
fn count_pagebreak_emits(out: &str) -> usize {
    out.matches("#pagebreak()\n\n").count()
}

#[test]
fn readme_line_211_pagebreak_is_recognized() {
    // README.md line 211 is the canonical positive fixture: a standalone
    // `<!-- pagebreak -->` HTML comment surrounded by blank lines, at
    // top level. Per W-pagebreak done_definition #7: round-trip produces
    // a single `#pagebreak()` in the emitted Typst source.
    //
    // The README has unrelated warning surfaces (image fetch from a
    // tmp working dir, mermaid stub) — we do NOT assert zero warnings
    // here, only that pagebreak recognition itself fires correctly.
    let (out, _) = emit_for(README_FIXTURE);
    let count = count_pagebreak_emits(&out);
    assert_eq!(
        count, 1,
        "README.md line 211 must emit exactly one #pagebreak(); got {count}"
    );
    // The pagebreak comment must NOT pass through as md_inline_html.
    assert!(
        !out.contains("md_inline_html(\"<!-- pagebreak -->\")"),
        "pagebreak comment leaked through md_inline_html literal path"
    );
}

#[test]
fn edge_cases_fixture_emits_expected_pagebreaks() {
    // The fixture has the following recognizable, top-level pagebreak
    // comments:
    //   Section B  — single mid-doc pagebreak (1)                         → 1 emit
    //   Section C  — two back-to-back mid-doc pagebreaks                  → 2 emits
    //   Section D  — page-break alias                                     → 1 emit
    //   Section E  — uppercase + whitespace                               → 1 emit
    //   Section F  — pagebreak inside fenced code block                   → 0 emits (Text, not Html)
    //   Section G  — pagebreak inside a blockquote                        → 0 emits (non-top-level)
    //   Section H  — payload-bearing form (`<!-- pagebreak: top -->`)     → 0 emits (rejected)
    //   Section I  — stub paragraph (no top-level pagebreak in this build) → 0 emits
    //   Section Z  — trailing pagebreak                                   → 0 emits (suppressed)
    //
    // Expected total: 5 `#pagebreak()` invocations in the emitted output.
    let (out, w) = emit_for(EDGE_CASES_FIXTURE);
    let count = count_pagebreak_emits(&out);
    if count != 5 {
        eprintln!("---EMITTED---\n{out}\n---END---");
    }
    assert_eq!(
        count, 5,
        "expected 5 #pagebreak() invocations from the edge-cases fixture, got {count}"
    );
    // No warnings on graceful fall-through paths (D-875e4b §6c).
    assert_eq!(
        w.count(),
        0,
        "pagebreak fall-through paths must not warn; got: {:?}",
        w.warnings()
    );
}

#[test]
fn edge_cases_fixture_blockquote_pagebreak_falls_through_to_literal() {
    // Section G's pagebreak comment is inside a Markdown blockquote;
    // per D-875e4b §1c top-level-only constraint, recognition does NOT
    // fire. The comment must surface in the output as literal source
    // via `md_inline_html`.
    let (out, _) = emit_for(EDGE_CASES_FIXTURE);
    assert!(
        out.contains("md_inline_html"),
        "edge-cases fixture must surface non-recognized comment forms via md_inline_html;\nout:\n{out}"
    );
}

#[test]
fn edge_cases_fixture_payload_form_falls_through_to_literal() {
    // Section H: `<!-- pagebreak: top -->` is rejected per the
    // strict-no-payload rule; the comment renders as literal source.
    // We assert that the literal payload string survives in the
    // emitted Typst.
    let (out, _) = emit_for(EDGE_CASES_FIXTURE);
    assert!(
        out.contains("pagebreak: top"),
        "payload-bearing comment must survive as literal source;\nout:\n{out}"
    );
}

#[test]
fn edge_cases_fixture_fenced_code_pagebreak_renders_as_code() {
    // Section F: pagebreak inside a fenced code block becomes
    // `Event::Text` per pulldown-cmark's CommonMark conformance, and
    // therefore renders as code source via `md_codeblock`.
    let (out, _) = emit_for(EDGE_CASES_FIXTURE);
    assert!(
        out.contains("md_codeblock"),
        "edge-cases fixture must render the fenced-code section via md_codeblock;\nout:\n{out}"
    );
}

#[test]
fn edge_cases_fixture_round_trips_to_pdf() {
    // End-to-end: drive the binary over the edge-cases fixture and
    // assert the PDF compiles + writes successfully. Confirms the
    // emitted `#pagebreak()` calls + surrounding `md_inline_html`
    // literals all compose into a clean Typst program.
    let dir = TempDir::new().unwrap();
    let md_path = dir.path().join("pagebreak_edge_cases.md");
    fs::write(&md_path, EDGE_CASES_FIXTURE).unwrap();

    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd.arg(&md_path).output().expect("spawn md2pdf");
    assert!(
        output.status.success(),
        "md2pdf exited non-zero: code={:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let pdf_path = dir.path().join("pagebreak_edge_cases.pdf");
    assert!(
        pdf_path.exists(),
        "expected {} to exist",
        pdf_path.display()
    );
    let bytes = fs::read(&pdf_path).unwrap();
    assert!(
        bytes.starts_with(b"%PDF"),
        "output does not start with PDF magic; first 8 bytes = {:?}",
        bytes.iter().take(8).collect::<Vec<_>>()
    );

    // Drop a copy under target/ for human inspection (the Reviewer can
    // `open target/pagebreak_edge_cases.pdf`).
    let inspect = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("pagebreak_edge_cases.pdf");
    let _ = fs::write(&inspect, &bytes);
}
