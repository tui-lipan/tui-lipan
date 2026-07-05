//! Shared public Gantt diagram model and backend-agnostic render rows.
#![allow(missing_docs)]

mod layout;
mod node;
mod reconcile;
pub mod render_model;
pub mod theme;
pub mod timeline;

pub use layout::measure_gantt_diagram;
pub use node::GanttDiagramNode;
pub use reconcile::reconcile_gantt_diagram;
pub use render_model::{
    GanttRenderConfig, GanttRenderRole, GanttRenderRows, build_gantt_render_rows,
    measure_gantt_rows,
};
pub use theme::GanttDiagramTheme;
pub use timeline::GanttError;

use std::sync::Arc;

use crate::core::element::{Element, ElementKind};
use crate::style::{Length, Padding, Style};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GanttDate {
    pub year: i32,
    pub month: u8,
    pub day: u8,
}

impl GanttDate {
    pub const fn new(year: i32, month: u8, day: u8) -> Self {
        Self { year, month, day }
    }
}

impl std::fmt::Display for GanttDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum GanttTaskStart {
    Date(GanttDate),
    After(Arc<str>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GanttDuration {
    pub days: u32,
}

impl GanttDuration {
    pub const fn days(days: u32) -> Self {
        Self { days }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GanttTaskStatus {
    #[default]
    Pending,
    Active,
    Done,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GanttTask {
    pub id: Option<Arc<str>>,
    pub label: Arc<str>,
    pub start: Option<GanttTaskStart>,
    pub duration: GanttDuration,
    pub status: GanttTaskStatus,
    pub milestone: bool,
}

impl GanttTask {
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            id: None,
            label: label.into(),
            start: None,
            duration: GanttDuration::days(1),
            status: GanttTaskStatus::Pending,
            milestone: false,
        }
    }

    pub fn id(mut self, id: impl Into<Arc<str>>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn start_date(mut self, date: impl Into<GanttDate>) -> Self {
        self.start = Some(GanttTaskStart::Date(date.into()));
        self
    }

    pub fn after(mut self, id: impl Into<Arc<str>>) -> Self {
        self.start = Some(GanttTaskStart::After(id.into()));
        self
    }

    pub fn duration(mut self, duration: GanttDuration) -> Self {
        self.duration = duration;
        self
    }

    pub fn duration_days(mut self, days: u32) -> Self {
        self.duration = GanttDuration::days(days);
        self
    }

    pub fn status(mut self, status: GanttTaskStatus) -> Self {
        self.status = status;
        self
    }

    pub fn pending(mut self) -> Self {
        self.status = GanttTaskStatus::Pending;
        self
    }

    pub fn active(mut self) -> Self {
        self.status = GanttTaskStatus::Active;
        self
    }

    pub fn done(mut self) -> Self {
        self.status = GanttTaskStatus::Done;
        self
    }

    pub fn critical(mut self) -> Self {
        self.status = GanttTaskStatus::Critical;
        self
    }

    pub fn milestone(mut self) -> Self {
        self.milestone = true;
        self.duration = GanttDuration::days(0);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GanttSection {
    pub title: Arc<str>,
    pub tasks: Vec<GanttTask>,
}

impl GanttSection {
    pub fn new(title: impl Into<Arc<str>>) -> Self {
        Self {
            title: title.into(),
            tasks: Vec::new(),
        }
    }

    pub fn task(mut self, task: GanttTask) -> Self {
        self.tasks.push(task);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct GanttSpec {
    pub title: Option<Arc<str>>,
    pub sections: Vec<GanttSection>,
}

impl GanttSpec {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn title(mut self, title: impl Into<Arc<str>>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn section(mut self, section: GanttSection) -> Self {
        self.sections.push(section);
        self
    }
}

#[derive(Clone)]
pub struct GanttDiagram {
    pub(crate) spec: GanttSpec,
    pub(crate) theme: GanttDiagramTheme,
    pub(crate) style: Style,
    pub(crate) title_override: Style,
    pub(crate) axis_override: Style,
    pub(crate) section_override: Style,
    pub(crate) task_override: Style,
    pub(crate) padding: Padding,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) max_timeline_width: u16,
}

impl Default for GanttDiagram {
    fn default() -> Self {
        Self {
            spec: GanttSpec::default(),
            theme: GanttDiagramTheme::default(),
            style: Style::default(),
            title_override: Style::default(),
            axis_override: Style::default(),
            section_override: Style::default(),
            task_override: Style::default(),
            padding: Padding::default(),
            width: Length::Auto,
            height: Length::Auto,
            max_timeline_width: 80,
        }
    }
}

impl GanttDiagram {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn spec(mut self, spec: GanttSpec) -> Self {
        self.spec = spec;
        self
    }

    pub fn title(mut self, title: impl Into<Arc<str>>) -> Self {
        self.spec.title = Some(title.into());
        self
    }

    pub fn section(mut self, section: GanttSection) -> Self {
        self.spec.sections.push(section);
        self
    }

    pub fn theme(mut self, theme: GanttDiagramTheme) -> Self {
        self.theme = theme;
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn title_style(mut self, style: Style) -> Self {
        self.title_override = style;
        self
    }

    pub fn axis_style(mut self, style: Style) -> Self {
        self.axis_override = style;
        self
    }

    pub fn section_style(mut self, style: Style) -> Self {
        self.section_override = style;
        self
    }

    pub fn task_style(mut self, style: Style) -> Self {
        self.task_override = style;
        self
    }

    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn max_timeline_width(mut self, width: u16) -> Self {
        self.max_timeline_width = width;
        self
    }
}

impl From<GanttDiagram> for Element {
    fn from(value: GanttDiagram) -> Self {
        Element::new(ElementKind::GanttDiagram(Box::new(value)))
    }
}
