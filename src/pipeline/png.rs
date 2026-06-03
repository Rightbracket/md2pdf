//! PNG output for md2pdf.
//!
//! Implements PNG rasterisation per **Decision D-30e622** (PNG via
//! in-engine `typst-render` at the `PagedDocument` fork) and the
//! Client-authored Understandings:
//!
//! - **U-b2bf02** — `--format pdf|png` flag and the strict-extension
//!   policy on `--out` (resolved upstream in `src/cli.rs`; this module
//!   takes a stem and a [`PagedDocument`]).
//! - **U-915ef2** — multi-page filename convention: every page is
//!   numbered (even single-page documents); zero-padding width is
//!   *dynamic*, derived from `floor(log10(N)) + 1` where `N` is the
//!   total page count.
//!
//! ## Pipeline placement
//!
//! Per D-30e622 §3 the PNG dispatch lives **after** the strict-mode
//! warning gate in `src/pipeline.rs`. By the time `render_pages` is
//! called, all four warning buckets (Image / Mermaid / Emitter /
//! TypstCompile per U-d302a0) have already had their chance to fire and
//! the strict gate has already returned `StrictEscalation` if needed
//! (preserving U-6173fb's "format-agnostic strict" guarantee).
//!
//! ## Resolution
//!
//! [`PIXELS_PER_PT`] is locked at `2.0` (= 144 DPI, matching the Typst
//! CLI default) per D-30e622 §1. A future Decision may surface a
//! `--dpi` flag if Client demand emerges; that is explicitly out of
//! scope for this Work (W-723d6e).
//!
//! ## Error model
//!
//! - PNG **write** failures map to [`Md2PdfError::PngWrite`] →
//!   `ExitCode::PdfWrite` (= 4); the binding exit-code table U-64e9ec
//!   is preserved (no new numeric code is introduced).
//! - PNG **encode** failures (which should be unreachable given a
//!   well-formed `Pixmap`) map to [`Md2PdfError::Internal`] →
//!   `ExitCode::Internal` (= 70).

use std::path::{Path, PathBuf};

use crate::error::{Md2PdfError, Result};

/// Pixels per Typst point used for PNG rasterisation.
///
/// `2.0` corresponds to 144 DPI (since 1 pt = 1/72 in, so 2 px/pt = 144
/// px/in). This matches the Typst CLI's default raster export and is
/// locked here per Decision D-30e622 §1. Configurable resolution is
/// out of scope for this Work and any future change must come via a
/// new Decision (D-30e622 §"Conditions that would invalidate", item 3).
pub const PIXELS_PER_PT: f32 = 2.0;

/// Render every page of `document` to a separate PNG file and write it
/// to disk. Filenames follow U-915ef2: `<stem>-<NN>.png` with a
/// dynamically padded page number (width = `floor(log10(total)) + 1`).
///
/// `stem` is the U-915ef2 *stem* — the path-without-`.png` from which
/// per-page filenames are composed. The CLI layer
/// (`compose_output_target` in `src/cli.rs`) is responsible for
/// producing it; this function never re-parses `--out`.
///
/// Always one-indexed. Always page-numbered (even for a single-page
/// document, per U-915ef2 verbatim).
pub(crate) fn render_pages(
    document: &typst::layout::PagedDocument,
    stem: &Path,
) -> Result<()> {
    let total = document.pages.len();
    if total == 0 {
        // A `PagedDocument` with zero pages is a Typst defect rather
        // than a user error — surface it as Internal per Decision §7.
        return Err(Md2PdfError::Internal(
            "typst produced a PagedDocument with zero pages".into(),
        ));
    }

    for (idx, page) in document.pages.iter().enumerate() {
        let pixmap = typst_render::render(page, PIXELS_PER_PT);
        let bytes = encode_pixmap_to_png(&pixmap)?;
        let path = compose_page_filename(stem, idx + 1, total);
        std::fs::write(&path, bytes).map_err(|source| Md2PdfError::PngWrite {
            path,
            source,
        })?;
    }
    Ok(())
}

/// Encode a `tiny_skia::Pixmap` to PNG bytes via tiny-skia's built-in
/// encoder (transitively pulled in by `typst-render`'s `tiny-skia` dep
/// per Decision §1).
fn encode_pixmap_to_png(pixmap: &tiny_skia::Pixmap) -> Result<Vec<u8>> {
    pixmap
        .encode_png()
        .map_err(|e| Md2PdfError::Internal(format!("PNG encode failed: {e}")))
}

/// Padding width for page numbers given the total page count, per
/// U-915ef2: `floor(log10(N)) + 1`.
///
/// | N         | width |
/// |-----------|-------|
/// | 1..=9     | 1     |
/// | 10..=99   | 2     |
/// | 100..=999 | 3     |
/// | …         | …     |
fn padding_width(total_pages: usize) -> usize {
    debug_assert!(total_pages >= 1, "padding_width requires N >= 1");
    let mut n = total_pages;
    let mut w = 0;
    while n > 0 {
        w += 1;
        n /= 10;
    }
    w
}

/// Compose the per-page output path: `<stem>-<NN>.png` where `NN` is
/// the one-based page number padded to the dynamic width derived from
/// the total page count (per U-915ef2 verbatim).
///
/// The parent directory of `stem` is preserved; the page number and
/// `.png` extension are appended to the stem's *final filename
/// component*, not as a new path segment.
fn compose_page_filename(stem: &Path, page_one_based: usize, total: usize) -> PathBuf {
    debug_assert!(page_one_based >= 1);
    debug_assert!(page_one_based <= total);
    let w = padding_width(total);
    let stem_name = stem
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let leaf = format!("{stem_name}-{:0width$}.png", page_one_based, width = w);
    match stem.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(leaf),
        _ => PathBuf::from(leaf),
    }
}

/// Test/integration helper: compile a Typst source string directly to
/// PNGs at the given stem.
///
/// Bypasses the markdown emitter so integration tests can construct
/// multi-page documents with explicit `#pagebreak()` calls to exercise
/// the U-915ef2 dynamic-padding boundary (1 vs 2 vs 3 digits).
/// Returns the number of pages written.
///
/// This helper is `pub` (with `#[doc(hidden)]`) precisely so
/// `tests/png_smoke.rs` can call it; it is not part of the binary's
/// CLI contract and may be removed if a better mechanism is found.
#[doc(hidden)]
pub fn render_typst_source_to_pngs(source: String, stem: &Path) -> Result<usize> {
    use crate::pipeline::world::ScaffoldWorld;
    use typst::layout::PagedDocument;

    let world = ScaffoldWorld::new(source);
    let compiled = typst::compile::<PagedDocument>(&world);
    let document = compiled.output.map_err(|errors| {
        let msg = errors
            .iter()
            .map(|d| d.message.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        Md2PdfError::TypstCompile(if msg.is_empty() {
            "(no diagnostics)".into()
        } else {
            msg
        })
    })?;
    let total = document.pages.len();
    render_pages(&document, stem)?;
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_width_table() {
        // U-915ef2 boundary table.
        let cases: &[(usize, usize)] = &[
            (1, 1),
            (9, 1),
            (10, 2),
            (99, 2),
            (100, 3),
            (999, 3),
            (1000, 4),
            (9999, 4),
            (10000, 5),
        ];
        for (n, expected) in cases {
            assert_eq!(
                padding_width(*n),
                *expected,
                "padding_width({n}) expected {expected}"
            );
        }
    }

    #[test]
    fn compose_page_filename_one_page_no_padding() {
        // U-915ef2 verbatim: 1-page doc → `foo-1.png`.
        let got = compose_page_filename(Path::new("foo"), 1, 1);
        assert_eq!(got, PathBuf::from("foo-1.png"));
    }

    #[test]
    fn compose_page_filename_nine_pages_width_one() {
        // U-915ef2 verbatim: 9-page doc → `foo-1.png` … `foo-9.png` (width 1).
        for page in 1..=9 {
            let got = compose_page_filename(Path::new("foo"), page, 9);
            assert_eq!(got, PathBuf::from(format!("foo-{page}.png")));
        }
    }

    #[test]
    fn compose_page_filename_ten_pages_width_two() {
        // U-915ef2 verbatim: 10-page doc → `foo-01.png` … `foo-10.png` (width 2).
        let got = compose_page_filename(Path::new("foo"), 1, 10);
        assert_eq!(got, PathBuf::from("foo-01.png"));
        let got = compose_page_filename(Path::new("foo"), 9, 10);
        assert_eq!(got, PathBuf::from("foo-09.png"));
        let got = compose_page_filename(Path::new("foo"), 10, 10);
        assert_eq!(got, PathBuf::from("foo-10.png"));
    }

    #[test]
    fn compose_page_filename_hundred_pages_width_three() {
        // U-915ef2 verbatim: 100-page doc → `foo-001.png` … `foo-100.png`.
        assert_eq!(
            compose_page_filename(Path::new("foo"), 1, 100),
            PathBuf::from("foo-001.png")
        );
        assert_eq!(
            compose_page_filename(Path::new("foo"), 42, 100),
            PathBuf::from("foo-042.png")
        );
        assert_eq!(
            compose_page_filename(Path::new("foo"), 100, 100),
            PathBuf::from("foo-100.png")
        );
    }

    #[test]
    fn compose_page_filename_with_parent_dir() {
        let got = compose_page_filename(Path::new("tmp/foo"), 1, 10);
        assert_eq!(got, PathBuf::from("tmp/foo-01.png"));
        let got = compose_page_filename(Path::new("/abs/dir/notes"), 7, 10);
        assert_eq!(got, PathBuf::from("/abs/dir/notes-07.png"));
    }

    #[test]
    fn compose_page_filename_stem_with_dots_u_b2bf02() {
        // U-b2bf02 verbatim worked example: `--format png --out bar.pdf`
        // resolves the stem to `bar.pdf`, and a 1-page doc lands at
        // `bar.pdf-1.png` (the `.pdf` is part of the literal stem; the
        // `.png` is appended after the page-number suffix).
        let got = compose_page_filename(Path::new("bar.pdf"), 1, 1);
        assert_eq!(got, PathBuf::from("bar.pdf-1.png"));
    }

    #[test]
    fn page_numbers_are_one_indexed() {
        // U-915ef2 verbatim: page numbers start at 1, never 0.
        let got = compose_page_filename(Path::new("foo"), 1, 1);
        assert!(got.file_name().unwrap().to_string_lossy().contains("-1.png"));
    }

    #[test]
    fn separator_is_hyphen_not_underscore_or_dot() {
        // U-915ef2 verbatim: separator is `-` (hyphen).
        let got = compose_page_filename(Path::new("foo"), 1, 1);
        let s = got.file_name().unwrap().to_string_lossy().into_owned();
        assert!(s.contains("foo-1.png"));
        assert!(!s.contains("foo_1"));
        assert!(!s.contains("foo.1.png"));
    }
}
