use std::hash::Hash;
use std::rc::Rc;
use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::layout::hash::LayoutHash;
use crate::style::{LayoutConstraints, Length, Style, StyleSlot};

use super::drop_target_layout::measure_drop_target;
use super::payload::{DragLeaveEvent, DragOverEvent, DragPayload, DropEvent, DropSlot};

/// Predicate used to accept or reject payloads.
pub type PayloadAcceptFn = Rc<dyn Fn(&dyn DragPayload) -> bool>;

/// How to visualize a compatible drag hovering a `DropTarget`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DropHighlight {
    /// No highlight layer (callbacks still run).
    #[default]
    None,
    /// Solid fill using [`DropTarget`](DropTarget) `highlight_style`.
    Fill,
    /// Bordered placeholder frame (same helper as Image pending decode).
    Placeholder,
    /// Tint drawn **after** the child so content remains visible underneath.
    Overlay,
}

#[derive(Clone)]
/// Wrapper widget that marks its child as a generic drop target.
pub struct DropTarget {
    pub(crate) child: Option<Box<Element>>,
    pub(crate) on_drag_over: Option<Callback<DragOverEvent>>,
    pub(crate) on_drag_leave: Option<Callback<DragLeaveEvent>>,
    pub(crate) on_drop: Option<Callback<DropEvent>>,
    pub(crate) accept_group: Option<Arc<str>>,
    pub(crate) can_accept: Option<PayloadAcceptFn>,
    pub(crate) highlight: DropHighlight,
    pub(crate) highlight_style: StyleSlot,
    pub(crate) drop_slot: DropSlot,
    pub(crate) enabled: bool,
}

impl Default for DropTarget {
    fn default() -> Self {
        Self {
            child: None,
            on_drag_over: None,
            on_drag_leave: None,
            on_drop: None,
            accept_group: None,
            can_accept: None,
            highlight: DropHighlight::None,
            highlight_style: StyleSlot::Inherit,
            drop_slot: DropSlot::Child,
            enabled: true,
        }
    }
}

impl DropTarget {
    /// Create an empty drop target wrapper.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the wrapped child element.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = Some(Box::new(child.into()));
        self
    }

    /// Callback emitted when a compatible payload hovers this target.
    pub fn on_drag_over(mut self, cb: Callback<DragOverEvent>) -> Self {
        self.on_drag_over = Some(cb);
        self
    }

    /// Callback emitted when an active drag leaves this target.
    pub fn on_drag_leave(mut self, cb: Callback<DragLeaveEvent>) -> Self {
        self.on_drag_leave = Some(cb);
        self
    }

    /// Callback emitted when payload is dropped on this target.
    pub fn on_drop(mut self, cb: Callback<DropEvent>) -> Self {
        self.on_drop = Some(cb);
        self
    }

    /// Restrict this target to a compatibility group.
    pub fn accept_group(mut self, group: impl Into<Arc<str>>) -> Self {
        self.accept_group = Some(group.into());
        self
    }

    /// Remove group restriction; target accepts all source groups.
    pub fn clear_accept_group(mut self) -> Self {
        self.accept_group = None;
        self
    }

    /// Set payload acceptance predicate.
    pub fn can_accept(mut self, accept: PayloadAcceptFn) -> Self {
        self.can_accept = Some(accept);
        self
    }

    /// Set payload acceptance predicate from closure.
    pub fn can_accept_with(mut self, accept: impl Fn(&dyn DragPayload) -> bool + 'static) -> Self {
        self.can_accept = Some(Rc::new(accept));
        self
    }

    /// Remove payload acceptance predicate.
    pub fn clear_can_accept(mut self) -> Self {
        self.can_accept = None;
        self
    }

    /// How to paint a compatible hover highlight (default: [`DropHighlight::None`]).
    pub fn highlight(mut self, highlight: DropHighlight) -> Self {
        self.highlight = highlight;
        self
    }

    /// Style used by [`DropHighlight::Fill`], [`DropHighlight::Placeholder`], and [`DropHighlight::Overlay`].
    pub fn highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed highlight style with the given style.
    pub fn extend_highlight_style(mut self, style: Style) -> Self {
        self.highlight_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit highlight style from the active theme.
    pub fn inherit_highlight_style(mut self) -> Self {
        self.highlight_style = StyleSlot::Inherit;
        self
    }

    /// Set the highlight style slot directly.
    pub fn highlight_style_slot(mut self, slot: StyleSlot) -> Self {
        self.highlight_style = slot;
        self
    }

    /// [`DropHighlight::Fill`] with the given style.
    pub fn highlight_fill(mut self, style: Style) -> Self {
        self.highlight = DropHighlight::Fill;
        self.highlight_style = StyleSlot::Replace(style);
        self
    }

    /// [`DropHighlight::Placeholder`] with the given style.
    pub fn highlight_placeholder(mut self, style: Style) -> Self {
        self.highlight = DropHighlight::Placeholder;
        self.highlight_style = StyleSlot::Replace(style);
        self
    }

    /// [`DropHighlight::Overlay`] with the given style (drawn after children).
    pub fn highlight_overlay(mut self, style: Style) -> Self {
        self.highlight = DropHighlight::Overlay;
        self.highlight_style = StyleSlot::Replace(style);
        self
    }

    /// What to render at this target while a compatible [`crate::prelude::DragPreview::SourceSnapshot`] drag hovers.
    pub fn drop_slot(mut self, slot: DropSlot) -> Self {
        self.drop_slot = slot;
        self
    }

    /// Render the dragged source's snapshot in-place; floating cursor preview is suppressed.
    /// Shorthand for `drop_slot(DropSlot::SourcePreview)`.
    pub fn drop_slot_source_preview(mut self) -> Self {
        self.drop_slot = DropSlot::SourcePreview;
        self
    }

    /// Enable or disable this drop target.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl From<DropTarget> for Element {
    fn from(value: DropTarget) -> Self {
        let (min_w, min_h) = measure_drop_target(&value, None, None);
        Element::new(ElementKind::DropTarget(value)).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}

impl LayoutHash for DropTarget {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        self.enabled.hash(hasher);
        self.on_drag_over.is_some().hash(hasher);
        self.on_drag_leave.is_some().hash(hasher);
        self.on_drop.is_some().hash(hasher);
        self.accept_group.hash(hasher);
        self.can_accept.is_some().hash(hasher);
        self.highlight.hash(hasher);
        self.highlight_style.hash(hasher);
        self.drop_slot.hash(hasher);
        if let Some(child) = self.child.as_ref() {
            recurse(child.as_ref())?.hash(hasher);
        }
        Some(())
    }
}
