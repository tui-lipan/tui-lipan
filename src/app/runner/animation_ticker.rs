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
    fn clock_now(&self) -> Instant {
        self.core.ctx.env().now()
    }

    fn clock_elapsed(&self, since: Instant) -> Duration {
        self.core.ctx.env().elapsed(since)
    }

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
                poll_timeout = poll_timeout.min(self.frame_interval);
            }
        }

        // A child program writes whenever it likes and tells nobody, so every live terminal - not
        // only the focused one - has to be looked at on a cadence. This is the whole frame rate a
        // program drawing at video rates can reach through a pane, and the only cost a pane with
        // nothing to say imposes.
        #[cfg(feature = "terminal")]
        if self.core.tree.has_live_terminals() {
            poll_timeout = poll_timeout.min(self.frame_interval);
        }

        if self.stationary_drag_autoscroll_pending() {
            let interval = self.stationary_drag_autoscroll_interval();
            let until_due = self
                .drag
                .last_autoscroll_tick
                .map(|last| interval.saturating_sub(self.clock_elapsed(last)))
                .unwrap_or(Duration::ZERO);
            poll_timeout = poll_timeout.min(until_due);
        }

        #[cfg(feature = "devtools")]
        {
            const DEVTOOLS_IDLE_PAINT_INTERVAL: Duration = Duration::from_millis(250);

            if self.devtools_state.borrow().visible {
                let logs_active = self.devtools_state.borrow().is_logs_tab_active();
                LAST_DEVTOOLS_PAINT_TICK.with(|last_tick| {
                    let now = self.clock_now();
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
                    .saturating_sub(self.clock_elapsed(self.animation.last_spinner_tick)),
            );
        }

        if self.core.tree.has_animated_widgets()
            || self.core.tree.has_animated_scrolls()
            || self.core.ctx.env().animations.has_active()
        {
            poll_timeout = poll_timeout.min(
                self.frame_interval
                    .saturating_sub(self.clock_elapsed(self.animation.last_animated_tick)),
            );
        }

        if let Some(interval) = self.core.tree.animated_effect_scope_interval() {
            poll_timeout = poll_timeout
                .min(interval.saturating_sub(self.clock_elapsed(self.animation.last_effect_tick)));
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
                .saturating_sub(self.clock_elapsed(self.animation.last_image_tick));
            poll_timeout = poll_timeout.min(until_due);
        } else {
            // Avoid large catch-up jumps when playback is paused.
            self.animation.last_image_tick = self.clock_now();
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
                let blink_elapsed = self.clock_elapsed(self.animation.last_blink);
                if blink_elapsed >= Duration::from_millis(500) {
                    self.animation.blink_visible = !self.animation.blink_visible;
                    self.animation.last_blink = self.clock_now();
                    crate::debug::internal_log!("[tui-lipan] dirty: cursor blink toggle");
                    dirty.mark_paint();
                    poll_timeout = poll_timeout.min(Duration::from_millis(500));
                } else {
                    poll_timeout =
                        poll_timeout.min(Duration::from_millis(500).saturating_sub(blink_elapsed));
                }
            }
        }

        // A deferred chord reveal is the one piece of chord chrome no key event can schedule: the
        // chord went pending on a keystroke, and the frame that reveals it has to come from the
        // loop instead. Skipped entirely at the default zero delay, where the key dispatch that
        // set the chord already painted everything there is to paint.
        if !self
            .core
            .ctx
            .env()
            .command_chord_reveal_delay
            .get()
            .is_zero()
        {
            let revealed = self.core.ctx.command_chord_revealed();
            if revealed != self.animation.command_chord_revealed {
                self.animation.command_chord_revealed = revealed;
                crate::debug::internal_log!("[tui-lipan] dirty: command chord reveal");
                // `mark_full`, not `mark_paint`: revealing chord chrome means the view returns a
                // subtree it did not return before, and a paint-only frame redraws the existing
                // tree without ever re-running `view`. This fires once per chord, not per frame.
                dirty.mark_full();
            }
            if let Some(remaining) = self.core.ctx.env().command_chord_reveal_due_in() {
                poll_timeout = poll_timeout.min(remaining);
            }
        }

        let copy_feedback_requests = self.core.ctx.take_copy_feedback_requests();
        if !copy_feedback_requests.is_empty() {
            let duration = Duration::from_millis(
                self.core
                    .ctx
                    .env()
                    .clipboard_config
                    .copy_feedback_duration_ms as u64,
            );
            if !duration.is_zero() {
                for (id, range) in copy_feedback_requests {
                    if self.core.tree.is_valid(id) {
                        let range = range.and_then(|selection| {
                            crate::app::copy_feedback::capture_terminal_range(
                                &self.core.tree,
                                id,
                                selection,
                            )
                        });
                        self.copy_feedback.trigger_range(id, duration, range);
                        dirty.mark_paint();
                    }
                }
            }
        }

        let copy_feedback_tick = self.copy_feedback.tick_at(self.clock_now());
        if let Some(next_due) = copy_feedback_tick.next_due {
            poll_timeout = poll_timeout.min(next_due);
        }
        if copy_feedback_tick.needs_paint {
            crate::debug::internal_log!("[tui-lipan] dirty: copy feedback expired");
            dirty.mark_paint();
        }

        // Spinner tick every 50ms - only if spinners exist.
        if self.core.tree.has_spinners()
            && self.clock_elapsed(self.animation.last_spinner_tick) >= Duration::from_millis(50)
        {
            self.animation.spinner_frame = self.animation.spinner_frame.wrapping_add(1);
            self.animation.last_spinner_tick = self.clock_now();
            self.update_spinner_frames();
            crate::debug::internal_log!("[tui-lipan] dirty: spinner tick");
            dirty.mark_paint();
        }

        if (self.core.tree.has_animated_widgets()
            || self.core.tree.has_animated_scrolls()
            || self.core.ctx.env().animations.has_active())
            && self.clock_elapsed(self.animation.last_animated_tick) >= self.frame_interval
        {
            let dt = self.clock_elapsed(self.animation.last_animated_tick);
            self.animation.last_animated_tick = self.clock_now();
            // Wall-clock gaps (idle, first tick after startup) must not advance a full
            // transition in one step - Transition::tick clamps elapsed to duration.
            let dt = dt.min(Duration::from_millis(50));
            let (changed, needs_paint, needs_layout) =
                crate::app::animation::tick_tree_animations(&mut self.core.tree, dt);
            // Property-scoped transitions: advance and mark full re-render when
            // any interpolated value changed (the new value must flow through
            // the next view() into the rendered styles).
            let transitions = self.core.ctx.env().animations.tick(dt);
            if changed || transitions.view_changed || transitions.paint_changed {
                crate::debug::internal_log!("[tui-lipan] dirty: animated widget tick");
            }
            // A value a view read concretely has to flow through the next `view()` to reach the
            // rendered styles. A late-bound paint does not: the renderer resolves it while drawing,
            // so the whole fade costs repaints.
            if transitions.view_changed {
                dirty.mark_full();
            } else if needs_layout {
                dirty.mark_layout();
            } else if needs_paint || transitions.paint_changed {
                dirty.mark_paint();
            }
        }

        if let Some(interval) = self.core.tree.animated_effect_scope_interval()
            && self.clock_elapsed(self.animation.last_effect_tick) >= interval
        {
            self.animation.last_effect_tick = self.clock_now();
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
                .is_none_or(|last| self.clock_elapsed(last) >= interval);
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
            && self.clock_elapsed(self.animation.last_image_tick)
                >= Duration::from_millis(due_ms as u64)
        {
            let delta_ms = self
                .clock_elapsed(self.animation.last_image_tick)
                .as_millis();
            let delta_ms = delta_ms.min(u32::MAX as u128) as u32;
            let delta_ms = delta_ms.min(super::image_tick_catchup_cap_ms()).max(1);
            self.animation.last_image_tick = self.clock_now();

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

        if self.clock_elapsed(self.animation.last_overlay_tick) >= overlay_tick_interval {
            let tick_result = self
                .core
                .overlay_manager
                .borrow_mut()
                .tick_at(self.clock_now());
            if tick_result.dirty {
                crate::debug::internal_log!("[tui-lipan] dirty: overlay tick");
                dirty.mark_full();
            }
            self.animation.last_overlay_tick = self.clock_now();
        }

        {
            let overlay_manager = self.core.overlay_manager.borrow();
            if !overlay_manager.entries().is_empty() {
                let interval = if overlay_manager.has_active_transitions() {
                    Duration::from_millis(33)
                } else {
                    Duration::from_millis(100)
                };
                poll_timeout = poll_timeout.min(
                    interval.saturating_sub(self.clock_elapsed(self.animation.last_overlay_tick)),
                );
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
