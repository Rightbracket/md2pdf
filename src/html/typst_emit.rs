//! IR → Typst source emission for HTML tables (D-875e4b §2b–§2g, §3, §4).
//!
//! ## Public surface
//!
//! - `emit_html_table_typst(&HtmlTable, &mut Pipeline) -> String` — top-level
//!   entry. Returns one Typst source line of the form
//!   `#md_html_table(columns, align_grid, inset_grid, fill_grid, stroke, cells)`
//!   ready for the emitter to splice into its current write target. The
//!   string has no leading or trailing whitespace; the caller wraps it in
//!   `write_block(...)` cadence (which adds a `\n\n`).
//!
//! ## Image resolution
//!
//! `<img>` elements inside cells are resolved during this call:
//! - With explicit `width`/`height` style hints → `Pipeline::fetch_for_html`
//!   (bytes only, no built-in sizing); emitter passes the CSS-derived
//!   width/height to `md_image_bytes`.
//! - Without style hints → `Pipeline::resolve` (existing flow; intrinsic-px
//!   sizing per U-8df478, identical to Markdown images).
//!
//! Both paths route fetch failures through `Pipeline.warnings`. The caller
//! must invoke `drain_image_warnings()` AFTER `emit_html_table_typst` to
//! bridge them into the unified `WarningCollector`.
//!
//! ## Logical-grid layout
//!
//! `colspan`/`rowspan` are applied to a logical 2D grid in two passes:
//!   1. Walk source rows in document order, placing each cell at its
//!      anchor position (advancing past any cells already occupied by
//!      a prior cell's rowspan extension).
//!   2. Use the resulting placements to build the per-cell grids
//!      (`align_grid`, `inset_grid`, `fill_grid`) — every position
//!      occupied by a cell carries that cell's style; gap positions
//!      use defaults.
//!
//! The flat `cells` array stays in source order (Typst's `table` element
//! handles span reconstruction natively via `table.cell(colspan:, rowspan:)`).
//!
//! ## Helper signature (assets/theme.typ)
//!
//! ```typst
//! #let md_html_table(columns, align_grid, inset_grid, fill_grid, stroke, cells) = {
//!     table(
//!         columns: columns,
//!         align: (col, row) => align_grid.at(row).at(col),
//!         inset: (col, row) => inset_grid.at(row).at(col),
//!         fill:  (col, row) => fill_grid.at(row).at(col),
//!         stroke: stroke,
//!         ..cells,
//!     )
//! }
//! ```
//!
//! Per D-875e4b §2b: "Exact helper signature is illustrative — the
//! Developer may simplify or restructure." The simplification here vs.
//! the Decision's text: NO `header_rows` parameter / no `table.header(...)`
//! repetition; instead, `<th>` cells are pre-bolded by wrapping their
//! content in `*…*`. This avoids the difficult interaction between
//! `table.header` and arbitrary colspan/rowspan that may straddle
//! head/body. The visual result (header cells bold) matches the
//! existing Markdown `md_table` helper.

use crate::emitter::{escape_typst_markup, typst_bytes_literal, typst_string};
use crate::html::css::{
    BorderCollapse, BorderStyle, CssBorder, ImgStyle,
};
use crate::html::parser::{Cell, CellContent, CellKind, HtmlTable, Inline, Row};
use crate::image_pipeline::{
    EmbeddedFormat, ImageRequest, Pipeline, ResolvedHtmlImage, ResolvedImage,
};

// =============================================================================
// Public entry point
// =============================================================================

/// Emit a `#md_html_table(...)` invocation for `table`, resolving
/// inline `<img>` references via `pipeline`.
///
/// The returned string is the complete Typst source for the table,
/// without a trailing newline. Caller is expected to wrap with
/// `write_block(...)` for the spacing convention used elsewhere.
pub fn emit_html_table_typst(table: &HtmlTable, pipeline: &mut Pipeline) -> String {
    let layout = compute_logical_layout(table);

    // Defensive guard: an empty <table></table> with no rows. Render
    // nothing — caller's `write_block` will add a stray newline pair
    // but Typst tolerates that.
    if layout.logical_rows == 0 || layout.logical_cols == 0 {
        // Emit a syntactically-valid empty md_html_table call so the
        // helper doesn't crash. Empty arrays + none stroke + empty
        // cells.
        return "#md_html_table((), (), (), (), none, ())".to_string();
    }

    let columns_str = emit_columns(&layout);
    let align_grid_str = emit_align_grid(&layout);
    let inset_grid_str = emit_inset_grid(&layout);
    let fill_grid_str = emit_fill_grid(&layout, table);
    let stroke_str = emit_table_stroke(table);
    let cells_str = emit_cells(&layout, pipeline);

    format!(
        "#md_html_table({columns}, {align}, {inset}, {fill}, {stroke}, {cells})",
        columns = columns_str,
        align = align_grid_str,
        inset = inset_grid_str,
        fill = fill_grid_str,
        stroke = stroke_str,
        cells = cells_str,
    )
}

// =============================================================================
// Internal: logical-layout pass
// =============================================================================

/// Result of walking the IR rows + applying `colspan`/`rowspan`.
struct LogicalLayout<'a> {
    /// All source rows, head_rows then body_rows, in document order.
    rows: Vec<&'a Row>,
    /// How many of the first `rows` entries originated from `<thead>`.
    /// Used to wrap their cells in `table.header(...)` per Decision §2c
    /// so Typst repeats them across page breaks.
    head_row_count: usize,
    /// Total logical rows in the grid (≥ source row count if rowspan
    /// extends past last row).
    logical_rows: usize,
    /// Total logical columns in the grid (max sum-of-colspan across
    /// rows, or extension by rowspan).
    logical_cols: usize,
    /// Per-source-row, per-source-cell: where in the logical grid the
    /// cell's anchor sits and how far its span extends. Indexed in the
    /// same order as `rows[i].cells`.
    placements: Vec<Vec<Placement>>,
}

#[derive(Clone, Copy)]
struct Placement {
    anchor_row: usize,
    anchor_col: usize,
    rowspan: usize,
    colspan: usize,
}

fn compute_logical_layout(table: &HtmlTable) -> LogicalLayout<'_> {
    let head_count = table.head_rows.len();
    let mut all_rows: Vec<&Row> = Vec::new();
    all_rows.extend(table.head_rows.iter());
    all_rows.extend(table.body_rows.iter());

    // 2D bool grid tracking which logical positions are already
    // claimed by a previously-placed cell.
    let mut occupied: Vec<Vec<bool>> = Vec::new();
    let mut placements: Vec<Vec<Placement>> = Vec::new();
    let mut max_cols: usize = 0;

    for (row_idx, row) in all_rows.iter().enumerate() {
        // Ensure the occupied grid has a slot for this row.
        while occupied.len() <= row_idx {
            occupied.push(Vec::new());
        }

        let mut col_cursor: usize = 0;
        let mut row_placements: Vec<Placement> = Vec::with_capacity(row.cells.len());

        for cell in &row.cells {
            // Skip past columns already claimed by a prior rowspan.
            loop {
                let claimed =
                    occupied[row_idx].get(col_cursor).copied().unwrap_or(false);
                if claimed {
                    col_cursor += 1;
                } else {
                    break;
                }
            }

            let anchor_row = row_idx;
            let anchor_col = col_cursor;
            let colspan = cell.colspan.max(1);
            let rowspan = cell.rowspan.max(1);

            // Mark every occupied position.
            for r in anchor_row..(anchor_row + rowspan) {
                while occupied.len() <= r {
                    occupied.push(Vec::new());
                }
                for c in anchor_col..(anchor_col + colspan) {
                    while occupied[r].len() <= c {
                        occupied[r].push(false);
                    }
                    occupied[r][c] = true;
                }
            }

            row_placements.push(Placement {
                anchor_row,
                anchor_col,
                rowspan,
                colspan,
            });

            col_cursor += colspan;
            max_cols = max_cols.max(col_cursor);
        }

        placements.push(row_placements);
    }

    let logical_rows = occupied.len().max(all_rows.len());

    // Recompute max_cols across the whole occupied grid in case a
    // rowspan-only column from a prior row pushes the column count
    // past any single row's cursor extent.
    let mut logical_cols = max_cols;
    for occ_row in &occupied {
        logical_cols = logical_cols.max(occ_row.len());
    }

    LogicalLayout {
        rows: all_rows,
        head_row_count: head_count,
        logical_rows,
        logical_cols,
        placements,
    }
}

// =============================================================================
// Internal: column widths (first-row-wins per D-875e4b §2d)
// =============================================================================

/// Build the `columns: (...)` argument. First row preferred (head over
/// body); cells without explicit `width` produce Typst `auto`. Per-cell
/// explicit widths apply to the cell's anchor logical column; remaining
/// colspan slots fall back to `auto` (v1 simplification — Client-accepted
/// per Decision §2d).
fn emit_columns(layout: &LogicalLayout) -> String {
    let n = layout.logical_cols;
    if n == 0 {
        return "()".to_string();
    }

    let mut widths: Vec<String> = vec!["auto".to_string(); n];

    if layout.rows.is_empty() {
        return tuple_array(&widths);
    }
    let first_row_idx = 0;
    let first_row = layout.rows[first_row_idx];
    let first_placements = &layout.placements[first_row_idx];

    for (cell, placement) in first_row.cells.iter().zip(first_placements.iter()) {
        if let Some(w) = &cell.style.width {
            let col = placement.anchor_col;
            if col < widths.len() {
                widths[col] = w.to_typst();
            }
        }
    }

    tuple_array(&widths)
}

// =============================================================================
// Internal: per-cell grids (align, inset, fill)
// =============================================================================

/// Build the 2D `align_grid` per Decision §2e: `<halign> + <valign>`
/// per cell.
///
/// Defaults:
/// - `<th>` cell: `center + horizon` (matches HTML / browser default
///   styling for header cells).
/// - `<td>` cell: `left + horizon` (CSS table-cell default; matches
///   user expectations for HTML-derived layout tables).
/// - Gap (no cell at this logical position): `left + horizon`.
///
/// Explicit `text-align`/`vertical-align` on the cell overrides.
fn emit_align_grid(layout: &LogicalLayout) -> String {
    let mut grid: Vec<Vec<String>> = vec![
        vec!["left + horizon".to_string(); layout.logical_cols];
        layout.logical_rows
    ];

    for (row_idx, row) in layout.rows.iter().enumerate() {
        let placements = &layout.placements[row_idx];
        for (cell, placement) in row.cells.iter().zip(placements.iter()) {
            let halign = cell.style.text_align.map(|h| h.to_typst()).unwrap_or(
                if cell.kind == CellKind::Th { "center" } else { "left" },
            );
            let valign = cell
                .style
                .vertical_align
                .map(|v| v.to_typst())
                .unwrap_or("horizon");
            let align = format!("{halign} + {valign}");
            fill_span(&mut grid, placement, layout, &align);
        }
    }

    emit_2d_array(&grid)
}

/// Build the 2D `inset_grid`. Each entry is either a uniform length
/// or a `(top:, right:, bottom:, left:)` dictionary.
///
/// Default: `5pt` (typical browser cell padding analog). Explicit
/// `padding` shorthand on a cell overrides.
fn emit_inset_grid(layout: &LogicalLayout) -> String {
    let default_inset = "5pt";
    let mut grid: Vec<Vec<String>> = vec![
        vec![default_inset.to_string(); layout.logical_cols];
        layout.logical_rows
    ];

    for (row_idx, row) in layout.rows.iter().enumerate() {
        let placements = &layout.placements[row_idx];
        for (cell, placement) in row.cells.iter().zip(placements.iter()) {
            if let Some(p) = &cell.style.padding {
                let inset_dict = format!(
                    "(top: {}, right: {}, bottom: {}, left: {})",
                    p.top.to_typst(),
                    p.right.to_typst(),
                    p.bottom.to_typst(),
                    p.left.to_typst(),
                );
                fill_span(&mut grid, placement, layout, &inset_dict);
            }
        }
    }

    emit_2d_array(&grid)
}

/// Build the 2D `fill_grid`. Each entry is either a Typst color
/// expression or `none`.
///
/// Default: table-level `background-color`, or `none`. Explicit
/// per-cell `background-color` overrides.
fn emit_fill_grid(layout: &LogicalLayout, table: &HtmlTable) -> String {
    let default_fill = table
        .style
        .background_color
        .as_ref()
        .map(|c| c.to_typst())
        .unwrap_or_else(|| "none".to_string());
    let mut grid: Vec<Vec<String>> = vec![
        vec![default_fill.clone(); layout.logical_cols];
        layout.logical_rows
    ];

    for (row_idx, row) in layout.rows.iter().enumerate() {
        let placements = &layout.placements[row_idx];
        for (cell, placement) in row.cells.iter().zip(placements.iter()) {
            if let Some(c) = &cell.style.background_color {
                fill_span(&mut grid, placement, layout, &c.to_typst());
            }
        }
    }

    emit_2d_array(&grid)
}

/// Helper: write `value` into every position of `grid` covered by
/// `placement` (its anchor + rowspan/colspan extent), bounded to the
/// logical grid dimensions.
fn fill_span(
    grid: &mut [Vec<String>],
    placement: &Placement,
    layout: &LogicalLayout,
    value: &str,
) {
    for r in placement.anchor_row..(placement.anchor_row + placement.rowspan) {
        for c in placement.anchor_col..(placement.anchor_col + placement.colspan) {
            if r < layout.logical_rows && c < layout.logical_cols {
                grid[r][c] = value.to_string();
            }
        }
    }
}

// =============================================================================
// Internal: table-level stroke (D-875e4b §2f)
// =============================================================================

/// Resolve the table-level stroke per Decision §2f:
///
/// - Explicit `border` attribute on the table → use that (per §3c v2
///   border-style mapping).
/// - `border-collapse: collapse` with no explicit `border` → default
///   `1pt + black`.
/// - Otherwise (no `border`, no `collapse`) → `none`.
///
/// Cell-level `border` overrides are NOT honored at v1 — documented
/// limitation. They are parsed and stored on `Cell.style.border` but
/// not consulted at emit time. The helper signature does not carry a
/// per-cell stroke grid; v2 may add one.
fn emit_table_stroke(table: &HtmlTable) -> String {
    if let Some(border) = &table.style.border {
        return emit_border_value(border);
    }
    if matches!(table.style.border_collapse, Some(BorderCollapse::Collapse)) {
        return "(thickness: 1pt, paint: rgb(\"#000000\"))".to_string();
    }
    "none".to_string()
}

/// Resolve a CSS `border` shorthand to a Typst stroke expression.
fn emit_border_value(border: &CssBorder) -> String {
    let bs = border.style.unwrap_or(BorderStyle::Solid);
    if matches!(bs, BorderStyle::None) {
        return "none".to_string();
    }
    let thickness = border
        .thickness
        .as_ref()
        .map(|t| t.to_typst())
        .unwrap_or_else(|| "1pt".to_string());
    let color = border
        .color
        .as_ref()
        .map(|c| c.to_typst())
        .unwrap_or_else(|| "rgb(\"#000000\")".to_string());
    match bs {
        BorderStyle::Solid => {
            format!("(thickness: {thickness}, paint: {color})")
        }
        BorderStyle::Dashed => format!(
            "(thickness: {thickness}, paint: {color}, dash: \"dashed\")"
        ),
        BorderStyle::Dotted => format!(
            "(thickness: {thickness}, paint: {color}, dash: \"dotted\")"
        ),
        BorderStyle::None => unreachable!("handled above"),
    }
}

// =============================================================================
// Internal: cell flat array
// =============================================================================

/// Build the `cells: (...)` argument: source-order cells with
/// `table.cell(colspan:, rowspan:)` wrappers when spanning. Bodies
/// rendered as Typst content blocks (`[…]`).
fn emit_cells(layout: &LogicalLayout, pipeline: &mut Pipeline) -> String {
    // Per Decision §2c: head-row cells wrap inside `table.header(...)`
    // so Typst auto-repeats them across page breaks. If there are no
    // head rows the wrapper is omitted entirely (an empty
    // `table.header()` would render visually identical, but Typst
    // emits a layout group regardless and we'd rather keep the source
    // minimal for the no-thead case).
    let mut head_entries: Vec<String> = Vec::new();
    let mut body_entries: Vec<String> = Vec::new();

    for (row_idx, row) in layout.rows.iter().enumerate() {
        let placements = &layout.placements[row_idx];
        let in_header = row_idx < layout.head_row_count;
        for (cell, placement) in row.cells.iter().zip(placements.iter()) {
            let entry = emit_one_cell(cell, placement, pipeline);
            if in_header {
                head_entries.push(entry);
            } else {
                body_entries.push(entry);
            }
        }
    }

    let mut out_entries: Vec<String> = Vec::new();
    if !head_entries.is_empty() {
        out_entries.push(format!(
            "table.header({})",
            head_entries.join(", ")
        ));
    }
    out_entries.extend(body_entries);
    tuple_array(&out_entries)
}

fn emit_one_cell(
    cell: &Cell,
    placement: &Placement,
    pipeline: &mut Pipeline,
) -> String {
    let body = emit_cell_body(cell, pipeline);
    if placement.colspan > 1 || placement.rowspan > 1 {
        format!(
            "table.cell(colspan: {}, rowspan: {})[{}]",
            placement.colspan, placement.rowspan, body
        )
    } else {
        format!("[{body}]")
    }
}

fn emit_cell_body(cell: &Cell, pipeline: &mut Pipeline) -> String {
    let CellContent::Inlines(inlines) = &cell.content;
    let inner = emit_inlines(inlines, pipeline);
    if cell.kind == CellKind::Th {
        // Bold the header cell content (matches md_table convention).
        format!("*{inner}*")
    } else {
        inner
    }
}

// =============================================================================
// Internal: inline content
// =============================================================================

/// Emit a `Vec<Inline>` to Typst markup inside a content block.
fn emit_inlines(inlines: &[Inline], pipeline: &mut Pipeline) -> String {
    let mut out = String::new();
    for inl in inlines {
        emit_inline(inl, pipeline, &mut out);
    }
    out
}

fn emit_inline(inl: &Inline, pipeline: &mut Pipeline, out: &mut String) {
    match inl {
        Inline::Text(s) => out.push_str(&escape_typst_markup(s)),
        Inline::Bold(inner) => {
            out.push('*');
            for child in inner {
                emit_inline(child, pipeline, out);
            }
            out.push('*');
        }
        Inline::Italic(inner) => {
            out.push('_');
            for child in inner {
                emit_inline(child, pipeline, out);
            }
            out.push('_');
        }
        Inline::LineBreak => out.push_str("#linebreak()"),
        Inline::Image { src, alt, style } => {
            out.push_str(&emit_image(src, alt, style.as_ref(), pipeline));
        }
    }
}

// =============================================================================
// Internal: image emission (D-875e4b §4b)
// =============================================================================

/// Three cases per D-875e4b §4b:
///
/// - **Case 1/3**: explicit `width` AND/OR `height` style hint →
///   `Pipeline::fetch_for_html`; emit `md_image_bytes` with
///   CSS-derived width/height (unspecified → `auto`).
/// - **Case 2**: no style hints → `Pipeline::resolve` (the existing
///   Markdown image flow); emit `md_image_bytes` with intrinsic-px
///   sizing OR `md_image_placeholder` on failure.
fn emit_image(
    src: &str,
    alt: &str,
    style: Option<&ImgStyle>,
    pipeline: &mut Pipeline,
) -> String {
    let has_style_hints = style
        .map(|s| s.width.is_some() || s.height.is_some())
        .unwrap_or(false);

    if !has_style_hints {
        // Case 2: identical to Markdown image emit.
        let req = ImageRequest {
            src,
            alt,
            explicit_width: None,
            explicit_height: None,
        };
        let resolved = pipeline.resolve(&req);
        return match resolved {
            ResolvedImage::Embedded {
                format,
                bytes,
                size,
                ..
            } => {
                let format_str = embedded_format_name(format);
                let bytes_lit = typst_bytes_literal(&bytes);
                let height = match size.height_pt {
                    Some(h) => format!("{}pt", h),
                    None => "none".to_string(),
                };
                format!(
                    "#md_image_bytes({}, \"{}\", {}pt, {})",
                    bytes_lit, format_str, size.width_pt, height
                )
            }
            ResolvedImage::Placeholder { display_text, .. } => {
                format!("#md_image_placeholder({})", typst_string(&display_text))
            }
        };
    }

    // Case 1/3: explicit hints. Use fetch_for_html (no built-in sizing)
    // and emit width/height from CSS.
    let req = ImageRequest {
        src,
        alt,
        explicit_width: None,
        explicit_height: None,
    };
    let result = pipeline.fetch_for_html(&req);

    let width_expr = style
        .and_then(|s| s.width.as_ref().map(|w| w.to_typst()))
        .unwrap_or_else(|| "auto".to_string());
    let height_expr = style
        .and_then(|s| s.height.as_ref().map(|h| h.to_typst()))
        .unwrap_or_else(|| "auto".to_string());

    match result {
        ResolvedHtmlImage::Bytes { format, bytes, .. } => {
            let format_str = embedded_format_name(format);
            let bytes_lit = typst_bytes_literal(&bytes);
            format!(
                "#md_image_bytes({}, \"{}\", {}, {})",
                bytes_lit, format_str, width_expr, height_expr
            )
        }
        ResolvedHtmlImage::Placeholder { display_text, .. } => {
            format!("#md_image_placeholder({})", typst_string(&display_text))
        }
    }
}

fn embedded_format_name(f: EmbeddedFormat) -> &'static str {
    match f {
        EmbeddedFormat::Png => "png",
        EmbeddedFormat::Jpeg => "jpeg",
        EmbeddedFormat::Svg => "svg",
    }
}

// =============================================================================
// Internal: array-emission helpers
// =============================================================================

/// Format a flat list of strings as a Typst array literal: `(a, b, c)`,
/// `()`, `(x,)` (the trailing comma disambiguates a 1-element array
/// from a parenthesized expression).
fn tuple_array(items: &[String]) -> String {
    if items.is_empty() {
        return "()".to_string();
    }
    if items.len() == 1 {
        return format!("({},)", items[0]);
    }
    format!("({})", items.join(", "))
}

/// Format a 2D grid (rows × cols) as a Typst array of arrays.
fn emit_2d_array(grid: &[Vec<String>]) -> String {
    let rows: Vec<String> = grid.iter().map(|r| tuple_array(r)).collect();
    tuple_array(&rows)
}

// Re-export referenced parser type alias so the prelude `use` block
// resolves cleanly.
#[allow(unused_imports)]
use crate::html::css::CellStyle as _CellStyle;
#[allow(unused_imports)]
use crate::html::css::ImgStyle as _ImgStyle;

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::html::css::{
        CellStyle, CssColor, CssLength, CssPadding, HAlign, VAlign,
    };
    use crate::html::parser::{Cell, CellContent, CellKind, HtmlTable, Inline, Row};
    use crate::image_pipeline::{CapturingSink, PipelineBuilder};

    // --- Test helpers -------------------------------------------------------

    /// Build a Pipeline that captures warnings rather than writing
    /// stderr. Base dir is the project root (irrelevant for tests
    /// that don't actually load images).
    fn test_pipeline() -> Pipeline {
        PipelineBuilder::new(std::env::temp_dir())
            .warn_sink(Box::new(CapturingSink::new()))
            .build()
    }

    /// Build a minimal cell with text content.
    fn td_text(s: &str) -> Cell {
        Cell {
            kind: CellKind::Td,
            colspan: 1,
            rowspan: 1,
            style: CellStyle::default(),
            content: CellContent::Inlines(vec![Inline::Text(s.to_string())]),
        }
    }

    fn th_text(s: &str) -> Cell {
        Cell {
            kind: CellKind::Th,
            colspan: 1,
            rowspan: 1,
            style: CellStyle::default(),
            content: CellContent::Inlines(vec![Inline::Text(s.to_string())]),
        }
    }

    fn row_of(cells: Vec<Cell>) -> Row {
        Row { cells }
    }

    fn empty_table() -> HtmlTable {
        HtmlTable {
            style: CellStyle::default(),
            head_rows: vec![],
            body_rows: vec![],
        }
    }

    fn simple_table_one_row(cells: Vec<Cell>) -> HtmlTable {
        HtmlTable {
            style: CellStyle::default(),
            head_rows: vec![],
            body_rows: vec![row_of(cells)],
        }
    }

    // --- tuple_array & emit_2d_array ---------------------------------------

    #[test]
    fn tuple_array_empty() {
        assert_eq!(tuple_array(&[]), "()");
    }

    #[test]
    fn tuple_array_one() {
        assert_eq!(tuple_array(&["x".to_string()]), "(x,)");
    }

    #[test]
    fn tuple_array_two() {
        assert_eq!(
            tuple_array(&["a".to_string(), "b".to_string()]),
            "(a, b)"
        );
    }

    #[test]
    fn emit_2d_array_basic() {
        let grid = vec![
            vec!["a".to_string(), "b".to_string()],
            vec!["c".to_string(), "d".to_string()],
        ];
        assert_eq!(emit_2d_array(&grid), "((a, b), (c, d))");
    }

    #[test]
    fn emit_2d_array_single_row() {
        let grid = vec![vec!["a".to_string(), "b".to_string()]];
        // Outer 1-element + inner 2-elements.
        assert_eq!(emit_2d_array(&grid), "((a, b),)");
    }

    // --- Logical layout (no spanning) ---------------------------------------

    #[test]
    fn layout_no_spanning_one_row() {
        let table = simple_table_one_row(vec![td_text("a"), td_text("b"), td_text("c")]);
        let layout = compute_logical_layout(&table);
        assert_eq!(layout.logical_rows, 1);
        assert_eq!(layout.logical_cols, 3);
        assert_eq!(layout.placements.len(), 1);
        assert_eq!(layout.placements[0].len(), 3);
        for (i, p) in layout.placements[0].iter().enumerate() {
            assert_eq!(p.anchor_row, 0);
            assert_eq!(p.anchor_col, i);
            assert_eq!(p.rowspan, 1);
            assert_eq!(p.colspan, 1);
        }
    }

    #[test]
    fn layout_no_spanning_two_rows() {
        let table = HtmlTable {
            style: CellStyle::default(),
            head_rows: vec![],
            body_rows: vec![
                row_of(vec![td_text("a"), td_text("b")]),
                row_of(vec![td_text("c"), td_text("d")]),
            ],
        };
        let layout = compute_logical_layout(&table);
        assert_eq!(layout.logical_rows, 2);
        assert_eq!(layout.logical_cols, 2);
    }

    // --- Logical layout (colspan) ------------------------------------------

    #[test]
    fn layout_colspan_two() {
        let table = simple_table_one_row(vec![
            Cell {
                colspan: 2,
                ..td_text("hdr")
            },
            td_text("c"),
        ]);
        let layout = compute_logical_layout(&table);
        assert_eq!(layout.logical_rows, 1);
        assert_eq!(layout.logical_cols, 3);
        assert_eq!(layout.placements[0][0].colspan, 2);
        assert_eq!(layout.placements[0][0].anchor_col, 0);
        assert_eq!(layout.placements[0][1].anchor_col, 2);
    }

    // --- Logical layout (rowspan) ------------------------------------------

    #[test]
    fn layout_rowspan_two() {
        // Row 0: a (rowspan=2), b
        // Row 1: c        ← b's column, plus c sits at col 1 due to a's rowspan
        // Wait — actually: a is at col 0 with rowspan 2; b is at col 1. Row 1: c
        //   should land at col 1 because col 0 is occupied by a. So row 1 has
        //   one cell at (1, 1).
        let table = HtmlTable {
            style: CellStyle::default(),
            head_rows: vec![],
            body_rows: vec![
                row_of(vec![
                    Cell {
                        rowspan: 2,
                        ..td_text("a")
                    },
                    td_text("b"),
                ]),
                row_of(vec![td_text("c")]),
            ],
        };
        let layout = compute_logical_layout(&table);
        assert_eq!(layout.logical_rows, 2);
        assert_eq!(layout.logical_cols, 2);
        assert_eq!(layout.placements[0][0].anchor_col, 0);
        assert_eq!(layout.placements[0][0].rowspan, 2);
        assert_eq!(layout.placements[0][1].anchor_col, 1);
        // Row 1's only cell is forced to col 1 because (1, 0) is
        // claimed by a's rowspan.
        assert_eq!(layout.placements[1][0].anchor_row, 1);
        assert_eq!(layout.placements[1][0].anchor_col, 1);
    }

    // --- emit_columns -------------------------------------------------------

    #[test]
    fn columns_all_auto_when_no_widths() {
        let table = simple_table_one_row(vec![td_text("a"), td_text("b")]);
        let layout = compute_logical_layout(&table);
        assert_eq!(emit_columns(&layout), "(auto, auto)");
    }

    #[test]
    fn columns_first_row_widths_applied() {
        let mut c1 = td_text("a");
        c1.style.width = Some(CssLength::Percent(50.0));
        let mut c2 = td_text("b");
        c2.style.width = Some(CssLength::Pt(100.0));
        let table = simple_table_one_row(vec![c1, c2]);
        let layout = compute_logical_layout(&table);
        assert_eq!(emit_columns(&layout), "(50%, 100pt)");
    }

    #[test]
    fn columns_partial_widths() {
        let mut c1 = td_text("a");
        c1.style.width = Some(CssLength::Percent(50.0));
        let table = simple_table_one_row(vec![c1, td_text("b"), td_text("c")]);
        let layout = compute_logical_layout(&table);
        assert_eq!(emit_columns(&layout), "(50%, auto, auto)");
    }

    #[test]
    fn columns_px_to_pt_conversion() {
        let mut c = td_text("a");
        c.style.width = Some(CssLength::Px(100.0));
        let table = simple_table_one_row(vec![c]);
        let layout = compute_logical_layout(&table);
        // 100px × 0.75 = 75pt
        assert_eq!(emit_columns(&layout), "(75pt,)");
    }

    // --- emit_align_grid ---------------------------------------------------

    #[test]
    fn align_grid_default_td() {
        let table = simple_table_one_row(vec![td_text("a"), td_text("b")]);
        let layout = compute_logical_layout(&table);
        let g = emit_align_grid(&layout);
        assert_eq!(g, "((left + horizon, left + horizon),)");
    }

    #[test]
    fn align_grid_default_th_centers() {
        let table = simple_table_one_row(vec![th_text("H")]);
        let layout = compute_logical_layout(&table);
        let g = emit_align_grid(&layout);
        // th defaults to center + horizon
        assert_eq!(g, "((center + horizon,),)");
    }

    #[test]
    fn align_grid_explicit_overrides() {
        let mut c = td_text("a");
        c.style.text_align = Some(HAlign::Right);
        c.style.vertical_align = Some(VAlign::Top);
        let table = simple_table_one_row(vec![c]);
        let layout = compute_logical_layout(&table);
        let g = emit_align_grid(&layout);
        assert_eq!(g, "((right + top,),)");
    }

    // --- emit_inset_grid ---------------------------------------------------

    #[test]
    fn inset_grid_default_5pt() {
        let table = simple_table_one_row(vec![td_text("a")]);
        let layout = compute_logical_layout(&table);
        assert_eq!(emit_inset_grid(&layout), "((5pt,),)");
    }

    #[test]
    fn inset_grid_padding_dict() {
        let mut c = td_text("a");
        c.style.padding = Some(CssPadding {
            top: CssLength::Pt(0.0),
            right: CssLength::Pt(4.0),
            bottom: CssLength::Pt(0.0),
            left: CssLength::Pt(4.0),
        });
        let table = simple_table_one_row(vec![c]);
        let layout = compute_logical_layout(&table);
        let g = emit_inset_grid(&layout);
        assert_eq!(
            g,
            "(((top: 0pt, right: 4pt, bottom: 0pt, left: 4pt),),)"
        );
    }

    // --- emit_fill_grid ----------------------------------------------------

    #[test]
    fn fill_grid_default_none() {
        let table = simple_table_one_row(vec![td_text("a")]);
        let layout = compute_logical_layout(&table);
        let g = emit_fill_grid(&layout, &table);
        assert_eq!(g, "((none,),)");
    }

    #[test]
    fn fill_grid_table_level_default() {
        let mut t = simple_table_one_row(vec![td_text("a"), td_text("b")]);
        t.style.background_color = Some(CssColor {
            hex: "FF0000".to_string(),
        });
        let layout = compute_logical_layout(&t);
        let g = emit_fill_grid(&layout, &t);
        assert_eq!(
            g,
            "((rgb(\"#FF0000\"), rgb(\"#FF0000\")),)"
        );
    }

    #[test]
    fn fill_grid_cell_overrides_table() {
        let mut t = simple_table_one_row(vec![td_text("a"), {
            let mut c = td_text("b");
            c.style.background_color = Some(CssColor {
                hex: "00FF00".to_string(),
            });
            c
        }]);
        t.style.background_color = Some(CssColor {
            hex: "FF0000".to_string(),
        });
        let layout = compute_logical_layout(&t);
        let g = emit_fill_grid(&layout, &t);
        assert_eq!(
            g,
            "((rgb(\"#FF0000\"), rgb(\"#00FF00\")),)"
        );
    }

    // --- emit_table_stroke -------------------------------------------------

    #[test]
    fn stroke_default_none() {
        let t = empty_table();
        assert_eq!(emit_table_stroke(&t), "none");
    }

    #[test]
    fn stroke_collapse_no_border_default_1pt_black() {
        let mut t = empty_table();
        t.style.border_collapse = Some(BorderCollapse::Collapse);
        assert_eq!(
            emit_table_stroke(&t),
            "(thickness: 1pt, paint: rgb(\"#000000\"))"
        );
    }

    #[test]
    fn stroke_explicit_border_solid() {
        let mut t = empty_table();
        t.style.border = Some(CssBorder {
            thickness: Some(CssLength::Pt(2.0)),
            style: Some(BorderStyle::Solid),
            color: Some(CssColor {
                hex: "FF0000".to_string(),
            }),
        });
        assert_eq!(
            emit_table_stroke(&t),
            "(thickness: 2pt, paint: rgb(\"#FF0000\"))"
        );
    }

    #[test]
    fn stroke_explicit_border_dashed() {
        let mut t = empty_table();
        t.style.border = Some(CssBorder {
            thickness: Some(CssLength::Pt(2.0)),
            style: Some(BorderStyle::Dashed),
            color: Some(CssColor {
                hex: "0000FF".to_string(),
            }),
        });
        assert_eq!(
            emit_table_stroke(&t),
            "(thickness: 2pt, paint: rgb(\"#0000FF\"), dash: \"dashed\")"
        );
    }

    #[test]
    fn stroke_explicit_border_dotted() {
        let mut t = empty_table();
        t.style.border = Some(CssBorder {
            thickness: None,
            style: Some(BorderStyle::Dotted),
            color: None,
        });
        assert_eq!(
            emit_table_stroke(&t),
            "(thickness: 1pt, paint: rgb(\"#000000\"), dash: \"dotted\")"
        );
    }

    #[test]
    fn stroke_explicit_border_none() {
        let mut t = empty_table();
        t.style.border = Some(CssBorder {
            thickness: None,
            style: Some(BorderStyle::None),
            color: None,
        });
        assert_eq!(emit_table_stroke(&t), "none");
    }

    // --- emit_inlines / inline content -------------------------------------

    #[test]
    fn inlines_text_escaped() {
        let mut p = test_pipeline();
        let inlines = vec![Inline::Text("a*b".to_string())];
        let s = emit_inlines(&inlines, &mut p);
        // `*` is escaped by escape_typst_markup.
        assert_eq!(s, "a\\*b");
    }

    #[test]
    fn inlines_bold_wraps() {
        let mut p = test_pipeline();
        let inlines = vec![Inline::Bold(vec![Inline::Text("hi".to_string())])];
        let s = emit_inlines(&inlines, &mut p);
        assert_eq!(s, "*hi*");
    }

    #[test]
    fn inlines_italic_wraps() {
        let mut p = test_pipeline();
        let inlines = vec![Inline::Italic(vec![Inline::Text("hi".to_string())])];
        let s = emit_inlines(&inlines, &mut p);
        assert_eq!(s, "_hi_");
    }

    #[test]
    fn inlines_nested_bold_italic() {
        let mut p = test_pipeline();
        let inlines = vec![Inline::Bold(vec![Inline::Italic(vec![Inline::Text(
            "x".to_string(),
        )])])];
        let s = emit_inlines(&inlines, &mut p);
        assert_eq!(s, "*_x_*");
    }

    #[test]
    fn inlines_linebreak() {
        let mut p = test_pipeline();
        let inlines = vec![Inline::LineBreak];
        let s = emit_inlines(&inlines, &mut p);
        assert_eq!(s, "#linebreak()");
    }

    // --- Image emission (placeholder path — no real fetch) -----------------

    #[test]
    fn emit_image_no_style_resolves_to_placeholder_for_missing() {
        let mut p = test_pipeline();
        // Bogus path → resolve fails → placeholder.
        let s = emit_image("/nonexistent/path/foo.png", "alt text", None, &mut p);
        assert!(s.starts_with("#md_image_placeholder("), "got: {s}");
        assert!(s.contains("alt text"), "alt text in display: {s}");
    }

    #[test]
    fn emit_image_with_style_uses_fetch_for_html() {
        let mut p = test_pipeline();
        let style = ImgStyle {
            width: Some(CssLength::Percent(100.0)),
            height: None, // → "auto"
        };
        // Bogus → placeholder.
        let s = emit_image(
            "/nonexistent/path/foo.png",
            "img alt",
            Some(&style),
            &mut p,
        );
        assert!(s.starts_with("#md_image_placeholder("), "got: {s}");
    }

    #[test]
    fn emit_image_with_unsupported_scheme_placeholder() {
        let mut p = test_pipeline();
        let s = emit_image("ftp://example.com/x.png", "alt", None, &mut p);
        // ftp → unsupported scheme → placeholder.
        assert!(s.starts_with("#md_image_placeholder("));
    }

    // --- emit_one_cell wrapping --------------------------------------------

    #[test]
    fn cell_no_span_simple_brackets() {
        let mut p = test_pipeline();
        let cell = td_text("hello");
        let placement = Placement {
            anchor_row: 0,
            anchor_col: 0,
            rowspan: 1,
            colspan: 1,
        };
        let s = emit_one_cell(&cell, &placement, &mut p);
        assert_eq!(s, "[hello]");
    }

    #[test]
    fn cell_th_bolded() {
        let mut p = test_pipeline();
        let cell = th_text("Header");
        let placement = Placement {
            anchor_row: 0,
            anchor_col: 0,
            rowspan: 1,
            colspan: 1,
        };
        let s = emit_one_cell(&cell, &placement, &mut p);
        assert_eq!(s, "[*Header*]");
    }

    #[test]
    fn cell_colspan_wraps() {
        let mut p = test_pipeline();
        let cell = Cell {
            colspan: 3,
            ..td_text("merged")
        };
        let placement = Placement {
            anchor_row: 0,
            anchor_col: 0,
            rowspan: 1,
            colspan: 3,
        };
        let s = emit_one_cell(&cell, &placement, &mut p);
        assert_eq!(s, "table.cell(colspan: 3, rowspan: 1)[merged]");
    }

    #[test]
    fn cell_rowspan_wraps() {
        let mut p = test_pipeline();
        let cell = Cell {
            rowspan: 2,
            ..td_text("vertical")
        };
        let placement = Placement {
            anchor_row: 0,
            anchor_col: 0,
            rowspan: 2,
            colspan: 1,
        };
        let s = emit_one_cell(&cell, &placement, &mut p);
        assert_eq!(s, "table.cell(colspan: 1, rowspan: 2)[vertical]");
    }

    // --- Top-level emit_html_table_typst -----------------------------------

    #[test]
    fn empty_table_emits_safe_invocation() {
        let mut p = test_pipeline();
        let t = empty_table();
        let s = emit_html_table_typst(&t, &mut p);
        // Defensive: should produce a syntactically-valid call with
        // empty arrays.
        assert_eq!(s, "#md_html_table((), (), (), (), none, ())");
    }

    #[test]
    fn one_row_one_cell_minimal() {
        let mut p = test_pipeline();
        let t = simple_table_one_row(vec![td_text("hi")]);
        let s = emit_html_table_typst(&t, &mut p);
        // Verify shape (not exact byte-equal — too brittle).
        assert!(s.starts_with("#md_html_table("));
        assert!(s.contains("(auto,)"), "columns auto: {s}");
        assert!(s.contains("none"), "stroke none: {s}");
        assert!(s.contains("[hi]"), "cell content: {s}");
    }

    #[test]
    fn one_row_multiple_cells() {
        let mut p = test_pipeline();
        let t = simple_table_one_row(vec![td_text("a"), td_text("b"), td_text("c")]);
        let s = emit_html_table_typst(&t, &mut p);
        assert!(s.contains("(auto, auto, auto)"));
        assert!(s.contains("[a]"));
        assert!(s.contains("[b]"));
        assert!(s.contains("[c]"));
    }

    #[test]
    fn header_cells_bolded_in_body() {
        let mut p = test_pipeline();
        let t = HtmlTable {
            style: CellStyle::default(),
            head_rows: vec![row_of(vec![th_text("Name"), th_text("Age")])],
            body_rows: vec![row_of(vec![td_text("Alice"), td_text("30")])],
        };
        let s = emit_html_table_typst(&t, &mut p);
        assert!(s.contains("[*Name*]"), "header bolded: {s}");
        assert!(s.contains("[*Age*]"));
        assert!(s.contains("[Alice]"));
        assert!(s.contains("[30]"));
    }

    #[test]
    fn thead_rows_wrap_in_table_header() {
        // Per Decision §2c: head-row cells must be enclosed in
        // `table.header(...)` so Typst auto-repeats them across page
        // breaks. The header wrapper sits inside the cells tuple
        // before any body cells.
        let mut p = test_pipeline();
        let t = HtmlTable {
            style: CellStyle::default(),
            head_rows: vec![row_of(vec![th_text("H1"), th_text("H2")])],
            body_rows: vec![row_of(vec![td_text("a"), td_text("b")])],
        };
        let s = emit_html_table_typst(&t, &mut p);
        assert!(
            s.contains("table.header([*H1*], [*H2*])"),
            "head cells must wrap in table.header(...);\nemit: {s}"
        );
        // Body cells stay outside the header wrapper.
        assert!(s.contains("[a]"));
        assert!(s.contains("[b]"));
        // No table.header for body cells.
        let header_count = s.matches("table.header(").count();
        assert_eq!(header_count, 1, "exactly one table.header(...) wrapper");
    }

    #[test]
    fn no_thead_rows_skip_table_header_wrapper() {
        // When the input has no `<thead>` rows, the emit must NOT
        // produce a `table.header()` wrapper.
        let mut p = test_pipeline();
        let t = HtmlTable {
            style: CellStyle::default(),
            head_rows: vec![],
            body_rows: vec![
                row_of(vec![td_text("a"), td_text("b")]),
                row_of(vec![td_text("c"), td_text("d")]),
            ],
        };
        let s = emit_html_table_typst(&t, &mut p);
        assert!(
            !s.contains("table.header"),
            "no table.header wrapper without <thead> rows;\nemit: {s}"
        );
    }

    #[test]
    fn colspan_cell_wrapped_with_table_cell() {
        let mut p = test_pipeline();
        let t = simple_table_one_row(vec![Cell {
            colspan: 3,
            ..td_text("merged")
        }]);
        let s = emit_html_table_typst(&t, &mut p);
        assert!(
            s.contains("table.cell(colspan: 3, rowspan: 1)[merged]"),
            "cell wrapping: {s}"
        );
    }

    #[test]
    fn readme_fixture_shape() {
        // Reproduce the README HTML table fixture's shape (3 cells in
        // 1 row, with width %, padding, vertical-align middle, and
        // text-align center on the middle cell). Verify the emit
        // produces the expected component structure.
        let mut p = test_pipeline();
        let mut c1 = td_text("img1");
        c1.style.width = Some(CssLength::Percent(50.0));
        c1.style.padding = Some(CssPadding {
            top: CssLength::Pt(0.0),
            right: CssLength::Pt(4.0),
            bottom: CssLength::Pt(0.0),
            left: CssLength::Pt(4.0),
        });
        c1.style.vertical_align = Some(VAlign::Middle);

        let mut c2 = td_text("text");
        c2.style.width = Some(CssLength::Percent(20.0));
        c2.style.padding = Some(CssPadding {
            top: CssLength::Pt(0.0),
            right: CssLength::Pt(8.0),
            bottom: CssLength::Pt(0.0),
            left: CssLength::Pt(8.0),
        });
        c2.style.vertical_align = Some(VAlign::Middle);
        c2.style.text_align = Some(HAlign::Center);

        let mut c3 = td_text("img2");
        c3.style.width = Some(CssLength::Percent(30.0));
        c3.style.padding = Some(CssPadding {
            top: CssLength::Pt(0.0),
            right: CssLength::Pt(4.0),
            bottom: CssLength::Pt(0.0),
            left: CssLength::Pt(4.0),
        });
        c3.style.vertical_align = Some(VAlign::Middle);

        let mut t = simple_table_one_row(vec![c1, c2, c3]);
        t.style.width = Some(CssLength::Percent(100.0));
        t.style.border_collapse = Some(BorderCollapse::Collapse);

        let s = emit_html_table_typst(&t, &mut p);

        // Columns: 50%, 20%, 30%
        assert!(s.contains("(50%, 20%, 30%)"), "columns: {s}");
        // Stroke: collapse + no border attr → 1pt + black.
        assert!(
            s.contains("(thickness: 1pt, paint: rgb(\"#000000\"))"),
            "stroke: {s}"
        );
        // Center cell has center + horizon alignment.
        assert!(s.contains("center + horizon"), "center align: {s}");
        // Other cells have left + horizon.
        assert!(s.contains("left + horizon"), "default align: {s}");
        // Insets dictionary present.
        assert!(s.contains("(top: 0pt, right: 4pt, bottom: 0pt, left: 4pt)"));
        assert!(s.contains("(top: 0pt, right: 8pt, bottom: 0pt, left: 8pt)"));
    }

    #[test]
    fn cell_content_with_inline_formatters() {
        let mut p = test_pipeline();
        let cell = Cell {
            kind: CellKind::Td,
            colspan: 1,
            rowspan: 1,
            style: CellStyle::default(),
            content: CellContent::Inlines(vec![
                Inline::Text("Plain ".to_string()),
                Inline::Bold(vec![Inline::Text("bold".to_string())]),
                Inline::Text(" and ".to_string()),
                Inline::Italic(vec![Inline::Text("italic".to_string())]),
                Inline::LineBreak,
                Inline::Text("after break".to_string()),
            ]),
        };
        let t = simple_table_one_row(vec![cell]);
        let s = emit_html_table_typst(&t, &mut p);
        assert!(s.contains("[Plain *bold* and _italic_#linebreak()after break]"));
    }

    #[test]
    fn rowspan_two_layout_emits_table_cell_wrapper() {
        let mut p = test_pipeline();
        let t = HtmlTable {
            style: CellStyle::default(),
            head_rows: vec![],
            body_rows: vec![
                row_of(vec![
                    Cell {
                        rowspan: 2,
                        ..td_text("v")
                    },
                    td_text("b"),
                ]),
                row_of(vec![td_text("c")]),
            ],
        };
        let s = emit_html_table_typst(&t, &mut p);
        assert!(s.contains("table.cell(colspan: 1, rowspan: 2)[v]"), "{s}");
        assert!(s.contains("[b]"));
        assert!(s.contains("[c]"));
    }

    #[test]
    fn pathological_input_does_not_panic() {
        // Long row of cells with various spans — no panic, finishes.
        let mut p = test_pipeline();
        let cells: Vec<Cell> = (0..50)
            .map(|i| Cell {
                colspan: if i % 5 == 0 { 2 } else { 1 },
                ..td_text(&format!("c{i}"))
            })
            .collect();
        let t = simple_table_one_row(cells);
        let _ = emit_html_table_typst(&t, &mut p); // should complete
    }

    #[test]
    fn deeply_nested_inlines_does_not_panic() {
        let mut p = test_pipeline();
        // 100 levels of bold/italic nesting.
        let mut inner = Inline::Text("x".to_string());
        for i in 0..100 {
            inner = if i % 2 == 0 {
                Inline::Bold(vec![inner])
            } else {
                Inline::Italic(vec![inner])
            };
        }
        let cell = Cell {
            kind: CellKind::Td,
            colspan: 1,
            rowspan: 1,
            style: CellStyle::default(),
            content: CellContent::Inlines(vec![inner]),
        };
        let t = simple_table_one_row(vec![cell]);
        let _ = emit_html_table_typst(&t, &mut p);
    }
}
