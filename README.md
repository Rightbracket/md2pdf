# md2pdf

Markdown → PDF CLI with bundled color-emoji rendering.

This is the v1 **scaffold**. The full Markdown emitter, image pipeline,
and mermaid sub-renderer land in subsequent Work items. The scaffold
produces a placeholder PDF that exercises the Typst engine and the
embedded Twemoji Mozilla font end-to-end.

## Usage

```
md2pdf [--strict] <FILE>
```

The output PDF is written next to the input file (e.g.
`README.md` → `README.pdf`).

## Build

```
cargo build --release
```

## Bundled font

`assets/fonts/Twemoji.Mozilla.ttf` is the Mozilla-maintained COLRv0
build of the Twitter emoji set. License: CC-BY 4.0 for the artwork.
Source: https://github.com/mozilla/twemoji-colr (release v0.7.0).
