# QA HTML table colspan/rowspan extended (W-8f4312, D-875e4b §7d.3)

Extends `html_table_colspan_rowspan.md` with multi-cell spans, span
combinations, spans inside thead/tbody, and adjacent-span layouts.

## Section A — Pure colspan on single row

<table style="border: 1pt solid black; border-collapse: collapse;">
  <tr>
    <td colspan="4">single cell spanning 4 columns</td>
  </tr>
  <tr>
    <td>a</td>
    <td>b</td>
    <td>c</td>
    <td>d</td>
  </tr>
</table>

## Section B — Pure rowspan on single column

<table style="border: 1pt solid black; border-collapse: collapse;">
  <tr>
    <td rowspan="3">single cell spanning 3 rows</td>
    <td>row 1</td>
  </tr>
  <tr>
    <td>row 2</td>
  </tr>
  <tr>
    <td>row 3</td>
  </tr>
</table>

## Section C — Combined colspan + rowspan on one cell

<table style="border: 1pt solid black; border-collapse: collapse;">
  <tr>
    <td colspan="2" rowspan="2">2x2 super-cell</td>
    <td>top-right</td>
  </tr>
  <tr>
    <td>middle-right</td>
  </tr>
  <tr>
    <td>bottom-1</td>
    <td>bottom-2</td>
    <td>bottom-3</td>
  </tr>
</table>

## Section D — Spans inside thead AND tbody

<table style="border: 1pt solid black; border-collapse: collapse;">
  <thead>
    <tr>
      <th colspan="3">Spanning header — repeats across page breaks per Decision §2c</th>
    </tr>
    <tr>
      <th>Col 1</th>
      <th>Col 2</th>
      <th>Col 3</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td rowspan="2">body rowspan</td>
      <td>r1c2</td>
      <td>r1c3</td>
    </tr>
    <tr>
      <td colspan="2">body colspan inside tbody</td>
    </tr>
  </tbody>
</table>

## Section E — Adjacent rowspans on the same row

<table style="border: 1pt solid black; border-collapse: collapse;">
  <tr>
    <td rowspan="2">A spans 2</td>
    <td rowspan="2">B spans 2</td>
    <td>C r1</td>
  </tr>
  <tr>
    <td>C r2</td>
  </tr>
</table>

## Section F — Asymmetric span overlay (5 columns × 4 rows)

<table style="border: 1pt solid black; border-collapse: collapse;">
  <tr>
    <th>1</th>
    <th>2</th>
    <th>3</th>
    <th>4</th>
    <th>5</th>
  </tr>
  <tr>
    <td colspan="2">1-2 wide</td>
    <td rowspan="2">3 tall</td>
    <td colspan="2">4-5 wide</td>
  </tr>
  <tr>
    <td>1</td>
    <td>2</td>
    <td>4</td>
    <td>5</td>
  </tr>
  <tr>
    <td colspan="5">full-width footer</td>
  </tr>
</table>

End of fixture.
