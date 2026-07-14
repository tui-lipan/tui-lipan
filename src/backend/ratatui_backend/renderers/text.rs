use std::sync::Arc;

use crate::backend::ratatui_backend::common::{
    ClipBounds, DrawCellStyledCtx, draw_cell_styled, style_has_alpha_paint,
    to_ratatui_style_with_terminal_bg, truncate_spans, truncate_spans_start,
};
use crate::style::{Color, Rect, Span as LipanSpan, Style};
use crate::widgets::Overflow;
use ratatui::buffer::Buffer;
use ratatui::layout::Alignment;
use ratatui::text::Line;
use ratatui::widgets::{Paragraph, Widget};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

pub(crate) struct TextRenderCtx {
    pub rect: Rect,
    pub rrect: ratatui::layout::Rect,
    pub clip_rect: Option<Rect>,
    pub terminal_bg: Option<Color>,
}

#[derive(Clone, Copy)]
struct LipanLineRenderCtx<'a> {
    clip: &'a ClipBounds,
    buf_bounds: &'a ClipBounds,
    terminal_bg: Option<Color>,
    alignment: Option<Alignment>,
}

pub(crate) fn render_text(
    f: &mut ratatui::Frame<'_>,
    spans: &[crate::style::Span],
    style: crate::style::Style,
    overflow: Overflow,
    ctx: TextRenderCtx,
) {
    let overflow = match overflow {
        Overflow::Auto => {
            if ctx.rect.h <= 1 {
                Overflow::Ellipsis
            } else {
                Overflow::Wrap
            }
        }
        other => other,
    };

    let r_clip = ctx
        .clip_rect
        .map(crate::backend::ratatui_backend::common::to_ratatui_rect);
    let effective_rrect = if let Some(clip) = r_clip {
        ctx.rrect.intersection(clip)
    } else {
        ctx.rrect
    };

    if effective_rrect.is_empty() {
        return;
    }

    if text_needs_cell_aware_render(spans, style) {
        render_text_cell_aware(
            f.buffer_mut(),
            spans,
            style,
            overflow,
            ctx.rect,
            effective_rrect,
            ctx.terminal_bg,
        );
        return;
    }

    match overflow {
        Overflow::Clip => {
            let lines = split_styled_lines(spans, style, ctx.terminal_bg);
            let dy = (effective_rrect.y as i32)
                .saturating_sub(ctx.rect.y as i32)
                .max(0) as u16;
            let dx = (effective_rrect.x as i32)
                .saturating_sub(ctx.rect.x as i32)
                .max(0) as u16;
            let output_lines: Vec<Line> = lines.into_iter().map(Line::from).collect();
            let p = Paragraph::new(output_lines).scroll((dy, dx));
            f.render_widget(p, effective_rrect);
        }
        Overflow::ClipStart => {
            let lines = split_styled_lines(spans, style, ctx.terminal_bg);
            let visible_start = (effective_rrect.y as i32)
                .saturating_sub(ctx.rect.y as i32)
                .max(0) as usize;
            let max_lines = effective_rrect.height as usize;
            let output_lines: Vec<Line> = lines
                .into_iter()
                .skip(visible_start)
                .take(max_lines)
                .map(|line| {
                    let clipped = truncate_spans_start(line, ctx.rect.w);
                    Line::from(clipped).alignment(Alignment::Right)
                })
                .collect();

            let p = Paragraph::new(output_lines);
            f.render_widget(p, effective_rrect);
        }
        Overflow::Wrap => {
            let wrapped = wrap_text_spans(spans, style, ctx.rect.w, ctx.terminal_bg);
            let dy = (effective_rrect.y as i32)
                .saturating_sub(ctx.rect.y as i32)
                .max(0) as u16;
            let dx = (effective_rrect.x as i32)
                .saturating_sub(ctx.rect.x as i32)
                .max(0) as u16;

            if dx == 0 && effective_rrect.width == ctx.rect.w {
                let p = Paragraph::new(wrapped).scroll((dy, 0));
                f.render_widget(p, effective_rrect);
            } else {
                render_wrapped_text_via_temp_buffer(
                    f.buffer_mut(),
                    wrapped,
                    ctx.rect.w,
                    effective_rrect,
                    dy,
                    dx,
                );
            }
        }
        Overflow::Ellipsis => {
            let lines = split_styled_lines(spans, style, ctx.terminal_bg);

            // Truncate and render
            let visible_start = (effective_rrect.y as i32)
                .saturating_sub(ctx.rect.y as i32)
                .max(0) as usize;
            let max_lines = effective_rrect.height as usize;
            let total_lines = lines.len();
            let mut output_lines = Vec::new();

            for (i, line) in lines
                .into_iter()
                .skip(visible_start)
                .enumerate()
                .take(max_lines)
            {
                let mut truncated = truncate_spans(line, ctx.rect.w);

                if i == max_lines - 1 && total_lines > max_lines {
                    let has_ellipsis = truncated
                        .last()
                        .map(|s| s.content.as_ref().ends_with('…'))
                        .unwrap_or(false);

                    if !has_ellipsis {
                        let ell = "…";
                        truncated.push(ratatui::text::Span::raw(ell));
                        truncated = truncate_spans(truncated, ctx.rect.w);
                    }
                }

                output_lines.push(Line::from(truncated));
            }

            // Apply the horizontal clip offset so a rect clipped on the left
            // (e.g. scrolled horizontally) shows the visible portion rather than
            // the start of the text. Mirrors the `Clip`/`Wrap` paths and the
            // vertical `visible_start` handling above.
            let dx = (effective_rrect.x as i32)
                .saturating_sub(ctx.rect.x as i32)
                .max(0) as u16;
            let p = Paragraph::new(output_lines).scroll((0, dx));
            f.render_widget(p, effective_rrect);
        }
        Overflow::Auto => unreachable!("Auto is resolved above"),
    }
}

// ── Cell-aware alpha paint path ──────────────────────────────────────

fn text_needs_cell_aware_render(spans: &[LipanSpan], base_style: Style) -> bool {
    if spans.is_empty() {
        return style_has_alpha_paint(base_style);
    }

    spans
        .iter()
        .any(|span| style_has_alpha_paint(base_style.patch(span.style)))
}

fn render_text_cell_aware(
    buf: &mut Buffer,
    spans: &[LipanSpan],
    style: Style,
    overflow: Overflow,
    rect: Rect,
    effective_rrect: ratatui::layout::Rect,
    terminal_bg: Option<Color>,
) {
    let clip_rect = rect_from_ratatui(effective_rrect);
    let clip = ClipBounds::from_rect(clip_rect);
    let buf_bounds = ClipBounds::from_rrect(buf.area);
    let line_ctx = |alignment: Option<Alignment>| LipanLineRenderCtx {
        clip: &clip,
        buf_bounds: &buf_bounds,
        terminal_bg,
        alignment,
    };

    match overflow {
        Overflow::Clip => {
            let lines = split_styled_lipan_lines(spans, style);
            render_lipan_lines(buf, &lines, rect, line_ctx(None));
        }
        Overflow::ClipStart => {
            let visible_start = (effective_rrect.y as i32)
                .saturating_sub(rect.y as i32)
                .max(0) as usize;
            let max_lines = effective_rrect.height as usize;
            let lines: Vec<Vec<LipanSpan>> = split_styled_lipan_lines(spans, style)
                .into_iter()
                .skip(visible_start)
                .take(max_lines)
                .map(|line| truncate_lipan_spans_start(line, rect.w))
                .collect();

            render_lipan_lines_at(
                buf,
                &lines,
                effective_rrect.x as i32,
                effective_rrect.y as i32,
                effective_rrect.width as i32,
                line_ctx(Some(Alignment::Right)),
            );
        }
        Overflow::Wrap => {
            let lines = wrap_text_spans_lipan(spans, style, rect.w);
            render_lipan_lines(buf, &lines, rect, line_ctx(None));
        }
        Overflow::Ellipsis => {
            let lines = split_styled_lipan_lines(spans, style);
            let visible_start = (effective_rrect.y as i32)
                .saturating_sub(rect.y as i32)
                .max(0) as usize;
            let max_lines = effective_rrect.height as usize;
            let total_lines = lines.len();
            let output_lines: Vec<Vec<LipanSpan>> = lines
                .into_iter()
                .skip(visible_start)
                .enumerate()
                .take(max_lines)
                .map(|(i, line)| {
                    let mut truncated = truncate_lipan_spans(line, rect.w);
                    if i == max_lines.saturating_sub(1) && total_lines > max_lines {
                        let has_ellipsis = truncated
                            .last()
                            .map(|s| s.content.as_ref().ends_with('…'))
                            .unwrap_or(false);

                        if !has_ellipsis {
                            truncated.push(LipanSpan::new("…"));
                            truncated = truncate_lipan_spans(truncated, rect.w);
                        }
                    }
                    truncated
                })
                .collect();

            // Draw from the element's true left (`rect.x`, which may be left of
            // the clip when horizontally scrolled) so per-cell clipping shows the
            // visible portion rather than the start. Vertical position uses the
            // already-skipped `effective_rrect.y`. No-op when unclipped.
            render_lipan_lines_at(
                buf,
                &output_lines,
                rect.x as i32,
                effective_rrect.y as i32,
                rect.w as i32,
                line_ctx(None),
            );
        }
        Overflow::Auto => unreachable!("Auto is resolved above"),
    }
}

fn rect_from_ratatui(rect: ratatui::layout::Rect) -> Rect {
    Rect {
        x: rect.x.min(i16::MAX as u16) as i16,
        y: rect.y.min(i16::MAX as u16) as i16,
        w: rect.width,
        h: rect.height,
    }
}

fn render_lipan_lines(
    buf: &mut Buffer,
    lines: &[Vec<LipanSpan>],
    rect: Rect,
    ctx: LipanLineRenderCtx<'_>,
) {
    render_lipan_lines_at(buf, lines, rect.x as i32, rect.y as i32, rect.w as i32, ctx);
}

fn render_lipan_lines_at(
    buf: &mut Buffer,
    lines: &[Vec<LipanSpan>],
    start_x: i32,
    start_y: i32,
    max_width: i32,
    ctx: LipanLineRenderCtx<'_>,
) {
    for (idx, line) in lines.iter().enumerate() {
        let y = start_y.saturating_add(idx as i32);
        render_lipan_line_clipped(buf, start_x, y, max_width, line, ctx);
    }
}

fn render_lipan_line_clipped(
    buf: &mut Buffer,
    start_x: i32,
    y: i32,
    max_width: i32,
    line: &[LipanSpan],
    ctx: LipanLineRenderCtx<'_>,
) {
    let LipanLineRenderCtx {
        clip,
        buf_bounds,
        terminal_bg,
        alignment,
    } = ctx;
    if max_width <= 0 {
        return;
    }

    if y < clip.min_y || y >= clip.max_y || y < buf_bounds.min_y || y >= buf_bounds.max_y {
        return;
    }

    let content_width = lipan_line_width(line) as i32;
    let offset_x = match alignment {
        Some(Alignment::Center) => max_width.saturating_sub(content_width) / 2,
        Some(Alignment::Right) => max_width.saturating_sub(content_width),
        _ => 0,
    };

    let mut x = start_x.saturating_add(offset_x);
    let end_x = start_x.saturating_add(max_width);

    for span in line {
        for grapheme in span.content.as_ref().graphemes(true) {
            if x >= end_x || x >= clip.max_x {
                break;
            }

            if grapheme == "\t" {
                let w = 4;
                if x + w > end_x {
                    break;
                }
                for _ in 0..w {
                    if x >= clip.max_x {
                        break;
                    }
                    draw_cell_styled(
                        buf,
                        x,
                        y,
                        " ",
                        span.style,
                        DrawCellStyledCtx {
                            clip,
                            buf_bounds,
                            terminal_bg,
                        },
                    );
                    x += 1;
                }
                continue;
            }

            let width = UnicodeWidthStr::width(grapheme) as i32;
            if width == 0 {
                continue;
            }
            if x + width > end_x || x + width > clip.max_x {
                break;
            }

            let first_cell_visible = clip.contains(x, y) && buf_bounds.contains(x, y);
            draw_cell_styled(
                buf,
                x,
                y,
                grapheme,
                span.style,
                DrawCellStyledCtx {
                    clip,
                    buf_bounds,
                    terminal_bg,
                },
            );
            if first_cell_visible && width > 1 {
                reset_wide_continuation_cells(buf, x, y, width, clip, buf_bounds);
            }
            x += width;
        }
    }
}

fn reset_wide_continuation_cells(
    buf: &mut Buffer,
    start_x: i32,
    y: i32,
    width: i32,
    clip: &ClipBounds,
    buf_bounds: &ClipBounds,
) {
    for x in (start_x + 1)..(start_x + width) {
        if !clip.contains(x, y) || !buf_bounds.contains(x, y) {
            continue;
        }
        if let Some(cell) = buf.cell_mut((x as u16, y as u16)) {
            cell.reset();
        }
    }
}

fn lipan_line_width(line: &[LipanSpan]) -> usize {
    line.iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum()
}

fn split_styled_lipan_lines(spans: &[LipanSpan], base_style: Style) -> Vec<Vec<LipanSpan>> {
    let mut lines = Vec::new();
    let mut current_line = Vec::new();

    for span in spans {
        let cell_style = base_style.patch(span.style);
        let content = span.content.as_ref();
        for (i, part) in content.split('\n').enumerate() {
            if i > 0 {
                lines.push(current_line);
                current_line = Vec::new();
            }
            if !part.is_empty() {
                current_line.push(LipanSpan {
                    content: Arc::from(part),
                    style: cell_style,
                    row_style_policy: span.row_style_policy,
                });
            }
        }
    }
    lines.push(current_line);
    lines
}

fn wrap_text_spans_lipan(
    spans: &[LipanSpan],
    base_style: Style,
    wrap_width: u16,
) -> Vec<Vec<LipanSpan>> {
    let logical_lines = split_styled_lipan_lines(spans, base_style);

    if wrap_width == 0 {
        return vec![Vec::new(); logical_lines.len().max(1)];
    }

    let mut result = Vec::new();

    for logical_line in logical_lines {
        let visual_lines =
            crate::utils::text::wrap_spans_for_budgets(&logical_line, wrap_width, wrap_width);
        result.extend(visual_lines);
    }

    if result.is_empty() {
        result.push(Vec::new());
    }

    result
}

fn truncate_lipan_spans(spans: Vec<LipanSpan>, max_width: u16) -> Vec<LipanSpan> {
    let max_width = max_width as usize;
    if max_width == 0 {
        return Vec::new();
    }

    let total_width: usize = spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
    if total_width <= max_width {
        return spans;
    }

    let ellipsis = "…";
    let target_width = max_width.saturating_sub(UnicodeWidthStr::width(ellipsis));
    let mut out = Vec::new();
    let mut current_width = 0;

    for span in spans {
        if current_width >= target_width {
            break;
        }

        let content = span.content.as_ref();
        let width = UnicodeWidthStr::width(content);
        if current_width + width <= target_width {
            current_width += width;
            out.push(span);
        } else {
            let available = target_width - current_width;
            let end = crate::utils::text::end_at_width(content, 0, available);
            out.push(LipanSpan {
                content: Arc::from(&content[..end]),
                style: span.style,
                row_style_policy: span.row_style_policy,
            });
            break;
        }
    }

    let style = out.last().map(|span| span.style).unwrap_or_default();
    out.push(LipanSpan::new(ellipsis).style(style));
    out
}

fn truncate_lipan_spans_start(spans: Vec<LipanSpan>, max_width: u16) -> Vec<LipanSpan> {
    let max_width = max_width as usize;
    if max_width == 0 {
        return Vec::new();
    }

    let total_width: usize = spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum();
    if total_width <= max_width {
        return spans;
    }

    let mut out_rev = Vec::new();
    let mut current_width = 0usize;

    for span in spans.into_iter().rev() {
        if current_width >= max_width {
            break;
        }

        let content = span.content.as_ref();
        let width = UnicodeWidthStr::width(content);
        if current_width + width <= max_width {
            current_width += width;
            out_rev.push(span);
        } else {
            let needed = max_width - current_width;
            let start = start_at_tail_width(content, needed);
            out_rev.push(LipanSpan {
                content: Arc::from(&content[start..]),
                style: span.style,
                row_style_policy: span.row_style_policy,
            });
            break;
        }
    }

    out_rev.reverse();
    out_rev
}

fn start_at_tail_width(line: &str, width: usize) -> usize {
    if width == 0 {
        return line.len();
    }

    let mut acc = 0usize;
    let mut start = line.len();
    for (idx, grapheme) in line.grapheme_indices(true).rev() {
        let grapheme_width = UnicodeWidthStr::width(grapheme);
        if acc + grapheme_width > width {
            break;
        }
        acc += grapheme_width;
        start = idx;
    }
    start
}

// ── Word-aware wrapping (delegates to shared wrap_spans_for_budgets) ─

/// Wrap text spans using the shared `wrap_spans_for_budgets` utility,
/// ensuring measurement and rendering always agree on line count.
fn wrap_text_spans(
    spans: &[crate::style::Span],
    base_style: crate::style::Style,
    wrap_width: u16,
    terminal_bg: Option<Color>,
) -> Vec<Line<'static>> {
    use crate::widgets::internal::split_spans_on_newlines;

    let logical_lines = split_spans_on_newlines(spans);

    if wrap_width == 0 {
        return vec![Line::default(); logical_lines.len().max(1)];
    }

    let mut result = Vec::new();

    for logical_line in logical_lines {
        let visual_lines =
            crate::utils::text::wrap_spans_for_budgets(&logical_line, wrap_width, wrap_width);
        for vline in visual_lines {
            let rat_spans: Vec<ratatui::text::Span<'static>> = vline
                .into_iter()
                .map(|s| {
                    ratatui::text::Span::styled(
                        s.content.to_string(),
                        to_ratatui_style_with_terminal_bg(base_style.patch(s.style), terminal_bg),
                    )
                })
                .collect();
            result.push(Line::from(rat_spans));
        }
    }

    if result.is_empty() {
        result.push(Line::default());
    }

    result
}

// ── Temp-buffer path (clipped wrap) ──────────────────────────────────

fn render_wrapped_text_via_temp_buffer(
    main_buf: &mut Buffer,
    wrapped_lines: Vec<Line<'static>>,
    width: u16,
    effective_rrect: ratatui::layout::Rect,
    dy: u16,
    dx: u16,
) {
    let temp_h = effective_rrect.height;
    if width == 0 || temp_h == 0 {
        return;
    }

    let temp_area = ratatui::layout::Rect::new(0, 0, width, temp_h);
    let mut temp_buf = Buffer::empty(temp_area);

    Paragraph::new(wrapped_lines)
        .scroll((dy, dx))
        .render(temp_area, &mut temp_buf);

    for y in 0..effective_rrect.height {
        for x in 0..effective_rrect.width {
            if let Some(cell) = temp_buf.cell((x, y))
                && let Some(dst_cell) =
                    main_buf.cell_mut((effective_rrect.x + x, effective_rrect.y + y))
            {
                let saved_bg = dst_cell.bg;
                *dst_cell = cell.clone();
                if dst_cell.bg == ratatui::style::Color::Reset {
                    dst_cell.bg = saved_bg;
                }
            }
        }
    }
}

// ── Shared helpers ───────────────────────────────────────────────────

fn split_styled_lines<'a>(
    spans: &'a [crate::style::Span],
    style: crate::style::Style,
    terminal_bg: Option<Color>,
) -> Vec<Vec<ratatui::text::Span<'a>>> {
    let mut lines = Vec::new();
    let mut current_line = Vec::new();

    for span in spans {
        let s_content = span.content.as_ref();
        for (i, part) in s_content.split('\n').enumerate() {
            if i > 0 {
                lines.push(current_line);
                current_line = Vec::new();
            }
            if !part.is_empty() {
                let cell_style = style.patch(span.style);
                current_line.push(ratatui::text::Span::styled(
                    part,
                    to_ratatui_style_with_terminal_bg(cell_style, terminal_bg),
                ));
            }
        }
    }
    lines.push(current_line);
    lines
}

#[cfg(test)]
mod tests {
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::style::Color as RColor;

    use super::{TextRenderCtx, render_text, split_styled_lines};
    use crate::backend::ratatui_backend::common::{blend_paint_over_ratatui, to_ratatui_rect};
    use crate::style::{Color, Paint, Rect, Span, Style};
    use crate::widgets::Overflow;

    #[test]
    fn split_styled_lines_preserves_newline_boundaries() {
        let spans = [Span::from("first\nsecond"), Span::from("\nthird")];
        let lines = split_styled_lines(&spans, Style::default(), None);

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].len(), 1);
        assert_eq!(lines[0][0].content.as_ref(), "first");
        assert_eq!(lines[1].len(), 1);
        assert_eq!(lines[1][0].content.as_ref(), "second");
        assert_eq!(lines[2].len(), 1);
        assert_eq!(lines[2][0].content.as_ref(), "third");
    }

    #[test]
    fn split_styled_lines_keeps_empty_middle_lines() {
        let spans = [Span::from("a\n\nb")];
        let lines = split_styled_lines(&spans, Style::default(), None);

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0][0].content.as_ref(), "a");
        assert!(lines[1].is_empty());
        assert_eq!(lines[2][0].content.as_ref(), "b");
    }

    #[test]
    fn alpha_foreground_uses_existing_cell_background() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 6,
            h: 1,
        };
        let panel_bg = RColor::Rgb(0x15, 0x15, 0x19);
        let terminal_bg = Color::Rgb(0x23, 0x23, 0x29);
        let alpha_fg = Paint::rgba(0xff, 0xff, 0xff, 0x40);
        let spans = [Span::from("alpha")];
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::new(backend).expect("terminal should init");

        terminal
            .draw(|f| {
                for cell in &mut f.buffer_mut().content {
                    cell.bg = panel_bg;
                }
                render_text(
                    f,
                    &spans,
                    Style::new().fg(alpha_fg),
                    Overflow::Clip,
                    TextRenderCtx {
                        rect,
                        rrect: to_ratatui_rect(rect),
                        clip_rect: None,
                        terminal_bg: Some(terminal_bg),
                    },
                );
            })
            .expect("draw should succeed");

        let cell = &terminal.backend().buffer()[(0, 0)];
        let expected = blend_paint_over_ratatui(alpha_fg, panel_bg);
        let fallback = blend_paint_over_ratatui(alpha_fg, RColor::Rgb(0x23, 0x23, 0x29));

        assert_eq!(cell.symbol(), "a");
        assert_eq!(cell.bg, panel_bg);
        assert_eq!(Some(cell.fg), expected);
        assert_ne!(Some(cell.fg), fallback);
    }

    #[test]
    fn alpha_text_clears_wide_grapheme_continuation_cell() {
        let rect = Rect {
            x: 0,
            y: 0,
            w: 4,
            h: 1,
        };
        let panel_bg = RColor::Rgb(0x15, 0x15, 0x19);
        let alpha_fg = Paint::rgba(0xff, 0xff, 0xff, 0x40);
        let spans = [Span::from("你a")];
        let backend = TestBackend::new(rect.w, rect.h);
        let mut terminal = Terminal::new(backend).expect("terminal should init");

        terminal
            .draw(|f| {
                for cell in &mut f.buffer_mut().content {
                    cell.set_symbol("x");
                    cell.bg = panel_bg;
                }
                render_text(
                    f,
                    &spans,
                    Style::new().fg(alpha_fg),
                    Overflow::Clip,
                    TextRenderCtx {
                        rect,
                        rrect: to_ratatui_rect(rect),
                        clip_rect: None,
                        terminal_bg: Some(Color::Rgb(0x23, 0x23, 0x29)),
                    },
                );
            })
            .expect("draw should succeed");

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), "你");
        assert_eq!(buffer[(1, 0)].symbol(), " ");
        assert_eq!(buffer[(2, 0)].symbol(), "a");
    }
}
