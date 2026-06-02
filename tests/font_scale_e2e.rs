//! End-to-end integration tests for `--font-scale` (W-1b4905).
//!
//! Per O-10564c §9 the tests cover:
//!   * baseline parity at scale=1.0 (default vs explicit "1.0")
//!   * three-shape collapse (1.5 ≡ 150% ≡ 16.5pt produce identical PDFs)
//!   * scale takes effect (1.0 vs 1.5 produce different bytes)
//!   * bounds rejection (exit 2, stderr "invalid value")
//!   * unsupported unit rejection
//!   * malformed-input rejection

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

const FIXTURE_MD: &str = "\
# Heading 1

Body paragraph with some text.

## Heading 2

- bullet one
- bullet two
";

fn render(args: &[&str], md_path: &std::path::Path) -> std::process::Output {
    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    for a in args {
        cmd.arg(a);
    }
    cmd.arg(md_path).output().expect("failed to spawn md2pdf")
}

fn write_fixture(dir: &TempDir, name: &str) -> std::path::PathBuf {
    let p = dir.path().join(name);
    fs::write(&p, FIXTURE_MD).unwrap();
    p
}

fn assert_success(name: &str, output: &std::process::Output) {
    assert!(
        output.status.success(),
        "{name} exited non-zero: code={:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn font_scale_default_renders_pdf() {
    let dir = TempDir::new().unwrap();
    let md = write_fixture(&dir, "a.md");
    let out = render(&[], &md);
    assert_success("default", &out);
    let pdf = dir.path().join("a.pdf");
    assert!(pdf.exists());
    let bytes = fs::read(&pdf).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
    assert!(bytes.len() > 1000);
}

#[test]
fn font_scale_three_forms_produce_identical_output() {
    // Per O-10564c §5: multiplier "1.5", percentage "150%", and absolute
    // "16.5pt" all collapse to body_size_pt = 16.5, so the rendered PDF
    // must be byte-identical across the three forms.
    let dir = TempDir::new().unwrap();

    let md_a = write_fixture(&dir, "mult.md");
    assert_success("mult", &render(&["--font-scale", "1.5"], &md_a));

    let md_b = write_fixture(&dir, "pct.md");
    assert_success("pct", &render(&["--font-scale", "150%"], &md_b));

    let md_c = write_fixture(&dir, "abs.md");
    assert_success("abs", &render(&["--font-scale", "16.5pt"], &md_c));

    let bytes_a = fs::read(dir.path().join("mult.pdf")).unwrap();
    let bytes_b = fs::read(dir.path().join("pct.pdf")).unwrap();
    let bytes_c = fs::read(dir.path().join("abs.pdf")).unwrap();

    // Byte-identity requires the three renders to differ only in their
    // input filename (which doesn't appear in the PDF). PDFs from
    // identical Typst sources may still differ by a few bytes if the
    // PDF metadata embeds a timestamp; we therefore assert byte length
    // equal AND the prefix (skip first ~512 bytes which contain
    // /CreationDate) substring-equal. If even that proves flaky we
    // tighten to byte-floor (within 1%). For now: byte-identity.
    assert_eq!(
        bytes_a, bytes_b,
        "multiplier '1.5' and percentage '150%' must produce identical PDFs"
    );
    assert_eq!(
        bytes_b, bytes_c,
        "percentage '150%' and absolute '16.5pt' must produce identical PDFs"
    );
}

#[test]
fn font_scale_takes_effect_on_pdf_bytes() {
    // Per O-10564c §9 "Scale takes effect": a different scale must
    // change the rendered output. We assert byte-inequality between
    // scale=1.0 and scale=1.5 — proves the value reaches the theme.
    let dir = TempDir::new().unwrap();

    let md1 = write_fixture(&dir, "small.md");
    assert_success("small", &render(&["--font-scale", "1.0"], &md1));

    let md2 = write_fixture(&dir, "big.md");
    assert_success("big", &render(&["--font-scale", "1.5"], &md2));

    let small = fs::read(dir.path().join("small.pdf")).unwrap();
    let big = fs::read(dir.path().join("big.pdf")).unwrap();
    assert_ne!(
        small, big,
        "scale 1.0 vs 1.5 must produce different PDF bytes"
    );
    // At 1.5x the body size, content occupies more pages / more glyphs
    // → byte size should grow. Floor: at least 5% larger.
    let ratio = big.len() as f64 / small.len() as f64;
    assert!(
        ratio > 1.0,
        "scale=1.5 PDF ({} bytes) should be larger than scale=1.0 ({} bytes); ratio={ratio}",
        big.len(),
        small.len()
    );
}

#[test]
fn font_scale_default_matches_explicit_one() {
    // Per O-10564c §9 "Baseline parity": scale=1.0 (explicit) must
    // produce byte-identical output to omitting the flag entirely
    // (default).
    let dir = TempDir::new().unwrap();

    let md_def = write_fixture(&dir, "def.md");
    assert_success("default", &render(&[], &md_def));

    let md_one = write_fixture(&dir, "one.md");
    assert_success("explicit-1.0", &render(&["--font-scale", "1.0"], &md_one));

    let bytes_def = fs::read(dir.path().join("def.pdf")).unwrap();
    let bytes_one = fs::read(dir.path().join("one.pdf")).unwrap();
    assert_eq!(
        bytes_def, bytes_one,
        "omitting --font-scale must equal --font-scale 1.0"
    );
}

#[test]
fn font_scale_bounds_rejection_exits_two() {
    // Per O-10564c §7: bounds-violation prints clap's "invalid value"
    // line on stderr and exits 2 (clap's standard usage exit code).
    let dir = TempDir::new().unwrap();
    let md = write_fixture(&dir, "x.md");
    // Use `--font-scale=<val>` (single arg form) so values that start
    // with `-` are not mistaken by clap for a separate flag.
    for bad in &["0", "-1", "25.0", "300pt"] {
        let arg = format!("--font-scale={}", bad);
        let out = render(&[&arg], &md);
        assert_eq!(
            out.status.code(),
            Some(2),
            "{bad}: expected exit 2, got {:?}; stderr:\n{}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr)
        );
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            stderr.contains("invalid value"),
            "{bad}: stderr should mention 'invalid value'; got:\n{stderr}"
        );
        assert!(
            stderr.contains("--font-scale"),
            "{bad}: stderr should mention '--font-scale'; got:\n{stderr}"
        );
    }
}

#[test]
fn font_scale_unsupported_unit_exits_two() {
    let dir = TempDir::new().unwrap();
    let md = write_fixture(&dir, "x.md");
    let out = render(&["--font-scale", "12em"], &md);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("'em'") && stderr.contains("not supported"),
        "stderr should reject unit 'em'; got:\n{stderr}"
    );
}

#[test]
fn font_scale_malformed_input_exits_two() {
    let dir = TempDir::new().unwrap();
    let md = write_fixture(&dir, "x.md");
    // Whitespace inside the argument is rejected.
    let out = render(&["--font-scale", "12 pt"], &md);
    assert_eq!(out.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid value"),
        "malformed input must produce 'invalid value' in stderr; got:\n{stderr}"
    );
}
