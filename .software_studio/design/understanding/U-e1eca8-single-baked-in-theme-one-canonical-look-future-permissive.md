# U-e1eca8 — Single baked-in theme — one canonical look, future-permissive

_Kind: **Understanding** · Exported 2026-06-03 02:16:08 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-e1eca8"></a>
## U-e1eca8 — Single baked-in theme — one canonical look, future-permissive

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:41:01

**Provenance.** Not a specific Client choice. The single-theme posture landed because nobody asked for alternatives. The Client said: "Wasn't a specific choice. It is fine for now."

## What is built today

- `src/theme.rs` exposes `pub const THEME: &str = include_str!("../assets/theme.typ");`.
- The theme is a **188-line Typst stylesheet** baked into the binary at compile time.
- There is **no theme selection mechanism** — no `--theme` flag, no config file, no runtime selection. Every md2pdf invocation produces a PDF styled by this one theme.
- Theme composition into the rendered source happens in `compose_typst_source` (`src/pipeline.rs`): `#let md2pdf_body_size = <N>pt\n<THEME>\n<body>`. The font-scale value lands above the theme so theme rules can reference it.

## Client posture

- One canonical theme is fine for now.
- **Future-permissive but unscheduled.** "Maybe one day we'll want different themes, but I cannot predict that future." Treat multi-theme support as a *possible* future scope expansion, not a planned one. No anticipatory abstraction needed today.
- Consistent with the self-contained-binary posture (U-6a598c): if multi-theme support ever lands, themes should be **bundled in the binary**, not loaded from disk, unless the Client explicitly authorizes runtime-loadable themes.

## How to apply this Understanding

- Modifying `assets/theme.typ` is the path for any styling change today. There is no abstraction layer to preserve.
- A Work item that proposes themed variants is a **scope expansion** — it should be raised as an explicit Decision, not a drive-by addition.
- A Work item that wants to *change* the existing theme's look (typography, spacing, color, page geometry) does NOT need a Decision — it's a routine edit to the single canonical stylesheet, subject to normal review.

## What this Understanding does NOT cover

- The contents of `assets/theme.typ` itself — the helper-function contract between the emitter (`md_*` calls) and the theme is artifact-level and lives in the source. The Archaeologist's proposed trench #4 (theme + emitter helper-contract) would catalogue it if needed.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-theme"
}
```

---

