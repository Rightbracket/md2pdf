//! md2pdf CLI entry point.
//!
//! Wires the `clap`-parsed `Cli` (per D-fb4ebb §3 + D-b53937) to the
//! rendering pipeline and maps `Md2PdfError` → exit code per D-fb4ebb §3.

use std::process::ExitCode as ProcessExitCode;

use clap::Parser;

use md2pdf::cli::{derive_output_path, Cli};
use md2pdf::error::Md2PdfError;
use md2pdf::pipeline::{render, RenderRequest};

fn main() -> ProcessExitCode {
    let cli = Cli::parse();
    let output = derive_output_path(&cli.file);

    let req = RenderRequest {
        input: &cli.file,
        output: &output,
        strict: cli.strict,
    };

    match render(&req) {
        Ok(()) => ProcessExitCode::from(0),
        Err(e) => {
            eprintln!("md2pdf: {}", error_message(&e));
            ProcessExitCode::from(e.exit_code().as_i32() as u8)
        }
    }
}

/// Render a Md2PdfError to a single-line stderr string. Keeps the
/// canonical "md2pdf: " prefix so log scrapers see a stable contract.
fn error_message(e: &Md2PdfError) -> String {
    e.to_string()
}
