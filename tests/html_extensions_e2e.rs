//! HTML extensions end-to-end test (W-650a51 per D-875e4b §1–§4, §6,
//! §7b).
//!
//! Two layers (mirrors `tests/pagebreak_e2e.rs`):
//!
//! 1. **Library-level Typst-emit verification** — drive `emit_typst_body`
//!    over the fixture and assert the emitted Typst source contains
//!    expected `md_html_table(...)` invocations, distinct border-style
//!    stroke shapes, paragraph-level `*…*` / `_…_` markup for outside-
//!    cell formatters, `#md_hardbreak()` for `<br>`, and pass-through
//!    of mismatched/unrecognized tags via `#md_inline_html(...)`.
//!
//! 2. **Binary-level PDF round-trip** — `md2pdf html_extensions.md` →
//!    assert exit 0 + non-trivial PDF written. Confirms the full
//!    pipeline (HTML parse → IR → Typst-emit → Typst-compile → PDF
//!    export) composes cleanly under the recipe the Reviewer + Security
//!    Engineer will read.

use std::fs;

use assert_cmd::Command;
use md2pdf::emitter::{emit_typst_body, StubMermaidDispatcher};
use md2pdf::image_pipeline::PipelineBuilder;
use md2pdf::warnings::WarningCollector;
use tempfile::TempDir;

const FIXTURE: &str = include_str!("fixtures/html_extensions.md");

fn emit_for(md: &str) -> (String, WarningCollector) {
    let tmp = std::env::temp_dir();
    let mut p = PipelineBuilder::new(tmp).build();
    let mut m = StubMermaidDispatcher;
    let mut w = WarningCollector::new();
    let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
    (out, w)
}

fn count_html_tables(out: &str) -> usize {
    out.matches("#md_html_table(").count()
}

#[test]
fn fixture_emits_one_html_table_per_table_block() {
    // The fixture has 8 `<table>...</table>` blocks: A, B×3, C, D, E, H.
    let (out, _) = emit_for(FIXTURE);
    let count = count_html_tables(&out);
    assert_eq!(
        count, 8,
        "expected 8 #md_html_table(...) invocations from the fixture, got {count}\n\
         emitted:\n{out}"
    );
}

#[test]
fn fixture_border_styles_emit_distinct_stroke_shapes() {
    // Section B has three tables: solid / dashed / dotted. Per
    // D-875e4b §3c the three styles must produce three distinct
    // Typst stroke shapes.
    //   solid  → (thickness: 1pt, paint: rgb("#000000"))
    //   dashed → (thickness: 1pt, paint: rgb("#000000"), dash: "dashed")
    //   dotted → (thickness: 1pt, paint: rgb("#000000"), dash: "dotted")
    let (out, _) = emit_for(FIXTURE);
    assert!(
        out.contains("dash: \"dashed\""),
        "dashed border style must emit dash: \"dashed\";\n{out}"
    );
    assert!(
        out.contains("dash: \"dotted\""),
        "dotted border style must emit dash: \"dotted\";\n{out}"
    );
    // Solid should NOT carry a dash key (it's the default).
    let solid_section_marker = out
        .find("solid")
        .expect("fixture references solid border somewhere");
    let solid_window =
        &out[solid_section_marker.saturating_sub(200)..solid_section_marker];
    assert!(
        !solid_window.contains("dash:"),
        "solid border emit must not carry a dash key in the nearest stroke shape;\nwindow:\n{solid_window}"
    );
}

#[test]
fn fixture_colspan_and_rowspan_emit_table_cell_wrappers() {
    // Section C exercises both colspan=3 (header) and rowspan=2 (body
    // first cell of 2025 row). The typst_emit module always emits the
    // colspan attribute before rowspan, so even a pure-rowspan cell
    // looks like `table.cell(colspan: 1, rowspan: 2)`.
    let (out, _) = emit_for(FIXTURE);
    assert!(
        out.contains("table.cell(colspan: 3"),
        "section C colspan=3 must emit table.cell(colspan: 3 ...);\n{out}"
    );
    assert!(
        out.contains("rowspan: 2)"),
        "section C rowspan=2 must emit a table.cell(... rowspan: 2);\n{out}"
    );
}

#[test]
fn fixture_thead_rows_wrap_in_table_header() {
    // Per Decision §2c: rows inside `<thead>` must be wrapped in
    // Typst `table.header(...)` so Typst repeats them across page
    // breaks. The fixture has thead in sections A, C, and D; body-only
    // tables (B×3, E, H) must NOT carry a header wrapper.
    let (out, _) = emit_for(FIXTURE);
    let header_count = out.matches("table.header(").count();
    assert_eq!(
        header_count, 3,
        "expected 3 table.header(...) wrappers (sections A, C, D); got {header_count}\n\
         emit:\n{out}"
    );
}

#[test]
fn fixture_collapse_default_emits_one_pt_black_border() {
    // Section E: `border-collapse: collapse` with no explicit border
    // → 1pt black per Decision §2f.
    let (out, _) = emit_for(FIXTURE);
    // The Section E table is the only one in the fixture with the
    // exact text "collapsed" inside a cell. Find its md_html_table
    // call by walking forward from the literal "collapsed".
    let collapsed_idx = out
        .find("[collapsed]")
        .expect("section E must emit cell text [collapsed];");
    // Walk back to the nearest preceding `#md_html_table(` so we
    // can inspect its stroke argument. The cells argument is the
    // last positional, so the stroke argument precedes it.
    let table_start = out[..collapsed_idx]
        .rfind("#md_html_table(")
        .expect("section E cell must sit inside an md_html_table call");
    let window = &out[table_start..collapsed_idx];
    assert!(
        window.contains("(thickness: 1pt, paint: rgb(\"#000000\"))"),
        "section E table must emit 1pt black stroke for collapse-default;\nwindow:\n{window}"
    );
}

#[test]
fn fixture_outside_cell_inline_formatters_emit_typst_markup() {
    // Section F flowing text:
    //   <b>bold</b>             → *bold*
    //   <i>italic</i>           → _italic_
    //   <strong>strong-bold</strong> → *strong\-bold* (the `-` is
    //                                  Typst-escaped per
    //                                  `escape_typst_markup`).
    //   <em>emphasized-italic</em> → _emphasized\-italic_
    //   <br>                    → #md_hardbreak()
    let (out, _) = emit_for(FIXTURE);
    assert!(
        out.contains("*bold*"),
        "<b>bold</b> must emit Typst *bold*;\n{out}"
    );
    assert!(
        out.contains("_italic_"),
        "<i>italic</i> must emit Typst _italic_;\n{out}"
    );
    assert!(
        out.contains(r"*strong\-bold*"),
        "<strong>strong-bold</strong> must emit Typst *strong\\-bold* \
         (hyphen Typst-escaped);\n{out}"
    );
    assert!(
        out.contains(r"_emphasized\-italic_"),
        "<em>emphasized-italic</em> must emit Typst _emphasized\\-italic_ \
         (hyphen Typst-escaped);\n{out}"
    );
    assert!(
        out.contains("#md_hardbreak()"),
        "<br> must emit #md_hardbreak();\n{out}"
    );
}

#[test]
fn fixture_mismatched_close_falls_through_to_inline_html() {
    // Section G: a stray `</b>` with no preceding open. Per Decision
    // §6k this falls through to literal pass-through via
    // `#md_inline_html(...)`.
    let (out, _) = emit_for(FIXTURE);
    assert!(
        out.contains("#md_inline_html(\"</b>\")"),
        "stray </b> must surface as literal #md_inline_html(\"</b>\");\n{out}"
    );
}

#[test]
fn fixture_unrecognized_inline_tag_falls_through() {
    // Section G: `<span>` is not in the recognized 5-element subset,
    // so it falls through to `#md_inline_html(...)`.
    let (out, _) = emit_for(FIXTURE);
    assert!(
        out.contains("#md_inline_html(\"<span>\")"),
        "<span> must surface as literal #md_inline_html(\"<span>\");\n{out}"
    );
    assert!(
        out.contains("#md_inline_html(\"</span>\")"),
        "</span> must surface as literal #md_inline_html(\"</span>\");\n{out}"
    );
}

#[test]
fn fixture_cell_content_inline_formatters_apply_inside_cells() {
    // Section H: `<b>bold</b>` inside a `<td>` should become Typst
    // `*bold*` inside the cell content, exercised through the IR-build
    // path of the parser (NOT the outside-cell streaming classifier).
    let (out, _) = emit_for(FIXTURE);
    assert!(
        out.contains("*bold*"),
        "section H cell content must emit *bold* via parser IR;\n{out}"
    );
    // The line-break inside the cell ("line one<br>line two") emits
    // a Typst linebreak() call (parser → emit goes through #linebreak()
    // for cell-internal <br>, NOT #md_hardbreak() — those are different
    // emit paths per typst_emit::inlines_linebreak unit test).
    assert!(
        out.contains("#linebreak()"),
        "section H cell content must emit #linebreak() for <br>;\n{out}"
    );
}

#[test]
fn fixture_emits_zero_warnings_on_recognized_paths() {
    // The fixture exercises only well-formed HTML for the recognized
    // subset; per Decision §6 graceful-degradation rules, none of the
    // outside-cell or in-cell paths should fire warnings on this
    // input. (Mismatched close / unrecognized inline tags fall
    // through silently; they are not warning-class events.)
    let (_, w) = emit_for(FIXTURE);
    assert_eq!(
        w.count(),
        0,
        "fixture must not produce warnings on recognized paths; got: {:?}",
        w.warnings()
    );
}

#[test]
fn fixture_round_trips_to_pdf() {
    let dir = TempDir::new().unwrap();
    let md_path = dir.path().join("html_extensions.md");
    fs::write(&md_path, FIXTURE).unwrap();

    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd.arg(&md_path).output().expect("spawn md2pdf");
    assert!(
        output.status.success(),
        "md2pdf exited non-zero: code={:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let pdf_path = dir.path().join("html_extensions.pdf");
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
    assert!(
        bytes.len() > 1500,
        "html-extensions PDF suspiciously small: {} bytes",
        bytes.len()
    );

    // Drop a copy under target/ for human inspection (Reviewer can
    // `open target/html_extensions.pdf`).
    let inspect = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("html_extensions.pdf");
    let _ = fs::write(&inspect, &bytes);
}

// =============================================================================
// Per-section micro-fixture round-trips (W-650a51 done_definition #18)
// =============================================================================
//
// Each micro-fixture lives in `tests/fixtures/` and isolates one
// Decision sub-section (basic table, css, colspan/rowspan, image
// sizing, outside-cell inline, border-styles). The Reviewer can
// `open target/<fixture>.pdf` to inspect each sub-section without
// scrolling through the comprehensive `html_extensions.md` PDF.
//
// We invoke the binary directly (not the library `emit_for` helper)
// because these fixtures need real path resolution for `<img src=...>`
// and the full Typst → PDF pipeline; they are integration smoke
// tests, not Typst-emit assertions. Library-level Typst-emit
// assertions for these paths live in `src/html/*` unit tests.

/// Run the binary on `tests/fixtures/<name>` (in place — relative
/// `<img src>` paths resolve against the project tree), assert exit
/// 0, assert a non-trivial PDF was written, and copy the PDF to
/// `target/<name-stem>.pdf` for human inspection.
fn assert_fixture_round_trips(fixture_name: &str) {
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let md_path = manifest.join("tests").join("fixtures").join(fixture_name);
    assert!(
        md_path.exists(),
        "fixture {} does not exist",
        md_path.display()
    );

    let dir = TempDir::new().unwrap();
    let stem = std::path::Path::new(fixture_name)
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let pdf_out = dir.path().join(format!("{stem}.pdf"));

    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd
        .arg(&md_path)
        .arg("--out")
        .arg(&pdf_out)
        .output()
        .expect("spawn md2pdf");

    assert!(
        output.status.success(),
        "{fixture_name} exited non-zero: code={:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(
        pdf_out.exists(),
        "{fixture_name}: expected {} to exist",
        pdf_out.display()
    );
    let bytes = fs::read(&pdf_out).unwrap();
    assert!(
        bytes.starts_with(b"%PDF"),
        "{fixture_name}: output does not start with PDF magic; first 8 bytes = {:?}",
        bytes.iter().take(8).collect::<Vec<_>>()
    );
    assert!(
        bytes.len() > 1500,
        "{fixture_name}: PDF suspiciously small: {} bytes",
        bytes.len()
    );

    // Drop a copy under target/ for human inspection.
    let inspect = manifest.join("target").join(format!("{stem}.pdf"));
    let _ = fs::write(&inspect, &bytes);
}

#[test]
fn fixture_html_table_basic_round_trips() {
    // Decision §1d–§2g, §7b — minimal positive case for the table
    // recognition path: thead + tbody, no styling.
    assert_fixture_round_trips("html_table_basic.md");
}

#[test]
fn fixture_html_table_css_basic_round_trips() {
    // Decision §3, §7b — exercises the CSS-property mini-parser:
    // padding (1- and 4-value shorthand), text-align, vertical-align,
    // background-color (named + hex), border tri-token,
    // border-collapse, width.
    assert_fixture_round_trips("html_table_css_basic.md");
}

#[test]
fn fixture_html_table_colspan_rowspan_round_trips() {
    // Decision §2c, §7b — both colspan and rowspan on the same
    // table; emitter must produce valid `table.cell(colspan: ...,
    // rowspan: ..., ...)` invocations.
    assert_fixture_round_trips("html_table_colspan_rowspan.md");
}

#[test]
fn fixture_html_image_sizing_basic_round_trips() {
    // Decision §4 — `<img>` with and without style hints. Without
    // hints: routed through `Pipeline::resolve` (intrinsic-px,
    // identical to Markdown). With hints: routed through
    // `Pipeline::fetch_for_html` and emitted with CSS-derived
    // width/height.
    assert_fixture_round_trips("html_image_sizing_basic.md");
}

#[test]
fn fixture_html_inline_outside_cells_round_trips() {
    // Decision §2h v2, §6k v2 — recognized inline formatters
    // (`<b>`, `<strong>`, `<i>`, `<em>`, `<br>`) in paragraphs,
    // headings, list items, blockquotes; nested formatting; drain
    // hook on paragraph end; mismatched close fall-through to
    // literal `md_inline_html(...)`.
    assert_fixture_round_trips("html_inline_outside_cells.md");
}

#[test]
fn fixture_html_border_styles_round_trips() {
    // Decision §3c v2 — solid/dashed/dotted/none borders side-by-
    // side, plus `double` degrade-to-solid. Visual distinctness is
    // verified by the library-level
    // `fixture_border_styles_emit_distinct_stroke_shapes` test
    // above; this test confirms the strokes survive into a real PDF.
    assert_fixture_round_trips("html_border_styles.md");
}

// =============================================================================
// README.md HTML-table round-trip (W-650a51 done_definition #17)
// =============================================================================
//
// README.md lines 220-238 hold the canonical HTML-table fixture —
// a single-row, three-column layout with side-by-side images. Done-
// definition item 17 calls this out as the "canonical positive
// HTML-table fixture (round-trip verification target)". The
// pagebreak round-trip (`tests/pagebreak_e2e.rs`) verifies line 211;
// this verifies the table block.

const README: &str = include_str!("../README.md");

#[test]
fn readme_html_table_emits_md_html_table_invocation() {
    // Library-level: README's HTML table must produce at least one
    // `#md_html_table(...)` invocation in the emitted Typst source.
    let (out, _) = emit_for(README);
    let count = count_html_tables(&out);
    assert!(
        count >= 1,
        "expected README.md to emit at least one #md_html_table(...); got {count}\n\
         (line 220-238 in the source markdown is the canonical HTML-table fixture)"
    );
}

#[test]
fn readme_html_table_round_trips_to_pdf() {
    // Binary-level: the full README must compile to PDF cleanly.
    // The HTML-table block at lines 220-238 sits inside this round-
    // trip; if it broke, the binary would emit invalid Typst and
    // exit non-zero. The README is invoked in place so its relative
    // image paths (`assets/images/rolled-paper.png`) resolve.
    let manifest = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let md_path = manifest.join("README.md");
    assert!(md_path.exists(), "README.md missing at project root");

    let dir = TempDir::new().unwrap();
    let pdf_out = dir.path().join("README.pdf");

    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd
        .arg(&md_path)
        .arg("--out")
        .arg(&pdf_out)
        .output()
        .expect("spawn md2pdf");

    assert!(
        output.status.success(),
        "md2pdf README.md exited non-zero: code={:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    assert!(pdf_out.exists(), "expected {} to exist", pdf_out.display());
    let bytes = fs::read(&pdf_out).unwrap();
    assert!(bytes.starts_with(b"%PDF"), "README PDF lacks PDF magic");
    assert!(
        bytes.len() > 10_000,
        "README PDF suspiciously small: {} bytes",
        bytes.len()
    );

    // Drop a copy for human inspection.
    let inspect = manifest.join("target").join("README_round_trip.pdf");
    let _ = fs::write(&inspect, &bytes);
}
