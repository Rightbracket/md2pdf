# Pagebreak edge cases (W-pagebreak fixture per D-875e4b §7d)

This fixture exercises the full set of pagebreak directive edge cases
enumerated in U-976c35 and D-875e4b §6c. Each section is named with
the case it covers and the expected outcome.

## Section A — leading pagebreak (suppressed)

The pagebreak directive **immediately follows** this paragraph but
falls AFTER it, so this section is actually section A's content. The
LEADING pagebreak (if any) is the one at the very top of the document
— this fixture has none, so leading suppression is exercised by
fixture variants in the test code (see `pagebreak_at_document_start_is_suppressed`
in `src/emitter.rs`).

## Section B — single mid-document pagebreak (one `#pagebreak()` emitted)

Content above the next pagebreak.

<!-- pagebreak -->

Content below the pagebreak.

## Section C — back-to-back mid-document pagebreaks (two `#pagebreak()` emitted)

Content immediately before the back-to-back pair.

<!-- pagebreak -->

<!-- pagebreak -->

Content immediately after — the two consecutive directives produce
ONE empty page between this content and the previous, per U-976c35
"back-to-back: each directive inserts a page break ... honor it
literally rather than collapsing".

## Section D — `page-break` alias recognized

Content above.

<!-- page-break -->

Content below — the hyphenated `page-break` form is also recognized
per U-976c35.

## Section E — case-insensitivity and whitespace-tolerance

Content above.

<!--   PAGEBREAK   -->

Content below — uppercase + extra whitespace inside the comment is
tolerated.

## Section F — pagebreak inside a fenced code block (NOT detected)

The directive inside a code fence is `Event::Text`, not `Event::Html`
(pulldown-cmark's CommonMark-conformant block discrimination, see
D-875e4b §6g). Recognition never fires; the comment renders as
literal code source.

```
<!-- pagebreak -->
```

## Section G — pagebreak inside a blockquote (falls through to literal)

Per D-875e4b §1c, §6c (edge case 2): the pagebreak is non-top-level
(parent EnvFrame is `BlockQuote`, not document body), so recognition
falls through to `md_inline_html` literal pass-through. No
`#pagebreak()` is emitted; no warning is fired (graceful per §6c).

> Quoted content above.
>
> <!-- pagebreak -->
>
> Quoted content below.

## Section H — pagebreak with payload form (NOT recognized)

U-976c35 strict-no-payload contract: any payload other than literal
`pagebreak` / `page-break` is rejected. The following comment renders
as literal source via `md_inline_html`.

<!-- pagebreak: top -->

Content after the unrecognized comment.

## Section I — pagebreak inside an HTML table cell (deferred)

Per the W-pagebreak brief done_definition #8: this case depends on
W-html-table (which introduces HTML-table parsing and thus the
TableCell parent context for inside-cell content). Because W-html-table
has not yet landed at the time of this Work, the in-table-cell case
is **stubbed** here as a literal `<!-- pagebreak -->` at top-level
followed by the trailing-suppression pattern. W-html-table will
extend this fixture with a proper HTML-table-cell variant.

(stub — will be extended by W-html-extensions-qa per D-875e4b §7d)

## Section Z — trailing pagebreak (suppressed)

The very next directive is the trailing pagebreak. Per U-976c35
"at the very end of the document: no-op", it is silently dropped.

<!-- pagebreak -->
