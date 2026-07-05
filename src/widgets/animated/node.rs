use std::time::Duration;

use super::Animated;
use crate::animation::{Easing, Transition};
use crate::callback::Callback;
use crate::core::node::{NodeKind, WidgetNode};
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
    pub current_bg: Option<Color>,
    pub target_bg: Option<Color>,
    pub bg_anim: Option<Transition<Color>>,
    pub transition_easing: Easing,
    pub transition_duration: Duration,
    pub prev_height: Option<u16>,
    pub target_height: Option<u16>,
    pub height_anim: Option<Transition<f32>>,
    pub position_transition: bool,
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

#[derive(Clone, Copy, Debug, Default)]
pub struct AnimatedTickResult {
    pub changed: bool,
    pub paint_dirty: bool,
    pub layout_dirty: bool,
    pub still_animating: bool,
}

impl AnimatedNode {
    pub fn is_animating(&self) -> bool {
        self.opacity_anim.is_some()
            || self.fg_anim.is_some()
            || self.bg_anim.is_some()
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
                if let Some(cb) = &self.on_opacity_transition_end {
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
                if let Some(cb) = &self.on_height_transition_end {
                    cb.emit(());
                }
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

        let was_position_animating = self.position_is_animating();
        if was_position_animating {
            let old_cells = self.visual_position_offset_cells();
            let old_x = self.current_x_offset;
            let old_y = self.current_y_offset;

            if let Some(transition) = &mut self.x_position_anim {
                transition.tick(dt);
                let progress = transition.progress();
                self.current_x_offset =
                    cap_position_offset_late_phase(transition.current(), old_x, progress);
                if transition.is_complete() {
                    self.x_position_anim = None;
                }
            }

            if let Some(transition) = &mut self.y_position_anim {
                transition.tick(dt);
                let progress = transition.progress();
                self.current_y_offset =
                    cap_position_offset_late_phase(transition.current(), old_y, progress);
                if transition.is_complete() {
                    self.y_position_anim = None;
                }
            }

            let position_completed = !self.position_is_animating();
            if position_completed {
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

            if position_completed && let Some(cb) = &self.on_position_transition_end {
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
            target_opacity: value.opacity,
            opacity_anim: None,
            current_fg: value.fg,
            target_fg: value.fg,
            fg_anim: None,
            current_bg: value.bg,
            target_bg: value.bg,
            bg_anim: None,
            transition_easing: value.transition.easing,
            transition_duration: value.transition.duration,
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
