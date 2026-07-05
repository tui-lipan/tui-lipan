//! Transition primitive and interpolation traits.

use std::time::Duration;

use crate::animation::easing::Easing;
use crate::style::{Color, FloatRect};

/// Linear interpolation support for transition values.
pub trait Lerp: Clone {
    /// Interpolate from `from` to `to` by eased progress `t`.
    fn lerp(from: &Self, to: &Self, t: f32) -> Self;
}

impl Lerp for f32 {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        from + (to - from) * t
    }
}

impl Lerp for u8 {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        let value = f32::lerp(&(*from as f32), &(*to as f32), t);
        value.round().clamp(0.0, u8::MAX as f32) as u8
    }
}

impl Lerp for u16 {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        let value = f32::lerp(&(*from as f32), &(*to as f32), t);
        value.round().clamp(0.0, u16::MAX as f32) as u16
    }
}

impl Lerp for FloatRect {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        Self {
            x: f32::lerp(&from.x, &to.x, t),
            y: f32::lerp(&from.y, &to.y, t),
            w: f32::lerp(&from.w, &to.w, t),
            h: f32::lerp(&from.h, &to.h, t),
        }
    }
}

impl Lerp for Color {
    fn lerp(from: &Self, to: &Self, t: f32) -> Self {
        from.blend_toward(*to, t)
    }
}

/// Active transition between two values over a fixed duration.
#[derive(Clone, Debug)]
pub struct Transition<T: Lerp> {
    /// Starting value.
    pub start: T,
    /// Ending value.
    pub end: T,
    /// Total duration of this transition.
    pub duration: Duration,
    /// Elapsed time since start.
    pub elapsed: Duration,
    /// Easing curve used to map progress.
    pub easing: Easing,
    /// Marks this as an exit transition.
    pub is_exit: bool,
}

impl<T: Lerp> Transition<T> {
    /// Create a new transition from `from` to `to`.
    pub fn new(from: T, to: T, duration: Duration, easing: Easing) -> Self {
        Self {
            start: from,
            end: to,
            duration,
            elapsed: Duration::ZERO,
            easing,
            is_exit: false,
        }
    }

    /// Return raw progress in `[0.0, 1.0]`.
    pub fn progress(&self) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (self.elapsed.as_secs_f32() / self.duration.as_secs_f32()).clamp(0.0, 1.0)
    }

    /// Return eased progress for the active easing curve.
    ///
    /// Most built-in curves stay in `[0.0, 1.0]`, but overshooting curves such
    /// as `Easing::EaseOutElastic` may temporarily return values outside that
    /// range.
    pub fn eased_progress(&self) -> f32 {
        self.easing.apply(self.progress())
    }

    /// Return the current interpolated value.
    pub fn current(&self) -> T {
        T::lerp(&self.start, &self.end, self.eased_progress())
    }

    /// Return true when transition reached completion.
    pub fn is_complete(&self) -> bool {
        self.elapsed >= self.duration
    }

    /// Advance transition by `dt`. Returns true when complete.
    pub fn tick(&mut self, dt: Duration) -> bool {
        self.elapsed = self.elapsed.saturating_add(dt).min(self.duration);
        self.is_complete()
    }
}

/// Default transition timing parameters.
#[derive(Clone, Copy, Debug)]
pub struct TransitionConfig {
    /// Total transition duration.
    pub duration: Duration,
    /// Easing curve.
    pub easing: Easing,
}

impl Default for TransitionConfig {
    fn default() -> Self {
        Self {
            duration: Duration::from_millis(200),
            easing: Easing::EaseOutQuad,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f32_lerp_midpoint_is_halfway() {
        assert!((f32::lerp(&0.0, &1.0, 0.5) - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn float_rect_transition_midpoint_is_halfway() {
        let from = FloatRect {
            x: 0.0,
            y: 10.0,
            w: 20.0,
            h: 30.0,
        };
        let to = FloatRect {
            x: 10.0,
            y: -10.0,
            w: 40.0,
            h: 0.0,
        };
        let mut transition = Transition::new(from, to, Duration::from_millis(100), Easing::Linear);

        transition.tick(Duration::from_millis(50));

        assert_eq!(
            transition.current(),
            FloatRect {
                x: 5.0,
                y: 0.0,
                w: 30.0,
                h: 15.0,
            }
        );
    }

    #[test]
    fn float_rect_lerp_allows_overshoot_progress() {
        let from = FloatRect {
            x: 0.0,
            y: 10.0,
            w: 20.0,
            h: 30.0,
        };
        let to = FloatRect {
            x: 10.0,
            y: -10.0,
            w: 40.0,
            h: 0.0,
        };

        assert_eq!(
            FloatRect::lerp(&from, &to, 1.25),
            FloatRect {
                x: 12.5,
                y: -15.0,
                w: 45.0,
                h: -7.5,
            }
        );
    }

    #[test]
    fn tick_advances_elapsed_and_clamps_to_duration() {
        let mut transition =
            Transition::new(0.0f32, 1.0f32, Duration::from_millis(100), Easing::Linear);

        assert!(!transition.tick(Duration::from_millis(30)));
        assert_eq!(transition.elapsed, Duration::from_millis(30));
        assert!((transition.current() - 0.3).abs() < 1e-6);

        assert!(transition.tick(Duration::from_millis(90)));
        assert_eq!(transition.elapsed, Duration::from_millis(100));
        assert!((transition.current() - 1.0).abs() < 1e-6);
        assert!(transition.is_complete());
    }

    #[test]
    fn progress_handles_zero_duration_as_complete() {
        let mut transition = Transition::new(0.0f32, 1.0f32, Duration::ZERO, Easing::EaseOutQuad);

        assert_eq!(transition.progress(), 1.0);
        assert!(transition.tick(Duration::from_millis(10)));
        assert_eq!(transition.current(), 1.0);
    }

    #[test]
    fn ease_out_elastic_transition_can_overshoot_for_f32_values() {
        let mut transition = Transition::new(
            0.0f32,
            1.0f32,
            Duration::from_millis(100),
            Easing::EaseOutElastic,
        );
        transition.tick(Duration::from_millis(15));

        assert!(transition.current() > 1.0);
    }
}
