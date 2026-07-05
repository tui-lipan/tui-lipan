use super::{GanttDiagram, measure_gantt_rows};

pub fn measure_gantt_diagram(diagram: &GanttDiagram) -> (u16, u16) {
    let rows = super::node::build_rows(&diagram.spec, diagram.max_timeline_width);
    let (content_w, content_h) = match rows.as_deref() {
        Ok(rows) => measure_gantt_rows(rows),
        Err(error) => measure_error(error),
    };

    (
        content_w.saturating_add(diagram.padding.horizontal()),
        content_h.saturating_add(diagram.padding.vertical()),
    )
}

fn measure_error(error: &str) -> (u16, u16) {
    (error.chars().count().min(usize::from(u16::MAX)) as u16, 1)
}
