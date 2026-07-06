use crate::backend::ratatui_backend::common::{ClipBounds, to_ratatui_style};
use crate::backend::ratatui_backend::render::RenderState;
use crate::core::node::NodeId;
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
    pub preserve_existing_symbols: bool,
    pub clip_rect: Option<Rect>,
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
        let ch = render.symbol.to_string();

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
                        draw_splitter_cell(
                            buf,
                            x,
                            y,
                            &ch,
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
                        draw_splitter_cell(
                            buf,
                            x,
                            y,
                            &ch,
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
            preserve_existing_symbols: splitter.rides_border(),
            clip_rect: clip_bounds,
        },
    );
}
