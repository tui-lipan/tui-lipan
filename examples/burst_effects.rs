//! Hold on the logo to charge, release to launch - ripple ring, supernova burst, per-glyph ripples, or a custom vortex effect on the same logo text.
//! Run with: cargo run --example burst_effects --features big-text

use std::collections::HashMap;
use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tui_lipan::CellMask;
use tui_lipan::prelude::*;

const R_CELLS_PER_SEC: f32 = 22.0;
const R_RING_WIDTH: f32 = 2.5;
const R_MIN_RADIUS: f32 = 8.0;
const R_MAX_RADIUS: f32 = 30.0;
const R_MAX_CHARGE_SIZE: f32 = 4.5;
const R_CHARGE_FILL_SECS: f32 = 1.2;

const SN_CHARGE_FILL_SECS: f32 = 1.4;
const SN_CHARGE_MAX_GLOW: f32 = 5.5;
const SN_AURA_RINGS: usize = 5;
const SN_SHOCKWAVE_SPEED_CORE: f32 = 52.0;
const SN_SHOCKWAVE_SPEED_FIRE: f32 = 36.0;
const SN_SHOCKWAVE_SPEED_OUTER: f32 = 24.0;
const SN_MIN_MAX_RADIUS: f32 = 10.0;
const SN_MAX_MAX_RADIUS: f32 = 30.0;
const SN_SPARK_COUNT: usize = 28;
const SN_FLASH_DURATION: f32 = 0.38;

const LOGO_BIG_TEXT: &str = "TUI-LIPAN";
const LETTER_FONT: BigFont = BigFont::AnsiShadow;
const LETTER_RING: f32 = 2.2;
const LETTER_R_SPEED: f32 = 18.0;
const LETTER_R_MAX: f32 = 36.0;
const LETTER_STAGGER_SECS: f32 = 0.07;
const OPENCODE_LOGO_JSON: &str = include_str!("assets/opencode_logo.json");

const OC_CHARGE_SECS: f32 = 3.0;
const OC_HOLD_SECS: f32 = 0.09;
const OC_LIFE_SECS: f32 = 1.02;
const OC_WIDTH: f32 = 0.76;
const OC_GAIN: f32 = 2.3;
const OC_FLASH: f32 = 2.15;
const OC_TRAIL: f32 = 0.28;
const OC_SWELL: f32 = 0.24;
const OC_WIDE: f32 = 1.85;
const OC_DRIFT: f32 = 1.45;
const OC_EXPAND: f32 = 1.62;
const OC_DIM: f32 = 1.04;
const OC_KICK: f32 = 0.86;
const OC_SUCK: f32 = 0.34;
const OC_ARC: f32 = 2.2;
const OC_FORK: f32 = 1.2;
const OC_GLOW_OUT: f32 = 1.6;
const OC_TRACE_SPEED: f32 = 0.033;
const OC_TRACE_TAIL: f32 = 1.8;
const OC_TRACE_IN: f32 = 0.2;
const OC_SPIN_MIN: f32 = 0.008;
const OC_SPIN_MAX: f32 = 0.052;

#[derive(Clone, Copy, Debug)]
struct OpenCodeTheme {
    background: TerminalColor,
    primary: TerminalColor,
    peak: TerminalColor,
}

impl OpenCodeTheme {
    fn upstream() -> Self {
        Self {
            background: TerminalColor::Rgb(10, 10, 12),
            primary: TerminalColor::Rgb(250, 178, 131),
            peak: TerminalColor::Rgb(255, 255, 255),
        }
    }
}

#[derive(Clone, Debug)]
struct OpenCodeLogoEffect {
    theme: OpenCodeTheme,
    charge: Option<OpenCodeChargeEffect>,
    held: Option<OpenCodeHeldGlyphEffect>,
    bursts: Arc<[OpenCodeBurstEffect]>,
    blooms: Arc<[OpenCodeBloomEffect]>,
}

#[derive(Clone, Copy, Debug)]
struct OpenCodeChargeEffect {
    cx: f32,
    cy: f32,
    rise: f32,
    phase: f32,
}

#[derive(Clone, Copy, Debug)]
struct OpenCodeBurstEffect {
    cx: f32,
    cy: f32,
    elapsed: f32,
    level: f32,
    rise: f32,
}

/// Mask + trace path for the glyph currently held — drives both vortex localization and trace effect.
#[derive(Clone, Debug)]
struct OpenCodeHeldGlyphEffect {
    mask: Arc<CellMask>,
    age_ms: f32,
    rise: f32,
    /// Per-cell trace index lookup: (path_index, path_length) keyed by mask-local cell.
    trace: Arc<HashMap<(u16, u16), (u16, u16)>>,
}

/// Per-glyph release flash (upstream `bloom`).
#[derive(Clone, Debug)]
struct OpenCodeBloomEffect {
    age: f32,
    force: f32,
    mask: Arc<CellMask>,
    cx: f32,
    cy_px: f32,
}

#[derive(Clone, Copy)]
struct OpenCodePulse {
    peak: f32,
    primary: f32,
    dim: f32,
}

#[derive(Clone, Copy)]
struct OpenCodeChargePulse {
    full: OpenCodePulse,
    glitch: OpenCodePulse,
    pick: OpenCodePulse,
}

#[derive(Clone, Copy, Debug)]
struct VortexEffect {
    intensity: f32,
}

impl CellEffect for VortexEffect {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        let w = ctx.bounds.w.max(1) as f32;
        let h = ctx.bounds.h.max(1) as f32;
        let lx = ctx.x as f32 - ctx.bounds.x as f32 + 0.5;
        let ly = ctx.y as f32 - ctx.bounds.y as f32 + 0.5;
        let dx = (lx - w * 0.5) / w.max(1.0);
        let dy = (ly - h * 0.5) / h.max(1.0) * 2.0;
        let radius = (dx * dx + dy * dy).sqrt();
        let angle = dy.atan2(dx);
        let spin = ctx.phase as f32 * 0.075;
        let spiral = ((angle * 4.0 - radius * 30.0 + spin).sin() * 0.5 + 0.5).powf(1.8);
        let core = (1.0 - radius * 3.2).clamp(0.0, 1.0);
        let glow = (spiral * self.intensity + core * 0.9).clamp(0.0, 1.0);

        if glow > 0.78 {
            cell.set_fg(TerminalColor::Rgb(255, 255, 230));
            if cell.bg != TerminalColor::Reset {
                cell.set_bg(TerminalColor::Rgb(70, 35, 120));
            }
        } else if glow > 0.52 {
            cell.set_fg(TerminalColor::Rgb(120, 230, 255));
            if cell.bg != TerminalColor::Reset {
                cell.set_bg(TerminalColor::Rgb(25, 35, 90));
            }
        } else if glow > 0.28 {
            cell.set_fg(TerminalColor::Rgb(120, 130, 255));
        }
    }

    fn is_animated(&self) -> bool {
        true
    }

    fn cache_key(&self) -> u64 {
        self.intensity.to_bits() as u64
    }
}

impl CellEffect for OpenCodeLogoEffect {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        let lx = ctx.x as f32 - ctx.bounds.x as f32;
        let ly = ctx.y as f32 - ctx.bounds.y as f32;
        let mask_x = ctx.x - ctx.bounds.x;
        let mask_y = ctx.y - ctx.bounds.y;

        // Ink-only contributions. Vortex + trace stay on the held glyph, while the upstream
        // crackle/fork/lash terms can leak around the charge point. None of these touch shadows.
        let mut ink_only = OpenCodePulse::default();
        let in_held = self
            .held
            .as_ref()
            .is_some_and(|h| h.mask.test_scope_local(mask_x, mask_y));
        let charge = self
            .charge
            .as_ref()
            .map(|charge| opencode_charge_pulse(lx, ly, *charge));
        if in_held {
            if let Some(charge) = charge.as_ref() {
                ink_only.peak += charge.full.peak + charge.pick.peak;
                ink_only.primary += charge.full.primary + charge.pick.primary;
            }
            if let Some(held) = self.held.as_ref()
                && let Some(&(idx, len)) = held.trace.get(&(mask_x as u16, mask_y as u16))
            {
                let trace = opencode_trace_pulse(idx, len, held.age_ms, held.rise);
                ink_only.peak += trace.peak;
                ink_only.primary += trace.primary;
            }
        } else if let Some(charge) = charge.as_ref() {
            // Upstream's field has sparse crackle around the charge point, not just
            // inside the selected glyph. Keep the vortex itself glyph-local, but let
            // the glitch/fork/lash terms leak onto nearby ink cells.
            ink_only.peak += charge.glitch.peak;
            ink_only.primary += charge.glitch.primary;
        }

        // Shared contributions — release shockwave + per-glyph bloom + the *dim* component
        // of the charge field. Shadow cells get a ghost-scaled (0.18 / 0.08) version of these,
        // matching upstream's `ghost()` treatment.
        let mut shared = OpenCodePulse::default();
        for bloom in self.blooms.iter() {
            if !bloom.mask.test_scope_local(mask_x, mask_y) {
                continue;
            }
            let pb = opencode_bloom_pulse(lx, ly, bloom);
            opencode_overlay_light_pulse(&mut shared, pb);
        }
        for burst in self.bursts.iter() {
            let wave = opencode_burst_wave(lx, ly, *burst, ctx.bounds);
            opencode_overlay_light_pulse(&mut shared, wave);
            shared.dim = shared.dim.max(wave.dim);
        }
        // Charge field's dim channel applies *globally*: the rest of the logo darkens
        // around the held letter while charging — this is what gives the held glyph
        // visual prominence without needing extra brightness on it.
        if let Some(charge) = charge.as_ref() {
            shared.dim += charge.full.dim;
        }

        let ink_pulse = OpenCodePulse {
            peak: ink_only.peak + shared.peak,
            primary: ink_only.primary + shared.primary,
            dim: shared.dim,
        };
        let shadow_pulse = OpenCodePulse {
            peak: shared.peak * 0.18,
            primary: shared.primary * 0.18,
            dim: shared.dim,
        };

        let fg_pulse = if opencode_is_ink(cell.fg) {
            ink_pulse
        } else {
            shadow_pulse
        };
        cell.fg = opencode_shade(cell.fg, fg_pulse, self.theme, 1.0);
        if cell.bg != TerminalColor::Reset {
            let bg_pulse = if opencode_is_ink(cell.bg) {
                ink_pulse
            } else {
                shadow_pulse
            };
            cell.bg = opencode_shade(cell.bg, bg_pulse, self.theme, 0.82);
        }
    }

    fn is_animated(&self) -> bool {
        true
    }
}

/// Classify a cell color as "ink" (the two brighter logo colors) vs "shadow" (the two darker
/// ones, including background). Threshold is tuned for the AsciiCanvas color map produced by
/// `opencode_color_map`: bright `#F2EDED` (lum≈239), mid `#B8B2B2` (lum≈180), shadow
/// `#4B4646` (lum≈71), background black (lum=0).
fn opencode_is_ink(color: TerminalColor) -> bool {
    let Some((r, g, b)) = terminal_rgb(color) else {
        return false;
    };
    let lum = 0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32;
    lum > 130.0
}

impl Default for OpenCodePulse {
    fn default() -> Self {
        Self {
            peak: 0.0,
            primary: 0.0,
            dim: 0.0,
        }
    }
}

fn opencode_charge_pulse(x: f32, y: f32, charge: OpenCodeChargeEffect) -> OpenCodeChargePulse {
    let rise = opencode_ramp(charge.rise, OC_HOLD_SECS, OC_CHARGE_SECS);
    let level = opencode_push(rise);
    let storm = level * level;
    let dx = x + 0.5 - charge.cx - 0.5;
    let dy = y * 2.0 + 1.0 - charge.cy * 2.0 - 1.0;
    let dist = (dx * dx + dy * dy).sqrt();
    let angle = dy.atan2(dx);
    let spin_phase = charge.phase * (1.0 + 1.25 * storm * level);
    let spin = spin_phase * (OC_SPIN_MIN + (OC_SPIN_MAX - OC_SPIN_MIN) * storm);
    let spark_phase = opencode_noise(charge.cx, charge.cy, charge.phase);
    let dim = OC_DIM * rise * (0.99 + 0.02 * (charge.phase * 0.014).sin());
    let core = (-(dist * dist) / (0.22 + (3.2 - 0.22) * rise).max(0.22)).exp()
        * (0.42 + (2.45 - 0.42) * rise);
    let shell_r = 0.16 + (2.05 - 0.16) * rise;
    let shell_w = (0.18 + (0.82 - 0.18) * rise).max(0.18);
    let shell = (-(((dist - shell_r) / shell_w).powi(2))).exp() * (0.1 + (0.95 - 0.1) * rise);
    let ember_r = 0.45 + (2.65 - 0.45) * rise;
    let ember_w = (0.14 + (0.62 - 0.14) * rise).max(0.14);
    let ember = (-(((dist - ember_r) / ember_w).powi(2))).exp() * (0.02 + (0.78 - 0.02) * rise);
    let arc = (angle * 3.0 - spin + spark_phase * 2.2)
        .cos()
        .max(0.0)
        .powi(8);
    let seam = (angle * 5.0 + spin * 1.55).cos().max(0.0).powi(12);
    let ring = (-(((dist - (1.05 + (3.0 - 1.05) * level)) / 0.48).powi(2))).exp()
        * arc
        * (0.03 + (0.5 + OC_ARC - 0.03) * storm);
    let fork = (-(((dist - (1.55 + storm * 2.1)) / 0.36).powi(2))).exp() * seam * storm * OC_FORK;
    let spark = (opencode_noise(x, y, charge.phase) - (0.94 + (0.66 - 0.94) * storm)).max(0.0)
        * 5.4
        * storm;
    let glitch = spark * (-dist / (3.1 - storm).max(1.2)).exp();
    let crack = ((dx - dy) * 1.6 + spin * 2.1).cos().max(0.0).powi(18);
    let lash = crack * (-(((dist - (1.95 + storm * 2.0)) / 0.28).powi(2))).exp() * storm * 1.1;
    let flicker = (opencode_noise(charge.cx * 3.1, charge.cy * 2.7, charge.phase * 1.7) - 0.72)
        .max(0.0)
        * (-(dist * dist) / 0.15).exp()
        * (0.08 + (0.42 - 0.08) * rise);
    let energy = core + shell + ember + ring + fork + glitch + lash;
    let n = (energy + flicker - dim).max(0.0);
    let mut full = opencode_glow_pulse(n);
    full.dim = (dim - energy - flicker).max(0.0);
    let glitch_falloff = (-((dist / 1.75).powi(2))).exp();
    let glitch_n =
        (fork + glitch + lash + flicker).max(0.0) * (0.38 + 0.62 * storm) * glitch_falloff;
    let pick_n = (-(dist * dist) / 1.7).exp() * (0.2 + (0.96 - 0.2) * rise);

    OpenCodeChargePulse {
        full,
        glitch: opencode_glow_pulse(glitch_n),
        pick: opencode_glow_pulse(pick_n),
    }
}

fn opencode_burst_wave(x: f32, y: f32, burst: OpenCodeBurstEffect, bounds: Rect) -> OpenCodePulse {
    if burst.elapsed < 0.0 || burst.elapsed > OC_LIFE_SECS {
        return OpenCodePulse::default();
    }
    let p = (burst.elapsed / OC_LIFE_SECS).clamp(0.0, 1.0);
    let span = ((bounds.w as f32).hypot(bounds.h as f32 * 2.0)) * 0.94;
    let wave_cx = burst.cx + 0.5;
    let wave_cy = burst.cy * 2.0 + 1.0;
    let dx = x + 0.5 - wave_cx;
    let dy = y * 2.0 + 1.0 - wave_cy;
    let dist = (dx * dx + dy * dy).sqrt();
    let radius = span * (1.0 - (1.0 - p).powf(OC_EXPAND));
    let fade = (1.0 - p).powf(1.32);
    let jitter =
        1.02 + opencode_noise(x + burst.cx * 0.7, y + burst.cy * 0.7, burst.elapsed * 60.0) * 0.52;
    let force = 0.82 + (2.55 - 0.82) * burst.level;
    let kick = 0.32 + OC_KICK * burst.level;
    let edge = (-(((dist - radius) / OC_WIDTH).powi(2))).exp() * OC_GAIN * fade * force * jitter;
    let swell = (-(((dist - (radius - OC_DRIFT).max(0.0)) / OC_WIDE).powi(2))).exp()
        * OC_SWELL
        * fade
        * force;
    let trail = if dist < radius {
        (-(radius - dist) / 2.4).exp() * OC_TRAIL * fade * force * (0.92 + (1.22 - 0.92) * jitter)
    } else {
        0.0
    };
    let wake = if dist < radius {
        (-(radius - dist) / 1.25).exp() * 0.32 * fade
    } else {
        0.0
    };
    let flash = (-(dist * dist) / 3.2).exp()
        * OC_FLASH
        * force
        * (1.0 - burst.elapsed / 0.14).max(0.0)
        * (0.95 + (1.18 - 0.95) * jitter);
    let kick_dim = (-(dist * dist) / 2.0).exp() * kick * (1.0 - burst.elapsed / 0.1).max(0.0);
    let suck = (-(((dist - 1.25) / 0.75).powi(2))).exp()
        * kick
        * OC_SUCK
        * (1.0 - burst.elapsed / 0.11).max(0.0);
    let remain = if dist <= radius {
        (1.0 - ((radius - dist) / 1.35).min(1.0)).max(0.0)
    } else {
        1.0
    };
    let positive_wave = (edge + swell + trail + flash + wake).max(0.0);
    let wave = positive_wave - kick_dim - suck;

    // Preserve the charge-time dim outside the wave radius so the surrounding logo
    // stays darkened until the expanding shockwave passes through it.
    // `remain` is 1.0 outside the wave and fades to 0 across a 1.35-cell band at the edge,
    // so the dim is "flushed" cell-by-cell as the wave sweeps over it.
    let preserved_dim = opencode_frozen_dim(x, y, burst.cx, burst.cy, burst.rise) * remain;

    // Glitchy tail behind the wave front: sparse noise pinpoints inside the wave radius,
    // strongest just behind the edge, fading toward the center. Force is non-zero even on
    // a quick tap (force ≥ 0.82), so the tail is visible on every release.
    let glitch_amp = if dist < radius {
        let n = opencode_noise(
            x + burst.cx * 1.7,
            y + burst.cy * 1.3,
            burst.elapsed * 80.0 + dist * 3.0,
        );
        let raw = (n - 0.55).max(0.0) * 2.2;
        let edge_proximity = (-(radius - dist) / 4.0).exp();
        raw * fade * force * edge_proximity
    } else {
        0.0
    };

    let ahead = (dist - radius).max(0.0);
    let front_glow = if ahead > 0.0 {
        (-((ahead / 1.35).powi(2))).exp() * fade * force * jitter * 0.34
    } else {
        0.0
    };
    let energy = positive_wave + front_glow + glitch_amp * 0.72;
    let mut pulse = opencode_glow_pulse(energy);
    pulse.peak += (edge * 0.24 + front_glow * 0.42 + flash.max(0.0) * 0.08).clamp(0.0, 1.0);
    pulse.primary += (trail * 0.78 + wake * 0.46 + swell * 0.28 + glitch_amp * 0.18).max(0.0);
    let dim_ahead = if dist > radius {
        preserved_dim * (1.0 - (front_glow * 1.8).clamp(0.0, 1.0))
    } else {
        0.0
    };
    pulse.dim = (-wave).max(0.0) * 0.22 + dim_ahead;
    pulse
}

/// Frozen-rise version of the charge field's dim channel. Used during the burst phase
/// (when the live `charge` is gone) so the held letter's local protection persists and
/// the surrounding cells stay darkened until the shockwave sweeps over them.
fn opencode_frozen_dim(x: f32, y: f32, cx: f32, cy: f32, rise: f32) -> f32 {
    if rise <= 0.0 {
        return 0.0;
    }
    let dx = x + 0.5 - cx - 0.5;
    let dy = y * 2.0 + 1.0 - cy * 2.0 - 1.0;
    let dist_sq = dx * dx + dy * dy;
    let dist = dist_sq.sqrt();
    let core =
        (-dist_sq / (0.22 + (3.2 - 0.22) * rise).max(0.22)).exp() * (0.42 + (2.45 - 0.42) * rise);
    let shell_r = 0.16 + (2.05 - 0.16) * rise;
    let shell_w = (0.18 + (0.82 - 0.18) * rise).max(0.18);
    let shell = (-(((dist - shell_r) / shell_w).powi(2))).exp() * (0.1 + (0.95 - 0.1) * rise);
    let local_protection = (core + shell * 0.55).clamp(0.0, 1.0);
    let dim = OC_DIM * rise;
    (dim * (1.0 - local_protection * 0.72)).max(0.0)
}

fn opencode_glow_pulse(n: f32) -> OpenCodePulse {
    let n = n.max(0.0);
    // Map upstream `glow(base, theme, n)` onto our (peak, primary) decomposition:
    //   n in [0, 1]: tint base toward (base+primary) by sqrt(n) * 1.14, scaled by 0.84
    //   n in (1, ∞): additionally tint that result toward white by 1 - exp(-2.4 * (n-1))
    let primary = (n.sqrt() * 1.14).min(1.0) * 0.84;
    let peak = if n > 1.0 {
        (1.0 - (-(2.4 * (n - 1.0))).exp()).clamp(0.0, 1.0)
    } else {
        0.0
    };
    OpenCodePulse {
        peak,
        primary,
        dim: 0.0,
    }
}

fn opencode_overlay_light_pulse(target: &mut OpenCodePulse, pulse: OpenCodePulse) {
    let alpha = pulse.peak.max(pulse.primary).clamp(0.0, 1.0);
    if alpha <= 0.0 {
        return;
    }
    target.peak += (pulse.peak - target.peak) * alpha;
    target.primary += (pulse.primary - target.primary) * alpha;
}

fn opencode_bloom_pulse(x: f32, y: f32, bloom: &OpenCodeBloomEffect) -> OpenCodePulse {
    if bloom.age < 0.0 || bloom.age > OC_GLOW_OUT {
        return OpenCodePulse::default();
    }
    let p = (bloom.age / OC_GLOW_OUT).clamp(0.0, 1.0);
    let flash = (1.0 - p).powi(2);
    let dx = x + 0.5 - bloom.cx;
    let dy = y * 2.0 + 1.0 - bloom.cy_px;
    let dist = (dx * dx + dy * dy).sqrt();
    let bias = (-((dist / 2.8).powi(2))).exp();
    let force_now = bloom.force + (bloom.force * 0.18 - bloom.force) * p;
    let n = (force_now * (0.72 + (1.1 - 0.72) * bias) * flash).max(0.0);
    opencode_glow_pulse(n)
}

fn opencode_trace_pulse(idx: u16, len: u16, age_ms: f32, rise: f32) -> OpenCodePulse {
    if len < 2 {
        return OpenCodePulse::default();
    }
    let l = len as f32;
    let i = idx as f32;
    let appear = opencode_ramp(age_ms / 1000.0, 0.0, OC_TRACE_IN);
    let speed = OC_TRACE_SPEED * 0.48 + (OC_TRACE_SPEED * 0.88 - OC_TRACE_SPEED * 0.48) * rise;
    let head = (age_ms * speed) % l;
    let tail = ((head - OC_TRACE_TAIL) % l + l) % l;
    let raw_d = (i - head).abs();
    let dist = raw_d.min(l - raw_d);
    let raw_l = (i - tail).abs();
    let lag = raw_l.min(l - raw_l);
    let core = (-((dist / 1.05).powi(2))).exp() * (0.8 + (2.35 - 0.8) * rise);
    let trace_glow = (-((dist / 1.85).powi(2))).exp() * (0.08 + (0.34 - 0.08) * rise);
    let trail = (-((lag / 1.45).powi(2))).exp() * (0.04 + (0.42 - 0.04) * rise);
    let n = (core + trace_glow + trail) * appear;
    opencode_glow_pulse(n)
}

#[derive(Clone, Debug)]
struct OpenCodeGlyph {
    mask: Arc<CellMask>,
    cx: f32,
    cy_px: f32,
    trace: Arc<HashMap<(u16, u16), (u16, u16)>>,
}

fn opencode_glyphs(union: &CellMask) -> Vec<OpenCodeGlyph> {
    let w = union.w as i32;
    let h = union.h as i32;
    let mut visited = vec![false; (w * h) as usize];
    let neighbors: [(i32, i32); 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];

    let mut glyphs = Vec::new();
    for sy in 0..h {
        for sx in 0..w {
            let idx = (sy * w + sx) as usize;
            if visited[idx] {
                continue;
            }
            if !union.test_region_local(sx as u16, sy as u16) {
                visited[idx] = true;
                continue;
            }
            // Flood-fill 8-neighborhood to collect this connected component.
            let mut cells: Vec<(i32, i32)> = Vec::new();
            let mut stack = vec![(sx, sy)];
            visited[idx] = true;
            while let Some((cx, cy)) = stack.pop() {
                cells.push((cx, cy));
                for (dx, dy) in neighbors {
                    let nx = cx + dx;
                    let ny = cy + dy;
                    if nx < 0 || ny < 0 || nx >= w || ny >= h {
                        continue;
                    }
                    let nidx = (ny * w + nx) as usize;
                    if visited[nidx] {
                        continue;
                    }
                    if !union.test_region_local(nx as u16, ny as u16) {
                        visited[nidx] = true;
                        continue;
                    }
                    visited[nidx] = true;
                    stack.push((nx, ny));
                }
            }

            // Build glyph mask (full union dimensions, only this component's bits set).
            let total_words = (union.w as usize * union.h as usize).div_ceil(64);
            let mut bits = vec![0u64; total_words];
            let mut sum_x = 0f32;
            let mut sum_y = 0f32;
            for &(cx, cy) in &cells {
                let bidx = cy as usize * union.w as usize + cx as usize;
                bits[bidx / 64] |= 1u64 << (bidx % 64);
                sum_x += cx as f32;
                sum_y += cy as f32;
            }
            let n = cells.len() as f32;
            let cx_avg = sum_x / n + 0.5;
            let cy_px = (sum_y / n) * 2.0 + 1.0;

            let path = opencode_route(&cells);
            let mut trace = HashMap::with_capacity(path.len());
            let path_len = path.len() as u16;
            for (i, &(cx, cy)) in path.iter().enumerate() {
                trace.insert((cx as u16, cy as u16), (i as u16, path_len));
            }

            glyphs.push(OpenCodeGlyph {
                mask: Arc::new(CellMask {
                    origin: (0, 0),
                    w: union.w,
                    h: union.h,
                    bits: bits.into(),
                }),
                cx: cx_avg,
                cy_px,
                trace: Arc::new(trace),
            });
        }
    }
    glyphs
}

/// Direction-preserving Hamiltonian-ish traversal of a connected component (port of upstream `route`).
fn opencode_route(cells: &[(i32, i32)]) -> Vec<(i32, i32)> {
    if cells.is_empty() {
        return Vec::new();
    }
    let mut left: HashMap<(i32, i32), ()> = cells.iter().map(|&p| (p, ())).collect();
    let neighbors: [(i32, i32); 8] = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ];
    let start = cells
        .iter()
        .copied()
        .min_by(|a, b| a.1.cmp(&b.1).then(a.0.cmp(&b.0)))
        .unwrap();
    let mut path = Vec::with_capacity(cells.len());
    let mut cur = start;
    let mut dir = (1i32, 0i32);

    loop {
        path.push(cur);
        left.remove(&cur);
        if left.is_empty() {
            return path;
        }

        let mut candidates: Vec<((i32, i32), (i32, i32))> = neighbors
            .iter()
            .filter_map(|&(dx, dy)| {
                let n = (cur.0 + dx, cur.1 + dy);
                if left.contains_key(&n) {
                    Some((n, (dx, dy)))
                } else {
                    None
                }
            })
            .collect();

        candidates.sort_by(|(_, ad), (_, bd)| {
            let adot = ad.0 * dir.0 + ad.1 * dir.1;
            let bdot = bd.0 * dir.0 + bd.1 * dir.1;
            if adot != bdot {
                return bdot.cmp(&adot);
            }
            (ad.0.abs() + ad.1.abs()).cmp(&(bd.0.abs() + bd.1.abs()))
        });

        if let Some(&(next, _)) = candidates.first() {
            dir = (next.0 - cur.0, next.1 - cur.1);
            cur = next;
        } else {
            // Disconnected — jump to nearest leftover cell.
            let next = left
                .keys()
                .copied()
                .min_by(|a, b| {
                    let da = (a.0 - cur.0).pow(2) + (a.1 - cur.1).pow(2);
                    let db = (b.0 - cur.0).pow(2) + (b.1 - cur.1).pow(2);
                    da.cmp(&db)
                })
                .unwrap();
            dir = (1, 0);
            cur = next;
        }
    }
}

fn opencode_ease(t: f32) -> f32 {
    let p = t.clamp(0.0, 1.0);
    p * p * (3.0 - 2.0 * p)
}

fn opencode_push(t: f32) -> f32 {
    opencode_ease(t.clamp(0.0, 1.0).powi(2))
}

fn opencode_ramp(t: f32, start: f32, end: f32) -> f32 {
    if end <= start {
        return opencode_ease(if t >= end { 1.0 } else { 0.0 });
    }
    opencode_ease((t - start) / (end - start))
}

fn opencode_noise(x: f32, y: f32, t: f32) -> f32 {
    let n = (x * 12.9898 + y * 78.233 + t * 0.043).sin() * 43758.547;
    n - n.floor()
}

fn opencode_shade(
    base: TerminalColor,
    pulse: OpenCodePulse,
    theme: OpenCodeTheme,
    scale: f32,
) -> TerminalColor {
    let dim_mix = (pulse.dim * scale * 0.64).clamp(0.0, 0.82);
    let base = mix_terminal(base, theme.background, dim_mix);
    let primary_mix = (pulse.primary * scale).clamp(0.0, 1.0);
    let peak_mix = (pulse.peak * scale).clamp(0.0, 1.0);
    let primary = mix_terminal(base, theme.primary, primary_mix);
    let peak = mix_terminal(primary, theme.peak, peak_mix);
    if peak_mix > 0.0 || primary_mix > 0.0 {
        peak
    } else {
        mix_terminal(base, theme.background, 0.02)
    }
}

fn mix_terminal(a: TerminalColor, b: TerminalColor, t: f32) -> TerminalColor {
    let Some((ar, ag, ab)) = terminal_rgb(a) else {
        return a;
    };
    let Some((br, bg, bb)) = terminal_rgb(b) else {
        return a;
    };
    let t = t.clamp(0.0, 1.0);
    TerminalColor::Rgb(
        (ar as f32 + (br as f32 - ar as f32) * t).round() as u8,
        (ag as f32 + (bg as f32 - ag as f32) * t).round() as u8,
        (ab as f32 + (bb as f32 - ab as f32) * t).round() as u8,
    )
}

fn terminal_rgb(color: TerminalColor) -> Option<(u8, u8, u8)> {
    match color {
        TerminalColor::Rgb(r, g, b) => Some((r, g, b)),
        TerminalColor::Black => Some((0, 0, 0)),
        TerminalColor::White => Some((255, 255, 255)),
        TerminalColor::Gray => Some((128, 128, 128)),
        TerminalColor::DarkGray => Some((64, 64, 64)),
        TerminalColor::Red => Some((255, 0, 0)),
        TerminalColor::Green => Some((0, 255, 0)),
        TerminalColor::Blue => Some((0, 0, 255)),
        TerminalColor::Yellow => Some((255, 255, 0)),
        TerminalColor::Magenta => Some((255, 0, 255)),
        TerminalColor::Cyan => Some((0, 255, 255)),
        _ => None,
    }
}

struct BurstEffectsDemo {
    opencode_sequence: Arc<FrameSequence>,
    opencode_mask: Arc<CellMask>,
    opencode_glyphs: Arc<[OpenCodeGlyph]>,
}

struct LetterBurstLane {
    glyphs: Vec<GlyphLayout>,
    next_id: u64,
    cursor_local: Option<(u16, u16)>,
    charge: Option<(usize, f32)>,
    charge_cancel: Option<Arc<AtomicBool>>,
    ripples: Vec<(u64, f32, f32, f32)>,
    letter_ripples: Vec<(u64, usize, f32)>,
    anim_cancel: HashMap<u64, Arc<AtomicBool>>,
}

impl LetterBurstLane {
    fn glyph_at_cursor(&self) -> Option<usize> {
        let (lx, ly) = self.cursor_local?;
        self.glyphs
            .iter()
            .position(|g| g.mask.test_scope_local(lx as i16, ly as i16))
    }
}

struct State {
    tab: usize,
    cursor: (f32, f32),
    next_id: u64,
    ripple: RippleLane,
    sn: SupernovaLane,
    letter: LetterBurstLane,
    opencode: OpenCodeLane,
}

#[derive(Default)]
struct RippleLane {
    press_start: Option<Instant>,
    charge: Option<(u64, f32, f32, f32)>,
    charge_cancel: Option<Arc<AtomicBool>>,
    ripples: Vec<(u64, RippleState)>,
}

struct RippleState {
    cx: f32,
    cy: f32,
    radius: f32,
    strength: f32,
    max_radius: f32,
}

#[derive(Default)]
struct SupernovaLane {
    press_start: Option<Instant>,
    charge: Option<ChargeState>,
    charge_cancel: Option<Arc<AtomicBool>>,
    shockwaves: Vec<(u64, ShockwaveState)>,
    sparks: Vec<SparkState>,
    spark_elapsed: f32,
    flash_alpha: f32,
}

struct ChargeState {
    id: u64,
    cx: f32,
    cy: f32,
    t: f32,
    aura_phase: f32,
}

struct ShockwaveState {
    cx: f32,
    cy: f32,
    radius: f32,
    max_radius: f32,
    color_start: Color,
    color_end: Color,
    ring_width: f32,
    base_strength: f32,
}

struct SparkState {
    launch_cx: f32,
    launch_cy: f32,
    vx: f32,
    vy: f32,
    color: Color,
    size: f32,
    max_age: f32,
}

#[derive(Default)]
struct OpenCodeLane {
    cursor_local: Option<(u16, u16)>,
    press_start: Option<Instant>,
    charge: Option<OpenCodeChargeState>,
    charge_cancel: Option<Arc<AtomicBool>>,
    anim_cancel: Option<Arc<AtomicBool>>,
    bursts: Vec<(u64, OpenCodeBurstState)>,
    blooms: Vec<(u64, OpenCodeBloomState)>,
}

struct OpenCodeChargeState {
    id: u64,
    cx: f32,
    cy: f32,
    rise: f32,
    phase: f32,
    glyph_id: Option<usize>,
}

struct OpenCodeBurstState {
    cx: f32,
    cy: f32,
    elapsed: f32,
    level: f32,
    rise: f32,
}

struct OpenCodeBloomState {
    glyph_id: usize,
    elapsed: f32,
    force: f32,
}

#[derive(Clone, Debug)]
enum Msg {
    TabChanged(TabsEvent),
    CursorMoved(f32, f32),
    HoverChanged(bool),
    MouseDown,
    MouseUp,
    RChargeTick {
        charge_id: u64,
        size: f32,
    },
    RRippleTick {
        id: u64,
        radius: f32,
    },
    RRippleDone(u64),
    SnChargeTick {
        charge_id: u64,
        t: f32,
        aura_phase: f32,
    },
    SnShockwaveTick {
        id: u64,
        radius: f32,
    },
    SnShockwaveDone(u64),
    SnSparkTick(f32),
    SnFlashTick(f32),
    LetterMove(u16, u16),
    LetterHover(bool),
    LetterMouseDown,
    LetterMouseUp,
    LetterChargeTick {
        idx: usize,
    },
    LetterCenterRippleTick {
        id: u64,
        dr: f32,
    },
    LetterGlyphRippleTick {
        id: u64,
        dr: f32,
    },
    OpenCodeMove(u16, u16),
    OpenCodeHover(bool),
    OpenCodeMouseDown,
    OpenCodeMouseUp,
    OpenCodeAutoRelease {
        id: u64,
    },
    OpenCodeChargeTick {
        id: u64,
        rise: f32,
        phase: f32,
    },
    OpenCodeAnimTick(f32),
}

fn rand01(seed: u64) -> f32 {
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    ((z >> 40) as f32) / ((1u64 << 24) as f32)
}

fn fire_color(t: f32) -> Color {
    let bucket = (t * 6.0) as u32;
    match bucket {
        0 => Color::Rgb(255, 255, 220),
        1 => Color::Rgb(255, 240, 130),
        2 => Color::Rgb(255, 180, 40),
        3 => Color::Rgb(255, 110, 20),
        4 => Color::Rgb(220, 50, 20),
        _ => Color::Rgb(160, 210, 255),
    }
}

fn make_sparks(cx: f32, cy: f32, strength: f32, seed_base: u64) -> Vec<SparkState> {
    (0..SN_SPARK_COUNT)
        .map(|i| {
            let s = seed_base.wrapping_add(i as u64 * 7919);
            let jitter = (rand01(s) - 0.5) * 0.6;
            let angle = (i as f32 / SN_SPARK_COUNT as f32) * TAU + jitter;
            let speed = (9.0 + rand01(s.wrapping_add(1)) * 16.0) * (0.6 + strength * 0.4);
            let color = fire_color(rand01(s.wrapping_add(2)));
            let size = 0.7 + rand01(s.wrapping_add(3)) * 0.8;
            let max_age = 0.5 + rand01(s.wrapping_add(4)) * 1.0;
            SparkState {
                launch_cx: cx,
                launch_cy: cy,
                vx: angle.cos() * speed,
                vy: angle.sin() * speed * 2.0,
                color,
                size,
                max_age,
            }
        })
        .collect()
}

fn mask_from_sequence(sequence: &FrameSequence) -> Arc<CellMask> {
    let frame = sequence
        .get(0)
        .expect("opencode logo sequence must contain a frame");
    let width = frame.width();
    let height = frame.height();
    let mut bits = vec![0u64; (width as usize * height as usize).div_ceil(64)];

    for (idx, cell) in frame.buffer.cells().iter().enumerate() {
        if cell.ch != ' ' {
            bits[idx / 64] |= 1u64 << (idx % 64);
        }
    }

    Arc::new(CellMask {
        origin: (0, 0),
        w: width,
        h: height,
        bits: bits.into(),
    })
}

fn opencode_color_map(sequence: &FrameSequence) -> Vec<(Color, Color)> {
    let colors = sequence.collect_colors();
    let bright = Color::hex("#F2EDED");
    let mid = Color::hex("#B8B2B2");
    let shadow = Color::hex("#4B4646");

    let mut mapping = Vec::new();
    if let Some(&c) = colors.first() {
        mapping.push((c, bright));
    }
    if let Some(&c) = colors.get(1) {
        mapping.push((c, mid));
    }
    if let Some(&c) = colors.get(2) {
        mapping.push((c, shadow));
    }
    mapping
}

fn masked_vortex_effects() -> Vec<VisualEffect> {
    let effect: Arc<dyn CellEffect> = Arc::new(VortexEffect { intensity: 0.92 });
    BigText::layout_glyphs(LOGO_BIG_TEXT, LETTER_FONT)
        .into_iter()
        .map(|glyph| VisualEffect::Clipped {
            bounds: Some(glyph.rect),
            mask: Some(glyph.mask),
            inner: Box::new(VisualEffect::Custom(Arc::clone(&effect))),
        })
        .collect()
}

fn cancel_letter_lane(ctx: &mut Context<BurstEffectsDemo>) {
    if let Some(c) = ctx.state.letter.charge_cancel.take() {
        c.store(true, Ordering::Release);
    }
    for (_, c) in ctx.state.letter.anim_cancel.drain() {
        c.store(true, Ordering::Release);
    }
    ctx.state.letter.charge = None;
    ctx.state.letter.ripples.clear();
    ctx.state.letter.letter_ripples.clear();
}

fn cancel_opencode_lane(ctx: &mut Context<BurstEffectsDemo>) {
    if let Some(c) = ctx.state.opencode.charge_cancel.take() {
        c.store(true, Ordering::Release);
    }
    if let Some(c) = ctx.state.opencode.anim_cancel.take() {
        c.store(true, Ordering::Release);
    }
    ctx.state.opencode.press_start = None;
    ctx.state.opencode.charge = None;
    ctx.state.opencode.bursts.clear();
    ctx.state.opencode.blooms.clear();
}

fn start_opencode_anim_ticker(ctx: &mut Context<BurstEffectsDemo>) -> Option<Command> {
    if ctx.state.opencode.anim_cancel.is_some() {
        return None;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    ctx.state.opencode.anim_cancel = Some(Arc::clone(&cancel));
    Some(Command::spawn(move |link| {
        let mut last = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(16));
            if cancel.load(Ordering::Acquire) {
                break;
            }
            let now = Instant::now();
            let dt = now.duration_since(last).as_secs_f32();
            last = now;
            link.send(Msg::OpenCodeAnimTick(dt));
        }
    }))
}

fn cancel_all(ctx: &mut Context<BurstEffectsDemo>) {
    if let Some(c) = ctx.state.ripple.charge_cancel.take() {
        c.store(true, Ordering::Release);
    }
    if let Some(c) = ctx.state.sn.charge_cancel.take() {
        c.store(true, Ordering::Release);
    }
    cancel_letter_lane(ctx);
    cancel_opencode_lane(ctx);
    ctx.state.ripple.press_start = None;
    ctx.state.ripple.charge = None;
    ctx.state.ripple.ripples.clear();
    ctx.state.sn.press_start = None;
    ctx.state.sn.charge = None;
    ctx.state.sn.shockwaves.clear();
    ctx.state.sn.sparks.clear();
    ctx.state.sn.spark_elapsed = 0.0;
    ctx.state.sn.flash_alpha = 0.0;
}

fn collect_effects(ctx: &Context<BurstEffectsDemo>, effects: &mut Vec<VisualEffect>) {
    match ctx.state.tab {
        0 => {
            effects.extend(ctx.state.ripple.ripples.iter().map(|(_, r)| {
                let t = (r.radius / r.max_radius).clamp(0.0, 1.0);
                let tint = Color::Rgb(255, 230, 100).blend_toward(Color::Rgb(160, 210, 255), t);
                let fade = 1.0 - ((t - 0.6) / 0.4).clamp(0.0, 1.0);
                VisualEffect::Ripple {
                    origin: EffectOrigin::cell(r.cx, r.cy),
                    radius: RippleRadius::Fixed(r.radius),
                    ring_width: R_RING_WIDTH,
                    tint,
                    strength: r.strength * fade,
                }
            }));
            if let Some((_, cx, cy, size)) = ctx.state.ripple.charge
                && size > 0.0
            {
                effects.push(VisualEffect::Ripple {
                    origin: EffectOrigin::cell(cx, cy),
                    radius: RippleRadius::Fixed(0.0),
                    ring_width: size,
                    tint: Color::Rgb(255, 230, 150),
                    strength: 1.0,
                });
            }
        }
        1 => {
            if let Some(ref ch) = ctx.state.sn.charge {
                if ch.t > 0.0 {
                    effects.push(VisualEffect::dim(ch.t * 0.45));
                }
                for i in 0..SN_AURA_RINGS {
                    let phase_off = i as f32 * (TAU / SN_AURA_RINGS as f32);
                    let pulse = ((ch.aura_phase + phase_off).sin() * 0.5) + 0.5;
                    let base_r = 3.0 + i as f32 * 2.2;
                    let radius = base_r * (1.0 - ch.t * 0.35) + pulse * 0.6;
                    let t_color = i as f32 / (SN_AURA_RINGS - 1).max(1) as f32;
                    let tint =
                        Color::Rgb(255, 160, 20).blend_toward(Color::Rgb(90, 120, 255), t_color);
                    effects.push(VisualEffect::Ripple {
                        origin: EffectOrigin::cell(ch.cx, ch.cy),
                        radius: RippleRadius::Fixed(radius),
                        ring_width: 0.9 + pulse * 0.7,
                        tint,
                        strength: (0.25 + pulse * 0.4) * ch.t.powf(0.45),
                    });
                }
                let glow = ch.t * SN_CHARGE_MAX_GLOW;
                if glow > 0.01 {
                    let flicker = 0.92 + (ch.aura_phase * 2.3).sin() * 0.08;
                    effects.push(VisualEffect::Ripple {
                        origin: EffectOrigin::cell(ch.cx, ch.cy),
                        radius: RippleRadius::Fixed(0.0),
                        ring_width: glow,
                        tint: Color::Rgb(255, 200, 80),
                        strength: 0.75 * flicker,
                    });
                    effects.push(VisualEffect::Ripple {
                        origin: EffectOrigin::cell(ch.cx, ch.cy),
                        radius: RippleRadius::Fixed(0.0),
                        ring_width: glow * 0.5,
                        tint: Color::Rgb(255, 255, 240),
                        strength: flicker,
                    });
                }
            }
            for (_, sw) in &ctx.state.sn.shockwaves {
                let t = (sw.radius / sw.max_radius).clamp(0.0, 1.0);
                let tint = sw.color_start.blend_toward(sw.color_end, t);
                let fade = 1.0 - ((t - 0.6) / 0.4).clamp(0.0, 1.0);
                effects.push(VisualEffect::Ripple {
                    origin: EffectOrigin::cell(sw.cx, sw.cy),
                    radius: RippleRadius::Fixed(sw.radius),
                    ring_width: sw.ring_width,
                    tint,
                    strength: sw.base_strength * fade,
                });
            }
            let elapsed = ctx.state.sn.spark_elapsed;
            for spark in &ctx.state.sn.sparks {
                if elapsed >= spark.max_age {
                    continue;
                }
                let t = elapsed / spark.max_age;
                let cx = spark.launch_cx + spark.vx * elapsed;
                let cy = spark.launch_cy + spark.vy * elapsed;
                let fade = (1.0 - t).powf(0.8);
                effects.push(VisualEffect::Ripple {
                    origin: EffectOrigin::cell(cx, cy),
                    radius: RippleRadius::Fixed(0.0),
                    ring_width: spark.size * (1.0 + t * 0.4),
                    tint: spark.color,
                    strength: fade * 0.95,
                });
            }
            if ctx.state.sn.flash_alpha > 0.0 {
                effects.push(VisualEffect::tint(
                    Color::Rgb(255, 240, 200),
                    ctx.state.sn.flash_alpha * 0.75,
                ));
            }
        }
        2 => {
            if let Some((idx, size)) = ctx.state.letter.charge
                && let Some(g) = ctx.state.letter.glyphs.get(idx)
            {
                let cx = g.rect.x as f32 + g.rect.w as f32 * 0.5;
                let cy = g.rect.y as f32 + g.rect.h as f32 * 0.5;
                let inner = VisualEffect::Ripple {
                    origin: EffectOrigin::cell(cx, cy),
                    radius: RippleRadius::Fixed(0.0),
                    ring_width: 1.5 + size * 3.0,
                    tint: Color::Rgb(255, 220, 120),
                    strength: 0.85,
                };
                effects.push(VisualEffect::Clipped {
                    bounds: Some(g.rect),
                    mask: Some(Arc::clone(&g.mask)),
                    inner: Box::new(inner),
                });
            }
            for &(_id, r, cx, cy) in &ctx.state.letter.ripples {
                let t = (r / LETTER_R_MAX).clamp(0.0, 1.0);
                let fade = 1.0 - t;
                effects.push(VisualEffect::Ripple {
                    origin: EffectOrigin::cell(cx, cy),
                    radius: RippleRadius::Fixed(r),
                    ring_width: LETTER_RING,
                    tint: Color::Rgb(100, 200, 255),
                    strength: 0.55 * fade,
                });
            }
            for &(_id, j, r) in &ctx.state.letter.letter_ripples {
                if let Some(gj) = ctx.state.letter.glyphs.get(j) {
                    let t = (r / LETTER_R_MAX).clamp(0.0, 1.0);
                    let fade = 1.0 - t;
                    let cx = gj.rect.x as f32 + gj.rect.w as f32 * 0.5;
                    let cy = gj.rect.y as f32 + gj.rect.h as f32 * 0.5;
                    let inner = VisualEffect::Ripple {
                        origin: EffectOrigin::cell(cx, cy),
                        radius: RippleRadius::Fixed(r),
                        ring_width: LETTER_RING * 0.85,
                        tint: Color::Rgb(255, 180, 90),
                        strength: 0.5 * fade,
                    };
                    effects.push(VisualEffect::Clipped {
                        bounds: Some(gj.rect),
                        mask: Some(Arc::clone(&gj.mask)),
                        inner: Box::new(inner),
                    });
                }
            }
        }
        _ => {}
    }
}

impl Component for BurstEffectsDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            tab: 0,
            cursor: (0.0, 0.0),
            next_id: 0,
            ripple: RippleLane::default(),
            sn: SupernovaLane::default(),
            letter: LetterBurstLane {
                glyphs: BigText::layout_glyphs(LOGO_BIG_TEXT, LETTER_FONT),
                next_id: 1,
                cursor_local: None,
                charge: None,
                charge_cancel: None,
                ripples: Vec::new(),
                letter_ripples: Vec::new(),
                anim_cancel: HashMap::new(),
            },
            opencode: OpenCodeLane::default(),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::TabChanged(ev) => {
                if ev.index != ctx.state.tab {
                    cancel_all(ctx);
                    ctx.state.tab = ev.index;
                    return Update::full();
                }
                Update::none()
            }

            Msg::CursorMoved(x, y) => {
                ctx.state.cursor = (x, y);
                Update::none()
            }

            Msg::HoverChanged(hovered) => {
                if !hovered {
                    let press = match ctx.state.tab {
                        0 => ctx.state.ripple.press_start.is_some(),
                        1 => ctx.state.sn.press_start.is_some(),
                        _ => false,
                    };
                    if press {
                        ctx.link().send(Msg::MouseUp);
                    }
                }
                Update::none()
            }

            Msg::MouseDown => match ctx.state.tab {
                0 => {
                    if let Some(c) = ctx.state.ripple.charge_cancel.take() {
                        c.store(true, Ordering::Release);
                    }
                    let cancel = Arc::new(AtomicBool::new(false));
                    ctx.state.ripple.charge_cancel = Some(Arc::clone(&cancel));
                    ctx.state.ripple.press_start = Some(Instant::now());
                    let (cx, cy) = ctx.state.cursor;
                    let charge_id = ctx.state.next_id;
                    ctx.state.next_id += 1;
                    ctx.state.ripple.charge = Some((charge_id, cx, cy, 0.0));

                    Update::with_command(Command::spawn(move |link| {
                        let start = Instant::now();
                        loop {
                            if cancel.load(Ordering::Acquire) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(16));
                            let secs = start.elapsed().as_secs_f32();
                            let size = (secs / R_CHARGE_FILL_SECS * R_MAX_CHARGE_SIZE)
                                .min(R_MAX_CHARGE_SIZE);
                            link.send(Msg::RChargeTick { charge_id, size });
                            if secs > 10.0 {
                                break;
                            }
                        }
                    }))
                }
                1 => {
                    if let Some(c) = ctx.state.sn.charge_cancel.take() {
                        c.store(true, Ordering::Release);
                    }
                    let cancel = Arc::new(AtomicBool::new(false));
                    ctx.state.sn.charge_cancel = Some(Arc::clone(&cancel));
                    ctx.state.sn.press_start = Some(Instant::now());
                    let (cx, cy) = ctx.state.cursor;
                    let charge_id = ctx.state.next_id;
                    ctx.state.next_id += 1;
                    ctx.state.sn.charge = Some(ChargeState {
                        id: charge_id,
                        cx,
                        cy,
                        t: 0.0,
                        aura_phase: 0.0,
                    });

                    Update::with_command(Command::spawn(move |link| {
                        let start = Instant::now();
                        loop {
                            if cancel.load(Ordering::Acquire) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(16));
                            let secs = start.elapsed().as_secs_f32();
                            let t = (secs / SN_CHARGE_FILL_SECS).min(1.0);
                            let aura_phase = secs * 5.5;
                            link.send(Msg::SnChargeTick {
                                charge_id,
                                t,
                                aura_phase,
                            });
                            if secs > 10.0 {
                                break;
                            }
                        }
                    }))
                }
                _ => Update::none(),
            },

            Msg::RChargeTick { charge_id, size } => {
                if ctx.state.tab != 0 {
                    return Update::none();
                }
                if let Some((id, _, _, ref mut s)) = ctx.state.ripple.charge
                    && id == charge_id
                {
                    *s = size;
                    return Update::full();
                }
                Update::none()
            }

            Msg::SnChargeTick {
                charge_id,
                t,
                aura_phase,
            } => {
                if ctx.state.tab != 1 {
                    return Update::none();
                }
                if let Some(ref mut ch) = ctx.state.sn.charge
                    && ch.id == charge_id
                {
                    ch.t = t;
                    ch.aura_phase = aura_phase;
                    return Update::full();
                }
                Update::none()
            }

            Msg::MouseUp => match ctx.state.tab {
                0 => {
                    if let Some(c) = ctx.state.ripple.charge_cancel.take() {
                        c.store(true, Ordering::Release);
                    }
                    let held_ms = ctx
                        .state
                        .ripple
                        .press_start
                        .take()
                        .map(|t| t.elapsed().as_millis())
                        .unwrap_or(50);

                    let launch_pos = if let Some((_, cx, cy, _)) = ctx.state.ripple.charge.take() {
                        (cx, cy)
                    } else {
                        ctx.state.cursor
                    };

                    let strength = (held_ms as f32 / 300.0).clamp(0.3, 1.0);
                    let max_radius = R_MIN_RADIUS + strength * (R_MAX_RADIUS - R_MIN_RADIUS);
                    let duration = Duration::from_secs_f32(max_radius / R_CELLS_PER_SEC);
                    let (cx, cy) = launch_pos;
                    let id = ctx.state.next_id;
                    ctx.state.next_id += 1;
                    ctx.state.ripple.ripples.push((
                        id,
                        RippleState {
                            cx,
                            cy,
                            radius: 0.0,
                            strength,
                            max_radius,
                        },
                    ));

                    Update::with_command(Command::spawn(move |link| {
                        let start = Instant::now();
                        loop {
                            std::thread::sleep(Duration::from_millis(16));
                            let elapsed = start.elapsed();
                            link.send(Msg::RRippleTick {
                                id,
                                radius: elapsed.as_secs_f32() * R_CELLS_PER_SEC,
                            });
                            if elapsed >= duration {
                                link.send(Msg::RRippleDone(id));
                                break;
                            }
                        }
                    }))
                }
                1 => {
                    if let Some(c) = ctx.state.sn.charge_cancel.take() {
                        c.store(true, Ordering::Release);
                    }
                    let held_secs = ctx
                        .state
                        .sn
                        .press_start
                        .take()
                        .map(|t| t.elapsed().as_secs_f32())
                        .unwrap_or(0.05);

                    let (cx, cy) = if let Some(ch) = ctx.state.sn.charge.take() {
                        (ch.cx, ch.cy)
                    } else {
                        ctx.state.cursor
                    };

                    let strength = (held_secs / SN_CHARGE_FILL_SECS).clamp(0.25, 1.0);
                    let max_r =
                        SN_MIN_MAX_RADIUS + strength * (SN_MAX_MAX_RADIUS - SN_MIN_MAX_RADIUS);

                    let core_id = ctx.state.next_id;
                    ctx.state.next_id += 1;
                    let fire_id = ctx.state.next_id;
                    ctx.state.next_id += 1;
                    let outer_id = ctx.state.next_id;
                    ctx.state.next_id += 1;
                    let seed_base = ctx.state.next_id;
                    ctx.state.next_id += 1;

                    ctx.state.sn.shockwaves.push((
                        core_id,
                        ShockwaveState {
                            cx,
                            cy,
                            radius: 0.0,
                            max_radius: max_r * 0.75,
                            color_start: Color::Rgb(255, 255, 230),
                            color_end: Color::Rgb(255, 200, 120),
                            ring_width: 2.0,
                            base_strength: 1.0,
                        },
                    ));
                    ctx.state.sn.shockwaves.push((
                        fire_id,
                        ShockwaveState {
                            cx,
                            cy,
                            radius: 0.0,
                            max_radius: max_r,
                            color_start: Color::Rgb(255, 160, 40),
                            color_end: Color::Rgb(220, 50, 20),
                            ring_width: 4.0,
                            base_strength: 0.95,
                        },
                    ));
                    ctx.state.sn.shockwaves.push((
                        outer_id,
                        ShockwaveState {
                            cx,
                            cy,
                            radius: 0.0,
                            max_radius: max_r * 1.25,
                            color_start: Color::Rgb(180, 100, 220),
                            color_end: Color::Rgb(80, 140, 240),
                            ring_width: 5.0,
                            base_strength: 0.8,
                        },
                    ));

                    ctx.state.sn.sparks = make_sparks(cx, cy, strength, seed_base);
                    ctx.state.sn.spark_elapsed = 0.0;
                    ctx.state.sn.flash_alpha = 0.85 * (0.4 + strength * 0.6);

                    let core_max = max_r * 0.75;
                    let fire_max = max_r;
                    let outer_max = max_r * 1.25;
                    let max_spark_age = ctx
                        .state
                        .sn
                        .sparks
                        .iter()
                        .map(|s| s.max_age)
                        .fold(0.0_f32, f32::max);

                    Update::with_command(Command::spawn(move |link| {
                        let l = link.clone();
                        std::thread::spawn(move || {
                            let start = Instant::now();
                            let dur = Duration::from_secs_f32(core_max / SN_SHOCKWAVE_SPEED_CORE);
                            loop {
                                std::thread::sleep(Duration::from_millis(16));
                                let e = start.elapsed();
                                l.send(Msg::SnShockwaveTick {
                                    id: core_id,
                                    radius: e.as_secs_f32() * SN_SHOCKWAVE_SPEED_CORE,
                                });
                                if e >= dur {
                                    l.send(Msg::SnShockwaveDone(core_id));
                                    break;
                                }
                            }
                        });

                        let l = link.clone();
                        std::thread::spawn(move || {
                            let start = Instant::now();
                            let dur = Duration::from_secs_f32(fire_max / SN_SHOCKWAVE_SPEED_FIRE);
                            loop {
                                std::thread::sleep(Duration::from_millis(16));
                                let e = start.elapsed();
                                l.send(Msg::SnShockwaveTick {
                                    id: fire_id,
                                    radius: e.as_secs_f32() * SN_SHOCKWAVE_SPEED_FIRE,
                                });
                                if e >= dur {
                                    l.send(Msg::SnShockwaveDone(fire_id));
                                    break;
                                }
                            }
                        });

                        let l = link.clone();
                        std::thread::spawn(move || {
                            let start = Instant::now();
                            let dur = Duration::from_secs_f32(outer_max / SN_SHOCKWAVE_SPEED_OUTER);
                            loop {
                                std::thread::sleep(Duration::from_millis(16));
                                let e = start.elapsed();
                                l.send(Msg::SnShockwaveTick {
                                    id: outer_id,
                                    radius: e.as_secs_f32() * SN_SHOCKWAVE_SPEED_OUTER,
                                });
                                if e >= dur {
                                    l.send(Msg::SnShockwaveDone(outer_id));
                                    break;
                                }
                            }
                        });

                        let l = link.clone();
                        std::thread::spawn(move || {
                            let start = Instant::now();
                            loop {
                                std::thread::sleep(Duration::from_millis(16));
                                let elapsed = start.elapsed().as_secs_f32();
                                l.send(Msg::SnSparkTick(elapsed));
                                if elapsed > max_spark_age {
                                    break;
                                }
                            }
                        });

                        std::thread::spawn(move || {
                            let start = Instant::now();
                            loop {
                                std::thread::sleep(Duration::from_millis(16));
                                let e = start.elapsed().as_secs_f32();
                                let a = (1.0 - e / SN_FLASH_DURATION).max(0.0);
                                link.send(Msg::SnFlashTick(a * a));
                                if a <= 0.0 {
                                    break;
                                }
                            }
                        });
                    }))
                }
                _ => Update::none(),
            },

            Msg::RRippleTick { id, radius } => {
                if let Some((_, r)) = ctx
                    .state
                    .ripple
                    .ripples
                    .iter_mut()
                    .find(|(rid, _)| *rid == id)
                {
                    r.radius = radius;
                }
                Update::full()
            }

            Msg::RRippleDone(id) => {
                ctx.state.ripple.ripples.retain(|(rid, _)| *rid != id);
                Update::full()
            }

            Msg::SnShockwaveTick { id, radius } => {
                if let Some((_, sw)) = ctx
                    .state
                    .sn
                    .shockwaves
                    .iter_mut()
                    .find(|(sid, _)| *sid == id)
                {
                    sw.radius = radius;
                }
                Update::full()
            }

            Msg::SnShockwaveDone(id) => {
                ctx.state.sn.shockwaves.retain(|(sid, _)| *sid != id);
                Update::full()
            }

            Msg::SnSparkTick(elapsed) => {
                ctx.state.sn.spark_elapsed = elapsed;
                let all_dead = ctx.state.sn.sparks.iter().all(|s| elapsed >= s.max_age);
                if all_dead {
                    ctx.state.sn.sparks.clear();
                }
                Update::full()
            }

            Msg::SnFlashTick(alpha) => {
                ctx.state.sn.flash_alpha = alpha;
                Update::full()
            }

            Msg::LetterMove(x, y) => {
                if ctx.state.tab != 2 {
                    return Update::none();
                }
                ctx.state.letter.cursor_local = Some((x, y));
                Update::none()
            }

            Msg::LetterHover(inside) => {
                if ctx.state.tab != 2 {
                    return Update::none();
                }
                if !inside {
                    if let Some(c) = ctx.state.letter.charge_cancel.take() {
                        c.store(true, Ordering::Release);
                    }
                    ctx.state.letter.charge = None;
                    return Update::full();
                }
                Update::none()
            }

            Msg::LetterMouseDown => {
                if ctx.state.tab != 2 {
                    return Update::none();
                }
                let Some(i) = ctx.state.letter.glyph_at_cursor() else {
                    return Update::none();
                };
                if let Some(c) = ctx.state.letter.charge_cancel.take() {
                    c.store(true, Ordering::Release);
                }
                let cancel = Arc::new(AtomicBool::new(false));
                ctx.state.letter.charge_cancel = Some(Arc::clone(&cancel));
                ctx.state.letter.charge = Some((i, 0.0));
                let idx = i;
                Update::with_command(Command::spawn(move |link| {
                    loop {
                        std::thread::sleep(Duration::from_millis(16));
                        if cancel.load(Ordering::Acquire) {
                            break;
                        }
                        link.send(Msg::LetterChargeTick { idx });
                    }
                }))
            }

            Msg::LetterChargeTick { idx } => {
                if ctx.state.tab != 2 {
                    return Update::none();
                }
                if let Some((ci, ref mut tr)) = ctx.state.letter.charge
                    && ci == idx
                {
                    *tr = (*tr + 0.06).min(2.5);
                }
                Update::full()
            }

            Msg::LetterMouseUp => {
                if ctx.state.tab != 2 {
                    return Update::none();
                }
                if let Some(c) = ctx.state.letter.charge_cancel.take() {
                    c.store(true, Ordering::Release);
                }
                let Some(i) = ctx.state.letter.glyph_at_cursor() else {
                    ctx.state.letter.charge = None;
                    return Update::full();
                };
                ctx.state.letter.charge = None;
                let g = &ctx.state.letter.glyphs[i];
                let cx = g.rect.x as f32 + g.rect.w as f32 * 0.5;
                let cy = g.rect.y as f32 + g.rect.h as f32 * 0.5;
                let id = ctx.state.letter.next_id;
                ctx.state.letter.next_id += 1;
                ctx.state.letter.ripples.push((id, 0.0, cx, cy));
                let cancel_r = Arc::new(AtomicBool::new(false));
                ctx.state
                    .letter
                    .anim_cancel
                    .insert(id, Arc::clone(&cancel_r));

                let mut letter_meta: Vec<(u64, f32, Arc<AtomicBool>)> = Vec::new();
                for (j, _) in ctx.state.letter.glyphs.iter().enumerate() {
                    let d = (i as i32 - j as i32).unsigned_abs() as f32;
                    let delay = d * LETTER_STAGGER_SECS;
                    let lid = ctx.state.letter.next_id;
                    ctx.state.letter.next_id += 1;
                    ctx.state.letter.letter_ripples.push((lid, j, 0.0));
                    let cancel_l = Arc::new(AtomicBool::new(false));
                    ctx.state
                        .letter
                        .anim_cancel
                        .insert(lid, Arc::clone(&cancel_l));
                    letter_meta.push((lid, delay, cancel_l));
                }

                Update::with_command(Command::spawn(move |link| {
                    let c2 = Arc::clone(&cancel_r);
                    let l = link.clone();
                    std::thread::spawn(move || {
                        loop {
                            std::thread::sleep(Duration::from_millis(16));
                            if c2.load(Ordering::Acquire) {
                                break;
                            }
                            l.send(Msg::LetterCenterRippleTick {
                                id,
                                dr: LETTER_R_SPEED * 0.016,
                            });
                        }
                    });

                    for (lid, delay, cancel_l) in letter_meta {
                        let l = link.clone();
                        let c3 = Arc::clone(&cancel_l);
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_secs_f32(delay));
                            loop {
                                std::thread::sleep(Duration::from_millis(16));
                                if c3.load(Ordering::Acquire) {
                                    break;
                                }
                                l.send(Msg::LetterGlyphRippleTick {
                                    id: lid,
                                    dr: LETTER_R_SPEED * 0.016,
                                });
                            }
                        });
                    }
                }))
            }

            Msg::LetterCenterRippleTick { id, dr } => {
                if ctx.state.tab != 2 {
                    return Update::none();
                }
                let mut remove = false;
                if let Some((_, r, _, _)) = ctx
                    .state
                    .letter
                    .ripples
                    .iter_mut()
                    .find(|(rid, _, _, _)| *rid == id)
                {
                    *r += dr;
                    if *r > LETTER_R_MAX {
                        remove = true;
                    }
                }
                if remove {
                    if let Some(c) = ctx.state.letter.anim_cancel.remove(&id) {
                        c.store(true, Ordering::Release);
                    }
                    ctx.state.letter.ripples.retain(|(rid, _, _, _)| *rid != id);
                }
                Update::full()
            }

            Msg::LetterGlyphRippleTick { id, dr } => {
                if ctx.state.tab != 2 {
                    return Update::none();
                }
                let mut remove = false;
                if let Some((_, _, r)) = ctx
                    .state
                    .letter
                    .letter_ripples
                    .iter_mut()
                    .find(|(rid, _, _)| *rid == id)
                {
                    *r += dr;
                    if *r > LETTER_R_MAX {
                        remove = true;
                    }
                }
                if remove {
                    if let Some(c) = ctx.state.letter.anim_cancel.remove(&id) {
                        c.store(true, Ordering::Release);
                    }
                    ctx.state
                        .letter
                        .letter_ripples
                        .retain(|(rid, _, _)| *rid != id);
                }
                Update::full()
            }

            Msg::OpenCodeMove(x, y) => {
                if ctx.state.tab != 4 {
                    return Update::none();
                }
                ctx.state.opencode.cursor_local = Some((x, y));
                ctx.state.cursor = (x as f32, y as f32);
                Update::none()
            }

            Msg::OpenCodeHover(inside) => {
                if ctx.state.tab != 4 {
                    return Update::none();
                }
                if !inside && ctx.state.opencode.press_start.is_some() {
                    ctx.link().send(Msg::OpenCodeMouseUp);
                }
                Update::none()
            }

            Msg::OpenCodeMouseDown => {
                if ctx.state.tab != 4 {
                    return Update::none();
                }
                if let Some(c) = ctx.state.opencode.charge_cancel.take() {
                    c.store(true, Ordering::Release);
                }
                let (cx, cy) = ctx
                    .state
                    .opencode
                    .cursor_local
                    .map(|(x, y)| (x as f32, y as f32))
                    .unwrap_or((
                        self.opencode_mask.w as f32 * 0.5,
                        self.opencode_mask.h as f32 * 0.5,
                    ));
                let id = ctx.state.next_id;
                ctx.state.next_id += 1;
                let cancel = Arc::new(AtomicBool::new(false));
                ctx.state.opencode.charge_cancel = Some(Arc::clone(&cancel));
                ctx.state.opencode.press_start = Some(Instant::now());
                let glyph_id = self.opencode_glyphs.iter().position(|g| {
                    g.mask
                        .test_scope_local(cx.round() as i16, cy.round() as i16)
                });
                ctx.state.opencode.charge = Some(OpenCodeChargeState {
                    id,
                    cx,
                    cy,
                    rise: 0.0,
                    phase: 0.0,
                    glyph_id,
                });

                Update::with_command(Command::spawn(move |link| {
                    let start = Instant::now();
                    loop {
                        std::thread::sleep(Duration::from_millis(16));
                        if cancel.load(Ordering::Acquire) {
                            break;
                        }
                        let secs = start.elapsed().as_secs_f32();
                        link.send(Msg::OpenCodeChargeTick {
                            id,
                            rise: secs,
                            phase: secs * 1000.0,
                        });
                        if secs >= OC_CHARGE_SECS {
                            link.send(Msg::OpenCodeAutoRelease { id });
                            break;
                        }
                    }
                }))
            }

            Msg::OpenCodeAutoRelease { id } => {
                if ctx.state.tab != 4 {
                    return Update::none();
                }
                if ctx
                    .state
                    .opencode
                    .charge
                    .as_ref()
                    .is_some_and(|c| c.id == id)
                {
                    ctx.link().send(Msg::OpenCodeMouseUp);
                }
                Update::none()
            }

            Msg::OpenCodeChargeTick { id, rise, phase } => {
                if ctx.state.tab != 4 {
                    return Update::none();
                }
                if let Some(ref mut charge) = ctx.state.opencode.charge
                    && charge.id == id
                {
                    charge.rise = rise;
                    charge.phase = phase;
                    return Update::full();
                }
                Update::none()
            }

            Msg::OpenCodeMouseUp => {
                if ctx.state.tab != 4 {
                    return Update::none();
                }
                if let Some(c) = ctx.state.opencode.charge_cancel.take() {
                    c.store(true, Ordering::Release);
                }
                let held = ctx
                    .state
                    .opencode
                    .press_start
                    .take()
                    .map(|start| start.elapsed().as_secs_f32())
                    .unwrap_or(0.05);
                let Some(charge) = ctx.state.opencode.charge.take() else {
                    return Update::full();
                };
                let rise = opencode_ramp(held, OC_HOLD_SECS, OC_CHARGE_SECS);
                let level = opencode_push(rise);
                let burst_id = ctx.state.next_id;
                ctx.state.next_id += 1;
                ctx.state.opencode.bursts.push((
                    burst_id,
                    OpenCodeBurstState {
                        cx: charge.cx,
                        cy: charge.cy,
                        elapsed: 0.0,
                        level,
                        rise,
                    },
                ));

                if let Some(gid) = charge.glyph_id {
                    let force = 0.18 + (1.5 - 0.18) * (rise * level);
                    let bloom_id = ctx.state.next_id;
                    ctx.state.next_id += 1;
                    ctx.state.opencode.blooms.push((
                        bloom_id,
                        OpenCodeBloomState {
                            glyph_id: gid,
                            elapsed: 0.0,
                            force,
                        },
                    ));
                }

                Update::with_command(start_opencode_anim_ticker(ctx))
            }

            Msg::OpenCodeAnimTick(dt) => {
                if ctx.state.tab != 4 {
                    return Update::none();
                }
                for (_, burst) in &mut ctx.state.opencode.bursts {
                    burst.elapsed += dt;
                }
                for (_, bloom) in &mut ctx.state.opencode.blooms {
                    bloom.elapsed += dt;
                }
                ctx.state
                    .opencode
                    .bursts
                    .retain(|(_, burst)| burst.elapsed < OC_LIFE_SECS);
                ctx.state
                    .opencode
                    .blooms
                    .retain(|(_, bloom)| bloom.elapsed < OC_GLOW_OUT);
                if ctx.state.opencode.bursts.is_empty()
                    && ctx.state.opencode.blooms.is_empty()
                    && let Some(c) = ctx.state.opencode.anim_cancel.take()
                {
                    c.store(true, Ordering::Release);
                }
                Update::full()
            }
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut effects: Vec<VisualEffect> = Vec::new();
        collect_effects(ctx, &mut effects);

        let (cursor_x, cursor_y) = ctx.state.cursor;
        let charging = match ctx.state.tab {
            0 => ctx.state.ripple.press_start.is_some(),
            1 => ctx.state.sn.press_start.is_some(),
            2 => ctx.state.letter.charge.is_some(),
            4 => ctx.state.opencode.charge.is_some(),
            _ => false,
        };

        let title_suffix = match ctx.state.tab {
            0 => "Ripple",
            1 => "Supernova",
            2 => "Letter burst",
            3 => "Custom vortex",
            4 => "OpenCode",
            _ => "",
        };
        let hint = match ctx.state.tab {
            0 => "Hold on the logo - release to launch the ripple.",
            1 => "Hold on the logo - release to detonate (shockwave + sparks).",
            2 => "Hold on a letter - release for a ripple; staggered bursts on every glyph.",
            3 => "Custom CellEffect wrapped in per-glyph Clipped masks.",
            4 => "Hold on the OpenCode logo to charge; release for the upstream-style burst.",
            _ => "",
        };

        let tabs = Tabs::new()
            .tabs(vec![
                Tab::new("Ripple"),
                Tab::new("Supernova"),
                Tab::new("Letter burst"),
                Tab::new("Custom vortex"),
                Tab::new("OpenCode"),
            ])
            .active(ctx.state.tab.min(4))
            .on_change(ctx.link().callback(Msg::TabChanged));

        let charge_label = if charging {
            " [charging…]"
        } else {
            " | hold to charge"
        };
        let (disp_x, disp_y) = match ctx.state.tab {
            2 => ctx
                .state
                .letter
                .cursor_local
                .map(|(x, y)| (x as f32, y as f32))
                .unwrap_or((cursor_x, cursor_y)),
            4 => ctx
                .state
                .opencode
                .cursor_local
                .map(|(x, y)| (x as f32, y as f32))
                .unwrap_or((cursor_x, cursor_y)),
            _ => (cursor_x, cursor_y),
        };
        let frame_title =
            format!("{title_suffix} - cursor ({disp_x:.0}, {disp_y:.0}){charge_label} | q: quit",);

        let stage: Element = if ctx.state.tab == 2 {
            let glyphs_hit = ctx.state.letter.glyphs.clone();
            MouseRegion::new()
                .capture_click(true)
                .hit_test(move |x, y| {
                    glyphs_hit
                        .iter()
                        .any(|g| g.mask.test_scope_local(x as i16, y as i16))
                })
                .on_mouse_move(
                    ctx.link()
                        .callback(|e: MouseMoveEvent| Msg::LetterMove(e.local_x, e.local_y)),
                )
                .on_hover_change(ctx.link().callback(Msg::LetterHover))
                .on_mouse_down(ctx.link().callback(|_: MouseEvent| Msg::LetterMouseDown))
                .on_click(ctx.link().callback(|_: MouseEvent| Msg::LetterMouseUp))
                .child(
                    EffectScope::new().effects(effects).child(
                        BigText::new()
                            .text(LOGO_BIG_TEXT)
                            .font(BigFont::AnsiShadow)
                            .gradient(
                                ColorGradient::new(
                                    Color::Rgb(80, 50, 180),
                                    Color::Rgb(20, 140, 180),
                                )
                                .with_center(Color::Rgb(60, 80, 200)),
                                GradientDirection::Horizontal,
                            ),
                    ),
                )
                .into()
        } else if ctx.state.tab == 3 {
            EffectScope::new()
                .effects(masked_vortex_effects())
                .child(
                    BigText::new()
                        .text(LOGO_BIG_TEXT)
                        .font(BigFont::AnsiShadow)
                        .gradient(
                            ColorGradient::new(Color::Rgb(120, 85, 255), Color::Rgb(80, 240, 255))
                                .with_center(Color::Rgb(255, 255, 220)),
                            GradientDirection::Horizontal,
                        ),
                )
                .into()
        } else if ctx.state.tab == 4 {
            let charge_state = ctx.state.opencode.charge.as_ref();
            let charge = charge_state.map(|charge| OpenCodeChargeEffect {
                cx: charge.cx,
                cy: charge.cy,
                rise: charge.rise,
                phase: charge.phase,
            });
            let held = charge_state.and_then(|charge| {
                let gid = charge.glyph_id?;
                let glyph = self.opencode_glyphs.get(gid)?;
                let rise_norm = opencode_ramp(charge.rise, OC_HOLD_SECS, OC_CHARGE_SECS);
                Some(OpenCodeHeldGlyphEffect {
                    mask: Arc::clone(&glyph.mask),
                    age_ms: charge.phase,
                    rise: rise_norm,
                    trace: Arc::clone(&glyph.trace),
                })
            });
            let bursts: Arc<[OpenCodeBurstEffect]> = ctx
                .state
                .opencode
                .bursts
                .iter()
                .map(|(_, burst)| OpenCodeBurstEffect {
                    cx: burst.cx,
                    cy: burst.cy,
                    elapsed: burst.elapsed,
                    level: burst.level,
                    rise: burst.rise,
                })
                .collect::<Vec<_>>()
                .into();
            let blooms: Arc<[OpenCodeBloomEffect]> = ctx
                .state
                .opencode
                .blooms
                .iter()
                .filter_map(|(_, bloom)| {
                    let glyph = self.opencode_glyphs.get(bloom.glyph_id)?;
                    Some(OpenCodeBloomEffect {
                        age: bloom.elapsed,
                        force: bloom.force,
                        mask: Arc::clone(&glyph.mask),
                        cx: glyph.cx,
                        cy_px: glyph.cy_px,
                    })
                })
                .collect::<Vec<_>>()
                .into();
            let logo_effect: Arc<dyn CellEffect> = Arc::new(OpenCodeLogoEffect {
                theme: OpenCodeTheme::upstream(),
                charge,
                held,
                bursts,
                blooms,
            });
            let effect = VisualEffect::Clipped {
                bounds: Some(Rect {
                    x: 0,
                    y: 0,
                    w: self.opencode_mask.w,
                    h: self.opencode_mask.h,
                }),
                mask: Some(Arc::clone(&self.opencode_mask)),
                inner: Box::new(VisualEffect::Custom(logo_effect)),
            };
            MouseRegion::new()
                .cell_mask(Arc::clone(&self.opencode_mask))
                .capture_click(true)
                .on_mouse_move(
                    ctx.link()
                        .callback(|e: MouseMoveEvent| Msg::OpenCodeMove(e.local_x, e.local_y)),
                )
                .on_hover_change(ctx.link().callback(Msg::OpenCodeHover))
                .on_mouse_down(ctx.link().callback(|_: MouseEvent| Msg::OpenCodeMouseDown))
                .on_click(ctx.link().callback(|_: MouseEvent| Msg::OpenCodeMouseUp))
                .child(
                    EffectScope::new().effect(effect).child(
                        AsciiCanvas::from_sequence(Arc::clone(&self.opencode_sequence))
                            .color_map(opencode_color_map(&self.opencode_sequence)),
                    ),
                )
                .into()
        } else {
            EffectScope::new()
                .effects(effects)
                .child(
                    MouseRegion::new()
                        .capture_click(true)
                        .on_mouse_move(ctx.link().callback(|e: MouseMoveEvent| {
                            Msg::CursorMoved(e.local_x as f32, e.local_y as f32)
                        }))
                        .on_hover_change(ctx.link().callback(Msg::HoverChanged))
                        .on_mouse_down(ctx.link().callback(|_: MouseEvent| Msg::MouseDown))
                        .on_click(ctx.link().callback(|_: MouseEvent| Msg::MouseUp))
                        .child(
                            BigText::new()
                                .text(LOGO_BIG_TEXT)
                                .font(BigFont::AnsiShadow)
                                .gradient(
                                    ColorGradient::new(
                                        Color::Rgb(80, 50, 180),
                                        Color::Rgb(20, 140, 180),
                                    )
                                    .with_center(Color::Rgb(60, 80, 200)),
                                    GradientDirection::Horizontal,
                                ),
                        ),
                )
                .into()
        };

        Frame::new()
            .title(frame_title)
            .border_style(BorderStyle::Rounded)
            .padding(2)
            .child(
                VStack::new().gap(1).child(tabs).child(
                    VStack::new()
                        .align(Align::Center)
                        .justify(Justify::Center)
                        .gap(2)
                        .child(stage)
                        .child(Text::new(hint)),
                ),
            )
            .into()
    }
}

fn main() -> Result<()> {
    let opencode_sequence = Arc::new(
        FrameSequence::from_json(OPENCODE_LOGO_JSON)
            .expect("Could not parse examples/assets/opencode_logo.json"),
    );
    let opencode_mask = mask_from_sequence(&opencode_sequence);
    let opencode_glyphs: Arc<[OpenCodeGlyph]> = opencode_glyphs(&opencode_mask).into();

    App::new()
        .title("Burst visual effects")
        .mount(BurstEffectsDemo {
            opencode_sequence,
            opencode_mask,
            opencode_glyphs,
        })
        .run()
}
