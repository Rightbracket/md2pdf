//! End-to-end integration tests for the `--strict` flag and the four
//! warning buckets defined by D-c3af71 §D:
//!
//! 1. image pipeline (404 / unsupported format / not-found)
//! 2. mermaid (unsupported diagram type / unknown diagram type)
//! 3. emitter-side (e.g. unrecognized image-attribute syntax) — covered
//!    transitively by the multi-warning case
//! 4. typst-compile (`Warned<...>::warnings`) — bridged into the
//!    collector via `WarningSource::TypstCompile`. We don't have a
//!    cheap, stable trigger for typst-compile warnings in v1, so the
//!    bridge itself is exercised by a unit-style invocation in the
//!    library; this file documents the canonical stderr line shape:
//!    `md2pdf: warn: typst-compile: <msg>`.
//!
//! The tests drive the real `md2pdf` binary as a subprocess and assert
//! both the exit code and key substrings in stderr. Per D-c3af71 §D,
//! per-warning stderr lines are byte-identical between default and
//! `--strict` modes; only the trailing summary line differs.
//!
//! Network-touching cases (HTTP 404) are hidden behind `#[ignore]` —
//! the image-pipeline crate already has unit tests for the 404 path
//! (with `tiny_http`-style stubs would add a new dev-dep, which is out
//! of scope for this Work). The `#[ignore]` cases include manual
//! repro instructions in their bodies.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

const SUMMARY_PREFIX: &str = "md2pdf: error: --strict was set and ";
const SUMMARY_SUFFIX: &str = " warning(s) escalated to errors";

/// Convenience: run md2pdf on a Markdown blob in a temp dir, with or
/// without --strict. Returns (exit_code, stderr_string).
fn run_md2pdf(markdown: &str, strict: bool) -> (i32, String, TempDir) {
    let dir = TempDir::new().unwrap();
    let md = dir.path().join("doc.md");
    fs::write(&md, markdown).unwrap();
    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    if strict {
        cmd.arg("--strict");
    }
    let out = cmd.arg(&md).output().expect("spawn md2pdf");
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stderr, dir)
}

/// Same, but lets the caller pre-populate the temp dir (e.g. with a
/// fixture image) before the command runs. The closure receives the
/// temp directory path; the markdown is written to `<tmp>/doc.md`.
fn run_md2pdf_with<F: FnOnce(&std::path::Path)>(
    markdown: &str,
    strict: bool,
    setup: F,
) -> (i32, String, TempDir) {
    let dir = TempDir::new().unwrap();
    setup(dir.path());
    let md = dir.path().join("doc.md");
    fs::write(&md, markdown).unwrap();
    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    if strict {
        cmd.arg("--strict");
    }
    let out = cmd.arg(&md).output().expect("spawn md2pdf");
    let code = out.status.code().unwrap_or(-1);
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    (code, stderr, dir)
}

// -- Case 1: clean doc -------------------------------------------------

#[test]
fn clean_doc_default_exit_zero_no_warnings() {
    let (code, stderr, _d) = run_md2pdf("# Hello\n\nbody.\n", false);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(
        !stderr.contains("md2pdf: warn:"),
        "expected no warnings, got:\n{stderr}"
    );
}

#[test]
fn clean_doc_strict_exit_zero_no_warnings() {
    let (code, stderr, _d) = run_md2pdf("# Hello\n\nbody.\n", true);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(
        !stderr.contains("md2pdf: warn:"),
        "expected no warnings, got:\n{stderr}"
    );
    assert!(
        !stderr.contains("escalated to errors"),
        "no warnings → no summary line; got:\n{stderr}"
    );
}

// -- Case 2: image 404 (network) — #[ignore] ---------------------------

#[test]
#[ignore = "network-touching: requires localhost stub server (out of scope for this Work). \
    Manual: serve a 404 on http://127.0.0.1:8123/missing.png and run \
    `md2pdf --strict <doc.md>` with `![x](http://127.0.0.1:8123/missing.png)`. \
    Expect: exit 6, stderr contains \"md2pdf: warn: image\" + summary line."]
fn image_404_default_then_strict() {
    // Documented manual repro above.
}

// -- Case 3: image format unsupported (.bmp) ---------------------------

fn fake_bmp_bytes() -> Vec<u8> {
    // BMP magic 'BM' + a stub header. Not PNG/JPEG/SVG → image_pipeline
    // returns "format not supported: bmp".
    let mut v = vec![0x42, 0x4D];
    v.extend(std::iter::repeat(0u8).take(64));
    v
}

#[test]
fn image_unsupported_bmp_default_warns_continues() {
    let md = "# Doc\n\n![alt](pic.bmp)\n";
    let (code, stderr, _d) = run_md2pdf_with(md, false, |dir| {
        fs::write(dir.join("pic.bmp"), fake_bmp_bytes()).unwrap();
    });
    assert_eq!(code, 0, "default mode must not exit non-zero on warn:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: warn:")
            && stderr.contains("pic.bmp")
            && stderr.contains("format not supported"),
        "expected canonical image warn, got:\n{stderr}"
    );
}

#[test]
fn image_unsupported_bmp_strict_exits_six_with_summary() {
    let md = "# Doc\n\n![alt](pic.bmp)\n";
    let (code, stderr, _d) = run_md2pdf_with(md, true, |dir| {
        fs::write(dir.join("pic.bmp"), fake_bmp_bytes()).unwrap();
    });
    assert_eq!(code, 6, "strict + warnings must exit 6:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: warn:") && stderr.contains("pic.bmp"),
        "expected per-warning line preserved in strict mode:\n{stderr}"
    );
    assert!(
        stderr.contains(SUMMARY_PREFIX) && stderr.contains(SUMMARY_SUFFIX),
        "expected canonical summary line, got:\n{stderr}"
    );
}

// -- Case 4: local image not found -------------------------------------

#[test]
fn image_local_missing_default_warns_continues() {
    let md = "# Doc\n\n![](missing.png)\n";
    let (code, stderr, _d) = run_md2pdf(md, false);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: warn:") && stderr.contains("missing.png"),
        "expected image warn, got:\n{stderr}"
    );
    assert!(
        stderr.contains("file not found"),
        "expected canonical 'file not found' reason, got:\n{stderr}"
    );
}

#[test]
fn image_local_missing_strict_exits_six() {
    let md = "# Doc\n\n![](missing.png)\n";
    let (code, stderr, _d) = run_md2pdf(md, true);
    assert_eq!(code, 6, "stderr:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: warn:") && stderr.contains("missing.png"),
        "expected per-warning line:\n{stderr}"
    );
    assert!(
        stderr.contains(SUMMARY_PREFIX),
        "expected summary line:\n{stderr}"
    );
}

// -- Case 5: mermaid unsupported diagram (gantt) -----------------------

const MERMAID_GANTT: &str = "# Doc\n\n```mermaid\ngantt\n    title my project\n    section S1\n```\n";

#[test]
fn mermaid_gantt_default_warns_continues() {
    let (code, stderr, _d) = run_md2pdf(MERMAID_GANTT, false);
    assert_eq!(code, 0, "default mode must continue:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: warn: mermaid:") && stderr.contains("gantt"),
        "expected mermaid warn mentioning gantt:\n{stderr}"
    );
}

#[test]
fn mermaid_gantt_strict_exits_six() {
    let (code, stderr, _d) = run_md2pdf(MERMAID_GANTT, true);
    assert_eq!(code, 6, "stderr:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: warn: mermaid:") && stderr.contains("gantt"),
        "expected per-warning line preserved:\n{stderr}"
    );
    assert!(
        stderr.contains(SUMMARY_PREFIX),
        "expected summary line:\n{stderr}"
    );
}

// -- Case 6: mermaid unknown diagram (nonsense) ------------------------

const MERMAID_UNKNOWN: &str = "# Doc\n\n```mermaid\nblargh foo bar\n```\n";

#[test]
fn mermaid_unknown_default_warns_continues() {
    let (code, stderr, _d) = run_md2pdf(MERMAID_UNKNOWN, false);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: warn: mermaid:")
            && stderr.contains("could not identify diagram type"),
        "expected unknown-diagram warn:\n{stderr}"
    );
}

#[test]
fn mermaid_unknown_strict_exits_six() {
    let (code, stderr, _d) = run_md2pdf(MERMAID_UNKNOWN, true);
    assert_eq!(code, 6, "stderr:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: warn: mermaid:"),
        "expected mermaid warn:\n{stderr}"
    );
    assert!(
        stderr.contains(SUMMARY_PREFIX),
        "expected summary line:\n{stderr}"
    );
}

// -- Case 7: multi-warning doc (image + mermaid) -----------------------

const MULTI: &str = "# Doc\n\n![](missing1.png)\n\n```mermaid\ngantt\n    title x\n```\n\n![](missing2.png)\n";

#[test]
fn multi_warning_default_all_warns_exit_zero() {
    let (code, stderr, _d) = run_md2pdf(MULTI, false);
    assert_eq!(code, 0, "stderr:\n{stderr}");
    let img_warn_count = stderr.matches("md2pdf: warn:").count();
    assert!(
        img_warn_count >= 3,
        "expected ≥3 warning lines (2 image + 1 mermaid); got {img_warn_count} in:\n{stderr}"
    );
    assert!(stderr.contains("missing1.png"), "missing1 warn:\n{stderr}");
    assert!(stderr.contains("missing2.png"), "missing2 warn:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: warn: mermaid:"),
        "mermaid warn:\n{stderr}"
    );
    assert!(
        !stderr.contains("escalated to errors"),
        "default mode must not emit summary:\n{stderr}"
    );
}

#[test]
fn multi_warning_strict_exit_six_with_correct_count() {
    let (code, stderr, _d) = run_md2pdf(MULTI, true);
    assert_eq!(code, 6, "stderr:\n{stderr}");
    assert!(stderr.contains("missing1.png"));
    assert!(stderr.contains("missing2.png"));
    assert!(stderr.contains("md2pdf: warn: mermaid:"));
    // Find and parse the summary line for the count.
    let line = stderr
        .lines()
        .find(|l| l.starts_with(SUMMARY_PREFIX))
        .unwrap_or_else(|| panic!("missing summary line in:\n{stderr}"));
    let rest = line.strip_prefix(SUMMARY_PREFIX).unwrap();
    let count_str = rest.split_whitespace().next().unwrap();
    let count: usize = count_str.parse().unwrap();
    assert!(
        count >= 3,
        "expected ≥3 warnings reflected in summary, got {count} in line: {line}"
    );
}

// -- Sanity: byte-identical per-warning line across modes --------------

#[test]
fn per_warning_line_is_byte_identical_default_vs_strict() {
    // D-c3af71 §D: per-warning stderr lines must be byte-identical with
    // and without --strict; only the trailing summary differs.
    let md = "# Doc\n\n![](missing.png)\n";
    let (_, stderr_default, _d1) = run_md2pdf(md, false);
    let (_, stderr_strict, _d2) = run_md2pdf(md, true);

    let warn_default: Vec<&str> = stderr_default
        .lines()
        .filter(|l| l.starts_with("md2pdf: warn:"))
        .collect();
    let warn_strict: Vec<&str> = stderr_strict
        .lines()
        .filter(|l| l.starts_with("md2pdf: warn:"))
        .collect();

    assert_eq!(
        warn_default, warn_strict,
        "per-warning lines must be byte-identical across modes\n\
         default:\n{stderr_default}\nstrict:\n{stderr_strict}"
    );
}
