//! Markdown → Typst event-stream emitter.
//!
//! Per D-c3af71 §A/§B/§C: walk a `pulldown_cmark` event stream with
//! GFM extensions enabled and emit Typst body content composed onto the
//! existing `theme::THEME` preamble. **No AST.** State lives in a
//! `MarkdownEmitter` struct (output `String`, env stack, borrowed
//! references to the image pipeline / mermaid dispatcher / warning
//! collector).
//!
//! ## What lives here
//!
//! - `emit_typst_body`: public entry point.
//! - `MarkdownEmitter`: visitor state, holds the env-frame stack.
//! - `MermaidDispatcher`: trait-shaped dispatch surface plus the
//!   stub impl that just returns `UnsupportedDiagramType` (W#6'
//!   replaces it with the real wiring).
//! - Helper functions for Typst escaping (markup vs string-literal vs
//!   bytes-literal forms).
//!
//! ## Footnotes (two-pass)
//!
//! pulldown_cmark emits `Tag::FootnoteDefinition` blocks at top level,
//! independent of where they're referenced. We collect all events into
//! a `Vec<Event>` first, then:
//!   1. Pass 1 — render each FootnoteDefinition body into a Typst
//!      content string, keyed by label.
//!   2. Pass 2 — render the document, skipping FootnoteDefinitions, and
//!      on `Event::FootnoteReference(label)` emit
//!      `#md_footnote[<rendered body>]`.
//!
//! Two passes is more memory than streaming but the simpler invariant
//! is worth it: definitions can come before *or* after their references
//! and we have a single bounded resolution pass.

use pulldown_cmark::{
    Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};

use crate::html::inline::{
    try_classify_inline_html, InlineHtmlKind, RecognizedInlineFormatter,
};
use crate::html::parser::{try_parse_html_table, ParseOutcome};
use crate::html::typst_emit::emit_html_table_typst;
use crate::image_pipeline::{
    EmbeddedFormat, ImageRequest, Pipeline, ResolvedImage,
};
use crate::warnings::{WarningCollector, WarningSource};

// -----------------------------------------------------------------------------
// MermaidDispatcher: API shape + stub.
// -----------------------------------------------------------------------------

/// Errors a mermaid dispatcher can return. The real dispatcher
/// (W-ddc4e7) carries the typed `crate::mermaid::MermaidError` into
/// `Other(String)`; the `UnsupportedDiagramType` variant is preserved
/// for the legacy stub used by tests that should not touch the real
/// `crate::mermaid` machinery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidDispatchError {
    UnsupportedDiagramType,
    /// The real dispatcher routes any `MermaidError` (parse, layout,
    /// unknown/unsupported diagram type, oversize input) here, with
    /// `to_string()` as the human reason. Per D-c3af71 §D-2, the
    /// emitter routes this to a `WarningSource::Mermaid` warning + the
    /// `md_mermaid_stub(src)` placeholder; never produces an
    /// `Md2PdfError::*` exit-5 hard error.
    Other(String),
}

impl std::fmt::Display for MermaidDispatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MermaidDispatchError::UnsupportedDiagramType => {
                write!(f, "diagram type not supported (mermaid sub-renderer not yet wired)")
            }
            MermaidDispatchError::Other(s) => write!(f, "{}", s),
        }
    }
}

/// What a successful render returns: SVG bytes the emitter hands to
/// `md_image_bytes(...)` with `format: "svg"`.
#[derive(Debug, Clone)]
pub struct MermaidRendered {
    pub svg_bytes: Vec<u8>,
    /// Suggested width in points. The stub never fires; the real
    /// dispatcher (W#6') will compute this from the layout pass.
    pub width_pt: f64,
    pub height_pt: Option<f64>,
}

/// Mermaid dispatch surface. The emitter calls `render(src)` for every
/// fenced ` ```mermaid ` block; W#6' replaces the stub with the real
/// per-diagram-type renderer.
pub trait MermaidDispatcher {
    fn render(&mut self, src: &str) -> Result<MermaidRendered, MermaidDispatchError>;
}

/// Stub impl: returns `UnsupportedDiagramType` for every input. Kept
/// as a test seam — emitter unit tests use this so they exercise the
/// "mermaid render failed → warning + placeholder" branch without
/// pulling the real `crate::mermaid` machinery into the test binary.
/// Production code uses [`RealMermaidDispatcher`] (constructed in
/// `pipeline::render`).
pub struct StubMermaidDispatcher;

impl MermaidDispatcher for StubMermaidDispatcher {
    fn render(&mut self, _src: &str) -> Result<MermaidRendered, MermaidDispatchError> {
        Err(MermaidDispatchError::UnsupportedDiagramType)
    }
}

/// Real dispatcher (W-ddc4e7). Routes fenced-`mermaid` source through
/// the in-tree `crate::mermaid::render` entry point, which:
/// - sniffs the first non-blank line for a diagram-type keyword;
/// - dispatches `flowchart`/`graph` to `crate::mermaid::flowchart` and
///   `sequenceDiagram` to `crate::mermaid::sequence`;
/// - returns `MermaidError::UnsupportedDiagramType` for the §E
///   deferred set (gantt, classDiagram, stateDiagram[-v2], erDiagram,
///   pie, journey, gitGraph, mindmap, timeline, quadrantChart,
///   requirementDiagram, C4Context, sankey-beta, xychart-beta,
///   block-beta);
/// - returns `MermaidError::UnknownDiagramType` for every other
///   leading token.
///
/// Successful renders produce SVG bytes that already carry an explicit
/// `width="<px>" height="<px>" viewBox="..."` on the root `<svg>`. The
/// dispatcher parses those attributes and converts pixels→points
/// (1 CSS px = 0.75 pt) so `md_image_bytes` gets a concrete length;
/// height is left `None` so the helper preserves aspect from the
/// width.
pub struct RealMermaidDispatcher;

impl MermaidDispatcher for RealMermaidDispatcher {
    fn render(&mut self, src: &str) -> Result<MermaidRendered, MermaidDispatchError> {
        match crate::mermaid::render(src) {
            Ok(svg_bytes) => {
                let width_pt = parse_svg_root_width_px(&svg_bytes)
                    .map(|px| px * 0.75)
                    .unwrap_or(360.0);
                Ok(MermaidRendered {
                    svg_bytes,
                    width_pt,
                    height_pt: None,
                })
            }
            Err(e) => Err(MermaidDispatchError::Other(e.to_string())),
        }
    }
}

/// Parse `width="<digits>"` from the root `<svg ...>` element. Returns
/// the pixel value as `f64` if found, else `None`. The mermaid
/// sub-renderers (`flowchart::emit`, `sequence`, `svg_buf`) all emit
/// the root `<svg>` as a single line with `width="..." height="..."
/// viewBox="..."`, so a tiny manual scan is sufficient — no regex/XML
/// parser dependency.
fn parse_svg_root_width_px(bytes: &[u8]) -> Option<f64> {
    let s = std::str::from_utf8(bytes).ok()?;
    let svg_idx = s.find("<svg")?;
    // Look only inside the root tag.
    let tag_end = s[svg_idx..].find('>').map(|n| svg_idx + n)?;
    let header = &s[svg_idx..tag_end];
    let attr_idx = header.find("width=\"")?;
    let after = &header[attr_idx + "width=\"".len()..];
    let close = after.find('"')?;
    after[..close].parse::<f64>().ok()
}

// -----------------------------------------------------------------------------
// EmitError.
// -----------------------------------------------------------------------------

/// Error returned by the emitter. Currently the emitter is total over
/// well-formed pulldown event streams — pulldown's parser does its own
/// recovery — so this variant set is mostly future-proofing.
#[derive(Debug, Clone)]
pub enum EmitError {
    /// pulldown-cmark cannot fail to parse (it recovers everything),
    /// but if a future hook surfaces a true parse error, route it here.
    MarkdownParse(String),
    /// Internal invariant violation. Should never fire — file a bug if
    /// it does.
    Internal(String),
}

impl std::fmt::Display for EmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmitError::MarkdownParse(s) => write!(f, "markdown parse: {}", s),
            EmitError::Internal(s) => write!(f, "emitter internal error: {}", s),
        }
    }
}

impl std::error::Error for EmitError {}

// -----------------------------------------------------------------------------
// Public entry point.
// -----------------------------------------------------------------------------

/// Parse `md` with the GFM-extension options pinned by D-c3af71 §A and
/// emit a Typst body string. The body is *not* yet composed onto the
/// theme preamble — that's `pipeline::render`'s job (so the same
/// emitter output can be used in tests with a stripped-down preamble).
pub fn emit_typst_body(
    md: &str,
    pipeline: &mut Pipeline,
    mermaid: &mut dyn MermaidDispatcher,
    warnings: &mut WarningCollector,
) -> Result<String, EmitError> {
    // §A — non-negotiable parser options.
    let opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM
        | Options::ENABLE_HEADING_ATTRIBUTES;

    let events: Vec<Event<'_>> = Parser::new_ext(md, opts).collect();

    // Pass 1 — render footnote definitions into a label→typst-body map.
    let footnotes = collect_footnotes(&events, pipeline, mermaid, warnings)?;

    // Pass 2 — main render, skipping footnote-definition blocks.
    let mut emitter = MarkdownEmitter::new(pipeline, mermaid, warnings, &footnotes);
    let mut iter = events.iter();
    while let Some(ev) = iter.next() {
        if let Event::Start(Tag::FootnoteDefinition(_)) = ev {
            // Skip the whole definition block (depth-counted to handle
            // nested footnote-like structures, even though pulldown
            // doesn't currently produce nested defs).
            let mut depth = 1usize;
            for inner in iter.by_ref() {
                match inner {
                    Event::Start(Tag::FootnoteDefinition(_)) => depth += 1,
                    Event::End(TagEnd::FootnoteDefinition) => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }
            continue;
        }
        emitter.handle(ev)?;
    }
    Ok(emitter.finish())
}

// Render every FootnoteDefinition block into a typst-content string.
fn collect_footnotes(
    events: &[Event<'_>],
    pipeline: &mut Pipeline,
    mermaid: &mut dyn MermaidDispatcher,
    warnings: &mut WarningCollector,
) -> Result<std::collections::HashMap<String, String>, EmitError> {
    let mut out = std::collections::HashMap::new();
    let mut i = 0;
    while i < events.len() {
        if let Event::Start(Tag::FootnoteDefinition(label)) = &events[i] {
            let label = label.to_string();
            // Find the matching End — depth-counted.
            let start = i + 1;
            let mut depth = 1usize;
            let mut j = start;
            while j < events.len() {
                match &events[j] {
                    Event::Start(Tag::FootnoteDefinition(_)) => depth += 1,
                    Event::End(TagEnd::FootnoteDefinition) => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                j += 1;
            }
            // Render events[start..j].
            // Use an isolated emitter with an empty footnote map (no
            // recursive footnote references; pulldown won't produce
            // them anyway — but be defensive).
            let empty: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            let mut sub = MarkdownEmitter::new(pipeline, mermaid, warnings, &empty);
            for ev in &events[start..j] {
                sub.handle(ev)?;
            }
            let body = sub.finish();
            out.insert(label, body);
            i = j + 1;
        } else {
            i += 1;
        }
    }
    Ok(out)
}

// -----------------------------------------------------------------------------
// MarkdownEmitter + EnvFrame stack.
// -----------------------------------------------------------------------------

/// One open construct on the emitter's stack. Each frame owns a buffer
/// that captures everything emitted between Start and End for that
/// construct; on End, the frame's `kind` decides how to fold the
/// captured body into the parent.
struct EnvFrame {
    kind: FrameKind,
    buf: String,
}

enum FrameKind {
    Paragraph,
    Heading {
        level: u32,
    },
    BlockQuote,
    Item {
        /// Set when a `TaskListMarker` event fires inside this Item.
        /// Prepended to the item content when the parent List is
        /// rendered.
        task_marker: Option<bool>,
    },
    List {
        ordered: bool,
        /// Each item's rendered Typst markup, with task-list prefix if
        /// any.
        items: Vec<String>,
    },
    TableCell,
    TableHead {
        cells: Vec<String>,
    },
    TableRow {
        cells: Vec<String>,
    },
    Table {
        alignments: Vec<Alignment>,
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Emphasis,
    Strong,
    Strikethrough,
    Link {
        dest: String,
    },
    Image {
        src: String,
        #[allow(dead_code)]
        title: String,
    },
    CodeBlock {
        lang: String,
    },
    FencedMermaid,
    HtmlBlock,
}

struct MarkdownEmitter<'a> {
    out: String,
    stack: Vec<EnvFrame>,
    pipeline: &'a mut Pipeline,
    mermaid: &'a mut dyn MermaidDispatcher,
    warnings: &'a mut WarningCollector,
    footnotes: &'a std::collections::HashMap<String, String>,
    /// How many image-pipeline warnings we've already drained into the
    /// shared collector. After every `pipeline.resolve(...)` call we
    /// pull the new ones over. Drain-after rather than threading a
    /// `&mut WarningCollector` into `Pipeline` itself — see the
    /// research-findings Outcome for rationale.
    image_warnings_drained: usize,
    /// Pagebreak directives recognized but not yet flushed (W-pagebreak,
    /// D-875e4b §1d, §2a, U-976c35). Each `<!-- pagebreak -->` /
    /// `<!-- page-break -->` HTML comment at top level increments this
    /// counter. The counter is flushed (emitted as N consecutive
    /// `#pagebreak()` invocations) at the next real-content top-level
    /// block emission, IFF `has_emitted_real_content` is true. At
    /// document end (`finish()`), any unflushed counter is silently
    /// dropped — this is the "trailing pagebreak suppressed" semantics
    /// from U-976c35.
    pending_pagebreaks: u32,
    /// Has any real-content top-level block been emitted yet? Gate for
    /// leading-pagebreak suppression (U-976c35 "at the very start: no-op").
    /// Set to true on the first call to `flush_pending_pagebreaks` (i.e.
    /// at the first real-content top-level block emission).
    has_emitted_real_content: bool,
    /// Stack of recognized inline-HTML formatters open in the current
    /// inline-content stream (D-875e4b §2h v2). Pushed when an
    /// `Event::InlineHtml` classifies as `OpenTag(Bold|Italic)`; popped
    /// when a matching `CloseTag` arrives. Drained at block-frame ends
    /// (Paragraph, Heading, Item, BlockQuote, TableCell) so unmatched
    /// openers auto-close — matches the "paragraph-level pairing only"
    /// rule (D-875e4b §2h.5).
    inline_html_stack: Vec<RecognizedInlineFormatter>,
}

impl<'a> MarkdownEmitter<'a> {
    fn new(
        pipeline: &'a mut Pipeline,
        mermaid: &'a mut dyn MermaidDispatcher,
        warnings: &'a mut WarningCollector,
        footnotes: &'a std::collections::HashMap<String, String>,
    ) -> Self {
        let drained = pipeline.warnings().len();
        Self {
            out: String::new(),
            stack: Vec::new(),
            pipeline,
            mermaid,
            warnings,
            footnotes,
            image_warnings_drained: drained,
            pending_pagebreaks: 0,
            has_emitted_real_content: false,
            inline_html_stack: Vec::new(),
        }
    }

    fn finish(mut self) -> String {
        // W-650a51 (D-875e4b §2h.5): drain any inline-HTML openers
        // still on the stack at end of document (defensive — block
        // frames usually drain first, but text outside any frame is
        // possible).
        self.drain_inline_html_stack();
        self.out
    }

    fn current_buf(&mut self) -> &mut String {
        match self.stack.last_mut() {
            Some(f) => &mut f.buf,
            None => &mut self.out,
        }
    }

    fn write(&mut self, s: &str) {
        self.current_buf().push_str(s);
    }

    fn push(&mut self, kind: FrameKind) {
        self.stack.push(EnvFrame {
            kind,
            buf: String::new(),
        });
    }

    fn drain_image_warnings(&mut self) {
        let all = self.pipeline.warnings();
        if all.len() > self.image_warnings_drained {
            // Bring the new ones across. Note the canonical stderr
            // line was already emitted by the image pipeline at the
            // moment the warning fired, so we *must not* re-emit. We
            // therefore push directly to the accumulator without going
            // through `WarningCollector::warn` (which would write to
            // stderr again). To do that without exposing internals,
            // we use `warn` with a no-op sink? No — simpler: bridge
            // through a dedicated collector method. We re-record the
            // structured form so the strict-gate (W#7') sees one
            // unified count.
            let new_slice = all[self.image_warnings_drained..].to_vec();
            self.image_warnings_drained = all.len();
            for w in new_slice {
                // The image pipeline already wrote to stderr; re-record
                // structured form silently.
                self.warnings.record_image_silently(w);
            }
        }
    }

    /// Apply one Markdown event. Errors propagate.
    fn handle(&mut self, ev: &Event<'_>) -> Result<(), EmitError> {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag_end) => self.end(tag_end),
            Event::Text(s) => {
                // Inside a code block / mermaid fence / html block,
                // text is raw — append unescaped to the buf. Outside,
                // escape for Typst markup.
                let raw = matches!(
                    self.stack.last().map(|f| &f.kind),
                    Some(FrameKind::CodeBlock { .. })
                        | Some(FrameKind::FencedMermaid)
                        | Some(FrameKind::HtmlBlock)
                );
                if raw {
                    self.write(s);
                } else {
                    let esc = escape_typst_markup(s);
                    self.write(&esc);
                }
                Ok(())
            }
            Event::Code(s) => {
                self.write(&format!("#md_inline_code({})", typst_string(s)));
                Ok(())
            }
            Event::Html(s) => {
                // Block-level html inside HtmlBlock; render via raw
                // pass-through. We accumulate Text events for the body
                // already; some pulldown versions emit Event::Html
                // instead of Event::Text inside HtmlBlock. Treat it
                // like raw text into the current buf.
                let in_html_block = matches!(
                    self.stack.last().map(|f| &f.kind),
                    Some(FrameKind::HtmlBlock)
                );
                if in_html_block {
                    self.write(s);
                } else if is_pagebreak_comment(s) && self.stack.is_empty() {
                    // W-pagebreak (D-875e4b §1c, §1d, §6c, U-976c35):
                    // pulldown sometimes emits a standalone block-html
                    // comment as a single Event::Html outside an
                    // HtmlBlock frame. Same recognition rules apply at
                    // the second site, with the same top-level
                    // constraint. Inside any open frame, fall through
                    // to the literal pass-through below.
                    self.record_pagebreak();
                } else {
                    // Stray block-html fragment — render as raw.
                    self.write_block(&format!(
                        "#md_inline_html({})\n\n",
                        typst_string(s)
                    ));
                }
                Ok(())
            }
            Event::InlineHtml(s) => {
                // W-650a51 (D-875e4b §2h v2 + U-ad8c6c v2): outside-cell
                // inline-HTML recognition. Classify the tag against the
                // 5-element subset (`<b>` `<strong>` `<i>` `<em>` `<br>`);
                // unrecognized payloads fall through to the existing
                // raw-pass-through. Pairing is paragraph-level only
                // (§2h.5): `inline_html_stack` is drained at block-frame
                // ends so unmatched openers auto-close gracefully.
                match try_classify_inline_html(s) {
                    InlineHtmlKind::OpenTag(formatter) => {
                        self.inline_html_stack.push(formatter);
                        self.write(formatter.typst_open());
                    }
                    InlineHtmlKind::CloseTag(formatter) => {
                        // Per §6k: only pop when the top of the stack
                        // matches the close-tag's formatter. Otherwise
                        // (mismatched/dangling close) fall through to
                        // raw-pass-through — the open marker stays
                        // on the stack and will auto-close at the
                        // block-frame drain.
                        if self.inline_html_stack.last() == Some(&formatter) {
                            self.inline_html_stack.pop();
                            self.write(formatter.typst_close());
                        } else {
                            self.write(&format!(
                                "#md_inline_html({})",
                                typst_string(s)
                            ));
                        }
                    }
                    InlineHtmlKind::SelfClosingBr => {
                        self.write("#md_hardbreak()");
                    }
                    InlineHtmlKind::Unrecognized => {
                        self.write(&format!(
                            "#md_inline_html({})",
                            typst_string(s)
                        ));
                    }
                }
                Ok(())
            }
            Event::FootnoteReference(label) => {
                let body = self
                    .footnotes
                    .get(label.as_ref())
                    .cloned()
                    .unwrap_or_else(|| {
                        // Reference with no matching definition. Emit a
                        // minimal placeholder rather than dropping;
                        // pulldown should never produce this, but be
                        // defensive.
                        format!("[{}]", escape_typst_markup(label))
                    });
                let body = body.trim().to_string();
                self.write(&format!("#md_footnote[{}]", body));
                Ok(())
            }
            Event::SoftBreak => {
                self.write(" ");
                Ok(())
            }
            Event::HardBreak => {
                self.write("#md_hardbreak()");
                Ok(())
            }
            Event::Rule => {
                self.write_block(
                    "#line(length: 100%, stroke: 0.5pt + rgb(\"#888888\"))\n\n",
                );
                Ok(())
            }
            Event::TaskListMarker(checked) => {
                // Set on the current Item frame; rendering happens when
                // the List frame closes.
                if let Some(f) = self.stack.last_mut() {
                    if let FrameKind::Item { task_marker } = &mut f.kind {
                        *task_marker = Some(*checked);
                    }
                }
                Ok(())
            }
            Event::DisplayMath(s) | Event::InlineMath(s) => {
                // ENABLE_MATH is not in the v1 option set — these
                // events shouldn't fire — but render as raw if they do.
                self.write(&format!("#md_inline_code({})", typst_string(s)));
                Ok(())
            }
        }
    }

    fn start(&mut self, tag: &Tag<'_>) -> Result<(), EmitError> {
        match tag {
            Tag::Paragraph => self.push(FrameKind::Paragraph),
            Tag::Heading { level, .. } => {
                let n = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                self.push(FrameKind::Heading { level: n });
            }
            Tag::BlockQuote(_) => self.push(FrameKind::BlockQuote),
            Tag::CodeBlock(kind) => {
                let lang = match kind {
                    CodeBlockKind::Indented => String::new(),
                    CodeBlockKind::Fenced(info) => info
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .to_lowercase(),
                };
                if lang == "mermaid" {
                    self.push(FrameKind::FencedMermaid);
                } else {
                    self.push(FrameKind::CodeBlock { lang });
                }
            }
            Tag::HtmlBlock => self.push(FrameKind::HtmlBlock),
            Tag::List(start) => self.push(FrameKind::List {
                ordered: start.is_some(),
                items: Vec::new(),
            }),
            Tag::Item => self.push(FrameKind::Item { task_marker: None }),
            Tag::FootnoteDefinition(_) => {
                // Pass 2 skips these; if we get one here, push a
                // throwaway frame so balanced End pops cleanly.
                self.push(FrameKind::Paragraph);
            }
            Tag::DefinitionList | Tag::DefinitionListTitle | Tag::DefinitionListDefinition => {
                // Not in the v1 GFM extension surface (definition
                // lists require ENABLE_DEFINITION_LIST). If a future
                // option turn-on lets these through, treat the title
                // like a paragraph and the definition like a
                // blockquote so content isn't dropped.
                match tag {
                    Tag::DefinitionListTitle => self.push(FrameKind::Paragraph),
                    Tag::DefinitionListDefinition => self.push(FrameKind::BlockQuote),
                    _ => self.push(FrameKind::Paragraph),
                }
            }
            Tag::Table(alignments) => self.push(FrameKind::Table {
                alignments: alignments.clone(),
                headers: Vec::new(),
                rows: Vec::new(),
            }),
            Tag::TableHead => self.push(FrameKind::TableHead { cells: Vec::new() }),
            Tag::TableRow => self.push(FrameKind::TableRow { cells: Vec::new() }),
            Tag::TableCell => self.push(FrameKind::TableCell),
            Tag::Emphasis => self.push(FrameKind::Emphasis),
            Tag::Strong => self.push(FrameKind::Strong),
            Tag::Strikethrough => self.push(FrameKind::Strikethrough),
            Tag::Superscript | Tag::Subscript => {
                // Out of v1 surface; treat as plain wrapper.
                self.push(FrameKind::Emphasis);
            }
            Tag::Link { dest_url, .. } => self.push(FrameKind::Link {
                dest: dest_url.to_string(),
            }),
            Tag::Image {
                dest_url, title, ..
            } => self.push(FrameKind::Image {
                src: dest_url.to_string(),
                title: title.to_string(),
            }),
            Tag::MetadataBlock(_) => {
                // Not enabled in the v1 option set; if it ever fires,
                // discard via a throwaway frame.
                self.push(FrameKind::Paragraph);
            }
        }
        Ok(())
    }

    fn end(&mut self, _tag_end: &TagEnd) -> Result<(), EmitError> {
        // W-650a51 (D-875e4b §2h.5): drain unmatched inline-HTML openers
        // at block-frame boundaries so each paragraph stays balanced.
        // Inline frames (Strong, Emphasis, Strikethrough, Link, Image,
        // table-row containers) do NOT drain — the opener legitimately
        // spans across them.
        let kind_drains = self
            .stack
            .last()
            .map(|f| frame_kind_drains_inline_html(&f.kind))
            .unwrap_or(false);
        if kind_drains {
            self.drain_inline_html_stack();
        }

        let frame = self.stack.pop().ok_or_else(|| {
            EmitError::Internal("unbalanced End event with empty stack".to_string())
        })?;
        let body = frame.buf;
        match frame.kind {
            FrameKind::Paragraph => {
                let trimmed = body.trim();
                if !trimmed.is_empty() {
                    self.write_block(&format!("{}\n\n", trimmed));
                }
            }
            FrameKind::Heading { level } => {
                let eq = "=".repeat(level as usize);
                self.write_block(&format!("{} {}\n\n", eq, body.trim()));
            }
            FrameKind::BlockQuote => {
                self.write_block(&format!("#md_blockquote[\n{}\n]\n\n", body.trim()));
            }
            FrameKind::Item { task_marker } => {
                let prefix = match task_marker {
                    Some(true) => "#md_task_checked() ",
                    Some(false) => "#md_task_unchecked() ",
                    None => "",
                };
                let content = format!("{}{}", prefix, body.trim());
                if let Some(parent) = self.stack.last_mut() {
                    if let FrameKind::List { items, .. } = &mut parent.kind {
                        items.push(content);
                    }
                }
            }
            FrameKind::List { ordered, items } => {
                let func = if ordered { "enum" } else { "list" };
                let mut s = format!("#{}(", func);
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push('[');
                    s.push_str(it);
                    s.push(']');
                }
                s.push_str(")\n\n");
                self.write_block(&s);
            }
            FrameKind::TableCell => {
                if let Some(parent) = self.stack.last_mut() {
                    match &mut parent.kind {
                        FrameKind::TableHead { cells } | FrameKind::TableRow { cells } => {
                            cells.push(body.trim().to_string());
                        }
                        _ => {}
                    }
                }
            }
            FrameKind::TableHead { cells } => {
                if let Some(parent) = self.stack.last_mut() {
                    if let FrameKind::Table { headers, .. } = &mut parent.kind {
                        *headers = cells;
                    }
                }
            }
            FrameKind::TableRow { cells } => {
                if let Some(parent) = self.stack.last_mut() {
                    if let FrameKind::Table { rows, .. } = &mut parent.kind {
                        rows.push(cells);
                    }
                }
            }
            FrameKind::Table {
                alignments,
                headers,
                rows,
            } => {
                let aligns: Vec<&'static str> = alignments
                    .iter()
                    .map(|a| match a {
                        Alignment::Left | Alignment::None => "\"left\"",
                        Alignment::Center => "\"center\"",
                        Alignment::Right => "\"right\"",
                    })
                    .collect();
                let hdr = headers
                    .iter()
                    .map(|c| format!("[{}]", c))
                    .collect::<Vec<_>>()
                    .join(", ");
                let rows_s = rows
                    .iter()
                    .map(|row| {
                        let cells = row
                            .iter()
                            .map(|c| format!("[{}]", c))
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("({},)", cells)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let s = format!(
                    "#md_table(({},), ({},), ({},))\n\n",
                    hdr,
                    aligns.join(", "),
                    rows_s
                );
                self.write_block(&s);
            }
            FrameKind::Emphasis => self.write(&format!("_{}_", body)),
            FrameKind::Strong => self.write(&format!("*{}*", body)),
            FrameKind::Strikethrough => self.write(&format!("#md_strike[{}]", body)),
            FrameKind::Link { dest } => {
                self.write(&format!(
                    "#md_link({}, [{}])",
                    typst_string(&dest),
                    body
                ));
            }
            FrameKind::Image { src, title: _ } => {
                let alt = strip_for_alt(&body);
                let req = ImageRequest {
                    src: &src,
                    alt: &alt,
                    explicit_width: None,
                    explicit_height: None,
                };
                let resolved = self.pipeline.resolve(&req);
                self.drain_image_warnings();
                let snippet = match resolved {
                    ResolvedImage::Embedded {
                        format,
                        bytes,
                        size,
                        ..
                    } => {
                        let format_str = match format {
                            EmbeddedFormat::Png => "png",
                            EmbeddedFormat::Jpeg => "jpeg",
                            EmbeddedFormat::Svg => "svg",
                        };
                        let bytes_lit = typst_bytes_literal(&bytes);
                        let height = match size.height_pt {
                            Some(h) => format!("{}pt", h),
                            None => "none".to_string(),
                        };
                        format!(
                            "#md_image_bytes({}, \"{}\", {}pt, {})",
                            bytes_lit, format_str, size.width_pt, height
                        )
                    }
                    ResolvedImage::Placeholder { .. } => {
                        format!("#md_image({})", typst_string(&src))
                    }
                };
                self.write(&snippet);
            }
            FrameKind::CodeBlock { lang } => {
                // Strip the trailing newline pulldown adds to the last
                // text chunk so md_codeblock doesn't render a blank
                // line at the bottom of every code box.
                let code = trim_trailing_newline(&body);
                self.write_block(&format!(
                    "#md_codeblock({}, {})\n\n",
                    typst_string(&lang),
                    typst_string(code)
                ));
            }
            FrameKind::FencedMermaid => {
                let src = body.clone();
                match self.mermaid.render(&src) {
                    Ok(rendered) => {
                        let bytes_lit = typst_bytes_literal(&rendered.svg_bytes);
                        let height = match rendered.height_pt {
                            Some(h) => format!("{}pt", h),
                            None => "none".to_string(),
                        };
                        self.write_block(&format!(
                            "#md_image_bytes({}, \"svg\", {}pt, {})\n\n",
                            bytes_lit, rendered.width_pt, height
                        ));
                    }
                    Err(e) => {
                        self.warnings.warn(
                            WarningSource::Mermaid,
                            format!("could not render diagram: {}", e),
                        );
                        self.write_block(&format!(
                            "#md_mermaid_stub({})\n\n",
                            typst_string(&src)
                        ));
                    }
                }
            }
            FrameKind::HtmlBlock => {
                let raw = body.trim();
                if raw.is_empty() {
                    // unchanged: drop empty html block
                } else if is_pagebreak_comment(raw) && self.stack.is_empty() {
                    // W-pagebreak (D-875e4b §1c, §1d, §6c, U-976c35):
                    // recognized pagebreak comment AT TOP LEVEL.
                    // `self.stack.is_empty()` after the HtmlBlock pop
                    // means there was no parent frame other than the
                    // (now-popped) HtmlBlock; this is the
                    // "top-level only" constraint per Decision §1c.
                    // If recognition succeeded but parent context is
                    // non-top-level (inside Item, BlockQuote, TableCell,
                    // or any nested frame), control falls through to
                    // the `md_inline_html` literal pass-through below
                    // — no warning emitted (graceful per §6c).
                    self.record_pagebreak();
                } else {
                    // W-650a51 (D-875e4b §1, §2): try to parse as an
                    // HTML <table>. Three outcomes:
                    //   - NotATable: fall through to existing
                    //     `md_inline_html` raw pass-through silently.
                    //   - Parsed(table): emit Typst via
                    //     `emit_html_table_typst`. Drain any image
                    //     warnings the emit pushed onto `self.pipeline`.
                    //   - ParseFailed(reason): record an Emitter-bucket
                    //     warning and fall through to raw pass-through
                    //     so the offending HTML is still visible.
                    match try_parse_html_table(raw) {
                        ParseOutcome::NotATable => {
                            self.write_block(&format!(
                                "#md_inline_html({})\n\n",
                                typst_string(raw)
                            ));
                        }
                        ParseOutcome::Parsed(table) => {
                            let typst_src = emit_html_table_typst(
                                &table,
                                self.pipeline,
                            );
                            self.drain_image_warnings();
                            self.write_block(&format!("{}\n\n", typst_src));
                        }
                        ParseOutcome::ParseFailed(reason) => {
                            self.warnings.warn(
                                WarningSource::Emitter,
                                format!(
                                    "html table parse failed: {}",
                                    reason
                                ),
                            );
                            self.write_block(&format!(
                                "#md_inline_html({})\n\n",
                                typst_string(raw)
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Write a block-level construct. If the current target is a
    /// content collector (table cell, item, blockquote, etc.) we just
    /// route to the buf same as inline; the calling site already
    /// suffix-newlines for top-level emission.
    ///
    /// W-pagebreak (D-875e4b §1d, U-976c35): when we're about to emit a
    /// real-content top-level block (stack is empty), first flush any
    /// pending pagebreaks. Inside a child frame's buf, no flush — pagebreaks
    /// only fire at top level and the stack will return to empty before
    /// the next top-level block.
    fn write_block(&mut self, s: &str) {
        if self.stack.is_empty() {
            self.flush_pending_pagebreaks();
        }
        self.write(s);
    }

    /// Record a recognized pagebreak directive (`<!-- pagebreak -->` or
    /// `<!-- page-break -->` at top level). Increments the pending
    /// counter; the actual emit (or discard, for leading) happens at
    /// the next call to `flush_pending_pagebreaks`. See Decision
    /// D-875e4b §1d and U-976c35 for the leading/trailing/back-to-back
    /// semantics.
    fn record_pagebreak(&mut self) {
        self.pending_pagebreaks = self.pending_pagebreaks.saturating_add(1);
    }

    /// Flush pending pagebreaks (D-875e4b §1d, U-976c35).
    ///
    /// Behavior matrix (see done_definition #4):
    /// - **Leading** pagebreaks (no real content emitted yet,
    ///   `has_emitted_real_content == false`): skip emit, set the flag,
    ///   reset the counter. → leading pagebreaks discarded.
    /// - **Mid-document** pagebreaks (`has_emitted_real_content == true`,
    ///   `pending_pagebreaks > 0`): emit `pending_pagebreaks` literal
    ///   `#pagebreak()` invocations, reset the counter. → preserves
    ///   back-to-back-pagebreak semantics (each comment renders as one
    ///   empty page between content).
    /// - **Trailing** pagebreaks: this function is never called after
    ///   the last real top-level block emission, so the counter is
    ///   silently dropped at `finish()`. → trailing pagebreaks
    ///   discarded.
    /// - **Pagebreak as the only document content**: leading + trailing
    ///   suppression both apply; the document is effectively empty (no
    ///   `#pagebreak()` emitted).
    ///
    /// Always sets `has_emitted_real_content = true`: the very act of
    /// calling this function from `write_block` at top level is the
    /// signal that a real-content block is about to be emitted.
    fn flush_pending_pagebreaks(&mut self) {
        if self.has_emitted_real_content && self.pending_pagebreaks > 0 {
            for _ in 0..self.pending_pagebreaks {
                self.out.push_str("#pagebreak()\n\n");
            }
        }
        self.pending_pagebreaks = 0;
        self.has_emitted_real_content = true;
    }

    /// W-650a51 (D-875e4b §2h.5): pop every open inline-HTML formatter
    /// from `inline_html_stack`, writing its close marker into the
    /// current frame's buffer. Called at block-frame ends (Paragraph,
    /// Heading, Item, BlockQuote, TableCell) and at `finish()` so any
    /// unbalanced `<b>`/`<i>` openers auto-close — the document
    /// remains compilable per "honest fail-soft" (§6k).
    fn drain_inline_html_stack(&mut self) {
        while let Some(f) = self.inline_html_stack.pop() {
            let close = f.typst_close().to_string();
            self.write(&close);
        }
    }
}

/// W-650a51 (D-875e4b §2h.5): does this frame's End act as a
/// block-pairing boundary for inline-HTML formatters? `true` for the
/// "block-ish" frames (Paragraph, Heading, Item, BlockQuote,
/// TableCell). `false` for inline frames (Strong, Emphasis,
/// Strikethrough, Link, Image), List/Table containers (their child
/// ends already drained), CodeBlock/Mermaid/HtmlBlock (raw text only).
fn frame_kind_drains_inline_html(kind: &FrameKind) -> bool {
    matches!(
        kind,
        FrameKind::Paragraph
            | FrameKind::Heading { .. }
            | FrameKind::Item { .. }
            | FrameKind::BlockQuote
            | FrameKind::TableCell
    )
}

// -----------------------------------------------------------------------------
// Pagebreak directive recognition.
// -----------------------------------------------------------------------------

/// Does `raw` recognize as the pagebreak HTML-comment directive?
///
/// Per U-976c35 and D-875e4b §1a:
///
/// - The input must be a single HTML comment (`<!-- ... -->`), tolerant
///   of leading/trailing whitespace **outside** the comment markers.
/// - The payload (the content between `<!--` and `-->`) is trimmed of
///   whitespace and ASCII-lower-cased.
/// - The lowered payload must be **exactly** `pagebreak` or `page-break`.
/// - Any payload-bearing form is rejected (e.g. `<!-- pagebreak: top -->`,
///   `<!-- pagebreaks -->`, `<!-- TODO pagebreak here -->`,
///   `<!-- pagebreak extra -->`). The recognition is strict-no-payload
///   per U-976c35.
///
/// NOTE: lives in `src/emitter.rs` rather than `src/html/pagebreak.rs`
/// per D-875e4b §1f — `src/html/` does not yet exist at the time of
/// this Work landing. W-html-table will introduce `src/html/` and may
/// move this helper.
pub(crate) fn is_pagebreak_comment(raw: &str) -> bool {
    let s = raw.trim();
    if !(s.starts_with("<!--") && s.ends_with("-->") && s.len() >= 7) {
        return false;
    }
    let inner = &s[4..s.len() - 3];
    let lowered = inner.trim().to_ascii_lowercase();
    lowered == "pagebreak" || lowered == "page-break"
}

// -----------------------------------------------------------------------------
// Escaping helpers.
// -----------------------------------------------------------------------------

/// Escape a string for inclusion in Typst *markup* (content blocks /
/// document body). Each metacharacter is preceded by a backslash so it
/// renders as the literal glyph.
pub(crate) fn escape_typst_markup(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' | '*' | '_' | '#' | '[' | ']' | '<' | '>' | '@' | '~' | '`' | '$' | '='
            | '/' | '-' => {
                // Conservative: escape every Typst markup
                // metacharacter. The cost is one extra byte per
                // potentially-meta char in body text — fine.
                o.push('\\');
                o.push(c);
            }
            _ => o.push(c),
        }
    }
    o
}

/// Render `s` as a Typst string literal: `"..."` with `\` and `"`
/// escaped, plus newline / tab to `\n` / `\t` so the literal stays on
/// one logical line (Typst itself is fine with multi-line string
/// literals, but tests find single-line forms easier to grep).
pub(crate) fn typst_string(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            _ => o.push(c),
        }
    }
    o.push('"');
    o
}

/// Render bytes as a Typst `bytes((b1, b2, ...))` literal. Used for
/// embedding decoded image bytes in the output.
pub(crate) fn typst_bytes_literal(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 5 + 16);
    s.push_str("bytes((");
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&b.to_string());
    }
    s.push_str(",))");
    s
}

/// The image alt-text is collected as Typst markup (formatted child
/// text). For the *image-pipeline* call we want a plain-text form (no
/// backslash escapes, no `_`/`*` decorations). Strip the cheap forms.
fn strip_for_alt(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                o.push(next);
                chars.next();
            }
        } else {
            o.push(c);
        }
    }
    o
}

fn trim_trailing_newline(s: &str) -> &str {
    if let Some(stripped) = s.strip_suffix('\n') {
        stripped
    } else {
        s
    }
}

// -----------------------------------------------------------------------------
// Tests.
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_pipeline::PipelineBuilder;
    use crate::warnings::CapturingSink;

    fn make_pipeline() -> Pipeline {
        PipelineBuilder::new(std::env::temp_dir())
            .warn_sink(Box::new(crate::image_pipeline::CapturingSink::new()))
            .build()
    }

    fn make_warnings() -> WarningCollector {
        WarningCollector::with_sink(Box::new(CapturingSink::new()))
    }

    #[test]
    fn heading_emits_typst_equals() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let out = emit_typst_body("# Hello\n", &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("= Hello"), "got: {out}");
    }

    #[test]
    fn paragraph_text_is_escaped() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let out = emit_typst_body("a *literal* star: \\*", &mut p, &mut m, &mut w).unwrap();
        // The Markdown renders `a literal star: *` (italic on
        // "literal"), so the leading `a` followed by an emphasis open
        // marker is what we expect, plus the escaped trailing `*`.
        assert!(out.contains("_literal_"), "got: {out}");
    }

    #[test]
    fn fenced_mermaid_falls_back_to_stub_with_warning() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "```mermaid\nflowchart TD\nA-->B\n```\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("md_mermaid_stub"), "got: {out}");
        assert_eq!(w.count(), 1);
    }

    #[test]
    fn fenced_code_uses_md_codeblock() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "```rust\nfn main() {}\n```\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("md_codeblock(\"rust\""), "got: {out}");
    }

    #[test]
    fn task_list_renders_task_helpers() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "- [x] done\n- [ ] todo\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("md_task_checked"), "got: {out}");
        assert!(out.contains("md_task_unchecked"), "got: {out}");
    }

    #[test]
    fn link_uses_md_link() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let out = emit_typst_body("[here](https://x.test)", &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("md_link(\"https://x.test\""), "got: {out}");
    }

    #[test]
    fn footnote_reference_resolves_to_md_footnote() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "Here[^a].\n\n[^a]: the footnote body\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("md_footnote["), "got: {out}");
        assert!(out.contains("the footnote body"), "got: {out}");
    }

    #[test]
    fn strikethrough_uses_md_strike() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let out = emit_typst_body("~~gone~~", &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("md_strike[gone]"), "got: {out}");
    }

    #[test]
    fn table_uses_md_table() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "| h1 | h2 |\n|:--|--:|\n| a | b |\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("md_table"), "got: {out}");
        assert!(out.contains("\"left\""), "got: {out}");
        assert!(out.contains("\"right\""), "got: {out}");
    }

    #[test]
    fn autolink_yields_link() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let out = emit_typst_body("<https://x.test>", &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("md_link"), "got: {out}");
    }

    #[test]
    fn typst_bytes_literal_produces_compileable_form() {
        let s = typst_bytes_literal(&[1u8, 2, 3]);
        assert_eq!(s, "bytes((1,2,3,))");
    }

    // -------------------------------------------------------------------------
    // W-pagebreak — tests for the pagebreak HTML-comment directive
    // (D-875e4b §1a–§1d, §2a, §6c, §7a, U-976c35).
    // -------------------------------------------------------------------------

    // Helper-level: `is_pagebreak_comment` recognition rules.

    #[test]
    fn is_pagebreak_recognizes_canonical_form() {
        assert!(is_pagebreak_comment("<!-- pagebreak -->"));
    }

    #[test]
    fn is_pagebreak_recognizes_no_inner_whitespace() {
        assert!(is_pagebreak_comment("<!--pagebreak-->"));
    }

    #[test]
    fn is_pagebreak_recognizes_uppercase() {
        assert!(is_pagebreak_comment("<!-- PAGEBREAK -->"));
    }

    #[test]
    fn is_pagebreak_recognizes_hyphenated_form() {
        assert!(is_pagebreak_comment("<!-- page-break -->"));
    }

    #[test]
    fn is_pagebreak_recognizes_mixed_case_hyphenated() {
        assert!(is_pagebreak_comment("<!-- Page-Break -->"));
    }

    #[test]
    fn is_pagebreak_recognizes_extra_inner_whitespace() {
        assert!(is_pagebreak_comment("<!--   pagebreak   -->"));
    }

    #[test]
    fn is_pagebreak_recognizes_with_outer_whitespace() {
        // Trim outside the comment is also tolerated (allows recognition
        // when the buffer carries surrounding pulldown whitespace).
        assert!(is_pagebreak_comment("  <!-- pagebreak -->  "));
        assert!(is_pagebreak_comment("\n<!-- pagebreak -->\n"));
    }

    #[test]
    fn is_pagebreak_rejects_payload_with_colon() {
        // U-976c35 strict-no-payload contract.
        assert!(!is_pagebreak_comment("<!-- pagebreak: top -->"));
    }

    #[test]
    fn is_pagebreak_rejects_plural() {
        assert!(!is_pagebreak_comment("<!-- pagebreaks -->"));
    }

    #[test]
    fn is_pagebreak_rejects_extra_text_before() {
        assert!(!is_pagebreak_comment("<!-- not a pagebreak -->"));
    }

    #[test]
    fn is_pagebreak_rejects_extra_text_after() {
        assert!(!is_pagebreak_comment("<!-- pagebreak extra -->"));
    }

    #[test]
    fn is_pagebreak_rejects_plain_comment() {
        assert!(!is_pagebreak_comment("<!-- comment -->"));
    }

    #[test]
    fn is_pagebreak_rejects_empty_comment() {
        assert!(!is_pagebreak_comment("<!---->"));
        assert!(!is_pagebreak_comment("<!-- -->"));
    }

    #[test]
    fn is_pagebreak_rejects_non_comment() {
        assert!(!is_pagebreak_comment("pagebreak"));
        assert!(!is_pagebreak_comment("<p>pagebreak</p>"));
        assert!(!is_pagebreak_comment("<!-- pagebreak"));
        assert!(!is_pagebreak_comment("pagebreak -->"));
    }

    #[test]
    fn is_pagebreak_rejects_underscore_form() {
        assert!(!is_pagebreak_comment("<!-- page_break -->"));
    }

    // End-to-end: pagebreak emission via the emitter.

    #[test]
    fn pagebreak_mid_document_emits_typst_pagebreak() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "before\n\n<!-- pagebreak -->\n\nafter\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(
            out.contains("#pagebreak()"),
            "expected #pagebreak() in output:\n{out}"
        );
        // Must NOT pass through as md_inline_html.
        assert!(
            !out.contains("md_inline_html(\"<!-- pagebreak -->\")"),
            "pagebreak comment should not pass through as md_inline_html:\n{out}"
        );
    }

    #[test]
    fn pagebreak_recognizes_page_break_alias() {
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "before\n\n<!-- page-break -->\n\nafter\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("#pagebreak()"), "got: {out}");
    }

    #[test]
    fn pagebreak_at_document_start_is_suppressed() {
        // U-976c35: "At the very start of the document: no-op."
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "<!-- pagebreak -->\n\nfirst content\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(
            !out.contains("#pagebreak()"),
            "leading pagebreak must be suppressed:\n{out}"
        );
    }

    #[test]
    fn pagebreak_at_document_end_is_suppressed() {
        // U-976c35: "At the very end of the document: no-op."
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "last content\n\n<!-- pagebreak -->\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(
            !out.contains("#pagebreak()"),
            "trailing pagebreak must be suppressed:\n{out}"
        );
    }

    #[test]
    fn pagebreak_back_to_back_emits_two() {
        // U-976c35: "Back-to-back: each directive inserts a page break;
        // honor it literally rather than collapsing."
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md =
            "alpha\n\n<!-- pagebreak -->\n\n<!-- pagebreak -->\n\nbravo\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        // Count #pagebreak() occurrences.
        let count = out.matches("#pagebreak()").count();
        assert_eq!(
            count, 2,
            "expected 2 #pagebreak() invocations, got {count}:\n{out}"
        );
    }

    #[test]
    fn pagebreak_only_document_is_empty() {
        // Decision §6c edge case (4): pagebreak as the only content →
        // leading + trailing both apply; effectively empty.
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "<!-- pagebreak -->\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(
            !out.contains("#pagebreak()"),
            "a pagebreak-only document should emit no #pagebreak():\n{out}"
        );
    }

    #[test]
    fn pagebreak_inside_blockquote_falls_through() {
        // Decision §1c, §6c (edge case 2): pagebreak inside a
        // blockquote is non-top-level → fall through to md_inline_html;
        // no #pagebreak() emitted.
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        // Use HTML-block-like form inside the quote so pulldown emits
        // it as raw inline-html within the blockquote.
        let md = "> quoted text\n>\n> <!-- pagebreak -->\n>\n> more quoted\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(
            !out.contains("#pagebreak()"),
            "pagebreak inside blockquote must NOT emit #pagebreak():\n{out}"
        );
    }

    #[test]
    fn pagebreak_inside_fenced_code_block_is_text() {
        // Decision §6c (edge case 1) / §6g: pagebreak inside fenced
        // code is Event::Text, not Event::Html / HtmlBlock. Recognition
        // never fires. Comment renders as code source.
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "before\n\n```\n<!-- pagebreak -->\n```\n\nafter\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(
            !out.contains("#pagebreak()"),
            "pagebreak inside code fence must NOT emit #pagebreak():\n{out}"
        );
        // The comment text must appear inside an md_codeblock(...) call.
        assert!(
            out.contains("md_codeblock"),
            "expected md_codeblock in output:\n{out}"
        );
    }

    #[test]
    fn pagebreak_inside_list_item_falls_through() {
        // Decision §1c: pagebreak inside a list item is non-top-level
        // → falls through. Note: depending on pulldown's html-block
        // discrimination, the comment may not even reach HtmlBlock
        // recognition; the assertion is symmetric: NO #pagebreak()
        // emitted from a non-top-level position.
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "- item one\n\n  <!-- pagebreak -->\n\n- item two\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(
            !out.contains("#pagebreak()"),
            "pagebreak inside list item must NOT emit #pagebreak():\n{out}"
        );
    }

    #[test]
    fn pagebreak_payload_form_falls_through_to_inline_html() {
        // U-976c35 strict-no-payload: <!-- pagebreak: top --> is NOT
        // recognized; renders as raw md_inline_html.
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "before\n\n<!-- pagebreak: top -->\n\nafter\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(
            !out.contains("#pagebreak()"),
            "payload-bearing pagebreak comment must NOT emit #pagebreak():\n{out}"
        );
        assert!(
            out.contains("md_inline_html"),
            "expected md_inline_html fallback for unrecognized comment:\n{out}"
        );
    }

    #[test]
    fn pagebreak_case_insensitive_uppercase() {
        // U-976c35: matching is case-insensitive.
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "alpha\n\n<!-- PAGEBREAK -->\n\nbeta\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(
            out.contains("#pagebreak()"),
            "uppercase PAGEBREAK should be recognized:\n{out}"
        );
    }

    #[test]
    fn pagebreak_preserves_surrounding_content() {
        // Ensure surrounding paragraphs are not damaged by the
        // recognition path.
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "alpha\n\n<!-- pagebreak -->\n\nbravo\n";
        let out = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert!(out.contains("alpha"), "missing alpha:\n{out}");
        assert!(out.contains("bravo"), "missing bravo:\n{out}");
        assert!(out.contains("#pagebreak()"), "missing pagebreak:\n{out}");
        // Order: alpha BEFORE pagebreak BEFORE bravo.
        let i_a = out.find("alpha").unwrap();
        let i_p = out.find("#pagebreak()").unwrap();
        let i_b = out.find("bravo").unwrap();
        assert!(i_a < i_p && i_p < i_b, "wrong order:\n{out}");
    }

    #[test]
    fn pagebreak_no_warning_emitted() {
        // U-976c35: "Strict-mode interaction (U-6173fb): none expected."
        // Pagebreak recognition never produces a warning.
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "before\n\n<!-- pagebreak -->\n\nafter\n";
        let _ = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert_eq!(w.count(), 0, "pagebreak must not produce warnings");
    }

    #[test]
    fn pagebreak_inside_blockquote_no_warning() {
        // Decision §6c: graceful fall-through, no warning emitted.
        let mut p = make_pipeline();
        let mut m = StubMermaidDispatcher;
        let mut w = make_warnings();
        let md = "> quoted\n>\n> <!-- pagebreak -->\n";
        let _ = emit_typst_body(md, &mut p, &mut m, &mut w).unwrap();
        assert_eq!(
            w.count(),
            0,
            "non-top-level pagebreak must not produce warnings"
        );
    }
}
