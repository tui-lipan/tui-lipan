use ratatui::buffer::Cell as BufferCell;
use ratatui::style::Color as RColor;
use ratatui::widgets::Block;

use crate::backend::ratatui_backend::common::{
    BufferSnapshot, apply_effect_style_clipped, blend_paint_over_ratatui, from_ratatui_color,
    paint_to_ratatui_bg, preserve_palette_blend, resolve_host_palette_color, to_ratatui_color,
    to_ratatui_rect,
};
use crate::core::node::NodeKind;
use crate::style::{Color, ColorTransform, Paint, Rect, Style};

use super::RenderState;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayClearRestoreMode {
    PreserveForeground,
    PreserveBackgroundOnly,
    /// The root paints the terminal background itself, so nothing beneath shows through.
    Opaque,
}

pub(super) fn scale_transform_for_opacity(
    transform: ColorTransform,
    overlay_opacity: f32,
) -> ColorTransform {
    let overlay_opacity = overlay_opacity.clamp(0.0, 1.0);
    match transform {
        ColorTransform::Dim(amount) => ColorTransform::Dim(amount * overlay_opacity),
        ColorTransform::Lighten(amount) => ColorTransform::Lighten(amount * overlay_opacity),
        ColorTransform::Elevate(amount) => ColorTransform::Elevate(amount * overlay_opacity),
        ColorTransform::Opacity(opacity) => {
            let washout = (1.0 - opacity).clamp(0.0, 1.0) * overlay_opacity;
            ColorTransform::Opacity(1.0 - washout)
        }
        ColorTransform::OpacityToward { factor, target } => {
            let washout = (1.0 - factor).clamp(0.0, 1.0) * overlay_opacity;
            ColorTransform::OpacityToward {
                factor: 1.0 - washout,
                target,
            }
        }
        ColorTransform::Tint(color, alpha) => ColorTransform::Tint(color, alpha * overlay_opacity),
    }
}

pub(crate) fn clip_overlay_clear_rect(
    content_rect: Rect,
    overlay_rect: Rect,
) -> ratatui::layout::Rect {
    let absolute_overlay_rect = Rect {
        x: content_rect.x.saturating_add(overlay_rect.x),
        y: content_rect.y.saturating_add(overlay_rect.y),
        w: overlay_rect.w,
        h: overlay_rect.h,
    };
    to_ratatui_rect(absolute_overlay_rect.intersection(&content_rect))
}

pub(crate) fn render_overlay_backdrop(
    state: &mut RenderState<'_, '_, '_>,
    content_rect: Rect,
    style: Style,
    overlay_opacity: f32,
) {
    if style.is_empty() || overlay_opacity <= 0.0 {
        return;
    }
    #[cfg(feature = "image")]
    let placeholders = kitty_placeholder_foregrounds(state.f, content_rect);
    paint_overlay_backdrop(state, content_rect, style, overlay_opacity);
    #[cfg(feature = "image")]
    restore_kitty_placeholder_foregrounds(state.f, placeholders);
}

/// The Unicode placeholder a Kitty virtual placement is drawn with.
#[cfg(feature = "image")]
const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

/// Foregrounds of the Kitty placeholder cells in `rect`.
///
/// A placeholder cell's foreground is not a color: the host reads the image id out of it. A
/// backdrop that tints foregrounds would point the cell at another image, or at none. The image's
/// own pixels are dimmed before they are encoded instead.
#[cfg(feature = "image")]
pub(crate) fn kitty_placeholder_foregrounds(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
) -> Vec<(ratatui::layout::Position, RColor)> {
    let area = f.area().intersection(to_ratatui_rect(rect));
    let buf = f.buffer_mut();
    let mut placeholders = Vec::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if let Some(cell) = buf.cell((x, y))
                && cell.symbol().contains(KITTY_PLACEHOLDER)
            {
                placeholders.push((ratatui::layout::Position::new(x, y), cell.fg));
            }
        }
    }
    placeholders
}

/// Put back the foregrounds [`kitty_placeholder_foregrounds`] recorded, on the cells that are
/// still placeholders.
#[cfg(feature = "image")]
pub(crate) fn restore_kitty_placeholder_foregrounds(
    f: &mut ratatui::Frame<'_>,
    placeholders: Vec<(ratatui::layout::Position, RColor)>,
) {
    let buf = f.buffer_mut();
    for (position, fg) in placeholders {
        if let Some(cell) = buf.cell_mut(position)
            && cell.symbol().contains(KITTY_PLACEHOLDER)
        {
            cell.fg = fg;
        }
    }
}

fn paint_overlay_backdrop(
    state: &mut RenderState<'_, '_, '_>,
    content_rect: Rect,
    style: Style,
    overlay_opacity: f32,
) {
    if overlay_opacity >= 1.0 {
        if let Some(bg) = style.bg
            && let Some(bg) = paint_to_ratatui_bg(bg, state.ctx.terminal_bg.map(from_ratatui_color))
        {
            let block = Block::default().style(ratatui::style::Style::default().bg(bg));
            state.f.render_widget(block, state.content);
        }
        apply_effect_style_clipped(state.f, content_rect, style, None, state.ctx.terminal_bg);
        return;
    }

    if let Some(bg) = style.bg
        && !bg.is_transparent_paint()
        && !bg.is_backdrop_sentinel()
    {
        apply_effect_style_clipped(
            state.f,
            content_rect,
            Style::new().transform_bg(ColorTransform::Tint(bg.color(), overlay_opacity)),
            None,
            state.ctx.terminal_bg,
        );
    }
    if let Some(fg) = style.fg
        && !fg.is_transparent_paint()
        && !fg.is_backdrop_sentinel()
    {
        apply_effect_style_clipped(
            state.f,
            content_rect,
            Style::new().transform_fg(ColorTransform::Tint(fg.color(), overlay_opacity)),
            None,
            state.ctx.terminal_bg,
        );
    }

    let mut effect_style = style;
    effect_style.fg = None;
    effect_style.bg = None;
    effect_style.fg_transform = effect_style
        .fg_transform
        .map(|transform| scale_transform_for_opacity(transform, overlay_opacity));
    effect_style.bg_transform = effect_style
        .bg_transform
        .map(|transform| scale_transform_for_opacity(transform, overlay_opacity));
    effect_style.dim_amount = effect_style
        .dim_amount
        .map(|amount| amount * overlay_opacity);
    effect_style.tint = effect_style
        .tint
        .map(|(color, alpha)| (color, alpha * overlay_opacity));
    apply_effect_style_clipped(
        state.f,
        content_rect,
        effect_style,
        None,
        state.ctx.terminal_bg,
    );
}
pub(crate) fn is_clear_equivalent(cell: &BufferCell) -> bool {
    cell.symbol() == " "
        && cell.bg == RColor::Reset
        && cell.underline_color == RColor::Reset
        && cell.modifier.is_empty()
}

pub(crate) fn composite_overlay_opacity(
    f: &mut ratatui::Frame<'_>,
    rect: ratatui::layout::Rect,
    underlay: &[BufferCell],
    terminal_bg: Option<RColor>,
    opacity: f32,
) {
    let opacity = opacity.clamp(0.0, 1.0);
    let buf = f.buffer_mut();
    for dy in 0..rect.height {
        for dx in 0..rect.width {
            let index = dy as usize * rect.width as usize + dx as usize;
            let Some(saved) = underlay.get(index) else {
                continue;
            };
            let Some(cell) = buf.cell_mut((rect.x + dx, rect.y + dy)) else {
                continue;
            };

            if opacity <= f32::EPSILON {
                *cell = saved.clone();
                continue;
            }
            if cells_match(cell, saved) {
                continue;
            }

            let source_fallback = non_reset(saved.bg).or(terminal_bg);
            let (bg, bg_dim) =
                blend_ratatui_toward(cell.bg, saved.bg, source_fallback, terminal_bg, opacity);
            cell.bg = bg;

            let fg_target = non_reset(cell.bg)
                .or_else(|| non_reset(saved.bg))
                .or(terminal_bg);
            let (fg, fg_dim) = blend_ratatui_toward(
                cell.fg,
                fg_target.unwrap_or(RColor::Reset),
                None,
                terminal_bg,
                opacity,
            );
            cell.fg = fg;
            if bg_dim || fg_dim {
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
/// the cell should gain `DIM`. Palette colors stay on-palette so the terminal palette remains in
/// control.
pub(crate) fn blend_ratatui_toward(
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
        // The terminal owns the concrete color behind Reset, so RGB interpolation is impossible.
        // DIM is the terminal-native opacity fallback and keeps palette semantics intact.
        return (source, opacity < 1.0 && source != RColor::Reset);
    };
    if source == target {
        return (source, false);
    }

    let src = resolve_host_palette_color(from_ratatui_color(source));
    let target = resolve_host_palette_color(from_ratatui_color(target));
    let result = src.blend_toward(target, 1.0 - opacity);
    if let Some(darkened) = preserve_palette_blend(src, result) {
        return (source, darkened);
    }
    (to_ratatui_color(result), false)
}

pub(crate) fn restore_fully_transparent_animated(
    f: &mut ratatui::Frame<'_>,
    snapshot: BufferSnapshot,
    fg_only: bool,
) {
    let buf = f.buffer_mut();
    for dy in 0..snapshot.rect().height {
        for dx in 0..snapshot.rect().width {
            let index = dy as usize * snapshot.rect().width as usize + dx as usize;
            let saved = &snapshot.cells()[index];
            let x = snapshot.rect().x + dx;
            let y = snapshot.rect().y + dy;
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };

            if fg_only {
                let rendered_bg = cell.bg;
                *cell = saved.clone();
                if rendered_bg != RColor::Reset {
                    cell.bg = rendered_bg;
                }
            } else {
                *cell = saved.clone();
            }
        }
    }
}

/// The background paint declared by an overlay's own root node, before resolution.
fn overlay_root_bg(node: &crate::core::node::Node) -> Option<Paint> {
    match &node.kind {
        NodeKind::Frame(frame) => frame.style.bg,
        NodeKind::Center(center) => center.style.bg,
        NodeKind::CenterPin(center) => center.style.bg,
        NodeKind::StatusBarLayout(layout) => layout.style.bg,
        NodeKind::ZStack(stack) => stack.style.bg,
        NodeKind::VStack(stack) => stack.props.style.bg,
        NodeKind::HStack(stack) => stack.props.style.bg,
        NodeKind::Grid(grid) => grid.props.style.bg,
        NodeKind::Flow(flow) => flow.style.bg,
        NodeKind::Animated(_) => None,
        _ => None,
    }
}

/// The translucent surface an overlay paints over the content it covers, if it declared one.
///
/// An overlay is drawn onto a cleared region, so the alpha flattening that happens while its
/// subtree renders has nothing to blend with and falls back to one flat backdrop. That turns a
/// translucent panel into a single opaque colour and throws away the variation underneath, which
/// is the whole point of making it translucent. Reporting the paint here lets the overlay loop
/// redo the blend per cell against what was actually there - see
/// [`composite_overlay_surface_alpha`].
pub(crate) fn overlay_surface_alpha(node: &crate::core::node::Node) -> Option<Paint> {
    let bg = overlay_root_bg(node)?;
    matches!(bg, Paint::Alpha { alpha, .. } if alpha < 255).then_some(bg)
}

/// Re-blend an overlay's translucent background against the cells it covers.
///
/// `underlay` is the pre-clear snapshot, so each cell blends with the colour that was genuinely
/// beneath it: three differently coloured rows stay three colours, shifted toward the surface
/// rather than replaced by it.
///
/// Only cells still holding the flat colour the subtree painted are touched. A child that set its
/// own background renders a different colour and is left alone, so this cannot repaint content
/// layered on top of the surface.
pub(crate) fn composite_overlay_surface_alpha(
    f: &mut ratatui::Frame<'_>,
    rect: ratatui::layout::Rect,
    underlay: &[BufferCell],
    terminal_bg: Option<RColor>,
    surface: Paint,
) {
    let Some(flattened) = paint_to_ratatui_bg(surface, terminal_bg.map(from_ratatui_color)) else {
        return;
    };
    let buf = f.buffer_mut();
    for dy in 0..rect.height {
        for dx in 0..rect.width {
            let index = dy as usize * rect.width as usize + dx as usize;
            let Some(saved) = underlay.get(index) else {
                continue;
            };
            let Some(cell) = buf.cell_mut((rect.x + dx, rect.y + dy)) else {
                continue;
            };
            if cell.bg != flattened {
                continue;
            }
            if let Some(blended) = blend_paint_over_ratatui(surface, saved.bg) {
                cell.bg = blended;
            }
        }
    }
}

pub(crate) fn overlay_clear_restore_mode(
    node: &crate::core::node::Node,
) -> OverlayClearRestoreMode {
    match overlay_root_bg(node) {
        Some(paint) if paint.is_transparent_sentinel() => {
            OverlayClearRestoreMode::PreserveForeground
        }
        Some(Paint::Solid(Color::Reset)) => OverlayClearRestoreMode::Opaque,
        _ => OverlayClearRestoreMode::PreserveBackgroundOnly,
    }
}

#[cfg(all(test, feature = "image"))]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::*;

    /// A backdrop tint must not reach the foreground a Kitty placeholder names its image with,
    /// while the cells beside it still dim.
    #[test]
    fn a_backdrop_keeps_the_image_id_in_kitty_placeholder_cells() {
        let id = RColor::Rgb(0, 0, 8);
        let text = RColor::Rgb(200, 200, 200);
        let rect = Rect {
            x: 0,
            y: 0,
            w: 2,
            h: 1,
        };
        let mut terminal = Terminal::new(TestBackend::new(2, 1)).expect("terminal");
        terminal
            .draw(|f| {
                let buf = f.buffer_mut();
                buf[(0, 0)].set_symbol("\u{10EEEE}\u{305}").set_fg(id);
                buf[(1, 0)].set_symbol("x").set_fg(text);

                let placeholders = kitty_placeholder_foregrounds(f, rect);
                apply_effect_style_clipped(
                    f,
                    rect,
                    Style::new().tint_by(crate::style::Color::rgb(0, 0, 0), 0.5),
                    None,
                    None,
                );
                restore_kitty_placeholder_foregrounds(f, placeholders);

                let buf = f.buffer_mut();
                assert_eq!(buf[(0, 0)].fg, id, "the placeholder still names its image");
                assert_ne!(buf[(1, 0)].fg, text, "ordinary text still dims");
            })
            .expect("draw");
    }
}
