#![allow(missing_docs)]

use std::sync::Arc;

use unicode_width::UnicodeWidthStr;

use super::timeline::{GanttError, resolve_gantt_spec};
use super::{GanttSpec, GanttTaskStatus};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum GanttRenderRole {
    #[default]
    Text,
    Title,
    Axis,
    Section,
    TaskLabel,
    PendingBar,
    ActiveBar,
    DoneBar,
    CriticalBar,
    Milestone,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GanttRenderCell {
    pub text: Arc<str>,
    pub role: GanttRenderRole,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GanttRenderRow {
    pub role: GanttRenderRole,
    pub cells: Vec<GanttRenderCell>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct GanttRenderRows {
    pub rows: Vec<GanttRenderRow>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GanttRenderConfig {
    pub max_timeline_width: u16,
}

impl Default for GanttRenderConfig {
    fn default() -> Self {
        Self {
            max_timeline_width: 80,
        }
    }
}

pub fn build_gantt_render_rows(
    spec: &GanttSpec,
    config: GanttRenderConfig,
) -> Result<GanttRenderRows, GanttError> {
    let resolved = resolve_gantt_spec(spec)?;
    let min_day = resolved
        .iter()
        .flat_map(|section| section.tasks.iter().map(|task| task.start_day))
        .min();
    let max_day = resolved
        .iter()
        .flat_map(|section| section.tasks.iter().map(|task| task.end_day_exclusive))
        .max();

    let label_width = resolved
        .iter()
        .flat_map(|section| section.tasks.iter())
        .map(|task| UnicodeWidthStr::width(task.task.label.as_ref()))
        .max()
        .unwrap_or(0);

    let mut rows = Vec::new();
    if let Some(title) = &spec.title {
        rows.push(row(
            GanttRenderRole::Title,
            vec![cell(title.clone(), GanttRenderRole::Title)],
        ));
    }

    let Some(min_day) = min_day else {
        return Ok(GanttRenderRows { rows });
    };
    let max_day = max_day.unwrap_or(min_day + 1).max(min_day + 1);
    let timeline_width = timeline_width(config.max_timeline_width, max_day - min_day);
    rows.push(row(
        GanttRenderRole::Axis,
        vec![cell(
            date_range_text(min_day, max_day).into(),
            GanttRenderRole::Axis,
        )],
    ));
    rows.push(row(
        GanttRenderRole::Axis,
        vec![
            cell(" ".repeat(label_width).into(), GanttRenderRole::TaskLabel),
            cell(axis_text(timeline_width).into(), GanttRenderRole::Axis),
        ],
    ));

    for section in resolved {
        rows.push(row(
            GanttRenderRole::Section,
            vec![cell(section.title, GanttRenderRole::Section)],
        ));
        for task in section.tasks {
            let bar_role = if task.task.milestone {
                GanttRenderRole::Milestone
            } else {
                role_for_status(task.task.status)
            };
            rows.push(row(
                GanttRenderRole::Text,
                vec![
                    cell(
                        pad_to_width(task.task.label.as_ref(), label_width).into(),
                        GanttRenderRole::TaskLabel,
                    ),
                    cell(
                        task_bar(
                            task.start_day,
                            task.end_day_exclusive,
                            min_day,
                            max_day,
                            timeline_width,
                            task.task.milestone,
                            bar_role,
                        )
                        .into(),
                        bar_role,
                    ),
                ],
            ));
        }
    }

    Ok(GanttRenderRows { rows })
}

pub fn measure_gantt_rows(rows: &GanttRenderRows) -> (u16, u16) {
    let width = rows
        .rows
        .iter()
        .map(|row| {
            row.cells
                .iter()
                .map(|cell| UnicodeWidthStr::width(cell.text.as_ref()))
                .sum::<usize>()
                + row.cells.len().saturating_sub(1)
        })
        .max()
        .unwrap_or(0);
    (
        width.min(usize::from(u16::MAX)) as u16,
        rows.rows.len().min(usize::from(u16::MAX)) as u16,
    )
}

fn row(role: GanttRenderRole, cells: Vec<GanttRenderCell>) -> GanttRenderRow {
    GanttRenderRow { role, cells }
}

fn cell(text: Arc<str>, role: GanttRenderRole) -> GanttRenderCell {
    GanttRenderCell { text, role }
}

fn role_for_status(status: GanttTaskStatus) -> GanttRenderRole {
    match status {
        GanttTaskStatus::Pending => GanttRenderRole::PendingBar,
        GanttTaskStatus::Active => GanttRenderRole::ActiveBar,
        GanttTaskStatus::Done => GanttRenderRole::DoneBar,
        GanttTaskStatus::Critical => GanttRenderRole::CriticalBar,
    }
}

fn timeline_width(max_timeline_width: u16, days: i64) -> usize {
    let max_timeline_width = usize::from(max_timeline_width.max(1));
    max_timeline_width.min(days.max(1) as usize).max(1)
}

fn date_range_text(min_day: i64, max_day: i64) -> String {
    let start = super::GanttDate::from_day_number(min_day).to_string();
    let end = super::GanttDate::from_day_number(max_day - 1).to_string();
    format!("{start} → {end}")
}

fn axis_text(width: usize) -> String {
    let mut chars = vec!['─'; width];
    if let Some(first) = chars.first_mut() {
        *first = '┬';
    }
    if let Some(last) = chars.last_mut() {
        *last = '┬';
    }
    chars.into_iter().collect()
}

fn pad_to_width(text: &str, target_width: usize) -> String {
    let width = UnicodeWidthStr::width(text);
    if width >= target_width {
        return text.to_owned();
    }
    format!("{text}{}", " ".repeat(target_width - width))
}

fn task_bar(
    start_day: i64,
    end_day_exclusive: i64,
    min_day: i64,
    max_day: i64,
    width: usize,
    milestone: bool,
    role: GanttRenderRole,
) -> String {
    let total_days = (max_day - min_day).max(1);
    let mut chars = vec![' '; width];
    let start_col = scale_day(start_day - min_day, total_days, width).min(width - 1);
    if milestone {
        chars[start_col.min(width - 1)] = '◆';
        return chars.into_iter().collect();
    }

    let mut end_col = scale_day(end_day_exclusive - min_day, total_days, width);
    end_col = end_col.max(start_col + 1).min(width);
    for ch in chars.iter_mut().take(end_col).skip(start_col) {
        *ch = glyph_for_role(role);
    }
    chars.into_iter().collect()
}

fn scale_day(offset: i64, total_days: i64, width: usize) -> usize {
    ((offset.clamp(0, total_days) as usize) * width / total_days as usize).min(width)
}

fn glyph_for_role(role: GanttRenderRole) -> char {
    match role {
        GanttRenderRole::DoneBar => '█',
        GanttRenderRole::ActiveBar => '▓',
        GanttRenderRole::CriticalBar => '▒',
        GanttRenderRole::PendingBar => '░',
        GanttRenderRole::Milestone => '◆',
        _ => '░',
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::gantt_diagram::{GanttSection, GanttSpec, GanttTask};

    #[test]
    fn render_output_contains_labels_and_bars() {
        let spec = GanttSpec::new().title("Release").section(
            GanttSection::new("Build")
                .task(
                    GanttTask::new("Design")
                        .id("design")
                        .start_date("2026-01-01")
                        .duration_days(2)
                        .done(),
                )
                .task(GanttTask::new("Ship").after("design").milestone()),
        );
        let rows = build_gantt_render_rows(
            &spec,
            GanttRenderConfig {
                max_timeline_width: 12,
            },
        )
        .unwrap();
        let text = rows
            .rows
            .iter()
            .flat_map(|row| row.cells.iter())
            .map(|cell| cell.text.as_ref())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Release"));
        assert!(text.contains("Build"));
        assert!(text.contains("Design"));
        assert!(text.contains('█'));
        assert!(text.contains('◆'));
    }
}
