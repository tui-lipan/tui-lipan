use ratatui::style::Style as RStyle;
use ratatui::text::Line;
use ratatui::widgets::Borders;

use crate::style::Rect;

use super::convert::to_ratatui_rect;

/// Render a placeholder border frame at `draw_rect` with an optional centered message.
///
/// Used by the Image widget during resize/pending decode, `DragSource` snapshot drags, and
/// drop-slot highlights.
pub(crate) fn render_placeholder_frame(
    f: &mut ratatui::Frame<'_>,
    draw_rect: Rect,
    style: RStyle,
    message: Option<&str>,
) {
    render_placeholder_frame_clipped(f, draw_rect, draw_rect, style, message);
}

/// Render a placeholder border frame for `full_rect`, clipped to `clip_rect`.
///
/// Only border edges that fall within the clip rect are drawn, so a partially
/// scrolled-out placeholder looks naturally clipped rather than shrunk.
pub(crate) fn render_placeholder_frame_clipped(
    f: &mut ratatui::Frame<'_>,
    full_rect: Rect,
    clip_rect: Rect,
    style: RStyle,
    message: Option<&str>,
) {
    use ratatui::widgets::Block;

    let visible = full_rect.intersection(&clip_rect);
    if visible.is_empty() {
        return;
    }

    if full_rect.w >= 2 && full_rect.h >= 2 {
        let full_top = full_rect.y;
        let full_bottom = full_rect.y.saturating_add(full_rect.h as i16);
        let full_left = full_rect.x;
        let full_right = full_rect.x.saturating_add(full_rect.w as i16);

        let clip_top = clip_rect.y;
        let clip_bottom = clip_rect.y.saturating_add(clip_rect.h as i16);
        let clip_left = clip_rect.x;
        let clip_right = clip_rect.x.saturating_add(clip_rect.w as i16);

        let mut borders = Borders::empty();
        if full_top >= clip_top {
            borders |= Borders::TOP;
        }
        if full_bottom <= clip_bottom {
            borders |= Borders::BOTTOM;
        }
        if full_left >= clip_left {
            borders |= Borders::LEFT;
        }
        if full_right <= clip_right {
            borders |= Borders::RIGHT;
        }

        let block = Block::default().borders(borders).style(style);
        f.render_widget(block, to_ratatui_rect(visible));
    } else {
        let block = Block::default().style(style);
        f.render_widget(block, to_ratatui_rect(visible));
    }

    if let Some(message) = message.filter(|_| visible.w >= 6 && visible.h >= 1) {
        use ratatui::layout::Alignment;
        use ratatui::text::Span;
        use ratatui::widgets::Paragraph;

        let msg_y = full_rect.y.saturating_add((full_rect.h / 2) as i16);
        let vis_top = visible.y;
        let vis_bottom = visible.y.saturating_add(visible.h as i16);
        if msg_y >= vis_top && msg_y < vis_bottom {
            let message_rect = Rect {
                x: visible.x,
                y: msg_y,
                w: visible.w,
                h: 1,
            };
            let line = Line::from(vec![Span::styled(message.to_string(), style)]);
            let paragraph = Paragraph::new(line).alignment(Alignment::Center);
            f.render_widget(paragraph, to_ratatui_rect(message_rect));
        }
    }
}
