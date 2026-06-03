//! md2pdf rendering pipeline.
//!
//! Reads the user's Markdown, hands it to `emitter::emit_typst_body`,
//! concatenates the body onto `theme::THEME`, and feeds the combined
//! source to Typst via the hand-rolled `World` for compile + format
//! export. The format dispatch (PDF vs PNG) lives at the
//! `PagedDocument` boundary, *after* the strict-mode warning gate, per
//! Decision **D-30e622** §3.
//!
//! ## Composition order
//!
//! ```text
//!   THEME (set rules + helpers, baked-in)
//! + emitted body (Typst markup with helper invocations)
//! ```
//!
//! ## Strict-mode plumbing
//!
//! `WarningCollector` is constructed here and threaded into both the
//! image pipeline (via the `WarnSink` indirection) and the emitter.
//! After emission and `typst::compile` (which contributes its own
//! `Warned<...>` warnings into the unified collector under
//! `WarningSource::TypstCompile`), if `req.strict` is set and the
//! collector reports any warning, we emit the canonical summary line
//! and return `Md2PdfError::StrictEscalation` *before* writing any
//! output bytes (PDF or PNG). Per U-6173fb (`--strict` semantics) and
//! D-30e622 §3 (gate placement is format-agnostic). All four
//! `WarningSource` buckets per U-d302a0 fire identically across
//! formats.
//!
//! ## Format dispatch
//!
//! After the strict gate, the pipeline forks on `req.format`:
//! - `OutputFormat::Pdf` → `typst_pdf::pdf` + single `fs::write`.
//! - `OutputFormat::Png` → `pipeline::png::render_pages` writes
//!   `<stem>-<NN>.png` files per U-915ef2.
//!
//! Per Decision D-30e622 §"Conditions that would invalidate" no new
//! `WarningSource` bucket is added for PNG-render-time issues; the
//! existing four-bucket taxonomy stays closed for this Work.

pub(crate) mod world;
pub mod png;

use std::path::Path;

use crate::cli::{OutputFormat, OutputTarget};
use crate::emitter::{emit_typst_body, RealMermaidDispatcher};
use crate::error::{Md2PdfError, Result};
use crate::image_pipeline::{Pipeline, PipelineBuilder};
use crate::pipeline::world::ScaffoldWorld;
use crate::theme::THEME;
use crate::warnings::{WarningCollector, WarningSource};

/// Inputs to a single render run.
pub struct RenderRequest<'a> {
    pub input: &'a Path,
    /// Resolved output target — exact PDF write path or PNG stem,
    /// produced upstream by `cli::compose_output_target`. Per Decision
    /// D-30e622 §5b: the pipeline never re-parses `--out`; the CLI
    /// layer is the single source of truth for path resolution.
    pub output: OutputTarget,
    /// Output format selector. Redundant with the `OutputTarget`
    /// variant by construction (the CLI layer composes them
    /// consistently), but kept as a separate field per Decision
    /// D-30e622 §3 so the dispatch in `render` reads as a `match` on
    /// `format`. Defaults to `Pdf` per U-b2bf02.
    pub format: OutputFormat,
    /// Per U-6173fb: when true, any recorded warning escalates to
    /// exit code 6 and no output (PDF or PNG) is written. The gate is
    /// format-agnostic per Decision D-30e622 §3.
    pub strict: bool,
    /// Body font size in points, derived from the `--font-scale` CLI
    /// flag. The three input shapes (multiplier, percentage, absolute)
    /// all collapse to a single value here. Default: 11.0 (canonical
    /// pre-flag body size, multiplier=1.0).
    pub body_size_pt: f64,
}

/// Format a `f64` body-size in pt for Typst preamble injection. Uses
/// Rust's `{}` Display, which produces a finite decimal Typst will
/// accept and elides trailing-zero noise (e.g. 11.0 → "11", 16.5 →
/// "16.5"). Per O-10564c §6.
fn format_body_pt(v: f64) -> String {
    format!("{}", v)
}

/// Compose the Typst source string with the font-scale preamble per
/// O-10564c §6: a single `#let md2pdf_body_size = <N>pt` binding
/// prepended before the THEME and the emitted body.
pub(crate) fn compose_typst_source(body_size_pt: f64, body: &str) -> String {
    format!(
        "#let md2pdf_body_size = {}pt\n{}\n{}\n",
        format_body_pt(body_size_pt),
        THEME,
        body
    )
}

/// Run the render pipeline end-to-end.
pub fn render(req: &RenderRequest<'_>) -> Result<()> {
    // 1. Input validation — exit-code-1 surface.
    if !req.input.exists() || !req.input.is_file() {
        return Err(Md2PdfError::InputNotFound {
            path: req.input.to_path_buf(),
        });
    }
    let markdown = std::fs::read_to_string(req.input).map_err(|source| Md2PdfError::InputRead {
        path: req.input.to_path_buf(),
        source,
    })?;

    // 2. Build the runtime collaborators.
    let base_dir = req
        .input
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let mut image_pipeline: Pipeline = PipelineBuilder::new(base_dir).build();
    let mut mermaid = RealMermaidDispatcher;
    let mut warnings = WarningCollector::new();

    // 3. Emit Typst body from the Markdown source.
    let body = emit_typst_body(&markdown, &mut image_pipeline, &mut mermaid, &mut warnings)
        .map_err(Md2PdfError::from)?;

    // 4. Compose theme + body and compile. Capture compile warnings via
    //    `Warned<...>` and bridge into the unified collector under
    //    `WarningSource::TypstCompile` (D-c3af71 §D-3). Stderr line shape:
    //    `md2pdf: warn: typst-compile: <msg>`.
    let typst_source = compose_typst_source(req.body_size_pt, &body);
    let world = ScaffoldWorld::new(typst_source);
    let compiled = typst::compile::<typst::layout::PagedDocument>(&world);
    for diag in &compiled.warnings {
        warnings.warn(WarningSource::TypstCompile, diag.message.to_string());
    }

    // 5. Strict-mode end-of-run gate (D-c3af71 §D). After all warning
    //    sources have had a chance to fire (image pipeline, mermaid,
    //    emitter, typst-compile) we ask the unified collector: "did any
    //    warning fire?" and, if so, emit the canonical summary line and
    //    return `StrictEscalation` *before* writing the PDF — the user
    //    gets a clean "no output" signal.
    if req.strict && warnings.any() {
        let count = warnings.count();
        eprintln!(
            "md2pdf: error: --strict was set and {count} warning(s) escalated to errors"
        );
        return Err(Md2PdfError::StrictEscalation { count });
    }

    let document = compiled
        .output
        .map_err(|errors| Md2PdfError::TypstCompile(format_diags(&errors)))?;

    // 6. Format dispatch (D-30e622 §3): PDF takes the well-trodden
    //    typst-pdf path; PNG forks into `pipeline::png::render_pages`
    //    which composes per-page filenames per U-915ef2.
    match (req.format, &req.output) {
        (OutputFormat::Pdf, OutputTarget::Pdf { path }) => {
            let pdf_bytes = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
                .map_err(|errors| Md2PdfError::TypstCompile(format_diags(&errors)))?;
            std::fs::write(path, pdf_bytes).map_err(|source| Md2PdfError::PdfWrite {
                path: path.clone(),
                source,
            })?;
        }
        (OutputFormat::Png, OutputTarget::PngStem { stem }) => {
            png::render_pages(&document, stem)?;
        }
        // The CLI layer composes (format, OutputTarget) consistently;
        // a mismatch here is an Internal invariant violation.
        (fmt, target) => {
            return Err(Md2PdfError::Internal(format!(
                "format/target variant mismatch: format={:?} target={:?}",
                fmt, target
            )));
        }
    }
    Ok(())
}

fn format_diags<C>(diags: &C) -> String
where
    for<'a> &'a C: IntoIterator<Item = &'a typst::diag::SourceDiagnostic>,
{
    let mut out = String::new();
    for d in diags {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&d.message);
    }
    if out.is_empty() {
        out.push_str("(no diagnostics)");
    }
    out
}
