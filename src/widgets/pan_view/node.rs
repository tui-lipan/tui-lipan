use crate::callback::Callback;
use crate::core::element::Key;
use crate::core::node::WidgetNode;

use super::{PanEvent, PanKeymap, PanView};

/// Runtime state for a `PanView` node.
#[derive(Clone)]
pub struct PanViewNode {
    pub offset_x: i32,
    pub offset_y: i32,
    pub content_w: u16,
    pub content_h: u16,
    pub viewport_w: u16,
    pub viewport_h: u16,
    pub keymap: PanKeymap,
    pub clamp: bool,
    pub free_pan_margin: Option<(u16, u16)>,
    pub drag_to_pan: bool,
    pub wheel_to_pan: bool,
    pub focusable: bool,
    pub tab_stop: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
    pub key_step: (u16, u16),
    pub input_override: Option<(i32, i32)>,
    pub input_dirty: bool,
    pub state_key: Option<Key>,
    pub on_pan: Option<Callback<PanEvent>>,
}

impl From<PanView> for PanViewNode {
    fn from(value: PanView) -> Self {
        let (offset_x, offset_y) = value.offset.unwrap_or((0, 0));
        Self {
            offset_x,
            offset_y,
            content_w: 0,
            content_h: 0,
            viewport_w: 0,
            viewport_h: 0,
            keymap: value.keymap,
            clamp: value.clamp,
            free_pan_margin: value.free_pan_margin,
            drag_to_pan: value.drag_to_pan,
            wheel_to_pan: value.wheel_to_pan,
            focusable: value.focusable,
            tab_stop: value.tab_stop,
            on_focus: value.on_focus,
            on_blur: value.on_blur,
            key_step: value.key_step,
            input_override: None,
            input_dirty: false,
            state_key: None,
            on_pan: value.on_pan,
        }
    }
}

impl WidgetNode for PanViewNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn is_tab_stop(&self) -> bool {
        self.focusable && self.tab_stop
    }

    fn on_focus_callback(&self) -> Option<&Callback<()>> {
        self.on_focus.as_ref()
    }

    fn on_blur_callback(&self) -> Option<&Callback<()>> {
        self.on_blur.as_ref()
    }

    fn has_on_click(&self) -> bool {
        self.drag_to_pan
    }
}
