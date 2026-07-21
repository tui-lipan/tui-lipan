pub(crate) mod layout;
pub(crate) mod node;
pub(crate) mod reconcile;

pub use node::PopoverNode;
pub(crate) use reconcile::*;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind, IntoElement};
use crate::overlay::OverlayScope;
use crate::style::Length;

/// Popover placement relative to the trigger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PopoverPlacement {
    /// Below the trigger, left-aligned.
    #[default]
    BelowStart,
    /// Below the trigger, centered.
    BelowCenter,
    /// Below the trigger, right-aligned.
    BelowEnd,
    /// Above the trigger, left-aligned.
    AboveStart,
    /// Above the trigger, centered.
    AboveCenter,
    /// Above the trigger, right-aligned.
    AboveEnd,
    /// Right of the trigger, top-aligned.
    RightStart,
    /// Right of the trigger, centered.
    RightCenter,
    /// Right of the trigger, bottom-aligned.
    RightEnd,
    /// Left of the trigger, top-aligned.
    LeftStart,
    /// Left of the trigger, centered.
    LeftCenter,
    /// Left of the trigger, bottom-aligned.
    LeftEnd,
}

/// Signed offset applied to the popover position.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PopoverOffset {
    /// Horizontal offset in cells.
    pub x: i16,
    /// Vertical offset in cells.
    pub y: i16,
}

impl PopoverOffset {
    /// Zero offset.
    pub const ZERO: Self = Self { x: 0, y: 0 };
}

impl From<(i16, i16)> for PopoverOffset {
    fn from(value: (i16, i16)) -> Self {
        Self {
            x: value.0,
            y: value.1,
        }
    }
}

/// A popover widget.
#[derive(Clone)]
pub struct Popover {
    pub(crate) trigger: Box<Element>,
    pub(crate) content: Box<Element>,
    pub(crate) on_close: Option<Callback<()>>,
    pub(crate) open: bool,
    pub(crate) scope: OverlayScope,
    pub(crate) placement: PopoverPlacement,
    pub(crate) offset: PopoverOffset,
    pub(crate) clamp: bool,
    pub(crate) auto_flip: bool,
    pub(crate) min_trigger_width: bool,
    pub(crate) fit_trigger_width: bool,
    pub(crate) max_width: Option<Length>,
    pub(crate) anchor: Option<(u16, u16)>,
    pub(crate) capture_focus: bool,
    pub(crate) auto_focus: bool,
}

impl Default for Popover {
    fn default() -> Self {
        Self::new()
    }
}

impl Popover {
    /// Create a new popover.
    pub fn new() -> Self {
        Self {
            trigger: Box::new(crate::widgets::Spacer::new().into()),
            content: Box::new(crate::widgets::Spacer::new().into()),
            on_close: None,
            open: false,
            scope: OverlayScope::RootPortal,
            placement: PopoverPlacement::default(),
            offset: PopoverOffset::ZERO,
            clamp: true,
            auto_flip: true,
            min_trigger_width: true,
            fit_trigger_width: false,
            max_width: None,
            anchor: None,
            capture_focus: true,
            auto_focus: true,
        }
    }

    /// Set the trigger element.
    pub fn trigger(mut self, trigger: impl IntoElement) -> Self {
        self.trigger = Box::new(trigger.into());
        self
    }

    /// Set the content element.
    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Box::new(content.into());
        self
    }

    /// Set open state.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Set overlay scope (portal vs local rendering).
    pub fn scope(mut self, scope: OverlayScope) -> Self {
        self.scope = scope;
        self
    }

    /// Control whether an open root-portal popover captures and traps focus.
    ///
    /// Disable this for passive overlays such as autocomplete suggestions that
    /// must render through the root portal while their trigger retains keyboard focus.
    /// This has no effect on local popovers.
    pub fn capture_focus(mut self, capture_focus: bool) -> Self {
        self.capture_focus = capture_focus;
        self
    }

    /// Control whether an open root-portal popover focuses its first focusable descendant.
    ///
    /// Disabling this keeps keyboard and pointer capture active while focus is suspended.
    pub fn auto_focus(mut self, auto_focus: bool) -> Self {
        self.auto_focus = auto_focus;
        self
    }

    /// Set popover placement relative to the trigger.
    pub fn placement(mut self, placement: PopoverPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Set popover offset.
    pub fn offset(mut self, offset: impl Into<PopoverOffset>) -> Self {
        self.offset = offset.into();
        self
    }

    /// Clamp the popover to the viewport bounds.
    pub fn clamp(mut self, clamp: bool) -> Self {
        self.clamp = clamp;
        self
    }

    /// Automatically flip placement when it overflows the viewport.
    pub fn auto_flip(mut self, auto_flip: bool) -> Self {
        self.auto_flip = auto_flip;
        self
    }

    /// Ensure the popover is at least as wide as the trigger.
    ///
    /// This is enabled by default. The popover may still grow wider when content
    /// requires more space, unless capped by [`Self::max_width`] or forced by
    /// [`Self::fit_trigger_width`].
    pub fn min_trigger_width(mut self, min_trigger_width: bool) -> Self {
        self.min_trigger_width = min_trigger_width;
        self
    }

    /// Force popover width to exactly match trigger width.
    pub fn fit_trigger_width(mut self, fit_trigger_width: bool) -> Self {
        self.fit_trigger_width = fit_trigger_width;
        self
    }

    /// Cap the resolved popover width.
    ///
    /// Percent values resolve against the active overlay bounds. The cap applies
    /// after trigger-width fitting/minimums, so it can intentionally make the
    /// popover narrower than its trigger.
    pub fn max_width(mut self, max_width: Length) -> Self {
        self.max_width = Some(max_width);
        self
    }

    /// Anchor the popover to an absolute position (content coordinates).
    pub fn anchor(mut self, anchor: Option<(u16, u16)>) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set on-close callback.
    pub fn on_close(mut self, cb: Callback<()>) -> Self {
        self.on_close = Some(cb);
        self
    }
}

impl From<Popover> for Element {
    fn from(popover: Popover) -> Self {
        Element::new(ElementKind::Popover(popover)).with_layout(crate::style::LayoutConstraints {
            min_w: crate::style::Length::Px(0),
            min_h: crate::style::Length::Px(0),
            ..Default::default()
        })
    }
}

impl crate::layout::hash::LayoutHash for Popover {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.open.hash(hasher);
        self.scope.hash(hasher);
        self.placement.hash(hasher);
        self.offset.hash(hasher);
        self.min_trigger_width.hash(hasher);
        self.fit_trigger_width.hash(hasher);
        self.max_width.hash(hasher);
        self.anchor.hash(hasher);
        self.capture_focus.hash(hasher);
        self.auto_focus.hash(hasher);
        recurse(self.trigger.as_ref())?.hash(hasher);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::element::ElementKind;

    #[test]
    fn popover_defaults_to_root_portal_scope() {
        let element: Element = Popover::new().into();

        let ElementKind::Popover(popover) = element.kind else {
            panic!("expected popover element");
        };

        assert_eq!(popover.scope, OverlayScope::RootPortal);
        assert!(popover.capture_focus);
    }

    #[test]
    fn popover_scope_builder_updates_scope() {
        let element: Element = Popover::new().scope(OverlayScope::Local).into();

        let ElementKind::Popover(popover) = element.kind else {
            panic!("expected popover element");
        };

        assert_eq!(popover.scope, OverlayScope::Local);
    }

    #[test]
    fn popover_capture_focus_builder_updates_capture() {
        let element: Element = Popover::new().capture_focus(false).into();

        let ElementKind::Popover(popover) = element.kind else {
            panic!("expected popover element");
        };

        assert!(!popover.capture_focus);
    }
}
