//! Drag handling for sliders, progress bars, text areas, and inputs.

use std::sync::Arc;
use web_time::Instant;

use crate::callback::Callback;
use crate::core::element::Key;
use crate::core::node::NodeId;
use crate::style::Rect;
use crate::widgets::{
    DragCancelEvent, DragPayload, DragPreview, DragReorderMode, DraggableTabReorderEvent,
    DraggableTabTransferEvent,
};

/// Progress bar drag state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ProgressDrag {
    pub id: NodeId,
}

/// Slider drag state.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SliderDrag {
    pub id: NodeId,
}

/// Draggable tab bar drag state.
#[derive(Clone)]
pub(crate) struct DraggableTabBarDrag {
    pub source_id: NodeId,
    pub source_bar_id: Option<Arc<str>>,
    pub source_index: usize,
    pub id: NodeId,
    pub bar_id: Option<Arc<str>>,
    pub current_index: usize,
    pub pending_id: NodeId,
    pub pending_bar_id: Option<Arc<str>>,
    pub pending_index: usize,
    pub drag_group: Option<Arc<str>>,
    pub on_transfer: Option<Callback<DraggableTabTransferEvent>>,
    pub reorder_mode: DragReorderMode,
    pub threshold: u16,
    pub start_x: u16,
    pub started: bool,
    pub preview_label: Option<Arc<str>>,
    /// Rect of the source tab in buffer space; set once on drag activation for snapshot preview.
    pub preview_snapshot_anchor: Option<crate::style::Rect>,
}

impl std::fmt::Debug for DraggableTabBarDrag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DraggableTabBarDrag")
            .field("source_id", &self.source_id)
            .field("source_bar_id", &self.source_bar_id)
            .field("source_index", &self.source_index)
            .field("id", &self.id)
            .field("bar_id", &self.bar_id)
            .field("current_index", &self.current_index)
            .field("pending_id", &self.pending_id)
            .field("pending_bar_id", &self.pending_bar_id)
            .field("pending_index", &self.pending_index)
            .field("drag_group", &self.drag_group)
            .field("reorder_mode", &self.reorder_mode)
            .field("threshold", &self.threshold)
            .field("start_x", &self.start_x)
            .field("started", &self.started)
            .field("preview_label", &self.preview_label)
            .finish()
    }
}

/// Generic drag-and-drop state for `DragSource`/`DropTarget`.
#[derive(Clone)]
pub(crate) struct DragDropDrag {
    pub payload: Arc<dyn DragPayload>,
    pub source_id: NodeId,
    pub drag_group: Option<Arc<str>>,
    pub preview: DragPreview,
    pub on_cancel: Option<Callback<DragCancelEvent>>,
    pub hovered_target: Option<NodeId>,
    /// Nearest `ScrollView` ancestor of the node under the pointer, updated
    /// on every move event so stationary-autoscroll can tick without a re-hit-test.
    pub scroll_view_id: Option<NodeId>,
    pub start_x: u16,
    pub start_y: u16,
    pub threshold: u16,
    pub started: bool,
    /// Last laid-out `DragSource` rect before this drag (used to collapse layout on the
    /// first frame while seeding `DragPreview::SourceSnapshot` pixels from the previous frame).
    pub preview_snapshot_anchor: Option<crate::style::Rect>,
}

impl std::fmt::Debug for DragDropDrag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DragDropDrag")
            .field("payload", &self.payload)
            .field("source_id", &self.source_id)
            .field("drag_group", &self.drag_group)
            .field("preview", &self.preview)
            .field("hovered_target", &self.hovered_target)
            .field("scroll_view_id", &self.scroll_view_id)
            .field("start_x", &self.start_x)
            .field("start_y", &self.start_y)
            .field("threshold", &self.threshold)
            .field("started", &self.started)
            .field("preview_snapshot_anchor", &self.preview_snapshot_anchor)
            .finish()
    }
}

pub(crate) enum DraggableTabDragEvent {
    Reorder(DraggableTabReorderEvent),
    Transfer(DraggableTabTransferEvent),
}

/// Splitter drag state.
#[derive(Clone, Debug)]
pub(crate) struct SplitterDrag {
    pub id: NodeId,
    pub handle: usize,
    pub start_pos: i16,
    pub start_sizes: Vec<u16>,
    /// Perpendicular splitter grabbed at a handle junction (corner drag).
    ///
    /// When the click lands where a vertical and a horizontal handle meet,
    /// both splitters resize together: the primary follows its own axis and
    /// the secondary follows the perpendicular one.
    pub secondary: Option<SplitterDragTarget>,
}

/// A second splitter participating in a corner (junction) drag.
#[derive(Clone, Debug)]
pub(crate) struct SplitterDragTarget {
    pub id: NodeId,
    pub handle: usize,
    pub start_pos: i16,
    pub start_sizes: Vec<u16>,
}

/// Find a perpendicular splitter whose handle touches `(x, y)`, i.e. the
/// click landed on a junction where a vertical and a horizontal handle meet,
/// so the drag should resize both splitters at once.
///
/// The perpendicular handle rect is expanded by one cell because the junction
/// cell itself belongs to only one of the two handles; the other handle ends
/// directly beside it.
pub(crate) fn find_junction_splitter(
    tree: &crate::core::node::NodeTree,
    primary: NodeId,
    primary_orientation: crate::widgets::Orientation,
    x: u16,
    y: u16,
) -> Option<SplitterDragTarget> {
    use crate::core::node::NodeKind;

    let (xi, yi) = (x as i16, y as i16);
    let (xi32, yi32) = (i32::from(xi), i32::from(yi));
    for node in tree.iter() {
        if node.id == primary {
            continue;
        }
        let NodeKind::Splitter(splitter) = &node.kind else {
            continue;
        };
        if splitter.orientation == primary_orientation {
            continue;
        }
        // `Rect::x`/`y` (i16) and `w`/`h` (u16) both fit comfortably in i32, so this bounds
        // math can never overflow, unlike a `w as i16`/`h as i16` cast which can wrap for
        // widths/heights above `i16::MAX`. Mirrors `Rect::contains`'s i32 approach.
        let Some(handle) = splitter.handle_rects.iter().position(|rect| {
            let x_lo = i32::from(rect.x) - 1;
            let x_hi = i32::from(rect.x) + i32::from(rect.w);
            let y_lo = i32::from(rect.y) - 1;
            let y_hi = i32::from(rect.y) + i32::from(rect.h);
            xi32 >= x_lo && xi32 <= x_hi && yi32 >= y_lo && yi32 <= y_hi
        }) else {
            continue;
        };
        let start_pos = match splitter.orientation {
            crate::widgets::Orientation::Vertical => xi,
            crate::widgets::Orientation::Horizontal => yi,
        };
        return Some(SplitterDragTarget {
            id: node.id,
            handle,
            start_pos,
            start_sizes: splitter.pane_sizes.clone(),
        });
    }
    None
}

/// Tracks active mouse drag selection for TextArea.
#[derive(Clone, Copy, Debug)]
pub(crate) struct TextAreaDrag {
    /// The node being dragged.
    pub id: NodeId,
    /// The anchor position (where the drag started).
    pub anchor: usize,
}

/// Tracks active mouse drag selection for Input (single-line).
#[derive(Clone, Copy, Debug)]
pub(crate) struct InputDrag {
    /// The node being dragged.
    pub id: NodeId,
    /// The anchor position (where the drag started).
    pub anchor: usize,
}

/// Tracks active mouse drag selection for HexArea.
#[derive(Clone, Copy, Debug)]
pub(crate) struct HexAreaDrag {
    /// The node being dragged.
    pub id: NodeId,
    /// The anchor byte index (where the drag started).
    pub anchor: usize,
}

/// Tracks active mouse drag selection for DocumentView.
#[derive(Clone, Debug)]
pub(crate) struct DocumentViewDrag {
    /// The node being dragged.
    pub id: NodeId,
    /// Drag anchor for linear or table-rect selection.
    pub anchor: DocumentViewDragAnchor,
    /// Shared selection ID (if any) for recovering after node sweep.
    pub shared_selection_id: Option<Arc<str>>,
    /// Ancestor scroll view, for recovering after node sweep.
    pub scroll_view_id: Option<NodeId>,
    /// Stable anchor for shared linear drags across virtual scroll (child key + doc slot).
    pub shared_drag_anchor: Option<SharedDocumentDragAnchor>,
}

/// Identifies which `DocumentView` started the drag across virtual scroll.
///
/// `virtual_child_index` matches `ScrollView`'s virtual child ordering (or `children` index
/// when the virtual cache is empty). `doc_slot` is the depth-first `DocumentView` index
/// within that scroll child subtree.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct SharedDocumentDragAnchor {
    pub virtual_child_index: usize,
    pub doc_slot: usize,
    pub local_byte: usize,
}

/// Drag anchor for `DocumentView` selection.
#[derive(Clone, Copy, Debug)]
pub(crate) enum DocumentViewDragAnchor {
    /// Linear byte-offset selection anchor in `flat_text`.
    Linear(usize),
    /// Rectangular table-cell selection anchor.
    TableCell {
        table_id: usize,
        row_index: usize,
        col_index: usize,
        row_line_index: usize,
        cell_line_anchor_byte: usize,
    },
}

/// Table-cell hit information at a pointer position.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DocumentTableCellHit {
    pub table_id: usize,
    pub row_index: usize,
    pub col_index: usize,
    pub row_line_index: usize,
    pub cell_line_anchor_byte: usize,
    pub cell_text_start_byte: usize,
    pub cell_text_end_byte: usize,
    /// True when the X coordinate was clamped to fit inside the table.
    pub x_clamped: bool,
    /// True when the Y coordinate was clamped to fit inside the table.
    pub y_clamped: bool,
}

/// Per-DocumentView selection update derived from a shared drag.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DocumentViewSelectionUpdate {
    pub id: NodeId,
    pub cursor: usize,
    pub anchor: Option<usize>,
}

/// Result of a cross-DocumentView linear drag.
#[derive(Clone, Debug)]
pub(crate) struct DocumentViewSharedLinearDragResult {
    pub updates: Vec<DocumentViewSelectionUpdate>,
    pub offscreen_patches: Vec<OffscreenSharedSelectionPatch>,
    pub selected_text: Arc<str>,
}

#[derive(Clone, Debug)]
pub(crate) struct OffscreenSharedSelectionPatch {
    pub child_key: Key,
    pub doc_slot: usize,
    pub selection_cursor: usize,
    pub selection_anchor: Option<usize>,
}

/// Shared selection text across sibling `DocumentView`s in one `ScrollView`.
#[derive(Clone, Debug)]
pub(crate) struct DocumentViewSharedSelectionText {
    pub scroll_view_id: NodeId,
    pub shared_selection_id: Arc<str>,
    pub selected_text: Arc<str>,
}

#[cfg(feature = "diff-view")]
#[derive(Clone, Debug)]
pub(crate) struct DiffSplitSelectionText {
    pub left_id: NodeId,
    pub right_id: NodeId,
    pub selected_text: Arc<str>,
}

#[cfg(feature = "diff-view")]
#[derive(Clone, Debug)]
pub(crate) struct DiffSplitSelectionResult {
    pub updates: Vec<DocumentViewSelectionUpdate>,
}

#[cfg(feature = "diff-view")]
#[derive(Clone, Copy, Debug)]
pub(super) struct DiffSplitDocumentPair {
    left: NodeId,
    right: NodeId,
}

#[derive(Clone, Debug)]
pub(crate) struct DocumentViewSharedLinearItem {
    node_id: Option<NodeId>,
    virtual_child_index: usize,
    child_key: Key,
    doc_slot: usize,
    global_start: usize,
    global_end: usize,
    text_len: usize,
    rect: Rect,
    phantom_flat_text: Option<Arc<str>>,
}

/// Tracks active mouse drag selection for Terminal.
#[cfg(feature = "terminal")]
#[derive(Clone, Copy, Debug)]
pub(crate) struct TerminalDrag {
    /// The node being dragged.
    pub id: NodeId,
    /// The anchor position (where the drag started).
    pub anchor: crate::utils::GridPos,
}

/// Tracks click state for double/triple-click detection.
#[derive(Clone, Copy)]
pub(crate) struct ClickState {
    /// Position of the last click.
    pub x: u16,
    pub y: u16,
    /// Time of the last click.
    pub time: Instant,
    /// Number of consecutive clicks (1 = single, 2 = double, 3 = triple).
    pub count: u8,
}

mod diff;
mod document;
mod scroll_view;
mod table;
mod widgets;

pub(crate) use document::*;
pub(crate) use scroll_view::*;
pub(crate) use table::*;
pub(crate) use widgets::*;

#[cfg(feature = "diff-view")]
pub(crate) use diff::*;

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests;
