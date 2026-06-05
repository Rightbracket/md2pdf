# QA HTML image sizing composition (W-8f4312, D-875e4b §7d.4, §4)

Full sizing-mode matrix per Decision §4: cell-relative percent,
absolute pt, mixed, no-style fallback. Plus error paths: unreachable
URL → Image-bucket warning + placeholder; oversize → ditto.

## Section A — `<img>` with style="width: 50%" (cell-relative percent)

The image sits inside a 200pt-wide cell; with `width: 50%` Typst lays
it out at 50% of the *cell* width = 100pt, NOT 50% of page width
(Decision §4 invariant verification).

<table style="border: 1pt solid black; border-collapse: collapse; width: 100%;">
  <tr>
    <td style="width: 200pt; padding: 4pt;">
      <img src="../../assets/images/rolled-paper.png"
           alt="50% of a 200pt cell = 100pt wide"
           style="width: 50%;" />
    </td>
    <td style="padding: 4pt;">
      Right-cell prose for layout reference.
    </td>
  </tr>
</table>

## Section B — `<img>` with style="width: 100pt" (absolute)

<img src="../../assets/images/rolled-paper.png"
     alt="absolute 100pt"
     style="width: 100pt;" />

## Section C — `<img>` with style="width: 50%; height: 30pt" (mixed)

<table style="border: 1pt solid black; border-collapse: collapse; width: 100%;">
  <tr>
    <td style="width: 200pt; padding: 4pt;">
      <img src="../../assets/images/rolled-paper.png"
           alt="mixed: 50% width × 30pt height"
           style="width: 50%; height: 30pt;" />
    </td>
    <td style="padding: 4pt;">
      Mixed sizing reference cell.
    </td>
  </tr>
</table>

## Section D — `<img>` with no style (intrinsic-px fallback per U-8df478)

This routes through `Pipeline::resolve` (the Markdown image rule) and
emits `md_image_bytes(..., intrinsic_pt, none)` — bit-for-bit
identical to the Markdown image path.

<img src="../../assets/images/rolled-paper.png"
     alt="no style hint, intrinsic-px sized" />

## Section E — Markdown image equivalent (control for U-8df478 invariant)

Same image, same path, no style hints, expressed as Markdown:

![Markdown control](../../assets/images/rolled-paper.png)

The Typst output for Section D and Section E should both call
`md_image_bytes(...)` with the same sizing arguments. Section D goes
through `Pipeline::resolve`; Section E goes through the same. The
emitted Typst sub-string for the image-load+layout call must match
modulo the surrounding paragraph wrapping (which differs because
Section E is just a Markdown paragraph and Section D is a stray
inline-HTML img).

## Section F — `<img>` with unreachable URL (Image-bucket warning)

Triggers the Image-bucket warning + placeholder image (Decision §4 +
U-954c6b). With `--strict` this MUST escalate to exit-code-6.

<img src="https://nonexistent.invalid/never-resolves.png"
     alt="placeholder will display this alt text"
     style="width: 100pt;" />

## Section G — `<img>` with file:// to a non-existent file

Local file path that does not exist; same Image-bucket warning path.

<img src="../../assets/images/this-file-does-not-exist.png"
     alt="local missing file placeholder"
     style="width: 100pt;" />

End of fixture.
