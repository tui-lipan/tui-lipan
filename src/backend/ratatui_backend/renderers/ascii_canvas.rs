use ratatui::style::Color as RColor;
use ratatui::widgets::{Block, Clear};
use unicode_width::UnicodeWidthChar;

use crate::backend::ratatui_backend::common::{
    ClipBounds, DrawCellClip, draw_cell, style_paints_bg, to_ratatui_color, to_ratatui_rect,
    to_ratatui_style, to_ratatui_style_with_terminal_bg,
};
use crate::style::resolve::resolve_base_style;
use crate::style::{Color, Paint, Rect, Theme};
use crate::utils::gradient::{ColorGradient, GradientDirection};
use crate::widgets::AsciiCell;
use crate::widgets::internal::AsciiCanvasNode;

pub(crate) fn render_ascii_canvas(
    f: &mut ratatui::Frame<'_>,
    node: &AsciiCanvasNode,
    theme: &Theme,
    rect: Rect,
    clip_rect: Option<Rect>,
    terminal_bg: Option<Color>,
) {
    let style = resolve_base_style(theme, node.style);
    let background = node
        .background
        .map(|style| resolve_base_style(theme, style));
    let clip = clip_rect
        .map(ClipBounds::from_rect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(f.buffer_mut().area);

    if let Some(bg) = background {
        let rrect = to_ratatui_rect(rect);
        if !rrect.is_empty() {
            let style = to_ratatui_style_with_terminal_bg(bg, terminal_bg);
            if style_paints_bg(bg) {
                f.render_widget(Clear, rrect);
                f.render_widget(Block::default().style(style), rrect);
            }
        }
    }

    if rect.w == 0 || rect.h == 0 {
        return;
    }

    // Sequence mode: render current frame's buffer
    if let Some(ref seq) = node.sequence {
        if let Some(frame) = seq.get(node.current_frame) {
            let buffer = &frame.buffer;
            render_cell_slice(
                f,
                buffer.cells(),
                buffer.width(),
                buffer.height(),
                style,
                rect,
                AsciiCanvasCellSliceCtx {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                    gradient: node.gradient,
                    color_map: node.color_map.as_deref(),
                    fg_color_map: node.fg_color_map.as_deref(),
                    bg_color_map: node.bg_color_map.as_deref(),
                    terminal_bg,
                },
            );
        }
        return;
    }

    // Cell grid mode
    if let Some(cells) = node.cells.as_ref() {
        let (grid_w, grid_h) = node.grid_size.unwrap_or((rect.w, rect.h));
        render_cell_slice(
            f,
            cells,
            grid_w,
            grid_h,
            style,
            rect,
            AsciiCanvasCellSliceCtx {
                clip: &clip,
                buf_bounds: &buf_bounds,
                gradient: node.gradient,
                color_map: node.color_map.as_deref(),
                fg_color_map: node.fg_color_map.as_deref(),
                bg_color_map: node.bg_color_map.as_deref(),
                terminal_bg,
            },
        );
    } else {
        // Text lines mode
        render_lines(f, node, style, rect, &clip, &buf_bounds);
    }
}

struct AsciiCanvasCellSliceCtx<'a> {
    clip: &'a ClipBounds,
    buf_bounds: &'a ClipBounds,
    gradient: Option<(ColorGradient, GradientDirection)>,
    color_map: Option<&'a [(Color, Color)]>,
    fg_color_map: Option<&'a [(Color, Color)]>,
    bg_color_map: Option<&'a [(Color, Color)]>,
    terminal_bg: Option<Color>,
}

fn render_cell_slice(
    f: &mut ratatui::Frame<'_>,
    cells: &[AsciiCell],
    grid_w: u16,
    grid_h: u16,
    base_style: crate::style::Style,
    rect: Rect,
    ctx: AsciiCanvasCellSliceCtx<'_>,
) {
    let AsciiCanvasCellSliceCtx {
        clip,
        buf_bounds,
        gradient,
        color_map,
        fg_color_map,
        bg_color_map,
        terminal_bg,
    } = ctx;
    let base_style = to_ratatui_style_with_terminal_bg(base_style, terminal_bg);
    let rows = rect.h.min(grid_h) as usize;
    let clip_min_row = clip.min_y.saturating_sub(rect.y as i32).max(0) as usize;
    let clip_max_row = (clip.max_y.saturating_sub(rect.y as i32).max(0) as usize).min(rows);

    // Precompute gradient color LUTs - O(rows) or O(cols) ratatui color
    // conversions done once here, then indexed cheaply per cell.
    let row_lut: Option<Vec<RColor>> = gradient.and_then(|(g, dir)| {
        if dir == GradientDirection::Vertical && rows > 0 {
            Some(
                g.precompute(rows)
                    .into_iter()
                    .map(to_ratatui_color)
                    .collect(),
            )
        } else {
            None
        }
    });
    let col_lut: Option<Vec<RColor>> = gradient.and_then(|(g, dir)| {
        if dir == GradientDirection::Horizontal && grid_w > 0 {
            Some(
                g.precompute(grid_w as usize)
                    .into_iter()
                    .map(to_ratatui_color)
                    .collect(),
            )
        } else {
            None
        }
    });

    // Determine whether any color mapping is active.
    let has_any_map = color_map.is_some() || fg_color_map.is_some() || bg_color_map.is_some();

    for row in clip_min_row..clip_max_row {
        let row_start = row.saturating_mul(grid_w as usize);
        let mut idx = row_start;
        let y = rect.y.saturating_add(row as i16) as i32;
        let mut x = rect.x as i32;
        let mut remaining = rect.w.min(grid_w);

        // Vertical gradient: one color per row, applied outside the column loop.
        let row_base = if let Some(ref lut) = row_lut {
            base_style.fg(lut[row.min(lut.len().saturating_sub(1))])
        } else {
            base_style
        };

        while remaining > 0 && idx < cells.len() {
            let cell = cells[idx];

            // Apply color map: per-channel maps take precedence, then unified map.
            let cell = if has_any_map {
                let new_fg = cell
                    .style
                    .fg
                    .and_then(|fg| {
                        let fg_color = fg.color();
                        // Try fg-specific map first, then unified map.
                        fg_color_map
                            .and_then(|m| {
                                m.iter()
                                    .find(|(src, _)| *src == fg_color)
                                    .map(|(_, r)| Paint::from(*r))
                            })
                            .or_else(|| {
                                color_map.and_then(|m| {
                                    m.iter()
                                        .find(|(src, _)| *src == fg_color)
                                        .map(|(_, r)| Paint::from(*r))
                                })
                            })
                    })
                    .or(cell.style.fg);
                let new_bg = cell
                    .style
                    .bg
                    .and_then(|bg| {
                        let bg_color = bg.color();
                        // Try bg-specific map first, then unified map.
                        bg_color_map
                            .and_then(|m| {
                                m.iter()
                                    .find(|(src, _)| *src == bg_color)
                                    .map(|(_, r)| Paint::from(*r))
                            })
                            .or_else(|| {
                                color_map.and_then(|m| {
                                    m.iter()
                                        .find(|(src, _)| *src == bg_color)
                                        .map(|(_, r)| Paint::from(*r))
                                })
                            })
                    })
                    .or(cell.style.bg);
                if new_fg != cell.style.fg || new_bg != cell.style.bg {
                    AsciiCell {
                        style: crate::style::Style {
                            fg: new_fg,
                            bg: new_bg,
                            ..cell.style
                        },
                        ..cell
                    }
                } else {
                    cell
                }
            } else {
                cell
            };

            let cell_rstyle = to_ratatui_style(cell.style);

            // Horizontal gradient: one color per visual column.
            // Priority: gradient overrides node base, cell style patches on top.
            let rstyle = if let Some(ref lut) = col_lut {
                let col_idx = (x - rect.x as i32) as usize;
                base_style
                    .fg(lut[col_idx.min(lut.len().saturating_sub(1))])
                    .patch(cell_rstyle)
            } else {
                row_base.patch(cell_rstyle)
            };

            let ch = cell.ch;
            let w = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
            let symbol = ch.to_string();

            if w == 1 {
                draw_cell(
                    f.buffer_mut(),
                    x,
                    y,
                    &symbol,
                    rstyle,
                    DrawCellClip { clip, buf_bounds },
                );
                x += 1;
                remaining = remaining.saturating_sub(1);
            } else {
                if remaining < w as u16 {
                    break;
                }
                draw_cell(
                    f.buffer_mut(),
                    x,
                    y,
                    &symbol,
                    rstyle,
                    DrawCellClip { clip, buf_bounds },
                );
                for fill in 1..w {
                    draw_cell(
                        f.buffer_mut(),
                        x + fill as i32,
                        y,
                        " ",
                        rstyle,
                        DrawCellClip { clip, buf_bounds },
                    );
                }
                x += w as i32;
                remaining = remaining.saturating_sub(w as u16);
            }
            idx += 1;
        }

        if idx >= cells.len() {
            break;
        }
    }
}

fn render_lines(
    f: &mut ratatui::Frame<'_>,
    node: &AsciiCanvasNode,
    style: crate::style::Style,
    rect: Rect,
    clip: &ClipBounds,
    buf_bounds: &ClipBounds,
) {
    let base_style = to_ratatui_style(style);
    let height = rect.h as usize;
    let total_lines = node.lines.len().min(height);

    // Precompute gradient LUTs.
    let row_lut: Option<Vec<RColor>> = node.gradient.and_then(|(g, dir)| {
        if dir == GradientDirection::Vertical && total_lines > 0 {
            Some(
                g.precompute(total_lines)
                    .into_iter()
                    .map(to_ratatui_color)
                    .collect(),
            )
        } else {
            None
        }
    });
    let col_lut: Option<Vec<RColor>> = node.gradient.and_then(|(g, dir)| {
        if dir == GradientDirection::Horizontal && rect.w > 0 {
            Some(
                g.precompute(rect.w as usize)
                    .into_iter()
                    .map(to_ratatui_color)
                    .collect(),
            )
        } else {
            None
        }
    });

    for (row_idx, line) in node.lines.iter().enumerate().take(height) {
        let y = rect.y.saturating_add(row_idx as i16) as i32;
        let mut x = rect.x as i32;
        let mut remaining = rect.w;

        let row_base = if let Some(ref lut) = row_lut {
            base_style.fg(lut[row_idx.min(lut.len().saturating_sub(1))])
        } else {
            base_style
        };

        for ch in line.chars() {
            if remaining == 0 {
                break;
            }
            let w = UnicodeWidthChar::width(ch).unwrap_or(1).max(1);
            if remaining < w as u16 {
                break;
            }

            let rstyle = if let Some(ref lut) = col_lut {
                let col_idx = (x - rect.x as i32) as usize;
                base_style.fg(lut[col_idx.min(lut.len().saturating_sub(1))])
            } else {
                row_base
            };

            let symbol = ch.to_string();
            draw_cell(
                f.buffer_mut(),
                x,
                y,
                &symbol,
                rstyle,
                DrawCellClip { clip, buf_bounds },
            );
            if w > 1 {
                for fill in 1..w {
                    draw_cell(
                        f.buffer_mut(),
                        x + fill as i32,
                        y,
                        " ",
                        rstyle,
                        DrawCellClip { clip, buf_bounds },
                    );
                }
            }
            x += w as i32;
            remaining = remaining.saturating_sub(w as u16);
        }
    }
}
