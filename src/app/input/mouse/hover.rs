use crate::app::interaction_state::{MouseRegionHoverState, MouseTrackingState};
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
                on_hover_change: region.on_hover_change.clone(),
                affects_paint: node.hover_affects_paint(),
            })
        })
        .collect()
}

fn contains_region(regions: &[MouseRegionHoverState], id: NodeId) -> bool {
    regions.iter().any(|region| region.id == id)
}

pub(crate) fn mouse_region_hover_chains_differ(
    previous: &[MouseRegionHoverState],
    current: &[MouseRegionHoverState],
) -> bool {
    previous
        .iter()
        .any(|region| !contains_region(current, region.id))
        || current
            .iter()
            .any(|region| !contains_region(previous, region.id))
}

fn emit_mouse_region_leaves(previous: &[MouseRegionHoverState], current: &[MouseRegionHoverState]) {
    for region in previous {
        if !contains_region(current, region.id)
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
        if !contains_region(previous, region.id)
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

pub(crate) fn clear_mouse_hover_state(state: &mut MouseTrackingState) -> bool {
    let previous_regions = std::mem::take(&mut state.mouse_region_hover_chain);
    emit_mouse_region_hover_changes(&previous_regions, &[]);
    state.hovered_item_index = None;
    state.hover_paint_target = None;
    state.hovered.take().is_some() || previous_regions.iter().any(|region| region.affects_paint)
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
