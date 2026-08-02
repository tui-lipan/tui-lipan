use crate::backend::ratatui_backend::common::{ClipBounds, to_ratatui_style};
use crate::backend::ratatui_backend::render::RenderState;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::resolve::{
    resolve_base_style, resolve_splitter_active_style, resolve_splitter_hover_style,
};
use crate::style::{Rect, Style};
use crate::widgets::Orientation;

pub(crate) struct SplitterHandleRender<'a> {
    pub orientation: Orientation,
    pub handle_rects: &'a [Rect],
    pub symbol: char,
    pub style: Style,
    pub hover_style: Style,
    pub active_style: Style,
    pub hovered_handle: Option<usize>,
    pub active_handle: Option<usize>,
    pub junctions: &'a [SplitterJunction],
    pub preserve_existing_symbols: bool,
    pub clip_rect: Option<Rect>,
}

#[derive(Clone, Copy)]
pub(crate) struct SplitterJunction {
    x: i32,
    y: i32,
    symbol: char,
}

#[derive(Clone, Copy, Default)]
struct JunctionArms {
    left: bool,
    right: bool,
    up: bool,
    down: bool,
}

#[derive(Clone, Copy)]
enum JunctionCharset {
    BoxDrawing,
    Ascii,
}

fn compatible_junction_charset(
    orientation: Orientation,
    symbol: char,
    other_symbol: char,
) -> Option<JunctionCharset> {
    match (orientation, symbol, other_symbol) {
        (Orientation::Vertical, '│', '─') | (Orientation::Horizontal, '─', '│') => {
            Some(JunctionCharset::BoxDrawing)
        }
        (Orientation::Vertical, '|', '-') | (Orientation::Horizontal, '-', '|') => {
            Some(JunctionCharset::Ascii)
        }
        _ => None,
    }
}

fn junction_symbol(arms: JunctionArms, charset: JunctionCharset) -> char {
    if matches!(charset, JunctionCharset::Ascii) {
        return '+';
    }
    match (arms.left, arms.right, arms.up, arms.down) {
        (true, true, true, true) => '┼',
        (true, false, true, true) => '┤',
        (false, true, true, true) => '├',
        (true, true, true, false) => '┴',
        (true, true, false, true) => '┬',
        (false, true, false, true) => '┌',
        (true, false, false, true) => '┐',
        (false, true, true, false) => '└',
        (true, false, true, false) => '┘',
        (true, true, false, false) => '─',
        (false, false, true, true) => '│',
        _ => '+',
    }
}

fn rect_end_x(rect: Rect) -> i32 {
    i32::from(rect.x) + i32::from(rect.w) - 1
}

fn rect_end_y(rect: Rect) -> i32 {
    i32::from(rect.y) + i32::from(rect.h) - 1
}

fn splitter_junctions(
    tree: &NodeTree,
    node_id: NodeId,
    splitter: &crate::widgets::internal::SplitterNode,
    offset_x: i32,
    offset_y: i32,
) -> Vec<SplitterJunction> {
    let mut junctions = Vec::new();
    for rect in &splitter.handle_rects {
        if rect.w == 0 || rect.h == 0 {
            continue;
        }
        let end_x = rect_end_x(*rect);
        let end_y = rect_end_y(*rect);
        for y in i32::from(rect.y)..=end_y {
            for x in i32::from(rect.x)..=end_x {
                let mut arms = match splitter.orientation {
                    Orientation::Horizontal => JunctionArms {
                        left: x > i32::from(rect.x),
                        right: x < end_x,
                        ..JunctionArms::default()
                    },
                    Orientation::Vertical => JunctionArms {
                        up: y > i32::from(rect.y),
                        down: y < end_y,
                        ..JunctionArms::default()
                    },
                };
                let base = arms;
                let mut charset = None;

                for node in tree.iter() {
                    if node.id == node_id {
                        continue;
                    }
                    let NodeKind::Splitter(other) = &node.kind else {
                        continue;
                    };
                    if other.orientation == splitter.orientation {
                        continue;
                    }
                    let Some(other_charset) = compatible_junction_charset(
                        splitter.orientation,
                        splitter.handle_symbol,
                        other.handle_symbol,
                    ) else {
                        continue;
                    };

                    for other_rect in &other.handle_rects {
                        if other_rect.w == 0 || other_rect.h == 0 {
                            continue;
                        }
                        let other_end_x = rect_end_x(*other_rect);
                        let other_end_y = rect_end_y(*other_rect);
                        match splitter.orientation {
                            Orientation::Vertical
                                if y >= i32::from(other_rect.y) && y <= other_end_y =>
                            {
                                arms.left |= other_end_x + 1 == x
                                    || (i32::from(other_rect.x) < x && other_end_x >= x);
                                arms.right |= i32::from(other_rect.x) == x + 1
                                    || (i32::from(other_rect.x) <= x && other_end_x > x);
                            }
                            Orientation::Horizontal
                                if x >= i32::from(other_rect.x) && x <= other_end_x =>
                            {
                                arms.up |= other_end_y + 1 == y
                                    || (i32::from(other_rect.y) < y && other_end_y >= y);
                                arms.down |= i32::from(other_rect.y) == y + 1
                                    || (i32::from(other_rect.y) <= y && other_end_y > y);
                            }
                            _ => continue,
                        }
                        if arms.left != base.left
                            || arms.right != base.right
                            || arms.up != base.up
                            || arms.down != base.down
                        {
                            charset = Some(other_charset);
                        }
                    }
                }

                if let Some(charset) = charset {
                    junctions.push(SplitterJunction {
                        x: x.saturating_add(offset_x),
                        y: y.saturating_add(offset_y),
                        symbol: junction_symbol(arms, charset),
                    });
                }
            }
        }
    }
    junctions
}

fn splitter_symbol_at(render: &SplitterHandleRender<'_>, x: i32, y: i32) -> char {
    render
        .junctions
        .iter()
        .find(|junction| junction.x == x && junction.y == y)
        .map_or(render.symbol, |junction| junction.symbol)
}

#[inline]
fn is_blank_symbol(symbol: &str) -> bool {
    symbol.trim().is_empty()
}

#[inline]
fn is_border_symbol(symbol: &str) -> bool {
    let mut chars = symbol.chars();
    let Some(ch) = chars.next() else {
        return false;
    };
    if chars.next().is_some() {
        return false;
    }

    matches!(ch, '-' | '|' | '+') || ('\u{2500}'..='\u{257F}').contains(&ch)
}

struct SplitterCellDrawCtx<'a> {
    clip: &'a ClipBounds,
    buf_bounds: &'a ClipBounds,
    preserve_existing_symbols: bool,
}

#[inline]
fn draw_splitter_cell(
    buf: &mut ratatui::buffer::Buffer,
    x: i32,
    y: i32,
    symbol: &str,
    style: ratatui::style::Style,
    ctx: SplitterCellDrawCtx<'_>,
) {
    let SplitterCellDrawCtx {
        clip,
        buf_bounds,
        preserve_existing_symbols,
    } = ctx;
    if !clip.contains(x, y) || !buf_bounds.contains(x, y) {
        return;
    }

    let Some(cell) = buf.cell_mut((x as u16, y as u16)) else {
        return;
    };

    if preserve_existing_symbols {
        let existing = cell.symbol();
        if is_blank_symbol(existing) || !is_border_symbol(existing) {
            return;
        }
        cell.set_style(style);
    } else {
        cell.set_symbol(symbol).set_style(style);
    }
}

pub(crate) fn render_splitter_handles(
    f: &mut ratatui::Frame<'_>,
    render: SplitterHandleRender<'_>,
) {
    if render.handle_rects.is_empty() {
        return;
    }

    let buf = f.buffer_mut();
    let clip = render
        .clip_rect
        .map(ClipBounds::from_rect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);

    for (idx, rect) in render.handle_rects.iter().enumerate() {
        if rect.w == 0 || rect.h == 0 {
            continue;
        }

        let is_active = render.active_handle == Some(idx);
        let is_hovered = render.hovered_handle == Some(idx);
        if render.preserve_existing_symbols && !is_active && !is_hovered {
            // In joined mode, keep default border look untouched.
            continue;
        }

        let mut style = render.style;
        if is_active {
            style = style.patch(render.active_style);
        } else if is_hovered {
            style = style.patch(render.hover_style);
        }

        let rstyle = to_ratatui_style(style);
        match render.orientation {
            Orientation::Horizontal => {
                let end_x = rect.x.saturating_add(rect.w as i16).saturating_sub(1);
                for y in
                    rect.y as i32..=(rect.y.saturating_add(rect.h as i16).saturating_sub(1)) as i32
                {
                    for x in rect.x as i32
                        ..=(rect.x.saturating_add(rect.w as i16).saturating_sub(1)) as i32
                    {
                        if render.preserve_existing_symbols
                            && ((x as i16) == rect.x || (x as i16) == end_x)
                        {
                            continue;
                        }
                        let mut symbol_buf = [0; 4];
                        let symbol = splitter_symbol_at(&render, x, y).encode_utf8(&mut symbol_buf);
                        draw_splitter_cell(
                            buf,
                            x,
                            y,
                            symbol,
                            rstyle,
                            SplitterCellDrawCtx {
                                clip: &clip,
                                buf_bounds: &buf_bounds,
                                preserve_existing_symbols: render.preserve_existing_symbols,
                            },
                        );
                    }
                }
            }
            Orientation::Vertical => {
                let end_y = rect.y.saturating_add(rect.h as i16).saturating_sub(1);
                for x in
                    rect.x as i32..=(rect.x.saturating_add(rect.w as i16).saturating_sub(1)) as i32
                {
                    for y in rect.y as i32
                        ..=(rect.y.saturating_add(rect.h as i16).saturating_sub(1)) as i32
                    {
                        if render.preserve_existing_symbols
                            && ((y as i16) == rect.y || (y as i16) == end_y)
                        {
                            continue;
                        }
                        let mut symbol_buf = [0; 4];
                        let symbol = splitter_symbol_at(&render, x, y).encode_utf8(&mut symbol_buf);
                        draw_splitter_cell(
                            buf,
                            x,
                            y,
                            symbol,
                            rstyle,
                            SplitterCellDrawCtx {
                                clip: &clip,
                                buf_bounds: &buf_bounds,
                                preserve_existing_symbols: render.preserve_existing_symbols,
                            },
                        );
                    }
                }
            }
        }
    }
}

pub(crate) fn render_splitter_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    splitter: &crate::widgets::internal::SplitterNode,
    _rect: Rect,
    clip_bounds: Option<Rect>,
) {
    let handle_rects = splitter
        .handle_rects
        .iter()
        .map(|rect| Rect {
            x: rect.x.saturating_add(state.content.x as i16),
            y: rect.y.saturating_add(state.content.y as i16),
            w: rect.w,
            h: rect.h,
        })
        .collect::<Vec<_>>();
    let hovered_handle = (state.ctx.hovered == Some(node_id))
        .then_some(())
        .and(state.ctx.mouse_pos)
        .and_then(|(mx, my)| {
            handle_rects
                .iter()
                .position(|rect| rect.contains(mx as i16, my as i16))
        });
    let junctions = splitter_junctions(
        state.ctx.tree,
        node_id,
        splitter,
        i32::from(state.content.x),
        i32::from(state.content.y),
    );
    render_splitter_handles(
        state.f,
        SplitterHandleRender {
            orientation: splitter.orientation,
            handle_rects: &handle_rects,
            symbol: splitter.handle_symbol,
            style: resolve_base_style(
                state.ctx.tree.node(node_id).active_theme(),
                splitter.handle_style,
            ),
            hover_style: resolve_splitter_hover_style(
                state.ctx.tree.node(node_id).active_theme(),
                splitter.handle_hover_style,
            ),
            active_style: resolve_splitter_active_style(
                state.ctx.tree.node(node_id).active_theme(),
                splitter.handle_active_style,
            ),
            hovered_handle,
            active_handle: splitter.active_handle,
            junctions: &junctions,
            preserve_existing_symbols: splitter.rides_border(),
            clip_rect: clip_bounds,
        },
    );
}
