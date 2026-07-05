//! AsciiCanvas exporter – materialize [`AsciiCanvas`] art with effects into the
//! JSON format consumed by [`FrameSequence::from_json`].
//!
//! Run with: `cargo run --example ascii_canvas_exporter`
//!
//! Keys: `Tab` cycle mode • `b` cycle banner placement • `s` export frame
//!       `a` export animation • `p` preview exported JSON • Space pause/play
//!       Left/Right step frames • `q` / `Esc` quit

use std::io::Write;
use std::sync::Arc;

use tui_lipan::EffectAxis;
use tui_lipan::prelude::*;

#[path = "common/tui_lipan_art.rs"]
mod art;
use art::{
    BannerPlacement, TAB_CRT_AMBER, TAB_HERO, TAB_LABELS, TAB_NEON, brand_gradient_scope_effects,
};

// ─── Materializer ──────────────────────────────────────────────────────────

/// Apply a list of [`VisualEffect`]s to every cell in `buffer` and return a
/// new [`AsciiCanvasBuffer`] with the resulting colours baked in.
fn materialize_effects(
    buffer: &AsciiCanvasBuffer,
    effects: &[VisualEffect],
    phase: u64,
) -> AsciiCanvasBuffer {
    let w = buffer.width();
    let h = buffer.height();
    let mut out = AsciiCanvasBuffer::new(w, h);

    for y in 0..h {
        for x in 0..w {
            let idx = (y as usize) * (w as usize) + (x as usize);
            let cell = &buffer.cells()[idx];
            let (mut fg, mut bg) = (cell.style.fg, cell.style.bg);

            for effect in effects {
                (fg, bg) = apply_effect(fg, bg, x, y, phase, effect, w, h);
            }

            out.set(
                x,
                y,
                AsciiCell {
                    ch: cell.ch,
                    style: Style {
                        fg,
                        bg,
                        ..cell.style
                    },
                },
            );
        }
    }

    out
}

#[allow(clippy::too_many_arguments)]
fn apply_effect(
    fg: Option<Paint>,
    bg: Option<Paint>,
    x: u16,
    y: u16,
    phase: u64,
    effect: &VisualEffect,
    buf_w: u16,
    buf_h: u16,
) -> (Option<Paint>, Option<Paint>) {
    // Strip clipping wrapper and check bounds / mask.
    if let VisualEffect::Clipped {
        bounds,
        mask,
        inner,
    } = effect
    {
        let inside = bounds.is_none_or(|r| {
            let rx = r.x as u16;
            let ry = r.y as u16;
            x >= rx && x < rx + r.w && y >= ry && y < ry + r.h
        });
        let masked = mask.as_ref().is_none_or(|m| m.test_region_local(x, y));
        if !inside || !masked {
            return (fg, bg);
        }
        return apply_effect(fg, bg, x, y, phase, inner, buf_w, buf_h);
    }

    match effect {
        VisualEffect::Gradient { .. } => {
            let (r, g, b, alpha) = gradient_wave_rgb_alpha(x, y, phase, buf_w, buf_h, effect);
            let c = Color::Rgb(r, g, b);
            (
                fg.map(|f| map_paint_color(f, |color| color.blend_toward(c, alpha))),
                bg.map(|b| map_paint_color(b, |color| color.blend_toward(c, alpha))),
            )
        }
        VisualEffect::RainbowWave { .. } => {
            let (r, g, b, alpha) = rainbow_wave_color(x as i16, y as i16, phase, effect);
            let c = Color::Rgb(r, g, b);
            (
                fg.map(|f| map_paint_color(f, |color| color.blend_toward(c, alpha))),
                bg.map(|b| map_paint_color(b, |color| color.blend_toward(c, alpha))),
            )
        }
        VisualEffect::Ripple {
            origin,
            radius,
            ring_width,
            tint,
            strength,
        } => {
            let Some((radius, strength_multiplier)) = resolve_ripple_radius(radius, phase) else {
                return (fg, bg);
            };
            let (cx, cy) = origin.resolve(buf_w, buf_h);
            let local_x = x as f32 + 0.5;
            let local_y = y as f32 + 0.5;
            let dist = ((local_x - cx).powi(2) + ((local_y - cy) * 2.0).powi(2)).sqrt();
            let rw = ring_width.max(f32::EPSILON);
            let falloff = (1.0 - ((dist - radius).abs() / rw)).max(0.0);
            if falloff > 0.0 {
                let alpha = falloff * strength * strength_multiplier;
                (
                    fg.map(|f| map_paint_color(f, |color| color.blend_toward(*tint, alpha))),
                    bg.map(|b| map_paint_color(b, |color| color.blend_toward(*tint, alpha))),
                )
            } else {
                (fg, bg)
            }
        }
        VisualEffect::Monochrome { strength } => {
            let apply = |c: Color| -> Color {
                let (r, g, b) = c.to_rgb().unwrap_or((0, 0, 0));
                let gray = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                c.blend_toward(Color::Rgb(gray, gray, gray), *strength)
            };
            (
                fg.map(|paint| map_paint_color(paint, apply)),
                bg.map(|paint| map_paint_color(paint, apply)),
            )
        }
        VisualEffect::Scanlines { strength, spacing } => {
            let spacing = (*spacing).max(1) as i16;
            if (y as i16).rem_euclid(spacing) == 0 {
                (
                    fg.map(|paint| map_paint_color(paint, |c| c.dim_by(*strength))),
                    bg.map(|paint| map_paint_color(paint, |c| c.dim_by(*strength))),
                )
            } else {
                (fg, bg)
            }
        }
        VisualEffect::ColorTransform { fg: fg_t, bg: bg_t } => {
            let new_fg = fg_t.and_then(|t| fg.map(|f| t.apply_paint_with_backdrop(f, bg)));
            let new_bg = bg_t.and_then(|t| bg.map(|b| t.apply_paint_with_backdrop(b, None)));
            (new_fg.or(fg), new_bg.or(bg))
        }
        VisualEffect::ContrastPolicy(_)
        | VisualEffect::RetroCrt { .. }
        | VisualEffect::PaletteQuantize { .. }
        | VisualEffect::Custom(_)
        | VisualEffect::Clipped { .. } => {
            // Not materialised in this example (Clipped handled above).
            (fg, bg)
        }
    }
}

fn map_paint_color(paint: Paint, f: impl FnOnce(Color) -> Color) -> Paint {
    match paint {
        Paint::Solid(color) => Paint::Solid(f(color)),
        Paint::Alpha { color, alpha } => Paint::Alpha {
            color: f(color),
            alpha,
        },
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

// Replicates `gradient_sample_t` from the ratatui backend.
#[allow(clippy::too_many_arguments)]
fn gradient_sample_t(
    x: u16,
    y: u16,
    phase: u64,
    buf_w: u16,
    buf_h: u16,
    frequency: f32,
    speed: f32,
    axis: EffectAxis,
) -> f64 {
    let bw = buf_w.max(1) as f32;
    let bh = buf_h.max(1) as f32;
    let lx = ((x as f32 + 0.5) / bw).clamp(0.0, 1.0);
    let ly = ((y as f32 + 0.5) / bh).clamp(0.0, 1.0);
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
    x: u16,
    y: u16,
    phase: u64,
    buf_w: u16,
    buf_h: u16,
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
    let t = gradient_sample_t(x, y, phase, buf_w, buf_h, *frequency, *speed, *axis);
    let c = gradient.color_at(t);
    let (r, g, b) = c.to_rgb().unwrap_or((255, 255, 255));
    (r, g, b, blend.clamp(0.0, 1.0))
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
    let theta = pos * *frequency * 0.08 + phase as f32 * *speed * 0.04;
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

// ─── Buffer builder ────────────────────────────────────────────────────────

fn cell_for_char(ch: char) -> AsciiCell {
    if ch.is_whitespace() {
        AsciiCell::new(ch)
    } else {
        AsciiCell::new(ch).style(Style::new().fg(Color::White))
    }
}

fn lines_to_buffer(lines: &[&str]) -> AsciiCanvasBuffer {
    let width = lines.iter().map(|l| l.chars().count()).max().unwrap_or(0) as u16;
    let height = lines.len() as u16;
    let mut buffer = AsciiCanvasBuffer::new(width, height);
    for (y, line) in lines.iter().enumerate() {
        for (x, ch) in line.chars().enumerate() {
            buffer.set(x as u16, y as u16, cell_for_char(ch));
        }
    }
    buffer
}

/// Compose logo + optional banner into a single buffer.
fn build_composed_buffer(placement: BannerPlacement) -> AsciiCanvasBuffer {
    let logo_lines: Vec<&str> = art::LOGO.lines().collect();
    let text_lines: Vec<&str> = art::TEXT.lines().collect();

    let logo_w = logo_lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let logo_h = logo_lines.len() as u16;
    let text_w = text_lines
        .iter()
        .map(|l| l.chars().count())
        .max()
        .unwrap_or(0) as u16;
    let text_h = text_lines.len() as u16;

    match placement {
        BannerPlacement::Off => lines_to_buffer(&logo_lines),
        BannerPlacement::Above | BannerPlacement::Below => {
            let w = logo_w.max(text_w);
            let h = logo_h + text_h + 1;
            let mut buf = AsciiCanvasBuffer::new(w, h);
            let (top_lines, bottom_lines) = if matches!(placement, BannerPlacement::Above) {
                (&text_lines, &logo_lines)
            } else {
                (&logo_lines, &text_lines)
            };
            for (y, line) in top_lines.iter().enumerate() {
                let start_x = (w as usize).saturating_sub(line.chars().count()) / 2;
                for (x, ch) in line.chars().enumerate() {
                    buf.set((start_x + x) as u16, y as u16, cell_for_char(ch));
                }
            }
            let bottom_y = top_lines.len() as u16 + 1;
            for (y, line) in bottom_lines.iter().enumerate() {
                let start_x = (w as usize).saturating_sub(line.chars().count()) / 2;
                for (x, ch) in line.chars().enumerate() {
                    buf.set((start_x + x) as u16, bottom_y + y as u16, cell_for_char(ch));
                }
            }
            buf
        }
        BannerPlacement::Left | BannerPlacement::Right => {
            let w = logo_w + text_w + 1;
            let h = logo_h.max(text_h);
            let mut buf = AsciiCanvasBuffer::new(w, h);
            let (left_lines, right_lines) = if matches!(placement, BannerPlacement::Left) {
                (&text_lines, &logo_lines)
            } else {
                (&logo_lines, &text_lines)
            };
            let left_h = left_lines.len() as u16;
            let left_y0 = (h - left_h) / 2;
            for (y, line) in left_lines.iter().enumerate() {
                for (x, ch) in line.chars().enumerate() {
                    buf.set(x as u16, left_y0 + y as u16, cell_for_char(ch));
                }
            }
            let right_x0 = left_lines
                .iter()
                .map(|l| l.chars().count())
                .max()
                .unwrap_or(0) as u16
                + 1;
            let right_h = right_lines.len() as u16;
            let right_y0 = (h - right_h) / 2;
            for (y, line) in right_lines.iter().enumerate() {
                for (x, ch) in line.chars().enumerate() {
                    buf.set(right_x0 + x as u16, right_y0 + y as u16, cell_for_char(ch));
                }
            }
            buf
        }
    }
}

// ─── JSON serializer ───────────────────────────────────────────────────────

fn color_to_hex(c: Color) -> String {
    let (r, g, b) = c.to_rgb().unwrap_or((0, 0, 0));
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn escape_json(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn export_buffer_to_json(buffer: &AsciiCanvasBuffer, title: &str, frame_rate: u16) -> String {
    export_buffers_to_json(&[(title.to_string(), buffer.clone())], frame_rate)
}

fn export_buffers_to_json(frames: &[(String, AsciiCanvasBuffer)], frame_rate: u16) -> String {
    let first = frames
        .first()
        .map(|(_, buffer)| buffer)
        .expect("export requires at least one frame");
    let w = first.width();
    let h = first.height();

    let mut out = String::new();
    out.push_str("{\n");
    out.push_str("  \"canvas\": {\n");
    out.push_str(&format!("    \"width\": {w},\n"));
    out.push_str(&format!("    \"height\": {h},\n"));
    out.push_str("    \"backgroundColor\": \"#000000\"\n");
    out.push_str("  },\n");
    out.push_str("  \"typography\": {\n");
    out.push_str("    \"fontSize\": 16,\n");
    out.push_str("    \"characterSpacing\": 1,\n");
    out.push_str("    \"lineSpacing\": 1\n");
    out.push_str("  },\n");
    out.push_str("  \"animation\": {\n");
    out.push_str(&format!("    \"frameRate\": {frame_rate},\n"));
    out.push_str("    \"looping\": true,\n");
    out.push_str("    \"currentFrame\": 0\n");
    out.push_str("  },\n");
    let duration_ms = 1000.0 / frame_rate as f64;
    out.push_str("  \"frames\": [\n");

    for (frame_idx, (title, buffer)) in frames.iter().enumerate() {
        out.push_str("    {\n");
        out.push_str(&format!("      \"title\": \"{}\",\n", escape_json(title)));
        out.push_str(&format!("      \"duration\": {duration_ms},\n"));
        out.push_str("      \"content\": [\n");

        let mut content_string_lines = Vec::new();
        for y in 0..h {
            let mut line = String::new();
            for x in 0..w {
                let idx = (y as usize) * (w as usize) + (x as usize);
                line.push(buffer.cells()[idx].ch);
            }
            content_string_lines.push(line.clone());
            let escaped = escape_json(&line);
            if y + 1 < h {
                out.push_str(&format!("        \"{escaped}\",\n"));
            } else {
                out.push_str(&format!("        \"{escaped}\"\n"));
            }
        }
        out.push_str("      ],\n");

        let content_string = escape_json(&content_string_lines.join("\n"));
        out.push_str(&format!("      \"contentString\": \"{content_string}\",\n"));

        out.push_str("      \"colors\": {\n");

        let mut fg_entries = Vec::new();
        let mut bg_entries = Vec::new();
        for y in 0..h {
            for x in 0..w {
                let idx = (y as usize) * (w as usize) + (x as usize);
                let cell = &buffer.cells()[idx];
                if let Some(fg) = cell.style.fg {
                    fg_entries.push(format!("\"{x},{y}\":\"{}\"", color_to_hex(fg.color())));
                }
                if let Some(bg) = cell.style.bg {
                    bg_entries.push(format!("\"{x},{y}\":\"{}\"", color_to_hex(bg.color())));
                }
            }
        }

        let fg_inner = fg_entries.join(",").replace('"', "\\\"");
        let bg_inner = bg_entries.join(",").replace('"', "\\\"");
        out.push_str(&format!("        \"foreground\": \"{{{fg_inner}}}\",\n"));
        out.push_str(&format!("        \"background\": \"{{{bg_inner}}}\"\n"));
        out.push_str("      }\n");
        if frame_idx + 1 < frames.len() {
            out.push_str("    },\n");
        } else {
            out.push_str("    }\n");
        }
    }
    out.push_str("  ]\n");
    out.push_str("}\n");
    out
}

// ─── App ───────────────────────────────────────────────────────────────────

struct ExporterApp;

struct State {
    tab: usize,
    banner_placement: BannerPlacement,
    show_preview: bool,
    preview_sequence: Option<Arc<FrameSequence>>,
    preview_frame: usize,
    preview_paused: bool,
    preview_playback_gen: u64,
    status: Option<String>,
    phase: u64,
}

#[derive(Clone, Debug)]
enum Msg {
    TabChanged(TabsEvent),
    CycleBanner,
    ExportFrame,
    ExportAnimation,
    TogglePreview,
    TogglePreviewPlayback,
    PrevPreviewFrame,
    NextPreviewFrame,
    PreviewTick(u64),
    ClearStatus,
}

fn export_dir() -> std::path::PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("target")
        .join("tui-lipan-export")
}

fn export_path() -> std::path::PathBuf {
    export_dir().join("exported_logo.json")
}

fn preview_len(state: &State) -> usize {
    state.preview_sequence.as_ref().map_or(0, |seq| seq.len())
}

fn schedule_preview_tick(ctx: &Context<ExporterApp>) -> Update {
    if ctx.state.preview_paused || !ctx.state.show_preview || preview_len(&ctx.state) <= 1 {
        return Update::full();
    }

    let generation = ctx.state.preview_playback_gen;
    let delay = ctx
        .state
        .preview_sequence
        .as_ref()
        .and_then(|seq| seq.get(ctx.state.preview_frame))
        .and_then(|frame| frame.duration_ms)
        .unwrap_or(83)
        .max(16);

    Update::with_command(ctx.link().command_keyed(
        "ascii-exporter-preview-tick",
        TaskPolicy::LatestOnly,
        move |link| {
            std::thread::sleep(std::time::Duration::from_millis(delay));
            link.send(Msg::PreviewTick(generation));
        },
    ))
}

fn mode_effects(tab: usize, _phase: u64) -> Vec<VisualEffect> {
    match tab {
        TAB_NEON => vec![
            VisualEffect::Monochrome { strength: 0.35 },
            VisualEffect::RainbowWave {
                blend: 0.82,
                frequency: 1.2,
                speed: 1.05,
                axis: EffectAxis::Diagonal,
            },
        ],
        TAB_CRT_AMBER => vec![VisualEffect::tint(Color::Rgb(255, 176, 0), 0.35)],
        _ => Vec::new(),
    }
}

fn materialized_logo_buffer(
    tab: usize,
    placement: BannerPlacement,
    phase: u64,
) -> AsciiCanvasBuffer {
    let base = build_composed_buffer(placement);
    let mut effects = brand_gradient_scope_effects(tab);
    effects.extend(mode_effects(tab, phase));
    materialize_effects(&base, &effects, phase)
}

fn controls_hint(state: &State) -> String {
    let base = "b banner • s screenshot • a animation • p preview • q quit";
    if state.show_preview {
        let frame_text = state.preview_sequence.as_ref().map_or_else(
            || "preview not loaded".to_string(),
            |seq| {
                format!(
                    "frame {}/{} ({})",
                    state.preview_frame + 1,
                    seq.len(),
                    if state.preview_paused {
                        "paused"
                    } else {
                        "playing"
                    }
                )
            },
        );
        format!("{base} • {frame_text} • Space play/pause • ←/→ step")
    } else {
        format!("{base} • preview supports multi-frame navigation")
    }
}

impl Component for ExporterApp {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State {
            tab: TAB_HERO,
            banner_placement: BannerPlacement::default(),
            show_preview: false,
            preview_sequence: None,
            preview_frame: 0,
            preview_paused: true,
            preview_playback_gen: 0,
            status: Some("Ready – use the help line below for controls".to_string()),
            phase: 0,
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::TabChanged(ev) => {
                if ev.index != ctx.state.tab {
                    ctx.state.tab = ev.index;
                    ctx.state.show_preview = false;
                    ctx.state.preview_paused = true;
                    return Update::full();
                }
                Update::none()
            }
            Msg::CycleBanner => {
                ctx.state.banner_placement = ctx.state.banner_placement.next();
                ctx.state.show_preview = false;
                ctx.state.preview_paused = true;
                Update::full()
            }
            Msg::ExportFrame => {
                let tab = ctx.state.tab;
                let placement = ctx.state.banner_placement;
                let phase = ctx.state.phase;

                let materialized = materialized_logo_buffer(tab, placement, phase);

                let json = export_buffer_to_json(
                    &materialized,
                    TAB_LABELS.get(tab).copied().unwrap_or("?"),
                    12,
                );

                let dir = export_dir();
                let path = export_path();
                let status = match std::fs::create_dir_all(&dir)
                    .and_then(|_| std::fs::File::create(&path))
                    .and_then(|mut f| f.write_all(json.as_bytes()))
                {
                    Ok(()) => format!("Exported to {}", path.display()),
                    Err(e) => format!("Export failed: {e}"),
                };
                ctx.state.status = Some(status);
                ctx.toast().push(Toast::new(format!(
                    "Saved screenshot to {}",
                    path.display()
                )));
                Update::with_command(ctx.link().command_keyed(
                    "clear-status",
                    TaskPolicy::LatestOnly,
                    |link| {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        link.send(Msg::ClearStatus);
                    },
                ))
            }
            Msg::ExportAnimation => {
                let tab = ctx.state.tab;
                let placement = ctx.state.banner_placement;
                let frames: Vec<(String, AsciiCanvasBuffer)> = (0..24)
                    .map(|i| {
                        let phase = ctx.state.phase.wrapping_add(i as u64 * 3);
                        (
                            format!("{} frame {i}", TAB_LABELS.get(tab).copied().unwrap_or("?")),
                            materialized_logo_buffer(tab, placement, phase),
                        )
                    })
                    .collect();
                let json = export_buffers_to_json(&frames, 12);

                let dir = export_dir();
                let path = export_path();
                let status = match std::fs::create_dir_all(&dir)
                    .and_then(|_| std::fs::File::create(&path))
                    .and_then(|mut f| f.write_all(json.as_bytes()))
                {
                    Ok(()) => format!("Exported 24-frame animation to {}", path.display()),
                    Err(e) => format!("Animation export failed: {e}"),
                };
                ctx.state.status = Some(status);
                ctx.toast()
                    .push(Toast::new(format!("Saved animation to {}", path.display())));
                Update::with_command(ctx.link().command_keyed(
                    "clear-status",
                    TaskPolicy::LatestOnly,
                    |link| {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        link.send(Msg::ClearStatus);
                    },
                ))
            }
            Msg::TogglePreview => {
                let path = export_path();
                if !path.exists() {
                    ctx.state.status = Some("No export to preview – press s first".to_string());
                    ctx.state.show_preview = false;
                } else {
                    match std::fs::read_to_string(&path).map(|s| FrameSequence::from_json(&s)) {
                        Ok(Ok(seq)) => {
                            let len = seq.len();
                            ctx.state.preview_sequence = Some(Arc::new(seq));
                            ctx.state.show_preview = true;
                            ctx.state.preview_frame = 0;
                            ctx.state.preview_paused = len <= 1;
                            ctx.state.preview_playback_gen =
                                ctx.state.preview_playback_gen.wrapping_add(1);
                            ctx.state.status = Some(format!(
                                "Previewing {} ({} frame{})",
                                path.display(),
                                len,
                                if len == 1 { "" } else { "s" }
                            ));
                            if len > 1 {
                                return schedule_preview_tick(ctx);
                            }
                        }
                        Ok(Err(e)) => {
                            ctx.state.status = Some(format!("Parse error: {e}"));
                            ctx.state.show_preview = false;
                        }
                        Err(e) => {
                            ctx.state.status = Some(format!("Read error: {e}"));
                            ctx.state.show_preview = false;
                        }
                    }
                }
                Update::full()
            }
            Msg::TogglePreviewPlayback => {
                let len = preview_len(&ctx.state);
                if ctx.state.show_preview && len > 1 {
                    ctx.state.preview_paused = !ctx.state.preview_paused;
                    ctx.state.preview_playback_gen = ctx.state.preview_playback_gen.wrapping_add(1);
                    ctx.state.status = Some(if ctx.state.preview_paused {
                        "Preview paused – Left/Right step frames".to_string()
                    } else {
                        "Preview playing – Space pauses".to_string()
                    });
                    if ctx.state.preview_paused {
                        Update::full()
                    } else {
                        schedule_preview_tick(ctx)
                    }
                } else {
                    Update::none()
                }
            }
            Msg::PrevPreviewFrame => {
                let len = preview_len(&ctx.state);
                if ctx.state.show_preview && len > 0 {
                    ctx.state.preview_paused = true;
                    ctx.state.preview_playback_gen = ctx.state.preview_playback_gen.wrapping_add(1);
                    ctx.state.preview_frame = if ctx.state.preview_frame == 0 {
                        len - 1
                    } else {
                        ctx.state.preview_frame - 1
                    };
                    Update::full()
                } else {
                    Update::none()
                }
            }
            Msg::NextPreviewFrame => {
                let len = preview_len(&ctx.state);
                if ctx.state.show_preview && len > 0 {
                    ctx.state.preview_paused = true;
                    ctx.state.preview_playback_gen = ctx.state.preview_playback_gen.wrapping_add(1);
                    ctx.state.preview_frame = (ctx.state.preview_frame + 1) % len;
                    Update::full()
                } else {
                    Update::none()
                }
            }
            Msg::PreviewTick(generation) => {
                if generation != ctx.state.preview_playback_gen
                    || ctx.state.preview_paused
                    || !ctx.state.show_preview
                {
                    return Update::none();
                }
                let len = preview_len(&ctx.state);
                if len <= 1 {
                    return Update::none();
                }
                ctx.state.preview_frame = (ctx.state.preview_frame + 1) % len;
                schedule_preview_tick(ctx)
            }
            Msg::ClearStatus => {
                ctx.state.status = None;
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
                ctx.link().send(Msg::CycleBanner);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('s') | KeyCode::Char('S') => {
                ctx.link().send(Msg::ExportFrame);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                ctx.link().send(Msg::ExportAnimation);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char('p') | KeyCode::Char('P') => {
                ctx.link().send(Msg::TogglePreview);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Char(' ') => {
                ctx.link().send(Msg::TogglePreviewPlayback);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Left => {
                ctx.link().send(Msg::PrevPreviewFrame);
                KeyUpdate::handled(Update::full())
            }
            KeyCode::Right => {
                ctx.link().send(Msg::NextPreviewFrame);
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let tab = ctx.state.tab;
        let placement = ctx.state.banner_placement;

        let tabs: Vec<Tab> = TAB_LABELS.iter().copied().map(Tab::new).collect();

        let body: Element = if ctx.state.show_preview {
            if let Some(ref seq) = ctx.state.preview_sequence {
                AsciiCanvas::from_sequence(seq.clone())
                    .frame(ctx.state.preview_frame)
                    .style(Style::new().fg(Color::White))
                    .into()
            } else {
                Text::new("No preview available").into()
            }
        } else {
            let canvas: Element = AsciiCanvas::new(art::LOGO.lines()).into();
            let text_canvas: Element = AsciiCanvas::new(art::TEXT.lines()).into();

            let logo = match placement {
                BannerPlacement::Off => canvas,
                BannerPlacement::Above => VStack::new()
                    .align(Align::Center)
                    .gap(1)
                    .child(text_canvas)
                    .child(canvas)
                    .into(),
                BannerPlacement::Below => VStack::new()
                    .align(Align::Center)
                    .gap(1)
                    .child(canvas)
                    .child(text_canvas)
                    .into(),
                BannerPlacement::Left => HStack::new()
                    .align(Align::Center)
                    .gap(1)
                    .child(text_canvas)
                    .child(canvas)
                    .into(),
                BannerPlacement::Right => HStack::new()
                    .align(Align::Center)
                    .gap(1)
                    .child(canvas)
                    .child(text_canvas)
                    .into(),
            };

            let mut effects = brand_gradient_scope_effects(tab);
            effects.extend(mode_effects(tab, ctx.state.phase));

            EffectScope::new().effects(effects).child(logo).into()
        };

        let status_text = ctx.state.status.clone().unwrap_or_default();

        let title = format!(
            "AsciiCanvas Exporter – {} | banner {} | tab/b/s/a/p/q",
            TAB_LABELS.get(tab).copied().unwrap_or("?"),
            placement.label(),
        );
        let hint = controls_hint(&ctx.state);

        Frame::new()
            .title(title)
            .status_left(hint)
            .status_right(status_text)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(
                        Tabs::new()
                            .tabs(tabs)
                            .active(tab.min(TAB_LABELS.len().saturating_sub(1)))
                            .focusable(false)
                            .on_change(ctx.link().callback(Msg::TabChanged)),
                    )
                    .child(Center::new().child(body)),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("ascii-canvas-exporter")
        .mount(ExporterApp)
        .run()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_export_and_parse() {
        let buffer = build_composed_buffer(BannerPlacement::Off);
        let effects = brand_gradient_scope_effects(TAB_HERO);
        let materialized = materialize_effects(&buffer, &effects, 0);
        let json = export_buffer_to_json(&materialized, "Hero", 12);
        let seq = FrameSequence::from_json(&json).expect("parse should succeed");
        assert_eq!(seq.width(), materialized.width());
        assert_eq!(seq.height(), materialized.height());
        assert!(!seq.is_empty());
    }

    #[test]
    fn neon_mode_exports_with_colors() {
        let buffer = build_composed_buffer(BannerPlacement::Off);
        let mut effects = brand_gradient_scope_effects(TAB_NEON);
        effects.extend_from_slice(&[
            VisualEffect::Monochrome { strength: 0.35 },
            VisualEffect::RainbowWave {
                blend: 0.82,
                frequency: 1.2,
                speed: 1.05,
                axis: EffectAxis::Diagonal,
            },
        ]);
        let materialized = materialize_effects(&buffer, &effects, 0);
        let json = export_buffer_to_json(&materialized, "Neon", 12);
        let seq = FrameSequence::from_json(&json).expect("parse should succeed");
        let frame = seq.get(0).expect("one frame");
        // At least some cells should have gained colours.
        let has_fg = frame.buffer.cells().iter().any(|c| c.style.fg.is_some());
        assert!(
            has_fg,
            "expected some foreground colours after materialisation"
        );
    }

    #[test]
    fn multi_frame_export_roundtrips() {
        let frames: Vec<(String, AsciiCanvasBuffer)> = (0..4)
            .map(|i| {
                (
                    format!("Hero frame {i}"),
                    materialized_logo_buffer(TAB_HERO, BannerPlacement::Off, i * 3),
                )
            })
            .collect();
        let json = export_buffers_to_json(&frames, 12);
        let seq = FrameSequence::from_json(&json).expect("parse should succeed");
        assert_eq!(seq.len(), 4);
        assert_eq!(seq.width(), frames[0].1.width());
        assert_eq!(seq.height(), frames[0].1.height());
    }
}
