---
{
  "id": "U-b2bf02",
  "kind": "understanding",
  "title": "Output format selection — --format flag, strict extension preservation",
  "version": 2,
  "created_at": "2026-06-03 02:35:08",
  "updated_at": "2026-06-03 03:08:05",
  "attrs": {
    "conversation_anchor": "client-turn-2026-06-03-format-flag"
  },
  "out_links": []
}
---
**Client-directed.** Output format selection is via an explicit `--format` flag, not by sniffing the `--out` path's extension. The flag value is the source of truth for the format produced.

## The `--format` flag

- `--format pdf|png` — selects the output format.
- Default is `pdf` (preserves today's CLI behavior — invocations that omit `--format` are unaffected).
- The flag does not accept multiple values — one render produces one format.

## Strict extension policy on `--out`

**The trailing extension on the `--out` path is preserved only if it matches the `--format` value. Otherwise the format extension is appended to the literal `--out` value.** No extension sniffing, no extension stripping, no inference of format from path.

Client-supplied examples (verbatim, behavior-defining for the *literal-target* derivation step):

- `--out foo.png` with default `--format pdf` → literal target `foo.png.pdf`.
- `--format png --out bar.pdf` → literal target `bar.pdf.png`.

Implied by the rule (not Client-stated but follow directly):

- `--out foo.pdf` with default `--format pdf` → literal target `foo.pdf` (extension matches).
- `--format png --out bar.png` → literal target `bar.png` (extension matches).
- `--out foo` with default `--format pdf` → literal target `foo.pdf` (no extension on input, format extension appended).

The rationale (per Client framing): if a user types `--out foo.png` they are getting a `.png` only if they also ask for PNG. Otherwise the path they typed is treated as a literal stem and the actual format's extension is appended.

## Composition with U-915ef2 for the PNG case (Client-ratified 2026-06-03)

**For PDF output, the literal target IS the final output path.** No further composition.

**For PNG output, the literal target is treated as the *stem*** — the trailing `.png` is stripped, then U-915ef2's always-page-numbered, dynamic-padding rule expands the stem into one or more files of the form `<stem>-<NN>.png`.

This means the Client-verbatim example `--format png --out bar.pdf → bar.pdf.png` is the literal-target derivation **only**; the actual file produced for a 1-page document is `bar.pdf-1.png` (because U-915ef2 mandates page-numbering even for single-page output). This composition was surfaced as a Client-facing ambiguity during the W-402de2 Architect Decision and **explicitly ratified by the Client on 2026-06-03**: the stem-then-suffix reading is correct; the verbatim U-b2bf02 example illustrates extension-preservation alone, not the complete final filename.

The Architect Decision **D-30e622 §5c** captures the full composition table; the Developer implementation (W-723d6e, Outcome O-1a54a2) tests the worked examples end-to-end.

## Strict-mode behavior is format-agnostic

Client-confirmed: `--strict` (U-6173fb) behaves identically regardless of format. The same warning-bucket logic gates the write — if `req.strict && warnings.any()`, no output is produced (PDF or PNG), the canonical summary line is emitted, and `StrictEscalation` is returned.

## What this Understanding does NOT cover

- Multi-page PNG filename convention itself — captured in U-915ef2.
- The implementation approach (typst-render, dispatch point, etc.) — captured in D-30e622.
- A future `--dpi` flag — explicitly out of scope; PNG output uses the Client-ratified default captured in U-<dpi>.