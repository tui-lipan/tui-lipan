use std::time::Duration;

use super::Animated;
use crate::animation::{Easing, ExitAnimation, Transition};
use crate::callback::Callback;
use crate::core::node::{NodeKind, WidgetNode};
use crate::layout::axis::Axis;
use crate::style::Color;

#[derive(Clone)]
pub struct AnimatedNode {
    pub opacity: f32,
    pub opacity_fg_only: bool,
    pub opacity_target: Option<Color>,
    pub target_opacity: f32,
    pub opacity_anim: Option<Transition<f32>>,
    pub current_fg: Option<Color>,
    pub target_fg: Option<Color>,
    pub fg_anim: Option<Transition<Color>>,
    pub(crate) inherited_fg_exit: Option<InheritedColorExit>,
    pub current_bg: Option<Color>,
    pub target_bg: Option<Color>,
    pub bg_anim: Option<Transition<Color>>,
    pub(crate) inherited_bg_exit: Option<InheritedColorExit>,
    pub transition_easing: Easing,
    pub transition_duration: Duration,
    pub prev_width: Option<u16>,
    pub target_width: Option<u16>,
    pub width_anim: Option<Transition<f32>>,
    pub prev_height: Option<u16>,
    pub target_height: Option<u16>,
    pub height_anim: Option<Transition<f32>>,
    pub position_transition: bool,
    /// What this widget animates to when its keyed element disappears from its parent container.
    /// `None` opts out.
    pub auto_exit: Option<ExitAnimation>,
    /// Set once the unsupported-container diagnostic has been emitted for this node, so the
    /// warning does not repeat every frame. Debug builds only; always `false` in release.
    pub(crate) auto_exit_warned: bool,
    /// Set while an automatic exit is playing. This is what
    /// [`auto_exit_finished`](Self::auto_exit_finished) keys off, so a widget that merely animates
    /// to `opacity(0.0)` on its own is never mistaken for one that is leaving.
    pub(crate) auto_exit_active: bool,
    /// Suppress callbacks retained past the disposal of their owning component scope.
    pub(crate) callbacks_suppressed: bool,
    /// Which transitions the running exit started, so completion waits on those alone.
    ///
    /// An exit must not be held open by animation that has nothing to do with it: a color fade
    /// still settling from a hover, a movement the element was part of when it was removed. Those
    /// keep ticking and keep painting, but they do not gate release.
    pub(crate) exit_owned: ExitOwned,
    /// Render-only visual offset from `node.rect`. Do not consult from event,
    /// hit-test, layout, focus, or scroll code — those must use `node.rect` so
    /// FLIP movement stays paint-only.
    pub current_x_offset: f32,
    /// Render-only visual offset from `node.rect`. See `current_x_offset`.
    pub current_y_offset: f32,
    pub x_position_anim: Option<Transition<f32>>,
    pub y_position_anim: Option<Transition<f32>>,
    pub on_opacity_transition_end: Option<Callback<()>>,
    pub on_height_transition_end: Option<Callback<()>>,
    pub on_position_transition_end: Option<Callback<()>>,
}

/// Which transitions a running [`ExitAnimation`] started.
///
/// Release waits on these and only these. Anything else still in flight when the element was
/// removed keeps ticking and keeps painting, but an exit that declares nothing about a property
/// must not be held open by it.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ExitOwned {
    pub opacity: bool,
    pub width: bool,
    pub height: bool,
    pub fg: bool,
    pub bg: bool,
    pub position: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct InheritedColorExit {
    pub target: Color,
    pub progress: Transition<f32>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AnimatedTickResult {
    pub changed: bool,
    pub paint_dirty: bool,
    pub layout_dirty: bool,
    pub still_animating: bool,
}

impl AnimatedNode {
    /// Begin the automatic exit animation, returning `false` when this node did not opt in.
    ///
    /// The widget's [`ExitAnimation`] says what to animate. `collapse_from` is separate because
    /// the main-axis collapse is the container's decision, not the widget's:
    ///
    /// - **Reflowing containers** always pass the width (`HStack`) or height (`VStack`) the node
    ///   actually occupies right now, taken from its laid-out rectangle. The collapse is what the
    ///   substituted spacer measures, so siblings reflow into the vacated space. It must not come
    ///   from a layout prop: `Length::Auto` resolves to the child's *natural* size, which for a
    ///   filled child can be far short of its real one, and the exit would visibly jump before it
    ///   started.
    /// - **Positioned containers** pass `Some(height)` only when the exit asked for it. Nothing
    ///   reflows around them, so the collapse is purely an effect, and it is opt-in because the
    ///   subtree is clipped rather than re-laid out: the content stays full size inside the
    ///   shrinking box and its bottom edge, border included, is cut away rather than travelling up.
    ///
    /// Idempotent: calling it again while the exit is already running leaves the in-flight
    /// transitions untouched so the animation is not restarted every frame.
    pub(crate) fn begin_auto_exit(&mut self, collapse_from: Option<(Axis, u16)>) -> bool {
        let Some(exit) = self.auto_exit else {
            return false;
        };
        if self.auto_exit_active {
            return true;
        }
        self.auto_exit_active = true;

        let duration = exit.duration;
        let easing = exit.easing.unwrap_or(self.transition_easing);
        let instant = duration.is_zero();
        let mut owned = ExitOwned::default();

        if let Some(target) = exit.opacity {
            self.target_opacity = target;
            if instant {
                self.opacity = target;
                self.opacity_anim = None;
            } else {
                self.opacity_anim = Some(Transition::new(self.opacity, target, duration, easing));
                owned.opacity = true;
            }
        }

        if let Some(target) = exit.fg {
            self.target_fg = Some(target);
            if instant {
                self.current_fg = Some(target);
                self.fg_anim = None;
            } else if let Some(start) = self.current_fg {
                self.fg_anim = Some(Transition::new(start, target, duration, easing));
                owned.fg = true;
            } else {
                self.inherited_fg_exit = Some(InheritedColorExit {
                    target,
                    progress: Transition::new(0.0, 1.0, duration, easing),
                });
                owned.fg = true;
            }
        }

        if let Some(target) = exit.bg {
            self.target_bg = Some(target);
            if instant {
                self.current_bg = Some(target);
                self.bg_anim = None;
            } else if let Some(start) = self.current_bg {
                self.bg_anim = Some(Transition::new(start, target, duration, easing));
                owned.bg = true;
            } else {
                self.inherited_bg_exit = Some(InheritedColorExit {
                    target,
                    progress: Transition::new(0.0, 1.0, duration, easing),
                });
                owned.bg = true;
            }
        }

        if let Some((dx, dy)) = exit.offset {
            // Translation is relative to the *removal snapshot*: wherever the element had actually
            // reached, not its layout rectangle. A row removed mid-reorder still carries a movement
            // offset, and restarting from zero would snap it back before sliding it out. Taking the
            // current offset as the baseline and dropping the in-flight transitions absorbs the
            // remainder of that move instead.
            let base_x = self.current_x_offset;
            let base_y = self.current_y_offset;
            let target_x = base_x + f32::from(dx);
            let target_y = base_y + f32::from(dy);
            self.x_position_anim = None;
            self.y_position_anim = None;
            if instant {
                self.current_x_offset = target_x;
                self.current_y_offset = target_y;
            } else {
                self.x_position_anim = Some(Transition::new(base_x, target_x, duration, easing));
                self.y_position_anim = Some(Transition::new(base_y, target_y, duration, easing));
                owned.position = true;
            }
        }

        if let Some((axis, from)) = collapse_from {
            match axis {
                Axis::Horizontal => {
                    self.target_width = Some(0);
                    if instant {
                        self.prev_width = Some(0);
                        self.width_anim = None;
                    } else {
                        self.prev_width = Some(from);
                        self.width_anim = Some(Transition::new(from as f32, 0.0, duration, easing));
                        owned.width = true;
                    }
                }
                Axis::Vertical => {
                    self.target_height = Some(0);
                    if instant {
                        self.prev_height = Some(0);
                        self.height_anim = None;
                    } else {
                        self.prev_height = Some(from);
                        self.height_anim =
                            Some(Transition::new(from as f32, 0.0, duration, easing));
                        owned.height = true;
                    }
                }
            }
        }

        self.exit_owned = owned;
        true
    }

    /// Height this node currently occupies while collapsing. Only meaningful for an exit that was
    /// started with a `collapse_from`; an exit that does not collapse keeps its rectangle.
    pub(crate) fn auto_exit_height(&self) -> u16 {
        self.height_anim
            .as_ref()
            .map(|transition| transition.current().round().max(0.0) as u16)
            .or(self.prev_height)
            .unwrap_or(0)
    }

    /// Width this node currently occupies during an `HStack` exit.
    pub(crate) fn auto_exit_width(&self) -> u16 {
        self.width_anim
            .as_ref()
            .map(|transition| transition.current().round().max(0.0) as u16)
            .or(self.prev_width)
            .unwrap_or(0)
    }

    /// Whether every transition the exit started has finished, so the subtree can be released.
    ///
    /// Deliberately not "nothing is animating". Unrelated animation still settling when the
    /// element was removed would otherwise extend retention past the exit it has nothing to do
    /// with, and an exit that declares no properties at all (a bare retention window) would never
    /// release while any of it ran.
    pub(crate) fn auto_exit_finished(&self) -> bool {
        self.auto_exit_active && !self.exit_animations_running()
    }

    fn exit_animations_running(&self) -> bool {
        let owned = self.exit_owned;
        (owned.opacity && self.opacity_anim.is_some())
            || (owned.width && self.width_anim.is_some())
            || (owned.height && self.height_anim.is_some())
            || (owned.fg && (self.fg_anim.is_some() || self.inherited_fg_exit.is_some()))
            || (owned.bg && (self.bg_anim.is_some() || self.inherited_bg_exit.is_some()))
            || (owned.position && self.position_is_animating())
    }

    /// Clear the exit so the node can be reconciled normally again, for a key the application
    /// described a second time before the animation ended.
    pub(crate) fn cancel_auto_exit(&mut self) {
        self.auto_exit_active = false;
        self.callbacks_suppressed = false;
        self.inherited_fg_exit = None;
        self.inherited_bg_exit = None;
        self.exit_owned = ExitOwned::default();
    }

    pub fn is_animating(&self) -> bool {
        self.opacity_anim.is_some()
            || self.fg_anim.is_some()
            || self.bg_anim.is_some()
            || self.inherited_fg_exit.is_some()
            || self.inherited_bg_exit.is_some()
            || self.width_anim.is_some()
            || self.height_anim.is_some()
            || self.position_is_animating()
    }

    fn position_is_animating(&self) -> bool {
        self.x_position_anim.is_some() || self.y_position_anim.is_some()
    }

    pub(crate) fn visual_position_offset_cells(&self) -> (i16, i16) {
        (
            offset_to_i16_cells(self.current_x_offset),
            offset_to_i16_cells(self.current_y_offset),
        )
    }

    pub fn current_visible_height(&self, fallback: u16) -> u16 {
        self.height_anim
            .as_ref()
            .map(|transition| transition.current().round().max(0.0) as u16)
            .or(self.prev_height)
            .or(self.target_height)
            .unwrap_or(fallback)
    }

    pub fn tick(&mut self, dt: Duration) -> AnimatedTickResult {
        let mut result = AnimatedTickResult::default();
        // A retained subtree's component scope was disposed on the frame it stopped being
        // described, so no transition-end callback may fire: it would run into a dropped scope.
        let notify = !self.callbacks_suppressed;

        if let Some(transition) = &mut self.opacity_anim {
            transition.tick(dt);
            let value = transition.current().clamp(0.0, 1.0);
            if (value - self.opacity).abs() > f32::EPSILON {
                self.opacity = value;
                result.changed = true;
                result.paint_dirty = true;
            }
            if transition.is_complete() {
                self.opacity = self.target_opacity;
                self.opacity_anim = None;
                if let Some(cb) = &self.on_opacity_transition_end
                    && notify
                {
                    cb.emit(());
                }
            }
        }

        if let Some(transition) = &mut self.height_anim {
            transition.tick(dt);
            let value = transition.current().round().max(0.0) as u16;
            if self.prev_height != Some(value) {
                self.prev_height = Some(value);
                result.changed = true;
                result.layout_dirty = true;
            }
            if transition.is_complete() {
                self.prev_height = self.target_height;
                self.height_anim = None;
                if let Some(cb) = &self.on_height_transition_end
                    && notify
                {
                    cb.emit(());
                }
            }
        }

        if let Some(transition) = &mut self.width_anim {
            transition.tick(dt);
            let value = transition.current().round().max(0.0) as u16;
            if self.prev_width != Some(value) {
                self.prev_width = Some(value);
                result.changed = true;
                result.layout_dirty = true;
            }
            if transition.is_complete() {
                self.prev_width = self.target_width;
                self.width_anim = None;
            }
        }

        if let Some(transition) = &mut self.fg_anim {
            transition.tick(dt);
            let value = transition.current();
            if self.current_fg != Some(value) {
                self.current_fg = Some(value);
                result.changed = true;
                result.paint_dirty = true;
            }
            if transition.is_complete() {
                self.current_fg = self.target_fg;
                self.fg_anim = None;
            }
        }

        if let Some(transition) = &mut self.bg_anim {
            transition.tick(dt);
            let value = transition.current();
            if self.current_bg != Some(value) {
                self.current_bg = Some(value);
                result.changed = true;
                result.paint_dirty = true;
            }
            if transition.is_complete() {
                self.current_bg = self.target_bg;
                self.bg_anim = None;
            }
        }

        if let Some(exit) = &mut self.inherited_fg_exit {
            exit.progress.tick(dt);
            result.changed = true;
            result.paint_dirty = true;
            if exit.progress.is_complete() {
                self.current_fg = Some(exit.target);
                self.inherited_fg_exit = None;
            }
        }

        if let Some(exit) = &mut self.inherited_bg_exit {
            exit.progress.tick(dt);
            result.changed = true;
            result.paint_dirty = true;
            if exit.progress.is_complete() {
                self.current_bg = Some(exit.target);
                self.inherited_bg_exit = None;
            }
        }

        // An exit translation moves *away* from the layout rectangle and settles there. The FLIP
        // machinery below does the opposite: it caps late-phase wobble toward zero and snaps the
        // offset to zero on completion, because a move ends at the rect it animated into. Applying
        // either to an exit would drag the element back as it left.
        let exit_translating = self.exit_owned.position;
        let was_position_animating = self.position_is_animating();
        if was_position_animating {
            let old_cells = self.visual_position_offset_cells();
            let old_x = self.current_x_offset;
            let old_y = self.current_y_offset;

            if let Some(transition) = &mut self.x_position_anim {
                transition.tick(dt);
                let progress = transition.progress();
                self.current_x_offset = if exit_translating {
                    transition.current()
                } else {
                    cap_position_offset_late_phase(transition.current(), old_x, progress)
                };
                if transition.is_complete() {
                    self.x_position_anim = None;
                }
            }

            if let Some(transition) = &mut self.y_position_anim {
                transition.tick(dt);
                let progress = transition.progress();
                self.current_y_offset = if exit_translating {
                    transition.current()
                } else {
                    cap_position_offset_late_phase(transition.current(), old_y, progress)
                };
                if transition.is_complete() {
                    self.y_position_anim = None;
                }
            }

            let position_completed = !self.position_is_animating();
            if position_completed && !exit_translating {
                self.current_x_offset = 0.0;
                self.current_y_offset = 0.0;
            }

            if (self.current_x_offset - old_x).abs() > f32::EPSILON
                || (self.current_y_offset - old_y).abs() > f32::EPSILON
                || self.visual_position_offset_cells() != old_cells
            {
                result.changed = true;
                result.paint_dirty = true;
            }

            if position_completed
                && notify
                && let Some(cb) = &self.on_position_transition_end
            {
                cb.emit(());
            }
        }

        result.still_animating = self.is_animating();
        result
    }
}

/// Late-phase wobble cap for position transitions.
///
/// Why: overshoot easings like `EaseOutElastic` oscillate around the target.
/// Sub-cell wobbles are rounded to integer cell offsets and read as 1-cell
/// jitter at the destination. After 70% of the transition duration, force
/// monotonic decay toward zero — same-sign as the previous offset, magnitude
/// non-increasing — so the destination settles cleanly while early overshoot
/// still renders.
fn cap_position_offset_late_phase(next: f32, prev: f32, progress: f32) -> f32 {
    if progress < 0.7 {
        return next;
    }
    let prev_sign = prev.signum();
    if prev == 0.0 {
        return 0.0;
    }
    if next.signum() != prev_sign {
        return 0.0;
    }
    if next.abs() > prev.abs() {
        return prev;
    }
    next
}

fn offset_to_i16_cells(offset: f32) -> i16 {
    if !offset.is_finite() {
        return 0;
    }
    offset.round().clamp(i16::MIN as f32, i16::MAX as f32) as i16
}

impl WidgetNode for AnimatedNode {}

impl From<Animated> for AnimatedNode {
    fn from(value: Animated) -> Self {
        Self {
            opacity: value.opacity,
            opacity_fg_only: value.opacity_fg_only,
            opacity_target: value.opacity_target,
            auto_exit: value.auto_exit,
            auto_exit_warned: false,
            auto_exit_active: false,
            callbacks_suppressed: false,
            exit_owned: ExitOwned::default(),
            target_opacity: value.opacity,
            opacity_anim: None,
            current_fg: value.fg,
            target_fg: value.fg,
            fg_anim: None,
            inherited_fg_exit: None,
            current_bg: value.bg,
            target_bg: value.bg,
            bg_anim: None,
            inherited_bg_exit: None,
            transition_easing: value.transition.easing,
            transition_duration: value.transition.duration,
            prev_width: None,
            target_width: None,
            width_anim: None,
            prev_height: None,
            target_height: None,
            height_anim: None,
            position_transition: value.position_transition,
            current_x_offset: 0.0,
            current_y_offset: 0.0,
            x_position_anim: None,
            y_position_anim: None,
            on_opacity_transition_end: value.on_opacity_transition_end,
            on_height_transition_end: value.on_height_transition_end,
            on_position_transition_end: value.on_position_transition_end,
        }
    }
}

impl From<AnimatedNode> for NodeKind {
    fn from(node: AnimatedNode) -> Self {
        NodeKind::Animated(node)
    }
}
