# QA HTML inline outside-cells extended (W-8f4312, D-875e4b §7d.7, §2h v2)

Extends `html_inline_outside_cells.md`: ALL 5 recognized inline
elements (`<b>`, `<strong>`, `<i>`, `<em>`, `<br>`) in EVERY inline
context — paragraphs (incl. nested formatting), headings (h1-h6),
list items (ordered + unordered + nested), blockquotes, GFM table
cells, mixed-Markdown-and-HTML — plus attribute-tolerance cases.

## Section A — Paragraph with all 5 elements

A paragraph using <b>bold</b>, <strong>strong</strong>, <i>italic</i>,
<em>emphasized</em>, with a hard<br>break.

## Section B — Headings (h1 through h6)

# H1 with <b>bold</b>

## H2 with <strong>strong</strong>

### H3 with <i>italic</i>

#### H4 with <em>emphasis</em>

##### H5 with hard<br>break

###### H6 with all five: <b>b</b><strong>s</strong><i>i</i><em>e</em><br>break

## Section C — Unordered list items with formatters

- Item with <b>bold</b>
- Item with <strong>strong</strong>
- Item with <i>italic</i>
- Item with <em>emphasis</em>
- Item with hard<br>break inside
- Item with all combined: <b>b</b> <strong>s</strong> <i>i</i> <em>e</em> done<br>here

## Section D — Ordered list items with formatters

1. Item with <b>bold</b>
2. Item with <strong>strong</strong>
3. Item with <i>italic</i>
4. Item with <em>emphasis</em>
5. Hard<br>break here

## Section E — Nested list items

- Outer item
  - Inner with <b>bold</b>
  - Inner with <i>italic</i>
- Another outer with <strong>strong</strong>
  - Inner with hard<br>break

## Section F — Blockquote with all formatters

> Quoted with <b>bold</b>, <strong>strong</strong>, <i>italic</i>,
> <em>em</em>, plus a hard<br>break and a closing prose line.
>
> Second quoted line: <b>bold continues</b> and <em>em</em>, with
> <strong>nested <i>combo</i> formatting</strong>.

## Section G — GFM Markdown table cells with HTML formatters

The Markdown GFM table is a different code path from HTML tables;
its cells exercise the inline-HTML drain hook at cell boundaries.

| Col 1 | Col 2 |
|-------|-------|
| <b>bold cell</b> | <i>italic cell</i> |
| <strong>strong</strong> | <em>em</em> |
| line<br>break | combined <b>b</b><i>i</i> |

## Section H — Nested formatting (stack-based pairing)

A paragraph with <b>bold and <i>italic inside</i> still bold</b>,
then <strong>strong with <em>em inside</em> still strong</strong>,
and <b><strong>double <i><em>quad</em></i></strong></b> nesting.

## Section I — Mixed Markdown + HTML formatters

A paragraph mixing **Markdown bold** with <b>HTML bold</b>, and
*Markdown italic* with <i>HTML italic</i>, and **Markdown bold _with
italic_** alongside <b>HTML bold <i>with italic</i></b>.

## Section J — Attribute tolerance (per Decision §2h v2)

The recognized opens classify by tag-name only; attributes are
ignored (not propagated to Typst output, not warned about).

A paragraph with <b class="foo">bold-with-class</b>, and
<i id="x">italic-with-id</i>, and <strong data-test="bar">strong-with-data</strong>,
and <em style="color: red">em-with-style</em>, and a self-closing
<br/> hardbreak after the formatters.

## Section K — Self-closing `<br>` in three forms

First<br>second<br/>third<br />fourth<br
>fifth (newline before `>`).

End of fixture.
