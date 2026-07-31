use std::collections::{HashMap, HashSet};

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

#[derive(Clone, Copy)]
pub(crate) struct RenderedDivider {
    pub orientation: Orientation,
    pub ch: char,
    pub rect: Rect,
    pub clip_rect: Option<Rect>,
    pub label_gap_rect: Option<Rect>,
}

pub(crate) struct DividerRenderCtx {
    pub clip_rect: Option<Rect>,
    pub label_rect: Option<Rect>,
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

    // Only the label's own cells are cleared; padding insets the label but still shows the
    // divider character, so a left inset of 1 yields `─title` rather than a blank.
    let gap_rect = label_rect
        .map(|gap| gap.intersection(&draw_rect))
        .filter(|gap| !gap.is_empty());

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
    let label_rect = divider_label_rect(state, node);
    render_divider(
        state.f,
        divider_node.orientation,
        divider_node.ch,
        resolve_border_style(node.active_theme(), divider_node.style),
        rect,
        DividerRenderCtx {
            clip_rect: clip_bounds,
            label_rect,
        },
    );

    if divider_node.join_frame {
        render_frame_junctions(state, node, divider_node, rect, label_rect);
    }
}

fn divider_label_rect(
    state: &RenderState<'_, '_, '_>,
    node: &crate::core::node::Node,
) -> Option<Rect> {
    node.children.first().and_then(|child_id| {
        if !state.ctx.tree.is_valid(*child_id) {
            return None;
        }
        let child_offset = render_offset_for_node(state.ctx.tree, *child_id);
        let mut child_rect = child_offset.apply_to_rect(state.ctx.tree.node(*child_id).rect);
        child_rect.x = child_rect.x.saturating_add(state.content.x as i16);
        child_rect.y = child_rect.y.saturating_add(state.content.y as i16);
        Some(child_rect)
    })
}

pub(crate) fn rendered_divider(
    state: &RenderState<'_, '_, '_>,
    node: &crate::core::node::Node,
    divider: &crate::widgets::internal::DividerNode,
    mut rect: Rect,
    clip_rect: Option<Rect>,
) -> RenderedDivider {
    match divider.orientation {
        Orientation::Horizontal => rect.h = rect.h.min(1),
        Orientation::Vertical => rect.w = rect.w.min(1),
    }
    if divider.join_frame {
        let mut parent_id = node.parent;
        while let Some(id) = parent_id {
            let parent = state.ctx.tree.node(id);
            if let NodeKind::Frame(props) = &parent.kind {
                if props.has_border() {
                    let parent_offset = render_offset_for_node(state.ctx.tree, id);
                    let mut frame_rect = parent_offset.apply_to_rect(parent.rect);
                    frame_rect.x = frame_rect.x.saturating_add(state.content.x as i16);
                    frame_rect.y = frame_rect.y.saturating_add(state.content.y as i16);
                    match divider.orientation {
                        Orientation::Horizontal => {
                            rect.x = frame_rect.x;
                            rect.w = frame_rect.w;
                        }
                        Orientation::Vertical => {
                            rect.y = frame_rect.y;
                            rect.h = frame_rect.h;
                        }
                    }
                }
                break;
            }
            parent_id = parent.parent;
        }
    }
    let label_gap_rect = divider_label_rect(state, node);

    RenderedDivider {
        orientation: divider.orientation,
        ch: divider.ch,
        rect,
        clip_rect,
        label_gap_rect,
    }
}

/// The continuous `─` run containing `x`, split around a label gap when present.
fn horizontal_run_bounds(divider: RenderedDivider, x: i16) -> Option<(i16, i16)> {
    let start = divider.rect.x;
    let end = divider
        .rect
        .x
        .saturating_add(divider.rect.w as i16)
        .saturating_sub(1);
    if let Some(gap) = divider.label_gap_rect {
        let gap_start = gap.x;
        let gap_end = gap.x.saturating_add(gap.w as i16).saturating_sub(1);
        if x < gap_start {
            return Some((start, gap_start.saturating_sub(1)));
        }
        if x > gap_end {
            return Some((gap_end.saturating_add(1), end));
        }
        return None;
    }
    Some((start, end))
}

/// Box-drawing glyph for every divider arm that meets at `(x, y)`.
///
/// Same-orientation segments that only meet at this cell (two titled horizontals sharing a
/// vertical) must combine into a through-line (`─`) rather than each contributing an endpoint
/// half (`╶`/`╴`); otherwise a vertical that starts here merges into a corner (`┌`) instead of a
/// tee (`┬`).
fn junction_symbol(dividers: &[RenderedDivider], x: i16, y: i16) -> char {
    let mut left = false;
    let mut right = false;
    let mut up = false;
    let mut down = false;
    let mut heavy_h = false;
    let mut heavy_v = false;

    for divider in dividers {
        if !supports_junctions(*divider) {
            continue;
        }
        match divider.orientation {
            Orientation::Horizontal => {
                let Some((start, end)) = horizontal_run_bounds(*divider, x) else {
                    continue;
                };
                if start < x {
                    left = true;
                }
                if end > x {
                    right = true;
                }
                if matches!(divider.ch, '━') {
                    heavy_h = true;
                }
            }
            Orientation::Vertical => {
                let start = divider.rect.y;
                let end = divider
                    .rect
                    .y
                    .saturating_add(divider.rect.h as i16)
                    .saturating_sub(1);
                if start < y {
                    up = true;
                }
                if end > y {
                    down = true;
                }
                if matches!(divider.ch, '┃') {
                    heavy_v = true;
                }
            }
        }
    }

    match (left, right, up, down, heavy_h || heavy_v) {
        (true, true, true, true, false) => '┼',
        (true, true, true, true, true) => '╋',
        (true, true, false, true, false) => '┬',
        (true, true, false, true, true) => '┳',
        (true, true, true, false, false) => '┴',
        (true, true, true, false, true) => '┻',
        (false, true, true, true, false) => '├',
        (false, true, true, true, true) => '┣',
        (true, false, true, true, false) => '┤',
        (true, false, true, true, true) => '┫',
        (false, true, false, true, false) => '┌',
        (false, true, false, true, true) => '┏',
        (true, false, false, true, false) => '┐',
        (true, false, false, true, true) => '┓',
        (false, true, true, false, false) => '└',
        (false, true, true, false, true) => '┗',
        (true, false, true, false, false) => '┘',
        (true, false, true, false, true) => '┛',
        (true, true, false, false, false) => '─',
        (true, true, false, false, true) => '━',
        (false, false, true, true, false) => '│',
        (false, false, true, true, true) => '┃',
        // Degenerate single-arm leftovers: fall back to a light endpoint.
        (false, true, false, false, _) => '╶',
        (true, false, false, false, _) => '╴',
        (false, false, false, true, _) => '╷',
        (false, false, true, false, _) => '╵',
        _ => '─',
    }
}

fn supports_junctions(divider: RenderedDivider) -> bool {
    matches!(
        (divider.orientation, divider.ch),
        (Orientation::Horizontal, '─' | '━') | (Orientation::Vertical, '│' | '┃')
    )
}

fn for_each_visible_cell(divider: RenderedDivider, mut visit: impl FnMut(i16, i16)) {
    let visible = divider
        .clip_rect
        .map_or(divider.rect, |clip| divider.rect.intersection(&clip));
    match divider.orientation {
        Orientation::Horizontal if visible.h > 0 => {
            let end = visible.x.saturating_add(visible.w as i16);
            for x in visible.x..end {
                if divider
                    .label_gap_rect
                    .is_none_or(|gap| !gap.contains(x, visible.y))
                {
                    visit(x, visible.y);
                }
            }
        }
        Orientation::Vertical if visible.w > 0 => {
            let end = visible.y.saturating_add(visible.h as i16);
            for y in visible.y..end {
                if divider
                    .label_gap_rect
                    .is_none_or(|gap| !gap.contains(visible.x, y))
                {
                    visit(visible.x, y);
                }
            }
        }
        _ => {}
    }
}

struct DividerCell {
    dividers: Vec<RenderedDivider>,
    rendered: ratatui::buffer::Cell,
}

pub(crate) struct PreparedDivider {
    divider: RenderedDivider,
    junctions: HashSet<(i16, i16)>,
}

#[derive(Default)]
pub(crate) struct DividerJunctionState {
    cells: HashMap<(i16, i16), DividerCell>,
}

impl DividerJunctionState {
    pub(crate) fn prepare(
        &self,
        f: &mut ratatui::Frame<'_>,
        divider: RenderedDivider,
    ) -> PreparedDivider {
        let mut junctions = HashSet::new();
        if supports_junctions(divider) {
            let buf = f.buffer_mut();
            for_each_visible_cell(divider, |x, y| {
                let Some(existing) = self.cells.get(&(x, y)) else {
                    return;
                };
                if !existing.dividers.iter().any(|prior| {
                    prior.orientation != divider.orientation && supports_junctions(*prior)
                }) {
                    return;
                }
                if buf
                    .cell((x as u16, y as u16))
                    .is_some_and(|cell| cell == &existing.rendered)
                {
                    junctions.insert((x, y));
                }
            });
        }
        PreparedDivider { divider, junctions }
    }

    pub(crate) fn finish(&mut self, f: &mut ratatui::Frame<'_>, prepared: PreparedDivider) {
        let divider = prepared.divider;
        let buf = f.buffer_mut();
        for_each_visible_cell(divider, |x, y| {
            let key = (x, y);
            if prepared.junctions.contains(&key)
                && let Some(existing) = self.cells.get_mut(&key)
                && let Some(cell) = buf.cell_mut((x as u16, y as u16))
            {
                let mut arms = existing.dividers.clone();
                arms.push(divider);
                cell.set_char(junction_symbol(&arms, x, y));
                existing.dividers.push(divider);
                existing.rendered = cell.clone();
                return;
            }

            if let Some(cell) = buf.cell((x as u16, y as u16)) {
                if let Some(existing) = self.cells.get_mut(&key) {
                    // Same-axis segments that share a cell (two titled horizontals meeting at a
                    // vertical) must all remain visible to the later perpendicular pass; replacing
                    // would leave only the last endpoint half and compose a corner (`┌`) instead
                    // of a tee (`┬`).
                    existing.dividers.push(divider);
                    existing.rendered = cell.clone();
                } else {
                    self.cells.insert(
                        key,
                        DividerCell {
                            dividers: vec![divider],
                            rendered: cell.clone(),
                        },
                    );
                }
            }
        });
    }
}

fn render_frame_junctions(
    state: &mut RenderState<'_, '_, '_>,
    node: &crate::core::node::Node,
    divider_node: &crate::widgets::internal::DividerNode,
    rect: Rect,
    label_rect: Option<Rect>,
) {
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
                let (style, _) =
                    crate::backend::ratatui_backend::renderers::frame::render::resolve_block_style(
                        props, active, is_hovered,
                    );
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
            let gap_rect = label_rect;

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
