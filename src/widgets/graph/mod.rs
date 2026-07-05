//! Node-edge graph widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_graph;
pub use node::GraphRenderNode;
pub(crate) use node::graph_local_content_point;
pub use reconcile::reconcile_graph;

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::style::{BorderStyle, Length, Padding, Style, StyleSlot};

/// Direction used to lay out a [`Graph`] tree.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GraphDirection {
    /// Parents are above children.
    #[default]
    TopDown,
    /// Parents are left of children.
    LeftRight,
}

/// Graph layout algorithm.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GraphLayout {
    /// Tidy layered tree layout.
    #[default]
    Tree,
}

/// Stable path identifying a node within a [`Graph`] tree.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GraphNodePath(Arc<[usize]>);

impl GraphNodePath {
    /// Return the root node path.
    pub fn root() -> Self {
        Self(Arc::new([]))
    }

    /// Build a node path from child-index segments.
    pub fn from_segments(segments: impl IntoIterator<Item = usize>) -> Self {
        Self(segments.into_iter().collect::<Vec<_>>().into())
    }

    /// Return the child-index segments from root to node.
    pub fn segments(&self) -> &[usize] {
        &self.0
    }
}

impl From<Vec<usize>> for GraphNodePath {
    fn from(value: Vec<usize>) -> Self {
        Self(value.into())
    }
}

impl AsRef<[usize]> for GraphNodePath {
    fn as_ref(&self) -> &[usize] {
        self.segments()
    }
}

/// Event payload for pointer and keyboard interactions on graph nodes.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GraphNodeEvent {
    /// Path of the target node in the graph tree.
    pub path: GraphNodePath,
    /// Label of the target node.
    pub label: Arc<str>,
}

/// A labeled node in a [`Graph`] tree.
#[derive(Clone, Debug)]
pub struct GraphNode {
    pub(crate) label: Arc<str>,
    pub(crate) children: Arc<[GraphNode]>,
    pub(crate) style: Style,
    pub(crate) hover_style: Style,
    pub(crate) focus_style: StyleSlot,
    pub(crate) border: Option<bool>,
}

impl GraphNode {
    /// Create a graph node with a label.
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            children: Arc::new([]),
            style: Style::default(),
            hover_style: Style::default(),
            focus_style: StyleSlot::Replace(Style::default()),
            border: None,
        }
    }

    /// Replace all child nodes.
    pub fn children(mut self, children: impl IntoIterator<Item = GraphNode>) -> Self {
        self.children = children.into_iter().collect::<Vec<_>>().into();
        self
    }

    /// Add one child node.
    pub fn child(mut self, child: GraphNode) -> Self {
        let mut children = self.children.to_vec();
        children.push(child);
        self.children = children.into();
        self
    }

    /// Set this node's style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the style patched onto this node when hovered.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = style;
        self
    }

    /// Set the style patched onto this node when it has internal graph focus.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme focus style when this node has internal graph focus.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the active theme focus style when this node has internal graph focus.
    pub fn inherit_focus_style(mut self) -> Self {
        self.focus_style = StyleSlot::Inherit;
        self
    }

    /// Set the focused node style slot directly for composite forwarding.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_style = slot;
        self
    }

    /// Override whether this node renders with a border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = Some(border);
        self
    }
}

/// A direct-paint node-edge visualization for trees.
#[derive(Clone)]
pub struct Graph {
    pub(crate) root: Option<GraphNode>,
    pub(crate) direction: GraphDirection,
    pub(crate) layout: GraphLayout,
    pub(crate) gap_x: u16,
    pub(crate) gap_y: u16,
    pub(crate) max_node_width: u16,
    pub(crate) node_padding: Padding,
    pub(crate) node_border: bool,
    pub(crate) node_border_style: BorderStyle,
    pub(crate) style: Style,
    pub(crate) node_style: Style,
    pub(crate) node_hover_style: Style,
    pub(crate) focusable: bool,
    pub(crate) focused_path: Option<GraphNodePath>,
    pub(crate) node_focus_style: StyleSlot,
    pub(crate) edge_style: Style,
    pub(crate) edge_border_style: BorderStyle,
    pub(crate) on_node_click: Option<Callback<GraphNodeEvent>>,
    pub(crate) on_node_hover: Option<Callback<GraphNodeEvent>>,
    pub(crate) on_node_focus: Option<Callback<GraphNodeEvent>>,
    pub(crate) on_node_activate: Option<Callback<GraphNodeEvent>>,
    pub(crate) padding: Padding,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    /// Requested width.
    /// Default: [`Length::Auto`].
    pub(crate) width: Length,
    /// Requested height.
    /// Default: [`Length::Auto`].
    pub(crate) height: Length,
}

impl Default for Graph {
    fn default() -> Self {
        Self {
            root: None,
            direction: GraphDirection::TopDown,
            layout: GraphLayout::Tree,
            gap_x: 2,
            gap_y: 1,
            max_node_width: 24,
            node_padding: (0, 1).into(),
            node_border: true,
            node_border_style: BorderStyle::Plain,
            style: Style::default(),
            node_style: Style::default(),
            node_hover_style: Style::default(),
            focusable: false,
            focused_path: None,
            node_focus_style: StyleSlot::Inherit,
            edge_style: Style::default(),
            edge_border_style: BorderStyle::Plain,
            on_node_click: None,
            on_node_hover: None,
            on_node_focus: None,
            on_node_activate: None,
            padding: Padding::default(),
            border: false,
            border_style: BorderStyle::Plain,
            width: Length::Auto,
            height: Length::Auto,
        }
    }
}

impl Graph {
    /// Create an empty graph.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the root tree node.
    pub fn root(mut self, root: GraphNode) -> Self {
        self.root = Some(root);
        self
    }

    /// Clear the root node.
    pub fn empty(mut self) -> Self {
        self.root = None;
        self
    }

    /// Set the layout direction.
    pub fn direction(mut self, direction: GraphDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Set the graph layout mode.
    pub fn layout(mut self, layout: GraphLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Set the horizontal gap in cells.
    pub fn gap_x(mut self, gap_x: u16) -> Self {
        self.gap_x = gap_x;
        self
    }

    /// Set the vertical gap in cells.
    pub fn gap_y(mut self, gap_y: u16) -> Self {
        self.gap_y = gap_y;
        self
    }

    /// Set maximum node label width before wrapping.
    pub fn max_node_width(mut self, width: u16) -> Self {
        self.max_node_width = width.max(1);
        self
    }

    /// Set padding inside each graph node.
    pub fn node_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.node_padding = padding.into();
        self
    }

    /// Enable or disable borders around graph nodes by default.
    pub fn node_border(mut self, node_border: bool) -> Self {
        self.node_border = node_border;
        self
    }

    /// Set the border style used for graph node boxes.
    pub fn node_border_style(mut self, border_style: BorderStyle) -> Self {
        self.node_border_style = border_style;
        self
    }

    /// Set the base graph style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the default node style.
    pub fn node_style(mut self, style: Style) -> Self {
        self.node_style = style;
        self
    }

    /// Set the style applied to hovered graph nodes.
    pub fn node_hover_style(mut self, style: Style) -> Self {
        self.node_hover_style = style;
        self
    }

    /// Enable or disable keyboard focus for graph nodes.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Set the focused graph node path.
    pub fn focused_path(mut self, path: GraphNodePath) -> Self {
        self.focused_path = Some(path);
        self
    }

    /// Set the style applied to the focused graph node.
    pub fn node_focus_style(mut self, style: Style) -> Self {
        self.node_focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme focus style for the focused graph node.
    pub fn extend_node_focus_style(mut self, style: Style) -> Self {
        self.node_focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the active theme focus style for the focused graph node.
    pub fn inherit_node_focus_style(mut self) -> Self {
        self.node_focus_style = StyleSlot::Inherit;
        self
    }

    /// Set the focused graph node style slot.
    pub fn node_focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.node_focus_style = slot;
        self
    }

    /// Set a callback for clicks on graph nodes.
    pub fn on_node_click(mut self, cb: Callback<GraphNodeEvent>) -> Self {
        self.on_node_click = Some(cb);
        self
    }

    /// Set a callback for hover events on graph nodes.
    pub fn on_node_hover(mut self, cb: Callback<GraphNodeEvent>) -> Self {
        self.on_node_hover = Some(cb);
        self
    }

    /// Set a callback for focus changes on graph nodes.
    pub fn on_node_focus(mut self, cb: Callback<GraphNodeEvent>) -> Self {
        self.on_node_focus = Some(cb);
        self
    }

    /// Set a callback for keyboard activation of graph nodes.
    pub fn on_node_activate(mut self, cb: Callback<GraphNodeEvent>) -> Self {
        self.on_node_activate = Some(cb);
        self
    }

    /// Set the edge style.
    pub fn edge_style(mut self, style: Style) -> Self {
        self.edge_style = style;
        self
    }

    /// Set the box-drawing style used for graph edge elbows.
    pub fn edge_border_style(mut self, border_style: BorderStyle) -> Self {
        self.edge_border_style = border_style;
        self
    }

    /// Set graph padding inside the optional outer border.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Enable or disable the graph border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set graph border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set requested graph width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested graph height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Pan offset that centers the node identified by `path` within a viewport
    /// of `viewport_w` x `viewport_h` cells.
    ///
    /// The returned `(x, y)` is in the same coordinate space as
    /// [`crate::widgets::PanView::offset`], so it can be fed straight to a
    /// `PanView` wrapping this graph to bring the node into the middle of the
    /// view. The node layout is computed from the graph definition alone, so
    /// this works before the graph is mounted. Returns `None` when `path` does
    /// not resolve to a laid-out node (e.g. an empty graph).
    pub fn center_offset_for(
        &self,
        path: &GraphNodePath,
        viewport_w: u16,
        viewport_h: u16,
    ) -> Option<(i32, i32)> {
        let output = layout::build_graph_output(self);
        let node = output.nodes.iter().find(|node| &node.path == path)?;
        let border = i32::from(self.border);
        let node_center_x = border
            + i32::from(self.padding.left)
            + i32::from(node.rect.x)
            + i32::from(node.rect.w) / 2;
        let node_center_y = border
            + i32::from(self.padding.top)
            + i32::from(node.rect.y)
            + i32::from(node.rect.h) / 2;
        Some((
            node_center_x - i32::from(viewport_w) / 2,
            node_center_y - i32::from(viewport_h) / 2,
        ))
    }
}

impl From<Graph> for Element {
    fn from(value: Graph) -> Self {
        Element::new(ElementKind::Graph(Box::new(value)))
    }
}
