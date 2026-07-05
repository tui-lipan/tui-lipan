use crate::backend::ratatui_backend::common::{
    ClipBounds, DrawCellStyledCtx, draw_cell_styled, style_paints_bg,
};
use crate::style::resolve::{resolve_base_style, resolve_muted_style};
use crate::style::{Padding, Rect, Style, Theme};
use crate::widgets::common::box_glyphs::glyph_for_bits;
use crate::widgets::common::simple_diagram::{
    EndpointGlyph, SimpleDiagramBox, SimpleDiagramEdge, SimpleDiagramOutput, normalized_dividers,
};
use unicode_width::UnicodeWidthChar;

pub(crate) struct SimpleDiagramRenderCtx<'a> {
    pub style: Style,
    pub padding: Padding,
    pub node_padding: Padding,
    pub boxes: &'a [SimpleDiagramBox],
    pub edges: &'a [SimpleDiagramEdge],
    pub output: &'a SimpleDiagramOutput,
}

pub(crate) fn render_simple_diagram(
    f: &mut ratatui::Frame<'_>,
    theme: &Theme,
    rect: Rect,
    clip_rect: Rect,
    ctx: SimpleDiagramRenderCtx<'_>,
) {
    let SimpleDiagramRenderCtx {
        style,
        padding,
        node_padding,
        boxes,
        edges,
        output,
    } = ctx;
    let base_style = resolve_base_style(theme, style);
    let line_style = resolve_muted_style(theme, Style::default());
    let outer_clip_rect = rect.intersection(&clip_rect);
    if !outer_clip_rect.is_empty() && style_paints_bg(base_style) {
        let clip = ClipBounds::from_rect(outer_clip_rect);
        fill_rect(f, &clip, rect, base_style);
    }
    let inner = rect.inner(false, padding);
    let content_clip = inner.intersection(&clip_rect);
    if content_clip.is_empty() {
        return;
    }
    let clip = ClipBounds::from_rect(content_clip);
    let bounds = ClipBounds::from_rrect(f.area());

    for edge in &output.edges {
        let Some(spec) = edges.get(edge.spec_index) else {
            continue;
        };
        let edge_style = base_style.patch(line_style).patch(spec.line_style);
        for cell in &edge.cells {
            let ch = edge_glyph(cell.bits, spec.dashed);
            draw_char(
                f,
                &clip,
                &bounds,
                inner.x.saturating_add(cell.x),
                inner.y.saturating_add(cell.y),
                ch,
                edge_style,
            );
        }
        if let Some((x, y)) = edge.from_pos {
            draw_endpoint(
                f,
                &clip,
                &bounds,
                inner.x.saturating_add(x),
                inner.y.saturating_add(y),
                spec.from_glyph,
                edge_style,
            );
        }
        if let Some((x, y)) = edge.to_pos {
            draw_endpoint(
                f,
                &clip,
                &bounds,
                inner.x.saturating_add(x),
                inner.y.saturating_add(y),
                spec.to_glyph,
                edge_style,
            );
        }
    }

    for node in &output.boxes {
        let Some(spec) = boxes.get(node.spec_index) else {
            continue;
        };
        let fill_style = base_style.patch(spec.fill_style);
        let border_style = fill_style.patch(spec.border_style_fg);
        let label_style = fill_style.patch(spec.label_style);
        let r = Rect {
            x: inner.x.saturating_add(node.rect.x),
            y: inner.y.saturating_add(node.rect.y),
            w: node.rect.w,
            h: node.rect.h,
        };
        if style_paints_bg(spec.fill_style) {
            fill_rect(f, &clip, r, fill_style);
        }
        draw_box(f, &clip, &bounds, r, spec.border_style, border_style);
        let content = r.inner(true, node_padding);
        let dividers = normalized_dividers(&spec.divider_after, spec.rows.len());
        for (row_idx, row) in spec.rows.iter().enumerate() {
            let consumed_dividers = dividers
                .iter()
                .filter(|divider| **divider < row_idx)
                .count() as i16;
            draw_text(
                f,
                &clip,
                &bounds,
                content.x,
                content
                    .y
                    .saturating_add(row_idx as i16)
                    .saturating_add(consumed_dividers),
                row,
                label_style,
            );
        }
        for (divider_idx, divider) in dividers.iter().enumerate() {
            let y = content
                .y
                .saturating_add(*divider as i16)
                .saturating_add(1)
                .saturating_add(divider_idx as i16);
            if y > r.y && y < r.y.saturating_add(r.h as i16).saturating_sub(1) {
                for dx in 1..r.w.saturating_sub(1) {
                    draw_char(
                        f,
                        &clip,
                        &bounds,
                        r.x.saturating_add(dx as i16),
                        y,
                        '─',
                        border_style,
                    );
                }
                draw_char(f, &clip, &bounds, r.x, y, '├', border_style);
                draw_char(
                    f,
                    &clip,
                    &bounds,
                    r.x.saturating_add(r.w as i16).saturating_sub(1),
                    y,
                    '┤',
                    border_style,
                );
            }
        }
    }

    for edge in &output.edges {
        let Some(spec) = edges.get(edge.spec_index) else {
            continue;
        };
        if let (Some((x, y)), Some(label)) = (edge.from_label_pos, spec.from_label.as_ref()) {
            clear_label_flanks(
                f,
                &clip,
                &bounds,
                inner.x.saturating_add(x),
                inner.y.saturating_add(y),
                label,
                base_style.patch(spec.label_style),
            );
            draw_text(
                f,
                &clip,
                &bounds,
                inner.x.saturating_add(x),
                inner.y.saturating_add(y),
                label,
                base_style.patch(spec.label_style),
            );
        }
        if let (Some((x, y)), Some(label)) = (edge.to_label_pos, spec.to_label.as_ref()) {
            clear_label_flanks(
                f,
                &clip,
                &bounds,
                inner.x.saturating_add(x),
                inner.y.saturating_add(y),
                label,
                base_style.patch(spec.label_style),
            );
            draw_text(
                f,
                &clip,
                &bounds,
                inner.x.saturating_add(x),
                inner.y.saturating_add(y),
                label,
                base_style.patch(spec.label_style),
            );
        }
        if let (Some((x, y)), Some(label)) = (edge.label_pos, spec.label.as_ref()) {
            clear_label_flanks(
                f,
                &clip,
                &bounds,
                inner.x.saturating_add(x),
                inner.y.saturating_add(y),
                label,
                base_style.patch(spec.label_style),
            );
            draw_text(
                f,
                &clip,
                &bounds,
                inner.x.saturating_add(x),
                inner.y.saturating_add(y),
                label,
                base_style.patch(spec.label_style),
            );
        }
    }
}

fn draw_endpoint(
    f: &mut ratatui::Frame<'_>,
    clip: &ClipBounds,
    bounds: &ClipBounds,
    x: i16,
    y: i16,
    glyph: EndpointGlyph,
    style: Style,
) {
    let text = match glyph {
        EndpointGlyph::None => return,
        EndpointGlyph::Arrow => "▶",
        EndpointGlyph::Triangle => "▷",
        EndpointGlyph::Diamond => "◆",
        EndpointGlyph::Circle => "○",
        EndpointGlyph::CrowZeroOrOne => "o|",
        EndpointGlyph::CrowExactlyOne => "||",
        EndpointGlyph::CrowZeroOrMore => "}o",
        EndpointGlyph::CrowOneOrMore => "}|",
    };
    draw_text(f, clip, bounds, x, y, text, style);
}

fn edge_glyph(bits: u8, dashed: bool) -> char {
    if dashed {
        let horizontal = bits
            & (crate::widgets::common::box_glyphs::EAST | crate::widgets::common::box_glyphs::WEST)
            != 0;
        let vertical = bits
            & (crate::widgets::common::box_glyphs::NORTH
                | crate::widgets::common::box_glyphs::SOUTH)
            != 0;
        match (horizontal, vertical) {
            (true, false) => '┄',
            (false, true) => '┆',
            _ => glyph_for_bits(bits, crate::style::BorderStyle::Plain),
        }
    } else {
        glyph_for_bits(bits, crate::style::BorderStyle::Plain)
    }
}
fn fill_rect(f: &mut ratatui::Frame<'_>, clip: &ClipBounds, rect: Rect, style: Style) {
    for y in rect.y..rect.y.saturating_add(rect.h as i16) {
        for x in rect.x..rect.x.saturating_add(rect.w as i16) {
            draw_char(f, clip, clip, x, y, ' ', style);
        }
    }
}
fn draw_box(
    f: &mut ratatui::Frame<'_>,
    clip: &ClipBounds,
    bounds: &ClipBounds,
    rect: Rect,
    border: crate::style::BorderStyle,
    style: Style,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    let (tl, tr, bl, br, h, v) = match border {
        crate::style::BorderStyle::Rounded => ('╭', '╮', '╰', '╯', '─', '│'),
        crate::style::BorderStyle::Double => ('╔', '╗', '╚', '╝', '═', '║'),
        _ => ('┌', '┐', '└', '┘', '─', '│'),
    };
    let right = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
    let bottom = rect.y.saturating_add(rect.h as i16).saturating_sub(1);
    draw_char(f, clip, bounds, rect.x, rect.y, tl, style);
    draw_char(f, clip, bounds, right, rect.y, tr, style);
    draw_char(f, clip, bounds, rect.x, bottom, bl, style);
    draw_char(f, clip, bounds, right, bottom, br, style);
    for x in rect.x.saturating_add(1)..right {
        draw_char(f, clip, bounds, x, rect.y, h, style);
        draw_char(f, clip, bounds, x, bottom, h, style);
    }
    for y in rect.y.saturating_add(1)..bottom {
        draw_char(f, clip, bounds, rect.x, y, v, style);
        draw_char(f, clip, bounds, right, y, v, style);
    }
}
fn draw_text(
    f: &mut ratatui::Frame<'_>,
    clip: &ClipBounds,
    bounds: &ClipBounds,
    mut x: i16,
    y: i16,
    text: &str,
    style: Style,
) {
    for ch in text.chars() {
        let width = ch.width().unwrap_or(0);
        draw_char(f, clip, bounds, x, y, ch, style);
        x = x.saturating_add(width as i16);
    }
}

fn clear_label_flanks(
    f: &mut ratatui::Frame<'_>,
    clip: &ClipBounds,
    bounds: &ClipBounds,
    x: i16,
    y: i16,
    text: &str,
    style: Style,
) {
    let width = text
        .chars()
        .map(|ch| ch.width().unwrap_or(0))
        .sum::<usize>() as i16;
    draw_char(f, clip, bounds, x.saturating_sub(1), y, ' ', style);
    draw_char(f, clip, bounds, x.saturating_add(width), y, ' ', style);
}

fn draw_char(
    f: &mut ratatui::Frame<'_>,
    clip: &ClipBounds,
    bounds: &ClipBounds,
    x: i16,
    y: i16,
    ch: char,
    style: Style,
) {
    let x_i = i32::from(x);
    let y_i = i32::from(y);
    if !clip.contains(x_i, y_i) || !bounds.contains(x_i, y_i) {
        return;
    }
    let mut symbol = [0; 4];
    let symbol = ch.encode_utf8(&mut symbol);
    draw_cell_styled(
        f.buffer_mut(),
        x_i,
        y_i,
        symbol,
        style,
        DrawCellStyledCtx {
            clip,
            buf_bounds: bounds,
            terminal_bg: None,
        },
    );
}
