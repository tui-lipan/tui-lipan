use std::any::Any;
use std::fmt::Debug;
use std::sync::Arc;

use crate::style::Length;

/// Type-erased payload for generic drag-and-drop.
pub trait DragPayload: Any + Debug + 'static {
    /// Downcast helper for payload inspection.
    fn as_any(&self) -> &dyn Any;

    /// Convert boxed payload into shared payload storage without double boxing.
    fn into_arc(self: Box<Self>) -> Arc<dyn DragPayload>;
}

impl<T> DragPayload for T
where
    T: Any + Debug + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn into_arc(self: Box<Self>) -> Arc<dyn DragPayload> {
        let payload: Arc<Self> = Arc::from(self);
        payload
    }
}

impl dyn DragPayload {
    /// Downcast payload to a concrete type.
    pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
        self.as_any().downcast_ref::<T>().or_else(|| {
            self.as_any()
                .downcast_ref::<Box<dyn DragPayload>>()
                .and_then(|boxed| boxed.as_ref().as_any().downcast_ref::<T>())
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Event emitted when drag activation threshold is exceeded.
pub struct DragStartEvent {
    /// Pointer x coordinate when drag starts.
    pub x: u16,
    /// Pointer y coordinate when drag starts.
    pub y: u16,
}

#[derive(Clone)]
/// Event emitted while a compatible payload hovers a drop target.
pub struct DragOverEvent {
    /// Current pointer x coordinate.
    pub x: u16,
    /// Current pointer y coordinate.
    pub y: u16,
    /// Pointer `y` minus the hovered drop target's top edge (content coordinates).
    pub local_y: u16,
    /// Height of the hovered drop target in cells.
    pub local_height: u16,
    /// Active drag payload.
    pub payload: Arc<dyn DragPayload>,
}

#[derive(Clone)]
/// Event emitted when payload leaves a drop target.
pub struct DragLeaveEvent {
    /// Active drag payload.
    pub payload: Arc<dyn DragPayload>,
}

#[derive(Clone)]
/// Event emitted when payload is dropped on a compatible target.
pub struct DropEvent {
    /// Pointer x coordinate at drop time.
    pub x: u16,
    /// Pointer y coordinate at drop time.
    pub y: u16,
    /// Pointer `y` minus the drop target's top edge (content coordinates).
    pub local_y: u16,
    /// Height of the drop target in cells.
    pub local_height: u16,
    /// Active drag payload.
    pub payload: Arc<dyn DragPayload>,
}

#[derive(Clone)]
/// Fired once when a generic drag becomes active (after the movement threshold).
pub struct DragStartedEvent {
    /// Pointer x coordinate when the drag activated.
    pub x: u16,
    /// Pointer y coordinate when the drag activated.
    pub y: u16,
    /// Active drag payload.
    pub payload: Arc<dyn DragPayload>,
}

#[derive(Clone)]
/// Event emitted when active drag is canceled.
pub struct DragCancelEvent {
    /// Active drag payload.
    pub payload: Arc<dyn DragPayload>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
/// Visual preview style rendered near pointer during drag.
pub enum DragPreview {
    /// Show this label near the pointer while dragging.
    Label(Arc<str>),
    /// Render a snapshot of the drag source's cells near the pointer while dragging.
    SourceSnapshot,
    /// Do not render a drag preview.
    #[default]
    None,
}

/// Default maximum width (cells) for a [`DragPreview::SourceSnapshot`] float preview.
pub const DEFAULT_PREVIEW_MAX_WIDTH: u16 = 60;
/// Default maximum height (cells) for a [`DragPreview::SourceSnapshot`] float preview.
pub const DEFAULT_PREVIEW_MAX_HEIGHT: u16 = 20;

/// Which axis [`DragSlot`] main-axis sizes apply to when measuring a drag source outside a stack
/// (e.g. inside `Frame`). In `VStack` / `HStack`, the stack's axis always wins.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DragSlotAxis {
    /// Vertical main axis (height) when measuring outside a stack.
    #[default]
    Vertical,
    /// Horizontal main axis (width) when measuring outside a stack.
    Horizontal,
}

/// Main-axis space reserved at the [`crate::widgets::DragSource`] while dragging (when using
/// [`DragPreview::SourceSnapshot`]).
///
/// In `VStack` / `HStack`, [`DragSlot::Specified`] uses the same main-axis rules as stack children.
/// [`DragSlot::Collapse`] is **0 cells** on the stack main axis.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DragSlot {
    /// Zero cells on the stack main axis while dragging.
    #[default]
    Collapse,
    /// Fixed main-axis length using the same rules as stack children.
    Specified(Length),
}

/// What the [`crate::widgets::DropTarget`] renders in place of its child while a compatible
/// [`DragPreview::SourceSnapshot`] drag is hovering over it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DropSlot {
    /// Render the child normally (default). [`crate::widgets::DropHighlight`] still applies on top.
    #[default]
    Child,
    /// Replace the child with the dragged source's snapshot cells and suppress the floating
    /// cursor preview. The snapshot is rendered top-left aligned and clipped to the target rect.
    /// [`crate::widgets::DropHighlight`] is still composited on top when set.
    SourcePreview,
}
