use std::sync::Arc;

use crate::widgets::gantt_diagram::timeline::resolve_gantt_spec;
use crate::widgets::gantt_diagram::{
    GanttDate, GanttDuration, GanttSection, GanttSpec, GanttTask, GanttTaskStart, GanttTaskStatus,
};

use super::super::diagram::ParsedDiagram;
use super::{MermaidParseError, parse_labelled_pair, significant_lines, strip_quotes};

const DEFAULT_SECTION: &str = "Tasks";

pub(crate) fn parse(src: &str) -> Result<ParsedDiagram, MermaidParseError> {
    let mut spec = GanttSpec::new();
    let mut current_section: Option<usize> = None;
    let mut task_count = 0usize;

    for (line_no, line) in significant_lines(src).into_iter().skip(1) {
        if let Some(rest) = line.strip_prefix("title ") {
            spec.title = Some(Arc::from(strip_quotes(rest)));
            continue;
        }
        if let Some(rest) = line.strip_prefix("dateFormat ") {
            if rest.trim() != "YYYY-MM-DD" {
                return Err(MermaidParseError::new(
                    "unsupported gantt dateFormat; expected YYYY-MM-DD",
                    Some(line_no),
                ));
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("section ") {
            spec.sections.push(GanttSection::new(strip_quotes(rest)));
            current_section = Some(spec.sections.len() - 1);
            continue;
        }
        if is_ignored_metadata(&line) {
            continue;
        }

        let task = parse_task(&line, line_no)?;
        let section_index = match current_section {
            Some(index) => index,
            None => {
                spec.sections.push(GanttSection::new(DEFAULT_SECTION));
                current_section = Some(0);
                0
            }
        };
        spec.sections[section_index].tasks.push(task);
        task_count += 1;
    }

    if task_count == 0 {
        return Err(MermaidParseError::new("empty gantt diagram", None));
    }
    resolve_gantt_spec(&spec).map_err(|err| MermaidParseError::new(err.to_string(), None))?;
    Ok(ParsedDiagram::Gantt(spec))
}

fn parse_task(line: &str, line_no: usize) -> Result<GanttTask, MermaidParseError> {
    let (label, rest) = parse_labelled_pair(line, ":", line_no)?;
    let mut parts = rest
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    let mut status = GanttTaskStatus::Pending;
    let mut milestone = false;
    while let Some(tag) = parts.first().copied().filter(|part| is_status_tag(part)) {
        apply_status_tag(tag, &mut status, &mut milestone);
        parts.remove(0);
    }

    let (id, start, duration) = match parts.as_slice() {
        [start, duration] => (None, *start, *duration),
        [id, start, duration] => (Some(*id), *start, *duration),
        _ => {
            return Err(MermaidParseError::new(
                "malformed gantt task; expected `[status tags,] [id,] <start>, <duration>`",
                Some(line_no),
            ));
        }
    };

    let start = parse_start(start, line_no)?;
    let duration = parse_duration(duration, line_no)?;
    let mut task = GanttTask::new(strip_quotes(label))
        .duration(duration)
        .status(status);
    task.start = Some(start);
    task.milestone = milestone;
    if milestone {
        task.duration = GanttDuration::days(0);
    }
    if let Some(id) = id {
        task.id = Some(Arc::from(strip_quotes(id)));
    }
    Ok(task)
}

fn parse_start(input: &str, line_no: usize) -> Result<GanttTaskStart, MermaidParseError> {
    if let Some(id) = input.strip_prefix("after ") {
        let id = strip_quotes(id).trim();
        if id.is_empty() {
            return Err(MermaidParseError::new(
                "empty gantt dependency id",
                Some(line_no),
            ));
        }
        return Ok(GanttTaskStart::After(Arc::from(id)));
    }
    GanttDate::parse_ymd(input)
        .map(GanttTaskStart::Date)
        .map_err(|err| MermaidParseError::new(err.to_string(), Some(line_no)))
}

fn parse_duration(input: &str, line_no: usize) -> Result<GanttDuration, MermaidParseError> {
    let days = input
        .strip_suffix('d')
        .ok_or_else(|| MermaidParseError::new("expected gantt duration in days", Some(line_no)))?
        .parse::<u32>()
        .map_err(|_| MermaidParseError::new("invalid gantt duration", Some(line_no)))?;
    Ok(GanttDuration::days(days))
}

fn is_status_tag(input: &str) -> bool {
    matches!(input, "crit" | "active" | "done" | "milestone")
}

fn apply_status_tag(input: &str, status: &mut GanttTaskStatus, milestone: &mut bool) {
    match input {
        "crit" => *status = GanttTaskStatus::Critical,
        "done" if *status != GanttTaskStatus::Critical => *status = GanttTaskStatus::Done,
        "active" if matches!(*status, GanttTaskStatus::Pending) => {
            *status = GanttTaskStatus::Active;
        }
        "milestone" => *milestone = true,
        _ => {}
    }
}

fn is_ignored_metadata(line: &str) -> bool {
    [
        "axisFormat",
        "tickInterval",
        "excludes",
        "todayMarker",
        "accTitle",
        "accDescr",
    ]
    .iter()
    .any(|keyword| line == *keyword || line.starts_with(&format!("{keyword} ")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_gantt_sample_with_sections_dependencies_and_milestone() {
        let parsed = parse(
            r#"gantt
title Release Plan
dateFormat YYYY-MM-DD
axisFormat %m/%d
section Build
Design :done, design, 2026-01-01, 2d
Implement :active, impl, after design, 3d
Ship :milestone, ship, after impl, 0d
"#,
        )
        .unwrap();

        let ParsedDiagram::Gantt(spec) = parsed else {
            panic!("expected gantt diagram");
        };
        assert_eq!(spec.title.as_deref(), Some("Release Plan"));
        assert_eq!(spec.sections.len(), 1);
        assert_eq!(spec.sections[0].title.as_ref(), "Build");
        assert_eq!(spec.sections[0].tasks.len(), 3);
        assert_eq!(spec.sections[0].tasks[0].status, GanttTaskStatus::Done);
        assert!(spec.sections[0].tasks[2].milestone);
    }

    #[test]
    fn rejects_missing_dependency() {
        let err = parse("gantt\nsection Build\nShip : after missing, 1d\n").unwrap_err();
        assert!(err.message.contains("missing task id"));
    }
}
