//! Typed errors and the exit-code mapping defined by D-fb4ebb §3 and
//! extended by D-b53937 (the `--strict` follow-on Decision).

use std::path::PathBuf;
use thiserror::Error;

/// Process exit codes. The numeric values are part of md2pdf's external
/// contract — every entry here corresponds to a row in the Decision's
/// exit-code table. Do not renumber without a superseding Decision.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitCode {
    Success = 0,
    InputNotFound = 1,
    MarkdownParse = 2,
    TypstCompile = 3,
    PdfWrite = 4,
    /// Reserved by D-fb4ebb §3 for the mermaid sub-renderer (lands in
    /// the mermaid Work). Kept here so the table is complete from day one.
    Mermaid = 5,
    /// `--strict` was set and at least one warning-class behavior fired.
    /// D-b53937 §3.
    StrictEscalation = 6,
    /// Catch-all for unexpected internal failures. Distinguishable from
    /// the named codes above.
    Internal = 70,
}

impl ExitCode {
    pub fn as_i32(self) -> i32 {
        self as i32
    }
}

/// Top-level error type. Each variant maps to exactly one [`ExitCode`]
/// via [`Md2PdfError::exit_code`].
#[derive(Debug, Error)]
pub enum Md2PdfError {
    #[error("input file not found or not a regular file: {path}")]
    InputNotFound { path: PathBuf },

    #[error("could not read input file {path}: {source}")]
    InputRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("markdown parse error: {0}")]
    MarkdownParse(String),

    #[error("typst compile error: {0}")]
    TypstCompile(String),

    #[error("could not write output PDF {path}: {source}")]
    PdfWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// PNG write failure. Maps to the same `ExitCode::PdfWrite` (= 4) as
    /// PDF write per Decision D-30e622 §7 — the binding exit-code table
    /// (U-64e9ec) is preserved; this variant exists only so the
    /// user-facing message reads "could not write output PNG …" instead
    /// of "PDF …".
    #[error("could not write output PNG {path}: {source}")]
    PngWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// Pre-flight `--out` path validation failed (trailing separator,
    /// existing-directory, or missing parent). Maps to [`ExitCode::PdfWrite`]
    /// because it is an I/O-class failure — the write cannot succeed.
    /// Per Decision O-92f7a9 §"Error model".
    #[error("{message}")]
    OutPathInvalid { path: PathBuf, message: String },

    #[error("--strict was set and {count} warning(s) were escalated to errors")]
    StrictEscalation { count: usize },

    #[error("internal error: {0}")]
    Internal(String),
}

impl From<crate::emitter::EmitError> for Md2PdfError {
    fn from(e: crate::emitter::EmitError) -> Self {
        match e {
            crate::emitter::EmitError::MarkdownParse(s) => Md2PdfError::MarkdownParse(s),
            crate::emitter::EmitError::Internal(s) => Md2PdfError::Internal(s),
        }
    }
}

impl Md2PdfError {
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::InputNotFound { .. } | Self::InputRead { .. } => ExitCode::InputNotFound,
            Self::MarkdownParse(_) => ExitCode::MarkdownParse,
            Self::TypstCompile(_) => ExitCode::TypstCompile,
            Self::PdfWrite { .. } | Self::PngWrite { .. } | Self::OutPathInvalid { .. } => ExitCode::PdfWrite,
            Self::StrictEscalation { .. } => ExitCode::StrictEscalation,
            Self::Internal(_) => ExitCode::Internal,
        }
    }
}

pub type Result<T> = std::result::Result<T, Md2PdfError>;
