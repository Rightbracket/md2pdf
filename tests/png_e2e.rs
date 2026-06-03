//! End-to-end PNG output tests for `--format png`.
//!
//! Covers the binding contracts from the Client-authored Understandings
//! and the Architect Decision that drives this Work:
//!
//! - **U-b2bf02** — `--format` flag (defaults `pdf`); strict-extension
//!   policy on `--out`; strict-mode behavior format-agnostic.
//! - **U-915ef2** — multi-page filename convention; every page numbered;
//!   dynamic zero-pad width = `floor(log10(N)) + 1`.
//! - **D-30e622** — PNG via in-engine `typst-render` at the
//!   PagedDocument fork (post strict-gate).
//!
//! Multi-page documents are produced via the doc-hidden helper
//! `md2pdf::pipeline::png::render_typst_source_to_pngs` so the test
//! binary can build a deterministic N-page document with explicit
//! `#pagebreak()` directives — exercising the 1-page / 9-page /
//! 10-page boundary the U-915ef2 dynamic-pad rule turns on.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

const SIMPLE_DOC: &str = "# Hello\n\nbody text\n";

fn write_input(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let p = dir.join("doc.md");
    fs::write(&p, body).unwrap();
    p
}

fn assert_is_png(path: &std::path::Path) {
    let bytes = fs::read(path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()));
    assert!(
        bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "{} is missing PNG magic; first 16 bytes = {:?}",
        path.display(),
        &bytes.iter().take(16).collect::<Vec<_>>()
    );
    assert!(
        bytes.len() > 100,
        "{} is suspiciously small: {} bytes",
        path.display(),
        bytes.len()
    );
}

// =====================================================================
// CLI surface — `--format png` end-to-end via the binary.
// =====================================================================

#[test]
fn format_png_default_path_writes_one_indexed_png_next_to_input() {
    // U-915ef2 verbatim: a 1-page doc → single file `<stem>-1.png`.
    // U-b2bf02: no `--out` + `--format png` + `notes.md` → stem
    // `notes`, page filename `notes-1.png`.
    let dir = TempDir::new().unwrap();
    let md = dir.path().join("notes.md");
    fs::write(&md, SIMPLE_DOC).unwrap();

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--format")
        .arg("png")
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    let stderr = String::from_utf8_lossy(&cmd.stderr);
    assert_eq!(cmd.status.code(), Some(0), "stderr:\n{stderr}");

    let png = dir.path().join("notes-1.png");
    assert!(png.exists(), "expected {png:?} to exist");
    assert_is_png(&png);

    // No PDF should be produced when --format png is selected.
    assert!(!dir.path().join("notes.pdf").exists());
    assert!(!dir.path().join("notes.png").exists());
}

#[test]
fn format_png_with_out_flag_extension_match_strips_to_stem() {
    // U-b2bf02 verbatim: `--format png --out bar.png` → stem `bar`,
    // 1-page doc → `bar-1.png`.
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);
    let out_path = dir.path().join("bar.png");

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--format")
        .arg("png")
        .arg("--out")
        .arg(&out_path)
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    assert_eq!(
        cmd.status.code(),
        Some(0),
        "stderr:\n{}",
        String::from_utf8_lossy(&cmd.stderr)
    );

    let expected = dir.path().join("bar-1.png");
    assert!(expected.exists(), "expected {expected:?}");
    assert_is_png(&expected);
}

#[test]
fn format_png_with_out_flag_extension_mismatch_keeps_stem_with_pdf_suffix() {
    // U-b2bf02 verbatim: `--format png --out bar.pdf` → stem `bar.pdf`,
    // 1-page doc → `bar.pdf-1.png`.
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);
    let out_path = dir.path().join("bar.pdf");

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--format")
        .arg("png")
        .arg("--out")
        .arg(&out_path)
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    assert_eq!(
        cmd.status.code(),
        Some(0),
        "stderr:\n{}",
        String::from_utf8_lossy(&cmd.stderr)
    );

    let expected = dir.path().join("bar.pdf-1.png");
    assert!(expected.exists(), "expected {expected:?}");
    assert_is_png(&expected);
    // The literal `bar.pdf` was treated as a stem, not a write path —
    // it must not exist as a file.
    assert!(
        !out_path.exists(),
        "U-b2bf02: --format png --out bar.pdf should NOT write a file at bar.pdf; the path is the stem"
    );
}

#[test]
fn format_pdf_with_out_png_extension_appends_pdf() {
    // U-b2bf02 verbatim: `--out foo.png` + default `--format pdf` →
    // `foo.png.pdf`.
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);
    let out_path = dir.path().join("foo.png");

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--out")
        .arg(&out_path)
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    assert_eq!(cmd.status.code(), Some(0));

    let appended = dir.path().join("foo.png.pdf");
    assert!(appended.exists(), "expected {appended:?}");
    let bytes = fs::read(&appended).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    // The literal `foo.png` must NOT exist (we never write PDF bytes
    // there under U-b2bf02).
    assert!(!out_path.exists());
}

// =====================================================================
// Multi-page dynamic-padding boundary (U-915ef2).
// =====================================================================
//
// We exercise the boundary via the doc-hidden helper because driving
// the binary with a markdown document that produces *exactly* N pages
// (for N=9 and N=10) is fragile to font-metric drift. The helper
// accepts a Typst source string, so we can use `#pagebreak()` to fix
// page count deterministically.

fn n_page_typst_source(n: usize) -> String {
    // 1-pt margins so each page is content-light but valid.
    let mut s = String::from("#set page(width: 10cm, height: 10cm, margin: 1cm)\n");
    for i in 1..=n {
        if i > 1 {
            s.push_str("#pagebreak()\n");
        }
        s.push_str(&format!("Page {i}\n"));
    }
    s
}

#[test]
fn multi_page_one_page_no_padding() {
    // U-915ef2 verbatim: 1-page doc → `<stem>-1.png` (width=1).
    let dir = TempDir::new().unwrap();
    let stem = dir.path().join("doc");
    let pages = md2pdf::pipeline::png::render_typst_source_to_pngs(
        n_page_typst_source(1),
        &stem,
    )
    .expect("render");
    assert_eq!(pages, 1);
    assert!(dir.path().join("doc-1.png").exists());
    assert_is_png(&dir.path().join("doc-1.png"));
    // Width=1, not zero-padded.
    assert!(!dir.path().join("doc-01.png").exists());
}

#[test]
fn multi_page_nine_pages_width_one() {
    // U-915ef2 verbatim: 9-page doc → `<stem>-1.png` … `<stem>-9.png`
    // (no zero padding, width=1).
    let dir = TempDir::new().unwrap();
    let stem = dir.path().join("doc");
    let pages = md2pdf::pipeline::png::render_typst_source_to_pngs(
        n_page_typst_source(9),
        &stem,
    )
    .expect("render");
    assert_eq!(pages, 9);
    for i in 1..=9 {
        let p = dir.path().join(format!("doc-{i}.png"));
        assert!(p.exists(), "expected {p:?}");
        assert_is_png(&p);
    }
    // 10th page must NOT exist; zero-padded form must NOT exist.
    assert!(!dir.path().join("doc-10.png").exists());
    assert!(!dir.path().join("doc-01.png").exists());
}

#[test]
fn multi_page_ten_pages_width_two() {
    // U-915ef2 verbatim: 10-page doc → `<stem>-01.png` … `<stem>-10.png`
    // (zero-padded to width=2 — the dynamic-pad transition point).
    let dir = TempDir::new().unwrap();
    let stem = dir.path().join("doc");
    let pages = md2pdf::pipeline::png::render_typst_source_to_pngs(
        n_page_typst_source(10),
        &stem,
    )
    .expect("render");
    assert_eq!(pages, 10);
    for i in 1..=10 {
        let p = dir.path().join(format!("doc-{i:02}.png"));
        assert!(p.exists(), "expected {p:?}");
        assert_is_png(&p);
    }
    // Unpadded form for low page numbers must NOT exist.
    assert!(!dir.path().join("doc-1.png").exists());
    assert!(!dir.path().join("doc-9.png").exists());
}

// =====================================================================
// Strict-mode parity (U-b2bf02): same warning gate semantics across
// formats. Use a missing-image fixture which fires WarningSource::Image
// — the gate in `pipeline::render` is format-agnostic, so PNG and PDF
// must exit identically.
// =====================================================================

const STRICT_DOC_WITH_MISSING_IMAGE: &str =
    "# Hi\n\n![missing](./this-file-does-not-exist.png)\n";

#[test]
fn strict_png_escalates_identically_to_strict_pdf() {
    // PDF run.
    let dir1 = TempDir::new().unwrap();
    let md1 = write_input(dir1.path(), STRICT_DOC_WITH_MISSING_IMAGE);
    let pdf_run = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--strict")
        .arg(&md1)
        .output()
        .expect("spawn md2pdf pdf");

    // PNG run.
    let dir2 = TempDir::new().unwrap();
    let md2 = write_input(dir2.path(), STRICT_DOC_WITH_MISSING_IMAGE);
    let png_run = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--strict")
        .arg("--format")
        .arg("png")
        .arg(&md2)
        .output()
        .expect("spawn md2pdf png");

    assert_eq!(
        pdf_run.status.code(),
        Some(6),
        "pdf strict expected exit 6, stderr:\n{}",
        String::from_utf8_lossy(&pdf_run.stderr)
    );
    assert_eq!(
        png_run.status.code(),
        Some(6),
        "png strict expected exit 6 (parity with pdf), stderr:\n{}",
        String::from_utf8_lossy(&png_run.stderr)
    );

    // No output files should have been written for either format.
    assert!(!dir1.path().join("doc.pdf").exists());
    assert!(!dir2.path().join("doc-1.png").exists());

    // Both runs should mention the canonical strict summary line.
    let pdf_err = String::from_utf8_lossy(&pdf_run.stderr);
    let png_err = String::from_utf8_lossy(&png_run.stderr);
    assert!(
        pdf_err.contains("--strict was set") && pdf_err.contains("escalated to errors"),
        "pdf strict summary missing: {pdf_err}"
    );
    assert!(
        png_err.contains("--strict was set") && png_err.contains("escalated to errors"),
        "png strict summary missing: {png_err}"
    );
}

#[test]
fn non_strict_png_warns_continues_writes_output() {
    // Same fixture as the strict test, but without `--strict`: warning
    // is logged, exit is 0, the PNG is still written.
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), STRICT_DOC_WITH_MISSING_IMAGE);
    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--format")
        .arg("png")
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    assert_eq!(
        cmd.status.code(),
        Some(0),
        "non-strict PNG expected exit 0, stderr:\n{}",
        String::from_utf8_lossy(&cmd.stderr)
    );
    assert!(dir.path().join("doc-1.png").exists());
    let stderr = String::from_utf8_lossy(&cmd.stderr);
    assert!(
        stderr.contains("warn:"),
        "expected at least one warn: line, got: {stderr}"
    );
}
