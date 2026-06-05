# HTML table CSS basic fixture (W-650a51, D-875e4b §3, §7b)

Exercises a representative subset of the 8 recognized CSS properties
on a single table:

- `padding` (shorthand: 1-value, 4-value)
- `text-align` (`left`, `center`, `right`)
- `vertical-align` (`top`, `middle`, `bottom`)
- `background-color` (16 named CSS Level 1 colors + 3-/6-digit hex)
- `border` (tri-token order-independent)
- `border-collapse: collapse` (Decision §2f)
- `width` (% / pt / em / px → pt)

<table style="border: 1px solid black; border-collapse: collapse; width: 80%;">
  <thead>
    <tr>
      <th style="background-color: silver; padding: 6pt 8pt; text-align: center;">Item</th>
      <th style="background-color: #c0c0c0; padding: 6pt 8pt; text-align: center;">Qty</th>
      <th style="background-color: silver; padding: 6pt 8pt; text-align: right;">Price</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td style="padding: 4pt; vertical-align: top; text-align: left;">Widget</td>
      <td style="padding: 4pt; vertical-align: middle; text-align: center;">42</td>
      <td style="padding: 4pt; vertical-align: bottom; text-align: right; background-color: #eeeeee;">$1.99</td>
    </tr>
    <tr>
      <td style="padding: 2pt 4pt 2pt 4pt;">Gadget</td>
      <td style="padding: 2pt 4pt 2pt 4pt; background-color: yellow;">7</td>
      <td style="padding: 2pt 4pt 2pt 4pt; text-align: right;">$12.50</td>
    </tr>
  </tbody>
</table>

Trailing prose to re-enter normal Markdown emission.
