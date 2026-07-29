use std::time::Duration;

use crate::core::node::{NodeKind, NodeTree};
use crate::widgets::{ScrollEvent, ScrollMetrics, ScrollWheelBehavior, calc_scroll_view_window};

pub(crate) fn tick_tree_animations(tree: &mut NodeTree, dt: Duration) -> (bool, bool, bool) {
    let (changed, needs_paint, needs_layout) = tick_animated_widgets(tree, dt);
    let (scroll_changed, scroll_needs_paint, scroll_needs_layout) = tick_smooth_scrolls(tree, dt);
    (
        changed || scroll_changed,
        needs_paint || scroll_needs_paint,
        needs_layout || scroll_needs_layout,
    )
}

pub(crate) fn tick_animated_widgets(tree: &mut NodeTree, dt: Duration) -> (bool, bool, bool) {
    let animated_ids = tree.animated_widget_ids().to_vec();

    let mut changed = false;
    let mut needs_paint = false;
    let mut needs_layout = false;

    for id in animated_ids {
        if let NodeKind::Animated(node) = &mut tree.node_mut(id).kind {
            let result = node.tick(dt);
            changed |= result.changed;
            needs_paint |= result.paint_dirty;
            needs_layout |= result.layout_dirty;
        }
    }

    tree.refresh_animated_widget_activity();

    (changed, needs_paint, needs_layout)
}

pub(crate) fn tick_smooth_scrolls(tree: &mut NodeTree, dt: Duration) -> (bool, bool, bool) {
    let animated_ids = tree.animated_scroll_ids().to_vec();

    let mut changed = false;
    let mut needs_paint = false;
    let mut needs_layout = false;

    for id in animated_ids {
        if !tree.is_valid(id) {
            continue;
        }

        let rect = tree.node(id).rect;
        let key = tree.node(id).key.clone();
        let mut scroll_view_wheel_event = None;
        match &mut tree.node_mut(id).kind {
            NodeKind::ScrollView(node) => {
                let result = node.smooth_scroll.tick(dt, node.max_offset);
                let _still_animating = result.still_animating;
                if result.changed {
                    let next = node.smooth_scroll.current_offset(node.max_offset);
                    node.offset = next;
                    node.scroll_offset = next.min(u16::MAX as usize) as u16;
                    node.scroll_override = Some(next);
                    changed = true;
                    needs_layout = true;
                }

                if !node.scroll_wheel
                    || matches!(node.scroll_wheel_behavior, ScrollWheelBehavior::Immediate)
                {
                    if node.wheel_scroll.is_animating() {
                        node.wheel_scroll.cancel_at(node.offset);
                    }
                } else if let ScrollWheelBehavior::Smooth(config) = node.scroll_wheel_behavior {
                    let result = node.wheel_scroll.tick(dt, node.max_offset, config);
                    let _still_animating = result.still_animating;
                    if result.changed {
                        let next = node.wheel_scroll.current_offset(node.max_offset);
                        node.offset = next;
                        node.scroll_offset = next.min(u16::MAX as usize) as u16;
                        node.scroll_override = Some(next);
                        node.scroll_handler_dirty = true;
                        let window = calc_scroll_view_window(
                            next,
                            node.content_height as usize,
                            node.viewport_height as usize,
                            node.show_scroll_indicators,
                        );
                        scroll_view_wheel_event = Some((
                            next,
                            ScrollMetrics {
                                len: node.content_height as usize,
                                visible: window.visible_rows,
                                max_offset: window.max_offset,
                            },
                            node.on_scroll_to.clone(),
                            node.on_scroll.clone(),
                        ));
                        changed = true;
                        needs_layout = true;
                    }
                }
            }
            NodeKind::DocumentView(node) => {
                let inner = rect.inner(node.border, node.padding);
                let visible = node.content_layout(inner).content_height as usize;
                let max_offset = node.total_visual_lines.saturating_sub(visible);
                let result = node.smooth_scroll.tick(dt, max_offset);
                let _still_animating = result.still_animating;
                if result.changed {
                    let next = node.smooth_scroll.current_offset(max_offset);
                    node.scroll_offset = next;
                    node.scroll_override = Some(next);
                    changed = true;
                    needs_paint = true;
                }
            }
            NodeKind::TextArea(node) => {
                let h_scrollbar_over_border = node.h_scrollbar
                    && matches!(
                        node.h_scrollbar_variant,
                        crate::style::ScrollbarVariant::Integrated
                    )
                    && node.border;
                let visible = node.geometry.content_viewport_h(h_scrollbar_over_border) as usize;
                let max_offset = node.visual_lines_count.saturating_sub(visible);
                let result = node.smooth_scroll.tick(dt, max_offset);
                let _still_animating = result.still_animating;
                if result.changed {
                    let next = node.smooth_scroll.current_offset(max_offset);
                    node.scroll_offset = next;
                    node.scroll_override = Some(next);
                    changed = true;
                    needs_paint = true;
                }
            }
            _ => {}
        }

        if let Some((next, metrics, on_scroll_to, on_scroll)) = scroll_view_wheel_event {
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
        }
    }

    tree.refresh_animated_scroll_activity();

    (changed, needs_paint, needs_layout)
}
