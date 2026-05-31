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

#set page(
  paper: "us-letter",
  margin: (x: 1in, y: 1in),
)

#set par(leading: 0.65em, spacing: 1.0em)

#show heading.where(level: 1): set text(size: 22pt, weight: "bold")
#show heading.where(level: 2): set text(size: 17pt, weight: "bold")
#show heading.where(level: 3): set text(size: 14pt, weight: "bold")
#show heading.where(level: 4): set text(size: 12pt, weight: "bold")
#show heading.where(level: 5): set text(size: 11pt, weight: "bold")
#show heading.where(level: 6): set text(size: 11pt, weight: "bold", style: "italic")

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
