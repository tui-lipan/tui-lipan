use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::callback::Callback;
use crate::core::event::KeyCode;
use crate::core::node::WidgetNode;
use crate::style::{BorderStyle, Length, Padding, Rect, Style, StyleSlot, Theme};

use super::{Graph, GraphDirection, GraphLayout, GraphNode, GraphNodeEvent, GraphNodePath};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GraphCacheKey {
    pub(crate) hash: u64,
}

impl GraphCacheKey {
    pub(crate) fn new(graph: &Graph) -> Self {
        Self {
            hash: structural_hash(graph),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GraphWidgetKey {
    pub(crate) hash: u64,
}

impl GraphWidgetKey {
    pub(crate) fn new(graph: &Graph) -> Self {
        Self {
            hash: widget_hash(graph),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedGraphNode {
    pub(crate) rect: Rect,
    pub(crate) path: GraphNodePath,
    pub(crate) label: Arc<str>,
    pub(crate) label_lines: Arc<[Arc<str>]>,
    pub(crate) style: Style,
    pub(crate) hover_style: Style,
    pub(crate) focus_style: StyleSlot,
    pub(crate) border: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct GraphEdgeCell {
    pub(crate) x: i16,
    pub(crate) y: i16,
    pub(crate) glyph: char,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct GraphRenderOutput {
    pub(crate) nodes: Vec<PositionedGraphNode>,
    pub(crate) edges: Vec<GraphEdgeCell>,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

/// Runtime node for the [`crate::widgets::Graph`] widget.
#[derive(Clone)]
pub struct GraphRenderNode {
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
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) output: Arc<GraphRenderOutput>,
    pub(crate) cache_key: GraphCacheKey,
    pub(crate) widget_key: GraphWidgetKey,
}

impl Default for GraphRenderNode {
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
            output: Arc::new(GraphRenderOutput::default()),
            cache_key: GraphCacheKey { hash: 0 },
            widget_key: GraphWidgetKey { hash: 0 },
        }
    }
}

impl From<Graph> for GraphRenderNode {
    fn from(value: Graph) -> Self {
        let mut node = Self::default();
        super::reconcile_graph(&value, &mut node);
        node
    }
}

impl GraphRenderNode {
    pub(crate) fn content_rect(&self, rect: Rect) -> Rect {
        graph_content_rect(rect, self.border, self.padding)
    }

    pub(crate) fn local_content_point(&self, rect: Rect, x: i16, y: i16) -> Option<(u16, u16)> {
        let content = self.content_rect(rect);
        if !content.contains(x, y) {
            return None;
        }

        Some((
            u16::try_from(x.saturating_sub(content.x)).ok()?,
            u16::try_from(y.saturating_sub(content.y)).ok()?,
        ))
    }

    pub(crate) fn hit_test(
        &self,
        local_x: u16,
        local_y: u16,
    ) -> Option<(usize, &PositionedGraphNode)> {
        let x = i16::try_from(local_x).unwrap_or(i16::MAX);
        let y = i16::try_from(local_y).unwrap_or(i16::MAX);
        self.output.nodes.iter().enumerate().find(|(_, node)| {
            x >= node.rect.x
                && y >= node.rect.y
                && x < node.rect.x.saturating_add(node.rect.w as i16)
                && y < node.rect.y.saturating_add(node.rect.h as i16)
        })
    }

    pub(crate) fn current_focused_path_or_first(&self) -> Option<GraphNodePath> {
        self.focused_path
            .as_ref()
            .filter(|path| self.has_path(path))
            .cloned()
            .or_else(|| self.output.nodes.first().map(|node| node.path.clone()))
    }

    pub(crate) fn focused_event(&self) -> Option<GraphNodeEvent> {
        self.current_focused_path_or_first()
            .and_then(|path| self.event_for_path(&path))
    }

    pub(crate) fn event_for_path(&self, path: &GraphNodePath) -> Option<GraphNodeEvent> {
        self.output
            .nodes
            .iter()
            .find(|node| &node.path == path)
            .map(|node| GraphNodeEvent {
                path: node.path.clone(),
                label: node.label.clone(),
            })
    }

    pub(crate) fn navigation_target(&self, key: KeyCode) -> Option<GraphNodePath> {
        match key {
            KeyCode::Home => return self.output.nodes.first().map(|node| node.path.clone()),
            KeyCode::End => return self.output.nodes.last().map(|node| node.path.clone()),
            _ => {}
        }

        let current = self.current_focused_path_or_first()?;
        match (self.direction, key) {
            (GraphDirection::TopDown, KeyCode::Up) | (GraphDirection::LeftRight, KeyCode::Left) => {
                self.parent_path(&current)
            }
            (GraphDirection::TopDown, KeyCode::Down)
            | (GraphDirection::LeftRight, KeyCode::Right) => self.first_child_path(&current),
            (GraphDirection::TopDown, KeyCode::Left) | (GraphDirection::LeftRight, KeyCode::Up) => {
                self.sibling_path(&current, -1)
            }
            (GraphDirection::TopDown, KeyCode::Right)
            | (GraphDirection::LeftRight, KeyCode::Down) => self.sibling_path(&current, 1),
            _ => None,
        }
    }

    pub(crate) fn set_focused_path(&mut self, path: GraphNodePath) -> bool {
        if !self.has_path(&path) || self.focused_path.as_ref() == Some(&path) {
            return false;
        }

        self.focused_path = Some(path);
        true
    }

    pub(crate) fn normalize_focused_path(&mut self) {
        let next = self
            .focused_path
            .as_ref()
            .filter(|path| self.has_path(path))
            .cloned()
            .or_else(|| self.output.nodes.first().map(|node| node.path.clone()));
        self.focused_path = next;
    }

    fn has_path(&self, path: &GraphNodePath) -> bool {
        self.output.nodes.iter().any(|node| &node.path == path)
    }

    fn parent_path(&self, path: &GraphNodePath) -> Option<GraphNodePath> {
        let segments = path.segments();
        if segments.is_empty() {
            return None;
        }
        Some(GraphNodePath::from_segments(
            segments[..segments.len().saturating_sub(1)].iter().copied(),
        ))
    }

    fn first_child_path(&self, path: &GraphNodePath) -> Option<GraphNodePath> {
        let mut segments = path.segments().to_vec();
        segments.push(0);
        let child = GraphNodePath::from_segments(segments);
        self.has_path(&child).then_some(child)
    }

    fn sibling_path(&self, path: &GraphNodePath, offset: isize) -> Option<GraphNodePath> {
        let mut segments = path.segments().to_vec();
        let last = segments.last_mut()?;
        let next = last.checked_add_signed(offset)?;
        *last = next;
        let sibling = GraphNodePath::from_segments(segments);
        self.has_path(&sibling).then_some(sibling)
    }
}

impl WidgetNode for GraphRenderNode {
    fn is_focusable(&self) -> bool {
        !self.output.nodes.is_empty()
            && (self.focusable || self.on_node_focus.is_some() || self.on_node_activate.is_some())
    }

    fn has_on_click(&self) -> bool {
        self.on_node_click.is_some() || self.on_node_focus.is_some()
    }

    fn is_hoverable(&self) -> bool {
        self.has_on_click()
            || self.on_node_hover.is_some()
            || !self.node_hover_style.is_empty()
            || self
                .output
                .nodes
                .iter()
                .any(|node| !node.hover_style.is_empty())
    }

    fn is_hoverable_for_theme(&self, _theme: &Theme) -> bool {
        self.is_hoverable()
    }

    fn hit_test_refinement(&self, x: i16, y: i16, rect: Rect) -> Option<bool> {
        if !(self.is_focusable() || self.has_on_click()) {
            return None;
        }

        Some(
            self.local_content_point(rect, x, y)
                .and_then(|(local_x, local_y)| self.hit_test(local_x, local_y))
                .is_some(),
        )
    }

    fn hover_test_refinement(&self, x: i16, y: i16, rect: Rect) -> Option<bool> {
        if !self.is_hoverable() {
            return Some(false);
        }

        Some(
            self.local_content_point(rect, x, y)
                .and_then(|(local_x, local_y)| self.hit_test(local_x, local_y))
                .is_some(),
        )
    }
}

pub(crate) fn graph_content_rect(rect: Rect, border: bool, padding: Padding) -> Rect {
    rect.inner(border, padding)
}

pub(crate) fn graph_local_content_point(
    rect: Rect,
    border: bool,
    padding: Padding,
    x: i16,
    y: i16,
) -> Option<(u16, u16)> {
    let content = graph_content_rect(rect, border, padding);
    if !content.contains(x, y) {
        return None;
    }

    Some((
        u16::try_from(x.saturating_sub(content.x)).ok()?,
        u16::try_from(y.saturating_sub(content.y)).ok()?,
    ))
}

fn structural_hash(graph: &Graph) -> u64 {
    let mut hasher = DefaultHasher::new();
    graph.direction.hash(&mut hasher);
    graph.layout.hash(&mut hasher);
    graph.gap_x.hash(&mut hasher);
    graph.gap_y.hash(&mut hasher);
    graph.max_node_width.hash(&mut hasher);
    graph.node_padding.hash(&mut hasher);
    graph.node_border.hash(&mut hasher);
    graph.edge_border_style.hash(&mut hasher);
    hash_graph_node(graph.root.as_ref(), &mut hasher);
    hasher.finish()
}

fn widget_hash(graph: &Graph) -> u64 {
    let mut hasher = DefaultHasher::new();
    structural_hash(graph).hash(&mut hasher);
    hash_graph_node_styles(graph.root.as_ref(), &mut hasher);
    graph.style.hash(&mut hasher);
    graph.node_style.hash(&mut hasher);
    graph.node_hover_style.hash(&mut hasher);
    graph.focusable.hash(&mut hasher);
    graph.focused_path.hash(&mut hasher);
    graph.node_focus_style.hash(&mut hasher);
    graph.node_border_style.hash(&mut hasher);
    graph.edge_style.hash(&mut hasher);
    graph.padding.hash(&mut hasher);
    graph.border.hash(&mut hasher);
    graph.border_style.hash(&mut hasher);
    graph.width.hash(&mut hasher);
    graph.height.hash(&mut hasher);
    hasher.finish()
}

fn hash_graph_node(node: Option<&GraphNode>, hasher: &mut impl Hasher) {
    match node {
        Some(node) => {
            true.hash(hasher);
            node.label.hash(hasher);
            node.border.hash(hasher);
            node.children.len().hash(hasher);
            for child in node.children.iter() {
                hash_graph_node(Some(child), hasher);
            }
        }
        None => false.hash(hasher),
    }
}

fn hash_graph_node_styles(node: Option<&GraphNode>, hasher: &mut impl Hasher) {
    if let Some(node) = node {
        node.style.hash(hasher);
        node.hover_style.hash(hasher);
        node.focus_style.hash(hasher);
        for child in node.children.iter() {
            hash_graph_node_styles(Some(child), hasher);
        }
    }
}
