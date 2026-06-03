# U-8df478 — Image sizing rule — agent-authored, Client-unverified

_Kind: **Understanding** · Exported 2026-06-03 03:11:38 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-8df478"></a>
## U-8df478 — Image sizing rule — agent-authored, Client-unverified

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 01:39:23

**Provenance.** The Client did not specify image sizing behavior. The current rule is entirely an agent's call from a prior (now-lost) session. The Client also stated: "I haven't gotten around to testing it yet, so I cannot comment on its current correctness."

## What is built today (artifact-level)

- `src/image_pipeline.rs` extracts **intrinsic pixel dimensions** from PNG/JPEG inputs via the `image` crate.
- **SVG sizing is delegated to Typst** (resvg) — the image_pipeline does not size SVGs.
- The exact sizing math (what units the pixel dimensions are converted to, whether display size is clamped to page width, whether Markdown-supplied width attributes are honored, what DPI assumption is in effect) is **not summarized in this Understanding** — lift directly from `src/image_pipeline.rs` if a Work item needs it. The Archaeologist trench (W-b6497a) confirmed sizing logic exists there but did not catalogue the math.

## Client posture

- No specification authored. The current behavior is binding-by-precedent only.
- **Untested by the Client.** Correctness of the sizing math against real Markdown documents has not been validated by the Client.
- A future Client report of "images are coming out wrong size" should be treated as a real defect candidate rather than a misunderstanding of intent — there is no Client intent to misalign with.

## Suggested follow-up Work (not yet posted)

- A small **QA Work** to feed a representative Markdown document with a mix of PNG, JPEG, and SVG images (some with explicit width attrs, some without; some larger than page width, some tiny) through md2pdf and characterize the actual sizing behavior. The output is a behavior-spec the Client can react to: "this is what the tool does today; is any of this wrong?"
- After Client review, that behavior-spec can be promoted to an authored Understanding (replacing this provenance-only one) or driven to Developer Work to fix specific defects.

## What this Understanding does NOT cover

- The fetch policy (timeouts, redirects, size caps) is captured separately in U-954c6b (Image policy).
- The decode-feature posture (PNG/JPEG only in `image`, no SVG features) is also in U-954c6b.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-image-sizing"
}
```

---

