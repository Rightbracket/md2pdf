# U-d302a0 — Runtime WarningCollector — four-bucket taxonomy feeds --strict

_Kind: **Understanding** · Exported 2026-06-03 03:11:38 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-d302a0"></a>
## U-d302a0 — Runtime WarningCollector — four-bucket taxonomy feeds --strict

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:36:45

**Client-ratified.** The runtime `WarningCollector` and its four-bucket taxonomy were initially flagged by the Client as a possibly misapplied response to a complaint about *compile-time* warnings. After clarification — the runtime collector exists to feed `--strict` mode (U-<strict>), not to silence cargo build warnings — the Client said: "Now knowing all the details, the bucket taxonomy makes sense."

## The taxonomy

`src/warnings.rs` defines a unified `WarningCollector` with exactly four `WarningSource` buckets:

- **Image** — image_pipeline issues (fetch, decode, sizing).
- **Mermaid** — mermaid subsystem issues (parse, unsupported diagram types, layout edge cases).
- **Emitter** — pulldown-cmark walker issues.
- **TypstCompile** — warnings emitted by `typst::compile` against the composed source.

Every render-time warning routes through one of these buckets.

## Why these four

They correspond to the **four producing subsystems** in the pipeline (see Archaeologist trench W-b6497a, finding F-ef2b76 step 4–7). One bucket per subsystem is the implicit organizing principle.

## How to apply this Understanding

- A new producing subsystem (e.g. a font-loading pipeline, a TOC generator) **may add a new bucket**, but the addition should be deliberate — the bucket taxonomy is part of the published `--strict` semantics. New buckets count toward strict-mode failure by default unless explicitly exempted.
- Routing a warning to the *wrong* bucket is a defect: it misleads the user about where the issue originated.

## What this Understanding is NOT

- **Not about cargo build warnings.** Those are a separate concern; this collector does not interact with them. If the Client's original complaint about build-time `unused_mut`-style warnings has not been addressed in the codebase, that is a separate cleanup item the partnership should track independently.
- The Client did not ask for that build-warning audit explicitly in this turn, but the topic surfaced. SE may want to post a small Reviewer or QA Work to spot-check current `cargo build` output for unaddressed compile warnings, separately from this Understanding.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-warning-buckets-ratified"
}
```

---

