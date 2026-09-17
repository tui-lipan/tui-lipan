//! KeyCapture widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_key_capture;
pub use node::KeyCaptureNode;
pub use reconcile::reconcile_key_capture;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::style::Length;

/// An invisible focus target that hands every key to one handler.
///
/// Use it where a view needs keyboard input without a visible control: a shortcut recorder, a
/// "press any key" prompt, or a card whose keys are all app-defined. It draws nothing and takes no
/// space by default. While it holds focus, each key reaches [`Self::on_key`] before anything else
/// sees it: pending and prefix chords (framework and app command), clipboard shortcuts, overlay
/// dismissal, component `on_key` handlers, and focus traversal. Tab and Esc are included. A
/// consumed key also cancels any chord in progress. Keys the handler declines continue through
/// normal dispatch as if the target were not there.
#[derive(Clone)]
pub struct KeyCapture {
    pub(crate) on_key: Option<KeyHandler>,
    pub(crate) tab_stop: bool,
    pub(crate) disabled: bool,
    pub(crate) on_focus: Option<Callback<()>>,
    pub(crate) on_blur: Option<Callback<()>>,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl Default for KeyCapture {
    fn default() -> Self {
        Self {
            on_key: None,
            tab_stop: true,
            disabled: false,
            on_focus: None,
            on_blur: None,
            width: Length::Auto,
            height: Length::Auto,
        }
    }
}

impl KeyCapture {
    /// Create a key capture target.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the handler that receives every key while this target is focused. Return `true` from
    /// the handler to consume the key.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }

    /// When `false`, the target stays focusable by key or click but is skipped by Tab traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Disable the target. A disabled target cannot take focus.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set the callback fired when the target gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the target loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
        self
    }

    /// Set requested width. Defaults to `Auto`, which measures zero.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested height. Defaults to `Auto`, which measures zero.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }
}

impl From<KeyCapture> for Element {
    fn from(value: KeyCapture) -> Self {
        Element::new(ElementKind::KeyCapture(value))
    }
}

impl crate::layout::hash::LayoutHash for KeyCapture {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        Some(())
    }
}
