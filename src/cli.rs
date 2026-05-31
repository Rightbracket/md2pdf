//! CLI argument surface.
//!
//! Implements the locked v1 CLI per **D-fb4ebb §3** and the `--strict`
//! follow-on **D-b53937**:
//!
//! ```text
//! md2pdf [--strict] <FILE>
//! ```
//!
//! Output-path derivation per **D-fb4ebb §3** worked-example table:
//! - case-insensitive `.md` / `.markdown` / `.mdown` / `.mkd` / `.mkdn`
//!   are stripped from the basename, then `.pdf` is appended.
//! - any other (or absent) extension: `.pdf` is appended verbatim.
//! - the output is placed in the same directory as the input.

use std::path::{Path, PathBuf};

use clap::Parser;

const RECOGNIZED_MD_EXTS: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn"];

/// `md2pdf [--strict] <FILE>`
#[derive(Debug, Parser)]
#[command(
    name = "md2pdf",
    version,
    about = "Render a Markdown file to a PDF next to it.",
    long_about = None,
)]
pub struct Cli {
    /// Elevate image-load warnings to hard errors (exit code 6).
    /// See D-b53937. Default OFF.
    #[arg(long = "strict", global = false, default_value_t = false)]
    pub strict: bool,

    /// Markdown input file. The output PDF is written next to it.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,
}

/// Derive the output PDF path from an input path per D-fb4ebb §3.
///
/// See the unit-test table at the bottom of this file for the exhaustive
/// case enumeration.
pub fn derive_output_path(input: &Path) -> PathBuf {
    let parent = input.parent();
    let file_name = input
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let new_basename = strip_recognized_md_ext(&file_name)
        .map(|stripped| format!("{stripped}.pdf"))
        .unwrap_or_else(|| format!("{file_name}.pdf"));

    match parent {
        Some(p) if !p.as_os_str().is_empty() => p.join(new_basename),
        _ => PathBuf::from(new_basename),
    }
}

/// If `name` ends in a case-insensitive recognized Markdown extension,
/// return the basename with that extension (and the dot) removed.
/// Otherwise return None.
fn strip_recognized_md_ext(name: &str) -> Option<String> {
    let dot = name.rfind('.')?;
    let (stem, ext_with_dot) = name.split_at(dot);
    if stem.is_empty() {
        // Files like ".md" — treat as no recognized extension; we don't
        // strip the only thing in the name.
        return None;
    }
    let ext = &ext_with_dot[1..]; // drop leading '.'
    let ext_lower = ext.to_ascii_lowercase();
    if RECOGNIZED_MD_EXTS.iter().any(|known| *known == ext_lower) {
        Some(stem.to_string())
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Cases drawn directly from D-fb4ebb §3 (the worked-example table)
    /// and O-ec289e §CLI. Format: (input, expected_output).
    const CASES: &[(&str, &str)] = &[
        ("foo.md", "foo.pdf"),
        ("notes.markdown", "notes.pdf"),
        ("README.MD", "README.pdf"),
        ("README", "README.pdf"),
        ("report.draft.md", "report.draft.pdf"),
        ("path/to/notes.md", "path/to/notes.pdf"),
        // unrecognized extension → verbatim append
        ("notes.txt", "notes.txt.pdf"),
        // additional recognized extensions
        ("doc.mdown", "doc.pdf"),
        ("doc.mkd", "doc.pdf"),
        ("doc.mkdn", "doc.pdf"),
        // case-insensitivity sweep
        ("doc.Markdown", "doc.pdf"),
        ("doc.MdOwN", "doc.pdf"),
        // dotfile that happens to be ".md" only — we do NOT strip the
        // only extension, otherwise the result is empty. Append .pdf.
        (".md", ".md.pdf"),
        // hidden-file with recognized extension
        (".env.md", ".env.pdf"),
    ];

    #[test]
    fn derive_output_path_matches_decision_table() {
        for (input, expected) in CASES {
            let got = derive_output_path(Path::new(input));
            assert_eq!(
                got,
                PathBuf::from(expected),
                "derive_output_path({input:?}) → {got:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn cli_no_args_errors_with_usage() {
        // clap returns an error with kind=MissingRequiredArgument when
        // the positional FILE is omitted.
        let result = Cli::try_parse_from(["md2pdf"]);
        let err = result.expect_err("no-args must error");
        assert_eq!(
            err.kind(),
            clap::error::ErrorKind::MissingRequiredArgument,
            "no-args should produce MissingRequiredArgument; got {:?}",
            err.kind()
        );
        let rendered = err.to_string();
        assert!(
            rendered.contains("FILE") || rendered.contains("<FILE>"),
            "usage text should mention <FILE>; got: {rendered}"
        );
    }

    #[test]
    fn cli_help_succeeds() {
        let result = Cli::try_parse_from(["md2pdf", "--help"]);
        let err = result.expect_err("--help short-circuits with DisplayHelp");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayHelp);
    }

    #[test]
    fn cli_version_succeeds() {
        let result = Cli::try_parse_from(["md2pdf", "--version"]);
        let err = result.expect_err("--version short-circuits with DisplayVersion");
        assert_eq!(err.kind(), clap::error::ErrorKind::DisplayVersion);
    }

    #[test]
    fn cli_strict_flag_parses_and_defaults_off() {
        let parsed = Cli::try_parse_from(["md2pdf", "foo.md"]).unwrap();
        assert!(!parsed.strict, "strict default must be OFF (D-b53937)");
        assert_eq!(parsed.file, PathBuf::from("foo.md"));

        let parsed = Cli::try_parse_from(["md2pdf", "--strict", "foo.md"]).unwrap();
        assert!(parsed.strict, "--strict must enable strict mode");

        // Order independence
        let parsed = Cli::try_parse_from(["md2pdf", "foo.md", "--strict"]).unwrap();
        assert!(parsed.strict);
    }

    #[test]
    fn cli_valid_file_parses() {
        let parsed = Cli::try_parse_from(["md2pdf", "/some/path/notes.markdown"]).unwrap();
        assert_eq!(parsed.file, PathBuf::from("/some/path/notes.markdown"));
        assert!(!parsed.strict);
    }

    #[test]
    fn cli_command_definition_is_well_formed() {
        // Catches programmer errors in the clap derive at compile-/test-time.
        Cli::command().debug_assert();
    }
}
