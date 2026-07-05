use ratatui::buffer::Buffer;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::backend::ratatui_backend::common::{draw_cell_clipped, to_ratatui_style};
use crate::backend::ratatui_backend::render::RenderState;
use crate::core::node::NodeId;
use crate::style::resolve::{resolve_base_style, resolve_force_accent_style};
use crate::style::{Padding, Rect, Style, ThemeRole, resolve_slot};
use crate::utils::gradient::ColorGradient;

pub(crate) struct SliderRenderCtx<'a> {
    pub style: Style,
    pub filled_track_style: Style,
    pub filled_track_gradient: Option<ColorGradient>,
    pub thumb_style: Style,
    pub thumb_gradient: Option<ColorGradient>,
    pub label: Option<&'a str>,
    pub label_style: Style,
    pub show_value: bool,
    pub padding: Padding,
    pub is_focused: bool,
    pub is_hovered: bool,
    pub focus_style: Style,
    pub focus_thumb_style: Style,
    pub hover_thumb_style: Style,
    pub thumb_symbol: &'a str,
    pub track_symbol: &'a str,
    pub filled_track_symbol: &'a str,
    pub hover_thumb_symbol: Option<&'a str>,
    pub clip_rect: Option<Rect>,
}

pub(crate) fn render_slider(
    buf: &mut Buffer,
    value: f64,
    min: f64,
    max: f64,
    area: Rect,
    ctx: SliderRenderCtx<'_>,
) {
    let SliderRenderCtx {
        style,
        filled_track_style,
        filled_track_gradient,
        thumb_style,
        thumb_gradient,
        label,
        label_style,
        show_value,
        padding,
        is_focused,
        is_hovered,
        focus_style,
        focus_thumb_style,
        hover_thumb_style,
        thumb_symbol,
        track_symbol,
        filled_track_symbol,
        hover_thumb_symbol,
        clip_rect,
    } = ctx;
    let inner = area.inset(padding);

    let y = inner.y as i32 + (inner.h.saturating_sub(1) / 2) as i32;
    let mut x = inner.x as i32;
    let mut w = inner.w as i32;

    if w <= 0 {
        return;
    }

    if let Some(label_text) = label {
        let label_w = UnicodeWidthStr::width(label_text) as i32;
        if w > label_w {
            render_text_clipped(buf, label_text, label_style, x, y, label_w, clip_rect);
            x = x.saturating_add(label_w + 1);
            w = w.saturating_sub(label_w + 1);
        }
    }

    if show_value {
        let value_w = crate::widgets::slider::value_slot_width(min, max) as i32;
        let value_text = format!("{:.1}", value);
        let value_text = format!("{:>width$}", value_text, width = value_w.max(0) as usize);

        if w > value_w {
            render_text_clipped(
                buf,
                &value_text,
                style,
                x.saturating_add(w).saturating_sub(value_w),
                y,
                value_w,
                clip_rect,
            );
            w = w.saturating_sub(value_w + 1);
        }
    }

    if w <= 0 {
        return;
    }

    let range = max - min;
    let progress = if range > 0.0 {
        (value - min) / range
    } else {
        0.0
    };
    let progress = progress.clamp(0.0, 1.0);

    let track_len = w.saturating_sub(1) as f64;
    let thumb_pos = (progress * track_len).round() as i32;

    let mut current_thumb_style = thumb_style;
    if is_hovered {
        current_thumb_style = current_thumb_style.patch(hover_thumb_style);
    }
    if is_focused {
        current_thumb_style = current_thumb_style.patch(focus_thumb_style);
    }

    let mut current_track_style = style;
    let mut current_filled_track_style = filled_track_style;
    if is_focused {
        current_track_style = current_track_style.patch(focus_style);
        current_filled_track_style = current_filled_track_style.patch(focus_style);
    }

    let mut current_thumb_char = thumb_symbol;
    if is_hovered && let Some(hover_sym) = hover_thumb_symbol {
        current_thumb_char = hover_sym;
    }

    let track_rstyle = to_ratatui_style(current_track_style);

    for i in 0..w {
        let cx = x + i;
        if i == thumb_pos {
            let thumb_style = if let Some(gradient) = thumb_gradient {
                current_thumb_style.patch(Style::new().fg(gradient.color_at(progress)))
            } else {
                current_thumb_style
            };
            draw_cell_clipped(
                buf,
                cx,
                y,
                current_thumb_char,
                to_ratatui_style(thumb_style),
                clip_rect,
            );
        } else if i < thumb_pos {
            let filled_style = if let Some(gradient) = filled_track_gradient {
                let t = if w <= 1 {
                    1.0
                } else {
                    i as f64 / (w - 1) as f64
                };
                current_filled_track_style.patch(Style::new().fg(gradient.color_at(t)))
            } else {
                current_filled_track_style
            };
            draw_cell_clipped(
                buf,
                cx,
                y,
                filled_track_symbol,
                to_ratatui_style(filled_style),
                clip_rect,
            );
        } else {
            draw_cell_clipped(buf, cx, y, track_symbol, track_rstyle, clip_rect);
        }
    }
}

fn render_text_clipped(
    buf: &mut Buffer,
    content: &str,
    style: Style,
    x: i32,
    y: i32,
    max_width: i32,
    clip_rect: Option<Rect>,
) {
    if max_width <= 0 {
        return;
    }

    let rstyle = to_ratatui_style(style);
    let mut curr_x = x;
    for g in content.graphemes(true) {
        let cw = UnicodeWidthStr::width(g) as i32;
        if cw == 0 {
            continue;
        }
        if curr_x - x + cw > max_width {
            break;
        }
        draw_cell_clipped(buf, curr_x, y, g, rstyle, clip_rect);
        curr_x += cw;
    }
}

pub(crate) fn render_slider_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::widgets::internal::SliderNode,
    rect: Rect,
    _rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) {
    let is_focused = Some(node_id) == state.ctx.focused;
    let is_hovered = Some(node_id) == state.ctx.hovered;
    let theme = state.ctx.tree.node(node_id).active_theme();
    render_slider(
        state.f.buffer_mut(),
        node.value,
        node.min,
        node.max,
        rect,
        SliderRenderCtx {
            style: resolve_base_style(theme, node.style),
            filled_track_style: resolve_force_accent_style(theme, node.filled_track_style),
            filled_track_gradient: node.filled_track_gradient,
            thumb_style: resolve_force_accent_style(theme, node.thumb_style),
            thumb_gradient: node.thumb_gradient,
            label: node.label.as_deref(),
            label_style: resolve_base_style(theme, node.label_style),
            show_value: node.show_value,
            padding: node.padding,
            is_focused,
            is_hovered,
            focus_style: resolve_slot(theme, ThemeRole::Focus, &node.focus_style),
            focus_thumb_style: resolve_slot(theme, ThemeRole::Focus, &node.focus_thumb_style),
            hover_thumb_style: resolve_slot(theme, ThemeRole::Hover, &node.hover_thumb_style),
            thumb_symbol: &node.thumb_symbol,
            track_symbol: &node.track_symbol,
            filled_track_symbol: &node.filled_track_symbol,
            hover_thumb_symbol: node.hover_thumb_symbol.as_deref(),
            clip_rect: clip_bounds,
        },
    );
}
