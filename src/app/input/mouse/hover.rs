use crate::app::interaction_state::{
    DocumentHintHoverState, ListRowHoverState, MouseRegionHoverState, MouseTrackingState,
};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::Rect;

pub(crate) fn mouse_region_accepts_point(
    region: &crate::widgets::internal::MouseRegionNode,
    rect: Rect,
    x: u16,
    y: u16,
) -> bool {
    if !region.enabled || !rect.contains(x as i16, y as i16) {
        return false;
    }
    region.hit_test.as_ref().is_none_or(|test| {
        let local_x = (x as i16).saturating_sub(rect.x) as u16;
        let local_y = (y as i16).saturating_sub(rect.y) as u16;
        test(local_x, local_y)
    })
}

/// Enabled, hoverable `MouseRegion`s containing the current hover target.
///
/// The deepest region comes first. Ancestors remain hovered while the pointer
/// moves between interactive descendants inside the same wrapper.
pub(crate) fn mouse_region_hover_chain(
    tree: &NodeTree,
    hovered: Option<NodeId>,
    x: u16,
    y: u16,
) -> Vec<NodeId> {
    let mut regions = Vec::new();
    let mut current = hovered;
    while let Some(id) = current {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if let NodeKind::MouseRegion(region) = &node.kind
            && node.is_hoverable()
            && mouse_region_accepts_point(region, node.rect, x, y)
        {
            regions.push(id);
        }
        current = node.parent;
    }
    regions
}

pub(crate) fn mouse_region_hover_state(
    tree: &NodeTree,
    hovered: Option<NodeId>,
    x: u16,
    y: u16,
) -> Vec<MouseRegionHoverState> {
    mouse_region_hover_chain(tree, hovered, x, y)
        .into_iter()
        .filter_map(|id| {
            let node = tree.node(id);
            let NodeKind::MouseRegion(region) = &node.kind else {
                return None;
            };
            Some(MouseRegionHoverState {
                id,
                identity: region.hover_identity,
                on_hover_change: region.on_hover_change.clone(),
                affects_paint: node.hover_affects_paint(),
            })
        })
        .collect()
}

fn contains_region(regions: &[MouseRegionHoverState], target: &MouseRegionHoverState) -> bool {
    regions
        .iter()
        .any(|region| region.id == target.id && region.identity == target.identity)
}

pub(crate) fn mouse_region_hover_chains_differ(
    previous: &[MouseRegionHoverState],
    current: &[MouseRegionHoverState],
) -> bool {
    previous
        .iter()
        .any(|region| !contains_region(current, region))
        || current
            .iter()
            .any(|region| !contains_region(previous, region))
}

fn emit_mouse_region_leaves(previous: &[MouseRegionHoverState], current: &[MouseRegionHoverState]) {
    for region in previous {
        if !contains_region(current, region)
            && let Some(callback) = &region.on_hover_change
        {
            callback.emit(false);
        }
    }
}

fn emit_mouse_region_entries(
    previous: &[MouseRegionHoverState],
    current: &[MouseRegionHoverState],
) {
    for region in current.iter().rev() {
        if !contains_region(previous, region)
            && let Some(callback) = &region.on_hover_change
        {
            callback.emit(true);
        }
    }
}

pub(crate) fn emit_mouse_region_hover_changes(
    previous: &[MouseRegionHoverState],
    current: &[MouseRegionHoverState],
) {
    emit_mouse_region_leaves(previous, current);
    emit_mouse_region_entries(previous, current);
}

/// The row under the pointer when the hovered node is a list that reports row hover.
pub(crate) fn list_row_hover_state(
    tree: &NodeTree,
    hovered: Option<NodeId>,
    x: u16,
    y: u16,
) -> Option<ListRowHoverState> {
    let id = hovered.filter(|id| tree.is_valid(*id))?;
    let node = tree.node(id);
    let NodeKind::List(list) = &node.kind else {
        return None;
    };
    let on_hover = list.on_hover.clone()?;
    let index = list.row_at(x as i16, y as i16, list.content_rect(node.rect, None))?;
    Some(ListRowHoverState {
        id,
        index,
        on_hover,
    })
}

/// Record the hovered list row, reporting the old row leaving before the new one entering.
pub(crate) fn set_list_row_hover(state: &mut MouseTrackingState, next: Option<ListRowHoverState>) {
    let unchanged = match (&state.list_row_hover, &next) {
        (Some(previous), Some(next)) => previous.id == next.id && previous.index == next.index,
        (previous, next) => previous.is_none() && next.is_none(),
    };
    if !unchanged {
        if let Some(previous) = &state.list_row_hover {
            previous.on_hover.emit(crate::widgets::ListHoverEvent {
                index: previous.index,
                hovered: false,
            });
        }
        if let Some(next) = &next {
            next.on_hover.emit(crate::widgets::ListHoverEvent {
                index: next.index,
                hovered: true,
            });
        }
    }
    // Keep the newest callback, so a leave reaches the handler the view last supplied.
    state.list_row_hover = next;
}

/// The hint under the pointer when the hovered node is a document view that shows tooltips.
pub(crate) fn document_hint_hover_state(
    tree: &NodeTree,
    hovered: Option<NodeId>,
    x: u16,
    y: u16,
) -> Option<DocumentHintHoverState> {
    let id = hovered.filter(|id| tree.is_valid(*id))?;
    let node = tree.node(id);
    let NodeKind::DocumentView(doc) = &node.kind else {
        return None;
    };
    let on_hover_hint = doc.on_hover_hint.clone()?;
    let hint = doc.hover_hint_at(node.rect, x as i16, y as i16)?;
    Some(DocumentHintHoverState {
        id,
        hint,
        on_hover_hint,
    })
}

/// Record the document hint under the pointer, telling a document the pointer left that its hint
/// is gone before telling the new one about its hint.
pub(crate) fn set_document_hint_hover(
    state: &mut MouseTrackingState,
    next: Option<DocumentHintHoverState>,
) {
    let previous = state.document_hint_hover.take();
    match (&previous, &next) {
        (Some(previous), Some(next)) if previous.id == next.id => {
            if previous.hint != next.hint {
                next.on_hover_hint.emit(Some(next.hint.clone()));
            }
        }
        _ => {
            if let Some(previous) = &previous {
                previous.on_hover_hint.emit(None);
            }
            if let Some(next) = &next {
                next.on_hover_hint.emit(Some(next.hint.clone()));
            }
        }
    }
    // Keep the newest callback, so a leave reaches the handler the view last supplied.
    state.document_hint_hover = next;
}

pub(crate) fn clear_mouse_hover_state(state: &mut MouseTrackingState) -> bool {
    set_list_row_hover(state, None);
    set_document_hint_hover(state, None);
    let previous_regions = std::mem::take(&mut state.mouse_region_hover_chain);
    emit_mouse_region_hover_changes(&previous_regions, &[]);
    state.hovered_item_index = None;
    state.hover_paint_target = None;
    state.hovered.take().is_some() || previous_regions.iter().any(|region| region.affects_paint)
}

/// Drop hover as if the pointer left the app surface.
///
/// Hosts do not always send a motion event for that: `FocusLost` is the usual
/// signal, and inline viewports treat an out-of-viewport report the same way.
pub(crate) fn release_pointer_hover(
    state: &mut MouseTrackingState,
    tree: &mut NodeTree,
    overlays: &mut crate::overlay::OverlayManager,
) -> bool {
    let toast_dirty = overlays.set_hovered_toast(None);
    #[cfg(feature = "terminal")]
    let link_dirty = {
        let previous = state.terminal_link_hover_node.take();
        if let Some(id) = previous.filter(|id| tree.is_valid(*id))
            && let NodeKind::Terminal(term) = &mut tree.node_mut(id).kind
        {
            term.link_hover.take().is_some()
        } else {
            false
        }
    };
    #[cfg(not(feature = "terminal"))]
    let link_dirty = false;
    let width_lock_cleared = if let Some(id) = state.hovered.filter(|id| tree.is_valid(*id))
        && let NodeKind::DraggableTabBar(tab_bar) = &mut tree.node_mut(id).kind
    {
        tab_bar.width_lock.take().is_some()
    } else {
        false
    };
    state.last_mouse.set(None);
    state.sub_cell.set(None);
    clear_mouse_hover_state(state) || toast_dirty || link_dirty || width_lock_cleared
}

pub(crate) fn mouse_region_hover_transition_affects_paint(
    previous: &[MouseRegionHoverState],
    current: &[MouseRegionHoverState],
) -> bool {
    previous.iter().any(|region| {
        region.affects_paint
            && !current
                .iter()
                .any(|candidate| candidate.id == region.id && candidate.affects_paint)
    }) || current.iter().any(|region| {
        region.affects_paint
            && !previous
                .iter()
                .any(|candidate| candidate.id == region.id && candidate.affects_paint)
    })
}

/// Check if a node should be hovered (for visual feedback).
pub(crate) fn should_hover(tree: &NodeTree, id: NodeId, x: u16, y: u16) -> bool {
    let node = tree.node(id);

    // Special case for Slider: only hover when over the track, not label/value
    if let NodeKind::Slider(slider) = &node.kind {
        if slider.disabled {
            return false;
        }
        if let Some(track) = crate::app::input::geometry::slider_track_geometry(slider, node.rect) {
            return (y as i16) == track.track_y
                && (x as i16) >= track.track_x
                && (x as i16) < track.track_x.saturating_add(track.track_w as i16);
        } else {
            return false;
        }
    }

    // Special case for Splitter: only hover when over a handle
    if let NodeKind::Splitter(splitter) = &node.kind {
        return splitter.handle_at(x as i16, y as i16).is_some();
    }

    // DraggableTabBar is effectively one row of interaction.
    if let NodeKind::DraggableTabBar(tab_bar) = &node.kind {
        let inner = node.rect.inner(tab_bar.border, tab_bar.padding);
        return inner.w > 0
            && inner.h > 0
            && (y as i16) == inner.y
            && (x as i16) >= inner.x
            && (x as i16) < inner.x.saturating_add(inner.w as i16);
    }

    node.is_hoverable()
}
