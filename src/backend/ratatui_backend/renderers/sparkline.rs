use ratatui::layout::Alignment;
use ratatui::widgets::{Paragraph, Wrap};

use crate::backend::ratatui_backend::common::{
    to_ratatui_style, truncate_spans, truncate_spans_start,
};
use crate::style::resolve::{resolve_accent_style, resolve_base_style};
use crate::style::{Rect, Theme};
use crate::widgets::Overflow;
use crate::widgets::internal::SparklineNode;

/// `rect`  - original unclipped sparkline rect (lipan coords), used to compute
///           the vertical scroll offset and horizontal truncation width.
/// `rrect` - pre-clipped ratatui rect to render into (already intersected by caller).
pub fn render_sparkline(
    f: &mut ratatui::Frame,
    node: &SparklineNode,
    theme: &Theme,
    rect: Rect,
    rrect: ratatui::layout::Rect,
    _clip_rect: Option<Rect>,
) {
    let base_style = resolve_base_style(theme, node.style);
    if rrect.is_empty() {
        return;
    }

    let output = &node.output;
    let overflow = match node.overflow {
        Overflow::Auto => Overflow::Ellipsis,
        other => other,
    };

    // When the sparkline is partially scrolled out of view from the top,
    // rrect.y > rect.y (rrect is already clipped to the visible area).
    // Paragraph renders from its first line, so skip the rows that are
    // above the visible clip boundary.
    let rows_clipped_top = (rrect.y as i32 - rect.y as i32).max(0) as usize;

    // Truncation uses the full unclipped content width.
    let content_width = rect.w;

    let mut ratatui_lines = Vec::new();
    for row in output.rows.iter().skip(rows_clipped_top) {
        let spans: Vec<ratatui::text::Span> = row
            .iter()
            .map(|span| {
                let style =
                    to_ratatui_style(base_style.patch(resolve_accent_style(theme, span.style)));
                ratatui::text::Span::styled(span.content.as_ref(), style)
            })
            .collect();

        let line = match overflow {
            Overflow::ClipStart => {
                let clipped = truncate_spans_start(spans, content_width);
                ratatui::text::Line::from(clipped).alignment(Alignment::Right)
            }
            Overflow::Ellipsis => {
                let clipped = truncate_spans(spans, content_width);
                ratatui::text::Line::from(clipped)
            }
            Overflow::Clip | Overflow::Wrap | Overflow::Auto => ratatui::text::Line::from(spans),
        };
        ratatui_lines.push(line);
    }

    let mut p = Paragraph::new(ratatui_lines);
    if overflow == Overflow::Wrap {
        p = p.wrap(Wrap { trim: false });
    }
    f.render_widget(p, rrect);
}
