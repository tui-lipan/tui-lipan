use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect as RRect;
use ratatui::style::{Color as RColor, Modifier as RMod, Style as RStyle};

use crate::app::ContrastPolicy;
use crate::style::{Color, Rect, Style};

use super::colors::to_ratatui_color;
use super::convert::{
    blend_paint_over_ratatui, style_has_alpha_paint, style_paints_bg, to_ratatui_rect,
    to_ratatui_style_with_terminal_bg,
};
use super::style_resolve::finalize_style;

pub(crate) struct ClipBounds {
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
}

impl ClipBounds {
    /// Create clip bounds from a lipan Rect.
    #[inline]
    pub fn from_rect(r: Rect) -> Self {
        Self {
            min_x: r.x as i32,
            min_y: r.y as i32,
            max_x: r.x as i32 + r.w as i32,
            max_y: r.y as i32 + r.h as i32,
        }
    }

    /// Create clip bounds from a ratatui RRect.
    #[inline]
    pub fn from_rrect(r: RRect) -> Self {
        Self {
            min_x: r.x as i32,
            min_y: r.y as i32,
            max_x: r.x as i32 + r.width as i32,
            max_y: r.y as i32 + r.height as i32,
        }
    }

    /// Check if a point is within bounds.
    #[inline]
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.min_x && y >= self.min_y && x < self.max_x && y < self.max_y
    }

    /// Create bounds that represent "no clipping" (everything passes).
    #[inline]
    pub fn unbounded() -> Self {
        Self {
            min_x: i32::MIN,
            min_y: i32::MIN,
            max_x: i32::MAX,
            max_y: i32::MAX,
        }
    }
}

/// Resolve the final interactive style from base + state-dependent patches.
///
/// Priority: disabled > (hover + focus).
/// When disabled, only `disabled_style` is applied so disabled remains terminal.
/// When not disabled, hover is transient and focus is durable: concrete fields
/// follow hover then focus precedence, while hover effects compose afterward.
///
/// Cascade semantics for color transforms (`transform_fg` / `transform_bg`):
/// transient hover effects compose after durable focus concrete colors have
/// resolved, while disabled remains terminal and bypasses hover/focus.
pub(crate) struct DrawCellClip<'a> {
    pub clip: &'a ClipBounds,
    pub buf_bounds: &'a ClipBounds,
}

pub(crate) struct DrawCellStyledCtx<'a> {
    pub clip: &'a ClipBounds,
    pub buf_bounds: &'a ClipBounds,
    pub terminal_bg: Option<Color>,
}

/// Fast cell drawing with pre-computed clip bounds.
/// Use this in loops where bounds are computed once before the loop.
#[inline]
pub(crate) fn draw_cell(
    buf: &mut Buffer,
    x: i32,
    y: i32,
    symbol: &str,
    style: RStyle,
    clip: DrawCellClip<'_>,
) {
    if !clip.clip.contains(x, y) || !clip.buf_bounds.contains(x, y) {
        return;
    }
    if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
        cell.set_symbol(symbol).set_style(style);
    }
}

pub(super) fn resolve_style_for_cell(
    style: Style,
    cell: &Cell,
    terminal_bg: Option<Color>,
) -> RStyle {
    let style = finalize_style(style, terminal_bg, ContrastPolicy::Off);
    let mut out = to_ratatui_style_with_terminal_bg(style, terminal_bg);
    let cell_bg = if cell.bg == RColor::Reset {
        terminal_bg.map(to_ratatui_color).unwrap_or(RColor::Reset)
    } else {
        cell.bg
    };

    if let Some(bg) = style.bg {
        if bg.is_backdrop_sentinel() {
            out.bg = if cell_bg == RColor::Reset {
                None
            } else {
                Some(cell_bg)
            };
        } else if bg.is_transparent_paint() {
            out.bg = None;
        } else if !bg.is_opaque() {
            out.bg = blend_paint_over_ratatui(bg, cell_bg);
        }
    }

    if let Some(fg) = style.fg {
        if fg.is_backdrop_sentinel() || fg.is_transparent_paint() {
            out.fg = None;
        } else if !fg.is_opaque() {
            let backdrop = out
                .bg
                .or_else(|| (cell_bg != RColor::Reset).then_some(cell_bg));
            out.fg = backdrop.and_then(|bg| blend_paint_over_ratatui(fg, bg));
        }
    }

    if let Some(underline) = style.underline_color
        && !underline.is_opaque()
    {
        let backdrop = out
            .bg
            .or_else(|| (cell_bg != RColor::Reset).then_some(cell_bg));
        out.underline_color = backdrop.and_then(|bg| blend_paint_over_ratatui(underline, bg));
    }

    out
}

/// Draw a cell from a lipan Style, blending alpha paint against the existing cell.
#[inline]
pub(crate) fn draw_cell_styled(
    buf: &mut Buffer,
    x: i32,
    y: i32,
    symbol: &str,
    style: Style,
    ctx: DrawCellStyledCtx<'_>,
) {
    if !ctx.clip.contains(x, y) || !ctx.buf_bounds.contains(x, y) {
        return;
    }
    if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
        let style = resolve_style_for_cell(style, cell, ctx.terminal_bg);
        cell.set_symbol(symbol).set_style(style);
    }
}

/// Draw a cell with optional clip rect. Computes bounds per-call.
/// For loops, prefer `draw_cell` with pre-computed `ClipBounds`.
pub(crate) fn draw_cell_clipped(
    buf: &mut Buffer,
    x: i32,
    y: i32,
    symbol: &str,
    style: RStyle,
    clip_rect: Option<Rect>,
) {
    let clip = clip_rect
        .map(ClipBounds::from_rect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);

    if !clip.contains(x, y) || !buf_bounds.contains(x, y) {
        return;
    }
    if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
        cell.set_symbol(symbol).set_style(style);
    }
}

pub(crate) fn fill_rect_clipped(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    style: RStyle,
    clip_rect: Option<Rect>,
) {
    if style.bg.is_none() {
        return;
    }

    let mut draw_rect = rect;
    if let Some(clip) = clip_rect {
        draw_rect = draw_rect.intersection(&clip);
    }

    if draw_rect.is_empty() {
        return;
    }

    let buf_area = f.area();
    let r_rect = to_ratatui_rect(draw_rect);

    let intersection = buf_area.intersection(r_rect);
    if intersection.width == 0 || intersection.height == 0 {
        return;
    }

    let buf = f.buffer_mut();
    let buf_width = buf.area.width as usize;
    let start_x = intersection.x.saturating_sub(buf.area.x) as usize;
    let start_y = intersection.y.saturating_sub(buf.area.y) as usize;
    let row_width = intersection.width as usize;
    let mut fill_cell = Cell::EMPTY;
    fill_cell.set_style(style);

    for row in 0..intersection.height as usize {
        let row_start = (start_y + row) * buf_width + start_x;
        let row_end = row_start + row_width;
        buf.content[row_start..row_end].fill(fill_cell.clone());
    }
}

pub(crate) fn fill_rect_clipped_style(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    style: Style,
    clip_rect: Option<Rect>,
    terminal_bg: Option<Color>,
) {
    if !style_paints_bg(style) {
        return;
    }

    let rt_style = to_ratatui_style_with_terminal_bg(style, terminal_bg);
    if !style_has_alpha_paint(style) {
        fill_rect_clipped(f, rect, rt_style, clip_rect);
        return;
    }

    let mut draw_rect = rect;
    if let Some(clip) = clip_rect {
        draw_rect = draw_rect.intersection(&clip);
    }
    if draw_rect.is_empty() {
        return;
    }

    let buf_area = f.area();
    let intersection = buf_area.intersection(to_ratatui_rect(draw_rect));
    if intersection.width == 0 || intersection.height == 0 {
        return;
    }

    let buf = f.buffer_mut();
    for y in intersection.y..intersection.y.saturating_add(intersection.height) {
        for x in intersection.x..intersection.x.saturating_add(intersection.width) {
            if let Some(cell) = buf.cell_mut((x, y)) {
                let resolved = resolve_style_for_cell(style, cell, terminal_bg);
                *cell = Cell::EMPTY;
                cell.set_style(resolved);
            }
        }
    }
}

pub(crate) fn clear_fg_preserve_bg_clipped(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    clip_rect: Option<Rect>,
) {
    let mut draw_rect = rect;
    if let Some(clip) = clip_rect {
        draw_rect = draw_rect.intersection(&clip);
    }

    if draw_rect.is_empty() {
        return;
    }

    let buf = f.buffer_mut();
    let bounds = ClipBounds::from_rrect(buf.area);
    for y in draw_rect.y..draw_rect.y.saturating_add(draw_rect.h as i16) {
        for x in draw_rect.x..draw_rect.x.saturating_add(draw_rect.w as i16) {
            if !bounds.contains(x as i32, y as i32) {
                continue;
            }
            if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
                cell.set_symbol(" ");
                cell.fg = RColor::Reset;
                cell.underline_color = RColor::Reset;
                cell.modifier = RMod::empty();
                #[allow(deprecated)]
                {
                    cell.skip = false;
                }
            }
        }
    }
}
