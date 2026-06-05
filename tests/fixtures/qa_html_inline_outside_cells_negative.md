# QA HTML inline negative cases (W-8f4312, D-875e4b §7d.8, §6k v2)

Mismatched closes, dangling-open drain at frame end, unrecognized
inline tags, malformed inline HTML, br self-closing variants. ALL
must fall through gracefully via `#md_inline_html(raw)` literal
pass-through — NO panic, NO warning.

## Section A — Mismatched close tags

A paragraph opens <b>bold but closes wrong</i> — the `</i>` falls
through to literal pass-through because the top of the stack is `<b>`,
not `<i>`. Then </b> closes correctly.

A paragraph with bare close: </strong> with no preceding open. Falls
through to literal.

## Section B — Cross-paragraph dangling (drain hook fires)

A paragraph that opens <b>bold but never closes it before paragraph
end (drain hook auto-closes).

A second paragraph — the leading text is NOT bold (the open did
NOT bleed across the paragraph boundary per Decision §6k v2 drain
rule).

A third paragraph that opens <i>italic and another <strong>open
bold but again never closes — drain fires twice (one per opener).

A fourth paragraph — also not formatted.

## Section C — Unrecognized inline tags (literal pass-through)

A paragraph with <u>underline</u> (not in the 5-element subset),
<font color="red">font</font>, <span>span</span>, <mark>mark</mark>,
<code>inline code</code>, <kbd>kbd</kbd>, <small>small</small>,
<s>strike</s>, <sup>sup</sup>, <sub>sub</sub> — all fall through to
literal `#md_inline_html(...)` pass-through.

## Section D — Malformed inline HTML (literal pass-through)

A paragraph with `<b extra="` (unterminated open quote — pulldown
likely emits this as InlineHtml with the partial fragment), then
the `</` half-close-tag, then a bare `<>` empty-angle, then a
nested-bracket `<<x>>` — all fall through to literal.

## Section E — All `<br>` variants accepted

First<br>second (canonical), then<br/>third (XHTML), then<br />fourth
(XHTML with space), then<BR>fifth (uppercase), then<Br>sixth (mixed
case). All five forms must emit `#md_hardbreak()`.

## Section F — Recognized opens with unknown attributes (per §2h v2 attribute tolerance)

A paragraph with <b style="color: red" onclick="alert(1)">bold-with-attrs</b>
— attributes are ignored (no warning), classification still succeeds,
the Typst output is `*bold-with-attrs*`. The `onclick` is silently
dropped (graceful — security risk is moot because the Typst output
ignores it and PDF has no script execution).

## Section G — Empty open + immediate close

A paragraph with <b></b> empty bold (open then immediate close —
emits `**` which Typst sees as empty markup; should not panic).

A paragraph with <b><i></i></b> empty nested formatters.

## Section H — Whitespace inside tags

A paragraph with <b >space-after-name</b > (space before `>`), and
<i  >double-space</i> (multiple spaces), and < b>leading-space-in-name</ b>
(space after `<` — likely classifies differently; should not panic).

End of fixture.
