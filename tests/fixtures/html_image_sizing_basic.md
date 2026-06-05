# HTML image-sizing fixture (W-650a51, D-875e4b §4)

Exercises the image-sizing composition rule: `<img>` WITHOUT inline
style hints falls through to the Markdown intrinsic-pixel path
(`Pipeline::resolve`); `<img>` WITH `style="width: ...; height: ...;"`
hints bypasses sizing and lets Typst layout resolve the CSS lengths
through `Pipeline::fetch_for_html`.

The Markdown image path remains untouched — same load-bearing
invariant as U-8df478.

## Section A — `<img>` without size hints (Case 2: intrinsic-px)

A bare HTML `<img>` outside any table, no `style`. Per Decision §4
this routes through `Pipeline::resolve` (identical to the Markdown
image rule) and emits `md_image_bytes(bytes, fmt, intrinsic_pt, none)`.

<img src="../../assets/images/rolled-paper.png"
     alt="A rolled-up piece of paper" />

## Section B — `<img>` with width only (Case 1)

Width is given as a percentage; the height is `auto`. Per Decision §4b
this emits `md_image_bytes(bytes, fmt, <css-width>, auto)` after
`fetch_for_html`.

<img src="../../assets/images/rolled-paper.png"
     alt="A rolled-up piece of paper, width 50%"
     style="width: 50%;" />

## Section C — `<img>` with both width and height (Case 3)

Both lengths explicit (one absolute pt, one auto-via-CSS).

<img src="../../assets/images/rolled-paper.png"
     alt="A rolled-up piece of paper, sized 80pt by auto"
     style="width: 80pt; height: auto;" />

## Section D — `<img>` inside an HTML table cell

The canonical README layout: an image inside a `<td>` whose own
`width: <pct>` controls the column, with `style="width: 100%; height: auto;"`
on the image so Typst layout fills the cell width.

<table style="width: 100%; border-collapse: collapse;">
  <tr>
    <td style="width: 60%; padding: 0 4pt; vertical-align: middle;">
      <img src="../../assets/images/rolled-paper.png"
           alt="A rolled-up piece of paper, in cell"
           style="width: 100%; height: auto;" />
    </td>
    <td style="width: 40%; padding: 0 8pt; vertical-align: middle;">
      Prose alongside the image — proves the image-sizing path
      composes cleanly with the HTML-table path.
    </td>
  </tr>
</table>

End of fixture.
