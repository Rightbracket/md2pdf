# U-954c6b — Image policy — local + remote required (Client); fetch caps Architect-chosen

_Kind: **Understanding** · Exported 2026-06-03 02:16:08 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-954c6b"></a>
## U-954c6b — Image policy — local + remote required (Client); fetch caps Architect-chosen

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:28:30

**Mixed authorship.** The Client stated the high-level requirement loosely; the Architect/agent filled in the policy details.

## Client-stated requirement

- "It should support local and remote images." That is the entirety of the Client's specification.

## Agent-authored policy details (in the artifact today)

- **HTTP(S) fetch** via `ureq` with `rustls` + `webpki-roots` (no OpenSSL) — consistent with the supply-chain / no-system-deps posture.
- **Caps:** 10-second timeout, 20 MiB max body size, 5 redirects max. These specific numbers were not Client-specified.
- **`file://` and relative-path** images are decoded locally without network.
- **PNG / JPEG** via the `image` crate; **SVG passthrough** is delegated to Typst (resvg). The `image` crate is deliberately compiled with PNG/JPEG features only — no SVG features enabled there.
- Strict-mode interaction with image fetch failures is *suspected by the Client* to be the rationale for `--strict`, but unconfirmed. To be resolved when we capture the strict-mode Understanding.

## Client posture

- The high-level "local + remote" requirement is binding.
- The specific caps (10s / 20MiB / 5 redirects) are agent-chosen. Treat as binding-by-precedent but open to re-tuning if a real use case demands it.

## What this Understanding does NOT cover

- The exact behavior of `--strict` when an image fetch fails — deferred.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-image-policy"
}
```

---

