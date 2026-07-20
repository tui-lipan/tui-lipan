//! ScrollView keyboard and scroll-wheel handlers.

use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::internal::{ScrollAction, apply_scroll_action, scroll_action_from_key};
use crate::widgets::{
    ScrollEvent, ScrollMetrics, ScrollWheelBehavior, calc_scroll_view_window,
    normalize_input_offset,
};

fn is_horizontal_action(action: ScrollAction) -> bool {
    matches!(
        action,
        ScrollAction::LineLeft(_) | ScrollAction::LineRight(_)
    )
}

fn h_scroll_metrics(sv: &crate::widgets::internal::ScrollViewNode) -> ScrollMetrics {
    ScrollMetrics {
        len: sv.content_width as usize,
        visible: sv.viewport_width as usize,
        max_offset: sv.h_max_offset,
    }
}

/// Outcome of a horizontal pan attempt.
///
/// Distinguishing `AtEdge` from `Moved` lets the wheel dispatcher bubble a
/// plain wheel tick to an ancestor once a horizontal-only view has run out of
/// travel, while keyboard panning still reports the key as consumed.
#[derive(Clone, Copy, PartialEq, Eq)]
enum HScrollOutcome {
    /// The node cannot pan horizontally at all.
    Rejected,
    /// Horizontal panning is possible but the offset did not change.
    AtEdge,
    /// The offset changed.
    Moved,
}

fn apply_h_scroll_inner(tree: &mut NodeTree, id: NodeId, action: ScrollAction) -> HScrollOutcome {
    let (next, offset) = {
        let NodeKind::ScrollView(sv) = &tree.node(id).kind else {
            return HScrollOutcome::Rejected;
        };
        if !sv.axis.horizontal_enabled() || sv.h_max_offset == 0 {
            return HScrollOutcome::Rejected;
        }
        let metrics = h_scroll_metrics(sv);
        let next = apply_scroll_action(sv.h_offset, metrics, action).min(sv.h_max_offset);
        (next, sv.h_offset)
    };

    if next == offset {
        return HScrollOutcome::AtEdge;
    }

    let NodeKind::ScrollView(sv) = &mut tree.node_mut(id).kind else {
        return HScrollOutcome::Rejected;
    };
    sv.h_offset = next;
    sv.h_scroll_offset = next as u16;
    sv.h_scroll_override = Some(next);
    sv.h_scroll_handler_dirty = true;
    HScrollOutcome::Moved
}

fn apply_h_scroll(tree: &mut NodeTree, id: NodeId, action: ScrollAction) -> bool {
    apply_h_scroll_inner(tree, id, action) != HScrollOutcome::Rejected
}

fn scroll_action_delta(action: crate::widgets::internal::ScrollAction) -> isize {
    match action {
        crate::widgets::internal::ScrollAction::LineUp(lines)
        | crate::widgets::internal::ScrollAction::LineLeft(lines) => -(lines as isize),
        crate::widgets::internal::ScrollAction::LineDown(lines)
        | crate::widgets::internal::ScrollAction::LineRight(lines) => lines as isize,
        crate::widgets::internal::ScrollAction::Home
        | crate::widgets::internal::ScrollAction::End => 0,
    }
}

/// Handle keyboard input for a focused ScrollView node.
///
/// Takes `key` by reference because this handler is also invoked during
/// ancestor bubble-up in `dispatch_key`.
///
/// When the offset changes, this function updates the node directly (like
/// `handle_scroll` does for mouse wheel) so that a layout-only re-reconcile
/// picks up the change without a full view() rebuild.
pub(crate) fn handle_key(tree: &mut NodeTree, node_id: NodeId, key: &KeyEvent) -> bool {
    let horizontal_action = {
        let node = tree.node(node_id);
        let NodeKind::ScrollView(sv) = &node.kind else {
            return false;
        };
        scroll_action_from_key(key, sv.scroll_keys)
            .filter(|action| is_horizontal_action(*action) && sv.axis.horizontal_enabled())
    };
    if let Some(action) = horizontal_action {
        return apply_h_scroll(tree, node_id, action);
    }

    // ── Read phase (immutable borrow) ────────────────────────────────────
    let (
        next,
        offset,
        metrics,
        on_scroll,
        on_scroll_to,
        scroll_keys,
        has_cancel_target,
        vertical_enabled,
    ) = {
        let node = tree.node(node_id);
        let NodeKind::ScrollView(sv) = &node.kind else {
            return false;
        };

        if !sv.axis.vertical_enabled() {
            return false;
        }

        let total = sv.content_height as usize;
        let viewport_h = sv.viewport_height as usize;

        let visible_for_scroll =
            calc_scroll_view_window(sv.offset, total, viewport_h, sv.show_scroll_indicators)
                .visible_rows;

        let metrics = ScrollMetrics {
            len: total,
            visible: visible_for_scroll,
            max_offset: sv.max_offset,
        };

        if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
            let mut current_visible = viewport_h;
            if sv.show_scroll_indicators && sv.top_indicator {
                current_visible = current_visible.saturating_sub(1);
            }
            if sv.show_scroll_indicators && sv.bottom_indicator {
                current_visible = current_visible.saturating_sub(1);
            }

            let page_size = current_visible.saturating_sub(1).max(1);

            let next = match key.code {
                KeyCode::PageDown => (sv.offset + page_size).min(sv.max_offset),
                KeyCode::PageUp => sv.offset.saturating_sub(page_size),
                _ => unreachable!(),
            };
            let next = normalize_input_offset(
                sv.offset,
                next,
                total,
                viewport_h,
                sv.show_scroll_indicators,
            );
            (
                next,
                sv.offset,
                metrics,
                sv.on_scroll.clone(),
                sv.on_scroll_to.clone(),
                sv.scroll_keys,
                sv.smooth_scroll.is_animating() || sv.scroll_target.is_some(),
                sv.axis.vertical_enabled(),
            )
        } else {
            let Some(action) = scroll_action_from_key(key, sv.scroll_keys) else {
                return false;
            };
            if is_horizontal_action(action) {
                return false;
            }
            let next = normalize_input_offset(
                sv.offset,
                apply_scroll_action(sv.offset, metrics, action).min(sv.max_offset),
                total,
                viewport_h,
                sv.show_scroll_indicators,
            );
            (
                next,
                sv.offset,
                metrics,
                sv.on_scroll.clone(),
                sv.on_scroll_to.clone(),
                sv.scroll_keys,
                sv.smooth_scroll.is_animating() || sv.scroll_target.is_some(),
                sv.axis.vertical_enabled(),
            )
        }
    };
    let _ = (scroll_keys, vertical_enabled);

    // ── Write phase (mutable borrow) ─────────────────────────────────────
    if next != offset {
        // Update node directly so layout-only re-reconcile picks up the change.
        let key = tree.node(node_id).key.clone();
        {
            let NodeKind::ScrollView(sv) = &mut tree.node_mut(node_id).kind else {
                unreachable!();
            };
            sv.smooth_scroll.cancel_at(next);
            sv.cancelled_scroll_target = sv.scroll_target.clone();
            sv.offset = next;
            sv.scroll_override = Some(next);
            sv.scroll_handler_dirty = true;
        }
        if let Some(key) = key {
            tree.scroll_input_offset_by_key.insert(key, next);
        }

        if let Some(cb) = on_scroll_to.as_ref() {
            cb.emit(next);
        } else if let Some(cb) = on_scroll.as_ref() {
            cb.emit(ScrollEvent {
                offset: next,
                metrics,
            });
        }
    } else if has_cancel_target {
        let NodeKind::ScrollView(sv) = &mut tree.node_mut(node_id).kind else {
            unreachable!();
        };
        sv.smooth_scroll.cancel_at(offset);
        sv.cancelled_scroll_target = sv.scroll_target.clone();
    }
    true
}

/// Whether a plain (unmodified) wheel tick over this node should pan
/// horizontally instead of being discarded.
///
/// Only horizontal-*only* views remap: when both axes are enabled the wheel
/// keeps its vertical meaning and `Shift` selects the horizontal axis.
pub(crate) fn wheel_remaps_to_horizontal(kind: &NodeKind) -> bool {
    let NodeKind::ScrollView(sv) = kind else {
        return false;
    };
    sv.scroll_wheel && sv.axis.horizontal_enabled() && !sv.axis.vertical_enabled()
}

/// Handle a plain wheel tick that [`wheel_remaps_to_horizontal`] redirected to
/// the horizontal axis.
///
/// Returns `false` when the view has no horizontal travel left, so the tick
/// bubbles to an ancestor that can still scroll. Without that, a horizontal
/// strip nested in a vertical `ScrollView` would trap the wheel.
pub(crate) fn handle_remapped_wheel_scroll(
    tree: &mut NodeTree,
    id: NodeId,
    action: ScrollAction,
) -> bool {
    apply_h_scroll_inner(tree, id, action) == HScrollOutcome::Moved
}

/// Handle scroll-wheel events for a ScrollView node.
pub(crate) fn handle_scroll(
    tree: &mut NodeTree,
    id: NodeId,
    action: crate::widgets::internal::ScrollAction,
) -> bool {
    if is_horizontal_action(action) {
        let NodeKind::ScrollView(sv) = &tree.node(id).kind else {
            return false;
        };
        if !sv.scroll_wheel {
            return false;
        }
        return apply_h_scroll(tree, id, action);
    }

    let (immediate_next, offset, metrics, on_scroll_to, on_scroll, key, has_cancel_target) = {
        let node = tree.node(id);
        let NodeKind::ScrollView(scroll) = &node.kind else {
            return false;
        };

        if !scroll.scroll_wheel || !scroll.axis.vertical_enabled() {
            return false;
        }

        let total = scroll.content_height as usize;
        let viewport_h = scroll.viewport_height as usize;
        let visible_for_scroll = calc_scroll_view_window(
            scroll.offset,
            total,
            viewport_h,
            scroll.show_scroll_indicators,
        )
        .visible_rows;

        let metrics = ScrollMetrics {
            len: total,
            visible: visible_for_scroll,
            max_offset: scroll.max_offset,
        };
        let immediate_next = normalize_input_offset(
            scroll.offset,
            apply_scroll_action(scroll.offset, metrics, action).min(scroll.max_offset),
            total,
            viewport_h,
            scroll.show_scroll_indicators,
        );
        (
            immediate_next,
            scroll.offset,
            metrics,
            scroll.on_scroll_to.clone(),
            scroll.on_scroll.clone(),
            node.key.clone(),
            scroll.smooth_scroll.is_animating() || scroll.scroll_target.is_some(),
        )
    };

    let next;
    let mut handled = false;
    let mut kinetic_started = false;
    let mut changed = false;
    {
        let NodeKind::ScrollView(scroll) = &mut tree.node_mut(id).kind else {
            return false;
        };
        match scroll.scroll_wheel_behavior {
            ScrollWheelBehavior::Immediate => {
                next = immediate_next;
                scroll.wheel_scroll.cancel_at(next);
            }
            ScrollWheelBehavior::Smooth(config) => {
                next = scroll.wheel_scroll.apply_impulse(
                    scroll.offset,
                    scroll_action_delta(action),
                    scroll.max_offset,
                    config,
                );
                kinetic_started = scroll.wheel_scroll.is_animating();
            }
        }

        if next != offset || kinetic_started {
            scroll.smooth_scroll.cancel_at(next);
            scroll.cancelled_scroll_target = scroll.scroll_target.clone();
            scroll.offset = next;
            scroll.scroll_override = Some(next);
            scroll.scroll_handler_dirty = true;
            handled = true;
            changed = next != offset;
        } else if has_cancel_target {
            scroll.smooth_scroll.cancel_at(offset);
            scroll.cancelled_scroll_target = scroll.scroll_target.clone();
            handled = true;
        }
    }

    if kinetic_started {
        tree.mark_animated_scroll(id);
    }

    if !handled {
        return false;
    }

    if !changed {
        return true;
    }

    if let Some(key) = key {
        tree.scroll_input_offset_by_key.insert(key, next);
    }

    if let Some(cb) = on_scroll_to.as_ref() {
        cb.emit(next);
    } else if let Some(cb) = on_scroll.as_ref() {
        cb.emit(ScrollEvent {
            offset: next,
            metrics,
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::time::Duration;

    use super::{
        handle_key, handle_remapped_wheel_scroll, handle_scroll, wheel_remaps_to_horizontal,
    };
    use crate::Length;
    use crate::animation::{Easing, TransitionConfig};
    use crate::callback::Callback;
    use crate::core::element::IntoElement;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::core::node::{NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::Rect;
    use crate::widgets::internal::ScrollAction;
    use crate::widgets::{HStack, ScrollAxis, ScrollBehavior, ScrollView, Text};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods::default(),
        }
    }

    fn make_scroll_view() -> crate::Element {
        ScrollView::new()
            .scroll_keys(crate::widgets::ScrollKeymap::DEFAULT)
            .children((0..20).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("row-{i}"))
            }))
            .into()
    }

    fn reconcile_scroll_view(root: &crate::Element) -> NodeTree {
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 5,
            },
            None,
        );
        tree
    }

    fn reconcile_scroll_view_into(tree: &mut NodeTree, root: &crate::Element) {
        LayoutEngine::reconcile_with_focus(
            tree,
            root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 5,
            },
            None,
        );
    }

    fn scroll_offset(tree: &NodeTree) -> usize {
        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected scroll view");
        };
        scroll.offset
    }

    #[test]
    fn keyboard_page_down_scrolls_without_callbacks() {
        let root = make_scroll_view();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;

        assert!(handle_key(&mut tree, root_id, &key(KeyCode::PageDown)));
        assert!(scroll_offset(&tree) > 0);

        let NodeKind::ScrollView(scroll) = &tree.node(tree.root).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(scroll.scroll_override, Some(scroll.offset));
        assert!(scroll.scroll_handler_dirty);
    }

    #[test]
    fn keyboard_arrow_down_scrolls_without_callbacks() {
        let root = make_scroll_view();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;

        assert!(handle_key(&mut tree, root_id, &key(KeyCode::Down)));
        assert_eq!(scroll_offset(&tree), 1);
    }

    #[test]
    fn wheel_scroll_is_immediate_by_default() {
        let root = make_scroll_view();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;

        assert!(handle_scroll(
            &mut tree,
            root_id,
            crate::widgets::internal::ScrollAction::LineDown(1),
        ));

        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(scroll.offset, 1);
        assert!(!scroll.wheel_scroll.is_animating());
        assert!(!tree.has_animated_scrolls());
    }

    #[test]
    fn smooth_wheel_scroll_starts_kinetic_animation() {
        let root: crate::Element = ScrollView::new()
            .smooth_wheel_scroll(true)
            .children((0..20).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("row-{i}"))
            }))
            .into();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;

        assert!(handle_scroll(
            &mut tree,
            root_id,
            crate::widgets::internal::ScrollAction::LineDown(1),
        ));

        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(scroll.offset, 0);
        assert!(scroll.wheel_scroll.is_animating());
        assert!(scroll.scroll_handler_dirty);
        assert!(tree.has_animated_scrolls());
    }

    #[test]
    fn keyboard_scroll_cancels_active_smooth_scroll() {
        let root = make_scroll_view();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;
        {
            let NodeKind::ScrollView(scroll) = &mut tree.node_mut(root_id).kind else {
                panic!("expected scroll view");
            };
            scroll.smooth_scroll.resolve_target(
                0,
                10,
                scroll.max_offset,
                ScrollBehavior::smooth(TransitionConfig {
                    duration: Duration::from_millis(100),
                    easing: Easing::Linear,
                }),
            );
            assert!(scroll.smooth_scroll.is_animating());
        }

        assert!(handle_key(&mut tree, root_id, &key(KeyCode::Down)));

        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(scroll.offset, 1);
        assert!(!scroll.smooth_scroll.is_animating());
    }

    #[test]
    fn no_op_user_scroll_cancels_active_smooth_scroll() {
        let root: crate::Element = ScrollView::new()
            .scroll_keys(crate::widgets::ScrollKeymap::DEFAULT)
            .scroll_to_key("row-10")
            .scroll_transition(TransitionConfig {
                duration: Duration::from_millis(100),
                easing: Easing::Linear,
            })
            .children((0..20).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("row-{i}"))
            }))
            .into();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;

        assert!(handle_scroll(
            &mut tree,
            root_id,
            crate::widgets::internal::ScrollAction::LineUp(1),
        ));

        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(scroll.offset, 0);
        assert!(!scroll.smooth_scroll.is_animating());

        reconcile_scroll_view_into(&mut tree, &root);
        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(scroll.offset, 0);
        assert!(!scroll.smooth_scroll.is_animating());
    }

    #[test]
    fn keyboard_scroll_suppresses_same_smooth_target_until_target_changes() {
        let root: crate::Element = ScrollView::new()
            .scroll_keys(crate::widgets::ScrollKeymap::DEFAULT)
            .scroll_to_key("row-10")
            .scroll_transition(TransitionConfig {
                duration: Duration::from_millis(100),
                easing: Easing::Linear,
            })
            .children((0..20).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("row-{i}"))
            }))
            .into();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;

        assert!(handle_key(&mut tree, root_id, &key(KeyCode::Down)));
        assert_eq!(scroll_offset(&tree), 1);

        reconcile_scroll_view_into(&mut tree, &root);

        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(scroll.offset, 1);
        assert!(!scroll.smooth_scroll.is_animating());
    }

    #[test]
    fn keyboard_scroll_uses_on_scroll_to_when_present() {
        let offsets = Rc::new(RefCell::new(Vec::new()));
        let offsets_cb = offsets.clone();
        let root: crate::Element = ScrollView::new()
            .scroll_keys(crate::widgets::ScrollKeymap::DEFAULT)
            .on_scroll_to(Callback::new(move |offset| {
                offsets_cb.borrow_mut().push(offset)
            }))
            .children((0..20).map(|i| {
                Text::new(format!("row {i}"))
                    .height(Length::Px(1))
                    .key(format!("row-{i}"))
            }))
            .into();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;

        assert!(handle_key(&mut tree, root_id, &key(KeyCode::PageDown)));
        assert_eq!(offsets.borrow().as_slice(), &[4]);
    }

    fn make_both_axes_scroll_view() -> crate::Element {
        ScrollView::new()
            .axis(ScrollAxis::Both)
            .h_scrollbar(true)
            .scroll_keys(crate::widgets::ScrollKeymap::DEFAULT)
            .children((0..15).map(|i| {
                HStack::new()
                    .child(Text::new(format!("row {i} leading")))
                    .child(Text::new(" · "))
                    .child(Text::new(
                        "extra wide trailing content for horizontal overflow",
                    ))
                    .height(Length::Px(1))
                    .width(Length::Auto)
                    .key(format!("row-{i}"))
            }))
            .into()
    }

    #[test]
    fn horizontal_arrow_right_updates_h_offset() {
        let root = make_both_axes_scroll_view();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;

        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert!(scroll.h_max_offset > 0, "expected horizontal overflow");

        assert!(handle_key(&mut tree, root_id, &key(KeyCode::Right)));

        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert!(scroll.h_offset > 0);
        assert!(scroll.h_scroll_handler_dirty);
    }

    fn make_horizontal_only_scroll_view() -> crate::Element {
        ScrollView::new()
            .axis(ScrollAxis::Horizontal)
            .h_scrollbar(true)
            .scroll_keys(crate::widgets::ScrollKeymap::DEFAULT)
            .child(
                HStack::new()
                    .child(Text::new(
                        "extra wide trailing content for horizontal overflow",
                    ))
                    .height(Length::Px(1))
                    .width(Length::Auto)
                    .key("row"),
            )
            .into()
    }

    #[test]
    fn plain_wheel_remaps_on_horizontal_only_view() {
        let root = make_horizontal_only_scroll_view();
        let tree = reconcile_scroll_view(&root);
        assert!(wheel_remaps_to_horizontal(&tree.node(tree.root).kind));
    }

    #[test]
    fn plain_wheel_does_not_remap_when_vertical_is_enabled() {
        for root in [make_both_axes_scroll_view(), make_scroll_view()] {
            let tree = reconcile_scroll_view(&root);
            assert!(!wheel_remaps_to_horizontal(&tree.node(tree.root).kind));
        }
    }

    #[test]
    fn remapped_wheel_pans_then_releases_at_edge() {
        let root = make_horizontal_only_scroll_view();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;

        assert!(handle_remapped_wheel_scroll(
            &mut tree,
            root_id,
            ScrollAction::LineRight(1),
        ));
        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert!(scroll.h_offset > 0);

        // Pan back to the left edge: still a real move, so still handled.
        assert!(handle_remapped_wheel_scroll(
            &mut tree,
            root_id,
            ScrollAction::LineLeft(99),
        ));
        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert_eq!(scroll.h_offset, 0);

        // Now out of travel: report unhandled so an ancestor can scroll.
        assert!(!handle_remapped_wheel_scroll(
            &mut tree,
            root_id,
            ScrollAction::LineLeft(1),
        ));
    }

    #[test]
    fn horizontal_wheel_action_updates_h_offset() {
        let root = make_both_axes_scroll_view();
        let mut tree = reconcile_scroll_view(&root);
        let root_id = tree.root;

        assert!(handle_scroll(
            &mut tree,
            root_id,
            crate::widgets::internal::ScrollAction::LineRight(1),
        ));

        let NodeKind::ScrollView(scroll) = &tree.node(root_id).kind else {
            panic!("expected scroll view");
        };
        assert!(scroll.h_offset > 0);
    }
}
