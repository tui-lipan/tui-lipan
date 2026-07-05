//! Mermaid-style flowchart widget.

mod layout;
mod node;
mod reconcile;
mod theme;

pub use layout::measure_flowchart;
pub(crate) use node::FlowchartItemEvent;
pub use node::FlowchartNode;
pub(crate) use node::PositionedEdge;
pub(crate) use node::flowchart_local_content_point;
pub use reconcile::reconcile_flowchart;
pub use theme::FlowchartTheme;

use std::collections::HashMap;
use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::style::{BorderStyle, Length, Padding, Style};

/// Direction used to lay out a [`Flowchart`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FlowDirection {
    /// Sources are above targets.
    #[default]
    TopDown,
    /// Sources are below targets.
    BottomUp,
    /// Sources are left of targets.
    LeftRight,
    /// Sources are right of targets.
    RightLeft,
}

/// Mermaid flowchart node shape.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NodeShape {
    /// Rectangle: `[text]`.
    #[default]
    Rect,
    /// Rounded rectangle: `(text)`.
    Round,
    /// Stadium: `([text])`.
    Stadium,
    /// Subroutine: `[[text]]`.
    Subroutine,
    /// Cylinder/database: `[(text)]`.
    Cylinder,
    /// Circle: `((text))`.
    Circle,
    /// Asymmetric: `>text]`.
    Asymmetric,
    /// Diamond: `{text}`.
    Diamond,
    /// Hexagon: `{{text}}`.
    Hexagon,
    /// Parallelogram: `[/text/]`.
    Parallelogram,
    /// Alternate parallelogram: `[\text\]`.
    ParallelogramAlt,
    /// Trapezoid: `[/text\]`.
    Trapezoid,
    /// Alternate trapezoid: `[\text/]`.
    TrapezoidAlt,
    /// Double circle: `(((text)))`.
    DoubleCircle,
}

/// Flowchart edge stroke style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EdgeStyle {
    /// Solid single-cell stroke.
    #[default]
    Solid,
    /// Dashed stroke.
    Dashed,
    /// Thick stroke.
    Thick,
    /// Hidden edge that still participates in layout.
    Invisible,
}

/// Flowchart edge arrowhead style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum EdgeArrow {
    /// No arrowhead.
    None,
    /// Open arrowhead.
    Open,
    /// Filled arrowhead.
    #[default]
    Filled,
    /// Cross marker.
    Cross,
    /// Circle marker.
    Circle,
}

/// Stable identifier for a flowchart node or subgraph.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(Arc<str>);

impl NodeId {
    /// Create an identifier.
    pub fn new(id: impl Into<Arc<str>>) -> Self {
        Self(id.into())
    }

    /// Return the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for NodeId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for NodeId {
    fn from(value: String) -> Self {
        Self::new(Arc::<str>::from(value))
    }
}

impl From<Arc<str>> for NodeId {
    fn from(value: Arc<str>) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for NodeId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A directed flowchart edge.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Edge {
    /// Source node id.
    pub from: NodeId,
    /// Target node id.
    pub to: NodeId,
    /// Optional edge label.
    pub label: Option<Arc<str>>,
    /// Stroke style.
    pub style: EdgeStyle,
    /// Arrowhead at the source side.
    pub head_from: EdgeArrow,
    /// Arrowhead at the target side.
    pub head_to: EdgeArrow,
    /// Optional stroke style override.
    pub line_style: Option<Style>,
    /// Optional label style override.
    pub label_style: Option<Style>,
}

impl Edge {
    /// Create a solid directed edge.
    pub fn solid(from: impl Into<NodeId>, to: impl Into<NodeId>) -> Self {
        Self::new(from, to, EdgeStyle::Solid)
    }

    /// Create a dashed directed edge.
    pub fn dashed(from: impl Into<NodeId>, to: impl Into<NodeId>) -> Self {
        Self::new(from, to, EdgeStyle::Dashed)
    }

    /// Create a thick directed edge.
    pub fn thick(from: impl Into<NodeId>, to: impl Into<NodeId>) -> Self {
        Self::new(from, to, EdgeStyle::Thick)
    }

    /// Create an invisible layout edge.
    pub fn invisible(from: impl Into<NodeId>, to: impl Into<NodeId>) -> Self {
        Self::new(from, to, EdgeStyle::Invisible).arrow_to(EdgeArrow::None)
    }

    /// Create an edge with a specific style.
    pub fn new(from: impl Into<NodeId>, to: impl Into<NodeId>, style: EdgeStyle) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
            label: None,
            style,
            head_from: EdgeArrow::None,
            head_to: EdgeArrow::Filled,
            line_style: None,
            label_style: None,
        }
    }

    /// Set the edge label.
    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the source-side arrowhead.
    pub fn arrow_from(mut self, arrow: EdgeArrow) -> Self {
        self.head_from = arrow;
        self
    }

    /// Set the target-side arrowhead.
    pub fn arrow_to(mut self, arrow: EdgeArrow) -> Self {
        self.head_to = arrow;
        self
    }

    /// Set both arrowheads.
    pub fn arrows(mut self, from: EdgeArrow, to: EdgeArrow) -> Self {
        self.head_from = from;
        self.head_to = to;
        self
    }

    /// Override the edge line style.
    pub fn line_style(mut self, style: Style) -> Self {
        self.line_style = Some(style);
        self
    }

    /// Override the edge label style.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = Some(style);
        self
    }
}

/// Event payload for node pointer interactions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlowchartNodeEvent {
    /// Target node id.
    pub id: NodeId,
    /// Target node label.
    pub label: Arc<str>,
}

/// Event payload for edge pointer interactions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlowchartEdgeEvent {
    /// Source node id.
    pub from: NodeId,
    /// Target node id.
    pub to: NodeId,
    /// Optional edge label.
    pub label: Option<Arc<str>>,
}

/// Event payload for subgraph pointer interactions.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FlowchartSubgraphEvent {
    /// Target subgraph id.
    pub id: NodeId,
    /// Target subgraph label.
    pub label: Arc<str>,
}

/// Stable path identifying an item in a [`Flowchart`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FlowchartItemPath {
    /// A node id.
    Node(NodeId),
    /// Edge index in insertion order.
    Edge(usize),
    /// A subgraph id.
    Subgraph(NodeId),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FlowchartNodeSpec {
    pub(crate) id: NodeId,
    pub(crate) label: Arc<str>,
    pub(crate) shape: NodeShape,
    pub(crate) style: Style,
    pub(crate) hover_style: Style,
    pub(crate) parent: Option<NodeId>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FlowchartSubgraphSpec {
    pub(crate) id: NodeId,
    pub(crate) label: Arc<str>,
    pub(crate) parent: Option<NodeId>,
    pub(crate) style: Style,
}

/// Direct-paint Mermaid-style flowchart visualization.
#[derive(Clone)]
pub struct Flowchart {
    pub(crate) direction: FlowDirection,
    pub(crate) nodes: Arc<[FlowchartNodeSpec]>,
    pub(crate) edges: Arc<[Edge]>,
    pub(crate) subgraphs: Arc<[FlowchartSubgraphSpec]>,
    pub(crate) class_defs: Arc<HashMap<Arc<str>, Style>>,
    pub(crate) class_assignments: Arc<HashMap<NodeId, Arc<str>>>,
    pub(crate) style: Style,
    pub(crate) node_style: Style,
    pub(crate) edge_style: Style,
    pub(crate) subgraph_style: Style,
    pub(crate) label_style: Style,
    pub(crate) item_hover_style: Style,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,
    pub(crate) node_gap: u16,
    pub(crate) layer_gap: u16,
    pub(crate) subgraph_padding: Padding,
    pub(crate) max_node_width: u16,
    pub(crate) theme: FlowchartTheme,
    pub(crate) on_node_click: Option<Callback<FlowchartNodeEvent>>,
    pub(crate) on_edge_click: Option<Callback<FlowchartEdgeEvent>>,
    pub(crate) on_subgraph_click: Option<Callback<FlowchartSubgraphEvent>>,
    pub(crate) on_node_hover: Option<Callback<FlowchartNodeEvent>>,
    pub(crate) on_edge_hover: Option<Callback<FlowchartEdgeEvent>>,
    pub(crate) on_subgraph_hover: Option<Callback<FlowchartSubgraphEvent>>,
    /// Requested width.
    pub(crate) width: Length,
    /// Requested height.
    pub(crate) height: Length,
}

impl Default for Flowchart {
    fn default() -> Self {
        Self {
            direction: FlowDirection::TopDown,
            nodes: Arc::new([]),
            edges: Arc::new([]),
            subgraphs: Arc::new([]),
            class_defs: Arc::new(HashMap::new()),
            class_assignments: Arc::new(HashMap::new()),
            style: Style::default(),
            node_style: Style::default(),
            edge_style: Style::default(),
            subgraph_style: Style::default(),
            label_style: Style::default(),
            item_hover_style: Style::default(),
            border: false,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            node_gap: 4,
            layer_gap: 3,
            subgraph_padding: (1, 2).into(),
            max_node_width: 24,
            theme: FlowchartTheme::classic(),
            on_node_click: None,
            on_edge_click: None,
            on_subgraph_click: None,
            on_node_hover: None,
            on_edge_hover: None,
            on_subgraph_hover: None,
            width: Length::Auto,
            height: Length::Auto,
        }
    }
}

impl Flowchart {
    /// Create an empty flowchart in the given direction.
    pub fn new(direction: FlowDirection) -> Self {
        Self {
            direction,
            ..Self::default()
        }
    }

    /// Add or replace a node.
    pub fn node(
        mut self,
        id: impl Into<NodeId>,
        label: impl Into<Arc<str>>,
        shape: NodeShape,
    ) -> Self {
        self.push_node(
            id.into(),
            label.into(),
            shape,
            None,
            Style::default(),
            Style::default(),
        );
        self
    }

    /// Add a node with a style override.
    pub fn styled_node(
        mut self,
        id: impl Into<NodeId>,
        label: impl Into<Arc<str>>,
        shape: NodeShape,
        style: Style,
    ) -> Self {
        self.push_node(
            id.into(),
            label.into(),
            shape,
            None,
            style,
            Style::default(),
        );
        self
    }

    /// Set the hover style patched onto an existing node id.
    pub fn node_hover_style(mut self, id: impl Into<NodeId>, style: Style) -> Self {
        let id = id.into();
        let mut nodes = self.nodes.to_vec();
        if let Some(node) = nodes.iter_mut().find(|node| node.id == id) {
            node.hover_style = style;
        }
        self.nodes = nodes.into();
        self
    }

    /// Add an edge.
    pub fn edge(mut self, edge: Edge) -> Self {
        let mut edges = self.edges.to_vec();
        edges.push(edge);
        self.edges = edges.into();
        self
    }

    /// Add a nested subgraph through a closure builder.
    pub fn subgraph(
        mut self,
        id: impl Into<NodeId>,
        label: impl Into<Arc<str>>,
        build: impl FnOnce(FlowchartSubgraphBuilder) -> FlowchartSubgraphBuilder,
    ) -> Self {
        let id = id.into();
        let label = label.into();
        let builder = build(FlowchartSubgraphBuilder::new(id.clone()));
        self.push_subgraph(id.clone(), label, None, Style::default());
        for node in builder.nodes {
            let parent = node.parent.or_else(|| Some(id.clone()));
            self.push_node(
                node.id,
                node.label,
                node.shape,
                parent,
                node.style,
                node.hover_style,
            );
        }
        for mut subgraph in builder.subgraphs {
            if subgraph.parent.is_none() {
                subgraph.parent = Some(id.clone());
            }
            self.push_subgraph(subgraph.id, subgraph.label, subgraph.parent, subgraph.style);
        }
        let mut edges = self.edges.to_vec();
        edges.extend(builder.edges);
        self.edges = edges.into();
        self
    }

    /// Define a named class style.
    pub fn class_def(mut self, name: impl Into<Arc<str>>, style: Style) -> Self {
        let mut class_defs = (*self.class_defs).clone();
        class_defs.insert(name.into(), style);
        self.class_defs = Arc::new(class_defs);
        self
    }

    /// Assign a class to a node or subgraph id.
    pub fn assign_class(mut self, id: impl Into<NodeId>, class: impl Into<Arc<str>>) -> Self {
        let mut assignments = (*self.class_assignments).clone();
        assignments.insert(id.into(), class.into());
        self.class_assignments = Arc::new(assignments);
        self
    }

    /// Set the base flowchart style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the default node style.
    pub fn node_style(mut self, style: Style) -> Self {
        self.node_style = style;
        self
    }

    /// Set the default edge style.
    pub fn edge_style(mut self, style: Style) -> Self {
        self.edge_style = style;
        self
    }

    /// Set the default subgraph style.
    pub fn subgraph_style(mut self, style: Style) -> Self {
        self.subgraph_style = style;
        self
    }

    /// Set the default edge-label style.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Set the item hover overlay style.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.item_hover_style = style;
        self
    }

    /// Set diagram-local glyph theme.
    pub fn theme(mut self, theme: FlowchartTheme) -> Self {
        self.theme = theme;
        self
    }

    /// Enable or disable the outer border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set outer border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set padding inside the optional outer border.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set horizontal gap between nodes in a layer.
    pub fn node_gap(mut self, gap: u16) -> Self {
        self.node_gap = gap;
        self
    }

    /// Set gap between layers.
    pub fn layer_gap(mut self, gap: u16) -> Self {
        self.layer_gap = gap;
        self
    }

    /// Set padding around subgraph contents.
    pub fn subgraph_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.subgraph_padding = padding.into();
        self
    }

    /// Set maximum node label width before wrapping.
    pub fn max_node_width(mut self, width: u16) -> Self {
        self.max_node_width = width.max(1);
        self
    }

    /// Set requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set a callback for node clicks.
    pub fn on_node_click(mut self, cb: Callback<FlowchartNodeEvent>) -> Self {
        self.on_node_click = Some(cb);
        self
    }

    /// Set a callback for edge clicks.
    pub fn on_edge_click(mut self, cb: Callback<FlowchartEdgeEvent>) -> Self {
        self.on_edge_click = Some(cb);
        self
    }

    /// Set a callback for subgraph header clicks.
    pub fn on_subgraph_click(mut self, cb: Callback<FlowchartSubgraphEvent>) -> Self {
        self.on_subgraph_click = Some(cb);
        self
    }

    /// Set a callback for node hover transitions.
    pub fn on_node_hover(mut self, cb: Callback<FlowchartNodeEvent>) -> Self {
        self.on_node_hover = Some(cb);
        self
    }

    /// Set a callback for edge hover transitions.
    pub fn on_edge_hover(mut self, cb: Callback<FlowchartEdgeEvent>) -> Self {
        self.on_edge_hover = Some(cb);
        self
    }

    /// Set a callback for subgraph header hover transitions.
    pub fn on_subgraph_hover(mut self, cb: Callback<FlowchartSubgraphEvent>) -> Self {
        self.on_subgraph_hover = Some(cb);
        self
    }

    fn push_node(
        &mut self,
        id: NodeId,
        label: Arc<str>,
        shape: NodeShape,
        parent: Option<NodeId>,
        style: Style,
        hover_style: Style,
    ) {
        let mut nodes = self.nodes.to_vec();
        if let Some(existing) = nodes.iter_mut().find(|node| node.id == id) {
            *existing = FlowchartNodeSpec {
                id,
                label,
                shape,
                style,
                hover_style,
                parent,
            };
        } else {
            nodes.push(FlowchartNodeSpec {
                id,
                label,
                shape,
                style,
                hover_style,
                parent,
            });
        }
        self.nodes = nodes.into();
    }

    fn push_subgraph(&mut self, id: NodeId, label: Arc<str>, parent: Option<NodeId>, style: Style) {
        let mut subgraphs = self.subgraphs.to_vec();
        if let Some(existing) = subgraphs.iter_mut().find(|subgraph| subgraph.id == id) {
            *existing = FlowchartSubgraphSpec {
                id,
                label,
                parent,
                style,
            };
        } else {
            subgraphs.push(FlowchartSubgraphSpec {
                id,
                label,
                parent,
                style,
            });
        }
        self.subgraphs = subgraphs.into();
    }
}

/// Builder passed to [`Flowchart::subgraph`].
pub struct FlowchartSubgraphBuilder {
    parent: NodeId,
    nodes: Vec<FlowchartNodeSpec>,
    edges: Vec<Edge>,
    subgraphs: Vec<FlowchartSubgraphSpec>,
}

impl FlowchartSubgraphBuilder {
    fn new(parent: NodeId) -> Self {
        Self {
            parent,
            nodes: Vec::new(),
            edges: Vec::new(),
            subgraphs: Vec::new(),
        }
    }

    /// Add a node inside this subgraph.
    pub fn node(
        mut self,
        id: impl Into<NodeId>,
        label: impl Into<Arc<str>>,
        shape: NodeShape,
    ) -> Self {
        self.nodes.push(FlowchartNodeSpec {
            id: id.into(),
            label: label.into(),
            shape,
            style: Style::default(),
            hover_style: Style::default(),
            parent: Some(self.parent.clone()),
        });
        self
    }

    /// Add a styled node inside this subgraph.
    pub fn styled_node(
        mut self,
        id: impl Into<NodeId>,
        label: impl Into<Arc<str>>,
        shape: NodeShape,
        style: Style,
    ) -> Self {
        self.nodes.push(FlowchartNodeSpec {
            id: id.into(),
            label: label.into(),
            shape,
            style,
            hover_style: Style::default(),
            parent: Some(self.parent.clone()),
        });
        self
    }

    /// Set the hover style patched onto an existing node inside this subgraph builder.
    pub fn node_hover_style(mut self, id: impl Into<NodeId>, style: Style) -> Self {
        let id = id.into();
        if let Some(node) = self.nodes.iter_mut().find(|node| node.id == id) {
            node.hover_style = style;
        }
        self
    }

    /// Add an edge inside this subgraph.
    pub fn edge(mut self, edge: Edge) -> Self {
        self.edges.push(edge);
        self
    }

    /// Add a nested subgraph.
    pub fn subgraph(
        mut self,
        id: impl Into<NodeId>,
        label: impl Into<Arc<str>>,
        build: impl FnOnce(FlowchartSubgraphBuilder) -> FlowchartSubgraphBuilder,
    ) -> Self {
        let id = id.into();
        let nested = build(FlowchartSubgraphBuilder::new(id.clone()));
        self.subgraphs.push(FlowchartSubgraphSpec {
            id: id.clone(),
            label: label.into(),
            parent: Some(self.parent.clone()),
            style: Style::default(),
        });
        self.nodes.extend(nested.nodes);
        self.edges.extend(nested.edges);
        self.subgraphs
            .extend(nested.subgraphs.into_iter().map(|mut sub| {
                if sub.parent.is_none() {
                    sub.parent = Some(id.clone());
                }
                sub
            }));
        self
    }
}

impl From<Flowchart> for Element {
    fn from(value: Flowchart) -> Self {
        Element::new(ElementKind::Flowchart(Box::new(value)))
    }
}
