//! PanView keyboard and scroll-wheel handlers.

use crate::core::event::KeyEvent;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::PanEvent;
use crate::widgets::internal::{
    PanAction, ScrollAction, apply_pan_action, pan_action_from_key, pan_metrics,
};

#[cfg(feature = "image")]
fn suspend_image_rendering_for_pan() {
    crate::backend::ratatui_backend::image_support::suspend_image_rendering_for(
        std::time::Duration::from_millis(120),
    );
}

/// Apply a resolved pan action to a node, emitting `on_pan` when it moves.
///
/// Returns whether the offset actually changed. Callers decide what an
/// unchanged offset means: keyboard panning still consumes the key, while a
/// wheel tick lets it bubble to an ancestor.
fn commit_pan(tree: &mut NodeTree, node_id: NodeId, action: PanAction) -> bool {
    let (next, offset, metrics, on_pan, state_key) = {
        let node = tree.node(node_id);
        let NodeKind::PanView(pan) = &node.kind else {
            return false;
        };
        let metrics = pan_metrics(pan.content_w, pan.content_h, pan.viewport_w, pan.viewport_h);
        let offset = (pan.offset_x, pan.offset_y);
        let next = apply_pan_action(offset, action, metrics, pan.clamp, pan.free_pan_margin);
        (
            next,
            offset,
            metrics,
            pan.on_pan.clone(),
            pan.state_key.clone().or_else(|| node.key.clone()),
        )
    };

    if next == offset {
        return false;
    }

    if let NodeKind::PanView(pan) = &mut tree.node_mut(node_id).kind {
        pan.offset_x = next.0;
        pan.offset_y = next.1;
        pan.input_override = Some(next);
        pan.input_dirty = true;
    }
    if let Some(key) = state_key {
        tree.pan_input_offset_by_key.insert(key, next);
    }
    if let Some(cb) = on_pan.as_ref() {
        cb.emit(PanEvent {
            x: next.0,
            y: next.1,
            metrics,
        });
    }
    #[cfg(feature = "image")]
    suspend_image_rendering_for_pan();
    true
}

/// Handle a scroll-wheel tick over a PanView.
///
/// The wheel pans vertically and `Shift`+wheel horizontally (the dispatcher
/// has already resolved which, via `ScrollAction`). Each tick moves one
/// `key_step` in that axis, so wheel and keyboard panning share one notion of
/// step size and the horizontal step stays wider to match cell aspect ratio.
///
/// Returns `false` when the offset does not move, so a clamped view at its
/// edge releases the wheel to an ancestor. An unclamped free canvas never
/// stops moving and therefore keeps consuming, which is the intended
/// behavior for an infinite surface.
pub(crate) fn handle_scroll(tree: &mut NodeTree, node_id: NodeId, action: ScrollAction) -> bool {
    let (step_x, step_y) = {
        let NodeKind::PanView(pan) = &tree.node(node_id).kind else {
            return false;
        };
        if !pan.wheel_to_pan {
            return false;
        }
        pan.key_step
    };

    let clamp_step = |step: u16, lines: usize| -> i16 {
        let cells = (step.max(1) as usize).saturating_mul(lines.max(1));
        cells.min(i16::MAX as usize) as i16
    };

    let delta = match action {
        ScrollAction::LineUp(lines) => PanAction::Delta(0, -clamp_step(step_y, lines)),
        ScrollAction::LineDown(lines) => PanAction::Delta(0, clamp_step(step_y, lines)),
        ScrollAction::LineLeft(lines) => PanAction::Delta(-clamp_step(step_x, lines), 0),
        ScrollAction::LineRight(lines) => PanAction::Delta(clamp_step(step_x, lines), 0),
        ScrollAction::Home | ScrollAction::End => return false,
    };

    commit_pan(tree, node_id, delta)
}

pub(crate) fn handle_key(tree: &mut NodeTree, node_id: NodeId, key: &KeyEvent) -> bool {
    let action = {
        let NodeKind::PanView(pan) = &tree.node(node_id).kind else {
            return false;
        };
        pan_action_from_key(key, pan.keymap, pan.key_step)
    };
    let Some(action) = action else {
        return false;
    };

    // A recognised pan key is consumed even when the view is already at its
    // edge, so arrows do not leak to an ancestor mid-pan.
    commit_pan(tree, node_id, action);
    true
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{handle_key, handle_scroll};
    use crate::callback::Callback;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::core::node::{NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::{Length, Rect};
    use crate::widgets::internal::ScrollAction;
    use crate::widgets::{PanEvent, PanView, Text};

    const KEY_STEP: (u16, u16) = (4, 2);

    fn pan_tree(clamp: bool, wheel: bool) -> (NodeTree, Rc<RefCell<Vec<PanEvent>>>) {
        let events = Rc::new(RefCell::new(Vec::new()));
        let sink = events.clone();
        let root: crate::Element = PanView::new()
            .width(Length::Px(10))
            .height(Length::Px(4))
            .clamp(clamp)
            .wheel_to_pan(wheel)
            .key_step(KEY_STEP)
            .on_pan(Callback::new(move |e| sink.borrow_mut().push(e)))
            .child(Text::new(
                "a very wide line of content that overflows the viewport horizontally",
            ))
            .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 4,
            },
            None,
        );
        (tree, events)
    }

    fn offsets(tree: &NodeTree) -> (i32, i32) {
        let NodeKind::PanView(pan) = &tree.node(tree.root).kind else {
            panic!("expected pan view");
        };
        (pan.offset_x, pan.offset_y)
    }

    #[test]
    fn wheel_pans_horizontally_by_one_key_step() {
        let (mut tree, events) = pan_tree(true, true);
        let root = tree.root;

        assert!(handle_scroll(&mut tree, root, ScrollAction::LineRight(1)));
        assert_eq!(offsets(&tree).0, i32::from(KEY_STEP.0));
        assert_eq!(events.borrow().len(), 1);
    }

    #[test]
    fn wheel_step_scales_with_tick_count() {
        let (mut tree, _) = pan_tree(true, true);
        let root = tree.root;

        assert!(handle_scroll(&mut tree, root, ScrollAction::LineRight(3)));
        assert_eq!(offsets(&tree).0, i32::from(KEY_STEP.0) * 3);
    }

    #[test]
    fn wheel_releases_at_clamped_edge_so_it_can_bubble() {
        let (mut tree, _) = pan_tree(true, true);
        let root = tree.root;

        // Already at the top-left: vertical content fits, so there is no travel.
        assert!(!handle_scroll(&mut tree, root, ScrollAction::LineUp(1)));
        assert!(!handle_scroll(&mut tree, root, ScrollAction::LineLeft(1)));
        assert_eq!(offsets(&tree), (0, 0));

        // Drive right to the clamp, then confirm the next tick is released.
        for _ in 0..200 {
            if !handle_scroll(&mut tree, root, ScrollAction::LineRight(1)) {
                break;
            }
        }
        assert!(!handle_scroll(&mut tree, root, ScrollAction::LineRight(1)));
        assert!(offsets(&tree).0 > 0);
    }

    #[test]
    fn unclamped_canvas_keeps_consuming_the_wheel() {
        let (mut tree, _) = pan_tree(false, true);
        let root = tree.root;

        // No edge to reach, so an infinite surface never releases the wheel.
        for _ in 0..50 {
            assert!(handle_scroll(&mut tree, root, ScrollAction::LineUp(1)));
        }
        assert_eq!(offsets(&tree).1, -i32::from(KEY_STEP.1) * 50);
    }

    #[test]
    fn wheel_to_pan_false_releases_every_tick() {
        let (mut tree, events) = pan_tree(true, false);
        let root = tree.root;

        assert!(!handle_scroll(&mut tree, root, ScrollAction::LineRight(1)));
        assert_eq!(offsets(&tree), (0, 0));
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn arrow_key_still_consumes_at_the_edge() {
        let (mut tree, _) = pan_tree(true, true);
        let root = tree.root;

        // Unlike the wheel, a recognised pan key is consumed even with no travel.
        assert!(handle_key(
            &mut tree,
            root,
            &KeyEvent {
                code: KeyCode::Left,
                mods: KeyMods::default(),
            },
        ));
        assert_eq!(offsets(&tree), (0, 0));
    }
}
