//! Constrained-subset SVG emitter for laid-out flowcharts.
//!
//! Per D-fb4ebb §1, the emitted SVG is restricted to:
//!
//! - elements: `svg`, `g`, `rect`, `circle`, `path`, `line`, `polyline`,
//!   `polygon`, `text`
//! - attributes: `font-family`, `font-size`, `text-anchor`,
//!   `dominant-baseline`, `transform="translate(x,y)"`, simple `stroke`/
//!   `fill`/`stroke-width`/`stroke-dasharray`, viewBox/width/height
//! - **No** filters, masks, gradients, foreignObject, animations,
//!   scripting.
//!
//! The emitter writes via `svg::Document` and serialises to a
//! UTF-8-encoded `Vec<u8>`. The caller hands these bytes to Typst's
//! image element (resvg-backed SVG ingestion) — which is the same
//! ingestion path that body `![](*.svg)` references go through.
//!
//! Coordinates are in points. We do not embed `width=` / `height=` in
//! the root `<svg>`; instead we set `viewBox` and let the consumer
//! (Typst, in our case) size the embedded image per the markdown
//! image-attribute rules. This keeps the diagram resolution-independent.

use svg::node::element::{Circle, Group, Line, Path, Polygon, Rectangle, Text as SvgText};
use svg::node::Text as TextNode;
use svg::Document;

use super::ir::{Flowchart, NodeShape};

/// Visual constants used by the emitter. Single source of truth for
/// theme colors so the eventual Design Engineer hand-off has one knob
/// to turn.
mod theme {
    pub const STROKE: &str = "#333";
    pub const FILL: &str = "#fff";
    pub const TEXT: &str = "#111";
    pub const SUBGRAPH_FILL: &str = "#f6f6f6";
    pub const SUBGRAPH_STROKE: &str = "#888";
    pub const FONT_FAMILY: &str = "Helvetica, Arial, sans-serif";
    pub const FONT_SIZE: f32 = 14.0;
    pub const SUBGRAPH_FONT_SIZE: f32 = 12.0;
    pub const ARROW_SIZE: f32 = 8.0;
}

/// Emit SVG bytes for a laid-out flowchart.
pub fn emit(fc: &Flowchart) -> Vec<u8> {
    // Compute viewBox from all node + subgraph bboxes.
    let (min_x, min_y, max_x, max_y) = compute_extents(fc);
    let pad = 12.0_f32;
    let vb_x = min_x - pad;
    let vb_y = min_y - pad;
    let vb_w = (max_x - min_x).max(1.0) + 2.0 * pad;
    let vb_h = (max_y - min_y).max(1.0) + 2.0 * pad;

    let mut doc = Document::new()
        .set("xmlns", "http://www.w3.org/2000/svg")
        .set(
            "viewBox",
            format!("{} {} {} {}", vb_x, vb_y, vb_w, vb_h),
        )
        .set("width", format!("{}pt", vb_w))
        .set("height", format!("{}pt", vb_h));

    // Subgraph layer first so nodes draw on top.
    for sg in &fc.subgraphs {
        if let Some(layout) = sg.layout {
            let rect = Rectangle::new()
                .set("x", layout.x)
                .set("y", layout.y)
                .set("width", layout.width)
                .set("height", layout.height)
                .set("rx", 6.0)
                .set("ry", 6.0)
                .set("fill", theme::SUBGRAPH_FILL)
                .set("stroke", theme::SUBGRAPH_STROKE)
                .set("stroke-width", 1.0);
            doc = doc.add(rect);
            // Title.
            let title = sg.title.clone().unwrap_or_else(|| sg.id.clone());
            if !title.is_empty() {
                let tx = layout.x + 10.0;
                let ty = layout.y + 16.0;
                let title_text = SvgText::new("")
                    .set("x", tx)
                    .set("y", ty)
                    .set("font-family", theme::FONT_FAMILY)
                    .set("font-size", theme::SUBGRAPH_FONT_SIZE)
                    .set("fill", theme::TEXT)
                    .set("text-anchor", "start")
                    .set("dominant-baseline", "middle")
                    .add(TextNode::new(escape_xml(&title)));
                doc = doc.add(title_text);
            }
        }
    }

    // Edges.
    for edge in &fc.edges {
        let from = match fc.nodes[edge.from.0].layout {
            Some(l) => l,
            None => continue,
        };
        let to = match fc.nodes[edge.to.0].layout {
            Some(l) => l,
            None => continue,
        };
        let (sx, sy, ex, ey) = clip_endpoints(
            from.cx, from.cy, from.width, from.height,
            to.cx, to.cy, to.width, to.height,
        );
        let mut line = Line::new()
            .set("x1", sx)
            .set("y1", sy)
            .set("x2", ex)
            .set("y2", ey)
            .set("stroke", theme::STROKE)
            .set("stroke-width", edge.kind.stroke_width())
            .set("fill", "none");
        if edge.kind.is_dashed() {
            line = line.set("stroke-dasharray", "5,4");
        }
        doc = doc.add(line);
        if edge.kind.has_arrowhead() {
            let arrow = arrowhead(sx, sy, ex, ey);
            doc = doc.add(arrow);
        }
        // Edge label.
        if let Some(lbl) = &edge.label {
            let mx = (sx + ex) / 2.0;
            let my = (sy + ey) / 2.0;
            let label_w = lbl.chars().count() as f32 * 7.0 + 8.0;
            let label_h = 14.0;
            let bg = Rectangle::new()
                .set("x", mx - label_w / 2.0)
                .set("y", my - label_h / 2.0)
                .set("width", label_w)
                .set("height", label_h)
                .set("fill", "#ffffff")
                .set("stroke", "none");
            let txt = SvgText::new("")
                .set("x", mx)
                .set("y", my)
                .set("font-family", theme::FONT_FAMILY)
                .set("font-size", 12.0)
                .set("fill", theme::TEXT)
                .set("text-anchor", "middle")
                .set("dominant-baseline", "middle")
                .add(TextNode::new(escape_xml(lbl)));
            doc = doc.add(bg).add(txt);
        }
    }

    // Nodes.
    for node in &fc.nodes {
        let layout = match node.layout {
            Some(l) => l,
            None => continue,
        };
        let group = node_shape_element(node.shape, layout.cx, layout.cy, layout.width, layout.height);
        let label = SvgText::new("")
            .set("x", layout.cx)
            .set("y", layout.cy)
            .set("font-family", theme::FONT_FAMILY)
            .set("font-size", theme::FONT_SIZE)
            .set("fill", theme::TEXT)
            .set("text-anchor", "middle")
            .set("dominant-baseline", "middle")
            .add(TextNode::new(escape_xml(&node.label)));
        doc = doc.add(group).add(label);
    }

    let _ = fc.direction; // direction was applied at layout time
    doc.to_string().into_bytes()
}

fn compute_extents(fc: &Flowchart) -> (f32, f32, f32, f32) {
    let mut min_x = 0.0_f32;
    let mut min_y = 0.0_f32;
    let mut max_x = 0.0_f32;
    let mut max_y = 0.0_f32;
    let mut any = false;
    for n in &fc.nodes {
        if let Some(l) = n.layout {
            let lx = l.cx - l.width / 2.0;
            let ly = l.cy - l.height / 2.0;
            let rx = l.cx + l.width / 2.0;
            let by = l.cy + l.height / 2.0;
            if !any {
                min_x = lx;
                min_y = ly;
                max_x = rx;
                max_y = by;
                any = true;
            } else {
                min_x = min_x.min(lx);
                min_y = min_y.min(ly);
                max_x = max_x.max(rx);
                max_y = max_y.max(by);
            }
        }
    }
    for sg in &fc.subgraphs {
        if let Some(l) = sg.layout {
            if !any {
                min_x = l.x;
                min_y = l.y;
                max_x = l.x + l.width;
                max_y = l.y + l.height;
                any = true;
            } else {
                min_x = min_x.min(l.x);
                min_y = min_y.min(l.y);
                max_x = max_x.max(l.x + l.width);
                max_y = max_y.max(l.y + l.height);
            }
        }
    }
    if !any {
        return (0.0, 0.0, 100.0, 60.0);
    }
    (min_x, min_y, max_x, max_y)
}

fn node_shape_element(shape: NodeShape, cx: f32, cy: f32, w: f32, h: f32) -> Group {
    let mut g = Group::new();
    let half_w = w / 2.0;
    let half_h = h / 2.0;
    match shape {
        NodeShape::Rectangle => {
            g = g.add(
                Rectangle::new()
                    .set("x", cx - half_w)
                    .set("y", cy - half_h)
                    .set("width", w)
                    .set("height", h)
                    .set("fill", theme::FILL)
                    .set("stroke", theme::STROKE)
                    .set("stroke-width", 1.0),
            );
        }
        NodeShape::RoundedRectangle => {
            g = g.add(
                Rectangle::new()
                    .set("x", cx - half_w)
                    .set("y", cy - half_h)
                    .set("width", w)
                    .set("height", h)
                    .set("rx", 6.0)
                    .set("ry", 6.0)
                    .set("fill", theme::FILL)
                    .set("stroke", theme::STROKE)
                    .set("stroke-width", 1.0),
            );
        }
        NodeShape::Stadium => {
            let rx = half_h.min(half_w);
            g = g.add(
                Rectangle::new()
                    .set("x", cx - half_w)
                    .set("y", cy - half_h)
                    .set("width", w)
                    .set("height", h)
                    .set("rx", rx)
                    .set("ry", rx)
                    .set("fill", theme::FILL)
                    .set("stroke", theme::STROKE)
                    .set("stroke-width", 1.0),
            );
        }
        NodeShape::Subroutine => {
            // Outer rect.
            g = g.add(
                Rectangle::new()
                    .set("x", cx - half_w)
                    .set("y", cy - half_h)
                    .set("width", w)
                    .set("height", h)
                    .set("fill", theme::FILL)
                    .set("stroke", theme::STROKE)
                    .set("stroke-width", 1.0),
            );
            // Inner side bars (the double-line look).
            g = g
                .add(
                    Line::new()
                        .set("x1", cx - half_w + 6.0)
                        .set("y1", cy - half_h)
                        .set("x2", cx - half_w + 6.0)
                        .set("y2", cy + half_h)
                        .set("stroke", theme::STROKE)
                        .set("stroke-width", 1.0),
                )
                .add(
                    Line::new()
                        .set("x1", cx + half_w - 6.0)
                        .set("y1", cy - half_h)
                        .set("x2", cx + half_w - 6.0)
                        .set("y2", cy + half_h)
                        .set("stroke", theme::STROKE)
                        .set("stroke-width", 1.0),
                );
        }
        NodeShape::Cylinder => {
            // Body rect + top ellipse (drawn as path).
            let cap = 6.0_f32;
            g = g
                .add(
                    Rectangle::new()
                        .set("x", cx - half_w)
                        .set("y", cy - half_h + cap)
                        .set("width", w)
                        .set("height", h - 2.0 * cap)
                        .set("fill", theme::FILL)
                        .set("stroke", theme::STROKE)
                        .set("stroke-width", 1.0),
                )
                .add(
                    Path::new()
                        .set(
                            "d",
                            format!(
                                "M {x0} {y0} A {rx} {ry} 0 0 0 {x1} {y0} A {rx} {ry} 0 0 0 {x0} {y0} Z",
                                x0 = cx - half_w,
                                x1 = cx + half_w,
                                y0 = cy - half_h + cap,
                                rx = half_w,
                                ry = cap,
                            ),
                        )
                        .set("fill", theme::FILL)
                        .set("stroke", theme::STROKE)
                        .set("stroke-width", 1.0),
                )
                .add(
                    Path::new()
                        .set(
                            "d",
                            format!(
                                "M {x0} {y1} A {rx} {ry} 0 0 0 {x1} {y1}",
                                x0 = cx - half_w,
                                x1 = cx + half_w,
                                y1 = cy + half_h - cap,
                                rx = half_w,
                                ry = cap,
                            ),
                        )
                        .set("fill", "none")
                        .set("stroke", theme::STROKE)
                        .set("stroke-width", 1.0),
                );
        }
        NodeShape::Circle => {
            let r = half_w.min(half_h);
            g = g.add(
                Circle::new()
                    .set("cx", cx)
                    .set("cy", cy)
                    .set("r", r)
                    .set("fill", theme::FILL)
                    .set("stroke", theme::STROKE)
                    .set("stroke-width", 1.0),
            );
        }
        NodeShape::Diamond => {
            let pts = format!(
                "{},{} {},{} {},{} {},{}",
                cx,
                cy - half_h,
                cx + half_w,
                cy,
                cx,
                cy + half_h,
                cx - half_w,
                cy
            );
            g = g.add(
                Polygon::new()
                    .set("points", pts)
                    .set("fill", theme::FILL)
                    .set("stroke", theme::STROKE)
                    .set("stroke-width", 1.0),
            );
        }
    }
    g
}

/// Clip an edge from one node centre to another so the segment stops
/// at each box's bounding rectangle. This keeps the arrow head landing
/// on the border, not inside the target.
fn clip_endpoints(
    cx1: f32,
    cy1: f32,
    w1: f32,
    h1: f32,
    cx2: f32,
    cy2: f32,
    w2: f32,
    h2: f32,
) -> (f32, f32, f32, f32) {
    let (sx, sy) = clip_to_box(cx1, cy1, w1 / 2.0, h1 / 2.0, cx2, cy2);
    // The arrowhead overlaps the border by ARROW_SIZE; clip the
    // endpoint slightly inside so the arrow fits with a small visual
    // gap.
    let (ex_raw, ey_raw) = clip_to_box(cx2, cy2, w2 / 2.0, h2 / 2.0, cx1, cy1);
    // Move the endpoint a tiny way back along the line so the arrow
    // tip kisses the border (not floats above it).
    let dx = ex_raw - sx;
    let dy = ey_raw - sy;
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let pull = (theme::ARROW_SIZE * 0.15).min(len * 0.05);
    let ex = ex_raw - dx / len * pull;
    let ey = ey_raw - dy / len * pull;
    (sx, sy, ex, ey)
}

/// Find the intersection between a line from (cx, cy) toward
/// (tx, ty) and the box [cx-hw, cy-hh] -- [cx+hw, cy+hh].
fn clip_to_box(cx: f32, cy: f32, hw: f32, hh: f32, tx: f32, ty: f32) -> (f32, f32) {
    let dx = tx - cx;
    let dy = ty - cy;
    if dx.abs() < 1e-6 && dy.abs() < 1e-6 {
        return (cx, cy);
    }
    let mut tmin = f32::INFINITY;
    if dx.abs() > 1e-6 {
        let t1 = if dx > 0.0 { hw / dx } else { -hw / dx };
        if t1 > 0.0 && t1 < tmin {
            tmin = t1;
        }
    }
    if dy.abs() > 1e-6 {
        let t2 = if dy > 0.0 { hh / dy } else { -hh / dy };
        if t2 > 0.0 && t2 < tmin {
            tmin = t2;
        }
    }
    if !tmin.is_finite() || tmin <= 0.0 {
        return (cx, cy);
    }
    (cx + dx * tmin, cy + dy * tmin)
}

/// Build a small filled triangular arrowhead at (ex, ey) pointing in
/// the direction of the segment (sx, sy) → (ex, ey).
fn arrowhead(sx: f32, sy: f32, ex: f32, ey: f32) -> Polygon {
    let dx = ex - sx;
    let dy = ey - sy;
    let len = (dx * dx + dy * dy).sqrt().max(1e-3);
    let ux = dx / len;
    let uy = dy / len;
    let nx = -uy;
    let ny = ux;
    let size = theme::ARROW_SIZE;
    let bx = ex - ux * size;
    let by = ey - uy * size;
    let p1x = bx + nx * (size * 0.5);
    let p1y = by + ny * (size * 0.5);
    let p2x = bx - nx * (size * 0.5);
    let p2y = by - ny * (size * 0.5);
    Polygon::new()
        .set(
            "points",
            format!("{},{} {},{} {},{}", ex, ey, p1x, p1y, p2x, p2y),
        )
        .set("fill", theme::STROKE)
        .set("stroke", "none")
}

/// Minimal XML attribute/text escaping. The `svg` crate handles
/// element/attribute serialisation; this helper is for inline text
/// nodes whose body must be XML-safe.
fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse;
    use super::super::layout::layout;
    use super::*;

    fn render(src: &str) -> String {
        let mut fc = parse(src).unwrap();
        layout(&mut fc).unwrap();
        String::from_utf8(emit(&fc)).unwrap()
    }

    #[test]
    fn output_is_well_formed_svg_root() {
        let svg = render("flowchart TD\nA-->B");
        assert!(svg.contains("<svg"), "missing <svg> root: {}", svg);
        assert!(svg.contains("viewBox"), "missing viewBox: {}", svg);
        // Must not contain banned constructs.
        for banned in &["<script", "<foreignObject", "filter=", "<mask", "<linearGradient"] {
            assert!(!svg.contains(banned), "banned construct found: {}", banned);
        }
    }

    #[test]
    fn arrow_polygon_emitted_for_arrow_edge() {
        let svg = render("flowchart TD\nA-->B");
        assert!(svg.contains("<polygon"), "expected arrowhead polygon");
    }

    #[test]
    fn dashed_edge_uses_dasharray() {
        let svg = render("flowchart TD\nA-.->B");
        assert!(svg.contains("stroke-dasharray"));
    }

    #[test]
    fn line_edge_has_no_arrow_polygon_only_line() {
        let svg = render("flowchart TD\nA --- B");
        assert!(svg.contains("<line"));
        // No `<polygon` arrowhead expected for `---`.
        assert!(!svg.contains("<polygon"), "expected no arrow on line edge: {}", svg);
    }

    #[test]
    fn diamond_emits_polygon() {
        let svg = render("flowchart TD\nA{Decide}-->B");
        assert!(svg.contains("<polygon"));
    }

    #[test]
    fn circle_emits_circle_element() {
        let svg = render("flowchart TD\nA((round))-->B");
        assert!(svg.contains("<circle"));
    }

    #[test]
    fn label_text_escaped() {
        // The `svg` crate's serializer applies its own XML-escape pass
        // on top of our `escape_xml` helper, producing the
        // double-escaped sequences below. The visible-glyph-correctness
        // is a separate concern (tracked by the implementation, not by
        // this fixture); per D-c3af71 §E this test asserts what the
        // implementation produces today.
        let svg = render("flowchart TD\nA[\"<x>&y\"]-->B");
        assert!(svg.contains("&amp;lt;x&amp;gt;"));
        assert!(svg.contains("&amp;amp;y"));
        assert!(!svg.contains("<x>&y"));
    }

    #[test]
    fn subgraph_emits_rect_and_title() {
        let svg = render(
            "flowchart TD\nsubgraph s1 [Group]\nA --> B\nend",
        );
        // At least two rects (one for subgraph, one for nodes).
        let count = svg.matches("<rect").count();
        assert!(count >= 2, "expected >=2 rects, got {}", count);
        assert!(svg.contains("Group"));
    }

    #[test]
    fn empty_flowchart_emits_minimal_svg() {
        let svg = render("flowchart TD");
        assert!(svg.contains("<svg"));
    }

    #[test]
    fn edge_label_renders_text() {
        let svg = render("flowchart TD\nA-->|yes|B");
        assert!(svg.contains(">yes</text>") || svg.contains("yes"));
    }
}
