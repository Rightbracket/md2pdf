# U-6173fb — --strict mode — any warning becomes a hard failure

_Kind: **Understanding** · Exported 2026-06-03 02:16:08 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-6173fb"></a>
## U-6173fb — --strict mode — any warning becomes a hard failure

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:36:45

**Client-confirmed semantics.** When `--strict` is passed, **any warning surfaced during render becomes a hard failure**: no PDF is written, the canonical summary line is emitted, and the process exits with the `StrictEscalation` exit code. Without `--strict`, warnings are reported but the PDF is still produced.

## Scope of "any warning"

All four `WarningSource` buckets in the runtime `WarningCollector` count equally:
- **Image** — e.g. remote fetch failed / timed out / exceeded size cap, local file missing, decode failure.
- **Mermaid** — parse failure on a mermaid block, unsupported diagram type.
- **Emitter** — issues raised while walking the Markdown AST.
- **TypstCompile** — warnings raised by `typst::compile` against the composed source.

The Client confirmed: "That semantic seems reasonable, and worth keeping exactly as is." No narrowing requested (e.g. "strict only on image failures") — strict is uniformly strict across all bucket sources.

## Gate placement

The strict check runs in `src/pipeline.rs` **after `typst::compile` but before `typst_pdf::pdf` and the `fs::write`**. This is deliberate: a strict build never writes a PDF that has warnings against it, even a partially-good one.

## Intended use cases

- **CI documentation builds.** "You don't want a PR's docs build to silently emit a PDF with broken image links and call it green." Client-confirmed as one of the use cases.
- Other use cases plausible but unenumerated: release-artifact production, automated quality gates, pre-publish verification. Client said: "I'm sure there are others."

## Default posture

- Default is **non-strict**: warnings are emitted but the PDF is still produced. This is the right default for interactive / local use where the user wants to see a result and judge for themselves.
- `--strict` is opt-in.

## Tests

- Network-touching strict-mode integration tests exist but are `#[ignore]`-gated (per Archaeologist trench W-b6497a). They run on demand, not in the default test pass.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-strict-confirmed"
}
```

---

