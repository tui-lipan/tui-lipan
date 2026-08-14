use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::core::component::{Component, Context, KeyUpdate, Update};
use crate::core::element::{Element, IntoElement};
use crate::core::event::KeyCode;
use crate::style::{Align, BorderStyle, Justify, Length, Paint, ScrollbarConfig, Style};
use crate::utils::gradient::ColorGradient;
use crate::widgets::{
    BorderMergeMode, Button, ButtonVariant, Frame, FrameLabel, HStack, Input, InputEvent,
    LogFilterMode, LogView, LogViewEvent, Overflow, ScrollView, Spacer, Sparkline,
    SparklineBarsPreset, SparklineVariant, SparklineZeroPolicy, TabEdge, TabsEvent, Text, VStack,
};

use unicode_width::UnicodeWidthStr;

use super::state::DevToolsState;

pub(crate) const DEVTOOLS_KEY: &str = "devtools-panel";
const DEVTOOLS_FILTER_KEY: &str = "devtools-filter";
const DEVTOOLS_TAB_LOGS: usize = 1;
const DEVTOOLS_TAB_APP: usize = 2;
const DEVTOOLS_APP_METRICS_KEY: &str = "devtools-app-metrics";

pub(crate) struct DevToolsPanel;

#[derive(Clone, PartialEq)]
pub(crate) struct DevToolsProps {
    pub(crate) state: Rc<RefCell<DevToolsState>>,
}

pub(crate) fn panel_element(state: Rc<RefCell<DevToolsState>>) -> Element {
    crate::child(|| DevToolsPanel, DevToolsProps { state })
}

#[derive(Clone, Debug)]
pub(crate) enum DevToolsMsg {
    TabChanged(TabsEvent),
    FilterChanged(InputEvent),
    LogSelected(LogViewEvent),
    ToggleAutoFollow,
    TogglePaused,
    ToggleFrameworkLogs,
    ClearLogs,
    /// Copy the currently selected log row (Ctrl+C).
    CopySelected,
    /// Copy a specific log line (double-click / Enter on a row).
    CopyEntry(String),
}

impl Component for DevToolsPanel {
    type Message = DevToolsMsg;
    type Properties = DevToolsProps;
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn on_key(&mut self, key: crate::core::event::KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        // Ctrl+C copies the selected log row while the Logs tab is active.
        if matches!(key.code, KeyCode::Char('c') | KeyCode::Char('C'))
            && key.mods.ctrl
            && ctx.props.state.borrow().is_logs_tab_active()
        {
            ctx.link().send(DevToolsMsg::CopySelected);
            return KeyUpdate::handled(Update::none());
        }
        KeyUpdate::unhandled(Update::none())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        // Copy paths don't mutate state; resolve text, drop the borrow, then
        // hit the clipboard (which needs an immutable `ctx` borrow of its own).
        match msg {
            DevToolsMsg::CopySelected => {
                let text = ctx.props.state.borrow().selected_log_text();
                self.copy_to_clipboard(ctx, text);
                return Update::none();
            }
            DevToolsMsg::CopyEntry(text) => {
                self.copy_to_clipboard(ctx, Some(text));
                return Update::none();
            }
            _ => {}
        }

        let mut state = ctx.props.state.borrow_mut();
        match msg {
            DevToolsMsg::TabChanged(event) => {
                state.set_active_tab(event.index);
            }
            DevToolsMsg::FilterChanged(event) => {
                state.apply_log_filter(&event);
            }
            DevToolsMsg::LogSelected(event) => {
                state.set_log_auto_follow(false);
                state.set_log_selected(event.visible_index);
            }
            DevToolsMsg::ToggleAutoFollow => {
                state.toggle_log_auto_follow();
            }
            DevToolsMsg::TogglePaused => {
                state.toggle_log_paused();
            }
            DevToolsMsg::ToggleFrameworkLogs => {
                state.toggle_hide_framework_logs();
            }
            DevToolsMsg::ClearLogs => {
                state.clear_logs();
            }
            DevToolsMsg::CopySelected | DevToolsMsg::CopyEntry(_) => unreachable!(),
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let state = ctx.props.state.borrow();
        if !state.visible {
            return Spacer::new().height(Length::Px(0)).into();
        }

        let theme = ctx.theme();
        // The devtools panel floats over the app as an overlay, so it uses the
        // elevated `menu` surface — an opaque, clearly-raised background rather
        // than the lower `panel` surface that can blend into a filled app
        // background and read as transparent.
        let frame_style = fg_style(theme.primary.fg).bg(theme.surface.menu);
        let secondary_style = fg_style(theme.muted.fg.or(theme.primary.fg));
        let viewport = ctx.viewport();
        // Stats is measured here rather than in `resolved_panel_size` because
        // only the view knows how its rows format. Every content-sized tab then
        // goes through the same width policy, so Stats and App grow alike
        // instead of Stats truncating at a hardcoded width.
        let stats_rows = (state.active_tab != DEVTOOLS_TAB_LOGS
            && state.active_tab != DEVTOOLS_TAB_APP)
            .then(|| stats_rows(&state));
        let stats_width = stats_rows.as_deref().map_or(0, stats_content_width);
        let (panel_width, panel_height) =
            state.resolved_panel_size(viewport.w, viewport.h, stats_width);

        let body = match (state.active_tab, stats_rows) {
            (DEVTOOLS_TAB_LOGS, _) => logs_body(ctx, &state),
            (DEVTOOLS_TAB_APP, _) => app_body(ctx, &state, state.app_label_width(viewport.w)),
            (_, Some(rows)) => stats_body(
                ctx,
                &state,
                rows,
                DevToolsState::sparkline_columns(panel_width, viewport.w),
            ),
            (_, None) => Spacer::new().height(Length::Px(0)).into(),
        };

        VStack::new()
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .justify(Justify::End)
            .child(
                Frame::new()
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    // DevTools is painted as a separate top layer; its border must not
                    // merge with app-layer borders that happen to occupy the same cells.
                    .border_merge_mode(BorderMergeMode::Replace)
                    .style(frame_style)
                    .header_left(FrameLabel::new("DevTools").style(secondary_style))
                    .header_style(secondary_style)
                    // The panel is bottom-anchored, so tabs on the bottom border
                    // keep the same screen position no matter how tall the active
                    // tab's body is. On the top border they moved with every
                    // switch, which is what made them hard to hit.
                    .tab_titles(["Stats", "Logs", "App"])
                    .tab_edge(TabEdge::Bottom)
                    .active_tab(state.active_tab.min(DEVTOOLS_TAB_APP))
                    .on_tab_change(ctx.link().callback(DevToolsMsg::TabChanged))
                    .footer_style(secondary_style)
                    .width(panel_width)
                    .height(panel_height)
                    .child(body)
                    .key(DEVTOOLS_KEY),
            )
            .into()
    }
}

impl DevToolsPanel {
    /// Copy `text` to the clipboard and surface a toast with the outcome.
    fn copy_to_clipboard(&self, ctx: &mut Context<Self>, text: Option<String>) {
        let message = match text {
            None => "No log selected to copy",
            Some(text) => match ctx.clipboard().copy(&text) {
                Ok(()) => "Copied log line",
                Err(_) => "Clipboard write failed",
            },
        };
        ctx.toast().push(crate::widgets::Toast::new(message));
    }
}

fn fg_style(color: Option<Paint>) -> Style {
    match color {
        Some(color) => Style::new().fg(color),
        None => Style::new(),
    }
}

/// The quietest readable foreground: placeholders, captions, and key hints.
fn dim_fg(theme: &crate::style::Theme) -> Style {
    fg_style(
        theme
            .muted
            .fg
            .map(|paint| Paint::solid(paint.color().dim())),
    )
}

/// Format a duration as compact milliseconds, e.g. `0.61ms`.
fn fmt_ms(duration: std::time::Duration) -> String {
    format!("{:.2}ms", duration.as_secs_f64() * 1000.0)
}

/// Join non-empty parts with a middle-dot separator, or return `empty`.
fn dotted(parts: &[String], empty: &str) -> String {
    if parts.is_empty() {
        empty.to_string()
    } else {
        parts.join(" \u{b7} ")
    }
}

/// How loud one stats row should read. Resolved to a concrete `Style` at
/// render time; kept separate so the rows can be measured without a theme.
#[derive(Clone, Copy, PartialEq, Eq)]
enum StatsTone {
    /// The headline counters.
    Headline,
    /// A row carrying real data.
    Value,
    /// A placeholder, a caption, or a quiet "nothing to report".
    Muted,
    /// An active warning.
    Warn,
}

/// One rendered stats row, as text. The sparkline is not a row: it is drawn
/// under `Chart` and stretches to whatever width the panel resolves to.
struct StatsRow {
    /// `None` for the full-width headline.
    label: Option<&'static str>,
    value: String,
    tone: StatsTone,
}

/// Fixed-width gutter holding the label column, including its trailing space.
const STATS_LABEL_WIDTH: u16 = 8;

impl StatsRow {
    fn labeled(label: &'static str, value: String, tone: StatsTone) -> Self {
        Self {
            label: Some(label),
            value,
            tone,
        }
    }

    /// Columns this row wants, gutter included.
    fn natural_width(&self) -> u16 {
        let value = u16::try_from(UnicodeWidthStr::width(self.value.as_str())).unwrap_or(u16::MAX);
        match self.label {
            Some(_) => value.saturating_add(STATS_LABEL_WIDTH),
            None => value,
        }
    }
}

/// The stats rows: a FIXED set of 11 text rows (plus the 2-row chart) so
/// nothing appears or disappears between frames. Every section always occupies
/// its line and shows a quiet placeholder when it has no data. All values
/// aggregate over the recent frame window (`DevToolsState::stats_window`), not
/// the latest frame, so the panel stays readable while the app animates at full
/// frame rate.
///
/// Text only, no theme: the panel measures these to size itself before it has
/// anything to render them into.
///
/// `DEFAULT_STATS_PANEL_HEIGHT` must stay in sync with the row count here.
fn stats_rows(state: &DevToolsState) -> Vec<StatsRow> {
    let window = state.stats_window();
    let (node_count, overlay_count) = state
        .latest_frame()
        .map(|frame| (frame.node_count, frame.overlay_count))
        .unwrap_or((0, 0));

    // 1: headline
    let mut rows = vec![StatsRow {
        label: None,
        value: format!(
            "FPS {:.0} \u{b7} Nodes {} \u{b7} Overlays {}",
            state.fps(),
            node_count,
            overlay_count,
        ),
        tone: StatsTone::Headline,
    }];

    // 2-3: frame timing over the window
    rows.push(StatsRow::labeled(
        "Frame",
        format!(
            "avg {} \u{b7} max {}",
            fmt_ms(window.avg_total),
            fmt_ms(window.max_total),
        ),
        StatsTone::Value,
    ));
    rows.push(StatsRow::labeled(
        "Recon",
        format!(
            "avg {} \u{b7} Draw avg {}",
            fmt_ms(window.avg_reconcile),
            fmt_ms(window.avg_draw),
        ),
        StatsTone::Value,
    ));

    // 4: caption for the chart drawn underneath it
    rows.push(StatsRow::labeled(
        "Chart",
        format!("scale {:.1}ms", state.chart_scale_us() as f64 / 1000.0),
        StatsTone::Muted,
    ));

    // 5: dirty-level distribution over the window
    rows.push(StatsRow::labeled(
        "Updates",
        format!(
            "full {} \u{b7} layout {} \u{b7} paint {}",
            window.full, window.layout, window.paint,
        ),
        StatsTone::Value,
    ));

    // 6: who requested the updates
    let source_parts: Vec<String> = window
        .top_sources
        .iter()
        .map(|(label, count)| format!("{label} x{count}"))
        .collect();
    rows.push(StatsRow::labeled(
        "Why",
        dotted(&source_parts, "idle"),
        tone_for(source_parts.is_empty()),
    ));

    // 7-8: memoization over the window
    let memo_total = window.memo_hits + window.memo_misses;
    let (memo_text, memo_tone) = if memo_total == 0 {
        ("no data".to_string(), StatsTone::Muted)
    } else {
        let hit_rate = (window.memo_hits as f64 / memo_total as f64) * 100.0;
        (
            format!("{hit_rate:.0}% hit ({}/{memo_total})", window.memo_hits),
            StatsTone::Value,
        )
    };
    rows.push(StatsRow::labeled("Memo", memo_text, memo_tone));
    let miss_parts: Vec<String> = window
        .top_miss_reasons
        .iter()
        .map(|(reason, count)| {
            let label = crate::core::nested::memo_miss_reason_label(*reason);
            format!("{label} x{count}")
        })
        .collect();
    rows.push(StatsRow::labeled(
        "Miss",
        dotted(&miss_parts, "none"),
        tone_for(miss_parts.is_empty()),
    ));

    // 9: worst view() times in the window
    let slow_parts: Vec<String> = window
        .top_slow_views
        .iter()
        .map(|(name, duration)| format!("{name} {}", fmt_ms(*duration)))
        .collect();
    rows.push(StatsRow::labeled(
        "Slow",
        dotted(&slow_parts, "none"),
        tone_for(slow_parts.is_empty()),
    ));

    // 10: focus, most-specific first so truncation drops the tail
    let focus_target = match (&state.focus.tag, &state.focus.key) {
        (Some(tag), Some(key)) => format!("{tag:?} \"{}\"", key.as_ref() as &str),
        (Some(tag), None) => format!("{tag:?}"),
        (None, Some(key)) => format!("\"{}\"", key.as_ref() as &str),
        (None, None) => "none".to_string(),
    };
    rows.push(StatsRow::labeled(
        "Focus",
        format!(
            "{focus_target} \u{b7} {:?} \u{b7} r{}",
            state.focus.policy, state.focus.ring_len,
        ),
        StatsTone::Value,
    ));

    // 11: input pressure, always present; only its tone changes
    let pressure = state.input_pressure();
    if pressure.should_warn() {
        rows.push(StatsRow::labeled(
            "Input",
            format!(
                "{}/{} full frames over budget",
                pressure.offending, pressure.window,
            ),
            StatsTone::Warn,
        ));
    } else {
        rows.push(StatsRow::labeled(
            "Input",
            "ok".to_string(),
            StatsTone::Muted,
        ));
    }

    rows
}

/// A row with nothing to report reads as a placeholder, not as data.
fn tone_for(is_placeholder: bool) -> StatsTone {
    if is_placeholder {
        StatsTone::Muted
    } else {
        StatsTone::Value
    }
}

/// Widest stats row, in columns, gutter included.
fn stats_content_width(rows: &[StatsRow]) -> u16 {
    rows.iter().map(StatsRow::natural_width).max().unwrap_or(0)
}

/// Render the measured rows, with the frame-time chart under the `Chart`
/// caption.
///
/// The chart takes microsecond samples; its scale floor is one 60fps frame
/// budget so bar height reads as "fraction of budget" until a spike stretches
/// the scale. Square-root height compression keeps typical sub-millisecond
/// frames visible next to a 20ms spike; linear scaling flattens them to
/// nothing.
fn stats_body(
    ctx: &Context<DevToolsPanel>,
    state: &DevToolsState,
    text_rows: Vec<StatsRow>,
    sparkline_cols: usize,
) -> Element {
    let theme = ctx.theme();
    let primary_style = fg_style(theme.primary.fg);
    let secondary_style = fg_style(theme.muted.fg.or(theme.primary.fg));
    let dim_style = dim_fg(&theme);
    // Bold bright labels in a fixed gutter, calm values to the right: the eye
    // scans the label column, then reads across.
    let label_style = primary_style.bold();
    let style_for = |tone: StatsTone| match tone {
        StatsTone::Headline => primary_style.bold(),
        StatsTone::Value => secondary_style,
        StatsTone::Muted => dim_style,
        StatsTone::Warn => Style::default().fg(crate::style::Color::Yellow),
    };

    let scale_us = state.chart_scale_us();
    let sqrt_max = (scale_us as f64).sqrt().ceil() as u64;
    let sqrt_history: Vec<u64> = state
        .duration_history_us(sparkline_cols)
        .iter()
        .map(|&us| (us as f64).sqrt().round() as u64)
        .collect();

    let mut stack = VStack::new().height(Length::Flex(1)).gap(0);
    for row in text_rows {
        let is_chart_caption = row.label == Some("Chart");
        let value = Text::new(row.value)
            .overflow(Overflow::Ellipsis)
            .width(Length::Flex(1))
            .style(style_for(row.tone));
        stack = match row.label {
            Some(label) => stack.child(
                HStack::new()
                    .height(Length::Auto)
                    .child(
                        Text::new(label)
                            .width(Length::Px(STATS_LABEL_WIDTH))
                            .style(label_style),
                    )
                    .child(value),
            ),
            None => stack.child(value),
        };

        if is_chart_caption {
            stack = stack.child(
                Sparkline::new(sqrt_history.clone())
                    .variant(SparklineVariant::Bars)
                    .min(0)
                    .max(sqrt_max)
                    .zero_policy(SparklineZeroPolicy::MinGlyph)
                    .chart_height(2)
                    .bars_preset(SparklineBarsPreset::Blocks)
                    // Row 0 of the gradient is the TOP chart row: accent up high so
                    // spikes pop, muted at the baseline so a quiet app stays quiet.
                    .height_gradient(ColorGradient::new(
                        theme
                            .accent
                            .fg
                            .map(Paint::color)
                            .unwrap_or(theme.border_active),
                        theme
                            .muted
                            .fg
                            .or(theme.primary.fg)
                            .map(Paint::color)
                            .unwrap_or(theme.border_active),
                    ))
                    .overflow(Overflow::ClipStart)
                    .width(Length::Flex(1))
                    // Fixed 2-row area: an empty chart must not collapse and shift the
                    // rows below it when the first frame arrives.
                    .height(Length::Px(2)),
            );
        }
    }
    stack.into()
}

fn logs_body(ctx: &Context<DevToolsPanel>, state: &DevToolsState) -> Element {
    let theme = ctx.theme();
    let secondary_style = fg_style(theme.muted.fg.or(theme.primary.fg));
    let dim_style = dim_fg(&theme);
    let primary_style = fg_style(theme.primary.fg);
    let accent_style = fg_style(theme.accent.fg.or(theme.primary.fg));

    let filter_input = Input::bound(&state.log_filter)
        .placeholder("Filter logs (fuzzy)...")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .on_change(ctx.link().callback(DevToolsMsg::FilterChanged))
        .key(DEVTOOLS_FILTER_KEY);

    // Toggle chips carry their state visually: a filled dot + accent when on,
    // a hollow dot + dimmed text when off. Hover brightens, focus shows in
    // accent, so the controls read as buttons instead of plain labels.
    let chip_bg = theme.surface.element;
    let toggle = |label: &'static str, on: bool, msg: DevToolsMsg| -> Button {
        let (icon, style) = if on {
            ("\u{25cf}", accent_style.bold())
        } else {
            ("\u{25cb}", dim_style)
        };
        Button::new(label)
            .variant(ButtonVariant::Filled)
            .icon(icon)
            .style(style.bg(chip_bg))
            .hover_style(primary_style.bold().bg(chip_bg))
            .focus_style(accent_style.bold().bg(chip_bg))
            .on_click(ctx.link().callback(move |_| msg.clone()))
    };

    let controls = HStack::new()
        .height(Length::Auto)
        .align(Align::Center)
        .gap(1)
        .child(toggle(
            "Follow",
            state.log_auto_follow,
            DevToolsMsg::ToggleAutoFollow,
        ))
        .child(toggle("Pause", state.log_paused, DevToolsMsg::TogglePaused))
        .child(toggle(
            "Framework",
            !state.hide_framework_logs,
            DevToolsMsg::ToggleFrameworkLogs,
        ))
        .child(
            Button::new("Clear")
                .variant(ButtonVariant::Filled)
                .style(secondary_style.bg(chip_bg))
                .hover_style(Style::default().fg(theme.status.error).bold().bg(chip_bg))
                .focus_style(accent_style.bold().bg(chip_bg))
                .on_click(ctx.link().callback(|_| DevToolsMsg::ClearLogs)),
        )
        .child(Spacer::new().width(Length::Flex(1)))
        .child(
            Text::new(format!(
                " {} / {} lines ",
                state.displayed_log_count(),
                state.log_entries.len()
            ))
            .overflow(Overflow::Clip)
            .style(dim_style),
        );

    let log_view = LogView::new()
        .entries_arc(state.log_entries())
        .filter(state.log_filter.text())
        .filter_mode(LogFilterMode::Fuzzy)
        .case_sensitive(false)
        .show_level(true)
        .trace_style(dim_style)
        .debug_style(dim_style)
        .info_style(secondary_style)
        .warn_style(Style::default().fg(theme.status.warning).bold())
        .error_style(Style::default().fg(theme.status.error).bold())
        .auto_follow(state.log_auto_follow)
        .paused(state.log_paused)
        .selected(state.log_selected)
        .scrollbar(true)
        .scrollbar_config(ScrollbarConfig::new())
        .empty_text("No logs")
        .width(Length::Flex(1))
        .height(Length::Flex(1))
        // Copy only on double-click / Enter, not a plain selecting click.
        .activate_on_click(false)
        .on_select(ctx.link().callback(DevToolsMsg::LogSelected))
        .on_activate(ctx.link().callback(|event: LogViewEvent| {
            DevToolsMsg::CopyEntry(event.entry.message.to_string())
        }));

    VStack::new()
        .height(Length::Flex(1))
        .gap(1)
        .child(
            VStack::new()
                .height(Length::Auto)
                .child(filter_input)
                .child(controls),
        )
        .child(log_view)
        .into()
}

fn app_body(ctx: &Context<DevToolsPanel>, state: &DevToolsState, label_width: u16) -> Element {
    let theme = ctx.theme();
    let label_style = fg_style(theme.primary.fg).bold();
    let value_style = fg_style(theme.muted.fg.or(theme.primary.fg));
    let dim_style = dim_fg(&theme);
    let metrics = state.app_metrics.rows.borrow();
    let mut rows = Vec::with_capacity(metrics.len().max(2));

    if metrics.is_empty() {
        // A `Metrics | none` row read as a metric actually named "Metrics".
        // The empty state is prose instead, and names the call that fills it.
        rows.push(
            Text::new("No app metrics registered.")
                .overflow(Overflow::Ellipsis)
                .width(Length::Flex(1))
                .style(value_style)
                .into(),
        );
        rows.push(
            Text::new("Call ctx.set_devtools_metrics() to add rows.")
                .overflow(Overflow::Ellipsis)
                .width(Length::Flex(1))
                .style(dim_style)
                .into(),
        );
    } else {
        rows.extend(metrics.iter().map(|metric| {
            HStack::new()
                .height(Length::Auto)
                .gap(1)
                .child(
                    Text::new(Arc::clone(&metric.label))
                        .overflow(Overflow::Ellipsis)
                        .width(Length::Px(label_width))
                        .style(label_style),
                )
                .child(
                    Text::new(Arc::clone(&metric.value))
                        .overflow(Overflow::Ellipsis)
                        .width(Length::Flex(1))
                        .style(value_style),
                )
                .into()
        }));
    }

    // Deliberately not focusable: the App tab is a read-out, and DevTools is an
    // inspector layered over the app rather than something to tab into. Taking
    // focus on a click would pull it off whatever the app had focused, which is
    // often the very thing being inspected.
    //
    // Reaching the overflowed rows therefore goes through the two paths that
    // need no focus: the wheel (dispatched by hit test) and ambient PageUp /
    // PageDown. Ambient scroll is a last-resort fallback - it runs only after
    // widget, bubble, command, and framework dispatch all decline the key, and
    // only when this pane can actually move in that direction - so the host
    // app keeps first claim on its own page keys.
    ScrollView::new()
        .height(Length::Flex(1))
        .scrollbar(true)
        .scrollbar_config(ScrollbarConfig::new())
        .focusable(false)
        .ambient_page_scroll(true)
        .estimated_child_height(1)
        .children(rows)
        .key(DEVTOOLS_APP_METRICS_KEY)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Color, Rect, Theme};
    use crate::test_backend::TestBackend;

    /// Seed a busy-app state: a filled frame window with mixed dirty levels,
    /// attributions, memo misses, slow views, spikes, and a focused input.
    #[cfg(feature = "devtools")]
    fn busy_state() -> DevToolsState {
        use crate::app::interaction_state::DirtyLevel;
        use crate::callback::ScopeId;
        use crate::core::nested::MemoMissReason;
        use crate::devtools::state::{
            ComponentTiming, FrameMetrics, UpdateAttribution, UpdateSource,
        };
        use std::time::Duration;
        use web_time::Instant;

        let mut state = DevToolsState::default();
        state.set_visible(true);
        state.focus.policy = crate::app::FocusPolicy::Auto;
        state.focus.tag = Some(crate::layout::tag::Tag::TextArea);
        state.focus.key = Some("search-input".into());
        state.focus.ring_len = 4;
        state.focus.stack_depth = 1;

        for i in 0..70u64 {
            let spike = i == 30 || i == 52;
            let full = i % 6 == 0;
            let mut attributions = vec![UpdateAttribution {
                source: UpdateSource::Input("input:scroll"),
                level: DirtyLevel::LayoutOnly,
                count: 2,
            }];
            let mut memo_miss_reasons = Vec::new();
            let mut component_timings = Vec::new();
            if full {
                attributions.push(UpdateAttribution {
                    source: UpdateSource::Component {
                        scope: ScopeId(7),
                        name: "Sidebar".into(),
                    },
                    level: DirtyLevel::Full,
                    count: 1,
                });
                memo_miss_reasons.push((MemoMissReason::SelfDirty, 3));
                memo_miss_reasons.push((
                    MemoMissReason::DependencyChanged(
                        crate::core::nested::MemoDependencyKind::Focus,
                    ),
                    1,
                ));
                component_timings.push(ComponentTiming {
                    name: "Sidebar".into(),
                    scope: ScopeId(7),
                    duration: Duration::from_micros(if spike { 14_200 } else { 1_180 }),
                    calls: 1,
                });
                component_timings.push(ComponentTiming {
                    name: "DiffTable".into(),
                    scope: ScopeId(9),
                    duration: Duration::from_micros(640),
                    calls: 1,
                });
            }
            state.push_frame_metrics(FrameMetrics {
                timestamp: Instant::now(),
                dirty_level: if full { "full" } else { "layout" }.into(),
                total_duration: Duration::from_micros(if spike {
                    21_400
                } else {
                    380 + (i % 7) * 130
                }),
                reconcile_duration: Duration::from_micros(210),
                draw_duration: Duration::from_micros(160),
                node_count: 47,
                overlay_count: 1,
                memo_hits: 9,
                memo_misses: if full { 4 } else { 0 },
                memo_miss_reasons,
                attributions,
                component_timings,
                input_sourced_full: spike,
            });
        }
        state
    }

    /// Visual review harness: renders the seeded stats panel and exports
    /// markdown (+ PNG with `ui-snapshot-png`) when `DEVTOOLS_SNAPSHOT_DIR`
    /// is set. Always asserts the stable row labels are present.
    #[test]
    fn stats_panel_renders_stable_rows() {
        let props = DevToolsProps {
            state: Rc::new(RefCell::new(busy_state())),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 52,
            h: 17,
        });
        backend.render();

        let snapshot =
            backend.capture_ui_snapshot_with_margin(4, 2, &crate::UiSnapshotOptions::default());
        let markdown = snapshot.to_markdown();
        if let Ok(dir) = std::env::var("DEVTOOLS_SNAPSHOT_DIR") {
            let _ = std::fs::write(format!("{dir}/devtools-stats.md"), &markdown);
            #[cfg(feature = "ui-snapshot-png")]
            let _ = std::fs::write(
                format!("{dir}/devtools-stats.png"),
                snapshot.to_png_default().unwrap_or_default(),
            );
        }

        for label in [
            "FPS", "Frame", "Recon", "Chart", "Updates", "Why", "Memo", "Miss", "Slow", "Focus",
            "Input",
        ] {
            assert!(
                markdown.contains(label),
                "stats panel should always render the `{label}` row; got:\n{markdown}"
            );
        }
    }

    /// Stats is content-sized by the same policy as the App tab: a row too wide
    /// for the shared minimum grows the panel instead of truncating in place.
    #[test]
    fn stats_panel_grows_for_a_row_wider_than_the_minimum() {
        let mut state = busy_state();
        // A realistic long focus target, the row that overflowed at the old
        // hardcoded 48 columns.
        state.focus.key = Some("rozi-terminal-1-left-pane".into());
        state.focus.policy = crate::app::FocusPolicy::OnDemand;

        let props = DevToolsProps {
            state: Rc::new(RefCell::new(state)),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 20,
        });
        backend.render();

        let snapshot =
            backend.capture_ui_snapshot_with_margin(4, 2, &crate::UiSnapshotOptions::default());
        let markdown = snapshot.to_markdown();
        if let Ok(dir) = std::env::var("DEVTOOLS_SNAPSHOT_DIR") {
            let _ = std::fs::write(format!("{dir}/devtools-stats-wide.md"), &markdown);
            #[cfg(feature = "ui-snapshot-png")]
            let _ = std::fs::write(
                format!("{dir}/devtools-stats-wide.png"),
                snapshot.to_png_default().unwrap_or_default(),
            );
        }

        let panel = panel_frame_rect(&backend);
        assert!(
            panel.w > 48,
            "a row wider than the shared minimum should grow the panel, got {}",
            panel.w
        );
        assert!(panel.w <= 100, "and never exceed the viewport");
        assert!(
            markdown.contains("rozi-terminal-1"),
            "the focus target should render in full; got:\n{markdown}"
        );
    }

    /// Visual review harness for the Logs tab: seeded entries, one selected,
    /// exports PNG via `DEVTOOLS_SNAPSHOT_DIR`. Asserts the control chips.
    #[test]
    fn logs_panel_renders_controls_and_entries() {
        use crate::debug::LogSource;
        use crate::devtools::state::DevLogEntry;
        use std::time::SystemTime;

        let mut state = DevToolsState::default();
        state.set_visible(true);
        state.set_active_tab(1);
        for (i, message) in [
            "Warning: OPENCODE_SERVER_PASSWORD is not set; server is unsecured.",
            "error: failed to reach update server (retrying in 30s)",
            "opencode server listening on http://127.0.0.1:40155",
            "session restored: 3 tabs, 14 panes",
            "watcher: 412 files under /src",
        ]
        .iter()
        .enumerate()
        {
            state.push_log_entry(DevLogEntry {
                timestamp: SystemTime::now(),
                message: (*message).to_string(),
                source: if i == 3 {
                    LogSource::Framework
                } else {
                    LogSource::App
                },
            });
        }
        state.set_log_auto_follow(false);
        state.set_log_selected(1);

        let props = DevToolsProps {
            state: Rc::new(RefCell::new(state)),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 28,
        });
        backend.render();
        // LogView fills its row cache from an async filter command spawned in
        // init(); pump until the results land, then render the real rows.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        while std::time::Instant::now() < deadline {
            let _ = backend.pump();
            backend.render();
            if backend
                .capture_ui_snapshot()
                .to_markdown()
                .contains("opencode server listening")
            {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        let snapshot =
            backend.capture_ui_snapshot_with_margin(4, 2, &crate::UiSnapshotOptions::default());
        let markdown = snapshot.to_markdown();
        if let Ok(dir) = std::env::var("DEVTOOLS_SNAPSHOT_DIR") {
            let _ = std::fs::write(format!("{dir}/devtools-logs.md"), &markdown);
            #[cfg(feature = "ui-snapshot-png")]
            let _ = std::fs::write(
                format!("{dir}/devtools-logs.png"),
                snapshot.to_png_default().unwrap_or_default(),
            );
        }

        for label in ["Follow", "Pause", "Framework", "Clear", "lines"] {
            assert!(
                markdown.contains(label),
                "logs panel should render the `{label}` control; got:\n{markdown}"
            );
        }
        assert!(markdown.contains("opencode server listening"));
    }

    /// The stats body must render the same row set with zero data: stable
    /// layout is the anti-flicker guarantee.
    #[test]
    fn stats_panel_renders_same_rows_with_no_frames() {
        let mut state = DevToolsState::default();
        state.set_visible(true);
        let props = DevToolsProps {
            state: Rc::new(RefCell::new(state)),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 52,
            h: 17,
        });
        backend.render();
        let snapshot = backend.capture_ui_snapshot();
        let markdown = snapshot.to_markdown();
        #[cfg(feature = "ui-snapshot-png")]
        if let Ok(dir) = std::env::var("DEVTOOLS_SNAPSHOT_DIR") {
            let _ = std::fs::write(
                format!("{dir}/devtools-stats-empty.png"),
                snapshot.to_png_default().unwrap_or_default(),
            );
        }
        for label in [
            "FPS", "Frame", "Recon", "Chart", "Updates", "Why", "Memo", "Miss", "Slow", "Focus",
            "Input",
        ] {
            assert!(
                markdown.contains(label),
                "empty stats panel should still render the `{label}` row"
            );
        }
        assert!(
            markdown.contains("idle"),
            "Why row should show idle placeholder"
        );
        assert!(
            markdown.contains("none"),
            "Miss/Slow rows should show none placeholder"
        );
    }

    /// Visual review harness for the App tab: host metrics plus the shared
    /// bottom tab strip, exported via `DEVTOOLS_SNAPSHOT_DIR`.
    #[test]
    fn app_panel_renders_host_metrics() {
        let mut state = DevToolsState::default();
        state.set_visible(true);
        state.set_active_tab(DEVTOOLS_TAB_APP);
        state.app_metrics.rows.borrow_mut().extend([
            crate::DevToolsMetric::new("Panes", "4 (2 split, 2 stacked)"),
            crate::DevToolsMetric::new("Clients", "2"),
            crate::DevToolsMetric::new("Queue", "idle"),
            crate::DevToolsMetric::new("Session", "restored 00:03:11 ago"),
        ]);
        let props = DevToolsProps {
            state: Rc::new(RefCell::new(state)),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 12,
        });
        backend.render();

        let snapshot =
            backend.capture_ui_snapshot_with_margin(4, 2, &crate::UiSnapshotOptions::default());
        let markdown = snapshot.to_markdown();
        if let Ok(dir) = std::env::var("DEVTOOLS_SNAPSHOT_DIR") {
            let _ = std::fs::write(format!("{dir}/devtools-app.md"), &markdown);
            #[cfg(feature = "ui-snapshot-png")]
            let _ = std::fs::write(
                format!("{dir}/devtools-app.png"),
                snapshot.to_png_default().unwrap_or_default(),
            );
        }

        for label in [
            "Panes", "Clients", "Queue", "Session", "Stats", "Logs", "App",
        ] {
            assert!(
                markdown.contains(label),
                "app panel should render `{label}`; got:\n{markdown}"
            );
        }
    }

    /// The App tab with no host metrics must still fill the shared minimum
    /// panel width, so the bottom tab strip never truncates.
    #[test]
    fn app_panel_empty_state_keeps_the_shared_panel_width() {
        let mut state = DevToolsState::default();
        state.set_visible(true);
        state.set_active_tab(DEVTOOLS_TAB_APP);
        let props = DevToolsProps {
            state: Rc::new(RefCell::new(state)),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 12,
        });
        backend.render();

        let snapshot =
            backend.capture_ui_snapshot_with_margin(4, 2, &crate::UiSnapshotOptions::default());
        let markdown = snapshot.to_markdown();
        if let Ok(dir) = std::env::var("DEVTOOLS_SNAPSHOT_DIR") {
            let _ = std::fs::write(format!("{dir}/devtools-app-empty.md"), &markdown);
            #[cfg(feature = "ui-snapshot-png")]
            let _ = std::fs::write(
                format!("{dir}/devtools-app-empty.png"),
                snapshot.to_png_default().unwrap_or_default(),
            );
        }

        let panel = panel_frame_rect(&backend);
        assert_eq!(panel.w, 48, "empty App tab should use the shared minimum");
        assert!(
            markdown.contains("set_devtools_metrics"),
            "empty state should name the call that fills it; got:\n{markdown}"
        );
        for label in ["Stats", "Logs", "App"] {
            assert!(
                markdown.contains(label),
                "bottom tab strip should render `{label}` untruncated; got:\n{markdown}"
            );
        }
    }

    /// The tab strip is bottom-anchored, so it must land on the same row for
    /// every tab regardless of how tall that tab's body is. This is the whole
    /// point of moving it out of the top border.
    #[test]
    fn tab_strip_stays_on_the_same_row_across_tabs() {
        let viewport = Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 30,
        };

        let tab_row = |tab: usize| -> i16 {
            let mut state = busy_state();
            state.set_active_tab(tab);
            state.app_metrics.rows.borrow_mut().extend([
                crate::DevToolsMetric::new("Panes", "4"),
                crate::DevToolsMetric::new("Clients", "2"),
            ]);
            let props = DevToolsProps {
                state: Rc::new(RefCell::new(state)),
            };
            let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
            backend.set_viewport(viewport);
            backend.render();

            let panel = panel_frame_rect(&backend);
            // The strip rides the bottom border itself.
            panel.y + i16::try_from(panel.h).unwrap_or(i16::MAX) - 1
        };

        let stats = tab_row(0);
        assert_eq!(tab_row(DEVTOOLS_TAB_LOGS), stats);
        assert_eq!(tab_row(DEVTOOLS_TAB_APP), stats);
        assert_eq!(
            stats,
            i16::try_from(viewport.h).unwrap() - 1,
            "the strip should sit on the panel's bottom border"
        );
    }

    /// Clicking a bottom-border tab must switch tabs: the strip lives on the
    /// frame's footer line now, so hit testing has to resolve there.
    #[test]
    fn clicking_a_footer_tab_switches_tabs() {
        use crate::core::event::{MouseButton, MouseEvent, MouseKind};

        let state = Rc::new(RefCell::new({
            let mut state = DevToolsState::default();
            state.set_visible(true);
            state
        }));
        let props = DevToolsProps {
            state: Rc::clone(&state),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 20,
        });
        backend.render();

        let panel = panel_frame_rect(&backend);
        let tab_row = panel.y + i16::try_from(panel.h).unwrap() - 1;
        // "[ Stats ]| Logs | App" - land inside the "Logs" label.
        let logs_col = panel.x + 12;

        backend
            .send_mouse(MouseEvent {
                kind: MouseKind::Down(MouseButton::Left),
                x: u16::try_from(logs_col).unwrap(),
                y: u16::try_from(tab_row).unwrap(),
                mods: Default::default(),
            })
            .expect("mouse down");
        backend
            .send_mouse(MouseEvent {
                kind: MouseKind::Up(MouseButton::Left),
                x: u16::try_from(logs_col).unwrap(),
                y: u16::try_from(tab_row).unwrap(),
                mods: Default::default(),
            })
            .expect("mouse up");
        let _ = backend.pump();

        assert_eq!(
            state.borrow().active_tab,
            DEVTOOLS_TAB_LOGS,
            "clicking the footer `Logs` tab should activate it"
        );
    }

    /// Only the Logs tab owns focusable controls. Clicking the Stats or App
    /// body must leave the app's focus where it was: DevTools is an inspector
    /// layered over the app, not something to tab into.
    #[test]
    fn only_the_logs_tab_takes_focus_from_a_body_click() {
        use crate::core::event::{MouseButton, MouseEvent, MouseKind};

        let viewport = Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 20,
        };

        let focus_after_body_click = |tab: usize| -> Option<crate::core::node::NodeId> {
            let mut state = DevToolsState::default();
            state.set_visible(true);
            state.set_active_tab(tab);
            state.app_metrics.rows.borrow_mut().extend([
                crate::DevToolsMetric::new("Panes", "4"),
                crate::DevToolsMetric::new("Clients", "2"),
            ]);
            let props = DevToolsProps {
                state: Rc::new(RefCell::new(state)),
            };
            let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
            backend.set_viewport(viewport);
            backend.render();

            let panel = panel_frame_rect(&backend);
            // First body row, one column inside the left border.
            let (x, y) = (panel.x + 1, panel.y + 1);
            for kind in [
                MouseKind::Down(MouseButton::Left),
                MouseKind::Up(MouseButton::Left),
            ] {
                backend
                    .send_mouse(MouseEvent {
                        kind,
                        x: u16::try_from(x).unwrap(),
                        y: u16::try_from(y).unwrap(),
                        mods: Default::default(),
                    })
                    .expect("mouse event");
            }
            let _ = backend.pump();
            backend.focused()
        };

        assert!(
            focus_after_body_click(0).is_none(),
            "the Stats body has nothing to focus"
        );
        assert!(
            focus_after_body_click(DEVTOOLS_TAB_APP).is_none(),
            "the App body is a read-out, so clicking it must not grab focus"
        );
        assert!(
            focus_after_body_click(DEVTOOLS_TAB_LOGS).is_some(),
            "the Logs tab does own focusable controls"
        );
    }

    /// The App pane is unfocusable, so PageUp / PageDown reach it through the
    /// ambient fallback. That fallback also has to stay polite: it declines the
    /// key when the pane cannot move, leaving it for the host app.
    #[test]
    fn ambient_page_keys_scroll_the_unfocusable_app_pane() {
        use crate::core::event::{KeyEvent, KeyMods};

        let mut state = DevToolsState::default();
        state.set_visible(true);
        state.set_active_tab(DEVTOOLS_TAB_APP);
        state.app_metrics.rows.borrow_mut().extend(
            (0..40).map(|i| crate::DevToolsMetric::new(format!("Metric{i}"), i.to_string())),
        );
        let props = DevToolsProps {
            state: Rc::new(RefCell::new(state)),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 12,
        });
        backend.render();

        let offset = |backend: &TestBackend<DevToolsPanel>| -> usize {
            backend
                .core
                .tree
                .iter()
                .find_map(|node| match &node.kind {
                    crate::core::node::NodeKind::ScrollView(scroll)
                        if node
                            .key
                            .as_ref()
                            .is_some_and(|key| key.as_ref() == DEVTOOLS_APP_METRICS_KEY) =>
                    {
                        Some(scroll.offset)
                    }
                    _ => None,
                })
                .expect("app metrics ScrollView")
        };
        let page = |code| KeyEvent {
            code,
            mods: KeyMods::default(),
        };

        assert_eq!(offset(&backend), 0);
        // Nothing is focused, so this can only land via the ambient fallback.
        assert!(backend.focused().is_none());
        assert!(
            backend
                .send_key(page(KeyCode::PageDown))
                .expect("page down"),
            "PageDown should reach the unfocused pane"
        );
        backend.render();
        let scrolled = offset(&backend);
        assert!(scrolled > 0, "PageDown should advance the pane");

        assert!(backend.send_key(page(KeyCode::PageUp)).expect("page up"));
        backend.render();
        assert!(offset(&backend) < scrolled, "PageUp should walk it back");

        // At the top the pane cannot move, so the key is left for the app.
        assert_eq!(offset(&backend), 0);
        assert!(
            !backend
                .send_key(page(KeyCode::PageUp))
                .expect("page up at top"),
            "a pane already at the top must not swallow PageUp"
        );
    }

    /// Rect of the DevTools panel frame in a rendered backend.
    fn panel_frame_rect(backend: &TestBackend<DevToolsPanel>) -> Rect {
        backend
            .core
            .tree
            .iter()
            .find(|node| {
                node.key
                    .as_ref()
                    .is_some_and(|key| key.as_ref() == DEVTOOLS_KEY)
            })
            .expect("devtools panel frame")
            .rect
    }

    #[test]
    fn app_panel_renders_metrics_in_host_order() {
        let mut state = DevToolsState::default();
        state.set_visible(true);
        state.set_active_tab(DEVTOOLS_TAB_APP);
        state.app_metrics.rows.borrow_mut().extend([
            crate::DevToolsMetric::new("Panes", "4"),
            crate::DevToolsMetric::new("Clients", "2"),
            crate::DevToolsMetric::new("Queue", "idle"),
        ]);
        let props = DevToolsProps {
            state: Rc::new(RefCell::new(state)),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 52,
            h: 12,
        });
        backend.render();

        let text = backend.capture_frame().plain_text();
        let panes = text.find("Panes").expect("Panes row");
        let clients = text.find("Clients").expect("Clients row");
        let queue = text.find("Queue").expect("Queue row");
        assert!(panes < clients && clients < queue);
        assert!(text.contains("4"));
        assert!(text.contains("idle"));
    }

    #[test]
    fn app_panel_uses_wide_viewport_to_render_full_values() {
        let value = "x".repeat(60);
        let mut state = DevToolsState::default();
        state.set_visible(true);
        state.set_active_tab(DEVTOOLS_TAB_APP);
        state
            .app_metrics
            .rows
            .borrow_mut()
            .push(crate::DevToolsMetric::new("Heap", value.clone()));
        let props = DevToolsProps {
            state: Rc::new(RefCell::new(state)),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 100,
            h: 8,
        });
        backend.render();

        assert!(backend.capture_frame().plain_text().contains(&value));
        let panel = backend
            .core
            .tree
            .iter()
            .find(|node| {
                node.key
                    .as_ref()
                    .is_some_and(|key| key.as_ref() == DEVTOOLS_KEY)
            })
            .expect("devtools panel frame");
        assert_eq!(panel.rect.w, 67);
    }

    #[test]
    fn app_panel_caps_to_viewport_and_scrolls_overflow_rows() {
        let mut state = DevToolsState::default();
        state.set_visible(true);
        state.set_active_tab(DEVTOOLS_TAB_APP);
        state.app_metrics.rows.borrow_mut().extend((0..24).map(|i| {
            crate::DevToolsMetric::new(
                format!("Metric{i}"),
                format!("value-{i}-{}", "x".repeat(40)),
            )
        }));
        let props = DevToolsProps {
            state: Rc::new(RefCell::new(state)),
        };
        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 32,
            h: 8,
        });
        backend.render();

        let panel = backend
            .core
            .tree
            .iter()
            .find(|node| {
                node.key
                    .as_ref()
                    .is_some_and(|key| key.as_ref() == DEVTOOLS_KEY)
            })
            .expect("devtools panel frame");
        assert_eq!(panel.rect.w, 32);

        let scroll = backend
            .core
            .tree
            .iter()
            .find_map(|node| match &node.kind {
                crate::core::node::NodeKind::ScrollView(scroll)
                    if node
                        .key
                        .as_ref()
                        .is_some_and(|key| key.as_ref() == DEVTOOLS_APP_METRICS_KEY) =>
                {
                    Some(scroll)
                }
                _ => None,
            })
            .expect("app metrics ScrollView");
        assert!(scroll.scrollbar);
        assert!(scroll.content_height > scroll.viewport_height);
        assert!(scroll.max_offset > 0);

        // The pane is not focusable, so the wheel is the only way to reach the
        // overflowed rows. Wheel dispatch resolves by hit test, not focus.
        let body = Rect {
            x: panel.rect.x + 1,
            y: panel.rect.y + 1,
            w: 1,
            h: 1,
        };
        backend
            .send_mouse(crate::core::event::MouseEvent {
                kind: crate::core::event::MouseKind::ScrollDown,
                x: u16::try_from(body.x).unwrap(),
                y: u16::try_from(body.y).unwrap(),
                mods: Default::default(),
            })
            .expect("wheel event");
        let _ = backend.pump();
        backend.render();

        let offset = backend
            .core
            .tree
            .iter()
            .find_map(|node| match &node.kind {
                crate::core::node::NodeKind::ScrollView(scroll)
                    if node
                        .key
                        .as_ref()
                        .is_some_and(|key| key.as_ref() == DEVTOOLS_APP_METRICS_KEY) =>
                {
                    Some(scroll.offset)
                }
                _ => None,
            })
            .expect("app metrics ScrollView");
        assert!(offset > 0, "the wheel should scroll the unfocusable pane");
        assert!(
            backend.focused().is_none(),
            "and must not focus it on the way"
        );
    }

    #[test]
    fn panel_paints_an_opaque_menu_surface_background() {
        let mut state = DevToolsState::default();
        state.set_visible(true);
        let props = DevToolsProps {
            state: Rc::new(RefCell::new(state)),
        };

        let mut backend = TestBackend::new_with_props(DevToolsPanel, props);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 20,
        });
        backend.render();

        // The panel resolves the default theme (no ThemeProvider in scope) and
        // must fill its surface with an opaque, elevated `menu` color — never a
        // transparent/`Reset` background that would show the app through it.
        let menu = Theme::default().surface.menu;
        assert_ne!(menu, Color::Reset);

        let frame = backend.capture_frame();
        assert!(
            frame.cells.iter().any(|cell| cell.bg == menu),
            "devtools panel should paint the opaque menu surface background"
        );
    }
}
