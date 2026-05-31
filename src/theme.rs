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
        ] {
            assert!(
                THEME.contains(name),
                "theme.typ must define {name}"
            );
        }
    }
}
