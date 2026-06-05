# U-64e9ec — Exit-code table — Architect-authored, Client-indifferent

_Kind: **Understanding** · Exported 2026-06-05 20:35:19 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-64e9ec"></a>
## U-64e9ec — Exit-code table — Architect-authored, Client-indifferent

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:28:20

**Provenance.** The Client did not specify the exit-code table. It originated from an Architect Decision in a prior (now-lost) session. The Client did not ask for anything specific and is indifferent on the substance.

## The table as it appears in the artifact (`src/error.rs`)

- `0` — success.
- `1` — generic failure.
- `2`–`6` — custom granularity (usage error, input-not-found, image-fetch-failure, mermaid-failure, strict-escalation, etc. — exact assignments to be confirmed against the source if it matters downstream).
- `70` — sysexits `EX_SOFTWARE` convention.

`Md2PdfError` (thiserror enum) maps to these via an `exit_code()` method.

## Client posture

- Indifferent. "Exit codes were an architect decision, I didn't ask for anything specific."
- Treat the table as binding (it is the published behavior of the CLI) but not as a Client-authored requirement. A future Work that wants to simplify or restructure the table should be raised as an explicit Decision, not a drive-by edit.

## What this Understanding does NOT cover

- The exact mapping of each error variant to its numeric code — lift from `src/error.rs` if a Work needs it; not worth duplicating here.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-exit-codes"
}
```

---

