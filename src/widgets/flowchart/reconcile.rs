use std::sync::Arc;

use super::Flowchart;
use super::layout::build_flowchart_output;
use super::node::{FlowchartCacheKey, FlowchartNode, FlowchartWidgetKey};

pub fn reconcile_flowchart(flowchart: &Flowchart, node: &mut FlowchartNode) -> bool {
    let cache_key = FlowchartCacheKey::new(flowchart);
    let widget_key = FlowchartWidgetKey::new(flowchart);
    let callbacks_changed = node.on_node_click != flowchart.on_node_click
        || node.on_edge_click != flowchart.on_edge_click
        || node.on_subgraph_click != flowchart.on_subgraph_click
        || node.on_node_hover != flowchart.on_node_hover
        || node.on_edge_hover != flowchart.on_edge_hover
        || node.on_subgraph_hover != flowchart.on_subgraph_hover;

    node.on_node_click = flowchart.on_node_click.clone();
    node.on_edge_click = flowchart.on_edge_click.clone();
    node.on_subgraph_click = flowchart.on_subgraph_click.clone();
    node.on_node_hover = flowchart.on_node_hover.clone();
    node.on_edge_hover = flowchart.on_edge_hover.clone();
    node.on_subgraph_hover = flowchart.on_subgraph_hover.clone();

    if widget_key == node.widget_key {
        return callbacks_changed;
    }

    let output = if cache_key == node.cache_key {
        #[cfg(feature = "profiling-tracing")]
        tracing::debug!(target: "tui_lipan::widgets::flowchart", "flowchart layout cache hit");
        node.output.clone()
    } else {
        #[cfg(feature = "profiling-tracing")]
        tracing::debug!(target: "tui_lipan::widgets::flowchart", "flowchart layout cache miss");
        Arc::new(build_flowchart_output(flowchart))
    };

    node.direction = flowchart.direction;
    node.nodes = flowchart.nodes.clone();
    node.edges = flowchart.edges.clone();
    node.subgraphs = flowchart.subgraphs.clone();
    node.class_defs = flowchart.class_defs.clone();
    node.class_assignments = flowchart.class_assignments.clone();
    node.style = flowchart.style;
    node.node_style = flowchart.node_style;
    node.edge_style = flowchart.edge_style;
    node.subgraph_style = flowchart.subgraph_style;
    node.label_style = flowchart.label_style;
    node.item_hover_style = flowchart.item_hover_style;
    node.border = flowchart.border;
    node.border_style = flowchart.border_style;
    node.padding = flowchart.padding;
    node.node_gap = flowchart.node_gap;
    node.layer_gap = flowchart.layer_gap;
    node.subgraph_padding = flowchart.subgraph_padding;
    node.max_node_width = flowchart.max_node_width;
    node.theme = flowchart.theme.clone();
    node.width = flowchart.width;
    node.height = flowchart.height;
    node.output = output;
    node.cache_key = cache_key;
    node.widget_key = widget_key;

    true
}
