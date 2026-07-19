use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use web_time::Instant;

use chrono::{DateTime, Local, Timelike, Utc};
use nucleo::pattern::{CaseMatching, Normalization};

use crate::app::FocusPolicy;
use crate::app::interaction_state::DirtyLevel;
use crate::callback::ScopeId;
use crate::core::element::Key;
use crate::core::node::NodeId;
use crate::debug::LogSource;
use crate::layout::tag::Tag;
use crate::style::Length;
use crate::text::input::TextInput;
use crate::widgets::log_view::matching::match_logs;
use crate::widgets::{LogEntry, LogLevel};

const FRAME_HISTORY_CAP: usize = 300;
const LOG_BUFFER_CAP: usize = 1000;
const FPS_WINDOW: Duration = Duration::from_secs(2);
const LOGS_TAB_INDEX: usize = 1;
const ATTRIBUTION_PENDING_CAP: usize = 16;
const ATTRIBUTION_FRAME_CAP: usize = 6;

const DEFAULT_CONFIG_PANEL_WIDTH: Length = Length::Flex(1);
const DEFAULT_CONFIG_PANEL_HEIGHT: Length = Length::Percent(30);

const DEFAULT_STATS_PANEL_WIDTH: Length = Length::Px(48);
/// 2 border rows + the fixed 13-row stats body (see `stats_body`).
const DEFAULT_STATS_PANEL_HEIGHT: Length = Length::Px(15);
/// Rolling window (in recorded frames) for stats aggregation and input pressure.
const RECENT_WINDOW_FRAMES: usize = 60;
const INPUT_PRESSURE_FRAME_BUDGET: Duration = Duration::from_millis(16);
const INPUT_PRESSURE_THRESHOLD: u32 = 8;
/// Sparkline scale floor in microseconds (one 60fps frame budget).
pub(crate) const FRAME_BUDGET_US: u64 = 16_667;
const DEFAULT_LOGS_PANEL_WIDTH: Length = Length::Flex(1);
const DEFAULT_LOGS_PANEL_HEIGHT: Length = Length::Px(26);

/// Origin of a dirty-level request recorded for DevTools frame metrics.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum UpdateSource {
    Component {
        scope: ScopeId,
        name: Arc<str>,
    },
    /// `"input:mouse"` | `"input:drag"` | `"input:scroll"` | `"input:key"`
    Input(&'static str),
}

/// One coalesced dirty request attributed to a component or input path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UpdateAttribution {
    pub(crate) source: UpdateSource,
    pub(crate) level: DirtyLevel,
    pub(crate) count: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InputPressure {
    pub(crate) offending: u32,
    pub(crate) window: u32,
}

impl InputPressure {
    pub(crate) fn should_warn(self) -> bool {
        self.offending >= INPUT_PRESSURE_THRESHOLD
    }
}

/// Aggregated exclusive `view()` time for one component scope in a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ComponentTiming {
    pub(crate) name: Arc<str>,
    pub(crate) scope: ScopeId,
    pub(crate) duration: Duration,
    pub(crate) calls: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FrameMetrics {
    pub(crate) timestamp: Instant,
    pub(crate) dirty_level: String,
    pub(crate) total_duration: Duration,
    pub(crate) reconcile_duration: Duration,
    pub(crate) draw_duration: Duration,
    pub(crate) node_count: usize,
    pub(crate) overlay_count: usize,
    pub(crate) memo_hits: u64,
    pub(crate) memo_misses: u64,
    pub(crate) memo_miss_reasons: Vec<(crate::core::nested::MemoMissReason, u32)>,
    pub(crate) attributions: Vec<UpdateAttribution>,
    pub(crate) component_timings: Vec<ComponentTiming>,
    /// True when this frame was a Full draw driven by at least one input-sourced Full attribution.
    pub(crate) input_sourced_full: bool,
}

/// Merge/dedupe/cap pending update attributions for the current frame.
///
/// Skips [`DirtyLevel::None`]. Matching `(source, level)` pairs increment
/// `count`. At most [`ATTRIBUTION_PENDING_CAP`] distinct entries are kept;
/// additional new sources are ignored once the cap is reached.
pub(crate) fn note_update_attribution(
    pending: &mut Vec<UpdateAttribution>,
    source: UpdateSource,
    level: DirtyLevel,
) {
    if matches!(level, DirtyLevel::None) {
        return;
    }
    if let Some(existing) = pending
        .iter_mut()
        .find(|entry| entry.source == source && entry.level == level)
    {
        existing.count = existing.count.saturating_add(1);
        return;
    }
    if pending.len() >= ATTRIBUTION_PENDING_CAP {
        return;
    }
    pending.push(UpdateAttribution {
        source,
        level,
        count: 1,
    });
}

fn dirty_level_sort_rank(level: DirtyLevel) -> u8 {
    match level {
        DirtyLevel::Full => 3,
        DirtyLevel::LayoutOnly => 2,
        DirtyLevel::PaintOnly => 1,
        DirtyLevel::None => 0,
    }
}

fn attribution_source_label(source: &UpdateSource) -> &str {
    match source {
        UpdateSource::Component { name, .. } => name.as_ref(),
        UpdateSource::Input(label) => label,
    }
}

/// Sort pending attributions for a recorded frame: level desc, then count desc.
/// Truncates to [`ATTRIBUTION_FRAME_CAP`] entries.
pub(crate) fn finalize_frame_attributions(
    mut pending: Vec<UpdateAttribution>,
) -> Vec<UpdateAttribution> {
    pending.sort_by(|a, b| {
        dirty_level_sort_rank(b.level)
            .cmp(&dirty_level_sort_rank(a.level))
            .then(b.count.cmp(&a.count))
    });
    pending.truncate(ATTRIBUTION_FRAME_CAP);
    pending
}

/// Stats aggregated over the last [`RECENT_WINDOW_FRAMES`] recorded frames.
///
/// The overlay renders from this window instead of the latest frame so lines
/// stay populated and readable while the app animates: per-frame data at 60fps
/// flickers in and out; a rolling window decays smoothly.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StatsWindow {
    pub(crate) frames: u32,
    pub(crate) full: u32,
    pub(crate) layout: u32,
    pub(crate) paint: u32,
    pub(crate) avg_total: Duration,
    pub(crate) max_total: Duration,
    pub(crate) avg_reconcile: Duration,
    pub(crate) avg_draw: Duration,
    pub(crate) memo_hits: u64,
    pub(crate) memo_misses: u64,
    /// Update sources merged across levels and frames, count-desc.
    pub(crate) top_sources: Vec<(String, u32)>,
    /// Memo miss reasons merged across frames, count-desc.
    pub(crate) top_miss_reasons: Vec<(crate::core::nested::MemoMissReason, u32)>,
    /// Worst single-frame view() time per component name, duration-desc.
    pub(crate) top_slow_views: Vec<(Arc<str>, Duration)>,
}

const WINDOW_TOP_SOURCES: usize = 3;
const WINDOW_TOP_MISS_REASONS: usize = 3;
const WINDOW_TOP_SLOW_VIEWS: usize = 2;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DevLogEntry {
    pub(crate) timestamp: SystemTime,
    pub(crate) message: String,
    pub(crate) source: LogSource,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct FocusMetrics {
    pub(crate) policy: FocusPolicy,
    pub(crate) node_id: Option<NodeId>,
    pub(crate) key: Option<Key>,
    pub(crate) tag: Option<Tag>,
    pub(crate) ring_len: usize,
    pub(crate) stack_depth: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DevToolsState {
    pub(crate) visible: bool,
    pub(crate) active_tab: usize,
    pub(crate) panel_width: Length,
    pub(crate) panel_height: Length,
    pub(crate) frame_history: VecDeque<FrameMetrics>,
    pub(crate) log_buffer: VecDeque<DevLogEntry>,
    pub(crate) log_entries: Arc<[LogEntry]>,
    pub(crate) log_filter: TextInput,
    pub(crate) log_auto_follow: bool,
    pub(crate) log_paused: bool,
    pub(crate) log_selected: usize,
    /// When set, framework-internal (`LogSource::Framework`) entries are hidden
    /// from the log view so host-application logs aren't drowned out.
    pub(crate) hide_framework_logs: bool,
    pub(crate) fps_samples: VecDeque<Instant>,
    pub(crate) focus: FocusMetrics,
}

impl Default for DevToolsState {
    fn default() -> Self {
        Self {
            visible: false,
            active_tab: 0,
            panel_width: DEFAULT_CONFIG_PANEL_WIDTH,
            panel_height: DEFAULT_CONFIG_PANEL_HEIGHT,
            frame_history: VecDeque::with_capacity(FRAME_HISTORY_CAP),
            log_buffer: VecDeque::with_capacity(LOG_BUFFER_CAP),
            log_entries: Arc::new([]),
            log_filter: TextInput::default(),
            log_auto_follow: true,
            log_paused: false,
            log_selected: 0,
            hide_framework_logs: false,
            fps_samples: VecDeque::new(),
            focus: FocusMetrics::default(),
        }
    }
}

impl DevToolsState {
    pub(crate) fn set_visible(&mut self, visible: bool) {
        self.visible = visible;
    }

    pub(crate) fn set_active_tab(&mut self, tab: usize) {
        self.active_tab = tab.min(LOGS_TAB_INDEX);
    }

    pub(crate) fn is_logs_tab_active(&self) -> bool {
        self.active_tab == LOGS_TAB_INDEX
    }

    pub(crate) fn resolved_panel_size(&self) -> (Length, Length) {
        let use_default_width = self.panel_width == DEFAULT_CONFIG_PANEL_WIDTH;
        let use_default_height = self.panel_height == DEFAULT_CONFIG_PANEL_HEIGHT;
        let showing_logs = self.active_tab == LOGS_TAB_INDEX;

        let width = if use_default_width {
            if showing_logs {
                DEFAULT_LOGS_PANEL_WIDTH
            } else {
                DEFAULT_STATS_PANEL_WIDTH
            }
        } else {
            self.panel_width
        };

        let height = if use_default_height {
            if showing_logs {
                DEFAULT_LOGS_PANEL_HEIGHT
            } else {
                DEFAULT_STATS_PANEL_HEIGHT
            }
        } else {
            self.panel_height
        };

        (width, height)
    }

    pub(crate) fn push_frame_metrics(&mut self, metrics: FrameMetrics) {
        let sample_ts = metrics.timestamp;
        self.frame_history.push_back(metrics);
        while self.frame_history.len() > FRAME_HISTORY_CAP {
            self.frame_history.pop_front();
        }

        self.fps_samples.push_back(sample_ts);
        self.prune_fps_samples(sample_ts);
    }

    /// Aggregate the recent frame window for the stats overlay.
    pub(crate) fn stats_window(&self) -> StatsWindow {
        let frames: Vec<&FrameMetrics> = self
            .frame_history
            .iter()
            .rev()
            .take(RECENT_WINDOW_FRAMES)
            .collect();
        let mut window = StatsWindow {
            frames: frames.len() as u32,
            ..StatsWindow::default()
        };
        if frames.is_empty() {
            return window;
        }

        let mut sources: Vec<(String, u32)> = Vec::new();
        let mut reasons: Vec<(crate::core::nested::MemoMissReason, u32)> = Vec::new();
        let mut slow: Vec<(Arc<str>, Duration)> = Vec::new();
        let mut sum_total = Duration::ZERO;
        let mut sum_reconcile = Duration::ZERO;
        let mut sum_draw = Duration::ZERO;

        for frame in &frames {
            match frame.dirty_level.as_str() {
                "full" => window.full += 1,
                "layout" => window.layout += 1,
                "paint" => window.paint += 1,
                _ => {}
            }
            sum_total += frame.total_duration;
            sum_reconcile += frame.reconcile_duration;
            sum_draw += frame.draw_duration;
            window.max_total = window.max_total.max(frame.total_duration);
            window.memo_hits += frame.memo_hits;
            window.memo_misses += frame.memo_misses;

            for attribution in &frame.attributions {
                let label = attribution_source_label(&attribution.source);
                if let Some((_, count)) = sources.iter_mut().find(|(l, _)| l == label) {
                    *count = count.saturating_add(attribution.count);
                } else {
                    sources.push((label.to_string(), attribution.count));
                }
            }
            for &(reason, count) in &frame.memo_miss_reasons {
                if let Some((_, total)) = reasons.iter_mut().find(|(r, _)| *r == reason) {
                    *total = total.saturating_add(count);
                } else {
                    reasons.push((reason, count));
                }
            }
            for timing in &frame.component_timings {
                if let Some((_, max)) = slow.iter_mut().find(|(n, _)| *n == timing.name) {
                    *max = (*max).max(timing.duration);
                } else {
                    slow.push((Arc::clone(&timing.name), timing.duration));
                }
            }
        }

        let count = frames.len() as u32;
        window.avg_total = sum_total / count;
        window.avg_reconcile = sum_reconcile / count;
        window.avg_draw = sum_draw / count;

        sources.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        sources.truncate(WINDOW_TOP_SOURCES);
        window.top_sources = sources;

        reasons.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        reasons.truncate(WINDOW_TOP_MISS_REASONS);
        window.top_miss_reasons = reasons;

        slow.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        slow.truncate(WINDOW_TOP_SLOW_VIEWS);
        window.top_slow_views = slow;

        window
    }

    /// Count recent Full frames that were both input-sourced and over budget.
    pub(crate) fn input_pressure(&self) -> InputPressure {
        let window = self.frame_history.len().min(RECENT_WINDOW_FRAMES);
        let mut offending = 0u32;
        for frame in self.frame_history.iter().rev().take(window) {
            if frame.input_sourced_full && frame.total_duration > INPUT_PRESSURE_FRAME_BUDGET {
                offending = offending.saturating_add(1);
            }
        }
        InputPressure {
            offending,
            window: window as u32,
        }
    }

    pub(crate) fn push_log_entry(&mut self, entry: DevLogEntry) {
        self.log_buffer.push_back(entry);
        while self.log_buffer.len() > LOG_BUFFER_CAP {
            self.log_buffer.pop_front();
        }
        if !self.log_paused {
            self.sync_logs();
        }
        if self.log_auto_follow && !self.log_paused {
            self.log_selected = self.filtered_log_count().saturating_sub(1);
        }
    }

    pub(crate) fn push_log_entry_hidden(&mut self, entry: DevLogEntry) {
        self.log_buffer.push_back(entry);
        while self.log_buffer.len() > LOG_BUFFER_CAP {
            self.log_buffer.pop_front();
        }
    }

    pub(crate) fn sync_logs(&mut self) {
        self.sync_log_entries();
        self.sync_log_selection();
    }

    pub(crate) fn clear_logs(&mut self) {
        self.log_buffer.clear();
        self.sync_logs();
        self.log_selected = 0;
    }

    pub(crate) fn apply_log_filter(&mut self, ev: &crate::widgets::InputEvent) {
        self.log_filter.apply(ev);
        self.sync_log_selection();
    }

    pub(crate) fn toggle_log_auto_follow(&mut self) {
        self.set_log_auto_follow(!self.log_auto_follow);
    }

    pub(crate) fn set_log_auto_follow(&mut self, auto_follow: bool) {
        self.log_auto_follow = auto_follow;
        if self.log_auto_follow && !self.log_paused {
            self.log_selected = self.filtered_log_count().saturating_sub(1);
        }
    }

    pub(crate) fn toggle_log_paused(&mut self) {
        self.log_paused = !self.log_paused;
        if !self.log_paused {
            self.sync_logs();
        }
    }

    pub(crate) fn set_log_selected(&mut self, selected: usize) {
        self.log_selected = selected;
    }

    pub(crate) fn toggle_hide_framework_logs(&mut self) {
        self.set_hide_framework_logs(!self.hide_framework_logs);
    }

    pub(crate) fn set_hide_framework_logs(&mut self, hide: bool) {
        if self.hide_framework_logs == hide {
            return;
        }
        self.hide_framework_logs = hide;
        // Rebuild the visible snapshot so the change takes effect immediately,
        // even while paused (the user explicitly asked to re-filter).
        self.sync_log_entries();
        self.sync_log_selection();
    }

    /// Text of the currently selected (filtered) log row, if any.
    ///
    /// Resolves `log_selected` through the active fuzzy filter so it matches
    /// exactly what the user sees highlighted in the log view.
    pub(crate) fn selected_log_text(&self) -> Option<String> {
        let results = match_logs(
            self.log_entries.as_ref(),
            self.log_filter.text(),
            crate::widgets::LogFilterMode::Fuzzy,
            CaseMatching::Ignore,
            Normalization::Smart,
        );
        let result = results.get(self.log_selected)?;
        self.log_entries
            .get(result.source_index)
            .map(|entry| entry.message.to_string())
    }

    pub(crate) fn log_entries(&self) -> Arc<[LogEntry]> {
        Arc::clone(&self.log_entries)
    }

    pub(crate) fn displayed_log_count(&self) -> usize {
        match_logs(
            self.log_entries.as_ref(),
            self.log_filter.text(),
            crate::widgets::LogFilterMode::Fuzzy,
            CaseMatching::Ignore,
            Normalization::Smart,
        )
        .len()
    }

    pub(crate) fn fps(&self) -> f32 {
        self.fps_samples.len() as f32 / FPS_WINDOW.as_secs_f32()
    }

    pub(crate) fn latest_frame(&self) -> Option<&FrameMetrics> {
        self.frame_history.back()
    }

    /// Return the last `width` frames as duration samples in microseconds.
    ///
    /// Each sample maps 1:1 to one sparkline column.  Pass the sparkline's
    /// actual column count so no bucket-averaging downsampling is needed.
    /// Microsecond resolution keeps sub-millisecond frames visible; whole
    /// milliseconds would round the typical frame down to an empty column.
    pub(crate) fn duration_history_us(&self, width: usize) -> Vec<u64> {
        let len = self.frame_history.len();
        let start = len.saturating_sub(width);
        self.frame_history
            .iter()
            .skip(start)
            .map(|metrics| metrics.total_duration.as_micros().min(u128::from(u64::MAX)) as u64)
            .collect()
    }

    /// Estimate the sparkline column count from the resolved panel width.
    /// Subtracts the frame border (the panel has no horizontal padding); a
    /// larger overhead leaves blank columns because `ClipStart` right-aligns
    /// the bars in the wider content area.
    pub(crate) fn sparkline_columns(&self, viewport_w: u16) -> usize {
        const FRAME_OVERHEAD: u16 = 2; // left + right border
        match self.resolved_panel_size().0 {
            Length::Px(w) => w.saturating_sub(FRAME_OVERHEAD) as usize,
            _ => viewport_w.saturating_sub(FRAME_OVERHEAD) as usize,
        }
    }

    pub(crate) fn filtered_log_count(&self) -> usize {
        self.displayed_log_count()
    }

    fn sync_log_entries(&mut self) {
        let hide_framework = self.hide_framework_logs;
        self.log_entries = self
            .log_buffer
            .iter()
            .filter(|entry| !(hide_framework && entry.source == LogSource::Framework))
            .map(|entry| {
                LogEntry::new(
                    Self::classify_log_level(&entry.message),
                    format!(
                        "{} {}",
                        Self::timestamp_label(entry.timestamp),
                        entry.message
                    ),
                )
            })
            .collect::<Vec<_>>()
            .into();
    }

    fn sync_log_selection(&mut self) {
        if self.log_auto_follow && !self.log_paused {
            self.log_selected = self.filtered_log_count().saturating_sub(1);
        } else {
            self.log_selected = self
                .log_selected
                .min(self.filtered_log_count().saturating_sub(1));
        }
    }

    fn classify_log_level(message: &str) -> LogLevel {
        let upper = message.to_ascii_uppercase();
        if upper.contains("ERROR") || upper.contains("FAIL") {
            LogLevel::Error
        } else if upper.contains("WARN") {
            LogLevel::Warn
        } else if upper.contains("TRACE") {
            LogLevel::Trace
        } else if upper.contains("INFO") {
            LogLevel::Info
        } else {
            LogLevel::Debug
        }
    }

    fn prune_fps_samples(&mut self, now: Instant) {
        let cutoff = now.checked_sub(FPS_WINDOW).unwrap_or(now);
        while let Some(ts) = self.fps_samples.front() {
            if *ts < cutoff {
                self.fps_samples.pop_front();
            } else {
                break;
            }
        }
    }

    pub(crate) fn timestamp_label(timestamp: SystemTime) -> String {
        let utc = DateTime::<Utc>::from(timestamp);
        let local = utc.with_timezone(&Local);
        format!(
            "[{:02}:{:02}:{:02}.{:03}]",
            local.hour(),
            local.minute(),
            local.second(),
            local.timestamp_subsec_millis()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_history_ring_cap_is_enforced() {
        let mut state = DevToolsState::default();
        let start = Instant::now();

        for i in 0..350 {
            state.push_frame_metrics(FrameMetrics {
                timestamp: start + Duration::from_millis(i as u64),
                dirty_level: "full".to_string(),
                total_duration: Duration::from_millis(i as u64),
                reconcile_duration: Duration::from_millis(1),
                draw_duration: Duration::from_millis(1),
                node_count: i,
                overlay_count: 0,
                memo_hits: 0,
                memo_misses: 0,
                memo_miss_reasons: Vec::new(),
                attributions: Vec::new(),
                component_timings: Vec::new(),
                input_sourced_full: false,
            });
        }

        assert_eq!(state.frame_history.len(), FRAME_HISTORY_CAP);
        assert_eq!(state.frame_history.front().map(|f| f.node_count), Some(50));
        assert_eq!(state.frame_history.back().map(|f| f.node_count), Some(349));
    }

    #[test]
    fn log_buffer_ring_cap_is_enforced() {
        let mut state = DevToolsState::default();

        for i in 0..1_200 {
            state.push_log_entry(DevLogEntry {
                timestamp: SystemTime::now(),
                source: LogSource::App,
                message: format!("log {i}"),
            });
        }

        assert_eq!(state.log_buffer.len(), LOG_BUFFER_CAP);
        assert_eq!(
            state.log_buffer.front().map(|e| e.message.as_str()),
            Some("log 200")
        );
        assert_eq!(
            state.log_buffer.back().map(|e| e.message.as_str()),
            Some("log 1199")
        );
    }

    #[test]
    fn fps_samples_keep_only_last_two_seconds() {
        let mut state = DevToolsState::default();
        let start = Instant::now();

        for i in 0..6 {
            state.push_frame_metrics(FrameMetrics {
                timestamp: start + Duration::from_millis(i * 500),
                dirty_level: "layout".to_string(),
                total_duration: Duration::from_millis(10),
                reconcile_duration: Duration::from_millis(4),
                draw_duration: Duration::from_millis(5),
                node_count: 1,
                overlay_count: 0,
                memo_hits: 0,
                memo_misses: 0,
                memo_miss_reasons: Vec::new(),
                attributions: Vec::new(),
                component_timings: Vec::new(),
                input_sourced_full: false,
            });
        }

        assert_eq!(state.fps_samples.len(), 5);
        assert!((state.fps() - 2.5).abs() < f32::EPSILON);
    }

    #[test]
    fn auto_follow_selects_tail_when_disabled() {
        let mut state = DevToolsState::default();
        for i in 0..5 {
            state.push_log_entry(DevLogEntry {
                timestamp: SystemTime::now(),
                source: LogSource::App,
                message: format!("log {i}"),
            });
        }

        assert_eq!(state.log_selected, 4);

        state.set_log_auto_follow(false);
        state.set_log_selected(1);
        assert_eq!(state.log_selected, 1);

        state.toggle_log_auto_follow();
        assert!(state.log_auto_follow);
        assert_eq!(state.log_selected, 4);

        state.toggle_log_auto_follow();
        assert!(!state.log_auto_follow);
        assert_eq!(state.log_selected, 4);
    }

    #[test]
    fn filter_matching_is_case_insensitive() {
        let mut state = DevToolsState::default();
        state.push_log_entry(DevLogEntry {
            timestamp: SystemTime::now(),
            source: LogSource::App,
            message: "Render tick: Layout pass".to_string(),
        });

        state.log_filter.set_text("render".to_string());
        assert_eq!(state.displayed_log_count(), 1);

        state.log_filter.set_text("LAYOUT".to_string());
        assert_eq!(state.displayed_log_count(), 1);

        state.log_filter.set_text("network".to_string());
        assert_eq!(state.displayed_log_count(), 0);
    }

    #[test]
    fn paused_logs_freeze_visible_snapshot_until_resumed() {
        let mut state = DevToolsState::default();
        state.push_log_entry(DevLogEntry {
            timestamp: SystemTime::now(),
            source: LogSource::App,
            message: "one".to_string(),
        });
        state.toggle_log_paused();
        state.push_log_entry(DevLogEntry {
            timestamp: SystemTime::now(),
            source: LogSource::App,
            message: "two".to_string(),
        });

        assert!(state.log_paused);
        assert_eq!(state.log_entries.len(), 1);
        assert_eq!(state.log_buffer.len(), 2);

        state.toggle_log_paused();

        assert!(!state.log_paused);
        assert_eq!(state.log_entries.len(), 2);
    }

    #[test]
    fn hidden_push_only_updates_ring_buffer_without_snapshot_rebuild() {
        let mut state = DevToolsState::default();
        state.push_log_entry(DevLogEntry {
            timestamp: SystemTime::now(),
            source: LogSource::App,
            message: "visible".to_string(),
        });
        let snapshot = state.log_entries();

        state.push_log_entry_hidden(DevLogEntry {
            timestamp: SystemTime::now(),
            source: LogSource::App,
            message: "hidden".to_string(),
        });

        assert_eq!(state.log_buffer.len(), 2);
        assert_eq!(state.log_entries.len(), 1);
        assert!(
            Arc::ptr_eq(&snapshot, &state.log_entries()),
            "hidden push should not rebuild visible snapshot"
        );
    }

    #[test]
    fn sync_logs_rebuilds_snapshot_and_selection_for_hidden_ingest() {
        let mut state = DevToolsState::default();
        state.set_log_auto_follow(true);

        state.push_log_entry_hidden(DevLogEntry {
            timestamp: SystemTime::now(),
            source: LogSource::App,
            message: "hidden one".to_string(),
        });
        state.push_log_entry_hidden(DevLogEntry {
            timestamp: SystemTime::now(),
            source: LogSource::App,
            message: "hidden two".to_string(),
        });

        assert_eq!(state.log_entries.len(), 0);
        state.sync_logs();

        assert_eq!(state.log_entries.len(), 2);
        assert_eq!(state.log_selected, 1);
    }

    #[test]
    fn default_panel_size_matches_app_defaults() {
        let state = DevToolsState::default();

        assert_eq!(state.panel_width, Length::Flex(1));
        assert_eq!(state.panel_height, Length::Percent(30));
    }

    #[test]
    fn resolved_panel_size_uses_compact_stats_defaults() {
        let state = DevToolsState::default();

        assert_eq!(
            state.resolved_panel_size(),
            (Length::Px(48), Length::Px(15))
        );
    }

    #[test]
    fn resolved_panel_size_uses_full_width_logs_defaults() {
        let mut state = DevToolsState::default();
        state.set_active_tab(LOGS_TAB_INDEX);

        assert_eq!(
            state.resolved_panel_size(),
            (Length::Flex(1), Length::Px(26))
        );
    }

    #[test]
    fn resolved_panel_size_preserves_explicit_overrides() {
        let mut state = DevToolsState {
            panel_width: Length::Px(72),
            panel_height: Length::Px(12),
            ..DevToolsState::default()
        };
        state.set_active_tab(LOGS_TAB_INDEX);

        assert_eq!(
            state.resolved_panel_size(),
            (Length::Px(72), Length::Px(12))
        );
    }

    fn push(state: &mut DevToolsState, source: LogSource, message: &str) {
        state.push_log_entry(DevLogEntry {
            timestamp: SystemTime::now(),
            source,
            message: message.to_string(),
        });
    }

    #[test]
    fn hide_framework_logs_filters_only_framework_entries() {
        let mut state = DevToolsState::default();
        push(&mut state, LogSource::Framework, "[tui-lipan] event: key");
        push(&mut state, LogSource::App, "app: tick");
        push(
            &mut state,
            LogSource::Framework,
            "[tui-lipan] dirty: resize",
        );

        assert_eq!(state.log_entries.len(), 3);

        state.set_hide_framework_logs(true);
        assert_eq!(state.log_entries.len(), 1);
        assert!(state.log_entries[0].message.contains("app: tick"));

        // Buffer is untouched; toggling back restores the framework lines.
        assert_eq!(state.log_buffer.len(), 3);
        state.set_hide_framework_logs(false);
        assert_eq!(state.log_entries.len(), 3);
    }

    #[test]
    fn hide_framework_logs_applies_to_subsequent_pushes() {
        let mut state = DevToolsState::default();
        state.set_hide_framework_logs(true);

        push(&mut state, LogSource::Framework, "[tui-lipan] noise");
        push(&mut state, LogSource::App, "app: visible");

        assert_eq!(state.log_entries.len(), 1);
        assert!(state.log_entries[0].message.contains("app: visible"));
    }

    #[test]
    fn selected_log_text_resolves_through_filter() {
        let mut state = DevToolsState::default();
        push(&mut state, LogSource::App, "alpha");
        push(&mut state, LogSource::App, "beta");
        push(&mut state, LogSource::App, "gamma");

        state.set_log_auto_follow(false);
        state.set_log_selected(1);
        assert!(state.selected_log_text().unwrap().contains("beta"));

        state.log_filter.set_text("gamma".to_string());
        state.sync_logs();
        state.set_log_selected(0);
        assert!(state.selected_log_text().unwrap().contains("gamma"));
    }

    #[test]
    fn selected_log_text_is_none_when_empty() {
        let state = DevToolsState::default();
        assert!(state.selected_log_text().is_none());
    }

    #[test]
    fn note_update_attribution_merges_dedupes_and_caps() {
        let mut pending = Vec::new();
        let sidebar = UpdateSource::Component {
            scope: ScopeId(2),
            name: Arc::from("MySidebar"),
        };

        note_update_attribution(
            &mut pending,
            UpdateSource::Input("input:key"),
            DirtyLevel::None,
        );
        assert!(pending.is_empty());

        note_update_attribution(&mut pending, sidebar.clone(), DirtyLevel::Full);
        note_update_attribution(&mut pending, sidebar.clone(), DirtyLevel::Full);
        note_update_attribution(
            &mut pending,
            UpdateSource::Input("input:drag"),
            DirtyLevel::Full,
        );
        note_update_attribution(&mut pending, sidebar, DirtyLevel::LayoutOnly);

        assert_eq!(pending.len(), 3);
        assert_eq!(pending[0].count, 2);
        assert_eq!(pending[1].count, 1);
        assert_eq!(pending[1].source, UpdateSource::Input("input:drag"));
        assert_eq!(pending[2].level, DirtyLevel::LayoutOnly);
        assert_eq!(pending[2].count, 1);

        // Fill to the pending cap with unique component sources.
        for i in 0..ATTRIBUTION_PENDING_CAP {
            note_update_attribution(
                &mut pending,
                UpdateSource::Component {
                    scope: ScopeId(100 + i as u32),
                    name: Arc::from(format!("Comp{i}")),
                },
                DirtyLevel::PaintOnly,
            );
        }
        assert_eq!(pending.len(), ATTRIBUTION_PENDING_CAP);

        // New sources are ignored once full.
        note_update_attribution(
            &mut pending,
            UpdateSource::Component {
                scope: ScopeId(999),
                name: Arc::from("Overflow"),
            },
            DirtyLevel::PaintOnly,
        );
        assert_eq!(pending.len(), ATTRIBUTION_PENDING_CAP);
        assert!(pending.iter().all(|entry| entry.source
            != UpdateSource::Component {
                scope: ScopeId(999),
                name: Arc::from("Overflow"),
            }));

        // Existing entries still increment after the cap is reached.
        let drag_count_before = pending
            .iter()
            .find(|entry| {
                entry.source == UpdateSource::Input("input:drag") && entry.level == DirtyLevel::Full
            })
            .map(|entry| entry.count)
            .expect("drag attribution");
        note_update_attribution(
            &mut pending,
            UpdateSource::Input("input:drag"),
            DirtyLevel::Full,
        );
        let drag_count_after = pending
            .iter()
            .find(|entry| {
                entry.source == UpdateSource::Input("input:drag") && entry.level == DirtyLevel::Full
            })
            .map(|entry| entry.count)
            .expect("drag attribution");
        assert_eq!(drag_count_after, drag_count_before + 1);
        assert_eq!(pending.len(), ATTRIBUTION_PENDING_CAP);

        let finalized = finalize_frame_attributions(pending);
        assert!(finalized.len() <= ATTRIBUTION_FRAME_CAP);
        for window in finalized.windows(2) {
            let left = dirty_level_sort_rank(window[0].level);
            let right = dirty_level_sort_rank(window[1].level);
            assert!(left >= right);
            if left == right {
                assert!(window[0].count >= window[1].count);
            }
        }
    }

    fn sample_frame(input_sourced_full: bool, total_ms: u64) -> FrameMetrics {
        FrameMetrics {
            timestamp: Instant::now(),
            dirty_level: "full".into(),
            total_duration: Duration::from_millis(total_ms),
            reconcile_duration: Duration::from_millis(1),
            draw_duration: Duration::from_millis(1),
            node_count: 1,
            overlay_count: 0,
            memo_hits: 0,
            memo_misses: 0,
            memo_miss_reasons: Vec::new(),
            attributions: Vec::new(),
            component_timings: Vec::new(),
            input_sourced_full,
        }
    }

    #[test]
    fn stats_window_aggregates_levels_durations_and_top_lists() {
        let mut state = DevToolsState::default();

        let mut layout_frame = sample_frame(false, 2);
        layout_frame.dirty_level = "layout".into();
        layout_frame.memo_hits = 8;
        layout_frame.memo_misses = 2;
        layout_frame.attributions = vec![UpdateAttribution {
            source: UpdateSource::Input("input:scroll"),
            level: DirtyLevel::LayoutOnly,
            count: 5,
        }];
        layout_frame.memo_miss_reasons = vec![(crate::core::nested::MemoMissReason::SelfDirty, 2)];
        layout_frame.component_timings = vec![ComponentTiming {
            name: Arc::from("Sidebar"),
            scope: ScopeId(7),
            duration: Duration::from_micros(900),
            calls: 1,
        }];
        state.push_frame_metrics(layout_frame.clone());

        let mut full_frame = sample_frame(false, 6);
        full_frame.memo_hits = 2;
        full_frame.memo_misses = 3;
        full_frame.attributions = vec![
            UpdateAttribution {
                source: UpdateSource::Input("input:scroll"),
                level: DirtyLevel::Full,
                count: 4,
            },
            UpdateAttribution {
                source: UpdateSource::Component {
                    scope: ScopeId(7),
                    name: Arc::from("Sidebar"),
                },
                level: DirtyLevel::Full,
                count: 1,
            },
        ];
        full_frame.memo_miss_reasons = vec![
            (crate::core::nested::MemoMissReason::SelfDirty, 1),
            (crate::core::nested::MemoMissReason::KeyChanged, 2),
        ];
        full_frame.component_timings = vec![ComponentTiming {
            name: Arc::from("Sidebar"),
            scope: ScopeId(7),
            duration: Duration::from_micros(1400),
            calls: 2,
        }];
        state.push_frame_metrics(full_frame);

        let window = state.stats_window();
        assert_eq!(window.frames, 2);
        assert_eq!((window.full, window.layout, window.paint), (1, 1, 0));
        assert_eq!(window.avg_total, Duration::from_millis(4));
        assert_eq!(window.max_total, Duration::from_millis(6));
        assert_eq!((window.memo_hits, window.memo_misses), (10, 5));
        // input:scroll merged across levels: 5 + 4 = 9.
        assert_eq!(
            window.top_sources,
            vec![("input:scroll".to_string(), 9), ("Sidebar".to_string(), 1)]
        );
        assert_eq!(
            window.top_miss_reasons,
            vec![
                (crate::core::nested::MemoMissReason::SelfDirty, 3),
                (crate::core::nested::MemoMissReason::KeyChanged, 2),
            ]
        );
        // Slow views keep the worst single-frame duration per name.
        assert_eq!(
            window.top_slow_views,
            vec![(Arc::from("Sidebar"), Duration::from_micros(1400))]
        );
    }

    #[test]
    fn stats_window_is_empty_defaults_without_frames() {
        let state = DevToolsState::default();
        let window = state.stats_window();
        assert_eq!(window.frames, 0);
        assert!(window.top_sources.is_empty());
        assert!(window.top_miss_reasons.is_empty());
        assert!(window.top_slow_views.is_empty());
    }

    #[test]
    fn duration_history_us_keeps_submillisecond_resolution() {
        let mut state = DevToolsState::default();
        let mut frame = sample_frame(false, 0);
        frame.total_duration = Duration::from_micros(420);
        state.push_frame_metrics(frame);
        assert_eq!(state.duration_history_us(10), vec![420]);
    }

    #[test]
    fn input_pressure_counts_only_slow_input_full_frames() {
        let mut state = DevToolsState::default();
        // cheap input-full frames do not count
        for _ in 0..10 {
            state.push_frame_metrics(sample_frame(true, 5));
        }
        assert_eq!(state.input_pressure().offending, 0);

        // non-input slow frames do not count
        for _ in 0..10 {
            state.push_frame_metrics(sample_frame(false, 30));
        }
        assert_eq!(state.input_pressure().offending, 0);

        for _ in 0..8 {
            state.push_frame_metrics(sample_frame(true, 20));
        }
        let pressure = state.input_pressure();
        assert_eq!(pressure.offending, 8);
        assert!(pressure.should_warn());
    }

    #[test]
    fn input_pressure_window_truncates_at_sixty() {
        let mut state = DevToolsState::default();
        for _ in 0..70 {
            state.push_frame_metrics(sample_frame(true, 20));
        }
        let pressure = state.input_pressure();
        assert_eq!(pressure.window, 60);
        assert_eq!(pressure.offending, 60);
    }
}
