//! HTML extension support — pagebreak comment recognition,
//! HTML-table parsing, inline HTML element rendering, and CSS
//! mini-parsing for inline `style="..."` attributes.
//!
//! Per D-875e4b §1f and U-ad8c6c v2, this module lives outside the
//! main `emitter.rs` to keep the parser surface (untrusted-input
//! processing) physically separated from the trusted Typst emitter.
//! Security review pays particular attention to this directory; see
//! `tokenizer.rs` and `parser.rs` for bounds and recursion controls.
//!
//! ## Sub-modules
//!
//! - `inline` — recognized inline-HTML element classifier (`<b>`,
//!   `<i>`, `<strong>`, `<em>`, `<br>`); shared by both the
//!   outside-cell streaming Typst-markup path and the inside-cell IR
//!   build path.
//! - `css` — CSS-property mini-parser for inline `style="..."`
//!   attributes (8 recognized properties; total/silent on errors).
//! - `tokenizer` — HTML tokenizer (start/end tags, text, comments).
//! - `parser` — HTML-table parser producing the `HtmlTable` IR.
//! - `typst_emit` — `HtmlTable` IR → Typst source string.

pub mod css;
pub mod inline;
pub mod parser;
pub mod tokenizer;
pub mod typst_emit;
