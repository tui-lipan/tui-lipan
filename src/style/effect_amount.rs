//! Scalar amounts for render-time color transforms.
//!
//! A [`ColorTransform`](crate::style::ColorTransform) carries an [`EffectAmount`] rather than a
//! bare `f32` so the amount can be *late-bound*: named in the element tree and resolved by the
//! renderer while it paints. That is what lets an animated dim, tint, or opacity advance with a
//! repaint instead of a `view()` pass, exactly as [`Paint::Animated`](crate::style::Paint::Animated)
//! does for colors.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::time::Duration;

use crate::animation::{AnimationHandle, Easing};

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
/// An `EffectAmount` is eight bytes and `Copy`: a tag and an
/// [`AnimationHandle`] packed into one word, beside the value.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectAmount {
    /// Kind in the top two bits; for a late-bound amount, its packed handle below them.
    meta: u32,
    /// The fixed value, the transition's target, or the pulse's `from`, as `f32` bits. Compared
    /// and hashed by bit pattern, like every other effect parameter.
    value: u32,
}

const KIND_SHIFT: u32 = 30;
const KIND_FIXED: u32 = 0;
const KIND_TRANSITION: u32 = 1;
const KIND_PULSE: u32 = 2;

impl fmt::Debug for EffectAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let value = self.resting_value();
        match (self.kind(), self.handle()) {
            (KIND_TRANSITION, Some(handle)) => f
                .debug_struct("Animated")
                .field("handle", &handle)
                .field("target", &value)
                .finish(),
            (KIND_PULSE, Some(handle)) => f
                .debug_struct("Pulse")
                .field("handle", &handle)
                .field("from", &value)
                .finish(),
            _ => f.debug_tuple("Fixed").field(&value).finish(),
        }
    }
}

impl EffectAmount {
    /// A fixed amount. The `const` form of `EffectAmount::from(value)`.
    pub const fn fixed(value: f32) -> Self {
        Self {
            meta: KIND_FIXED << KIND_SHIFT,
            value: value.to_bits(),
        }
    }

    fn late_bound(kind: u32, handle: AnimationHandle, value: f32) -> Self {
        Self {
            meta: (kind << KIND_SHIFT) | handle.pack(),
            value: value.to_bits(),
        }
    }

    /// A late-bound transition named by `handle`, settling on `target`.
    pub(crate) fn animated(handle: AnimationHandle, target: f32) -> Self {
        Self::late_bound(KIND_TRANSITION, handle, target)
    }

    /// A late-bound pulse named by `handle`, resting at `from`.
    pub(crate) fn pulsing(handle: AnimationHandle, from: f32) -> Self {
        Self::late_bound(KIND_PULSE, handle, from)
    }

    fn kind(self) -> u32 {
        self.meta >> KIND_SHIFT
    }

    /// The registry handle of a late-bound amount.
    pub(crate) fn handle(self) -> Option<AnimationHandle> {
        (self.kind() != KIND_FIXED).then(|| AnimationHandle::unpack(self.meta))
    }

    /// The amount when it is fixed, or `None` when the renderer resolves it.
    pub fn as_fixed(self) -> Option<f32> {
        (self.kind() == KIND_FIXED).then(|| self.resting_value())
    }

    /// Whether this amount comes from [`Context::animated_amount`](crate::Context::animated_amount).
    pub fn is_transition(self) -> bool {
        self.kind() == KIND_TRANSITION
    }

    /// Whether this amount comes from [`Context::pulsing_amount`](crate::Context::pulsing_amount).
    pub fn is_pulse(self) -> bool {
        self.kind() == KIND_PULSE
    }

    /// Whether the renderer, rather than the element tree, decides this amount.
    pub fn is_late_bound(self) -> bool {
        self.kind() != KIND_FIXED
    }

    /// A single number standing in for this amount where no live value can be had: the fixed
    /// value, the transition's target, or the pulse's starting value.
    pub fn resting_value(self) -> f32 {
        f32::from_bits(self.value)
    }

    /// The amount as the renderer sees it right now.
    ///
    /// A late-bound amount reads its registry entry, which holds one sampled value between
    /// animation ticks - so every paint in between, including a partial one, agrees on it.
    /// Outside a draw, or once its animation is gone, it answers with its resting value.
    pub(crate) fn resolved(self) -> f32 {
        match self.handle() {
            None => self.resting_value(),
            Some(handle) => crate::animation::registry::resolve_render_scalar(handle)
                .unwrap_or_else(|| self.resting_value()),
        }
    }

    /// The value an image recolor may bake in, or `None` when the amount never settles.
    ///
    /// Following a late-bound amount per frame would re-encode every image under it on every
    /// frame. A transition is recorded at its target instead - one encode for the whole fade - and
    /// a pulse is left out of image pixels entirely.
    pub(crate) fn settled(self) -> Option<f32> {
        (!self.is_pulse()).then(|| self.resting_value())
    }

    /// This amount limited to `[0.0, 1.0]`.
    ///
    /// A late-bound amount only has its resting value clamped: the live value belongs to the
    /// registry, and every transform consumer clamps what it reads.
    pub(crate) fn clamped_unit(self) -> Self {
        Self {
            value: self.resting_value().clamp(0.0, 1.0).to_bits(),
            ..self
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

    fn handle(slot: u16) -> AnimationHandle {
        AnimationHandle::new(slot, 0)
    }

    #[test]
    fn only_settling_amounts_reach_image_pixels() {
        assert_eq!(EffectAmount::fixed(0.2).settled(), Some(0.2));
        assert_eq!(EffectAmount::animated(handle(3), 0.4).settled(), Some(0.4));
        assert_eq!(EffectAmount::pulsing(handle(4), 0.1).settled(), None);
    }

    #[test]
    fn late_bound_amounts_rest_outside_a_draw() {
        assert_eq!(
            EffectAmount::animated(handle(u16::MAX), 0.4).resolved(),
            0.4
        );
        assert_eq!(
            EffectAmount::pulsing(handle(u16::MAX), 0.2).resolved(),
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
            EffectAmount::pulsing(handle(2), -0.5).clamped_unit(),
            EffectAmount::pulsing(handle(2), 0.0)
        );
    }

    #[test]
    fn late_bound_amounts_keep_their_kind_handle_and_value() {
        let handle = AnimationHandle::new(0xBEEF, 0x3ABC);
        let transition = EffectAmount::animated(handle, 0.4);
        let pulse = EffectAmount::pulsing(handle, 0.1);
        assert!(transition.is_transition() && !transition.is_pulse());
        assert!(pulse.is_pulse() && !pulse.is_transition());
        assert_eq!(transition.handle(), Some(handle));
        assert_eq!(pulse.handle(), Some(handle));
        assert_eq!(
            (transition.resting_value(), pulse.resting_value()),
            (0.4, 0.1)
        );
        assert_ne!(transition, pulse);
        assert_ne!(
            transition,
            EffectAmount::animated(AnimationHandle::new(0xBEEF, 0x3ABD), 0.4),
            "a new generation is a different amount"
        );
        assert_eq!(EffectAmount::fixed(0.4).handle(), None);
    }

    #[test]
    fn effect_amount_stays_compact() {
        assert_eq!(std::mem::size_of::<EffectAmount>(), 8);
    }

    /// Every `Style` carries two transforms and three paints; a token that grew would grow every
    /// style with it. A pulse once stored inline took `Style` from 68 to 160 bytes and overflowed a
    /// debug stack. Generation-tagged handles cost 6 bytes of that budget (76 -> 82).
    #[test]
    fn style_stays_within_its_size_budget() {
        assert!(
            std::mem::size_of::<crate::style::Style>() <= 84,
            "Style is {} bytes",
            std::mem::size_of::<crate::style::Style>()
        );
    }

    #[cfg(feature = "terminal-serde")]
    #[test]
    fn amounts_serialize_as_their_resting_number() {
        let pulse = EffectAmount::pulsing(handle(0), 0.2);
        assert_eq!(serde_json::to_string(&pulse).unwrap(), "0.2");
        assert_eq!(
            serde_json::from_str::<EffectAmount>("0.2").unwrap(),
            EffectAmount::fixed(0.2)
        );
    }
}
