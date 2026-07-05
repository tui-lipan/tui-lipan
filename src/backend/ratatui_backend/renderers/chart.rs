use ratatui::widgets::{Block, Borders};

use crate::backend::ratatui_backend::common::{
    ClipBounds, calculate_visible_borders, style_paints_bg, to_ratatui_border_set,
    to_ratatui_border_type, to_ratatui_style,
};
use std::sync::Arc;

use crate::style::resolve::{resolve_accent_style, resolve_base_style, resolve_muted_style};
use crate::style::{Rect, Theme};
use crate::widgets::ChartSeriesMode;
use crate::widgets::internal::ChartNode;

pub(crate) fn render_chart(
    f: &mut ratatui::Frame<'_>,
    node: &ChartNode,
    theme: &Theme,
    rect: Rect,
    clip_rect: Rect,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let mut node = node.clone();
    node.style = resolve_base_style(theme, node.style);
    node.axis_style = resolve_base_style(theme, node.axis_style);
    node.grid_style = resolve_muted_style(theme, node.grid_style);
    node.legend_style = resolve_base_style(theme, node.legend_style);
    node.x_axis.style = resolve_base_style(theme, node.x_axis.style);
    node.y_axis.style = resolve_base_style(theme, node.y_axis.style);
    for series in &mut Arc::make_mut(&mut node.output).series {
        series.style = resolve_accent_style(theme, series.style);
    }
    for threshold in &mut Arc::make_mut(&mut node.output).thresholds {
        threshold.style = resolve_accent_style(theme, threshold.style);
    }

    let node = &node;
    let mut inner = rect;
    let rrect = crate::backend::ratatui_backend::common::to_ratatui_rect(rect);

    if style_paints_bg(node.style) {
        let clear = Block::default().style(to_ratatui_style(node.style));
        f.render_widget(clear, rrect);
    }

    if node.border {
        let borders = calculate_visible_borders(rect, Some(clip_rect));
        let mut block = Block::default()
            .borders(borders)
            .border_type(to_ratatui_border_type(node.border_style))
            .style(to_ratatui_style(node.style));
        if let Some(set) = to_ratatui_border_set(node.border_style) {
            block = block.border_set(set);
        }
        f.render_widget(block, rrect);

        if borders.contains(Borders::LEFT) {
            inner.x = inner.x.saturating_add(1);
            inner.w = inner.w.saturating_sub(1);
        }
        if borders.contains(Borders::RIGHT) {
            inner.w = inner.w.saturating_sub(1);
        }
        if borders.contains(Borders::TOP) {
            inner.y = inner.y.saturating_add(1);
            inner.h = inner.h.saturating_sub(1);
        }
        if borders.contains(Borders::BOTTOM) {
            inner.h = inner.h.saturating_sub(1);
        }
    }

    inner = inner.inset(node.padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    let legend_rows = if node.show_legend && !node.output.series.is_empty() {
        1
    } else {
        0
    };
    let x_axis_rows = if node.x_axis.show { 1 } else { 0 };
    let y_axis_cols = if node.y_axis.show { 8 } else { 0 };

    let plot_rect = Rect {
        x: inner.x.saturating_add(y_axis_cols as i16),
        y: inner.y.saturating_add(legend_rows as i16),
        w: inner.w.saturating_sub(y_axis_cols),
        h: inner.h.saturating_sub(legend_rows + x_axis_rows),
    };
    if plot_rect.w == 0 || plot_rect.h == 0 {
        return;
    }

    let content_clip = inner.intersection(&clip_rect);
    let clip = ClipBounds::from_rect(content_clip);
    let bounds = ClipBounds::from_rrect(f.area());

    if legend_rows > 0 {
        render_legend(f, node, inner, &clip, &bounds);
    }
    if node.show_grid {
        render_grid(f, node, plot_rect, &clip, &bounds);
    }
    render_thresholds(f, node, plot_rect, &clip, &bounds);
    render_series(f, node, plot_rect, &clip, &bounds);
    render_axes(f, node, inner, plot_rect, &clip, &bounds);
}

fn render_legend(
    f: &mut ratatui::Frame<'_>,
    node: &ChartNode,
    inner: Rect,
    clip: &ClipBounds,
    bounds: &ClipBounds,
) {
    let mut x = inner.x;
    let y = inner.y;
    let sep = node.legend_separator.as_ref();
    let series_len = node.output.series.len();
    let sep_style = to_ratatui_style(node.legend_style);

    for (idx, series) in node.output.series.iter().enumerate() {
        let style = to_ratatui_style(node.legend_style.patch(series.style));
        let marker = match series.mode {
            ChartSeriesMode::Line => series.point_char,
            ChartSeriesMode::Bars => series.bar_char,
        };
        let label = format!("{marker} {}", series.name);
        x = draw_text(f, x, y, &label, style, clip, bounds);

        if idx + 1 < series_len {
            x = draw_text(f, x, y, sep, sep_style, clip, bounds);
        }
    }
}

fn render_grid(
    f: &mut ratatui::Frame<'_>,
    node: &ChartNode,
    plot_rect: Rect,
    clip: &ClipBounds,
    bounds: &ClipBounds,
) {
    let ticks_x = node.x_axis.ticks.max(2) as i16;
    let ticks_y = node.y_axis.ticks.max(2) as i16;
    let vstyle = to_ratatui_style(node.style.patch(node.grid_style));

    for i in 0..ticks_x {
        let x = plot_rect
            .x
            .saturating_add(scale_index(i, ticks_x - 1, plot_rect.w));
        for y in plot_rect.y..plot_rect.y.saturating_add(plot_rect.h as i16) {
            draw_char(f, x, y, '┆', vstyle, clip, bounds);
        }
    }
    for i in 0..ticks_y {
        let y = plot_rect
            .y
            .saturating_add(scale_index(i, ticks_y - 1, plot_rect.h));
        for x in plot_rect.x..plot_rect.x.saturating_add(plot_rect.w as i16) {
            draw_char(f, x, y, '┈', vstyle, clip, bounds);
        }
    }
}

fn render_thresholds(
    f: &mut ratatui::Frame<'_>,
    node: &ChartNode,
    plot_rect: Rect,
    clip: &ClipBounds,
    bounds: &ClipBounds,
) {
    for threshold in node.output.thresholds.iter() {
        let y = map_y(plot_rect, threshold.y_norm);
        let style = to_ratatui_style(node.style.patch(threshold.style));
        for x in plot_rect.x..plot_rect.x.saturating_add(plot_rect.w as i16) {
            draw_char(f, x, y, threshold.glyph, style, clip, bounds);
        }

        let label = threshold
            .label
            .as_deref()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{:.2}", threshold.value));
        let label_x = plot_rect
            .x
            .saturating_add(plot_rect.w as i16)
            .saturating_sub(label.len() as i16 + 1);
        let _ = draw_text(f, label_x, y, &label, style, clip, bounds);
    }
}

fn render_series(
    f: &mut ratatui::Frame<'_>,
    node: &ChartNode,
    plot_rect: Rect,
    clip: &ClipBounds,
    bounds: &ClipBounds,
) {
    let bottom = plot_rect
        .y
        .saturating_add(plot_rect.h as i16)
        .saturating_sub(1);
    for series in node.output.series.iter() {
        let style = to_ratatui_style(node.style.patch(series.style));
        match series.mode {
            ChartSeriesMode::Bars => {
                for (x_norm, y_norm) in series.points.iter().copied() {
                    let x = map_x(plot_rect, x_norm);
                    let top = map_y(plot_rect, y_norm);
                    for y in top..=bottom {
                        draw_char(f, x, y, series.bar_char, style, clip, bounds);
                    }
                }
            }
            ChartSeriesMode::Line => {
                let mut prev = None;
                for (x_norm, y_norm) in series.points.iter().copied() {
                    let x = map_x(plot_rect, x_norm);
                    let y = map_y(plot_rect, y_norm);
                    if let Some((px, py)) = prev {
                        draw_line(
                            f,
                            clip,
                            bounds,
                            LineDrawCtx {
                                x0: px,
                                y0: py,
                                x1: x,
                                y1: y,
                                ch: series.line_char,
                                style,
                            },
                        );
                    }
                    draw_char(f, x, y, series.point_char, style, clip, bounds);
                    prev = Some((x, y));
                }
            }
        }
    }
}

fn render_axes(
    f: &mut ratatui::Frame<'_>,
    node: &ChartNode,
    inner: Rect,
    plot_rect: Rect,
    clip: &ClipBounds,
    bounds: &ClipBounds,
) {
    let axis_style = to_ratatui_style(node.style.patch(node.axis_style));

    if node.y_axis.show {
        let axis_x = plot_rect.x.saturating_sub(1);
        for y in plot_rect.y..plot_rect.y.saturating_add(plot_rect.h as i16) {
            draw_char(f, axis_x, y, '│', axis_style, clip, bounds);
        }

        let top = format!("{:>7.2}", node.output.y_max);
        let bottom = format!("{:>7.2}", node.output.y_min);
        let _ = draw_text(
            f,
            inner.x,
            plot_rect.y,
            &top,
            to_ratatui_style(node.style.patch(node.axis_style).patch(node.y_axis.style)),
            clip,
            bounds,
        );
        let _ = draw_text(
            f,
            inner.x,
            plot_rect
                .y
                .saturating_add(plot_rect.h as i16)
                .saturating_sub(1),
            &bottom,
            to_ratatui_style(node.style.patch(node.axis_style).patch(node.y_axis.style)),
            clip,
            bounds,
        );
    }

    if node.x_axis.show {
        let y = plot_rect.y.saturating_add(plot_rect.h as i16);
        for x in plot_rect.x..plot_rect.x.saturating_add(plot_rect.w as i16) {
            draw_char(f, x, y, '─', axis_style, clip, bounds);
        }

        let start = node.viewport_start;
        let end = if node.output.sample_count == 0 {
            start
        } else {
            start + node.output.sample_count.saturating_sub(1)
        };
        let _ = draw_text(
            f,
            plot_rect.x,
            y,
            &format!("{start}"),
            to_ratatui_style(node.style.patch(node.axis_style).patch(node.x_axis.style)),
            clip,
            bounds,
        );
        let _ = draw_text(
            f,
            plot_rect
                .x
                .saturating_add(plot_rect.w as i16)
                .saturating_sub(end.to_string().len() as i16),
            y,
            &format!("{end}"),
            to_ratatui_style(node.style.patch(node.axis_style).patch(node.x_axis.style)),
            clip,
            bounds,
        );
    }
}

struct LineDrawCtx {
    x0: i16,
    y0: i16,
    x1: i16,
    y1: i16,
    ch: char,
    style: ratatui::style::Style,
}

fn draw_line(f: &mut ratatui::Frame<'_>, clip: &ClipBounds, bounds: &ClipBounds, ctx: LineDrawCtx) {
    let LineDrawCtx {
        x0,
        y0,
        x1,
        y1,
        ch,
        style,
    } = ctx;
    let mut x = x0;
    let mut y = y0;
    let dx = (x1 - x0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    loop {
        draw_char(f, x, y, ch, style, clip, bounds);
        if x == x1 && y == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x += sx;
        }
        if e2 <= dx {
            err += dx;
            y += sy;
        }
    }
}

fn draw_text(
    f: &mut ratatui::Frame<'_>,
    mut x: i16,
    y: i16,
    text: &str,
    style: ratatui::style::Style,
    clip: &ClipBounds,
    bounds: &ClipBounds,
) -> i16 {
    for ch in text.chars() {
        draw_char(f, x, y, ch, style, clip, bounds);
        x = x.saturating_add(1);
    }
    x
}

fn draw_char(
    f: &mut ratatui::Frame<'_>,
    x: i16,
    y: i16,
    ch: char,
    style: ratatui::style::Style,
    clip: &ClipBounds,
    bounds: &ClipBounds,
) {
    let x = x as i32;
    let y = y as i32;
    if !clip.contains(x, y) || !bounds.contains(x, y) {
        return;
    }
    let Some(cell) = f.buffer_mut().cell_mut((x as u16, y as u16)) else {
        return;
    };
    cell.set_char(ch).set_style(style);
}

fn map_x(plot_rect: Rect, x_norm: f64) -> i16 {
    let width = plot_rect.w.saturating_sub(1) as f64;
    plot_rect
        .x
        .saturating_add((x_norm.clamp(0.0, 1.0) * width).round() as i16)
}

fn map_y(plot_rect: Rect, y_norm: f64) -> i16 {
    let height = plot_rect.h.saturating_sub(1) as f64;
    let scaled = (y_norm.clamp(0.0, 1.0) * height).round() as i16;
    plot_rect
        .y
        .saturating_add(plot_rect.h.saturating_sub(1) as i16)
        .saturating_sub(scaled)
}

fn scale_index(idx: i16, max_idx: i16, span: u16) -> i16 {
    if max_idx <= 0 || span <= 1 {
        return 0;
    }
    ((idx as f64 / max_idx as f64) * (span.saturating_sub(1) as f64)).round() as i16
}
