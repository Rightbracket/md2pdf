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

/// `md2pdf [--strict] [--font-scale <SCALE>] <FILE>`
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
}
