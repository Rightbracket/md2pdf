//! Layered layout for the flowchart IR.
//!
//! ## Why hand-rolled, not `layout-rs`
//!
//! D-fb4ebb §1 names `layout-rs` as the primary layout choice but
//! explicitly permits the Developer to swap with concrete reason
//! ("Developer may swap with concrete reason"). The hand-rolled pass
//! below was chosen because:
//!
//! 1. **Deterministic golden testing.** The Work's
//!    test plan calls for golden-SVG comparison; a fully in-tree
//!    layout has stable output across rebuilds without pinning a
//!    third-party crate's micro-version. (If `layout-rs` is bumped its
//!    tie-breaking heuristics can shift, churning goldens.)
//! 2. **Smaller supply-chain surface.** The Security Engineer reviews
//!    the dep graph; a self-contained layout pass eliminates one
//!    transitive surface.
//! 3. **The Decision's fallback list explicitly allows hand-rolled
//!    Sugiyama as the "if `layout-rs` produces visibly worse"
//!    invalidation outcome.** The same code is what we would write
//!    after the `layout-rs` invalidation condition trips.
//!
//! The downside is that this layout is simpler than mermaid-cli's
//! dagre-based output. For the v1 subset (small flowcharts up to a few
//! dozen nodes), the result reads cleanly: ranks are assigned by
//! longest path from sources, in-rank ordering is insertion-order, and
//! subgraphs are framed with axis-aligned bounding boxes around their
//! members. If/when this becomes inadequate the Decision's
//! invalidation condition trips and a future Work re-evaluates
//! layout-rs vs. hand-rolled-Sugiyama-with-crossing-minimisation.
//!
//! ## Algorithm (in order)
//!
//! 1. **Cycle handling.** Edges that would create a cycle (back-edges)
//!    are kept in the IR (so the SVG emitter still draws them) but
//!    excluded from the rank-assignment graph.
//! 2. **Rank = longest-path-from-sources.** Each node's rank is
//!    `max(rank(pred)) + 1` over forward-DAG predecessors; sources are
//!    rank 0.
//! 3. **In-rank ordering.** First appearance order in the source.
//! 4. **Coordinate assignment.** Width/height per node from label
//!    text + shape padding; ranks are stacked top-down with a fixed
//!    rank gap; in-rank x-positions are evenly spaced and centred.
//! 5. **Direction transform.** TopDown is the canonical layout. BT
//!    flips the y-axis; LR/RL swap x↔y; RL also flips.
//! 6. **Subgraph bounding boxes** are computed last as the
//!    axis-aligned hull of their members plus padding.

use std::collections::HashMap;

use petgraph::graphmap::DiGraphMap;

use super::super::{MermaidError, MAX_NODES};
use super::ir::{
    Direction, Flowchart, NodeLayout, NodeShape, SubgraphLayout,
};

/// Visual constants. Kept private; tweak with goldens.
mod tune {
    pub const FONT_SIZE: f32 = 14.0;
    /// Approximate em-width at 14pt. Mermaid uses a similar heuristic
    /// and we don't have the actual font metrics here (rustybuzz +
    /// the body sans font would be the eventual upgrade — flagged as
    /// future work in research-findings).
    pub const EM_WIDTH: f32 = 7.6;
    pub const NODE_PAD_X: f32 = 16.0;
    pub const NODE_PAD_Y: f32 = 12.0;
    pub const MIN_NODE_W: f32 = 40.0;
    pub const MIN_NODE_H: f32 = 32.0;
    pub const RANK_GAP: f32 = 60.0;
    pub const NODE_HGAP: f32 = 32.0;
    /// Padding around subgraph member content.
    pub const SUBGRAPH_PAD: f32 = 16.0;
    /// Extra top padding inside subgraph for its title row.
    pub const SUBGRAPH_TITLE_GAP: f32 = 22.0;
}

pub fn layout(fc: &mut Flowchart) -> Result<(), MermaidError> {
    if fc.nodes.is_empty() {
        // Empty graph: trivial layout. Caller will emit a small
        // placeholder SVG.
        return Ok(());
    }
    if fc.nodes.len() > MAX_NODES {
        return Err(MermaidError::LayoutError {
            reason: format!("too many nodes ({} > {})", fc.nodes.len(), MAX_NODES),
        });
    }

    // 1. Build petgraph::DiGraphMap of *forward* edges only (cycles
    //    broken below). Petgraph backs the rank pass.
    let mut g: DiGraphMap<usize, ()> = DiGraphMap::new();
    for (i, _n) in fc.nodes.iter().enumerate() {
        g.add_node(i);
    }
    // Detect back-edges via a simple DFS-based cycle check; we want
    // deterministic output so we walk in node-insertion order.
    let back_edges = detect_back_edges(&fc.nodes, &fc.edges);
    for (eidx, e) in fc.edges.iter().enumerate() {
        if back_edges.contains(&eidx) {
            continue;
        }
        g.add_edge(e.from.0, e.to.0, ());
    }

    // 2. Rank assignment by longest path from sources. Iterate until
    //    fixed point (bounded by node count for safety).
    let mut rank: Vec<usize> = vec![0; fc.nodes.len()];
    for _iter in 0..fc.nodes.len() + 1 {
        let mut changed = false;
        for i in 0..fc.nodes.len() {
            let mut max_pred_rank: Option<usize> = None;
            for pred in g.neighbors_directed(i, petgraph::Direction::Incoming) {
                let pr = rank[pred];
                max_pred_rank = Some(max_pred_rank.map_or(pr, |m| m.max(pr)));
            }
            let new_rank = max_pred_rank.map_or(0, |m| m + 1);
            if new_rank != rank[i] {
                rank[i] = new_rank;
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }

    // 3. Per-rank ordering: insertion order.
    let max_rank = *rank.iter().max().unwrap_or(&0);
    let mut by_rank: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    for (i, r) in rank.iter().enumerate() {
        by_rank[*r].push(i);
    }

    // 4. Compute per-node dimensions.
    let dims: Vec<(f32, f32)> = fc
        .nodes
        .iter()
        .map(|n| node_dims(&n.label, n.shape))
        .collect();

    // 5. Assign canonical TD coordinates.
    //    For each rank, total_width = sum(dim.w) + (k-1)*HGAP. Centre
    //    the rank around x=0, then translate so min-x = 0.
    let rank_widths: Vec<f32> = by_rank
        .iter()
        .map(|row| {
            if row.is_empty() {
                return 0.0;
            }
            let sum: f32 = row.iter().map(|&i| dims[i].0).sum();
            let gaps = (row.len() as f32 - 1.0) * tune::NODE_HGAP;
            sum + gaps.max(0.0)
        })
        .collect();
    let total_canvas_w = rank_widths.iter().cloned().fold(0.0_f32, f32::max);
    let rank_heights: Vec<f32> = by_rank
        .iter()
        .map(|row| {
            row.iter().map(|&i| dims[i].1).fold(0.0_f32, f32::max)
        })
        .collect();

    let mut centers: Vec<(f32, f32)> = vec![(0.0, 0.0); fc.nodes.len()];
    let mut y_cursor = 0.0_f32;
    for (r, row) in by_rank.iter().enumerate() {
        let row_w = rank_widths[r];
        let row_h = rank_heights[r];
        let mut x = (total_canvas_w - row_w) / 2.0;
        // y centre of this row:
        let y_center = y_cursor + row_h / 2.0;
        for &i in row {
            let (w, _h) = dims[i];
            let cx = x + w / 2.0;
            centers[i] = (cx, y_center);
            x += w + tune::NODE_HGAP;
        }
        y_cursor += row_h + tune::RANK_GAP;
    }

    // 6. Direction transform.
    let dir = fc.direction;
    let total_h = (y_cursor - tune::RANK_GAP).max(0.0);
    let total_w = total_canvas_w;
    for (i, n) in fc.nodes.iter_mut().enumerate() {
        let (mut cx, mut cy) = centers[i];
        let (w, h) = dims[i];
        match dir {
            Direction::TopDown => {}
            Direction::BottomTop => {
                cy = total_h - cy;
            }
            Direction::LeftRight => {
                let (ncx, ncy) = (cy, cx);
                cx = ncx;
                cy = ncy;
            }
            Direction::RightLeft => {
                let (ncx, ncy) = (total_h - cy, cx);
                cx = ncx;
                cy = ncy;
            }
        }
        n.layout = Some(NodeLayout {
            cx,
            cy,
            width: w,
            height: h,
            rank: rank[i],
        });
    }

    let _ = total_w; // unused placeholder for symmetry

    // 7. Subgraph bounding boxes.
    let mut sg_layouts: Vec<Option<SubgraphLayout>> = vec![None; fc.subgraphs.len()];
    // Layout in deepest-first order so parents include children.
    let order = subgraph_eval_order(&fc.subgraphs);
    let snapshot: Vec<NodeLayout> = fc
        .nodes
        .iter()
        .map(|n| {
            n.layout.unwrap_or(NodeLayout {
                cx: 0.0,
                cy: 0.0,
                width: 0.0,
                height: 0.0,
                rank: 0,
            })
        })
        .collect();
    for sgi in order {
        let sg = &fc.subgraphs[sgi];
        // Bounds from members.
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for m in &sg.members {
            let nl = snapshot[m.0];
            min_x = min_x.min(nl.cx - nl.width / 2.0);
            min_y = min_y.min(nl.cy - nl.height / 2.0);
            max_x = max_x.max(nl.cx + nl.width / 2.0);
            max_y = max_y.max(nl.cy + nl.height / 2.0);
        }
        // Include nested subgraphs.
        for &c in &sg.children {
            if let Some(cb) = sg_layouts[c] {
                min_x = min_x.min(cb.x);
                min_y = min_y.min(cb.y);
                max_x = max_x.max(cb.x + cb.width);
                max_y = max_y.max(cb.y + cb.height);
            }
        }
        if !min_x.is_finite() {
            // Empty subgraph — record a tiny degenerate box at origin.
            sg_layouts[sgi] = Some(SubgraphLayout {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            });
            continue;
        }
        let pad = tune::SUBGRAPH_PAD;
        let title_gap = if sg.title.is_some() || !sg.id.is_empty() {
            tune::SUBGRAPH_TITLE_GAP
        } else {
            0.0
        };
        sg_layouts[sgi] = Some(SubgraphLayout {
            x: min_x - pad,
            y: min_y - pad - title_gap,
            width: (max_x - min_x) + 2.0 * pad,
            height: (max_y - min_y) + 2.0 * pad + title_gap,
        });
    }
    for (i, sl) in sg_layouts.into_iter().enumerate() {
        fc.subgraphs[i].layout = sl;
    }
    Ok(())
}

/// Compute (width, height) of a node's bounding box from its label and
/// shape. Pure, deterministic.
fn node_dims(label: &str, shape: NodeShape) -> (f32, f32) {
    // Single-line label width. Multi-line wrap is future polish.
    let chars = label.chars().count() as f32;
    let raw_w = chars * tune::EM_WIDTH;
    let raw_h = tune::FONT_SIZE * 1.4;
    let mut w = raw_w + 2.0 * tune::NODE_PAD_X;
    let mut h = raw_h + 2.0 * tune::NODE_PAD_Y;
    // Shape-specific minima.
    match shape {
        NodeShape::Circle => {
            // Force square so the circle fits the label.
            let d = w.max(h);
            w = d;
            h = d;
        }
        NodeShape::Stadium | NodeShape::RoundedRectangle => {
            w += 8.0; // pill ends eat a touch more horizontal space
        }
        NodeShape::Diamond => {
            // Diamond fits its inscribed rectangle; pad both axes so
            // the label doesn't crowd the points.
            w *= 1.25;
            h *= 1.25;
        }
        NodeShape::Cylinder => {
            h += 8.0; // top/bottom ellipse
        }
        NodeShape::Subroutine => {
            w += 16.0; // double border
        }
        NodeShape::Rectangle => {}
    }
    (
        w.max(tune::MIN_NODE_W),
        h.max(tune::MIN_NODE_H),
    )
}

/// Detect the indices of edges that would form a cycle if added in
/// insertion order. This is intentionally a simple greedy scheme:
/// process edges in order; an edge is a back-edge iff its target is an
/// ancestor of its source in the partially-built graph.
fn detect_back_edges(
    nodes: &[super::ir::Node],
    edges: &[super::ir::Edge],
) -> std::collections::HashSet<usize> {
    let mut g: DiGraphMap<usize, ()> = DiGraphMap::new();
    for i in 0..nodes.len() {
        g.add_node(i);
    }
    let mut back = std::collections::HashSet::new();
    for (i, e) in edges.iter().enumerate() {
        // If `e.to` already reaches `e.from`, adding e.from->e.to
        // creates a cycle.
        if reachable(&g, e.to.0, e.from.0) {
            back.insert(i);
            continue;
        }
        g.add_edge(e.from.0, e.to.0, ());
    }
    back
}

fn reachable(g: &DiGraphMap<usize, ()>, src: usize, dst: usize) -> bool {
    if src == dst {
        return true;
    }
    let mut stack = vec![src];
    let mut seen = std::collections::HashSet::new();
    seen.insert(src);
    let mut steps = 0;
    while let Some(n) = stack.pop() {
        steps += 1;
        if steps > 10_000 {
            // Defensive: bound the BFS so a degenerate input cannot
            // hang. 10k steps is several orders of magnitude beyond
            // the IR cap (MAX_NODES * MAX_EDGES upper bound), but cap
            // anyway.
            return false;
        }
        for nx in g.neighbors_directed(n, petgraph::Direction::Outgoing) {
            if nx == dst {
                return true;
            }
            if seen.insert(nx) {
                stack.push(nx);
            }
        }
    }
    false
}

/// Topological order on the subgraph parent-child tree, deepest first.
fn subgraph_eval_order(subgraphs: &[super::ir::Subgraph]) -> Vec<usize> {
    // Bucket by depth, descending.
    let mut by_depth: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut max_depth = 0;
    for (i, sg) in subgraphs.iter().enumerate() {
        by_depth.entry(sg.depth).or_default().push(i);
        max_depth = max_depth.max(sg.depth);
    }
    let mut out = Vec::with_capacity(subgraphs.len());
    for d in (0..=max_depth).rev() {
        if let Some(v) = by_depth.remove(&d) {
            out.extend(v);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::parser::parse;
    use super::*;

    #[test]
    fn simple_two_node_chain_lays_out() {
        let mut fc = parse("flowchart TD\nA-->B").unwrap();
        layout(&mut fc).unwrap();
        let a = fc.nodes[0].layout.unwrap();
        let b = fc.nodes[1].layout.unwrap();
        // Top-down: B is below A.
        assert!(b.cy > a.cy);
        // Roughly aligned x (single node per rank).
        assert!((a.cx - b.cx).abs() < 1e-3);
        assert_eq!(a.rank, 0);
        assert_eq!(b.rank, 1);
    }

    #[test]
    fn lr_swaps_axes() {
        let mut fc = parse("flowchart LR\nA-->B").unwrap();
        layout(&mut fc).unwrap();
        let a = fc.nodes[0].layout.unwrap();
        let b = fc.nodes[1].layout.unwrap();
        // LR: B is to the right of A.
        assert!(b.cx > a.cx);
        assert!((a.cy - b.cy).abs() < 1e-3);
    }

    #[test]
    fn cycle_does_not_crash() {
        let mut fc = parse("flowchart TD\nA-->B\nB-->A").unwrap();
        layout(&mut fc).unwrap();
        // Both nodes laid out, ranks finite.
        assert!(fc.nodes[0].layout.is_some());
        assert!(fc.nodes[1].layout.is_some());
    }

    #[test]
    fn subgraph_box_encloses_members() {
        let src = "\
flowchart TD
subgraph s1
  A --> B
end
B --> C
";
        let mut fc = parse(src).unwrap();
        layout(&mut fc).unwrap();
        let sg = fc.subgraphs[0].layout.unwrap();
        for m in &fc.subgraphs[0].members {
            let n = fc.nodes[m.0].layout.unwrap();
            assert!(n.cx - n.width / 2.0 >= sg.x - 0.001, "member off left");
            assert!(n.cx + n.width / 2.0 <= sg.x + sg.width + 0.001, "member off right");
            assert!(n.cy - n.height / 2.0 >= sg.y - 0.001, "member off top");
            assert!(n.cy + n.height / 2.0 <= sg.y + sg.height + 0.001, "member off bottom");
        }
    }

    #[test]
    fn many_node_chain_lays_out_without_panic() {
        let mut s = String::from("flowchart TD\n");
        for i in 0..100 {
            s.push_str(&format!("n{} --> n{}\n", i, i + 1));
        }
        let mut fc = parse(&s).unwrap();
        layout(&mut fc).unwrap();
        assert_eq!(fc.nodes.len(), 101);
        // Last node's rank is 100.
        assert_eq!(fc.nodes[100].layout.unwrap().rank, 100);
    }

    #[test]
    fn empty_flowchart_lays_out_trivially() {
        let mut fc = parse("flowchart TD").unwrap();
        layout(&mut fc).unwrap();
        assert!(fc.nodes.is_empty());
    }
}
