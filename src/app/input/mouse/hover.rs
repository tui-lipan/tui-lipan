use crate::core::node::{NodeId, NodeKind, NodeTree};

/// Check if a node should be hovered (for visual feedback).
pub(crate) fn should_hover(tree: &NodeTree, id: NodeId, x: u16, y: u16) -> bool {
    let node = tree.node(id);

    // Special case for Slider: only hover when over the track, not label/value
    if let NodeKind::Slider(slider) = &node.kind {
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
