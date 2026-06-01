//! Hand-rolled SVG buffer used by every mermaid sub-renderer.
//!
//! Why hand-rolled rather than the `svg` crate (D-fb4ebb §1
//! recommended `svg` as primary): the constrained subset we emit
//! (`<rect>`, `<circle>`, `<line>`, `<polygon>`, `<path>`, `<text>`,
//! `<g>` with `transform="translate(x,y)"`) is small enough that a
//! string-builder produces deterministic, reviewable output without
//! pulling a new dependency. Determinism matters for the
//! golden-fixture tests (`svg`-crate output ordering is determined by
//! a builder API but the builder's internal ordering can drift across
//! versions). The Decision explicitly permits this fallback.
//!
//! The buffer is intentionally tiny — no DOM, no parsing, no
//! validation. Every helper uses XML-attribute escaping for text and
//! attribute values. We never embed user input as a tag name.

use std::fmt::Write;

/// In-memory SVG document under construction.
pub struct SvgBuf {
    out: String,
    width: f32,
    height: f32,
    closed: bool,
}

impl SvgBuf {
    pub fn new(width: f32, height: f32) -> Self {
        let mut out = String::new();
        // XML prolog kept off — Typst's resvg-backed ingestion is
        // happy with a bare `<svg>` and the smaller output is easier
        // to golden-test.
        let _ = write!(
            &mut out,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" \
             width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">",
            w = ftos(width),
            h = ftos(height),
        );
        Self {
            out,
            width,
            height,
            closed: false,
        }
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    /// Open a `<g>` with a translate transform.
    pub fn group_translate(&mut self, dx: f32, dy: f32) {
        let _ = write!(
            &mut self.out,
            "<g transform=\"translate({},{})\">",
            ftos(dx),
            ftos(dy)
        );
    }

    pub fn group_end(&mut self) {
        self.out.push_str("</g>");
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, attrs: &Attrs) {
        let _ = write!(
            &mut self.out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}/>",
            ftos(x),
            ftos(y),
            ftos(w),
            ftos(h),
            attrs.render(),
        );
    }

    pub fn rounded_rect(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        rx: f32,
        attrs: &Attrs,
    ) {
        let _ = write!(
            &mut self.out,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" rx=\"{}\" ry=\"{}\"{}/>",
            ftos(x),
            ftos(y),
            ftos(w),
            ftos(h),
            ftos(rx),
            ftos(rx),
            attrs.render(),
        );
    }

    pub fn line(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, attrs: &Attrs) {
        let _ = write!(
            &mut self.out,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{}/>",
            ftos(x1),
            ftos(y1),
            ftos(x2),
            ftos(y2),
            attrs.render(),
        );
    }

    pub fn polygon(&mut self, points: &[(f32, f32)], attrs: &Attrs) {
        self.out.push_str("<polygon points=\"");
        let mut first = true;
        for (x, y) in points {
            if !first {
                self.out.push(' ');
            }
            first = false;
            let _ = write!(&mut self.out, "{},{}", ftos(*x), ftos(*y));
        }
        let _ = write!(&mut self.out, "\"{}/>", attrs.render());
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, attrs: &Attrs) {
        let _ = write!(
            &mut self.out,
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}/>",
            ftos(cx),
            ftos(cy),
            ftos(r),
            attrs.render(),
        );
    }

    pub fn text(&mut self, x: f32, y: f32, content: &str, attrs: &Attrs) {
        let _ = write!(
            &mut self.out,
            "<text x=\"{}\" y=\"{}\"{}>{}</text>",
            ftos(x),
            ftos(y),
            attrs.render(),
            xml_escape(content),
        );
    }

    pub fn finish(mut self) -> Vec<u8> {
        if !self.closed {
            self.out.push_str("</svg>");
            self.closed = true;
        }
        self.out.into_bytes()
    }
}

/// Pre-built XML attribute list. Use `Attrs::new().set(...)`. Stable
/// ordering (insertion order) so fixture diffs are deterministic.
pub struct Attrs(Vec<(&'static str, String)>);

impl Attrs {
    pub fn new() -> Self {
        Self(Vec::new())
    }

    pub fn set(mut self, key: &'static str, value: impl Into<String>) -> Self {
        self.0.push((key, value.into()));
        self
    }

    pub fn set_f(mut self, key: &'static str, value: f32) -> Self {
        self.0.push((key, ftos(value)));
        self
    }

    fn render(&self) -> String {
        let mut s = String::new();
        for (k, v) in &self.0 {
            s.push(' ');
            s.push_str(k);
            s.push_str("=\"");
            s.push_str(&xml_escape(v));
            s.push('"');
        }
        s
    }
}

impl Default for Attrs {
    fn default() -> Self {
        Self::new()
    }
}

/// Format a float for SVG output: trim trailing zeros, but keep at
/// least one significant digit. The output is byte-stable for golden
/// tests across architectures (no locale).
pub fn ftos(v: f32) -> String {
    // Round to two decimals; `format!("{:.2}", ...)` is locale-free in
    // Rust (uses '.' as the radix marker).
    let mut s = format!("{:.2}", v);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    if s.is_empty() || s == "-0" {
        s = "0".to_string();
    }
    s
}

/// XML-escape a value used as character data or an attribute value.
///
/// We restrict to the small set of characters that can appear in our
/// constrained subset: `&`, `<`, `>`, `"`, `'`. Control characters are
/// dropped (resvg rejects them anyway), preventing one class of
/// hostile-input injection — a label containing a literal `</text>`
/// cannot escape its element, because `<` becomes `&lt;`.
pub fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Drop ASCII control characters except tab/newline (which
            // we replace with a space — text labels are single-line in
            // our constrained subset).
            '\t' | '\n' | '\r' => out.push(' '),
            c if (c as u32) < 0x20 => {} // drop other control chars
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ftos_strips_trailing_zeros() {
        assert_eq!(ftos(1.0), "1");
        assert_eq!(ftos(1.50), "1.5");
        assert_eq!(ftos(1.234), "1.23");
        assert_eq!(ftos(0.0), "0");
        assert_eq!(ftos(-1.20), "-1.2");
    }

    #[test]
    fn xml_escape_handles_specials() {
        assert_eq!(xml_escape("a<b>&c\"d'e"), "a&lt;b&gt;&amp;c&quot;d&apos;e");
    }

    #[test]
    fn xml_escape_drops_control_chars() {
        let s: String = (0u8..32).map(|c| c as char).collect();
        let escaped = xml_escape(&s);
        // Tabs/newlines/CR became spaces; everything else dropped.
        assert!(escaped.chars().all(|c| c == ' '));
        assert_eq!(escaped.len(), 3);
    }

    #[test]
    fn xml_escape_neutralises_injection_attempt() {
        let payload = "</text><script>alert(1)</script><text>";
        let escaped = xml_escape(payload);
        assert!(!escaped.contains("<script"));
        assert!(escaped.contains("&lt;script&gt;"));
    }

    #[test]
    fn finish_closes_svg() {
        let mut s = SvgBuf::new(10.0, 5.0);
        s.rect(0.0, 0.0, 10.0, 5.0, &Attrs::new().set("fill", "red"));
        let bytes = s.finish();
        let str_form = String::from_utf8(bytes).unwrap();
        assert!(str_form.starts_with("<svg"));
        assert!(str_form.ends_with("</svg>"));
        assert!(str_form.contains("fill=\"red\""));
    }
}
