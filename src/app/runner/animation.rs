#[cfg(feature = "image")]
use std::collections::hash_map::DefaultHasher;
#[cfg(feature = "image")]
use std::hash::{Hash, Hasher};
use std::time::Duration;
#[cfg(any(feature = "image", feature = "profiling-tracing"))]
use web_time::Instant;

#[cfg(feature = "image")]
use crate::backend::ratatui_backend::image_support;
use crate::core::component::Component;
use crate::core::node::NodeKind;
#[cfg(feature = "image")]
use crate::style::Rect;
use crate::widgets::{ScrollEvent, ScrollMetrics, ScrollWheelBehavior, calc_scroll_view_window};

use super::{AppRunner, spinner_frame_for_speed};

impl<C: Component> AppRunner<C> {
    pub(crate) fn update_spinner_frames(&mut self) {
        let spinner_ids = self.core.tree.spinner_ids().to_vec();

        for id in spinner_ids {
            match &mut self.core.tree.node_mut(id).kind {
                NodeKind::Spinner(node) if node.auto_frame => {
                    node.frame = spinner_frame_for_speed(self.animation.spinner_frame, node.speed);
                }
                NodeKind::DraggableTabBar(node) => {
                    for tab in std::sync::Arc::make_mut(&mut node.tabs) {
                        if let Some(spinner) = tab
                            .leading
                            .as_mut()
                            .and_then(|content| content.spinner_mut())
                            && spinner.auto_frame
                        {
                            spinner.spinner.frame = Some(spinner_frame_for_speed(
                                self.animation.spinner_frame,
                                spinner.spinner.speed,
                            ));
                        }
                    }
                }
                NodeKind::List(node) => {
                    for item in std::sync::Arc::make_mut(&mut node.items) {
                        if let Some(spinner) = item.status.as_mut().and_then(|s| s.spinner_mut())
                            && spinner.auto_frame
                        {
                            spinner.frame = spinner_frame_for_speed(
                                self.animation.spinner_frame,
                                spinner.speed,
                            );
                        }
                        if let Some(spinner) = item.gutter.as_mut().and_then(|g| g.spinner_mut())
                            && spinner.auto_frame
                        {
                            spinner.frame = spinner_frame_for_speed(
                                self.animation.spinner_frame,
                                spinner.speed,
                            );
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub(crate) fn update_animated_widgets(&mut self, dt: Duration) -> (bool, bool, bool) {
        let animated_ids = self.core.tree.animated_widget_ids().to_vec();

        let mut changed = false;
        let mut needs_paint = false;
        let mut needs_layout = false;

        for id in animated_ids {
            if let NodeKind::Animated(node) = &mut self.core.tree.node_mut(id).kind {
                let result = node.tick(dt);
                changed |= result.changed;
                needs_paint |= result.paint_dirty;
                needs_layout |= result.layout_dirty;
            }
        }

        self.core.tree.refresh_animated_widget_activity();

        (changed, needs_paint, needs_layout)
    }

    pub(crate) fn update_smooth_scrolls(&mut self, dt: Duration) -> (bool, bool, bool) {
        let animated_ids = self.core.tree.animated_scroll_ids().to_vec();

        let mut changed = false;
        let mut needs_paint = false;
        let mut needs_layout = false;

        for id in animated_ids {
            if !self.core.tree.is_valid(id) {
                continue;
            }

            let rect = self.core.tree.node(id).rect;
            let key = self.core.tree.node(id).key.clone();
            let mut scroll_view_wheel_event = None;
            match &mut self.core.tree.node_mut(id).kind {
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
                    let visible =
                        node.geometry.content_viewport_h(h_scrollbar_over_border) as usize;
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
                    self.core.tree.scroll_input_offset_by_key.insert(key, next);
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

        self.core.tree.refresh_animated_scroll_activity();

        (changed, needs_paint, needs_layout)
    }

    #[cfg(feature = "image")]
    pub(crate) fn update_image_frames(&mut self, delta_ms: u32) -> bool {
        if self.surface.is_inline() {
            return false;
        }

        let Some(viewport) = self.image_animation_viewport() else {
            return false;
        };

        let image_ids = self.core.tree.animated_image_ids().to_vec();

        let mut any_advanced = false;
        for id in image_ids {
            if self
                .core
                .tree
                .node(id)
                .rect
                .intersection(&viewport)
                .is_empty()
            {
                continue;
            }
            if let NodeKind::Image(node) = &mut self.core.tree.node_mut(id).kind
                && node.tick_animation(delta_ms)
            {
                any_advanced = true;
            }
        }

        any_advanced
    }

    #[cfg(feature = "image")]
    pub(crate) fn next_image_frame_due_in_ms(&self) -> Option<u32> {
        if self.surface.is_inline() {
            return None;
        }

        let viewport = self.image_animation_viewport()?;

        self.core
            .tree
            .animated_image_ids()
            .iter()
            .filter_map(|&id| {
                let node = self.core.tree.node(id);
                if node.rect.intersection(&viewport).is_empty() {
                    return None;
                }
                match &node.kind {
                    NodeKind::Image(image) => image.next_frame_due_in_ms(),
                    _ => None,
                }
            })
            .min()
    }

    #[cfg(feature = "image")]
    pub(crate) fn image_animations_suspended(&self) -> bool {
        self.animation
            .image_animation_suspend_until
            .is_some_and(|deadline| Instant::now() < deadline)
    }

    #[cfg(feature = "image")]
    pub(crate) fn suspend_image_animations_for(&mut self, duration: Duration) {
        let now = Instant::now();
        let requested_deadline = now + duration;
        self.animation.image_animation_suspend_until = Some(
            self.animation
                .image_animation_suspend_until
                .map(|current| current.max(requested_deadline))
                .unwrap_or(requested_deadline),
        );
        self.animation.last_image_tick = now;
    }

    #[cfg(feature = "image")]
    pub(crate) fn refresh_image_layout_suspension(&mut self) {
        if self.surface.is_inline() {
            self.animation.last_image_layout_hash = None;
            self.animation.image_animation_suspend_until = None;
            self.animation.last_image_tick = Instant::now();
            return;
        }

        let new_hash = self.animated_image_layout_hash();
        if self.animation.last_image_layout_hash != new_hash {
            self.animation.last_image_layout_hash = new_hash;
            if new_hash.is_some() {
                let pause = Duration::from_millis(super::image_layout_stabilize_ms() as u64);
                self.suspend_image_animations_for(pause);
                image_support::suspend_image_rendering_for(pause);
            } else {
                self.animation.image_animation_suspend_until = None;
                self.animation.last_image_tick = Instant::now();
            }
        }
    }

    #[cfg(feature = "image")]
    fn animated_image_layout_hash(&self) -> Option<u64> {
        let mut hasher = DefaultHasher::new();
        let mut found = false;

        for &id in self.core.tree.animated_image_ids() {
            let node = self.core.tree.node(id);
            let NodeKind::Image(image) = &node.kind else {
                continue;
            };
            found = true;
            image.source_hash.hash(&mut hasher);
            node.rect.hash(&mut hasher);
        }

        found.then_some(hasher.finish())
    }

    #[cfg(feature = "image")]
    fn image_animation_viewport(&self) -> Option<Rect> {
        if !self.core.tree.is_valid(self.core.tree.root) {
            return None;
        }
        Some(self.core.tree.node(self.core.tree.root).rect)
    }
}
