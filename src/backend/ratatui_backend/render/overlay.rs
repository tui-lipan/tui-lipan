use ratatui::buffer::Cell as BufferCell;
use ratatui::style::Color as RColor;
use ratatui::widgets::Block;

use crate::backend::ratatui_backend::common::{
    apply_effect_style_clipped, from_ratatui_color, paint_to_ratatui_bg, to_ratatui_rect,
};
use crate::core::node::NodeKind;
use crate::style::{ColorTransform, Rect, Style};

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

pub(crate) fn overlay_clear_restore_mode(
    node: &crate::core::node::Node,
) -> OverlayClearRestoreMode {
    let bg = match &node.kind {
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
    };

    if matches!(bg, Some(paint) if paint.is_transparent_sentinel()) {
        OverlayClearRestoreMode::PreserveForeground
    } else {
        OverlayClearRestoreMode::PreserveBackgroundOnly
    }
}
