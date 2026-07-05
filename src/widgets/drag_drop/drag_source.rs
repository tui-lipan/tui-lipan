use std::hash::Hash;
use std::sync::Arc;

use crate::core::element::{Element, ElementKind};
use crate::layout::drag_source_layout_hint::drag_source_snapshot_collapse_key;
use crate::layout::hash::LayoutHash;
use crate::style::{LayoutConstraints, Length, Style, StyleSlot};

use super::drag_source_layout::measure_drag_source;
use super::payload::{
    DragCancelEvent, DragPayload, DragPreview, DragSlot, DragSlotAxis, DragStartEvent,
    DragStartedEvent,
};

/// Callback used to start a drag and produce its payload.
/// Handler used to start drag and produce payload.
pub type DragStartHandler = Arc<dyn Fn(DragStartEvent) -> Option<Box<dyn DragPayload>>>;

/// Wrapper widget that turns its child into a drag source.
#[derive(Clone)]
pub struct DragSource {
    pub(crate) child: Option<Box<Element>>,
    pub(crate) on_drag_start: Option<DragStartHandler>,
    pub(crate) on_drag_cancel: Option<crate::callback::Callback<DragCancelEvent>>,
    pub(crate) on_drag_started: Option<crate::callback::Callback<DragStartedEvent>>,
    pub(crate) drag_group: Option<Arc<str>>,
    pub(crate) preview: DragPreview,
    pub(crate) dragging_style: StyleSlot,
    pub(crate) drag_slot: DragSlot,
    pub(crate) drag_slot_axis: DragSlotAxis,
    pub(crate) preview_max_width: Option<u16>,
    pub(crate) preview_max_height: Option<u16>,
    pub(crate) threshold: u16,
    pub(crate) enabled: bool,
}

impl Default for DragSource {
    fn default() -> Self {
        Self {
            child: None,
            on_drag_start: None,
            on_drag_cancel: None,
            on_drag_started: None,
            drag_group: None,
            preview: DragPreview::None,
            dragging_style: StyleSlot::Inherit,
            drag_slot: DragSlot::Collapse,
            drag_slot_axis: DragSlotAxis::default(),
            preview_max_width: None,
            preview_max_height: None,
            threshold: 3,
            enabled: true,
        }
    }
}

impl DragSource {
    /// Create an empty drag source wrapper.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the wrapped child element.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = Some(Box::new(child.into()));
        self
    }

    /// Set drag-start callback that returns the payload for this drag.
    pub fn on_drag_start(
        mut self,
        cb: impl Fn(DragStartEvent) -> Option<Box<dyn DragPayload>> + 'static,
    ) -> Self {
        self.on_drag_start = Some(Arc::new(cb));
        self
    }

    /// Set cancellation callback fired when drop fails or is canceled.
    pub fn on_drag_cancel(mut self, cb: crate::callback::Callback<DragCancelEvent>) -> Self {
        self.on_drag_cancel = Some(cb);
        self
    }

    /// Callback fired once when the drag activates (after the movement threshold).
    pub fn on_drag_started(mut self, cb: crate::callback::Callback<DragStartedEvent>) -> Self {
        self.on_drag_started = Some(cb);
        self
    }

    /// Restrict this source to a compatibility group.
    pub fn drag_group(mut self, group: impl Into<Arc<str>>) -> Self {
        self.drag_group = Some(group.into());
        self
    }

    /// Remove any compatibility group restriction.
    pub fn clear_drag_group(mut self) -> Self {
        self.drag_group = None;
        self
    }

    /// Configure drag preview behavior.
    pub fn preview(mut self, preview: DragPreview) -> Self {
        self.preview = preview;
        self
    }

    /// Configure a simple text preview label.
    pub fn preview_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.preview = DragPreview::Label(label.into());
        self
    }

    /// Render a snapshot of the drag source's cells near the cursor as a drag preview.
    pub fn preview_snapshot(mut self) -> Self {
        self.preview = DragPreview::SourceSnapshot;
        self
    }

    /// Disable preview rendering.
    pub fn no_preview(mut self) -> Self {
        self.preview = DragPreview::None;
        self
    }

    /// Style overlay while this source is actively dragging (tint, reserved slot, label preview).
    pub fn dragging_style(mut self, style: Style) -> Self {
        self.dragging_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed dragging style with the given style.
    pub fn extend_dragging_style(mut self, style: Style) -> Self {
        self.dragging_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit dragging style from the active theme.
    pub fn inherit_dragging_style(mut self) -> Self {
        self.dragging_style = StyleSlot::Inherit;
        self
    }

    /// Set the dragging style slot directly.
    pub fn dragging_style_slot(mut self, slot: StyleSlot) -> Self {
        self.dragging_style = slot;
        self
    }

    /// Main-axis space reserved at the source while dragging with [`DragPreview::SourceSnapshot`].
    pub fn drag_slot(mut self, slot: DragSlot) -> Self {
        self.drag_slot = slot;
        self
    }

    /// Shorthand for `drag_slot(DragSlot::Collapse)`.
    pub fn drag_slot_collapse(mut self) -> Self {
        self.drag_slot = DragSlot::Collapse;
        self
    }

    /// Same as `drag_slot(DragSlot::Specified(len))`.
    pub fn drag_slot_length(mut self, len: Length) -> Self {
        self.drag_slot = DragSlot::Specified(len);
        self
    }

    /// Which axis [`DragSlot`] sizes apply on when not inside a `VStack` / `HStack`.
    pub fn drag_slot_axis(mut self, axis: DragSlotAxis) -> Self {
        self.drag_slot_axis = axis;
        self
    }

    /// `None` uses [`crate::DEFAULT_PREVIEW_MAX_WIDTH`].
    pub fn preview_max_width(mut self, max_width: Option<u16>) -> Self {
        self.preview_max_width = max_width;
        self
    }

    /// `None` uses [`crate::DEFAULT_PREVIEW_MAX_HEIGHT`].
    pub fn preview_max_height(mut self, max_height: Option<u16>) -> Self {
        self.preview_max_height = max_height;
        self
    }

    /// Set both floating preview max dimensions (`None` per axis uses the framework default).
    pub fn preview_max_size(mut self, max_width: Option<u16>, max_height: Option<u16>) -> Self {
        self.preview_max_width = max_width;
        self.preview_max_height = max_height;
        self
    }

    /// Set pointer movement threshold (cells) before drag starts.
    pub fn threshold(mut self, threshold: u16) -> Self {
        self.threshold = threshold;
        self
    }

    /// Enable or disable this drag source.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

impl From<DragSource> for Element {
    fn from(value: DragSource) -> Self {
        let (min_w, min_h) = measure_drag_source(&value, None, None, None);
        // SourceSnapshot sources may collapse to zero height during a drag,
        // so their min_height must be 0 to allow that.
        let min_h = if matches!(value.preview, DragPreview::SourceSnapshot) {
            0
        } else {
            min_h
        };
        Element::new(ElementKind::DragSource(value)).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}

impl LayoutHash for DragSource {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        self.enabled.hash(hasher);
        self.on_drag_start.is_some().hash(hasher);
        self.on_drag_cancel.is_some().hash(hasher);
        self.on_drag_started.is_some().hash(hasher);
        self.drag_group.hash(hasher);
        self.preview.hash(hasher);
        self.dragging_style.hash(hasher);
        self.drag_slot.hash(hasher);
        self.drag_slot_axis.hash(hasher);
        self.preview_max_width.hash(hasher);
        self.preview_max_height.hash(hasher);
        self.threshold.hash(hasher);
        // Include the collapse hint so the global measure cache invalidates
        // when a SourceSnapshot drag starts or ends.
        if matches!(self.preview, DragPreview::SourceSnapshot) {
            drag_source_snapshot_collapse_key().hash(hasher);
        }
        if let Some(child) = self.child.as_ref() {
            recurse(child.as_ref())?.hash(hasher);
        }
        Some(())
    }
}
