#![allow(missing_docs)]

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

use super::{GanttDate, GanttSpec, GanttTask, GanttTaskStart};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GanttError {
    InvalidDate(String),
    MissingDependency {
        task: Arc<str>,
        dependency: Arc<str>,
    },
    DuplicateTaskId(Arc<str>),
    CyclicDependency(Vec<Arc<str>>),
    UnresolvedStart(Arc<str>),
}

impl fmt::Display for GanttError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDate(date) => write!(f, "invalid Gantt date: {date}"),
            Self::MissingDependency { task, dependency } => {
                write!(f, "task '{task}' depends on missing task id '{dependency}'")
            }
            Self::DuplicateTaskId(id) => write!(f, "duplicate Gantt task id '{id}'"),
            Self::CyclicDependency(path) => {
                let path = path
                    .iter()
                    .map(|id| id.as_ref())
                    .collect::<Vec<_>>()
                    .join(" -> ");
                write!(f, "cyclic Gantt dependency: {path}")
            }
            Self::UnresolvedStart(task) => {
                write!(f, "task '{task}' must define start_date(...) or after(...)")
            }
        }
    }
}

impl std::error::Error for GanttError {}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedGanttTask {
    pub task: GanttTask,
    pub start: GanttDate,
    pub end: GanttDate,
    pub start_day: i64,
    pub end_day_exclusive: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResolvedGanttSection {
    pub title: Arc<str>,
    pub tasks: Vec<ResolvedGanttTask>,
}

impl GanttDate {
    pub fn parse_ymd(input: &str) -> Result<Self, GanttError> {
        input.parse()
    }

    pub fn add_days(self, days: i64) -> Self {
        Self::from_day_number(self.day_number() + days)
    }

    pub fn day_number(self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }

    pub fn from_day_number(days: i64) -> Self {
        let (year, month, day) = civil_from_days(days);
        Self { year, month, day }
    }
}

impl FromStr for GanttDate {
    type Err = GanttError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let mut parts = input.split('-');
        let year = parts
            .next()
            .and_then(|p| p.parse::<i32>().ok())
            .ok_or_else(|| GanttError::InvalidDate(input.to_owned()))?;
        let month = parts
            .next()
            .and_then(|p| p.parse::<u8>().ok())
            .ok_or_else(|| GanttError::InvalidDate(input.to_owned()))?;
        let day = parts
            .next()
            .and_then(|p| p.parse::<u8>().ok())
            .ok_or_else(|| GanttError::InvalidDate(input.to_owned()))?;
        if parts.next().is_some() || !is_valid_date(year, month, day) {
            return Err(GanttError::InvalidDate(input.to_owned()));
        }
        Ok(Self { year, month, day })
    }
}

impl From<&str> for GanttDate {
    fn from(value: &str) -> Self {
        value
            .parse()
            .unwrap_or_else(|_| panic!("invalid Gantt date literal '{value}'"))
    }
}

pub fn resolve_gantt_spec(spec: &GanttSpec) -> Result<Vec<ResolvedGanttSection>, GanttError> {
    let mut id_to_task = HashMap::<Arc<str>, &GanttTask>::new();
    for section in &spec.sections {
        for task in &section.tasks {
            if let Some(id) = &task.id
                && id_to_task.insert(id.clone(), task).is_some()
            {
                return Err(GanttError::DuplicateTaskId(id.clone()));
            }
        }
    }

    for section in &spec.sections {
        for task in &section.tasks {
            if let Some(GanttTaskStart::After(dependency)) = &task.start
                && !id_to_task.contains_key(dependency)
            {
                return Err(GanttError::MissingDependency {
                    task: task_name(task),
                    dependency: dependency.clone(),
                });
            }
        }
    }

    let mut resolved = HashMap::<Arc<str>, (GanttDate, GanttDate, i64, i64)>::new();
    let mut visiting = HashSet::<Arc<str>>::new();
    let mut stack = Vec::<Arc<str>>::new();
    for id in id_to_task.keys() {
        resolve_task(id, &id_to_task, &mut resolved, &mut visiting, &mut stack)?;
    }

    spec.sections
        .iter()
        .map(|section| {
            let tasks = section
                .tasks
                .iter()
                .map(|task| {
                    let range = match &task.id {
                        Some(id) => resolved[id],
                        None => resolve_anonymous_task(task, &resolved)?,
                    };
                    Ok(ResolvedGanttTask {
                        task: task.clone(),
                        start: range.0,
                        end: range.1,
                        start_day: range.2,
                        end_day_exclusive: range.3,
                    })
                })
                .collect::<Result<Vec<_>, GanttError>>()?;
            Ok(ResolvedGanttSection {
                title: section.title.clone(),
                tasks,
            })
        })
        .collect()
}

fn resolve_task(
    id: &Arc<str>,
    id_to_task: &HashMap<Arc<str>, &GanttTask>,
    resolved: &mut HashMap<Arc<str>, (GanttDate, GanttDate, i64, i64)>,
    visiting: &mut HashSet<Arc<str>>,
    stack: &mut Vec<Arc<str>>,
) -> Result<(GanttDate, GanttDate, i64, i64), GanttError> {
    if let Some(range) = resolved.get(id) {
        return Ok(*range);
    }
    if !visiting.insert(id.clone()) {
        let cycle_start = stack.iter().position(|seen| seen == id).unwrap_or(0);
        let mut path = stack[cycle_start..].to_vec();
        path.push(id.clone());
        return Err(GanttError::CyclicDependency(path));
    }
    stack.push(id.clone());

    let task = id_to_task[id];
    let start = match &task.start {
        Some(GanttTaskStart::Date(date)) => *date,
        Some(GanttTaskStart::After(dependency)) => {
            let (_, _, _, predecessor_end) =
                resolve_task(dependency, id_to_task, resolved, visiting, stack)?;
            GanttDate::from_day_number(predecessor_end)
        }
        None => return Err(GanttError::UnresolvedStart(id.clone())),
    };
    let range = task_range(task, start);
    resolved.insert(id.clone(), range);
    visiting.remove(id);
    stack.pop();
    Ok(range)
}

fn resolve_anonymous_task(
    task: &GanttTask,
    resolved: &HashMap<Arc<str>, (GanttDate, GanttDate, i64, i64)>,
) -> Result<(GanttDate, GanttDate, i64, i64), GanttError> {
    match task.start {
        Some(GanttTaskStart::Date(date)) => Ok(task_range(task, date)),
        Some(GanttTaskStart::After(ref dependency)) => {
            let (_, _, _, predecessor_end) =
                resolved
                    .get(dependency)
                    .ok_or_else(|| GanttError::MissingDependency {
                        task: task_name(task),
                        dependency: dependency.clone(),
                    })?;
            Ok(task_range(
                task,
                GanttDate::from_day_number(*predecessor_end),
            ))
        }
        None => Err(GanttError::UnresolvedStart(task_name(task))),
    }
}

fn task_range(task: &GanttTask, start: GanttDate) -> (GanttDate, GanttDate, i64, i64) {
    let start_day = start.day_number();
    let span = if task.milestone {
        1
    } else {
        i64::from(task.duration.days.max(1))
    };
    let end_day_exclusive = start_day + span;
    let end = GanttDate::from_day_number(end_day_exclusive.saturating_sub(1));
    (start, end, start_day, end_day_exclusive)
}

fn task_name(task: &GanttTask) -> Arc<str> {
    task.id.clone().unwrap_or_else(|| task.label.clone())
}

fn is_valid_date(year: i32, month: u8, day: u8) -> bool {
    (1..=12).contains(&month) && (1..=days_in_month(year, month)).contains(&day)
}

fn days_in_month(year: i32, month: u8) -> u8 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_from_civil(year: i32, month: u8, day: u8) -> i64 {
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(days: i64) -> (i32, u8, u8) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year as i32, month as u8, day as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::gantt_diagram::{GanttSection, GanttTask};

    #[test]
    fn date_roundtrip_formats_ymd() {
        let date = GanttDate::parse_ymd("2026-05-20").unwrap();
        assert_eq!(date.to_string(), "2026-05-20");
        assert_eq!(GanttDate::from_day_number(date.day_number()), date);
    }

    #[test]
    fn add_days_crosses_leap_and_month_boundaries() {
        assert_eq!(
            GanttDate::parse_ymd("2024-02-28").unwrap().add_days(1),
            GanttDate::parse_ymd("2024-02-29").unwrap()
        );
        assert_eq!(
            GanttDate::parse_ymd("2024-02-29").unwrap().add_days(1),
            GanttDate::parse_ymd("2024-03-01").unwrap()
        );
    }

    #[test]
    fn resolves_after_chain_sample() {
        let spec = GanttSpec::new().section(
            GanttSection::new("Build")
                .task(
                    GanttTask::new("Design")
                        .id("design")
                        .start_date("2026-01-01")
                        .duration_days(3),
                )
                .task(
                    GanttTask::new("Implement")
                        .id("impl")
                        .after("design")
                        .duration_days(2),
                ),
        );
        let resolved = resolve_gantt_spec(&spec).unwrap();
        assert_eq!(resolved[0].tasks[1].start.to_string(), "2026-01-04");
        assert_eq!(resolved[0].tasks[1].end.to_string(), "2026-01-05");
    }

    #[test]
    fn reports_missing_dependency() {
        let spec = GanttSpec::new()
            .section(GanttSection::new("Build").task(GanttTask::new("Implement").after("missing")));
        assert!(matches!(
            resolve_gantt_spec(&spec),
            Err(GanttError::MissingDependency { .. })
        ));
    }

    #[test]
    fn reports_cycle() {
        let spec = GanttSpec::new().section(
            GanttSection::new("Build")
                .task(GanttTask::new("A").id("a").after("b"))
                .task(GanttTask::new("B").id("b").after("a")),
        );
        assert!(matches!(
            resolve_gantt_spec(&spec),
            Err(GanttError::CyclicDependency(_))
        ));
    }
}
