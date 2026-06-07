---
{
  "id": "V-7e24cd",
  "kind": "vision",
  "title": "md2pdf — CLI tool to render Markdown files as PDF",
  "version": 3,
  "created_at": "2026-06-03 01:17:28",
  "updated_at": "2026-06-03 02:34:54",
  "attrs": {
    "conversation_anchor": "client-turn-2026-06-03-vision-statement"
  },
  "out_links": []
}
---
md2pdf is a CLI tool to render Markdown files into output documents. It is just a CLI tool — no specific use case outside its general utility.

## Initial requirements (Client-stated)

- **Emoji support.** Color-emoji rendering must work; Twemoji was selected for this.
- **No Chromium.** Chromium is "way too much baggage for what should be a simple renderer."
- **Mermaid support.** Mermaid diagrams in Markdown must render. (Client-confirmed as a requirement, not an emergent feature.)

## Output formats (Client-defined scope)

- **PDF** — original and primary output format.
- **PNG** — explicitly in-scope as of 2026-06-03 by Client directive. Multi-page Markdown produces multiple PNG files per the page-numbered filename convention captured separately.
- **SVG** — **not in scope.** The proposal that surfaced PNG support (`to_png.txt`) also floated SVG, but the Client's directive was specifically PNG. SVG would be a separate scope expansion requiring its own Decision.

## Posture

- A simpler Pandoc.
- Implemented in Rust (novel for this niche).
- Markdown rendering remains the focus — the tool stays narrow on Markdown input; it is not trying to be a general document toolchain.

## What this Vision is not

- Not a Pandoc replacement in scope; deliberately narrower.
- No specific target user persona or workflow is privileged — local dev rendering, CI artifacts, etc. are all equally valid; the tool serves general utility.