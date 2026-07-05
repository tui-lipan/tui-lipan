//! Easing curves for transitions.

use std::f32::consts::PI;

/// Function type for easing curves.
pub type EasingFn = fn(f32) -> f32;

/// Linear interpolation curve.
pub fn linear(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

/// Quadratic ease-in curve.
pub fn ease_in_quad(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t
}

/// Quadratic ease-out curve.
pub fn ease_out_quad(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * (2.0 - t)
}

/// Cubic ease-in-out curve.
pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - ((-2.0 * t + 2.0).powi(3) / 2.0)
    }
}

/// Sinusoidal ease-in-out curve.
pub fn ease_in_out_sine(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    -(f32::cos(PI * t) - 1.0) / 2.0
}

/// Elastic ease-out curve (overshoots past 1.0 with decaying oscillation).
///
/// This is the standard `easeOutElastic` from easings.net — fixed amplitude and
/// frequency, not a tunable spring. The curve crosses 1.0 by ~t = 0.05 and then
/// oscillates toward 1.0; on a terminal cell grid that wobble can read as
/// 1-cell jitter near the destination, so prefer it for opacity/color rather
/// than position for short distances.
pub fn ease_out_elastic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t == 0.0 {
        return 0.0;
    }
    if t == 1.0 {
        return 1.0;
    }

    let c4 = (2.0 * PI) / 3.0;
    f32::powf(2.0, -10.0 * t) * f32::sin((t * 10.0 - 0.75) * c4) + 1.0
}

/// Built-in easing curves.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Easing {
    /// Linear interpolation.
    Linear,
    /// Quadratic ease-in.
    EaseInQuad,
    /// Quadratic ease-out.
    EaseOutQuad,
    /// Cubic ease-in-out.
    EaseInOutCubic,
    /// Sinusoidal ease-in-out.
    EaseInOutSine,
    /// Elastic ease-out with decaying overshoot (easings.net `easeOutElastic`).
    EaseOutElastic,
}

impl Easing {
    /// Apply this easing function to `t` in `[0.0, 1.0]`.
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Self::Linear => linear(t),
            Self::EaseInQuad => ease_in_quad(t),
            Self::EaseOutQuad => ease_out_quad(t),
            Self::EaseInOutCubic => ease_in_out_cubic(t),
            Self::EaseInOutSine => ease_in_out_sine(t),
            Self::EaseOutElastic => ease_out_elastic(t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_curves_are_clamped_for_out_of_range_inputs() {
        let curves: [EasingFn; 6] = [
            linear,
            ease_in_quad,
            ease_out_quad,
            ease_in_out_cubic,
            ease_in_out_sine,
            ease_out_elastic,
        ];

        for curve in curves {
            let below = curve(-0.5);
            let above = curve(1.5);
            assert!((0.0..=1.0).contains(&below));
            assert!((0.0..=1.0).contains(&above));
        }
    }

    #[test]
    fn monotonic_curves_are_non_decreasing() {
        let curves: [EasingFn; 5] = [
            linear,
            ease_in_quad,
            ease_out_quad,
            ease_in_out_cubic,
            ease_in_out_sine,
        ];

        for curve in curves {
            let mut prev = curve(0.0);
            for step in 1..=200 {
                let t = step as f32 / 200.0;
                let current = curve(t);
                assert!(current + 1e-6 >= prev);
                prev = current;
            }
        }
    }

    #[test]
    fn ease_out_elastic_hits_expected_endpoints_and_overshoots() {
        assert_eq!(ease_out_elastic(0.0), 0.0);
        assert_eq!(ease_out_elastic(1.0), 1.0);

        let peak = (1..=200)
            .map(|step| ease_out_elastic(step as f32 / 200.0))
            .fold(f32::MIN, f32::max);
        assert!(
            peak > 1.0,
            "ease_out_elastic peak should overshoot, got {peak}"
        );
    }
}
