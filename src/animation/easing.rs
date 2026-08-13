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

/// Tension of the standard easings.net `easeOutBack`, whose peak overshoot is 10%.
const STANDARD_BACK_TENSION: f32 = 1.701_58;
/// Peak overshoot of the standard `easeOutBack`, in thousandths of the animated distance.
pub const STANDARD_BACK_OVERSHOOT_PERMILLE: u16 = 100;
/// Maximum peak overshoot delivered by [`ease_out_back`], in thousandths of the distance.
///
/// Larger requests saturate at this value because greater amplitudes stop reading as a settle and
/// begin to look like a wind-up.
pub const MAX_BACK_OVERSHOOT_PERMILLE: u16 = 500;

/// Back ease-out curve: overshoots past 1.0 exactly once, then settles.
///
/// `overshoot_permille` is the peak overshoot as thousandths of the animated
/// distance; [`STANDARD_BACK_OVERSHOOT_PERMILLE`] is the standard easings.net
/// `easeOutBack`. Unlike [`ease_out_elastic`] this crosses 1.0 a single time, so
/// on a terminal cell grid it reads as one deliberate nudge rather than as
/// jitter — which is what makes it usable for a *position* or a rectangle, where
/// an oscillating curve becomes a 1-cell tremor.
///
/// The overshoot is a fraction of the animated distance, so a caller animating a
/// long distance and wanting a bounded nudge should scale the request down:
/// `permille = 1000 * wanted_units / distance_units`. Requesting `0` degenerates
/// to a plain cubic ease-out, which is the natural floor rather than a special
/// case. Requests above [`MAX_BACK_OVERSHOOT_PERMILLE`] saturate at that ceiling.
pub fn ease_out_back(t: f32, overshoot_permille: u16) -> f32 {
    let t = t.clamp(0.0, 1.0);
    // Both endpoints are algebraically exact but cancel to a few ULPs in f32, and a transition that
    // ends a hair short of its target is worse than a branch. Same guard as `ease_out_elastic`.
    if t == 0.0 {
        return 0.0;
    }
    if t == 1.0 {
        return 1.0;
    }

    let c1 = back_tension(overshoot_permille);
    let u = t - 1.0;
    1.0 + (c1 + 1.0) * u * u * u + c1 * u * u
}

/// The tension whose peak overshoot is `overshoot_permille` thousandths of the distance.
///
/// Forcing `f(0) = 0` and `f(1) = 1` on the cubic `1 + c3·u³ + c1·u²` pins `c3 = c1 + 1`, which leaves
/// the peak overshoot as `4c1³ / (27(c1 + 1)²)` — a cubic in `c1` with no usable closed-form inverse.
/// Newton from a power-law seed reaches f32 precision in a couple of steps across the supported
/// range, so the amplitude a caller asks for is the amplitude it gets up to the documented ceiling.
fn back_tension(overshoot_permille: u16) -> f32 {
    let target = f32::from(overshoot_permille.min(MAX_BACK_OVERSHOOT_PERMILLE)) / 1000.0;
    if target <= 0.0 {
        // c1 = 0 leaves `1 + u³`, a cubic ease-out with no overshoot at all.
        return 0.0;
    }
    let standard = f32::from(STANDARD_BACK_OVERSHOOT_PERMILLE) / 1000.0;
    let mut c1 = STANDARD_BACK_TENSION * (target / standard).sqrt();
    for _ in 0..4 {
        let value = 4.0 * c1 * c1 * c1 - 27.0 * target * (c1 + 1.0) * (c1 + 1.0);
        let slope = 12.0 * c1 * c1 - 54.0 * target * (c1 + 1.0);
        if slope.abs() <= f32::EPSILON {
            break;
        }
        c1 -= value / slope;
    }
    c1.max(0.0)
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
    /// Back ease-out with a single overshoot, sized as thousandths of the animated distance
    /// (easings.net `easeOutBack` at [`Easing::EASE_OUT_BACK`]).
    EaseOutBack {
        /// Peak overshoot in thousandths of the animated distance. `0` is a plain cubic ease-out;
        /// values above [`MAX_BACK_OVERSHOOT_PERMILLE`] saturate at that ceiling.
        overshoot_permille: u16,
    },
}

impl Easing {
    /// The standard easings.net `easeOutBack`, overshooting by 10% of the animated distance.
    pub const EASE_OUT_BACK: Self = Self::EaseOutBack {
        overshoot_permille: STANDARD_BACK_OVERSHOOT_PERMILLE,
    };

    /// Apply this easing function to `t` in `[0.0, 1.0]`.
    pub fn apply(self, t: f32) -> f32 {
        match self {
            Self::Linear => linear(t),
            Self::EaseInQuad => ease_in_quad(t),
            Self::EaseOutQuad => ease_out_quad(t),
            Self::EaseInOutCubic => ease_in_out_cubic(t),
            Self::EaseInOutSine => ease_in_out_sine(t),
            Self::EaseOutElastic => ease_out_elastic(t),
            Self::EaseOutBack { overshoot_permille } => ease_out_back(t, overshoot_permille),
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

        // Takes an amplitude, so it does not fit `EasingFn`.
        for permille in [0, STANDARD_BACK_OVERSHOOT_PERMILLE, 1_000] {
            assert!((0.0..=1.0).contains(&ease_out_back(-0.5, permille)));
            assert!((0.0..=1.0).contains(&ease_out_back(1.5, permille)));
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

    fn back_samples(overshoot_permille: u16) -> Vec<f32> {
        (0..=1000)
            .map(|step| ease_out_back(step as f32 / 1000.0, overshoot_permille))
            .collect()
    }

    #[test]
    fn ease_out_back_hits_expected_endpoints_and_overshoots_once() {
        let standard = STANDARD_BACK_OVERSHOOT_PERMILLE;
        assert_eq!(ease_out_back(0.0, standard), 0.0);
        assert_eq!(ease_out_back(1.0, standard), 1.0);

        let samples = back_samples(standard);
        let peak = samples.iter().copied().fold(f32::MIN, f32::max);
        assert!(
            (1.09..1.11).contains(&peak),
            "the standard curve should peak near 1.10, got {peak}"
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

    /// The whole point of the amplitude: a caller animating a long distance asks for a small fraction
    /// of it and gets that fraction, so a bounded nudge stays bounded.
    #[test]
    fn ease_out_back_delivers_the_overshoot_it_is_asked_for() {
        for permille in [10, 25, 50, STANDARD_BACK_OVERSHOOT_PERMILLE, 250] {
            let peak = back_samples(permille)
                .iter()
                .copied()
                .fold(f32::MIN, f32::max);
            let wanted = 1.0 + f32::from(permille) / 1000.0;
            assert!(
                (peak - wanted).abs() < 1e-3,
                "{permille}permille should peak at {wanted}, got {peak}"
            );
        }
    }

    #[test]
    fn ease_out_back_without_overshoot_is_a_plain_ease_out() {
        let samples = back_samples(0);
        assert_eq!(samples[0], 0.0);
        assert_eq!(samples[samples.len() - 1], 1.0);
        assert!(
            samples.iter().all(|value| *value <= 1.0 + 1e-6),
            "a zero amplitude must not overshoot at all"
        );
        for window in samples.windows(2) {
            assert!(
                window[1] + 1e-6 >= window[0],
                "a zero amplitude should rise monotonically"
            );
        }
    }

    #[test]
    fn ease_out_back_is_clamped_to_a_settling_amplitude() {
        // Past the ceiling the request saturates rather than winding up ever further.
        let capped = back_samples(MAX_BACK_OVERSHOOT_PERMILLE)
            .iter()
            .copied()
            .fold(f32::MIN, f32::max);
        let beyond = back_samples(u16::MAX)
            .iter()
            .copied()
            .fold(f32::MIN, f32::max);
        assert!((capped - beyond).abs() < 1e-6);
    }

    #[test]
    fn ease_out_back_rises_to_its_peak_then_settles() {
        // One rise and one fall, so the overshoot is a settle rather than a bounce.
        for permille in [25, STANDARD_BACK_OVERSHOOT_PERMILLE, 250] {
            let samples = back_samples(permille);
            let peak_index = samples
                .iter()
                .enumerate()
                .max_by(|a, b| a.1.total_cmp(b.1))
                .map(|(index, _)| index)
                .expect("samples are non-empty");

            for window in samples[..=peak_index].windows(2) {
                assert!(
                    window[1] + 1e-6 >= window[0],
                    "{permille}permille should rise up to its peak"
                );
            }
            for window in samples[peak_index..].windows(2) {
                assert!(
                    window[1] <= window[0] + 1e-6,
                    "{permille}permille should settle back down after its peak"
                );
            }
        }
    }
}
