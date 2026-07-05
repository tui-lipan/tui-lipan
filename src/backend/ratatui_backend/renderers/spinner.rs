use std::cell::RefCell;
use std::rc::Rc;

use ratatui::buffer::Buffer;
use ratatui::style::Style as RtStyle;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::backend::ratatui_backend::common::{draw_cell_clipped, to_ratatui_style};
use crate::backend::ratatui_backend::glyph_paint_cache::{PaintGlyphCaches, SpinnerSimpleGlyphKey};
use crate::style::{Color, Rect, Style};
use crate::utils::gradient::ColorGradient;
use crate::widgets::SpinnerStyle;

#[inline]
fn spinner_draw_clip(rect: Rect, clip_rect: Option<Rect>) -> Option<Rect> {
    let draw_clip = match clip_rect {
        None => rect,
        Some(c) => rect.intersection(&c),
    };
    (!draw_clip.is_empty()).then_some(draw_clip)
}

fn truncate_spinner_label_for_width(lbl: &str, remaining: usize) -> String {
    if remaining == 0 {
        return String::new();
    }
    let label_w = UnicodeWidthStr::width(lbl);
    if label_w <= remaining {
        return lbl.to_string();
    }
    let mut truncated = String::new();
    let mut w = 0usize;
    for ch in lbl.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw + 1 > remaining {
            break;
        }
        truncated.push(ch);
        w += cw;
    }
    truncated.push('…');
    truncated
}

#[inline]
fn draw_gap_cells(
    buf: &mut Buffer,
    mut cx: i32,
    y: i32,
    gap: u16,
    clip: Option<Rect>,
    gap_rt: RtStyle,
) -> i32 {
    for _ in 0..gap {
        draw_cell_clipped(buf, cx, y, " ", gap_rt, clip);
        cx += 1;
    }
    cx
}

#[inline]
fn advance_painted_x(
    buf: &mut Buffer,
    cx: i32,
    y: i32,
    symbol: &str,
    rt: RtStyle,
    clip: Option<Rect>,
) -> i32 {
    draw_glyph_run_rt(buf, cx, y, symbol, rt, clip)
}

#[inline]
fn draw_glyph_run_rt(
    buf: &mut Buffer,
    mut cx: i32,
    y: i32,
    text: &str,
    rt: RtStyle,
    clip: Option<Rect>,
) -> i32 {
    for g in text.graphemes(true) {
        let cw = UnicodeWidthStr::width(g) as i32;
        if cw == 0 {
            continue;
        }
        draw_cell_clipped(buf, cx, y, g, rt, clip);
        cx += cw;
    }
    cx
}

fn draw_truncated_spinner_label(
    buf: &mut Buffer,
    cx: i32,
    y: i32,
    lbl: &str,
    remaining: usize,
    label_style: Style,
    clip: Option<Rect>,
) {
    if remaining == 0 || lbl.is_empty() {
        return;
    }
    let text = truncate_spinner_label_for_width(lbl, remaining);
    let rt = to_ratatui_style(label_style);
    draw_glyph_run_rt(buf, cx, y, &text, rt, clip);
}

pub(crate) struct SpinnerRenderCtx<'a> {
    pub frame: usize,
    pub label: Option<&'a str>,
    pub gap: u16,
    pub style: Style,
    pub label_style: Style,
    pub clip_rect: Option<Rect>,
    pub paint_glyph_caches: Option<Rc<RefCell<PaintGlyphCaches>>>,
}

pub(crate) fn render_spinner(
    f: &mut ratatui::Frame<'_>,
    spinner_style: SpinnerStyle,
    rect: Rect,
    _rrect: ratatui::layout::Rect,
    ctx: SpinnerRenderCtx<'_>,
) {
    let SpinnerRenderCtx {
        frame,
        label,
        gap,
        style,
        label_style,
        clip_rect,
        paint_glyph_caches,
    } = ctx;
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    if matches!(spinner_style, SpinnerStyle::OpenCode) {
        render_opencode_spinner(
            f,
            rect,
            SpinnerLabelRenderCtx {
                frame,
                gap,
                style,
                label,
                label_style,
                clip_rect,
            },
        );
        return;
    }

    if matches!(spinner_style, SpinnerStyle::Lightsaber) {
        render_lightsaber_spinner(
            f,
            rect,
            SpinnerLabelRenderCtx {
                frame,
                gap,
                style,
                label,
                label_style,
                clip_rect,
            },
        );
        return;
    }

    let frames = spinner_style.frames();
    let idx = frame % frames.len();
    let symbol = frames[idx];
    let symbol_w = UnicodeWidthStr::width(symbol);

    let glyph_rt_style = paint_glyph_caches.as_ref().map_or_else(
        || to_ratatui_style(style),
        |rc| {
            let key = SpinnerSimpleGlyphKey {
                spinner_style,
                frame_mod: idx as u16,
                lipan_style: style,
            };
            *rc.borrow_mut()
                .spinner_rat_style
                .entry(key)
                .or_insert_with(|| to_ratatui_style(style))
        },
    );

    let Some(draw_clip) = spinner_draw_clip(rect, clip_rect) else {
        return;
    };

    let buf = f.buffer_mut();
    let clip_opt = Some(draw_clip);
    let y_row = rect.y as i32;
    let gap_rt = to_ratatui_style(Style::default());
    let mut cx = rect.x as i32;
    cx = advance_painted_x(buf, cx, y_row, symbol, glyph_rt_style, clip_opt);

    if let Some(lbl) = label.filter(|s| !s.is_empty()) {
        cx = draw_gap_cells(buf, cx, y_row, gap, clip_opt, gap_rt);
        let remaining = (rect.w as usize)
            .saturating_sub(symbol_w)
            .saturating_sub(gap as usize);
        draw_truncated_spinner_label(buf, cx, y_row, lbl, remaining, label_style, clip_opt);
    }
}

fn tint_color(base: Color, factor: f32, shadow_mix: f32) -> Color {
    let (r, g, b) = base.to_rgb().unwrap_or((0, 0, 0));
    let shadow = Color::Rgb(
        ((r as f32) * shadow_mix).round() as u8,
        ((g as f32) * shadow_mix).round() as u8,
        ((b as f32) * shadow_mix).round() as u8,
    );
    let glow = Color::Rgb(
        (r as f32 + (255.0 - r as f32) * 0.88).round() as u8,
        (g as f32 + (255.0 - g as f32) * 0.88).round() as u8,
        (b as f32 + (255.0 - b as f32) * 0.88).round() as u8,
    );

    let t = if factor <= 1.0 {
        (factor.clamp(0.0, 1.0) as f64) * 0.5
    } else {
        0.5 + ((factor - 1.0).clamp(0.0, 1.0) as f64) * 0.5
    };

    ColorGradient::new(shadow, glow)
        .with_center(base)
        .color_at(t)
}

const OPENCODE_SPINNER_WIDTH: isize = 8;
const OPENCODE_CYCLE_LEN: usize = 44;
const OPENCODE_FRAME_ZERO_PREROLL_TICKS: usize = 2;
const OPENCODE_TRAIL_SYMBOL_LEN: f32 = 5.0;
const OPENCODE_GLOW_LEN: isize = 3;
const OPENCODE_GLOW_FACTORS: [f32; 3] = [0.70, 0.48, 0.32];
const OPENCODE_HEAD_PEAK: f32 = 1.12;
const OPENCODE_TRAIL_TAU: f32 = 2.23;
const OPENCODE_SHADOW_MIX: f32 = 0.12;

const OPENCODE_TRACK_COOLDOWN: [f32; 16] = [
    0.52, 0.44, 0.37, 0.32, 0.28, 0.25, 0.23, 0.21, 0.20, 0.19, 0.18, 0.17, 0.16, 0.16, 0.15, 0.15,
];
const OPENCODE_TRACK_COLD: f32 = 0.14;

/// Tick at which the head traverses each cell during the rightward sweep.
/// Rightward covers 8 cells in 7 frames by jumping pos 2→4 at tick 3;
/// cells 3 and 4 are both traversed at tick 3. Tuned to match upstream
/// OpenCode timing at SpinnerSpeed::Fast (50ms/tick, 2200ms cycle).
const OPENCODE_RIGHT_VISIT_TICK: [usize; 8] = [0, 1, 2, 3, 3, 4, 5, 6];
/// Tick at which the head traverses each cell during the leftward sweep
/// (symmetric jump pos 5→3 at tick 17).
const OPENCODE_LEFT_VISIT_TICK: [usize; 8] = [20, 19, 18, 17, 17, 16, 15, 14];
/// Fractional ts offset per cell for jumped-over positions. The head passes
/// through these cells mid-frame, so they're effectively 0.5 ticks "older"
/// than the cell the head actually landed on at that tick. This breaks the
/// color tie that would otherwise make the jumped cell and landing cell
/// render identically.
const OPENCODE_RIGHT_TS_OFFSET: [f32; 8] = [0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0];
const OPENCODE_LEFT_TS_OFFSET: [f32; 8] = [0.0, 0.0, 0.0, 0.0, 0.5, 0.0, 0.0, 0.0];

fn opencode_head_pos(tick: usize) -> Option<(isize, bool)> {
    match tick {
        0 => Some((0, true)),
        1 => Some((1, true)),
        2 => Some((2, true)),
        3 => Some((4, true)),
        4 => Some((5, true)),
        5 => Some((6, true)),
        6 => Some((7, true)),
        7..=13 => Some((8, true)),
        14 => Some((7, false)),
        15 => Some((6, false)),
        16 => Some((5, false)),
        17 => Some((3, false)),
        18 => Some((2, false)),
        19 => Some((1, false)),
        20 => Some((0, false)),
        _ => None,
    }
}

fn opencode_cycle_tick(frame: usize) -> usize {
    (frame + OPENCODE_CYCLE_LEN - OPENCODE_FRAME_ZERO_PREROLL_TICKS) % OPENCODE_CYCLE_LEN
}

fn opencode_time_since(cell: isize, now: usize) -> Option<f32> {
    if !(0..OPENCODE_SPINNER_WIDTH).contains(&cell) {
        return None;
    }
    let c = cell as usize;
    let d_right = ((now + OPENCODE_CYCLE_LEN - OPENCODE_RIGHT_VISIT_TICK[c]) % OPENCODE_CYCLE_LEN)
        as f32
        + OPENCODE_RIGHT_TS_OFFSET[c];
    let d_left = ((now + OPENCODE_CYCLE_LEN - OPENCODE_LEFT_VISIT_TICK[c]) % OPENCODE_CYCLE_LEN)
        as f32
        + OPENCODE_LEFT_TS_OFFSET[c];
    Some(d_right.min(d_left))
}

fn opencode_trail_factor(ts: f32) -> f32 {
    // Piecewise: head sits above base (HEAD_PEAK) and ramps down to 1.0 at
    // ts=1 so the first tail cell matches the spinner style exactly. From
    // ts>=1 the tail follows exponential decay anchored so ts=5 hits the
    // dim last-tail color without brightening the mid tail.
    if ts <= 1.0 {
        OPENCODE_HEAD_PEAK + (1.0 - OPENCODE_HEAD_PEAK) * ts
    } else {
        (-(ts - 1.0) / OPENCODE_TRAIL_TAU).exp()
    }
}

/// Cooldown table stretched 2x over ts so the track keeps its heat color
/// roughly twice as long. Linear interpolation also means fractional ts
/// values (from jumped-over cells) produce distinct colors instead of
/// collapsing to the same slot via floor.
fn opencode_track_factor(ts_after_tail: f32) -> f32 {
    let t = ts_after_tail * 0.5;
    let idx = t.floor().max(0.0) as usize;
    let frac = t - (idx as f32);
    let a = OPENCODE_TRACK_COOLDOWN
        .get(idx)
        .copied()
        .unwrap_or(OPENCODE_TRACK_COLD);
    let b = OPENCODE_TRACK_COOLDOWN
        .get(idx + 1)
        .copied()
        .unwrap_or(OPENCODE_TRACK_COLD);
    let factor = a + (b - a) * frac;
    // Ember boost: the ⬝ cell that flipped from ■ this tick gets 25% extra
    // brightness to sell the afterglow before settling into the cooldown curve.
    if ts_after_tail < 1.0 {
        factor * 1.25
    } else {
        factor
    }
}

fn opencode_forward_glow(cell: isize, now: usize) -> f32 {
    if let Some((head_pos, moving_right)) = opencode_head_pos(now) {
        let ahead = if moving_right {
            cell - head_pos
        } else {
            head_pos - cell
        };
        if (1..=OPENCODE_GLOW_LEN).contains(&ahead) {
            return OPENCODE_GLOW_FACTORS[(ahead - 1) as usize];
        }
    }
    0.0
}

struct SpinnerLabelRenderCtx<'a> {
    frame: usize,
    gap: u16,
    style: Style,
    label: Option<&'a str>,
    label_style: Style,
    clip_rect: Option<Rect>,
}

fn render_opencode_spinner(f: &mut ratatui::Frame<'_>, rect: Rect, ctx: SpinnerLabelRenderCtx<'_>) {
    let SpinnerLabelRenderCtx {
        frame,
        gap,
        style,
        label,
        label_style,
        clip_rect,
    } = ctx;
    let Some(draw_clip) = spinner_draw_clip(rect, clip_rect) else {
        return;
    };

    let buf = f.buffer_mut();
    let clip_opt = Some(draw_clip);
    let y_row = rect.y as i32;
    let gap_rt = to_ratatui_style(Style::default());
    let mut cx = rect.x as i32;
    let base_color = style.fg.map(|paint| paint.color()).unwrap_or(Color::Cyan);
    let t = opencode_cycle_tick(frame);

    for i in 0..OPENCODE_SPINNER_WIDTH {
        let (symbol, base_factor) = match opencode_time_since(i, t) {
            Some(ts) if ts < OPENCODE_TRAIL_SYMBOL_LEN => ("■", opencode_trail_factor(ts)),
            Some(ts) => ("⬝", opencode_track_factor(ts - OPENCODE_TRAIL_SYMBOL_LEN)),
            None => ("⬝", OPENCODE_TRACK_COLD),
        };
        let factor = base_factor.max(opencode_forward_glow(i, t));
        let rt =
            to_ratatui_style(Style::new().fg(tint_color(base_color, factor, OPENCODE_SHADOW_MIX)));
        cx = advance_painted_x(buf, cx, y_row, symbol, rt, clip_opt);
    }

    if let Some(lbl) = label.filter(|s| !s.is_empty()) {
        cx = draw_gap_cells(buf, cx, y_row, gap, clip_opt, gap_rt);

        let remaining = (rect.w as usize)
            .saturating_sub(OPENCODE_SPINNER_WIDTH as usize)
            .saturating_sub(gap as usize);
        draw_truncated_spinner_label(buf, cx, y_row, lbl, remaining, label_style, clip_opt);
    }
}

fn render_lightsaber_spinner(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    ctx: SpinnerLabelRenderCtx<'_>,
) {
    let SpinnerLabelRenderCtx {
        frame,
        gap,
        style,
        label,
        label_style,
        clip_rect,
    } = ctx;
    let Some(draw_clip) = spinner_draw_clip(rect, clip_rect) else {
        return;
    };

    const HILT: &str = "⁌==⁍";
    const HILT_WIDTH: usize = 4;
    const BLADE_LEN: usize = 12;
    const CYCLE_LEN: usize = 78;

    let base_color = style
        .fg
        .map(|paint| paint.color())
        .unwrap_or(Color::Rgb(0, 200, 255));

    let t = frame % CYCLE_LEN;

    enum Phase {
        Idle,
        PreSpark(usize),
        Ignite(usize),
        Flicker(usize),
        Retract(usize),
        PostSpark(usize),
    }
    let phase = match t {
        0..=4 => Phase::Idle,
        5..=6 => Phase::PreSpark(t - 5),
        7..=12 => Phase::Ignite(t - 7),
        13..=62 => Phase::Flicker(t - 13),
        63..=70 => Phase::Retract(t - 63),
        71..=72 => Phase::PostSpark(t - 71),
        _ => Phase::Idle,
    };

    let buf = f.buffer_mut();
    let clip_opt = Some(draw_clip);
    let y_row = rect.y as i32;
    let gap_rt = to_ratatui_style(Style::default());
    let mut cx = rect.x as i32;

    let hilt_style = to_ratatui_style(Style::new().fg(Color::Rgb(180, 180, 190)));
    cx = draw_glyph_run_rt(buf, cx, y_row, HILT, hilt_style, clip_opt);

    let flicker_offset = |tick: usize, pos: usize| -> f32 {
        let patterns = [0.0_f32, 0.04, -0.03, 0.05, -0.04, 0.03, -0.05, 0.02];
        patterns[(tick * 3 + pos * 7) % patterns.len()]
    };

    let spark_chars = ['✦', '✧', '·', '∗'];
    let mut scratch = [0u8; 4];

    match phase {
        Phase::Idle => {
            for _ in 0..BLADE_LEN {
                cx = advance_painted_x(buf, cx, y_row, " ", gap_rt, clip_opt);
            }
        }
        Phase::PreSpark(tick) => {
            for i in 0..BLADE_LEN {
                let show_spark = i == 0 || (tick == 1 && i == 1);
                if show_spark {
                    let brightness = 0.6 + (tick as f32 * 0.2);
                    let ch = spark_chars[(tick + i) % spark_chars.len()];
                    let rt =
                        to_ratatui_style(Style::new().fg(tint_color(base_color, brightness, 0.16)));
                    let s = ch.encode_utf8(&mut scratch[..]);
                    cx = advance_painted_x(buf, cx, y_row, s, rt, clip_opt);
                } else {
                    cx = advance_painted_x(buf, cx, y_row, " ", gap_rt, clip_opt);
                }
            }
        }
        Phase::Ignite(tick) => {
            let lit_count = ((tick + 1) * 2).min(BLADE_LEN);
            let spark_pos = if lit_count <= BLADE_LEN {
                Some(lit_count.saturating_sub(1))
            } else {
                None
            };

            for i in 0..BLADE_LEN {
                if i < lit_count {
                    let dist_from_tip = lit_count.saturating_sub(i + 1);
                    let base_brightness = if spark_pos == Some(i) {
                        1.25
                    } else {
                        0.88 + (dist_from_tip as f32 * 0.02).min(0.12)
                    };
                    let brightness = base_brightness + flicker_offset(tick, i);
                    let rt =
                        to_ratatui_style(Style::new().fg(tint_color(base_color, brightness, 0.16)));
                    cx = advance_painted_x(buf, cx, y_row, "═", rt, clip_opt);
                } else {
                    cx = advance_painted_x(buf, cx, y_row, " ", gap_rt, clip_opt);
                }
            }
        }
        Phase::Flicker(tick) => {
            for i in 0..BLADE_LEN {
                let pos_variance = ((i * 7 + tick * 3) % 11) as f32 / 55.0;
                let brightness = 0.93 + flicker_offset(tick, i) + pos_variance;
                let rt =
                    to_ratatui_style(Style::new().fg(tint_color(base_color, brightness, 0.16)));
                cx = advance_painted_x(buf, cx, y_row, "═", rt, clip_opt);
            }
        }
        Phase::Retract(tick) => {
            let retract_per_frame = BLADE_LEN.div_ceil(8);
            let visible_count = BLADE_LEN.saturating_sub((tick + 1) * retract_per_frame);
            let spark_pos = if visible_count > 0 {
                Some(visible_count.saturating_sub(1))
            } else {
                None
            };

            for i in 0..BLADE_LEN {
                if i < visible_count {
                    let progress = tick as f32 / 8.0;
                    let fade = 1.0 - (i as f32 / BLADE_LEN as f32) * 0.25 * progress;
                    let base_brightness = if spark_pos == Some(i) {
                        fade * 1.2
                    } else {
                        fade * 0.88
                    };
                    let brightness = base_brightness + flicker_offset(tick, i);
                    let rt =
                        to_ratatui_style(Style::new().fg(tint_color(base_color, brightness, 0.16)));
                    cx = advance_painted_x(buf, cx, y_row, "═", rt, clip_opt);
                } else {
                    cx = advance_painted_x(buf, cx, y_row, " ", gap_rt, clip_opt);
                }
            }
        }
        Phase::PostSpark(tick) => {
            for i in 0..BLADE_LEN {
                let show_spark = i == 0 || (tick == 0 && i == 1);
                if show_spark {
                    let brightness = 0.5 - (tick as f32 * 0.2);
                    let ch = spark_chars[(tick + i * 2) % spark_chars.len()];
                    let rt =
                        to_ratatui_style(Style::new().fg(tint_color(base_color, brightness, 0.16)));
                    let s = ch.encode_utf8(&mut scratch[..]);
                    cx = advance_painted_x(buf, cx, y_row, s, rt, clip_opt);
                } else {
                    cx = advance_painted_x(buf, cx, y_row, " ", gap_rt, clip_opt);
                }
            }
        }
    }

    if let Some(lbl) = label.filter(|s| !s.is_empty()) {
        cx = draw_gap_cells(buf, cx, y_row, gap, clip_opt, gap_rt);

        let total_width = HILT_WIDTH + BLADE_LEN;
        let remaining = (rect.w as usize)
            .saturating_sub(total_width)
            .saturating_sub(gap as usize);
        draw_truncated_spinner_label(buf, cx, y_row, lbl, remaining, label_style, clip_opt);
    }
}

#[cfg(test)]
mod tests {
    use super::{OPENCODE_CYCLE_LEN, opencode_cycle_tick, opencode_head_pos};

    #[test]
    fn opencode_frame_zero_prerolls_before_left_edge_entry() {
        assert_eq!(opencode_cycle_tick(0), OPENCODE_CYCLE_LEN - 2);
        assert_eq!(opencode_cycle_tick(1), OPENCODE_CYCLE_LEN - 1);
        assert_eq!(opencode_head_pos(opencode_cycle_tick(0)), None);
        assert_eq!(opencode_head_pos(opencode_cycle_tick(1)), None);
        assert_eq!(opencode_head_pos(opencode_cycle_tick(2)), Some((0, true)));
    }
}
