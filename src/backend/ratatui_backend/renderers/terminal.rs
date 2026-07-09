use std::cell::RefCell;

use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthChar;

use crate::backend::ratatui_backend::common::{
    DEFAULT_SCROLLBAR_THUMB, IntegratedScrollbarAppearance, ScrollbarAppearance,
    ScrollbarScrollState, calculate_visible_borders, integrated_vscrollbar_track_char,
    is_cursor_visible, remember_cursor_position, render_integrated_scrollbar, render_vscrollbar,
    resolve_scrollbar_thumb_style, style_paints_bg, to_ratatui_border_set, to_ratatui_border_type,
    to_ratatui_rect, to_ratatui_span, to_ratatui_style,
};
use crate::backend::ratatui_backend::render::{
    FrameIntegratedVTrack, RenderState, ancestor_frame_integrated_vtrack,
    apply_copy_feedback_to_selection_style,
};
use crate::core::node::NodeId;
use crate::style::resolve::{resolve_base_style, resolve_style_defaults};
use crate::style::{Rect, ScrollbarVariant, Span, Style, Theme, ThemeRole, resolve_slot};
use crate::utils::GridSelection;
use crate::utils::scrollbar::ScrollbarMetricsCache;
use crate::widgets::internal::terminal_content_layout;

fn clip_spans_no_ellipsis<'a>(
    spans: Vec<ratatui::text::Span<'a>>,
    max_width: u16,
) -> Vec<ratatui::text::Span<'a>> {
    if max_width == 0 {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut used = 0u16;

    for span in spans {
        if used >= max_width {
            break;
        }

        let mut chunk = String::new();
        for ch in span.content.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0) as u16;
            if w == 0 {
                chunk.push(ch);
                continue;
            }
            if used.saturating_add(w) > max_width {
                break;
            }
            chunk.push(ch);
            used = used.saturating_add(w);
        }

        if !chunk.is_empty() {
            out.push(ratatui::text::Span::styled(chunk, span.style));
        }
    }

    out
}

pub(crate) struct TerminalRenderCtx<'a> {
    pub is_focused: bool,
    pub is_hovered: bool,
    pub blink_visible: bool,
    pub clip_rect: Option<Rect>,
    pub cursor_sink: Option<&'a std::cell::Cell<Option<ratatui::layout::Position>>>,
    pub parent_integrated_v: Option<FrameIntegratedVTrack>,
    pub metrics_cache: Option<&'a RefCell<ScrollbarMetricsCache>>,
    pub node_theme: &'a Theme,
    pub selection_style: Style,
}

pub(crate) fn render_terminal(
    f: &mut ratatui::Frame<'_>,
    node: &crate::widgets::internal::TerminalNode,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    ctx: TerminalRenderCtx<'_>,
) {
    let TerminalRenderCtx {
        is_focused,
        is_hovered,
        blink_visible,
        clip_rect,
        cursor_sink,
        parent_integrated_v,
        metrics_cache,
        node_theme,
        selection_style,
    } = ctx;
    let theme = node_theme;
    let style = resolve_base_style(theme, node.style);
    let hover_style = resolve_slot(theme, ThemeRole::Hover, &node.hover_style);
    let focus_style = resolve_slot(theme, ThemeRole::Focus, &node.focus_style);
    let focus_content_style =
        resolve_style_defaults(node.focus_content_style, theme.terminal.focus);

    let mut content_style = style;
    let mut chrome_style = style;
    if is_hovered {
        content_style = content_style.patch(hover_style);
        chrome_style = chrome_style.patch(hover_style);
    }
    if is_focused {
        chrome_style = chrome_style.patch(focus_style);
        content_style = content_style.patch(focus_content_style);
    }

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

        if borders.contains(Borders::LEFT) {
            inner.x = inner.x.saturating_add(1);
            inner.w = inner.w.saturating_sub(1);
        }
        if borders.contains(Borders::RIGHT) {
            inner.w = inner.w.saturating_sub(1);
        }
        if borders.contains(Borders::TOP) {
            inner.y = inner.y.saturating_add(1);
            inner.h = inner.h.saturating_sub(1);
        }
        if borders.contains(Borders::BOTTOM) {
            inner.h = inner.h.saturating_sub(1);
        }
    } else if style_paints_bg(chrome_style) {
        f.render_widget(Clear, rrect);
        f.render_widget(
            Block::default().style(to_ratatui_style(chrome_style)),
            rrect,
        );
    }

    inner = inner.inset(node.padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    let layout = terminal_content_layout(
        inner,
        node.border,
        node.scrollbar,
        node.scrollbar_variant,
        node.scrollbar_gap,
        node.total_scrollback_rows,
        parent_integrated_v.is_some(),
    );
    let content_rect = layout.content_rect;
    let scrollbar = layout.scrollbar_visible;
    let use_integrated = layout.scrollbar_integrated;
    let content_w = content_rect.w;

    let content_rrect = to_ratatui_rect(content_rect);
    let effective = content_rrect.intersection(rrect);

    let mut rendered_content = false;
    if content_rect.w > 0 && content_rect.h > 0 && effective.width > 0 && effective.height > 0 {
        let dx = (effective.x as i32)
            .saturating_sub(content_rect.x as i32)
            .max(0) as u16;
        let dy = (effective.y as i32)
            .saturating_sub(content_rect.y as i32)
            .max(0) as u16;

        let lines: Vec<Line<'_>> = (0..content_rect.h as usize)
            .map(|row| {
                let mut spans: Vec<ratatui::text::Span<'_>> = node
                    .lines
                    .get(row)
                    .map(|line| {
                        apply_selection_to_row(
                            line,
                            row,
                            &node.selection,
                            selection_style,
                            content_style,
                        )
                    })
                    .unwrap_or_default();

                if spans.is_empty() {
                    spans.push(ratatui::text::Span::styled(
                        "",
                        to_ratatui_style(content_style),
                    ));
                }

                Line::from(clip_spans_no_ellipsis(spans, content_w))
            })
            .collect();

        f.render_widget(Paragraph::new(lines).scroll((dy, dx)), effective);
        rendered_content = true;
    }

    // Render vertical scrollbar when scrollback is available.
    if scrollbar {
        let total = node.viewport_rows + node.total_scrollback_rows;
        let visible = node.viewport_rows;

        // Map vt100 scrollback (0 = bottom) to standard offset (0 = top).
        let std_offset = node
            .total_scrollback_rows
            .saturating_sub(node.scrollback_offset);

        let thumb = node.scrollbar_thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB);
        let mut thumb_style = resolve_scrollbar_thumb_style(
            is_focused,
            node.scrollbar_thumb_style,
            node.scrollbar_thumb_focus_style,
        );
        if !is_focused && node.scrollbar_thumb_style.is_none() {
            thumb_style = thumb_style.dim();
        }
        let track_style = node.scrollbar_track_style;

        if use_integrated {
            let sb_x = parent_integrated_v
                .map(|p| p.track_x)
                .unwrap_or_else(|| rect.x.saturating_add(rect.w.saturating_sub(1) as i16));
            let sb_rect = Rect {
                x: sb_x,
                y: inner.y,
                w: 1,
                h: inner.h,
            };

            let b_style = parent_integrated_v
                .map(|p| p.border_style_fallback)
                .unwrap_or(node.border_style);
            let track_glyph = parent_integrated_v.and_then(|p| p.track_glyph);
            let mut track_scratch = [0u8; 4];
            let border_char =
                integrated_vscrollbar_track_char(track_glyph, b_style, &mut track_scratch);
            let integrated_base_style = parent_integrated_v
                .map(|p| p.track_style)
                .unwrap_or(selection_style);

            render_integrated_scrollbar(
                f,
                sb_rect,
                ScrollbarScrollState {
                    offset: std_offset,
                    visible,
                    total,
                },
                IntegratedScrollbarAppearance {
                    thumb_char: thumb,
                    border_char,
                    base_style: integrated_base_style,
                    thumb_style,
                    track_style,
                    clip_rect: None,
                    metrics_cache,
                },
            );
        } else {
            let sb_rect = Rect {
                x: inner
                    .x
                    .saturating_add(inner.w.saturating_sub(1) as i16)
                    .max(0),
                y: inner.y,
                w: 1,
                h: inner.h,
            };

            let clip_rrect = clip_rect.map(to_ratatui_rect);
            render_vscrollbar(
                f,
                sb_rect,
                ScrollbarScrollState {
                    offset: std_offset,
                    visible,
                    total,
                },
                ScrollbarAppearance {
                    thumb_char: thumb,
                    thumb_style,
                    track_style,
                    clip_rect: clip_rrect,
                    metrics_cache,
                },
            );
        }
    }

    // Use the real hardware cursor (like Input/TextArea) instead of a
    // software REVERSED-cell cursor. TerminalManager applies the child's
    // DECSCUSR shape as a steady hardware cursor; the framework blink timer
    // drives blinking here, but only when the child asked for a blinking
    // cursor. A steady cursor stays lit so the child's request is honored.
    //
    // Only show cursor when at live view (scrollback_offset == 0).
    // Hide cursor when there's an active selection.
    let has_selection = node.selection.as_ref().is_some_and(|sel| !sel.is_empty());
    let cursor_lit = !node.cursor_blinking || blink_visible;
    if rendered_content
        && is_focused
        && node.cursor_visible
        && cursor_lit
        && node.scrollback_offset == 0
        && !has_selection
    {
        let cursor_x = content_rect.x.saturating_add(node.cursor_col as i16);
        let cursor_y = content_rect.y.saturating_add(node.cursor_row as i16);
        if is_cursor_visible(cursor_x, cursor_y, content_rect, clip_rect) {
            let position = ratatui::layout::Position::new(cursor_x as u16, cursor_y as u16);
            f.set_cursor_position(position);
            remember_cursor_position(cursor_sink, position);
        }
    }
}

fn apply_selection_to_row<'a>(
    spans: &'a [Span],
    row: usize,
    selection: &Option<GridSelection>,
    selection_style: Style,
    base_style: Style,
) -> Vec<ratatui::text::Span<'a>> {
    let Some(sel) = selection else {
        return spans
            .iter()
            .map(|span| to_ratatui_span(span, base_style))
            .collect();
    };

    if sel.is_empty() {
        return spans
            .iter()
            .map(|span| to_ratatui_span(span, base_style))
            .collect();
    }

    let line_width: usize = spans
        .iter()
        .map(|span| {
            span.content
                .chars()
                .map(|ch| UnicodeWidthChar::width(ch).unwrap_or(0))
                .sum::<usize>()
        })
        .sum();

    let Some((col_start, col_end)) = sel.columns_for_row(row, line_width) else {
        return spans
            .iter()
            .map(|span| to_ratatui_span(span, base_style))
            .collect();
    };

    apply_column_selection_to_spans(spans, col_start, col_end, selection_style, base_style)
}

fn apply_column_selection_to_spans(
    spans: &[Span],
    col_start: usize,
    col_end: usize,
    selection_style: Style,
    base_style: Style,
) -> Vec<ratatui::text::Span<'_>> {
    if col_start >= col_end {
        return spans
            .iter()
            .map(|span| to_ratatui_span(span, base_style))
            .collect();
    }

    let mut out = Vec::new();
    let mut col = 0usize;
    let mut run_text = String::new();
    let mut run_style: Option<ratatui::style::Style> = None;

    for span in spans {
        let span_style = base_style.patch(span.style);
        for ch in span.content.chars() {
            let w = UnicodeWidthChar::width(ch).unwrap_or(0);
            let selected = if w == 0 {
                col >= col_start && col < col_end
            } else {
                let cell_end = col.saturating_add(w);
                col < col_end && cell_end > col_start
            };
            let style = if selected {
                to_ratatui_style(span_style.patch(selection_style))
            } else {
                to_ratatui_style(span_style)
            };

            if run_style == Some(style) {
                run_text.push(ch);
            } else {
                if let Some(run_style) = run_style.take()
                    && !run_text.is_empty()
                {
                    out.push(ratatui::text::Span::styled(
                        std::mem::take(&mut run_text),
                        run_style,
                    ));
                }
                run_style = Some(style);
                run_text.push(ch);
            }

            if w > 0 {
                col = col.saturating_add(w);
            }
        }
    }

    if let Some(run_style) = run_style
        && !run_text.is_empty()
    {
        out.push(ratatui::text::Span::styled(run_text, run_style));
    }

    out
}

#[cfg(feature = "terminal")]
pub(crate) fn render_terminal_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::core::node::Node,
    term: &crate::widgets::internal::TerminalNode,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) {
    let is_focused = Some(node_id) == state.ctx.focused;
    let is_hovered = Some(node_id) == state.ctx.hovered;

    let parent_integrated_v = if !term.border
        && term.scrollbar
        && matches!(term.scrollbar_variant, ScrollbarVariant::Integrated)
    {
        ancestor_frame_integrated_vtrack(state, node.parent)
    } else {
        None
    };

    let scrollbar_cache = &state.ctx.scrollbar_metrics_cache;
    let theme = node.active_theme();
    let selection_style = apply_copy_feedback_to_selection_style(
        state.ctx,
        node_id,
        resolve_slot(theme, ThemeRole::TextSelection, &term.selection_style),
    );
    let f = &mut *state.f;
    render_terminal(
        f,
        term,
        rect,
        rrect,
        TerminalRenderCtx {
            is_focused,
            is_hovered,
            blink_visible: state.ctx.blink_visible,
            clip_rect: clip_bounds,
            cursor_sink: Some(state.ctx.cursor_position),
            parent_integrated_v,
            metrics_cache: Some(scrollbar_cache),
            node_theme: theme,
            selection_style,
        },
    );
}

pub(crate) fn terminal_cursor_position(
    node: &crate::widgets::internal::TerminalNode,
    rect: Rect,
    clip_rect: Option<Rect>,
    parent_integrated_v_edge: bool,
) -> Option<ratatui::layout::Position> {
    let inner = rect.inner(node.border, node.padding);
    if inner.w == 0 || inner.h == 0 {
        return None;
    }

    let layout = terminal_content_layout(
        inner,
        node.border,
        node.scrollbar,
        node.scrollbar_variant,
        node.scrollbar_gap,
        node.total_scrollback_rows,
        parent_integrated_v_edge,
    );
    let content_rect = layout.content_rect;
    let cursor_x = content_rect.x.saturating_add(node.cursor_col as i16);
    let cursor_y = content_rect.y.saturating_add(node.cursor_row as i16);
    if !is_cursor_visible(cursor_x, cursor_y, content_rect, clip_rect) {
        return None;
    }

    Some(ratatui::layout::Position::new(
        cursor_x as u16,
        cursor_y as u16,
    ))
}
