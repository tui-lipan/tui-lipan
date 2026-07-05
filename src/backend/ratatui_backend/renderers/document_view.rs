//! Renderer for [`DocumentView`](crate::widgets::DocumentView).

use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;
use unicode_width::UnicodeWidthStr;

use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::common::{
    DEFAULT_SCROLLBAR_THUMB, IntegratedScrollbarAppearance, InteractiveStyleState,
    ScrollbarAppearance, ScrollbarScrollState, border_horizontal_char, border_vertical_char,
    calculate_visible_borders, fill_rect_clipped_style, render_hscrollbar,
    render_integrated_hscrollbar, render_integrated_scrollbar, render_vscrollbar,
    resolve_interactive_style, resolve_scrollbar_thumb_style, style_paints_bg,
    to_ratatui_border_set, to_ratatui_border_type, to_ratatui_rect, to_ratatui_style,
};
use crate::style::resolve::{resolve_base_style, resolve_scrollbar_theme, resolve_style_defaults};
use crate::style::{Rect, ScrollbarVariant, Style, Theme, ThemeRole, resolve_slot};
use crate::widgets::ColumnAlign;
use crate::widgets::DocumentLineNumberMode;
use crate::widgets::document_view::node::{
    CODE_BLOCK_LEFT_INSET_COLS, DocumentTableRectSelection, DocumentViewNode, DocumentVisualLine,
    TableBorderKind, VisualLineKind, span_width, visual_line_render_prefix_bytes,
};
use crate::widgets::table::{TableBorderLineKind, table_border_glyphs, table_border_line};

#[derive(Clone)]
struct StyledSegment<'a> {
    text: Cow<'a, str>,
    style: Style,
    start: usize,
    end: usize,
}

pub(crate) struct DocumentViewRenderCtx<'a> {
    pub rect: Rect,
    pub clip_bounds: Option<Rect>,
    pub is_focused: bool,
    pub is_hovered: bool,
    pub scrollbar_focus_override: bool,
    pub theme: &'a Theme,
    pub copy_feedback_style: Option<Style>,
    #[cfg(feature = "diff-view")]
    pub hover_mouse_pos: Option<(u16, u16)>,
}

/// Render a `DocumentView` node to the terminal frame.
pub(crate) fn render_document_view(
    f: &mut ratatui::Frame,
    node: &DocumentViewNode,
    ctx: DocumentViewRenderCtx<'_>,
) {
    let DocumentViewRenderCtx {
        rect,
        clip_bounds,
        is_focused,
        is_hovered,
        scrollbar_focus_override,
        theme,
        copy_feedback_style,
        #[cfg(feature = "diff-view")]
        hover_mouse_pos,
    } = ctx;
    if rect.w == 0 || rect.h == 0 {
        return;
    }

    let clip = clip_bounds.unwrap_or(rect);
    let visible_rect = rect.intersection(&clip);
    if visible_rect.w == 0 || visible_rect.h == 0 {
        return;
    }

    let style = resolve_base_style(theme, node.style);
    let hover_style = resolve_slot(theme, ThemeRole::Hover, &node.hover_style);
    let focus_style = resolve_slot(theme, ThemeRole::Focus, &node.focus_style);
    let focus_content_style =
        resolve_style_defaults(node.focus_content_style, theme.document_view.focus);
    let selection_style = resolve_slot(theme, ThemeRole::TextSelection, &node.selection_style);
    let selection_style = copy_feedback_style
        .map(|flash| selection_style.patch(flash))
        .unwrap_or(selection_style);
    let (scrollbar_thumb_style, scrollbar_thumb_focus_style, scrollbar_track_style) =
        resolve_scrollbar_theme(
            theme,
            node.scrollbar_thumb_style,
            node.scrollbar_thumb_focus_style,
            node.scrollbar_track_style,
        );

    let chrome_style = resolve_interactive_style(
        style,
        focus_style,
        hover_style,
        Style::default(),
        InteractiveStyleState {
            is_focused,
            is_hovered,
            is_disabled: false,
            policy: ContrastPolicy::Off,
        },
    );
    let content_style = resolve_interactive_style(
        style,
        focus_content_style,
        hover_style,
        Style::default(),
        InteractiveStyleState {
            is_focused,
            is_hovered,
            is_disabled: false,
            policy: ContrastPolicy::Off,
        },
    );

    // ── Background ──────────────────────────────────────────────────────
    if style_paints_bg(chrome_style) {
        fill_rect_clipped_style(
            f,
            visible_rect,
            Style {
                bg: chrome_style.bg,
                ..Style::default()
            },
            Some(clip),
            None,
        );
    }

    // ── Border rendering ────────────────────────────────────────────────
    let border = node.border;
    let border_style = node.border_style;
    if border {
        let mut border_type = border_style;
        if is_hovered && let Some(bt) = node.hover_border_style {
            border_type = bt;
        }
        let borders = calculate_visible_borders(rect, clip_bounds);
        let mut block = ratatui::widgets::Block::default()
            .borders(borders)
            .border_type(to_ratatui_border_type(border_type))
            .style(to_ratatui_style(chrome_style));
        if let Some(set) = to_ratatui_border_set(border_type) {
            block = block.border_set(set);
        }
        block.render(to_ratatui_rect(rect), f.buffer_mut());
    }

    // ── Inner rect (after border + padding) ─────────────────────────────
    let inner = rect.inner(border, node.padding);
    if inner.w == 0 || inner.h == 0 {
        return;
    }

    let gutter_base = node.gutter_base_width();
    let gutter = node.gutter_width();

    // ── Scrollbar space calculation ─────────────────────────────────────
    let is_v_integrated = matches!(node.scrollbar_variant, ScrollbarVariant::Integrated) && border;
    let scrollbar_cols: u16 = if node.scrollbar && !is_v_integrated {
        1
    } else {
        0
    };

    let content_w = inner
        .w
        .saturating_sub(gutter)
        .saturating_sub(scrollbar_cols);
    let h_scrollbar_visible =
        node.h_scrollbar && !node.wrap && (node.max_line_width as usize) > (content_w as usize);
    let is_h_integrated =
        matches!(node.h_scrollbar_variant, ScrollbarVariant::Integrated) && border;

    let mut viewport_h = inner.h as usize;
    if h_scrollbar_visible && !is_h_integrated {
        viewport_h = viewport_h.saturating_sub(1);
    }

    let content_x = inner.x.saturating_add(gutter as i16);
    let content_bg_x = content_x;

    // ── Selection range ─────────────────────────────────────────────────
    let linear_selection_range = node
        .selection_anchor
        .map(|anchor| {
            if anchor <= node.selection_cursor {
                (anchor, node.selection_cursor)
            } else {
                (node.selection_cursor, anchor)
            }
        })
        .filter(|(s, e)| s != e);

    let offset = node.scroll_offset;
    let h_offset = node.h_scroll_offset;
    let vis_lines = &node.visual_cache.lines;
    let clip_rrect = to_ratatui_rect(clip);

    // ── Content lines ───────────────────────────────────────────────────
    for row in 0..viewport_h {
        let vi = offset + row;
        if vi >= vis_lines.len() {
            break;
        }
        let y = inner.y.saturating_add(row as i16);
        if y < clip.y || y >= clip.y.saturating_add(clip.h as i16) {
            continue;
        }

        let vline = &vis_lines[vi];

        // Gutter (custom or line numbers)
        if let Some(custom_gutter) = node.gutter_lines.as_deref() {
            // Custom gutter: render per-source-line spans; empty for continuation lines
            if gutter > 0 {
                let gutter_area = ratatui::layout::Rect::new(
                    inner.x.max(0) as u16,
                    y.max(0) as u16,
                    gutter.min(inner.w),
                    1,
                );
                let is_continuation = matches!(
                    vline.kind,
                    VisualLineKind::Text {
                        continuation: true,
                        ..
                    }
                );
                let is_synthetic_padding_line = matches!(
                    vline.kind,
                    VisualLineKind::Text {
                        continuation: true,
                        ref spans,
                        ..
                    } if spans.is_empty()
                );
                let spans: Vec<Span<'static>> = if !is_continuation {
                    let idx = vline.source_line;
                    custom_gutter
                        .get(idx)
                        .map(|line_spans| gutter_spans_with_inset(line_spans, node.gutter_gap))
                        .unwrap_or_default()
                } else {
                    let idx = vline.source_line;
                    custom_gutter
                        .get(idx)
                        .map(|line_spans| {
                            blank_gutter_spans_with_inset(
                                line_spans,
                                gutter,
                                node.gutter_gap,
                                if is_synthetic_padding_line {
                                    node.split_wrap_padding_gutter_style
                                } else {
                                    None
                                },
                            )
                        })
                        .unwrap_or_default()
                };
                ratatui::widgets::Paragraph::new(Line::from(spans))
                    .render(gutter_area, f.buffer_mut());
            }
        } else if node.line_numbers && gutter > 0 {
            let (source_num, is_continuation) = match node.line_number_mode {
                DocumentLineNumberMode::Visual => (vi + 1, false),
                DocumentLineNumberMode::Source => (
                    vline.source_line + 1,
                    is_source_mode_continuation(vis_lines, vi, offset),
                ),
            };
            let gutter_text = if is_continuation {
                " ".repeat(gutter as usize)
            } else {
                built_in_line_number_gutter_text(
                    source_num,
                    gutter_base,
                    node.gutter_gap,
                    node.line_number_separator,
                    node.line_number_content_gap,
                )
            };
            let gutter_style = to_ratatui_style(built_in_line_number_style(
                content_style,
                node.line_number_style,
            ));
            let gutter_span = Span::styled(gutter_text, gutter_style);
            let gutter_area = ratatui::layout::Rect::new(
                inner.x.max(0) as u16,
                y.max(0) as u16,
                gutter.min(inner.w),
                1,
            );
            ratatui::widgets::Paragraph::new(Line::from(vec![gutter_span]))
                .render(gutter_area, f.buffer_mut());
        }

        // Content
        let avail_w = content_w;
        let content_area =
            ratatui::layout::Rect::new(content_x.max(0) as u16, y.max(0) as u16, avail_w, 1);
        let content_bg_area =
            ratatui::layout::Rect::new(content_bg_x.max(0) as u16, y.max(0) as u16, avail_w, 1);
        if content_area.width == 0 {
            continue;
        }

        #[cfg(feature = "diff-view")]
        let context_separator_hover_style = hover_mouse_pos.and_then(|(mx, my)| {
            let mx = mx as i16;
            let my = my as i16;
            let right = content_bg_x.saturating_add(avail_w as i16);
            if my == y && mx >= content_bg_x && mx < right {
                diff_context_separator_hover_style(
                    node.diff_context_separator_click.as_ref(),
                    vline.source_line,
                )
            } else {
                None
            }
        });
        #[cfg(not(feature = "diff-view"))]
        let context_separator_hover_style: Option<Style> = None;

        let line_start = node.visual_cache.line_starts.get(vi).copied().unwrap_or(0);
        let line_len = node.visual_cache.line_lengths.get(vi).copied().unwrap_or(0);

        let mut segments = line_segments(
            vline,
            content_style,
            &node.doc_styles,
            content_area.width as usize,
        );

        let mut row_bg = if node.highlight_full_width {
            full_row_bg_style(&segments)
                .or_else(|| split_wrap_padding_row_bg_style(vline, node.split_wrap_padding_style))
        } else {
            None
        };

        if let Some(hover_style) = context_separator_hover_style {
            segments = patch_segments(segments, hover_style);
            row_bg = row_bg_from_hover_style(hover_style).or(row_bg);
        }

        // Apply horizontal scroll to segments
        let segments = if h_offset > 0 && !node.wrap {
            h_scroll_segments(segments, h_offset)
        } else {
            segments
        };

        if let Some(row_bg) = row_bg {
            ratatui::widgets::Block::default()
                .style(to_ratatui_style(row_bg))
                .render(content_bg_area, f.buffer_mut());
        }

        let selection_ranges = if let Some(table_sel) = &node.table_rect_selection {
            table_selection_ranges_for_line(vline, line_start, table_sel)
        } else if let Some(range) = linear_selection_range {
            vec![range]
        } else {
            Vec::new()
        };

        let render_prefix_len = visual_line_render_prefix_bytes(vline);
        let mut spans = segments_to_spans(
            segments,
            line_start,
            line_len,
            &selection_ranges,
            selection_style,
            render_prefix_len,
        );

        // Mirror TextArea: for empty lines within a linear selection that extends past
        // this line, append a highlighted space to represent the selected newline.
        // Skip excluded source lines (e.g. diff filler lines).
        let line_selection_excluded = node
            .copy_excluded_source_lines
            .as_deref()
            .is_some_and(|excl| excl.contains(&vline.source_line));
        if spans.is_empty()
            && line_len == 0
            && !line_selection_excluded
            && node.table_rect_selection.is_none()
            && let Some((sel_start, sel_end)) = linear_selection_range
            && sel_start <= line_start
            && sel_end > line_start
        {
            spans.push(Span::styled(
                " ",
                to_ratatui_style(content_style.patch(selection_style)),
            ));
        }

        let effective_content_area = content_area.intersection(clip_rrect);
        if effective_content_area.width == 0 || effective_content_area.height == 0 {
            continue;
        }
        if effective_content_area == content_area {
            render_single_line_spans(f.buffer_mut(), effective_content_area, &spans);
        } else {
            let skip_cols = effective_content_area.x.saturating_sub(content_area.x);
            let spans = crop_spans_for_viewport(spans, skip_cols, effective_content_area.width);
            render_single_line_spans(f.buffer_mut(), effective_content_area, &spans);
        }
    }

    // ── Vertical scrollbar ──────────────────────────────────────────────
    // When scrollbar_focus_override is set (single-scrollbar DiffView with
    // sibling pane focused), use the focus thumb style for the scrollbar.
    let effective_thumb_style = if scrollbar_focus_override {
        scrollbar_thumb_focus_style.or(scrollbar_thumb_style)
    } else {
        scrollbar_thumb_style
    };

    let v_scrollbar_needed = node.scrollbar && node.total_visual_lines > viewport_h;
    if v_scrollbar_needed {
        let thumb = node.scrollbar_thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB);
        let thumb_style = resolve_scrollbar_thumb_style(
            is_focused,
            effective_thumb_style,
            scrollbar_thumb_focus_style,
        );
        let track_style = scrollbar_track_style;

        if is_v_integrated {
            // Integrated: render on the right border column
            let sb_rect = Rect {
                x: rect.x.saturating_add(rect.w.saturating_sub(1) as i16),
                y: inner.y,
                w: 1,
                h: viewport_h as u16,
            };
            let border_char = border_vertical_char(border_style);
            render_integrated_scrollbar(
                f,
                sb_rect,
                ScrollbarScrollState {
                    offset,
                    visible: viewport_h,
                    total: node.total_visual_lines,
                },
                IntegratedScrollbarAppearance {
                    thumb_char: thumb,
                    border_char,
                    base_style: chrome_style,
                    thumb_style,
                    track_style,
                    clip_rect: Some(clip_rrect),
                    metrics_cache: None,
                },
            );
        } else {
            // Standalone: render in its own column
            let sb_rect = Rect {
                x: inner.x.saturating_add(inner.w.saturating_sub(1) as i16),
                y: inner.y,
                w: 1,
                h: viewport_h as u16,
            };
            render_vscrollbar(
                f,
                sb_rect,
                ScrollbarScrollState {
                    offset,
                    visible: viewport_h,
                    total: node.total_visual_lines,
                },
                ScrollbarAppearance {
                    thumb_char: thumb,
                    thumb_style,
                    track_style,
                    clip_rect: Some(clip_rrect),
                    metrics_cache: None,
                },
            );
        }
    }

    // ── Horizontal scrollbar ────────────────────────────────────────────
    if h_scrollbar_visible && content_w > 0 {
        let total_cols = node.max_line_width as usize;
        let visible_cols = content_w as usize;
        let h_thumb = node.h_scrollbar_thumb.unwrap_or(DEFAULT_SCROLLBAR_THUMB);
        let h_thumb_style = resolve_scrollbar_thumb_style(
            is_focused,
            effective_thumb_style,
            scrollbar_thumb_focus_style,
        );
        let h_track_style = scrollbar_track_style;

        if is_h_integrated {
            // Integrated: render on the bottom border row
            let sb_rect = Rect {
                x: content_x,
                y: rect.y.saturating_add(rect.h.saturating_sub(1) as i16),
                w: content_w,
                h: 1,
            };
            let border_char = border_horizontal_char(border_style);
            render_integrated_hscrollbar(
                f,
                sb_rect,
                ScrollbarScrollState {
                    offset: h_offset,
                    visible: visible_cols,
                    total: total_cols,
                },
                IntegratedScrollbarAppearance {
                    thumb_char: h_thumb,
                    border_char,
                    base_style: chrome_style,
                    thumb_style: h_thumb_style,
                    track_style: h_track_style,
                    clip_rect: Some(clip_rrect),
                    metrics_cache: None,
                },
            );
        } else {
            // Standalone: render in its own row below content
            let sb_rect = Rect {
                x: content_x,
                y: inner.y.saturating_add(viewport_h as i16),
                w: content_w,
                h: 1,
            };
            render_hscrollbar(
                f,
                sb_rect,
                ScrollbarScrollState {
                    offset: h_offset,
                    visible: visible_cols,
                    total: total_cols,
                },
                ScrollbarAppearance {
                    thumb_char: h_thumb,
                    thumb_style: h_thumb_style,
                    track_style: h_track_style,
                    clip_rect: Some(clip_rrect),
                    metrics_cache: None,
                },
            );
        }
    }
}

fn render_single_line_spans(buf: &mut Buffer, area: ratatui::layout::Rect, spans: &[Span<'_>]) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let mut x = area.x;
    let max_x = area.x.saturating_add(area.width);

    for span in spans {
        if x >= max_x {
            break;
        }
        if span.content.is_empty() {
            continue;
        }

        let remaining = max_x.saturating_sub(x);
        let (next_x, _) = buf.set_span(x, area.y, span, remaining);
        if next_x == x {
            if UnicodeWidthStr::width(span.content.as_ref()) == 0 {
                continue;
            }
            break;
        }
        x = next_x;
    }
}

fn is_source_mode_continuation(
    vis_lines: &[DocumentVisualLine],
    visual_idx: usize,
    first_visible_idx: usize,
) -> bool {
    visual_idx > first_visible_idx
        && vis_lines
            .get(visual_idx - 1)
            .zip(vis_lines.get(visual_idx))
            .is_some_and(|(prev, curr)| prev.source_line == curr.source_line)
}

fn crop_spans_for_viewport<'a>(
    spans: Vec<Span<'a>>,
    skip_cols: u16,
    max_width: u16,
) -> Vec<Span<'a>> {
    if max_width == 0 {
        return Vec::new();
    }

    let mut remaining_skip = skip_cols as usize;
    let mut remaining_width = max_width as usize;
    let mut out = Vec::new();

    for span in spans {
        if remaining_width == 0 {
            break;
        }

        let content = span.content.as_ref();
        let content_width = UnicodeWidthStr::width(content);

        if remaining_skip >= content_width {
            remaining_skip -= content_width;
            continue;
        }

        let start = if remaining_skip > 0 {
            let byte = crate::utils::text::byte_at_col(content, remaining_skip);
            remaining_skip = 0;
            byte
        } else {
            0
        };

        let available = crate::utils::text::end_at_width(&content[start..], 0, remaining_width);
        let slice = &content[start..start + available];
        if !slice.is_empty() {
            remaining_width = remaining_width.saturating_sub(UnicodeWidthStr::width(slice));
            out.push(Span::styled(slice.to_string(), span.style));
        }
    }

    out
}
fn full_row_bg_style(segments: &[(Cow<'_, str>, Style)]) -> Option<Style> {
    if let Some(bg) = segments.iter().find_map(|(text, style)| {
        (text.is_empty() && style_paints_bg(*style))
            .then_some(style.bg)
            .flatten()
    }) {
        return Some(Style {
            bg: Some(bg),
            ..Style::default()
        });
    }

    let mut dominant: Option<(crate::style::Paint, usize)> = None;
    for (text, style) in segments {
        let Some(bg) = style.bg else {
            continue;
        };
        let w = unicode_width::UnicodeWidthStr::width(text.as_ref());
        if w == 0 {
            continue;
        }
        match dominant {
            Some((_, dw)) if dw >= w => {}
            _ => dominant = Some((bg, w)),
        }
    }

    let bg = dominant.map(|(bg, _)| bg)?;
    Some(Style {
        bg: Some(bg),
        ..Style::default()
    })
}

#[cfg(feature = "diff-view")]
fn diff_context_separator_hover_style(
    config: Option<&crate::widgets::DiffContextSeparatorClickConfig>,
    source_line: usize,
) -> Option<Style> {
    let config = config?;
    config.events_by_source_line.get(source_line)?.as_ref()?;
    let style = config.hover_style?;
    (!style.is_empty()).then_some(style)
}

fn row_bg_from_hover_style(style: Style) -> Option<Style> {
    style.bg.map(|bg| Style {
        bg: Some(bg),
        ..Style::default()
    })
}

fn patch_segments<'a>(
    segments: Vec<(Cow<'a, str>, Style)>,
    overlay: Style,
) -> Vec<(Cow<'a, str>, Style)> {
    segments
        .into_iter()
        .map(|(text, style)| (text, style.patch(overlay)))
        .collect()
}

fn is_split_wrap_padding_line(vline: &DocumentVisualLine) -> bool {
    matches!(
        vline.kind,
        VisualLineKind::Text {
            continuation: true,
            ref spans,
            ..
        } if spans.is_empty()
    )
}

fn split_wrap_padding_row_bg_style(
    vline: &DocumentVisualLine,
    padding_style: Option<Style>,
) -> Option<Style> {
    if !is_split_wrap_padding_line(vline) {
        return None;
    }
    let style = padding_style?;
    style_paints_bg(style).then_some(Style {
        bg: style.bg,
        ..Style::default()
    })
}

fn gutter_spans_with_inset(spans: &[crate::style::Span], inset: u16) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    if inset > 0 {
        let inset_style = spans
            .iter()
            .find(|span| !span.content.is_empty())
            .or_else(|| spans.first())
            .map(|span| span.style)
            .unwrap_or_default();
        out.push(Span::styled(
            " ".repeat(inset as usize),
            to_ratatui_style(inset_style),
        ));
    }
    out.extend(
        spans
            .iter()
            .map(|s| Span::styled(s.content.as_ref().to_owned(), to_ratatui_style(s.style))),
    );
    out
}

fn built_in_line_number_gutter_text(
    source_num: usize,
    gutter_base: u16,
    gutter_gap: u16,
    show_separator: bool,
    line_number_content_gap: u16,
) -> String {
    let separator_cols = if show_separator { 2 } else { 0 };
    let digits = gutter_base
        .saturating_sub(separator_cols)
        .saturating_sub(line_number_content_gap);
    let right_gap = " ".repeat(line_number_content_gap as usize);
    if show_separator {
        format!(
            "{}{:>width$} │{}",
            " ".repeat(gutter_gap as usize),
            source_num,
            right_gap,
            width = digits as usize
        )
    } else {
        format!(
            "{}{:>width$}{}",
            " ".repeat(gutter_gap as usize),
            source_num,
            right_gap,
            width = digits as usize
        )
    }
}

fn built_in_line_number_style(content_style: Style, line_number_style: Style) -> Style {
    Style {
        fg: content_style.fg,
        dim: Some(true),
        ..Style::default()
    }
    .patch(line_number_style)
}

fn blank_gutter_spans_with_inset(
    spans: &[crate::style::Span],
    gutter_width: u16,
    inset: u16,
    override_style: Option<Style>,
) -> Vec<Span<'static>> {
    use unicode_width::UnicodeWidthStr;

    let mut out = Vec::new();
    let mut painted_width = 0usize;
    let fill_style = override_style.unwrap_or_else(|| {
        spans
            .iter()
            .rev()
            .find(|span| !span.content.is_empty())
            .or_else(|| spans.first())
            .map(|span| span.style)
            .unwrap_or_default()
    });

    if inset > 0 {
        let inset_style = override_style.unwrap_or_else(|| {
            spans
                .iter()
                .find(|span| !span.content.is_empty())
                .or_else(|| spans.first())
                .map(|span| span.style)
                .unwrap_or(fill_style)
        });
        out.push(Span::styled(
            " ".repeat(inset as usize),
            to_ratatui_style(inset_style),
        ));
        painted_width = painted_width.saturating_add(inset as usize);
    }

    for span in spans {
        let span_width = UnicodeWidthStr::width(span.content.as_ref());
        if span_width == 0 {
            continue;
        }
        let span_style = override_style.unwrap_or(span.style);
        out.push(Span::styled(
            " ".repeat(span_width),
            to_ratatui_style(span_style),
        ));
        painted_width = painted_width.saturating_add(span_width);
    }

    let remaining = (gutter_width as usize).saturating_sub(painted_width);
    if remaining > 0 {
        out.push(Span::styled(
            " ".repeat(remaining),
            to_ratatui_style(fill_style),
        ));
    }

    out
}

/// Apply horizontal scroll offset to line segments by skipping `h_offset` display columns.
fn h_scroll_segments<'a>(
    segments: Vec<(Cow<'a, str>, Style)>,
    h_offset: usize,
) -> Vec<(Cow<'a, str>, Style)> {
    let mut remaining = h_offset;
    let mut result = Vec::new();
    for (text, style) in segments {
        if remaining == 0 {
            result.push((text, style));
            continue;
        }
        let w = unicode_width::UnicodeWidthStr::width(text.as_ref());
        if w <= remaining {
            remaining -= w;
            continue;
        }
        // Partial skip within this segment
        let byte_pos = crate::utils::text::byte_at_col(text.as_ref(), remaining);
        remaining = 0;
        if byte_pos < text.len() {
            result.push((Cow::Owned(text[byte_pos..].to_string()), style));
        }
    }
    result
}

fn line_segments<'a>(
    vline: &'a DocumentVisualLine,
    base_style: Style,
    doc_styles: &crate::widgets::document_view::DocumentStyles,
    rule_width: usize,
) -> Vec<(Cow<'a, str>, Style)> {
    match &vline.kind {
        VisualLineKind::Text {
            spans, indent_cols, ..
        } => {
            let mut segs = Vec::new();
            if *indent_cols > 0 {
                let indent_text = " ".repeat(*indent_cols as usize);
                segs.push((Cow::Owned(indent_text), base_style));
            }
            for span in spans {
                segs.push((
                    Cow::Borrowed(span.content.as_ref()),
                    base_style.patch(span.style),
                ));
            }
            segs
        }
        VisualLineKind::TableRow {
            cells,
            alignments,
            widths,
            border_variant,
            outer_frame,
            column_separators,
            cell_padding,
            ..
        } => {
            let border_style = base_style.patch(doc_styles.table_border_style);
            let glyphs = table_border_glyphs(*border_variant);
            let mut segs = Vec::new();
            if *outer_frame {
                segs.push((Cow::Owned(glyphs.left.to_string()), border_style));
            }
            for (i, (cell, &col_w)) in cells.iter().zip(widths.iter()).enumerate() {
                let align = alignments.get(i).copied().unwrap_or(ColumnAlign::Left);
                if *cell_padding > 0 {
                    segs.push((Cow::Owned(" ".repeat(*cell_padding as usize)), base_style));
                }
                let content_col_w = col_w.saturating_sub(cell_padding.saturating_mul(2)) as usize;
                let content_w = span_width(cell) as usize;
                let pad = content_col_w.saturating_sub(content_w);
                let (lpad, rpad) = match align {
                    ColumnAlign::Left => (0, pad),
                    ColumnAlign::Right => (pad, 0),
                    ColumnAlign::Center => (pad / 2, pad - pad / 2),
                };
                if lpad > 0 {
                    segs.push((Cow::Owned(" ".repeat(lpad)), base_style));
                }
                for span in cell {
                    segs.push((
                        Cow::Borrowed(span.content.as_ref()),
                        base_style.patch(span.style),
                    ));
                }
                if rpad > 0 {
                    segs.push((Cow::Owned(" ".repeat(rpad)), base_style));
                }
                if *cell_padding > 0 {
                    segs.push((Cow::Owned(" ".repeat(*cell_padding as usize)), base_style));
                }
                let has_next = i + 1 < widths.len();
                if has_next && *column_separators {
                    segs.push((Cow::Owned(glyphs.center.to_string()), border_style));
                }
            }
            if *outer_frame {
                segs.push((Cow::Owned(glyphs.right.to_string()), border_style));
            }
            segs
        }
        VisualLineKind::TableBorder {
            kind,
            widths,
            border_variant,
            outer_frame,
            column_separators,
        } => {
            let border_style = base_style.patch(doc_styles.table_border_style);
            let glyphs = table_border_glyphs(*border_variant);
            let kind = match kind {
                TableBorderKind::Top => TableBorderLineKind::Top,
                TableBorderKind::Mid => TableBorderLineKind::Mid,
                TableBorderKind::Bottom => TableBorderLineKind::Bottom,
            };
            let text = table_border_line(kind, widths, glyphs, *outer_frame, *column_separators);
            vec![(Cow::Owned(text), border_style)]
        }
        VisualLineKind::HorizontalRule => {
            vec![(
                Cow::Owned("─".repeat(rule_width.max(1))),
                base_style.patch(doc_styles.hr_style),
            )]
        }
        VisualLineKind::DiagramRow { spans, .. } => spans
            .iter()
            .map(|span| {
                (
                    Cow::Borrowed(span.content.as_ref()),
                    base_style.patch(span.style),
                )
            })
            .collect(),
        VisualLineKind::CodeLine { spans, block_style } => {
            let effective = base_style.patch(*block_style);
            let mut segs = vec![(
                Cow::Owned(" ".repeat(CODE_BLOCK_LEFT_INSET_COLS as usize)),
                effective,
            )];
            for span in spans {
                segs.push((
                    Cow::Borrowed(span.content.as_ref()),
                    effective.patch(span.style),
                ));
            }
            segs
        }
        VisualLineKind::BlockQuoteLine { spans, depth, .. } => {
            let bar_style = base_style.patch(doc_styles.blockquote_bar_style);
            let mut segs = Vec::new();
            for _ in 0..*depth {
                segs.push((Cow::Borrowed("│ "), bar_style));
            }
            for span in spans {
                segs.push((
                    Cow::Borrowed(span.content.as_ref()),
                    base_style.patch(span.style),
                ));
            }
            segs
        }
    }
}

fn table_row_local_text_byte_ranges(
    cell_line_texts: &[std::sync::Arc<str>],
    widths: &[u16],
    alignments: &[ColumnAlign],
    cell_padding: u16,
    outer_frame: bool,
    column_separators: bool,
) -> Vec<Option<(usize, usize)>> {
    let mut out = Vec::with_capacity(widths.len());
    let mut cursor = 0usize;

    if outer_frame {
        cursor = cursor.saturating_add('│'.len_utf8());
    }

    for (i, &w) in widths.iter().enumerate() {
        cursor = cursor.saturating_add(cell_padding as usize);

        let content_col_w = (w as usize).saturating_sub(cell_padding.saturating_mul(2) as usize);
        let text = cell_line_texts.get(i).map(|s| s.as_ref()).unwrap_or("");
        let text_w = unicode_width::UnicodeWidthStr::width(text);
        let pad = content_col_w.saturating_sub(text_w);
        let align = alignments.get(i).copied().unwrap_or(ColumnAlign::Left);
        let (lpad, rpad) = match align {
            ColumnAlign::Left => (0, pad),
            ColumnAlign::Right => (pad, 0),
            ColumnAlign::Center => (pad / 2, pad - pad / 2),
        };

        cursor = cursor.saturating_add(lpad);
        let start = cursor;
        cursor = cursor.saturating_add(text.len());
        let end = cursor;
        cursor = cursor.saturating_add(rpad);

        cursor = cursor.saturating_add(cell_padding as usize);

        if i + 1 < widths.len() && column_separators {
            cursor = cursor.saturating_add('│'.len_utf8());
        }

        if start < end {
            out.push(Some((start, end)));
        } else {
            out.push(None);
        }
    }

    if outer_frame {
        cursor = cursor.saturating_add('│'.len_utf8());
    }
    let _ = cursor;

    out
}

fn table_selection_ranges_for_line(
    vline: &DocumentVisualLine,
    line_start: usize,
    selection: &DocumentTableRectSelection,
) -> Vec<(usize, usize)> {
    let VisualLineKind::TableRow {
        table_id,
        row_index,
        row_line_index,
        cell_line_texts,
        alignments,
        widths,
        outer_frame,
        column_separators,
        cell_padding,
        ..
    } = &vline.kind
    else {
        return Vec::new();
    };

    if *table_id != selection.table_id
        || *row_index < selection.row_start
        || *row_index > selection.row_end
    {
        return Vec::new();
    }

    let locals = table_row_local_text_byte_ranges(
        cell_line_texts,
        widths,
        alignments,
        *cell_padding,
        *outer_frame,
        *column_separators,
    );
    let cursor_cell_use_suffix = selection.cursor_col_index < selection.anchor_col_index;

    let mut ranges = Vec::new();
    for col in selection.col_start..=selection.col_end {
        if let Some(Some((start, end))) = locals.get(col) {
            let base_start = *start;
            let mut local_start = base_start;
            let mut local_end = *end;

            if *row_index == selection.cursor_row_index && col == selection.cursor_col_index {
                if *row_line_index > selection.cursor_row_line_index {
                    continue;
                }
                if *row_line_index == selection.cursor_row_line_index
                    && selection.cursor_cell_line_anchor_byte != usize::MAX
                {
                    let text_len = cell_line_texts
                        .get(col)
                        .map(|s| s.len())
                        .unwrap_or(0)
                        .min(selection.cursor_cell_line_anchor_byte);
                    if cursor_cell_use_suffix {
                        local_start = base_start.saturating_add(text_len);
                    } else {
                        local_end = base_start.saturating_add(text_len);
                    }
                    if local_end < local_start {
                        local_end = local_start;
                    }
                }
            }

            if selection.anchor_row_index == selection.cursor_row_index
                && selection.anchor_col_index == selection.cursor_col_index
                && *row_index == selection.anchor_row_index
                && col == selection.anchor_col_index
                && *row_line_index == selection.anchor_row_line_index
                && selection.anchor_row_line_index == selection.cursor_row_line_index
            {
                let cell_len = cell_line_texts.get(col).map(|s| s.len()).unwrap_or(0);
                if selection.anchor_cell_line_anchor_byte == usize::MAX
                    && selection.cursor_cell_line_anchor_byte == usize::MAX
                {
                    local_start = base_start;
                    local_end = base_start.saturating_add(cell_len);
                } else {
                    let a = selection.anchor_cell_line_anchor_byte.min(cell_len);
                    let b = selection.cursor_cell_line_anchor_byte.min(cell_len);
                    let (a, b) = if a <= b { (a, b) } else { (b, a) };
                    local_start = base_start.saturating_add(a);
                    local_end = base_start.saturating_add(b);
                }
            }

            ranges.push((
                line_start.saturating_add(local_start),
                line_start.saturating_add(local_end),
            ));
        }
    }
    ranges
}

fn segments_to_spans<'a>(
    segments: Vec<(Cow<'a, str>, Style)>,
    line_start: usize,
    line_len: usize,
    selection_ranges: &[(usize, usize)],
    selection_style: Style,
    render_prefix_len: usize,
) -> Vec<Span<'a>> {
    if selection_ranges.is_empty() {
        return segments
            .into_iter()
            .map(|(text, style)| Span::styled(text, to_ratatui_style(style)))
            .collect();
    }

    let mut with_offsets = Vec::new();
    let mut content_cursor = line_start;
    let mut prefix_skipped = 0usize;
    for (text, style) in segments {
        let len = text.len();
        let mut offset = 0usize;

        if prefix_skipped < render_prefix_len {
            let chrome_len = len.min(render_prefix_len.saturating_sub(prefix_skipped));
            if chrome_len > 0 {
                with_offsets.push(StyledSegment {
                    text: Cow::Owned(text[offset..offset + chrome_len].to_string()),
                    style,
                    start: 0,
                    end: 0,
                });
                offset = offset.saturating_add(chrome_len);
                prefix_skipped = prefix_skipped.saturating_add(chrome_len);
            }
        }

        if offset < len {
            let remainder_len = len.saturating_sub(offset);
            let segment_text = if offset == 0 {
                text
            } else {
                Cow::Owned(text[offset..].to_string())
            };
            with_offsets.push(StyledSegment {
                text: segment_text,
                style,
                start: content_cursor,
                end: content_cursor.saturating_add(remainder_len),
            });
            content_cursor = content_cursor.saturating_add(remainder_len);
        }
    }
    if content_cursor != line_start.saturating_add(line_len) {
        // Keep robust if render text and index diverge slightly.
    }
    let mut out = Vec::new();
    for seg in with_offsets {
        let mut cuts = vec![0usize, seg.text.len()];
        for (sel_start, sel_end) in selection_ranges {
            if *sel_end <= seg.start || *sel_start >= seg.end {
                continue;
            }
            let raw_a = sel_start.saturating_sub(seg.start).min(seg.text.len());
            let raw_b = sel_end.saturating_sub(seg.start).min(seg.text.len());
            // Snap to char boundaries to avoid panics from stale selection offsets.
            cuts.push(snap_cut_to_char_boundary(&seg.text, raw_a));
            cuts.push(snap_cut_to_char_boundary(&seg.text, raw_b));
        }
        cuts.sort_unstable();
        cuts.dedup();

        for win in cuts.windows(2) {
            let a = win[0];
            let b = win[1];
            if a >= b {
                continue;
            }
            let global_a = seg.start.saturating_add(a);
            let selected = selection_ranges
                .iter()
                .any(|(s, e)| global_a >= *s && global_a < *e);
            let style = if selected {
                seg.style.patch(selection_style)
            } else {
                seg.style
            };
            out.push(Span::styled(
                seg.text[a..b].to_string(),
                to_ratatui_style(style),
            ));
        }
    }

    out
}

/// Snap a byte offset to the nearest char boundary (rounding down) to avoid
/// panics when slicing strings with stale selection offsets.
fn snap_cut_to_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{
        blank_gutter_spans_with_inset, built_in_line_number_gutter_text,
        built_in_line_number_style, full_row_bg_style, gutter_spans_with_inset, h_scroll_segments,
        is_source_mode_continuation, render_single_line_spans, table_selection_ranges_for_line,
    };
    use crate::backend::ratatui_backend::common::to_ratatui_style;
    use crate::style::{Color, Style};
    use crate::widgets::ColumnAlign;
    use crate::widgets::document_view::node::{
        DocumentTableRectSelection, DocumentVisualLine, VisualLineKind,
    };
    use std::sync::Arc;

    #[test]
    fn render_single_line_spans_skips_zero_width_style_anchors() {
        let area = ratatui::layout::Rect::new(0, 0, 4, 1);
        let mut buffer = ratatui::buffer::Buffer::empty(area);
        let spans = vec![
            ratatui::text::Span::raw(""),
            ratatui::text::Span::raw("abc"),
        ];

        render_single_line_spans(&mut buffer, area, &spans);

        let rendered = (0..3).map(|x| buffer[(x, 0)].symbol()).collect::<String>();
        assert_eq!(rendered, "abc");
    }

    #[test]
    fn full_row_background_survives_when_scrolling_hides_all_visible_text() {
        let line_bg = Style::new().bg(Color::Green);
        let segments: Vec<(Cow<'_, str>, Style)> =
            vec![(Cow::Borrowed(""), line_bg), (Cow::Borrowed("}"), line_bg)];

        assert_eq!(full_row_bg_style(&segments), Some(line_bg));
        let scrolled = h_scroll_segments(segments, 1);
        assert!(scrolled.is_empty());
    }

    #[test]
    fn full_row_background_uses_hidden_anchor_not_scrolled_word_highlight() {
        let line_bg = Style::new().bg(Color::Green);
        let word_bg = Style::new().bg(Color::LightGreen);
        let segments: Vec<(Cow<'_, str>, Style)> = vec![
            (Cow::Borrowed(""), line_bg),
            (Cow::Borrowed("let "), line_bg),
            (Cow::Borrowed("changed"), word_bg),
        ];

        assert_eq!(full_row_bg_style(&segments), Some(line_bg));
        assert_eq!(
            full_row_bg_style(&h_scroll_segments(segments, 4)),
            Some(word_bg)
        );
    }

    #[test]
    fn gutter_inset_preserves_custom_gutter_style() {
        let style = Style::new().bg(Color::Green);
        let spans = vec![crate::style::Span::new("1 +").style(style)];

        let rendered = gutter_spans_with_inset(&spans, 2);

        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[0].content.as_ref(), "  ");
        assert_eq!(rendered[0].style.bg, to_ratatui_style(style).bg);
    }

    #[test]
    fn wrapped_custom_gutter_keeps_background_painted() {
        let style = Style::new().bg(Color::Green);
        let spans = vec![crate::style::Span::new("12 + ").style(style)];

        let rendered = blank_gutter_spans_with_inset(&spans, 7, 2, None);

        assert_eq!(rendered[0].content.as_ref(), "  ");
        assert_eq!(rendered[1].content.as_ref(), "     ");
        assert_eq!(rendered[0].style.bg, to_ratatui_style(style).bg);
        assert_eq!(rendered[1].style.bg, to_ratatui_style(style).bg);
    }

    #[test]
    fn wrapped_custom_gutter_override_style_replaces_source_style() {
        let source = Style::new().bg(Color::Green);
        let override_style = Style::new().fg(Color::DarkGray).bg(Color::Blue);
        let spans = vec![crate::style::Span::new("12 + ").style(source)];

        let rendered = blank_gutter_spans_with_inset(&spans, 7, 2, Some(override_style));

        assert_eq!(rendered[0].style.bg, to_ratatui_style(override_style).bg);
        assert_eq!(rendered[1].style.bg, to_ratatui_style(override_style).bg);
        assert_eq!(rendered[1].style.fg, to_ratatui_style(override_style).fg);
    }

    #[test]
    fn builtin_line_number_text_uses_separator_when_enabled() {
        let text = built_in_line_number_gutter_text(7, 3, 1, true, 0);
        assert_eq!(text, " 7 │");
    }

    #[test]
    fn builtin_line_number_text_omits_separator_when_disabled() {
        let text = built_in_line_number_gutter_text(7, 3, 1, false, 0);
        assert_eq!(text, "   7");
    }

    #[test]
    fn builtin_line_number_text_applies_content_gap_with_and_without_separator() {
        let with_separator = built_in_line_number_gutter_text(7, 5, 1, true, 2);
        let without_separator = built_in_line_number_gutter_text(7, 3, 1, false, 2);

        assert_eq!(with_separator, " 7 │  ");
        assert_eq!(without_separator, " 7  ");
    }

    #[test]
    fn source_mode_continuation_detects_same_source_line_across_kinds() {
        let lines = vec![
            DocumentVisualLine {
                kind: VisualLineKind::TableBorder {
                    kind: crate::widgets::document_view::node::TableBorderKind::Top,
                    widths: vec![3],
                    border_variant: crate::style::BorderStyle::Plain,
                    outer_frame: true,
                    column_separators: true,
                },
                source_line: 4,
            },
            DocumentVisualLine {
                kind: VisualLineKind::TableRow {
                    cells: vec![vec![crate::style::Span::new("x")]],
                    cell_line_texts: vec![Arc::from("x")],
                    full_cell_texts: vec![Arc::from("x")],
                    alignments: vec![ColumnAlign::Left],
                    widths: vec![3],
                    table_id: 0,
                    row_index: 0,
                    row_line_index: 0,
                    border_variant: crate::style::BorderStyle::Plain,
                    outer_frame: true,
                    column_separators: true,
                    cell_padding: 0,
                },
                source_line: 4,
            },
            DocumentVisualLine {
                kind: VisualLineKind::Text {
                    spans: vec![crate::style::Span::new("next")],
                    indent_cols: 0,
                    continuation: false,
                    links: Vec::new(),
                },
                source_line: 5,
            },
        ];

        assert!(!is_source_mode_continuation(&lines, 0, 0));
        assert!(is_source_mode_continuation(&lines, 1, 0));
        assert!(!is_source_mode_continuation(&lines, 2, 0));
        assert!(!is_source_mode_continuation(&lines, 1, 1));
    }

    #[test]
    fn builtin_line_number_style_defaults_to_dim_content_color() {
        let content = Style::new().fg(Color::Green);
        let style = built_in_line_number_style(content, Style::default());
        assert_eq!(style.fg, Some(crate::style::Paint::Solid(Color::Green)));
        assert_eq!(style.dim, Some(true));
    }

    #[test]
    fn builtin_line_number_style_allows_overrides() {
        let content = Style::new().fg(Color::Green);
        let override_style = Style::new().fg(Color::Yellow).not_bold();
        let style = built_in_line_number_style(content, override_style);
        assert_eq!(style.fg, Some(crate::style::Paint::Solid(Color::Yellow)));
        assert_eq!(style.dim, Some(true));
        assert_eq!(style.bold, Some(false));
    }

    #[test]
    fn reverse_table_selection_trims_cursor_cell_from_right_edge() {
        let vline = DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("alpha"), Arc::from("beta")],
                full_cell_texts: vec![Arc::from("alpha"), Arc::from("beta")],
                alignments: vec![ColumnAlign::Left, ColumnAlign::Left],
                widths: vec![5, 4],
                table_id: 7,
                row_index: 0,
                row_line_index: 0,
                border_variant: crate::style::BorderStyle::Plain,
                outer_frame: false,
                column_separators: true,
                cell_padding: 0,
            },
            source_line: 0,
        };
        let selection = DocumentTableRectSelection {
            table_id: 7,
            row_start: 0,
            row_end: 1,
            col_start: 0,
            col_end: 1,
            anchor_row_index: 1,
            anchor_col_index: 1,
            anchor_row_line_index: 0,
            anchor_cell_line_anchor_byte: 4,
            cursor_row_index: 0,
            cursor_col_index: 0,
            cursor_row_line_index: 0,
            cursor_cell_line_anchor_byte: 2,
            tsv_text: Arc::from("pha\tbeta\ngamma\tdelta"),
        };

        let ranges = table_selection_ranges_for_line(&vline, 0, &selection);

        assert_eq!(ranges, vec![(2, 5), (8, 12)]);
    }

    #[test]
    fn table_selection_suffix_mode_clamped_cursor_highlights_full_cell() {
        let vline = DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("alpha"), Arc::from("beta")],
                full_cell_texts: vec![Arc::from("alpha"), Arc::from("beta")],
                alignments: vec![ColumnAlign::Left, ColumnAlign::Left],
                widths: vec![5, 4],
                table_id: 7,
                row_index: 0,
                row_line_index: 0,
                border_variant: crate::style::BorderStyle::Plain,
                outer_frame: false,
                column_separators: true,
                cell_padding: 0,
            },
            source_line: 0,
        };
        let selection = DocumentTableRectSelection {
            table_id: 7,
            row_start: 0,
            row_end: 1,
            col_start: 0,
            col_end: 1,
            anchor_row_index: 1,
            anchor_col_index: 1,
            anchor_row_line_index: 0,
            anchor_cell_line_anchor_byte: 4,
            cursor_row_index: 0,
            cursor_col_index: 0,
            cursor_row_line_index: 0,
            cursor_cell_line_anchor_byte: usize::MAX,
            tsv_text: Arc::from("alpha\tbeta\ngamma\tdelta"),
        };

        let ranges = table_selection_ranges_for_line(&vline, 0, &selection);

        assert_eq!(ranges, vec![(0, 5), (8, 12)]);
    }

    #[test]
    fn table_selection_same_column_vertical_highlights_prefix_to_cursor() {
        let vline = DocumentVisualLine {
            kind: VisualLineKind::TableRow {
                cells: Vec::new(),
                cell_line_texts: vec![Arc::from("delta")],
                full_cell_texts: vec![Arc::from("delta")],
                alignments: vec![ColumnAlign::Left],
                widths: vec![5],
                table_id: 3,
                row_index: 0,
                row_line_index: 0,
                border_variant: crate::style::BorderStyle::Plain,
                outer_frame: false,
                column_separators: true,
                cell_padding: 0,
            },
            source_line: 0,
        };
        let selection = DocumentTableRectSelection {
            table_id: 3,
            row_start: 0,
            row_end: 1,
            col_start: 0,
            col_end: 0,
            anchor_row_index: 1,
            anchor_col_index: 0,
            anchor_row_line_index: 0,
            anchor_cell_line_anchor_byte: 5,
            cursor_row_index: 0,
            cursor_col_index: 0,
            cursor_row_line_index: 0,
            cursor_cell_line_anchor_byte: 2,
            tsv_text: Arc::from("de\nomega"),
        };

        let ranges = table_selection_ranges_for_line(&vline, 0, &selection);

        assert_eq!(ranges, vec![(0, 2)]);
    }
}
