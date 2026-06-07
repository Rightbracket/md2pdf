---
{
  "id": "U-8cdc0d",
  "kind": "understanding",
  "title": "PNG raster resolution — 144 DPI (PIXELS_PER_PT = 2.0) Client-ratified default",
  "version": 1,
  "created_at": "2026-06-03 03:08:05",
  "updated_at": "2026-06-03 03:08:05",
  "attrs": {
    "conversation_anchor": "client-turn-2026-06-03-png-dpi-ratified"
  },
  "out_links": []
}
---
**Client-ratified.** When PNG output renders the `PagedDocument`, the per-page raster resolution is locked at `PIXELS_PER_PT = 2.0_f32`, equivalent to **144 DPI** (matching the Typst CLI's default). Defined as a module-level `const` in `src/pipeline/png.rs`; **not** a CLI flag at this time.

## Provenance

During W-402de2 (Architect Decision D-30e622), the Architect chose 144 DPI as the v1 default and explicitly placed `--dpi` out of scope. The SE surfaced this as a Client-facing default-value ratification point during the W-402de2 in-review surface in conversation. The Client ratified by selecting Path 1 ("ratify both" — the bar.pdf-1.png composition AND the 144 DPI default) on 2026-06-03.

Without this Understanding, the 144 DPI choice would live only in D-30e622 and read as an Architect pick rather than a Client-endorsed anchor. This Understanding makes the Client endorsement explicit so future Work proposing `--dpi` knows it is amending a ratified default, not patching an arbitrary one.

## Why 144 DPI specifically

- **Matches the Typst CLI default.** Existing Typst tooling and documentation use 2.0 px/pt; users coming from Typst itself will see consistent rasterization.
- **Reasonable for screen + decent print preview.** 1.5× of typical 96 DPI screen; sufficient detail for most uses without producing absurdly large files.
- **Fixed-by-default keeps the v1 CLI surface narrow** — consistent with the Vision posture of staying narrow on Markdown-rendering.

## What is *not* captured by this Understanding

- A future `--dpi` flag is **not foreclosed**. It is **deferred** — a future Decision may surface it if Client demand emerges (high-DPI print output, low-DPI thumbnail use cases, etc.). When that happens, this Understanding becomes the prior-art baseline: the default must remain 144 DPI for backward compatibility.
- Per-page DPI variation is not contemplated. PNG output is uniform across all pages of a document.
- Color depth, color profile, and encoder options are tiny-skia's defaults (per D-30e622); not Client-ratified, just inherited.

## How to apply this Understanding

- Code changes that touch `PIXELS_PER_PT` should require an explicit Decision — the constant is a Client-ratified default, not a tunable.
- A `--dpi` proposal is a CLI surface change (touches U-8b7e5c) and a default-policy change (touches this Understanding); both Understandings should be referenced in any such Work.
- A user asking "why is the PNG output blurry?" or "why is it so large?" before a `--dpi` flag exists is a real signal that demand may be surfacing; capture as Client utterance and consider Architect Work.