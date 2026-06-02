//! Embedded built-in theme (Typst stylesheet).
//!
//! Per U-f2b045 we ship exactly one theme baked into the binary. The
//! `--theme` flag is explicitly dropped (D-fb4ebb §3). The theme is a
//! Typst module the emitter prepends before document body output.

pub const THEME: &str = include_str!("../assets/theme.typ");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn theme_is_non_empty() {
        assert!(!THEME.is_empty());
    }

    #[test]
    fn theme_defines_required_helpers() {
        // Helper-function contract used by the emitter:
        for name in [
            "md_blockquote",
            "md_codeblock",
            "md_link",
            "md_image",
            "md_image_sized",
            "md_image_placeholder",
            "md_hardbreak",
            "md_inline_html",
            "md_mermaid_stub",
            // GFM extensions added by W-e1a99c (D-c3af71 §B):
            "md_table",
            "md_task_unchecked",
            "md_task_checked",
            "md_strike",
            "md_footnote",
            "md_inline_code",
            "md_image_bytes",
        ] {
            assert!(
                THEME.contains(name),
                "theme.typ must define {name}"
            );
        }
    }

    /// Sanity-compile the THEME plus a tiny invocation of every new GFM
    /// helper (W-e1a99c). Confirms each helper is callable in real Typst,
    /// not just a substring lookup. Compiles to PDF (and discards the
    /// bytes); a failure here means a helper has a syntax/semantic error
    /// that the lex-only `theme_defines_required_helpers` test cannot
    /// catch.
    #[test]
    fn gfm_helpers_compile_and_render() {
        use crate::pipeline::world::ScaffoldWorld;

        // Build a real, decoder-strict 1×1 RGBA PNG via the `image`
        // crate (already a runtime dep) so Typst's PNG validator
        // accepts it. The crate-internal `tiny_png_fixture` has a
        // pre-baked CRC that Typst's strict decoder rejects.
        let png: Vec<u8> = {
            let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([0, 0, 0, 0]));
            let mut buf: Vec<u8> = Vec::new();
            image::DynamicImage::ImageRgba8(img)
                .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
                .expect("encode tiny PNG");
            buf
        };
        let mut png_literal = String::from("bytes((");
        for b in &png {
            png_literal.push_str(&format!("{},", b));
        }
        png_literal.push_str("))");

        let body = format!(
            r#"
= GFM helper sanity

A paragraph with #md_inline_code("inline_code") inline.

#md_strike[a strikethrough phrase]

A claim with a footnote.#md_footnote[Footnote body text.]

== Task list

- #md_task_unchecked() pending item
- #md_task_checked() done item

== Table

#md_table(
  ([H1], [H2], [H3]),
  ("left", "center", "right"),
  (
    ([a], [b], [c]),
    ([dd], [ee], [ff]),
  ),
)

== Image from bytes

#md_image_bytes({png}, "png", 12pt, none)
#md_image_bytes({png}, "png", 12pt, 12pt)
"#,
            png = png_literal,
        );

        let full = format!(
            "#let md2pdf_body_size = 11pt\n{}\n{}",
            THEME, body
        );
        let world = ScaffoldWorld::new(full);
        let result = typst::compile(&world).output;
        match result {
            Ok(doc) => {
                let pdf = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default());
                assert!(pdf.is_ok(), "PDF export failed");
                let bytes = pdf.unwrap();
                assert!(bytes.starts_with(b"%PDF-"), "not a PDF");
                assert!(bytes.len() > 1000, "PDF suspiciously small");
                // Drop a copy under target/ for human inspection (the
                // Reviewer can `open target/gfm_helpers_sanity.pdf`).
                let out = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .join("target")
                    .join("gfm_helpers_sanity.pdf");
                let _ = std::fs::write(&out, &bytes);
            }
            Err(diags) => {
                let msgs: Vec<String> =
                    diags.iter().map(|d| d.message.to_string()).collect();
                panic!("Typst compile failed:\n{}", msgs.join("\n"));
            }
        }
    }

    /// W-1b4905 / O-10564c §9 — theme must Typst-compile cleanly at
    /// the absolute-form bounds (4pt and 200pt) and at the canonical
    /// pre-flag body size (11pt). Guards the proportional-scaling
    /// refactor against extreme-bound layout failures.
    #[test]
    fn theme_compiles_across_font_scale_bounds() {
        use crate::pipeline::world::ScaffoldWorld;

        let body = r#"
= Heading 1
== Heading 2
=== Heading 3

A paragraph at the configured body size.

#md_inline_code("code") inline.

#md_strike[strike text]
"#;
        for size_pt in [4.0_f64, 11.0, 16.5, 200.0] {
            let full = format!(
                "#let md2pdf_body_size = {}pt\n{}\n{}",
                size_pt, THEME, body
            );
            let world = ScaffoldWorld::new(full);
            let result = typst::compile::<typst::layout::PagedDocument>(&world).output;
            match result {
                Ok(doc) => {
                    let pdf = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default());
                    assert!(
                        pdf.is_ok(),
                        "PDF export failed at body_size_pt={size_pt}"
                    );
                }
                Err(diags) => {
                    let msgs: Vec<String> =
                        diags.iter().map(|d| d.message.to_string()).collect();
                    panic!(
                        "Typst compile failed at body_size_pt={size_pt}:\n{}",
                        msgs.join("\n")
                    );
                }
            }
        }
    }
}
