//! md2pdf rendering pipeline.
//!
//! Per W-58a2ba: the placeholder body that the v1 scaffold emitted is
//! gone; this module now reads the user's Markdown, hands it to
//! `emitter::emit_typst_body`, concatenates the body onto
//! `theme::THEME`, and feeds the combined source to Typst via the
//! hand-rolled `World` for compile + PDF export.
//!
//! ## Composition order (per D-c3af71 §B "theme composition")
//!
//! ```text
//!   THEME (set rules + helpers, baked-in)
//! + emitted body (Typst markup with helper invocations)
//! ```
//!
//! ## Strict-mode plumbing
//!
//! `WarningCollector` is constructed here and threaded into both the
//! image pipeline (via the `WarnSink` indirection — image pipeline
//! still writes its own stderr lines, then we drain into the unified
//! accumulator inside the emitter) and the emitter itself. After
//! emission, if `req.strict` is set and the collector reports any
//! warning, we return `Md2PdfError::StrictEscalation` *before* writing
//! the PDF, per D-b53937 §3.

pub(crate) mod world;

use std::path::Path;

use crate::emitter::{emit_typst_body, RealMermaidDispatcher};
use crate::error::{Md2PdfError, Result};
use crate::image_pipeline::{Pipeline, PipelineBuilder};
use crate::pipeline::world::ScaffoldWorld;
use crate::theme::THEME;
use crate::warnings::{WarningCollector, WarningSource};

/// Inputs to a single render run.
pub struct RenderRequest<'a> {
    pub input: &'a Path,
    pub output: &'a Path,
    /// Per D-b53937 §3: when true, any recorded warning escalates to
    /// exit code 6 and no PDF is written.
    pub strict: bool,
    /// Body font size in points, derived from the `--font-scale` CLI
    /// flag per O-10564c §5/§6. The three input shapes (multiplier,
    /// percentage, absolute) all collapse to a single value here.
    /// Default: 11.0 (canonical pre-flag body size, multiplier=1.0).
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
    let pdf_bytes = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
        .map_err(|errors| Md2PdfError::TypstCompile(format_diags(&errors)))?;

    // 6. Write the PDF next to the input.
    std::fs::write(req.output, pdf_bytes).map_err(|source| Md2PdfError::PdfWrite {
        path: req.output.to_path_buf(),
        source,
    })?;
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
