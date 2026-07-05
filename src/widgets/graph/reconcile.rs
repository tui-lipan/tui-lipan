use std::sync::Arc;

use super::Graph;
use super::layout::build_graph_output;
use super::node::{GraphCacheKey, GraphRenderNode, GraphWidgetKey};

pub fn reconcile_graph(graph: &Graph, node: &mut GraphRenderNode) -> bool {
    let cache_key = GraphCacheKey::new(graph);
    let widget_key = GraphWidgetKey::new(graph);
    let callbacks_changed = node.on_node_click != graph.on_node_click
        || node.on_node_hover != graph.on_node_hover
        || node.on_node_focus != graph.on_node_focus
        || node.on_node_activate != graph.on_node_activate;
    node.on_node_click = graph.on_node_click.clone();
    node.on_node_hover = graph.on_node_hover.clone();
    node.on_node_focus = graph.on_node_focus.clone();
    node.on_node_activate = graph.on_node_activate.clone();
    if widget_key == node.widget_key {
        let previous_focused_path = node.focused_path.clone();
        if let Some(path) = graph.focused_path.clone() {
            node.focused_path = Some(path);
            node.normalize_focused_path();
        }
        return callbacks_changed || node.focused_path != previous_focused_path;
    }

    let focused_path = graph
        .focused_path
        .clone()
        .or_else(|| node.focused_path.clone());

    let output = if cache_key == node.cache_key {
        #[cfg(feature = "profiling-tracing")]
        tracing::debug!(target: "tui_lipan::widgets::graph", "graph layout cache hit");
        node.output.clone()
    } else {
        #[cfg(feature = "profiling-tracing")]
        tracing::debug!(target: "tui_lipan::widgets::graph", "graph layout cache miss");
        Arc::new(build_graph_output(graph))
    };

    node.root = graph.root.clone();
    node.direction = graph.direction;
    node.layout = graph.layout;
    node.gap_x = graph.gap_x;
    node.gap_y = graph.gap_y;
    node.max_node_width = graph.max_node_width;
    node.node_padding = graph.node_padding;
    node.node_border = graph.node_border;
    node.node_border_style = graph.node_border_style;
    node.style = graph.style;
    node.node_style = graph.node_style;
    node.node_hover_style = graph.node_hover_style;
    node.focusable = graph.focusable;
    node.focused_path = focused_path;
    node.node_focus_style = graph.node_focus_style;
    node.edge_style = graph.edge_style;
    node.edge_border_style = graph.edge_border_style;
    node.padding = graph.padding;
    node.border = graph.border;
    node.border_style = graph.border_style;
    node.width = graph.width;
    node.height = graph.height;
    node.output = output;
    node.normalize_focused_path();
    node.cache_key = cache_key;
    node.widget_key = widget_key;

    true
}
