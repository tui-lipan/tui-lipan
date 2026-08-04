use ratatui::buffer::Cell as BufferCell;
use ratatui::style::Color as RColor;
use ratatui::widgets::Block;

use crate::backend::ratatui_backend::common::{
    apply_effect_style_clipped, blend_paint_over_ratatui, from_ratatui_color, paint_to_ratatui_bg,
    preserve_palette_blend, to_ratatui_color, to_ratatui_rect,
};
use crate::core::node::NodeKind;
use crate::style::{ColorTransform, Paint, Rect, Style};

use super::RenderState;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OverlayClearRestoreMode {
    PreserveForeground,
    PreserveBackgroundOnly,
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

    let src = from_ratatui_color(source);
    let result = src.blend_toward(from_ratatui_color(target), 1.0 - opacity);
    if let Some(darkened) = preserve_palette_blend(src, result) {
        return (source, darkened);
    }
    (to_ratatui_color(result), false)
}

pub(crate) struct AnimatedRestoreSnapshot {
    rect: ratatui::layout::Rect,
    cells: Vec<BufferCell>,
}

impl AnimatedRestoreSnapshot {
    pub(crate) fn cell_at(&self, x: u16, y: u16) -> Option<&BufferCell> {
        if x < self.rect.x
            || y < self.rect.y
            || x >= self.rect.x.saturating_add(self.rect.width)
            || y >= self.rect.y.saturating_add(self.rect.height)
        {
            return None;
        }

        let dx = x.saturating_sub(self.rect.x) as usize;
        let dy = y.saturating_sub(self.rect.y) as usize;
        let index = dy
            .saturating_mul(self.rect.width as usize)
            .saturating_add(dx);
        self.cells.get(index)
    }
}

pub(crate) fn snapshot_animated_restore_rect(
    f: &mut ratatui::Frame<'_>,
    rect: Rect,
    clip_rect: Option<Rect>,
) -> Option<AnimatedRestoreSnapshot> {
    let mut draw_rect = rect;
    if let Some(clip) = clip_rect {
        draw_rect = draw_rect.intersection(&clip);
    }
    if draw_rect.is_empty() {
        return None;
    }

    let r_rect = to_ratatui_rect(draw_rect);
    let intersection = f.area().intersection(r_rect);
    if intersection.width == 0 || intersection.height == 0 {
        return None;
    }

    let buf = f.buffer_mut();
    let mut cells = Vec::with_capacity(intersection.width as usize * intersection.height as usize);
    for y in intersection.y..intersection.y + intersection.height {
        for x in intersection.x..intersection.x + intersection.width {
            cells.push(buf.cell((x, y)).cloned().unwrap_or(BufferCell::EMPTY));
        }
    }

    Some(AnimatedRestoreSnapshot {
        rect: intersection,
        cells,
    })
}

pub(crate) fn restore_fully_transparent_animated(
    f: &mut ratatui::Frame<'_>,
    snapshot: AnimatedRestoreSnapshot,
    fg_only: bool,
) {
    let buf = f.buffer_mut();
    for dy in 0..snapshot.rect.height {
        for dx in 0..snapshot.rect.width {
            let index = dy as usize * snapshot.rect.width as usize + dx as usize;
            let saved = &snapshot.cells[index];
            let x = snapshot.rect.x + dx;
            let y = snapshot.rect.y + dy;
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
    if matches!(overlay_root_bg(node), Some(paint) if paint.is_transparent_sentinel()) {
        OverlayClearRestoreMode::PreserveForeground
    } else {
        OverlayClearRestoreMode::PreserveBackgroundOnly
    }
}
