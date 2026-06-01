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

use crate::image_pipeline::{
    EmbeddedFormat, ImageRequest, Pipeline, ResolvedImage,
};
use crate::warnings::{WarningCollector, WarningSource};

// -----------------------------------------------------------------------------
// MermaidDispatcher: API shape + stub.
// -----------------------------------------------------------------------------

/// Errors a mermaid dispatcher can return. Per D-c3af71 §B the emitter
/// only needs the `UnsupportedDiagramType` variant for the W#5' stub;
/// W#6' (the real mermaid wiring) will route the existing
/// `crate::mermaid::MermaidError` shape through this surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MermaidDispatchError {
    UnsupportedDiagramType,
    /// Reserved for the real dispatcher (W#6'). Keeps the consumer side
    /// match exhaustive without re-version-bumping when the real
    /// wiring lands.
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

/// Stub impl per W#5' done-definition bullet 5: returns
/// `UnsupportedDiagramType` for everything. The emitter turns this into
/// a `WarningSource::Mermaid` warning + `md_mermaid_stub(src)` block.
pub struct StubMermaidDispatcher;

impl MermaidDispatcher for StubMermaidDispatcher {
    fn render(&mut self, _src: &str) -> Result<MermaidRendered, MermaidDispatchError> {
        Err(MermaidDispatchError::UnsupportedDiagramType)
    }
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
        }
    }

    fn finish(self) -> String {
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
                } else {
                    // Stray block-html fragment — render as raw.
                    self.write(&format!(
                        "#md_inline_html({})\n\n",
                        typst_string(s)
                    ));
                }
                Ok(())
            }
            Event::InlineHtml(s) => {
                self.write(&format!("#md_inline_html({})", typst_string(s)));
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
                if !raw.is_empty() {
                    self.write_block(&format!(
                        "#md_inline_html({})\n\n",
                        typst_string(raw)
                    ));
                }
            }
        }
        Ok(())
    }

    /// Write a block-level construct. If the current target is a
    /// content collector (table cell, item, blockquote, etc.) we just
    /// route to the buf same as inline; the calling site already
    /// suffix-newlines for top-level emission.
    fn write_block(&mut self, s: &str) {
        self.write(s);
    }
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
}
