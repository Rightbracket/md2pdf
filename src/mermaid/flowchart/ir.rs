//! Flowchart IR — the typed representation of a parsed mermaid
//! flowchart, before and after the layout pass annotates coordinates.
//!
//! The IR deliberately uses a flat `nodes: Vec<Node>` indexed by
//! [`NodeIdx`] (a wrapper around `usize`) rather than `HashMap<String,
//! Node>`. Reasons: deterministic insertion-order iteration (golden
//! tests depend on this), trivial bridge to `petgraph::Graph` indices,
//! and constant-time `nodes[idx]` access during layout.
//!
//! Subgraphs hold the indices of their member nodes; they're flattened
//! into the same `nodes` table so the layout pass treats subgraph
//! members and top-level nodes uniformly.

/// Newtype index into [`Flowchart::nodes`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeIdx(pub usize);

/// Layout direction (mermaid `TD`/`TB`, `BT`, `LR`, `RL`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Top-Down (default). `TD` and `TB` are synonyms.
    TopDown,
    /// Bottom-Up.
    BottomTop,
    /// Left-Right.
    LeftRight,
    /// Right-Left.
    RightLeft,
}

impl Default for Direction {
    fn default() -> Self {
        Direction::TopDown
    }
}

impl Direction {
    /// Whether the layout primary-axis is horizontal (LR/RL). Used by
    /// the layout pass to decide whether ranks march down or across.
    pub fn is_horizontal(self) -> bool {
        matches!(self, Direction::LeftRight | Direction::RightLeft)
    }
    /// Whether the layout direction is reversed along its primary axis
    /// (BT or RL). Used for coordinate flipping after layered
    /// placement.
    pub fn is_reversed(self) -> bool {
        matches!(self, Direction::BottomTop | Direction::RightLeft)
    }
}

/// Mermaid node shapes supported in v1 (W-3686a9 scope).
///
/// The shape determines the SVG element family (rect / ellipse / path /
/// polygon) and how label-padding is computed. Labels are rendered the
/// same across shapes; only the surrounding outline differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeShape {
    /// `id[Label]` — default.
    Rectangle,
    /// `id(Label)` — small rounded corners.
    RoundedRectangle,
    /// `id([Label])` — full pill ends.
    Stadium,
    /// `id[[Label]]` — double-line "subroutine" rectangle.
    Subroutine,
    /// `id[(Label)]` — cylinder (database-disk).
    Cylinder,
    /// `id((Label))` — circle.
    Circle,
    /// `id{Label}` — diamond / rhombus.
    Diamond,
}

/// Edge style. Mermaid expresses these as `-->`, `---`, `-.->`, `==>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// `-->` — solid arrow.
    Arrow,
    /// `---` — solid line, no arrow head.
    Line,
    /// `-.->` — dashed/dotted, with arrow head.
    Dashed,
    /// `==>` — thick line, with arrow head.
    Thick,
}

impl EdgeKind {
    pub fn has_arrowhead(self) -> bool {
        matches!(self, EdgeKind::Arrow | EdgeKind::Dashed | EdgeKind::Thick)
    }
    pub fn is_dashed(self) -> bool {
        matches!(self, EdgeKind::Dashed)
    }
    pub fn stroke_width(self) -> f32 {
        match self {
            EdgeKind::Thick => 2.5,
            _ => 1.2,
        }
    }
}

/// A node declaration. The label is the displayed text; `id` is the
/// unique key by which edges and subsequent declarations reference it.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: NodeShape,
    /// Computed by the layout pass. Coordinates are in points.
    pub layout: Option<NodeLayout>,
}

/// Geometry post-layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeLayout {
    /// Centre of the node, in points (x = right, y = down).
    pub cx: f32,
    pub cy: f32,
    /// Box dimensions, in points.
    pub width: f32,
    pub height: f32,
    /// 0-indexed rank assigned by the layered layout.
    pub rank: usize,
}

/// One directed edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Edge {
    pub from: NodeIdx,
    pub to: NodeIdx,
    pub kind: EdgeKind,
    pub label: Option<String>,
}

/// A named group of nodes. Subgraph membership is recorded as a
/// flat list of [`NodeIdx`]; the layout pass currently treats subgraph
/// members as ordinary nodes and renders the subgraph as a labelled
/// bounding box around them. Nested subgraphs nest the boxes.
#[derive(Debug, Clone, PartialEq)]
pub struct Subgraph {
    pub id: String,
    pub title: Option<String>,
    pub members: Vec<NodeIdx>,
    /// Nested subgraphs (1-level-down children).
    pub children: Vec<usize>,
    /// Nesting depth, 0 = top-level. Used by the SVG emitter to compute
    /// padding-from-children.
    pub depth: usize,
    /// Computed by layout: outer bounding box.
    pub layout: Option<SubgraphLayout>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SubgraphLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// The whole flowchart, post-parse and (after layout) post-layout.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Flowchart {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    /// Top-level subgraphs first; nested subgraphs follow at higher
    /// indices but are referenced by `children` from their parents.
    pub subgraphs: Vec<Subgraph>,
    /// Indices into `subgraphs` that are top-level (no parent).
    pub top_level_subgraphs: Vec<usize>,
}

impl Flowchart {
    /// Look up a node by its mermaid id. O(n) — only used during
    /// parsing where the alternative (a HashMap) buys little because
    /// the parser already needs to maintain insertion order.
    pub fn find_node(&self, id: &str) -> Option<NodeIdx> {
        self.nodes
            .iter()
            .position(|n| n.id == id)
            .map(NodeIdx)
    }
}
