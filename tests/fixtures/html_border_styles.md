# HTML border-styles fixture (W-650a51, D-875e4b §3c v2, §7b)

Border-style v2 amendment: `solid` / `dashed` / `dotted` / `none` must
render as four visually distinct stroke shapes. Per Decision §3c v2:

- `solid` → `stroke: <thickness> + <color>` (default dash, omit
  `dash:` arg)
- `dashed` → `stroke: (..., dash: "dashed")`
- `dotted` → `stroke: (..., dash: "dotted")`
- `none` → `stroke: none`

Other style keywords (`double`, `groove`, `ridge`, `inset`, `outset`)
degrade to `Solid` per the Architect's call documented in §3c v2.

## Solid border

<table style="border: 1px solid black;">
  <tr>
    <td>solid border row 1</td>
    <td>cell</td>
  </tr>
  <tr>
    <td>solid border row 2</td>
    <td>cell</td>
  </tr>
</table>

## Dashed border

<table style="border: 1px dashed black;">
  <tr>
    <td>dashed border row 1</td>
    <td>cell</td>
  </tr>
  <tr>
    <td>dashed border row 2</td>
    <td>cell</td>
  </tr>
</table>

## Dotted border

<table style="border: 1px dotted black;">
  <tr>
    <td>dotted border row 1</td>
    <td>cell</td>
  </tr>
  <tr>
    <td>dotted border row 2</td>
    <td>cell</td>
  </tr>
</table>

## None border

<table style="border: 1px none black;">
  <tr>
    <td>no border row 1</td>
    <td>cell</td>
  </tr>
  <tr>
    <td>no border row 2</td>
    <td>cell</td>
  </tr>
</table>

## Degrade-to-solid: double

<table style="border: 2px double black;">
  <tr>
    <td>double border (degrades to solid)</td>
    <td>cell</td>
  </tr>
</table>

End of fixture.
