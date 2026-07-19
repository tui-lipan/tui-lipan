//! Mouse region widget.

mod layout;
mod node;
mod reconcile;

pub(crate) use self::layout::measure_mouse_region;
pub use self::node::MouseRegionNode;
pub(crate) use self::reconcile::reconcile_mouse_region;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::core::event::{KeyMods, MouseDragEvent, MouseEvent, MouseMoveEvent};
use crate::core::mask::CellMask;
use crate::style::{LayoutConstraints, Length, Style, StyleSlot, VisualEffect};
use std::sync::Arc;

/// A wrapper that handles pointer interactions for its subtree.
#[derive(Clone, Default)]
pub struct MouseRegion {
    pub(crate) child: Option<Box<Element>>,
    pub(crate) on_click: Option<Callback<MouseEvent>>,
    pub(crate) on_mouse_down: Option<Callback<MouseEvent>>,
    pub(crate) bubble_mouse_down: bool,
    pub(crate) on_mouse_up: Option<Callback<MouseEvent>>,
    pub(crate) on_mouse_move: Option<Callback<MouseMoveEvent>>,
    pub(crate) on_drag_start: Option<Callback<MouseDragEvent>>,
    pub(crate) on_drag: Option<Callback<MouseDragEvent>>,
    pub(crate) on_drag_end: Option<Callback<MouseDragEvent>>,
    pub(crate) drag_required_mods: Option<KeyMods>,
    pub(crate) on_right_drag_start: Option<Callback<MouseDragEvent>>,
    pub(crate) on_right_drag: Option<Callback<MouseDragEvent>>,
    pub(crate) on_right_drag_end: Option<Callback<MouseDragEvent>>,
    pub(crate) right_drag_required_mods: Option<KeyMods>,
    pub(crate) on_hover_change: Option<Callback<bool>>,
    pub(crate) hit_test: Option<Arc<dyn Fn(u16, u16) -> bool + Send + Sync>>,
    pub(crate) capture_click: bool,
    pub(crate) capture_required_mods: Option<KeyMods>,
    pub(crate) hover_style: StyleSlot,
    pub(crate) hover_effects: Vec<VisualEffect>,
    pub(crate) enabled: bool,
}

impl MouseRegion {
    /// Create an empty mouse region.
    pub fn new() -> Self {
        Self {
            enabled: true,
            ..Self::default()
        }
    }

    /// Set wrapped child content.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = Some(Box::new(child.into()));
        self
    }

    /// Set pointer-move callback.
    pub fn on_mouse_move(mut self, cb: Callback<MouseMoveEvent>) -> Self {
        self.on_mouse_move = Some(cb);
        self
    }

    /// Set drag-start callback (fires once after left-button movement exceeds click threshold).
    pub fn on_drag_start(mut self, cb: Callback<MouseDragEvent>) -> Self {
        self.on_drag_start = Some(cb);
        self
    }

    /// Set drag callback (fires on each left-button drag tick after drag start).
    pub fn on_drag(mut self, cb: Callback<MouseDragEvent>) -> Self {
        self.on_drag = Some(cb);
        self
    }

    /// Set drag-end callback (fires on left-button release after a drag started).
    pub fn on_drag_end(mut self, cb: Callback<MouseDragEvent>) -> Self {
        self.on_drag_end = Some(cb);
        self
    }

    /// Require these modifiers before left-button drag callbacks can start.
    ///
    /// Each `true` flag in `mods` must be held when the mouse button is pressed;
    /// extra modifiers are allowed. `KeyMods::ALT` therefore means "Alt must be
    /// held", not "Alt and no other modifiers".
    pub fn drag_requires_mods(mut self, mods: KeyMods) -> Self {
        self.drag_required_mods = Some(mods);
        self
    }

    /// Set right-button drag-start callback.
    ///
    /// Fires once after right-button movement exceeds the click threshold.
    pub fn on_right_drag_start(mut self, cb: Callback<MouseDragEvent>) -> Self {
        self.on_right_drag_start = Some(cb);
        self
    }

    /// Set right-button drag callback.
    ///
    /// Fires on each right-button drag tick after drag start.
    pub fn on_right_drag(mut self, cb: Callback<MouseDragEvent>) -> Self {
        self.on_right_drag = Some(cb);
        self
    }

    /// Set right-button drag-end callback.
    ///
    /// Fires on right-button release after a drag started.
    pub fn on_right_drag_end(mut self, cb: Callback<MouseDragEvent>) -> Self {
        self.on_right_drag_end = Some(cb);
        self
    }

    /// Require these modifiers before right-button drag callbacks can start.
    ///
    /// Each `true` flag in `mods` must be held when the mouse button is pressed;
    /// extra modifiers are allowed.
    pub fn right_drag_requires_mods(mut self, mods: KeyMods) -> Self {
        self.right_drag_required_mods = Some(mods);
        self
    }

    /// Set hover change callback (fires true on enter, false on leave).
    ///
    /// A hover transition repaints on its own only when the region has hover
    /// visuals (`hover_style` or `hover_effects`). For click-only regions the
    /// `Update` returned from this callback's message decides — return
    /// `Update::none()` when nothing rendered depends on hover state.
    pub fn on_hover_change(mut self, cb: Callback<bool>) -> Self {
        self.on_hover_change = Some(cb);
        self
    }

    /// Set click callback (fires on mouse-up after a press on the same node).
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set custom hit-test predicate using local coordinates.
    pub fn hit_test(mut self, f: impl Fn(u16, u16) -> bool + Send + Sync + 'static) -> Self {
        self.hit_test = Some(Arc::new(f));
        self
    }

    /// Pointer hit testing against a [`CellMask`] in region-local coordinates (same as effect scope).
    pub fn cell_mask(mut self, mask: Arc<CellMask>) -> Self {
        let m = Arc::clone(&mask);
        self.hit_test = Some(Arc::new(move |x, y| m.test_scope_local(x as i16, y as i16)));
        self
    }

    /// Set mouse-down callback (fires immediately on left button press).
    pub fn on_mouse_down(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_mouse_down = Some(cb);
        self
    }

    /// Also fire `on_mouse_down` when a descendant receives the left-button press.
    ///
    /// This is non-consuming: the descendant still receives its normal click or
    /// focus handling. It is useful for container-level focus policies such as a
    /// window manager focusing a pane when any child is clicked.
    pub fn bubble_mouse_down(mut self, bubble: bool) -> Self {
        self.bubble_mouse_down = bubble;
        self
    }

    /// Set mouse-up callback (fires immediately on left button release over the region).
    pub fn on_mouse_up(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_mouse_up = Some(cb);
        self
    }

    /// Control whether this region captures left-clicks over interactive children.
    pub fn capture_click(mut self, capture: bool) -> Self {
        self.capture_click = capture;
        self
    }

    /// Capture pointer handling over descendants while these modifiers are held.
    ///
    /// This is useful for compositor-style wrappers around focusable children such as
    /// terminals: `capture_requires_mods(KeyMods::ALT)` lets Alt-click/Alt-drag be
    /// consumed by the wrapper instead of starting terminal selection or forwarding a
    /// mouse report to the PTY. Extra modifiers are allowed.
    pub fn capture_requires_mods(mut self, mods: KeyMods) -> Self {
        self.capture_required_mods = Some(mods);
        self
    }

    /// Set style applied while hovered as an underlay block.
    ///
    /// Painted before children, so it is effective for `bg` color and modifier
    /// changes (e.g. `BOLD`, `UNDERLINE`). Setting only `fg` has no visible
    /// effect because child content overwrites foreground colors on top. To
    /// change text color on hover use
    /// `hover_effect(VisualEffect::transform_fg(ColorTransform::Tint(color, 1.0)))`.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's hover style with additional fields.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit hover style from the active theme.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Add a visual effect applied while hovered (post-processing, affects text fg/bg).
    ///
    /// Use [`VisualEffect`] constructors for common cases:
    /// `VisualEffect::transform_fg(ColorTransform::Tint(color, 1.0))`,
    /// `VisualEffect::dim(0.3)`, etc.
    pub fn hover_effect(mut self, effect: VisualEffect) -> Self {
        self.hover_effects.push(effect);
        self
    }

    /// Enable/disable click, move, and hover behavior.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl From<MouseRegion> for Element {
    fn from(value: MouseRegion) -> Self {
        let (min_w, min_h) = measure_mouse_region(&value, None, None);
        Element::new(ElementKind::MouseRegion(value)).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}

impl crate::layout::hash::LayoutHash for MouseRegion {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.enabled.hash(hasher);
        self.on_click.is_some().hash(hasher);
        self.on_mouse_down.is_some().hash(hasher);
        self.bubble_mouse_down.hash(hasher);
        self.on_mouse_up.is_some().hash(hasher);
        self.on_mouse_move.is_some().hash(hasher);
        self.on_drag_start.is_some().hash(hasher);
        self.on_drag.is_some().hash(hasher);
        self.on_drag_end.is_some().hash(hasher);
        self.drag_required_mods.hash(hasher);
        self.on_right_drag_start.is_some().hash(hasher);
        self.on_right_drag.is_some().hash(hasher);
        self.on_right_drag_end.is_some().hash(hasher);
        self.right_drag_required_mods.hash(hasher);
        self.on_hover_change.is_some().hash(hasher);
        self.capture_click.hash(hasher);
        self.capture_required_mods.hash(hasher);
        self.hover_style.hash(hasher);
        self.hover_effects.hash(hasher);
        if let Some(child) = self.child.as_ref() {
            recurse(child.as_ref())?.hash(hasher);
        }
        Some(())
    }
}
