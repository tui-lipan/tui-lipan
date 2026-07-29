use ratatui::buffer::Cell as BufferCell;
use ratatui::style::Color as RColor;

use crate::backend::ratatui_backend::common::{
    apply_effect_style_clipped, from_ratatui_color, to_ratatui_color, to_ratatui_rect,
};
use crate::backend::ratatui_backend::render::{AnimatedRestoreSnapshot, blend_ratatui_toward};
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
    let has_fg_override = node.current_fg.is_some() || node.inherited_fg_exit.is_some();
    let has_bg_override = node.current_bg.is_some() || node.inherited_bg_exit.is_some();
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
                let inherited_fg = node
                    .inherited_fg_exit
                    .as_ref()
                    .map(|exit| (exit.target, exit.progress.current().clamp(0.0, 1.0)));
                let inherited_bg = node
                    .inherited_bg_exit
                    .as_ref()
                    .map(|exit| (exit.target, exit.progress.current().clamp(0.0, 1.0)));
                let buf = f.buffer_mut();
                for y in intersection.y..intersection.y + intersection.height {
                    for x in intersection.x..intersection.x + intersection.width {
                        if let Some(cell) = buf.cell_mut((x, y)) {
                            if let Some(fg) = fg {
                                cell.fg = fg;
                            } else if let Some((target, progress)) = inherited_fg {
                                cell.fg = to_ratatui_color(
                                    from_ratatui_color(cell.fg).blend_toward(target, progress),
                                );
                            }
                            if let Some(bg) = bg {
                                cell.bg = bg;
                            } else if let Some((target, progress)) = inherited_bg {
                                let source = non_reset(cell.bg).or(terminal_bg).unwrap_or(cell.bg);
                                cell.bg = to_ratatui_color(
                                    from_ratatui_color(source).blend_toward(target, progress),
                                );
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color as RColor;

    use super::render_animated;
    use crate::animation::{Easing, ExitAnimation};
    use crate::style::{Color, Rect};
    use crate::widgets::{Animated, Text};

    #[test]
    fn inherited_exit_colors_blend_from_the_rendered_child_colors() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 1,
            h: 1,
        };
        let mut node = crate::widgets::internal::AnimatedNode::from(
            Animated::new(Text::new("x")).auto_exit(
                ExitAnimation::new(100)
                    .keep_opacity()
                    .fg(Color::Rgb(220, 240, 200))
                    .bg(Color::Rgb(200, 220, 240))
                    .easing(Easing::Linear),
            ),
        );
        assert!(node.begin_auto_exit(None));
        node.tick(Duration::from_millis(50));
        assert!(node.current_fg.is_none() && node.current_bg.is_none());

        let backend = TestBackend::new(1, 1);
        let mut terminal = Terminal::new(backend).expect("terminal should initialize");
        terminal
            .draw(|frame| {
                let cell = frame.buffer_mut().cell_mut((0, 0)).expect("cell");
                cell.fg = RColor::Rgb(20, 40, 60);
                cell.bg = RColor::Reset;
                render_animated(
                    frame,
                    &node,
                    rect,
                    None,
                    None,
                    Some(RColor::Rgb(60, 40, 20)),
                );
            })
            .expect("draw should succeed");

        let cell = &terminal.backend().buffer()[(0, 0)];
        assert_eq!(cell.fg, RColor::Rgb(120, 140, 130));
        assert_eq!(cell.bg, RColor::Rgb(130, 130, 130));
    }
}
