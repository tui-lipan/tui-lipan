use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use crate::callback::Callback;
use crate::core::node::WidgetNode;
use crate::style::{BorderStyle, Length, Padding, Rect, Style, Theme};

use super::{
    Edge, EdgeArrow, EdgeStyle, FlowDirection, Flowchart, FlowchartEdgeEvent, FlowchartItemPath,
    FlowchartNodeEvent, FlowchartSubgraphEvent, FlowchartTheme, NodeId, NodeShape,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FlowchartCacheKey {
    pub(crate) hash: u64,
}

impl FlowchartCacheKey {
    pub(crate) fn new(flowchart: &Flowchart) -> Self {
        Self {
            hash: structural_hash(flowchart),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FlowchartWidgetKey {
    pub(crate) hash: u64,
}

impl FlowchartWidgetKey {
    pub(crate) fn new(flowchart: &Flowchart) -> Self {
        Self {
            hash: widget_hash(flowchart),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedNode {
    pub(crate) rect: Rect,
    pub(crate) id: NodeId,
    pub(crate) label: Arc<str>,
    pub(crate) label_lines: Arc<[Arc<str>]>,
    pub(crate) shape: NodeShape,
    pub(crate) style: Style,
    pub(crate) hover_style: Style,
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedSubgraph {
    pub(crate) rect: Rect,
    pub(crate) header_rect: Rect,
    pub(crate) id: NodeId,
    pub(crate) label: Arc<str>,
    pub(crate) depth: usize,
    pub(crate) style: Style,
}

#[derive(Clone, Debug)]
pub(crate) struct EdgeCell {
    pub(crate) x: i16,
    pub(crate) y: i16,
    /// Direction bits (N|S|E|W) that this edge contributes to the cell. The renderer ORs bits
    /// across overlapping edges so confluences render as junction glyphs (`┬`, `┼`, …) instead
    /// of one edge overpainting the other.
    pub(crate) bits: u8,
}

#[derive(Clone, Debug)]
pub(crate) struct PositionedEdge {
    pub(crate) index: usize,
    pub(crate) from: NodeId,
    pub(crate) to: NodeId,
    pub(crate) label: Option<Arc<str>>,
    pub(crate) style: EdgeStyle,
    pub(crate) head_from: EdgeArrow,
    pub(crate) head_to: EdgeArrow,
    pub(crate) line_style: Option<Style>,
    pub(crate) label_style: Option<Style>,
    pub(crate) cells: Vec<EdgeCell>,
    pub(crate) label_pos: Option<(i16, i16)>,
    pub(crate) head_from_pos: Option<(i16, i16, FlowDirection)>,
    pub(crate) head_to_pos: Option<(i16, i16, FlowDirection)>,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FlowchartRenderOutput {
    pub(crate) nodes: Vec<PositionedNode>,
    pub(crate) subgraphs: Vec<PositionedSubgraph>,
    pub(crate) edges: Vec<PositionedEdge>,
    pub(crate) width: u16,
    pub(crate) height: u16,
}

/// Runtime node for the [`crate::widgets::Flowchart`] widget.
#[derive(Clone)]
pub struct FlowchartNode {
    pub(crate) direction: FlowDirection,
    pub(crate) nodes: Arc<[super::FlowchartNodeSpec]>,
    pub(crate) edges: Arc<[Edge]>,
    pub(crate) subgraphs: Arc<[super::FlowchartSubgraphSpec]>,
    pub(crate) class_defs: Arc<std::collections::HashMap<Arc<str>, Style>>,
    pub(crate) class_assignments: Arc<std::collections::HashMap<NodeId, Arc<str>>>,
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
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) output: Arc<FlowchartRenderOutput>,
    pub(crate) cache_key: FlowchartCacheKey,
    pub(crate) widget_key: FlowchartWidgetKey,
}

impl Default for FlowchartNode {
    fn default() -> Self {
        let flowchart = Flowchart::default();
        Self {
            direction: flowchart.direction,
            nodes: flowchart.nodes,
            edges: flowchart.edges,
            subgraphs: flowchart.subgraphs,
            class_defs: flowchart.class_defs,
            class_assignments: flowchart.class_assignments,
            style: flowchart.style,
            node_style: flowchart.node_style,
            edge_style: flowchart.edge_style,
            subgraph_style: flowchart.subgraph_style,
            label_style: flowchart.label_style,
            item_hover_style: flowchart.item_hover_style,
            border: flowchart.border,
            border_style: flowchart.border_style,
            padding: flowchart.padding,
            node_gap: flowchart.node_gap,
            layer_gap: flowchart.layer_gap,
            subgraph_padding: flowchart.subgraph_padding,
            max_node_width: flowchart.max_node_width,
            theme: flowchart.theme,
            on_node_click: None,
            on_edge_click: None,
            on_subgraph_click: None,
            on_node_hover: None,
            on_edge_hover: None,
            on_subgraph_hover: None,
            width: Length::Auto,
            height: Length::Auto,
            output: Arc::new(FlowchartRenderOutput::default()),
            cache_key: FlowchartCacheKey { hash: 0 },
            widget_key: FlowchartWidgetKey { hash: 0 },
        }
    }
}

impl From<Flowchart> for FlowchartNode {
    fn from(value: Flowchart) -> Self {
        let mut node = Self::default();
        super::reconcile_flowchart(&value, &mut node);
        node
    }
}

impl FlowchartNode {
    pub(crate) fn content_rect(&self, rect: Rect) -> Rect {
        flowchart_content_rect(rect, self.border, self.padding)
    }

    pub(crate) fn local_content_point(&self, rect: Rect, x: i16, y: i16) -> Option<(u16, u16)> {
        flowchart_local_content_point(rect, self.border, self.padding, x, y)
    }

    pub(crate) fn hit_test(
        &self,
        local_x: u16,
        local_y: u16,
    ) -> Option<(usize, FlowchartItemPath)> {
        let x = i16::try_from(local_x).unwrap_or(i16::MAX);
        let y = i16::try_from(local_y).unwrap_or(i16::MAX);

        if let Some((index, node)) = self.output.nodes.iter().enumerate().find(|(_, node)| {
            x >= node.rect.x
                && y >= node.rect.y
                && x < node.rect.x.saturating_add(node.rect.w as i16)
                && y < node.rect.y.saturating_add(node.rect.h as i16)
        }) {
            return Some((index, FlowchartItemPath::Node(node.id.clone())));
        }

        for edge in &self.output.edges {
            if edge.cells.iter().any(|cell| cell.x == x && cell.y == y)
                || edge.label_pos.is_some_and(|(lx, ly)| {
                    edge.label.as_ref().is_some_and(|label| {
                        y == ly && x >= lx && x < lx.saturating_add(label.chars().count() as i16)
                    })
                })
            {
                return Some((
                    1_000_000usize.saturating_add(edge.index),
                    FlowchartItemPath::Edge(edge.index),
                ));
            }
        }

        self.output
            .subgraphs
            .iter()
            .enumerate()
            .find(|(_, subgraph)| {
                x >= subgraph.header_rect.x
                    && y >= subgraph.header_rect.y
                    && x < subgraph
                        .header_rect
                        .x
                        .saturating_add(subgraph.header_rect.w as i16)
                    && y < subgraph
                        .header_rect
                        .y
                        .saturating_add(subgraph.header_rect.h as i16)
            })
            .map(|(index, subgraph)| {
                (
                    2_000_000usize.saturating_add(index),
                    FlowchartItemPath::Subgraph(subgraph.id.clone()),
                )
            })
    }

    pub(crate) fn item_event(&self, path: &FlowchartItemPath) -> Option<FlowchartItemEvent> {
        match path {
            FlowchartItemPath::Node(id) => self
                .output
                .nodes
                .iter()
                .find(|node| &node.id == id)
                .map(|node| {
                    FlowchartItemEvent::Node(FlowchartNodeEvent {
                        id: node.id.clone(),
                        label: node.label.clone(),
                    })
                }),
            FlowchartItemPath::Edge(index) => self
                .output
                .edges
                .iter()
                .find(|edge| edge.index == *index)
                .map(|edge| {
                    FlowchartItemEvent::Edge(FlowchartEdgeEvent {
                        from: edge.from.clone(),
                        to: edge.to.clone(),
                        label: edge.label.clone(),
                    })
                }),
            FlowchartItemPath::Subgraph(id) => self
                .output
                .subgraphs
                .iter()
                .find(|subgraph| &subgraph.id == id)
                .map(|subgraph| {
                    FlowchartItemEvent::Subgraph(FlowchartSubgraphEvent {
                        id: subgraph.id.clone(),
                        label: subgraph.label.clone(),
                    })
                }),
        }
    }
}

pub(crate) enum FlowchartItemEvent {
    Node(FlowchartNodeEvent),
    Edge(FlowchartEdgeEvent),
    Subgraph(FlowchartSubgraphEvent),
}

impl WidgetNode for FlowchartNode {
    fn has_on_click(&self) -> bool {
        self.on_node_click.is_some()
            || self.on_edge_click.is_some()
            || self.on_subgraph_click.is_some()
    }

    fn is_hoverable(&self) -> bool {
        self.has_on_click()
            || self.on_node_hover.is_some()
            || self.on_edge_hover.is_some()
            || self.on_subgraph_hover.is_some()
            || !self.item_hover_style.is_empty()
            || !self.theme.item_hover_style.is_empty()
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
        self.has_on_click().then(|| {
            self.local_content_point(rect, x, y)
                .and_then(|(local_x, local_y)| self.hit_test(local_x, local_y))
                .is_some()
        })
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

pub(crate) fn flowchart_content_rect(rect: Rect, border: bool, padding: Padding) -> Rect {
    rect.inner(border, padding)
}

pub(crate) fn flowchart_local_content_point(
    rect: Rect,
    border: bool,
    padding: Padding,
    x: i16,
    y: i16,
) -> Option<(u16, u16)> {
    let content = flowchart_content_rect(rect, border, padding);
    if !content.contains(x, y) {
        return None;
    }
    Some((
        u16::try_from(x.saturating_sub(content.x)).ok()?,
        u16::try_from(y.saturating_sub(content.y)).ok()?,
    ))
}

fn structural_hash(flowchart: &Flowchart) -> u64 {
    let mut hasher = DefaultHasher::new();
    flowchart.direction.hash(&mut hasher);
    flowchart.nodes.hash(&mut hasher);
    flowchart.edges.hash(&mut hasher);
    flowchart.subgraphs.hash(&mut hasher);
    hash_assignments(&flowchart.class_assignments, &mut hasher);
    hash_style_map(&flowchart.class_defs, &mut hasher);
    flowchart.node_gap.hash(&mut hasher);
    flowchart.layer_gap.hash(&mut hasher);
    flowchart.subgraph_padding.hash(&mut hasher);
    flowchart.max_node_width.hash(&mut hasher);
    flowchart.theme.edge_glyphs.hash(&mut hasher);
    flowchart.theme.node_styles.border_style.hash(&mut hasher);
    flowchart.theme.subgraph.border_style.hash(&mut hasher);
    hasher.finish()
}

fn hash_style_map(map: &std::collections::HashMap<Arc<str>, Style>, hasher: &mut impl Hasher) {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by_key(|(key, _)| *key);
    entries.len().hash(hasher);
    for (key, value) in entries {
        key.hash(hasher);
        value.hash(hasher);
    }
}

fn hash_assignments(map: &std::collections::HashMap<NodeId, Arc<str>>, hasher: &mut impl Hasher) {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by_key(|(key, _)| *key);
    entries.len().hash(hasher);
    for (key, value) in entries {
        key.hash(hasher);
        value.hash(hasher);
    }
}

fn widget_hash(flowchart: &Flowchart) -> u64 {
    let mut hasher = DefaultHasher::new();
    structural_hash(flowchart).hash(&mut hasher);
    flowchart.style.hash(&mut hasher);
    flowchart.node_style.hash(&mut hasher);
    flowchart.edge_style.hash(&mut hasher);
    flowchart.subgraph_style.hash(&mut hasher);
    flowchart.label_style.hash(&mut hasher);
    flowchart.item_hover_style.hash(&mut hasher);
    flowchart.border.hash(&mut hasher);
    flowchart.border_style.hash(&mut hasher);
    flowchart.padding.hash(&mut hasher);
    flowchart.theme.hash(&mut hasher);
    flowchart.width.hash(&mut hasher);
    flowchart.height.hash(&mut hasher);
    hasher.finish()
}
