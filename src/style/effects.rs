use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::time::Duration;

use crate::app::ContrastPolicy;
use crate::core::mask::CellMask;
use crate::style::{Align, Color, ColorTransform, Rect};
use crate::utils::gradient::ColorGradient;

/// Terminal cell type passed to [`CellEffect::apply`].
pub use ratatui::buffer::Cell as EffectCell;
/// Terminal color type used by custom visual effects.
pub use ratatui::style::Color as TerminalColor;

/// Per-cell inputs supplied to custom visual effects.
#[derive(Clone, Copy, Debug)]
pub struct EffectContext {
    /// Absolute frame column for the cell being processed.
    pub x: i16,
    /// Absolute frame row for the cell being processed.
    pub y: i16,
    /// Absolute effect-scope rect in terminal cells.
    pub bounds: Rect,
    /// Renderer animation phase counter.
    pub phase: u64,
    /// Resolved terminal background color, when known.
    pub terminal_bg: Option<TerminalColor>,
}

/// Per-frame inputs supplied before a custom visual effect processes a scope.
#[derive(Clone, Copy, Debug)]
pub struct EffectPrepareContext {
    /// Absolute effect-scope rect in terminal cells.
    pub bounds: Rect,
    /// Renderer animation phase counter.
    pub phase: u64,
    /// Resolved terminal background color, when known.
    pub terminal_bg: Option<TerminalColor>,
}

/// Prepared per-cell visual effect for one render phase and effect-scope bounds.
///
/// Implement this when a [`CellEffect`] has expensive values that can be computed once
/// per frame instead of once per cell.
pub trait PreparedCellEffect: Send + Sync + std::fmt::Debug + 'static {
    /// Apply this prepared effect to one cell inside the prepared scope.
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext);
}

/// User-defined per-cell visual post-processing effect.
///
/// Effects must be `'static` because `VisualEffect::Custom` stores them in an
/// [`Arc`]. Store owned data in the effect rather than borrowing component state.
pub trait CellEffect: Send + Sync + std::fmt::Debug + 'static {
    /// Apply this effect to one cell. Called once per cell inside the effect scope, per frame.
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext);

    /// Prepare this effect once for the current scope bounds and render phase.
    ///
    /// Return `Some` when frame-constant work can be reused for every cell. The renderer
    /// will call the prepared effect's `apply` method for cells in this scope. The default
    /// returns `None`, preserving the direct per-cell [`Self::apply`] path.
    fn prepare(&self, _ctx: &EffectPrepareContext) -> Option<Box<dyn PreparedCellEffect>> {
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

/// Quantization palette presets for post-processing.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
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
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
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
#[derive(Clone, Copy, Debug)]
pub enum RippleRadius {
    /// Static ring radius in character columns.
    Fixed(f32),
    /// Continuously restart the shockwave from zero to `max_radius` every `period_ticks`.
    Loop {
        /// Maximum ring radius in character columns.
        max_radius: f32,
        /// Number of renderer animation ticks per cycle.
        period_ticks: u32,
    },
    /// Play one shockwave from `start_tick` for `duration_ticks` renderer animation ticks.
    Once {
        /// Maximum ring radius in character columns.
        max_radius: f32,
        /// Number of renderer animation ticks for this burst.
        duration_ticks: u32,
        /// Renderer animation phase captured when the burst starts.
        start_tick: u64,
    },
}

impl PartialEq for RippleRadius {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Fixed(a), Self::Fixed(b)) => a.to_bits() == b.to_bits(),
            (
                Self::Loop {
                    max_radius: max_radius_a,
                    period_ticks: period_ticks_a,
                },
                Self::Loop {
                    max_radius: max_radius_b,
                    period_ticks: period_ticks_b,
                },
            ) => {
                max_radius_a.to_bits() == max_radius_b.to_bits() && period_ticks_a == period_ticks_b
            }
            (
                Self::Once {
                    max_radius: max_radius_a,
                    duration_ticks: duration_ticks_a,
                    start_tick: start_tick_a,
                },
                Self::Once {
                    max_radius: max_radius_b,
                    duration_ticks: duration_ticks_b,
                    start_tick: start_tick_b,
                },
            ) => {
                max_radius_a.to_bits() == max_radius_b.to_bits()
                    && duration_ticks_a == duration_ticks_b
                    && start_tick_a == start_tick_b
            }
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
            Self::Loop {
                max_radius,
                period_ticks,
            } => {
                1u8.hash(state);
                max_radius.to_bits().hash(state);
                period_ticks.hash(state);
            }
            Self::Once {
                max_radius,
                duration_ticks,
                start_tick,
            } => {
                2u8.hash(state);
                max_radius.to_bits().hash(state);
                duration_ticks.hash(state);
                start_tick.hash(state);
            }
        }
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
#[derive(Clone, Debug)]
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
    /// [`RippleRadius::Fixed`] is static; [`RippleRadius::Loop`] and [`RippleRadius::Once`]
    /// derive radius and fade from the renderer animation phase.
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
    pub fn dim(amount: f32) -> Self {
        Self::ColorTransform {
            fg: Some(ColorTransform::Dim(amount)),
            bg: Some(ColorTransform::Dim(amount)),
        }
    }

    /// Lighten both fg and bg by `amount` in `[0.0, 1.0]`.
    pub fn lighten(amount: f32) -> Self {
        Self::ColorTransform {
            fg: Some(ColorTransform::Lighten(amount)),
            bg: Some(ColorTransform::Lighten(amount)),
        }
    }

    /// Blend both fg and bg toward `color` by `alpha` in `[0.0, 1.0]`.
    pub fn tint(color: Color, alpha: f32) -> Self {
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
            radius: RippleRadius::Loop {
                max_radius,
                period_ticks,
            },
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
            radius: RippleRadius::Once {
                max_radius,
                duration_ticks,
                start_tick,
            },
            ring_width,
            tint,
            strength,
        }
    }

    /// Returns whether this effect requires frame-to-frame animation.
    pub fn is_animated(&self) -> bool {
        match self {
            Self::RainbowWave { .. } => true,
            Self::Gradient { speed, .. } => speed.abs() > f32::EPSILON,
            Self::RetroCrt { flicker, .. } => *flicker > 0.0,
            Self::Ripple { radius, .. } => !matches!(radius, RippleRadius::Fixed(_)),
            Self::Clipped { inner, .. } => inner.is_animated(),
            Self::Custom(effect) => effect.is_animated(),
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
            Self::Clipped { inner, .. } => inner
                .animation_interval()
                .unwrap_or_else(|| Duration::from_millis(16)),
            Self::Custom(effect) => effect.animation_interval(),
            _ => Duration::from_millis(16),
        };
        Some(interval.max(Duration::from_millis(1)))
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
    fn ripple_radius_eq_and_hash_use_variant_payloads() {
        let cases = [
            (
                RippleRadius::Fixed(4.0),
                RippleRadius::Fixed(4.0),
                RippleRadius::Fixed(5.0),
            ),
            (
                RippleRadius::Loop {
                    max_radius: 12.0,
                    period_ticks: 30,
                },
                RippleRadius::Loop {
                    max_radius: 12.0,
                    period_ticks: 30,
                },
                RippleRadius::Loop {
                    max_radius: 12.0,
                    period_ticks: 31,
                },
            ),
            (
                RippleRadius::Once {
                    max_radius: 12.0,
                    duration_ticks: 30,
                    start_tick: 7,
                },
                RippleRadius::Once {
                    max_radius: 12.0,
                    duration_ticks: 30,
                    start_tick: 7,
                },
                RippleRadius::Once {
                    max_radius: 12.0,
                    duration_ticks: 30,
                    start_tick: 8,
                },
            ),
        ];

        for (a, b, c) in cases {
            assert_eq!(a, b);
            assert_eq!(hash_radius(&a), hash_radius(&b));
            assert_ne!(a, c);
            assert_ne!(hash_radius(&a), hash_radius(&c));
        }
    }
}
