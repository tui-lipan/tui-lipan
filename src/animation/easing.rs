//! Easing curves for transitions.

use std::f32::consts::PI;

/// A coordinate in a CSS-style cubic Bézier control point.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CubicBezierCoordinate {
    /// The first control point's x coordinate.
    X1,
    /// The first control point's y coordinate.
    Y1,
    /// The second control point's x coordinate.
    X2,
    /// The second control point's y coordinate.
    Y2,
}

impl std::fmt::Display for CubicBezierCoordinate {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::X1 => "x1",
            Self::Y1 => "y1",
            Self::X2 => "x2",
            Self::Y2 => "y2",
        };
        formatter.write_str(name)
    }
}

/// Why a cubic Bézier control point was rejected.
#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum CubicBezierError {
    /// A control point coordinate was not finite.
    #[error("{coordinate} must be finite")]
    NonFinite {
        /// The rejected coordinate.
        coordinate: CubicBezierCoordinate,
    },
    /// An x coordinate was outside the CSS-compatible `[0, 1]` range.
    #[error("{coordinate} must be in the range [0, 1]")]
    XOutOfRange {
        /// The rejected coordinate.
        coordinate: CubicBezierCoordinate,
    },
}

/// A validated CSS-style cubic Bézier timing curve.
///
/// The curve starts at `(0, 0)` and ends at `(1, 1)`. Its x coordinates are
/// constrained to `[0, 1]`, while its y coordinates may overshoot that range.
/// This is the same control-point representation used by CSS and Hyprland.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct CubicBezier {
    x1: u32,
    y1: u32,
    x2: u32,
    y2: u32,
}

impl std::fmt::Debug for CubicBezier {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CubicBezier")
            .field("x1", &self.x1())
            .field("y1", &self.y1())
            .field("x2", &self.x2())
            .field("y2", &self.y2())
            .finish()
    }
}

impl CubicBezier {
    /// Creates a validated cubic Bézier curve from its four control points.
    pub fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Result<Self, CubicBezierError> {
        validate_coordinate(CubicBezierCoordinate::X1, x1, true)?;
        validate_coordinate(CubicBezierCoordinate::Y1, y1, false)?;
        validate_coordinate(CubicBezierCoordinate::X2, x2, true)?;
        validate_coordinate(CubicBezierCoordinate::Y2, y2, false)?;

        Ok(Self::from_values(x1, y1, x2, y2))
    }

    /// Returns the first control point's x coordinate.
    pub fn x1(self) -> f32 {
        f32::from_bits(self.x1)
    }

    /// Returns the first control point's y coordinate.
    pub fn y1(self) -> f32 {
        f32::from_bits(self.y1)
    }

    /// Returns the second control point's x coordinate.
    pub fn x2(self) -> f32 {
        f32::from_bits(self.x2)
    }

    /// Returns the second control point's y coordinate.
    pub fn y2(self) -> f32 {
        f32::from_bits(self.y2)
    }

    /// Returns the temporal reverse of this curve.
    ///
    /// The reversed control points are exactly `(1-x2, 1-y2, 1-x1, 1-y1)`.
    pub fn reversed(self) -> Self {
        Self::from_values(
            1.0 - self.x2(),
            1.0 - self.y2(),
            1.0 - self.x1(),
            1.0 - self.y1(),
        )
    }

    fn from_values(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        Self {
            x1: normalized_bits(x1),
            y1: normalized_bits(y1),
            x2: normalized_bits(x2),
            y2: normalized_bits(y2),
        }
    }

    fn apply(self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t == 0.0 {
            return 0.0;
        }
        if t == 1.0 {
            return 1.0;
        }

        let parameter = solve_parameter(f64::from(t), f64::from(self.x1()), f64::from(self.x2()));
        finite_f32(cubic_value(
            parameter,
            f64::from(self.y1()),
            f64::from(self.y2()),
        ))
    }
}

/// Validates one control-point coordinate.
fn validate_coordinate(
    coordinate: CubicBezierCoordinate,
    value: f32,
    is_x: bool,
) -> Result<(), CubicBezierError> {
    if !value.is_finite() {
        return Err(CubicBezierError::NonFinite { coordinate });
    }
    if is_x && !(0.0..=1.0).contains(&value) {
        return Err(CubicBezierError::XOutOfRange { coordinate });
    }
    Ok(())
}

fn normalized_bits(value: f32) -> u32 {
    if value == 0.0 {
        0.0f32.to_bits()
    } else {
        value.to_bits()
    }
}

/// Solves the monotone x component while keeping every trial inside its bracket.
fn solve_parameter(target: f64, x1: f64, x2: f64) -> f64 {
    let mut lower = 0.0;
    let mut upper = 1.0;
    let mut parameter = target;

    // Newton converges quickly for ordinary curves. A trial that would leave the
    // bracket, or a derivative too small to trust, immediately falls back to its
    // bracket midpoint.
    for _ in 0..8 {
        let value = cubic_value(parameter, x1, x2);
        let residual = value - target;
        if residual.abs() <= 1e-14 {
            return parameter;
        }
        if residual < 0.0 {
            lower = parameter;
        } else {
            upper = parameter;
        }

        let derivative = cubic_derivative(parameter, x1, x2);
        let newton = if derivative > 1e-14 {
            parameter - residual / derivative
        } else {
            f64::NAN
        };
        parameter = if newton.is_finite() && newton > lower && newton < upper {
            newton
        } else {
            (lower + upper) * 0.5
        };
    }

    // Bisection is the reliable path for endpoint-flat curves and the final
    // precision pass for curves for which Newton did not settle.
    for _ in 0..64 {
        parameter = (lower + upper) * 0.5;
        let residual = cubic_value(parameter, x1, x2) - target;
        if residual.abs() <= 1e-14 || upper - lower <= 1e-14 {
            break;
        }
        if residual < 0.0 {
            lower = parameter;
        } else {
            upper = parameter;
        }
    }
    parameter
}

fn cubic_value(parameter: f64, first: f64, second: f64) -> f64 {
    let remaining = 1.0 - parameter;
    3.0 * remaining * remaining * parameter * first
        + 3.0 * remaining * parameter * parameter * second
        + parameter * parameter * parameter
}

fn cubic_derivative(parameter: f64, first: f64, second: f64) -> f64 {
    let remaining = 1.0 - parameter;
    3.0 * remaining * remaining * first
        + 6.0 * remaining * parameter * (second - first)
        + 3.0 * parameter * parameter * (1.0 - second)
}

fn finite_f32(value: f64) -> f32 {
    if value > f64::from(f32::MAX) {
        f32::MAX
    } else if value < f64::from(f32::MIN) {
        f32::MIN
    } else {
        value as f32
    }
}

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
    /// A CSS-style cubic Bézier timing curve.
    CubicBezier(CubicBezier),
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
            Self::CubicBezier(curve) => curve.apply(t),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    fn curve_hash(curve: CubicBezier) -> u64 {
        let mut hasher = DefaultHasher::new();
        curve.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn cubic_bezier_rejects_non_finite_coordinates() {
        for (coordinate, values) in [
            (CubicBezierCoordinate::X1, (f32::NAN, 0.0, 0.0, 0.0)),
            (CubicBezierCoordinate::Y1, (0.0, f32::INFINITY, 0.0, 0.0)),
            (
                CubicBezierCoordinate::X2,
                (0.0, 0.0, f32::NEG_INFINITY, 0.0),
            ),
            (CubicBezierCoordinate::Y2, (0.0, 0.0, 0.0, f32::NAN)),
        ] {
            let error = CubicBezier::new(values.0, values.1, values.2, values.3)
                .expect_err("non-finite coordinate should be rejected");
            assert_eq!(error, CubicBezierError::NonFinite { coordinate });
        }
    }

    #[test]
    fn cubic_bezier_rejects_x_coordinates_outside_the_unit_interval() {
        for (coordinate, values) in [
            (CubicBezierCoordinate::X1, (-0.01, 0.0, 0.0, 0.0)),
            (CubicBezierCoordinate::X1, (1.01, 0.0, 0.0, 0.0)),
            (CubicBezierCoordinate::X2, (0.0, 0.0, -0.01, 0.0)),
            (CubicBezierCoordinate::X2, (0.0, 0.0, 1.01, 0.0)),
        ] {
            let error = CubicBezier::new(values.0, values.1, values.2, values.3)
                .expect_err("out-of-range x coordinate should be rejected");
            assert_eq!(error, CubicBezierError::XOutOfRange { coordinate });
        }
    }

    #[test]
    fn cubic_bezier_accessors_and_reverse_preserve_control_points() {
        let curve = CubicBezier::new(0.2, -0.4, 0.8, 1.2).unwrap();
        assert_eq!(
            (curve.x1(), curve.y1(), curve.x2(), curve.y2()),
            (0.2, -0.4, 0.8, 1.2)
        );

        let reversed = curve.reversed();
        assert_eq!(
            (reversed.x1(), reversed.y1(), reversed.x2(), reversed.y2()),
            (
                1.0 - curve.x2(),
                1.0 - curve.y2(),
                1.0 - curve.x1(),
                1.0 - curve.y1()
            )
        );
        let twice_reversed = reversed.reversed();
        assert!((twice_reversed.x1() - curve.x1()).abs() <= f32::EPSILON);
        assert!((twice_reversed.y1() - curve.y1()).abs() <= f32::EPSILON);
        assert!((twice_reversed.x2() - curve.x2()).abs() <= f32::EPSILON);
        assert!((twice_reversed.y2() - curve.y2()).abs() <= f32::EPSILON);
    }

    #[test]
    fn cubic_bezier_normalizes_signed_zero_for_equality_and_hashing() {
        let negative = CubicBezier::new(-0.0, -0.0, -0.0, -0.0).unwrap();
        let positive = CubicBezier::new(0.0, 0.0, 0.0, 0.0).unwrap();
        assert_eq!(negative, positive);
        assert_eq!(curve_hash(negative), curve_hash(positive));
        assert_eq!(Easing::CubicBezier(negative), Easing::CubicBezier(positive));
    }

    #[test]
    fn cubic_bezier_clamps_input_and_has_exact_endpoints() {
        let easing = Easing::CubicBezier(CubicBezier::new(0.25, 0.1, 0.25, 1.0).unwrap());
        assert_eq!(easing.apply(-1.0), 0.0);
        assert_eq!(easing.apply(0.0), 0.0);
        assert_eq!(easing.apply(1.0), 1.0);
        assert_eq!(easing.apply(2.0), 1.0);
    }

    #[test]
    fn cubic_bezier_matches_css_reference_samples() {
        let easing = Easing::CubicBezier(CubicBezier::new(0.25, 0.1, 0.25, 1.0).unwrap());
        assert!((easing.apply(0.25) - 0.4085106).abs() < 1e-5);
        assert!((easing.apply(0.5) - 0.8024034).abs() < 1e-5);
        assert!((easing.apply(0.75) - 0.960_459).abs() < 1e-5);

        let linear = Easing::CubicBezier(CubicBezier::new(0.0, 0.0, 1.0, 1.0).unwrap());
        for sample in [0.1, 0.5, 0.9] {
            assert!((linear.apply(sample) - sample).abs() < 1e-12);
        }
    }

    #[test]
    fn cubic_bezier_preserves_y_overshoot() {
        let easing = Easing::CubicBezier(CubicBezier::new(0.0, 2.0, 1.0, 2.0).unwrap());
        assert!(easing.apply(0.5) > 1.5);
    }

    #[test]
    fn cubic_bezier_handles_degenerate_x_slopes_with_bisection() {
        for curve in [
            CubicBezier::new(0.0, 0.0, 0.0, 1.0).unwrap(),
            CubicBezier::new(1.0, 0.0, 1.0, 1.0).unwrap(),
        ] {
            let easing = Easing::CubicBezier(curve);
            for sample in [0.000_001, 0.01, 0.5, 0.99, 0.999_999] {
                let value = easing.apply(sample);
                assert!(value.is_finite());
                assert!((0.0..=1.0).contains(&value));
            }
        }
    }

    #[test]
    fn cubic_bezier_keeps_extreme_finite_y_values_finite() {
        let easing = Easing::CubicBezier(CubicBezier::new(0.25, f32::MAX, 0.75, f32::MIN).unwrap());
        for sample in [0.001, 0.25, 0.5, 0.75, 0.999] {
            assert!(easing.apply(sample).is_finite());
        }
    }

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
