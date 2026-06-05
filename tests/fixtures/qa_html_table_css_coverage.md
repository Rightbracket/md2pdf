# QA HTML table CSS coverage (W-8f4312, D-875e4b §7d.2 v2)

Exercises ALL 8 recognized CSS properties × ALL 16 CSS Level 1 named
colors × hex (3-digit and 6-digit) × padding shorthand (1/2/3/4-value)
× border tri-token order variations.

Recognized 8 properties (per U-ad8c6c v2): `width`, `height`, `padding`,
`vertical-align`, `text-align`, `border-collapse`, `border`,
`background-color`.

CSS Level 1 named colors: black, silver, gray, white, maroon, red,
purple, fuchsia, green, lime, olive, yellow, navy, blue, teal, aqua.

## Section A — All 8 properties on one table

<table style="border: 1px solid black; border-collapse: collapse; width: 80%; height: 40pt;">
  <thead>
    <tr>
      <th style="background-color: silver; padding: 6pt; text-align: center; vertical-align: middle;">Header</th>
      <th style="background-color: silver; padding: 6pt; text-align: center; vertical-align: middle;">Header</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td style="padding: 4pt; vertical-align: top; text-align: left;">top-left</td>
      <td style="padding: 4pt; vertical-align: bottom; text-align: right;">bottom-right</td>
    </tr>
  </tbody>
</table>

## Section B — All 16 CSS Level 1 named colors as backgrounds

<table style="border: 1px solid black; border-collapse: collapse;">
  <tr>
    <td style="background-color: black; padding: 4pt;">black</td>
    <td style="background-color: silver; padding: 4pt;">silver</td>
    <td style="background-color: gray; padding: 4pt;">gray</td>
    <td style="background-color: white; padding: 4pt;">white</td>
  </tr>
  <tr>
    <td style="background-color: maroon; padding: 4pt;">maroon</td>
    <td style="background-color: red; padding: 4pt;">red</td>
    <td style="background-color: purple; padding: 4pt;">purple</td>
    <td style="background-color: fuchsia; padding: 4pt;">fuchsia</td>
  </tr>
  <tr>
    <td style="background-color: green; padding: 4pt;">green</td>
    <td style="background-color: lime; padding: 4pt;">lime</td>
    <td style="background-color: olive; padding: 4pt;">olive</td>
    <td style="background-color: yellow; padding: 4pt;">yellow</td>
  </tr>
  <tr>
    <td style="background-color: navy; padding: 4pt;">navy</td>
    <td style="background-color: blue; padding: 4pt;">blue</td>
    <td style="background-color: teal; padding: 4pt;">teal</td>
    <td style="background-color: aqua; padding: 4pt;">aqua</td>
  </tr>
</table>

## Section C — Hex color forms (3-digit, 6-digit)

<table style="border: 1px solid black; border-collapse: collapse;">
  <tr>
    <td style="background-color: #f00; padding: 4pt;">#f00 (3-digit red)</td>
    <td style="background-color: #FF0000; padding: 4pt;">#FF0000 (6-digit red)</td>
    <td style="background-color: #abc; padding: 4pt;">#abc (3-digit)</td>
    <td style="background-color: #aabbcc; padding: 4pt;">#aabbcc (6-digit)</td>
  </tr>
</table>

## Section D — padding shorthand (1, 2, 3, 4 values)

<table style="border: 1px solid black; border-collapse: collapse;">
  <tr>
    <td style="padding: 4pt;">1-value: 4pt all sides</td>
    <td style="padding: 2pt 8pt;">2-value: 2pt v, 8pt h</td>
    <td style="padding: 2pt 4pt 8pt;">3-value: top right+left bottom</td>
    <td style="padding: 1pt 2pt 4pt 8pt;">4-value: t r b l</td>
  </tr>
</table>

## Section E — border tri-token order variations

The Decision allows `<width> <style> <color>` in any order; the
mini-parser classifies tokens by shape (length / keyword / color).

<table style="border: 1pt solid black;">
  <tr><td>width style color</td></tr>
</table>

<table style="border: solid 2pt red;">
  <tr><td>style width color</td></tr>
</table>

<table style="border: green 1pt dashed;">
  <tr><td>color width style</td></tr>
</table>

<table style="border: dotted 3pt #0000ff;">
  <tr><td>style width color (hex)</td></tr>
</table>

<table style="border: blue dashed 2pt;">
  <tr><td>color style width</td></tr>
</table>

<table style="border: 4pt #ff00ff solid;">
  <tr><td>width color (hex) style</td></tr>
</table>

## Section F — Width units variety (% / pt / em / px → pt)

<table style="border: 1pt solid black; border-collapse: collapse;">
  <tr>
    <td style="width: 25%;">25%</td>
    <td style="width: 50pt;">50pt</td>
    <td style="width: 5em;">5em</td>
    <td style="width: 80px;">80px</td>
  </tr>
</table>

## Section G — text-align values

<table style="border: 1pt solid black; border-collapse: collapse; width: 80%;">
  <tr>
    <td style="text-align: left;">left-aligned</td>
    <td style="text-align: center;">center-aligned</td>
    <td style="text-align: right;">right-aligned</td>
  </tr>
</table>

## Section H — vertical-align values (with explicit cell height)

<table style="border: 1pt solid black; border-collapse: collapse; width: 60%;">
  <tr>
    <td style="vertical-align: top; height: 40pt;">top-aligned</td>
    <td style="vertical-align: middle; height: 40pt;">middle-aligned</td>
    <td style="vertical-align: bottom; height: 40pt;">bottom-aligned</td>
  </tr>
</table>

End of fixture.
