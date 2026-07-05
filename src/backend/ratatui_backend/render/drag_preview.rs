use ratatui::style::{Color as RColor, Style as RStyle};
use ratatui::widgets::Paragraph;

use super::{RenderContext, RenderState};

pub(crate) fn drag_preview_origin(
    ctx: &RenderContext<'_>,
    x: u16,
    y: u16,
    area: ratatui::layout::Rect,
    preview_w: u16,
    preview_h: u16,
) -> (u16, u16) {
    let offset = u16::from(!ctx.drag_preview_at_mouse);
    let px = x
        .saturating_add(offset)
        .min(area.right().saturating_sub(preview_w));
    let py = y
        .saturating_add(offset)
        .min(area.bottom().saturating_sub(preview_h));
    (px, py)
}

pub(crate) fn render_drag_preview(
    state: &mut RenderState<'_, '_, '_>,
    ctx: &RenderContext<'_>,
    label: &str,
    x: u16,
    y: u16,
) {
    if label.is_empty() {
        return;
    }

    let area = state.f.area();
    if area.width == 0 || area.height == 0 {
        return;
    }

    let max_x = area.x.saturating_add(area.width.saturating_sub(1));
    let max_y = area.y.saturating_add(area.height.saturating_sub(1));
    let (mut px, py) = drag_preview_origin(ctx, x, y, area, 1, 1);
    px = px.clamp(area.x, max_x);
    let py = py.clamp(area.y, max_y);

    let remaining = max_x.saturating_sub(px).saturating_add(1) as usize;
    if remaining == 0 {
        return;
    }
    let preview_text = if label.chars().count() > remaining {
        label.chars().take(remaining).collect::<String>()
    } else {
        label.to_owned()
    };

    let rect = ratatui::layout::Rect::new(px, py, preview_text.chars().count() as u16, 1);
    state.f.render_widget(
        Paragraph::new(preview_text).style(RStyle::default().fg(RColor::Black).bg(RColor::Gray)),
        rect,
    );
}

/// Renders a snapshot of `src_rect` cells near the cursor during a `DragPreview::SourceSnapshot`
/// drag. Uses the persistent cache once the source subtree has been collapsed.
pub(crate) fn render_drag_snapshot_preview(
    state: &mut RenderState<'_, '_, '_>,
    ctx: &RenderContext<'_>,
    src_rect: ratatui::layout::Rect,
    cursor_x: u16,
    cursor_y: u16,
) {
    let max_preview_w = ctx
        .drag_preview_max_width
        .unwrap_or(crate::widgets::DEFAULT_PREVIEW_MAX_WIDTH);
    let max_preview_h = ctx
        .drag_preview_max_height
        .unwrap_or(crate::widgets::DEFAULT_PREVIEW_MAX_HEIGHT);

    let area = state.f.area();
    if area.width == 0 || area.height == 0 || src_rect.width == 0 || src_rect.height == 0 {
        return;
    }

    {
        let mut cache = ctx.dnd_snapshot_cells.borrow_mut();
        if cache.is_none() {
            let buf = state.f.buffer_mut();
            let mut cells: Vec<ratatui::buffer::Cell> =
                Vec::with_capacity((src_rect.width as usize) * (src_rect.height as usize));
            for dy in 0..src_rect.height {
                for dx in 0..src_rect.width {
                    let sx = src_rect.x + dx;
                    let sy = src_rect.y + dy;
                    let cell = buf
                        .cell(ratatui::layout::Position::new(sx, sy))
                        .cloned()
                        .unwrap_or_default();
                    cells.push(cell);
                }
            }
            *cache = Some((src_rect.width, src_rect.height, cells));
        }
    }

    let (src_w, src_h, snapshot) = {
        let g = ctx.dnd_snapshot_cells.borrow();
        let Some((w, h, cells)) = g.as_ref() else {
            return;
        };
        (*w, *h, cells.clone())
    };

    let preview_w = src_w.min(max_preview_w);
    let preview_h = src_h.min(max_preview_h);

    let (px, py) = drag_preview_origin(ctx, cursor_x, cursor_y, area, preview_w, preview_h);

    let buf = state.f.buffer_mut();
    for dy in 0..preview_h {
        for dx in 0..preview_w {
            let dst_x = px + dx;
            let dst_y = py + dy;
            if dst_x >= area.right() || dst_y >= area.bottom() {
                continue;
            }
            if let Some(dst) = buf.cell_mut(ratatui::layout::Position::new(dst_x, dst_y)) {
                let src_i = (dy as usize) * (src_w as usize) + (dx as usize);
                *dst = snapshot[src_i].clone();
            }
        }
    }
}

/// Renders the source snapshot at `target_rect` (top-left aligned, clipped to target dimensions).
/// Seeds the snapshot cache from the buffer if not yet populated, then suppresses the cursor float.
pub(crate) fn render_drag_snapshot_at_target(
    state: &mut RenderState<'_, '_, '_>,
    ctx: &RenderContext<'_>,
    src_rect: ratatui::layout::Rect,
    target_rect: ratatui::layout::Rect,
) {
    let area = state.f.area();
    if area.width == 0 || area.height == 0 || src_rect.width == 0 || src_rect.height == 0 {
        return;
    }

    {
        let mut cache = ctx.dnd_snapshot_cells.borrow_mut();
        if cache.is_none() {
            let buf = state.f.buffer_mut();
            let mut cells: Vec<ratatui::buffer::Cell> =
                Vec::with_capacity((src_rect.width as usize) * (src_rect.height as usize));
            for dy in 0..src_rect.height {
                for dx in 0..src_rect.width {
                    let cell = buf
                        .cell(ratatui::layout::Position::new(
                            src_rect.x + dx,
                            src_rect.y + dy,
                        ))
                        .cloned()
                        .unwrap_or_default();
                    cells.push(cell);
                }
            }
            *cache = Some((src_rect.width, src_rect.height, cells));
        }
    }

    let (src_w, src_h, snapshot) = {
        let g = ctx.dnd_snapshot_cells.borrow();
        let Some((w, h, cells)) = g.as_ref() else {
            return;
        };
        (*w, *h, cells.clone())
    };

    let render_w = src_w.min(target_rect.width);
    let render_h = src_h.min(target_rect.height);

    let buf = state.f.buffer_mut();
    for dy in 0..render_h {
        for dx in 0..render_w {
            let dst_x = target_rect.x + dx;
            let dst_y = target_rect.y + dy;
            if dst_x >= area.right() || dst_y >= area.bottom() {
                continue;
            }
            if let Some(dst) = buf.cell_mut(ratatui::layout::Position::new(dst_x, dst_y)) {
                let src_i = (dy as usize) * (src_w as usize) + (dx as usize);
                *dst = snapshot[src_i].clone();
            }
        }
    }
}
