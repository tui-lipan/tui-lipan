use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::Rect;

use super::{DraggableTabBar, measure_draggable_tab_bar};

pub fn reconcile_draggable_tab_bar(
    tree: &mut NodeTree,
    id: NodeId,
    bar: &DraggableTabBar,
    rect: Rect,
) -> NodeId {
    let (w, h) = measure_draggable_tab_bar(bar);

    let mut rect = rect;
    if matches!(bar.width, crate::style::Length::Auto) {
        rect.w = w.min(rect.w);
    }
    if matches!(bar.height, crate::style::Length::Auto) {
        rect.h = h.min(rect.h);
    }

    let (old_offset, old_override, old_previous_active, old_tabs, old_width_lock, old_pending_swap) =
        match &tree.node(id).kind {
            NodeKind::DraggableTabBar(node) => (
                node.scroll_offset,
                node.scroll_override,
                node.previous_active,
                Some(node.tabs.clone()),
                node.width_lock,
                node.pending_swap,
            ),
            _ => (bar.scroll_offset, None, bar.active, None, None, None),
        };

    // A newly mounted bar has no earlier position to keep, so it opens on its active tab unless the
    // app asked for a position of its own. Its first layout can be provisional, so a later width
    // reveals again, until the user scrolls the strip themselves.
    let mounting = !matches!(tree.node(id).kind, NodeKind::DraggableTabBar(_));
    let resized = old_override.is_none() && tree.node(id).rect.w != rect.w;
    let reveal_active = bar.active < bar.tabs.len()
        && (bar.active != old_previous_active || ((mounting || resized) && bar.scroll_offset == 0));
    let viewport_w = rect.w.saturating_sub(bar.padding.horizontal()).max(1) as usize;
    let display_options = bar.display_options();
    let mut next_offset = old_override.unwrap_or(old_offset);
    if bar.tabs.is_empty() {
        next_offset = 0;
    } else if reveal_active {
        next_offset = DraggableTabBar::scroll_offset_to_reveal_tab(
            &bar.tabs,
            &display_options,
            &super::TabViewportOptions {
                scroll_offset: next_offset,
                viewport_width: viewport_w,
                show_overflow_controls: bar.show_overflow_controls,
                overflow_labels: bar.overflow_labels.clone(),
            },
            bar.active,
        );
    } else {
        let total_width =
            DraggableTabBar::content_width_for_viewport(&bar.tabs, &display_options, viewport_w);
        next_offset = next_offset.min(total_width.saturating_sub(1));
    }

    let mut next_node: crate::widgets::internal::DraggableTabBarNode = bar.clone().into();
    if let Some(old_tabs) = old_tabs.as_deref() {
        for (next_tab, old_tab) in std::sync::Arc::make_mut(&mut next_node.tabs)
            .iter_mut()
            .zip(old_tabs.iter())
        {
            let old_frame = old_tab
                .leading
                .as_ref()
                .and_then(|leading| leading.spinner_frame());
            if let Some(next_spinner) = next_tab
                .leading
                .as_mut()
                .and_then(|leading| leading.spinner_mut())
                && next_spinner.spinner.frame.is_none()
            {
                next_spinner.spinner.frame = old_frame;
            }
        }
    }
    next_node.previous_active = bar.active;
    next_node.scroll_offset = next_offset;
    next_node.scroll_override = old_override.map(|_| next_offset);
    next_node.width_lock = old_width_lock.filter(|lock| {
        next_node
            .tabs
            .get(lock.index)
            .is_some_and(super::is_reorderable_tab)
    });
    next_node.pending_swap = old_pending_swap.filter(|&(from, to)| {
        from != to
            && next_node
                .tabs
                .get(from)
                .is_some_and(super::is_reorderable_tab)
            && next_node
                .tabs
                .get(to)
                .is_some_and(super::is_reorderable_tab)
    });

    let node = tree.node_mut(id);
    node.rect = rect;
    node.children.clear();
    node.kind = NodeKind::DraggableTabBar(next_node);
    id
}

#[cfg(test)]
mod tests {
    use crate::core::element::Element;
    use crate::core::node::{NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::Rect;
    use crate::widgets::{DraggableTab, DraggableTabBar};

    fn bar(tabs: &[&str]) -> Element {
        DraggableTabBar::new()
            .tabs(
                tabs.iter()
                    .map(|label| DraggableTab::new(*label).closeable(true)),
            )
            .into()
    }

    #[test]
    fn reconcile_keeps_closed_tab_width_for_its_replacement() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 1,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &bar(&["a-very-wide-tab", "x", "tail"]),
            bounds,
            None,
        );

        let NodeKind::DraggableTabBar(node) = &mut tree.node_mut(tree.root).kind else {
            panic!("expected draggable tab bar");
        };
        node.lock_closed_tab_width(0, bounds.w as usize);
        let locked_width = node.width_lock.expect("width lock").width;

        LayoutEngine::reconcile_with_focus(&mut tree, &bar(&["x", "tail"]), bounds, None);

        let NodeKind::DraggableTabBar(node) = &tree.node(tree.root).kind else {
            panic!("expected draggable tab bar");
        };
        let layout = DraggableTabBar::viewport_layout(
            &node.tabs,
            &node.display_options(),
            &node.viewport_options(bounds.w as usize),
        );
        assert_eq!(layout.visible_tabs[0].metrics.width, locked_width);
        assert_eq!(
            layout.visible_tabs[0].metrics.close_end,
            Some(locked_width.saturating_sub(1))
        );
    }

    const BOUNDS: Rect = Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 1,
    };
    const LABELS: [&str; 8] = [
        "alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
    ];

    fn scrolling_bar(active: usize) -> DraggableTabBar {
        DraggableTabBar::new()
            .tabs(LABELS.iter().map(|label| DraggableTab::new(*label)))
            .active(active)
            .show_close_buttons(false)
            .border(false)
            .divider(' ')
    }

    /// The columns of the content strip the active tab occupies, and the strip's scroll offset.
    fn active_span(tree: &NodeTree) -> (usize, usize, usize, usize) {
        let NodeKind::DraggableTabBar(node) = &tree.node(tree.root).kind else {
            panic!("expected draggable tab bar");
        };
        let layout = DraggableTabBar::viewport_layout(
            &node.tabs,
            &node.display_options(),
            &node.viewport_options(BOUNDS.w as usize),
        );
        let active = layout
            .visible_tabs
            .iter()
            .find(|tab| tab.index == node.active)
            .expect("active tab is on screen");
        (
            active.clip_left,
            active.end - active.start,
            active.metrics.width,
            layout.offset,
        )
    }

    #[test]
    fn a_newly_mounted_bar_opens_on_its_active_tab() {
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &scrolling_bar(6).into(), BOUNDS, None);

        let (clip_left, shown, width, offset) = active_span(&tree);
        assert!(offset > 0, "the bar should have scrolled to reach the tab");
        assert_eq!(clip_left, 0, "the active tab starts on screen");
        assert_eq!(shown, width, "the active tab is shown whole");
    }

    #[test]
    fn a_bar_first_laid_out_wider_than_it_ends_up_still_opens_on_its_active_tab() {
        let wide = Rect { w: 200, ..BOUNDS };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &scrolling_bar(6).into(), wide, None);
        LayoutEngine::reconcile_with_focus(&mut tree, &scrolling_bar(6).into(), BOUNDS, None);

        let (clip_left, shown, width, _) = active_span(&tree);
        assert_eq!(clip_left, 0, "the active tab starts on screen");
        assert_eq!(shown, width, "the active tab is shown whole");
    }

    #[test]
    fn a_resize_does_not_undo_a_scroll_the_user_made() {
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &scrolling_bar(0).into(), BOUNDS, None);
        let NodeKind::DraggableTabBar(node) = &mut tree.node_mut(tree.root).kind else {
            panic!("expected draggable tab bar");
        };
        node.scroll_override = Some(12);

        let narrower = Rect { w: 25, ..BOUNDS };
        LayoutEngine::reconcile_with_focus(&mut tree, &scrolling_bar(0).into(), narrower, None);

        let NodeKind::DraggableTabBar(node) = &tree.node(tree.root).kind else {
            panic!("expected draggable tab bar");
        };
        assert_eq!(node.scroll_offset, 12);
    }

    #[test]
    fn a_newly_mounted_bar_keeps_a_position_the_app_asked_for() {
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &scrolling_bar(0).scroll_offset(8).into(),
            BOUNDS,
            None,
        );

        let NodeKind::DraggableTabBar(node) = &tree.node(tree.root).kind else {
            panic!("expected draggable tab bar");
        };
        assert_eq!(node.scroll_offset, 8);
    }

    #[test]
    fn changing_to_a_tab_under_an_overflow_control_reveals_it() {
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &scrolling_bar(0).into(), BOUNDS, None);
        // Walk right one tab at a time: each step lands on a tab that fits the bar's full width but
        // can sit behind the control the strip reserves on that side.
        for active in 1..LABELS.len() {
            LayoutEngine::reconcile_with_focus(
                &mut tree,
                &scrolling_bar(active).into(),
                BOUNDS,
                None,
            );
            let (clip_left, shown, width, _) = active_span(&tree);
            assert_eq!(clip_left, 0, "tab {active} starts on screen");
            assert_eq!(shown, width, "tab {active} is shown whole");
        }
    }

    #[test]
    fn changing_to_a_tab_already_on_screen_leaves_the_strip_where_it_is() {
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(&mut tree, &scrolling_bar(6).into(), BOUNDS, None);
        let (_, _, _, offset) = active_span(&tree);

        LayoutEngine::reconcile_with_focus(&mut tree, &scrolling_bar(5).into(), BOUNDS, None);

        let (_, _, _, after) = active_span(&tree);
        assert_eq!(after, offset, "stepping to a visible neighbor scrolled");
    }

    fn bar_with_action(tabs: &[&str]) -> Element {
        DraggableTabBar::new()
            .tabs(
                tabs.iter()
                    .map(|label| DraggableTab::new(*label).closeable(true)),
            )
            .tab(DraggableTab::action("+"))
            .into()
    }

    #[test]
    fn lock_closed_tab_width_skips_trailing_action_tab() {
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 80,
            h: 1,
        };
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &bar_with_action(&["a-very-wide-tab"]),
            bounds,
            None,
        );

        let NodeKind::DraggableTabBar(node) = &mut tree.node_mut(tree.root).kind else {
            panic!("expected draggable tab bar");
        };
        let natural_action_width = {
            let layout = DraggableTabBar::viewport_layout(
                &node.tabs,
                &node.display_options(),
                &node.viewport_options(bounds.w as usize),
            );
            layout.visible_tabs[1].metrics.width
        };
        node.lock_closed_tab_width(0, bounds.w as usize);
        assert_eq!(node.width_lock, None);

        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &DraggableTabBar::new().tab(DraggableTab::action("+")).into(),
            bounds,
            None,
        );

        let NodeKind::DraggableTabBar(node) = &tree.node(tree.root).kind else {
            panic!("expected draggable tab bar");
        };
        assert_eq!(node.width_lock, None);
        let layout = DraggableTabBar::viewport_layout(
            &node.tabs,
            &node.display_options(),
            &node.viewport_options(bounds.w as usize),
        );
        assert_eq!(layout.visible_tabs[0].metrics.width, natural_action_width);
    }
}
