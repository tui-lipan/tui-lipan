use std::sync::Arc;

use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{StyleSlot, Theme, ThemeRole};

use super::drag_source::{DragSource, DragStartHandler};
use super::payload::{DragCancelEvent, DragPreview, DragSlot, DragStartedEvent};

/// Runtime node for `DragSource`.
#[derive(Clone)]
pub struct DragSourceNode {
    pub on_drag_start: Option<DragStartHandler>,
    pub on_drag_cancel: Option<crate::callback::Callback<DragCancelEvent>>,
    pub on_drag_started: Option<crate::callback::Callback<DragStartedEvent>>,
    pub drag_group: Option<Arc<str>>,
    pub preview: DragPreview,
    pub dragging_style: StyleSlot,
    pub drag_slot: DragSlot,
    pub preview_max_width: Option<u16>,
    pub preview_max_height: Option<u16>,
    pub threshold: u16,
    pub enabled: bool,
    pub is_dragging: bool,
}

impl Default for DragSourceNode {
    fn default() -> Self {
        Self {
            on_drag_start: None,
            on_drag_cancel: None,
            on_drag_started: None,
            drag_group: None,
            preview: DragPreview::None,
            dragging_style: StyleSlot::Inherit,
            drag_slot: DragSlot::Collapse,
            preview_max_width: None,
            preview_max_height: None,
            threshold: 3,
            enabled: true,
            is_dragging: false,
        }
    }
}

impl WidgetNode for DragSourceNode {
    fn has_on_click(&self) -> bool {
        self.enabled && self.on_drag_start.is_some()
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        self.enabled
            && (self
                .dragging_style
                .resolves_non_empty(theme, ThemeRole::DragSource)
                || self.on_drag_start.is_some())
    }
}

impl From<DragSource> for DragSourceNode {
    fn from(value: DragSource) -> Self {
        Self {
            on_drag_start: value.on_drag_start,
            on_drag_cancel: value.on_drag_cancel,
            on_drag_started: value.on_drag_started,
            drag_group: value.drag_group,
            preview: value.preview,
            dragging_style: value.dragging_style,
            drag_slot: value.drag_slot,
            preview_max_width: value.preview_max_width,
            preview_max_height: value.preview_max_height,
            threshold: value.threshold,
            enabled: value.enabled,
            is_dragging: false,
        }
    }
}

impl From<DragSourceNode> for NodeKind {
    fn from(node: DragSourceNode) -> Self {
        NodeKind::DragSource(node)
    }
}
