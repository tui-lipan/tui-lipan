use ratatui::buffer::Buffer;
use ratatui::symbols::merge::MergeStrategy;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::backend::ratatui_backend::common::{
    ClipBounds, border_horizontal_char, border_tabs_title_line, clear_fg_preserve_bg_clipped,
    fill_rect_clipped_style, render_line_clipped, richtext_to_spans, style_paints_bg,
    style_uses_backdrop_bg, to_ratatui_border_set, to_ratatui_style,
    to_ratatui_style_with_terminal_bg, truncate_spans,
};
use crate::backend::ratatui_backend::renderers::frame::utils::build_header_line;
use crate::style::{Color, Edge, Paint, Rect, Style};
use crate::widgets::internal::{FrameGeometry, FrameProps};
use crate::widgets::{BorderMergeMode, DecorationGlyph, DecorationPlacement, EdgeDecoration};

pub(crate) struct FrameRenderCtx {
    pub active: bool,
    pub is_hovered: bool,
    pub clip_rect: Option<Rect>,
    pub terminal_bg: Option<Color>,
}

struct BorderCellDraw<'a> {
    style: ratatui::style::Style,
    clip: &'a ClipBounds,
    buf_bounds: &'a ClipBounds,
    border_merge_mode: BorderMergeMode,
}

struct CapDraw<'a> {
    style: ratatui::style::Style,
    clip: &'a ClipBounds,
    buf_bounds: &'a ClipBounds,
    is_start: bool,
    border_merge_mode: BorderMergeMode,
}

pub(crate) fn render_frame(
    f: &mut ratatui::Frame<'_>,
    props: &FrameProps,
    geometry: &FrameGeometry,
    ctx: FrameRenderCtx,
) {
    let body_rect = geometry.body_rect;

    let transparent_decoration_bg_snapshot =
        snapshot_transparent_decoration_backgrounds(f, props, geometry, &ctx);

    if props.border {
        render_border_frame(f, props, geometry, &ctx);
    } else {
        render_plain_frame(f, props, body_rect, &ctx);
        render_plain_frame_status(f, props, geometry, &ctx);
    }

    render_frame_decorations(f, props, geometry, &ctx);

    restore_decoration_backgrounds(f, &transparent_decoration_bg_snapshot, ctx.clip_rect);
}

#[derive(Clone, Debug)]
struct DecorationBackgroundSnapshot {
    rect: Rect,
    colors: Vec<ratatui::style::Color>,
}

fn snapshot_transparent_decoration_backgrounds(
    f: &mut ratatui::Frame<'_>,
    props: &FrameProps,
    geometry: &FrameGeometry,
    ctx: &FrameRenderCtx,
) -> Vec<DecorationBackgroundSnapshot> {
    if props.decorations.is_empty() {
        return Vec::new();
    }

    let content_rect = geometry.content_rect;
    let mut outside_offsets = crate::style::Padding::default();
    let mut snapshots = Vec::new();
    let buf = f.buffer_mut();
    let bounds = ClipBounds::from_rrect(buf.area);

    for decoration in &props.decorations {
        let style = resolve_edge_decoration_style(decoration, ctx.active, ctx.is_hovered);
        if !matches!(style.bg, Some(bg) if bg.is_transparent_sentinel()) {
            continue;
        }

        let mut band_rect = match decoration.placement {
            DecorationPlacement::Outside => {
                decoration_band_outside(geometry.outer_rect, decoration, &mut outside_offsets)
            }
            DecorationPlacement::Border => {
                decoration_band_border(geometry.outer_rect, geometry.body_rect, decoration)
            }
            DecorationPlacement::Inside => decoration_band_on_rect(content_rect, decoration),
        };

        if let Some(clip) = ctx.clip_rect {
            band_rect = band_rect.intersection(&clip);
        }
        if band_rect.is_empty() {
            continue;
        }

        let mut colors = Vec::with_capacity(band_rect.w as usize * band_rect.h as usize);
        for y in band_rect.y..band_rect.y.saturating_add(band_rect.h as i16) {
            for x in band_rect.x..band_rect.x.saturating_add(band_rect.w as i16) {
                if !bounds.contains(x as i32, y as i32) {
                    colors.push(ratatui::style::Color::Reset);
                    continue;
                }
                let color = buf
                    .cell((x as u16, y as u16))
                    .map(|cell| cell.bg)
                    .unwrap_or(ratatui::style::Color::Reset);
                colors.push(color);
            }
        }
        snapshots.push(DecorationBackgroundSnapshot {
            rect: band_rect,
            colors,
        });
    }

    snapshots
}

fn restore_decoration_backgrounds(
    f: &mut ratatui::Frame<'_>,
    snapshots: &[DecorationBackgroundSnapshot],
    clip_rect: Option<Rect>,
) {
    if snapshots.is_empty() {
        return;
    }

    let buf = f.buffer_mut();
    let bounds = ClipBounds::from_rrect(buf.area);
    for snapshot in snapshots {
        let mut rect = snapshot.rect;
        if let Some(clip) = clip_rect {
            rect = rect.intersection(&clip);
        }
        if rect.is_empty() {
            continue;
        }

        let mut i = 0usize;
        for y in snapshot.rect.y..snapshot.rect.y.saturating_add(snapshot.rect.h as i16) {
            for x in snapshot.rect.x..snapshot.rect.x.saturating_add(snapshot.rect.w as i16) {
                let saved_bg = snapshot.colors[i];
                i += 1;
                if !rect.contains(x, y) || !bounds.contains(x as i32, y as i32) {
                    continue;
                }
                if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
                    cell.bg = saved_bg;
                }
            }
        }
    }
}

fn render_frame_decorations(
    f: &mut ratatui::Frame<'_>,
    props: &FrameProps,
    geometry: &FrameGeometry,
    ctx: &FrameRenderCtx,
) {
    if props.decorations.is_empty() {
        return;
    }

    let content_rect = geometry.content_rect;

    let buf = f.buffer_mut();
    let clip = ctx
        .clip_rect
        .map(ClipBounds::from_rect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);

    let mut outside_offsets = crate::style::Padding::default();

    for decoration in &props.decorations {
        let band_rect = match decoration.placement {
            DecorationPlacement::Outside => {
                decoration_band_outside(geometry.outer_rect, decoration, &mut outside_offsets)
            }
            DecorationPlacement::Border => {
                decoration_band_border(geometry.outer_rect, geometry.body_rect, decoration)
            }
            DecorationPlacement::Inside => decoration_band_on_rect(content_rect, decoration),
        };

        if band_rect.w == 0 || band_rect.h == 0 {
            continue;
        }

        let decoration_merge_mode =
            if props.border && matches!(decoration.placement, DecorationPlacement::Border) {
                props.border_merge_mode
            } else {
                BorderMergeMode::Replace
            };

        let style = resolve_edge_decoration_style(decoration, ctx.active, ctx.is_hovered);
        let rstyle = to_ratatui_style_with_terminal_bg(style, ctx.terminal_bg);
        let symbol = decoration.glyph.resolve(decoration.edge).to_string();
        draw_symbol_rect(
            buf,
            band_rect,
            &symbol,
            rstyle,
            &clip,
            &buf_bounds,
            decoration_merge_mode,
        );

        if let Some(cap) = decoration.cap_start {
            draw_cap(
                buf,
                band_rect,
                decoration.edge,
                cap,
                &CapDraw {
                    style: rstyle,
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    is_start: true,
                    border_merge_mode: decoration_merge_mode,
                },
            );
        }
        if let Some(cap) = decoration.cap_end {
            draw_cap(
                buf,
                band_rect,
                decoration.edge,
                cap,
                &CapDraw {
                    style: rstyle,
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    is_start: false,
                    border_merge_mode: decoration_merge_mode,
                },
            );
        }
    }
}

fn draw_cap(
    buf: &mut Buffer,
    band: Rect,
    edge: crate::style::Edge,
    glyph: DecorationGlyph,
    draw: &CapDraw<'_>,
) {
    if band.w == 0 || band.h == 0 {
        return;
    }
    let symbol = glyph.resolve(edge).to_string();
    let cap_rect = match edge {
        Edge::Left | Edge::Right => {
            let y = if draw.is_start {
                band.y
            } else {
                band.y.saturating_add(band.h as i16).saturating_sub(1)
            };
            Rect {
                x: band.x,
                y,
                w: band.w,
                h: 1,
            }
        }
        Edge::Top | Edge::Bottom => {
            let x = if draw.is_start {
                band.x
            } else {
                band.x.saturating_add(band.w as i16).saturating_sub(1)
            };
            Rect {
                x,
                y: band.y,
                w: 1,
                h: band.h,
            }
        }
    };
    draw_symbol_rect(
        buf,
        cap_rect,
        &symbol,
        draw.style,
        draw.clip,
        draw.buf_bounds,
        draw.border_merge_mode,
    );
}

pub(crate) fn resolve_edge_decoration_style(
    decoration: &EdgeDecoration,
    active: bool,
    is_hovered: bool,
) -> Style {
    let mut style = decoration.style;
    if is_hovered && let Some(hover_style) = decoration.hover_style {
        style = style.patch(hover_style);
    }
    if active && let Some(focus_style) = decoration.focus_style {
        style = style.patch(focus_style);
    }
    if style.fg.is_none() {
        style.fg = Some(Paint::Solid(Color::Reset));
    }
    if style.bg.is_none() {
        style.bg = Some(Paint::Solid(Color::Reset));
    }
    style
}

fn decoration_band_outside(
    rect: Rect,
    decoration: &EdgeDecoration,
    offsets: &mut crate::style::Padding,
) -> Rect {
    let thickness = decoration.thickness.max(1);
    match decoration.edge {
        Edge::Top => {
            let y = rect.y.saturating_add(offsets.top as i16);
            offsets.top = offsets.top.saturating_add(thickness);
            Rect {
                x: rect.x,
                y,
                w: rect.w,
                h: thickness.min(rect.h),
            }
        }
        Edge::Bottom => {
            let h = thickness.min(rect.h);
            let y = rect
                .y
                .saturating_add(rect.h as i16)
                .saturating_sub(h as i16)
                .saturating_sub(offsets.bottom as i16);
            offsets.bottom = offsets.bottom.saturating_add(thickness);
            Rect {
                x: rect.x,
                y,
                w: rect.w,
                h,
            }
        }
        Edge::Left => {
            let x = rect.x.saturating_add(offsets.left as i16);
            offsets.left = offsets.left.saturating_add(thickness);
            Rect {
                x,
                y: rect.y,
                w: thickness.min(rect.w),
                h: rect.h,
            }
        }
        Edge::Right => {
            let w = thickness.min(rect.w);
            let x = rect
                .x
                .saturating_add(rect.w as i16)
                .saturating_sub(w as i16)
                .saturating_sub(offsets.right as i16);
            offsets.right = offsets.right.saturating_add(thickness);
            Rect {
                x,
                y: rect.y,
                w,
                h: rect.h,
            }
        }
    }
}

fn decoration_band_on_rect(rect: Rect, decoration: &EdgeDecoration) -> Rect {
    let thickness = decoration.thickness.max(1);
    match decoration.edge {
        Edge::Top => Rect {
            x: rect.x,
            y: rect.y,
            w: rect.w,
            h: thickness.min(rect.h),
        },
        Edge::Bottom => Rect {
            x: rect.x,
            y: rect
                .y
                .saturating_add(rect.h as i16)
                .saturating_sub(thickness.min(rect.h) as i16),
            w: rect.w,
            h: thickness.min(rect.h),
        },
        Edge::Left => Rect {
            x: rect.x,
            y: rect.y,
            w: thickness.min(rect.w),
            h: rect.h,
        },
        Edge::Right => Rect {
            x: rect
                .x
                .saturating_add(rect.w as i16)
                .saturating_sub(thickness.min(rect.w) as i16),
            y: rect.y,
            w: thickness.min(rect.w),
            h: rect.h,
        },
    }
}

fn decoration_band_border(frame_rect: Rect, _body_rect: Rect, decoration: &EdgeDecoration) -> Rect {
    let thickness = decoration.thickness.max(1);
    match decoration.edge {
        Edge::Top => Rect {
            x: frame_rect.x,
            y: frame_rect.y,
            w: frame_rect.w,
            h: thickness.min(frame_rect.h),
        },
        Edge::Bottom => Rect {
            x: frame_rect.x,
            y: frame_rect
                .y
                .saturating_add(frame_rect.h as i16)
                .saturating_sub(thickness.min(frame_rect.h) as i16),
            w: frame_rect.w,
            h: thickness.min(frame_rect.h),
        },
        Edge::Left => Rect {
            x: frame_rect.x,
            y: frame_rect.y,
            w: thickness.min(frame_rect.w),
            h: frame_rect.h,
        },
        Edge::Right => Rect {
            x: frame_rect
                .x
                .saturating_add(frame_rect.w as i16)
                .saturating_sub(thickness.min(frame_rect.w) as i16),
            y: frame_rect.y,
            w: thickness.min(frame_rect.w),
            h: frame_rect.h,
        },
    }
}

fn is_box_drawing_symbol(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    let Some(ch) = chars.next() else {
        return false;
    };
    chars.next().is_none() && (0x2500..=0x257F).contains(&(ch as u32))
}

fn to_merge_strategy(strategy: BorderMergeMode) -> MergeStrategy {
    match strategy {
        BorderMergeMode::Replace => MergeStrategy::Replace,
        BorderMergeMode::Exact => MergeStrategy::Exact,
        BorderMergeMode::Fuzzy => MergeStrategy::Fuzzy,
    }
}

fn draw_border_cell(buf: &mut Buffer, x: i32, y: i32, symbol: &str, draw: &BorderCellDraw<'_>) {
    if !draw.clip.contains(x, y) || !draw.buf_bounds.contains(x, y) {
        return;
    }
    let Some(cell) = buf.cell_mut((x as u16, y as u16)) else {
        return;
    };

    let should_merge = draw.border_merge_mode != BorderMergeMode::Replace
        && is_box_drawing_symbol(cell.symbol())
        && is_box_drawing_symbol(symbol);

    if should_merge {
        cell.merge_symbol(symbol, to_merge_strategy(draw.border_merge_mode));
    } else {
        cell.set_symbol(symbol);
    }
    cell.set_style(draw.style);
}

fn draw_symbol_rect(
    buf: &mut Buffer,
    rect: Rect,
    symbol: &str,
    style: ratatui::style::Style,
    clip: &ClipBounds,
    buf_bounds: &ClipBounds,
    border_merge_mode: BorderMergeMode,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }
    let x = rect.x as i32;
    let y = rect.y as i32;
    let w = rect.w as i32;
    let h = rect.h as i32;
    let start_x = x.max(clip.min_x);
    let end_x = (x + w - 1).min(clip.max_x);
    let start_y = y.max(clip.min_y);
    let end_y = (y + h - 1).min(clip.max_y);
    let border_draw = BorderCellDraw {
        style,
        clip,
        buf_bounds,
        border_merge_mode,
    };

    for cy in start_y..=end_y {
        for cx in start_x..=end_x {
            draw_border_cell(buf, cx, cy, symbol, &border_draw);
        }
    }
}

pub(crate) fn resolve_block_style(
    props: &FrameProps,
    active: bool,
    is_hovered: bool,
) -> (Style, crate::style::BorderStyle) {
    let mut block_style = props.style;
    let mut border_style = props.border_style;

    if active && let Some(fbs) = props.focus_border_style() {
        border_style = fbs;
    }

    if is_hovered && let Some(hs) = props.hover_style() {
        block_style = block_style.patch(hs);
    }

    if active && let Some(fs) = props.focus_style() {
        block_style = block_style.patch(fs);
    }

    (block_style, border_style)
}

fn render_border_frame(
    f: &mut ratatui::Frame<'_>,
    props: &FrameProps,
    geometry: &FrameGeometry,
    ctx: &FrameRenderCtx,
) {
    let rect = geometry.frame_rect;
    let join_overlap = geometry.join_overlap;

    if props.compact || (props.collapsible && rect.h < 3) {
        render_compact_frame(f, props, rect, ctx);
        return;
    }

    let (block_style, border_style) = resolve_block_style(props, ctx.active, ctx.is_hovered);
    let border_rstyle = to_ratatui_style(block_style);

    if style_uses_backdrop_bg(block_style) {
        clear_fg_preserve_bg_clipped(f, rect, ctx.clip_rect);
    } else if style_paints_bg(block_style) {
        fill_rect_clipped_style(f, rect, block_style, ctx.clip_rect, ctx.terminal_bg);
    }

    let buf = f.buffer_mut();
    let x = rect.x as i32;
    let y = rect.y as i32;
    let w = rect.w as i32;
    let h = rect.h as i32;
    let mut left = x;
    let mut top = y;
    if props.join_frame {
        if join_overlap.left {
            left = left.saturating_sub(1);
        }
        if join_overlap.top {
            top = top.saturating_sub(1);
        }
    }
    let right = x + w - 1;
    let bottom = y + h - 1;

    let set = to_ratatui_border_set(border_style).unwrap_or(ratatui::symbols::border::PLAIN);

    let clip = ctx
        .clip_rect
        .map(ClipBounds::from_rect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);
    let border_merge_mode = props.border_merge_mode;
    let border_draw = BorderCellDraw {
        style: border_rstyle,
        clip: &clip,
        buf_bounds: &buf_bounds,
        border_merge_mode,
    };

    draw_border_cell(buf, left, top, set.top_left, &border_draw);
    draw_border_cell(buf, right, top, set.top_right, &border_draw);
    draw_border_cell(buf, left, bottom, set.bottom_left, &border_draw);
    draw_border_cell(buf, right, bottom, set.bottom_right, &border_draw);

    let h_char = set.horizontal_top;
    let b_char = set.horizontal_bottom;
    let start_x = (left + 1).max(clip.min_x);
    let end_x = right.min(clip.max_x);
    for cx in start_x..end_x {
        draw_border_cell(buf, cx, top, h_char, &border_draw);
        draw_border_cell(buf, cx, bottom, b_char, &border_draw);
    }

    let start_y = (top + 1).max(clip.min_y);
    let end_y = bottom.min(clip.max_y);
    for cy in start_y..end_y {
        if props.border_edges.has_left() {
            draw_border_cell(buf, left, cy, set.vertical_left, &border_draw);
        }
        if props.border_edges.has_right() {
            draw_border_cell(buf, right, cy, set.vertical_right, &border_draw);
        }
    }

    if let Some(line) = build_header_line(props, block_style, ctx.active, rect.w, h_char, None) {
        let line_width = right.saturating_sub(left).saturating_sub(1);
        render_line_clipped(buf, left + 1, top, line_width, &line, ctx.clip_rect);
    }

    let max_title_w = rect.w.saturating_sub(2);
    let status_style = {
        let mut s = block_style.patch(props.status_style);
        if ctx.active
            && let Some(fss) = props.focus_status_style()
        {
            s = block_style.patch(fss);
        }
        s
    };

    let footer_line = if let Some(status) = &props.status {
        let spans = richtext_to_spans(status, status_style);
        let spans = truncate_spans(spans, max_title_w);
        let line = Line::from(spans).left_aligned();
        Some(apply_footer_padding(
            line,
            props.footer_padding,
            b_char,
            block_style,
        ))
    } else if let Some(status) = &props.status_center {
        let spans = richtext_to_spans(status, status_style);
        let spans = truncate_spans(spans, max_title_w);
        let line = Line::from(spans).centered();
        Some(apply_footer_padding(
            line,
            props.footer_padding,
            b_char,
            block_style,
        ))
    } else if let Some(status) = &props.status_right {
        let spans = richtext_to_spans(status, status_style);
        let spans = truncate_spans(spans, max_title_w);
        let line = Line::from(spans).right_aligned();
        Some(apply_footer_padding(
            line,
            props.footer_padding,
            b_char,
            block_style,
        ))
    } else {
        None
    };

    if let Some(line) = footer_line {
        let line_width = right.saturating_sub(left).saturating_sub(1);
        render_line_clipped(buf, left + 1, bottom, line_width, &line, ctx.clip_rect);
    }

    if let Some(inner_style) = props.inner_style() {
        fill_rect_clipped_style(
            f,
            geometry.body_rect,
            inner_style,
            ctx.clip_rect,
            ctx.terminal_bg,
        );
    }
}

fn apply_footer_padding<'a>(
    mut line: Line<'a>,
    p: crate::style::Padding,
    fill_char: &str,
    style: Style,
) -> Line<'a> {
    if p.left == 0 && p.right == 0 {
        return line;
    }
    let rstyle = to_ratatui_style(style);
    let left_span = Span::styled(fill_char.repeat(p.left as usize), rstyle);
    let right_span = Span::styled(fill_char.repeat(p.right as usize), rstyle);
    if p.left > 0 {
        line.spans.insert(0, left_span);
    }
    if p.right > 0 {
        line.spans.push(right_span);
    }
    line
}

fn render_plain_frame(
    f: &mut ratatui::Frame<'_>,
    props: &FrameProps,
    rect: Rect,
    ctx: &FrameRenderCtx,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let (block_style, _) = resolve_block_style(props, ctx.active, ctx.is_hovered);
    fill_rect_clipped_style(f, rect, block_style, ctx.clip_rect, ctx.terminal_bg);

    if let Some(inner_style) = props.inner_style() {
        fill_rect_clipped_style(f, rect, inner_style, ctx.clip_rect, ctx.terminal_bg);
    }
}

fn render_plain_frame_status(
    f: &mut ratatui::Frame<'_>,
    props: &FrameProps,
    geometry: &FrameGeometry,
    ctx: &FrameRenderCtx,
) {
    let Some(status_rect) = geometry.status_rect else {
        return;
    };

    let (block_style, _) = resolve_block_style(props, ctx.active, ctx.is_hovered);
    let mut status_style = block_style.patch(props.status_style);
    if ctx.active
        && let Some(fss) = props.focus_status_style()
    {
        status_style = block_style.patch(fss);
    }

    let line = if let Some(status) = &props.status {
        Some(
            Line::from(truncate_spans(
                richtext_to_spans(status, status_style),
                status_rect.w,
            ))
            .left_aligned(),
        )
    } else if let Some(status) = &props.status_center {
        Some(
            Line::from(truncate_spans(
                richtext_to_spans(status, status_style),
                status_rect.w,
            ))
            .centered(),
        )
    } else {
        props.status_right.as_ref().map(|status| {
            Line::from(truncate_spans(
                richtext_to_spans(status, status_style),
                status_rect.w,
            ))
            .right_aligned()
        })
    };

    if let Some(line) = line {
        render_line_clipped(
            f.buffer_mut(),
            status_rect.x as i32,
            status_rect.y as i32,
            status_rect.w as i32,
            &line,
            ctx.clip_rect,
        );
    }
}

fn render_compact_frame(
    f: &mut ratatui::Frame<'_>,
    props: &FrameProps,
    rect: Rect,
    ctx: &FrameRenderCtx,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let compact_rect = Rect { h: 1, ..rect };

    let (block_style, border_style) = resolve_block_style(props, ctx.active, ctx.is_hovered);

    let mut title_style = block_style.patch(props.title_style);
    if ctx.active
        && let Some(fts) = props.focus_title_style()
    {
        title_style = block_style.patch(fts);
    }

    let status_style = block_style.patch(props.status_style);

    let width = rect.w as usize;

    let dash = border_horizontal_char(border_style);
    let block_rstyle = to_ratatui_style(block_style);

    let mut title_spans: Vec<Span<'_>> = if props.has_header {
        Vec::new()
    } else if props.has_border() && !props.tab_titles.is_empty() {
        let mut active_tab_style = block_style.patch(props.active_tab_style);
        if ctx.active
            && let Some(fts) = props.focus_active_tab_style()
        {
            active_tab_style = block_style.patch(fts);
        }
        let mut inactive_tab_style = block_style.patch(props.inactive_tab_style);
        if ctx.active
            && let Some(ifts) = props.focus_inactive_tab_style()
        {
            inactive_tab_style = block_style.patch(ifts);
        }
        border_tabs_title_line(
            &props.tab_titles,
            props.active_tab,
            active_tab_style,
            inactive_tab_style,
            props.tab_variant,
            block_style,
            title_style,
        )
        .spans
    } else if let Some(t) = &props.title {
        richtext_to_spans(t, title_style)
    } else {
        Vec::new()
    };

    if let Some(prefix) = &props.title_prefix {
        let prefix_spans = richtext_to_spans(prefix, title_style);
        if title_spans.is_empty() {
            title_spans = prefix_spans;
        } else {
            let sep_span = Span::styled(dash.to_string(), block_rstyle);
            let mut out = Vec::with_capacity(title_spans.len() + prefix_spans.len() + 1);
            out.extend(prefix_spans);
            out.push(sep_span);
            out.extend(title_spans);
            title_spans = out;
        }
    }

    if let Some(suffix) = &props.title_suffix {
        let suffix_spans = richtext_to_spans(suffix, title_style);
        if title_spans.is_empty() {
            title_spans = suffix_spans;
        } else {
            let sep_span = Span::styled(dash.to_string(), block_rstyle);
            title_spans.push(sep_span);
            title_spans.extend(suffix_spans);
        }
    }

    let status_spans: Vec<Span<'_>> = props
        .status_right
        .as_ref()
        .or(props.status_center.as_ref())
        .or(props.status.as_ref())
        .map(|s| richtext_to_spans(s, status_style))
        .unwrap_or_default();

    let title_w: usize = title_spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
    let status_w: usize = status_spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum();

    let has_title = title_w > 0;
    let has_status = status_w > 0;

    let cap_left = 1usize;
    let cap_right = if width > 1 { 1usize } else { 0usize };

    let title_left_count = if has_title {
        props.header_padding.left as usize
    } else {
        0
    };
    let title_right_count = if has_title {
        props.header_padding.right as usize
    } else {
        0
    };
    let status_left_count = if has_status {
        props.footer_padding.left as usize
    } else {
        0
    };
    let status_right_count = if has_status {
        props.footer_padding.right as usize
    } else {
        0
    };

    let separator_min = if has_title && has_status { 2 } else { 0 };

    let fixed_w = cap_left
        + cap_right
        + title_left_count
        + title_right_count
        + status_left_count
        + status_right_count
        + separator_min;
    let content_budget = width.saturating_sub(fixed_w);

    let (final_title_spans, final_status_spans) = if title_w + status_w <= content_budget {
        (title_spans, status_spans)
    } else if title_w <= content_budget.saturating_sub(status_w.min(content_budget / 3)) {
        let remaining = content_budget.saturating_sub(title_w);
        (title_spans, truncate_spans(status_spans, remaining as u16))
    } else {
        let title_budget = content_budget.saturating_mul(2) / 3;
        let status_budget = content_budget.saturating_sub(title_budget);
        (
            truncate_spans(title_spans, title_budget as u16),
            truncate_spans(status_spans, status_budget as u16),
        )
    };

    let separator_dashes = width.saturating_sub(
        fixed_w
            + final_title_spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum::<usize>()
            + final_status_spans
                .iter()
                .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
                .sum::<usize>(),
    ) + separator_min;

    let mut spans = Vec::with_capacity(9);

    spans.push(Span::styled(dash.repeat(cap_left), block_rstyle));

    if title_left_count > 0 {
        spans.push(Span::styled(dash.repeat(title_left_count), block_rstyle));
    }

    spans.extend(final_title_spans);

    if title_right_count > 0 {
        spans.push(Span::styled(dash.repeat(title_right_count), block_rstyle));
    }

    if separator_dashes > 0 {
        spans.push(Span::styled(dash.repeat(separator_dashes), block_rstyle));
    }

    if status_left_count > 0 {
        spans.push(Span::styled(dash.repeat(status_left_count), block_rstyle));
    }

    if !final_status_spans.is_empty() {
        spans.extend(final_status_spans);
    }

    if status_right_count > 0 {
        spans.push(Span::styled(dash.repeat(status_right_count), block_rstyle));
    }

    if cap_right > 0 {
        spans.push(Span::styled(dash.repeat(cap_right), block_rstyle));
    }

    let line = Line::from(spans);

    if style_uses_backdrop_bg(block_style) {
        clear_fg_preserve_bg_clipped(f, compact_rect, ctx.clip_rect);
    } else if style_paints_bg(block_style) {
        fill_rect_clipped_style(f, compact_rect, block_style, ctx.clip_rect, ctx.terminal_bg);
    }

    let buf = f.buffer_mut();
    render_line_clipped(
        buf,
        rect.x as i32,
        rect.y as i32,
        rect.w as i32,
        &line,
        ctx.clip_rect,
    );
}
