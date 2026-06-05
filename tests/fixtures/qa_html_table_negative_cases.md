# QA HTML table negative cases (W-8f4312, D-875e4b §7d.5, §6e)

Malformed inputs that MUST gracefully degrade — never panic, never
escalate beyond Emitter-bucket warnings. Per the Client-accepted
simplifications in U-ad8c6c v2: nested tables degrade to silent text
fallback; unrecognized properties/attributes/elements are silently
dropped; tag mismatches recover sensibly.

## Section A — Malformed CSS in style attr (silently dropped)

The malformed declarations are stripped; recognized declarations
still apply.

<table style="width: ; height: !!; padding: 4pt; background-color: yellow;">
  <tr>
    <td>cell with malformed `width` and `height` declarations stripped, but `padding` and `background-color` honored</td>
  </tr>
</table>

## Section B — Unrecognized CSS property (silently dropped)

`z-index` and `font-family` are not in the recognized 8-property
subset; they're silently dropped. `padding` survives.

<table style="border: 1pt solid black; border-collapse: collapse;">
  <tr>
    <td style="z-index: 999; font-family: Arial; padding: 4pt;">recognized props survive; the unknowns are dropped</td>
  </tr>
</table>

## Section C — Unrecognized HTML attribute (silently dropped)

`foo`, `data-test`, and `aria-label` are not in the recognized 4-attr
subset (`style`, `colspan`, `rowspan`, `align` per Decision); they're
silently dropped.

<table style="border: 1pt solid black; border-collapse: collapse;">
  <tr>
    <td foo="bar" data-test="ignored" aria-label="dropped">cell renders normally</td>
  </tr>
</table>

## Section D — Unrecognized child element inside cell

`<aside>` is not in the recognized table-subset; behavior per the
fall-through rule (rendered as raw inline HTML or silently elided).
Either way the surrounding cell is fine.

<table style="border: 1pt solid black; border-collapse: collapse;">
  <tr>
    <td>before <aside>random aside</aside> after</td>
  </tr>
</table>

## Section E — Nested HTML table inside a cell (silent text fallback)

Per Client-accepted simplification (U-ad8c6c v2 + §6e): outer table
renders normally; inner `<table>` degrades to silent text fallback
inside the outer cell.

<table style="border: 1pt solid black; border-collapse: collapse;">
  <tr>
    <td>
      Outer cell with nested:
      <table>
        <tr>
          <td>inner cell A</td>
          <td>inner cell B</td>
        </tr>
      </table>
      End of outer cell.
    </td>
  </tr>
</table>

## Section F — Tag mismatch / structurally invalid

These pathological inputs trigger ParseFailed (Emitter-bucket warning)
and fall through to literal raw pass-through. The point is: NO PANIC
and NO crash.

<table>
  <tr>
    <td>cell with no closing tag in row
  </tr>
</table>

## Section G — Unclosed `<table>`

<table>
  <tr>
    <td>row in unclosed table</td>
  </tr>

(No `</table>` — ParseFailed via "unterminated <table>".)

## Section H — Mismatched th/td

<table>
  <tr>
    <th>header opened</td>
  </tr>
</table>

End of fixture.
