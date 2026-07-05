use std::borrow::Cow;

use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::backend::ratatui_backend::common::{
    calculate_visible_borders, clear_fg_preserve_bg_clipped, finalize_style,
    remember_cursor_position, resolve_interactive_style_raw, style_backdrop, style_paints_bg,
    style_uses_backdrop_bg, to_ratatui_border_set, to_ratatui_border_type, to_ratatui_rect,
    to_ratatui_style, truncate_end_with_ellipsis,
};
use crate::backend::ratatui_backend::render::{
    RenderState, apply_copy_feedback_to_selection_style,
};
use crate::backend::ratatui_backend::renderers::theme::{
    with_theme_error, with_theme_input_focus, with_theme_muted, with_theme_primary,
};
use crate::core::node::NodeId;
use crate::style::{BorderStyle, Padding, Rect, Style, ThemeRole, resolve_slot};
use crate::utils::text as util;

pub(crate) struct InputRenderCtx<'a> {
    pub placeholder: Option<&'a str>,
    pub prefix: Option<&'a str>,
    pub prefix_style: Style,
    pub focus_prefix_style: Style,
    pub suffix: Option<&'a str>,
    pub suffix_style: Style,
    pub focus_suffix_style: Style,
    pub truncate_head: bool,
    pub style: Style,
    pub focus_content_style: Style,
    pub placeholder_style: Style,
    pub focus_placeholder_style: Style,
    pub selection_style: Style,
    pub border: bool,
    pub border_style: BorderStyle,
    pub hover_border_style: Option<BorderStyle>,
    pub padding: Padding,
    pub mask: Option<char>,
    pub is_focused: bool,
    pub is_hovered: bool,
    pub blink_visible: bool,
    pub disabled: bool,
    pub read_only: bool,
    pub cursor_sink: Option<&'a std::cell::Cell<Option<ratatui::layout::Position>>>,
    pub rrect: ratatui::layout::Rect,
    pub clip_rect: Option<Rect>,
    pub error: Option<&'a str>,
    pub error_style: Style,
    pub reserve_error_row: bool,
}

pub(crate) fn render_input(
    f: &mut ratatui::Frame<'_>,
    value: &str,
    cursor: usize,
    anchor: Option<usize>,
    rect: Rect,
    ctx: InputRenderCtx<'_>,
) {
    let InputRenderCtx {
        placeholder,
        prefix,
        prefix_style,
        focus_prefix_style,
        suffix,
        suffix_style,
        focus_suffix_style,
        truncate_head,
        style,
        focus_content_style,
        placeholder_style,
        focus_placeholder_style,
        selection_style,
        border,
        border_style,
        hover_border_style,
        padding,
        mask,
        is_focused,
        is_hovered,
        blink_visible,
        disabled,
        read_only,
        cursor_sink,
        rrect,
        clip_rect,
        error,
        error_style,
        reserve_error_row,
    } = ctx;
    let mut chrome_style = style;
    let content_style = if is_focused {
        focus_content_style
    } else {
        style
    };
    let ph_style = if is_focused {
        focus_placeholder_style
    } else {
        placeholder_style
    };
    let pre_style = if is_focused {
        focus_prefix_style
    } else {
        prefix_style
    };
    let suf_style = if is_focused {
        focus_suffix_style
    } else {
        suffix_style
    };

    if !disabled
        && error.is_some()
        && let Some(error_fg) = error_style.fg
    {
        chrome_style = chrome_style.patch(Style::new().fg(error_fg));
    }

    let has_error_row = (reserve_error_row || error.is_some()) && rect.h > 0;
    let content_rect = if has_error_row {
        Rect {
            h: rect.h.saturating_sub(1),
            ..rect
        }
    } else {
        rect
    };
    let content_rrect = to_ratatui_rect(content_rect).intersection(rrect);

    let mut inner = content_rect;
    if border {
        let block_style = chrome_style;
        let mut border_type = border_style;
        if !disabled
            && is_hovered
            && let Some(bt) = hover_border_style
        {
            border_type = bt;
        }

        let borders = calculate_visible_borders(content_rect, clip_rect);
        let mut block = Block::default()
            .borders(borders)
            .border_type(to_ratatui_border_type(border_type))
            .style(to_ratatui_style(block_style));

        if let Some(set) = to_ratatui_border_set(border_type) {
            block = block.border_set(set);
        }

        if style_uses_backdrop_bg(block_style) {
            clear_fg_preserve_bg_clipped(f, content_rect, clip_rect);
        } else if style_paints_bg(block_style) {
            f.render_widget(Clear, content_rrect);
        }
        f.render_widget(block, content_rrect);

        // Always reserve space for borders if enabled, even if clipped
        inner.x = inner.x.saturating_add(1);
        inner.w = inner.w.saturating_sub(2);
        inner.y = inner.y.saturating_add(1);
        inner.h = inner.h.saturating_sub(2);
    } else if style_uses_backdrop_bg(chrome_style) {
        clear_fg_preserve_bg_clipped(f, content_rect, clip_rect);
    } else if style_paints_bg(chrome_style) {
        f.render_widget(Clear, content_rrect);
        let bg = Block::default().style(to_ratatui_style(chrome_style));
        f.render_widget(bg, content_rrect);
    }

    inner = inner.inset(padding);
    if inner.w == 0 || inner.h == 0 {
        if let Some(err) = error {
            let error_rect = Rect {
                x: rect.x,
                y: rect.y.saturating_add(rect.h.saturating_sub(1) as i16),
                w: rect.w,
                h: 1,
            };
            let error_rrect = to_ratatui_rect(error_rect).intersection(rrect);
            if error_rrect.width > 0 && error_rrect.height > 0 {
                let err_line = Line::from(Span::styled(
                    truncate_end_with_ellipsis(err, error_rrect.width),
                    to_ratatui_style(error_style),
                ));
                f.render_widget(Paragraph::new(err_line), error_rrect);
            }
        }
        return;
    }

    let inner_rrect = to_ratatui_rect(inner);
    let effective_rrect = inner_rrect.intersection(rrect);

    // Calculate scroll offset to account for clipping
    let dx = (effective_rrect.x as i32)
        .saturating_sub(inner.x as i32)
        .max(0) as u16;
    let dy = (effective_rrect.y as i32)
        .saturating_sub(inner.y as i32)
        .max(0) as u16;

    let prefix = prefix.unwrap_or("");
    let suffix = suffix.unwrap_or("");
    let prefix_w = UnicodeWidthStr::width(prefix) as u16;
    let suffix_w = UnicodeWidthStr::width(suffix) as u16;
    let content_w = inner.w.saturating_sub(prefix_w.saturating_add(suffix_w));

    if content_w == 0 {
        let mut spans = Vec::new();
        if !prefix.is_empty() {
            spans.push(Span::styled(prefix, to_ratatui_style(pre_style)));
        }
        if !suffix.is_empty() {
            spans.push(Span::styled(suffix, to_ratatui_style(suf_style)));
        }
        let line = Line::from(spans);
        let p = Paragraph::new(line).scroll((dy, dx));
        f.render_widget(p, effective_rrect);
        return;
    }

    let line_orig = value.lines().next().unwrap_or("");

    let (line, cursor, anchor) = if let Some(m) = mask {
        let mut masked = String::with_capacity(line_orig.len());
        let mut new_cursor = 0;
        let mut new_anchor = anchor.map(|_| 0);

        for (i, _ch) in line_orig.char_indices() {
            if i < cursor {
                new_cursor += m.len_utf8();
            }
            if let Some(a) = anchor
                && i < a
            {
                new_anchor = Some(new_anchor.unwrap() + m.len_utf8());
            }
            masked.push(m);
        }
        if cursor >= line_orig.len() {
            new_cursor = masked.len();
        }
        if let Some(a) = anchor
            && a >= line_orig.len()
        {
            new_anchor = Some(masked.len());
        }
        (Cow::Owned(masked), new_cursor, new_anchor)
    } else {
        (Cow::Borrowed(line_orig), cursor, anchor)
    };

    let cursor = util::clamp_cursor(&line, cursor.min(line.len()));

    // Compute selection range (in byte indices)
    let selection = anchor.map(|a| {
        let a = util::clamp_cursor(&line, a.min(line.len()));
        (a.min(cursor), a.max(cursor))
    });

    let text_w = UnicodeWidthStr::width(line.as_ref()) as u16;
    let needs_cursor_reserve = is_focused && text_w >= content_w;
    let visible_w = if needs_cursor_reserve {
        content_w.saturating_sub(1)
    } else {
        content_w
    };

    let mut cursor_x = 0u16;
    let mut display_w = visible_w;
    // Track viewport start for selection calculation
    let mut viewport_start: usize = 0;

    if line.is_empty() {
        let mut spans = Vec::new();
        if !prefix.is_empty() {
            spans.push(Span::styled(prefix, to_ratatui_style(pre_style)));
        }

        if let Some(ph) = placeholder {
            let ph_line = ph.lines().next().unwrap_or("");
            let content = truncate_end_with_ellipsis(ph_line, visible_w);
            spans.push(Span::styled(
                content.to_string(),
                to_ratatui_style(ph_style),
            ));
            // Pad to content_w to keep suffix at a consistent position
            let pad = content_w.saturating_sub(UnicodeWidthStr::width(content.as_ref()) as u16);
            if pad > 0 {
                spans.push(Span::styled(
                    " ".repeat(pad as usize),
                    to_ratatui_style(content_style),
                ));
            }
        } else {
            // Pad to content_w to keep suffix at a consistent position
            if content_w > 0 {
                spans.push(Span::styled(
                    " ".repeat(content_w as usize),
                    to_ratatui_style(content_style),
                ));
            }
        }

        if !suffix.is_empty() {
            spans.push(Span::styled(suffix, to_ratatui_style(suf_style)));
        }

        let p = Paragraph::new(Line::from(spans)).scroll((dy, dx));
        f.render_widget(p, effective_rrect);

        if !is_focused {
            if let Some(err) = error {
                let error_rect = Rect {
                    x: rect.x,
                    y: rect.y.saturating_add(rect.h.saturating_sub(1) as i16),
                    w: rect.w,
                    h: 1,
                };
                let err_rrect = to_ratatui_rect(error_rect).intersection(rrect);
                if err_rrect.width > 0 && err_rrect.height > 0 {
                    let err_line = Line::from(Span::styled(
                        truncate_end_with_ellipsis(err, err_rrect.width),
                        to_ratatui_style(error_style),
                    ));
                    f.render_widget(Paragraph::new(err_line), err_rrect);
                }
            }
            return;
        }
    } else {
        let mut content: Cow<'_, str> = Cow::Borrowed("");

        if visible_w > 0 {
            let view_cursor = if is_focused { cursor } else { line.len() };
            if truncate_head {
                display_w = content_w;
                let (start, end, _) = util::viewport(&line, view_cursor, content_w);
                viewport_start = start;
                let left_hidden = start > 0;
                let right_hidden = end < line.len();

                let ellipsis_count = (left_hidden as u16) + (right_hidden as u16);

                if content_w <= ellipsis_count {
                    let mut out = String::new();
                    for _ in 0..content_w {
                        out.push('…');
                    }
                    content = Cow::Owned(out);
                    cursor_x = 0;
                } else {
                    let text_w = content_w.saturating_sub(ellipsis_count);
                    let (s2, e2, c2) = util::viewport(&line, view_cursor, text_w);
                    viewport_start = s2;
                    let mut out = String::new();
                    if left_hidden {
                        out.push('…');
                        cursor_x = c2.saturating_add(1);
                    } else {
                        cursor_x = c2;
                    }
                    out.push_str(&line[s2..e2]);
                    if right_hidden {
                        out.push('…');
                    }
                    content = Cow::Owned(out);
                }
            } else {
                // When truncate_head is disabled:
                // - Don't show ellipsis
                // - Use full content_w for viewport since viewport() already handles cursor visibility
                // - Pad to content_w to keep suffix position consistent
                display_w = content_w;
                let (start, end, cx) = util::viewport(&line, view_cursor, content_w);
                viewport_start = start;
                content = Cow::Borrowed(&line[start..end]);
                cursor_x = cx;
            }
        }

        // Build spans with selection highlighting
        let mut spans = Vec::new();
        if !prefix.is_empty() {
            spans.push(Span::styled(prefix, to_ratatui_style(pre_style)));
        }

        // Handle selection highlighting in the visible content
        if let Some((sel_start, sel_end)) = selection {
            if is_focused && sel_start != sel_end && !truncate_head {
                // We have a selection to render
                // The viewport shows line[viewport_start..viewport_start+content.len()]
                let viewport_end = viewport_start + content.len();

                // Calculate visible selection bounds (in viewport-relative byte indices)
                let vis_sel_start = sel_start.saturating_sub(viewport_start).min(content.len());
                let vis_sel_end = sel_end.saturating_sub(viewport_start).min(content.len());

                if vis_sel_start < vis_sel_end
                    && vis_sel_start < viewport_end.saturating_sub(viewport_start)
                {
                    // Split content into: before selection, selected, after selection
                    let content_str = content.as_ref();
                    let before = &content_str[..vis_sel_start];
                    let selected = &content_str[vis_sel_start..vis_sel_end];
                    let after = &content_str[vis_sel_end..];

                    if !before.is_empty() {
                        spans.push(Span::styled(
                            before.to_string(),
                            to_ratatui_style(content_style),
                        ));
                    }
                    if !selected.is_empty() {
                        spans.push(Span::styled(
                            selected.to_string(),
                            to_ratatui_style(selection_style),
                        ));
                    }
                    if !after.is_empty() {
                        spans.push(Span::styled(
                            after.to_string(),
                            to_ratatui_style(content_style),
                        ));
                    }
                } else {
                    spans.push(Span::styled(
                        content.to_string(),
                        to_ratatui_style(content_style),
                    ));
                }
            } else {
                spans.push(Span::styled(
                    content.to_string(),
                    to_ratatui_style(content_style),
                ));
            }
        } else {
            spans.push(Span::styled(
                content.to_string(),
                to_ratatui_style(content_style),
            ));
        }

        let content_width = UnicodeWidthStr::width(content.as_ref()) as u16;
        let pad = display_w.saturating_sub(content_width);
        if pad > 0 {
            spans.push(Span::styled(
                " ".repeat(pad as usize),
                to_ratatui_style(content_style),
            ));
        }

        if !suffix.is_empty() {
            spans.push(Span::styled(suffix, to_ratatui_style(suf_style)));
        }

        let p = Paragraph::new(Line::from(spans)).scroll((dy, dx));
        f.render_widget(p, effective_rrect);
    }

    // Render cursor (only if no selection or at selection edge)
    if is_focused && !disabled && !read_only {
        let cx = inner
            .x
            .saturating_add(prefix_w as i16)
            .saturating_add(cursor_x as i16);
        let cy = inner.y;

        // Only show cursor if there's no selection (or blink is visible)
        let has_selection = selection.map(|(s, e)| s != e).unwrap_or(false);

        if blink_visible
            && !has_selection
            && cx >= effective_rrect.x as i16
            && cx < (effective_rrect.x as i32 + effective_rrect.width as i32) as i16
            && cy >= effective_rrect.y as i16
            && cy < (effective_rrect.y as i32 + effective_rrect.height as i32) as i16
        {
            let position = ratatui::layout::Position::new(cx.max(0) as u16, cy.max(0) as u16);
            f.set_cursor_position(position);
            remember_cursor_position(cursor_sink, position);
        }
    }

    // Render error message below the input
    if let Some(err) = error {
        let error_rect = Rect {
            x: rect.x,
            y: rect.y.saturating_add(rect.h.saturating_sub(1) as i16),
            w: rect.w,
            h: 1,
        };
        let err_rrect = to_ratatui_rect(error_rect).intersection(rrect);
        if err_rrect.width > 0 && err_rrect.height > 0 {
            let err_line = Line::from(Span::styled(
                truncate_end_with_ellipsis(err, err_rrect.width),
                to_ratatui_style(error_style),
            ));
            f.render_widget(Paragraph::new(err_line), err_rrect);
        }
    }
}

pub(crate) fn render_input_node(
    state: &mut RenderState<'_, '_, '_>,
    node_id: NodeId,
    node: &crate::widgets::internal::InputNode,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    clip_bounds: Option<Rect>,
) {
    let is_focused = Some(node_id) == state.ctx.focused && !node.disabled;
    let is_hovered = Some(node_id) == state.ctx.hovered && !node.disabled;
    let contrast_policy = state.ctx.contrast_policy;
    let theme = state.ctx.tree.node(node_id).active_theme();
    let focus_style = resolve_slot(theme, ThemeRole::Focus, &node.focus_style);
    let hover_style = resolve_slot(theme, ThemeRole::Hover, &node.hover_style);
    let selection_style = apply_copy_feedback_to_selection_style(
        state.ctx,
        node_id,
        resolve_slot(theme, ThemeRole::TextSelection, &node.selection_style),
    );
    let style = with_theme_primary(theme, node.style);
    let disabled_style = with_theme_muted(theme, node.disabled_style);
    let base_style = finalize_style(
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
    let (effective_cursor, effective_anchor) = if node.read_only && node.on_change.is_none() {
        if let Some(sel) = state.ctx.read_only_selection.and_then(|s| s.get(&node_id)) {
            (sel.0, sel.1)
        } else {
            (node.cursor, node.anchor)
        }
    } else {
        (node.cursor, node.anchor)
    };

    render_input(
        state.f,
        &node.value,
        effective_cursor,
        effective_anchor,
        rect,
        InputRenderCtx {
            placeholder: node.placeholder.as_deref(),
            prefix: node.prefix.as_deref(),
            prefix_style: finalize_style(
                base_style.patch(with_theme_muted(theme, node.prefix_style)),
                style_backdrop(base_style),
                contrast_policy,
            ),
            focus_prefix_style: finalize_style(
                base_style.patch(with_theme_primary(theme, node.focus_prefix_style)),
                style_backdrop(base_style),
                contrast_policy,
            ),
            suffix: node.suffix.as_deref(),
            suffix_style: finalize_style(
                base_style.patch(with_theme_muted(theme, node.suffix_style)),
                style_backdrop(base_style),
                contrast_policy,
            ),
            focus_suffix_style: finalize_style(
                base_style.patch(with_theme_primary(theme, node.focus_suffix_style)),
                style_backdrop(base_style),
                contrast_policy,
            ),
            truncate_head: node.truncate_head,
            style: base_style,
            focus_content_style: finalize_style(
                base_style.patch(with_theme_input_focus(theme, node.focus_content_style)),
                style_backdrop(base_style),
                contrast_policy,
            ),
            placeholder_style: finalize_style(
                base_style.patch(with_theme_muted(theme, node.placeholder_style)),
                style_backdrop(base_style),
                contrast_policy,
            ),
            focus_placeholder_style: finalize_style(
                base_style.patch(with_theme_muted(theme, node.focus_placeholder_style)),
                style_backdrop(base_style),
                contrast_policy,
            ),
            selection_style: finalize_style(
                base_style.patch(selection_style),
                style_backdrop(base_style),
                contrast_policy,
            ),
            border: node.border,
            border_style: node.border_style,
            hover_border_style: node.hover_border_style,
            padding: node.padding,
            mask: node.mask,
            is_focused,
            is_hovered,
            blink_visible: state.ctx.blink_visible,
            disabled: node.disabled,
            read_only: node.read_only,
            cursor_sink: Some(state.ctx.cursor_position),
            rrect,
            clip_rect: clip_bounds,
            error: node.error.as_deref(),
            error_style: finalize_style(
                with_theme_error(theme, node.error_style),
                None,
                contrast_policy,
            ),
            reserve_error_row: node.reserve_error_row,
        },
    );
}

#[cfg(test)]
mod tests {
    use ratatui::backend::TestBackend;
    use ratatui::style::Modifier;
    use ratatui::{Terminal, TerminalOptions, Viewport};

    use super::{InputRenderCtx, render_input};
    use crate::style::{BorderStyle, Padding, Rect, Style};

    fn test_input_ctx<'a>(
        rect: Rect,
        focus_content_style: Style,
        border: bool,
        is_focused: bool,
        placeholder: Option<&'a str>,
        error: Option<&'a str>,
        reserve_error_row: bool,
    ) -> InputRenderCtx<'a> {
        InputRenderCtx {
            placeholder,
            prefix: None,
            prefix_style: Style::default(),
            focus_prefix_style: Style::default(),
            suffix: None,
            suffix_style: Style::default(),
            focus_suffix_style: Style::default(),
            truncate_head: false,
            style: Style::default(),
            focus_content_style,
            placeholder_style: Style::default(),
            focus_placeholder_style: Style::default(),
            selection_style: Style::default(),
            border,
            border_style: BorderStyle::Plain,
            hover_border_style: None,
            padding: Padding::default(),
            mask: None,
            is_focused,
            is_hovered: false,
            blink_visible: true,
            disabled: false,
            read_only: false,
            cursor_sink: None,
            rrect: ratatui::layout::Rect::new(0, 0, rect.w, rect.h),
            clip_rect: None,
            error,
            error_style: Style::default(),
            reserve_error_row,
        }
    }

    #[test]
    fn focus_style_does_not_bold_input_content() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 1,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_input(
                    f,
                    "hello",
                    0,
                    None,
                    rect,
                    test_input_ctx(rect, Style::default(), false, true, None, None, false),
                );
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert!(!buffer[(0, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn bordered_focus_does_not_auto_bold_input_chrome() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 3,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_input(
                    f,
                    "hello",
                    0,
                    None,
                    rect,
                    test_input_ctx(
                        rect,
                        Style::new().fg(crate::style::Color::Cyan),
                        true,
                        true,
                        None,
                        None,
                        false,
                    ),
                );
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert!(!buffer[(0, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn focus_content_style_can_bold_input_content_explicitly() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 8,
            h: 1,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_input(
                    f,
                    "hello",
                    0,
                    None,
                    rect,
                    test_input_ctx(rect, Style::new().bold(), false, true, None, None, false),
                );
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert!(buffer[(0, 0)].modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn bordered_input_reserves_error_row_below_border() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 4,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_input(
                    f,
                    "hi",
                    0,
                    None,
                    rect,
                    test_input_ctx(
                        rect,
                        Style::default(),
                        true,
                        false,
                        None,
                        Some("required"),
                        false,
                    ),
                );
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 2)].symbol(), "└");
        assert_eq!(buffer[(11, 2)].symbol(), "┘");

        let error_row = (0..rect.w)
            .map(|x| buffer[(x, 3)].symbol())
            .collect::<String>();
        assert!(error_row.starts_with("required"));
    }

    #[test]
    fn reserved_error_row_keeps_layout_stable_without_message() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 12,
            h: 4,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_input(
                    f,
                    "hi",
                    0,
                    None,
                    rect,
                    test_input_ctx(rect, Style::default(), true, false, None, None, true),
                );
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 2)].symbol(), "└");
        assert_eq!(buffer[(11, 2)].symbol(), "┘");
        let bottom_row = (0..rect.w)
            .map(|x| buffer[(x, 3)].symbol())
            .collect::<String>();
        assert!(bottom_row.trim().is_empty());
    }

    #[test]
    fn empty_unfocused_input_still_renders_error_text() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 18,
            h: 4,
        };
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::with_options(
            backend,
            TerminalOptions {
                viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, rect.w, rect.h)),
            },
        )
        .expect("terminal");

        terminal
            .draw(|f| {
                render_input(
                    f,
                    "",
                    0,
                    None,
                    rect,
                    test_input_ctx(
                        rect,
                        Style::default(),
                        true,
                        false,
                        Some("placeholder"),
                        Some("required"),
                        true,
                    ),
                );
            })
            .expect("draw");

        let buffer = terminal.backend().buffer();
        let error_row = (0..rect.w)
            .map(|x| buffer[(x, 3)].symbol())
            .collect::<String>();
        assert!(error_row.starts_with("required"));
    }
}
