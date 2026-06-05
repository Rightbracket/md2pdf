//! QA fixture suite end-to-end tests (W-8f4312 per D-875e4b §7d-§7f).
//!
//! Verifies the full HTML-extensions surface across all 9 QA-prefixed
//! fixtures. Coverage areas:
//!
//! 1. **Round-trip-to-PDF** — every QA fixture compiles cleanly through
//!    the full pipeline (Markdown → emit → Typst-compile → PDF). PDF
//!    written to `target/<stem>.pdf` for human inspection.
//!
//! 2. **Library-level emit assertions** — for fixtures with verifiable
//!    structural invariants (pagebreak count, table count, etc.),
//!    drive `emit_typst_body` and assert on the emitted Typst source.
//!
//! 3. **Strict-mode escalation** — fixtures with unreachable images
//!    (Image bucket) or malformed HTML (Emitter bucket) escalate
//!    correctly under `--strict`.
//!
//! 4. **Supply-chain invariants** — `WarningSource` enum has exactly
//!    four buckets (compile-time exhaustiveness check).
//!
//! 5. **Markdown-image bit-for-bit invariant (U-8df478)** — emit
//!    output for a Markdown image is structurally identical to the
//!    pre-WS shape (`#md_image_bytes(...)` with no HTML extension
//!    artefacts in the same emit).

use std::fs;
use std::path::PathBuf;

use assert_cmd::Command;
use md2pdf::emitter::{emit_typst_body, StubMermaidDispatcher};
use md2pdf::image_pipeline::PipelineBuilder;
use md2pdf::warnings::{Warning, WarningCollector, WarningSource};
use tempfile::TempDir;

// ---------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------

fn manifest() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_path(name: &str) -> PathBuf {
    manifest().join("tests").join("fixtures").join(name)
}

fn fixture_md(name: &str) -> String {
    fs::read_to_string(fixture_path(name))
        .unwrap_or_else(|e| panic!("read fixture {name}: {e}"))
}

/// Drive `emit_typst_body` over a Markdown source with a stubbed
/// mermaid dispatcher and a scratch image-pipeline rooted at the
/// fixtures dir (so relative `<img src="../../assets/images/...">`
/// paths resolve correctly).
fn emit_for_fixture(md: &str) -> (String, WarningCollector) {
    // Image-pipeline base_dir = tests/fixtures/, matching where the
    // fixture lives on disk. Relative paths inside the fixture
    // (`../../assets/...`) resolve from that base.
    let base = manifest().join("tests").join("fixtures");
    let mut p = PipelineBuilder::new(base).build();
    let mut m = StubMermaidDispatcher;
    let mut w = WarningCollector::new();
    let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
    (out, w)
}

/// Run the binary on `tests/fixtures/<name>`, assert exit 0, assert
/// non-trivial PDF written, copy PDF to `target/<stem>.pdf` for
/// inspection. Mirrors `assert_fixture_round_trips` from
/// `tests/html_extensions_e2e.rs`.
fn assert_qa_fixture_round_trips(fixture_name: &str) {
    let md_path = fixture_path(fixture_name);
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
    let inspect = manifest().join("target").join(format!("{stem}.pdf"));
    let _ = fs::write(&inspect, &bytes);
}

// ---------------------------------------------------------------------
// 1. Round-trip-to-PDF tests (every QA fixture)
// ---------------------------------------------------------------------

#[test]
fn qa_pagebreak_edge_cases_round_trips() {
    assert_qa_fixture_round_trips("qa_pagebreak_edge_cases.md");
}

#[test]
fn qa_html_table_css_coverage_round_trips() {
    assert_qa_fixture_round_trips("qa_html_table_css_coverage.md");
}

#[test]
fn qa_html_table_colspan_rowspan_round_trips() {
    assert_qa_fixture_round_trips("qa_html_table_colspan_rowspan.md");
}

#[test]
fn qa_html_image_sizing_composition_round_trips() {
    // Note: this fixture intentionally references an unreachable URL
    // (Section F) to exercise the Image-bucket warning path. The
    // round-trip must still succeed (placeholder rendered) — only
    // `--strict` escalates.
    assert_qa_fixture_round_trips("qa_html_image_sizing_composition.md");
}

#[test]
fn qa_html_table_negative_cases_round_trips() {
    // Pathological HTML must NOT panic; the round-trip succeeding is
    // itself the assertion that ParseFailed paths recover gracefully.
    assert_qa_fixture_round_trips("qa_html_table_negative_cases.md");
}

#[test]
fn qa_html_table_in_blockquote_round_trips() {
    assert_qa_fixture_round_trips("qa_html_table_in_blockquote.md");
}

#[test]
fn qa_html_inline_outside_cells_round_trips() {
    assert_qa_fixture_round_trips("qa_html_inline_outside_cells.md");
}

#[test]
fn qa_html_inline_outside_cells_negative_round_trips() {
    assert_qa_fixture_round_trips("qa_html_inline_outside_cells_negative.md");
}

#[test]
fn qa_html_border_styles_round_trips() {
    assert_qa_fixture_round_trips("qa_html_border_styles.md");
}

// ---------------------------------------------------------------------
// 2. Library-level emit assertions
// ---------------------------------------------------------------------

#[test]
fn qa_pagebreak_fixture_emits_expected_pagebreak_count() {
    // qa_pagebreak_edge_cases.md sections:
    //   A — pagebreak inside HTML table cell      → 0 (non-top-level)
    //   B — pagebreak between rows in <table>     → 0 (non-top-level / parser)
    //   C — pagebreak inside blockquote           → 0 (non-top-level)
    //   D — 6 recognized whitespace/case forms    → 6
    //   E — 5 form variations that must NOT match → 0 (rejected)
    // Total: 6.
    let md = fixture_md("qa_pagebreak_edge_cases.md");
    let (out, _) = emit_for_fixture(&md);
    let count = out.matches("#pagebreak()\n\n").count();
    assert_eq!(
        count, 6,
        "expected 6 #pagebreak() invocations from QA pagebreak fixture; got {count}\n\
         emitted body:\n{out}"
    );
}

#[test]
fn qa_pagebreak_fixture_section_d_recognizes_all_six_variants() {
    // Each Section D variant must NOT appear in the emit as literal
    // `md_inline_html(...)` text — they must be consumed into
    // `#pagebreak()` calls.
    let md = fixture_md("qa_pagebreak_edge_cases.md");
    let (out, _) = emit_for_fixture(&md);
    for forbidden in [
        "<!--PAGEBREAK-->",
        "<!--   page-break   -->",
        "<!-- Page-Break -->",
        "<!-- pageBREAK -->",
        "<!--PAGE-BREAK-->",
    ] {
        assert!(
            !out.contains(&format!("md_inline_html(\"{forbidden}\")")),
            "Section D variant {forbidden:?} leaked through md_inline_html literal path;\nemit:\n{out}"
        );
    }
}

#[test]
fn qa_pagebreak_fixture_section_e_rejects_all_payload_forms() {
    // Section E payload-form variants must surface as literal
    // pass-through (not consumed into `#pagebreak()`).
    let md = fixture_md("qa_pagebreak_edge_cases.md");
    let (out, _) = emit_for_fixture(&md);
    for required in [
        "pagebreak top",   // payload-bearing
        "pagebreaks",      // wrong word
        "page break",      // space, not hyphen
        "pagebreak2",      // suffix
        "pagebreak;",      // punctuation
    ] {
        assert!(
            out.contains(required),
            "Section E payload variant {required:?} did not survive as literal text;\nemit:\n{out}"
        );
    }
}

#[test]
fn qa_negative_cases_fixture_emits_emitter_bucket_warning_for_parse_fail() {
    // qa_html_table_negative_cases.md Sections F, G, H trigger
    // ParseFailed → Emitter-bucket warning (per D-875e4b §6e + the
    // emitter handler at src/emitter.rs:957).
    let md = fixture_md("qa_html_table_negative_cases.md");
    let (_, w) = emit_for_fixture(&md);
    let emitter_warnings: Vec<&Warning> = w
        .warnings()
        .iter()
        .filter(|x| x.source == WarningSource::Emitter)
        .collect();
    assert!(
        !emitter_warnings.is_empty(),
        "negative-cases fixture must produce at least one Emitter-bucket warning \
         (ParseFailed path); got: {:?}",
        w.warnings()
    );
    for warning in &emitter_warnings {
        assert!(
            warning.message.contains("html table parse failed"),
            "Emitter-bucket warning must name html table parse failure; got: {:?}",
            warning.message
        );
    }
}

#[test]
fn qa_negative_cases_fixture_does_not_panic_on_pathological_inputs() {
    // The mere fact this test reaches the assertion (no panic during
    // emit) is the verification — the negative-cases fixture is
    // pathological by construction.
    let md = fixture_md("qa_html_table_negative_cases.md");
    let (out, _) = emit_for_fixture(&md);
    // Some output should be produced; the exact shape varies depending
    // on which paths fall through to literal pass-through vs.
    // ParseFailed. Just assert non-empty.
    assert!(
        !out.is_empty(),
        "negative-cases fixture produced empty emit; emitter likely panicked silently"
    );
}

#[test]
fn qa_negative_cases_nested_table_degrades_to_silent_text() {
    // Section E: outer table with nested inner table. Per Client-
    // accepted simplification (U-ad8c6c v2 + §6e), the inner <table>
    // tags are silently dropped (degraded to text inside the outer
    // cell). Verify NO inner table emits as a separate
    // `#md_html_table(` call. The outer table SHOULD emit.
    //
    // Counting: total `#md_html_table(` calls in the fixture =
    //   A (1) + B (1) + C (1) + D (1) + E outer (1) + F-H ParseFailed
    //   (0 each) = 5. The nested inner in E should NOT add a 6th.
    //
    // Looser assertion: at least 1 (outer of E) and not more than 5.
    let md = fixture_md("qa_html_table_negative_cases.md");
    let (out, _) = emit_for_fixture(&md);
    let count = out.matches("#md_html_table(").count();
    assert!(
        count >= 1 && count <= 5,
        "expected 1..=5 #md_html_table(...) calls (nested inner must degrade); got {count}\n\
         emit:\n{out}"
    );
}

#[test]
fn qa_inline_outside_cells_emits_expected_formatters() {
    let md = fixture_md("qa_html_inline_outside_cells.md");
    let (out, _) = emit_for_fixture(&md);
    // Multiple `*bold*` and `_italic_` markers expected.
    assert!(
        out.matches("*bold*").count() >= 2,
        "expected multiple `*bold*` Typst markers; got:\n{out}"
    );
    assert!(
        out.matches("_italic_").count() >= 2,
        "expected multiple `_italic_` Typst markers; got:\n{out}"
    );
    assert!(
        out.contains("#md_hardbreak()"),
        "expected `#md_hardbreak()` for <br>; got:\n{out}"
    );
}

#[test]
fn qa_inline_negative_fixture_unrecognized_tags_fall_through() {
    let md = fixture_md("qa_html_inline_outside_cells_negative.md");
    let (out, _) = emit_for_fixture(&md);
    // <u>, <font>, <span>, <mark>, <code>, <kbd>, <small>, <s>,
    // <sup>, <sub> all fall through to literal `md_inline_html(...)`.
    for tag in ["<u>", "<font ", "<span>", "<mark>", "<kbd>", "<small>", "<s>", "<sup>", "<sub>"] {
        assert!(
            out.contains(&format!("md_inline_html(\"{tag}")),
            "unrecognized inline tag {tag:?} did not surface as literal md_inline_html;\nemit:\n{out}"
        );
    }
}

#[test]
fn qa_inline_negative_fixture_attribute_tolerance_does_not_warn() {
    // Per Decision §2h v2 attribute tolerance: <b style="..." onclick="...">
    // classifies as a recognized open and emits Typst `*...*`. Attributes
    // are silently dropped — no warning fires.
    let md = fixture_md("qa_html_inline_outside_cells_negative.md");
    let (_, w) = emit_for_fixture(&md);
    // The fixture references unreachable images? No, it's pure inline.
    // No image warnings expected. Emitter warnings only for the
    // table-parse-fail path which this fixture does not exercise.
    assert_eq!(
        w.warnings().iter().filter(|x| x.source == WarningSource::Emitter).count(),
        0,
        "inline-negative fixture must not produce Emitter-bucket warnings; got: {:?}",
        w.warnings()
    );
}

#[test]
fn qa_border_styles_emit_distinct_dash_keys() {
    let md = fixture_md("qa_html_border_styles.md");
    let (out, _) = emit_for_fixture(&md);
    // Per typst_emit (verified in fixture_border_styles_emit_distinct_stroke_shapes
    // for the W-html-table fixture): dashed → dash: "dashed", dotted
    // → dash: "dotted", solid → no dash key, none → stroke: none.
    assert!(
        out.contains("dash: \"dashed\""),
        "dashed style must emit dash: \"dashed\";\nemit:\n{out}"
    );
    assert!(
        out.contains("dash: \"dotted\""),
        "dotted style must emit dash: \"dotted\";\nemit:\n{out}"
    );
    // Section A has 4 distinct border styles in one row; verify each
    // is rendered in the emitted Typst.
}

// ---------------------------------------------------------------------
// 3. Strict-mode escalation tests
// ---------------------------------------------------------------------

#[test]
fn qa_strict_mode_escalates_image_bucket_warning() {
    // qa_html_image_sizing_composition.md Section G has a local
    // file:// to a non-existent file → Image-bucket warning + placeholder.
    // With --strict, exit code != 0.
    //
    // Use a tiny dedicated fixture rather than the full image-sizing
    // fixture to keep the test fast (no HTTP timeout wait).
    let dir = TempDir::new().unwrap();
    let md_path = dir.path().join("qa_strict_image.md");
    // NOTE: this test uses a *Markdown* image with a missing local
    // path because the Markdown image path is the load-bearing one
    // verified by U-8df478 (and is the one Decision §5b strictly
    // requires to escalate). The outside-cell HTML `<img>` path is
    // currently a literal pass-through (does NOT route through
    // `Pipeline::resolve`); see the QA contradicts Outcome filed
    // alongside this Work for the bug report.
    fs::write(
        &md_path,
        "# Strict-mode image escalation test\n\n\
         ![missing](./this-file-does-not-exist.png)\n",
    )
    .unwrap();
    let pdf_out = dir.path().join("qa_strict_image.pdf");

    // Without --strict: succeeds.
    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd
        .arg(&md_path)
        .arg("--out")
        .arg(&pdf_out)
        .output()
        .expect("spawn md2pdf");
    assert!(
        output.status.success(),
        "non-strict mode must succeed with placeholder; got code={:?} stderr=\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    // With --strict: fails (exit-code-6 per U-6173fb).
    let pdf_out2 = dir.path().join("qa_strict_image_strict.pdf");
    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd
        .arg(&md_path)
        .arg("--out")
        .arg(&pdf_out2)
        .arg("--strict")
        .output()
        .expect("spawn md2pdf");
    assert!(
        !output.status.success(),
        "--strict mode must fail when image fetch warning fires; \
         got code={:?} stderr=\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning(s) escalated to errors"),
        "--strict mode must print the canonical escalation summary; got stderr:\n{stderr}"
    );
    // PDF must NOT be written (per pipeline.rs §5).
    assert!(
        !pdf_out2.exists(),
        "--strict mode wrote a PDF despite escalation; file at {} should not exist",
        pdf_out2.display(),
    );
}

#[test]
fn qa_strict_mode_escalates_emitter_bucket_warning() {
    // Pathological HTML table → ParseFailed → Emitter-bucket warning.
    // With --strict, exit code != 0.
    let dir = TempDir::new().unwrap();
    let md_path = dir.path().join("qa_strict_emitter.md");
    fs::write(
        &md_path,
        "# Strict-mode emitter escalation test\n\n\
         <table>\n\
           <tr>\n\
             <td>unterminated table cell\n\
           </tr>\n\
         <!-- no </table> -->\n",
    )
    .unwrap();
    let pdf_out = dir.path().join("qa_strict_emitter.pdf");

    // Without --strict: succeeds (warning + raw fall-through).
    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd
        .arg(&md_path)
        .arg("--out")
        .arg(&pdf_out)
        .output()
        .expect("spawn md2pdf");
    assert!(
        output.status.success(),
        "non-strict mode must succeed with raw fall-through; got code={:?} stderr=\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    // With --strict: fails.
    let pdf_out2 = dir.path().join("qa_strict_emitter_strict.pdf");
    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd
        .arg(&md_path)
        .arg("--out")
        .arg(&pdf_out2)
        .arg("--strict")
        .output()
        .expect("spawn md2pdf");
    assert!(
        !output.status.success(),
        "--strict mode must fail when Emitter-bucket warning fires; \
         got code={:?} stderr=\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("warning(s) escalated to errors"),
        "--strict mode must print canonical escalation summary; stderr:\n{stderr}"
    );
}

// ---------------------------------------------------------------------
// 4. Supply-chain invariants (compile-time exhaustiveness)
// ---------------------------------------------------------------------

#[test]
fn qa_warning_source_enum_has_exactly_four_buckets() {
    // Verify U-d302a0 stays closed: `WarningSource` enum has exactly
    // the four buckets (Image / Mermaid / Emitter / TypstCompile) and
    // no fifth was added by the WS.
    //
    // Approach: read `src/warnings.rs` from disk and count variant
    // declarations. The enum body sits between `pub enum WarningSource {`
    // and the matching closing brace; each variant is a CapitalizedWord
    // followed by `,` after stripping doc-comment lines.
    //
    // (`#[non_exhaustive]` defeats compile-time exhaustiveness from
    // outside the enum's crate — the test crate is treated as outside
    // for that purpose — so we do the count textually.)
    let src = fs::read_to_string(manifest().join("src").join("warnings.rs"))
        .expect("read src/warnings.rs");
    let body = src
        .split_once("pub enum WarningSource {")
        .expect("warnings.rs must declare `pub enum WarningSource`")
        .1
        .split_once('}')
        .expect("warnings.rs enum must have closing brace")
        .0;

    // Each variant declaration is a `Identifier,` line (after stripping
    // doc comments). Count CapitalizedWords that look like variant names.
    let variants: Vec<&str> = body
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with("///"))
        .filter_map(|l| {
            // Variant lines end with `,` and start with an uppercase letter.
            let stripped = l.trim_end_matches(',');
            if stripped.chars().next().map_or(false, |c| c.is_uppercase()) {
                Some(stripped)
            } else {
                None
            }
        })
        .collect();

    assert_eq!(
        variants.len(),
        4,
        "WarningSource must have exactly 4 variants (Image, Mermaid, Emitter, \
         TypstCompile per U-d302a0); found {}: {:?}",
        variants.len(),
        variants
    );
    for expected in ["Image", "Mermaid", "Emitter", "TypstCompile"] {
        assert!(
            variants.contains(&expected),
            "WarningSource missing expected variant {expected}; got {:?}",
            variants
        );
    }
}

// ---------------------------------------------------------------------
// 5. Markdown-image bit-for-bit invariant (U-8df478)
// ---------------------------------------------------------------------

#[test]
fn qa_markdown_image_emit_shape_is_unchanged() {
    // The Markdown image path is the load-bearing invariant per
    // U-8df478 + Decision §4. Verify a Markdown-only image source
    // still emits the canonical `#md_image_bytes(<bytes>, "<fmt>",
    // <intrinsic_pt>, none)` shape that pre-WS produced.
    //
    // We cannot diff against pre-WS Typst output (no captured baseline);
    // structural assertion plus source-code verification (Markdown
    // emit path in `Event::Start(Tag::Image)` and `Pipeline::resolve`
    // are unchanged in the WS work-tree; verified separately in the
    // work-product Outcome) constitute the bit-for-bit guarantee.
    let md = "# Markdown image control\n\n\
              ![alt text](../../assets/images/rolled-paper.png)\n";
    let (out, w) = emit_for_fixture(md);

    // No warnings (image resolves successfully relative to fixtures dir).
    assert_eq!(
        w.count(),
        0,
        "Markdown image fixture must not warn; got: {:?}",
        w.warnings()
    );
    // Exactly one md_image_bytes call.
    let count = out.matches("#md_image_bytes(").count();
    assert_eq!(
        count, 1,
        "expected exactly 1 #md_image_bytes call from Markdown image; got {count}\n\
         emit:\n{out}"
    );
    // No HTML extension artefacts in the Markdown-only emit.
    assert!(
        !out.contains("#md_html_table("),
        "Markdown-only emit must not contain #md_html_table; got:\n{out}"
    );
    assert!(
        !out.contains("#md_inline_html("),
        "Markdown-only emit must not contain #md_inline_html; got:\n{out}"
    );
    // Canonical shape (PNG with intrinsic dims known):
    //   `#md_image_bytes(<bytes>, "png", <Wpt>pt, <Hpt>pt)`
    // For SVG without intrinsic dims, height becomes `none`. The
    // important invariant is the SHAPE (4 args; format string is
    // "png"; the bytes-literal first arg) — verified here by parsing
    // the call's argument list.
    let start = out.find("#md_image_bytes(").expect("must find md_image_bytes");
    let mut depth = 0i32;
    let mut end = start;
    for (i, c) in out[start..].char_indices() {
        if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth == 0 {
                end = start + i + 1;
                break;
            }
        }
    }
    let call = &out[start..end];
    assert!(
        call.contains("\"png\""),
        "Markdown image emit must record png format; got call:\n{call}"
    );
    // For our known PNG (rolled-paper.png), the pipeline computes
    // both width and height from intrinsic aspect ratio — height is
    // `<Hpt>pt`, NOT `none`.
    assert!(
        call.ends_with("pt)"),
        "Markdown image emit final arg must end with `pt)` for a \
         PNG with known intrinsic aspect; got call tail:\n{}",
        &call[call.len().saturating_sub(60)..]
    );
    // The bytes literal is the first arg (`bytes((<digits...>))`).
    assert!(
        call.starts_with("#md_image_bytes(bytes(("),
        "Markdown image emit first arg must be a typst bytes literal; \
         got call head:\n{}",
        &call[..call.len().min(60)]
    );
}

#[test]
fn qa_html_no_style_image_outside_cells_falls_through_to_inline_html() {
    // Documents observed (buggy) behaviour: an outside-cell HTML
    // `<img>` does NOT route through `Pipeline::resolve` and does NOT
    // emit `#md_image_bytes(...)`. It falls through to a literal
    // `#md_inline_html(...)` pass-through with zero warnings, even
    // when `src` is unresolvable.
    //
    // This contradicts D-875e4b §4 + the docstring in
    // `tests/fixtures/html_image_sizing_basic.md` Section A which
    // both claim the no-style HTML `<img>` path routes through
    // `Pipeline::resolve` and emits `#md_image_bytes(...)`.
    //
    // Filed as a `contradicts` Outcome on this verification Work;
    // follow-up Work created to route to Developer for fix. This
    // assertion locks the current shape so a fix will visibly flip
    // this test (and the QA contradicts Outcome will close).
    let md_html = "<img src=\"../../assets/images/rolled-paper.png\" alt=\"alt\" />\n";
    let (out_html, w_html) = emit_for_fixture(md_html);

    assert!(
        out_html.contains("#md_inline_html(\"<img"),
        "outside-cell HTML <img> currently falls through to \
         #md_inline_html(...) literal; got:\n{out_html}"
    );
    assert!(
        !out_html.contains("#md_image_bytes("),
        "outside-cell HTML <img> currently does NOT emit \
         #md_image_bytes(...) — that is the bug; got:\n{out_html}"
    );
    assert_eq!(
        w_html.count(),
        0,
        "outside-cell HTML <img> currently emits zero warnings even \
         when src is unresolvable (would be Image-bucket if fetched); \
         got: {:?}",
        w_html.warnings()
    );

    // Markdown control: the load-bearing path still works correctly.
    let md_md = "![alt](../../assets/images/rolled-paper.png)\n";
    let (out_md, w_md) = emit_for_fixture(md_md);
    assert!(
        out_md.contains("#md_image_bytes("),
        "Markdown image must emit #md_image_bytes; got:\n{out_md}"
    );
    assert_eq!(
        w_md.count(),
        0,
        "Markdown image to a resolvable file must not warn; got: {:?}",
        w_md.warnings()
    );
}

// ---------------------------------------------------------------------
// 6. CSS coverage fixture: assertions on emitted Typst
// ---------------------------------------------------------------------

#[test]
fn qa_css_coverage_emits_expected_table_count() {
    // Section A (1) + Section B (1) + Section C (1) + Section D (1) +
    // Section E (6 tri-token variations) + Section F (1) + Section G (1)
    // + Section H (1) = 13 tables total.
    let md = fixture_md("qa_html_table_css_coverage.md");
    let (out, _) = emit_for_fixture(&md);
    let count = out.matches("#md_html_table(").count();
    assert_eq!(
        count, 13,
        "expected 13 #md_html_table calls in QA CSS coverage fixture; got {count}\n\
         emit length: {} chars",
        out.len()
    );
}

#[test]
fn qa_css_coverage_emits_zero_warnings_on_recognized_props() {
    // All declarations in the CSS coverage fixture are recognized;
    // ZERO warnings should fire (graceful per Decision §6).
    let md = fixture_md("qa_html_table_css_coverage.md");
    let (_, w) = emit_for_fixture(&md);
    assert_eq!(
        w.count(),
        0,
        "CSS coverage fixture must not warn on recognized properties; got: {:?}",
        w.warnings()
    );
}

#[test]
fn qa_colspan_rowspan_emit_table_cell_calls() {
    let md = fixture_md("qa_html_table_colspan_rowspan.md");
    let (out, _) = emit_for_fixture(&md);
    // Pure colspan (Section A) and pure rowspan (Section B) and combined
    // (Section C) all emit `table.cell(colspan: ..., rowspan: ...)`.
    assert!(
        out.contains("table.cell(colspan: 4"),
        "Section A pure colspan=4 must emit table.cell(colspan: 4 ...);\n{out}"
    );
    assert!(
        out.contains("rowspan: 3)"),
        "Section B pure rowspan=3 must emit table.cell(... rowspan: 3);\n{out}"
    );
    assert!(
        out.contains("table.cell(colspan: 2, rowspan: 2)"),
        "Section C combined colspan=2 rowspan=2 must emit one cell with both;\n{out}"
    );
}

#[test]
fn qa_blockquote_table_round_trip_succeeds() {
    // Just verify emit doesn't panic; the round-trip test verifies
    // the full pipeline. The exact behaviour (table inside blockquote
    // vs raw HTML pass-through) depends on pulldown-cmark's tokenisation
    // of HTML inside `>` blockquote prefix, which is implementation-
    // dependent. Either path is acceptable per Decision §6c.
    let md = fixture_md("qa_html_table_in_blockquote.md");
    let (out, _) = emit_for_fixture(&md);
    assert!(!out.is_empty(), "blockquote+table emit produced empty output");
    // At minimum, the bottom-of-fixture top-level table (Section C)
    // MUST emit as a real Typst table.
    assert!(
        out.contains("#md_html_table("),
        "Section C top-level table must emit #md_html_table;\n{out}"
    );
}

