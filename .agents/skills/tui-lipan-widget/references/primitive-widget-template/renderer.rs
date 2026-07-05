use ratatui::Frame;

use crate::backend::ratatui_backend::common::{apply_border_style, apply_style};
use crate::core::node::NodeKind;
use crate::style::Rect;
use crate::widgets::#NAME_SNAKE#::#Name#Node;

pub fn render_#NAME_SNAKE#(
    f: &mut Frame,
    node: &#Name#Node,
    rect: Rect,
    clip_rect: Rect,
    is_focused: bool,
    is_hovering: bool,
) {
    // Determine effective styles
    let style = if node.disabled {
        node.disabled_style
    } else if is_focused {
        node.focus_style.patch(node.style)
    } else if is_hovering {
        node.hover_style.patch(node.style)
    } else {
        node.style
    };
    
    let border_style = if node.disabled {
        node.border_style
    } else if is_focused {
        node.focus_border_style.unwrap_or(node.border_style)
    } else if is_hovering {
        node.hover_border_style.unwrap_or(node.border_style)
    } else {
        node.border_style
    };
    
    // Calculate inner rect after padding
    let inner = rect.inner(true, node.padding);
    
    // Render border if needed
    if border_style != BorderStyle::default() {
        render_border(f, rect, border_style, style, clip_rect);
    }
    
    // Render content
    let content = node.label.as_ref();
    let ratatui_style = apply_style(style);
    
    // Clip to available space
    let visible_rect = inner.intersection(clip_rect);
    if visible_rect.w > 0 && visible_rect.h > 0 {
        let x = inner.x;
        let y = inner.y;
        
        // Handle alignment
        let x_offset = match node.align {
            crate::style::Align::Left => 0,
            crate::style::Align::Center => {
                let content_width = content.len().min(inner.w as usize) as i16;
                (inner.w as i16 - content_width) / 2
            }
            crate::style::Align::Right => {
                let content_width = content.len().min(inner.w as usize) as i16;
                inner.w as i16 - content_width
            }
        };
        
        // Render text
        let span = ratatui::text::Span::styled(content, ratatui_style);
        let x_pos = (x + x_offset).max(visible_rect.x);
        
        if x_pos < visible_rect.x + visible_rect.w as i16 && y >= visible_rect.y && y < visible_rect.y + visible_rect.h as i16 {
            ratatui::widgets::Paragraph::new(span)
                .render(
                    ratatui::layout::Rect::new(
                        x_pos as u16,
                        y as u16,
                        (visible_rect.x + visible_rect.w as i16 - x_pos).max(0) as u16,
                        1,
                    ),
                    f.buffer_mut(),
                );
        }
    }
}

fn render_border(
    f: &mut Frame,
    rect: Rect,
    border_style: BorderStyle,
    style: crate::style::Style,
    clip_rect: Rect,
) {
    use ratatui::widgets::{Block, Borders};
    
    let borders = match border_style {
        BorderStyle::None => Borders::NONE,
        BorderStyle::Single => Borders::ALL,
        BorderStyle::Double => Borders::ALL,
        BorderStyle::Rounded => Borders::ALL,
        BorderStyle::Thick => Borders::ALL,
    };
    
    if borders != Borders::NONE {
        let block = Block::default()
            .borders(borders)
            .border_style(apply_style(style));
        
        let area = ratatui::layout::Rect::new(
            rect.x as u16,
            rect.y as u16,
            rect.w as u16,
            rect.h as u16,
        );
        
        block.render(area, f.buffer_mut());
    }
}
