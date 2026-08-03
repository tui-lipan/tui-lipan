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

    let (old_offset, old_override, old_previous_active, old_tabs, old_width_lock) =
        match &tree.node(id).kind {
            NodeKind::DraggableTabBar(node) => (
                node.scroll_offset,
                node.scroll_override,
                node.previous_active,
                Some(node.tabs.clone()),
                node.width_lock,
            ),
            _ => (bar.scroll_offset, None, bar.active, None, None),
        };

    let active_changed =
        !bar.tabs.is_empty() && bar.active < bar.tabs.len() && bar.active != old_previous_active;
    let viewport_w = rect.w.saturating_sub(bar.padding.horizontal()).max(1) as usize;
    let display_options = bar.display_options();
    let mut next_offset = old_override.unwrap_or(old_offset);
    if bar.tabs.is_empty() {
        next_offset = 0;
    } else if active_changed && bar.active < bar.tabs.len() {
        let still_visible = super::tab_fully_visible_at_offset(
            &bar.tabs,
            &display_options,
            bar.active,
            next_offset,
            viewport_w,
        );
        if !still_visible {
            next_offset = DraggableTabBar::scroll_offset_to_reveal_tab(
                &bar.tabs,
                &display_options,
                &super::TabViewportOptions {
                    scroll_offset: 0,
                    viewport_width: viewport_w,
                    show_overflow_controls: bar.show_overflow_controls,
                },
                bar.active,
            );
        }
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
