use ratatui::widgets::Block;

use crate::app::ContrastPolicy;
use crate::backend::ratatui_backend::common::{
    finalize_style, resolve_interactive_style_raw, to_ratatui_rect, to_ratatui_style,
};
use crate::style::{Rect, Theme, ThemeRole, resolve_slot};
use crate::widgets::internal::MouseRegionNode;

pub(crate) fn render_mouse_region(
    f: &mut ratatui::Frame<'_>,
    node: &MouseRegionNode,
    rect: Rect,
    clip_rect: Option<Rect>,
    is_hovered: bool,
    theme: &Theme,
    contrast_policy: ContrastPolicy,
) {
    let hover_style = resolve_slot(theme, ThemeRole::Hover, &node.hover_style);
    if !is_hovered || hover_style.is_empty() {
        return;
    }

    let mut draw_rect = rect;
    if let Some(clip) = clip_rect {
        draw_rect = draw_rect.intersection(&clip);
    }
    if draw_rect.is_empty() {
        return;
    }

    let rrect = to_ratatui_rect(draw_rect);
    let intersection = f.area().intersection(rrect);
    if intersection.width == 0 || intersection.height == 0 {
        return;
    }

    let hover_style = finalize_style(
        resolve_interactive_style_raw(
            crate::style::Style::default(),
            crate::style::Style::default(),
            hover_style,
            crate::style::Style::default(),
            false,
            is_hovered,
            false,
        ),
        None,
        contrast_policy,
    );

    f.render_widget(
        Block::default().style(to_ratatui_style(hover_style)),
        intersection,
    );
}
