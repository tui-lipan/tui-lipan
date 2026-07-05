//! Centered Braille-style logo on [`AsciiCanvas`] with five visual modes:
//! **Hero** matches the website home demo (charge rings, triple shockwaves,
//! sparks, flash); shared brand gradient via nested [`EffectScope`] on the optional
//! Braille banner + main logo; **Void**, **Neon**, **CRT amber**, and **Quad**
//! add alternate palettes and burst patterns - hold/release on the art like
//! `burst_effects`, or tap for a quick pulse.
//!
//! Run with: `cargo run --example tui-lipan`
//!
//! Keys: `q` / `Esc` quit; `b` cycles optional **AsciiCanvas** Braille banner placement
//! (above / below / left / right / off). Mouse: move, hold on the logo, release
//! to fire (mode determines colors and wave count).

use std::f32::consts::TAU;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use tui_lipan::prelude::*;
use tui_lipan::{CellMask, EffectAxis};

#[path = "common/tui_lipan_art.rs"]
mod art;
use art::{
    BannerPlacement, LOGO, TAB_CRT_AMBER, TAB_HERO, TAB_LABELS, TAB_NEON, TAB_QUAD, TAB_VOID, TEXT,
    brand_gradient_scope_effects,
};

const R_SPEED: f32 = 22.0;
const R_RING: f32 = 2.4;
const R_MAX: f32 = 34.0;
const R_CHARGE_MAX: f32 = 4.2;
const R_CHARGE_SECS: f32 = 1.15;

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

const BANNER_GAP: u16 = 1;

struct TuiLipanShowcase;

struct Ripple {
    cx: f32,
    cy: f32,
    radius: f32,
    max_radius: f32,
    ring_width: f32,
    strength: f32,
    tint_a: Color,
    tint_b: Color,
}

/// Arguments for [`push_ripple`].
struct RippleSpec {
    cx: f32,
    cy: f32,
    max_radius: f32,
    strength: f32,
    ring_width: f32,
    tint_a: Color,
    tint_b: Color,
}

struct HeroChargeState {
    id: u64,
    cx: f32,
    cy: f32,
    t: f32,
    aura_phase: f32,
}

struct HeroShockwaveState {
    cx: f32,
    cy: f32,
    radius: f32,
    max_radius: f32,
    color_start: Color,
    color_end: Color,
    ring_width: f32,
    base_strength: f32,
}

struct HeroSparkState {
    launch_cx: f32,
    launch_cy: f32,
    vx: f32,
    vy: f32,
    color: Color,
    size: f32,
    max_age: f32,
}

#[derive(Default)]
struct HeroLane {
    press_start: Option<Instant>,
    charge: Option<HeroChargeState>,
    charge_cancel: Option<Arc<AtomicBool>>,
    shockwaves: Vec<(u64, HeroShockwaveState)>,
    sparks: Vec<HeroSparkState>,
    spark_elapsed: f32,
    flash_alpha: f32,
}

/// Cancel flag for the idle logo wave animation loop (website home matches `tick()` ~60ms).
#[derive(Default)]
struct HeroAnim {
    cancel: Option<Arc<AtomicBool>>,
}

struct State {
    tab: usize,
    cursor: (f32, f32),
    next_id: u64,
    ripples: Vec<(u64, Ripple)>,
    hero: HeroLane,
    hero_anim: HeroAnim,
    /// Press start for hold strength (non-void tabs except Hero).
    press_start: Option<Instant>,
    void_press: Option<Instant>,
    void_charge: Option<(u64, f32, f32, f32)>,
    void_charge_cancel: Option<Arc<AtomicBool>>,
    banner_placement: BannerPlacement,
}

#[derive(Clone, Debug)]
enum Msg {
    TabChanged(TabsEvent),
    CursorMoved(f32, f32),
    HoverChanged(bool),
    MouseDown,
    MouseUp,
    RippleTick {
        id: u64,
        radius: f32,
        max_radius: f32,
    },
    RippleDone(u64),
    VoidChargeTick {
        id: u64,
        size: f32,
    },
    HeroChargeTick {
        charge_id: u64,
        t: f32,
        aura_phase: f32,
    },
    HeroShockwaveTick {
        id: u64,
        radius: f32,
    },
    HeroShockwaveDone(u64),
    HeroSparkTick(f32),
    HeroFlashTick(f32),
    /// Advances the Hero tab logo gradient (website WASM calls `tick()` ~60ms).
    LogoAnimTick,
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

fn make_sparks(cx: f32, cy: f32, strength: f32, seed_base: u64) -> Vec<HeroSparkState> {
    (0..SN_SPARK_COUNT)
        .map(|i| {
            let s = seed_base.wrapping_add(i as u64 * 7919);
            let jitter = (rand01(s) - 0.5) * 0.6;
            let angle = (i as f32 / SN_SPARK_COUNT as f32) * TAU + jitter;
            let speed = (9.0 + rand01(s.wrapping_add(1)) * 16.0) * (0.6 + strength * 0.4);
            let color = fire_color(rand01(s.wrapping_add(2)));
            let size = 0.7 + rand01(s.wrapping_add(3)) * 0.8;
            let max_age = 0.5 + rand01(s.wrapping_add(4)) * 1.0;
            HeroSparkState {
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

fn cancel_hero_charge(ctx: &mut Context<TuiLipanShowcase>) {
    if let Some(c) = ctx.state.hero.charge_cancel.take() {
        c.store(true, Ordering::Release);
    }
    ctx.state.hero.charge = None;
}

fn cancel_void_charge(ctx: &mut Context<TuiLipanShowcase>) {
    if let Some(c) = ctx.state.void_charge_cancel.take() {
        c.store(true, Ordering::Release);
    }
    ctx.state.void_charge = None;
}

fn cancel_hero_anim(ctx: &mut Context<TuiLipanShowcase>) {
    if let Some(c) = ctx.state.hero_anim.cancel.take() {
        c.store(true, Ordering::Release);
    }
}

/// Starts ~60ms logo wave ticks (matches website WASM `tick()` interval).
fn maybe_start_hero_anim(ctx: &mut Context<TuiLipanShowcase>) -> Option<Update> {
    if ctx.state.tab != TAB_HERO || ctx.state.hero_anim.cancel.is_some() {
        return None;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    ctx.state.hero_anim.cancel = Some(Arc::clone(&cancel));
    let c = Arc::clone(&cancel);
    Some(Update::with_command(Command::spawn(move |link| {
        std::thread::spawn(move || {
            loop {
                if c.load(Ordering::Acquire) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(60));
                link.send(Msg::LogoAnimTick);
            }
        });
    })))
}

fn cancel_all(ctx: &mut Context<TuiLipanShowcase>) {
    cancel_hero_charge(ctx);
    cancel_void_charge(ctx);
    cancel_hero_anim(ctx);
    ctx.state.ripples.clear();
    ctx.state.press_start = None;
    ctx.state.void_press = None;
    ctx.state.hero.press_start = None;
    ctx.state.hero.charge = None;
    ctx.state.hero.shockwaves.clear();
    ctx.state.hero.sparks.clear();
    ctx.state.hero.spark_elapsed = 0.0;
    ctx.state.hero.flash_alpha = 0.0;
}

fn push_ripple(ripples: &mut Vec<(u64, Ripple)>, next_id: &mut u64, spec: RippleSpec) -> u64 {
    let id = *next_id;
    *next_id += 1;
    ripples.push((
        id,
        Ripple {
            cx: spec.cx,
            cy: spec.cy,
            radius: 0.0,
            max_radius: spec.max_radius,
            ring_width: spec.ring_width,
            strength: spec.strength,
            tint_a: spec.tint_a,
            tint_b: spec.tint_b,
        },
    ));
    id
}

fn collect_ripple_effects(ripples: &[(u64, Ripple)], out: &mut Vec<VisualEffect>) {
    for (_, r) in ripples {
        let t = (r.radius / r.max_radius).clamp(0.0, 1.0);
        let tint = r.tint_a.blend_toward(r.tint_b, t);
        let fade = 1.0 - ((t - 0.55) / 0.45).clamp(0.0, 1.0);
        out.push(VisualEffect::Ripple {
            origin: EffectOrigin::cell(r.cx, r.cy),
            radius: RippleRadius::Fixed(r.radius),
            ring_width: r.ring_width,
            tint,
            strength: r.strength * fade,
        });
    }
}

fn collect_hero_effects(hero: &HeroLane, out: &mut Vec<VisualEffect>) {
    if let Some(ref ch) = hero.charge {
        if ch.t > 0.0 {
            out.push(VisualEffect::dim(ch.t * 0.45));
        }
        for i in 0..SN_AURA_RINGS {
            let phase_off = i as f32 * (TAU / SN_AURA_RINGS as f32);
            let pulse = ((ch.aura_phase + phase_off).sin() * 0.5) + 0.5;
            let base_r = 3.0 + i as f32 * 2.2;
            let radius = base_r * (1.0 - ch.t * 0.35) + pulse * 0.6;
            let t_color = i as f32 / (SN_AURA_RINGS - 1).max(1) as f32;
            let tint = Color::Rgb(255, 160, 20).blend_toward(Color::Rgb(90, 120, 255), t_color);
            out.push(VisualEffect::Ripple {
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
            out.push(VisualEffect::Ripple {
                origin: EffectOrigin::cell(ch.cx, ch.cy),
                radius: RippleRadius::Fixed(0.0),
                ring_width: glow,
                tint: Color::Rgb(255, 200, 80),
                strength: 0.75 * flicker,
            });
            out.push(VisualEffect::Ripple {
                origin: EffectOrigin::cell(ch.cx, ch.cy),
                radius: RippleRadius::Fixed(0.0),
                ring_width: glow * 0.5,
                tint: Color::Rgb(255, 255, 240),
                strength: flicker,
            });
        }
    }
    for (_, sw) in &hero.shockwaves {
        let t = (sw.radius / sw.max_radius).clamp(0.0, 1.0);
        let tint = sw.color_start.blend_toward(sw.color_end, t);
        let fade = 1.0 - ((t - 0.6) / 0.4).clamp(0.0, 1.0);
        out.push(VisualEffect::Ripple {
            origin: EffectOrigin::cell(sw.cx, sw.cy),
            radius: RippleRadius::Fixed(sw.radius),
            ring_width: sw.ring_width,
            tint,
            strength: sw.base_strength * fade,
        });
    }
    let elapsed = hero.spark_elapsed;
    for spark in &hero.sparks {
        if elapsed >= spark.max_age {
            continue;
        }
        let t = elapsed / spark.max_age;
        let cx = spark.launch_cx + spark.vx * elapsed;
        let cy = spark.launch_cy + spark.vy * elapsed;
        let fade = (1.0 - t).powf(0.8);
        out.push(VisualEffect::Ripple {
            origin: EffectOrigin::cell(cx, cy),
            radius: RippleRadius::Fixed(0.0),
            ring_width: spark.size * (1.0 + t * 0.4),
            tint: spark.color,
            strength: fade * 0.95,
        });
    }
    if hero.flash_alpha > 0.0 {
        out.push(VisualEffect::tint(
            Color::Rgb(255, 240, 200),
            hero.flash_alpha * 0.75,
        ));
    }
}

fn base_effects(tab: usize, void_charge_size: f32) -> Vec<VisualEffect> {
    let mut v = Vec::new();

    match tab {
        TAB_VOID if void_charge_size > 0.01 => {
            v.push(VisualEffect::dim(
                (void_charge_size / R_CHARGE_MAX * 0.5).min(0.55),
            ));
        }
        TAB_VOID => {}
        TAB_NEON => {
            v.push(VisualEffect::Monochrome { strength: 0.35 });
            v.push(VisualEffect::RainbowWave {
                blend: 0.82,
                frequency: 1.2,
                speed: 1.05,
                axis: EffectAxis::Diagonal,
            });
        }
        TAB_CRT_AMBER => {
            v.push(VisualEffect::RetroCrt {
                preset: RetroPreset::Amber,
                flicker: 0.35,
                scanline_strength: 0.2,
            });
        }
        _ => {}
    }

    v
}

fn void_charge_glow_effects(cx: f32, cy: f32, size: f32) -> Vec<VisualEffect> {
    if size < 0.02 {
        return Vec::new();
    }
    let pulse = 0.88 + (size * 2.1).sin() * 0.12;
    vec![
        VisualEffect::Ripple {
            origin: EffectOrigin::cell(cx, cy),
            radius: RippleRadius::Fixed(0.0),
            ring_width: 1.2 + size * 2.8,
            tint: Color::Rgb(120, 200, 255),
            strength: 0.55 * pulse,
        },
        VisualEffect::Ripple {
            origin: EffectOrigin::cell(cx, cy),
            radius: RippleRadius::Fixed(0.0),
            ring_width: (1.0 + size * 2.2) * 0.55,
            tint: Color::Rgb(255, 255, 255),
            strength: 0.35 * pulse,
        },
    ]
}

fn art_dimensions(text: &str) -> (u16, u16) {
    let mut width = 0u16;
    let mut height = 0u16;
    for line in text.lines() {
        width = width.max(line.chars().count() as u16);
        height = height.saturating_add(1);
    }
    (width, height)
}

fn centered_offset(outer: u16, inner: u16) -> u16 {
    outer.saturating_sub(inner) / 2
}

fn art_scope_size(placement: BannerPlacement) -> (u16, u16) {
    let (logo_w, logo_h) = art_dimensions(LOGO);
    let (text_w, text_h) = art_dimensions(TEXT);
    match placement {
        BannerPlacement::Off => (logo_w, logo_h),
        BannerPlacement::Above | BannerPlacement::Below => (
            logo_w.max(text_w),
            logo_h.saturating_add(BANNER_GAP).saturating_add(text_h),
        ),
        BannerPlacement::Left | BannerPlacement::Right => (
            logo_w.saturating_add(BANNER_GAP).saturating_add(text_w),
            logo_h.max(text_h),
        ),
    }
}

fn logo_scope_offset(placement: BannerPlacement) -> (u16, u16) {
    let (logo_w, logo_h) = art_dimensions(LOGO);
    let (text_w, text_h) = art_dimensions(TEXT);
    match placement {
        BannerPlacement::Off => (0, 0),
        BannerPlacement::Above => (
            centered_offset(logo_w.max(text_w), logo_w),
            text_h.saturating_add(BANNER_GAP),
        ),
        BannerPlacement::Below => (centered_offset(logo_w.max(text_w), logo_w), 0),
        BannerPlacement::Left => (
            text_w.saturating_add(BANNER_GAP),
            centered_offset(logo_h.max(text_h), logo_h),
        ),
        BannerPlacement::Right => (0, centered_offset(logo_h.max(text_h), logo_h)),
    }
}

fn banner_scope_offset(placement: BannerPlacement) -> Option<(u16, u16)> {
    let (logo_w, logo_h) = art_dimensions(LOGO);
    let (text_w, text_h) = art_dimensions(TEXT);
    match placement {
        BannerPlacement::Off => None,
        BannerPlacement::Above => Some((centered_offset(logo_w.max(text_w), text_w), 0)),
        BannerPlacement::Below => Some((
            centered_offset(logo_w.max(text_w), text_w),
            logo_h.saturating_add(BANNER_GAP),
        )),
        BannerPlacement::Left => Some((0, centered_offset(logo_h.max(text_h), text_h))),
        BannerPlacement::Right => Some((
            logo_w.saturating_add(BANNER_GAP),
            centered_offset(logo_h.max(text_h), text_h),
        )),
    }
}

fn set_mask_bit(bits: &mut [u64], w: u16, x: u16, y: u16) {
    let idx = y as usize * w as usize + x as usize;
    if let Some(word) = bits.get_mut(idx / 64) {
        *word |= 1u64 << (idx % 64);
    }
}

fn add_art_to_mask(bits: &mut [u64], scope_w: u16, art: &str, offset: (u16, u16)) {
    let lines: Vec<String> = art.lines().map(str::to_owned).collect();
    let Some((ink, mask)) = CellMask::from_char_lines(&lines) else {
        return;
    };
    for y in 0..mask.h {
        for x in 0..mask.w {
            if !mask.test_region_local(x, y) {
                continue;
            }
            let scope_x = offset
                .0
                .saturating_add(ink.x.max(0) as u16)
                .saturating_add(x);
            let scope_y = offset
                .1
                .saturating_add(ink.y.max(0) as u16)
                .saturating_add(y);
            set_mask_bit(bits, scope_w, scope_x, scope_y);
        }
    }
}

fn art_scope_mask(placement: BannerPlacement) -> Arc<CellMask> {
    let (scope_w, scope_h) = art_scope_size(placement);
    let total = scope_w as usize * scope_h as usize;
    let mut bits = vec![0u64; total.div_ceil(64)];

    add_art_to_mask(&mut bits, scope_w, LOGO, logo_scope_offset(placement));
    if let Some(offset) = banner_scope_offset(placement) {
        add_art_to_mask(&mut bits, scope_w, TEXT, offset);
    }

    Arc::new(CellMask {
        origin: (0, 0),
        w: scope_w,
        h: scope_h,
        bits: bits.into(),
    })
}

impl Component for TuiLipanShowcase {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            tab: 0,
            cursor: (0.0, 0.0),
            next_id: 1,
            ripples: Vec::new(),
            hero: HeroLane::default(),
            hero_anim: HeroAnim::default(),
            press_start: None,
            void_press: None,
            void_charge: None,
            void_charge_cancel: None,
            banner_placement: BannerPlacement::default(),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::TabChanged(ev) => {
                if ev.index != ctx.state.tab {
                    cancel_all(ctx);
                    ctx.state.tab = ev.index;
                    if let Some(u) = maybe_start_hero_anim(ctx) {
                        return u;
                    }
                    return Update::full();
                }
                Update::none()
            }

            Msg::CursorMoved(x, y) => {
                ctx.state.cursor = (x, y);
                if let Some(u) = maybe_start_hero_anim(ctx) {
                    return u;
                }
                Update::none()
            }

            Msg::HoverChanged(hovered) => {
                if !hovered {
                    let hero_press =
                        ctx.state.tab == TAB_HERO && ctx.state.hero.press_start.is_some();
                    let other_press = ctx.state.tab != TAB_HERO && ctx.state.press_start.is_some();
                    if hero_press || other_press {
                        ctx.link().send(Msg::MouseUp);
                    }
                    if ctx.state.tab == TAB_VOID && ctx.state.void_charge.is_some() {
                        cancel_void_charge(ctx);
                        ctx.state.void_press = None;
                        return Update::full();
                    }
                }
                Update::none()
            }

            Msg::MouseDown => {
                if ctx.state.tab == TAB_HERO {
                    cancel_hero_charge(ctx);
                    let cancel = Arc::new(AtomicBool::new(false));
                    ctx.state.hero.charge_cancel = Some(Arc::clone(&cancel));
                    ctx.state.hero.press_start = Some(Instant::now());
                    let (cx, cy) = ctx.state.cursor;
                    let charge_id = ctx.state.next_id;
                    ctx.state.next_id += 1;
                    ctx.state.hero.charge = Some(HeroChargeState {
                        id: charge_id,
                        cx,
                        cy,
                        t: 0.0,
                        aura_phase: 0.0,
                    });
                    return Update::with_command(Command::spawn(move |link| {
                        let start = Instant::now();
                        loop {
                            if cancel.load(Ordering::Acquire) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(16));
                            let secs = start.elapsed().as_secs_f32();
                            let t = (secs / SN_CHARGE_FILL_SECS).min(1.0);
                            let aura_phase = secs * 5.5;
                            link.send(Msg::HeroChargeTick {
                                charge_id,
                                t,
                                aura_phase,
                            });
                            if secs > 10.0 {
                                break;
                            }
                        }
                    }));
                }
                if ctx.state.tab == TAB_VOID {
                    cancel_void_charge(ctx);
                    let cancel = Arc::new(AtomicBool::new(false));
                    ctx.state.void_charge_cancel = Some(Arc::clone(&cancel));
                    ctx.state.void_press = Some(Instant::now());
                    let (cx, cy) = ctx.state.cursor;
                    let id = ctx.state.next_id;
                    ctx.state.next_id += 1;
                    ctx.state.void_charge = Some((id, cx, cy, 0.0));
                    return Update::with_command(Command::spawn(move |link| {
                        let start = Instant::now();
                        loop {
                            if cancel.load(Ordering::Acquire) {
                                break;
                            }
                            std::thread::sleep(Duration::from_millis(16));
                            let secs = start.elapsed().as_secs_f32();
                            let size = (secs / R_CHARGE_SECS * R_CHARGE_MAX).min(R_CHARGE_MAX);
                            link.send(Msg::VoidChargeTick { id, size });
                            if secs > 12.0 {
                                break;
                            }
                        }
                    }));
                }
                ctx.state.press_start = Some(Instant::now());
                Update::none()
            }

            Msg::HeroChargeTick {
                charge_id,
                t,
                aura_phase,
            } => {
                if ctx.state.tab != TAB_HERO {
                    return Update::none();
                }
                if let Some(ref mut ch) = ctx.state.hero.charge
                    && ch.id == charge_id
                {
                    ch.t = t;
                    ch.aura_phase = aura_phase;
                    return Update::full();
                }
                Update::none()
            }

            Msg::VoidChargeTick { id, size } => {
                if ctx.state.tab != TAB_VOID {
                    return Update::none();
                }
                if let Some((cid, _, _, ref mut s)) = ctx.state.void_charge
                    && cid == id
                {
                    *s = size;
                    return Update::full();
                }
                Update::none()
            }

            Msg::MouseUp => {
                let tab = ctx.state.tab;
                let (cx, cy) = ctx.state.cursor;

                if tab == TAB_HERO {
                    if let Some(c) = ctx.state.hero.charge_cancel.take() {
                        c.store(true, Ordering::Release);
                    }
                    let held_secs = ctx
                        .state
                        .hero
                        .press_start
                        .take()
                        .map(|t| t.elapsed().as_secs_f32())
                        .unwrap_or(0.05);

                    let (cx, cy) = if let Some(ch) = ctx.state.hero.charge.take() {
                        (ch.cx, ch.cy)
                    } else {
                        (cx, cy)
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

                    ctx.state.hero.shockwaves.push((
                        core_id,
                        HeroShockwaveState {
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
                    ctx.state.hero.shockwaves.push((
                        fire_id,
                        HeroShockwaveState {
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
                    ctx.state.hero.shockwaves.push((
                        outer_id,
                        HeroShockwaveState {
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

                    ctx.state.hero.sparks = make_sparks(cx, cy, strength, seed_base);
                    ctx.state.hero.spark_elapsed = 0.0;
                    ctx.state.hero.flash_alpha = 0.85 * (0.4 + strength * 0.6);

                    let core_max = max_r * 0.75;
                    let fire_max = max_r;
                    let outer_max = max_r * 1.25;
                    let max_spark_age = ctx
                        .state
                        .hero
                        .sparks
                        .iter()
                        .map(|s| s.max_age)
                        .fold(0.0_f32, f32::max);

                    return Update::with_command(Command::spawn(move |link| {
                        let l = link.clone();
                        std::thread::spawn(move || {
                            let start = Instant::now();
                            let dur = Duration::from_secs_f32(core_max / SN_SHOCKWAVE_SPEED_CORE);
                            loop {
                                std::thread::sleep(Duration::from_millis(16));
                                let e = start.elapsed();
                                l.send(Msg::HeroShockwaveTick {
                                    id: core_id,
                                    radius: e.as_secs_f32() * SN_SHOCKWAVE_SPEED_CORE,
                                });
                                if e >= dur {
                                    l.send(Msg::HeroShockwaveDone(core_id));
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
                                l.send(Msg::HeroShockwaveTick {
                                    id: fire_id,
                                    radius: e.as_secs_f32() * SN_SHOCKWAVE_SPEED_FIRE,
                                });
                                if e >= dur {
                                    l.send(Msg::HeroShockwaveDone(fire_id));
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
                                l.send(Msg::HeroShockwaveTick {
                                    id: outer_id,
                                    radius: e.as_secs_f32() * SN_SHOCKWAVE_SPEED_OUTER,
                                });
                                if e >= dur {
                                    l.send(Msg::HeroShockwaveDone(outer_id));
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
                                l.send(Msg::HeroSparkTick(elapsed));
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
                                link.send(Msg::HeroFlashTick(a * a));
                                if a <= 0.0 {
                                    break;
                                }
                            }
                        });
                    }));
                }

                if tab == TAB_VOID {
                    cancel_void_charge(ctx);
                    let void_held = ctx
                        .state
                        .void_press
                        .take()
                        .map(|t| t.elapsed().as_secs_f32());

                    let strength = void_held.unwrap_or(0.08).clamp(0.06, 2.0) / R_CHARGE_SECS;
                    let max_r = 14.0 + strength * (R_MAX - 14.0);
                    let tint_a = Color::Rgb(80, 220, 255);
                    let tint_b = Color::Rgb(200, 100, 255);
                    let id = push_ripple(
                        &mut ctx.state.ripples,
                        &mut ctx.state.next_id,
                        RippleSpec {
                            cx,
                            cy,
                            max_radius: max_r,
                            strength: 0.75 + strength * 0.2,
                            ring_width: R_RING * 1.1,
                            tint_a,
                            tint_b,
                        },
                    );
                    return Update::with_command(Command::spawn(move |l| {
                        let start = Instant::now();
                        loop {
                            std::thread::sleep(Duration::from_millis(16));
                            let r = start.elapsed().as_secs_f32() * R_SPEED;
                            l.send(Msg::RippleTick {
                                id,
                                radius: r,
                                max_radius: max_r,
                            });
                            if r >= max_r {
                                l.send(Msg::RippleDone(id));
                                break;
                            }
                        }
                    }));
                }

                let held = ctx
                    .state
                    .press_start
                    .take()
                    .map(|t| t.elapsed().as_secs_f32())
                    .unwrap_or(0.06)
                    .clamp(0.04, 2.5);

                let s = (held / 0.35).clamp(0.35, 1.0);

                match tab {
                    TAB_QUAD => {
                        let offs = [
                            (18.0_f32, 10.0),
                            (-18.0, 10.0),
                            (18.0, -10.0),
                            (-18.0, -10.0),
                        ];
                        let mut meta: Vec<(u64, f32, u64)> = Vec::new();
                        for (i, (ox, oy)) in offs.iter().enumerate() {
                            let tcx = (cx + ox).max(2.0);
                            let tcy = (cy + oy).max(2.0);
                            let seed = ctx.state.next_id.wrapping_add(i as u64 * 997);
                            let tint_a = Color::Rgb(100 + ((seed as u8) % 80), 200, 255);
                            let tint_b = Color::Rgb(255, 120u8.saturating_add(i as u8 * 22), 80);
                            let max_r = 10.0 + s * 18.0;
                            let id = push_ripple(
                                &mut ctx.state.ripples,
                                &mut ctx.state.next_id,
                                RippleSpec {
                                    cx: tcx,
                                    cy: tcy,
                                    max_radius: max_r,
                                    strength: 0.62,
                                    ring_width: R_RING * 0.9,
                                    tint_a,
                                    tint_b,
                                },
                            );
                            meta.push((id, max_r, i as u64));
                        }
                        Update::with_command(Command::spawn(move |link| {
                            for (id, max_r, i) in meta {
                                let l = link.clone();
                                std::thread::spawn(move || {
                                    std::thread::sleep(Duration::from_millis(30 * i));
                                    let start = Instant::now();
                                    loop {
                                        std::thread::sleep(Duration::from_millis(16));
                                        let r = start.elapsed().as_secs_f32() * (R_SPEED * 0.95);
                                        l.send(Msg::RippleTick {
                                            id,
                                            radius: r,
                                            max_radius: max_r,
                                        });
                                        if r >= max_r {
                                            l.send(Msg::RippleDone(id));
                                            break;
                                        }
                                    }
                                });
                            }
                        }))
                    }

                    _ => {
                        let (a, b) = match tab {
                            TAB_NEON => (Color::Rgb(100, 220, 255), Color::Rgb(255, 120, 200)),
                            TAB_CRT_AMBER => (Color::Rgb(255, 220, 140), Color::Rgb(255, 160, 70)),
                            _ => (Color::Rgb(180, 140, 255), Color::Rgb(255, 200, 100)),
                        };
                        let max_r = 12.0 + s * (R_MAX - 12.0);
                        let id = push_ripple(
                            &mut ctx.state.ripples,
                            &mut ctx.state.next_id,
                            RippleSpec {
                                cx,
                                cy,
                                max_radius: max_r,
                                strength: 0.68,
                                ring_width: R_RING,
                                tint_a: a,
                                tint_b: b,
                            },
                        );
                        Update::with_command(Command::spawn(move |l| {
                            let start = Instant::now();
                            loop {
                                std::thread::sleep(Duration::from_millis(16));
                                let r = start.elapsed().as_secs_f32() * R_SPEED;
                                l.send(Msg::RippleTick {
                                    id,
                                    radius: r,
                                    max_radius: max_r,
                                });
                                if r >= max_r {
                                    l.send(Msg::RippleDone(id));
                                    break;
                                }
                            }
                        }))
                    }
                }
            }

            Msg::RippleTick {
                id,
                radius,
                max_radius,
            } => {
                if let Some((_, r)) = ctx.state.ripples.iter_mut().find(|(i, _)| *i == id) {
                    r.radius = radius.min(max_radius);
                    r.max_radius = max_radius;
                }
                Update::full()
            }

            Msg::RippleDone(id) => {
                ctx.state.ripples.retain(|(i, _)| *i != id);
                Update::full()
            }

            Msg::HeroShockwaveTick { id, radius } => {
                if let Some((_, sw)) = ctx
                    .state
                    .hero
                    .shockwaves
                    .iter_mut()
                    .find(|(sid, _)| *sid == id)
                {
                    sw.radius = radius;
                }
                Update::full()
            }

            Msg::HeroShockwaveDone(id) => {
                ctx.state.hero.shockwaves.retain(|(sid, _)| *sid != id);
                Update::full()
            }

            Msg::HeroSparkTick(elapsed) => {
                ctx.state.hero.spark_elapsed = elapsed;
                let all_dead = ctx.state.hero.sparks.iter().all(|s| elapsed >= s.max_age);
                if all_dead {
                    ctx.state.hero.sparks.clear();
                }
                Update::full()
            }

            Msg::HeroFlashTick(alpha) => {
                ctx.state.hero.flash_alpha = alpha;
                Update::full()
            }

            Msg::LogoAnimTick => {
                if ctx.state.tab != TAB_HERO {
                    return Update::none();
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
            KeyCode::Char('b') | KeyCode::Char('B') => {
                ctx.state.banner_placement = ctx.state.banner_placement.next();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let tab = ctx.state.tab;
        let void_size = ctx.state.void_charge.map(|(_, _, _, sz)| sz).unwrap_or(0.0);

        let mut effects = Vec::new();
        if tab == TAB_HERO {
            collect_hero_effects(&ctx.state.hero, &mut effects);
        } else {
            effects.extend(base_effects(tab, void_size));
        }
        collect_ripple_effects(&ctx.state.ripples, &mut effects);

        if tab == TAB_VOID {
            if let Some((_, cx, cy, sz)) = ctx.state.void_charge {
                effects.extend(void_charge_glow_effects(cx, cy, sz));
            }
        }

        let charging = if tab == TAB_VOID {
            ctx.state.void_charge.is_some()
        } else if tab == TAB_HERO {
            ctx.state.hero.press_start.is_some()
        } else {
            ctx.state.press_start.is_some()
        };

        let title = format!(
            "tui-lipan showcase - {} ({}){} | banner {} | b cycle | q quit",
            TAB_LABELS.get(tab).copied().unwrap_or("?"),
            if charging { "charging" } else { "ready" },
            if charging { "…" } else { "" },
            ctx.state.banner_placement.label(),
        );

        let canvas: Element = AsciiCanvas::new(LOGO.lines()).into();
        let text_canvas: Element = AsciiCanvas::new(TEXT.lines()).into();

        let tabs: Vec<Tab> = TAB_LABELS.iter().copied().map(Tab::new).collect();

        let placement = ctx.state.banner_placement;
        let art_mask = art_scope_mask(placement);
        let banner_and_logo: Element = match placement {
            BannerPlacement::Off => canvas,
            BannerPlacement::Above => VStack::new()
                .align(Align::Center)
                .gap(BANNER_GAP)
                .child(text_canvas)
                .child(canvas)
                .into(),
            BannerPlacement::Below => VStack::new()
                .align(Align::Center)
                .gap(BANNER_GAP)
                .child(canvas)
                .child(text_canvas)
                .into(),
            BannerPlacement::Left => HStack::new()
                .align(Align::Center)
                .gap(BANNER_GAP)
                .child(text_canvas)
                .child(canvas)
                .into(),
            BannerPlacement::Right => HStack::new()
                .align(Align::Center)
                .gap(BANNER_GAP)
                .child(canvas)
                .child(text_canvas)
                .into(),
        };

        let gradient_wrapped = EffectScope::new()
            .effects(brand_gradient_scope_effects(tab))
            .child(banner_and_logo);

        let interactive_art =
            MouseRegion::new()
                .capture_click(true)
                .cell_mask(art_mask)
                .on_mouse_move(ctx.link().callback(|e: MouseMoveEvent| {
                    Msg::CursorMoved(e.local_x as f32, e.local_y as f32)
                }))
                .on_hover_change(ctx.link().callback(Msg::HoverChanged))
                .on_mouse_down(ctx.link().callback(|_: MouseEvent| Msg::MouseDown))
                .on_click(ctx.link().callback(|_: MouseEvent| Msg::MouseUp))
                .child(gradient_wrapped);

        let stage = Center::new().child(EffectScope::new().effects(effects).child(interactive_art));

        Frame::new()
            .title(title)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Tabs::new()
                            .tabs(tabs)
                            .active(tab.min(TAB_LABELS.len().saturating_sub(1)))
                            .on_change(ctx.link().callback(Msg::TabChanged)),
                    )
                    .child(VStack::new().align(Align::Center).gap(1).child(stage)),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan AsciiCanvas showcase")
        .mount(TuiLipanShowcase)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mask_has_any_scope_local_cell(mask: &CellMask) -> bool {
        for y in 0..mask.h {
            for x in 0..mask.w {
                let sx = mask.origin.0.saturating_add(x) as i16;
                let sy = mask.origin.1.saturating_add(y) as i16;
                if mask.test_scope_local(sx, sy) {
                    return true;
                }
            }
        }
        false
    }

    #[test]
    fn art_mask_covers_logo_and_banner_placements() {
        let off = art_scope_mask(BannerPlacement::Off);
        let above = art_scope_mask(BannerPlacement::Above);
        let below = art_scope_mask(BannerPlacement::Below);
        let left = art_scope_mask(BannerPlacement::Left);
        let right = art_scope_mask(BannerPlacement::Right);

        let (off_w, off_h) = art_scope_size(BannerPlacement::Off);
        let (above_w, above_h) = art_scope_size(BannerPlacement::Above);
        let (left_w, left_h) = art_scope_size(BannerPlacement::Left);
        assert_eq!((off.w, off.h), (off_w, off_h));
        assert_eq!((above.w, above.h), (above_w, above_h));
        assert_eq!((left.w, left.h), (left_w, left_h));

        assert!(above.h > off.h);
        assert!(below.h > off.h);
        assert!(left.w > off.w);
        assert!(right.w > off.w);

        for mask in [off, above, below, left, right] {
            assert!(mask_has_any_scope_local_cell(&mask));
        }
    }
}
