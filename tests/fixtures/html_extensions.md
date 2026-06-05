# HTML extensions fixture (W-650a51, D-875e4b §1–§4, §6, §7b)

This fixture exercises the HTML subset md2pdf renders through a Typst
table primitive: `<table>` / `<thead>` / `<tbody>` / `<tr>` / `<td>` /
`<th>` (with `colspan` / `rowspan` and a small CSS subset) plus the
outside-cell inline-HTML formatters `<b>` / `<i>` / `<strong>` /
`<em>` / `<br>`.

## Section A — Basic table

A minimal `<table>` with `<thead>` + `<tbody>`, no styling.

<table>
  <thead>
    <tr>
      <th>Name</th>
      <th>Role</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td>Alice</td>
      <td>Engineer</td>
    </tr>
    <tr>
      <td>Bob</td>
      <td>Designer</td>
    </tr>
  </tbody>
</table>

## Section B — Border styles (D-875e4b §3c)

Three visually distinct border styles: solid, dashed, dotted.

<table style="border: 1px solid black;">
  <tr>
    <td>solid</td>
    <td>border</td>
  </tr>
</table>

<table style="border: 1px dashed black;">
  <tr>
    <td>dashed</td>
    <td>border</td>
  </tr>
</table>

<table style="border: 1px dotted black;">
  <tr>
    <td>dotted</td>
    <td>border</td>
  </tr>
</table>

## Section C — colspan + rowspan

A 3-column grid where the head spans, and rowspan applies to a body cell.

<table style="border: 1px solid black; border-collapse: collapse;">
  <thead>
    <tr>
      <th colspan="3">Quarterly results</th>
    </tr>
    <tr>
      <th>Quarter</th>
      <th>Revenue</th>
      <th>Notes</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td rowspan="2">2025</td>
      <td>$1.2M</td>
      <td>Q1</td>
    </tr>
    <tr>
      <td>$1.4M</td>
      <td>Q2</td>
    </tr>
  </tbody>
</table>

## Section D — CSS styling

Padding, vertical-align, text-align, background-color, named colors.

<table style="border: 1px solid black; border-collapse: collapse;">
  <thead>
    <tr>
      <th style="background-color: silver; padding: 8px;">Item</th>
      <th style="background-color: silver; padding: 8px; text-align: right;">Qty</th>
    </tr>
  </thead>
  <tbody>
    <tr>
      <td style="padding: 6px; vertical-align: top;">Widget</td>
      <td style="padding: 6px; text-align: right;">42</td>
    </tr>
    <tr>
      <td style="padding: 6px; background-color: #eeeeee;">Gadget</td>
      <td style="padding: 6px; text-align: right; background-color: #eeeeee;">7</td>
    </tr>
  </tbody>
</table>

## Section E — collapsed-border default (D-875e4b §2f)

`border-collapse: collapse` with no explicit `border` produces a 1pt
black border per Decision-mandated default.

<table style="border-collapse: collapse;">
  <tr>
    <td>collapsed</td>
    <td>default</td>
  </tr>
</table>

## Section F — Outside-cell inline HTML

Recognized formatters in flowing text (paragraph-level pairing per §2h.5):

A paragraph with <b>bold</b>, <i>italic</i>, <strong>strong-bold</strong>,
and <em>emphasized-italic</em> spans, plus a hard<br>break and a
<b>nested <i>combo</i></b> for good measure.

A second paragraph: <strong>cross</strong>-paragraph nesting is not
supported (Client-accepted limit) — formatters auto-close at paragraph
end.

## Section G — Mismatched / unrecognized inline HTML (D-875e4b §6k)

A close tag with no matching open: </b> falls through to literal
pass-through. An unrecognized element <span>like this</span> also
passes through verbatim. A bare `<br>` mid-text<br>linebreaks cleanly.

## Section H — Cell content with formatters and image-less inlines

Inside cell content, `<b>` / `<i>` etc. work the same way.

<table style="border: 1px solid black;">
  <tr>
    <th>Header A</th>
    <th>Header B</th>
  </tr>
  <tr>
    <td>Plain text in <b>bold</b> here</td>
    <td><i>italic</i> body cell</td>
  </tr>
  <tr>
    <td>line one<br>line two</td>
    <td><strong>strong</strong> + <em>em</em></td>
  </tr>
</table>

## Section I — End

The end of the fixture.
