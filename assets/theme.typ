// md2pdf v1 default theme (one built-in, baked in per U-f2b045).
//
// All Typst show/set rules and helper functions used by emitter output
// live here. Helper-function contract (called from emitter output):
//   md_link(url, body)              → hyperlink
//   md_image(src)                   → image, fit-to-content-width
//   md_image_sized(src, w, h)       → image with explicit size
//   md_image_placeholder(label)     → bordered placeholder for stub/unresolved
//   md_blockquote(body)             → blockquote rule
//   md_codeblock(lang, code)        → fenced/indented code block
//   md_hardbreak()                  → hard line break
//   md_inline_html(raw)             → fall-through for raw inline HTML
//   md_mermaid_stub(src)            → labelled stub for `mermaid` fences
//   md_table(headers, alignments, rows) → GFM table
//   md_task_unchecked()             → GFM task list, unchecked box glyph
//   md_task_checked()               → GFM task list, checked box glyph
//   md_strike(body)                 → GFM strikethrough wrapper
//   md_footnote(body)               → GFM footnote wrapper
//   md_inline_code(s)               → inline code (soft fill, monospace)
//   md_image_bytes(bytes, format, width_pt, height_opt_pt) → image from bytes

#set page(
  paper: "us-letter",
  margin: (x: 1in, y: 1in),
)

#set par(leading: 0.65em, spacing: 1.0em)
#set footnote(numbering: "1")

// W-1b4905 / O-10564c §5 — proportional font scaling.
//
// `md2pdf_body_size` is injected as a `#let` preamble by `pipeline.rs`
// before this theme is read. It is a `length` value in pt. All text
// sizes downstream derive from this single binding so the user-facing
// `--font-scale` flag drives body and headings together with their
// historical relative ratios preserved (h1=22/11, h2=17/11, h3=14/11,
// h4=12/11, h5=h6=11/11). At md2pdf_body_size=11pt the output is
// arithmetically equivalent to the pre-flag absolute sizes.
#set text(size: md2pdf_body_size)
#show heading.where(level: 1): set text(size: md2pdf_body_size * (22/11), weight: "bold")
#show heading.where(level: 2): set text(size: md2pdf_body_size * (17/11), weight: "bold")
#show heading.where(level: 3): set text(size: md2pdf_body_size * (14/11), weight: "bold")
#show heading.where(level: 4): set text(size: md2pdf_body_size * (12/11), weight: "bold")
#show heading.where(level: 5): set text(size: md2pdf_body_size * (11/11), weight: "bold")
#show heading.where(level: 6): set text(size: md2pdf_body_size * (11/11), weight: "bold", style: "italic")

#show heading: it => {
  v(0.6em, weak: true)
  it
  v(0.3em, weak: true)
}

#show link: set text(fill: rgb("#1a5fb4"))

#let md_image_placeholder(label) = {
  block(
    width: 100%,
    inset: 0.8em,
    stroke: 1pt + rgb("#aaaaaa"),
    fill: rgb("#fafafa"),
    align(center, text(fill: rgb("#666666"), [#emph[image: ] #label])),
  )
}

#let md_blockquote(body) = {
  block(
    width: 100%,
    inset: (left: 1em, top: 0.4em, bottom: 0.4em),
    stroke: (left: 2pt + rgb("#888888")),
    text(style: "italic", body),
  )
}

#let md_codeblock(lang, code) = {
  block(
    width: 100%,
    fill: rgb("#f4f4f4"),
    inset: 0.6em,
    radius: 3pt,
    raw(code, lang: lang, block: true),
  )
}

#let md_link(url, body) = link(url, body)

// Image: emitted with raw source string. The image-pipeline Work
// (separate W-df3269) will replace this stub with a real resolver. For
// now we route through a placeholder so the emitter Work compiles
// end-to-end without needing a real image pipeline.
#let md_image(src) = md_image_placeholder(src)
#let md_image_sized(src, w, h) = md_image_placeholder(src + " " + repr(w) + "×" + repr(h))

#let md_hardbreak() = linebreak()

// Raw inline HTML: classic Gruber Markdown allows inline HTML to pass
// through. We don't render HTML — we surface it as code so it's visible
// rather than silently dropped.
#let md_inline_html(s) = raw(s)

// Mermaid stub block: emitted when a fenced code block declares
// `mermaid` as its language. The mermaid sub-renderer (separate Work
// per D-fb4ebb §1) will replace this with the rendered SVG. Until then
// the stub is clearly labelled so reviewers see where the diagram
// would appear.
#let md_mermaid_stub(src) = {
  block(
    width: 100%,
    inset: 0.8em,
    stroke: 1pt + rgb("#1a5fb4"),
    fill: rgb("#eef4ff"),
    [
      #text(fill: rgb("#1a5fb4"), weight: "bold")[mermaid diagram (stub — sub-renderer not yet wired)] \
      #raw(src, block: true)
    ],
  )
}

// --- GFM extensions (W-e1a99c, D-c3af71 §B) -----------------------------

// md_table: native Typst table with per-column alignment.
//   `headers`     : array of inline content (rendered bold).
//   `alignments`  : array of strings ("left" | "center" | "right"), one per column.
//   `rows`        : array of rows; each row is an array of inline content cells.
#let md_table(headers, alignments, rows) = {
  let aligns = alignments.map(a => {
    if a == "center" { center }
    else if a == "right" { right }
    else { left }
  })
  let cells = ()
  for h in headers { cells.push(strong(h)) }
  for row in rows {
    for cell in row { cells.push(cell) }
  }
  table(
    columns: headers.len(),
    align: (col, row) => aligns.at(col),
    ..cells,
  )
}

// md_task_unchecked / md_task_checked: GFM task-list checkbox glyphs.
// Sized to body text; the checkmark uses Typst's symbol API for
// portability across emoji/text fonts.
#let md_task_unchecked() = box(
  width: 0.9em,
  height: 0.9em,
  stroke: 0.5pt + rgb("#444444"),
  baseline: 0.1em,
)

#let md_task_checked() = box(
  width: 0.9em,
  height: 0.9em,
  stroke: 0.5pt + rgb("#444444"),
  baseline: 0.1em,
  align(center + horizon, text(size: 0.75em, weight: "bold")[#sym.checkmark]),
)

// md_strike: GFM ~~strike~~ wrapper.
#let md_strike(body) = strike(body)

// md_footnote: GFM footnote wrapper. Numbering set above (Arabic).
#let md_footnote(body) = footnote(body)

// md_inline_code: inline `code` with soft fill + monospace.
#let md_inline_code(s) = box(
  fill: rgb("#f4f4f4"),
  inset: (x: 2pt),
  outset: (y: 2pt),
  radius: 2pt,
  raw(s, block: false),
)

// md_image_bytes: render an image from raw bytes (image-pipeline output).
//   `bytes`           : raw image bytes (Typst `bytes`).
//   `format`          : "png" | "jpeg" | "svg" (string).
//   `width_pt`        : width as a Typst length (e.g. `120pt`).
//   `height_opt_pt`   : height length, or `none` to keep aspect from width.
// Note: Typst 0.14.x's `image.decode(...)` is deprecated in favor of
// passing bytes directly to `image(...)`; we use the modern form.
#let md_image_bytes(bytes, format, width_pt, height_opt_pt) = {
  if height_opt_pt != none {
    image(bytes, format: format, width: width_pt, height: height_opt_pt)
  } else {
    image(bytes, format: format, width: width_pt)
  }
}
