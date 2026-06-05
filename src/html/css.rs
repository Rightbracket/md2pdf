//! CSS-property mini-parser for the inline `style="..."` attribute on
//! HTML tables (D-875e4b §3, U-ad8c6c v2).
//!
//! Two public entry points:
//!
//! - `parse_style_attribute(raw)` — for `<table>`, `<thead>`, `<tbody>`,
//!   `<tr>`, `<td>`, `<th>` elements. Returns a `CellStyle` populated
//!   only with recognized declarations.
//!
//! - `parse_img_style_attribute(raw)` — for `<img>` elements. Returns
//!   an `ImgStyle` containing only the `width` and `height` properties.
//!
//! Both functions are **total** — given any input string, they return a
//! valid `CellStyle`/`ImgStyle`. Unrecognized properties are silently
//! dropped. Unrecognized values for recognized properties leave that
//! property as `None`. No warnings are fired for any parse outcome,
//! per U-ad8c6c "graceful degradation".
//!
//! ## Recognized properties (8)
//!
//! - `width` — `<length>` → `CssLength`
//! - `height` — `<length>` | `auto` → `Option<CssLength>` (None = auto)
//! - `padding` — `<length>{1,4}` shorthand → `CssPadding` (per-side)
//! - `vertical-align` — `top` | `middle` | `bottom` → `VAlign`
//! - `text-align` — `left` | `center` | `right` → `HAlign`
//! - `border-collapse` — `collapse` | `separate` → `BorderCollapse`
//! - `border` — `<length> <style> <color>` (any order, 1-3 of) → `CssBorder`
//! - `background-color` — `<color>` → `CssColor`
//!
//! ## Recognized lengths (4 units + bare)
//!
//! `%`, `pt`, `px`, `em`, plus bare numbers (treated as `px` per CSS
//! default). `px` → `pt` conversion at 0.75 ratio happens at emit time
//! (not here); the `CssLength` enum preserves the unit verbatim.
//!
//! ## Recognized colors (16 named + 3-/6-digit hex)
//!
//! CSS Level 1 named-color set (16): `black`, `silver`, `gray`,
//! `white`, `maroon`, `red`, `purple`, `fuchsia`, `green`, `lime`,
//! `olive`, `yellow`, `navy`, `blue`, `teal`, `aqua`. Plus `#RRGGBB`
//! 6-digit hex and `#RGB` 3-digit hex (expanded to 6-digit).
//!
//! ## Recognized border styles (per D-875e4b §3c v2)
//!
//! Distinct: `solid`, `dashed`, `dotted`, `none`. Degrade-to-solid:
//! `double`, `groove`, `ridge`, `inset`, `outset`. Anything else →
//! `BorderStyle::None`.
//!
//! ## Security
//!
//! - Total: no `unwrap()` on user input, no `panic!()`, no recursion.
//! - Bounded iteration: at most O(input length) work.
//! - Bounded allocation: each lowercased name is ≤ input length.
//! - The tokenizer respects quoted strings inside values so that
//!   `font-family: "Comic Sans, Arial"` doesn't break declaration
//!   delimiters.

// =============================================================================
// Public types
// =============================================================================

/// A CSS length value preserving its unit. The unit determines the
/// Typst-emit treatment (D-875e4b §3c, §4):
///
/// - `Percent`/`Pt`/`Em` → emitted verbatim (e.g. `100%`, `200pt`,
///   `5em`).
/// - `Px` → converted to `pt` at the canonical 0.75 ratio (matching
///   `Length::Px::to_pt` elsewhere in the codebase).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CssLength {
    Percent(f64),
    Pt(f64),
    Px(f64),
    Em(f64),
}

impl CssLength {
    /// Render as a Typst length expression. Per D-875e4b §3c value
    /// table: `%`/`pt`/`em` verbatim; `px` converted at 0.75 ratio.
    pub fn to_typst(&self) -> String {
        match self {
            Self::Percent(n) => format!("{}%", format_number(*n)),
            Self::Pt(n) => format!("{}pt", format_number(*n)),
            Self::Px(n) => format!("{}pt", format_number(*n * 0.75)),
            Self::Em(n) => format!("{}em", format_number(*n)),
        }
    }
}

/// CSS color parsed and normalized to a 6-digit uppercase hex string
/// (no leading `#`). Emitted to Typst as `rgb("#RRGGBB")`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CssColor {
    /// Hex digits, uppercase, no leading `#`. Always exactly 6 chars.
    pub hex: String,
}

impl CssColor {
    /// Render as a Typst color expression: `rgb("#RRGGBB")`.
    pub fn to_typst(&self) -> String {
        format!("rgb(\"#{}\")", self.hex)
    }
}

/// Padding shorthand resolved to per-side lengths.
///
/// 1-value: all sides; 2-value: top/bottom, left/right; 3-value: top,
/// left/right, bottom; 4-value: top, right, bottom, left (CSS standard
/// clockwise-from-top order).
#[derive(Debug, Clone, PartialEq)]
pub struct CssPadding {
    pub top: CssLength,
    pub right: CssLength,
    pub bottom: CssLength,
    pub left: CssLength,
}

/// Vertical alignment keyword (CSS `vertical-align`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VAlign {
    Top,
    Middle,
    Bottom,
}

impl VAlign {
    /// Render as a Typst alignment keyword.
    pub fn to_typst(self) -> &'static str {
        match self {
            Self::Top => "top",
            Self::Middle => "horizon",
            Self::Bottom => "bottom",
        }
    }
}

/// Horizontal alignment keyword (CSS `text-align`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HAlign {
    Left,
    Center,
    Right,
}

impl HAlign {
    /// Render as a Typst alignment keyword.
    pub fn to_typst(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

/// `border-collapse` keyword (CSS `border-collapse`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderCollapse {
    Collapse,
    Separate,
}

/// Border-style keyword. Per D-875e4b §3c v2:
///
/// - `Solid`/`Dashed`/`Dotted`/`None` are the four distinct styles
///   that render visually different in Typst (via `dash:` parameter).
/// - Other recognized CSS keywords (`double`, `groove`, `ridge`,
///   `inset`, `outset`) degrade to `Solid` at parse time.
/// - Unrecognized tokens map to `None` (graceful per U-ad8c6c).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BorderStyle {
    Solid,
    Dashed,
    Dotted,
    None,
}

/// A border value — `<length> <style> <color>` shorthand. Each part is
/// optional. The Typst emitter applies defaults for missing parts
/// (1pt thickness, solid style, black color).
#[derive(Debug, Clone, PartialEq)]
pub struct CssBorder {
    pub thickness: Option<CssLength>,
    pub style: Option<BorderStyle>,
    pub color: Option<CssColor>,
}

/// All recognized cell-/table-level CSS properties.
///
/// Used for both `<table>`/`<thead>`/`<tbody>`/`<tr>` (table-level
/// styles like `border-collapse`) and `<td>`/`<th>` (per-cell styles
/// like `text-align`). Properties not relevant at a given level are
/// simply ignored at emit time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CellStyle {
    pub width: Option<CssLength>,
    pub height: Option<CssLength>,
    pub padding: Option<CssPadding>,
    pub vertical_align: Option<VAlign>,
    pub text_align: Option<HAlign>,
    pub border_collapse: Option<BorderCollapse>,
    pub border: Option<CssBorder>,
    pub background_color: Option<CssColor>,
}

/// Image-specific style. Only `width` and `height` are honored on
/// `<img>` elements (D-875e4b §4).
///
/// `height: None` means either "absent" or "auto" — both render as
/// Typst `auto` (preserves aspect ratio).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImgStyle {
    pub width: Option<CssLength>,
    pub height: Option<CssLength>,
}

// =============================================================================
// Public entry points
// =============================================================================

/// Parse a `style="..."` attribute value for table-level or cell-level
/// elements. Returns a `CellStyle` containing only the recognized
/// declarations; everything else is silently dropped.
///
/// Total — never panics, never returns an error.
pub fn parse_style_attribute(raw: &str) -> CellStyle {
    let mut style = CellStyle::default();
    for (name, value) in tokenize_declarations(raw) {
        match name.as_str() {
            "width" => style.width = parse_length(&value),
            "height" => style.height = parse_height_or_auto(&value),
            "padding" => style.padding = parse_padding(&value),
            "vertical-align" => style.vertical_align = parse_valign(&value),
            "text-align" => style.text_align = parse_halign(&value),
            "border-collapse" => style.border_collapse = parse_border_collapse(&value),
            "border" => style.border = parse_border(&value),
            "background-color" => style.background_color = parse_color(&value),
            _ => {} // unrecognized property → silently dropped
        }
    }
    style
}

/// Parse a `style="..."` attribute value for an `<img>` element.
/// Returns an `ImgStyle` with only `width` and `height` populated;
/// everything else is silently dropped.
///
/// Total — never panics, never returns an error.
pub fn parse_img_style_attribute(raw: &str) -> ImgStyle {
    let mut style = ImgStyle::default();
    for (name, value) in tokenize_declarations(raw) {
        match name.as_str() {
            "width" => style.width = parse_length(&value),
            "height" => style.height = parse_height_or_auto(&value),
            _ => {} // unrecognized property on <img> → silently dropped
        }
    }
    style
}

// =============================================================================
// Internal: declaration tokenizer
// =============================================================================

/// Tokenize a `style="..."` attribute value into `(name, value)` pairs.
/// Names are ASCII-lowercased; values preserve their case (since color
/// values may include hex digits and the parsers re-lowercase as
/// needed).
///
/// Quotes (`"`, `'`) inside a value defer the `;` declaration
/// delimiter until the matching close quote, so that values like
/// `font-family: "Comic Sans, MS"` parse as a single declaration.
///
/// Malformed declarations (no `:` separator, empty name) are silently
/// skipped per D-875e4b §3d.
fn tokenize_declarations(raw: &str) -> Vec<(String, String)> {
    let bytes = raw.as_bytes();
    let len = bytes.len();
    let mut out = Vec::new();
    let mut cursor = 0;

    while cursor < len {
        // Find the end of this declaration (either `;` outside quotes,
        // or end of string).
        let decl_start = cursor;
        let mut quote: Option<u8> = None;
        while cursor < len {
            let c = bytes[cursor];
            match quote {
                Some(q) if c == q => {
                    quote = None;
                }
                None if c == b'"' || c == b'\'' => {
                    quote = Some(c);
                }
                None if c == b';' => {
                    break;
                }
                _ => {}
            }
            cursor += 1;
        }
        let decl_text = &raw[decl_start..cursor];
        // Skip the `;` separator, if any.
        if cursor < len {
            cursor += 1;
        }

        // Split on the first `:` to separate property name from value.
        if let Some(colon_pos) = decl_text.find(':') {
            let name = decl_text[..colon_pos].trim().to_ascii_lowercase();
            let value = decl_text[colon_pos + 1..].trim().to_string();
            if !name.is_empty() {
                out.push((name, value));
            }
        }
        // No colon found → malformed declaration; silently skip.
    }

    out
}

// =============================================================================
// Internal: per-property value parsers
// =============================================================================

/// Parse a CSS `<length>` token: `<n>%`, `<n>pt`, `<n>px`, `<n>em`,
/// or a bare number (treated as px per CSS default).
fn parse_length(raw: &str) -> Option<CssLength> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }

    // Strip an optional unit suffix (longest-match: try `%`, then 2-char
    // units `pt`/`px`/`em`).
    let (num_str, unit) = if let Some(stripped) = strip_suffix_ci(raw, "%") {
        (stripped, "%")
    } else if let Some(stripped) = strip_suffix_ci(raw, "pt") {
        (stripped, "pt")
    } else if let Some(stripped) = strip_suffix_ci(raw, "px") {
        (stripped, "px")
    } else if let Some(stripped) = strip_suffix_ci(raw, "em") {
        (stripped, "em")
    } else {
        (raw, "px") // bare number → treated as px (CSS default)
    };

    let n: f64 = num_str.trim().parse().ok()?;
    if !n.is_finite() {
        return None;
    }

    Some(match unit {
        "%" => CssLength::Percent(n),
        "pt" => CssLength::Pt(n),
        "px" => CssLength::Px(n),
        "em" => CssLength::Em(n),
        _ => unreachable!("unit was set by us above"),
    })
}

/// Parse the value of a `height` declaration: either a `<length>` or
/// the `auto` keyword (case-insensitive). Returns `None` for `auto` OR
/// for any unrecognized/malformed value, since both render the same in
/// Typst (the helper omits the height argument, defaulting to auto).
fn parse_height_or_auto(raw: &str) -> Option<CssLength> {
    let trimmed = raw.trim();
    if trimmed.eq_ignore_ascii_case("auto") {
        return None;
    }
    parse_length(trimmed)
}

/// Parse `padding: <length>{1,4}` shorthand into per-side lengths.
fn parse_padding(raw: &str) -> Option<CssPadding> {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    let lengths: Vec<CssLength> = parts
        .iter()
        .map(|p| parse_length(p))
        .collect::<Option<Vec<_>>>()?;

    let (top, right, bottom, left) = match lengths.len() {
        1 => (lengths[0], lengths[0], lengths[0], lengths[0]),
        2 => (lengths[0], lengths[1], lengths[0], lengths[1]),
        3 => (lengths[0], lengths[1], lengths[2], lengths[1]),
        4 => (lengths[0], lengths[1], lengths[2], lengths[3]),
        _ => return None, // 0 or 5+ values → malformed
    };

    Some(CssPadding {
        top,
        right,
        bottom,
        left,
    })
}

/// Parse `vertical-align: top|middle|bottom`. Case-insensitive.
fn parse_valign(raw: &str) -> Option<VAlign> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "top" => Some(VAlign::Top),
        "middle" => Some(VAlign::Middle),
        "bottom" => Some(VAlign::Bottom),
        _ => None,
    }
}

/// Parse `text-align: left|center|right`. Case-insensitive.
fn parse_halign(raw: &str) -> Option<HAlign> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "left" => Some(HAlign::Left),
        "center" => Some(HAlign::Center),
        "right" => Some(HAlign::Right),
        _ => None,
    }
}

/// Parse `border-collapse: collapse|separate`. Case-insensitive.
fn parse_border_collapse(raw: &str) -> Option<BorderCollapse> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "collapse" => Some(BorderCollapse::Collapse),
        "separate" => Some(BorderCollapse::Separate),
        _ => None,
    }
}

/// Parse `border: <length> <style> <color>` shorthand. Tokens may
/// appear in any order; any 1, 2, or 3 of the three components may be
/// present (missing parts default at emit time).
///
/// Returns `None` if a token is found that doesn't match any of
/// length, color, or style — this is the "malformed declaration"
/// graceful-degradation case from D-875e4b §3d.
///
/// Note: per D-875e4b §3c v2, an unrecognized but otherwise plausible
/// "style" keyword (e.g. `wave`, `unknown`) is *not* parsed as
/// malformed — it maps to `BorderStyle::None`. The classifier
/// distinguishes by trying length and color first; anything left over
/// is treated as a style attempt.
fn parse_border(raw: &str) -> Option<CssBorder> {
    let tokens: Vec<&str> = raw.split_whitespace().collect();
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() > 3 {
        // CSS spec allows max 3 tokens in border shorthand.
        return None;
    }

    let mut thickness: Option<CssLength> = None;
    let mut style: Option<BorderStyle> = None;
    let mut color: Option<CssColor> = None;

    for token in tokens {
        // Try length first.
        if thickness.is_none() {
            if let Some(len) = parse_length(token) {
                thickness = Some(len);
                continue;
            }
        }
        // Try color next (hex / named).
        if color.is_none() {
            if let Some(c) = parse_color(token) {
                color = Some(c);
                continue;
            }
        }
        // Whatever's left is treated as a style attempt. Recognized
        // style keywords map per the table; unrecognized tokens map
        // to BorderStyle::None per §3c v2 graceful degradation.
        if style.is_none() {
            style = Some(parse_border_style_keyword(token));
            continue;
        }
        // All three slots filled and an extra token remains →
        // malformed.
        return None;
    }

    Some(CssBorder {
        thickness,
        style,
        color,
    })
}

/// Map a single style keyword to a `BorderStyle`. Recognized:
///
/// - Distinct: `solid`, `dashed`, `dotted`, `none`.
/// - Degrade-to-solid: `double`, `groove`, `ridge`, `inset`, `outset`.
/// - Anything else: `BorderStyle::None` (graceful per §3c v2).
fn parse_border_style_keyword(raw: &str) -> BorderStyle {
    match raw.trim().to_ascii_lowercase().as_str() {
        "solid" => BorderStyle::Solid,
        "dashed" => BorderStyle::Dashed,
        "dotted" => BorderStyle::Dotted,
        "none" => BorderStyle::None,
        // Degrade-to-solid keywords.
        "double" | "groove" | "ridge" | "inset" | "outset" => BorderStyle::Solid,
        // Anything else: degrade to None.
        _ => BorderStyle::None,
    }
}

/// Parse a CSS `<color>`: `#RRGGBB`, `#RGB`, or one of the 16 CSS
/// Level 1 named colors.
fn parse_color(raw: &str) -> Option<CssColor> {
    let s = raw.trim();
    if let Some(hex) = s.strip_prefix('#') {
        match hex.len() {
            3 => {
                // `#RGB` → `#RRGGBB` (each digit doubled).
                if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                let bytes = hex.as_bytes();
                let r = bytes[0] as char;
                let g = bytes[1] as char;
                let b = bytes[2] as char;
                Some(CssColor {
                    hex: format!("{r}{r}{g}{g}{b}{b}").to_ascii_uppercase(),
                })
            }
            6 => {
                if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return None;
                }
                Some(CssColor {
                    hex: hex.to_ascii_uppercase(),
                })
            }
            _ => None, // unsupported hex length (e.g. `#RRGGBBAA`)
        }
    } else {
        named_color(&s.to_ascii_lowercase())
    }
}

/// Map a CSS Level 1 named color to its 6-digit hex value.
/// Returns `None` for unrecognized names — extending to CSS Level 4
/// (147 colors) is open scope per D-875e4b §3c.
fn named_color(name: &str) -> Option<CssColor> {
    let hex = match name {
        "black" => "000000",
        "silver" => "C0C0C0",
        "gray" => "808080",
        "white" => "FFFFFF",
        "maroon" => "800000",
        "red" => "FF0000",
        "purple" => "800080",
        "fuchsia" => "FF00FF",
        "green" => "008000",
        "lime" => "00FF00",
        "olive" => "808000",
        "yellow" => "FFFF00",
        "navy" => "000080",
        "blue" => "0000FF",
        "teal" => "008080",
        "aqua" => "00FFFF",
        _ => return None,
    };
    Some(CssColor {
        hex: hex.to_string(),
    })
}

// =============================================================================
// Helpers
// =============================================================================

/// Case-insensitive suffix strip. Returns the prefix if `s` ends with
/// `suffix` (case-insensitively); else `None`.
fn strip_suffix_ci<'a>(s: &'a str, suffix: &str) -> Option<&'a str> {
    if s.len() < suffix.len() {
        return None;
    }
    let split = s.len() - suffix.len();
    let tail = &s[split..];
    if tail.eq_ignore_ascii_case(suffix) {
        Some(&s[..split])
    } else {
        None
    }
}

/// Format an `f64` in a way that's pleasant for Typst output: integers
/// have no decimal point; non-integers use the default `{}` format.
fn format_number(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // --- Length parsing -----------------------------------------------------

    #[test]
    fn length_percent() {
        assert_eq!(parse_length("100%"), Some(CssLength::Percent(100.0)));
        assert_eq!(parse_length("50%"), Some(CssLength::Percent(50.0)));
        assert_eq!(parse_length("33.33%"), Some(CssLength::Percent(33.33)));
    }

    #[test]
    fn length_pt() {
        assert_eq!(parse_length("12pt"), Some(CssLength::Pt(12.0)));
        assert_eq!(parse_length("0.5pt"), Some(CssLength::Pt(0.5)));
    }

    #[test]
    fn length_px() {
        assert_eq!(parse_length("100px"), Some(CssLength::Px(100.0)));
    }

    #[test]
    fn length_em() {
        assert_eq!(parse_length("1.5em"), Some(CssLength::Em(1.5)));
    }

    #[test]
    fn length_bare_number_treated_as_px() {
        assert_eq!(parse_length("100"), Some(CssLength::Px(100.0)));
        assert_eq!(parse_length("0"), Some(CssLength::Px(0.0)));
    }

    #[test]
    fn length_case_insensitive_units() {
        assert_eq!(parse_length("100PT"), Some(CssLength::Pt(100.0)));
        assert_eq!(parse_length("100Px"), Some(CssLength::Px(100.0)));
        assert_eq!(parse_length("100Em"), Some(CssLength::Em(100.0)));
    }

    #[test]
    fn length_with_whitespace() {
        assert_eq!(parse_length("  100pt  "), Some(CssLength::Pt(100.0)));
    }

    #[test]
    fn length_malformed() {
        assert_eq!(parse_length(""), None);
        assert_eq!(parse_length("x100%"), None);
        assert_eq!(parse_length("abc"), None);
        assert_eq!(parse_length("--"), None);
    }

    #[test]
    fn length_to_typst_verbatim() {
        assert_eq!(CssLength::Percent(100.0).to_typst(), "100%");
        assert_eq!(CssLength::Pt(12.5).to_typst(), "12.5pt");
        assert_eq!(CssLength::Em(2.0).to_typst(), "2em");
    }

    #[test]
    fn length_to_typst_px_to_pt() {
        // 100px × 0.75 = 75pt
        assert_eq!(CssLength::Px(100.0).to_typst(), "75pt");
        // 50px × 0.75 = 37.5pt
        assert_eq!(CssLength::Px(50.0).to_typst(), "37.5pt");
    }

    // --- Height / auto ------------------------------------------------------

    #[test]
    fn height_auto_returns_none() {
        assert_eq!(parse_height_or_auto("auto"), None);
        assert_eq!(parse_height_or_auto("AUTO"), None);
        assert_eq!(parse_height_or_auto("  Auto  "), None);
    }

    #[test]
    fn height_length() {
        assert_eq!(parse_height_or_auto("100pt"), Some(CssLength::Pt(100.0)));
    }

    #[test]
    fn height_invalid_returns_none() {
        assert_eq!(parse_height_or_auto("garbage"), None);
    }

    // --- Color parsing ------------------------------------------------------

    #[test]
    fn color_six_digit_hex() {
        assert_eq!(
            parse_color("#FF0000"),
            Some(CssColor {
                hex: "FF0000".to_string()
            })
        );
    }

    #[test]
    fn color_six_digit_lowercase_normalized() {
        assert_eq!(
            parse_color("#abcdef"),
            Some(CssColor {
                hex: "ABCDEF".to_string()
            })
        );
    }

    #[test]
    fn color_three_digit_hex_expanded() {
        assert_eq!(
            parse_color("#F0A"),
            Some(CssColor {
                hex: "FF00AA".to_string()
            })
        );
    }

    #[test]
    fn color_named_black() {
        assert_eq!(
            parse_color("black"),
            Some(CssColor {
                hex: "000000".to_string()
            })
        );
    }

    #[test]
    fn color_named_red() {
        assert_eq!(
            parse_color("red"),
            Some(CssColor {
                hex: "FF0000".to_string()
            })
        );
    }

    #[test]
    fn color_named_case_insensitive() {
        assert_eq!(
            parse_color("BLUE"),
            Some(CssColor {
                hex: "0000FF".to_string()
            })
        );
        assert_eq!(
            parse_color("Aqua"),
            Some(CssColor {
                hex: "00FFFF".to_string()
            })
        );
    }

    #[test]
    fn color_all_16_named_colors_recognized() {
        let names = [
            "black", "silver", "gray", "white", "maroon", "red", "purple",
            "fuchsia", "green", "lime", "olive", "yellow", "navy", "blue",
            "teal", "aqua",
        ];
        for n in names {
            assert!(parse_color(n).is_some(), "expected {n} to parse");
        }
    }

    #[test]
    fn color_unrecognized_named() {
        // chartreuse is CSS Level 4, not Level 1 → not in our 16
        assert_eq!(parse_color("chartreuse"), None);
        assert_eq!(parse_color("rebeccapurple"), None);
    }

    #[test]
    fn color_malformed() {
        assert_eq!(parse_color(""), None);
        assert_eq!(parse_color("#GGGGGG"), None);   // non-hex digits
        assert_eq!(parse_color("#FF00"), None);     // wrong length
        assert_eq!(parse_color("#FF00000"), None);  // wrong length
        assert_eq!(parse_color("rgb(1,2,3)"), None);// not supported
    }

    #[test]
    fn color_to_typst() {
        let c = parse_color("red").unwrap();
        assert_eq!(c.to_typst(), "rgb(\"#FF0000\")");
    }

    // --- Padding parsing ----------------------------------------------------

    #[test]
    fn padding_one_value_all_sides() {
        let p = parse_padding("8pt").unwrap();
        assert_eq!(p.top, CssLength::Pt(8.0));
        assert_eq!(p.right, CssLength::Pt(8.0));
        assert_eq!(p.bottom, CssLength::Pt(8.0));
        assert_eq!(p.left, CssLength::Pt(8.0));
    }

    #[test]
    fn padding_two_values_top_bottom_lr() {
        let p = parse_padding("8pt 12pt").unwrap();
        assert_eq!(p.top, CssLength::Pt(8.0));
        assert_eq!(p.right, CssLength::Pt(12.0));
        assert_eq!(p.bottom, CssLength::Pt(8.0));
        assert_eq!(p.left, CssLength::Pt(12.0));
    }

    #[test]
    fn padding_three_values_top_lr_bottom() {
        let p = parse_padding("8pt 12pt 16pt").unwrap();
        assert_eq!(p.top, CssLength::Pt(8.0));
        assert_eq!(p.right, CssLength::Pt(12.0));
        assert_eq!(p.bottom, CssLength::Pt(16.0));
        assert_eq!(p.left, CssLength::Pt(12.0));
    }

    #[test]
    fn padding_four_values_clockwise() {
        let p = parse_padding("1pt 2pt 3pt 4pt").unwrap();
        assert_eq!(p.top, CssLength::Pt(1.0));
        assert_eq!(p.right, CssLength::Pt(2.0));
        assert_eq!(p.bottom, CssLength::Pt(3.0));
        assert_eq!(p.left, CssLength::Pt(4.0));
    }

    #[test]
    fn padding_zero_values_returns_none() {
        assert!(parse_padding("").is_none());
    }

    #[test]
    fn padding_five_values_returns_none() {
        assert!(parse_padding("1pt 2pt 3pt 4pt 5pt").is_none());
    }

    #[test]
    fn padding_with_invalid_token_returns_none() {
        assert!(parse_padding("8pt garbage 16pt").is_none());
    }

    // --- VAlign / HAlign ----------------------------------------------------

    #[test]
    fn valign_keywords() {
        assert_eq!(parse_valign("top"), Some(VAlign::Top));
        assert_eq!(parse_valign("middle"), Some(VAlign::Middle));
        assert_eq!(parse_valign("bottom"), Some(VAlign::Bottom));
        assert_eq!(parse_valign("TOP"), Some(VAlign::Top));
    }

    #[test]
    fn valign_unrecognized() {
        assert_eq!(parse_valign("super"), None);
        assert_eq!(parse_valign("baseline"), None);
        assert_eq!(parse_valign(""), None);
    }

    #[test]
    fn halign_keywords() {
        assert_eq!(parse_halign("left"), Some(HAlign::Left));
        assert_eq!(parse_halign("center"), Some(HAlign::Center));
        assert_eq!(parse_halign("right"), Some(HAlign::Right));
        assert_eq!(parse_halign("CENTER"), Some(HAlign::Center));
    }

    #[test]
    fn halign_unrecognized() {
        assert_eq!(parse_halign("justify"), None);
        assert_eq!(parse_halign(""), None);
    }

    #[test]
    fn valign_halign_to_typst() {
        assert_eq!(VAlign::Top.to_typst(), "top");
        assert_eq!(VAlign::Middle.to_typst(), "horizon");
        assert_eq!(VAlign::Bottom.to_typst(), "bottom");
        assert_eq!(HAlign::Left.to_typst(), "left");
        assert_eq!(HAlign::Center.to_typst(), "center");
        assert_eq!(HAlign::Right.to_typst(), "right");
    }

    // --- Border collapse ----------------------------------------------------

    #[test]
    fn border_collapse_keywords() {
        assert_eq!(
            parse_border_collapse("collapse"),
            Some(BorderCollapse::Collapse)
        );
        assert_eq!(
            parse_border_collapse("separate"),
            Some(BorderCollapse::Separate)
        );
        assert_eq!(
            parse_border_collapse("COLLAPSE"),
            Some(BorderCollapse::Collapse)
        );
    }

    #[test]
    fn border_collapse_unrecognized() {
        assert_eq!(parse_border_collapse("inherit"), None);
        assert_eq!(parse_border_collapse(""), None);
    }

    // --- Border style keyword (§3c v2 distinct/degrade) ---------------------

    #[test]
    fn border_style_distinct_solid() {
        assert_eq!(parse_border_style_keyword("solid"), BorderStyle::Solid);
    }

    #[test]
    fn border_style_distinct_dashed() {
        assert_eq!(parse_border_style_keyword("dashed"), BorderStyle::Dashed);
    }

    #[test]
    fn border_style_distinct_dotted() {
        assert_eq!(parse_border_style_keyword("dotted"), BorderStyle::Dotted);
    }

    #[test]
    fn border_style_distinct_none() {
        assert_eq!(parse_border_style_keyword("none"), BorderStyle::None);
    }

    #[test]
    fn border_style_degrade_to_solid() {
        // Per §3c v2: double/groove/ridge/inset/outset → Solid
        assert_eq!(parse_border_style_keyword("double"), BorderStyle::Solid);
        assert_eq!(parse_border_style_keyword("groove"), BorderStyle::Solid);
        assert_eq!(parse_border_style_keyword("ridge"), BorderStyle::Solid);
        assert_eq!(parse_border_style_keyword("inset"), BorderStyle::Solid);
        assert_eq!(parse_border_style_keyword("outset"), BorderStyle::Solid);
    }

    #[test]
    fn border_style_unrecognized_to_none() {
        // Per §3c v2: any other token → None (graceful)
        assert_eq!(parse_border_style_keyword("wave"), BorderStyle::None);
        assert_eq!(parse_border_style_keyword("garbage"), BorderStyle::None);
        assert_eq!(parse_border_style_keyword(""), BorderStyle::None);
    }

    #[test]
    fn border_style_case_insensitive() {
        assert_eq!(parse_border_style_keyword("SOLID"), BorderStyle::Solid);
        assert_eq!(parse_border_style_keyword("Dashed"), BorderStyle::Dashed);
    }

    // --- Border shorthand ---------------------------------------------------

    #[test]
    fn border_canonical_three_tokens() {
        let b = parse_border("1pt solid black").unwrap();
        assert_eq!(b.thickness, Some(CssLength::Pt(1.0)));
        assert_eq!(b.style, Some(BorderStyle::Solid));
        assert_eq!(
            b.color,
            Some(CssColor {
                hex: "000000".to_string()
            })
        );
    }

    #[test]
    fn border_order_independence() {
        // length-color-style
        let b = parse_border("1pt black solid").unwrap();
        assert_eq!(b.thickness, Some(CssLength::Pt(1.0)));
        assert_eq!(b.style, Some(BorderStyle::Solid));
        assert!(b.color.is_some());

        // style-color-length
        let b = parse_border("solid red 2px").unwrap();
        assert_eq!(b.thickness, Some(CssLength::Px(2.0)));
        assert_eq!(b.style, Some(BorderStyle::Solid));
        assert_eq!(
            b.color,
            Some(CssColor {
                hex: "FF0000".to_string()
            })
        );

        // color-length-style
        let b = parse_border("blue 3pt dashed").unwrap();
        assert_eq!(b.thickness, Some(CssLength::Pt(3.0)));
        assert_eq!(b.style, Some(BorderStyle::Dashed));
        assert_eq!(
            b.color,
            Some(CssColor {
                hex: "0000FF".to_string()
            })
        );
    }

    #[test]
    fn border_one_token_each() {
        // Just length.
        let b = parse_border("2pt").unwrap();
        assert_eq!(b.thickness, Some(CssLength::Pt(2.0)));
        assert!(b.style.is_none());
        assert!(b.color.is_none());

        // Just style.
        let b = parse_border("dashed").unwrap();
        assert_eq!(b.style, Some(BorderStyle::Dashed));
        assert!(b.thickness.is_none());
        assert!(b.color.is_none());

        // Just color.
        let b = parse_border("red").unwrap();
        assert!(b.color.is_some());
        assert!(b.thickness.is_none());
        assert!(b.style.is_none());
    }

    #[test]
    fn border_two_tokens() {
        let b = parse_border("1pt solid").unwrap();
        assert_eq!(b.thickness, Some(CssLength::Pt(1.0)));
        assert_eq!(b.style, Some(BorderStyle::Solid));
        assert!(b.color.is_none());
    }

    #[test]
    fn border_unrecognized_style_keyword() {
        // Per §3c v2: unrecognized "style" keyword in the third
        // position degrades to BorderStyle::None.
        let b = parse_border("1pt wave red").unwrap();
        assert_eq!(b.thickness, Some(CssLength::Pt(1.0)));
        assert_eq!(b.style, Some(BorderStyle::None));
        assert!(b.color.is_some());
    }

    #[test]
    fn border_too_many_tokens_returns_none() {
        // 4 tokens > spec max of 3 → malformed.
        assert!(parse_border("1pt solid red blue").is_none());
    }

    #[test]
    fn border_empty_returns_none() {
        assert!(parse_border("").is_none());
        assert!(parse_border("   ").is_none());
    }

    // --- parse_style_attribute (full integration) ---------------------------

    #[test]
    fn style_attr_empty_returns_default() {
        assert_eq!(parse_style_attribute(""), CellStyle::default());
    }

    #[test]
    fn style_attr_single_decl() {
        let s = parse_style_attribute("width: 100%");
        assert_eq!(s.width, Some(CssLength::Percent(100.0)));
        assert!(s.height.is_none());
    }

    #[test]
    fn style_attr_multiple_decls() {
        let s = parse_style_attribute(
            "width: 50%; padding: 8pt; text-align: center; \
             background-color: #ffeecc",
        );
        assert_eq!(s.width, Some(CssLength::Percent(50.0)));
        assert!(s.padding.is_some());
        assert_eq!(s.text_align, Some(HAlign::Center));
        assert_eq!(
            s.background_color,
            Some(CssColor {
                hex: "FFEECC".to_string()
            })
        );
    }

    #[test]
    fn style_attr_trailing_semicolon() {
        let s = parse_style_attribute("width: 100pt;");
        assert_eq!(s.width, Some(CssLength::Pt(100.0)));
    }

    #[test]
    fn style_attr_leading_semicolon() {
        let s = parse_style_attribute("; width: 100pt");
        assert_eq!(s.width, Some(CssLength::Pt(100.0)));
    }

    #[test]
    fn style_attr_unrecognized_property_dropped() {
        let s = parse_style_attribute(
            "width: 100pt; font-family: Arial; color: chartreuse",
        );
        assert_eq!(s.width, Some(CssLength::Pt(100.0)));
        // font-family and color are silently dropped.
    }

    #[test]
    fn style_attr_unrecognized_value_for_recognized_property() {
        // text-align: justify isn't in our recognized set → None.
        let s = parse_style_attribute("text-align: justify");
        assert!(s.text_align.is_none());
    }

    #[test]
    fn style_attr_malformed_decl_no_colon() {
        // `width 100pt` (no colon) is malformed → silently skipped.
        let s = parse_style_attribute("width 100pt; height: 50pt");
        assert!(s.width.is_none());
        assert_eq!(s.height, Some(CssLength::Pt(50.0)));
    }

    #[test]
    fn style_attr_quoted_value_does_not_break_delimiter() {
        let s = parse_style_attribute(
            "font-family: \"Comic Sans, Arial\"; width: 100pt",
        );
        // font-family is dropped, width still parses correctly.
        assert_eq!(s.width, Some(CssLength::Pt(100.0)));
    }

    #[test]
    fn style_attr_property_name_case_insensitive() {
        let s = parse_style_attribute("WIDTH: 100pt; Background-Color: red");
        assert_eq!(s.width, Some(CssLength::Pt(100.0)));
        assert!(s.background_color.is_some());
    }

    #[test]
    fn style_attr_border_collapse() {
        let s = parse_style_attribute("border-collapse: collapse");
        assert_eq!(s.border_collapse, Some(BorderCollapse::Collapse));
    }

    #[test]
    fn style_attr_border_full() {
        let s = parse_style_attribute("border: 1pt solid black");
        let b = s.border.unwrap();
        assert_eq!(b.thickness, Some(CssLength::Pt(1.0)));
        assert_eq!(b.style, Some(BorderStyle::Solid));
        assert_eq!(
            b.color,
            Some(CssColor {
                hex: "000000".to_string()
            })
        );
    }

    #[test]
    fn style_attr_height_auto() {
        // `height: auto` → height stays None (treated same as absent).
        let s = parse_style_attribute("height: auto");
        assert!(s.height.is_none());
    }

    #[test]
    fn style_attr_padding_shorthand() {
        let s = parse_style_attribute("padding: 4pt 8pt");
        let p = s.padding.unwrap();
        assert_eq!(p.top, CssLength::Pt(4.0));
        assert_eq!(p.right, CssLength::Pt(8.0));
        assert_eq!(p.bottom, CssLength::Pt(4.0));
        assert_eq!(p.left, CssLength::Pt(8.0));
    }

    // --- parse_img_style_attribute -----------------------------------------

    #[test]
    fn img_style_width_and_height() {
        let s = parse_img_style_attribute("width: 200px; height: 150px");
        assert_eq!(s.width, Some(CssLength::Px(200.0)));
        assert_eq!(s.height, Some(CssLength::Px(150.0)));
    }

    #[test]
    fn img_style_height_auto() {
        let s = parse_img_style_attribute("width: 100%; height: auto");
        assert_eq!(s.width, Some(CssLength::Percent(100.0)));
        assert!(s.height.is_none());
    }

    #[test]
    fn img_style_other_props_ignored() {
        // <img> doesn't honor padding, border, etc.
        let s = parse_img_style_attribute(
            "width: 100pt; padding: 8pt; border: 1pt solid red",
        );
        assert_eq!(s.width, Some(CssLength::Pt(100.0)));
        assert!(s.height.is_none());
    }

    #[test]
    fn img_style_empty() {
        assert_eq!(parse_img_style_attribute(""), ImgStyle::default());
    }

    // --- Tokenizer edge cases ----------------------------------------------

    #[test]
    fn tokenizer_handles_no_trailing_semicolon() {
        let decls = tokenize_declarations("width: 100pt; height: 50pt");
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0], ("width".to_string(), "100pt".to_string()));
        assert_eq!(decls[1], ("height".to_string(), "50pt".to_string()));
    }

    #[test]
    fn tokenizer_skips_empty_decl_between_semicolons() {
        let decls = tokenize_declarations("width: 100pt;; height: 50pt");
        // Empty middle decl has no `:` → silently skipped.
        assert_eq!(decls.len(), 2);
    }

    #[test]
    fn tokenizer_quoted_value_preserves_semicolon() {
        let decls = tokenize_declarations(
            "font-family: 'Foo;Bar'; width: 100pt",
        );
        assert_eq!(decls.len(), 2);
        assert_eq!(decls[0].0, "font-family");
        // The quoted string contains the `;` but we treat it as one
        // declaration value.
        assert_eq!(decls[0].1, "'Foo;Bar'");
        assert_eq!(decls[1].0, "width");
    }

    #[test]
    fn tokenizer_lowercases_property_name() {
        let decls = tokenize_declarations("WIDTH: 100PT");
        assert_eq!(decls[0].0, "width");
        // Value preserved as-is (parsers re-lowercase as needed).
        assert_eq!(decls[0].1, "100PT");
    }

    #[test]
    fn tokenizer_handles_whitespace_around_colon() {
        let decls = tokenize_declarations("  width  :  100pt  ");
        assert_eq!(decls.len(), 1);
        assert_eq!(decls[0], ("width".to_string(), "100pt".to_string()));
    }

    // --- Security / robustness ---------------------------------------------

    #[test]
    fn does_not_panic_on_pathological_input() {
        // Long input → bounded work, no panic.
        let huge = "width: 100pt; ".repeat(10_000);
        let _ = parse_style_attribute(&huge); // should complete
    }

    #[test]
    fn does_not_panic_on_unmatched_quote() {
        // Open quote with no close → should reach end gracefully.
        let _ = parse_style_attribute("font-family: 'unterminated");
        let _ = parse_style_attribute("font-family: \"unterminated");
    }

    #[test]
    fn does_not_panic_on_malformed_unicode_boundaries() {
        // Inputs with non-ASCII inside a value should not panic.
        let _ = parse_style_attribute("background-color: 中文; width: 100pt");
        // Non-ASCII property name is unrecognized, dropped silently.
        let _ = parse_style_attribute("中文: 100pt");
    }
}
