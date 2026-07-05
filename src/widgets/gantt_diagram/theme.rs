#![allow(missing_docs)]

use crate::style::{Color, Style};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GanttDiagramTheme {
    pub axis: Style,
    pub title: Style,
    pub section: Style,
    pub task: Style,
    pub pending: Style,
    pub active: Style,
    pub done: Style,
    pub critical: Style,
    pub milestone: Style,
}

impl Default for GanttDiagramTheme {
    fn default() -> Self {
        Self {
            axis: Style::new().fg(Color::DarkGray),
            title: Style::new().bold(),
            section: Style::new().fg(Color::Cyan).bold(),
            task: Style::default(),
            pending: Style::new().fg(Color::Gray),
            active: Style::new().fg(Color::Yellow),
            done: Style::new().fg(Color::Green),
            critical: Style::new().fg(Color::LightRed),
            milestone: Style::new().fg(Color::Magenta),
        }
    }
}
