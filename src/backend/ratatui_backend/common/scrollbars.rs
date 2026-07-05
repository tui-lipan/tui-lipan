use std::cell::RefCell;

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect as RRect;
use ratatui::style::{Color as RColor, Style as RStyle};
use ratatui::text::{Line, Span};

use crate::style::{BorderStyle, Color, Rect, Style};
use crate::utils::scrollbar::{ScrollbarMetrics, ScrollbarMetricsCache};

use super::cells::{
    ClipBounds, DrawCellClip, DrawCellStyledCtx, draw_cell, draw_cell_styled,
    resolve_style_for_cell,
};
use super::colors::{from_ratatui_color, to_ratatui_color};
use super::convert::to_ratatui_style;
use super::style_resolve::{DEFAULT_SCROLLBAR_THUMB, current_render_terminal_bg};

pub(crate) struct ScrollbarScrollState {
    pub offset: usize,
    pub visible: usize,
    pub total: usize,
}

pub(crate) struct ScrollbarAppearance<'a> {
    pub thumb_char: char,
    pub thumb_style: Style,
    pub track_style: Option<Style>,
    pub clip_rect: Option<RRect>,
    pub metrics_cache: Option<&'a RefCell<ScrollbarMetricsCache>>,
}

pub(crate) struct IntegratedScrollbarAppearance<'a> {
    pub thumb_char: char,
    pub border_char: &'a str,
    pub base_style: Style,
    pub thumb_style: Style,
    pub track_style: Option<Style>,
    pub clip_rect: Option<RRect>,
    pub metrics_cache: Option<&'a RefCell<ScrollbarMetricsCache>>,
}

struct HalfBlockCellDraw<'a> {
    thumb_style: Style,
    clip: &'a ClipBounds,
    buf_bounds: &'a ClipBounds,
    terminal_bg: Option<Color>,
}

pub(crate) fn get_scrollbar_metrics(
    cache: Option<&RefCell<ScrollbarMetricsCache>>,
    total: usize,
    visible: usize,
    offset: usize,
    track_size: usize,
) -> ScrollbarMetrics {
    get_scrollbar_metrics_ex(cache, total, visible, offset, track_size, false)
}

/// Get scrollbar metrics in half-cell units for sub-cell precision.
pub(crate) fn get_scrollbar_metrics_half(
    cache: Option<&RefCell<ScrollbarMetricsCache>>,
    total: usize,
    visible: usize,
    offset: usize,
    track_size: usize,
) -> ScrollbarMetrics {
    get_scrollbar_metrics_ex(cache, total, visible, offset, track_size, true)
}

fn get_scrollbar_metrics_ex(
    cache: Option<&RefCell<ScrollbarMetricsCache>>,
    total: usize,
    visible: usize,
    offset: usize,
    track_size: usize,
    half_cell: bool,
) -> ScrollbarMetrics {
    if let Some(cache) = cache {
        let mut cache = cache.borrow_mut();
        if let Some(metrics) = cache.get(total, visible, offset, track_size, half_cell) {
            return metrics;
        }
        let metrics = if half_cell {
            ScrollbarMetrics::new_with_half_track(total, visible, offset, track_size)
        } else {
            ScrollbarMetrics::new_with_track(total, visible, offset, track_size)
        };
        cache.insert(total, visible, offset, track_size, half_cell, metrics);
        metrics
    } else if half_cell {
        ScrollbarMetrics::new_with_half_track(total, visible, offset, track_size)
    } else {
        ScrollbarMetrics::new_with_track(total, visible, offset, track_size)
    }
}

/// Resolve the effective scrollbar thumb style based on focus state.
///
/// When focused: prefer `focus_style`, fall back to `normal_style`, then default.
/// When not focused: use `normal_style` or default.
pub(crate) fn resolve_scrollbar_thumb_style(
    is_focused: bool,
    normal_style: Option<crate::style::Style>,
    focus_style: Option<crate::style::Style>,
) -> crate::style::Style {
    if is_focused {
        focus_style.or(normal_style).unwrap_or_default()
    } else {
        normal_style.unwrap_or_default()
    }
}

/// Pre-computed clip bounds in i32 for fast per-cell clipping checks.
/// Computing these once before a loop avoids repeated `as i32` casts per cell.
pub(crate) fn render_vscrollbar(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    scroll: ScrollbarScrollState,
    appearance: ScrollbarAppearance<'_>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    if scroll.total == 0 || scroll.visible == 0 || scroll.total <= scroll.visible {
        return;
    }

    let use_half = appearance.thumb_char == DEFAULT_SCROLLBAR_THUMB;
    if use_half {
        let metrics = get_scrollbar_metrics_half(
            appearance.metrics_cache,
            scroll.total,
            scroll.visible,
            scroll.offset,
            rect.h as usize,
        );
        render_vscrollbar_half_block(
            f,
            rect,
            metrics,
            appearance.thumb_style,
            appearance.track_style,
            appearance.clip_rect,
        );
    } else {
        let metrics = get_scrollbar_metrics(
            appearance.metrics_cache,
            scroll.total,
            scroll.visible,
            scroll.offset,
            rect.h as usize,
        );
        render_vscrollbar_with_metrics(
            f,
            rect,
            metrics,
            appearance.thumb_char,
            appearance.thumb_style,
            appearance.track_style,
            appearance.clip_rect,
        );
    }
}

pub(crate) fn render_vscrollbar_with_metrics(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    metrics: ScrollbarMetrics,
    thumb_char: char,
    thumb_style: Style,
    track_style: Option<Style>,
    clip_rect: Option<RRect>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    if metrics.thumb_len == 0 {
        return;
    }

    let buf = f.buffer_mut();
    let x = rect.x as i32;
    let y = rect.y as i32;
    let height = rect.h as i32;

    let thumb_start = metrics.thumb_start as i32;
    let thumb_len = metrics.thumb_len as i32;

    let track_style = track_style.unwrap_or_default();
    let terminal_bg = current_render_terminal_bg().map(from_ratatui_color);

    let mut thumb_buf = [0u8; 4];
    let thumb_s: &str = thumb_char.encode_utf8(&mut thumb_buf);

    // Pre-compute bounds once before the loop
    let clip = clip_rect
        .map(ClipBounds::from_rrect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);

    for dy in 0..height {
        let is_thumb = dy >= thumb_start && dy < thumb_start + thumb_len;
        draw_cell_styled(
            buf,
            x,
            y + dy,
            " ",
            track_style,
            DrawCellStyledCtx {
                clip: &clip,
                buf_bounds: &buf_bounds,
                terminal_bg,
            },
        );
        if is_thumb {
            draw_cell_styled(
                buf,
                x,
                y + dy,
                thumb_s,
                thumb_style,
                DrawCellStyledCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    terminal_bg,
                },
            );
        }
    }
}

/// Render a vertical scrollbar using half-block characters for sub-cell precision.
/// `metrics` must be computed in half-cell units (via `new_with_half_track`).
pub(crate) fn render_vscrollbar_half_block(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    metrics: ScrollbarMetrics,
    thumb_style: Style,
    track_style: Option<Style>,
    clip_rect: Option<RRect>,
) {
    if rect.w == 0 || rect.h == 0 || metrics.thumb_len == 0 {
        return;
    }

    let buf = f.buffer_mut();
    let x = rect.x as i32;
    let y = rect.y as i32;
    let height = rect.h as i32;

    let thumb_start = metrics.thumb_start as i32;
    let thumb_end = thumb_start + metrics.thumb_len as i32;

    let track_style = track_style.unwrap_or_default();
    let terminal_bg = current_render_terminal_bg().map(from_ratatui_color);

    let clip = clip_rect
        .map(ClipBounds::from_rrect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);
    let half_draw = HalfBlockCellDraw {
        thumb_style,
        clip: &clip,
        buf_bounds: &buf_bounds,
        terminal_bg,
    };

    for dy in 0..height {
        let top_half = dy * 2;
        let bot_half = dy * 2 + 1;
        let top_in = top_half >= thumb_start && top_half < thumb_end;
        let bot_in = bot_half >= thumb_start && bot_half < thumb_end;

        draw_cell_styled(
            buf,
            x,
            y + dy,
            " ",
            track_style,
            DrawCellStyledCtx {
                clip: &clip,
                buf_bounds: &buf_bounds,
                terminal_bg,
            },
        );

        match (top_in, bot_in) {
            (true, true) => draw_cell_styled(
                buf,
                x,
                y + dy,
                "█",
                thumb_style,
                DrawCellStyledCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    terminal_bg,
                },
            ),
            (true, false) => draw_half_block_cell(buf, x, y + dy, "▀", &half_draw),
            (false, true) => draw_half_block_cell(buf, x, y + dy, "▄", &half_draw),
            (false, false) => {}
        }
    }
}

/// Draw a half-block scrollbar thumb over the already-painted track cell.
fn draw_half_block_cell(
    buf: &mut Buffer,
    x: i32,
    y: i32,
    symbol: &str,
    draw: &HalfBlockCellDraw<'_>,
) {
    if !draw.clip.contains(x, y) || !draw.buf_bounds.contains(x, y) {
        return;
    }

    if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
        let style = half_block_style_for_cell(draw.thumb_style, cell, draw.terminal_bg);
        cell.set_symbol(symbol).set_style(style);
    }
}

/// Build an RStyle for half-block characters: thumb fg color over the existing
/// track cell bg. This keeps alpha thumb foregrounds blending against the track
/// color instead of the terminal fallback.
fn half_block_style_for_cell(
    mut thumb_style: Style,
    cell: &Cell,
    terminal_bg: Option<Color>,
) -> RStyle {
    thumb_style.bg = None;
    let mut style = resolve_style_for_cell(thumb_style, cell, terminal_bg);
    let cell_bg = if cell.bg == RColor::Reset {
        terminal_bg.map(to_ratatui_color).unwrap_or(RColor::Reset)
    } else {
        cell.bg
    };
    style.bg = (cell_bg != RColor::Reset).then_some(cell_bg);
    style
}

pub(crate) fn render_integrated_scrollbar(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    scroll: ScrollbarScrollState,
    appearance: IntegratedScrollbarAppearance<'_>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    if scroll.total == 0 || scroll.visible == 0 || scroll.total <= scroll.visible {
        return;
    }

    let use_half = appearance.thumb_char == DEFAULT_SCROLLBAR_THUMB;
    if use_half {
        let metrics = get_scrollbar_metrics_half(
            appearance.metrics_cache,
            scroll.total,
            scroll.visible,
            scroll.offset,
            rect.h as usize,
        );
        render_integrated_vscrollbar_half_block(f, rect, metrics, appearance);
    } else {
        let metrics = get_scrollbar_metrics(
            appearance.metrics_cache,
            scroll.total,
            scroll.visible,
            scroll.offset,
            rect.h as usize,
        );
        render_integrated_scrollbar_with_metrics(f, rect, metrics, appearance);
    }
}

pub(crate) fn render_integrated_scrollbar_with_metrics(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    metrics: ScrollbarMetrics,
    appearance: IntegratedScrollbarAppearance<'_>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    if metrics.thumb_len == 0 {
        return;
    }

    let IntegratedScrollbarAppearance {
        thumb_char,
        border_char,
        base_style,
        thumb_style,
        track_style,
        clip_rect,
        metrics_cache: _,
    } = appearance;

    let buf = f.buffer_mut();
    let x = rect.x as i32;
    let y = rect.y as i32;
    let height = rect.h as i32;

    let thumb_start = metrics.thumb_start as i32;
    let thumb_len = metrics.thumb_len as i32;

    let explicit_track_style = track_style.filter(|style| style.bg.is_some());
    let track_style = explicit_track_style.unwrap_or(base_style);
    let track_rstyle = to_ratatui_style(track_style);
    let terminal_bg = current_render_terminal_bg().map(from_ratatui_color);

    let mut thumb_buf = [0u8; 4];
    let thumb_s: &str = thumb_char.encode_utf8(&mut thumb_buf);

    // Pre-compute bounds once before the loop
    let clip = clip_rect
        .map(ClipBounds::from_rrect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);

    for dy in 0..height {
        let is_thumb = dy >= thumb_start && dy < thumb_start + thumb_len;
        if explicit_track_style.is_some() {
            draw_cell_styled(
                buf,
                x,
                y + dy,
                border_char,
                track_style,
                DrawCellStyledCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    terminal_bg,
                },
            );
        } else {
            draw_cell(
                buf,
                x,
                y + dy,
                border_char,
                track_rstyle,
                DrawCellClip {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                },
            );
        }
        if is_thumb {
            draw_cell_styled(
                buf,
                x,
                y + dy,
                thumb_s,
                thumb_style,
                DrawCellStyledCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    terminal_bg,
                },
            );
        }
    }
}

/// Render an integrated vertical scrollbar with half-block precision.
/// `metrics` must be in half-cell units.
pub(crate) fn render_integrated_vscrollbar_half_block(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    metrics: ScrollbarMetrics,
    appearance: IntegratedScrollbarAppearance<'_>,
) {
    if rect.w == 0 || rect.h == 0 || metrics.thumb_len == 0 {
        return;
    }

    let IntegratedScrollbarAppearance {
        border_char,
        base_style,
        thumb_style,
        track_style,
        clip_rect,
        thumb_char: _,
        metrics_cache: _,
    } = appearance;

    let buf = f.buffer_mut();
    let x = rect.x as i32;
    let y = rect.y as i32;
    let height = rect.h as i32;

    let thumb_start = metrics.thumb_start as i32;
    let thumb_end = thumb_start + metrics.thumb_len as i32;

    let explicit_track_style = track_style.filter(|style| style.bg.is_some());
    let track_style = explicit_track_style.unwrap_or(base_style);
    let track_rstyle = to_ratatui_style(track_style);
    let terminal_bg = current_render_terminal_bg().map(from_ratatui_color);

    let clip = clip_rect
        .map(ClipBounds::from_rrect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);
    let half_draw = HalfBlockCellDraw {
        thumb_style,
        clip: &clip,
        buf_bounds: &buf_bounds,
        terminal_bg,
    };

    for dy in 0..height {
        let top_half = dy * 2;
        let bot_half = dy * 2 + 1;
        let top_in = top_half >= thumb_start && top_half < thumb_end;
        let bot_in = bot_half >= thumb_start && bot_half < thumb_end;

        if explicit_track_style.is_some() {
            draw_cell_styled(
                buf,
                x,
                y + dy,
                border_char,
                track_style,
                DrawCellStyledCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    terminal_bg,
                },
            );
        } else {
            draw_cell(
                buf,
                x,
                y + dy,
                border_char,
                track_rstyle,
                DrawCellClip {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                },
            );
        }

        match (top_in, bot_in) {
            (true, true) => draw_cell_styled(
                buf,
                x,
                y + dy,
                "█",
                thumb_style,
                DrawCellStyledCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    terminal_bg,
                },
            ),
            (true, false) => draw_half_block_cell(buf, x, y + dy, "▀", &half_draw),
            (false, true) => draw_half_block_cell(buf, x, y + dy, "▄", &half_draw),
            (false, false) => {}
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) enum IndicatorDirection {
    Top,
    Bottom,
}

pub(crate) fn scroll_indicator_line(
    count: usize,
    direction: IndicatorDirection,
    base_style: Style,
    indicator_style: Style,
) -> Line<'static> {
    let text = match direction {
        IndicatorDirection::Top => format!("↑ {} more", count),
        IndicatorDirection::Bottom => format!("↓ {} more", count),
    };
    let style = base_style.patch(indicator_style);
    Line::from(Span::styled(text, to_ratatui_style(style)))
}

pub(crate) fn single_line_scroll_indicator(
    count: usize,
    base_style: Style,
    indicator_style: Style,
) -> Line<'static> {
    let text = format!("↕ {} items", count);
    let style = base_style.patch(indicator_style);
    Line::from(Span::styled(text, to_ratatui_style(style)))
}

pub(crate) fn render_hscrollbar(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    scroll: ScrollbarScrollState,
    appearance: ScrollbarAppearance<'_>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    if scroll.total == 0 || scroll.visible == 0 || scroll.total <= scroll.visible {
        return;
    }

    let use_half = appearance.thumb_char == DEFAULT_SCROLLBAR_THUMB;
    if use_half {
        let metrics = get_scrollbar_metrics_half(
            appearance.metrics_cache,
            scroll.total,
            scroll.visible,
            scroll.offset,
            rect.w as usize,
        );
        render_hscrollbar_half_block(
            f,
            rect,
            metrics,
            appearance.thumb_style,
            appearance.track_style,
            appearance.clip_rect,
        );
    } else {
        let metrics = get_scrollbar_metrics(
            appearance.metrics_cache,
            scroll.total,
            scroll.visible,
            scroll.offset,
            rect.w as usize,
        );
        if metrics.thumb_len == 0 {
            return;
        }

        let buf = f.buffer_mut();
        let x = rect.x as i32;
        let y = rect.y as i32;
        let width = rect.w as i32;

        let thumb_start = metrics.thumb_start as i32;
        let thumb_len = metrics.thumb_len as i32;

        let track_style = appearance.track_style.unwrap_or_default();
        let terminal_bg = current_render_terminal_bg().map(from_ratatui_color);

        let mut thumb_buf = [0u8; 4];
        let thumb_s: &str = appearance.thumb_char.encode_utf8(&mut thumb_buf);

        let clip = appearance
            .clip_rect
            .map(ClipBounds::from_rrect)
            .unwrap_or_else(ClipBounds::unbounded);
        let buf_bounds = ClipBounds::from_rrect(buf.area);

        for dx in 0..width {
            let is_thumb = dx >= thumb_start && dx < thumb_start + thumb_len;
            draw_cell_styled(
                buf,
                x + dx,
                y,
                " ",
                track_style,
                DrawCellStyledCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    terminal_bg,
                },
            );
            if is_thumb {
                draw_cell_styled(
                    buf,
                    x + dx,
                    y,
                    thumb_s,
                    appearance.thumb_style,
                    DrawCellStyledCtx {
                        clip: &clip,
                        buf_bounds: &buf_bounds,
                        terminal_bg,
                    },
                );
            }
        }
    }
}

/// Render a horizontal scrollbar using half-block characters for sub-cell precision.
/// `metrics` must be computed in half-cell units.
fn render_hscrollbar_half_block(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    metrics: ScrollbarMetrics,
    thumb_style: Style,
    track_style: Option<Style>,
    clip_rect: Option<RRect>,
) {
    if rect.w == 0 || rect.h == 0 || metrics.thumb_len == 0 {
        return;
    }

    let buf = f.buffer_mut();
    let x = rect.x as i32;
    let y = rect.y as i32;
    let width = rect.w as i32;

    let thumb_start = metrics.thumb_start as i32;
    let thumb_end = thumb_start + metrics.thumb_len as i32;

    let track_style = track_style.unwrap_or_default();
    let terminal_bg = current_render_terminal_bg().map(from_ratatui_color);

    let clip = clip_rect
        .map(ClipBounds::from_rrect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);
    let half_draw = HalfBlockCellDraw {
        thumb_style,
        clip: &clip,
        buf_bounds: &buf_bounds,
        terminal_bg,
    };

    for dx in 0..width {
        let left_half = dx * 2;
        let right_half = dx * 2 + 1;
        let left_in = left_half >= thumb_start && left_half < thumb_end;
        let right_in = right_half >= thumb_start && right_half < thumb_end;

        draw_cell_styled(
            buf,
            x + dx,
            y,
            " ",
            track_style,
            DrawCellStyledCtx {
                clip: &clip,
                buf_bounds: &buf_bounds,
                terminal_bg,
            },
        );

        match (left_in, right_in) {
            (true, true) => draw_cell_styled(
                buf,
                x + dx,
                y,
                "█",
                thumb_style,
                DrawCellStyledCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    terminal_bg,
                },
            ),
            (true, false) => draw_half_block_cell(buf, x + dx, y, "▌", &half_draw),
            (false, true) => draw_half_block_cell(buf, x + dx, y, "▐", &half_draw),
            (false, false) => {}
        }
    }
}

pub(crate) fn render_integrated_hscrollbar(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    scroll: ScrollbarScrollState,
    appearance: IntegratedScrollbarAppearance<'_>,
) {
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    if scroll.total == 0 || scroll.visible == 0 || scroll.total <= scroll.visible {
        return;
    }

    let use_half = appearance.thumb_char == DEFAULT_SCROLLBAR_THUMB;
    if use_half {
        let metrics = get_scrollbar_metrics_half(
            appearance.metrics_cache,
            scroll.total,
            scroll.visible,
            scroll.offset,
            rect.w as usize,
        );
        render_integrated_hscrollbar_half_block(f, rect, metrics, appearance);
    } else {
        let metrics = get_scrollbar_metrics(
            appearance.metrics_cache,
            scroll.total,
            scroll.visible,
            scroll.offset,
            rect.w as usize,
        );
        if metrics.thumb_len == 0 {
            return;
        }

        let IntegratedScrollbarAppearance {
            thumb_char,
            border_char,
            base_style,
            thumb_style,
            track_style,
            clip_rect,
            metrics_cache: _,
        } = appearance;

        let buf = f.buffer_mut();
        let x = rect.x as i32;
        let y = rect.y as i32;
        let width = rect.w as i32;

        let thumb_start = metrics.thumb_start as i32;
        let thumb_len = metrics.thumb_len as i32;

        let explicit_track_style = track_style.filter(|style| style.bg.is_some());
        let track_style = explicit_track_style.unwrap_or(base_style);
        let track_rstyle = to_ratatui_style(track_style);
        let terminal_bg = current_render_terminal_bg().map(from_ratatui_color);

        let mut thumb_buf = [0u8; 4];
        let thumb_s: &str = thumb_char.encode_utf8(&mut thumb_buf);

        let clip = clip_rect
            .map(ClipBounds::from_rrect)
            .unwrap_or_else(ClipBounds::unbounded);
        let buf_bounds = ClipBounds::from_rrect(buf.area);

        for dx in 0..width {
            let is_thumb = dx >= thumb_start && dx < thumb_start + thumb_len;
            if explicit_track_style.is_some() {
                draw_cell_styled(
                    buf,
                    x + dx,
                    y,
                    border_char,
                    track_style,
                    DrawCellStyledCtx {
                        clip: &clip,
                        buf_bounds: &buf_bounds,
                        terminal_bg,
                    },
                );
            } else {
                draw_cell(
                    buf,
                    x + dx,
                    y,
                    border_char,
                    track_rstyle,
                    DrawCellClip {
                        clip: &clip,
                        buf_bounds: &buf_bounds,
                    },
                );
            }
            if is_thumb {
                draw_cell_styled(
                    buf,
                    x + dx,
                    y,
                    thumb_s,
                    thumb_style,
                    DrawCellStyledCtx {
                        clip: &clip,
                        buf_bounds: &buf_bounds,
                        terminal_bg,
                    },
                );
            }
        }
    }
}

/// Render an integrated horizontal scrollbar with half-block precision.
/// `metrics` must be in half-cell units.
fn render_integrated_hscrollbar_half_block(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    metrics: ScrollbarMetrics,
    appearance: IntegratedScrollbarAppearance<'_>,
) {
    if rect.w == 0 || rect.h == 0 || metrics.thumb_len == 0 {
        return;
    }

    let IntegratedScrollbarAppearance {
        border_char,
        base_style,
        thumb_style,
        track_style,
        clip_rect,
        thumb_char: _,
        metrics_cache: _,
    } = appearance;

    let buf = f.buffer_mut();
    let x = rect.x as i32;
    let y = rect.y as i32;
    let width = rect.w as i32;

    let thumb_start = metrics.thumb_start as i32;
    let thumb_end = thumb_start + metrics.thumb_len as i32;

    let explicit_track_style = track_style.filter(|style| style.bg.is_some());
    let track_style = explicit_track_style.unwrap_or(base_style);
    let track_rstyle = to_ratatui_style(track_style);
    let terminal_bg = current_render_terminal_bg().map(from_ratatui_color);

    let clip = clip_rect
        .map(ClipBounds::from_rrect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);
    let half_draw = HalfBlockCellDraw {
        thumb_style,
        clip: &clip,
        buf_bounds: &buf_bounds,
        terminal_bg,
    };

    for dx in 0..width {
        let left_half = dx * 2;
        let right_half = dx * 2 + 1;
        let left_in = left_half >= thumb_start && left_half < thumb_end;
        let right_in = right_half >= thumb_start && right_half < thumb_end;

        if explicit_track_style.is_some() {
            draw_cell_styled(
                buf,
                x + dx,
                y,
                border_char,
                track_style,
                DrawCellStyledCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    terminal_bg,
                },
            );
        } else {
            draw_cell(
                buf,
                x + dx,
                y,
                border_char,
                track_rstyle,
                DrawCellClip {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                },
            );
        }

        match (left_in, right_in) {
            (true, true) => draw_cell_styled(
                buf,
                x + dx,
                y,
                "█",
                thumb_style,
                DrawCellStyledCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    terminal_bg,
                },
            ),
            (true, false) => draw_half_block_cell(buf, x + dx, y, "▌", &half_draw),
            (false, true) => draw_half_block_cell(buf, x + dx, y, "▐", &half_draw),
            (false, false) => {}
        }
    }
}

pub(crate) fn border_vertical_char(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::Plain | BorderStyle::Rounded => "│",
        BorderStyle::Double => "║",
        BorderStyle::Thick => "┃",
        BorderStyle::LightDoubleDashed => "╎",
        BorderStyle::HeavyDoubleDashed => "╏",
        BorderStyle::LightTripleDashed => "┆",
        BorderStyle::HeavyTripleDashed => "┇",
        BorderStyle::LightQuadrupleDashed => "┊",
        BorderStyle::HeavyQuadrupleDashed => "┋",
        BorderStyle::Custom { glyphs: g } => g.left,
    }
}

pub(crate) fn border_horizontal_char(style: BorderStyle) -> &'static str {
    match style {
        BorderStyle::Plain | BorderStyle::Rounded => "─",
        BorderStyle::Double => "═",
        BorderStyle::Thick => "━",
        BorderStyle::LightDoubleDashed => "╌",
        BorderStyle::HeavyDoubleDashed => "╍",
        BorderStyle::LightTripleDashed => "┄",
        BorderStyle::HeavyTripleDashed => "┅",
        BorderStyle::LightQuadrupleDashed => "┈",
        BorderStyle::HeavyQuadrupleDashed => "┉",
        BorderStyle::Custom { glyphs: g } => g.top,
    }
}

/// Empty-cell character for an integrated vertical scrollbar track (frame border or edge decoration glyph).
pub(crate) fn integrated_vscrollbar_track_char(
    track_glyph: Option<char>,
    border_style_fallback: BorderStyle,
    scratch: &mut [u8; 4],
) -> &str {
    if let Some(g) = track_glyph {
        g.encode_utf8(scratch)
    } else {
        border_vertical_char(border_style_fallback)
    }
}

/// Empty-cell character for an integrated horizontal scrollbar track.
pub(crate) fn integrated_hscrollbar_track_char(
    track_glyph: Option<char>,
    border_style_fallback: BorderStyle,
    scratch: &mut [u8; 4],
) -> &str {
    if let Some(g) = track_glyph {
        g.encode_utf8(scratch)
    } else {
        border_horizontal_char(border_style_fallback)
    }
}
