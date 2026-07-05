use ratatui::buffer::Cell as BufferCell;
use ratatui::style::Color as RColor;

use crate::backend::ratatui_backend::common::{
    apply_effect_style_clipped, from_ratatui_color, preserve_palette_blend, to_ratatui_color,
    to_ratatui_rect,
};
use crate::backend::ratatui_backend::render::AnimatedRestoreSnapshot;
use crate::style::{ColorTransform, Rect, Style};
use crate::widgets::internal::AnimatedNode;

pub(crate) fn render_animated(
    f: &mut ratatui::Frame<'_>,
    node: &AnimatedNode,
    rect: Rect,
    clip_rect: Option<Rect>,
    underlay: Option<&AnimatedRestoreSnapshot>,
    terminal_bg: Option<ratatui::style::Color>,
) {
    let opacity = node.opacity.clamp(0.0, 1.0);
    let has_fg_override = node.current_fg.is_some();
    let has_bg_override = node.current_bg.is_some();
    if opacity >= 1.0 && !has_fg_override && !has_bg_override {
        return;
    }

    if has_fg_override || has_bg_override {
        let mut draw_rect = rect;
        if let Some(clip) = clip_rect {
            draw_rect = draw_rect.intersection(&clip);
        }
        if !draw_rect.is_empty() {
            let r_rect = crate::backend::ratatui_backend::common::to_ratatui_rect(draw_rect);
            let intersection = f.area().intersection(r_rect);
            if intersection.width > 0 && intersection.height > 0 {
                let fg = node.current_fg.map(to_ratatui_color);
                let bg = node.current_bg.map(to_ratatui_color);
                let buf = f.buffer_mut();
                for y in intersection.y..intersection.y + intersection.height {
                    for x in intersection.x..intersection.x + intersection.width {
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            if let Some(fg) = fg {
                                cell.fg = fg;
                            }
                            if let Some(bg) = bg {
                                cell.bg = bg;
                            }
                        }
                    }
                }
            }
        }
    }

    if opacity >= 1.0 {
        return;
    }

    if node.opacity_target.is_none()
        && let Some(underlay) = underlay
    {
        composite_opacity_over_underlay(
            f,
            node.opacity_fg_only,
            rect,
            clip_rect,
            underlay,
            terminal_bg,
            opacity,
        );
        return;
    }

    let opacity_tf = if let Some(target) = node.opacity_target {
        ColorTransform::OpacityToward {
            factor: opacity,
            target,
        }
    } else {
        ColorTransform::Opacity(opacity)
    };

    let mut style = Style::new().transform_fg(opacity_tf);
    if !node.opacity_fg_only {
        style = style.transform_bg(opacity_tf);
    }

    apply_effect_style_clipped(f, rect, style, clip_rect, terminal_bg);
}

fn composite_opacity_over_underlay(
    f: &mut ratatui::Frame<'_>,
    fg_only: bool,
    rect: Rect,
    clip_rect: Option<Rect>,
    underlay: &AnimatedRestoreSnapshot,
    terminal_bg: Option<RColor>,
    opacity: f32,
) {
    let mut draw_rect = rect;
    if let Some(clip) = clip_rect {
        draw_rect = draw_rect.intersection(&clip);
    }
    if draw_rect.is_empty() {
        return;
    }

    let r_rect = to_ratatui_rect(draw_rect);
    let intersection = f.area().intersection(r_rect);
    if intersection.width == 0 || intersection.height == 0 {
        return;
    }

    let buf = f.buffer_mut();
    for y in intersection.y..intersection.y + intersection.height {
        for x in intersection.x..intersection.x + intersection.width {
            let Some(saved) = underlay.cell_at(x, y) else {
                continue;
            };
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };
            if cells_match(cell, saved) {
                continue;
            }

            let mut dim_cell = false;
            if !fg_only {
                let source_fallback = non_reset(saved.bg).or(terminal_bg);
                let (bg, dim) =
                    blend_ratatui_toward(cell.bg, saved.bg, source_fallback, terminal_bg, opacity);
                cell.bg = bg;
                dim_cell |= dim;
            }

            let fg_target = non_reset(cell.bg)
                .or_else(|| non_reset(saved.bg))
                .or(terminal_bg);
            if let Some(target) = fg_target {
                let (fg, dim) = blend_ratatui_toward(cell.fg, target, None, terminal_bg, opacity);
                cell.fg = fg;
                dim_cell |= dim;
            }
            if dim_cell {
                cell.set_style(cell.style().add_modifier(ratatui::style::Modifier::DIM));
            }
        }
    }
}

fn cells_match(cell: &BufferCell, saved: &BufferCell) -> bool {
    cell.symbol() == saved.symbol()
        && cell.fg == saved.fg
        && cell.bg == saved.bg
        && cell.underline_color == saved.underline_color
        && cell.modifier == saved.modifier
}

fn non_reset(color: RColor) -> Option<RColor> {
    (color != RColor::Reset).then_some(color)
}

/// Blend `source` toward `target` by `1.0 - opacity`, returning the resolved color and whether
/// the cell should gain `DIM`. A palette color whose blend would carry a foreign hue is kept
/// on-palette (see [`preserve_palette_blend`]) so the user's terminal palette stays in control.
fn blend_ratatui_toward(
    source: RColor,
    target: RColor,
    source_fallback: Option<RColor>,
    target_fallback: Option<RColor>,
    opacity: f32,
) -> (RColor, bool) {
    if source == RColor::Reset && source_fallback.is_none() {
        return (source, false);
    }
    let source = non_reset(source).or(source_fallback).unwrap_or(source);
    let Some(target) = non_reset(target).or(target_fallback) else {
        return (source, false);
    };
    if source == target {
        return (source, false);
    }

    let src = from_ratatui_color(source);
    let result = src.blend_toward(from_ratatui_color(target), 1.0 - opacity);
    if let Some(darkened) = preserve_palette_blend(src, result) {
        return (source, darkened);
    }
    (to_ratatui_color(result), false)
}
