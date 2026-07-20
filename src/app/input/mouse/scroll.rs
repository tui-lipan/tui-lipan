//! Mouse scroll-wheel dispatch.
//!
//! The per-widget scroll handlers live in [`crate::app::input::handlers`]; this
//! module provides the thin dispatcher that hit-tests, classifies the target
//! node, and delegates to the appropriate handler while bubbling up the tree.

use crate::app::input::handlers::{self, ScrollableTag};
use crate::core::event::MouseEvent;
use crate::core::node::{NodeKind, NodeTree};
use crate::widgets::internal::{ScrollAction, scroll_action_from_mouse_n};

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

        // A plain wheel tick over a horizontal-only ScrollView pans that view
        // instead of being discarded; `Shift` stays the explicit horizontal
        // override everywhere else.
        let remapped = !event.mods.shift
            && handlers::scroll_view::wheel_remaps_to_horizontal(&tree.node(id).kind);
        let scroll_lines = effective_scroll_lines(
            &tree.node(id).kind,
            scroll_ticks,
            fallback_multiplier,
            event.mods.shift || remapped,
        );
        let Some(action) = scroll_action_from_mouse_n(event, scroll_lines) else {
            return false;
        };
        let action = if remapped {
            to_horizontal_action(action)
        } else {
            action
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
            ScrollableTag::ScrollView if remapped => {
                handlers::scroll_view::handle_remapped_wheel_scroll(tree, id, action)
            }
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
            ScrollableTag::PanView => handlers::pan_view::handle_scroll(tree, id, action),
            ScrollableTag::NonScrollable => false,
        };

        if handled {
            return true;
        }

        cur = parent;
    }

    false
}

/// Rewrite a vertical wheel action onto the horizontal axis.
fn to_horizontal_action(action: ScrollAction) -> ScrollAction {
    match action {
        ScrollAction::LineUp(lines) => ScrollAction::LineLeft(lines),
        ScrollAction::LineDown(lines) => ScrollAction::LineRight(lines),
        other => other,
    }
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
        // PanView scales each tick by its own `key_step` instead, so the
        // dispatcher must hand it raw tick counts.
        NodeKind::PanView(_) => 1,
        _ => fallback_multiplier,
    };
    scroll_ticks.saturating_mul(usize::from(multiplier.max(1)))
}

#[cfg(test)]
mod tests {
    use super::handle_scroll_wheel_n;
    use crate::Length;
    use crate::core::element::IntoElement;
    use crate::core::event::{KeyMods, MouseEvent, MouseKind};
    use crate::core::node::{NodeId, NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::Rect;
    use crate::widgets::{HStack, ScrollAxis, ScrollView, Text, VStack};

    /// Vertical outer ScrollView wrapping a horizontal-only inner strip.
    fn nested_tree() -> NodeTree {
        let strip = ScrollView::new()
            .axis(ScrollAxis::Horizontal)
            .height(Length::Px(1))
            .child(
                HStack::new()
                    .child(Text::new("wide horizontal strip content that overflows"))
                    .height(Length::Px(1))
                    .width(Length::Auto)
                    .key("strip-row"),
            );

        let column = (0..20).fold(VStack::new().child(strip), |column, i| {
            column.child(
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("row-{i}")),
            )
        });

        let root: crate::Element = ScrollView::new().child(column).into();

        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 8,
            },
            None,
        );
        tree
    }

    fn wheel_down(x: u16, y: u16) -> MouseEvent {
        MouseEvent {
            x,
            y,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        }
    }

    /// Depth-first ScrollView ids: `[0]` is the outer view, `[1]` the strip.
    fn scroll_views(tree: &NodeTree) -> Vec<NodeId> {
        fn walk(tree: &NodeTree, id: NodeId, out: &mut Vec<NodeId>) {
            if matches!(tree.node(id).kind, NodeKind::ScrollView(_)) {
                out.push(id);
            }
            for child in tree.node(id).children.clone() {
                walk(tree, child, out);
            }
        }
        let mut out = Vec::new();
        walk(tree, tree.root, &mut out);
        out
    }

    /// `(vertical offset, horizontal offset)` of the nth ScrollView.
    fn offsets(tree: &NodeTree, index: usize) -> (usize, usize) {
        let id = scroll_views(tree)[index];
        let NodeKind::ScrollView(sv) = &tree.node(id).kind else {
            panic!("expected scroll view");
        };
        (sv.offset, sv.h_offset)
    }

    const OUTER: usize = 0;
    const STRIP: usize = 1;

    #[test]
    fn plain_wheel_pans_horizontal_only_child() {
        let mut tree = nested_tree();
        assert!(handle_scroll_wheel_n(&mut tree, wheel_down(2, 0), 1, 1));

        let (_, h) = offsets(&tree, STRIP);
        assert!(h > 0, "inner strip should have panned right");
        assert_eq!(offsets(&tree, OUTER).0, 0, "outer must not scroll");
    }

    #[test]
    fn exhausted_horizontal_child_bubbles_to_vertical_ancestor() {
        let mut tree = nested_tree();

        // Drive the strip to its right edge.
        for _ in 0..200 {
            if !handle_scroll_wheel_n(&mut tree, wheel_down(2, 0), 1, 1) {
                break;
            }
            if offsets(&tree, OUTER).0 > 0 {
                break;
            }
        }

        let (outer, _) = offsets(&tree, OUTER);
        assert!(
            outer > 0,
            "wheel should bubble to the vertical ancestor once the strip is exhausted"
        );
    }
}
