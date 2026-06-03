//! End-to-end integration tests for the `--out` CLI flag.
//!
//! Implements the test plan in Decision **O-92f7a9 §Test strategy →
//! Integration tests** (W-0dc539). The tests drive the real `md2pdf`
//! binary as a subprocess and assert: the rendered PDF lands at the
//! requested path, the byte-magic is a valid PDF header, and the
//! pre-flight validators map to exit code 4 with the canonical stderr
//! shape.
//!
//! Default-path (no-flag) regression for V-866ace is covered by a single
//! representative case here; the exhaustive derivation table is already
//! exercised by `derive_output_path_matches_decision_table` in
//! `src/cli.rs::tests`.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

const PDF_MAGIC: &[u8] = b"%PDF-";

const SIMPLE_DOC: &str = "# Hello\n\nbody.\n";

fn write_input(dir: &std::path::Path, body: &str) -> std::path::PathBuf {
    let md = dir.join("doc.md");
    fs::write(&md, body).unwrap();
    md
}

fn assert_is_pdf(path: &std::path::Path) {
    let bytes = fs::read(path).unwrap_or_else(|e| panic!("could not read {path:?}: {e}"));
    assert!(
        bytes.starts_with(PDF_MAGIC),
        "expected PDF magic at {path:?}, got first bytes {:?}",
        &bytes[..bytes.len().min(8)]
    );
}

// -- 1. --out writes PDF at explicit path ------------------------------

#[test]
fn out_flag_writes_pdf_at_explicit_path() {
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);
    let out_path = dir.path().join("custom.pdf");

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--out")
        .arg(&out_path)
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    let stderr = String::from_utf8_lossy(&cmd.stderr);
    assert_eq!(cmd.status.code(), Some(0), "stderr:\n{stderr}");
    assert!(out_path.exists(), "expected output at {out_path:?}");
    assert_is_pdf(&out_path);
    // Default-derived path must NOT exist when --out was given.
    assert!(
        !dir.path().join("doc.pdf").exists(),
        "default-path should not be written when --out is set"
    );
}

// -- 2. --out overwrites existing file silently ------------------------

#[test]
fn out_flag_overwrites_existing_file() {
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);
    let out_path = dir.path().join("custom.pdf");
    fs::write(&out_path, b"this is not a PDF").unwrap();

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--out")
        .arg(&out_path)
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    let stderr = String::from_utf8_lossy(&cmd.stderr);
    assert_eq!(cmd.status.code(), Some(0), "stderr:\n{stderr}");
    // O-92f7a9 §Policy B: silent overwrite. Stderr must NOT contain a warning
    // or refusal mentioning the existing file.
    assert!(
        !stderr.contains("exists") && !stderr.contains("clobber"),
        "silent-overwrite policy violated; stderr:\n{stderr}"
    );
    assert_is_pdf(&out_path);
}

// -- 3. --out rejects existing directory -------------------------------

#[test]
fn out_flag_rejects_existing_directory() {
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);
    let target_dir = dir.path().join("subdir");
    fs::create_dir(&target_dir).unwrap();

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--out")
        .arg(&target_dir)
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    let stderr = String::from_utf8_lossy(&cmd.stderr);
    assert_eq!(cmd.status.code(), Some(4), "stderr:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: ") && stderr.contains("is an existing directory"),
        "expected canonical existing-directory error; got:\n{stderr}"
    );
}

// -- 4. --out rejects missing parent -----------------------------------

#[test]
fn out_flag_rejects_missing_parent() {
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);
    let bad_target = dir.path().join("nope").join("nada").join("out.pdf");

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--out")
        .arg(&bad_target)
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    let stderr = String::from_utf8_lossy(&cmd.stderr);
    assert_eq!(cmd.status.code(), Some(4), "stderr:\n{stderr}");
    assert!(
        stderr.contains("md2pdf: ") && stderr.contains("parent directory"),
        "expected canonical missing-parent error; got:\n{stderr}"
    );
    assert!(
        stderr.contains("does not exist"),
        "expected 'does not exist' phrase; got:\n{stderr}"
    );
}

// -- 5. --out rejects trailing-separator path --------------------------

#[test]
fn out_flag_rejects_trailing_slash() {
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);
    let mut bad = dir.path().to_string_lossy().into_owned();
    bad.push('/');

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--out")
        .arg(&bad)
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    let stderr = String::from_utf8_lossy(&cmd.stderr);
    // The trailing-slash check runs first in validate_out_path, so we
    // assert the canonical separator-error message even though the
    // existing-directory check would also catch this case.
    assert_eq!(cmd.status.code(), Some(4), "stderr:\n{stderr}");
    assert!(
        stderr.contains("ends with a path separator")
            || stderr.contains("is an existing directory"),
        "expected separator-or-directory rejection; got:\n{stderr}"
    );
}

// -- 6. -o short form is equivalent to --out ---------------------------

#[test]
fn out_flag_short_form_equivalent() {
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);
    let out_path = dir.path().join("short.pdf");

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("-o")
        .arg(&out_path)
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    let stderr = String::from_utf8_lossy(&cmd.stderr);
    assert_eq!(cmd.status.code(), Some(0), "stderr:\n{stderr}");
    assert_is_pdf(&out_path);
}

// -- 7. No --out preserves V-866ace default behavior -------------------

#[test]
fn no_out_flag_preserves_default_behavior() {
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    let stderr = String::from_utf8_lossy(&cmd.stderr);
    assert_eq!(cmd.status.code(), Some(0), "stderr:\n{stderr}");

    // V-866ace: output is `<input-stem>.pdf` next to input.
    let expected = dir.path().join("doc.pdf");
    assert!(expected.exists(), "expected default-path output at {expected:?}");
    assert_is_pdf(&expected);
}

// -- 8. --out strict-extension policy (U-b2bf02 supersedes O-92f7a9 §A) -

#[test]
fn out_flag_honors_extension_verbatim() {
    // U-b2bf02 (Client-directed) supersedes the previous O-92f7a9
    // §Policy A "no appending" stance: the trailing extension on
    // `--out` is preserved only when it matches `--format`. Here
    // `weird.txt` does not match `--format pdf` (the default), so the
    // format extension is appended literally → `weird.txt.pdf`.
    let dir = TempDir::new().unwrap();
    let md = write_input(dir.path(), SIMPLE_DOC);
    let out_path = dir.path().join("weird.txt");

    let cmd = Command::cargo_bin("md2pdf")
        .unwrap()
        .arg("--out")
        .arg(&out_path)
        .arg(&md)
        .output()
        .expect("spawn md2pdf");

    let stderr = String::from_utf8_lossy(&cmd.stderr);
    assert_eq!(cmd.status.code(), Some(0), "stderr:\n{stderr}");

    let appended = dir.path().join("weird.txt.pdf");
    assert!(
        appended.exists(),
        "expected output at strict-extension-appended path {appended:?}"
    );
    assert_is_pdf(&appended);
    // The original literal path must NOT exist — we don't write the
    // PDF bytes there under U-b2bf02.
    assert!(
        !out_path.exists(),
        "U-b2bf02: when --out extension does not match --format, \
         the literal path is not used; got file at {out_path:?}"
    );
}
