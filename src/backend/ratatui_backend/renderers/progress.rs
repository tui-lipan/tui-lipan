use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use ratatui::buffer::Buffer;
use ratatui::style::Style as RtStyle;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::common::{
    current_render_terminal_bg, draw_cell_clipped, finalize_style, from_ratatui_color,
    style_backdrop, style_paints_bg, to_ratatui_style,
};
use crate::backend::ratatui_backend::glyph_paint_cache::{
    PaintGlyphCaches, ProgressTrackCacheKey, fingerprint_progress_zones,
};
use crate::backend::ratatui_backend::render::RenderState;
use crate::core::node::NodeId;
use crate::style::resolve::{
    resolve_accent_style, resolve_base_style, resolve_force_accent_style, resolve_muted_style,
};
use crate::style::{Color, Padding, Paint, Rect, Style, ThemeRole, resolve_slot};
use crate::utils::color_contrast::{
    readable_text_color, readable_text_color_apca, readable_text_color_black_or_white,
};
use crate::utils::gradient::ColorGradient;
use crate::widgets::{ProgressStyle, ProgressTextPosition, ProgressZone};

fn readable_text_for_policy(
    preferred: Option<Paint>,
    bg: Paint,
    contrast_policy: ContrastPolicy,
) -> Color {
    let terminal_bg = current_render_terminal_bg()
        .map(from_ratatui_color)
        .unwrap_or(Color::Reset);
    let bg_resolved = bg.flatten_over(terminal_bg);
    let preferred = preferred.map(|p| p.flatten_over(bg_resolved));
    match contrast_policy {
        ContrastPolicy::Off | ContrastPolicy::Wcag => readable_text_color(preferred, bg_resolved),
        ContrastPolicy::BlackOrWhite => readable_text_color_black_or_white(preferred, bg_resolved),
        ContrastPolicy::Apca => readable_text_color_apca(preferred, bg_resolved),
    }
}

/// Geometry describing how progress maps onto discrete track cells (before overlays).
struct ProgressTrackGeom {
    full_filled: usize,
    partial_char: Option<char>,
    has_partial: bool,
    partial_idx_inverted: usize,
    partial_slot: Option<u8>,
}

fn analyze_progress_track(
    bar_width: usize,
    progress: f64,
    progress_style: ProgressStyle,
) -> ProgressTrackGeom {
    let exact_filled = progress * bar_width as f64;
    let full_filled = exact_filled.floor() as usize;
    let partial_progress = exact_filled - full_filled as f64;
    let partial_chars = progress_style.partial_chars();
    let has_partial = partial_chars.is_some() && partial_progress > 0.0 && full_filled < bar_width;

    let partial_slot = partial_chars.and_then(|chars| {
        if chars.is_empty() || !has_partial {
            return None;
        }
        Some(((partial_progress * chars.len() as f64).floor() as usize).min(chars.len() - 1) as u8)
    });

    let partial_char =
        partial_chars.and_then(|chars| partial_slot.map(|slot| chars[usize::from(slot)]));

    let filled_start = bar_width.saturating_sub(full_filled);
    let partial_idx_inverted = if has_partial {
        filled_start.saturating_sub(1)
    } else {
        usize::MAX
    };

    ProgressTrackGeom {
        full_filled,
        partial_char,
        has_partial,
        partial_idx_inverted,
        partial_slot,
    }
}

struct ProgressTrackCellsCtx<'a> {
    filled_style: Style,
    empty_style: Style,
    filled_gradient: Option<ColorGradient>,
    inverted: bool,
    zones: &'a [ProgressZone],
    block_empty_bg_dim: f32,
    is_block_mode: bool,
}

fn compute_progress_track_cells(
    bar_width: usize,
    geom: &ProgressTrackGeom,
    progress_style: ProgressStyle,
    ctx: ProgressTrackCellsCtx<'_>,
) -> Vec<(char, Style)> {
    let ProgressTrackCellsCtx {
        filled_style,
        empty_style,
        filled_gradient,
        inverted,
        zones,
        block_empty_bg_dim,
        is_block_mode,
    } = ctx;
    let filled_char = progress_style.filled_char();
    let empty_char = progress_style.empty_char();
    let mut out = Vec::with_capacity(bar_width);

    for i in 0..bar_width {
        let (mut ch, mut style, is_filled_cell, is_partial) = if inverted {
            if i == geom.partial_idx_inverted {
                (
                    geom.partial_char.unwrap_or(filled_char),
                    filled_style,
                    true,
                    true,
                )
            } else if i >= bar_width.saturating_sub(geom.full_filled) {
                (filled_char, filled_style, true, false)
            } else {
                (empty_char, empty_style, false, false)
            }
        } else if i < geom.full_filled {
            (filled_char, filled_style, true, false)
        } else if geom.has_partial && i == geom.full_filled {
            (
                geom.partial_char.unwrap_or(filled_char),
                filled_style,
                true,
                true,
            )
        } else {
            (empty_char, empty_style, false, false)
        };

        style = if !is_filled_cell {
            style
        } else if let Some(gradient) = filled_gradient {
            let t = if bar_width <= 1 {
                1.0
            } else {
                i as f64 / (bar_width - 1) as f64
            };
            if is_block_mode {
                style.patch(Style::new().bg(gradient.color_at(t)))
            } else {
                style.patch(Style::new().fg(gradient.color_at(t)))
            }
        } else {
            style
        };

        let t = if bar_width <= 1 {
            1.0
        } else {
            i as f64 / (bar_width - 1) as f64
        };
        if let Some(zone) = zone_for(zones, t) {
            style = style.patch(zone.style);
            if is_filled_cell
                && !is_partial
                && let Some(zone_symbol) = zone.symbol
            {
                ch = zone_symbol;
            }
        }

        if is_block_mode {
            style = with_block_track_bg(style, is_filled_cell, block_empty_bg_dim);
            ch = ' ';
        }

        out.push((ch, style));
    }

    out
}

pub(crate) struct ProgressBarRenderCtx<'a> {
    pub show_percentage: bool,
    pub percentage_position: ProgressTextPosition,
    pub label: Option<&'a str>,
    pub label_position: ProgressTextPosition,
    pub filled_style: Style,
    pub filled_gradient: Option<ColorGradient>,
    pub empty_style: Style,
    pub label_style: Style,
    pub style: Style,
    pub hover_style: Style,
    pub target: Option<f64>,
    pub target_style: Style,
    pub target_symbol: char,
    pub zones: &'a [ProgressZone],
    pub block_empty_bg_dim: f32,
    pub padding: Padding,
    pub inverted: bool,
    pub is_hovered: bool,
    pub contrast_policy: ContrastPolicy,
    pub clip_rect: Option<Rect>,
    pub paint_glyph_caches: Option<Rc<RefCell<PaintGlyphCaches>>>,
}

pub(crate) fn render_progress_bar(
    f: &mut ratatui::Frame<'_>,
    progress: f64,
    progress_style: ProgressStyle,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    ctx: ProgressBarRenderCtx<'_>,
) {
    let ProgressBarRenderCtx {
        show_percentage,
        percentage_position,
        label,
        label_position,
        filled_style,
        filled_gradient,
        empty_style,
        label_style,
        style,
        hover_style,
        target,
        target_style,
        target_symbol,
        zones,
        block_empty_bg_dim,
        padding,
        inverted,
        is_hovered,
        contrast_policy,
        clip_rect,
        paint_glyph_caches,
    } = ctx;
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let mut base_style = style;
    let mut filled_style = filled_style;
    let mut empty_style = empty_style;
    let mut label_style = label_style;
    let mut target_style = target_style;
    if is_hovered {
        base_style = base_style.patch(hover_style);
        filled_style = filled_style.patch(hover_style);
        empty_style = empty_style.patch(hover_style);
        label_style = label_style.patch(hover_style);
        target_style = target_style.patch(hover_style);
    }

    let base_style_final = finalize_style(base_style, None, contrast_policy);
    let base_backdrop = style_backdrop(base_style_final);

    if style_paints_bg(base_style_final) {
        let bg = ratatui::widgets::Block::default().style(to_ratatui_style(base_style_final));
        f.render_widget(bg, rrect);
    }

    let inner = rect.inset(padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    let is_block_mode = matches!(progress_style, ProgressStyle::Block);
    let mut pct_position = normalized_text_position(percentage_position, is_block_mode);
    let mut label_position = normalized_text_position(label_position, is_block_mode);
    if inner.h < 2
        && matches!(
            pct_position,
            ProgressTextPosition::Above | ProgressTextPosition::Below
        )
    {
        pct_position = ProgressTextPosition::Right;
    }
    if inner.h < 2
        && matches!(
            label_position,
            ProgressTextPosition::Above | ProgressTextPosition::Below
        )
    {
        label_position = ProgressTextPosition::Right;
    }

    let percentage_text = if show_percentage {
        // Always 4 chars wide ("100%", " 50%", "  0%") so bar track width stays constant.
        Some(format!("{:>3}%", (progress * 100.0).round() as u8))
    } else {
        None
    };
    let display_label = label.map(str::to_owned);

    let mut bar_width = inner.w as usize;
    if let Some(text) = percentage_text.as_deref()
        && matches!(
            pct_position,
            ProgressTextPosition::Left | ProgressTextPosition::Right
        )
    {
        bar_width = bar_width.saturating_sub(UnicodeWidthStr::width(text).saturating_add(1));
    }
    if let Some(lbl) = display_label.as_deref()
        && matches!(
            label_position,
            ProgressTextPosition::Left | ProgressTextPosition::Right
        )
    {
        bar_width = bar_width.saturating_sub(UnicodeWidthStr::width(lbl).saturating_add(1));
    }

    if bar_width == 0 {
        return;
    }

    let geom = analyze_progress_track(bar_width, progress, progress_style);

    let mut track_chars: Vec<char> = Vec::with_capacity(bar_width);
    let mut track_styles: Vec<Style> = Vec::with_capacity(bar_width);

    if let Some(rc) = paint_glyph_caches.as_ref() {
        let key = ProgressTrackCacheKey {
            bar_width: bar_width as u16,
            full_filled_cells: geom.full_filled.min(bar_width).min(u16::MAX as usize) as u16,
            partial_slot: geom.partial_slot,
            inverted,
            progress_style,
            filled_style,
            empty_style,
            filled_gradient,
            zones_fingerprint: fingerprint_progress_zones(zones),
            block_empty_bg_dim_bits: block_empty_bg_dim.to_bits(),
            is_block_mode,
        };
        let arc_track = {
            let mut caches = rc.borrow_mut();
            caches
                .progress_track
                .entry(key)
                .or_insert_with(|| {
                    Arc::from(
                        compute_progress_track_cells(
                            bar_width,
                            &geom,
                            progress_style,
                            ProgressTrackCellsCtx {
                                filled_style,
                                empty_style,
                                filled_gradient,
                                inverted,
                                zones,
                                block_empty_bg_dim,
                                is_block_mode,
                            },
                        )
                        .into_boxed_slice(),
                    )
                })
                .clone()
        };
        for &(ch, st) in arc_track.iter() {
            track_chars.push(ch);
            track_styles.push(st);
        }
    } else {
        let fill_track = compute_progress_track_cells(
            bar_width,
            &geom,
            progress_style,
            ProgressTrackCellsCtx {
                filled_style,
                empty_style,
                filled_gradient,
                inverted,
                zones,
                block_empty_bg_dim,
                is_block_mode,
            },
        );
        for (ch, style) in fill_track {
            track_chars.push(ch);
            track_styles.push(style);
        }
    }

    let middle_text = match (
        percentage_text
            .as_deref()
            .filter(|_| matches!(pct_position, ProgressTextPosition::Middle)),
        display_label
            .as_deref()
            .filter(|_| matches!(label_position, ProgressTextPosition::Middle)),
    ) {
        (Some(pct), Some(label)) => Some(format!("{pct} {label}")),
        (Some(pct), None) => Some(pct.to_string()),
        (None, Some(label)) => Some(label.to_string()),
        (None, None) => None,
    };

    if let Some(text) = middle_text.as_deref() {
        overlay_centered_text(
            &mut track_chars,
            &mut track_styles,
            text,
            label_style,
            contrast_policy,
        );
    }

    if let Some(target) = target {
        let idx = ((target.clamp(0.0, 1.0) * bar_width.saturating_sub(1) as f64).round() as usize)
            .min(bar_width.saturating_sub(1));
        if idx < track_chars.len() {
            if is_block_mode {
                let cell_style = track_styles[idx];
                let cell_bg = cell_style.bg.or(cell_style.fg);
                let target_non_color = Style {
                    fg: None,
                    bg: None,
                    ..target_style
                };

                let mut marker_style = cell_style.patch(target_non_color);
                marker_style.bg = cell_bg;
                marker_style.fg = cell_bg
                    .map(|bg| {
                        Paint::from(readable_text_for_policy(
                            target_style.fg,
                            bg,
                            contrast_policy,
                        ))
                    })
                    .or(target_style.fg);
                marker_style.dim = target_style.dim.or(Some(false));

                track_styles[idx] = marker_style;
                track_chars[idx] = target_symbol;
            } else {
                track_chars[idx] = target_symbol;
                track_styles[idx] = track_styles[idx].patch(base_style.patch(target_style));
            }
        }
    }

    let above_text = merged_position_text(
        percentage_text.as_deref(),
        pct_position,
        display_label.as_deref(),
        label_position,
        ProgressTextPosition::Above,
    );
    let below_text = merged_position_text(
        percentage_text.as_deref(),
        pct_position,
        display_label.as_deref(),
        label_position,
        ProgressTextPosition::Below,
    );

    let draw_clip = match clip_rect {
        None => inner,
        Some(c) => inner.intersection(&c),
    };
    if draw_clip.w == 0 || draw_clip.h == 0 {
        return;
    }

    let clip_opt = Some(draw_clip);
    let label_rt = to_ratatui_style(finalize_style(label_style, None, contrast_policy));
    let space_rt = RtStyle::default();

    let buf = f.buffer_mut();
    let mut current_y = inner.y as i32;

    if let Some(text) = above_text.as_deref() {
        draw_centered_text_row_clip(buf, draw_clip, current_y, text, label_rt, clip_opt);
        current_y += 1;
    }

    let track_y = current_y;
    let mut cx = inner.x as i32;

    if matches!(pct_position, ProgressTextPosition::Left)
        && let Some(text) = percentage_text.as_deref()
    {
        draw_grapheme_row(buf, cx, track_y, text, label_rt, clip_opt);
        cx += UnicodeWidthStr::width(text) as i32;
        draw_cell_clipped(buf, cx, track_y, " ", space_rt, clip_opt);
        cx += 1;
    }
    if matches!(label_position, ProgressTextPosition::Left)
        && let Some(lbl) = display_label.as_deref()
    {
        draw_grapheme_row(buf, cx, track_y, lbl, label_rt, clip_opt);
        cx += UnicodeWidthStr::width(lbl) as i32;
        draw_cell_clipped(buf, cx, track_y, " ", space_rt, clip_opt);
        cx += 1;
    }

    let mut sym_buf = [0u8; 4];
    for (&ch, &st) in track_chars.iter().zip(track_styles.iter()) {
        let final_style = finalize_style(st, base_backdrop, contrast_policy);
        let rst = to_ratatui_style(final_style);
        let s = ch.encode_utf8(&mut sym_buf[..]);
        draw_cell_clipped(buf, cx, track_y, s, rst, clip_opt);
        cx += UnicodeWidthChar::width(ch).unwrap_or(1).max(1) as i32;
    }

    if matches!(pct_position, ProgressTextPosition::Right)
        && let Some(text) = percentage_text.as_deref()
    {
        draw_cell_clipped(buf, cx, track_y, " ", space_rt, clip_opt);
        cx += 1;
        draw_grapheme_row(buf, cx, track_y, text, label_rt, clip_opt);
        cx += UnicodeWidthStr::width(text) as i32;
    }
    if matches!(label_position, ProgressTextPosition::Right)
        && let Some(lbl) = display_label.as_deref()
    {
        draw_cell_clipped(buf, cx, track_y, " ", space_rt, clip_opt);
        cx += 1;
        draw_grapheme_row(buf, cx, track_y, lbl, label_rt, clip_opt);
    }

    current_y += 1;
    if let Some(text) = below_text.as_deref() {
        draw_centered_text_row_clip(buf, draw_clip, current_y, text, label_rt, clip_opt);
    }
}

fn draw_grapheme_row(
    buf: &mut Buffer,
    mut x: i32,
    y: i32,
    text: &str,
    rt: RtStyle,
    clip: Option<Rect>,
) {
    for g in text.graphemes(true) {
        let cw = UnicodeWidthStr::width(g) as i32;
        if cw == 0 {
            continue;
        }
        draw_cell_clipped(buf, x, y, g, rt, clip);
        x += cw;
    }
}

fn draw_centered_text_row_clip(
    buf: &mut Buffer,
    viewport: Rect,
    y: i32,
    text: &str,
    rt: RtStyle,
    clip: Option<Rect>,
) {
    let tw = UnicodeWidthStr::width(text) as i32;
    let start_x = viewport.x as i32 + ((viewport.w as i32).saturating_sub(tw)).max(0) / 2;
    draw_grapheme_row(buf, start_x, y, text, rt, clip);
}

fn zone_for(zones: &[ProgressZone], t: f64) -> Option<&ProgressZone> {
    if zones.is_empty() {
        return None;
    }

    for zone in zones {
        if t <= zone.upto {
            return Some(zone);
        }
    }

    zones.last()
}

fn normalized_text_position(
    position: ProgressTextPosition,
    is_block_mode: bool,
) -> ProgressTextPosition {
    if is_block_mode || !matches!(position, ProgressTextPosition::Middle) {
        position
    } else {
        ProgressTextPosition::Right
    }
}

fn merged_position_text(
    percentage_text: Option<&str>,
    percentage_position: ProgressTextPosition,
    label: Option<&str>,
    label_position: ProgressTextPosition,
    target_position: ProgressTextPosition,
) -> Option<String> {
    match (
        percentage_text.filter(|_| percentage_position == target_position),
        label.filter(|_| label_position == target_position),
    ) {
        (Some(pct), Some(label)) => Some(format!("{pct} {label}")),
        (Some(pct), None) => Some(pct.to_string()),
        (None, Some(label)) => Some(label.to_string()),
        (None, None) => None,
    }
}

fn with_block_track_bg(mut style: Style, is_filled_cell: bool, empty_dim: f32) -> Style {
    let source = style.bg.or(style.fg);
    let bg = if is_filled_cell {
        source
    } else {
        source.map(|paint| dim_paint_by(paint, empty_dim))
    };

    style.bg = bg;
    style
}

fn overlay_centered_text(
    chars: &mut [char],
    styles: &mut [Style],
    text: &str,
    label_style: Style,
    contrast_policy: ContrastPolicy,
) {
    if chars.is_empty() || text.is_empty() || chars.len() != styles.len() {
        return;
    }

    let text_chars: Vec<char> = text.chars().collect();
    let len = text_chars.len().min(chars.len());
    let start = chars.len().saturating_sub(len) / 2;

    for (i, ch) in text_chars.into_iter().take(len).enumerate() {
        let idx = start + i;
        chars[idx] = ch;

        let mut overlay_style = styles[idx].patch(label_style);
        overlay_style.bg = styles[idx].bg;
        if let Some(bg) = overlay_style.bg {
            overlay_style.fg = Some(Paint::from(readable_text_for_policy(
                label_style.fg,
                bg,
                contrast_policy,
            )));
        } else if overlay_style.fg.is_none() {
            overlay_style.fg = Some(Color::White.into());
        }
        overlay_style.dim = Some(false);
        styles[idx] = overlay_style;
    }
}

fn dim_paint_by(paint: Paint, amount: f32) -> Paint {
    match paint {
        Paint::Solid(color) => Paint::Solid(color.dim_by(amount)),
        Paint::Alpha { color, alpha } => Paint::from_color_alpha_u8(color.dim_by(amount), alpha),
    }
}

pub(crate) fn render_progress_bar_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::widgets::internal::ProgressNode,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) {
    let is_focused = Some(node_id) == state.ctx.focused;
    let is_hovered = Some(node_id) == state.ctx.hovered;
    let contrast_policy = state.ctx.contrast_policy;
    let theme = state.ctx.tree.node(node_id).active_theme();
    let focus_style = resolve_slot(theme, ThemeRole::Focus, &node.focus_style);
    let mut effective_filled = resolve_force_accent_style(theme, node.filled_style);
    if is_focused {
        effective_filled = effective_filled.patch(focus_style);
    }
    let zones = node
        .zones
        .iter()
        .cloned()
        .map(|mut zone| {
            zone.style = resolve_accent_style(theme, zone.style);
            zone
        })
        .collect::<Vec<_>>();
    render_progress_bar(
        state.f,
        node.progress,
        node.progress_style,
        rect,
        rrect,
        ProgressBarRenderCtx {
            show_percentage: node.show_percentage,
            percentage_position: node.percentage_position,
            label: node.label.as_deref(),
            label_position: node.label_position,
            filled_style: effective_filled,
            filled_gradient: node.filled_gradient,
            empty_style: resolve_muted_style(theme, node.empty_style),
            label_style: resolve_base_style(theme, node.label_style),
            style: resolve_base_style(theme, node.style),
            hover_style: resolve_slot(theme, ThemeRole::Hover, &node.hover_style),
            target: node.target,
            target_style: resolve_accent_style(theme, node.target_style),
            target_symbol: node.target_symbol,
            zones: &zones,
            block_empty_bg_dim: node.block_empty_bg_dim,
            padding: node.padding,
            inverted: node.inverted,
            is_hovered,
            contrast_policy,
            clip_rect: clip_bounds,
            paint_glyph_caches: state.ctx.paint_glyph_caches.clone(),
        },
    );
}
