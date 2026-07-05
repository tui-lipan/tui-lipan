//! Mouse scroll-wheel dispatch.
//!
//! The per-widget scroll handlers live in [`crate::app::input::handlers`]; this
//! module provides the thin dispatcher that hit-tests, classifies the target
//! node, and delegates to the appropriate handler while bubbling up the tree.

use crate::app::input::handlers::{self, ScrollableTag};
use crate::core::event::MouseEvent;
use crate::core::node::{NodeKind, NodeTree};
use crate::widgets::internal::scroll_action_from_mouse_n;

/// Handle scroll wheel events with a coalesced raw wheel-tick count.
pub(crate) fn handle_scroll_wheel_n(
    tree: &mut NodeTree,
    event: MouseEvent,
    scroll_ticks: usize,
    fallback_multiplier: u16,
) -> bool {
    let Some(hit) = tree.hit_test(event.x as i16, event.y as i16) else {
        return false;
    };

    let mut cur = Some(hit);
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            break;
        }

        // Classify without holding a mutable borrow.
        let tag = handlers::classify_scrollable(&tree.node(id).kind);
        let parent = tree.node(id).parent;
        let scroll_lines = effective_scroll_lines(
            &tree.node(id).kind,
            scroll_ticks,
            fallback_multiplier,
            event.mods.shift,
        );
        let Some(action) = scroll_action_from_mouse_n(event, scroll_lines) else {
            return false;
        };

        // For TextArea scroll, whether the parent frame exposes a vertical integrated edge.
        let parent_integrated_v_edge = if tag == ScrollableTag::TextArea {
            tree.parent_frame_integrated_v_edge(id).unwrap_or(false)
        } else {
            false
        };

        let handled = match tag {
            ScrollableTag::List => handlers::list_table::handle_list_scroll(tree, id, action),
            ScrollableTag::Table => handlers::list_table::handle_table_scroll(tree, id, action),
            ScrollableTag::ScrollView => handlers::scroll_view::handle_scroll(tree, id, action),
            ScrollableTag::TextArea => {
                handlers::text_area::handle_scroll(tree, id, action, parent_integrated_v_edge)
            }
            ScrollableTag::HexArea => handlers::hex_area::handle_scroll(tree, id, action),
            #[cfg(feature = "terminal")]
            ScrollableTag::Terminal => handlers::terminal::handle_scroll(tree, id, action),
            ScrollableTag::DraggableTabBar => {
                handlers::tabs::handle_tab_bar_scroll(tree, id, action)
            }
            ScrollableTag::DocumentView => handlers::document_view::handle_scroll(tree, id, action),
            ScrollableTag::NonScrollable => false,
        };

        if handled {
            return true;
        }

        cur = parent;
    }

    false
}

fn effective_scroll_lines(
    kind: &NodeKind,
    scroll_ticks: usize,
    fallback_multiplier: u16,
    horizontal: bool,
) -> usize {
    let multiplier = match kind {
        NodeKind::ScrollView(node) if horizontal => node
            .h_scroll_wheel_multiplier
            .or(node.scroll_wheel_multiplier)
            .unwrap_or(fallback_multiplier),
        NodeKind::ScrollView(node) => node.scroll_wheel_multiplier.unwrap_or(fallback_multiplier),
        NodeKind::TextArea(node) => node.scroll_wheel_multiplier.unwrap_or(fallback_multiplier),
        NodeKind::DocumentView(node) => node.scroll_wheel_multiplier.unwrap_or(fallback_multiplier),
        _ => fallback_multiplier,
    };
    scroll_ticks.saturating_mul(usize::from(multiplier.max(1)))
}
