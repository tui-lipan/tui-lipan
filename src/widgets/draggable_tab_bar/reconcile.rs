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

    let (old_offset, old_override, old_previous_active, old_tabs) = match &tree.node(id).kind {
        NodeKind::DraggableTabBar(node) => (
            node.scroll_offset,
            node.scroll_override,
            node.previous_active,
            Some(node.tabs.clone()),
        ),
        _ => (bar.scroll_offset, None, bar.active, None),
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

    let node = tree.node_mut(id);
    node.rect = rect;
    node.children.clear();
    node.kind = NodeKind::DraggableTabBar(next_node);
    id
}
