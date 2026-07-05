use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::backend::ratatui_backend::common::{
    ClipBounds, calculate_visible_borders, style_paints_bg, to_ratatui_border_set,
    to_ratatui_border_type, to_ratatui_rect, to_ratatui_span, to_ratatui_style,
};
use crate::style::resolve::resolve_base_style;
use crate::style::{Rect, Style, Theme};
use crate::widgets::internal::HeatmapNode;
use unicode_width::UnicodeWidthStr;

pub(crate) fn render_heatmap(
    f: &mut ratatui::Frame<'_>,
    node: &HeatmapNode,
    theme: &Theme,
    rect: Rect,
    clip_rect: Rect,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let mut node = node.clone();
    node.style = resolve_base_style(theme, node.style);
    node.label_style = resolve_base_style(theme, node.label_style);
    node.legend_style = resolve_base_style(theme, node.legend_style);
    let node = &node;
    let mut inner = rect;
    let rrect = crate::backend::ratatui_backend::common::to_ratatui_rect(rect);

    // Background fill.
    if style_paints_bg(node.style) {
        let clear = Block::default().style(to_ratatui_style(node.style));
        f.render_widget(clear, rrect);
    }

    // Border.
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

    // Padding.
    inner = inner.inset(node.padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    let content_clip = inner.intersection(&clip_rect);
    let clip = ClipBounds::from_rect(content_clip);
    let bounds = ClipBounds::from_rrect(f.area());

    // Layout regions.
    let label_gutter: u16 = if node.row_labels.is_empty() {
        0
    } else {
        node.row_labels
            .iter()
            .map(|l| {
                unicode_width::UnicodeWidthStr::width(l.as_ref()).min(u16::MAX as usize) as u16
            })
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    };

    let header_rows: u16 = if node.column_labels.is_empty() { 0 } else { 1 };

    let legend_rows: u16 = if node.show_legend {
        1u16.saturating_add(node.legend_spacing)
    } else {
        0
    };

    let data_origin_x = inner.x.saturating_add(label_gutter as i16);
    let data_origin_y = inner.y.saturating_add(header_rows as i16);
    let data_avail_h = inner
        .h
        .saturating_sub(header_rows)
        .saturating_sub(legend_rows);

    // Render column labels.
    if header_rows > 0
        && let Some(header_line) = &node.output.header_line
    {
        let header_rect = Rect {
            x: data_origin_x,
            y: inner.y,
            w: inner.w.saturating_sub(label_gutter),
            h: 1,
        }
        .intersection(&content_clip);
        if !header_rect.is_empty() {
            let spans: Vec<ratatui::text::Span<'_>> = header_line
                .iter()
                .map(|span| to_ratatui_span(span, node.style.patch(node.label_style)))
                .collect();
            let dx = (header_rect.x as i32)
                .saturating_sub(data_origin_x as i32)
                .max(0) as u16;
            f.render_widget(
                Paragraph::new(Line::from(spans)).scroll((0, dx)),
                to_ratatui_rect(header_rect),
            );
        }
    }

    // Render row labels + data cells.
    let visible_rows = (node.output.total_data_rows as u16).min(data_avail_h);
    for row_idx in 0..visible_rows as usize {
        let y = data_origin_y.saturating_add(row_idx as i16);
        let row_stride = node.gap_y as usize + 1;
        let data_row_idx = row_idx / row_stride;
        let is_gap_row = row_idx % row_stride != 0;

        // Row label.
        if !is_gap_row
            && !node.row_labels.is_empty()
            && let Some(label) = node.row_labels.get(data_row_idx)
        {
            let max_w = label_gutter.saturating_sub(1);
            draw_text(
                f,
                inner.x,
                y,
                label,
                HeatmapDrawTextCtx {
                    style: node.style.patch(node.label_style),
                    max_w,
                    clip: &clip,
                    bounds: &bounds,
                },
            );
        }

        // Data cells.
        if let Some(line) = node.output.data_lines.get(row_idx) {
            let row_rect = Rect {
                x: data_origin_x,
                y,
                w: inner.w.saturating_sub(label_gutter),
                h: 1,
            }
            .intersection(&content_clip);
            if !row_rect.is_empty() {
                let spans: Vec<ratatui::text::Span<'_>> = line
                    .iter()
                    .map(|span| to_ratatui_span(span, node.style))
                    .collect();
                let dx = (row_rect.x as i32)
                    .saturating_sub(data_origin_x as i32)
                    .max(0) as u16;
                f.render_widget(
                    Paragraph::new(Line::from(spans)).scroll((0, dx)),
                    to_ratatui_rect(row_rect),
                );
            }
        }
    }

    // Render legend.
    if node.show_legend && legend_rows > 0 {
        let legend_y = data_origin_y
            .saturating_add(visible_rows as i16)
            .saturating_add(node.legend_spacing as i16);
        render_legend(f, node, inner, data_origin_x, legend_y, &clip, &bounds);
    }
}

fn render_legend(
    f: &mut ratatui::Frame<'_>,
    node: &HeatmapNode,
    inner: Rect,
    data_origin_x: i16,
    y: i16,
    clip: &ClipBounds,
    bounds: &ClipBounds,
) {
    let legend_style = node.style.patch(node.legend_style);

    let min_label = format!("{:.1}", node.output.value_min);
    let max_label = format!("{:.1}", node.output.value_max);
    let marker_width = legend_marker_width(node);
    let (legend_x, avail_w) = match node.legend_width {
        crate::widgets::HeatmapLegendWidth::Grid => (
            data_origin_x,
            inner.w.saturating_sub((data_origin_x - inner.x) as u16) as usize,
        ),
        crate::widgets::HeatmapLegendWidth::Full => (inner.x, inner.w as usize),
    };

    let label_space =
        UnicodeWidthStr::width(min_label.as_str()) + UnicodeWidthStr::width(max_label.as_str()) + 2;
    let swatch_budget = avail_w.saturating_sub(label_space);
    let min_swatch_count = 4usize;
    let gap = node.legend_gap as usize;
    let bar_len = if gap == 0 {
        (swatch_budget / marker_width.max(1)).max(min_swatch_count)
    } else {
        let count = (swatch_budget.saturating_add(gap)) / (marker_width.saturating_add(gap));
        count.max(min_swatch_count)
    };

    let mut x = legend_x;
    x = draw_char_run(f, x, y, &min_label, legend_style, clip, bounds);
    x = draw_char_run(f, x, y, " ", legend_style, clip, bounds);

    // Gradient bar.
    for i in 0..bar_len {
        let t = i as f64 / (bar_len.saturating_sub(1).max(1)) as f64;
        let color = node.gradient.color_at(t);
        x = render_legend_marker(LegendMarkerRender {
            f,
            x,
            y,
            node,
            legend_style,
            color,
            clip,
            bounds,
        });
        if i + 1 < bar_len && node.legend_gap > 0 {
            x = draw_char_run(
                f,
                x,
                y,
                &" ".repeat(node.legend_gap as usize),
                legend_style,
                clip,
                bounds,
            );
        }
    }

    x = draw_char_run(f, x, y, " ", legend_style, clip, bounds);
    draw_char_run(f, x, y, &max_label, legend_style, clip, bounds);
}

fn legend_marker_width(node: &HeatmapNode) -> usize {
    match &node.cell_mode {
        crate::widgets::HeatmapCellMode::GlyphForeground(glyph)
        | crate::widgets::HeatmapCellMode::Glyph(glyph) => {
            UnicodeWidthStr::width(glyph.as_ref()).max(1)
        }
        crate::widgets::HeatmapCellMode::Background => 1,
    }
}

struct LegendMarkerRender<'a, 'b> {
    f: &'a mut ratatui::Frame<'b>,
    x: i16,
    y: i16,
    node: &'a HeatmapNode,
    legend_style: Style,
    color: crate::style::Color,
    clip: &'a ClipBounds,
    bounds: &'a ClipBounds,
}

fn render_legend_marker(args: LegendMarkerRender<'_, '_>) -> i16 {
    let LegendMarkerRender {
        f,
        x,
        y,
        node,
        legend_style,
        color,
        clip,
        bounds,
    } = args;

    match &node.cell_mode {
        crate::widgets::HeatmapCellMode::GlyphForeground(glyph) => draw_char_run(
            f,
            x,
            y,
            glyph.as_ref(),
            Style {
                fg: Some(color.into()),
                ..legend_style
            },
            clip,
            bounds,
        ),
        crate::widgets::HeatmapCellMode::Glyph(glyph) => draw_char_run(
            f,
            x,
            y,
            glyph.as_ref(),
            Style {
                bg: Some(color.into()),
                ..legend_style
            },
            clip,
            bounds,
        ),
        crate::widgets::HeatmapCellMode::Background => draw_char_run(
            f,
            x,
            y,
            " ",
            Style {
                bg: Some(color.into()),
                ..legend_style
            },
            clip,
            bounds,
        ),
    }
}

struct HeatmapDrawTextCtx<'a> {
    style: Style,
    max_w: u16,
    clip: &'a ClipBounds,
    bounds: &'a ClipBounds,
}

fn draw_text(
    f: &mut ratatui::Frame<'_>,
    x: i16,
    y: i16,
    text: &str,
    ctx: HeatmapDrawTextCtx<'_>,
) -> i16 {
    let HeatmapDrawTextCtx {
        style,
        max_w,
        clip,
        bounds,
    } = ctx;
    let rstyle = to_ratatui_style(style);
    let mut cx = x;
    for (i, ch) in text.chars().enumerate() {
        if i as u16 >= max_w {
            break;
        }
        if clip.contains(cx as i32, y as i32)
            && bounds.contains(cx as i32, y as i32)
            && let Some(cell) = f.buffer_mut().cell_mut((cx as u16, y as u16))
        {
            cell.set_char(ch).set_style(rstyle);
        }
        cx = cx.saturating_add(1);
    }
    cx
}

/// Draw all characters of `text` at `(x, y)` without a width limit.
fn draw_char_run(
    f: &mut ratatui::Frame<'_>,
    x: i16,
    y: i16,
    text: &str,
    style: Style,
    clip: &ClipBounds,
    bounds: &ClipBounds,
) -> i16 {
    let rstyle = to_ratatui_style(style);
    let mut cx = x;
    for ch in text.chars() {
        if clip.contains(cx as i32, y as i32)
            && bounds.contains(cx as i32, y as i32)
            && let Some(cell) = f.buffer_mut().cell_mut((cx as u16, y as u16))
        {
            cell.set_char(ch).set_style(rstyle);
        }
        cx = cx.saturating_add(1);
    }
    cx
}
