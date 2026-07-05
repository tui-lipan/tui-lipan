use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::backend::ratatui_backend::common::{
    fill_rect_clipped, style_paints_bg, to_ratatui_rect, to_ratatui_style,
};
use crate::style::resolve::resolve_base_style;
use crate::style::{Rect, Style, Theme};
use crate::widgets::GanttRenderRole;
use crate::widgets::internal::GanttDiagramNode;

pub(crate) fn render_gantt_diagram(
    f: &mut ratatui::Frame<'_>,
    node: &GanttDiagramNode,
    theme: &Theme,
    rect: Rect,
    clip_rect: Rect,
) {
    let outer = rect.intersection(&clip_rect);
    if outer.is_empty() {
        return;
    }

    let base_style = resolve_base_style(theme, node.style);
    if style_paints_bg(base_style) {
        fill_rect_clipped(f, rect, to_ratatui_style(base_style), Some(clip_rect));
    }

    let content = rect.inset(node.padding);
    let content_clip = content.intersection(&clip_rect);
    if content.is_empty() || content_clip.is_empty() {
        return;
    }

    let lines = match &node.render_rows {
        Ok(rows) => rows
            .rows
            .iter()
            .map(|row| {
                let mut spans = Vec::new();
                for (index, cell) in row.cells.iter().enumerate() {
                    if index > 0 {
                        spans.push(Span::styled(" ", to_ratatui_style(base_style)));
                    }
                    spans.push(Span::styled(
                        cell.text.to_string(),
                        to_ratatui_style(style_for_role(node, base_style, cell.role)),
                    ));
                }
                Line::from(spans)
            })
            .collect::<Vec<_>>(),
        Err(error) => vec![Line::from(Span::styled(
            error.to_string(),
            to_ratatui_style(base_style),
        ))],
    };

    let dx = (content_clip.x as i32)
        .saturating_sub(content.x as i32)
        .max(0) as u16;
    let dy = (content_clip.y as i32)
        .saturating_sub(content.y as i32)
        .max(0) as u16;
    let paragraph = Paragraph::new(lines)
        .style(to_ratatui_style(base_style))
        .scroll((dy, dx));
    paragraph.render(to_ratatui_rect(content_clip), f.buffer_mut());
}

fn style_for_role(node: &GanttDiagramNode, base_style: Style, role: GanttRenderRole) -> Style {
    match role {
        GanttRenderRole::Text => base_style,
        GanttRenderRole::Title => base_style
            .patch(node.theme.title)
            .patch(node.title_override),
        GanttRenderRole::Axis => base_style.patch(node.theme.axis).patch(node.axis_override),
        GanttRenderRole::Section => base_style
            .patch(node.theme.section)
            .patch(node.section_override),
        GanttRenderRole::TaskLabel => base_style.patch(node.theme.task).patch(node.task_override),
        GanttRenderRole::PendingBar => base_style
            .patch(node.theme.pending)
            .patch(node.task_override),
        GanttRenderRole::ActiveBar => base_style
            .patch(node.theme.active)
            .patch(node.task_override),
        GanttRenderRole::DoneBar => base_style.patch(node.theme.done).patch(node.task_override),
        GanttRenderRole::CriticalBar => base_style
            .patch(node.theme.critical)
            .patch(node.task_override),
        GanttRenderRole::Milestone => base_style
            .patch(node.theme.milestone)
            .patch(node.task_override),
    }
}
