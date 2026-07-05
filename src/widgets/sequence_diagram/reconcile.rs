use std::sync::Arc;

use super::SequenceDiagram;
use super::layout::{build_sequence_output, build_sequence_output_for_width};
use super::node::{SequenceCacheKey, SequenceDiagramNode, SequenceWidgetKey};

pub fn reconcile_sequence_diagram(
    diagram: &SequenceDiagram,
    node: &mut SequenceDiagramNode,
) -> bool {
    reconcile_sequence_diagram_with_width(diagram, node, None)
}

pub fn reconcile_sequence_diagram_with_width(
    diagram: &SequenceDiagram,
    node: &mut SequenceDiagramNode,
    content_width: Option<u16>,
) -> bool {
    let cache_key = SequenceCacheKey::new(diagram);
    let widget_key = SequenceWidgetKey::new(diagram);
    let callbacks_changed =
        node.on_item_click != diagram.on_item_click || node.on_item_hover != diagram.on_item_hover;
    node.on_item_click = diagram.on_item_click.clone();
    node.on_item_hover = diagram.on_item_hover.clone();
    if widget_key == node.widget_key && content_width == node.output_content_width {
        return callbacks_changed;
    }

    let output = if cache_key == node.cache_key && content_width == node.output_content_width {
        #[cfg(feature = "profiling-tracing")]
        tracing::debug!(
            target: "tui_lipan::widgets::sequence_diagram",
            "sequence diagram layout cache hit"
        );
        node.output.clone()
    } else {
        #[cfg(feature = "profiling-tracing")]
        tracing::debug!(
            target: "tui_lipan::widgets::sequence_diagram",
            "sequence diagram layout cache miss"
        );
        Arc::new(match content_width {
            Some(width) => build_sequence_output_for_width(diagram, width),
            None => build_sequence_output(diagram),
        })
    };

    node.participants = diagram.participants.clone();
    node.steps = diagram.steps.clone();
    node.variant = diagram.variant;
    node.actor_glyph = diagram.actor_glyph.clone();
    node.style = diagram.style;
    node.theme = Arc::new(diagram.theme.clone());
    node.border = diagram.border;
    node.border_style = diagram.border_style;
    node.padding = diagram.padding;
    node.width = diagram.width;
    node.height = diagram.height;
    node.max_label_cells = diagram.max_label_cells;
    node.message_label_overflow = diagram.message_label_overflow;
    node.autonumber = diagram.autonumber;
    node.repeat_participants_at_bottom = diagram.repeat_participants_at_bottom;
    node.output = output;
    node.output_content_width = content_width;
    node.cache_key = cache_key;
    node.widget_key = widget_key;

    true
}
