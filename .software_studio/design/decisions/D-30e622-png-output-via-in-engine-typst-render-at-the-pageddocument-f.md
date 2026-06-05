# D-30e622 — PNG output via in-engine typst-render at the PagedDocument fork

_Kind: **Decisions** · Exported 2026-06-05 20:35:19 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="D-30e622"></a>
## D-30e622 — PNG output via in-engine typst-render at the PagedDocument fork

*Kind:* `decision` · *Version:* 1 · *Updated:* 2026-06-03 02:41:16

## Decision

md2pdf adds PNG as a second output format via **in-engine rendering using `typst-render = "=0.14.2"`**, dispatched at the `PagedDocument` boundary in `src/pipeline.rs::render` *after* the strict-mode warning gate. PDF behavior is preserved unchanged. PNG output filenames follow the U-b2bf02 strict-extension rule (to derive the *stem*) composed with the U-915ef2 always-page-numbered, dynamic-zero-padding rule (to expand the stem into one-or-more output files). Format selection is via a new `--format pdf|png` CLI flag, defaulting to `pdf`.

## §1. Implementation approach — typst-render, in-engine

**Chosen:** Path A. Add `typst-render = "=0.14.2"` (sibling of typst/typst-pdf/typst-assets in the same version family). For each page in the compiled `PagedDocument`, call `typst_render::render(&page, PIXELS_PER_PT)` to obtain a `tiny_skia::Pixmap`, then encode to PNG bytes via `Pixmap::encode_png()` (tiny-skia's built-in PNG encoder, transitively enabled by typst-render's tiny-skia dep).

**Resolution at v1:** `PIXELS_PER_PT = 2.0_f32` (= 144 DPI, matches Typst CLI default). Defined as a module-level `const` in the new png module — *not* a CLI flag. A future Decision may surface a `--dpi` flag if Client demand emerges.

**Encoder fallback (documented, not chosen):** if `Pixmap::encode_png()` is unavailable in the resolved tiny-skia transitive (the `png` feature on tiny-skia would have to be off), encode via `image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, pixmap.data().to_vec()).unwrap().write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)`. Verified at Decision time: `image = "=0.25.6"` with `features = ["png"]` exposes `image::codecs::png::PngEncoder` (the `png` feature in image 0.25 is the unified codec feature). The Developer should attempt the primary path first; if it does not compile, fall back without further architectural input and document the choice in the implementation Outcome.

### Rejected: Path B (external pdftoppm / Poppler)

Rejected on two independent grounds, either sufficient on its own:
- **U-6a598c** (self-contained binary): users would need poppler-utils on PATH. Disallowed.
- **U-7c65ac** (supply-chain posture): an `exec` call is an unauditable trust boundary; the binary's version is unknown, the behavior unpinnable.

Quality is also strictly worse: rasterizing from PDF is one level of fidelity downstream of rasterizing from `PagedDocument`.

### Rejected: standalone raster stack (cairo, skia-rs, resvg-only)

Sprawl. typst-render exists, is in our version family, and is the canonical answer.

## §2. Cargo.toml addition

Add to `[dependencies]`:

```toml
# In-engine PNG rendering. Renders typst::layout::PagedDocument → tiny_skia::Pixmap
# per page, then encodes via Pixmap::encode_png(). Pinned to the typst 0.14.2
# version family per U-e61b99. default-features=false per U-7c65ac.
typst-render = { version = "=0.14.2", default-features = false }
```

No other dep additions. `image = "=0.25.6"` already covers PNG encode if the fallback path is ever taken.

## §3. Dispatch point in pipeline::render — PagedDocument fork after the strict gate

The existing `pipeline::render` (src/pipeline.rs lines 72–135) is restructured *only* between line 121 and line 134. Lines 1–121 (input read, body emission, compose, compile, warning bridge, **strict gate**) are unchanged — preserving U-6173fb (strict gate placement) and U-d302a0 (all four warning buckets fire identically before the gate, regardless of format).

After line 121's strict-gate `return`, the dispatch becomes:

```rust
let document = compiled
    .output
    .map_err(|errors| Md2PdfError::TypstCompile(format_diags(&errors)))?;

match req.format {
    OutputFormat::Pdf => {
        let pdf_bytes = typst_pdf::pdf(&document, &typst_pdf::PdfOptions::default())
            .map_err(|errors| Md2PdfError::TypstCompile(format_diags(&errors)))?;
        std::fs::write(&req.output_target.pdf_path(), pdf_bytes)
            .map_err(|source| Md2PdfError::PdfWrite { path: ..., source })?;
    }
    OutputFormat::Png => {
        png::render_pages(&document, req.output_target.png_stem(), &mut warnings)?;
        // png::render_pages handles per-page render + write internally.
    }
}
Ok(())
```

`RenderRequest` gains two new fields (described in §5):
- `pub format: OutputFormat` (enum Pdf|Png).
- `pub output_target: OutputTarget` (replaces today's `pub output: &'a Path`).

The `WarningCollector` is *not* threaded into `png::render_pages` for new warnings — the strict gate already fired. If a future PNG-render-time failure mode warrants a warning bucket, the Developer surfaces it as a hard error (existing four-bucket taxonomy stays closed for this Decision).

## §4. Module layout

The PNG renderer lives at `src/pipeline/png.rs`, parallel to the existing `src/pipeline/world.rs`. Single public function:

```rust
pub(crate) fn render_pages(
    document: &typst::layout::PagedDocument,
    stem: &Path,
) -> Result<()>;
```

Behavior: computes padding width from `document.pages.len()`, iterates pages, calls `typst_render::render`, writes each `<stem>-<NN>.png`. Module-private helpers: `padding_width(n: usize) -> usize`, `compose_page_filename(stem: &Path, page_index_one_based: usize, total: usize) -> PathBuf`, `encode_pixmap_to_png(pixmap: &Pixmap) -> Result<Vec<u8>>`.

PDF export stays inline in `pipeline.rs` (it is one `typst_pdf::pdf` call + one `fs::write`; not module-worthy on its own).

`src/pipeline/mod.rs` declares `pub(crate) mod png;` alongside `pub(crate) mod world;`.

## §5. CLI integration — `--format` flag, `OutputTarget`, filename composition

### 5a. clap derive

Add to `src/cli.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
#[clap(rename_all = "lower")]
pub enum OutputFormat {
    Pdf,
    Png,
}

impl Default for OutputFormat {
    fn default() -> Self { OutputFormat::Pdf }
}

// Inside `Cli`:
/// Output format. Default: pdf (preserves pre-flag CLI behavior).
#[arg(long = "format", value_enum, default_value_t = OutputFormat::Pdf)]
pub format: OutputFormat,
```

Validation rules:
- `--format` accepts exactly one of `pdf` or `png` (clap ValueEnum enforces). Any other value → clap usage error → exit 2 (existing path, no new code).
- Default is `pdf`. Honors U-b2bf02: invocations omitting `--format` are unaffected.
- The flag is *non-repeatable* (no `action = Append`); ValueEnum is implicitly `ArgAction::Set`.

### 5b. `OutputTarget` enum — the post-resolution shape

Introduce `OutputTarget` in `src/cli.rs`:

```rust
pub enum OutputTarget {
    Pdf { path: PathBuf },         // exact write path
    PngStem { stem: PathBuf },     // stem; writes are <stem>-N.png
}
```

`OutputTarget` is the resolved-output shape passed into `RenderRequest`. The CLI layer is responsible for producing it; the pipeline layer never re-parses `--out`.

### 5c. Filename composition algorithm — `compose_output_target`

New pure function in `src/cli.rs`:

```rust
pub fn compose_output_target(
    format: OutputFormat,
    out: Option<&Path>,
    input: &Path,
) -> OutputTarget;
```

**Step 1 — derive the literal target path** per U-b2bf02 strict-extension rule. Define `FORMAT_EXT(format)` = `"pdf"` or `"png"`. Define `extension_matches(path, format_ext)` = true iff the trailing `.<ext>` of `path.file_name()` (case-insensitive ASCII compare) equals `format_ext`. Then:

| Input | Literal target |
|---|---|
| `out = Some(p)` and `extension_matches(p, FORMAT_EXT)` | `p` verbatim |
| `out = Some(p)` and not matches | `p` with `.<FORMAT_EXT>` appended literally |
| `out = None`, format = pdf | `derive_output_path(input)` (existing function, unchanged) |
| `out = None`, format = png | `derive_default_stem(input)` then append `.png` to it (so the literal target ends in `.png`, ready for step 2) |

`derive_default_stem(input)`: same logic as today's `derive_output_path` minus the final `.pdf` append — i.e. strip a recognized markdown ext if present, otherwise keep the basename as-is, placed in the input's parent dir.

**Step 2 — fork on format:**

- **PDF:** the literal target IS the output path. Return `OutputTarget::Pdf { path: literal }`.
- **PNG:** strip the trailing `.png` from the literal target (case-insensitive) to recover the stem. Return `OutputTarget::PngStem { stem }`. (The literal target always ends in `.png` after step 1 for the PNG case, by construction.)

Worked examples (these are the test surface for `compose_output_target`):

| Invocation | Step-1 literal | Step-2 result |
|---|---|---|
| `--out foo.png` (default `--format pdf`) | `foo.png.pdf` | `Pdf { foo.png.pdf }` ✅ U-b2bf02 verbatim |
| `--format png --out bar.pdf` | `bar.pdf.png` | `PngStem { bar.pdf }` → writes `bar.pdf-1.png` … |
| `--out foo.pdf` (default `--format pdf`) | `foo.pdf` | `Pdf { foo.pdf }` ✅ U-b2bf02 implied |
| `--format png --out bar.png` | `bar.png` | `PngStem { bar }` → writes `bar-1.png` … |
| `--out foo` (default `--format pdf`) | `foo.pdf` | `Pdf { foo.pdf }` ✅ U-b2bf02 implied |
| `--format png --out foo` | `foo.png` | `PngStem { foo }` → writes `foo-1.png` … |
| no `--out`, `--format pdf`, input `notes.md` | `notes.pdf` | `Pdf { notes.pdf }` (today's behavior, unchanged) |
| no `--out`, `--format png`, input `notes.md` | `notes.png` | `PngStem { notes }` → writes `notes-1.png` … |
| no `--out`, `--format png`, input `notes.txt` | `notes.txt.png` | `PngStem { notes.txt }` → writes `notes.txt-1.png` … |

### 5d. Page-number expansion — U-915ef2 algorithm

Inside `png::render_pages`:

```rust
fn padding_width(total_pages: usize) -> usize {
    debug_assert!(total_pages >= 1);
    // floor(log10(N)) + 1
    let mut n = total_pages;
    let mut w = 0;
    while n > 0 { w += 1; n /= 10; }
    w
}

fn compose_page_filename(stem: &Path, page_one_based: usize, total: usize) -> PathBuf {
    let w = padding_width(total);
    // Append "-{:0width$}.png" to the stem's final filename component.
    let parent = stem.parent();
    let stem_name = stem.file_name().expect("stem must have a filename").to_string_lossy();
    let leaf = format!("{stem_name}-{:0w$}.png", page_one_based, w = w);
    match parent {
        Some(p) if !p.as_os_str().is_empty() => p.join(leaf),
        _ => PathBuf::from(leaf),
    }
}
```

Properties (each is a unit-test obligation):
- 1 page → `foo-1.png` (width 1, no leading zeros). U-915ef2 verbatim.
- 9 pages → `foo-1.png` … `foo-9.png` (width 1). U-915ef2 verbatim.
- 10 pages → `foo-01.png` … `foo-10.png` (width 2). U-915ef2 verbatim.
- 100 pages → `foo-001.png` … `foo-100.png` (width 3). U-915ef2 verbatim.
- Hyphen separator, never underscore/dot/none. U-915ef2 verbatim.
- One-indexed, never zero-indexed. U-915ef2 verbatim.

## §6. Interaction with `derive_output_path` / `validate_out_path`

**`derive_output_path`:** unchanged; remains the no-`--out` PDF path. Extracted twin `derive_default_stem` (described in §5c) handles the no-`--out` PNG path. Both functions live in `src/cli.rs`.

**`validate_out_path`:** still called pre-flight on the user-supplied `--out` value (if any), once, *before* `compose_output_target` runs. The three existing checks (trailing-separator reject, existing-directory reject, missing-parent-directory reject) all continue to apply to the path-as-typed. This is correct for both formats — the user-typed path is the anchor for parent-dir existence regardless of whether we ultimately treat it as a stem or a literal write target.

The "existing file → silent overwrite" Policy B from O-92f7a9 carries over to PNG: a PNG run that produces `foo-1.png` over an existing `foo-1.png` silently overwrites it. No collision check, no prompt.

`derive_output_path`'s test vector table at `src/cli.rs` lines 299–320 stays as-is. New test vectors for `compose_output_target` and `compose_page_filename` are additive (see §8).

## §7. Error variants and exit codes

**No new exit codes.** Adding entries to U-64e9ec's binding table is reserved for genuinely new failure semantics, and PNG-write failure is not one — it is the same I/O-class failure as PDF write.

**One new error variant** for clean user-facing messages:

```rust
#[error("could not write output PNG {path}: {source}")]
PngWrite {
    path: PathBuf,
    #[source]
    source: std::io::Error,
},
```

Mapped to `ExitCode::PdfWrite` (= 4) in `Md2PdfError::exit_code()`. The exit code is shared because the table is binding (U-64e9ec) and there is no new external semantic to name. The variant exists *only* so the error message reads `"could not write output PNG foo-1.png"` instead of `"could not write output PDF foo-1.png"`.

**Future cleanup (NOT this Decision):** the `ExitCode::PdfWrite` name is now misleading — it covers both PDF and PNG output writes. Renaming the variant to `ExitCode::OutputWrite` (no numeric change) is a tidy follow-up; out of scope here. The Decision explicitly does not require it because Cargo enum-variant renames ripple through the code without behavioral effect, and U-64e9ec is about *numeric* code stability.

If a render-time PNG failure surfaces from `typst_render::render` itself (which today returns a `Pixmap` infallibly — no `Result`), there is no error variant needed. If `Pixmap::encode_png()` returns `Option<Vec<u8>>` or `Result<…>` and fails, that's an `Md2PdfError::Internal(format!("PNG encode failed: {…}"))` mapping to `ExitCode::Internal` (= 70). Encode failure is a genuine internal-defect case; users cannot trigger it.

## §8. Test surface plan

### Unit tests (in-source, beside the implementation)

In `src/cli.rs` mod tests:
- `compose_output_target_strict_extension_matrix` — exhaustive table from §5c (all 9 worked examples). One assertion per row.
- `derive_default_stem_handles_md_extensions` — mirrors the existing `derive_output_path_matches_decision_table` table but expects the stem (no extension), gating the `.png` append to `compose_output_target`.
- `cli_format_flag_default_is_pdf` — `Cli::try_parse_from(["md2pdf", "foo.md"]).format == Pdf`.
- `cli_format_flag_parses_pdf_and_png` — both values accepted via clap.
- `cli_format_flag_rejects_other_values` — `--format svg` errors out with clap usage error.

In `src/pipeline/png.rs` mod tests (no Typst document needed for these — pure path math):
- `padding_width_table` — { 1→1, 9→1, 10→2, 99→2, 100→3, 999→3, 1000→4 }.
- `compose_page_filename_one_page` — stem `foo` + total=1 → `foo-1.png`.
- `compose_page_filename_ten_pages` — stem `foo` + total=10, page 1 → `foo-01.png`, page 10 → `foo-10.png`.
- `compose_page_filename_with_parent_dir` — stem `tmp/foo`, total=10 → `tmp/foo-01.png`.
- `compose_page_filename_stem_with_dots` — stem `bar.pdf`, total=1 → `bar.pdf-1.png` (the U-b2bf02-verbatim worked example).

### Integration tests (under `tests/`)

New file `tests/png_smoke.rs` (gated to local-only, no network):
- `png_single_page_writes_one_file` — render a 1-paragraph MD → assert exactly `out-1.png` is written, valid PNG header (`\x89PNG\r\n\x1a\n`), parent dir empty afterwards otherwise.
- `png_multi_page_writes_n_files_with_correct_padding` — force a multi-page document via a `#pagebreak()`-style MD trick (or a long enough body to wrap onto N pages); assert the exact filename set with the expected padding width.
- `png_format_flag_default_pdf_unchanged` — without `--format`, behavior matches the existing `tests/pdf_smoke.rs` (regression guard).
- `png_strict_mode_blocks_write` — broken-image MD + `--strict --format png`: assert NO `*-N.png` file is written, exit code = 6, canonical summary line on stderr. **This test proves U-6173fb is preserved across formats.**
- `png_extension_policy_format_png_out_bar_pdf` — invoke with `--format png --out bar.pdf`, assert `bar.pdf-1.png` exists. The verbatim U-b2bf02 worked example, end-to-end.
- `png_no_out_input_neighbor` — input `notes.md`, `--format png`, no `--out`: assert `notes-1.png` written next to input.

### Existing test impact

- `src/cli.rs` tests for `derive_output_path`, `validate_out_path`, `parse_font_scale`, the strict flag, and `--out` parsing all continue to pass unchanged. The `--format` flag is purely additive at the clap surface.
- `tests/pdf_smoke.rs` (existing) requires zero changes — default `--format pdf` preserves all behavior.

## §9. Out of scope for this Decision

The following are explicitly NOT addressed and require their own future Decisions if pursued:

- **SVG output** — Vision V-7e24cd v3 lists SVG as not in scope. `to_png.txt`'s `typst-svg` suggestion is shelved.
- **JPEG / WebP / TIFF / any other raster format.**
- **`--dpi` / `--pixels-per-pt` configurable resolution flag.** v1 locks 2.0 px/pt (= 144 DPI) as a module constant.
- **Parallel page rendering.** v1 is sequential per-page. Profile first if it ever matters.
- **PNG metadata customization** (color profiles, sRGB chunks, text chunks). Tiny-skia's default encoder output is the contract.
- **Renaming `ExitCode::PdfWrite` → `ExitCode::OutputWrite`.** Internal cleanup, no exit-code-table churn (per U-64e9ec). Worth a separate small Work item.
- **Combining `--format pdf,png` in a single invocation** (multi-format output). One render produces one format per U-b2bf02. Multi-format requires a separate Decision and a different CLI shape.
- **Page-range selection for PNG** (e.g. `--pages 1-5`). Not requested.

## Conditions that would invalidate this Decision

Re-open this Decision (write a superseding one) if any of the following occur:

1. `typst-render` is dropped from the typst 0.14.x family (or future major) without a same-version sibling. The "in-engine, version-pinned with typst" property is the load-bearing reason this Decision rejects Path B.
2. typst-render develops a public surface change that costs more to maintain than calling Poppler would (extremely unlikely; named for completeness).
3. Client raises a multi-format-per-invocation requirement, or a configurable-DPI requirement — both reshape the CLI and the pipeline-fork shape enough to warrant a new Decision rather than an extension of this one.
4. SVG output is added to the Vision. The composition of the strict-extension rule with the page-numbering rule was designed for the two-format case; a third format may motivate a unified `OutputTarget` redesign.
5. A future Understanding splits the strict-mode warning gate into per-format semantics. This Decision relies on U-6173fb's "format-agnostic" invariant — if that invariant is narrowed, the dispatch placement (after the gate, single-gate-shared-by-both-formats) needs revisiting.

**Attributes**

```json
{
  "produced_by_work": "W-402de2",
  "serves": [
    "V-7e24cd",
    "U-b2bf02",
    "U-915ef2",
    "U-6173fb",
    "U-d302a0",
    "U-7c65ac",
    "U-6a598c",
    "U-e61b99",
    "U-64e9ec",
    "U-8b7e5c"
  ]
}
```

---

