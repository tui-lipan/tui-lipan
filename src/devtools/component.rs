use std::cell::RefCell;
use std::rc::Rc;

use crate::core::component::{Component, Context, KeyUpdate, Update};
use crate::core::element::{Element, IntoElement};
use crate::core::event::KeyCode;
use crate::style::{Align, BorderStyle, Justify, Length, Paint, ScrollbarConfig, Style};
use crate::utils::gradient::ColorGradient;
use crate::widgets::{
    Button, ButtonVariant, Frame, HStack, Input, InputEvent, LogFilterMode, LogView, LogViewEvent,
    Overflow, Spacer, Sparkline, SparklineBarsPreset, SparklineVariant, TabsEvent, Text, VStack,
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
    let mut rows: Vec<Element> = Vec::new();

    rows.push(
        Text::new(format!(
            "Focus: {:?} tag={:?} key={:?} id={:?} ring={} stack={}",
            state.focus.policy,
            state.focus.tag,
            state.focus.key.as_ref().map(AsRef::<str>::as_ref),
            state.focus.node_id,
            state.focus.ring_len,
            state.focus.stack_depth,
        ))
        .overflow(Overflow::Ellipsis)
        .width(Length::Flex(1))
        .style(secondary_style)
        .into(),
    );

    if let Some(frame) = state.latest_frame() {
        let total_ms = frame.total_duration.as_secs_f64() * 1000.0;
        let reconcile_ms = frame.reconcile_duration.as_secs_f64() * 1000.0;
        let draw_ms = frame.draw_duration.as_secs_f64() * 1000.0;
        let avg_total = state.avg_frame_ms();
        let avg_reconcile = state.avg_reconcile_ms();
        let avg_draw = state.avg_draw_ms();
        let memo_total = frame.memo_hits + frame.memo_misses;
        let hit_rate = if memo_total == 0 {
            0.0
        } else {
            (frame.memo_hits as f32 / memo_total as f32) * 100.0
        };

        rows.push(
            Text::new(format!("FPS: {:.0}", state.fps()))
                .style(primary_style)
                .into(),
        );
        rows.push(
            Text::new(format!(
                "Frame:     {:.3}ms (avg: {:.3}ms)",
                total_ms, avg_total,
            ))
            .style(secondary_style)
            .into(),
        );
        rows.push(
            Text::new(format!(
                "Reconcile: {:.3}ms (avg: {:.3}ms)",
                reconcile_ms, avg_reconcile,
            ))
            .style(secondary_style)
            .into(),
        );
        rows.push(
            Text::new(format!(
                "Draw:      {:.3}ms (avg: {:.3}ms)",
                draw_ms, avg_draw,
            ))
            .style(secondary_style)
            .into(),
        );
        rows.push(
            Text::new(format!(
                "Nodes: {}  Overlays: {}",
                frame.node_count, frame.overlay_count,
            ))
            .style(secondary_style)
            .into(),
        );
        rows.push(
            Text::new(format!(
                "Memo: {:.0}% hit ({}/{})",
                hit_rate, frame.memo_hits, memo_total,
            ))
            .style(secondary_style)
            .into(),
        );
        rows.push(
            Text::new(format!("Dirty: {}", frame.dirty_level))
                .style(dim_style)
                .into(),
        );
    } else {
        rows.push(
            Text::new("No frame metrics yet")
                .style(secondary_style)
                .into(),
        );
    }

    // Sparkline: pass exactly as many data points as sparkline columns
    // so each sample maps 1:1 - no downsampling, no clipping overhead.
    let cols = state.sparkline_columns(ctx.viewport().w);
    let history = state.duration_history_ms(cols);
    rows.push(
        Sparkline::new(history)
            .variant(SparklineVariant::Bars)
            .min(0)
            .chart_height(2)
            .bars_preset(SparklineBarsPreset::Blocks)
            .height_gradient(ColorGradient::new(
                theme
                    .muted
                    .fg
                    .or(theme.primary.fg)
                    .map(Paint::color)
                    .unwrap_or(theme.border_active),
                theme
                    .accent
                    .fg
                    .map(Paint::color)
                    .unwrap_or(theme.border_active),
            ))
            .overflow(Overflow::ClipStart)
            .width(Length::Flex(1))
            .height(Length::Auto)
            .into(),
    );

    let mut stack = VStack::new().height(Length::Flex(1)).gap(0);
    for row in rows {
        stack = stack.child(row);
    }
    stack.into()
}

fn logs_body(ctx: &Context<DevToolsPanel>, state: &DevToolsState) -> Element {
    let theme = ctx.theme();
    let secondary_style = fg_style(theme.muted.fg.or(theme.primary.fg));

    let filter_input = Input::bound(&state.log_filter)
        .placeholder("Filter logs...")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .on_change(ctx.link().callback(DevToolsMsg::FilterChanged))
        .key(DEVTOOLS_FILTER_KEY);

    let follow_label = if state.log_auto_follow {
        "Auto-Follow: ON"
    } else {
        "Auto-Follow: OFF"
    };
    let paused_label = if state.log_paused {
        "Paused: ON"
    } else {
        "Paused: OFF"
    };
    let framework_label = if state.hide_framework_logs {
        "tui-lipan: OFF"
    } else {
        "tui-lipan: ON"
    };

    let controls = HStack::new()
        .height(Length::Auto)
        .align(Align::Center)
        .gap(1)
        .child(
            Button::new(follow_label)
                .variant(ButtonVariant::Filled)
                .on_click(ctx.link().callback(|_| DevToolsMsg::ToggleAutoFollow)),
        )
        .child(
            Button::new(paused_label)
                .variant(ButtonVariant::Filled)
                .on_click(ctx.link().callback(|_| DevToolsMsg::TogglePaused)),
        )
        .child(
            Button::new(framework_label)
                .variant(ButtonVariant::Filled)
                .on_click(ctx.link().callback(|_| DevToolsMsg::ToggleFrameworkLogs)),
        )
        .child(
            Button::new("Clear")
                .variant(ButtonVariant::Filled)
                .on_click(ctx.link().callback(|_| DevToolsMsg::ClearLogs)),
        )
        .child(Spacer::new().width(Length::Flex(1)))
        .child(
            Text::new(format!(
                " {} / {} lines",
                state.displayed_log_count(),
                state.log_entries.len()
            ))
            .overflow(Overflow::Clip)
            .style(secondary_style),
        );

    let log_view = LogView::new()
        .entries_arc(state.log_entries())
        .filter(state.log_filter.text())
        .filter_mode(LogFilterMode::Fuzzy)
        .case_sensitive(false)
        .show_level(false)
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
