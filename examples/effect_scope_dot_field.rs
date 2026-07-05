use std::{f32::consts::TAU, time::Duration};

use tui_lipan::prelude::*;

const VARIANT_TITLES: [&str; 4] = ["Neon Bloom", "Veiled Lights", "Ember Drift", "Hearth Glow"];
const NEON_CHASE_TICKS: f32 = 28.0;
const NEON_IDLE_START_TICKS: f32 = 90.0;
const NEON_IDLE_END_TICKS: f32 = 170.0;

#[derive(Clone, Copy, Debug)]
struct NeonBloomEffect {
    pointer: Option<NeonPointer>,
}

#[derive(Clone, Copy, Debug)]
struct NeonPointer {
    from_x: f32,
    from_y: f32,
    target_x: f32,
    target_y: f32,
    move_start_phase: u64,
    last_move_phase: u64,
    acquired: bool,
}

#[derive(Clone, Copy, Debug)]
struct VeiledLightsEffect;

#[derive(Clone, Copy, Debug)]
struct VeiledLight {
    sx: f32,
    sy: f32,
    pulse: f32,
    reveal_radius: f32,
    reveal_radius2: f32,
    ambient_radius: f32,
    ambient_cutoff2: f32,
}

#[derive(Debug)]
struct PreparedVeiledLightsEffect {
    lights: Vec<VeiledLight>,
}

#[derive(Clone, Copy, Debug)]
struct EmberDriftEffect;

#[derive(Clone, Copy, Debug)]
struct HearthGlowEffect;

#[derive(Default)]
struct State {
    active_variant: usize,
    neon_pointer: Option<NeonPointer>,
}

#[derive(Clone, Debug)]
enum Msg {
    TabChanged(TabsEvent),
    NeonMouseMove(MouseMoveEvent),
    NeonHover(bool),
}

impl CellEffect for NeonBloomEffect {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        let w = ctx.bounds.w.max(1) as f32;
        let h = ctx.bounds.h.max(1) as f32;
        let lx = ctx.x as f32 - ctx.bounds.x as f32 + 0.5;
        let ly = ctx.y as f32 - ctx.bounds.y as f32 + 0.5;

        let t = ctx.phase as f32 * 0.026;
        let auto_cx = w * (0.5 + 0.43 * (t * 0.73).sin());
        let auto_cy = h * (0.5 + 0.38 * (t * 0.51 + TAU * 0.18).cos());
        let (cx, cy) = self.pointer.map_or((auto_cx, auto_cy), |pointer| {
            let move_t = (ctx.phase.saturating_sub(pointer.move_start_phase) as f32
                / NEON_CHASE_TICKS)
                .clamp(0.0, 1.0);
            let move_ease = neon_chase_ease(move_t);
            let chased_x = pointer.from_x + (pointer.target_x - pointer.from_x) * move_ease;
            let chased_y = pointer.from_y + (pointer.target_y - pointer.from_y) * move_ease;
            let idle = ctx.phase.saturating_sub(pointer.last_move_phase) as f32;
            let follow = (1.0 - smoothstep(NEON_IDLE_START_TICKS, NEON_IDLE_END_TICKS, idle))
                .clamp(0.0, 1.0);
            (
                auto_cx + (chased_x - auto_cx) * follow,
                auto_cy + (chased_y - auto_cy) * follow,
            )
        });
        let dx = lx - cx;
        let dy = (ly - cy) * 2.05;
        let dist = (dx * dx + dy * dy).sqrt();
        let glow_radius = (w.min(h * 2.0) * 0.22).clamp(5.0, 18.0);
        let glow = (1.0 - dist / glow_radius).clamp(0.0, 1.0).powf(1.55);

        let shimmer = ((lx * 0.31 + ly * 0.47 + t * 3.0).sin() * 0.5 + 0.5) * 0.08;
        let intensity = (0.16 + glow * 0.84 + shimmer).clamp(0.0, 1.0);

        let (glyph, r, g, b) = if intensity > 0.88 {
            ("●", 245, 250, 255)
        } else if intensity > 0.68 {
            ("●", 170, 220, 255)
        } else if intensity > 0.46 {
            ("•", 95, 155, 215)
        } else if intensity > 0.28 {
            ("·", 48, 76, 112)
        } else {
            ("·", 25, 36, 54)
        };

        cell.set_symbol(glyph);
        cell.set_fg(TerminalColor::Rgb(r, g, b));
        cell.set_bg(TerminalColor::Rgb(2, 4, 10));
    }

    fn is_animated(&self) -> bool {
        true
    }

    fn cache_key(&self) -> u64 {
        0xD07F_1E1D
    }
}

impl CellEffect for VeiledLightsEffect {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        PreparedVeiledLightsEffect::new(ctx.bounds, ctx.phase).apply(cell, ctx);
    }

    fn prepare(&self, ctx: &EffectPrepareContext) -> Option<Box<dyn PreparedCellEffect>> {
        Some(Box::new(PreparedVeiledLightsEffect::new(
            ctx.bounds, ctx.phase,
        )))
    }

    fn is_animated(&self) -> bool {
        true
    }

    fn animation_interval(&self) -> Duration {
        Duration::from_millis(33)
    }

    fn cache_key(&self) -> u64 {
        0x5E11_0D1E_u64
    }
}

impl PreparedVeiledLightsEffect {
    fn new(bounds: Rect, phase: u64) -> Self {
        let w = bounds.w.max(1) as f32;
        let h = bounds.h.max(1) as f32;
        let t = phase as f32 * 0.011;
        let radius_scale = (w.min(h * 2.0) / 64.0).clamp(0.48, 2.25);

        let mut lights = Vec::with_capacity(10);
        for i in 0..10 {
            let seed = i as f32;
            let cycle = t * (0.22 + seed * 0.024) + seed * 11.37;
            let life = cycle.fract();
            let appear = smoothstep(0.03, 0.24, life);
            let disappear = 1.0 - smoothstep(0.70, 0.98, life);
            let pulse = (appear * disappear).powf(2.6) * (0.26 + hashed_unit(i, 9) * 0.18);
            if pulse < 0.002 {
                continue;
            }

            let base_x = 0.08 + hashed_unit(i, 1) * 0.84;
            let base_y = 0.10 + hashed_unit(i, 2) * 0.78;
            let travel = smoothstep(0.02, 0.82, life);
            let arc = (travel - 0.5) * 2.0;
            let direction = hashed_unit(i, 10) * TAU;
            let span_x = 0.05 + hashed_unit(i, 4) * 0.08;
            let span_y = 0.04 + hashed_unit(i, 6) * 0.07;
            let wander_x = (cycle * TAU * (0.82 + hashed_unit(i, 3) * 0.36)).sin() * span_x * 0.28;
            let wander_y = (cycle * TAU * (0.74 + hashed_unit(i, 5) * 0.34)).cos() * span_y * 0.28;
            let drift_x = arc * direction.cos() * span_x + wander_x;
            let drift_y = arc * direction.sin() * span_y + wander_y;
            let sx = w * (base_x + drift_x).clamp(0.05, 0.95);
            let sy = h * (base_y + drift_y).clamp(0.06, 0.94);
            let reveal_radius = (24.0 + hashed_unit(i, 7) * 20.0) * radius_scale;
            let reveal_radius2 = reveal_radius * reveal_radius;
            let ambient_radius = reveal_radius * 2.35;
            let ambient_cutoff = ambient_radius * 1.25;
            let ambient_cutoff2 = ambient_cutoff * ambient_cutoff;

            lights.push(VeiledLight {
                sx,
                sy,
                pulse,
                reveal_radius,
                reveal_radius2,
                ambient_radius,
                ambient_cutoff2,
            });
        }

        Self { lights }
    }
}

impl PreparedCellEffect for PreparedVeiledLightsEffect {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        let lx = ctx.x as f32 - ctx.bounds.x as f32 + 0.5;
        let ly = ctx.y as f32 - ctx.bounds.y as f32 + 0.5;

        let mut reveal_light = 0.0;
        let mut ambient_light = 0.0;
        for light in &self.lights {
            let dx = lx - light.sx;
            let dy = (ly - light.sy) * 2.0;
            let dist2 = dx * dx + dy * dy;
            if dist2 <= light.reveal_radius2 {
                let dist = dist2.sqrt();
                let reveal_halo = (1.0 - dist / light.reveal_radius).powf(3.35);
                reveal_light += light.pulse * reveal_halo;
            }
            if dist2 <= light.ambient_cutoff2 {
                let dist = dist2.sqrt();
                let ambient_halo = (-(dist / light.ambient_radius).powf(2.35) * 9.0).exp();
                ambient_light += light.pulse * ambient_halo;
            }
        }

        let backdrop = (8.0 + ambient_light * 0.42).clamp(0.0, 255.0) as u8;
        let bg = TerminalColor::Rgb(backdrop, backdrop, backdrop.saturating_add(1));

        let light = reveal_light * 0.62 + ambient_light * 0.13;
        if light < 0.018 {
            cell.set_symbol(" ");
            cell.set_fg(bg);
            cell.set_bg(bg);
            return;
        }

        let reveal = smoothstep(0.018, 0.30, light);
        let opacity = (reveal.powf(2.2) * 0.42).clamp(0.0, 0.42);
        let glyph = if opacity > 0.34 {
            "•"
        } else if opacity > 0.025 {
            "·"
        } else {
            " "
        };

        let fg = TerminalColor::Rgb(
            blend_channel(backdrop, 178, opacity),
            blend_channel(backdrop.saturating_add(1), 186, opacity),
            blend_channel(backdrop.saturating_add(2), 195, opacity),
        );

        cell.set_symbol(glyph);
        cell.set_fg(fg);
        cell.set_bg(bg);
    }
}

impl CellEffect for EmberDriftEffect {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        let w = ctx.bounds.w.max(1) as f32;
        let h = ctx.bounds.h.max(1) as f32;
        let lx = ctx.x as f32 - ctx.bounds.x as f32 + 0.5;
        let ly = ctx.y as f32 - ctx.bounds.y as f32 + 0.5;
        let t = ctx.phase as f32 * 0.021;
        let radius_scale = (w.min(h * 2.0) / 72.0).clamp(0.72, 1.8);

        let ember_a = (
            w * (0.28 + 0.05 * (t * 0.59).sin()),
            h * (0.28 + 0.04 * (t * 0.41 + TAU * 0.11).cos()),
            ((t * 0.58).sin() * 0.5 + 0.5).powf(2.25) * 0.95,
            9.5 * radius_scale,
        );
        let ember_b = (
            w * (0.66 + 0.06 * (t * 0.46 + TAU * 0.27).cos()),
            h * (0.62 + 0.05 * (t * 0.52 + TAU * 0.48).sin()),
            ((t * 0.49 + TAU * 0.33).sin() * 0.5 + 0.5).powf(2.4) * 0.9,
            10.5 * radius_scale,
        );
        let ember_c = (
            w * (0.46 + 0.05 * (t * 0.53 + TAU * 0.62).sin()),
            h * (0.42 + 0.05 * (t * 0.45 + TAU * 0.19).cos()),
            ((t * 0.42 + TAU * 0.7).sin() * 0.5 + 0.5).powf(2.55) * 0.72,
            8.5 * radius_scale,
        );
        let ember_d = (
            w * (0.78 + 0.04 * (t * 0.61 + TAU * 0.4).cos()),
            h * (0.33 + 0.04 * (t * 0.57 + TAU * 0.76).sin()),
            ((t * 0.67 + TAU * 0.18).sin() * 0.5 + 0.5).powf(2.75) * 0.58,
            7.8 * radius_scale,
        );

        let sources = [ember_a, ember_b, ember_c, ember_d];
        let mut ember = 0.0;
        for (sx, sy, pulse, radius) in sources {
            let dx = lx - sx;
            let dy = (ly - sy) * 2.0;
            let dist = (dx * dx + dy * dy).sqrt();
            ember += pulse * (1.0 - dist / radius).clamp(0.0, 1.0).powf(2.0);
        }

        let haze = ((lx * 0.15 + ly * 0.09 + t * 0.8).sin() * 0.5 + 0.5) * 0.05;
        let grain = hashed_noise(ctx.x as i32, ctx.y as i32) * 0.04;
        let backdrop = (15.0 + grain * 16.0 + haze * 12.0 + ember * 9.0).clamp(0.0, 255.0) as u8;
        let bg = TerminalColor::Rgb(
            backdrop,
            backdrop.saturating_add(1),
            backdrop.saturating_add(3),
        );

        if ember < 0.08 {
            cell.set_symbol(" ");
            cell.set_fg(bg);
            cell.set_bg(bg);
            return;
        }

        let reveal = smoothstep(0.08, 0.58, ember);
        let glyph = if reveal > 0.58 {
            "•"
        } else if reveal > 0.16 {
            "·"
        } else {
            " "
        };

        let fg = if reveal > 0.9 {
            TerminalColor::Rgb(255, 238, 204)
        } else if reveal > 0.62 {
            TerminalColor::Rgb(255, 224, 176)
        } else if reveal > 0.32 {
            TerminalColor::Rgb(250, 179, 108)
        } else {
            TerminalColor::Rgb(169, 94, 63)
        };

        cell.set_symbol(glyph);
        cell.set_fg(fg);
        cell.set_bg(bg);
    }

    fn is_animated(&self) -> bool {
        true
    }

    fn animation_interval(&self) -> Duration {
        Duration::from_millis(33)
    }

    fn cache_key(&self) -> u64 {
        0xE5B3_0D1D
    }
}

impl CellEffect for HearthGlowEffect {
    fn apply(&self, cell: &mut EffectCell, ctx: &EffectContext) {
        let w = ctx.bounds.w.max(1) as f32;
        let h = ctx.bounds.h.max(1) as f32;
        let lx = ctx.x as f32 - ctx.bounds.x as f32 + 0.5;
        let ly = ctx.y as f32 - ctx.bounds.y as f32 + 0.5;

        let t = ctx.phase as f32 * 0.015;
        let x = lx / w.max(1.0);
        let y = ly / h.max(1.0);

        let column_motion = fireplace_columns(x, y, t);
        let base_glow = smoothstep(0.86, 1.0, y).powf(2.4);
        let flame_field = (column_motion.0 * 1.28 + base_glow * 0.05).clamp(0.0, 1.0);
        let hot_core = (column_motion.1 * 1.18).clamp(0.0, 1.0);
        let smoke = fireplace_smoke(x, y, t, w, h);
        let sparks = fireplace_sparks(x, y, t, w, h);

        let grain = hashed_noise(ctx.x as i32, ctx.y as i32) * 0.05;
        let backdrop =
            (12.0 + grain * 12.0 + smoke * 16.0 + flame_field * 12.0).clamp(0.0, 255.0) as u8;
        let bg = TerminalColor::Rgb(
            backdrop,
            backdrop.saturating_add(1),
            backdrop.saturating_add(3),
        );

        let heat = (flame_field + hot_core * 0.72 + sparks * 0.7).clamp(0.0, 1.0);
        if heat < 0.03 && smoke < 0.06 {
            cell.set_symbol(" ");
            cell.set_fg(bg);
            cell.set_bg(bg);
            return;
        }

        let upper_smoke = smoothstep(0.42, 0.08, y);
        let flame_presence = smoothstep(0.08, 0.52, heat);
        let sparkiness = smoothstep(0.62, 0.95, sparks);

        let glyph = if sparkiness > 0.88 {
            "*"
        } else if heat > 0.82 {
            "$"
        } else if heat > 0.56 {
            "s"
        } else if flame_presence > 0.22 || smoke > 0.10 || upper_smoke > 0.46 {
            "."
        } else {
            " "
        };

        let fg = if sparkiness > 0.78 || heat > 0.9 {
            TerminalColor::Rgb(255, 248, 226)
        } else if heat > 0.72 {
            TerminalColor::Rgb(255, 234, 160)
        } else if heat > 0.54 {
            TerminalColor::Rgb(255, 189, 92)
        } else if heat > 0.34 {
            TerminalColor::Rgb(240, 120, 52)
        } else if smoke > 0.12 {
            TerminalColor::Rgb(112, 105, 98)
        } else {
            TerminalColor::Rgb(160, 88, 48)
        };

        cell.set_symbol(glyph);
        cell.set_fg(fg);
        cell.set_bg(bg);
    }

    fn is_animated(&self) -> bool {
        true
    }

    fn animation_interval(&self) -> Duration {
        Duration::from_millis(33)
    }

    fn cache_key(&self) -> u64 {
        0x1A57_6E4B_u64
    }
}

struct DotFieldDemo;

impl Component for DotFieldDemo {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::TabChanged(event) => {
                ctx.state.active_variant = event.index.min(VARIANT_TITLES.len() - 1);
                Update::full()
            }
            Msg::NeonMouseMove(event) => {
                let phase = ctx.effect_phase();
                let target_x = event.local_x as f32 + 0.5;
                let target_y = event.local_y as f32 + 0.5;
                let existing = ctx.state.neon_pointer;
                let currently_acquired = existing.is_some_and(|pointer| {
                    let chase_done =
                        phase.saturating_sub(pointer.move_start_phase) as f32 >= NEON_CHASE_TICKS;
                    let still_active = (phase.saturating_sub(pointer.last_move_phase) as f32)
                        < NEON_IDLE_START_TICKS;
                    (pointer.acquired || chase_done) && still_active
                });
                let still_chasing = existing.is_some_and(|pointer| {
                    let still_active = (phase.saturating_sub(pointer.last_move_phase) as f32)
                        < NEON_IDLE_START_TICKS;
                    !currently_acquired && still_active
                });
                let (from_x, from_y, move_start_phase, acquired) = if currently_acquired {
                    // Once the initial chase reaches the cursor, stay glued under the moving
                    // mouse. After idle fallback starts, the next move reacquires smoothly.
                    (
                        target_x,
                        target_y,
                        phase.saturating_sub(NEON_CHASE_TICKS as u64),
                        true,
                    )
                } else if still_chasing {
                    let pointer = existing.expect("still_chasing implies pointer");
                    // Keep the original chase start while the target updates. This lets the
                    // accelerating chase complete instead of restarting every mouse move.
                    (
                        pointer.from_x,
                        pointer.from_y,
                        pointer.move_start_phase,
                        false,
                    )
                } else {
                    let (from_x, from_y) = current_neon_display_position(
                        existing,
                        phase,
                        event.target_w,
                        event.target_h,
                    );
                    (from_x, from_y, phase, false)
                };
                ctx.state.neon_pointer = Some(NeonPointer {
                    from_x,
                    from_y,
                    target_x,
                    target_y,
                    move_start_phase,
                    last_move_phase: phase,
                    acquired,
                });
                Update::full()
            }
            Msg::NeonHover(false) => {
                ctx.state.neon_pointer = None;
                Update::full()
            }
            Msg::NeonHover(true) => Update::none(),
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Left => {
                ctx.state.active_variant = if ctx.state.active_variant == 0 {
                    VARIANT_TITLES.len() - 1
                } else {
                    ctx.state.active_variant - 1
                };
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Right => {
                ctx.state.active_variant = (ctx.state.active_variant + 1) % VARIANT_TITLES.len();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('1') => {
                ctx.state.active_variant = 0;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('2') => {
                ctx.state.active_variant = 1;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('3') => {
                ctx.state.active_variant = 2;
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('4') => {
                ctx.state.active_variant = 3;
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let tabs = Tabs::new()
            .tab(VARIANT_TITLES[0])
            .tab(VARIANT_TITLES[1])
            .tab(VARIANT_TITLES[2])
            .tab(VARIANT_TITLES[3])
            .active(ctx.state.active_variant.min(VARIANT_TITLES.len() - 1))
            .active_style(Style::new().fg(Color::rgb(240, 246, 255)).bold())
            .style(Style::new().bg(Color::rgb(8, 10, 14)))
            .on_change(ctx.link().callback(Msg::TabChanged))
            .height(Length::Px(3));

        let effect: Element = match ctx.state.active_variant {
            0 => MouseRegion::new()
                .on_mouse_move(ctx.link().callback(Msg::NeonMouseMove))
                .on_hover_change(ctx.link().callback(Msg::NeonHover))
                .child(
                    EffectScope::new()
                        .custom_effect(NeonBloomEffect {
                            pointer: ctx.state.neon_pointer,
                        })
                        .child(
                            ZStack::new()
                                .style(Style::new().bg(Color::rgb(2, 4, 10)))
                                .child(Spacer::new()),
                        ),
                )
                .into(),
            1 => EffectScope::new()
                .custom_effect(VeiledLightsEffect)
                .child(
                    ZStack::new()
                        .style(Style::new().bg(Color::rgb(18, 19, 21)))
                        .child(Spacer::new()),
                )
                .into(),
            3 => EffectScope::new()
                .custom_effect(HearthGlowEffect)
                .child(
                    ZStack::new()
                        .style(Style::new().bg(Color::rgb(14, 10, 8)))
                        .child(Spacer::new()),
                )
                .into(),
            _ => EffectScope::new()
                .custom_effect(EmberDriftEffect)
                .child(
                    ZStack::new()
                        .style(Style::new().bg(Color::rgb(13, 9, 10)))
                        .child(Spacer::new()),
                )
                .into(),
        };

        VStack::new()
            .height(Length::Flex(1))
            .style(Style::new().bg(Color::rgb(8, 10, 14)))
            .child(tabs)
            .child(effect)
            .child(
                Text::new("←/→ or 1-4 switch variants · q/Esc quit")
                    .style(Style::new().fg(Color::rgb(106, 114, 128)).dim()),
            )
            .into()
    }
}

fn hashed_noise(x: i32, y: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA6B);
    n ^= n >> 16;
    n = n.wrapping_mul(0x7FEB_352D);
    n ^= n >> 15;
    n = n.wrapping_mul(0x846C_A68B);
    n ^= n >> 16;
    (n as f32) / (u32::MAX as f32)
}

fn blend_channel(base: u8, target: u8, opacity: f32) -> u8 {
    let opacity = opacity.clamp(0.0, 1.0);
    (base as f32 + (target as f32 - base as f32) * opacity).round() as u8
}

fn hashed_unit(a: i32, b: i32) -> f32 {
    hashed_noise(a.wrapping_mul(31), b.wrapping_mul(17))
}

fn autonomous_neon_position(width: u16, height: u16, phase: u64) -> (f32, f32) {
    let w = width.max(1) as f32;
    let h = height.max(1) as f32;
    let t = phase as f32 * 0.026;
    (
        w * (0.5 + 0.43 * (t * 0.73).sin()),
        h * (0.5 + 0.38 * (t * 0.51 + TAU * 0.18).cos()),
    )
}

fn current_neon_display_position(
    pointer: Option<NeonPointer>,
    phase: u64,
    width: u16,
    height: u16,
) -> (f32, f32) {
    let auto = autonomous_neon_position(width, height, phase);
    pointer.map_or(auto, |pointer| {
        let move_t = (phase.saturating_sub(pointer.move_start_phase) as f32 / NEON_CHASE_TICKS)
            .clamp(0.0, 1.0);
        let move_ease = neon_chase_ease(move_t);
        let chased = (
            pointer.from_x + (pointer.target_x - pointer.from_x) * move_ease,
            pointer.from_y + (pointer.target_y - pointer.from_y) * move_ease,
        );
        let idle = phase.saturating_sub(pointer.last_move_phase) as f32;
        let follow =
            (1.0 - smoothstep(NEON_IDLE_START_TICKS, NEON_IDLE_END_TICKS, idle)).clamp(0.0, 1.0);
        (
            auto.0 + (chased.0 - auto.0) * follow,
            auto.1 + (chased.1 - auto.1) * follow,
        )
    })
}

fn neon_chase_ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fireplace_columns(x: f32, y: f32, t: f32) -> (f32, f32) {
    let mut flame = 0.0;
    let mut core = 0.0;
    let above_floor = (0.985 - y).clamp(0.0, 1.0);

    for i in 0..30 {
        let idx = i as f32;
        let band = idx / 29.0;
        let jitter = hashed_unit(i, 11) - 0.5;
        let sway = (t * (1.15 + idx * 0.04) + idx * 1.31).sin() * 0.014;
        let center = (0.025 + band * 0.95 + jitter * 0.022 + sway).clamp(0.0, 1.0);

        let base_width = 0.038 + hashed_unit(i, 4) * 0.040;
        let peak = 0.10 + hashed_unit(i, 7) * 0.32;
        let height_wave = 0.075 * (t * 1.35 + idx * 0.57 + x * 3.2).sin();
        let tongue_height = (peak + height_wave).clamp(0.08, 0.44);

        let height_pos = (1.0 - above_floor / tongue_height).clamp(0.0, 1.0);
        let taper_width = base_width * (0.24 + height_pos * 1.08);
        let column = (1.0 - ((x - center).abs() / taper_width))
            .clamp(0.0, 1.0)
            .powf(1.55);
        let flicker = 0.72
            + 0.20 * (t * 3.1 + idx * 1.7 + y * 14.0).sin()
            + 0.08 * (t * 5.2 + idx * 0.9 + x * 18.0).cos();

        let base_anchor = height_pos.powf(0.38);
        let tip_lick = (height_pos * (1.0 - height_pos)).powf(0.28);
        let strength = 0.55 + hashed_unit(i, 13) * 0.70;
        let column_fire = column * flicker * strength * (base_anchor * 0.72 + tip_lick * 0.38);
        flame += column_fire;
        core += column.powf(1.25) * strength * height_pos.powf(1.9);
    }

    (
        (flame * 0.58).clamp(0.0, 1.0),
        (core * 0.50).clamp(0.0, 1.0),
    )
}

fn fireplace_smoke(x: f32, y: f32, t: f32, _w: f32, _h: f32) -> f32 {
    if y < 0.18 {
        return 0.0;
    }

    let mut smoke = 0.0;

    for i in 0..8 {
        let idx = i as f32;
        let life = (t * (0.26 + idx * 0.022) + idx * 0.61).fract();
        let rise = smoothstep(0.0, 0.96, life);
        let fade = 1.0 - smoothstep(0.40, 1.0, life);
        let base_x = 0.10 + hashed_unit(i, 21) * 0.80;
        let drift = (life * TAU * (0.8 + hashed_unit(i, 5) * 0.5)).sin() * 0.045;
        let plume_x = (base_x + drift + (t * 0.12 + idx * 0.9).sin() * 0.015).clamp(0.02, 0.98);
        let plume_y = (0.78 - rise * (0.58 + hashed_unit(i, 8) * 0.14)).clamp(0.02, 0.98);
        let dx = x - plume_x;
        let dy = (y - plume_y) * 1.65;
        let dist = (dx * dx + dy * dy).sqrt();
        let radius = 0.022 + hashed_unit(i, 14) * 0.026 + rise * 0.03;
        let puff = (1.0 - dist / radius).clamp(0.0, 1.0).powf(2.1);
        smoke += puff * fade * (0.16 + hashed_unit(i, 9) * 0.18);
    }

    let haze = smoothstep(0.62, 0.20, y) * (0.14 + 0.12 * (t * 0.42 + x * 2.8).sin().abs());
    (smoke * 0.78 + haze * 0.22).clamp(0.0, 1.0)
}

fn fireplace_sparks(x: f32, y: f32, t: f32, _w: f32, _h: f32) -> f32 {
    let mut sparks = 0.0;

    for i in 0..9 {
        let idx = i as f32;
        let cycle = t * (0.88 + idx * 0.05) + idx * 1.17;
        let life = cycle.fract();
        let rise = smoothstep(0.0, 0.86, life);
        let fade = 1.0 - smoothstep(0.56, 1.0, life);
        let start_x = 0.10 + hashed_unit(i, 31) * 0.80;
        let lift = 0.45 + hashed_unit(i, 17) * 0.18;
        let sway = (cycle * 3.2 + idx * 0.8).sin() * 0.03;
        let sx = (start_x + sway * rise).clamp(0.02, 0.98);
        let sy = (0.80 - rise * lift).clamp(0.04, 0.95);
        let dx = x - sx;
        let dy = (y - sy) * 1.55;
        let dist = (dx * dx + dy * dy).sqrt();
        let radius = 0.010 + hashed_unit(i, 23) * 0.010;
        let spark = (1.0 - dist / radius).clamp(0.0, 1.0).powf(2.4);
        sparks += spark * fade * (0.18 + hashed_unit(i, 19) * 0.22);
    }

    sparks.clamp(0.0, 1.0)
}

fn main() -> Result<()> {
    App::new()
        .title("EffectScope Dot Field")
        .mount(DotFieldDemo)
        .run()
}
