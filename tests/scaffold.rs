//! Integration test: drive the binary end-to-end against a fixture.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

#[test]
fn scaffold_writes_pdf_next_to_input() {
    let dir = TempDir::new().unwrap();
    let md = dir.path().join("notes.md");
    fs::write(&md, "# hello\n\nbody text\n").unwrap();

    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd.arg(&md).output().expect("failed to spawn md2pdf");

    assert!(
        output.status.success(),
        "md2pdf exited non-zero: code={:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );

    let pdf = dir.path().join("notes.pdf");
    assert!(pdf.exists(), "expected {} to exist", pdf.display());

    let bytes = fs::read(&pdf).unwrap();
    assert!(
        bytes.starts_with(b"%PDF-"),
        "output does not start with PDF magic; first 8 bytes = {:?}",
        &bytes.iter().take(8).collect::<Vec<_>>()
    );
    assert!(
        bytes.len() > 1000,
        "scaffold PDF suspiciously small: {} bytes",
        bytes.len()
    );
}

#[test]
fn missing_input_exits_with_code_one() {
    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd
        .arg("/tmp/this/path/should/not/exist/blip.md")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "missing input must exit 1; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn no_args_exits_nonzero_with_usage() {
    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd.output().unwrap();
    assert!(!output.status.success(), "no-args must fail");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("FILE") || stderr.contains("Usage"),
        "no-args stderr should describe usage; got:\n{stderr}"
    );
}

#[test]
fn strict_flag_accepted() {
    let dir = TempDir::new().unwrap();
    let md = dir.path().join("a.md");
    fs::write(&md, "# hi\n").unwrap();

    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd.arg("--strict").arg(&md).output().unwrap();
    assert!(
        output.status.success(),
        "--strict on a clean doc must succeed; stderr:\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(dir.path().join("a.pdf").exists());
}

#[test]
fn unrecognized_extension_appends_pdf_verbatim() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("notes.txt");
    fs::write(&input, "# heading\n").unwrap();

    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd.arg(&input).output().unwrap();
    assert!(output.status.success(), "stderr:\n{}", String::from_utf8_lossy(&output.stderr));

    // Per D-fb4ebb §3 worked example: notes.txt → notes.txt.pdf
    assert!(dir.path().join("notes.txt.pdf").exists());
}
