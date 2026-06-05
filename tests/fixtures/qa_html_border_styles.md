# QA HTML border styles (W-8f4312, D-875e4b §7d.9, §3c v2)

Side-by-side visual distinctness verification: solid / dashed / dotted
/ none must produce four visually distinct strokes in BOTH PDF and
PNG outputs (after passing through typst-render → tiny-skia → PNG).

Plus all degrade-to-solid keywords: double, groove, ridge, inset,
outset.

## Section A — Side-by-side distinct border styles (single row)

The four styles in one row to make the visual difference obvious at
a glance.

<table style="border: 2pt solid black; border-collapse: collapse;">
  <tr>
    <td style="border: 2pt solid black; padding: 8pt;">solid</td>
    <td style="border: 2pt dashed black; padding: 8pt;">dashed</td>
    <td style="border: 2pt dotted black; padding: 8pt;">dotted</td>
    <td style="border: 2pt none black; padding: 8pt;">none</td>
  </tr>
</table>

## Section B — Solid (full table border)

<table style="border: 2pt solid black;">
  <tr><td>solid border row 1</td><td>cell</td></tr>
  <tr><td>solid border row 2</td><td>cell</td></tr>
</table>

## Section C — Dashed

<table style="border: 2pt dashed black;">
  <tr><td>dashed row 1</td><td>cell</td></tr>
  <tr><td>dashed row 2</td><td>cell</td></tr>
</table>

## Section D — Dotted

<table style="border: 2pt dotted black;">
  <tr><td>dotted row 1</td><td>cell</td></tr>
  <tr><td>dotted row 2</td><td>cell</td></tr>
</table>

## Section E — None

<table style="border: 2pt none black;">
  <tr><td>none row 1</td><td>cell</td></tr>
  <tr><td>none row 2</td><td>cell</td></tr>
</table>

## Section F — Degrade-to-solid: double

<table style="border: 3pt double black;">
  <tr><td>double (degrades to solid)</td><td>cell</td></tr>
</table>

## Section G — Degrade-to-solid: groove

<table style="border: 3pt groove black;">
  <tr><td>groove (degrades to solid)</td><td>cell</td></tr>
</table>

## Section H — Degrade-to-solid: ridge

<table style="border: 3pt ridge black;">
  <tr><td>ridge (degrades to solid)</td><td>cell</td></tr>
</table>

## Section I — Degrade-to-solid: inset

<table style="border: 3pt inset black;">
  <tr><td>inset (degrades to solid)</td><td>cell</td></tr>
</table>

## Section J — Degrade-to-solid: outset

<table style="border: 3pt outset black;">
  <tr><td>outset (degrades to solid)</td><td>cell</td></tr>
</table>

End of fixture.
