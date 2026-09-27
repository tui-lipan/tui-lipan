//! Scalar amounts for render-time color transforms.
//!
//! A [`ColorTransform`](crate::style::ColorTransform) carries an [`EffectAmount`] rather than a
//! bare `f32` so the amount can be *late-bound*: named in the element tree and resolved by the
//! renderer while it paints. That is what lets an animated dim, tint, or opacity advance with a
//! repaint instead of a `view()` pass, exactly as [`Paint::Animated`](crate::style::Paint::Animated)
//! does for colors.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::time::Duration;

use crate::animation::Easing;

/// Default length of one [`EffectPulse`] cycle.
const DEFAULT_PULSE_PERIOD: Duration = Duration::from_millis(1500);
/// Default repaint rate of an [`EffectPulse`]. A breathing tint is subtle; it does not need the
/// app's full geometry frame rate.
const DEFAULT_PULSE_FRAME_RATE: u16 = 30;

/// The strength of a render-time color transform: a fixed number, or one the renderer resolves.
///
/// Builders that take an amount accept `impl Into<EffectAmount>`, so a plain `f32` keeps working:
///
/// ```
/// use tui_lipan::prelude::*;
///
/// let dimmed = EffectScope::new().dim_by(0.4).child(Text::new("inactive pane"));
/// ```
///
/// The two late-bound forms keep the element tree unchanged while the amount moves, which is what
/// makes advancing them a repaint instead of a rebuild:
///
/// - [`Context::animated_amount`](crate::Context::animated_amount) transitions toward a target the
///   app picks, like [`Context::animated_color`](crate::Context::animated_color) does for colors.
/// - [`EffectAmount::pulse`] oscillates forever on a clock the renderer owns - a breathing alert
///   tint needs no app-side timer or target toggling.
///
/// ```
/// use std::time::Duration;
/// use tui_lipan::prelude::*;
///
/// let breathing = EffectScope::new()
///     .tint_by(
///         Color::Rgb(220, 60, 60),
///         EffectAmount::pulse(0.08, 0.20)
///             .period(Duration::from_millis(1400))
///             .frame_rate(10),
///     )
///     .child(Text::new("needs attention"));
/// ```
///
/// Late-bound amounts only resolve while the renderer paints. Read outside a paint, they answer
/// with their [`resting_value`](Self::resting_value): the transition's target, or the pulse's
/// starting value.
///
/// An `EffectAmount` is eight bytes and `Copy`, like the `f32` it replaces. A pulse's parameters
/// are interned process-wide and the amount holds their id; see [`EffectPulse`].
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct EffectAmount(Repr);

#[derive(Clone, Copy)]
enum Repr {
    /// A plain amount, baked into the element tree.
    Fixed(f32),
    /// A transition the app owns, named by its registry slot. Compares equal for the whole
    /// transition, because it does not embed where the transition currently is.
    Animated { slot: u16, target: f32 },
    /// An interned [`EffectPulse`].
    Pulse(u16),
}

// `f32` fields compare and hash by bit pattern, like every other effect parameter.
impl PartialEq for Repr {
    fn eq(&self, other: &Self) -> bool {
        match (*self, *other) {
            (Self::Fixed(a), Self::Fixed(b)) => a.to_bits() == b.to_bits(),
            (
                Self::Animated {
                    slot: slot_a,
                    target: target_a,
                },
                Self::Animated {
                    slot: slot_b,
                    target: target_b,
                },
            ) => slot_a == slot_b && target_a.to_bits() == target_b.to_bits(),
            (Self::Pulse(a), Self::Pulse(b)) => a == b,
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
            Self::Pulse(id) => {
                2u8.hash(state);
                id.hash(state);
            }
        }
    }
}

impl fmt::Debug for EffectAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.0 {
            Repr::Fixed(value) => f.debug_tuple("Fixed").field(&value).finish(),
            Repr::Animated { slot, target } => f
                .debug_struct("Animated")
                .field("slot", &slot)
                .field("target", &target)
                .finish(),
            Repr::Pulse(_) => match self.as_pulse() {
                Some(pulse) => f.debug_tuple("Pulse").field(&pulse).finish(),
                None => f.write_str("Pulse(?)"),
            },
        }
    }
}

impl EffectAmount {
    /// A fixed amount. The `const` form of `EffectAmount::from(value)`.
    pub const fn fixed(value: f32) -> Self {
        Self(Repr::Fixed(value))
    }

    /// An amount that oscillates between `from` and `to` on the renderer's clock.
    ///
    /// Returns an [`EffectPulse`] to configure; it converts into an `EffectAmount` wherever one is
    /// accepted.
    pub fn pulse(from: f32, to: f32) -> EffectPulse {
        EffectPulse::new(from, to)
    }

    /// A late-bound amount naming registry `slot`, settling on `target`.
    pub(crate) fn animated(slot: u16, target: f32) -> Self {
        Self(Repr::Animated { slot, target })
    }

    /// The amount when it is fixed, or `None` when the renderer resolves it.
    pub fn as_fixed(self) -> Option<f32> {
        match self.0 {
            Repr::Fixed(value) => Some(value),
            Repr::Animated { .. } | Repr::Pulse(_) => None,
        }
    }

    /// The pulse this amount follows, if it is one.
    pub fn as_pulse(self) -> Option<EffectPulse> {
        match self.0 {
            Repr::Pulse(id) => pulse_by_id(id),
            Repr::Fixed(_) | Repr::Animated { .. } => None,
        }
    }

    /// Whether this amount comes from [`Context::animated_amount`](crate::Context::animated_amount).
    pub fn is_transition(self) -> bool {
        matches!(self.0, Repr::Animated { .. })
    }

    /// Whether the renderer, rather than the element tree, decides this amount.
    pub fn is_late_bound(self) -> bool {
        !matches!(self.0, Repr::Fixed(_))
    }

    /// A single number standing in for this amount where no live value can be had: the fixed
    /// value, the transition's target, or the pulse's starting value.
    pub fn resting_value(self) -> f32 {
        match self.0 {
            Repr::Fixed(value) | Repr::Animated { target: value, .. } => value,
            Repr::Pulse(_) => self.as_pulse().map_or(0.0, |pulse| pulse.from),
        }
    }

    /// The amount as the renderer sees it right now.
    ///
    /// A transition slot resolves against the registry installed for the current draw, and a
    /// pulse against the draw's clock. Outside a draw, both answer with their resting value.
    pub(crate) fn resolved(self) -> f32 {
        match self.0 {
            Repr::Fixed(value) => value,
            Repr::Animated { slot, target } => {
                crate::animation::registry::resolve_render_scalar_slot(slot).unwrap_or(target)
            }
            Repr::Pulse(_) => self.as_pulse().map_or(0.0, |pulse| {
                pulse.value_at(crate::animation::registry::render_elapsed())
            }),
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
            Repr::Pulse(_) => None,
        }
    }

    /// Repaint cadence this amount needs by itself, if it changes without the tree changing.
    ///
    /// Only a pulse reports one: a transition is advanced by the animation registry's own ticker.
    pub(crate) fn animation_interval(self) -> Option<Duration> {
        self.as_pulse().map(EffectPulse::interval)
    }

    /// This amount limited to `[0.0, 1.0]`.
    pub(crate) fn clamped_unit(self) -> Self {
        let unit = |value: f32| value.clamp(0.0, 1.0);
        match self.0 {
            Repr::Fixed(value) => Self::fixed(unit(value)),
            Repr::Animated { slot, target } => Self::animated(slot, unit(target)),
            Repr::Pulse(_) => match self.as_pulse() {
                Some(pulse) => EffectPulse {
                    from: unit(pulse.from),
                    to: unit(pulse.to),
                    ..pulse
                }
                .into(),
                None => self,
            },
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

impl From<EffectPulse> for EffectAmount {
    /// Interns `pulse`. Should the table ever fill, the amount degrades to the pulse's starting
    /// value rather than naming the wrong pulse.
    fn from(pulse: EffectPulse) -> Self {
        match intern_pulse(pulse) {
            Some(id) => Self(Repr::Pulse(id)),
            None => Self::fixed(pulse.from),
        }
    }
}

/// Pulse parameters by id. Interning keeps [`EffectAmount`] - and so every `ColorTransform` and
/// `Style` - as small as the `f32` it replaced. Equal pulses share an id, so an amount built from
/// the same parameters every `view()` compares equal. Process-wide rather than per runtime because
/// styles are `Send` and outlive any one runtime.
static PULSES: Mutex<Vec<EffectPulse>> = Mutex::new(Vec::new());

fn intern_pulse(pulse: EffectPulse) -> Option<u16> {
    let mut pulses = PULSES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(id) = pulses.iter().position(|known| *known == pulse) {
        return u16::try_from(id).ok();
    }
    let id = u16::try_from(pulses.len()).ok()?;
    pulses.push(pulse);
    Some(id)
}

fn pulse_by_id(id: u16) -> Option<EffectPulse> {
    PULSES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(usize::from(id))
        .copied()
}

// Serialized as its resting value. A late-bound amount names state that lives in one running
// app - a registry slot, that app's clock - so another process could not resolve it anyway, and a
// plain number keeps the wire format what it was when transforms carried an `f32`.
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

/// An [`EffectAmount`] that oscillates between two values for as long as it is in the tree.
///
/// One cycle runs `from -> to -> from` over [`period`](Self::period), shaped by
/// [`easing`](Self::easing) on each half. The default `EaseInOutSine` makes that a smooth breath
/// with no visible turnaround.
///
/// The pulse is evaluated from the renderer's monotonic clock, not counted in frames, so a delayed
/// frame does not slow it down, and every pulse with the same period breathes in step. An effect
/// scope holding one asks for repaints at [`frame_rate`](Self::frame_rate) and never re-runs
/// `view()`.
///
/// Image pixels do not follow a pulse: recoloring and re-encoding a picture on every breath is
/// the cost the pulse exists to avoid. Text and cell colors breathe; images under the same scope
/// are left untouched by the pulsing transform.
///
/// Converting a pulse into an [`EffectAmount`] interns its parameters for the life of the process,
/// so build pulses from a fixed set of parameters - not from a value that changes every frame.
/// To move an amount toward a changing value, use
/// [`Context::animated_amount`](crate::Context::animated_amount).
#[derive(Clone, Copy, Debug)]
pub struct EffectPulse {
    from: f32,
    to: f32,
    period: Duration,
    easing: Easing,
    frame_rate: u16,
}

// `f32` fields compare and hash by bit pattern, like every other effect parameter.
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

    /// Repaint rate while the pulse is on screen, clamped to 1-480 fps. Defaults to 30.
    ///
    /// Several animated effects on screen share the fastest cadence among them.
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

    /// The pulse's value `elapsed` into the renderer's clock.
    pub fn value_at(self, elapsed: Duration) -> f32 {
        let period = self.period.as_nanos().max(1);
        let t = (elapsed.as_nanos() % period) as f64 / period as f64;
        // Triangle wave: 0 at the start of the cycle, 1 halfway, back to 0 at the end.
        let rise = (1.0 - (2.0 * t - 1.0).abs()) as f32;
        let eased = self.easing.apply(rise);
        self.from + (self.to - self.from) * eased
    }

    /// Time between repaints while the pulse is on screen.
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
        let pulse = EffectAmount::pulse(0.1, 0.5)
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
        let pulse = EffectPulse::new(0.0, 1.0).frame_rate(10);
        assert_eq!(pulse.interval(), Duration::from_millis(100));
        assert_eq!(
            EffectAmount::from(pulse).animation_interval(),
            Some(Duration::from_millis(100))
        );
        assert_eq!(EffectAmount::fixed(0.3).animation_interval(), None);
        assert_eq!(
            EffectPulse::new(0.0, 1.0).frame_rate(0).interval(),
            Duration::from_secs(1),
            "frame rates clamp to at least 1 fps"
        );
    }

    #[test]
    fn only_settling_amounts_reach_image_pixels() {
        let transition = EffectAmount::animated(3, 0.4);
        assert_eq!(EffectAmount::fixed(0.2).settled(), Some(0.2));
        assert_eq!(transition.settled(), Some(0.4));
        assert_eq!(
            EffectAmount::from(EffectPulse::new(0.1, 0.3)).settled(),
            None
        );
    }

    #[test]
    fn late_bound_amounts_fall_back_outside_a_draw() {
        let transition = EffectAmount::animated(u16::MAX, 0.4);
        assert_eq!(
            transition.resolved(),
            0.4,
            "a transition rests at its target"
        );
        let pulse = EffectAmount::from(EffectPulse::new(0.2, 0.6));
        assert_eq!(
            pulse.resolved(),
            0.2,
            "no draw clock: the start of the cycle"
        );
    }

    #[test]
    fn clamping_keeps_every_form_in_the_unit_range() {
        assert_eq!(
            EffectAmount::fixed(1.5).clamped_unit(),
            EffectAmount::fixed(1.0)
        );
        let pulse = EffectAmount::from(EffectPulse::new(-0.5, 2.0))
            .clamped_unit()
            .as_pulse()
            .expect("a pulse stays a pulse");
        assert_eq!((pulse.from_value(), pulse.to_value()), (0.0, 1.0));
    }

    #[test]
    fn equal_pulses_share_one_interned_id() {
        let a = EffectAmount::from(EffectPulse::new(0.11, 0.33).frame_rate(12));
        let b = EffectAmount::from(EffectPulse::new(0.11, 0.33).frame_rate(12));
        let c = EffectAmount::from(EffectPulse::new(0.11, 0.34).frame_rate(12));
        assert_eq!(a, b, "a pulse rebuilt every view() compares equal");
        assert_ne!(a, c);
        assert_eq!(
            a.as_pulse(),
            Some(EffectPulse::new(0.11, 0.33).frame_rate(12))
        );
    }

    #[test]
    fn an_amount_stays_as_small_as_the_number_it_replaced() {
        assert_eq!(std::mem::size_of::<EffectAmount>(), 8);
    }

    #[cfg(feature = "terminal-serde")]
    #[test]
    fn amounts_serialize_as_their_resting_number() {
        let pulse = EffectAmount::from(EffectPulse::new(0.2, 0.6));
        assert_eq!(serde_json::to_string(&pulse).unwrap(), "0.2");
        assert_eq!(
            serde_json::from_str::<EffectAmount>("0.2").unwrap(),
            EffectAmount::fixed(0.2)
        );
    }
}
