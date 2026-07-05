use std::borrow::Cow;

use ratatui::buffer::Buffer;
use ratatui::text::{Line, Span};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::style::{Rect, Style};
use crate::utils::text;

use super::cells::{ClipBounds, DrawCellClip, draw_cell};
use super::convert::{richtext_to_spans, to_ratatui_style};

#[cfg(feature = "terminal")]
pub(crate) fn is_cursor_visible(
    cursor_x: i16,
    cursor_y: i16,
    inner: Rect,
    clip_rect: Option<Rect>,
) -> bool {
    let clip = clip_rect
        .map(|rect| rect.intersection(&inner))
        .unwrap_or(inner);

    if clip.is_empty() {
        return false;
    }

    cursor_x >= clip.x
        && cursor_y >= clip.y
        && cursor_x < clip.x.saturating_add(clip.w as i16)
        && cursor_y < clip.y.saturating_add(clip.h as i16)
}

pub(crate) fn render_line_clipped(
    buf: &mut Buffer,
    start_x: i32,
    y: i32,
    max_width: i32,
    line: &Line<'_>,
    clip_rect: Option<Rect>,
) {
    if max_width <= 0 {
        return;
    }

    let content_width = line.width() as i32;
    let offset_x = match line.alignment {
        Some(ratatui::layout::Alignment::Center) => (max_width.saturating_sub(content_width)) / 2,
        Some(ratatui::layout::Alignment::Right) => max_width.saturating_sub(content_width),
        _ => 0,
    };

    let mut x = start_x + offset_x;
    let end_x = start_x + max_width;

    // Pre-compute bounds once before the loop
    let clip = clip_rect
        .map(ClipBounds::from_rect)
        .unwrap_or_else(ClipBounds::unbounded);
    let buf_bounds = ClipBounds::from_rrect(buf.area);

    // Early exit if entire line is outside clip bounds
    if y < clip.min_y || y >= clip.max_y || y < buf_bounds.min_y || y >= buf_bounds.max_y {
        return;
    }

    for span in &line.spans {
        for g in span.content.graphemes(true) {
            if x >= end_x || x >= clip.max_x {
                break;
            }

            if g == "\t" {
                let w = 4;
                if x + w > end_x {
                    break;
                }
                for _ in 0..w {
                    if x >= clip.max_x {
                        break;
                    }
                    draw_cell(
                        buf,
                        x,
                        y,
                        " ",
                        span.style,
                        DrawCellClip {
                            clip: &clip,
                            buf_bounds: &buf_bounds,
                        },
                    );
                    x += 1;
                }
                continue;
            }

            let w = UnicodeWidthStr::width(g) as i32;
            if w == 0 {
                continue;
            }
            if x + w > end_x || x + w > clip.max_x {
                break;
            }

            draw_cell(
                buf,
                x,
                y,
                g,
                span.style,
                DrawCellClip {
                    clip: &clip,
                    buf_bounds: &buf_bounds,
                },
            );
            x += w;
        }
    }
}

pub(crate) fn border_tabs_title_line<'a>(
    tab_titles: &'a [crate::style::RichText],
    active_tab: usize,
    active_tab_style: Style,
    inactive_tab_style: Style,
    tab_variant: crate::widgets::TabVariant,
    base_style: Style,
    _title_style: Style,
) -> Line<'a> {
    let active_tab = active_tab.min(tab_titles.len().saturating_sub(1));

    let active_style = to_ratatui_style(active_tab_style);
    let inactive_style = to_ratatui_style(inactive_tab_style);
    let separator_style = to_ratatui_style(base_style);

    let mut spans = Vec::new();

    for (idx, title) in tab_titles.iter().enumerate() {
        if idx > 0 {
            spans.push(Span::styled(tab_variant.separator(), separator_style));
        }

        let is_active = idx == active_tab;
        let (prefix, suffix) = if is_active {
            tab_variant.active_surround()
        } else {
            tab_variant.inactive_surround()
        };

        let tab_base_style = if is_active {
            active_tab_style
        } else {
            inactive_tab_style
        };
        let style = if is_active {
            active_style
        } else {
            inactive_style
        };

        if !prefix.is_empty() {
            spans.push(Span::styled(prefix, style));
        }
        spans.extend(richtext_to_spans(title, tab_base_style));
        if !suffix.is_empty() {
            spans.push(Span::styled(suffix, style));
        }
    }

    Line::from(spans)
}

pub(crate) fn truncate_end_with_ellipsis<'a>(line: &'a str, width: u16) -> Cow<'a, str> {
    let width = width as usize;
    if width == 0 {
        return Cow::Borrowed("");
    }

    if UnicodeWidthStr::width(line) <= width {
        return Cow::Borrowed(line);
    }

    let ell = '…';
    let ell_w = UnicodeWidthChar::width(ell).unwrap_or(1).max(1);

    if width <= ell_w {
        return Cow::Owned(ell.to_string());
    }

    let target = width.saturating_sub(ell_w);
    let end = text::end_at_width(line, 0, target);

    let mut out = String::with_capacity(end.saturating_add(ell.len_utf8()));
    out.push_str(&line[..end]);
    out.push(ell);
    Cow::Owned(out)
}

pub(crate) fn spaces(width: usize) -> Cow<'static, str> {
    const SPACE_CACHE: &str = concat!(
        "                                                                ",
        "                                                                ",
        "                                                                ",
        "                                                                "
    );

    if width <= SPACE_CACHE.len() {
        Cow::Borrowed(&SPACE_CACHE[..width])
    } else {
        Cow::Owned(" ".repeat(width))
    }
}

pub(crate) fn truncate_spans<'a>(spans: Vec<Span<'a>>, max_width: u16) -> Vec<Span<'a>> {
    let max_width = max_width as usize;
    if max_width == 0 {
        return Vec::new();
    }

    let total_width: usize = spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
        .sum();

    if total_width <= max_width {
        return spans;
    }

    let ell = "…";
    let ell_w = UnicodeWidthStr::width(ell);
    let target_width = max_width.saturating_sub(ell_w);

    let mut out = Vec::new();
    let mut current_width = 0;

    for span in spans {
        if current_width >= target_width {
            break;
        }

        let content = span.content.as_ref();
        let w = UnicodeWidthStr::width(content);

        if current_width + w <= target_width {
            out.push(span);
            current_width += w;
        } else {
            let avail = target_width - current_width;
            let end = text::end_at_width(content, 0, avail);
            let sub = &content[..end];
            out.push(Span::styled(sub.to_string(), span.style));
            break;
        }
    }

    let style = out.last().map(|s| s.style).unwrap_or_default();
    out.push(Span::styled(ell, style));

    out
}

pub(crate) fn truncate_spans_start<'a>(spans: Vec<Span<'a>>, max_width: u16) -> Vec<Span<'a>> {
    let max_width = max_width as usize;
    if max_width == 0 {
        return Vec::new();
    }

    let total_width: usize = spans
        .iter()
        .map(|s| UnicodeWidthStr::width(s.content.as_ref()))
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
        let w = UnicodeWidthStr::width(content);

        if current_width + w <= max_width {
            current_width += w;
            out_rev.push(span);
        } else {
            let need = max_width - current_width;
            let start = start_at_tail_width(content, need);
            let sub = &content[start..];
            out_rev.push(Span::styled(sub.to_string(), span.style));
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

    for (idx, g) in line.grapheme_indices(true).rev() {
        let gw = UnicodeWidthStr::width(g);
        if acc + gw > width {
            break;
        }
        acc += gw;
        start = idx;
    }

    start
}
