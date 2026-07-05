use std::sync::Arc;

use super::{
    GanttDiagram, GanttDiagramTheme, GanttError, GanttRenderConfig, GanttRenderRows, GanttSpec,
    build_gantt_render_rows,
};
use crate::core::node::WidgetNode;
use crate::style::{Padding, Style};

#[derive(Clone)]
pub struct GanttDiagramNode {
    pub(crate) theme: GanttDiagramTheme,
    pub(crate) style: Style,
    pub(crate) title_override: Style,
    pub(crate) axis_override: Style,
    pub(crate) section_override: Style,
    pub(crate) task_override: Style,
    pub(crate) padding: Padding,
    pub(crate) render_rows: Result<Arc<GanttRenderRows>, Arc<str>>,
}

impl WidgetNode for GanttDiagramNode {}

impl From<GanttDiagram> for GanttDiagramNode {
    fn from(value: GanttDiagram) -> Self {
        let render_rows = build_rows(&value.spec, value.max_timeline_width);
        Self {
            theme: value.theme,
            style: value.style,
            title_override: value.title_override,
            axis_override: value.axis_override,
            section_override: value.section_override,
            task_override: value.task_override,
            padding: value.padding,
            render_rows,
        }
    }
}

pub(crate) fn build_rows(
    spec: &GanttSpec,
    max_timeline_width: u16,
) -> Result<Arc<GanttRenderRows>, Arc<str>> {
    build_gantt_render_rows(spec, GanttRenderConfig { max_timeline_width })
        .map(Arc::new)
        .map_err(error_message)
}

fn error_message(error: GanttError) -> Arc<str> {
    format!("Gantt error: {error}").into()
}
