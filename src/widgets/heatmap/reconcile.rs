use std::sync::Arc;

use super::{
    Heatmap, HeatmapNode,
    node::{HeatmapCacheKey, HeatmapWidgetKey, build_render_output},
};

pub(crate) fn reconcile_heatmap_node(heatmap: &Heatmap, node: &mut HeatmapNode) -> bool {
    let cache_key = HeatmapCacheKey::new(heatmap);
    let widget_key = HeatmapWidgetKey::new(heatmap, cache_key);
    if widget_key == node.widget_key {
        return false;
    }

    let output = if cache_key == node.cache_key {
        Arc::clone(&node.output)
    } else {
        Arc::new(build_render_output(heatmap))
    };

    node.row_labels = heatmap.row_labels.clone();
    node.column_labels = heatmap.column_labels.clone();
    node.gradient = heatmap.gradient;
    node.cell_mode = heatmap.cell_mode.clone();
    node.cell_width = heatmap.effective_cell_width();
    node.gap_x = heatmap.gap_x;
    node.gap_y = heatmap.gap_y;
    node.legend_gap = heatmap.legend_gap;
    node.legend_spacing = heatmap.legend_spacing;
    node.legend_width = heatmap.legend_width;
    node.show_values = heatmap.show_values;
    node.show_legend = heatmap.show_legend;
    node.style = heatmap.style;
    node.label_style = heatmap.label_style;
    node.legend_style = heatmap.legend_style;
    node.padding = heatmap.padding;
    node.border = heatmap.border;
    node.border_style = heatmap.border_style;
    node.output = output;
    node.cache_key = cache_key;
    node.widget_key = widget_key;

    true
}
