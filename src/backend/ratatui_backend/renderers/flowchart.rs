use ratatui::widgets::Borders;

use crate::backend::ratatui_backend::common::{
    ClipBounds, calculate_visible_borders, style_paints_bg, to_ratatui_border_set, to_ratatui_style,
};
use crate::style::resolve::{resolve_accent_style, resolve_base_style, resolve_muted_style};
use crate::style::{Rect, Style, Theme};
use crate::widgets::internal::{FlowchartNode, flowchart_local_content_point};
use crate::widgets::{EdgeArrow, EdgeStyle, FlowDirection, FlowchartItemPath, NodeShape};

pub(crate) fn render_flowchart(
    f: &mut ratatui::Frame<'_>,
    node: &FlowchartNode,
    theme: &Theme,
    rect: Rect,
    clip_rect: Rect,
    mouse_pos: Option<(u16, u16)>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let base_style = resolve_base_style(theme, node.style);
    let default_node_style =
        resolve_accent_style(theme, node.theme.node_styles.default.patch(node.node_style));
    let default_edge_style = resolve_muted_style(theme, node.edge_style);
    let default_subgraph_style =
        resolve_muted_style(theme, node.theme.subgraph.style.patch(node.subgraph_style));
    let default_label_style =
        resolve_base_style(theme, node.theme.label_style.patch(node.label_style));
    let hover_style =
        flowchart_hover_style(node.theme.item_hover_style.patch(node.item_hover_style));
    let ascii_theme = node.theme.edge_glyphs[EdgeStyle::Solid.theme_index()].horizontal == '-';
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

    let hovered = mouse_pos
        .and_then(|(mx, my)| {
            flowchart_local_content_point(rect, node.border, node.padding, mx as i16, my as i16)
        })
        .and_then(|(local_x, local_y)| node.hit_test(local_x, local_y).map(|(_, path)| path));

    let content_clip = inner.intersection(&clip_rect);
    if content_clip.is_empty() {
        return;
    }
    let clip = ClipBounds::from_rect(content_clip);
    let mut paint = PaintCtx {
        f,
        clip: &clip,
        bounds: &bounds,
    };

    for subgraph in &node.output.subgraphs {
        let rect = translate(subgraph.rect, inner.x, inner.y);
        let mut style = base_style
            .patch(default_subgraph_style)
            .patch(subgraph.style);
        if hovered.as_ref() == Some(&FlowchartItemPath::Subgraph(subgraph.id.clone())) {
            if let Some(hover_style) = hover_style {
                style = style.patch(hover_style);
            }
        }
        draw_box(
            &mut paint,
            rect,
            node.theme.subgraph.border_style,
            to_ratatui_style(style),
        );
        let header_x = rect.x.saturating_add(2);
        draw_text_clipped(
            &mut paint,
            header_x,
            rect.y,
            subgraph.label.as_ref(),
            rect.w.saturating_sub(4) as usize,
            to_ratatui_style(style.patch(node.theme.subgraph.header_style)),
        );
    }

    // Pass 1: merge bits across all overlapping edge cells so a confluence renders as a
    // junction glyph (`┬`, `┼`, …) instead of one edge overpainting another. The dominant
    // edge style for a cell follows the precedence Thick > Solid > Dashed; line/hover styling
    // tracks the dominant edge.
    let merged = merge_edge_cells(&node.output.edges, hovered.as_ref());
    for ((x, y), cell) in &merged {
        let glyphs = node.theme.edge_glyphs[cell.style.theme_index()];
        let border_style = glyphs.border_style;
        let raw_glyph = crate::widgets::common::box_glyphs::glyph_for_bits(cell.bits, border_style);
        let glyph = edge_glyph_char(raw_glyph, glyphs.horizontal, glyphs.vertical, ascii_theme);
        let mut line_style = base_style
            .patch(default_edge_style)
            .patch(cell.line_style.unwrap_or_default());
        if cell.hovered
            && let Some(hover_style) = hover_style
        {
            line_style = line_style.patch(hover_style);
        }
        draw_char(
            &mut paint,
            inner.x.saturating_add(*x),
            inner.y.saturating_add(*y),
            glyph,
            to_ratatui_style(line_style),
        );
    }

    // Pass 2: per-edge labels and arrowheads (these don't merge — each carries its own
    // identity).
    for edge in &node.output.edges {
        if matches!(edge.style, EdgeStyle::Invisible) {
            continue;
        }
        let mut style = base_style
            .patch(default_edge_style)
            .patch(edge.line_style.unwrap_or_default());
        if hovered.as_ref() == Some(&FlowchartItemPath::Edge(edge.index)) {
            if let Some(hover_style) = hover_style {
                style = style.patch(hover_style);
            }
        }
        let style = to_ratatui_style(style);
        if let (Some((x, y)), Some(label)) = (edge.label_pos, edge.label.as_ref()) {
            let label_style = to_ratatui_style(
                base_style
                    .patch(default_label_style)
                    .patch(edge.label_style.unwrap_or_default()),
            );
            draw_text_clipped(
                &mut paint,
                inner.x.saturating_add(x),
                inner.y.saturating_add(y),
                label,
                label.chars().count(),
                label_style,
            );
        }
        draw_arrow(
            &mut paint,
            node,
            edge.head_from,
            edge.head_from_pos,
            inner,
            style,
        );
        draw_arrow(
            &mut paint,
            node,
            edge.head_to,
            edge.head_to_pos,
            inner,
            style,
        );
    }

    for flow_node in &node.output.nodes {
        let rect = translate(flow_node.rect, inner.x, inner.y);
        let mut style = base_style.patch(default_node_style).patch(flow_node.style);
        if hovered.as_ref() == Some(&FlowchartItemPath::Node(flow_node.id.clone())) {
            if let Some(hover_style) = hover_style {
                style = style.patch(hover_style);
            }
            if let Some(node_hover_style) = flowchart_hover_style(flow_node.hover_style) {
                style = style.patch(node_hover_style);
            }
        }
        render_node_shape(
            &mut paint,
            rect,
            flow_node.shape,
            &flow_node.label_lines,
            node.theme.node_styles.border_style,
            to_ratatui_style(style),
            ascii_theme,
        );
    }
}

fn flowchart_hover_style(style: Style) -> Option<Style> {
    (!style.is_empty()).then_some(style)
}

struct PaintCtx<'a, 'b, 'c> {
    f: &'a mut ratatui::Frame<'b>,
    clip: &'c ClipBounds,
    bounds: &'c ClipBounds,
}

#[derive(Clone, Copy)]
struct MergedCell {
    bits: u8,
    style: EdgeStyle,
    line_style: Option<Style>,
    hovered: bool,
}

fn edge_style_priority(style: EdgeStyle) -> u8 {
    match style {
        EdgeStyle::Thick => 3,
        EdgeStyle::Solid => 2,
        EdgeStyle::Dashed => 1,
        EdgeStyle::Invisible => 0,
    }
}

fn merge_edge_cells(
    edges: &[crate::widgets::internal::PositionedEdge],
    hovered: Option<&FlowchartItemPath>,
) -> std::collections::HashMap<(i16, i16), MergedCell> {
    let mut grid: std::collections::HashMap<(i16, i16), MergedCell> =
        std::collections::HashMap::new();
    for edge in edges {
        if matches!(edge.style, EdgeStyle::Invisible) {
            continue;
        }
        let edge_hovered = hovered == Some(&FlowchartItemPath::Edge(edge.index));
        for cell in &edge.cells {
            let entry = grid.entry((cell.x, cell.y)).or_insert(MergedCell {
                bits: 0,
                style: edge.style,
                line_style: edge.line_style,
                hovered: edge_hovered,
            });
            entry.bits |= cell.bits;
            if edge_style_priority(edge.style) >= edge_style_priority(entry.style) {
                entry.style = edge.style;
                entry.line_style = edge.line_style;
            }
            entry.hovered = entry.hovered || edge_hovered;
        }
    }
    grid
}

fn translate(rect: Rect, dx: i16, dy: i16) -> Rect {
    Rect {
        x: rect.x.saturating_add(dx),
        y: rect.y.saturating_add(dy),
        w: rect.w,
        h: rect.h,
    }
}

fn fill_rect(paint: &mut PaintCtx<'_, '_, '_>, rect: Rect, style: ratatui::style::Style) {
    for y in rect.y..rect.y.saturating_add(rect.h as i16) {
        for x in rect.x..rect.x.saturating_add(rect.w as i16) {
            draw_char(paint, x, y, ' ', style);
        }
    }
}

fn draw_outer_border(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    borders: Borders,
    border_style: crate::style::BorderStyle,
    style: Style,
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

fn render_node_shape(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    shape: NodeShape,
    label_lines: &[std::sync::Arc<str>],
    border_style: crate::style::BorderStyle,
    style: ratatui::style::Style,
    ascii: bool,
) {
    fill_rect(paint, rect, style);
    if ascii {
        draw_ascii_box(paint, rect, style);
        draw_centered_lines(paint, rect, label_lines, style);
        return;
    }
    match shape {
        NodeShape::Round | NodeShape::Stadium => {
            draw_box(paint, rect, crate::style::BorderStyle::Rounded, style)
        }
        NodeShape::Rect => draw_box(paint, rect, border_style, style),
        NodeShape::Subroutine => draw_subroutine(paint, rect, border_style, style),
        NodeShape::Cylinder => draw_cylinder(paint, rect, style),
        NodeShape::Circle | NodeShape::DoubleCircle => draw_circle_like(paint, rect, shape, style),
        NodeShape::Diamond => draw_diamond(paint, rect, style),
        NodeShape::Hexagon => draw_hexagon(paint, rect, style),
        NodeShape::Asymmetric => draw_asymmetric(paint, rect, style),
        NodeShape::Parallelogram
        | NodeShape::ParallelogramAlt
        | NodeShape::Trapezoid
        | NodeShape::TrapezoidAlt => draw_slanted(paint, rect, shape, style),
    }
    draw_centered_lines(paint, rect, label_lines, style);
}

fn edge_glyph_char(source: char, horizontal: char, vertical: char, ascii: bool) -> char {
    match source {
        '─' => horizontal,
        '│' => vertical,
        '┌' | '┐' | '└' | '┘' | '╭' | '╮' | '╰' | '╯' | '├' | '┤' | '┬' | '┴' | '┼' if ascii => {
            '+'
        }
        other => other,
    }
}

fn draw_ascii_box(paint: &mut PaintCtx<'_, '_, '_>, rect: Rect, style: ratatui::style::Style) {
    if rect.w < 2 || rect.h < 2 {
        return;
    }
    let left = rect.x;
    let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
    let top = rect.y;
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);
    draw_char(paint, left, top, '+', style);
    draw_char(paint, right, top, '+', style);
    draw_char(paint, left, bottom, '+', style);
    draw_char(paint, right, bottom, '+', style);
    for x in left.saturating_add(1)..right {
        draw_char(paint, x, top, '-', style);
        draw_char(paint, x, bottom, '-', style);
    }
    for y in top.saturating_add(1)..bottom {
        draw_char(paint, left, y, '|', style);
        draw_char(paint, right, y, '|', style);
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

fn draw_subroutine(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    border_style: crate::style::BorderStyle,
    style: ratatui::style::Style,
) {
    draw_box(paint, rect, border_style, style);
    if rect.w <= 4 || rect.h <= 2 {
        return;
    }

    let left_inner = rect.x.saturating_add(1);
    let right_inner = rect.x.saturating_add(rect.w as i16).saturating_sub(2);
    for y in rect.y.saturating_add(1)..rect.y.saturating_add(rect.h as i16).saturating_sub(1) {
        draw_char(paint, left_inner, y, '│', style);
        draw_char(paint, right_inner, y, '│', style);
    }
}

fn draw_cylinder(paint: &mut PaintCtx<'_, '_, '_>, rect: Rect, style: ratatui::style::Style) {
    draw_box(paint, rect, crate::style::BorderStyle::Rounded, style);
    if rect.w <= 4 || rect.h <= 2 {
        return;
    }

    let left = rect.x;
    let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
    let top_inner = rect.y.saturating_add(1);

    // A compact database cue: curved sides remain visible even in 3-row nodes,
    // while taller nodes also get an inner top ellipse line.
    draw_char(paint, left, top_inner, '(', style);
    draw_char(paint, right, top_inner, ')', style);
    if rect.h > 3 {
        for x in left.saturating_add(1)..right {
            draw_char(paint, x, top_inner, '─', style);
        }
    }
}

fn draw_circle_like(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    shape: NodeShape,
    style: ratatui::style::Style,
) {
    draw_box(paint, rect, crate::style::BorderStyle::Rounded, style);
    if matches!(shape, NodeShape::DoubleCircle) && rect.w > 4 && rect.h > 4 {
        draw_box(
            paint,
            Rect {
                x: rect.x.saturating_add(1),
                y: rect.y.saturating_add(1),
                w: rect.w.saturating_sub(2),
                h: rect.h.saturating_sub(2),
            },
            crate::style::BorderStyle::Rounded,
            style,
        );
    }
}

fn draw_diamond(paint: &mut PaintCtx<'_, '_, '_>, rect: Rect, style: ratatui::style::Style) {
    let cx = rect.x.saturating_add(rect.w as i16 / 2);
    let top = rect.y;
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);
    let cy = rect.y.saturating_add(rect.h as i16 / 2);
    draw_char(paint, cx, top, '╱', style);
    draw_char(paint, cx.saturating_add(1), top, '╲', style);
    draw_char(paint, rect.x, cy, '╲', style);
    draw_char(
        paint,
        rect.x.saturating_add(rect.w as i16).saturating_sub(1),
        cy,
        '╱',
        style,
    );
    draw_char(paint, cx, bottom, '╲', style);
    draw_char(paint, cx.saturating_add(1), bottom, '╱', style);
}

fn draw_hexagon(paint: &mut PaintCtx<'_, '_, '_>, rect: Rect, style: ratatui::style::Style) {
    draw_slanted(paint, rect, NodeShape::Parallelogram, style);
    let top = rect.y;
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);
    for x in rect.x.saturating_add(2)..rect.x.saturating_add(rect.w as i16).saturating_sub(2) {
        draw_char(paint, x, top, '─', style);
        draw_char(paint, x, bottom, '─', style);
    }
}

fn draw_asymmetric(paint: &mut PaintCtx<'_, '_, '_>, rect: Rect, style: ratatui::style::Style) {
    draw_box(paint, rect, crate::style::BorderStyle::Plain, style);
    if rect.h > 1 {
        draw_char(paint, rect.x, rect.y, '>', style);
    }
}

fn draw_slanted(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    shape: NodeShape,
    style: ratatui::style::Style,
) {
    if rect.w < 2 || rect.h < 2 {
        return;
    }
    let left_top = matches!(shape, NodeShape::ParallelogramAlt | NodeShape::TrapezoidAlt);
    let right_top = matches!(shape, NodeShape::Parallelogram | NodeShape::TrapezoidAlt);
    let left = rect.x;
    let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
    let top = rect.y;
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);
    for x in left.saturating_add(1)..right {
        draw_char(paint, x, top, '─', style);
        draw_char(paint, x, bottom, '─', style);
    }
    draw_char(paint, left, top, if left_top { '╲' } else { '╱' }, style);
    draw_char(paint, left, bottom, if left_top { '╱' } else { '╲' }, style);
    draw_char(paint, right, top, if right_top { '╱' } else { '╲' }, style);
    draw_char(
        paint,
        right,
        bottom,
        if right_top { '╲' } else { '╱' },
        style,
    );
}

fn draw_centered_lines(
    paint: &mut PaintCtx<'_, '_, '_>,
    rect: Rect,
    lines: &[std::sync::Arc<str>],
    style: ratatui::style::Style,
) {
    let content_h = rect.h.saturating_sub(2);
    let start_y = rect
        .y
        .saturating_add(1)
        .saturating_add((content_h.saturating_sub(lines.len() as u16) / 2) as i16);
    for (row, line) in lines.iter().enumerate() {
        let width = line.chars().count().min(rect.w as usize);
        let x = rect
            .x
            .saturating_add(((rect.w as usize).saturating_sub(width) / 2) as i16);
        draw_text_clipped(
            paint,
            x,
            start_y.saturating_add(row as i16),
            line,
            width,
            style,
        );
    }
}

fn draw_arrow(
    paint: &mut PaintCtx<'_, '_, '_>,
    node: &FlowchartNode,
    arrow: EdgeArrow,
    pos: Option<(i16, i16, FlowDirection)>,
    inner: Rect,
    style: ratatui::style::Style,
) {
    if matches!(arrow, EdgeArrow::None) {
        return;
    }
    let Some((x, y, direction)) = pos else {
        return;
    };
    let glyphs = node.theme.arrow_heads[arrow.theme_index()];
    let ch = match direction {
        FlowDirection::TopDown => glyphs.down,
        FlowDirection::BottomUp => glyphs.up,
        FlowDirection::LeftRight => glyphs.right,
        FlowDirection::RightLeft => glyphs.left,
    };
    draw_char(
        paint,
        inner.x.saturating_add(x),
        inner.y.saturating_add(y),
        ch,
        style,
    );
}

fn draw_text_clipped(
    paint: &mut PaintCtx<'_, '_, '_>,
    mut x: i16,
    y: i16,
    text: &str,
    available: usize,
    style: ratatui::style::Style,
) {
    for ch in text.chars().take(available) {
        draw_char(paint, x, y, ch, style);
        x = x.saturating_add(1);
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

    use super::flowchart_hover_style;

    #[test]
    fn transform_only_hover_style_does_not_gain_accent_color() {
        let style = flowchart_hover_style(Style::new().lighten_by(0.0))
            .expect("transform-only hover style should be non-empty");

        assert_eq!(style.fg, None);
        assert_eq!(style.bg, None);
        assert_eq!(style.fg_transform, Some(ColorTransform::Lighten(0.0)));
        assert_eq!(style.bg_transform, Some(ColorTransform::Lighten(0.0)));
    }

    #[test]
    fn concrete_hover_style_is_preserved() {
        let style = flowchart_hover_style(Style::new().fg(Color::Black).bg(Color::LightCyan))
            .expect("concrete hover style should be non-empty");

        assert_eq!(style.fg, Some(crate::style::Paint::Solid(Color::Black)));
        assert_eq!(style.bg, Some(crate::style::Paint::Solid(Color::LightCyan)));
    }
}
