# U-e61b99 — Dependency pinning policy — exact-version pin + minimal default features

_Kind: **Understanding** · Exported 2026-06-05 20:35:19 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-e61b99"></a>
## U-e61b99 — Dependency pinning policy — exact-version pin + minimal default features

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:21:11

**Provenance.** The Client did not author this rule. It originated from the Security Engineer in a prior (now-lost) session. The Client, asked directly, is indifferent on the substance but stands by the prior agent's choices.

## The rule as it appears in the artifact

- All production dependencies in `Cargo.toml` are pinned to an exact version with `=X.Y.Z` (no caret-range, no minor-bump tolerance).
- Non-clap dependencies use `default-features = false`, with features named explicitly.
- Cargo.toml line 26 cites a now-lost `Decision D-c3af71` as the source.

## Client posture

- "Indifferent, but I would stand by that agent's prior choices."
- Treat the rule as binding for now — future Work that wants to relax it must surface that as an explicit Decision, not a drive-by edit.

## What this Understanding does NOT cover

- The original D-c3af71 also reportedly bundled the runtime warning-bucket taxonomy (Image / Mermaid / Emitter / TypstCompile). That taxonomy is **deliberately not captured here** — the Client raised a concern that the runtime collector may be a misapplied response to a complaint about compile-time build warnings. To be revisited as a separate item.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-pinning-attribution"
}
```

---

