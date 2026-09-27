//! Scalar amounts for render-time color transforms.
//!
//! A [`ColorTransform`](crate::style::ColorTransform) carries an [`EffectAmount`] rather than a
//! bare `f32` so the amount can be *late-bound*: named in the element tree and resolved by the
//! renderer while it paints. That is what lets an animated dim, tint, or opacity advance with a
//! repaint instead of a `view()` pass, exactly as [`Paint::Animated`](crate::style::Paint::Animated)
//! does for colors.

use std::hash::{Hash, Hasher};
use std::time::Duration;

use crate::animation::Easing;

/// Default length of one [`EffectPulse`] cycle.
const DEFAULT_PULSE_PERIOD: Duration = Duration::from_millis(1500);
/// Default sampling rate of an [`EffectPulse`]. A breathing tint is subtle; it does not need the
/// app's full geometry frame rate.
const DEFAULT_PULSE_FRAME_RATE: u16 = 30;

/// The strength of a render-time color transform: a fixed number, or one the renderer resolves.
///
/// Every [`ColorTransform`](crate::style::ColorTransform) carries one, and the
/// [`EffectScope`](crate::widgets::EffectScope) builders `dim_by`, `lighten_by`, and `tint_by`
/// accept `impl Into<EffectAmount>`, so a plain `f32` keeps working:
///
/// ```
/// use tui_lipan::prelude::*;
///
/// let dimmed = EffectScope::new().dim_by(0.4).child(Text::new("inactive pane"));
/// ```
///
/// (The [`Style`](crate::style::Style) shorthands `dim_by`, `tint_by`, `lighten_by`, and
/// `elevate_by` stay fixed `f32`s; pass a late-bound amount to a style through
/// [`Style::transform_fg`](crate::style::Style::transform_fg) /
/// [`transform_bg`](crate::style::Style::transform_bg) instead.)
///
/// The two late-bound forms come from the component [`Context`](crate::Context) and name an entry
/// in its animation registry. The element tree holds the same amount while the registry moves the
/// value, so advancing it is a repaint instead of a rebuild:
///
/// - [`Context::animated_amount`](crate::Context::animated_amount) transitions toward a target the
///   app picks, like [`Context::animated_color`](crate::Context::animated_color) does for colors.
/// - [`Context::pulsing_amount`](crate::Context::pulsing_amount) oscillates for as long as the
///   view keeps asking for it - a breathing alert tint needs no app-side timer or target toggling.
///
/// Late-bound amounts only resolve while the renderer paints. Read outside a paint, they answer
/// with their [`resting_value`](Self::resting_value): the transition's target, or the pulse's
/// starting value.
///
/// An `EffectAmount` is eight bytes and `Copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EffectAmount(Repr);

#[derive(Clone, Copy, Debug)]
enum Repr {
    /// A plain amount, baked into the element tree.
    Fixed(f32),
    /// A registry transition, named by its slot. Compares equal for the whole transition, because
    /// it does not embed where the transition currently is.
    Animated { slot: u16, target: f32 },
    /// A registry pulse, named by its slot, resting at `from`.
    Pulse { slot: u16, from: f32 },
}

// `f32` fields compare and hash by bit pattern, like every other effect parameter.
impl PartialEq for Repr {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Fixed(a), Self::Fixed(b)) => a.to_bits() == b.to_bits(),
            (
                Self::Animated {
                    slot: slot_a,
                    target: a,
                },
                Self::Animated {
                    slot: slot_b,
                    target: b,
                },
            )
            | (
                Self::Pulse {
                    slot: slot_a,
                    from: a,
                },
                Self::Pulse {
                    slot: slot_b,
                    from: b,
                },
            ) => slot_a == slot_b && a.to_bits() == b.to_bits(),
            _ => false,
        }
    }
}

impl Eq for Repr {}

impl Hash for Repr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match *self {
            Self::Fixed(value) => {
                0u8.hash(state);
                value.to_bits().hash(state);
            }
            Self::Animated { slot, target } => {
                1u8.hash(state);
                slot.hash(state);
                target.to_bits().hash(state);
            }
            Self::Pulse { slot, from } => {
                2u8.hash(state);
                slot.hash(state);
                from.to_bits().hash(state);
            }
        }
    }
}

impl EffectAmount {
    /// A fixed amount. The `const` form of `EffectAmount::from(value)`.
    pub const fn fixed(value: f32) -> Self {
        Self(Repr::Fixed(value))
    }

    /// A late-bound transition naming registry `slot`, settling on `target`.
    pub(crate) fn animated(slot: u16, target: f32) -> Self {
        Self(Repr::Animated { slot, target })
    }

    /// A late-bound pulse naming registry `slot`, resting at `from`.
    pub(crate) fn pulsing(slot: u16, from: f32) -> Self {
        Self(Repr::Pulse { slot, from })
    }

    /// The amount when it is fixed, or `None` when the renderer resolves it.
    pub fn as_fixed(self) -> Option<f32> {
        match self.0 {
            Repr::Fixed(value) => Some(value),
            Repr::Animated { .. } | Repr::Pulse { .. } => None,
        }
    }

    /// Whether this amount comes from [`Context::animated_amount`](crate::Context::animated_amount).
    pub fn is_transition(self) -> bool {
        matches!(self.0, Repr::Animated { .. })
    }

    /// Whether this amount comes from [`Context::pulsing_amount`](crate::Context::pulsing_amount).
    pub fn is_pulse(self) -> bool {
        matches!(self.0, Repr::Pulse { .. })
    }

    /// Whether the renderer, rather than the element tree, decides this amount.
    pub fn is_late_bound(self) -> bool {
        !matches!(self.0, Repr::Fixed(_))
    }

    /// A single number standing in for this amount where no live value can be had: the fixed
    /// value, the transition's target, or the pulse's starting value.
    pub fn resting_value(self) -> f32 {
        match self.0 {
            Repr::Fixed(value)
            | Repr::Animated { target: value, .. }
            | Repr::Pulse { from: value, .. } => value,
        }
    }

    /// The amount as the renderer sees it right now.
    ///
    /// A late-bound amount reads its registry slot, which holds one sampled value between
    /// animation ticks - so every paint in between, including a partial one, agrees on it.
    /// Outside a draw it answers with its resting value.
    pub(crate) fn resolved(self) -> f32 {
        match self.0 {
            Repr::Fixed(value) => value,
            Repr::Animated { slot, target: rest } | Repr::Pulse { slot, from: rest } => {
                crate::animation::registry::resolve_render_scalar_slot(slot).unwrap_or(rest)
            }
        }
    }

    /// The value an image recolor may bake in, or `None` when the amount never settles.
    ///
    /// Following a late-bound amount per frame would re-encode every image under it on every
    /// frame. A transition is recorded at its target instead - one encode for the whole fade - and
    /// a pulse is left out of image pixels entirely.
    pub(crate) fn settled(self) -> Option<f32> {
        match self.0 {
            Repr::Fixed(value) | Repr::Animated { target: value, .. } => Some(value),
            Repr::Pulse { .. } => None,
        }
    }

    /// This amount limited to `[0.0, 1.0]`.
    ///
    /// A late-bound amount only has its resting value clamped: the live value belongs to the
    /// registry, and every transform consumer clamps what it reads.
    pub(crate) fn clamped_unit(self) -> Self {
        let unit = |value: f32| value.clamp(0.0, 1.0);
        match self.0 {
            Repr::Fixed(value) => Self::fixed(unit(value)),
            Repr::Animated { slot, target } => Self::animated(slot, unit(target)),
            Repr::Pulse { slot, from } => Self::pulsing(slot, unit(from)),
        }
    }
}

impl Default for EffectAmount {
    fn default() -> Self {
        Self::fixed(0.0)
    }
}

impl From<f32> for EffectAmount {
    fn from(value: f32) -> Self {
        Self::fixed(value)
    }
}

// Serialized as its resting value. A late-bound amount names a registry slot that lives in one
// running app, so another process could not resolve it anyway, and a plain number keeps the wire
// format what it was when transforms carried an `f32`.
#[cfg(feature = "terminal-serde")]
impl serde::Serialize for EffectAmount {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_f32(self.resting_value())
    }
}

#[cfg(feature = "terminal-serde")]
impl<'de> serde::Deserialize<'de> for EffectAmount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        f32::deserialize(deserializer).map(Self::fixed)
    }
}

/// How a pulse from [`Context::pulsing_amount`](crate::Context::pulsing_amount) oscillates.
///
/// One cycle runs `from -> to -> from` over [`period`](Self::period), shaped by
/// [`easing`](Self::easing) on each half. The default `EaseInOutSine` makes that a smooth breath
/// with no visible turnaround.
///
/// A pulse starts at `from` when its key first appears and is owned by the runtime's animation
/// registry from then on. The registry samples it [`frame_rate`](Self::frame_rate) times a
/// second on the pulse's own timeline, and every paint between two samples - including a partial
/// repaint of a few damaged terminal rows - sees the same value. Each sample costs a paint and no
/// `view()` pass.
///
/// Image pixels do not follow a pulse: recoloring and re-encoding a picture on every breath is
/// the cost the pulse exists to avoid. Text and cell colors breathe; images under the same
/// transform are left untouched by it.
#[derive(Clone, Copy, Debug)]
pub struct EffectPulse {
    pub(crate) from: f32,
    pub(crate) to: f32,
    pub(crate) period: Duration,
    pub(crate) easing: Easing,
    frame_rate: u16,
}

// `f32` fields compare by bit pattern, like every other effect parameter.
impl PartialEq for EffectPulse {
    fn eq(&self, other: &Self) -> bool {
        self.from.to_bits() == other.from.to_bits()
            && self.to.to_bits() == other.to.to_bits()
            && self.period == other.period
            && self.easing == other.easing
            && self.frame_rate == other.frame_rate
    }
}

impl Eq for EffectPulse {}

impl Hash for EffectPulse {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.from.to_bits().hash(state);
        self.to.to_bits().hash(state);
        self.period.hash(state);
        self.easing.hash(state);
        self.frame_rate.hash(state);
    }
}

impl EffectPulse {
    /// A pulse between `from` and `to` with the default period, easing, and frame rate.
    pub fn new(from: f32, to: f32) -> Self {
        Self {
            from,
            to,
            period: DEFAULT_PULSE_PERIOD,
            easing: Easing::EaseInOutSine,
            frame_rate: DEFAULT_PULSE_FRAME_RATE,
        }
    }

    /// Length of one full `from -> to -> from` cycle. Defaults to 1.5 s.
    pub fn period(mut self, period: Duration) -> Self {
        self.period = period.max(Duration::from_millis(1));
        self
    }

    /// Curve applied to each half of the cycle. Defaults to [`Easing::EaseInOutSine`].
    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    /// How many times a second the pulse is sampled - and so repainted - clamped to 1-480.
    /// Defaults to 30.
    ///
    /// Several late-bound animations on screen share the fastest cadence among them for their
    /// repaints, but each pulse still only changes value at its own rate. The rate never exceeds
    /// [`App::frame_rate`](crate::App::frame_rate).
    pub fn frame_rate(mut self, frame_rate: u16) -> Self {
        self.frame_rate = frame_rate.clamp(1, 480);
        self
    }

    /// The value the cycle starts and ends on.
    pub fn from_value(self) -> f32 {
        self.from
    }

    /// The value at the middle of the cycle.
    pub fn to_value(self) -> f32 {
        self.to
    }

    /// The pulse's value `elapsed` into its own timeline.
    pub fn value_at(self, elapsed: Duration) -> f32 {
        let period = self.period.as_nanos().max(1);
        let t = (elapsed.as_nanos() % period) as f64 / period as f64;
        // Triangle wave: 0 at the start of the cycle, 1 halfway, back to 0 at the end.
        let rise = (1.0 - (2.0 * t - 1.0).abs()) as f32;
        let eased = self.easing.apply(rise);
        self.from + (self.to - self.from) * eased
    }

    /// Time between samples.
    pub fn interval(self) -> Duration {
        crate::app::context::frame_interval(self.frame_rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_numbers_convert_to_fixed_amounts() {
        let amount: EffectAmount = 0.25.into();
        assert_eq!(amount, EffectAmount::fixed(0.25));
        assert_eq!(amount.as_fixed(), Some(0.25));
        assert!(!amount.is_late_bound());
        assert_eq!(amount.resolved(), 0.25);
    }

    #[test]
    fn a_pulse_runs_from_to_and_back_over_its_period() {
        let pulse = EffectPulse::new(0.1, 0.5)
            .period(Duration::from_millis(1000))
            .easing(Easing::Linear);
        let at = |ms| pulse.value_at(Duration::from_millis(ms));
        assert!((at(0) - 0.1).abs() < 1e-6);
        assert!((at(250) - 0.3).abs() < 1e-6);
        assert!((at(500) - 0.5).abs() < 1e-6);
        assert!((at(750) - 0.3).abs() < 1e-6);
        assert!(
            (at(1000) - 0.1).abs() < 1e-6,
            "one period is one full breath"
        );
        assert!(
            (at(10_500) - 0.5).abs() < 1e-6,
            "and it keeps time indefinitely"
        );
    }

    #[test]
    fn a_pulse_eases_each_half_of_its_cycle() {
        let pulse = EffectPulse::new(0.0, 1.0).period(Duration::from_millis(1000));
        let quarter = pulse.value_at(Duration::from_millis(125));
        assert!(
            quarter < 0.25,
            "ease-in-out starts slow: {quarter} at a quarter of the rise"
        );
    }

    #[test]
    fn a_pulse_reports_its_frame_rate_as_an_interval() {
        assert_eq!(
            EffectPulse::new(0.0, 1.0).frame_rate(10).interval(),
            Duration::from_millis(100)
        );
        assert_eq!(
            EffectPulse::new(0.0, 1.0).frame_rate(0).interval(),
            Duration::from_secs(1),
            "frame rates clamp to at least 1 fps"
        );
    }

    #[test]
    fn only_settling_amounts_reach_image_pixels() {
        assert_eq!(EffectAmount::fixed(0.2).settled(), Some(0.2));
        assert_eq!(EffectAmount::animated(3, 0.4).settled(), Some(0.4));
        assert_eq!(EffectAmount::pulsing(4, 0.1).settled(), None);
    }

    #[test]
    fn late_bound_amounts_rest_outside_a_draw() {
        assert_eq!(EffectAmount::animated(u16::MAX, 0.4).resolved(), 0.4);
        assert_eq!(
            EffectAmount::pulsing(u16::MAX, 0.2).resolved(),
            0.2,
            "a pulse rests at the start of its cycle"
        );
    }

    #[test]
    fn clamping_keeps_resting_values_in_the_unit_range() {
        assert_eq!(
            EffectAmount::fixed(1.5).clamped_unit(),
            EffectAmount::fixed(1.0)
        );
        assert_eq!(
            EffectAmount::pulsing(2, -0.5).clamped_unit(),
            EffectAmount::pulsing(2, 0.0)
        );
    }

    #[test]
    fn effect_amount_stays_compact() {
        assert_eq!(std::mem::size_of::<EffectAmount>(), 8);
    }

    /// Every `Style` carries two transforms; an amount that grew would grow every style with it.
    /// A pulse once stored inline took `Style` from 68 to 160 bytes and overflowed a debug stack.
    #[test]
    fn style_stays_within_its_size_budget() {
        assert!(
            std::mem::size_of::<crate::style::Style>() <= 80,
            "Style is {} bytes",
            std::mem::size_of::<crate::style::Style>()
        );
    }

    #[cfg(feature = "terminal-serde")]
    #[test]
    fn amounts_serialize_as_their_resting_number() {
        let pulse = EffectAmount::pulsing(0, 0.2);
        assert_eq!(serde_json::to_string(&pulse).unwrap(), "0.2");
        assert_eq!(
            serde_json::from_str::<EffectAmount>("0.2").unwrap(),
            EffectAmount::fixed(0.2)
        );
    }
}
