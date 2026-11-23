//! Keyboard and scroll handlers for Tabs and DraggableTabBar.
//!
//! The keyboard logic is identical for both variants, so a single `handle_key`
//! function handles both `NodeKind::Tabs` and `NodeKind::DraggableTabBar`.

use crate::callback::{Callback, KeyHandler};
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::TabsEvent;

/// Shared tab navigation state extracted from either `Tabs` or `DraggableTabBar`.
struct TabState {
    len: usize,
    selected: Option<usize>,
    on_select: Option<Callback<TabsEvent>>,
    on_key: Option<KeyHandler>,
}

fn extract_tab_state(kind: &NodeKind) -> Option<TabState> {
    match kind {
        NodeKind::Tabs(node) => Some(TabState {
            len: node.tabs.len(),
            selected: node.selected,
            on_select: node.on_select.clone(),
            on_key: node.on_key.clone(),
        }),
        NodeKind::DraggableTabBar(node) => Some(TabState {
            len: node.tabs.len(),
            selected: node.selected,
            on_select: node.on_select.clone(),
            on_key: node.on_key.clone(),
        }),
        _ => None,
    }
}

/// Handle keyboard input for a focused Tabs or DraggableTabBar node.
///
/// Left/Right/Home/End navigate between tabs; unhandled keys fall through to `on_key`.
pub(crate) fn handle_key(tree: &NodeTree, id: NodeId, key: KeyEvent) -> bool {
    let Some(state) = extract_tab_state(&tree.node(id).kind) else {
        return false;
    };

    let handle_key_cb = |handler: &KeyHandler| -> bool { handler.handle(key) };
    let mut handled = false;

    if state.len == 0 {
        if let Some(cb) = state.on_key.as_ref() {
            handled = handle_key_cb(cb);
        }
    } else {
        let current = state.selected.map(|s| s.min(state.len.saturating_sub(1)));

        if let Some(cb) = state.on_select.as_ref() {
            let next: Option<usize> = match (current, key.code) {
                (Some(active), KeyCode::Left) => Some(active.saturating_sub(1)),
                (Some(active), KeyCode::Right) => {
                    Some((active + 1).min(state.len.saturating_sub(1)))
                }
                (None, KeyCode::Left) => Some(state.len.saturating_sub(1)),
                (None, KeyCode::Right) => Some(0),
                (_, KeyCode::Home) => Some(0),
                (_, KeyCode::End) => Some(state.len.saturating_sub(1)),
                _ => None,
            };

            if let Some(next) = next {
                if current != Some(next) {
                    cb.emit(TabsEvent { index: next });
                }
                handled = true;
            }
        }

        if !handled && let Some(cb) = state.on_key.as_ref() {
            handled = handle_key_cb(cb);
        }
    }

    handled
}

/// Handle scroll-wheel events for a DraggableTabBar node.
pub(crate) fn handle_tab_bar_scroll(
    tree: &mut NodeTree,
    id: NodeId,
    action: crate::widgets::internal::ScrollAction,
) -> bool {
    let rect = tree.node(id).rect;
    let NodeKind::DraggableTabBar(tab_bar) = &mut tree.node_mut(id).kind else {
        return false;
    };

    if !tab_bar.scroll_wheel || tab_bar.disabled {
        return false;
    }

    let step_right = match action {
        crate::widgets::internal::ScrollAction::LineDown(_)
        | crate::widgets::internal::ScrollAction::LineRight(_) => Some(true),
        crate::widgets::internal::ScrollAction::LineUp(_)
        | crate::widgets::internal::ScrollAction::LineLeft(_) => Some(false),
        _ => None,
    };

    let Some(step_right) = step_right else {
        return false;
    };

    let inner = rect.inner(tab_bar.border, tab_bar.padding);
    let tab_disp_opts = tab_bar.display_options();
    let tab_vp_opts = tab_bar.viewport_options(inner.w as usize);
    let layout = crate::widgets::DraggableTabBar::viewport_layout(
        &tab_bar.tabs,
        &tab_disp_opts,
        &tab_vp_opts,
    );

    if (step_right && layout.hidden_right == 0) || (!step_right && layout.hidden_left == 0) {
        return false;
    }

    let next = crate::widgets::DraggableTabBar::scroll_offset_for_step(
        &tab_bar.tabs,
        &tab_disp_opts,
        &crate::widgets::draggable_tab_bar::TabViewportOptions {
            scroll_offset: layout.offset,
            viewport_width: inner.w as usize,
            show_overflow_controls: tab_bar.show_overflow_controls,
            overflow_labels: tab_bar.overflow_labels.clone(),
        },
        step_right,
        crate::widgets::draggable_tab_bar::TAB_SCROLL_STEP_CHARS,
    );
    if next != tab_bar.scroll_offset {
        tab_bar.scroll_offset = next;
        tab_bar.scroll_override = Some(next);
        true
    } else {
        false
    }
}
