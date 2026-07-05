use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};

use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::common::{
    calculate_visible_borders, finalize_style, resolve_interactive_style_raw, style_backdrop,
    style_paints_bg, to_ratatui_border_set, to_ratatui_border_type, to_ratatui_rect,
    to_ratatui_style,
};
use crate::backend::ratatui_backend::render::RenderState;
use crate::core::node::NodeId;
use crate::style::resolve::{
    resolve_base_style, resolve_hex_pending_edit_style, resolve_muted_style, resolve_style_defaults,
};
use crate::style::{Rect, Theme, ThemeRole, resolve_slot};
use crate::widgets::internal::HexAreaNode;
use crate::widgets::{HexAreaPointerHitArgs, pointer_hit};

pub(crate) struct HexAreaRenderCtx<'a> {
    pub is_focused: bool,
    pub is_hovered: bool,
    pub mouse_pos: Option<(u16, u16)>,
    pub contrast_policy: ContrastPolicy,
    pub rect: Rect,
    pub rrect: ratatui::layout::Rect,
    pub clip_rect: Option<Rect>,
    pub theme: &'a Theme,
}

#[derive(Clone, Copy)]
struct HexRowFormatCtx {
    base_style: crate::style::Style,
    selection_style: crate::style::Style,
    cursor_style: crate::style::Style,
    selection: Option<(usize, usize)>,
    cursor: usize,
    is_focused: bool,
    hovered_index: Option<usize>,
    hover_style: crate::style::Style,
    pending_index: Option<usize>,
    pending_edit_style: crate::style::Style,
}

#[derive(Clone, Copy)]
struct HexByteStyleCtx {
    selection_style: crate::style::Style,
    cursor_style: crate::style::Style,
    selection: Option<(usize, usize)>,
    cursor: usize,
    is_focused: bool,
    hovered_index: Option<usize>,
    hover_style: crate::style::Style,
    pending_index: Option<usize>,
    pending_edit_style: crate::style::Style,
}

pub(crate) fn render_hex_area(
    f: &mut ratatui::Frame<'_>,
    node: &HexAreaNode,
    ctx: HexAreaRenderCtx<'_>,
) {
    let HexAreaRenderCtx {
        is_focused,
        is_hovered,
        mouse_pos,
        contrast_policy,
        rect,
        rrect,
        clip_rect,
        theme,
    } = ctx;
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let style = resolve_base_style(theme, node.style);
    let hover_style = resolve_slot(theme, ThemeRole::Hover, &node.hover_style);
    let focus_style = resolve_slot(theme, ThemeRole::Focus, &node.focus_style);
    let focus_content_style =
        resolve_style_defaults(node.focus_content_style, theme.hex_area.focus);
    let disabled_style = resolve_muted_style(theme, node.disabled_style);
    let selection_style = resolve_slot(theme, ThemeRole::TextSelection, &node.selection_style);
    let cursor_style = resolve_style_defaults(node.cursor_style, theme.hex_area.cursor);
    let pending_edit_style = resolve_hex_pending_edit_style(theme, node.pending_edit_style);

    let chrome_style = finalize_style(
        resolve_interactive_style_raw(
            style,
            focus_style,
            hover_style,
            disabled_style,
            is_focused,
            is_hovered,
            node.disabled,
        ),
        None,
        contrast_policy,
    );
    let content_style = finalize_style(
        resolve_interactive_style_raw(
            style,
            focus_content_style,
            hover_style,
            disabled_style,
            is_focused,
            is_hovered,
            node.disabled,
        ),
        None,
        contrast_policy,
    );

    let mut inner = rect;
    if node.border {
        let borders = calculate_visible_borders(rect, clip_rect);
        let mut block = Block::default()
            .borders(borders)
            .border_type(to_ratatui_border_type(node.border_style))
            .style(to_ratatui_style(chrome_style));
        if let Some(set) = to_ratatui_border_set(node.border_style) {
            block = block.border_set(set);
        }
        if style_paints_bg(chrome_style) {
            f.render_widget(Clear, rrect);
        }
        f.render_widget(block, rrect);
        inner.x = inner.x.saturating_add(1);
        inner.y = inner.y.saturating_add(1);
        inner.w = inner.w.saturating_sub(2);
        inner.h = inner.h.saturating_sub(2);
    } else if style_paints_bg(chrome_style) {
        f.render_widget(Clear, rrect);
        let bg = Block::default().style(to_ratatui_style(chrome_style));
        f.render_widget(bg, rrect);
    }

    inner = inner.inset(node.padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    let bytes_per_row = node.bytes_per_row.max(1) as usize;
    let bytes = &node.bytes;
    let len = bytes.len();
    let total_rows = len.div_ceil(bytes_per_row).max(1);
    let visible_rows = inner.h as usize;

    let clamped_cursor = if len == 0 {
        0
    } else {
        node.cursor.min(len.saturating_sub(1))
    };
    let selection = selection_range(clamped_cursor, node.anchor, len);
    let cursor_style = if node.read_only {
        cursor_style.patch(crate::style::Style::new().dim())
    } else {
        cursor_style
    };

    let start_row = node.scroll_offset.map_or_else(
        || {
            if visible_rows == 0 {
                0
            } else {
                let cursor_row = if len == 0 {
                    0
                } else {
                    clamped_cursor / bytes_per_row
                };
                cursor_row.saturating_sub(visible_rows.saturating_sub(1))
            }
        },
        |offset| offset.min(total_rows.saturating_sub(1)),
    );
    let end_row = start_row.saturating_add(visible_rows).min(total_rows);
    let hovered_index = if is_hovered {
        mouse_pos.and_then(|(x, y)| {
            pointer_hit(
                rect,
                HexAreaPointerHitArgs {
                    bytes_len: len,
                    cursor: clamped_cursor,
                    bytes_per_row: node.bytes_per_row,
                    show_offsets: node.show_offsets,
                    show_ascii: node.show_ascii,
                    scroll_offset: node.scroll_offset,
                    border: node.border,
                    padding: node.padding,
                },
                x,
                y,
            )
            .map(|hit| hit.index)
        })
    } else {
        None
    };
    let pending_index = if is_focused && node.pending_high_nibble.is_some() {
        Some(clamped_cursor)
    } else {
        None
    };

    let row_ctx = HexRowFormatCtx {
        base_style: content_style,
        selection_style: finalize_style(
            content_style.patch(selection_style),
            style_backdrop(content_style),
            contrast_policy,
        ),
        cursor_style: finalize_style(
            content_style.patch(cursor_style),
            style_backdrop(content_style),
            contrast_policy,
        ),
        selection,
        cursor: clamped_cursor,
        is_focused,
        hovered_index,
        hover_style: finalize_style(
            content_style.patch(hover_style),
            style_backdrop(content_style),
            contrast_policy,
        ),
        pending_index,
        pending_edit_style: finalize_style(
            content_style.patch(pending_edit_style),
            style_backdrop(content_style),
            contrast_policy,
        ),
    };

    let mut lines: Vec<Line<'static>> = Vec::with_capacity(end_row.saturating_sub(start_row));
    for row in start_row..end_row {
        lines.push(format_row(
            HexFormatRowArgs {
                row,
                bytes,
                bytes_per_row,
                show_offsets: node.show_offsets,
                show_ascii: node.show_ascii,
                uppercase_hex: node.uppercase_hex,
                pending_high_nibble: node.pending_high_nibble,
            },
            row_ctx,
        ));
    }

    let inner_rrect = to_ratatui_rect(inner);
    let effective_rrect = if let Some(clip) = clip_rect {
        inner_rrect.intersection(to_ratatui_rect(clip))
    } else {
        inner_rrect
    };

    if effective_rrect.is_empty() {
        return;
    }

    let dy = (effective_rrect.y as i32)
        .saturating_sub(inner.y as i32)
        .max(0) as u16;
    let dx = (effective_rrect.x as i32)
        .saturating_sub(inner.x as i32)
        .max(0) as u16;

    let paragraph = Paragraph::new(lines).scroll((dy, dx));
    f.render_widget(paragraph, effective_rrect);
}

struct HexFormatRowArgs<'a> {
    row: usize,
    bytes: &'a [u8],
    bytes_per_row: usize,
    show_offsets: bool,
    show_ascii: bool,
    uppercase_hex: bool,
    pending_high_nibble: Option<u8>,
}

fn format_row(args: HexFormatRowArgs<'_>, ctx: HexRowFormatCtx) -> Line<'static> {
    let HexFormatRowArgs {
        row,
        bytes,
        bytes_per_row,
        show_offsets,
        show_ascii,
        uppercase_hex,
        pending_high_nibble,
    } = args;
    let HexRowFormatCtx {
        base_style,
        selection_style,
        cursor_style,
        selection,
        cursor,
        is_focused,
        hovered_index,
        hover_style,
        pending_index,
        pending_edit_style,
    } = ctx;
    let byte_ctx = HexByteStyleCtx {
        selection_style,
        cursor_style,
        selection,
        cursor,
        is_focused,
        hovered_index,
        hover_style,
        pending_index,
        pending_edit_style,
    };
    let mut spans: Vec<Span<'static>> = Vec::new();
    let row_start = row.saturating_mul(bytes_per_row);

    if show_offsets {
        spans.push(Span::styled(
            format!("{row_start:08X}: "),
            to_ratatui_style(base_style),
        ));
    }

    for col in 0..bytes_per_row {
        let index = row_start.saturating_add(col);
        let style = byte_style(base_style, index, byte_ctx);
        if let Some(byte) = bytes.get(index) {
            let hex = if pending_high_nibble.is_some() && is_focused && index == cursor {
                let hi = pending_high_nibble.unwrap_or_default();
                if uppercase_hex {
                    format!("{:X} ", hi)
                } else {
                    format!("{:x} ", hi)
                }
            } else if uppercase_hex {
                format!("{byte:02X}")
            } else {
                format!("{byte:02x}")
            };
            spans.push(Span::styled(hex, to_ratatui_style(style)));
        } else {
            spans.push(Span::styled("  ", to_ratatui_style(base_style)));
        }
        if col + 1 < bytes_per_row {
            spans.push(Span::styled(" ", to_ratatui_style(base_style)));
        }
    }

    if show_ascii {
        spans.push(Span::styled("  ", to_ratatui_style(base_style)));
        for col in 0..bytes_per_row {
            let index = row_start.saturating_add(col);
            let style = byte_style(base_style, index, byte_ctx);
            let glyph = bytes
                .get(index)
                .copied()
                .map(printable_ascii)
                .unwrap_or(' ')
                .to_string();
            spans.push(Span::styled(glyph, to_ratatui_style(style)));
        }
    }

    Line::from(spans)
}

fn printable_ascii(byte: u8) -> char {
    if (0x20..=0x7e).contains(&byte) {
        byte as char
    } else {
        '.'
    }
}

fn selection_range(cursor: usize, anchor: Option<usize>, len: usize) -> Option<(usize, usize)> {
    if len == 0 {
        return None;
    }
    anchor.map(|anchor| {
        let a = anchor.min(len.saturating_sub(1));
        (a.min(cursor), a.max(cursor))
    })
}

fn byte_style(
    base_style: crate::style::Style,
    index: usize,
    ctx: HexByteStyleCtx,
) -> crate::style::Style {
    let HexByteStyleCtx {
        selection_style,
        cursor_style,
        selection,
        cursor,
        is_focused,
        hovered_index,
        hover_style,
        pending_index,
        pending_edit_style,
    } = ctx;
    let mut style = base_style;
    if let Some((start, end)) = selection
        && index >= start
        && index <= end
    {
        style = style.patch(selection_style);
    }
    if is_focused && index == cursor {
        style = style.patch(cursor_style);
    }
    if hovered_index == Some(index) {
        style = style.patch(hover_style);
    }
    if pending_index == Some(index) {
        style = style.patch(pending_edit_style);
    }
    style
}

pub(crate) fn render_hex_area_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::widgets::internal::HexAreaNode,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) {
    let is_focused = Some(node_id) == state.ctx.focused && !node.disabled;
    let is_hovered = Some(node_id) == state.ctx.hovered && !node.disabled;
    let contrast_policy = state.ctx.contrast_policy;

    render_hex_area(
        state.f,
        node,
        HexAreaRenderCtx {
            is_focused,
            is_hovered,
            mouse_pos: state.ctx.mouse_pos,
            contrast_policy,
            rect,
            rrect,
            clip_rect: clip_bounds,
            theme: state.ctx.tree.node(node_id).active_theme(),
        },
    );
}
