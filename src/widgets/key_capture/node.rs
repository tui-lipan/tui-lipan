use crate::callback::{Callback, KeyHandler};
use crate::core::node::{NodeKind, WidgetNode};

use super::KeyCapture;

#[derive(Clone)]
pub struct KeyCaptureNode {
    pub on_key: Option<KeyHandler>,
    pub tab_stop: bool,
    pub disabled: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
}

impl WidgetNode for KeyCaptureNode {
    fn is_disabled(&self) -> bool {
        self.disabled
    }
    fn is_focusable(&self) -> bool {
        true
    }
    fn is_tab_stop(&self) -> bool {
        self.tab_stop
    }
    fn on_focus_callback(&self) -> Option<&Callback<()>> {
        self.on_focus.as_ref()
    }
    fn on_blur_callback(&self) -> Option<&Callback<()>> {
        self.on_blur.as_ref()
    }
}

impl From<KeyCapture> for KeyCaptureNode {
    fn from(capture: KeyCapture) -> Self {
        Self {
            on_key: capture.on_key,
            tab_stop: capture.tab_stop,
            disabled: capture.disabled,
            on_focus: capture.on_focus,
            on_blur: capture.on_blur,
        }
    }
}

impl From<KeyCaptureNode> for NodeKind {
    fn from(node: KeyCaptureNode) -> Self {
        NodeKind::KeyCapture(node)
    }
}
