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
