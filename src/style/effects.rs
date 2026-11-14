use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use crate::animation::Easing;
use crate::app::ContrastPolicy;
use crate::core::mask::CellMask;
use crate::style::{Align, Color, ColorTransform, EffectAmount, Rect};
use crate::utils::gradient::ColorGradient;

/// Terminal cell type passed to [`CellEffect::apply`].
pub use ratatui::buffer::Cell as EffectCell;
/// Terminal color type used by custom visual effects.
pub use ratatui::style::Color as TerminalColor;

/// Per-cell inputs supplied to custom visual effects.
///
/// Produced by the renderer and consumed by effects, so it is `#[non_exhaustive]`: new inputs can
/// be added without breaking effect code. To unit-test an effect, build one with
/// [`EffectContext::new`] and the `with_*` setters.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct EffectContext {
    /// Absolute frame column for the cell being processed.
    pub x: i16,
    /// Absolute frame row for the cell being processed.
    pub y: i16,
    /// Absolute effect-scope rect in terminal cells.
    pub bounds: Rect,
    /// Renderer animation phase counter.
    ///
    /// Counts effect ticks, so it slows down when frames are delayed. Use [`Self::elapsed`] for
    /// animation that has to keep time.
    pub phase: u64,
    /// Monotonic runtime clock at the start of this draw.
    ///
    /// Time since the runtime started, following its virtual clock under headless capture and
    /// [`TestBackend::advance`](crate::TestBackend::advance). A time-based animation evaluated
    /// from this keeps its pace however irregular the frames are. It is read per draw, so a
    /// partial repaint between two of the effect's own ticks sees a later time than the cells
    /// it leaves alone; quantize to [`CellEffect::animation_interval`] if that matters.
    pub elapsed: Duration,
    /// Resolved terminal background color, when known.
    pub terminal_bg: Option<TerminalColor>,
}

/// Per-frame inputs supplied before a custom visual effect processes a scope.
///
/// `#[non_exhaustive]` for the same reason as [`EffectContext`]; build one with
/// [`EffectPrepareContext::new`] to unit-test a [`CellEffect::prepare`].
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct EffectPrepareContext {
    /// Absolute effect-scope rect in terminal cells.
    pub bounds: Rect,
    /// Renderer animation phase counter.
    ///
    /// Counts effect ticks, so it slows down when frames are delayed. Use [`Self::elapsed`] for
    /// animation that has to keep time.
    pub phase: u64,
    /// Monotonic runtime clock at the start of this draw.
    ///
    /// Time since the runtime started, following its virtual clock under headless capture and
    /// [`TestBackend::advance`](crate::TestBackend::advance). A time-based animation evaluated
    /// from this keeps its pace however irregular the frames are. It is read per draw, so a
    /// partial repaint between two of the effect's own ticks sees a later time than the cells
    /// it leaves alone; quantize to [`CellEffect::animation_interval`] if that matters.
    pub elapsed: Duration,
    /// Resolved terminal background color, when known.
    pub terminal_bg: Option<TerminalColor>,
}

impl EffectContext {
    /// Inputs for the cell at absolute `(x, y)` inside a scope covering `bounds`, at phase `0`,
    /// clock zero, and no known terminal background.
    pub fn new(x: i16, y: i16, bounds: Rect) -> Self {
        Self {
            x,
            y,
            bounds,
            phase: 0,
            elapsed: Duration::ZERO,
            terminal_bg: None,
        }
    }

    /// Set the renderer animation phase.
    pub fn with_phase(mut self, phase: u64) -> Self {
        self.phase = phase;
        self
    }

    /// Set the runtime clock reading.
    pub fn with_elapsed(mut self, elapsed: Duration) -> Self {
        self.elapsed = elapsed;
        self
    }

    /// Set the resolved terminal background.
    pub fn with_terminal_bg(mut self, terminal_bg: Option<TerminalColor>) -> Self {
        self.terminal_bg = terminal_bg;
        self
    }
}

impl EffectPrepareContext {
    /// Inputs for a scope covering `bounds`, at phase `0`, clock zero, and no known terminal
    /// background.
    pub fn new(bounds: Rect) -> Self {
        Self {
            bounds,
            phase: 0,
            elapsed: Duration::ZERO,
            terminal_bg: None,
        }
    }

    /// Set the renderer animation phase.
    pub fn with_phase(mut self, phase: u64) -> Self {
        self.phase = phase;
        self
    }

    /// Set the runtime clock reading.
    pub fn with_elapsed(mut self, elapsed: Duration) -> Self {
        self.elapsed = elapsed;
        self
    }

    /// Set the resolved terminal background.
    pub fn with_terminal_bg(mut self, terminal_bg: Option<TerminalColor>) -> Self {
        self.terminal_bg = terminal_bg;
        self
    }
}

/// Prepared per-cell visual effect for one render phase and effect-scope bounds.
///
/// Implement this when a [`CellEffect`] has expensive values that can be computed once
/// per frame instead of once per cell.
pub trait PreparedCellEffect: Send + Sync + std::fmt::Debug + 'static {
    /// Apply this prepared effect to one cell inside the prepared scope.
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext);

    /// Apply this prepared effect with the cell that lay beneath the scope before it painted.
    ///
    /// Called instead of [`Self::apply`] when the effect that prepared this one reports
    /// [`CellEffect::uses_backdrop`]. The default ignores the backdrop.
    fn apply_with_backdrop(
        &self,
        cell: &mut EffectCell,
        backdrop: &EffectCell,
        ctx: &EffectContext,
    ) {
        let _ = backdrop;
        self.apply(cell, ctx);
    }
}

/// User-defined per-cell visual post-processing effect.
///
/// Effects must be `'static` because `VisualEffect::Custom` stores them in an
/// [`Arc`]. Store owned data in the effect rather than borrowing component state.
pub trait CellEffect: Send + Sync + std::fmt::Debug + 'static {
    /// Apply this effect to one cell. Called once per cell inside the effect scope, per frame.
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext);

    /// Whether this effect reads its **backdrop**: the cells that lay beneath the effect scope
    /// before the scope's children painted over them.
    ///
    /// Return `true` and the renderer keeps a copy of that area each frame and calls
    /// [`Self::apply_with_backdrop`] instead of [`Self::apply`]. That turns an effect from a
    /// post-process into a compositor: it can let what was underneath show through per cell,
    /// which is what reveals, wipes, dissolves, and per-cell crossfades between two layers need.
    ///
    /// Stack the outgoing content under an [`EffectScope`](crate::widgets::EffectScope) in a
    /// [`ZStack`](crate::widgets::ZStack) (for example retained with
    /// [`Animated::auto_exit`](crate::widgets::Animated::auto_exit)) and the scope sees it as its
    /// backdrop. Where no pre-paint copy exists - an effect applied through a
    /// [`Style`](crate::style::Style) or a hover effect - the backdrop is the cell as painted, so a
    /// backdrop-reading effect degrades to leaving the cell alone.
    ///
    /// The copy costs one cell clone per scope cell per frame, so return `true` only while the
    /// effect actually composites; an idle reveal should report `false`.
    fn uses_backdrop(&self) -> bool {
        false
    }

    /// Apply this effect with `backdrop`, the cell that lay beneath this one before the scope
    /// painted. Called instead of [`Self::apply`] when [`Self::uses_backdrop`] returns `true`.
    ///
    /// `*cell = backdrop.clone()` reveals the backdrop at this cell; leaving `cell` alone keeps
    /// the scope's content. The default ignores the backdrop and calls [`Self::apply`].
    fn apply_with_backdrop(
        &self,
        cell: &mut EffectCell,
        backdrop: &EffectCell,
        ctx: &EffectContext,
    ) {
        let _ = backdrop;
        self.apply(cell, ctx);
    }

    /// Prepare this effect once for the current scope bounds and render phase.
    ///
    /// Return `Some` when frame-constant work can be reused for every cell. The renderer
    /// will call the prepared effect's `apply` method for cells in this scope. The default
    /// returns `None`, preserving the direct per-cell [`Self::apply`] path.
    fn prepare(&self, _ctx: &EffectPrepareContext) -> Option<Box<dyn PreparedCellEffect>> {
        None
    }

    /// Visible image cells inside an `EffectScope`, sampled once before its children paint.
    ///
    /// With `terminal-images` enabled, return a scope-local mask to clip Kitty child images and
    /// captured image cells to its set bits. This lets a reveal or
    /// dissolve remove picture fragments along with text without re-encoding image pixels.
    /// Cells outside the mask are hidden. `None` leaves images unchanged, and `cells_only`
    /// scopes ignore the mask. Style and hover effects do not use image masks.
    ///
    /// Use the same elapsed-time sample as [`Self::prepare`] so pictures and cells agree.
    fn image_mask(&self, _ctx: &EffectPrepareContext) -> Option<CellMask> {
        None
    }

    /// Whether this effect requires a redraw every frame.
    fn is_animated(&self) -> bool {
        false
    }

    /// Preferred interval between animation ticks for this effect.
    ///
    /// The renderer uses the smallest interval requested by animated effects in the tree.
    /// Non-animated effects ignore this value. The default preserves the framework's
    /// historical ~60 FPS animation cadence.
    fn animation_interval(&self) -> Duration {
        Duration::from_millis(16)
    }

    /// Stable cache key for layout-hash purposes.
    ///
    /// The default `0` is safe: custom effects also hash their `Arc` identity.
    fn cache_key(&self) -> u64 {
        0
    }
}

/// Direction used by directional visual effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum EffectAxis {
    /// Apply the effect left-to-right.
    Horizontal,
    /// Apply the effect top-to-bottom.
    Vertical,
    /// Apply the effect diagonally.
    Diagonal,
}

/// Color channels a [`VisualEffect`] may modify.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum EffectChannels {
    /// Allow the effect to modify foreground and background colors.
    #[default]
    Both,
    /// Allow the effect to modify only foreground colors.
    Foreground,
    /// Allow the effect to modify only background colors.
    Background,
}

/// Quantization palette presets for post-processing.
///
/// `#[non_exhaustive]` so presets can be added in minor releases.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum EffectPalette {
    /// CGA palette.
    Cga,
    /// Nintendo Game Boy style green palette.
    Gameboy,
    /// Amber monochrome terminal palette.
    Amber,
    /// Green phosphor monochrome terminal palette.
    Green,
    /// User-provided custom palette.
    Custom(Vec<Color>),
}

/// Preset bundles for CRT-style retro rendering.
///
/// `#[non_exhaustive]` so presets can be added in minor releases.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
#[non_exhaustive]
pub enum RetroPreset {
    /// Amber phosphor-like CRT look.
    Amber,
    /// Green phosphor-like CRT look.
    Green,
    /// CGA-like CRT look.
    Cga,
    /// Game Boy-like LCD/retro look.
    Gameboy,
    /// Fallout-style Vault-Tec monitor look.
    VaultTec,
}

/// Two-dimensional alignment used to resolve effect positions from an effect scope.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct EffectAlignment {
    /// Horizontal alignment inside the effect scope.
    pub horizontal: Align,
    /// Vertical alignment inside the effect scope.
    pub vertical: Align,
}

impl EffectAlignment {
    /// Top-left corner of the effect scope.
    pub const TOP_LEFT: Self = Self::new(Align::Start, Align::Start);
    /// Top-center edge of the effect scope.
    pub const TOP_CENTER: Self = Self::new(Align::Center, Align::Start);
    /// Top-right corner of the effect scope.
    pub const TOP_RIGHT: Self = Self::new(Align::End, Align::Start);
    /// Center-left edge of the effect scope.
    pub const CENTER_LEFT: Self = Self::new(Align::Start, Align::Center);
    /// Center of the effect scope.
    pub const CENTER: Self = Self::new(Align::Center, Align::Center);
    /// Center-right edge of the effect scope.
    pub const CENTER_RIGHT: Self = Self::new(Align::End, Align::Center);
    /// Bottom-left corner of the effect scope.
    pub const BOTTOM_LEFT: Self = Self::new(Align::Start, Align::End);
    /// Bottom-center edge of the effect scope.
    pub const BOTTOM_CENTER: Self = Self::new(Align::Center, Align::End);
    /// Bottom-right corner of the effect scope.
    pub const BOTTOM_RIGHT: Self = Self::new(Align::End, Align::End);

    /// Create a two-dimensional effect alignment.
    pub const fn new(horizontal: Align, vertical: Align) -> Self {
        Self {
            horizontal,
            vertical,
        }
    }
}

/// Origin for positional visual effects.
#[derive(Clone, Copy, Debug)]
pub enum EffectOrigin {
    /// Explicit cell coordinates relative to the effect scope's top-left corner.
    Cell {
        /// Horizontal coordinate, in columns from the effect scope's left edge.
        x: f32,
        /// Vertical coordinate, in rows from the effect scope's top edge.
        y: f32,
    },
    /// Alignment resolved from the effect scope's dimensions at render time.
    Aligned(EffectAlignment),
}

impl EffectOrigin {
    /// Center of the effect scope.
    pub const CENTER: Self = Self::Aligned(EffectAlignment::CENTER);

    /// Create an explicit cell origin from `f32` coordinates.
    pub fn cell(x: f32, y: f32) -> Self {
        Self::Cell { x, y }
    }

    /// Create an aligned origin.
    pub const fn aligned(alignment: EffectAlignment) -> Self {
        Self::Aligned(alignment)
    }

    /// Resolve this origin to scope-local coordinates.
    pub fn resolve(self, width: u16, height: u16) -> (f32, f32) {
        match self {
            Self::Cell { x, y } => (x, y),
            Self::Aligned(alignment) => (
                resolve_effect_align(alignment.horizontal, width),
                resolve_effect_align(alignment.vertical, height),
            ),
        }
    }
}

impl PartialEq for EffectOrigin {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Cell { x: x_a, y: y_a }, Self::Cell { x: x_b, y: y_b }) => {
                x_a.to_bits() == x_b.to_bits() && y_a.to_bits() == y_b.to_bits()
            }
            (Self::Aligned(a), Self::Aligned(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for EffectOrigin {}

impl Hash for EffectOrigin {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Cell { x, y } => {
                0u8.hash(state);
                x.to_bits().hash(state);
                y.to_bits().hash(state);
            }
            Self::Aligned(alignment) => {
                1u8.hash(state);
                alignment.hash(state);
            }
        }
    }
}

/// Radius behavior for [`VisualEffect::Ripple`].
///
/// `#[non_exhaustive]`: match it with a wildcard arm, or call [`Self::resolve`] to get the ring's
/// radius and strength without matching.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub enum RippleRadius {
    /// Static ring radius in character columns.
    Fixed(f32),
    /// Ring radius and strength driven by the renderer animation phase. See [`RippleAnimation`].
    Animated(RippleAnimation),
}

impl RippleRadius {
    /// Repeat a shockwave from zero to `max_radius` every `period_ticks`, fading as it grows.
    ///
    /// Shorthand for [`RippleAnimation::looping`].
    pub fn looping(max_radius: f32, period_ticks: u32) -> Self {
        Self::Animated(RippleAnimation::looping(max_radius, period_ticks))
    }

    /// Play one shockwave out to `max_radius` from `start_tick` for `duration_ticks`.
    ///
    /// Shorthand for [`RippleAnimation::once`].
    pub fn once(max_radius: f32, duration_ticks: u32, start_tick: u64) -> Self {
        Self::Animated(RippleAnimation::once(
            max_radius,
            duration_ticks,
            start_tick,
        ))
    }

    /// Play one shockwave inward from `max_radius` into the origin, from `start_tick` for
    /// `duration_ticks`.
    ///
    /// Shorthand for [`RippleAnimation::collapse`].
    pub fn collapse(max_radius: f32, duration_ticks: u32, start_tick: u64) -> Self {
        Self::Animated(RippleAnimation::collapse(
            max_radius,
            duration_ticks,
            start_tick,
        ))
    }

    /// Whether the ring changes with the renderer phase, so the effect needs repaint ticks.
    pub fn is_animated(&self) -> bool {
        matches!(self, Self::Animated(_))
    }

    /// The ring's radius and strength multiplier at renderer phase `phase`, or `None` while an
    /// animation has not started or has already ended.
    ///
    /// This is the curve the renderer paints with, so an exporter or a custom effect that draws
    /// its own ring stays in step with the built-in one.
    pub fn resolve(&self, phase: u64) -> Option<(f32, f32)> {
        match self {
            Self::Fixed(radius) => Some((*radius, 1.0)),
            Self::Animated(animation) => animation.resolve(phase),
        }
    }
}

impl From<RippleAnimation> for RippleRadius {
    fn from(animation: RippleAnimation) -> Self {
        Self::Animated(animation)
    }
}

impl PartialEq for RippleRadius {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Fixed(a), Self::Fixed(b)) => a.to_bits() == b.to_bits(),
            (Self::Animated(a), Self::Animated(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for RippleRadius {}

impl Hash for RippleRadius {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Fixed(radius) => {
                0u8.hash(state);
                radius.to_bits().hash(state);
            }
            Self::Animated(animation) => {
                1u8.hash(state);
                animation.hash(state);
            }
        }
    }
}

/// How a [`RippleAnimation`] behaves once its `duration_ticks` have passed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RippleRepeat {
    /// Play once, then paint nothing.
    #[default]
    Once,
    /// Restart from the beginning every `duration_ticks`.
    Loop,
    /// Play forward, then backward, then forward again, each leg lasting `duration_ticks`.
    PingPong,
}

/// A ripple ring moving between two radii over renderer animation ticks.
///
/// The radius travels `from_radius -> to_radius` along `easing`; the strength multiplier travels
/// `from_strength -> to_strength` linearly over the same ticks, and scales
/// [`VisualEffect::Ripple`]'s `strength`. The animation starts at renderer phase `start_tick`
/// (capture it from `ctx.effect_phase()`) and paints nothing before it.
///
/// Start from a preset ([`Self::once`], [`Self::collapse`], [`Self::looping`]) or from
/// [`Self::new`], then adjust it with the `with_*` setters:
///
/// ```
/// use tui_lipan::prelude::*;
///
/// // A ring that springs out past its target and settles, brightening as it goes.
/// let start_tick = 0;
/// let radius = RippleAnimation::new(0.0, 12.0, 40)
///     .with_easing(Easing::EASE_OUT_BACK)
///     .with_strength(0.4, 1.0)
///     .starting_at(start_tick);
/// let effect = VisualEffect::Ripple {
///     origin: EffectOrigin::CENTER,
///     radius: radius.into(),
///     ring_width: 1.5,
///     tint: Color::Cyan,
///     strength: 0.6,
/// };
/// # let _ = effect;
/// ```
///
/// `#[non_exhaustive]` so new controls can be added without breaking callers; build one through
/// the constructors rather than a struct literal.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct RippleAnimation {
    /// Ring radius at the start of each play, in character columns.
    pub from_radius: f32,
    /// Ring radius at the end of each play, in character columns.
    pub to_radius: f32,
    /// Curve the radius follows from `from_radius` to `to_radius`.
    pub easing: Easing,
    /// Strength multiplier at the start of each play.
    pub from_strength: f32,
    /// Strength multiplier at the end of each play.
    pub to_strength: f32,
    /// Renderer animation ticks one play lasts. `0` is treated as `1`.
    pub duration_ticks: u32,
    /// Renderer animation phase the animation starts at.
    pub start_tick: u64,
    /// What happens after one play.
    pub repeat: RippleRepeat,
}

impl RippleAnimation {
    /// A single linear play from `from_radius` to `to_radius` over `duration_ticks`, at full
    /// strength, starting at phase `0`.
    pub fn new(from_radius: f32, to_radius: f32, duration_ticks: u32) -> Self {
        Self {
            from_radius,
            to_radius,
            easing: Easing::Linear,
            from_strength: 1.0,
            to_strength: 1.0,
            duration_ticks,
            start_tick: 0,
            repeat: RippleRepeat::Once,
        }
    }

    /// A shockwave leaving the origin: the ring grows from zero to `max_radius` easing out
    /// ([`Easing::EaseOutQuad`]) while its strength fades from full to zero.
    pub fn once(max_radius: f32, duration_ticks: u32, start_tick: u64) -> Self {
        Self::new(0.0, max_radius, duration_ticks)
            .with_easing(Easing::EaseOutQuad)
            .with_strength(1.0, 0.0)
            .starting_at(start_tick)
    }

    /// A shockwave arriving at the origin, the mirror of [`Self::once`]: the ring gathers from
    /// `max_radius` into the origin easing in ([`Easing::EaseInQuad`]), so it lands fast, while
    /// its strength rises from 30% to full.
    pub fn collapse(max_radius: f32, duration_ticks: u32, start_tick: u64) -> Self {
        Self::new(max_radius, 0.0, duration_ticks)
            .with_easing(Easing::EaseInQuad)
            .with_strength(0.3, 1.0)
            .starting_at(start_tick)
    }

    /// [`Self::once`] restarting every `period_ticks`, from phase `0`.
    pub fn looping(max_radius: f32, period_ticks: u32) -> Self {
        Self::once(max_radius, period_ticks, 0).with_repeat(RippleRepeat::Loop)
    }

    /// Set the curve the radius follows.
    pub fn with_easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    /// Set the strength multiplier at the start and end of each play.
    pub fn with_strength(mut self, from: f32, to: f32) -> Self {
        self.from_strength = from;
        self.to_strength = to;
        self
    }

    /// Set the renderer phase the animation starts at.
    pub fn starting_at(mut self, start_tick: u64) -> Self {
        self.start_tick = start_tick;
        self
    }

    /// Set what happens after one play.
    pub fn with_repeat(mut self, repeat: RippleRepeat) -> Self {
        self.repeat = repeat;
        self
    }

    /// The ring's radius and strength multiplier at renderer phase `phase`, or `None` before
    /// `start_tick` and after a [`RippleRepeat::Once`] play has ended.
    pub fn resolve(&self, phase: u64) -> Option<(f32, f32)> {
        let elapsed = phase.checked_sub(self.start_tick)?;
        let duration = u64::from(self.duration_ticks.max(1));
        let t = match self.repeat {
            RippleRepeat::Once => {
                if elapsed >= duration {
                    return None;
                }
                elapsed as f32 / duration as f32
            }
            RippleRepeat::Loop => (elapsed % duration) as f32 / duration as f32,
            RippleRepeat::PingPong => {
                let leg = (elapsed % (duration * 2)) as f32 / duration as f32;
                if leg <= 1.0 { leg } else { 2.0 - leg }
            }
        };
        let lerp = |from: f32, to: f32, t: f32| from + (to - from) * t;
        Some((
            lerp(self.from_radius, self.to_radius, self.easing.apply(t)),
            lerp(self.from_strength, self.to_strength, t),
        ))
    }
}

impl PartialEq for RippleAnimation {
    fn eq(&self, other: &Self) -> bool {
        self.from_radius.to_bits() == other.from_radius.to_bits()
            && self.to_radius.to_bits() == other.to_radius.to_bits()
            && self.easing == other.easing
            && self.from_strength.to_bits() == other.from_strength.to_bits()
            && self.to_strength.to_bits() == other.to_strength.to_bits()
            && self.duration_ticks == other.duration_ticks
            && self.start_tick == other.start_tick
            && self.repeat == other.repeat
    }
}

impl Eq for RippleAnimation {}

impl Hash for RippleAnimation {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.from_radius.to_bits().hash(state);
        self.to_radius.to_bits().hash(state);
        self.easing.hash(state);
        self.from_strength.to_bits().hash(state);
        self.to_strength.to_bits().hash(state);
        self.duration_ticks.hash(state);
        self.start_tick.hash(state);
        self.repeat.hash(state);
    }
}

fn resolve_effect_align(align: Align, length: u16) -> f32 {
    match align {
        Align::Start => 0.0,
        Align::Center | Align::Stretch => length as f32 * 0.5,
        Align::End => length as f32,
    }
}

/// Backend-agnostic visual post-processing effects.
///
/// `#[non_exhaustive]` so built-in effects can be added in minor releases: match it with a
/// wildcard arm. Effects outside the built-in set are written as a [`CellEffect`] and wrapped in
/// [`VisualEffect::Custom`].
#[derive(Clone, Debug)]
#[non_exhaustive]
pub enum VisualEffect {
    /// Convert colors toward monochrome by `strength`.
    Monochrome {
        /// Monochrome blend amount in `[0.0, 1.0]`.
        strength: f32,
    },
    /// Quantize output colors to a preset palette.
    PaletteQuantize {
        /// Target quantization palette.
        palette: EffectPalette,
    },
    /// Overlay scanlines with configurable strength and spacing.
    Scanlines {
        /// Scanline opacity/strength.
        strength: f32,
        /// Row/column distance between scanlines.
        spacing: u16,
    },
    /// Animated rainbow wave over the effect scope.
    RainbowWave {
        /// Blend amount toward the generated wave color in `[0.0, 1.0]`.
        blend: f32,
        /// Wave frequency.
        frequency: f32,
        /// Wave speed.
        speed: f32,
        /// Wave direction.
        axis: EffectAxis,
    },
    /// Gradient wash using [`ColorGradient`] stops, sampled in scope-local space.
    ///
    /// Spatial position is normalized to the effect [`Rect`] so nested scopes remap
    /// independently. The ramp sine-eases at the mirrored ends (`min -> max -> min`) to
    /// avoid hard wrap seams and sharp endpoint troughs. Time evolution uses the render
    /// `phase` (same as [`VisualEffect::RainbowWave`]).
    Gradient {
        /// Color stops (`min` → optional `center` → `max`).
        gradient: ColorGradient,
        /// Blend toward the sampled gradient color in `[0.0, 1.0]`.
        blend: f32,
        /// Number of mirrored gradient cycles across the scope on the chosen axis.
        frequency: f32,
        /// Temporal contribution per frame `phase` (`0.0` = static pattern).
        speed: f32,
        /// Sampling direction in normalized scope space.
        axis: EffectAxis,
    },
    /// CRT simulation preset with optional temporal flicker.
    RetroCrt {
        /// Preset profile.
        preset: RetroPreset,
        /// Flicker strength.
        flicker: f32,
        /// Scanline intensity.
        scanline_strength: f32,
    },
    /// Radial shockwave ring emanating from a point.
    ///
    /// Cells within `ring_width` cells of `radius` distance from the center are tinted.
    /// Coordinates are in character cells relative to the effect scope's top-left corner.
    /// [`RippleRadius::Fixed`] is static; [`RippleRadius::Animated`] derives radius and fade from
    /// the renderer animation phase.
    Ripple {
        /// Ripple origin, resolved in effect scope-local coordinates.
        origin: EffectOrigin,
        /// Ring radius behavior. Internally treats one row as roughly two columns to compensate
        /// for terminal cell aspect ratio.
        radius: RippleRadius,
        /// Half-width of the wave band in character columns.
        ring_width: f32,
        /// Color to tint cells within the ring.
        tint: Color,
        /// Peak tint strength at the ring's center, in `[0.0, 1.0]`.
        strength: f32,
    },
    /// Restrict an inner effect to a sub-rectangle and/or per-cell mask (scope-local coordinates).
    Clipped {
        /// Clip rect in scope-local cells; `None` uses the full effect scope.
        bounds: Option<Rect>,
        /// Per-cell inclusion; `None` applies a solid rectangle (`bounds` or full scope).
        mask: Option<Arc<CellMask>>,
        /// Effect evaluated only where the clip allows.
        inner: Box<VisualEffect>,
    },
    /// Restrict an inner effect to selected cell color channels.
    Channels {
        /// Color channels the inner effect may modify.
        channels: EffectChannels,
        /// Effect evaluated with the channel restriction.
        inner: Box<VisualEffect>,
    },
    /// Apply a relative `ColorTransform` to fg and/or bg of each cell in the scope.
    ///
    /// Independent channels: `fg = None` leaves text colors untouched, same for `bg`.
    /// Reuses the full `ColorTransform` vocabulary (Dim, Lighten, Opacity,
    /// OpacityToward, Tint), so this single variant covers what `Style.fg_transform`
    /// / `Style.bg_transform` / `dim_amount` / `tint` / `lighten` previously did.
    ColorTransform {
        /// Transform to apply to the foreground color.
        fg: Option<ColorTransform>,
        /// Transform to apply to the background color.
        bg: Option<ColorTransform>,
    },
    /// Apply a `ContrastPolicy` to ensure legibility.
    ContrastPolicy(ContrastPolicy),
    /// Apply a user-defined per-cell effect. See [`CellEffect`].
    Custom(Arc<dyn CellEffect>),
}

impl VisualEffect {
    /// Dim both fg and bg by `amount` in `[0.0, 1.0]`.
    ///
    /// `amount` may be late-bound; see [`EffectAmount`].
    pub fn dim(amount: impl Into<EffectAmount>) -> Self {
        let amount = amount.into();
        Self::ColorTransform {
            fg: Some(ColorTransform::Dim(amount)),
            bg: Some(ColorTransform::Dim(amount)),
        }
    }

    /// Lighten both fg and bg by `amount` in `[0.0, 1.0]`.
    ///
    /// `amount` may be late-bound; see [`EffectAmount`].
    pub fn lighten(amount: impl Into<EffectAmount>) -> Self {
        let amount = amount.into();
        Self::ColorTransform {
            fg: Some(ColorTransform::Lighten(amount)),
            bg: Some(ColorTransform::Lighten(amount)),
        }
    }

    /// Blend both fg and bg toward `color` by `alpha` in `[0.0, 1.0]`.
    ///
    /// `alpha` may be late-bound; see [`EffectAmount`].
    pub fn tint(color: Color, alpha: impl Into<EffectAmount>) -> Self {
        let alpha = alpha.into();
        Self::ColorTransform {
            fg: Some(ColorTransform::Tint(color, alpha)),
            bg: Some(ColorTransform::Tint(color, alpha)),
        }
    }

    /// Apply `t` to the foreground only; background is unchanged.
    ///
    /// Matches [`EffectScope::transform_fg`](crate::widgets::EffectScope::transform_fg)
    /// and `Style::transform_fg` vocabulary.
    pub fn transform_fg(t: ColorTransform) -> Self {
        Self::ColorTransform {
            fg: Some(t),
            bg: None,
        }
    }

    /// Apply `t` to the background only; foreground is unchanged.
    ///
    /// Matches [`EffectScope::transform_bg`](crate::widgets::EffectScope::transform_bg)
    /// and `Style::transform_bg` vocabulary.
    pub fn transform_bg(t: ColorTransform) -> Self {
        Self::ColorTransform {
            fg: None,
            bg: Some(t),
        }
    }

    /// Restrict this effect to selected cell color channels.
    pub fn channels(self, channels: EffectChannels) -> Self {
        if channels == EffectChannels::Both {
            self
        } else {
            Self::Channels {
                channels,
                inner: Box::new(self),
            }
        }
    }

    /// Restrict this effect to foreground colors, leaving cell backgrounds unchanged.
    pub fn foreground_only(self) -> Self {
        self.channels(EffectChannels::Foreground)
    }

    /// Restrict this effect to background colors, leaving cell foregrounds unchanged.
    pub fn background_only(self) -> Self {
        self.channels(EffectChannels::Background)
    }

    /// Create a [`VisualEffect::Ripple`] from an explicit scope-local cell origin.
    pub fn ripple(
        origin: EffectOrigin,
        radius: f32,
        ring_width: f32,
        tint: Color,
        strength: f32,
    ) -> Self {
        Self::Ripple {
            origin,
            radius: RippleRadius::Fixed(radius),
            ring_width,
            tint,
            strength,
        }
    }

    /// Create a centered [`VisualEffect::Ripple`] that stays centered in its effect scope.
    pub fn centered_ripple(radius: f32, ring_width: f32, tint: Color, strength: f32) -> Self {
        Self::ripple(EffectOrigin::CENTER, radius, ring_width, tint, strength)
    }

    /// Create a centered looping [`VisualEffect::Ripple`] that restarts every `period_ticks`.
    pub fn centered_looping_ripple(
        max_radius: f32,
        period_ticks: u32,
        ring_width: f32,
        tint: Color,
        strength: f32,
    ) -> Self {
        Self::Ripple {
            origin: EffectOrigin::CENTER,
            radius: RippleRadius::looping(max_radius, period_ticks),
            ring_width,
            tint,
            strength,
        }
    }

    /// Create a centered one-shot [`VisualEffect::Ripple`] starting at `start_tick`.
    pub fn centered_burst_ripple(
        max_radius: f32,
        duration_ticks: u32,
        start_tick: u64,
        ring_width: f32,
        tint: Color,
        strength: f32,
    ) -> Self {
        Self::Ripple {
            origin: EffectOrigin::CENTER,
            radius: RippleRadius::once(max_radius, duration_ticks, start_tick),
            ring_width,
            tint,
            strength,
        }
    }

    /// Returns whether this effect requires frame-to-frame animation.
    ///
    /// A color transform never does by itself: a late-bound strength from
    /// [`Context::animated_amount`](crate::Context::animated_amount) or
    /// [`Context::pulsing_amount`](crate::Context::pulsing_amount) is advanced by the animation
    /// registry, which asks for its own repaints wherever the transform is used.
    pub fn is_animated(&self) -> bool {
        match self {
            Self::RainbowWave { .. } => true,
            Self::Gradient { speed, .. } => speed.abs() > f32::EPSILON,
            Self::RetroCrt { flicker, .. } => *flicker > 0.0,
            Self::Ripple { radius, .. } => radius.is_animated(),
            Self::Clipped { inner, .. } => inner.is_animated(),
            Self::Channels { inner, .. } => inner.is_animated(),
            Self::Custom(effect) => effect.is_animated(),
            _ => false,
        }
    }

    /// Returns whether this effect reads the cells beneath its scope. See
    /// [`CellEffect::uses_backdrop`]; only custom effects can.
    pub fn uses_backdrop(&self) -> bool {
        match self {
            Self::Clipped { inner, .. } | Self::Channels { inner, .. } => inner.uses_backdrop(),
            Self::Custom(effect) => effect.uses_backdrop(),
            _ => false,
        }
    }

    /// Preferred interval between animation ticks for this visual effect.
    ///
    /// Returns `None` for static effects. Animated built-in effects default to 16ms;
    /// custom effects may override [`CellEffect::animation_interval`].
    pub fn animation_interval(&self) -> Option<Duration> {
        if !self.is_animated() {
            return None;
        }

        let interval = match self {
            Self::Clipped { inner, .. } | Self::Channels { inner, .. } => inner
                .animation_interval()
                .unwrap_or_else(|| Duration::from_millis(16)),
            Self::Custom(effect) => effect.animation_interval(),
            _ => Duration::from_millis(16),
        };
        Some(interval.max(Duration::from_millis(1)))
    }

    /// This effect with every late-bound color-transform strength bound to the value the renderer
    /// sees now, or `None` when it has none to bind.
    ///
    /// The renderer calls this once per pass so cells do not each resolve the amount.
    pub(crate) fn with_resolved_amounts(&self) -> Option<Self> {
        match self {
            Self::ColorTransform { fg, bg } => {
                let late = |transform: &Option<ColorTransform>| {
                    transform.is_some_and(|transform| transform.amount().is_late_bound())
                };
                (late(fg) || late(bg)).then(|| Self::ColorTransform {
                    fg: fg.map(ColorTransform::resolved),
                    bg: bg.map(ColorTransform::resolved),
                })
            }
            Self::Clipped {
                bounds,
                mask,
                inner,
            } => inner.with_resolved_amounts().map(|inner| Self::Clipped {
                bounds: *bounds,
                mask: mask.clone(),
                inner: Box::new(inner),
            }),
            Self::Channels { channels, inner } => {
                inner.with_resolved_amounts().map(|inner| Self::Channels {
                    channels: *channels,
                    inner: Box::new(inner),
                })
            }
            _ => None,
        }
    }
}

impl PartialEq for VisualEffect {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Monochrome { strength: a }, Self::Monochrome { strength: b }) => {
                a.to_bits() == b.to_bits()
            }
            (Self::PaletteQuantize { palette: a }, Self::PaletteQuantize { palette: b }) => a == b,
            (
                Self::Scanlines {
                    strength: strength_a,
                    spacing: spacing_a,
                },
                Self::Scanlines {
                    strength: strength_b,
                    spacing: spacing_b,
                },
            ) => strength_a.to_bits() == strength_b.to_bits() && spacing_a == spacing_b,
            (
                Self::RainbowWave {
                    blend: blend_a,
                    frequency: frequency_a,
                    speed: speed_a,
                    axis: axis_a,
                },
                Self::RainbowWave {
                    blend: blend_b,
                    frequency: frequency_b,
                    speed: speed_b,
                    axis: axis_b,
                },
            ) => {
                blend_a.to_bits() == blend_b.to_bits()
                    && frequency_a.to_bits() == frequency_b.to_bits()
                    && speed_a.to_bits() == speed_b.to_bits()
                    && axis_a == axis_b
            }
            (
                Self::Gradient {
                    gradient: g_a,
                    blend: blend_a,
                    frequency: frequency_a,
                    speed: speed_a,
                    axis: axis_a,
                },
                Self::Gradient {
                    gradient: g_b,
                    blend: blend_b,
                    frequency: frequency_b,
                    speed: speed_b,
                    axis: axis_b,
                },
            ) => {
                g_a == g_b
                    && blend_a.to_bits() == blend_b.to_bits()
                    && frequency_a.to_bits() == frequency_b.to_bits()
                    && speed_a.to_bits() == speed_b.to_bits()
                    && axis_a == axis_b
            }
            (
                Self::RetroCrt {
                    preset: preset_a,
                    flicker: flicker_a,
                    scanline_strength: scanline_strength_a,
                },
                Self::RetroCrt {
                    preset: preset_b,
                    flicker: flicker_b,
                    scanline_strength: scanline_strength_b,
                },
            ) => {
                preset_a == preset_b
                    && flicker_a.to_bits() == flicker_b.to_bits()
                    && scanline_strength_a.to_bits() == scanline_strength_b.to_bits()
            }
            (
                Self::Ripple {
                    origin: origin_a,
                    radius: radius_a,
                    ring_width: rw_a,
                    tint: tint_a,
                    strength: strength_a,
                },
                Self::Ripple {
                    origin: origin_b,
                    radius: radius_b,
                    ring_width: rw_b,
                    tint: tint_b,
                    strength: strength_b,
                },
            ) => {
                origin_a == origin_b
                    && radius_a == radius_b
                    && rw_a.to_bits() == rw_b.to_bits()
                    && tint_a == tint_b
                    && strength_a.to_bits() == strength_b.to_bits()
            }
            (
                Self::Clipped {
                    bounds: b_a,
                    mask: m_a,
                    inner: i_a,
                },
                Self::Clipped {
                    bounds: b_b,
                    mask: m_b,
                    inner: i_b,
                },
            ) => b_a == b_b && m_a == m_b && i_a == i_b,
            (
                Self::Channels {
                    channels: c_a,
                    inner: i_a,
                },
                Self::Channels {
                    channels: c_b,
                    inner: i_b,
                },
            ) => c_a == c_b && i_a == i_b,
            (
                Self::ColorTransform { fg: fg_a, bg: bg_a },
                Self::ColorTransform { fg: fg_b, bg: bg_b },
            ) => fg_a == fg_b && bg_a == bg_b,
            (Self::ContrastPolicy(a), Self::ContrastPolicy(b)) => a == b,
            (Self::Custom(a), Self::Custom(b)) => {
                Arc::ptr_eq(a, b) && a.cache_key() == b.cache_key()
            }
            _ => false,
        }
    }
}

impl Eq for VisualEffect {}

impl Hash for VisualEffect {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Self::Monochrome { strength } => {
                2u8.hash(state);
                strength.to_bits().hash(state);
            }
            Self::PaletteQuantize { palette } => {
                3u8.hash(state);
                palette.hash(state);
            }
            Self::Scanlines { strength, spacing } => {
                4u8.hash(state);
                strength.to_bits().hash(state);
                spacing.hash(state);
            }
            Self::RainbowWave {
                blend,
                frequency,
                speed,
                axis,
            } => {
                5u8.hash(state);
                blend.to_bits().hash(state);
                frequency.to_bits().hash(state);
                speed.to_bits().hash(state);
                axis.hash(state);
            }
            Self::Gradient {
                gradient,
                blend,
                frequency,
                speed,
                axis,
            } => {
                9u8.hash(state);
                gradient.hash(state);
                blend.to_bits().hash(state);
                frequency.to_bits().hash(state);
                speed.to_bits().hash(state);
                axis.hash(state);
            }
            Self::RetroCrt {
                preset,
                flicker,
                scanline_strength,
            } => {
                6u8.hash(state);
                preset.hash(state);
                flicker.to_bits().hash(state);
                scanline_strength.to_bits().hash(state);
            }
            Self::Ripple {
                origin,
                radius,
                ring_width,
                tint,
                strength,
            } => {
                7u8.hash(state);
                origin.hash(state);
                radius.hash(state);
                ring_width.to_bits().hash(state);
                tint.hash(state);
                strength.to_bits().hash(state);
            }
            Self::Clipped {
                bounds,
                mask,
                inner,
            } => {
                10u8.hash(state);
                bounds.hash(state);
                mask.hash(state);
                inner.hash(state);
            }
            Self::Channels { channels, inner } => {
                14u8.hash(state);
                channels.hash(state);
                inner.hash(state);
            }
            Self::ColorTransform { fg, bg } => {
                11u8.hash(state);
                fg.hash(state);
                bg.hash(state);
            }
            Self::ContrastPolicy(policy) => {
                12u8.hash(state);
                policy.hash(state);
            }
            Self::Custom(effect) => {
                13u8.hash(state);
                (Arc::as_ptr(effect) as *const () as usize).hash(state);
                effect.cache_key().hash(state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;

    use super::*;

    #[test]
    fn late_bound_transform_strengths_never_schedule_the_effect_clock() {
        for amount in [
            EffectAmount::fixed(0.2),
            EffectAmount::animated(crate::animation::AnimationHandle::new(0, 0), 0.2),
            EffectAmount::pulsing(crate::animation::AnimationHandle::new(1, 0), 0.1),
        ] {
            let effect = VisualEffect::tint(Color::Red, amount);
            assert!(
                !effect.is_animated(),
                "the animation registry drives {amount:?}, wherever it is used"
            );
            assert_eq!(effect.animation_interval(), None);
        }
    }

    #[test]
    fn a_late_bound_effect_compares_equal_for_its_whole_animation() {
        let pulse = VisualEffect::dim(EffectAmount::pulsing(
            crate::animation::AnimationHandle::new(3, 0),
            0.0,
        ));
        assert_eq!(
            pulse,
            VisualEffect::dim(EffectAmount::pulsing(
                crate::animation::AnimationHandle::new(3, 0),
                0.0
            ))
        );
        let mut a = DefaultHasher::new();
        let mut b = DefaultHasher::new();
        pulse.hash(&mut a);
        VisualEffect::dim(EffectAmount::pulsing(
            crate::animation::AnimationHandle::new(3, 0),
            0.0,
        ))
        .hash(&mut b);
        assert_eq!(a.finish(), b.finish(), "so layout hashes hold still too");
    }

    #[test]
    fn resolving_amounts_binds_only_late_bound_transforms() {
        assert!(VisualEffect::dim(0.4).with_resolved_amounts().is_none());
        let resolved = VisualEffect::dim(EffectAmount::pulsing(
            crate::animation::AnimationHandle::new(u16::MAX, 0),
            0.25,
        ))
        .background_only()
        .with_resolved_amounts()
        .expect("a pulse resolves");
        assert_eq!(
            resolved,
            VisualEffect::dim(0.25).background_only(),
            "outside a draw a pulse binds at the start of its cycle"
        );
    }

    #[derive(Debug)]
    struct NoopEffect;

    impl CellEffect for NoopEffect {
        fn apply(&self, _cell: &mut EffectCell, _ctx: &EffectContext) {}
    }

    #[derive(Debug)]
    struct KeyedEffect(u64);

    impl CellEffect for KeyedEffect {
        fn apply(&self, _cell: &mut EffectCell, _ctx: &EffectContext) {}

        fn cache_key(&self) -> u64 {
            self.0
        }
    }

    #[derive(Debug)]
    struct AnimatedEffect;

    impl CellEffect for AnimatedEffect {
        fn apply(&self, _cell: &mut EffectCell, _ctx: &EffectContext) {}

        fn is_animated(&self) -> bool {
            true
        }
    }

    fn hash_effect(effect: &VisualEffect) -> u64 {
        let mut hasher = DefaultHasher::new();
        effect.hash(&mut hasher);
        hasher.finish()
    }

    fn hash_radius(radius: &RippleRadius) -> u64 {
        let mut hasher = DefaultHasher::new();
        radius.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn custom_effect_identity_controls_eq_and_hash() {
        let shared: Arc<dyn CellEffect> = Arc::new(NoopEffect);
        let a = VisualEffect::Custom(Arc::clone(&shared));
        let b = VisualEffect::Custom(Arc::clone(&shared));
        let c = VisualEffect::Custom(Arc::new(NoopEffect));

        assert_eq!(a, b);
        assert_eq!(hash_effect(&a), hash_effect(&b));
        assert_ne!(a, c);
    }

    #[test]
    fn custom_effect_same_cache_key_different_arc_is_not_equal() {
        let a = VisualEffect::Custom(Arc::new(KeyedEffect(7)));
        let b = VisualEffect::Custom(Arc::new(KeyedEffect(7)));

        assert_ne!(a, b);
        assert_ne!(hash_effect(&a), hash_effect(&b));
    }

    #[test]
    fn custom_effect_animation_flag_is_delegated() {
        assert!(VisualEffect::Custom(Arc::new(AnimatedEffect)).is_animated());
        assert!(!VisualEffect::Custom(Arc::new(NoopEffect)).is_animated());
    }

    #[test]
    fn channel_restriction_controls_eq_hash_and_animation() {
        let foreground = VisualEffect::Custom(Arc::new(AnimatedEffect)).foreground_only();
        let same = foreground.clone();
        let background = match &foreground {
            VisualEffect::Channels { inner, .. } => VisualEffect::Channels {
                channels: EffectChannels::Background,
                inner: inner.clone(),
            },
            _ => unreachable!("foreground_only should wrap the effect"),
        };

        assert_eq!(foreground, same);
        assert_eq!(hash_effect(&foreground), hash_effect(&same));
        assert_ne!(foreground, background);
        assert_ne!(hash_effect(&foreground), hash_effect(&background));
        assert!(foreground.is_animated());
    }

    #[test]
    fn ripple_radius_controls_animation_flag() {
        assert!(!VisualEffect::centered_ripple(4.0, 1.5, Color::Cyan, 0.6).is_animated());
        assert!(
            VisualEffect::centered_looping_ripple(12.0, 30, 1.5, Color::Cyan, 0.6).is_animated()
        );
        assert!(
            VisualEffect::centered_burst_ripple(12.0, 30, 7, 1.5, Color::Cyan, 0.6).is_animated()
        );
    }

    #[test]
    fn ripple_radius_eq_and_hash_use_payloads() {
        let cases = [
            (
                RippleRadius::Fixed(4.0),
                RippleRadius::Fixed(4.0),
                RippleRadius::Fixed(5.0),
            ),
            (
                RippleRadius::looping(12.0, 30),
                RippleRadius::looping(12.0, 30),
                RippleRadius::looping(12.0, 31),
            ),
            (
                RippleRadius::once(12.0, 30, 7),
                RippleRadius::once(12.0, 30, 7),
                RippleRadius::once(12.0, 30, 8),
            ),
            (
                RippleRadius::once(12.0, 30, 7),
                RippleRadius::once(12.0, 30, 7),
                RippleRadius::collapse(12.0, 30, 7),
            ),
            (
                RippleRadius::from(RippleAnimation::new(0.0, 8.0, 20)),
                RippleRadius::from(RippleAnimation::new(0.0, 8.0, 20)),
                RippleRadius::from(
                    RippleAnimation::new(0.0, 8.0, 20).with_easing(Easing::EaseInOutSine),
                ),
            ),
            (
                RippleRadius::from(RippleAnimation::new(0.0, 8.0, 20)),
                RippleRadius::from(RippleAnimation::new(0.0, 8.0, 20)),
                RippleRadius::from(
                    RippleAnimation::new(0.0, 8.0, 20).with_repeat(RippleRepeat::PingPong),
                ),
            ),
        ];

        for (a, b, c) in cases {
            assert_eq!(a, b);
            assert_eq!(hash_radius(&a), hash_radius(&b));
            assert_ne!(a, c);
            assert_ne!(hash_radius(&a), hash_radius(&c));
        }
    }

    fn assert_close(actual: Option<(f32, f32)>, expected: (f32, f32)) {
        let (radius, strength) = actual.expect("the animation is playing");
        assert!(
            (radius - expected.0).abs() < 1e-4 && (strength - expected.1).abs() < 1e-4,
            "{actual:?} != {expected:?}"
        );
    }

    #[test]
    fn once_grows_easing_out_and_fades() {
        let once = RippleRadius::once(10.0, 10, 5);
        assert_eq!(once.resolve(4), None, "before its start tick");
        assert_close(once.resolve(5), (0.0, 1.0));
        // t = 0.5: 1 - (1 - t)^2 = 0.75 of the radius, strength 1 - t.
        assert_close(once.resolve(10), (7.5, 0.5));
        assert_eq!(once.resolve(15), None, "after its last tick");
    }

    #[test]
    fn collapse_gathers_easing_in_and_brightens() {
        let collapse = RippleRadius::collapse(10.0, 10, 0);
        assert_close(collapse.resolve(0), (10.0, 0.3));
        // t = 0.5: (1 - t^2) of the radius, strength 0.3 + 0.7 t.
        assert_close(collapse.resolve(5), (7.5, 0.65));
        assert_eq!(collapse.resolve(10), None);
    }

    #[test]
    fn looping_restarts_every_period() {
        let looping = RippleRadius::looping(10.0, 10);
        assert_close(looping.resolve(5), (7.5, 0.5));
        assert_close(looping.resolve(25), (7.5, 0.5));
        assert_close(looping.resolve(30), (0.0, 1.0));
    }

    #[test]
    fn ping_pong_plays_back_and_forth() {
        let ping_pong = RippleAnimation::new(0.0, 10.0, 10).with_repeat(RippleRepeat::PingPong);
        assert_close(ping_pong.resolve(5), (5.0, 1.0));
        assert_close(ping_pong.resolve(10), (10.0, 1.0));
        assert_close(ping_pong.resolve(15), (5.0, 1.0));
        assert_close(ping_pong.resolve(20), (0.0, 1.0));
        assert_close(ping_pong.resolve(25), (5.0, 1.0));
    }

    #[test]
    fn zero_duration_is_one_tick() {
        let flash = RippleAnimation::new(0.0, 10.0, 0).starting_at(3);
        assert_close(flash.resolve(3), (0.0, 1.0));
        assert_eq!(flash.resolve(4), None);
    }
}
