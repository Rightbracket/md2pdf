//! CLI argument surface.
//!
//! Implements the locked v1 CLI per **D-fb4ebb §3** and the `--strict`
//! follow-on **D-b53937**, extended for PNG output by **Decision D-30e622**
//! (PNG via in-engine typst-render at the PagedDocument fork; the
//! `--format` flag selecting `pdf|png`; the strict-extension policy on
//! `--out` per U-b2bf02; the multi-page filename convention per U-915ef2).
//!
//! ```text
//! md2pdf [--strict] [--out PATH] [--font-scale N] [--format pdf|png] <FILE>
//! ```
//!
//! Output-path derivation per **D-fb4ebb §3** worked-example table (PDF
//! default) and **D-30e622 §5c** (PNG):
//! - case-insensitive `.md` / `.markdown` / `.mdown` / `.mkd` / `.mkdn`
//!   are stripped from the basename, then `.pdf` (or `.png` for PNG) is
//!   appended.
//! - any other (or absent) extension: format-extension is appended verbatim.
//! - the output is placed in the same directory as the input.
//! - When `--out` is provided, U-b2bf02's strict-extension policy
//!   applies: the trailing extension is preserved only when it matches
//!   `--format`; otherwise the format-extension is appended literally.

use std::path::{Path, PathBuf};

use clap::Parser;

const RECOGNIZED_MD_EXTS: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn"];

/// Canonical pre-flag body size in pt. The multiplier form anchors here:
/// `body_size_pt = CANONICAL_BODY_PT * multiplier`. Per O-10564c §5.
pub const CANONICAL_BODY_PT: f64 = 11.0;

/// Parsed `--font-scale` value, collapsed to a single body-size in pt
/// per O-10564c §5. The three input shapes (multiplier, percentage,
/// absolute) all converge to a single `f64` here before the value
/// reaches the render pipeline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontScale {
    pub body_size_pt: f64,
}

impl Default for FontScale {
    fn default() -> Self {
        Self {
            body_size_pt: CANONICAL_BODY_PT,
        }
    }
}

const FONT_SCALE_BAD_FORM: &str = "must be a multiplier (e.g. 1.25), a percentage (e.g. 125%), or an absolute size with unit pt|mm|cm|in (e.g. 12pt)";
const FONT_SCALE_BAD_MULTIPLIER: &str = "multiplier must be in [0.1, 20.0]";
const FONT_SCALE_BAD_ABSOLUTE: &str = "base font size must be in [4pt, 200pt]";

const MULT_MIN: f64 = 0.1;
const MULT_MAX: f64 = 20.0;
const ABS_MIN_PT: f64 = 4.0;
const ABS_MAX_PT: f64 = 200.0;

/// Parse the `--font-scale` argument string per O-10564c §2 + §4.
///
/// The grammar is suffix-distinguished:
/// * trailing `%` → percentage form (divide by 100, then multiplier gate)
/// * trailing ASCII alphabetic suffix → absolute form
///   (suffix must be `pt`/`mm`/`cm`/`in`; other letter suffixes are
///   rejected as unsupported units; non-numeric prefix is bad form)
/// * else → multiplier form (parse whole as `f64`)
///
/// All three shapes collapse to a single `body_size_pt` value.
pub fn parse_font_scale(s: &str) -> Result<FontScale, String> {
    if s.is_empty() {
        return Err(FONT_SCALE_BAD_FORM.to_string());
    }
    if s.chars().any(|c| c.is_whitespace()) {
        return Err(FONT_SCALE_BAD_FORM.to_string());
    }

    // Percentage form: single trailing '%'.
    if let Some(prefix) = s.strip_suffix('%') {
        if prefix.is_empty() || prefix.contains('%') {
            return Err(FONT_SCALE_BAD_FORM.to_string());
        }
        let pct: f64 = prefix
            .parse()
            .map_err(|_| FONT_SCALE_BAD_FORM.to_string())?;
        if !pct.is_finite() {
            return Err(FONT_SCALE_BAD_FORM.to_string());
        }
        let m = pct / 100.0;
        check_multiplier(m)?;
        return Ok(FontScale {
            body_size_pt: CANONICAL_BODY_PT * m,
        });
    }

    // Detect a trailing ASCII-alphabetic suffix (longest run of
    // ASCII letters at the end). All accepted units are 2 ASCII
    // letters, so this is byte-safe even when the prefix contains
    // non-ASCII (which we'd reject as bad form via f64 parse).
    let alpha_suffix_chars = s.chars().rev().take_while(|c| c.is_ascii_alphabetic()).count();
    if alpha_suffix_chars > 0 {
        // ASCII letters are 1 byte, so char count == byte count.
        let split = s.len() - alpha_suffix_chars;
        let prefix = &s[..split];
        let suffix = &s[split..];
        if prefix.is_empty() {
            // e.g. "pt", "abc" — no number to anchor.
            return Err(FONT_SCALE_BAD_FORM.to_string());
        }
        let n: f64 = prefix
            .parse()
            .map_err(|_| FONT_SCALE_BAD_FORM.to_string())?;
        if !n.is_finite() {
            return Err(FONT_SCALE_BAD_FORM.to_string());
        }
        let pt_per_unit = match suffix {
            "pt" => 1.0,
            "in" => 72.0,
            "cm" => 28.346_456_7,
            "mm" => 2.834_645_67,
            other => {
                return Err(format!(
                    "unit '{}' is not supported; use one of pt, mm, cm, in",
                    other
                ));
            }
        };
        let pt = n * pt_per_unit;
        check_absolute(pt)?;
        return Ok(FontScale { body_size_pt: pt });
    }

    // Multiplier form (no recognized suffix).
    let m: f64 = s.parse().map_err(|_| FONT_SCALE_BAD_FORM.to_string())?;
    if !m.is_finite() {
        return Err(FONT_SCALE_BAD_FORM.to_string());
    }
    check_multiplier(m)?;
    Ok(FontScale {
        body_size_pt: CANONICAL_BODY_PT * m,
    })
}

fn check_multiplier(m: f64) -> Result<(), String> {
    if !m.is_finite() || m < MULT_MIN || m > MULT_MAX {
        Err(FONT_SCALE_BAD_MULTIPLIER.to_string())
    } else {
        Ok(())
    }
}

fn check_absolute(pt: f64) -> Result<(), String> {
    if !pt.is_finite() || pt < ABS_MIN_PT || pt > ABS_MAX_PT {
        Err(FONT_SCALE_BAD_ABSOLUTE.to_string())
    } else {
        Ok(())
    }
}

/// `md2pdf [--strict] [--font-scale <SCALE>] [--out <PATH>] [--format pdf|png] <FILE>`
#[derive(Debug, Parser)]
#[command(
    name = "md2pdf",
    version,
    about = "Render a Markdown file to a PDF or PNG next to it.",
    long_about = None,
)]
pub struct Cli {
    /// Elevate image-load warnings to hard errors (exit code 6).
    /// See D-b53937. Default OFF.
    #[arg(long = "strict", global = false, default_value_t = false)]
    pub strict: bool,

    /// Scale font sizes proportionally. Accepts a multiplier (e.g. "1.25"),
    /// a percentage (e.g. "125%"), or an absolute base size (e.g. "12pt",
    /// "4mm", "1cm", "0.2in"). Default: 1.0 (no scaling).
    #[arg(
        long = "font-scale",
        value_name = "SCALE",
        value_parser = parse_font_scale,
        default_value = "1.0",
    )]
    pub font_scale: FontScale,

    /// Explicit output path. When omitted, the output is written next to
    /// the input with the markdown extension stripped and the
    /// format-extension appended (per D-fb4ebb §3 / D-30e622 §5c).
    /// Per U-b2bf02 strict-extension policy: the trailing extension on
    /// `--out` is preserved only when it matches `--format`; otherwise
    /// the format-extension is appended literally to the path-as-typed.
    #[arg(long = "out", short = 'o', value_name = "PATH")]
    pub out: Option<PathBuf>,

    /// Output format. Default `pdf` (preserves pre-flag CLI behavior —
    /// invocations omitting `--format` are unaffected). Per U-b2bf02 the
    /// flag is the source of truth for the format produced; the
    /// `--out` extension is never sniffed.
    #[arg(
        long = "format",
        value_enum,
        default_value_t = OutputFormat::Pdf,
        value_name = "FORMAT",
    )]
    pub format: OutputFormat,

    /// Markdown input file. The output is written next to it.
    #[arg(value_name = "FILE")]
    pub file: PathBuf,
}

/// Output format selector (per U-b2bf02 / Decision D-30e622 §5a).
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "lower")]
pub enum OutputFormat {
    Pdf,
    Png,
}

impl Default for OutputFormat {
    fn default() -> Self {
        OutputFormat::Pdf
    }
}

impl OutputFormat {
    /// Format extension (lowercase, no leading dot).
    pub fn extension(self) -> &'static str {
        match self {
            OutputFormat::Pdf => "pdf",
            OutputFormat::Png => "png",
        }
    }
}

/// Resolved output shape passed to the render pipeline. Per Decision
/// D-30e622 §5b: PDF carries an exact write path; PNG carries a *stem*
/// from which `<stem>-NN.png` page filenames are composed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputTarget {
    /// Exact PDF write path.
    Pdf { path: PathBuf },
    /// PNG stem; per U-915ef2, page-numbered filenames are composed as
    /// `<stem>-<dynamic-padded-N>.png` for each page in the document.
    PngStem { stem: PathBuf },
}

impl OutputTarget {
    /// Display label for diagnostic / log lines. For PDF this is the
    /// exact write path; for PNG this is the stem with `-N.png` shape
    /// hint.
    pub fn display(&self) -> String {
        match self {
            OutputTarget::Pdf { path } => path.display().to_string(),
            OutputTarget::PngStem { stem } => format!("{}-N.png", stem.display()),
        }
    }
}

/// Compose the resolved [`OutputTarget`] from the user-typed `--out`
/// path (if any), the chosen `--format`, and the input file path. Per
/// Decision D-30e622 §5c — the algorithm is: (1) derive the literal
/// target by applying U-b2bf02's strict-extension rule, (2) fork on
/// format to produce either a PDF write-path or a PNG stem.
pub fn compose_output_target(
    format: OutputFormat,
    out: Option<&Path>,
    input: &Path,
) -> OutputTarget {
    let format_ext = format.extension();

    // Step 1 — literal target.
    let literal: PathBuf = match out {
        Some(p) => {
            if extension_matches(p, format_ext) {
                p.to_path_buf()
            } else {
                // Append `.<format_ext>` literally to the user-typed
                // path. No stripping, no inference.
                let mut s = p.as_os_str().to_os_string();
                s.push(".");
                s.push(format_ext);
                PathBuf::from(s)
            }
        }
        None => match format {
            OutputFormat::Pdf => derive_output_path(input),
            OutputFormat::Png => {
                let stem = derive_default_stem(input);
                let mut s = stem.into_os_string();
                s.push(".png");
                PathBuf::from(s)
            }
        },
    };

    // Step 2 — fork on format.
    match format {
        OutputFormat::Pdf => OutputTarget::Pdf { path: literal },
        OutputFormat::Png => {
            // The literal target ends in `.png` (case-insensitive) by
            // construction in step 1: either the user typed it that way
            // and we preserved it, or we appended ".png". Strip it to
            // recover the stem.
            let stem = strip_trailing_ext(&literal, "png").unwrap_or(literal);
            OutputTarget::PngStem { stem }
        }
    }
}

/// True iff `path`'s final-component extension matches `ext`
/// (case-insensitive ASCII compare, no leading dot in `ext`).
fn extension_matches(path: &Path, ext: &str) -> bool {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) => e.eq_ignore_ascii_case(ext),
        None => false,
    }
}

/// Strip the trailing `.<ext>` (case-insensitive) from `path`'s final
/// component, returning a new PathBuf with the parent preserved.
/// Returns `None` if the extension does not match or the resulting stem
/// would be empty.
fn strip_trailing_ext(path: &Path, ext: &str) -> Option<PathBuf> {
    let file_name = path.file_name()?.to_string_lossy().into_owned();
    let dot = file_name.rfind('.')?;
    let actual_ext = &file_name[dot + 1..];
    if !actual_ext.eq_ignore_ascii_case(ext) {
        return None;
    }
    let stem_name = &file_name[..dot];
    if stem_name.is_empty() {
        return None;
    }
    let parent = path.parent();
    Some(match parent {
        Some(p) if !p.as_os_str().is_empty() => p.join(stem_name),
        _ => PathBuf::from(stem_name),
    })
}

/// Derive the PNG/raster *stem* from an input path. Mirrors
/// [`derive_output_path`] but does NOT append the PDF extension — the
/// caller (typically [`compose_output_target`]) appends the appropriate
/// format extension. Per Decision D-30e622 §5c.
pub fn derive_default_stem(input: &Path) -> PathBuf {
    let parent = input.parent();
    let file_name = input
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let new_basename = strip_recognized_md_ext(&file_name).unwrap_or_else(|| file_name.clone());

    match parent {
        Some(p) if !p.as_os_str().is_empty() => p.join(new_basename),
        _ => PathBuf::from(new_basename),
    }
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

/// Pre-flight validation for an explicit `--out` path per Decision
/// O-92f7a9 §"Validation".
///
/// Three checks are run **in order**:
///
/// 1. Trailing path-separator (`foo/`, or `foo\` on Windows) → reject.
/// 2. Path exists and is a directory → reject.
/// 3. Parent directory is named (i.e. not the implicit CWD case for a
///    bare filename) and does not exist → reject.
///
/// Existing-file collisions are NOT checked: silent overwrite is the
/// policy (Decision §Policy B). Permission / write-time failures
/// surface later as [`Md2PdfError::PdfWrite`] from `std::fs::write`.
pub fn validate_out_path(path: &Path) -> std::result::Result<(), crate::error::Md2PdfError> {
    use crate::error::Md2PdfError;

    let p_disp = path.display().to_string();

    // 1. Trailing path-separator check. Use the raw string form; on
    //    Windows the user's literal separator is also caught.
    let s = path.to_string_lossy();
    let ends_with_sep = s.ends_with('/') || (cfg!(windows) && s.ends_with('\\'));
    if ends_with_sep {
        return Err(Md2PdfError::OutPathInvalid {
            path: path.to_path_buf(),
            message: format!(
                "--out path \"{}\" ends with a path separator; pass an explicit filename instead",
                p_disp
            ),
        });
    }

    // 2. Existing directory rejected.
    if path.is_dir() {
        return Err(Md2PdfError::OutPathInvalid {
            path: path.to_path_buf(),
            message: format!(
                "--out path \"{}\" is an existing directory; pass an explicit filename instead",
                p_disp
            ),
        });
    }

    // 3. Parent directory existence. `path.parent()` returns `Some("")`
    //    for a bare filename like `foo.pdf` — that's the implicit CWD
    //    case and is fine. Only fail when parent is named-but-missing.
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            return Err(Md2PdfError::OutPathInvalid {
                path: path.to_path_buf(),
                message: format!(
                    "parent directory \"{}\" does not exist for --out path \"{}\"",
                    parent.display(),
                    p_disp
                ),
            });
        }
    }

    Ok(())
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

    // -------- font-scale parser tests (W-1b4905, per O-10564c §9) --------

    fn approx_eq(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn font_scale_default_is_canonical_body_size() {
        let parsed = Cli::try_parse_from(["md2pdf", "foo.md"]).unwrap();
        assert!(approx_eq(parsed.font_scale.body_size_pt, CANONICAL_BODY_PT));
    }

    #[test]
    fn font_scale_multiplier_form_accepted() {
        for (input, expected_pt) in &[
            ("1.0", 11.0),
            ("1.25", 13.75),
            ("0.5", 5.5),
            ("2", 22.0),
            ("10", 110.0),
            ("0.1", 1.1),
            ("20.0", 220.0),
            ("1.5", 16.5),
        ] {
            let fs = parse_font_scale(input).unwrap_or_else(|e| {
                panic!("multiplier {input:?} should parse, got error: {e}")
            });
            assert!(
                approx_eq(fs.body_size_pt, *expected_pt),
                "multiplier {input:?}: body_size_pt={} expected {}",
                fs.body_size_pt,
                expected_pt
            );
        }
    }

    #[test]
    fn font_scale_percentage_form_accepted() {
        for (input, expected_pt) in &[
            ("100%", 11.0),
            ("125%", 13.75),
            ("50%", 5.5),
            ("200%", 22.0),
            ("10%", 1.1),
            ("2000%", 220.0),
            ("150%", 16.5),
        ] {
            let fs = parse_font_scale(input).unwrap_or_else(|e| {
                panic!("percentage {input:?} should parse, got error: {e}")
            });
            assert!(
                approx_eq(fs.body_size_pt, *expected_pt),
                "percentage {input:?}: body_size_pt={} expected {}",
                fs.body_size_pt,
                expected_pt
            );
        }
    }

    #[test]
    fn font_scale_absolute_form_accepted() {
        // pt
        for (input, expected_pt) in &[
            ("11pt", 11.0),
            ("12pt", 12.0),
            ("4pt", 4.0),
            ("200pt", 200.0),
            ("16.5pt", 16.5),
        ] {
            let fs = parse_font_scale(input).unwrap_or_else(|e| {
                panic!("absolute {input:?} should parse, got error: {e}")
            });
            assert!(
                approx_eq(fs.body_size_pt, *expected_pt),
                "absolute {input:?}: body_size_pt={} expected {}",
                fs.body_size_pt,
                expected_pt
            );
        }
        // mm — 4mm ≈ 11.3386pt
        let fs = parse_font_scale("4mm").unwrap();
        assert!(
            (fs.body_size_pt - 11.338_582_68).abs() < 1e-3,
            "4mm got {}",
            fs.body_size_pt
        );
        // cm — 1cm ≈ 28.3464567pt
        let fs = parse_font_scale("1cm").unwrap();
        assert!((fs.body_size_pt - 28.346_456_7).abs() < 1e-3);
        // in — 0.5in = 36pt; 1in = 72pt
        let fs = parse_font_scale("0.5in").unwrap();
        assert!((fs.body_size_pt - 36.0).abs() < 1e-9);
        let fs = parse_font_scale("1in").unwrap();
        assert!((fs.body_size_pt - 72.0).abs() < 1e-9);
    }

    #[test]
    fn font_scale_three_forms_collapse_to_same_body_size() {
        let a = parse_font_scale("1.5").unwrap();
        let b = parse_font_scale("150%").unwrap();
        let c = parse_font_scale("16.5pt").unwrap();
        assert!(approx_eq(a.body_size_pt, 16.5));
        assert!(approx_eq(b.body_size_pt, 16.5));
        assert!(approx_eq(c.body_size_pt, 16.5));
    }

    #[test]
    fn font_scale_out_of_bounds_multiplier_rejected() {
        for input in &["0", "-1", "0%", "-50%", "21.0", "0.05", "2001%", "25.0"] {
            let err = parse_font_scale(input)
                .map(|fs| fs.body_size_pt)
                .err()
                .unwrap_or_else(|| panic!("{input:?} should be rejected"));
            assert!(
                err.contains("multiplier must be in"),
                "{input:?} should hit multiplier-bounds error, got: {err}"
            );
        }
    }

    #[test]
    fn font_scale_out_of_bounds_absolute_rejected() {
        for input in &["0pt", "-12pt", "300pt", "3.99pt", "201pt"] {
            let err = parse_font_scale(input)
                .err()
                .unwrap_or_else(|| panic!("{input:?} should be rejected"));
            assert!(
                err.contains("base font size must be in"),
                "{input:?} should hit absolute-bounds error, got: {err}"
            );
        }
    }

    #[test]
    fn font_scale_rejected_units() {
        for (input, unit) in &[
            ("12em", "em"),
            ("12px", "px"),
            ("12pc", "pc"),
            ("12foo", "foo"),
        ] {
            let err = parse_font_scale(input)
                .err()
                .unwrap_or_else(|| panic!("{input:?} should be rejected"));
            assert!(
                err.contains(&format!("unit '{}' is not supported", unit)),
                "{input:?} should name unit {unit:?}, got: {err}"
            );
            assert!(err.contains("pt, mm, cm, in"));
        }
    }

    #[test]
    fn font_scale_malformed_inputs_rejected() {
        for input in &[
            "", "abc", "12 pt", "pt", "12pt12", "12.5.5pt", "nan", "inf", "12%pt",
            "%", "1.0.0",
        ] {
            let err = parse_font_scale(input)
                .err()
                .unwrap_or_else(|| panic!("{input:?} should be rejected"));
            // Bad-form text must always mention the multiplier example to
            // give the user something to copy.
            assert!(
                err.contains("must be a multiplier")
                    || err.contains("multiplier must be in")
                    || err.contains("base font size must be in")
                    || err.contains("is not supported"),
                "{input:?} should produce a recognisable error, got: {err}"
            );
        }
    }

    #[test]
    fn font_scale_clap_default_no_flag() {
        let parsed = Cli::try_parse_from(["md2pdf", "foo.md"]).unwrap();
        assert!(approx_eq(parsed.font_scale.body_size_pt, 11.0));
    }

    #[test]
    fn font_scale_clap_accepts_each_form() {
        let parsed = Cli::try_parse_from(["md2pdf", "--font-scale", "1.5", "foo.md"]).unwrap();
        assert!(approx_eq(parsed.font_scale.body_size_pt, 16.5));
        let parsed = Cli::try_parse_from(["md2pdf", "--font-scale", "150%", "foo.md"]).unwrap();
        assert!(approx_eq(parsed.font_scale.body_size_pt, 16.5));
        let parsed =
            Cli::try_parse_from(["md2pdf", "--font-scale", "16.5pt", "foo.md"]).unwrap();
        assert!(approx_eq(parsed.font_scale.body_size_pt, 16.5));
    }

    #[test]
    fn font_scale_clap_rejects_bad_input() {
        let err = Cli::try_parse_from(["md2pdf", "--font-scale", "12em", "foo.md"]).unwrap_err();
        let rendered = err.to_string();
        assert!(
            rendered.contains("invalid value")
                && rendered.contains("12em")
                && rendered.contains("--font-scale"),
            "clap error should mention invalid value + input + flag, got: {rendered}"
        );
    }

    #[test]
    fn font_scale_clap_rejects_no_short_form() {
        // O-10564c §1: no short form. `-s` must NOT bind to font-scale.
        let err = Cli::try_parse_from(["md2pdf", "-s", "1.5", "foo.md"]).unwrap_err();
        // Whatever clap thinks `-s` is, it must not parse as a successful
        // font-scale binding. Either UnknownArgument or a downstream error.
        assert!(
            !matches!(err.kind(), clap::error::ErrorKind::Format),
            "unexpected error kind for -s: {:?}",
            err.kind()
        );
    }

    // -------- --out flag tests (W-0dc539, per O-92f7a9) --------

    use crate::error::{ExitCode, Md2PdfError};

    #[test]
    fn cli_out_flag_default_is_none() {
        let parsed = Cli::try_parse_from(["md2pdf", "foo.md"]).unwrap();
        assert!(parsed.out.is_none(), "no --out should leave out=None");
    }

    #[test]
    fn cli_out_flag_parses_long() {
        let parsed = Cli::try_parse_from(["md2pdf", "--out", "custom.pdf", "foo.md"]).unwrap();
        assert_eq!(parsed.out, Some(PathBuf::from("custom.pdf")));
    }

    #[test]
    fn cli_out_flag_parses_short() {
        let parsed = Cli::try_parse_from(["md2pdf", "-o", "custom.pdf", "foo.md"]).unwrap();
        assert_eq!(parsed.out, Some(PathBuf::from("custom.pdf")));
    }

    #[test]
    fn cli_out_flag_accepts_relative_and_absolute() {
        let parsed = Cli::try_parse_from(["md2pdf", "--out", "foo.pdf", "in.md"]).unwrap();
        assert_eq!(parsed.out, Some(PathBuf::from("foo.pdf")));
        let parsed = Cli::try_parse_from(["md2pdf", "--out", "/tmp/foo.pdf", "in.md"]).unwrap();
        assert_eq!(parsed.out, Some(PathBuf::from("/tmp/foo.pdf")));
    }

    #[test]
    fn cli_out_flag_honors_extension_verbatim() {
        // O-92f7a9 §Policy A: no .pdf appending. The path is what the user wrote.
        let parsed = Cli::try_parse_from(["md2pdf", "--out", "foo.txt", "in.md"]).unwrap();
        assert_eq!(parsed.out, Some(PathBuf::from("foo.txt")));
        let parsed = Cli::try_parse_from(["md2pdf", "--out", "foo", "in.md"]).unwrap();
        assert_eq!(parsed.out, Some(PathBuf::from("foo")));
    }

    #[test]
    fn validate_out_path_rejects_trailing_slash() {
        let err = validate_out_path(Path::new("foo/")).unwrap_err();
        match err {
            Md2PdfError::OutPathInvalid { message, .. } => {
                assert!(
                    message.contains("ends with a path separator"),
                    "got: {message}"
                );
            }
            other => panic!("expected OutPathInvalid, got {other:?}"),
        }
    }

    #[test]
    fn validate_out_path_rejects_existing_directory() {
        let dir = tempfile::tempdir().unwrap();
        let err = validate_out_path(dir.path()).unwrap_err();
        match err {
            Md2PdfError::OutPathInvalid { message, .. } => {
                assert!(
                    message.contains("is an existing directory"),
                    "got: {message}"
                );
            }
            other => panic!("expected OutPathInvalid, got {other:?}"),
        }
    }

    #[test]
    fn validate_out_path_rejects_missing_parent() {
        let err = validate_out_path(Path::new("/this/does/not/exist/foo.pdf")).unwrap_err();
        match err {
            Md2PdfError::OutPathInvalid { message, .. } => {
                assert!(
                    message.contains("parent directory"),
                    "got: {message}"
                );
                assert!(message.contains("does not exist"), "got: {message}");
            }
            other => panic!("expected OutPathInvalid, got {other:?}"),
        }
    }

    #[test]
    fn validate_out_path_accepts_writable_target() {
        // Bare filename: parent is "" → implicit CWD → accepted.
        validate_out_path(Path::new("foo.pdf")).expect("bare filename should validate");

        // File inside an existing directory.
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("custom.pdf");
        validate_out_path(&target).expect("non-existent file in existing dir should validate");
    }

    #[test]
    fn validate_out_path_accepts_existing_file_for_overwrite() {
        // O-92f7a9 §Policy B: silent overwrite is the policy. An existing
        // regular file at the target must pass pre-flight validation.
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("preexisting.pdf");
        std::fs::write(&target, b"old contents").unwrap();
        validate_out_path(&target)
            .expect("existing regular file should pass validation (silent overwrite policy)");
    }

    #[test]
    fn out_path_invalid_maps_to_pdfwrite_exit_code() {
        let e = Md2PdfError::OutPathInvalid {
            path: PathBuf::from("foo/"),
            message: "x".into(),
        };
        assert_eq!(e.exit_code(), ExitCode::PdfWrite);
    }

    // -------- --format flag tests (W-723d6e, per Decision D-30e622) -----

    #[test]
    fn cli_format_flag_default_is_pdf() {
        let parsed = Cli::try_parse_from(["md2pdf", "foo.md"]).unwrap();
        assert_eq!(parsed.format, OutputFormat::Pdf);
    }

    #[test]
    fn cli_format_flag_parses_pdf_and_png() {
        let parsed = Cli::try_parse_from(["md2pdf", "--format", "pdf", "foo.md"]).unwrap();
        assert_eq!(parsed.format, OutputFormat::Pdf);
        let parsed = Cli::try_parse_from(["md2pdf", "--format", "png", "foo.md"]).unwrap();
        assert_eq!(parsed.format, OutputFormat::Png);
    }

    #[test]
    fn cli_format_flag_rejects_other_values() {
        // Per D-30e622 §5a: clap ValueEnum rejects anything that isn't
        // pdf|png. The variant kind is InvalidValue (clap's usage error).
        let err = Cli::try_parse_from(["md2pdf", "--format", "svg", "foo.md"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
        let err = Cli::try_parse_from(["md2pdf", "--format", "jpg", "foo.md"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

    #[test]
    fn cli_format_flag_case_insensitive_via_value_enum() {
        // clap's ValueEnum with rename_all="lower" rejects "PNG" — value
        // labels are lowercase per the Decision. Confirm the strict-case
        // posture.
        let err = Cli::try_parse_from(["md2pdf", "--format", "PNG", "foo.md"]).unwrap_err();
        assert_eq!(err.kind(), clap::error::ErrorKind::InvalidValue);
    }

    // -------- compose_output_target — U-b2bf02 strict-extension matrix --

    /// The exhaustive worked-example table from Decision D-30e622 §5c.
    /// Rows whose source is U-b2bf02 verbatim are flagged with the comment.
    #[test]
    fn compose_output_target_strict_extension_matrix() {
        let cases: &[(OutputFormat, Option<&str>, &str, &str, OutputTargetTag, &str)] = &[
            // (format, out, input, expected-target-stem-or-path, tag, label)
            // Verbatim from U-b2bf02:
            (OutputFormat::Pdf, Some("foo.png"), "in.md", "foo.png.pdf", OutputTargetTag::Pdf, "U-b2bf02 verbatim: --out foo.png + format pdf → foo.png.pdf"),
            (OutputFormat::Png, Some("bar.pdf"), "in.md", "bar.pdf", OutputTargetTag::PngStem, "U-b2bf02 verbatim: --format png --out bar.pdf → stem bar.pdf"),
            // Implied by U-b2bf02 (Decision §5c table):
            (OutputFormat::Pdf, Some("foo.pdf"), "in.md", "foo.pdf", OutputTargetTag::Pdf, "extension matches → preserved"),
            (OutputFormat::Png, Some("bar.png"), "in.md", "bar", OutputTargetTag::PngStem, "extension matches → stem bar"),
            (OutputFormat::Pdf, Some("foo"), "in.md", "foo.pdf", OutputTargetTag::Pdf, "no extension → appended"),
            (OutputFormat::Png, Some("foo"), "in.md", "foo", OutputTargetTag::PngStem, "no extension + png → stem foo"),
            // No --out cases:
            (OutputFormat::Pdf, None, "notes.md", "notes.pdf", OutputTargetTag::Pdf, "no --out, format pdf, .md input"),
            (OutputFormat::Png, None, "notes.md", "notes", OutputTargetTag::PngStem, "no --out, format png, .md input"),
            (OutputFormat::Png, None, "notes.txt", "notes.txt", OutputTargetTag::PngStem, "no --out, format png, .txt input → stem notes.txt"),
        ];

        for (format, out, input, expected, tag, label) in cases {
            let out_path = out.map(Path::new);
            let got = compose_output_target(*format, out_path, Path::new(input));
            match (got, tag) {
                (OutputTarget::Pdf { path }, OutputTargetTag::Pdf) => {
                    assert_eq!(
                        path,
                        PathBuf::from(expected),
                        "{label}: pdf path mismatch"
                    );
                }
                (OutputTarget::PngStem { stem }, OutputTargetTag::PngStem) => {
                    assert_eq!(
                        stem,
                        PathBuf::from(expected),
                        "{label}: png stem mismatch"
                    );
                }
                (other, _) => panic!("{label}: wrong variant: {other:?}"),
            }
        }
    }

    /// Tag used to discriminate `OutputTarget` variants in the test
    /// table without needing patterns inside the data.
    enum OutputTargetTag {
        Pdf,
        PngStem,
    }

    #[test]
    fn compose_output_target_format_png_default_uses_input_neighbor() {
        // Confirms the no-`--out` PNG default: stem = input neighbor with
        // markdown extension stripped, then the literal-target step
        // appends `.png` and step 2 strips it. Net result: stem = input
        // neighbor (no extension) for `notes.md`.
        let got = compose_output_target(OutputFormat::Png, None, Path::new("path/to/notes.md"));
        match got {
            OutputTarget::PngStem { stem } => {
                assert_eq!(stem, PathBuf::from("path/to/notes"));
            }
            other => panic!("expected PngStem, got {other:?}"),
        }
    }

    #[test]
    fn derive_default_stem_table() {
        // Mirrors derive_output_path_matches_decision_table but expects
        // the bare stem (no .pdf, no .png).
        let cases: &[(&str, &str)] = &[
            ("foo.md", "foo"),
            ("notes.markdown", "notes"),
            ("README.MD", "README"),
            ("README", "README"),
            ("notes.txt", "notes.txt"),
            ("doc.mdown", "doc"),
            ("path/to/notes.md", "path/to/notes"),
            ("doc.Markdown", "doc"),
            (".md", ".md"),
            (".env.md", ".env"),
        ];
        for (input, expected) in cases {
            let got = derive_default_stem(Path::new(input));
            assert_eq!(
                got,
                PathBuf::from(expected),
                "derive_default_stem({input:?}) mismatch"
            );
        }
    }
}
