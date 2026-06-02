//! md2pdf CLI entry point.
//!
//! Wires the `clap`-parsed `Cli` (per D-fb4ebb §3 + D-b53937) to the
//! rendering pipeline and maps `Md2PdfError` → exit code per D-fb4ebb §3.

use std::process::ExitCode as ProcessExitCode;

use clap::Parser;

use md2pdf::cli::{derive_output_path, validate_out_path, Cli};
use md2pdf::error::Md2PdfError;
use md2pdf::pipeline::{render, RenderRequest};

fn main() -> ProcessExitCode {
    let cli = Cli::parse();

    // Resolve the output path: explicit `--out` (validated) or the
    // derived neighbor-of-input path (D-fb4ebb §3 default).
    let output = match &cli.out {
        Some(p) => {
            if let Err(e) = validate_out_path(p) {
                eprintln!("md2pdf: {}", error_message(&e));
                return ProcessExitCode::from(e.exit_code().as_i32() as u8);
            }
            p.clone()
        }
        None => derive_output_path(&cli.file),
    };

    let req = RenderRequest {
        input: &cli.file,
        output: &output,
        strict: cli.strict,
        body_size_pt: cli.font_scale.body_size_pt,
    };

    match render(&req) {
        Ok(()) => ProcessExitCode::from(0),
        Err(e) => {
            // For StrictEscalation, the canonical summary line was
            // already emitted by `pipeline::render` per D-c3af71 §D.
            // Don't double-print here.
            if !matches!(e, Md2PdfError::StrictEscalation { .. }) {
                eprintln!("md2pdf: {}", error_message(&e));
            }
            ProcessExitCode::from(e.exit_code().as_i32() as u8)
        }
    }
}

/// Render a Md2PdfError to a single-line stderr string. Keeps the
/// canonical "md2pdf: " prefix so log scrapers see a stable contract.
fn error_message(e: &Md2PdfError) -> String {
    e.to_string()
}
