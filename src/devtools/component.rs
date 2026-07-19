use std::cell::RefCell;
use std::rc::Rc;

use crate::core::component::{Component, Context, KeyUpdate, Update};
use crate::core::element::{Element, IntoElement};
use crate::core::event::KeyCode;
use crate::style::{Align, BorderStyle, Justify, Length, Paint, ScrollbarConfig, Style};
use crate::utils::gradient::ColorGradient;
use crate::widgets::{
    Button, ButtonVariant, Frame, HStack, Input, InputEvent, LogFilterMode, LogView, LogViewEvent,
    Overflow, Spacer, Sparkline, SparklineBarsPreset, SparklineVariant, SparklineZeroPolicy,
    TabsEvent, Text, VStack,
};

use super::state::DevToolsState;

pub(crate) const DEVTOOLS_KEY: &str = "devtools-panel";
const DEVTOOLS_FILTER_KEY: &str = "devtools-filter";
const DEVTOOLS_TAB_LOGS: usize = 1;

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
    Hide,
}

impl Component for DevToolsPanel {
    type Message = DevToolsMsg;
    type Properties = DevToolsProps;
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn on_key(&mut self, key: crate::core::event::KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if matches!(key.code, KeyCode::Esc) {
            ctx.link().send(DevToolsMsg::Hide);
            return KeyUpdate::handled(Update::none());
        }
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
            DevToolsMsg::Hide => {
                state.set_visible(false);
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
        let (panel_width, panel_height) = state.resolved_panel_size();

        let body = if state.active_tab == DEVTOOLS_TAB_LOGS {
            logs_body(ctx, &state)
        } else {
            stats_body(ctx, &state)
        };

        VStack::new()
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .justify(Justify::End)
            .child(
                Frame::new()
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .tab_titles(["Stats", "Logs"])
                    .active_tab(state.active_tab.min(DEVTOOLS_TAB_LOGS))
                    .on_tab_change(ctx.link().callback(DevToolsMsg::TabChanged))
                    .style(frame_style)
                    .status_right("DevTools")
                    .status_style(secondary_style)
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

/// The stats body renders a FIXED set of 13 rows so nothing appears or
/// disappears between frames: every section always occupies its line and shows
/// a quiet placeholder when it has no data. All values aggregate over the
/// recent frame window (`DevToolsState::stats_window`), not the latest frame,
/// so the panel stays readable while the app animates at full frame rate.
///
/// `DEFAULT_STATS_PANEL_HEIGHT` must stay in sync with the row count here.
fn stats_body(ctx: &Context<DevToolsPanel>, state: &DevToolsState) -> Element {
    let theme = ctx.theme();
    let primary_style = fg_style(theme.primary.fg);
    let secondary_style = fg_style(theme.muted.fg.or(theme.primary.fg));
    let dim_style = fg_style(
        theme
            .muted
            .fg
            .map(|paint| Paint::solid(paint.color().dim())),
    );

    let window = state.stats_window();
    let (node_count, overlay_count) = state
        .latest_frame()
        .map(|frame| (frame.node_count, frame.overlay_count))
        .unwrap_or((0, 0));

    let mut rows: Vec<Element> = Vec::new();
    // Bold bright labels in a fixed 7-column gutter, calm values to the
    // right: the eye scans the label column, then reads across.
    let label_style = primary_style.bold();
    let labeled = |label: &'static str, value: String, value_style: Style| -> Element {
        HStack::new()
            .height(Length::Auto)
            .child(Text::new(label).width(Length::Px(8)).style(label_style))
            .child(
                Text::new(value)
                    .overflow(Overflow::Ellipsis)
                    .width(Length::Flex(1))
                    .style(value_style),
            )
            .into()
    };

    // 1: headline
    rows.push(
        Text::new(format!(
            "FPS {:.0} \u{b7} Nodes {} \u{b7} Overlays {}",
            state.fps(),
            node_count,
            overlay_count,
        ))
        .overflow(Overflow::Ellipsis)
        .width(Length::Flex(1))
        .style(primary_style.bold())
        .into(),
    );

    // 2-3: frame timing over the window
    rows.push(labeled(
        "Frame",
        format!(
            "avg {} \u{b7} max {}",
            fmt_ms(window.avg_total),
            fmt_ms(window.max_total),
        ),
        secondary_style,
    ));
    rows.push(labeled(
        "Recon",
        format!(
            "avg {} \u{b7} Draw avg {}",
            fmt_ms(window.avg_reconcile),
            fmt_ms(window.avg_draw),
        ),
        secondary_style,
    ));

    // 4-6: frame-time history chart with an explicit scale caption.
    // Microsecond samples; the scale floor is one 60fps frame budget so bar
    // height reads as "fraction of budget" until a spike stretches the scale.
    // Square-root height compression keeps typical sub-millisecond frames
    // visible next to a 20ms spike; linear scaling flattens them to nothing.
    let cols = state.sparkline_columns(ctx.viewport().w);
    let history = state.duration_history_us(cols);
    let scale_us = history
        .iter()
        .copied()
        .max()
        .unwrap_or(0)
        .max(crate::devtools::state::FRAME_BUDGET_US);
    rows.push(labeled(
        "Chart",
        format!("scale {:.1}ms", scale_us as f64 / 1000.0),
        dim_style,
    ));
    let sqrt_max = (scale_us as f64).sqrt().ceil() as u64;
    let sqrt_history: Vec<u64> = history
        .iter()
        .map(|&us| (us as f64).sqrt().round() as u64)
        .collect();
    rows.push(
        Sparkline::new(sqrt_history)
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
            .height(Length::Px(2))
            .into(),
    );

    // 7: dirty-level distribution over the window
    rows.push(labeled(
        "Updates",
        format!(
            "full {} \u{b7} layout {} \u{b7} paint {}",
            window.full, window.layout, window.paint,
        ),
        secondary_style,
    ));

    // 8: who requested the updates
    let source_parts: Vec<String> = window
        .top_sources
        .iter()
        .map(|(label, count)| format!("{label} x{count}"))
        .collect();
    let why_style = if source_parts.is_empty() {
        dim_style
    } else {
        secondary_style
    };
    rows.push(labeled("Why", dotted(&source_parts, "idle"), why_style));

    // 9-10: memoization over the window
    let memo_total = window.memo_hits + window.memo_misses;
    let (memo_text, memo_style) = if memo_total == 0 {
        ("no data".to_string(), dim_style)
    } else {
        let hit_rate = (window.memo_hits as f64 / memo_total as f64) * 100.0;
        (
            format!("{hit_rate:.0}% hit ({}/{memo_total})", window.memo_hits),
            secondary_style,
        )
    };
    rows.push(labeled("Memo", memo_text, memo_style));
    let miss_parts: Vec<String> = window
        .top_miss_reasons
        .iter()
        .map(|(reason, count)| {
            let label = crate::core::nested::memo_miss_reason_label(*reason);
            format!("{label} x{count}")
        })
        .collect();
    let miss_style = if miss_parts.is_empty() {
        dim_style
    } else {
        secondary_style
    };
    rows.push(labeled("Miss", dotted(&miss_parts, "none"), miss_style));

    // 11: worst view() times in the window
    let slow_parts: Vec<String> = window
        .top_slow_views
        .iter()
        .map(|(name, duration)| format!("{name} {}", fmt_ms(*duration)))
        .collect();
    let slow_style = if slow_parts.is_empty() {
        dim_style
    } else {
        secondary_style
    };
    rows.push(labeled("Slow", dotted(&slow_parts, "none"), slow_style));

    // 12: focus, most-specific first so truncation drops the tail
    let focus_target = match (&state.focus.tag, &state.focus.key) {
        (Some(tag), Some(key)) => format!("{tag:?} \"{}\"", key.as_ref() as &str),
        (Some(tag), None) => format!("{tag:?}"),
        (None, Some(key)) => format!("\"{}\"", key.as_ref() as &str),
        (None, None) => "none".to_string(),
    };
    rows.push(labeled(
        "Focus",
        format!(
            "{focus_target} \u{b7} {:?} \u{b7} r{}",
            state.focus.policy, state.focus.ring_len,
        ),
        secondary_style,
    ));

    // 13: input pressure, always present; only its style changes
    let pressure = state.input_pressure();
    if pressure.should_warn() {
        rows.push(labeled(
            "Input",
            format!(
                "{}/{} full frames over budget",
                pressure.offending, pressure.window,
            ),
            Style::default().fg(crate::style::Color::Yellow),
        ));
    } else {
        rows.push(labeled("Input", "ok".to_string(), dim_style));
    }

    let mut stack = VStack::new().height(Length::Flex(1)).gap(0);
    for row in rows {
        stack = stack.child(row);
    }
    stack.into()
}

fn logs_body(ctx: &Context<DevToolsPanel>, state: &DevToolsState) -> Element {
    let theme = ctx.theme();
    let secondary_style = fg_style(theme.muted.fg.or(theme.primary.fg));
    let dim_style = fg_style(
        theme
            .muted
            .fg
            .map(|paint| Paint::solid(paint.color().dim())),
    );
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
                snapshot.to_png_default(),
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
                snapshot.to_png_default(),
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
        if let Ok(dir) = std::env::var("DEVTOOLS_SNAPSHOT_DIR") {
            #[cfg(feature = "ui-snapshot-png")]
            let _ = std::fs::write(
                format!("{dir}/devtools-stats-empty.png"),
                snapshot.to_png_default(),
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
