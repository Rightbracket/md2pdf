//! md2pdf — Markdown → PDF CLI with bundled color-emoji rendering.
//!
//! v1 scaffold per Work W-1bab5f. The full Markdown→Typst emitter, image
//! pipeline, and mermaid sub-renderer land in subsequent Work items.

pub mod cli;
pub mod emitter;
pub mod error;
pub mod image_pipeline;
pub mod pipeline;
pub mod theme;
pub mod warnings;

pub use error::{ExitCode, Md2PdfError};
