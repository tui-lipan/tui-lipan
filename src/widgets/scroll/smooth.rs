use std::time::Duration;

use super::behavior::ScrollBehavior;
use crate::animation::Transition;

const KINETIC_LAUNCH_MAX_STEP_ROWS: f32 = 4.0;
const KINETIC_SETTLE_NEXT_ROW_WAIT_SECS: f32 = 0.06;

/// Runtime state for inertial wheel scrolling.
#[derive(Clone, Debug, Default)]
pub struct KineticScrollState {
    position: f32,
    velocity: f32,
    queued_distance: f32,
    limit_next_step: bool,
}

/// Result of advancing one kinetic scroll frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct KineticScrollTickResult {
    pub changed: bool,
    pub still_animating: bool,
}

impl KineticScrollState {
    pub(crate) fn is_animating(&self) -> bool {
        self.velocity != 0.0 || self.queued_distance != 0.0
    }

    pub(crate) fn current_offset(&self, max_offset: usize) -> usize {
        self.position.round().clamp(0.0, max_offset as f32) as usize
    }

    pub(crate) fn cancel_at(&mut self, offset: usize) {
        self.position = offset as f32;
        self.velocity = 0.0;
        self.queued_distance = 0.0;
        self.limit_next_step = false;
    }

    pub(crate) fn apply_impulse(
        &mut self,
        current_offset: usize,
        delta_lines: isize,
        max_offset: usize,
        config: super::ScrollWheelConfig,
    ) -> usize {
        let current = current_offset.min(max_offset);
        if max_offset == 0 || delta_lines == 0 {
            self.cancel_at(current);
            return current;
        }

        let was_animating = self.is_animating();
        if !was_animating {
            self.position = current as f32;
        } else {
            self.position = self.position.clamp(0.0, max_offset as f32);
        }
        self.limit_next_step |= !was_animating;

        let impulse_direction = (delta_lines as f32).signum();
        if self.queued_distance.signum() != 0.0
            && self.queued_distance.signum() != impulse_direction
        {
            self.queued_distance = 0.0;
        }

        let acceleration = finite_positive(config.acceleration, 40.0);
        let deceleration = finite_positive(config.deceleration, 12.0);
        let max_velocity = finite_positive(config.max_velocity, 320.0);
        let target_velocity = if self.velocity != 0.0 && self.velocity.signum() != impulse_direction
        {
            delta_lines as f32 * acceleration
        } else {
            self.velocity + (delta_lines as f32 * acceleration)
        };

        let overflow_velocity = (target_velocity.abs() - max_velocity).max(0.0);
        if overflow_velocity > 0.0 {
            self.queued_distance = (self.queued_distance
                + impulse_direction * overflow_velocity / deceleration)
                .clamp(-(max_offset as f32), max_offset as f32);
        }

        self.velocity = target_velocity.clamp(-max_velocity, max_velocity);

        if (self.position <= 0.0 && self.velocity < 0.0)
            || (self.position >= max_offset as f32 && self.velocity > 0.0)
        {
            self.velocity = 0.0;
            self.queued_distance = 0.0;
            self.limit_next_step = false;
        }

        self.current_offset(max_offset)
    }

    pub(crate) fn tick(
        &mut self,
        dt: Duration,
        max_offset: usize,
        config: super::ScrollWheelConfig,
    ) -> KineticScrollTickResult {
        if !self.is_animating() {
            self.position = self.position.clamp(0.0, max_offset as f32);
            return KineticScrollTickResult::default();
        }

        let before = self.current_offset(max_offset);
        if max_offset == 0 {
            self.cancel_at(0);
            return KineticScrollTickResult {
                changed: before != 0,
                still_animating: false,
            };
        }

        let dt_secs = dt.as_secs_f32().clamp(0.0, 0.05);
        let spending_queue = self.queued_distance != 0.0;
        if spending_queue {
            let max_velocity = finite_positive(config.max_velocity, 320.0);
            self.velocity = self.queued_distance.signum() * max_velocity;
        }

        let mut step = self.velocity * dt_secs;
        if self.limit_next_step {
            self.limit_next_step = false;
            let max_launch_step = KINETIC_LAUNCH_MAX_STEP_ROWS.min(max_offset as f32).max(1.0);
            if step.abs() > max_launch_step {
                step = step.signum() * max_launch_step;
            }
        }
        self.position += step;
        if spending_queue && self.queued_distance.signum() == step.signum() {
            self.queued_distance -= step;
            if self.queued_distance.signum() != step.signum() || self.queued_distance.abs() < 0.001
            {
                self.queued_distance = 0.0;
            }
        }

        if self.position <= 0.0 {
            self.position = 0.0;
            if self.velocity < 0.0 {
                self.velocity = 0.0;
                self.queued_distance = 0.0;
                self.limit_next_step = false;
            }
        } else if self.position >= max_offset as f32 {
            self.position = max_offset as f32;
            if self.velocity > 0.0 {
                self.velocity = 0.0;
                self.queued_distance = 0.0;
                self.limit_next_step = false;
            }
        }

        if self.queued_distance == 0.0 {
            let deceleration = finite_positive(config.deceleration, 12.0);
            self.velocity *= (-deceleration * dt_secs).exp();
        }

        let stop_velocity = finite_positive(config.stop_velocity, 0.05);
        if self.queued_distance == 0.0
            && (self.velocity.abs() <= stop_velocity
                || should_settle_at_visible_row(
                    self.position,
                    self.velocity,
                    max_offset,
                    KINETIC_SETTLE_NEXT_ROW_WAIT_SECS,
                ))
        {
            let settled = self.current_offset(max_offset);
            self.cancel_at(settled);
        }

        let after = self.current_offset(max_offset);
        KineticScrollTickResult {
            changed: before != after,
            still_animating: self.is_animating(),
        }
    }
}

fn finite_positive(value: f32, fallback: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        fallback
    }
}

fn should_settle_at_visible_row(
    position: f32,
    velocity: f32,
    max_offset: usize,
    max_wait_secs: f32,
) -> bool {
    if !position.is_finite() || !velocity.is_finite() || velocity == 0.0 {
        return true;
    }

    let current = position.round().clamp(0.0, max_offset as f32);
    let distance_to_next_row = if velocity > 0.0 {
        if current >= max_offset as f32 {
            return true;
        }
        (current + 0.5 - position).max(0.0)
    } else {
        if current <= 0.0 {
            return true;
        }
        (position - (current - 0.5)).max(0.0)
    };

    distance_to_next_row / velocity.abs() > max_wait_secs
}

/// Internal row-offset state for opt-in smooth target scrolling.
#[derive(Clone, Debug, Default)]
pub(crate) struct SmoothScrollState {
    transition: Option<Transition<f32>>,
    current: f32,
    target: Option<usize>,
}

/// Result of advancing one smooth-scroll transition frame.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct SmoothScrollTickResult {
    pub changed: bool,
    pub still_animating: bool,
}

impl SmoothScrollState {
    /// Returns true while a target scroll transition is active.
    pub(crate) fn is_animating(&self) -> bool {
        self.transition.is_some()
    }

    /// Resolve a semantic target into the row that should be displayed now.
    pub(crate) fn resolve_target(
        &mut self,
        current_offset: usize,
        target_offset: usize,
        max_offset: usize,
        behavior: ScrollBehavior,
    ) -> usize {
        let target = target_offset.min(max_offset);
        let current = current_offset.min(max_offset) as f32;

        if self.transition.is_none() {
            self.current = current;
        } else {
            self.current = self.clamped_transition_current(max_offset);
        }
        self.current = self.current.clamp(0.0, max_offset as f32);

        let distance = self.current_offset(max_offset).abs_diff(target);
        let Some(config) = behavior.transition_config_for_distance(distance) else {
            self.cancel_at(target);
            self.target = Some(target);
            return target;
        };
        if config.duration == Duration::ZERO {
            self.cancel_at(target);
            self.target = Some(target);
            return target;
        }

        if self.target != Some(target) {
            self.target = Some(target);
            if (self.current - target as f32).abs() <= f32::EPSILON {
                self.transition = None;
                self.current = target as f32;
                return target;
            }
            self.transition = Some(Transition::new(
                self.current,
                target as f32,
                config.duration,
                config.easing,
            ));
        } else if self.transition.is_none()
            && (self.current.round() as usize).min(max_offset) != target
        {
            self.transition = Some(Transition::new(
                self.current,
                target as f32,
                config.duration,
                config.easing,
            ));
        }

        self.current_offset(max_offset)
    }

    /// Advance the active transition, if any.
    pub(crate) fn tick(&mut self, dt: Duration, max_offset: usize) -> SmoothScrollTickResult {
        let Some(mut transition) = self.transition.take() else {
            self.current = self.current.clamp(0.0, max_offset as f32);
            self.target = self.target.map(|target| target.min(max_offset));
            return SmoothScrollTickResult::default();
        };

        let before = self.current_offset(max_offset);
        let complete = transition.tick(dt);
        self.current = clamped_transition_current(&transition, max_offset);
        self.target = self.target.map(|target| target.min(max_offset));

        if complete {
            if let Some(target) = self.target {
                self.current = target.min(max_offset) as f32;
            }
        } else {
            self.transition = Some(transition);
        }

        let after = self.current_offset(max_offset);
        SmoothScrollTickResult {
            changed: before != after,
            still_animating: self.transition.is_some(),
        }
    }

    /// Cancel any active target animation and settle at `offset`.
    pub(crate) fn cancel_at(&mut self, offset: usize) {
        self.transition = None;
        self.current = offset as f32;
        self.target = None;
    }

    /// Current displayed integer row.
    pub(crate) fn current_offset(&self, max_offset: usize) -> usize {
        self.current.round().clamp(0.0, max_offset as f32) as usize
    }

    fn clamped_transition_current(&self, max_offset: usize) -> f32 {
        match &self.transition {
            Some(transition) => clamped_transition_current(transition, max_offset),
            None => self.current.clamp(0.0, max_offset as f32),
        }
    }
}

fn clamped_transition_current(transition: &Transition<f32>, max_offset: usize) -> f32 {
    let eased = transition.current();
    let lo = transition.start.min(transition.end);
    let hi = transition.start.max(transition.end);
    eased.clamp(lo, hi).clamp(0.0, max_offset as f32)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use crate::animation::{Easing, TransitionConfig};
    use crate::widgets::{ScrollBehavior, ScrollDistanceConfig, ScrollWheelConfig};

    use super::{KineticScrollState, SmoothScrollState};

    fn smooth(duration_ms: u64, easing: Easing) -> ScrollBehavior {
        ScrollBehavior::smooth(TransitionConfig {
            duration: Duration::from_millis(duration_ms),
            easing,
        })
    }

    fn smooth_distance(
        min_ms: u64,
        max_ms: u64,
        ms_per_row: u64,
        easing: Easing,
    ) -> ScrollBehavior {
        ScrollBehavior::smooth_distance(ScrollDistanceConfig::new(
            Duration::from_millis(min_ms),
            Duration::from_millis(max_ms),
            Duration::from_millis(ms_per_row),
            easing,
        ))
    }

    #[test]
    fn zero_duration_smooth_snaps_and_clears_state() {
        let mut state = SmoothScrollState::default();
        let offset = state.resolve_target(3, 12, 20, smooth(0, Easing::Linear));

        assert_eq!(offset, 12);
        assert!(!state.is_animating());
        assert_eq!(state.current_offset(20), 12);
    }

    #[test]
    fn same_target_reconcile_does_not_restart_elapsed_progress() {
        let mut state = SmoothScrollState::default();
        let behavior = smooth(100, Easing::Linear);

        assert_eq!(state.resolve_target(0, 10, 20, behavior), 0);
        let tick = state.tick(Duration::from_millis(40), 20);
        assert!(tick.changed);
        assert_eq!(state.current_offset(20), 4);

        let offset = state.resolve_target(4, 10, 20, behavior);
        assert_eq!(offset, 4);
        state.tick(Duration::from_millis(20), 20);
        assert_eq!(state.current_offset(20), 6);
    }

    #[test]
    fn retargeting_starts_from_current_visual_offset() {
        let mut state = SmoothScrollState::default();
        let behavior = smooth(100, Easing::Linear);

        state.resolve_target(0, 10, 20, behavior);
        state.tick(Duration::from_millis(50), 20);
        assert_eq!(state.current_offset(20), 5);

        assert_eq!(state.resolve_target(5, 15, 20, behavior), 5);
        state.tick(Duration::from_millis(50), 20);
        assert_eq!(state.current_offset(20), 10);
    }

    #[test]
    fn elastic_easing_cannot_overshoot_target() {
        let mut state = SmoothScrollState::default();
        let behavior = smooth(100, Easing::EaseOutElastic);

        state.resolve_target(0, 10, 20, behavior);
        state.tick(Duration::from_millis(15), 20);

        assert!(state.current_offset(20) <= 10);
    }

    #[test]
    fn cancel_clears_active_state() {
        let mut state = SmoothScrollState::default();
        state.resolve_target(0, 10, 20, smooth(100, Easing::Linear));
        assert!(state.is_animating());

        state.cancel_at(3);

        assert!(!state.is_animating());
        assert_eq!(state.current_offset(20), 3);
    }

    #[test]
    fn distance_config_scales_and_caps_duration() {
        let config = ScrollDistanceConfig::new(
            Duration::from_millis(100),
            Duration::from_millis(300),
            Duration::from_millis(10),
            Easing::Linear,
        );

        assert_eq!(config.duration_for_distance(0), Duration::ZERO);
        assert_eq!(config.duration_for_distance(1), Duration::from_millis(110));
        assert_eq!(config.duration_for_distance(20), Duration::from_millis(300));
    }

    #[test]
    fn adaptive_smooth_uses_distance_based_duration() {
        let mut state = SmoothScrollState::default();
        let behavior = smooth_distance(100, 500, 10, Easing::Linear);

        state.resolve_target(0, 5, 100, behavior);
        assert_eq!(
            state
                .transition
                .as_ref()
                .map(|transition| transition.duration),
            Some(Duration::from_millis(150)),
        );

        state.cancel_at(0);
        state.resolve_target(0, 100, 100, behavior);
        assert_eq!(
            state
                .transition
                .as_ref()
                .map(|transition| transition.duration),
            Some(Duration::from_millis(500)),
        );
    }

    #[test]
    fn kinetic_impulse_starts_velocity_and_ticks_offset() {
        let mut state = KineticScrollState::default();
        let config = ScrollWheelConfig::new(100.0, 5.0, 200.0, 0.01);

        assert_eq!(state.apply_impulse(0, 3, 20, config), 0);
        assert!(state.is_animating());

        let tick = state.tick(Duration::from_millis(10), 20, config);

        assert!(tick.changed);
        assert!(tick.still_animating);
        assert!(state.current_offset(20) > 0);
    }

    #[test]
    fn kinetic_impulse_at_edge_does_not_animate_past_bounds() {
        let mut state = KineticScrollState::default();
        let config = ScrollWheelConfig::new(100.0, 5.0, 200.0, 0.01);

        assert_eq!(state.apply_impulse(20, 1, 20, config), 20);

        assert!(!state.is_animating());
        assert_eq!(state.current_offset(20), 20);
    }

    #[test]
    fn kinetic_coalesced_burst_preserves_overflow_distance() {
        let mut state = KineticScrollState::default();
        let config = ScrollWheelConfig::new(56.0, 10.0, 180.0, 0.05);

        assert_eq!(state.apply_impulse(0, 10, 200, config), 0);
        assert_eq!(state.velocity, 180.0);
        assert!(state.queued_distance > 0.0);

        for _ in 0..5 {
            state.tick(Duration::from_millis(50), 200, config);
        }

        assert!(state.current_offset(200) > 30);
        assert!(state.is_animating());
    }

    #[test]
    fn kinetic_coalesced_burst_caps_launch_step() {
        let mut state = KineticScrollState::default();
        let config = ScrollWheelConfig::new(56.0, 10.0, 320.0, 0.05);

        state.apply_impulse(0, 10, 200, config);
        let tick = state.tick(Duration::from_millis(50), 200, config);

        assert!(tick.changed);
        assert!(tick.still_animating);
        assert_eq!(state.current_offset(200), 4);
        assert!(state.queued_distance > 0.0);
    }

    #[test]
    fn kinetic_launch_cap_does_not_queue_single_notch_remainder() {
        let mut state = KineticScrollState::default();
        let config = ScrollWheelConfig::new(56.0, 10.0, 320.0, 0.05);

        state.apply_impulse(0, 3, 200, config);
        state.tick(Duration::from_millis(50), 200, config);

        assert_eq!(state.current_offset(200), 4);
        assert_eq!(state.queued_distance, 0.0);
        assert!(state.velocity < 168.0);
    }

    #[test]
    fn kinetic_reverse_impulse_clears_queued_burst_distance() {
        let mut state = KineticScrollState::default();
        let config = ScrollWheelConfig::new(56.0, 10.0, 180.0, 0.05);

        state.apply_impulse(50, 10, 200, config);
        assert!(state.queued_distance > 0.0);

        state.apply_impulse(50, -1, 200, config);

        assert_eq!(state.queued_distance, 0.0);
    }

    #[test]
    fn kinetic_settles_instead_of_creeping_to_delayed_final_row() {
        let mut state = KineticScrollState {
            position: 12.2,
            velocity: 2.0,
            queued_distance: 0.0,
            limit_next_step: false,
        };
        let config = ScrollWheelConfig::new(56.0, 10.0, 320.0, 0.05);

        let tick = state.tick(Duration::from_millis(16), 200, config);

        assert!(!tick.changed);
        assert!(!tick.still_animating);
        assert_eq!(state.current_offset(200), 12);
    }

    #[test]
    fn kinetic_keeps_animating_when_next_visible_row_is_imminent() {
        let mut state = KineticScrollState {
            position: 12.45,
            velocity: 2.0,
            queued_distance: 0.0,
            limit_next_step: false,
        };
        let config = ScrollWheelConfig::new(56.0, 10.0, 320.0, 0.05);

        let tick = state.tick(Duration::from_millis(16), 200, config);

        assert!(!tick.changed);
        assert!(tick.still_animating);
    }

    #[test]
    fn kinetic_settle_cutoff_is_aggressive_near_the_next_row() {
        let mut state = KineticScrollState {
            position: 12.35,
            velocity: 2.0,
            queued_distance: 0.0,
            limit_next_step: false,
        };
        let config = ScrollWheelConfig::new(56.0, 10.0, 320.0, 0.05);

        let tick = state.tick(Duration::from_millis(16), 200, config);

        assert!(!tick.changed);
        assert!(!tick.still_animating);
        assert_eq!(state.current_offset(200), 12);
    }
}
