# HTML table basic fixture (W-650a51, D-875e4b §1d–§2g, §7b)

Minimal positive case for the recognized HTML-table subset: a single
`<table>` with `<thead>` and `<tbody>`, no inline styling. Exercises
the recognition path at `FrameKind::HtmlBlock` end and the emission
of `#md_html_table(...)` with a wrapping `table.header(...)` for the
`<thead>` row (Decision §2c).

<table>
  <thead>
    <tr>
      <th>Column A</th>
      <th>Column B</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>row 1, A</td>
      <td>row 1, B</td>
    </tr>
    <tr>
      <td>row 2, A</td>
      <td>row 2, B</td>
    </tr>
  </tbody>
</table>

Trailing prose so the round-trip walks past the table back into normal
Markdown emission.
