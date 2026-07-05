use crate::backend::ratatui_backend::common::{
    ClipBounds, DrawCellClip, draw_cell, to_ratatui_style,
};
use crate::backend::ratatui_backend::render::{
    RenderState, is_box_drawing_symbol, render_offset_for_node, scroll_view_clip_rect,
    to_merge_strategy,
};
use crate::core::node::NodeKind;
use crate::style::resolve::resolve_border_style;
use crate::style::{Rect, Style};
use crate::widgets::BorderMergeMode;
use crate::widgets::Orientation;

pub(crate) struct DividerRenderCtx {
    pub clip_rect: Option<Rect>,
    pub label_rect: Option<Rect>,
    pub label_padding: u16,
}

pub(crate) fn render_divider(
    f: &mut ratatui::Frame<'_>,
    orientation: Orientation,
    ch: char,
    style: Style,
    rect: Rect,
    ctx: DividerRenderCtx,
) {
    let DividerRenderCtx {
        clip_rect,
        label_rect,
        label_padding,
    } = ctx;
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let mut draw_rect = rect;
    match orientation {
        Orientation::Horizontal => draw_rect.h = 1,
        Orientation::Vertical => draw_rect.w = 1,
    }

    if draw_rect.w == 0 || draw_rect.h == 0 {
        return;
    }

    if let Some(clip) = clip_rect
        && draw_rect.intersection(&clip).is_empty()
    {
        return;
    }

    let mut gap_rect = label_rect;
    if let Some(gap) = &mut gap_rect {
        let pad = label_padding as i16;
        gap.x = gap.x.saturating_sub(pad);
        gap.w = gap.w.saturating_add(label_padding.saturating_mul(2));
        *gap = gap.intersection(&draw_rect);
        if gap.is_empty() {
            gap_rect = None;
        }
    }

    let buf = f.buffer_mut();
    let clip = clip_rect
        .map(ClipBounds::from_rect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);
    let rstyle = to_ratatui_style(style);
    let symbol = ch.to_string();

    match orientation {
        Orientation::Horizontal => {
            let y = draw_rect.y as i32;
            let start_x = draw_rect.x as i32;
            let end_x = draw_rect
                .x
                .saturating_add(draw_rect.w as i16)
                .saturating_sub(1) as i32;
            for x in start_x..=end_x {
                if gap_rect.is_some_and(|gap| gap.contains(x as i16, draw_rect.y)) {
                    continue;
                }
                draw_cell(
                    buf,
                    x,
                    y,
                    &symbol,
                    rstyle,
                    DrawCellClip {
                        clip: &clip,
                        buf_bounds: &buf_bounds,
                    },
                );
            }
        }
        Orientation::Vertical => {
            let x = draw_rect.x as i32;
            let start_y = draw_rect.y as i32;
            let end_y = draw_rect
                .y
                .saturating_add(draw_rect.h as i16)
                .saturating_sub(1) as i32;
            for y in start_y..=end_y {
                if gap_rect.is_some_and(|gap| gap.contains(draw_rect.x, y as i16)) {
                    continue;
                }
                draw_cell(
                    buf,
                    x,
                    y,
                    &symbol,
                    rstyle,
                    DrawCellClip {
                        clip: &clip,
                        buf_bounds: &buf_bounds,
                    },
                );
            }
        }
    }
}

pub(crate) fn render_divider_node(
    state: &mut RenderState<'_, '_, '_>,
    node: &crate::core::node::Node,
    divider_node: &crate::widgets::internal::DividerNode,
    rect: Rect,
    clip_bounds: Option<Rect>,
) {
    let label_rect = node.children.first().and_then(|child_id| {
        if !state.ctx.tree.is_valid(*child_id) {
            return None;
        }
        let child_offset = render_offset_for_node(state.ctx.tree, *child_id);
        let mut child_rect = child_offset.apply_to_rect(state.ctx.tree.node(*child_id).rect);
        child_rect.x = child_rect.x.saturating_add(state.content.x as i16);
        child_rect.y = child_rect.y.saturating_add(state.content.y as i16);
        Some(child_rect)
    });
    render_divider(
        state.f,
        divider_node.orientation,
        divider_node.ch,
        resolve_border_style(node.active_theme(), divider_node.style),
        rect,
        DividerRenderCtx {
            clip_rect: clip_bounds,
            label_rect,
            label_padding: divider_node.label_padding,
        },
    );

    if divider_node.join_frame {
        let mut parent_id = node.parent;
        let mut frame_rect = None;
        let mut frame_style = None;
        let mut frame_merge_mode = None;
        while let Some(id) = parent_id {
            let parent = state.ctx.tree.node(id);
            if let NodeKind::Frame(props) = &parent.kind {
                if props.has_border() {
                    let active = state.focus_chain.contains(&id);
                    let is_hovered = Some(id) == state.ctx.hovered;
                    let (style, _) = crate::backend::ratatui_backend::renderers::frame::render::resolve_block_style(props, active, is_hovered);
                    let parent_offset = render_offset_for_node(state.ctx.tree, id);
                    let mut absolute = parent_offset.apply_to_rect(parent.rect);
                    absolute.x = absolute.x.saturating_add(state.content.x as i16);
                    absolute.y = absolute.y.saturating_add(state.content.y as i16);
                    frame_rect = Some(absolute);
                    frame_style = Some(style);
                    frame_merge_mode = Some(props.border_merge_mode);
                }
                break;
            }
            parent_id = parent.parent;
        }
        if let (Some(frame_rect), Some(frame_style), Some(frame_merge_mode)) =
            (frame_rect, frame_style, frame_merge_mode)
            && frame_rect.w > 1
            && frame_rect.h > 1
        {
            let buf = state.f.buffer_mut();
            let buf_bounds = ClipBounds::from_rrect(buf.area);
            let border_style = to_ratatui_style(frame_style);
            let line_style = to_ratatui_style(resolve_border_style(
                node.active_theme(),
                divider_node.style,
            ));
            let symbol = divider_node.ch.to_string();
            let scroll_clip = scroll_view_clip_rect(state.ctx.tree, node.parent, state.content);
            let mut clip_rect = frame_rect;
            if let Some(clip) = scroll_clip {
                clip_rect = clip_rect.intersection(&clip);
            }
            if !clip_rect.is_empty() {
                let clip = ClipBounds::from_rect(clip_rect);
                let mut gap_rect = label_rect;
                if let Some(gap) = &mut gap_rect {
                    let pad = divider_node.label_padding as i16;
                    gap.x = gap.x.saturating_sub(pad);
                    gap.w = gap
                        .w
                        .saturating_add(divider_node.label_padding.saturating_mul(2));
                }

                match divider_node.orientation {
                    crate::widgets::Orientation::Horizontal => {
                        let y = rect.y;
                        if y > frame_rect.y
                            && y < frame_rect
                                .y
                                .saturating_add(frame_rect.h as i16)
                                .saturating_sub(1)
                        {
                            let start_x = frame_rect.x;
                            let end_x = frame_rect
                                .x
                                .saturating_add(frame_rect.w as i16)
                                .saturating_sub(1);

                            for x in start_x..=end_x {
                                if gap_rect.is_some_and(|gap| gap.contains(x, y)) {
                                    continue;
                                }

                                if !clip.contains(x as i32, y as i32)
                                    || !buf_bounds.contains(x as i32, y as i32)
                                {
                                    continue;
                                }

                                let Some(cell) = buf.cell_mut((x as u16, y as u16)) else {
                                    continue;
                                };

                                let at_border = x == start_x || x == end_x;
                                let endpoint_symbol = if x == start_x { "╶" } else { "╴" };
                                let merge_symbol = if at_border {
                                    endpoint_symbol
                                } else {
                                    symbol.as_str()
                                };
                                let should_merge = at_border
                                    && frame_merge_mode != BorderMergeMode::Replace
                                    && is_box_drawing_symbol(cell.symbol())
                                    && is_box_drawing_symbol(merge_symbol);

                                if should_merge {
                                    cell.merge_symbol(
                                        merge_symbol,
                                        to_merge_strategy(frame_merge_mode),
                                    );
                                    cell.set_style(border_style);
                                } else if at_border {
                                    // Keep existing border glyph/style when merge cannot apply.
                                } else {
                                    cell.set_symbol(symbol.as_str()).set_style(line_style);
                                }
                            }
                        }
                    }
                    crate::widgets::Orientation::Vertical => {
                        let x = rect.x;
                        if x > frame_rect.x
                            && x < frame_rect
                                .x
                                .saturating_add(frame_rect.w as i16)
                                .saturating_sub(1)
                        {
                            let start_y = frame_rect.y;
                            let end_y = frame_rect
                                .y
                                .saturating_add(frame_rect.h as i16)
                                .saturating_sub(1);

                            for y in start_y..=end_y {
                                if !clip.contains(x as i32, y as i32)
                                    || !buf_bounds.contains(x as i32, y as i32)
                                {
                                    continue;
                                }

                                let Some(cell) = buf.cell_mut((x as u16, y as u16)) else {
                                    continue;
                                };

                                let at_border = y == start_y || y == end_y;
                                let endpoint_symbol = if y == start_y { "╷" } else { "╵" };
                                let merge_symbol = if at_border {
                                    endpoint_symbol
                                } else {
                                    symbol.as_str()
                                };
                                let should_merge = at_border
                                    && frame_merge_mode != BorderMergeMode::Replace
                                    && is_box_drawing_symbol(cell.symbol())
                                    && is_box_drawing_symbol(merge_symbol);

                                if should_merge {
                                    cell.merge_symbol(
                                        merge_symbol,
                                        to_merge_strategy(frame_merge_mode),
                                    );
                                    cell.set_style(border_style);
                                } else if at_border {
                                    // Keep existing border glyph/style when merge cannot apply.
                                } else {
                                    cell.set_symbol(symbol.as_str()).set_style(line_style);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}
