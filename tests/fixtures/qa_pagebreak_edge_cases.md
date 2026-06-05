# QA pagebreak edge cases (W-8f4312, D-875e4b §7d v2)

This fixture extends `pagebreak_edge_cases.md` with cases that depend
on W-html-table being landed: in-table-cell fall-through, in-blockquote
fall-through (with the blockquote also containing a table), and an
expanded whitespace/case matrix. Per D-875e4b §1c the directive is
top-level-only; these fixtures verify the non-top-level paths surface
the comment as literal `md_inline_html(...)` and emit ZERO
`#pagebreak()` calls.

## Section A — Pagebreak inside an HTML table cell falls through

Top-level prose above the table.

<table>
  <tr>
    <td>Cell with a <!-- pagebreak --> directive that must fall through to literal pass-through (no `#pagebreak()` emitted; the comment text survives in the cell content because `self.stack` is non-empty when the HtmlBlock end fires).</td>
    <td>Plain neighbouring cell.</td>
  </tr>
</table>

Trailing prose.

## Section B — Pagebreak between rows (outside cells, inside table)

The `<!-- pagebreak -->` between rows is structurally invalid HTML
inside a `<table>`; the parser may fall back to ParseFailed or
swallow the comment (Client-accepted simplification). Either path is
acceptable; what is NOT acceptable is emitting a `#pagebreak()`.

<table>
  <tr>
    <td>row above</td>
  </tr>
  <!-- pagebreak -->
  <tr>
    <td>row below</td>
  </tr>
</table>

## Section C — Pagebreak inside a blockquote containing prose only

Per pagebreak_edge_cases.md Section G: blockquote pagebreak falls
through to literal. This section reproduces the case for QA
regression coverage (the existing fixture asserts `count == 5` and
must not be modified in-place).

> Quoted prose above.
>
> <!-- pagebreak -->
>
> Quoted prose below.

## Section D — Expanded whitespace / case-insensitive matrix (top-level)

Each of these MUST be recognized and emit a `#pagebreak()`:

<!--PAGEBREAK-->

<!--   page-break   -->

<!-- Page-Break -->

<!--	pagebreak	-->

<!-- pageBREAK -->

<!--PAGE-BREAK-->

## Section E — Form variations that MUST NOT be recognized

Each of these is rejected per U-976c35 strict-no-payload rule and
falls through to literal pass-through:

<!-- pagebreak top -->

<!-- pagebreaks -->

<!-- page break -->

<!--pagebreak2-->

<!-- pagebreak; -->

End of fixture.
