# U-915ef2 — Multi-page PNG filename convention — always page-numbered, dynamic padding

_Kind: **Understanding** · Exported 2026-06-03 03:11:38 UTC from `/Volumes/OBELISK/eshork/projects/md2pdf`._

---

<a id="U-915ef2"></a>
## U-915ef2 — Multi-page PNG filename convention — always page-numbered, dynamic padding

*Kind:* `understanding` · *Version:* 1 · *Updated:* 2026-06-03 02:35:08

**Client-directed.** PNG output filenames are **always** page-numbered, even when the document is a single page. Page-number padding is **dynamic** — the width is determined by the total page count of the rendered document, not a fixed constant.

## The convention

For an output stem `foo` (derived from `--out foo` or from the default-output-path rule):

- A 1-page document produces `foo-1.png`.
- A 9-page document produces `foo-1.png` through `foo-9.png` (1-wide).
- A 10-page document produces `foo-01.png` through `foo-10.png` (2-wide).
- A 100-page document produces `foo-001.png` through `foo-100.png` (3-wide).
- Etc.

Padding width = `floor(log10(N)) + 1` where N is the total page count.

## Client framing (verbatim)

> "PNG always produces files with embedded page numbers, even if there is only 1 page. So `foo-1.png` would be normal for a 1-page document. The page numbers must be appropriately padded based on the total number of pages. Don't arbitrarily pick 3 wide zero padding. Use padding that is appropriate for the actual number of pages."

## Why this is captured as Client-authored, not Architect-deferred

The page-numbered-always-and-dynamic-padding rule is a **deliberate UX choice the Client made directly**, not a pick-something-reasonable detail. Specifically:

- *Always page-numbered* (no `foo.png` for single-page) means consumers of the output don't have to write filename-handling code that branches on page count. There is exactly one shape.
- *Dynamic padding* (no fixed 3-wide zero-pad) means the filenames sort lexicographically in the natural page order without unnecessary leading zeros for small documents.

## Separator and filename shape

- Separator is `-` (hyphen) per the Client's `foo-1.png` example. Not underscore, not dot, not none.
- Page numbers start at `1` (one-indexed) per the example. Not `0`.
- Page number is positioned between the stem and the `.png` extension.

## What this Understanding does NOT cover

- The shape of the shared *stem* across pages (i.e. how `--out` and the default-output-path interact with the page-number suffix) — the Architect should specify, but the constraint from this Understanding is: whatever the stem is, page-number-and-extension are appended per the rule above.
- PDF output is unaffected — PDF remains a single-file output regardless of page count.

**Attributes**

```json
{
  "conversation_anchor": "client-turn-2026-06-03-png-filename-convention"
}
```

---

