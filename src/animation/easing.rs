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

/// Back ease-out curve (overshoots past 1.0 once, then settles).
///
/// The standard `easeOutBack` from easings.net. Unlike [`ease_out_elastic`] it
/// crosses 1.0 a single time, peaking at ~1.102 around t = 0.58 before settling,
/// so on a terminal cell grid it reads as one deliberate nudge rather than as
/// jitter. That makes it the curve to reach for when a *position* or a rectangle
/// should feel springy — an oscillating curve turns the same motion into a
/// 1-cell tremor.
///
/// The overshoot is a fraction of the animated distance, not a fixed size, so a
/// caller animating a long distance may want to damp the part of the curve that
/// exceeds 1.0.
pub fn ease_out_back(t: f32) -> f32 {
    /// Overshoot strength from the easings.net definition; `C3 = C1 + 1`.
    const C1: f32 = 1.701_58;
    const C3: f32 = C1 + 1.0;

    let t = t.clamp(0.0, 1.0);
    let u = t - 1.0;
    1.0 + C3 * u * u * u + C1 * u * u
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
    /// Back ease-out with a single overshoot (easings.net `easeOutBack`).
    EaseOutBack,
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
            Self::EaseOutBack => ease_out_back(t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easing_curves_are_clamped_for_out_of_range_inputs() {
        let curves: [EasingFn; 7] = [
            linear,
            ease_in_quad,
            ease_out_quad,
            ease_in_out_cubic,
            ease_in_out_sine,
            ease_out_elastic,
            ease_out_back,
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

    #[test]
    fn ease_out_back_hits_expected_endpoints_and_overshoots_once() {
        assert_eq!(ease_out_back(0.0), 0.0);
        assert_eq!(ease_out_back(1.0), 1.0);

        let samples: Vec<f32> = (0..=200)
            .map(|step| ease_out_back(step as f32 / 200.0))
            .collect();
        let peak = samples.iter().copied().fold(f32::MIN, f32::max);
        assert!(
            (1.09..1.11).contains(&peak),
            "ease_out_back should peak near 1.102, got {peak}"
        );

        // A single crossing is what separates this from `ease_out_elastic`: an oscillating curve
        // would cross 1.0 repeatedly and read as jitter rather than as one nudge.
        let crossings = samples
            .windows(2)
            .filter(|pair| (pair[0] < 1.0) != (pair[1] < 1.0))
            .count();
        assert_eq!(
            crossings, 1,
            "expected one crossing of 1.0, got {crossings}"
        );
    }

    #[test]
    fn ease_out_back_rises_to_its_peak_then_settles() {
        // One rise and one fall, so the overshoot is a settle rather than a bounce.
        let samples: Vec<f32> = (0..=200)
            .map(|step| ease_out_back(step as f32 / 200.0))
            .collect();
        let peak_index = samples
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.total_cmp(b.1))
            .map(|(index, _)| index)
            .expect("samples are non-empty");

        for window in samples[..=peak_index].windows(2) {
            assert!(
                window[1] + 1e-6 >= window[0],
                "ease_out_back should rise up to its peak"
            );
        }
        for window in samples[peak_index..].windows(2) {
            assert!(
                window[1] <= window[0] + 1e-6,
                "ease_out_back should settle back down after its peak"
            );
        }
    }
}
