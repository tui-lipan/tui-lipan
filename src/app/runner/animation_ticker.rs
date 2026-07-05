#[cfg(feature = "devtools")]
use std::cell::Cell;
use std::time::Duration;
use web_time::Instant;

#[cfg(feature = "image")]
use crate::backend::ratatui_backend::image_support;
use crate::core::component::Component;
use crate::core::node::NodeKind;

use super::{AppRunner, DirtyTracker};

#[cfg(feature = "devtools")]
thread_local! {
    static LAST_DEVTOOLS_PAINT_TICK: Cell<Option<Instant>> = const { Cell::new(None) };
}

impl<C: Component> AppRunner<C> {
    pub(super) fn update_animation_cycle(&mut self, dirty: &mut DirtyTracker) -> Duration {
        // Suppress framework-internal debug_log! entries (cursor blink, spinner
        // tick, etc.) from appearing in the devtools panel — they are not useful
        // to the application developer and would pollute the log view.
        #[cfg(feature = "devtools")]
        let _devtools_guard = crate::debug::suppress_devtools_log();

        // Base timeout keeps command/message responsiveness acceptable when idle.
        let mut poll_timeout = Duration::from_millis(50);

        // Detect image rendering suspension transitions: when suspension just
        // expired, trigger a repaint so images replace their placeholders.
        #[cfg(feature = "image")]
        {
            let currently_suspended = image_support::image_rendering_suspended();
            if self.animation.image_rendering_was_suspended && !currently_suspended {
                crate::debug::internal_log!(
                    "[tui-lipan] dirty: image rendering suspension expired"
                );
                dirty.mark_paint();
            }
            self.animation.image_rendering_was_suspended = currently_suspended;
            if currently_suspended {
                // Ensure the poll timeout is short enough to wake up when the
                // suspension expires so we can trigger the repaint promptly.
                poll_timeout = poll_timeout.min(Duration::from_millis(16));
            }
        }

        if let Some(id) = self.focus.focused
            && self.core.tree.is_valid(id)
            && {
                #[cfg(feature = "terminal")]
                {
                    matches!(self.core.tree.node(id).kind, NodeKind::Terminal(_))
                }
                #[cfg(not(feature = "terminal"))]
                {
                    false
                }
            }
        {
            poll_timeout = poll_timeout.min(Duration::from_millis(16));
        }

        if self.stationary_drag_autoscroll_pending() {
            let interval = self.stationary_drag_autoscroll_interval();
            let until_due = self
                .drag
                .last_autoscroll_tick
                .map(|last| interval.saturating_sub(last.elapsed()))
                .unwrap_or(Duration::ZERO);
            poll_timeout = poll_timeout.min(until_due);
        }

        #[cfg(feature = "devtools")]
        {
            const DEVTOOLS_IDLE_PAINT_INTERVAL: Duration = Duration::from_millis(250);

            if self.devtools_state.borrow().visible {
                let logs_active = self.devtools_state.borrow().is_logs_tab_active();
                LAST_DEVTOOLS_PAINT_TICK.with(|last_tick| {
                    let now = Instant::now();
                    match last_tick.get() {
                        Some(last) => {
                            let elapsed = now.saturating_duration_since(last);
                            if elapsed >= DEVTOOLS_IDLE_PAINT_INTERVAL {
                                if logs_active {
                                    // Logs tab needs reconciliation to pick up
                                    // new entries but we only do it on the idle
                                    // tick to avoid per-log flickering.
                                    dirty.mark_full();
                                } else {
                                    dirty.mark_paint();
                                }
                                last_tick.set(Some(now));
                            } else {
                                poll_timeout = poll_timeout
                                    .min(DEVTOOLS_IDLE_PAINT_INTERVAL.saturating_sub(elapsed));
                            }
                        }
                        None => {
                            last_tick.set(Some(now));
                        }
                    }
                });
            } else {
                LAST_DEVTOOLS_PAINT_TICK.with(|last_tick| {
                    last_tick.set(None);
                });
            }
        }

        if self.core.tree.has_spinners() {
            poll_timeout = poll_timeout.min(
                Duration::from_millis(50)
                    .saturating_sub(self.animation.last_spinner_tick.elapsed()),
            );
        }

        if self.core.tree.has_animated_widgets()
            || self.core.tree.has_animated_scrolls()
            || self.core.ctx.env().animations.has_active()
        {
            poll_timeout = poll_timeout.min(
                Duration::from_millis(16)
                    .saturating_sub(self.animation.last_animated_tick.elapsed()),
            );
        }

        if let Some(interval) = self.core.tree.animated_effect_scope_interval() {
            poll_timeout = poll_timeout
                .min(interval.saturating_sub(self.animation.last_effect_tick.elapsed()));
        }

        #[cfg(feature = "image")]
        let next_image_due_ms = {
            let image_animations_suspended = self.image_animations_suspended();
            if self.core.tree.has_animated_images() && !image_animations_suspended {
                self.next_image_frame_due_in_ms()
                    .map(|due| due.max(super::image_tick_floor_ms()))
            } else {
                None
            }
        };

        #[cfg(feature = "image")]
        if let Some(due_ms) = next_image_due_ms {
            let until_due = Duration::from_millis(due_ms as u64)
                .saturating_sub(self.animation.last_image_tick.elapsed());
            poll_timeout = poll_timeout.min(until_due);
        } else {
            // Avoid large catch-up jumps when playback is paused.
            self.animation.last_image_tick = Instant::now();
        }

        // Cursor blink: only tick when a blinking text widget is focused.
        if self.focus.window_focused {
            let has_blinking_cursor = if let Some(id) = self.focus.focused
                && self.core.tree.is_valid(id)
            {
                let node = self.core.tree.node(id);
                matches!(&node.kind, NodeKind::Input(n) if !n.read_only)
                    || matches!(&node.kind, NodeKind::TextArea(n) if !n.read_only)
                    || {
                        #[cfg(feature = "terminal")]
                        {
                            matches!(&node.kind, NodeKind::Terminal(n) if n.cursor_visible)
                        }
                        #[cfg(not(feature = "terminal"))]
                        {
                            false
                        }
                    }
            } else {
                false
            };

            if has_blinking_cursor {
                let blink_elapsed = self.animation.last_blink.elapsed();
                if blink_elapsed >= Duration::from_millis(500) {
                    self.animation.blink_visible = !self.animation.blink_visible;
                    self.animation.last_blink = Instant::now();
                    crate::debug::internal_log!("[tui-lipan] dirty: cursor blink toggle");
                    dirty.mark_paint();
                    poll_timeout = poll_timeout.min(Duration::from_millis(500));
                } else {
                    poll_timeout =
                        poll_timeout.min(Duration::from_millis(500).saturating_sub(blink_elapsed));
                }
            }
        }

        let copy_feedback_tick = self.copy_feedback.tick();
        if let Some(next_due) = copy_feedback_tick.next_due {
            poll_timeout = poll_timeout.min(next_due);
        }
        if copy_feedback_tick.needs_paint {
            crate::debug::internal_log!("[tui-lipan] dirty: copy feedback expired");
            dirty.mark_paint();
        }

        // Spinner tick every 50ms - only if spinners exist.
        if self.core.tree.has_spinners()
            && self.animation.last_spinner_tick.elapsed() >= Duration::from_millis(50)
        {
            self.animation.spinner_frame = self.animation.spinner_frame.wrapping_add(1);
            self.animation.last_spinner_tick = Instant::now();
            self.update_spinner_frames();
            crate::debug::internal_log!("[tui-lipan] dirty: spinner tick");
            dirty.mark_paint();
        }

        if (self.core.tree.has_animated_widgets()
            || self.core.tree.has_animated_scrolls()
            || self.core.ctx.env().animations.has_active())
            && self.animation.last_animated_tick.elapsed() >= Duration::from_millis(16)
        {
            let dt = self.animation.last_animated_tick.elapsed();
            self.animation.last_animated_tick = Instant::now();
            // Wall-clock gaps (idle, first tick after startup) must not advance a full
            // transition in one step - Transition::tick clamps elapsed to duration.
            let dt = dt.min(Duration::from_millis(50));
            let (changed, needs_paint, needs_layout) = self.update_animated_widgets(dt);
            let (scroll_changed, scroll_needs_paint, scroll_needs_layout) =
                self.update_smooth_scrolls(dt);
            // Property-scoped transitions: advance and mark full re-render when
            // any interpolated value changed (the new value must flow through
            // the next view() into the rendered styles).
            let property_transitions_changed = self.core.ctx.env().animations.tick(dt);
            if changed || scroll_changed || property_transitions_changed {
                crate::debug::internal_log!("[tui-lipan] dirty: animated widget tick");
            }
            if property_transitions_changed {
                dirty.mark_full();
            } else if needs_layout || scroll_needs_layout {
                dirty.mark_layout();
            } else if needs_paint || scroll_needs_paint {
                dirty.mark_paint();
            }
        }

        if let Some(interval) = self.core.tree.animated_effect_scope_interval()
            && self.animation.last_effect_tick.elapsed() >= interval
        {
            self.animation.last_effect_tick = Instant::now();
            self.animation.effect_phase_tick = self.animation.effect_phase_tick.wrapping_add(1);
            self.core.set_effect_phase(self.animation.effect_phase_tick);
            self.core.tree.refresh_animated_effect_scope_activity();
            if self.core.tree.has_animated_effect_scopes() {
                dirty.mark_paint();
            }
        }

        if self.stationary_drag_autoscroll_pending() {
            let interval = self.stationary_drag_autoscroll_interval();
            let due = self
                .drag
                .last_autoscroll_tick
                .is_none_or(|last| last.elapsed() >= interval);
            if due && self.tick_stationary_drag_autoscroll() {
                crate::debug::internal_log!("[tui-lipan] dirty: stationary drag autoscroll");
                if self.drag.autoscroll_layout_dirty {
                    dirty.mark_layout();
                } else {
                    dirty.mark_paint();
                }
            }
        }

        #[cfg(feature = "image")]
        if let Some(due_ms) = next_image_due_ms
            && self.animation.last_image_tick.elapsed() >= Duration::from_millis(due_ms as u64)
        {
            let delta_ms = self.animation.last_image_tick.elapsed().as_millis();
            let delta_ms = delta_ms.min(u32::MAX as u128) as u32;
            let delta_ms = delta_ms.min(super::image_tick_catchup_cap_ms()).max(1);
            self.animation.last_image_tick = Instant::now();

            if self.update_image_frames(delta_ms.max(1)) {
                crate::debug::internal_log!("[tui-lipan] dirty: image animation tick");
                dirty.mark_paint();
            }
        }

        let overlay_tick_interval = {
            let overlay_manager = self.core.overlay_manager.borrow();
            if overlay_manager.has_active_transitions() {
                Duration::from_millis(33)
            } else {
                Duration::from_millis(100)
            }
        };

        if self.animation.last_overlay_tick.elapsed() >= overlay_tick_interval {
            let tick_result = self.core.overlay_manager.borrow_mut().tick();
            if tick_result.dirty {
                crate::debug::internal_log!("[tui-lipan] dirty: overlay tick");
                dirty.mark_full();
            }
            self.animation.last_overlay_tick = Instant::now();
        }

        {
            let overlay_manager = self.core.overlay_manager.borrow();
            if !overlay_manager.entries().is_empty() {
                let interval = if overlay_manager.has_active_transitions() {
                    Duration::from_millis(33)
                } else {
                    Duration::from_millis(100)
                };
                poll_timeout = poll_timeout
                    .min(interval.saturating_sub(self.animation.last_overlay_tick.elapsed()));
            }
        }

        #[cfg(feature = "image")]
        {
            let protocol_epoch =
                crate::backend::ratatui_backend::renderers::image::image_protocol_ready_epoch();
            if protocol_epoch != self.animation.last_image_protocol_epoch {
                self.animation.last_image_protocol_epoch = protocol_epoch;
                crate::debug::internal_log!("[tui-lipan] dirty: image protocol ready");
                dirty.mark_paint();
            }
        }

        poll_timeout
    }
}
