# HTML inline outside-cells fixture (W-650a51, D-875e4b §2h v2, §6k v2)

Exercises the v2 amendment: recognized inline-HTML formatters
(`<b>`, `<strong>`, `<i>`, `<em>`, `<br>`) work in any inline
context — paragraphs, headings, list items, blockquotes — not just
table cells. Streaming Typst-markup with stack-based pairing per
Decision §2h v2; mismatched closes fall through to `md_inline_html(...)`
per §6k v2.

## Section A — Paragraph-level inline formatters

A paragraph with <b>bold</b>, <strong>strong-bold</strong>,
<i>italic</i>, <em>emphasized-italic</em>, plus a hard<br>break
between clauses.

A second paragraph: nested <b>bold with <i>italic inside</i> still
bold</b> — stack-based pairing keeps the markers balanced.

## Section B — Heading

### A heading with <b>bold</b> and <em>emphasis</em> inline

A heading body still flows after the heading frame closes.

## Section C — List items

Recognized formatters work inside list items too.

- Item with <b>bold</b> emphasis
- Item with <i>italic</i> emphasis
- Item with <strong>strong</strong> + <em>em</em> combined
- Item with a hard<br>break inside

## Section D — Blockquote

A blockquote with formatters per Decision §6c (composition with
blockquote frames):

> A quoted line with <b>bold</b> text and an <i>italic</i> span,
> plus a <strong>strong-bold</strong> emphasis.
>
> A second quoted line with a hard<br>break in the middle.

## Section E — Cross-paragraph auto-close (drain hook)

If a paragraph ends with an unclosed open tag, the drain hook fires
at the paragraph-end frame and emits the close marker so Typst
markup remains balanced. The next paragraph starts fresh — the open
tag does NOT bleed across the paragraph boundary.

A paragraph that opens <b>bold but never closes it (drain fires).

A second paragraph — the leading text is NOT bold.

## Section F — Mismatched close falls through

A close tag with no matching open: </b> renders as literal
pass-through via `md_inline_html(...)` per Decision §6k v2.
Unrecognized tags such as <span>this</span> and <font>that</font>
also pass through verbatim.

## Section G — Self-closing variants of `<br>`

The three accepted forms — `<br>`, `<br/>`, `<br />` — all emit
`md_hardbreak()`. First<br>second<br/>third<br />fourth.

End of fixture.
