use std::sync::Arc;

use crate::callback::Callback;
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{StyleSlot, Theme, ThemeRole};

use super::drop_target::{DropHighlight, DropTarget, PayloadAcceptFn};
use super::payload::{DragLeaveEvent, DragOverEvent, DropEvent, DropSlot};

#[derive(Clone)]
pub struct DropTargetNode {
    pub on_drag_over: Option<Callback<DragOverEvent>>,
    pub on_drag_leave: Option<Callback<DragLeaveEvent>>,
    pub on_drop: Option<Callback<DropEvent>>,
    pub accept_group: Option<Arc<str>>,
    pub can_accept: Option<PayloadAcceptFn>,
    pub highlight: DropHighlight,
    pub highlight_style: StyleSlot,
    pub drop_slot: DropSlot,
    pub enabled: bool,
    pub dnd_highlighted: bool,
}

impl Default for DropTargetNode {
    fn default() -> Self {
        Self {
            on_drag_over: None,
            on_drag_leave: None,
            on_drop: None,
            accept_group: None,
            can_accept: None,
            highlight: DropHighlight::None,
            highlight_style: StyleSlot::Inherit,
            drop_slot: DropSlot::Child,
            enabled: true,
            dnd_highlighted: false,
        }
    }
}

impl WidgetNode for DropTargetNode {
    fn has_on_click(&self) -> bool {
        self.enabled
            && (self.on_drop.is_some()
                || self.on_drag_over.is_some()
                || (self.highlight != DropHighlight::None
                    && !matches!(self.highlight_style, StyleSlot::Inherit)))
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        self.enabled
            && (self.on_drag_over.is_some()
                || (self.highlight != DropHighlight::None
                    && self
                        .highlight_style
                        .resolves_non_empty(theme, ThemeRole::DropTargetActive)))
    }
}

impl From<DropTarget> for DropTargetNode {
    fn from(value: DropTarget) -> Self {
        Self {
            on_drag_over: value.on_drag_over,
            on_drag_leave: value.on_drag_leave,
            on_drop: value.on_drop,
            accept_group: value.accept_group,
            can_accept: value.can_accept,
            highlight: value.highlight,
            highlight_style: value.highlight_style,
            drop_slot: value.drop_slot,
            enabled: value.enabled,
            dnd_highlighted: false,
        }
    }
}

impl From<DropTargetNode> for NodeKind {
    fn from(node: DropTargetNode) -> Self {
        NodeKind::DropTarget(node)
    }
}
