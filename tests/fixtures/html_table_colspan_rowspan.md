# HTML table colspan/rowspan fixture (W-650a51, D-875e4b §2c, §7b)

Exercises both `colspan` and `rowspan` per Decision §2c — emit goes
through `table.cell(colspan: N, rowspan: M, ...)` in the
`#md_html_table(...)` invocation.

<table style="border: 1px solid black; border-collapse: collapse;">
  <thead>
    <tr>
      <th colspan="3">Quarterly results 2025</th>
    </tr>
    <tr>
      <th>Quarter</th>
      <th>Revenue</th>
      <th>Notes</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td rowspan="2">Q1+Q2</td>
      <td>$1.2M</td>
      <td>strong start</td>
    </tr>
    <tr>
      <td>$1.4M</td>
      <td>steady growth</td>
    </tr>
    <tr>
      <td>Q3</td>
      <td colspan="2">$1.6M (notes folded into the revenue cell)</td>
    </tr>
  </tbody>
</table>

Trailing prose.
