use ratatui::widgets::Borders;
use unicode_width::UnicodeWidthChar;

use crate::backend::ratatui_backend::common::{
    ClipBounds, calculate_visible_borders, style_paints_bg, to_ratatui_border_set, to_ratatui_style,
};
use crate::style::resolve::{
    Durability, StateLayer, resolve_accent_style, resolve_base_style, resolve_muted_style,
    resolve_slot, resolve_state_cascade,
};
use crate::style::{Padding, Rect, Theme, ThemeRole};
use crate::widgets::internal::{GraphRenderNode, graph_local_content_point};

pub(crate) fn render_graph(
    f: &mut ratatui::Frame<'_>,
    node: &GraphRenderNode,
    theme: &Theme,
    rect: Rect,
    clip_rect: Rect,
    is_focused: bool,
    mouse_pos: Option<(u16, u16)>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let base_style = resolve_base_style(theme, node.style);
    let node_style = resolve_accent_style(theme, node.node_style);
    let node_hover_style = graph_node_hover_style(node.node_hover_style);
    let graph_focus_style = resolve_slot(theme, ThemeRole::Focus, &node.node_focus_style);
    let edge_style = resolve_muted_style(theme, node.edge_style);
    let bounds = ClipBounds::from_rrect(f.area());

    let outer_clip_rect = rect.intersection(&clip_rect);
    if !outer_clip_rect.is_empty() && (style_paints_bg(base_style) || node.border) {
        let outer_clip = ClipBounds::from_rect(outer_clip_rect);
        let mut paint = PaintCtx {
            f,
            clip: &outer_clip,
            bounds: &bounds,
        };

        if style_paints_bg(base_style) {
            fill_rect(&mut paint, rect, to_ratatui_style(base_style));
        }

        if node.border {
            let borders = calculate_visible_borders(rect, Some(clip_rect));
            draw_outer_border(&mut paint, rect, borders, node.border_style, base_style);
        }
    }

    let inner = node.content_rect(rect);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    let hovered_index = mouse_pos
        .and_then(|(mx, my)| {
            graph_local_content_point(rect, node.border, node.padding, mx as i16, my as i16)
        })
        .and_then(|(local_x, local_y)| node.hit_test(local_x, local_y).map(|(index, _)| index));

    let content_clip = inner.intersection(&clip_rect);
    if content_clip.is_empty() {
        return;
    }
    let clip = ClipBounds::from_rect(content_clip);
    let edge_style = to_ratatui_style(base_style.patch(edge_style));
    let mut paint = PaintCtx {
        f,
        clip: &clip,
        bounds: &bounds,
    };

    for edge in &node.output.edges {
        draw_char(
            &mut paint,
            inner.x.saturating_add(edge.x),
            inner.y.saturating_add(edge.y),
            edge.glyph,
            edge_style,
        );
    }

    let focused_path = is_focused
        .then(|| node.current_focused_path_or_first())
        .flatten();
    for (index, graph_node) in node.output.nodes.iter().enumerate() {
        let rect = Rect {
            x: inner.x.saturating_add(graph_node.rect.x),
            y: inner.y.saturating_add(graph_node.rect.y),
            w: graph_node.rect.w,
            h: graph_node.rect.h,
        };
        let style = base_style.patch(node_style).patch(graph_node.style);
        let is_hovered = Some(index) == hovered_index;
        let is_node_focused = focused_path
            .as_ref()
            .is_some_and(|focused_path| &graph_node.path == focused_path);
        let hover_style = node_hover_style
            .unwrap_or_default()
            .patch(graph_node_hover_style(graph_node.hover_style).unwrap_or_default());
        let focus_style = graph_focus_style.patch(resolve_slot(
            theme,
            ThemeRole::Focus,
            &graph_node.focus_style,
        ));
        let style = graph_node_interactive_style(
            style,
            hover_style,
            focus_style,
            is_hovered,
            is_node_focused,
        );
        let style = to_ratatui_style(style);
        render_node(
            &mut paint,
            rect,
            &graph_node.label_lines,
            graph_node.border,
            node.node_border_style,
            node.node_padding,
            style,
        );
    }
}

fn graph_node_hover_style(style: crate::style::Style) -> Option<crate::style::Style> {
    (!style.is_empty()).then_some(style)
}

fn graph_node_interactive_style(
    base: crate::style::Style,
    hover_style: crate::style::Style,
    focus_style: crate::style::Style,
    is_hovered: bool,
    is_focused: bool,
) -> crate::style::Style {
    let mut layers = Vec::new();
    if is_hovered {
        layers.push(StateLayer {
            style: &hover_style,
            durability: Durability::Transient,
        });
    }
    if is_focused {
        layers.push(StateLayer {
            style: &focus_style,
            durability: Durability::Durable,
        });
    }
    resolve_state_cascade(base, &layers)
}

struct PaintCtx<'a, 'b, 'c> {
    f: &'a mut ratatui::Frame<'b>,
    clip: &'c ClipBounds,
    bounds: &'c ClipBounds,
}

fn fill_rect(paint: &mut PaintCtx<'_, '_, '_>, rect: Rect, style: ratatui::style::Style) {
    let bottom = rect.y.saturating_add(rect.h as i16);
    let right = rect.x.saturating_add(rect.w as i16);
    for y in rect.y..bottom {
        for x in rect.x..right {
            draw_char(paint, x, y, ' ', style);
        }
    }
}

fn draw_outer_border(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    borders: Borders,
    border_style: crate::style::BorderStyle,
    style: crate::style::Style,
) {
    if rect.w < 2 || rect.h < 2 || borders.is_empty() {
        return;
    }

    let set = to_ratatui_border_set(border_style).unwrap_or(ratatui::symbols::border::PLAIN);
    let style = to_ratatui_style(style);
    let left = rect.x;
    let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
    let top = rect.y;
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);

    if borders.contains(Borders::TOP) {
        for x in left.saturating_add(1)..right {
            draw_symbol(paint, x, top, set.horizontal_top, style);
        }
    }
    if borders.contains(Borders::BOTTOM) {
        for x in left.saturating_add(1)..right {
            draw_symbol(paint, x, bottom, set.horizontal_bottom, style);
        }
    }
    if borders.contains(Borders::LEFT) {
        for y in top.saturating_add(1)..bottom {
            draw_symbol(paint, left, y, set.vertical_left, style);
        }
    }
    if borders.contains(Borders::RIGHT) {
        for y in top.saturating_add(1)..bottom {
            draw_symbol(paint, right, y, set.vertical_right, style);
        }
    }

    if borders.contains(Borders::TOP) && borders.contains(Borders::LEFT) {
        draw_symbol(paint, left, top, set.top_left, style);
    }
    if borders.contains(Borders::TOP) && borders.contains(Borders::RIGHT) {
        draw_symbol(paint, right, top, set.top_right, style);
    }
    if borders.contains(Borders::BOTTOM) && borders.contains(Borders::LEFT) {
        draw_symbol(paint, left, bottom, set.bottom_left, style);
    }
    if borders.contains(Borders::BOTTOM) && borders.contains(Borders::RIGHT) {
        draw_symbol(paint, right, bottom, set.bottom_right, style);
    }
}

fn render_node(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    label_lines: &[std::sync::Arc<str>],
    border: bool,
    border_style: crate::style::BorderStyle,
    padding: Padding,
    style: ratatui::style::Style,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    fill_rect(paint, rect, style);

    if border {
        draw_box(paint, rect, border_style, style);
    }

    let border_offset = if border { 1 } else { 0 };
    let label_x = rect
        .x
        .saturating_add(border_offset)
        .saturating_add(padding.left as i16);
    let label_y = rect
        .y
        .saturating_add(border_offset)
        .saturating_add(padding.top as i16);
    let chrome = if border { 2 } else { 0 };
    let available = rect
        .w
        .saturating_sub(chrome)
        .saturating_sub(padding.horizontal()) as usize;
    let available_rows = rect
        .h
        .saturating_sub(chrome)
        .saturating_sub(padding.vertical()) as usize;
    draw_label_lines_clipped(
        paint,
        label_x,
        label_y,
        label_lines,
        available,
        available_rows,
        style,
    );
}

fn draw_label_lines_clipped(
    paint: &mut PaintCtx<'_, '_, '_>,
    x: i16,
    y: i16,
    lines: &[std::sync::Arc<str>],
    available: usize,
    available_rows: usize,
    style: ratatui::style::Style,
) {
    for (row, line) in lines.iter().take(available_rows).enumerate() {
        draw_text_clipped(
            paint,
            x,
            y.saturating_add(row as i16),
            line,
            available,
            style,
        );
    }
}

fn draw_box(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    border_style: crate::style::BorderStyle,
    style: ratatui::style::Style,
) {
    if rect.w < 2 || rect.h < 2 {
        return;
    }
    let set = to_ratatui_border_set(border_style).unwrap_or(ratatui::symbols::border::PLAIN);
    let left = rect.x;
    let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
    let top = rect.y;
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);

    draw_symbol(paint, left, top, set.top_left, style);
    draw_symbol(paint, right, top, set.top_right, style);
    draw_symbol(paint, left, bottom, set.bottom_left, style);
    draw_symbol(paint, right, bottom, set.bottom_right, style);

    for x in left.saturating_add(1)..right {
        draw_symbol(paint, x, top, set.horizontal_top, style);
        draw_symbol(paint, x, bottom, set.horizontal_bottom, style);
    }
    for y in top.saturating_add(1)..bottom {
        draw_symbol(paint, left, y, set.vertical_left, style);
        draw_symbol(paint, right, y, set.vertical_right, style);
    }
}

fn draw_text_clipped(
    paint: &mut PaintCtx<'_, '_, '_>,
    mut x: i16,
    y: i16,
    text: &str,
    available: usize,
    style: ratatui::style::Style,
) {
    let mut used = 0usize;
    for ch in text.chars() {
        let width = ch.width().unwrap_or(0);
        if used.saturating_add(width) > available {
            break;
        }
        draw_char(paint, x, y, ch, style);
        x = x.saturating_add(width as i16);
        used = used.saturating_add(width);
    }
}

fn draw_char(
    paint: &mut PaintCtx<'_, '_, '_>,
    x: i16,
    y: i16,
    ch: char,
    style: ratatui::style::Style,
) {
    let x = i32::from(x);
    let y = i32::from(y);
    if !paint.clip.contains(x, y) || !paint.bounds.contains(x, y) {
        return;
    }
    let Some(cell) = paint.f.buffer_mut().cell_mut((x as u16, y as u16)) else {
        return;
    };
    cell.set_char(ch).set_style(style);
}

fn draw_symbol(
    paint: &mut PaintCtx<'_, '_, '_>,
    x: i16,
    y: i16,
    symbol: &str,
    style: ratatui::style::Style,
) {
    let x = i32::from(x);
    let y = i32::from(y);
    if !paint.clip.contains(x, y) || !paint.bounds.contains(x, y) {
        return;
    }
    let Some(cell) = paint.f.buffer_mut().cell_mut((x as u16, y as u16)) else {
        return;
    };
    cell.set_symbol(symbol).set_style(style);
}

#[cfg(test)]
mod tests {
    use crate::style::{Color, ColorTransform, Style};

    use super::{graph_node_hover_style, graph_node_interactive_style};

    #[test]
    fn transform_only_hover_style_does_not_gain_accent_color() {
        let style = graph_node_hover_style(Style::new().lighten_by(0.0))
            .expect("transform-only hover style should be non-empty");

        assert_eq!(style.fg, None);
        assert_eq!(style.bg, None);
        assert_eq!(style.fg_transform, Some(ColorTransform::Lighten(0.0)));
        assert_eq!(style.bg_transform, Some(ColorTransform::Lighten(0.0)));
    }

    #[test]
    fn concrete_hover_style_is_preserved() {
        let style = graph_node_hover_style(Style::new().fg(Color::Black).bg(Color::LightCyan))
            .expect("concrete hover style should be non-empty");

        assert_eq!(style.fg, Some(crate::style::Paint::Solid(Color::Black)));
        assert_eq!(style.bg, Some(crate::style::Paint::Solid(Color::LightCyan)));
    }

    #[test]
    fn hover_transform_composes_over_focused_node_style() {
        let style = graph_node_interactive_style(
            Style::new().bg(Color::Blue),
            Style::new().transform_bg(ColorTransform::Lighten(0.4)),
            Style::new().bg(Color::Yellow),
            true,
            true,
        );

        assert_eq!(
            style.bg,
            Some(crate::style::Paint::Solid(Color::rgb(225, 225, 102)))
        );
        assert_eq!(style.bg_transform, None);
    }

    #[test]
    fn focus_concrete_color_wins_over_hover_concrete_color() {
        let style = graph_node_interactive_style(
            Style::new().bg(Color::Blue),
            Style::new().bg(Color::Red),
            Style::new().bg(Color::Yellow),
            true,
            true,
        );

        assert_eq!(style.bg, Some(crate::style::Paint::Solid(Color::Yellow)));
    }
}
