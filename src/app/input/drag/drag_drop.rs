//! Shared generic drag-and-drop logic for `DragSource`/`DropTarget`, used by
//! both the terminal `AppRunner` and the headless `TestBackend`.

use std::sync::Arc;

use crate::app::interaction_state::{ActiveDrag, DragState, MouseTrackingState};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::{DragLeaveEvent, DragPayload, DragStartEvent, DragStartedEvent};

use super::DragDropDrag;

/// Walk up from `start` to the nearest enabled `DropTarget` ancestor.
pub(crate) fn nearest_ancestor_drop_target(tree: &NodeTree, start: NodeId) -> Option<NodeId> {
    let mut cur = Some(start);
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if let NodeKind::DropTarget(target) = &node.kind
            && target.enabled
        {
            return Some(id);
        }
        cur = node.parent;
    }
    None
}

/// Check group and payload compatibility between an active drag and a target.
pub(crate) fn drag_drop_target_is_compatible(
    tree: &NodeTree,
    target_id: NodeId,
    drag_group: Option<&Arc<str>>,
    payload: &dyn DragPayload,
) -> bool {
    if !tree.is_valid(target_id) {
        return false;
    }
    let NodeKind::DropTarget(target) = &tree.node(target_id).kind else {
        return false;
    };
    if !target.enabled {
        return false;
    }

    if let Some(target_group) = target.accept_group.as_ref()
        && drag_group != Some(target_group)
    {
        return false;
    }

    target
        .can_accept
        .as_ref()
        .is_none_or(|accept| accept(payload))
}

/// Toggle the `is_dragging` flag on a `DragSource` node.
pub(crate) fn set_drag_source_dragging(tree: &mut NodeTree, id: NodeId, dragging: bool) {
    if !tree.is_valid(id) {
        return;
    }
    if let NodeKind::DragSource(source) = &mut tree.node_mut(id).kind {
        source.is_dragging = dragging;
    }
}

/// Toggle the `dnd_highlighted` flag on a `DropTarget` node.
pub(crate) fn set_drop_target_highlighted(tree: &mut NodeTree, id: NodeId, highlighted: bool) {
    if !tree.is_valid(id) {
        return;
    }
    if let NodeKind::DropTarget(target) = &mut tree.node_mut(id).kind {
        target.dnd_highlighted = highlighted;
    }
}

/// Emit `on_drag_leave` on a `DropTarget` node, if set.
pub(crate) fn emit_drop_target_leave(tree: &NodeTree, id: NodeId, payload: &Arc<dyn DragPayload>) {
    if !tree.is_valid(id) {
        return;
    }
    let NodeKind::DropTarget(target) = &tree.node(id).kind else {
        return;
    };
    if let Some(cb) = &target.on_drag_leave {
        cb.emit(DragLeaveEvent {
            payload: payload.clone(),
        });
    }
}

/// Try to promote a pending `DragSource` grab into an active generic drag.
///
/// Returns `true` when the drag activated; the caller is expected to follow up
/// with its own drag-move handling for the same pointer position.
pub(crate) fn try_activate_drag_drop(
    tree: &mut NodeTree,
    mouse: &mut MouseTrackingState,
    drag: &mut DragState,
    x: u16,
    y: u16,
) -> bool {
    if !matches!(drag.active, ActiveDrag::None) {
        return false;
    }

    let Some(source_id) = mouse.pending_drag_source else {
        return false;
    };
    if !tree.is_valid(source_id) {
        mouse.pending_drag_source = None;
        return false;
    }

    let Some((start_x, start_y)) = mouse.left_down_pos else {
        mouse.pending_drag_source = None;
        return false;
    };

    let (threshold, on_drag_start, drag_group, preview, on_cancel, on_drag_started) = {
        let node = tree.node(source_id);
        let NodeKind::DragSource(source) = &node.kind else {
            mouse.pending_drag_source = None;
            return false;
        };
        if !source.enabled {
            mouse.pending_drag_source = None;
            return false;
        }
        (
            source.threshold,
            source.on_drag_start.clone(),
            source.drag_group.clone(),
            source.preview.clone(),
            source.on_drag_cancel.clone(),
            source.on_drag_started.clone(),
        )
    };

    let dx = x.abs_diff(start_x);
    let dy = y.abs_diff(start_y);
    if dx < threshold && dy < threshold {
        return false;
    }

    let start_event = DragStartEvent { x, y };
    let Some(payload) = on_drag_start
        .as_ref()
        .and_then(|callback| callback(start_event))
        .map(|payload| payload.into_arc())
    else {
        mouse.pending_drag_source = None;
        return false;
    };

    if let Some(cb) = on_drag_started {
        cb.emit(DragStartedEvent {
            x,
            y,
            payload: payload.clone(),
        });
    }

    set_drag_source_dragging(tree, source_id, true);
    let preview_snapshot_anchor = matches!(preview, crate::widgets::DragPreview::SourceSnapshot)
        .then(|| tree.node(source_id).rect);
    drag.active = ActiveDrag::DragDrop(DragDropDrag {
        payload,
        source_id,
        drag_group,
        preview,
        on_cancel,
        hovered_target: None,
        scroll_view_id: None,
        start_x,
        start_y,
        threshold,
        started: true,
        preview_snapshot_anchor,
    });
    mouse.pending_drag_source = None;
    true
}
