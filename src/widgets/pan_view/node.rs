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
    pub focusable: bool,
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
            focusable: value.focusable,
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

    fn has_on_click(&self) -> bool {
        self.drag_to_pan
    }
}
