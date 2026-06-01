//! End-to-end emitter smoke test (W-58a2ba bullet 7).
//!
//! Sample Markdown exercises every event class the emitter handles:
//! heading, paragraph, list (ordered+unordered), table, task list
//! (checked+unchecked), link, autolink, image-local PNG fixture,
//! inline code, fenced code, fenced mermaid (any unrecognized →
//! placeholder), blockquote, strikethrough, footnote, hard break.
//!
//! Round-trip: drive the binary over a temp Markdown file with the
//! fixture image alongside, assert the emitted PDF starts with `%PDF`
//! and is non-trivial in size.

use std::fs;

use assert_cmd::Command;
use tempfile::TempDir;

const SAMPLE_MD: &str = r#"# Emitter smoke

A paragraph with **bold**, *italic*, ~~strike~~, `inline_code`, and a hard break here\
and the rest after the break.

## Lists

- bullet one
- bullet two with [link](https://example.test)

1. ordered one
2. ordered two

- [x] done task
- [ ] pending task

## Table

| left | center | right |
|:-----|:------:|------:|
| a    | b      | c     |
| dd   | ee     | ff    |

## Code

Inline `code_token`.

```rust
fn main() { println!("hi"); }
```

```mermaid
flowchartZZZ TD
A-->B
```

## Image

![pixel](pixel.png)

## Quote

> a quoted line
>
> another quoted paragraph

## Footnote

Reference[^a].

[^a]: footnote body text.

## Autolink

<https://example.test/path>
"#;

/// 1×1 RGBA PNG built with the same machinery the theme tests use, so
/// Typst's strict decoder accepts it.
fn build_pixel_png() -> Vec<u8> {
    let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
    let mut buf: Vec<u8> = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .expect("encode pixel");
    buf
}

#[test]
fn end_to_end_emitter_round_trip_to_pdf() {
    let dir = TempDir::new().unwrap();
    let md_path = dir.path().join("smoke.md");
    let png_path = dir.path().join("pixel.png");
    fs::write(&md_path, SAMPLE_MD).unwrap();
    fs::write(&png_path, build_pixel_png()).unwrap();

    let mut cmd = Command::cargo_bin("md2pdf").unwrap();
    let output = cmd.arg(&md_path).output().expect("spawn md2pdf");
    assert!(
        output.status.success(),
        "md2pdf exited non-zero: code={:?}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr),
    );

    let pdf_path = dir.path().join("smoke.pdf");
    assert!(pdf_path.exists(), "expected {} to exist", pdf_path.display());
    let bytes = fs::read(&pdf_path).unwrap();
    assert!(
        bytes.starts_with(b"%PDF"),
        "output does not start with PDF magic; first 8 bytes = {:?}",
        bytes.iter().take(8).collect::<Vec<_>>()
    );
    assert!(
        bytes.len() > 1500,
        "smoke PDF suspiciously small: {} bytes",
        bytes.len()
    );

    // Mermaid fence with an unrecognized leading token must surface as
    // a warning per emitter contract, but must NOT abort the run.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("mermaid"),
        "expected a mermaid stub warning on stderr; got:\n{stderr}"
    );

    // Drop a copy under target/ for human inspection.
    let inspect = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join("emitter_smoke.pdf");
    let _ = fs::write(&inspect, &bytes);
}
