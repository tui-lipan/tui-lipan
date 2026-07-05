use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect as RRect;
use ratatui::style::{Color as RColor, Modifier as RMod};

use crate::app::ContrastPolicy;
use crate::style::{
    Color, ColorTransform, EffectAxis, EffectContext, EffectPalette, EffectPrepareContext, Rect,
    RetroPreset, RippleRadius, Style, VisualEffect,
};
use crate::utils::color_contrast::{
    readable_text_color, readable_text_color_apca, readable_text_color_black_or_white,
};

use super::colors::{from_ratatui_color, to_ratatui_color};
use super::convert::to_ratatui_rect;

pub(crate) fn dim_ratatui_color(color: RColor, amount: f32) -> RColor {
    if color == RColor::Reset {
        return RColor::Reset;
    }
    let our = from_ratatui_color(color);

    match our.dim_by(amount) {
        Color::Rgb(r, g, b) => RColor::Rgb(r, g, b),
        _ => color,
    }
}

/// Apply `transform` to a ratatui color while preserving terminal-palette fidelity.
///
/// Named/indexed ("palette") colors carry no fixed RGB: the terminal resolves them against
/// the user's configured 16/256-color palette. Blending such a color (opacity, tint, dim)
/// forces it to its *standard* ANSI RGB and emits truecolor, which bypasses any remapped
/// palette and visibly shifts the hue — e.g. a palette whose bright-cyan is remapped to a
/// pink would suddenly render literal cyan as a pane fades behind a modal backdrop. When a
/// transform would push a palette color into truecolor, keep the palette color and report
/// whether it darkened, so the caller can express the de-emphasis with the terminal's own
/// `DIM` attribute instead. Truecolor inputs blend exactly as before.
///
/// Returns the resolved color and whether the cell should gain the `DIM` modifier.
fn transform_ratatui_color(
    color: RColor,
    transform: ColorTransform,
    backdrop: Option<RColor>,
    preserve_reset: bool,
) -> (RColor, bool) {
    if preserve_reset && color == RColor::Reset {
        return (color, false);
    }

    let source = from_ratatui_color(color);
    let result = transform.apply_with_backdrop(source, backdrop.map(from_ratatui_color));
    if let Some(darkened) = preserve_palette_blend(source, result) {
        return (color, darkened);
    }

    (to_ratatui_color(result), false)
}

/// Decide whether a blend/transform that produced `result` from `source` should keep the
/// original palette color instead of emitting `result` as truecolor.
///
/// Named/indexed ("palette") colors have no fixed RGB — the terminal resolves them against
/// the user's configured palette. Any blend (opacity, tint, dim, compositing) forces them to
/// their *standard* ANSI RGB, which bypasses a remapped palette and shifts the hue (e.g. a
/// remapped bright-cyan rendering as literal cyan). Only the hue is at risk, and only a
/// *chromatic* result carries hue: a grayscale/near-black result (white/gray dimming, full-dim
/// spotlight masks) has nothing to corrupt and should darken normally.
///
/// Returns `Some(darkened)` to keep the palette `source` on-palette — the caller should add the
/// terminal's `DIM` attribute when `darkened` is true — or `None` to emit the truecolor `result`.
pub(crate) fn preserve_palette_blend(source: Color, result: Color) -> Option<bool> {
    if matches!(source, Color::Rgb(..)) || source.to_rgb().is_none() {
        return None;
    }
    let Color::Rgb(r, g, b) = result else {
        return None;
    };
    let chroma = r.max(g).max(b) - r.min(g).min(b);
    (chroma > PALETTE_HUE_CHROMA_THRESHOLD).then(|| result.luminance() < source.luminance())
}

/// Minimum RGB chroma (max channel − min channel, 0–255) for a blended palette color to be
/// treated as still carrying a hue worth preserving. Below this the result is effectively
/// grayscale and safe to emit as truecolor.
const PALETTE_HUE_CHROMA_THRESHOLD: u8 = 24;

fn dedupe_effect_transform(
    transform: Option<ColorTransform>,
    dim_amount: Option<f32>,
    tint: Option<(Color, f32)>,
) -> Option<ColorTransform> {
    let transform = transform?;

    match transform {
        ColorTransform::Dim(amount)
            if dim_amount.is_some_and(|dim| dim.to_bits() == amount.to_bits()) =>
        {
            None
        }
        ColorTransform::Tint(color, alpha)
            if tint.is_some_and(|(tint_color, tint_alpha)| {
                tint_color == color && tint_alpha.to_bits() == alpha.to_bits()
            }) =>
        {
            None
        }
        _ => Some(transform),
    }
}

fn apply_dim_amount_to_cell(cell: &mut Cell, amount: f32, terminal_bg: Option<RColor>) {
    let skip_fg_dim = terminal_bg.is_some_and(|tbg| cell.bg == RColor::Reset && cell.fg == tbg);
    let new_fg = if skip_fg_dim {
        cell.fg
    } else {
        dim_ratatui_color(cell.fg, amount)
    };
    let new_bg = dim_ratatui_color(cell.bg, amount);

    if new_fg != RColor::Reset {
        cell.fg = new_fg;
    }
    if new_bg != RColor::Reset {
        cell.bg = new_bg;
    }

    if amount > 0.0 && new_fg == RColor::Reset && new_bg == RColor::Reset {
        cell.set_style(cell.style().add_modifier(RMod::DIM));
    }
}

fn apply_contrast_policy_to_cell(cell: &mut Cell, policy: ContrastPolicy) {
    if cell.fg == RColor::Reset || cell.bg == RColor::Reset {
        return;
    }

    let fg = from_ratatui_color(cell.fg);
    let bg = from_ratatui_color(cell.bg);
    cell.fg = match policy {
        ContrastPolicy::Off => cell.fg,
        ContrastPolicy::Wcag => to_ratatui_color(readable_text_color(Some(fg), bg)),
        ContrastPolicy::BlackOrWhite => {
            to_ratatui_color(readable_text_color_black_or_white(Some(fg), bg))
        }
        ContrastPolicy::Apca => to_ratatui_color(readable_text_color_apca(Some(fg), bg)),
    };
}

pub(crate) fn apply_color_transforms_to_cell(
    cell: &mut Cell,
    fg_transform: Option<ColorTransform>,
    bg_transform: Option<ColorTransform>,
    terminal_bg: Option<RColor>,
) {
    let mut dim_cell = false;
    if let Some(transform) = bg_transform {
        // Update the background first so later foreground opacity uses
        // the faded backdrop, not the stale pre-fade one.
        let bg_source = if cell.bg == RColor::Reset {
            terminal_bg.unwrap_or(cell.bg)
        } else {
            cell.bg
        };
        let (bg, dim) = transform_ratatui_color(bg_source, transform, terminal_bg, true);
        cell.bg = bg;
        dim_cell |= dim;
    }
    if let Some(transform) = fg_transform {
        let skip_fg = matches!(transform, ColorTransform::Dim(_))
            && terminal_bg.is_some_and(|tbg| cell.bg == RColor::Reset && cell.fg == tbg);
        if !skip_fg {
            let backdrop = if cell.bg == RColor::Reset {
                terminal_bg
            } else {
                Some(cell.bg)
            };
            let (fg, dim) = transform_ratatui_color(cell.fg, transform, backdrop, true);
            cell.fg = fg;
            dim_cell |= dim;
        }
    }
    if dim_cell {
        cell.set_style(cell.style().add_modifier(RMod::DIM));
    }
}

pub(crate) fn apply_effect_style_clipped(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    style: Style,
    clip_rect: Option<Rect>,
    terminal_bg: Option<RColor>,
) {
    let mut draw_rect = rect;
    if let Some(clip) = clip_rect {
        draw_rect = draw_rect.intersection(&clip);
    }
    if draw_rect.is_empty() {
        return;
    }

    let fg_transform = dedupe_effect_transform(style.fg_transform, style.dim_amount, style.tint);
    let bg_transform = dedupe_effect_transform(style.bg_transform, style.dim_amount, style.tint);

    if style.dim_amount.is_none()
        && style.tint.is_none()
        && fg_transform.is_none()
        && bg_transform.is_none()
        && style.contrast_policy.is_none()
    {
        return;
    }

    let r_rect = to_ratatui_rect(draw_rect);
    let intersection = f.area().intersection(r_rect);
    if intersection.width == 0 || intersection.height == 0 {
        return;
    }

    let buf = f.buffer_mut();
    let mut tint_cache = style.tint.map(|_| RatatuiTintCache::new());
    for y in intersection.y..intersection.y + intersection.height {
        for x in intersection.x..intersection.x + intersection.width {
            if let Some(cell) = buf.cell_mut((x, y)) {
                if let Some(amount) = style.dim_amount {
                    apply_dim_amount_to_cell(cell, amount, terminal_bg);
                }
                apply_color_transforms_to_cell(cell, fg_transform, bg_transform, terminal_bg);
                if let (Some((tint, alpha)), Some(cache)) = (style.tint, tint_cache.as_mut()) {
                    // Resolve Reset bg against the terminal bg so a scrim
                    // darkens the underlying terminal uniformly instead of
                    // collapsing to pure black (blend_toward treats Reset as
                    // (0,0,0)). When terminal_bg is unknown, leave the cell
                    // untinted — its rendered color is unknowable.
                    let bg_source = if cell.bg == RColor::Reset {
                        terminal_bg
                    } else {
                        Some(cell.bg)
                    };
                    if let Some(bg) = bg_source {
                        cell.bg = cache.tint(bg, tint, alpha);
                    }
                    if cell.fg != RColor::Reset {
                        cell.fg = cache.tint(cell.fg, tint, alpha);
                    }
                }
                if let Some(policy) = style.contrast_policy {
                    apply_contrast_policy_to_cell(cell, policy);
                }
            }
        }
    }
}

const RATATUI_TINT_CACHE_CAP: usize = 32;

pub(crate) struct RatatuiTintCache {
    entries: [Option<(RColor, RColor)>; RATATUI_TINT_CACHE_CAP],
    len: usize,
}

impl RatatuiTintCache {
    pub(crate) fn new() -> Self {
        Self {
            entries: [None; RATATUI_TINT_CACHE_CAP],
            len: 0,
        }
    }

    pub(crate) fn tint(&mut self, color: RColor, tint: Color, alpha: f32) -> RColor {
        for entry in &self.entries[..self.len] {
            if let Some((source, tinted)) = entry
                && *source == color
            {
                return *tinted;
            }
        }

        let tinted = tint_ratatui_color(color, tint, alpha);
        if self.len < RATATUI_TINT_CACHE_CAP {
            self.entries[self.len] = Some((color, tinted));
            self.len += 1;
        }
        tinted
    }
}

fn blend_rgb_toward(
    (r1, g1, b1): (u8, u8, u8),
    (r2, g2, b2): (u8, u8, u8),
    alpha: f32,
) -> (u8, u8, u8) {
    let alpha = alpha.clamp(0.0, 1.0);
    let blend = |a: u8, b: u8| -> u8 {
        (a as f32 * (1.0 - alpha) + b as f32 * alpha)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (blend(r1, r2), blend(g1, g2), blend(b1, b2))
}

fn map_ratatui_channel_if_not_reset(
    color: RColor,
    map: impl FnOnce((u8, u8, u8)) -> (u8, u8, u8),
) -> RColor {
    if color == RColor::Reset {
        return color;
    }
    let Some(rgb) = from_ratatui_color(color).to_rgb() else {
        return color;
    };
    let (r, g, b) = map(rgb);
    RColor::Rgb(r, g, b)
}

pub(crate) fn monochrome_ratatui_color(color: RColor, strength: f32) -> RColor {
    map_ratatui_channel_if_not_reset(color, |(r, g, b)| {
        let gray = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32)
            .round()
            .clamp(0.0, 255.0) as u8;
        blend_rgb_toward((r, g, b), (gray, gray, gray), strength)
    })
}

fn nearest_palette_color((r, g, b): (u8, u8, u8), palette: &[(u8, u8, u8)]) -> (u8, u8, u8) {
    palette
        .iter()
        .copied()
        .min_by_key(|&(pr, pg, pb)| {
            let dr = r as i32 - pr as i32;
            let dg = g as i32 - pg as i32;
            let db = b as i32 - pb as i32;
            dr * dr + dg * dg + db * db
        })
        .unwrap_or((r, g, b))
}

const CGA_EFFECT_PALETTE: &[(u8, u8, u8)] =
    &[(0, 0, 0), (85, 255, 255), (255, 85, 255), (255, 255, 255)];
const GAMEBOY_EFFECT_PALETTE: &[(u8, u8, u8)] =
    &[(15, 56, 15), (48, 98, 48), (139, 172, 15), (155, 188, 15)];
const AMBER_EFFECT_PALETTE: &[(u8, u8, u8)] =
    &[(32, 16, 0), (96, 52, 8), (180, 110, 24), (255, 191, 80)];
const GREEN_EFFECT_PALETTE: &[(u8, u8, u8)] =
    &[(0, 20, 0), (0, 60, 0), (40, 140, 40), (140, 255, 140)];
const VAULTTEC_EFFECT_PALETTE: &[(u8, u8, u8)] = &[
    (0, 18, 6),
    (8, 34, 16),
    (18, 58, 28),
    (40, 104, 52),
    (92, 184, 110),
    (148, 255, 178),
];

fn visual_effect_palette(palette: &EffectPalette) -> Vec<(u8, u8, u8)> {
    match palette {
        EffectPalette::Cga => CGA_EFFECT_PALETTE.to_vec(),
        EffectPalette::Gameboy => GAMEBOY_EFFECT_PALETTE.to_vec(),
        EffectPalette::Amber => AMBER_EFFECT_PALETTE.to_vec(),
        EffectPalette::Green => GREEN_EFFECT_PALETTE.to_vec(),
        EffectPalette::Custom(colors) => colors.iter().filter_map(|c| c.to_rgb()).collect(),
    }
}

#[derive(Clone, Copy)]
enum RetroRefreshWaveCadence {
    Classic,
    VaultTec,
}

#[derive(Clone, Copy)]
pub(crate) struct RetroCrtParams {
    pub(crate) mono_strength: f32,
    pub(crate) base_dim: f32,
    pub(crate) tint: Option<(Color, f32)>,
    pub(crate) palette: &'static [(u8, u8, u8)],
    flicker_strength: f32,
    vignette_strength: f32,
    refresh_wave_cadence: RetroRefreshWaveCadence,
    pub(crate) screen_bg: Option<RColor>,
}

pub(crate) fn retro_crt_params(preset: RetroPreset) -> RetroCrtParams {
    match preset {
        RetroPreset::Amber => RetroCrtParams {
            mono_strength: 0.12,
            base_dim: 0.04,
            tint: None,
            palette: AMBER_EFFECT_PALETTE,
            flicker_strength: 0.15,
            vignette_strength: 0.0,
            refresh_wave_cadence: RetroRefreshWaveCadence::Classic,
            screen_bg: None,
        },
        RetroPreset::Green => RetroCrtParams {
            mono_strength: 0.12,
            base_dim: 0.04,
            tint: None,
            palette: GREEN_EFFECT_PALETTE,
            flicker_strength: 0.15,
            vignette_strength: 0.0,
            refresh_wave_cadence: RetroRefreshWaveCadence::Classic,
            screen_bg: None,
        },
        RetroPreset::Cga => RetroCrtParams {
            mono_strength: 0.20,
            base_dim: 0.08,
            tint: None,
            palette: CGA_EFFECT_PALETTE,
            flicker_strength: 0.15,
            vignette_strength: 0.0,
            refresh_wave_cadence: RetroRefreshWaveCadence::Classic,
            screen_bg: None,
        },
        RetroPreset::Gameboy => RetroCrtParams {
            mono_strength: 0.18,
            base_dim: 0.08,
            tint: None,
            palette: GAMEBOY_EFFECT_PALETTE,
            flicker_strength: 0.15,
            vignette_strength: 0.0,
            refresh_wave_cadence: RetroRefreshWaveCadence::Classic,
            screen_bg: None,
        },
        RetroPreset::VaultTec => RetroCrtParams {
            mono_strength: 0.16,
            base_dim: 0.30,
            tint: Some((Color::rgb(120, 255, 170), 0.15)),
            palette: VAULTTEC_EFFECT_PALETTE,
            flicker_strength: 0.035,
            vignette_strength: 0.46,
            refresh_wave_cadence: RetroRefreshWaveCadence::VaultTec,
            // Keep the substrate dark enough that explicit panel fills stay
            // visually above the CRT glass after quantize + tint + dim.
            screen_bg: Some(RColor::Rgb(0, 18, 6)),
        },
    }
}

pub(crate) fn palette_quantize_ratatui_color(color: RColor, palette: &[(u8, u8, u8)]) -> RColor {
    map_ratatui_channel_if_not_reset(color, |rgb| nearest_palette_color(rgb, palette))
}

fn rainbow_wave_color(x: i16, y: i16, phase: u64, effect: &VisualEffect) -> (u8, u8, u8, f32) {
    let VisualEffect::RainbowWave {
        blend,
        frequency,
        speed,
        axis,
    } = effect
    else {
        return (255, 255, 255, 0.0);
    };

    let pos = match axis {
        EffectAxis::Horizontal => x as f32,
        EffectAxis::Vertical => y as f32,
        EffectAxis::Diagonal => (x as f32 + y as f32) * 0.707_106_77,
    };
    // These factors keep the spatial and temporal wave speeds legible in a
    // terminal cell grid without making neighboring cells flicker too sharply.
    let theta = pos * *frequency * 0.08 + phase as f32 * *speed * 0.04;
    // Offsets are 0, 2pi/3, and 4pi/3 so the three channels stay evenly
    // distributed around the color cycle.
    let wave = |phase_off: f32| -> u8 {
        (((theta + phase_off).sin() * 0.5 + 0.5) * 255.0_f32)
            .round()
            .clamp(0.0, 255.0) as u8
    };

    (
        wave(0.0),
        wave(2.094_395_2),
        wave(4.188_790_3),
        blend.clamp(0.0, 1.0),
    )
}

/// Normalized coordinate along `axis` in `[0.0, 1.0]` within `bounds`, then sine-eased
/// mirrored gradient phase (same temporal scaling as [`rainbow_wave_color`]).
pub(crate) fn gradient_sample_t(
    x: i16,
    y: i16,
    phase: u64,
    bounds: RRect,
    frequency: f32,
    speed: f32,
    axis: EffectAxis,
) -> f64 {
    let bw = bounds.width.max(1) as f32;
    let bh = bounds.height.max(1) as f32;
    let bx = bounds.x as f32;
    let by = bounds.y as f32;
    let lx = ((x as f32 - bx + 0.5) / bw).clamp(0.0, 1.0);
    let ly = ((y as f32 - by + 0.5) / bh).clamp(0.0, 1.0);
    let pos = match axis {
        EffectAxis::Horizontal => lx,
        EffectAxis::Vertical => ly,
        EffectAxis::Diagonal => ((lx + ly) * 0.5).clamp(0.0, 1.0),
    };
    let freq = frequency.max(f32::EPSILON);
    let theta = (pos * freq + phase as f32 * speed * 0.04) * std::f32::consts::TAU;
    let t = theta.cos().mul_add(-0.5, 0.5);
    t as f64
}

fn gradient_wave_rgb_alpha(
    x: i16,
    y: i16,
    phase: u64,
    bounds: RRect,
    effect: &VisualEffect,
) -> (u8, u8, u8, f32) {
    let VisualEffect::Gradient {
        gradient,
        blend,
        frequency,
        speed,
        axis,
    } = effect
    else {
        return (255, 255, 255, 0.0);
    };

    let t = gradient_sample_t(x, y, phase, bounds, *frequency, *speed, *axis);
    let c = gradient.color_at(t);
    let (r, g, b) = c.to_rgb().unwrap_or((255, 255, 255));
    (r, g, b, blend.clamp(0.0, 1.0))
}

struct RetroRefreshWave {
    center_y: f32,
    half_height: f32,
    brightness: f32,
    distortion: f32,
    tail_length: f32,
    tail_brightness: f32,
}

struct RetroRefreshWaveArgs {
    half_height: f32,
    brightness: f32,
    distortion: f32,
    tail_length: f32,
    tail_brightness: f32,
}

fn build_retro_refresh_wave(
    intersection: RRect,
    progress: f32,
    flicker: f32,
    args: RetroRefreshWaveArgs,
) -> RetroRefreshWave {
    let flicker = flicker.clamp(0.0, 1.0);
    let expanded_half_height = args.half_height + flicker * 2.0;
    let expanded_tail_length = args.tail_length + flicker * 2.0;
    let travel_margin = expanded_half_height + expanded_tail_length;
    let start_y = intersection.y as f32 - travel_margin;
    let end_y =
        intersection.y as f32 + intersection.height.saturating_sub(1) as f32 + travel_margin;

    RetroRefreshWave {
        center_y: start_y + progress.clamp(0.0, 1.0) * (end_y - start_y),
        half_height: expanded_half_height,
        brightness: args.brightness + flicker * 0.10,
        distortion: args.distortion + flicker * 0.6,
        tail_length: expanded_tail_length,
        tail_brightness: args.tail_brightness + flicker * 0.05,
    }
}

fn vaulttec_hash_u64(mut x: u64) -> u64 {
    x ^= x >> 30;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
}

fn vaulttec_companion_delay(tick: u64) -> Option<u64> {
    let h = vaulttec_hash_u64(tick ^ 0xd31a_0f27_b8c3_9e11);
    if h % 100 < 35 {
        Some(4 + ((h >> 9) % 25))
    } else {
        None
    }
}

struct VaultTecWaveSpawnArgs {
    dimmer: f32,
    half_height: f32,
    tail_length: f32,
}

fn push_vaulttec_wave_if_active(
    waves: &mut Vec<RetroRefreshWave>,
    phase: u64,
    start_tick: u64,
    life: u64,
    flicker: f32,
    intersection: RRect,
    args: VaultTecWaveSpawnArgs,
) {
    if phase < start_tick {
        return;
    }
    let age = phase - start_tick;
    if age >= life {
        return;
    }

    let progress = age as f32 / (life.saturating_sub(1).max(1)) as f32;
    let mut wave = build_retro_refresh_wave(
        intersection,
        progress,
        flicker,
        RetroRefreshWaveArgs {
            half_height: args.half_height,
            brightness: 0.018,
            distortion: 0.0,
            tail_length: args.tail_length,
            tail_brightness: 0.022,
        },
    );
    wave.brightness *= args.dimmer;
    wave.tail_brightness *= args.dimmer;
    waves.push(wave);
}

fn retro_crt_refresh_wave_at_phase(
    intersection: RRect,
    phase: u64,
    flicker: f32,
) -> Option<RetroRefreshWave> {
    if flicker <= 0.0 || intersection.height == 0 {
        return None;
    }

    let (
        window,
        start_mod,
        len_base,
        len_var,
        half_height,
        brightness,
        distortion,
        tail_length,
        tail_brightness,
    ) = (104u64, 46u64, 22u64, 12u64, 1.5, 0.08, 0.6, 1.8, 0.02);
    let segment = phase / window;
    let tick = phase % window;
    let start = segment.wrapping_mul(23) % start_mod;
    let len = len_base + (segment.wrapping_mul(7) % len_var.max(1));
    if tick < start || tick >= start + len {
        return None;
    }

    let progress = (tick - start) as f32 / (len.saturating_sub(1).max(1)) as f32;
    Some(build_retro_refresh_wave(
        intersection,
        progress,
        flicker,
        RetroRefreshWaveArgs {
            half_height,
            brightness,
            distortion,
            tail_length,
            tail_brightness,
        },
    ))
}

fn retro_crt_refresh_waves(
    intersection: RRect,
    phase: u64,
    cadence: RetroRefreshWaveCadence,
    flicker: f32,
) -> Vec<RetroRefreshWave> {
    if matches!(cadence, RetroRefreshWaveCadence::VaultTec) {
        // Non-cyclic scheduler: spawn bands from absolute phase ticks using
        // deterministic pseudo-random slots. Each band owns its own lifecycle.
        let life = 280u64;
        let slot = 160u64;
        let lookback = life + slot * 2;
        let start = phase.saturating_sub(lookback);
        let first_slot = start / slot;
        let last_slot = phase / slot;
        let mut waves = Vec::new();

        for slot_index in first_slot..=last_slot {
            let slot_start = slot_index * slot;
            let h = vaulttec_hash_u64(slot_index ^ 0x8b9d_26af_31c4_17d3);
            let primary_start = slot_start + (h % 100);

            push_vaulttec_wave_if_active(
                &mut waves,
                phase,
                primary_start,
                life,
                flicker,
                intersection,
                VaultTecWaveSpawnArgs {
                    dimmer: 1.0,
                    half_height: 7.6,
                    tail_length: 38.0,
                },
            );

            if let Some(delay) = vaulttec_companion_delay(primary_start) {
                push_vaulttec_wave_if_active(
                    &mut waves,
                    phase,
                    primary_start + delay,
                    life,
                    flicker,
                    intersection,
                    VaultTecWaveSpawnArgs {
                        dimmer: 0.8,
                        half_height: 1.8,
                        tail_length: 12.0,
                    },
                );
            }

            let tertiary = vaulttec_hash_u64(slot_index ^ 0x5f2c_1e88_44b1_0923);
            if tertiary % 100 < 18 {
                let delay = 20 + ((tertiary >> 7) % 36);
                push_vaulttec_wave_if_active(
                    &mut waves,
                    phase,
                    primary_start + delay,
                    life,
                    flicker,
                    intersection,
                    VaultTecWaveSpawnArgs {
                        dimmer: 0.6,
                        half_height: 3.0,
                        tail_length: 18.0,
                    },
                );
            }
        }
        return waves;
    }

    let mut waves = Vec::new();
    if let Some(wave) = retro_crt_refresh_wave_at_phase(intersection, phase, flicker) {
        waves.push(wave);
    }
    waves
}

fn retro_crt_refresh_wave_strength(y: i16, wave: &RetroRefreshWave) -> f32 {
    let dist = ((y as f32 - wave.center_y).abs() / wave.half_height).clamp(0.0, 1.0);
    let envelope = 1.0 - dist;
    envelope * envelope
}

fn retro_crt_refresh_wave_lift(y: i16, phase: u64, wave: &RetroRefreshWave) -> f32 {
    let core = retro_crt_refresh_wave_strength(y, wave) * wave.brightness;
    let tail = if y as f32 <= wave.center_y {
        let distance = (wave.center_y - y as f32) / wave.tail_length.max(1.0);
        if distance <= 1.0 {
            let ripple =
                (((y as f32 * 0.22) + phase as f32 * 0.025).sin() * 0.5 + 0.5) * 0.12 + 0.88;
            (1.0 - distance).powi(2) * wave.tail_brightness * ripple
        } else {
            0.0
        }
    } else {
        0.0
    };
    core + tail
}

fn retro_crt_vignette_dim(x: i16, y: i16, bounds: RRect, strength: f32) -> f32 {
    if strength <= 0.0 || bounds.width <= 1 || bounds.height <= 1 {
        return 0.0;
    }

    let nx = (x as f32 - bounds.x as f32) / bounds.width.saturating_sub(1) as f32;
    let ny = (y as f32 - bounds.y as f32) / bounds.height.saturating_sub(1) as f32;
    let dx = ((nx - 0.5).abs() / 0.5).clamp(0.0, 1.0);
    let dy = ((ny - 0.5).abs() / 0.5).clamp(0.0, 1.0);
    let radial = ((dx * dx + dy * dy) * 0.5).clamp(0.0, 1.0);
    radial.powf(2.2) * strength
}

fn strip_clipped(mut e: &VisualEffect) -> &VisualEffect {
    while let VisualEffect::Clipped { inner, .. } = e {
        e = inner;
    }
    e
}

fn cell_passes_visual_clip(effect: &VisualEffect, scope_rect: Rect, x: i16, y: i16) -> bool {
    let mut e = effect;
    while let VisualEffect::Clipped {
        bounds,
        mask,
        inner,
    } = e
    {
        let lx = x - scope_rect.x;
        let ly = y - scope_rect.y;
        if let Some(b) = bounds
            && !b.contains(lx, ly)
        {
            return false;
        }
        if let Some(m) = mask
            && !m.test_scope_local(lx, ly)
        {
            return false;
        }
        e = inner;
    }
    true
}

fn apply_retro_crt_refresh_wave(
    buf: &mut Buffer,
    intersection: RRect,
    phase: u64,
    wave: &RetroRefreshWave,
    gate: &dyn Fn(u16, u16) -> bool,
) {
    if wave.distortion <= 0.0 {
        return;
    }

    let width = intersection.width as usize;
    let height = intersection.height as usize;
    let mut snapshot = Vec::with_capacity(width * height);
    for y in intersection.y..intersection.y + intersection.height {
        for x in intersection.x..intersection.x + intersection.width {
            snapshot.push(buf.cell((x, y)).cloned().unwrap_or_default());
        }
    }

    let sample = |sx: usize, sy: usize| -> &Cell { &snapshot[sy * width + sx] };
    for local_y in 0..height {
        let row_y = intersection.y as i16 + local_y as i16;
        let strength = retro_crt_refresh_wave_strength(row_y, wave);
        if strength <= 0.01 {
            continue;
        }

        let offset =
            (((local_y as f32 * 0.8) + phase as f32 * 0.35).sin() * wave.distortion * strength)
                .round() as isize;
        if offset == 0 {
            continue;
        }

        for local_x in 0..width {
            let source_x = (local_x as isize + offset).clamp(0, width.saturating_sub(1) as isize);
            let dst_x = intersection.x + local_x as u16;
            let dst_y = intersection.y + local_y as u16;
            if !gate(dst_x, dst_y) {
                continue;
            }
            if let Some(cell) = buf.cell_mut((dst_x, dst_y)) {
                *cell = sample(source_x as usize, local_y).clone();
            }
        }
    }
}

fn apply_visual_effect_base_fill(
    buf: &mut Buffer,
    intersection: RRect,
    fill_color: Option<RColor>,
    gate: &dyn Fn(u16, u16) -> bool,
) {
    let Some(fill_color) = fill_color else {
        return;
    };

    for y in intersection.y..intersection.y + intersection.height {
        for x in intersection.x..intersection.x + intersection.width {
            if !gate(x, y) {
                continue;
            }
            if let Some(cell) = buf.cell_mut((x, y))
                && cell.bg == RColor::Reset
            {
                cell.bg = fill_color;
            }
        }
    }
}

struct VisualEffectCellParams<'a> {
    quantize_palette: Option<&'a [(u8, u8, u8)]>,
    retro_crt: Option<RetroCrtParams>,
    retro_refresh_waves: &'a [RetroRefreshWave],
    terminal_bg: Option<RColor>,
}

fn apply_visual_effect_to_cell(
    cell: &mut Cell,
    x: i16,
    y: i16,
    phase: u64,
    effect_bounds: RRect,
    effect: &VisualEffect,
    params: &VisualEffectCellParams<'_>,
) {
    let effect = strip_clipped(effect);
    let quantize_palette = params.quantize_palette;
    let retro_crt = params.retro_crt;
    let retro_refresh_waves = params.retro_refresh_waves;
    match effect {
        VisualEffect::Monochrome { strength } => {
            cell.fg = monochrome_ratatui_color(cell.fg, *strength);
            cell.bg = monochrome_ratatui_color(cell.bg, *strength);
        }
        VisualEffect::PaletteQuantize { .. } => {
            let Some(palette) = quantize_palette else {
                return;
            };
            cell.fg = palette_quantize_ratatui_color(cell.fg, palette);
            cell.bg = palette_quantize_ratatui_color(cell.bg, palette);
        }
        VisualEffect::Scanlines { strength, spacing } => {
            let spacing = (*spacing).max(1) as i16;
            if y.rem_euclid(spacing) == 0 {
                cell.fg = dim_ratatui_color(cell.fg, *strength);
                cell.bg = dim_ratatui_color(cell.bg, *strength);
            }
        }
        VisualEffect::RainbowWave { .. } => {
            let (rr, rg, rb, alpha) = rainbow_wave_color(x, y, phase, effect);
            cell.fg = map_ratatui_channel_if_not_reset(cell.fg, |rgb| {
                blend_rgb_toward(rgb, (rr, rg, rb), alpha)
            });
            cell.bg = map_ratatui_channel_if_not_reset(cell.bg, |rgb| {
                blend_rgb_toward(rgb, (rr, rg, rb), alpha)
            });
        }
        VisualEffect::Gradient { .. } => {
            let (rr, rg, rb, alpha) = gradient_wave_rgb_alpha(x, y, phase, effect_bounds, effect);
            cell.fg = map_ratatui_channel_if_not_reset(cell.fg, |rgb| {
                blend_rgb_toward(rgb, (rr, rg, rb), alpha)
            });
            cell.bg = map_ratatui_channel_if_not_reset(cell.bg, |rgb| {
                blend_rgb_toward(rgb, (rr, rg, rb), alpha)
            });
        }
        VisualEffect::Ripple {
            origin,
            radius,
            ring_width,
            tint,
            strength,
        } => {
            let Some((effective_radius, strength_multiplier)) =
                resolve_ripple_radius(radius, phase)
            else {
                return;
            };
            let (cx, cy) = origin.resolve(effect_bounds.width, effect_bounds.height);
            apply_ripple_to_cell(
                cell,
                x,
                y,
                effect_bounds,
                RippleCellArgs {
                    cx,
                    cy,
                    radius: effective_radius,
                    ring_width: *ring_width,
                    tint: *tint,
                    strength: *strength * strength_multiplier,
                },
            );
        }
        VisualEffect::RetroCrt {
            preset: _,
            flicker,
            scanline_strength,
        } => {
            let Some(retro_params) = retro_crt else {
                return;
            };

            cell.fg = monochrome_ratatui_color(cell.fg, retro_params.mono_strength);
            cell.bg = monochrome_ratatui_color(cell.bg, retro_params.mono_strength);

            cell.fg = palette_quantize_ratatui_color(cell.fg, retro_params.palette);
            cell.bg = palette_quantize_ratatui_color(cell.bg, retro_params.palette);

            if let Some((tint, alpha)) = retro_params.tint {
                if cell.fg != RColor::Reset {
                    cell.fg = tint_ratatui_color(cell.fg, tint, alpha);
                }
                if cell.bg != RColor::Reset {
                    cell.bg = tint_ratatui_color(cell.bg, tint, alpha);
                } else {
                    cell.bg = RColor::Reset;
                }
            }

            let scanline = VisualEffect::Scanlines {
                strength: scanline_strength.clamp(0.0, 1.0),
                spacing: 2,
            };
            let scanline_params = VisualEffectCellParams {
                quantize_palette: None,
                retro_crt: None,
                retro_refresh_waves: &[],
                terminal_bg: params.terminal_bg,
            };
            apply_visual_effect_to_cell(
                cell,
                x,
                y,
                phase,
                effect_bounds,
                &scanline,
                &scanline_params,
            );

            let flicker = flicker.clamp(0.0, 1.0)
                * ((phase as f32 * 0.7).sin() * 0.5 + 0.5)
                * retro_params.flicker_strength;
            let refresh_lift = retro_refresh_waves
                .iter()
                .map(|wave| retro_crt_refresh_wave_lift(y, phase, wave))
                .sum::<f32>();
            let vignette =
                retro_crt_vignette_dim(x, y, effect_bounds, retro_params.vignette_strength);
            let fg_dim =
                (retro_params.base_dim + flicker + vignette - refresh_lift).clamp(0.0, 1.0);
            cell.fg = dim_ratatui_color(cell.fg, fg_dim);
            cell.bg = dim_ratatui_color(cell.bg, fg_dim);
        }
        VisualEffect::ColorTransform { fg, bg } => {
            apply_color_transforms_to_cell(cell, *fg, *bg, params.terminal_bg);
        }
        VisualEffect::ContrastPolicy(policy) => {
            apply_contrast_policy_to_cell(cell, *policy);
        }
        VisualEffect::Custom(effect) => {
            let ctx = EffectContext {
                x,
                y,
                bounds: Rect {
                    x: effect_bounds.x as i16,
                    y: effect_bounds.y as i16,
                    w: effect_bounds.width,
                    h: effect_bounds.height,
                },
                phase,
                terminal_bg: params.terminal_bg,
            };
            effect.apply(cell, &ctx);
        }
        VisualEffect::Clipped { .. } => {}
    }
}

fn resolve_ripple_radius(radius: &RippleRadius, phase: u64) -> Option<(f32, f32)> {
    match radius {
        RippleRadius::Fixed(radius) => Some((*radius, 1.0)),
        RippleRadius::Loop {
            max_radius,
            period_ticks,
        } => {
            let period = (*period_ticks).max(1) as u64;
            let t = (phase % period) as f32 / period as f32;
            Some((ease_out_quad(t) * *max_radius, 1.0 - t))
        }
        RippleRadius::Once {
            max_radius,
            duration_ticks,
            start_tick,
        } => {
            let elapsed = phase.checked_sub(*start_tick)?;
            let duration = (*duration_ticks).max(1) as u64;
            if elapsed >= duration {
                return None;
            }
            let t = elapsed as f32 / duration as f32;
            Some((ease_out_quad(t) * *max_radius, 1.0 - t))
        }
    }
}

fn ease_out_quad(t: f32) -> f32 {
    1.0 - (1.0 - t).powi(2)
}

struct RippleCellArgs {
    cx: f32,
    cy: f32,
    radius: f32,
    ring_width: f32,
    tint: Color,
    strength: f32,
}

fn apply_ripple_to_cell(
    cell: &mut Cell,
    x: i16,
    y: i16,
    effect_bounds: RRect,
    args: RippleCellArgs,
) {
    // Convert to local cell coords. Scale y up because terminal cells are roughly
    // twice as tall as wide; one row should count like about two columns.
    let local_x = x as f32 - effect_bounds.x as f32 + 0.5;
    let local_y = y as f32 - effect_bounds.y as f32 + 0.5;
    let dist = ((local_x - args.cx).powi(2) + ((local_y - args.cy) * 2.0).powi(2)).sqrt();
    let rw = args.ring_width.max(f32::EPSILON);
    let falloff = (1.0 - ((dist - args.radius).abs() / rw)).max(0.0);
    if falloff > 0.0 {
        let alpha = falloff * args.strength;
        if cell.fg != RColor::Reset {
            cell.fg = tint_ratatui_color(cell.fg, args.tint, alpha);
        }
        if cell.bg != RColor::Reset {
            cell.bg = tint_ratatui_color(cell.bg, args.tint, alpha);
        }
    }
}

pub(crate) fn apply_visual_effects_clipped(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    effects: &[VisualEffect],
    phase: u64,
    clip_rect: Option<Rect>,
    terminal_bg: Option<RColor>,
) {
    if effects.is_empty() {
        return;
    }

    let mut draw_rect = rect;
    if let Some(clip) = clip_rect {
        draw_rect = draw_rect.intersection(&clip);
    }
    if draw_rect.is_empty() {
        return;
    }

    let r_rect = to_ratatui_rect(draw_rect);
    let intersection = f.area().intersection(r_rect);
    if intersection.width == 0 || intersection.height == 0 {
        return;
    }

    let buf = f.buffer_mut();
    for effect in effects {
        let gate = |x: u16, y: u16| cell_passes_visual_clip(effect, draw_rect, x as i16, y as i16);

        let peeled = strip_clipped(effect);
        let retro_crt = match peeled {
            VisualEffect::RetroCrt { preset, .. } => Some(retro_crt_params(*preset)),
            _ => None,
        };
        let fill_color = match peeled {
            VisualEffect::ColorTransform { .. }
            | VisualEffect::ContrastPolicy(_)
            | VisualEffect::Custom(_) => None,
            _ => retro_crt
                .and_then(|params| params.screen_bg)
                .or(terminal_bg),
        };
        apply_visual_effect_base_fill(buf, intersection, fill_color, &gate);

        let retro_refresh_waves = match peeled {
            VisualEffect::RetroCrt { flicker, .. } => retro_crt_refresh_waves(
                intersection,
                phase,
                retro_crt
                    .map(|params| params.refresh_wave_cadence)
                    .unwrap_or(RetroRefreshWaveCadence::Classic),
                *flicker,
            ),
            _ => Vec::new(),
        };
        for wave in &retro_refresh_waves {
            apply_retro_crt_refresh_wave(buf, intersection, phase, wave, &gate);
        }
        let quantize_palette = match peeled {
            VisualEffect::PaletteQuantize { palette } => Some(visual_effect_palette(palette)),
            _ => None,
        };
        let quantize_palette = quantize_palette.as_deref();
        let effect_bounds = Rect {
            x: r_rect.x as i16,
            y: r_rect.y as i16,
            w: r_rect.width,
            h: r_rect.height,
        };
        let prepared_custom = match peeled {
            VisualEffect::Custom(effect) => effect.prepare(&EffectPrepareContext {
                bounds: effect_bounds,
                phase,
                terminal_bg,
            }),
            _ => None,
        };
        let cell_params = VisualEffectCellParams {
            quantize_palette,
            retro_crt,
            retro_refresh_waves: retro_refresh_waves.as_slice(),
            terminal_bg,
        };
        for y in intersection.y..intersection.y + intersection.height {
            for x in intersection.x..intersection.x + intersection.width {
                if !gate(x, y) {
                    continue;
                }
                if let Some(cell) = buf.cell_mut((x, y)) {
                    match peeled {
                        VisualEffect::Custom(effect) => {
                            let ctx = EffectContext {
                                x: x as i16,
                                y: y as i16,
                                bounds: effect_bounds,
                                phase,
                                terminal_bg,
                            };
                            if let Some(prepared) = prepared_custom.as_deref() {
                                prepared.apply(cell, &ctx);
                            } else {
                                effect.apply(cell, &ctx);
                            }
                        }
                        _ => apply_visual_effect_to_cell(
                            cell,
                            x as i16,
                            y as i16,
                            phase,
                            r_rect,
                            effect,
                            &cell_params,
                        ),
                    }
                }
            }
        }
    }
}

pub(crate) fn tint_ratatui_color(color: RColor, tint: Color, alpha: f32) -> RColor {
    if matches!(tint, Color::Transparent | Color::Backdrop | Color::Reset) {
        return color;
    }

    let our = match color {
        RColor::Reset => Color::Reset,
        RColor::Black => Color::Black,
        RColor::Red => Color::Red,
        RColor::Green => Color::Green,
        RColor::Yellow => Color::Yellow,
        RColor::Blue => Color::Blue,
        RColor::Magenta => Color::Magenta,
        RColor::Cyan => Color::Cyan,
        RColor::Gray => Color::Gray,
        RColor::DarkGray => Color::DarkGray,
        RColor::LightRed => Color::LightRed,
        RColor::LightGreen => Color::LightGreen,
        RColor::LightYellow => Color::LightYellow,
        RColor::LightBlue => Color::LightBlue,
        RColor::LightMagenta => Color::LightMagenta,
        RColor::LightCyan => Color::LightCyan,
        RColor::White => Color::White,
        RColor::Indexed(i) => Color::Indexed(i),
        RColor::Rgb(r, g, b) => Color::Rgb(r, g, b),
    };

    match our.blend_toward(tint, alpha) {
        Color::Rgb(r, g, b) => RColor::Rgb(r, g, b),
        _ => color,
    }
}

#[cfg(test)]
mod palette_fidelity_tests {
    use super::*;

    #[test]
    fn opacity_keeps_palette_color_and_dims_instead_of_emitting_truecolor() {
        // A named (palette) foreground must survive an opacity fade as the same named
        // color so the terminal palette still resolves it; de-emphasis is carried by DIM.
        let mut cell = Cell::default();
        cell.set_fg(RColor::LightCyan);
        apply_color_transforms_to_cell(
            &mut cell,
            Some(ColorTransform::Opacity(0.4)),
            None,
            Some(RColor::Black),
        );
        assert_eq!(
            cell.fg,
            RColor::LightCyan,
            "palette fg must stay on-palette"
        );
        assert!(
            cell.modifier.contains(RMod::DIM),
            "darkened palette fg should gain DIM"
        );
    }

    #[test]
    fn opacity_still_blends_truecolor_foregrounds() {
        // Truecolor inputs keep blending exactly as before (no behavior change).
        let mut cell = Cell::default();
        cell.set_fg(RColor::Rgb(0, 255, 255));
        cell.set_bg(RColor::Black);
        apply_color_transforms_to_cell(
            &mut cell,
            Some(ColorTransform::Opacity(0.4)),
            None,
            Some(RColor::Black),
        );
        assert!(
            matches!(cell.fg, RColor::Rgb(..)),
            "truecolor fg should remain blended truecolor, got {:?}",
            cell.fg
        );
        assert!(!cell.modifier.contains(RMod::DIM));
    }

    #[test]
    fn transform_preserves_indexed_palette_colors() {
        let (color, dim) = transform_ratatui_color(
            RColor::Indexed(14),
            ColorTransform::Opacity(0.3),
            Some(RColor::Black),
            true,
        );
        assert_eq!(color, RColor::Indexed(14));
        assert!(dim);
    }
}
