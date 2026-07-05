use crate::callback::Callback;
use crate::core::event::{KeyMods, MouseDragEvent, MouseEvent, MouseMoveEvent};
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{Rect, StyleSlot, Theme, ThemeRole, VisualEffect};
use std::sync::Arc;

use super::MouseRegion;

/// Runtime node for pointer event routing.
#[derive(Clone, Default)]
pub struct MouseRegionNode {
    pub on_click: Option<Callback<MouseEvent>>,
    pub on_mouse_down: Option<Callback<MouseEvent>>,
    pub bubble_mouse_down: bool,
    pub on_mouse_up: Option<Callback<MouseEvent>>,
    pub on_mouse_move: Option<Callback<MouseMoveEvent>>,
    pub on_drag_start: Option<Callback<MouseDragEvent>>,
    pub on_drag: Option<Callback<MouseDragEvent>>,
    pub on_drag_end: Option<Callback<MouseDragEvent>>,
    pub drag_required_mods: Option<KeyMods>,
    pub on_right_drag_start: Option<Callback<MouseDragEvent>>,
    pub on_right_drag: Option<Callback<MouseDragEvent>>,
    pub on_right_drag_end: Option<Callback<MouseDragEvent>>,
    pub right_drag_required_mods: Option<KeyMods>,
    pub on_hover_change: Option<Callback<bool>>,
    pub hit_test: Option<Arc<dyn Fn(u16, u16) -> bool + Send + Sync>>,
    pub capture_click: bool,
    pub capture_required_mods: Option<KeyMods>,
    pub hover_style: StyleSlot,
    pub hover_effects: Vec<VisualEffect>,
    pub enabled: bool,
}

impl WidgetNode for MouseRegionNode {
    fn has_on_click(&self) -> bool {
        self.enabled
            && (self.on_click.is_some()
                || self.on_mouse_down.is_some()
                || self.on_mouse_up.is_some()
                || self.on_drag_start.is_some()
                || self.on_drag.is_some()
                || self.on_drag_end.is_some()
                || self.on_right_drag_start.is_some()
                || self.on_right_drag.is_some()
                || self.on_right_drag_end.is_some())
    }

    fn has_on_mouse_move(&self) -> bool {
        self.enabled
            && (self.on_mouse_move.is_some()
                || self.on_drag_start.is_some()
                || self.on_drag.is_some()
                || self.on_drag_end.is_some()
                || self.on_right_drag_start.is_some()
                || self.on_right_drag.is_some()
                || self.on_right_drag_end.is_some())
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        self.enabled
            && (self.on_click.is_some()
                || self.on_mouse_up.is_some()
                || self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
                || !self.hover_effects.is_empty())
    }

    fn hit_test_refinement(&self, x: i16, y: i16, rect: Rect) -> Option<bool> {
        let f = self.hit_test.as_ref()?;
        let local_x = x.saturating_sub(rect.x) as u16;
        let local_y = y.saturating_sub(rect.y) as u16;
        Some(f(local_x, local_y))
    }
}

impl From<MouseRegion> for MouseRegionNode {
    fn from(value: MouseRegion) -> Self {
        Self {
            on_click: value.on_click,
            on_mouse_down: value.on_mouse_down,
            bubble_mouse_down: value.bubble_mouse_down,
            on_mouse_up: value.on_mouse_up,
            on_mouse_move: value.on_mouse_move,
            on_drag_start: value.on_drag_start,
            on_drag: value.on_drag,
            on_drag_end: value.on_drag_end,
            drag_required_mods: value.drag_required_mods,
            on_right_drag_start: value.on_right_drag_start,
            on_right_drag: value.on_right_drag,
            on_right_drag_end: value.on_right_drag_end,
            right_drag_required_mods: value.right_drag_required_mods,
            on_hover_change: value.on_hover_change,
            hit_test: value.hit_test,
            capture_click: value.capture_click,
            capture_required_mods: value.capture_required_mods,
            hover_style: value.hover_style,
            hover_effects: value.hover_effects,
            enabled: value.enabled,
        }
    }
}

impl From<MouseRegionNode> for NodeKind {
    fn from(node: MouseRegionNode) -> Self {
        NodeKind::MouseRegion(node)
    }
}
