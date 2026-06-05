# QA HTML table in blockquote (W-8f4312, D-875e4b §7d.6, §6c)

Decision §6c composition: a blockquote containing an HTML table. The
table is honored inside the blockquote (parser recognizes it; emit
goes through `#md_html_table(...)` at the HtmlBlock end inside the
blockquote frame). Result: `#md_blockquote[ ... #md_html_table(...) ... ]`.

Note: pulldown-cmark may NOT pass the indented table HTML through as
a single `Tag::HtmlBlock` event — block-level HTML inside a blockquote
can be tokenised as inline-HTML or text under some grammars. The QA
behaviour expectation is: the conversion succeeds, no panic, and
either (a) the table renders as a Typst table inside the blockquote,
OR (b) the literal HTML pass-through renders as raw HTML inside the
blockquote. Both are acceptable per the Decision's
graceful-composition rule.

## Section A — Blockquote with simple HTML table

> Quoted prose introducing the table:
>
> <table>
>   <tr>
>     <th>Col 1</th>
>     <th>Col 2</th>
>   </tr>
>   <tr>
>     <td>cell A</td>
>     <td>cell B</td>
>   </tr>
> </table>
>
> Quoted prose after the table.

## Section B — Blockquote without HTML table (control)

> Plain quoted prose with no embedded HTML, for visual layout
> reference alongside Section A.

## Section C — Blockquote followed by top-level table

Top-level prose.

> A blockquote with NO HTML table inside.

<table>
  <tr>
    <td>Top-level table immediately after the blockquote.</td>
  </tr>
</table>

End of fixture.
