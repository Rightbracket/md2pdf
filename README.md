# md2pdf

Markdown → PDF/PNG CLI with bundled color-emoji rendering.

md2pdf renders a Markdown file to either PDF or per-page PNGs through a
hand-rolled `typst::World`, embedding the Twemoji Mozilla COLRv0 font so
emoji rendering does not depend on the host system.

## Usage

```
md2pdf [--strict] [--font-scale SCALE] [--out PATH] [--format pdf|png] <FILE>
```

By default the output is written next to the input file:

- `--format pdf` (default) → `<input-stem>.pdf` next to the input
  (e.g. `README.md` → `README.pdf`).
- `--format png` → `<input-stem>-<N>.png` next to the input
  (e.g. `notes.md` → `notes-1.png`, `notes-2.png`, …).

### `--format` and the strict-extension policy on `--out`

The `--format` flag is the source of truth for the output format. The
`--out` path's extension is **never** sniffed to infer the format.

When `--out` is provided, the trailing extension is preserved only when
it matches `--format`; otherwise the format extension is appended
literally to the path-as-typed (Client-defined behavior; see the
governing Understanding **U-b2bf02**):

| invocation                                | resulting output             |
| ----------------------------------------- | ---------------------------- |
| `--out foo.pdf` (default `--format pdf`)  | `foo.pdf`                    |
| `--out foo.png` (default `--format pdf`)  | `foo.png.pdf`                |
| `--format png --out bar.png`              | `bar-1.png` (stem = `bar`)   |
| `--format png --out bar.pdf`              | `bar.pdf-1.png`              |
| `--out foo` (default `--format pdf`)      | `foo.pdf`                    |

### Multi-page PNG filenames

Per **U-915ef2**, every PNG page filename is one-indexed and uses
*dynamic* zero-padding derived from the total page count:

| total pages   | filenames                                |
| ------------- | ---------------------------------------- |
| 1             | `<stem>-1.png`                           |
| 9             | `<stem>-1.png` … `<stem>-9.png`          |
| 10            | `<stem>-01.png` … `<stem>-10.png`        |
| 100           | `<stem>-001.png` … `<stem>-100.png`      |

### `--strict`

`--strict` (per **U-6173fb**) escalates any warning (image-pipeline,
mermaid, emitter, or Typst-compile) to a hard error and exits with code
`6`. The gate is format-agnostic: `--strict` behaves identically for
PDF and PNG.

## Build

```
cargo build --release
```

PNG output uses `typst-render` (an in-engine rasteriser) and tiny-skia
to produce PNG bytes; both are pinned to exact versions in `Cargo.toml`
per the supply-chain posture **U-7c65ac** / **U-e61b99** and add no
new transitive surface beyond what was already present in the Typst
crate family.

## Bundled font

`assets/fonts/Twemoji.Mozilla.ttf` is the Mozilla-maintained COLRv0
build of the Twitter emoji set. License: CC-BY 4.0 for the artwork.
Source: https://github.com/mozilla/twemoji-colr (release v0.7.0).
