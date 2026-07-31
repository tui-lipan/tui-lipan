use ratatui::buffer::Buffer;
use ratatui::symbols::merge::MergeStrategy;
use ratatui::text::{Line, Span};

use crate::backend::ratatui_backend::common::{
    ClipBounds, border_horizontal_char, clear_fg_preserve_bg_clipped, fill_rect_clipped_style,
    render_line_clipped, richtext_to_spans, style_paints_bg, style_uses_backdrop_bg,
    to_ratatui_border_set, to_ratatui_style, to_ratatui_style_with_terminal_bg, truncate_spans,
};
use crate::backend::ratatui_backend::renderers::frame::utils::build_tabs_line;
use crate::style::{Color, Edge, Paint, Rect, Style};
use crate::widgets::internal::{FrameGeometry, FrameProps};
use crate::widgets::{BorderLabels, FrameLabel};
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
        render_plain_frame_header(f, props, geometry, &ctx);
        render_plain_frame_footer(f, props, geometry, &ctx);
    }

    render_frame_decorations(f, props, geometry, &ctx);

    restore_decoration_backgrounds(f, &transparent_decoration_bg_snapshot, ctx.clip_rect);
}

fn render_plain_frame_header(
    f: &mut ratatui::Frame<'_>,
    props: &FrameProps,
    geometry: &FrameGeometry,
    ctx: &FrameRenderCtx,
) {
    let Some(header_rect) = geometry.header_labels_rect else {
        return;
    };
    let (block_style, _) = resolve_block_style(props, ctx.active, ctx.is_hovered);
    render_border_labels(
        f.buffer_mut(),
        &BorderLabelsRender {
            x: header_rect.x as i32,
            y: header_rect.y as i32,
            width: header_rect.w as i32,
            labels: &props.header,
            block_style,
            active: ctx.active,
            fill_char: " ",
            clip_rect: ctx.clip_rect,
            terminal_bg: ctx.terminal_bg,
        },
    );
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
    let Some(existing) = buf.cell((x as u16, y as u16)).map(|cell| cell.symbol().to_owned()) else {
        return;
    };

    let should_merge = draw.border_merge_mode != BorderMergeMode::Replace
        && is_box_drawing_symbol(&existing)
        && is_box_drawing_symbol(symbol);

    if should_merge {
        let Some(cell) = buf.cell_mut((x as u16, y as u16)) else {
            return;
        };
        cell.merge_symbol(symbol, to_merge_strategy(draw.border_merge_mode));
        cell.set_style(draw.style);
        return;
    }

    // Fuzzy/Exact is for composing box-drawing seams. A neighbor's border title must survive an
    // overlapping later edge: keep non-box glyphs, and keep spaces that sit next to that text
    // (the `icon  title` gap). Plain backdrop spaces - even when a parent painted a fg - still
    // accept the border. Replace mode still overwrites so occluding frames win.
    if draw.border_merge_mode != BorderMergeMode::Replace
        && should_preserve_border_content(buf, x, y, &existing)
    {
        return;
    }

    let Some(cell) = buf.cell_mut((x as u16, y as u16)) else {
        return;
    };
    cell.set_symbol(symbol);
    cell.set_style(draw.style);
}

fn should_preserve_border_content(buf: &Buffer, x: i32, y: i32, existing: &str) -> bool {
    if is_box_drawing_symbol(existing) {
        return false;
    }
    if existing.is_empty() || existing == " " {
        return neighbor_is_border_title_content(buf, x - 1, y)
            || neighbor_is_border_title_content(buf, x + 1, y);
    }
    true
}

fn neighbor_is_border_title_content(buf: &Buffer, x: i32, y: i32) -> bool {
    if x < 0 || y < 0 {
        return false;
    }
    let Some(cell) = buf.cell((x as u16, y as u16)) else {
        return false;
    };
    let sym = cell.symbol();
    !sym.is_empty() && sym != " " && !is_box_drawing_symbol(sym)
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

pub(crate) fn resolve_label_style(
    block_style: Style,
    group: &BorderLabels,
    label: &FrameLabel,
    active: bool,
) -> Style {
    let mut style = block_style.patch(group.style);
    if let Some(label_style) = label.style {
        style = style.patch(label_style);
    }
    if active {
        if let Some(focused_style) = group.focused_style {
            style = style.patch(focused_style);
        }
        if let Some(focused_style) = label.focused_style {
            style = style.patch(focused_style);
        }
    }
    style
}

fn label_spans<'a>(
    label: &'a FrameLabel,
    group: &BorderLabels,
    block_style: Style,
    active: bool,
    fill_char: &str,
    terminal_bg: Option<Color>,
) -> Vec<Span<'a>> {
    let style = resolve_label_style(block_style, group, label, active);
    let padding_style = to_ratatui_style_with_terminal_bg(block_style, terminal_bg);
    let mut spans = Vec::new();
    if group.padding.left > 0 {
        spans.push(Span::styled(
            fill_char.repeat(group.padding.left as usize),
            padding_style,
        ));
    }
    spans.extend(richtext_to_spans(&label.content, style));
    if group.padding.right > 0 {
        spans.push(Span::styled(
            fill_char.repeat(group.padding.right as usize),
            padding_style,
        ));
    }
    spans
}

fn spans_width(spans: &[Span<'_>]) -> usize {
    spans
        .iter()
        .map(|span| unicode_width::UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

struct BorderLabelsRender<'a> {
    x: i32,
    y: i32,
    width: i32,
    labels: &'a BorderLabels,
    block_style: Style,
    active: bool,
    fill_char: &'a str,
    clip_rect: Option<Rect>,
    terminal_bg: Option<Color>,
}

fn render_border_labels(buf: &mut Buffer, render: &BorderLabelsRender<'_>) {
    let BorderLabelsRender {
        x,
        y,
        width,
        labels,
        block_style,
        active,
        fill_char,
        clip_rect,
        terminal_bg,
    } = render;
    if *width <= 0 || !labels.has_labels() {
        return;
    }

    let left = labels
        .left
        .as_ref()
        .filter(|label| !label.content.is_empty())
        .map(|label| {
            label_spans(
                label,
                labels,
                *block_style,
                *active,
                fill_char,
                *terminal_bg,
            )
        });
    let center = labels
        .center
        .as_ref()
        .filter(|label| !label.content.is_empty())
        .map(|label| {
            label_spans(
                label,
                labels,
                *block_style,
                *active,
                fill_char,
                *terminal_bg,
            )
        });
    let right = labels
        .right
        .as_ref()
        .filter(|label| !label.content.is_empty())
        .map(|label| {
            label_spans(
                label,
                labels,
                *block_style,
                *active,
                fill_char,
                *terminal_bg,
            )
        });

    let left_w = left.as_ref().map_or(0, |spans| spans_width(spans));
    let right_w = right.as_ref().map_or(0, |spans| spans_width(spans));
    let available = *width as usize;

    let (left_budget, right_budget) = if left.is_some() && right.is_some() {
        if left_w.saturating_add(right_w) <= available {
            (left_w, right_w)
        } else {
            let left_budget = available.div_ceil(2);
            (left_budget, available.saturating_sub(left_budget))
        }
    } else {
        (
            left_w.min(available),
            right_w.min(available.saturating_sub(left_w.min(available))),
        )
    };
    let center_budget = available.saturating_sub(left_budget.saturating_add(right_budget));

    if let Some(spans) = left {
        let line = Line::from(truncate_spans(
            spans,
            left_budget.min(u16::MAX as usize) as u16,
        ))
        .left_aligned();
        render_line_clipped(buf, *x, *y, left_budget as i32, &line, *clip_rect);
    }
    if let Some(spans) = center {
        let line = Line::from(truncate_spans(
            spans,
            center_budget.min(u16::MAX as usize) as u16,
        ))
        .centered();
        render_line_clipped(
            buf,
            x.saturating_add(left_budget as i32),
            *y,
            center_budget as i32,
            &line,
            *clip_rect,
        );
    }
    if let Some(spans) = right {
        let line = Line::from(truncate_spans(
            spans,
            right_budget.min(u16::MAX as usize) as u16,
        ))
        .right_aligned();
        render_line_clipped(
            buf,
            x.saturating_add(*width).saturating_sub(right_budget as i32),
            *y,
            right_budget as i32,
            &line,
            *clip_rect,
        );
    }
}

fn render_border_tabs_header(
    buf: &mut Buffer,
    props: &FrameProps,
    render: &BorderLabelsRender<'_>,
) -> bool {
    let BorderLabelsRender {
        x,
        y,
        width,
        labels,
        block_style,
        active,
        fill_char,
        clip_rect,
        terminal_bg,
    } = render;
    let Some(tabs) = build_tabs_line(
        props,
        *block_style,
        *active,
        (*width).max(0).min(u16::MAX as i32) as u16,
    ) else {
        return false;
    };

    let left = labels
        .left
        .as_ref()
        .filter(|label| !label.content.is_empty())
        .map(|label| {
            label_spans(
                label,
                labels,
                *block_style,
                *active,
                fill_char,
                *terminal_bg,
            )
        });
    let right = labels
        .right
        .as_ref()
        .filter(|label| !label.content.is_empty())
        .map(|label| {
            label_spans(
                label,
                labels,
                *block_style,
                *active,
                fill_char,
                *terminal_bg,
            )
        });
    let left_w = left.as_ref().map_or(0, |spans| spans_width(spans));
    let right_w = right.as_ref().map_or(0, |spans| spans_width(spans));
    let available = (*width).max(0) as usize;
    let has_left = left.is_some();
    let has_right = right.is_some();
    let left_separator = has_left && labels.padding.right == 0;
    let right_separator = has_right && labels.padding.left == 0;
    let separator_count = usize::from(left_separator) + usize::from(right_separator);
    let label_budget = available.saturating_sub(separator_count);
    let left_budget = left_w.min(label_budget);
    let right_budget = right_w.min(label_budget.saturating_sub(left_budget));
    let tabs_budget = label_budget.saturating_sub(left_budget + right_budget);
    let separator_style = to_ratatui_style_with_terminal_bg(*block_style, *terminal_bg);

    if let Some(spans) = left.as_ref() {
        let line = Line::from(truncate_spans(
            spans.clone(),
            left_budget.min(u16::MAX as usize) as u16,
        ))
        .left_aligned();
        render_line_clipped(buf, *x, *y, left_budget as i32, &line, *clip_rect);
        if left_separator {
            render_line_clipped(
                buf,
                (*x).saturating_add(left_budget as i32),
                *y,
                1,
                &Line::from(Span::styled(fill_char.to_owned(), separator_style)),
                *clip_rect,
            );
        }
    }

    let tabs_x = (*x)
        .saturating_add(left_budget as i32)
        .saturating_add(i32::from(left_separator));
    if tabs_budget > 0 {
        let line = Line::from(truncate_spans(
            tabs.spans,
            tabs_budget.min(u16::MAX as usize) as u16,
        ));
        render_line_clipped(buf, tabs_x, *y, tabs_budget as i32, &line, *clip_rect);
    }

    if let Some(spans) = right.as_ref() {
        let right_x = (*x)
            .saturating_add(*width)
            .saturating_sub(right_budget as i32);
        if right_separator {
            render_line_clipped(
                buf,
                right_x.saturating_sub(1),
                *y,
                1,
                &Line::from(Span::styled(fill_char.to_owned(), separator_style)),
                *clip_rect,
            );
        }
        let line = Line::from(truncate_spans(
            spans.clone(),
            right_budget.min(u16::MAX as usize) as u16,
        ))
        .right_aligned();
        render_line_clipped(buf, right_x, *y, right_budget as i32, &line, *clip_rect);
    }

    true
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

    // Fill only the interior, never the border ring: the border draw below styles those cells
    // itself, and painting over them would clobber a neighbor frame's border that this frame is
    // meant to merge with (adjacent/overlapping bordered frames sharing a seam). The inset uses
    // the unshifted border padding so a `join_frame` frame still keeps its fill inside its own
    // rect rather than bleeding into the neighbor it draws its shared border onto.
    let fill_rect = rect.inset(geometry.border_padding);
    if style_uses_backdrop_bg(block_style) {
        clear_fg_preserve_bg_clipped(f, fill_rect, ctx.clip_rect);
    } else if style_paints_bg(block_style) {
        fill_rect_clipped_style(f, fill_rect, block_style, ctx.clip_rect, ctx.terminal_bg);
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

    let line_width = right.saturating_sub(left).saturating_sub(1);
    let header_render = BorderLabelsRender {
        x: left + 1,
        y: top,
        width: line_width,
        labels: &props.header,
        block_style,
        active: ctx.active,
        fill_char: h_char,
        clip_rect: ctx.clip_rect,
        terminal_bg: ctx.terminal_bg,
    };
    if !render_border_tabs_header(buf, props, &header_render) {
        render_border_labels(buf, &header_render);
    }
    render_border_labels(
        buf,
        &BorderLabelsRender {
            x: left + 1,
            y: bottom,
            width: line_width,
            labels: &props.footer,
            block_style,
            active: ctx.active,
            fill_char: b_char,
            clip_rect: ctx.clip_rect,
            terminal_bg: ctx.terminal_bg,
        },
    );

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

fn render_plain_frame_footer(
    f: &mut ratatui::Frame<'_>,
    props: &FrameProps,
    geometry: &FrameGeometry,
    ctx: &FrameRenderCtx,
) {
    let Some(footer_rect) = geometry.footer_rect else {
        return;
    };

    let (block_style, _) = resolve_block_style(props, ctx.active, ctx.is_hovered);
    render_border_labels(
        f.buffer_mut(),
        &BorderLabelsRender {
            x: footer_rect.x as i32,
            y: footer_rect.y as i32,
            width: footer_rect.w as i32,
            labels: &props.footer,
            block_style,
            active: ctx.active,
            fill_char: " ",
            clip_rect: ctx.clip_rect,
            terminal_bg: ctx.terminal_bg,
        },
    );
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
    let dash = border_horizontal_char(border_style);
    let block_rstyle = to_ratatui_style(block_style);
    let cap_left = usize::from(rect.w > 0);
    let cap_right = usize::from(rect.w > 1);
    let line = Line::from(vec![Span::styled(
        dash.repeat(rect.w as usize),
        block_rstyle,
    )]);

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
    let label_x = rect.x as i32 + cap_left as i32;
    let label_width = rect.w as i32 - cap_left as i32 - cap_right as i32;
    let header_render = BorderLabelsRender {
        x: label_x,
        y: rect.y as i32,
        width: label_width,
        labels: &props.header,
        block_style,
        active: ctx.active,
        fill_char: dash,
        clip_rect: ctx.clip_rect,
        terminal_bg: ctx.terminal_bg,
    };
    let rendered_tabs = render_border_tabs_header(buf, props, &header_render);
    if !rendered_tabs && props.header.has_labels() {
        render_border_labels(buf, &header_render);
    } else if !rendered_tabs {
        render_border_labels(
            buf,
            &BorderLabelsRender {
                x: label_x,
                y: rect.y as i32,
                width: label_width,
                labels: &props.footer,
                block_style,
                active: ctx.active,
                fill_char: dash,
                clip_rect: ctx.clip_rect,
                terminal_bg: ctx.terminal_bg,
            },
        );
    }

    if (rendered_tabs || props.header.has_labels())
        && props.header.right.is_none()
        && props.footer.has_labels()
    {
        let mut footer_right = props.footer.clone();
        footer_right.left = None;
        footer_right.center = None;
        render_border_labels(
            buf,
            &BorderLabelsRender {
                x: label_x,
                y: rect.y as i32,
                width: label_width,
                labels: &footer_right,
                block_style,
                active: ctx.active,
                fill_char: dash,
                clip_rect: ctx.clip_rect,
                terminal_bg: ctx.terminal_bg,
            },
        );
    }
}
