use std::cell::Cell;
use std::cell::RefCell;
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

use super::{
    AppRunner, DirtyLevel, DirtyTracker, DragState, FrameworkCommandAction,
    effective_active_drag_dirty_level, mouse_dispatch_dirty_level, spinner_frame_for_speed,
};
use crate::TextEditor;
use crate::animation::{Easing, TransitionConfig};
#[cfg(feature = "devtools")]
use crate::app::context::DevToolsConfig;
use crate::app::context::{App, FocusPolicy, SurfaceMode};
use crate::app::input::runtime_dispatch::should_dispatch_text_area_tab_first;
use crate::callback::{Callback, ScopeId};
use crate::clipboard::{ClipboardConfig, ClipboardError, ClipboardProvider};
use crate::core::component::{Component, Context, Update};
use crate::core::element::{Element, IntoElement};
use crate::core::event::{KeyCode, KeyEvent, KeyMods, MouseButton, MouseEvent, MouseKind};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::LayoutEngine;
use crate::runtime::RuntimeCore;
#[cfg(feature = "terminal")]
use crate::style::Span;
use crate::style::{Color, HostTerminalColors, Length, Rect, Style, Theme};
#[cfg(feature = "terminal")]
use crate::widgets::Terminal;
use crate::widgets::internal::AnimatedNode;
use crate::widgets::{
    Animated, DocumentView, DragPayload, DragSource, DropTarget, Frame, HStack, Input, List,
    ListItem, MouseRegion, ScrollView, Spacer, Spinner, SpinnerSpeed, Text, TextArea,
    TextAreaEvent, VStack,
};
use crossterm::event::{
    Event as CEvent, KeyCode as CKeyCode, KeyEvent as CKeyEvent, KeyModifiers as CKeyModifiers,
    MouseEvent as CMouseEvent, MouseEventKind as CMouseEventKind,
};
use ratatui::Terminal as RatatuiTerminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Cell as RatatuiCell;
use ratatui::widgets::Paragraph;

struct RunnerKeymapSmoke;

#[cfg(feature = "devtools")]
#[derive(Clone)]
struct RunnerDynamicThemeSmoke {
    backdrop: Rc<Cell<Color>>,
}

#[cfg(feature = "devtools")]
impl Component for RunnerDynamicThemeSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let mut theme = Theme::default();
        theme.surface.backdrop = self.backdrop.get();
        crate::widgets::ThemeProvider::new(theme)
            .child(Text::new("root"))
            .into()
    }
}

#[derive(Clone)]
struct RecordingClipboardProvider {
    writes: Rc<RefCell<Vec<String>>>,
}

impl ClipboardProvider for RecordingClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
        Ok(String::new())
    }

    fn write_clipboard_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.writes.borrow_mut().push(text.to_string());
        Ok(())
    }
}

impl Component for RunnerKeymapSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Input::new("").key("input")
    }
}

fn c_key(ch: char) -> CEvent {
    CEvent::Key(CKeyEvent::new(CKeyCode::Char(ch), CKeyModifiers::NONE))
}

fn c_mouse(kind: CMouseEventKind, column: u16, row: u16) -> CEvent {
    CEvent::Mouse(CMouseEvent {
        kind,
        column,
        row,
        modifiers: CKeyModifiers::NONE,
    })
}

fn host_colors(bg: Color) -> HostTerminalColors {
    HostTerminalColors {
        ansi: std::array::from_fn(|i| Color::indexed(i as u8)),
        fg: Color::White,
        bg,
    }
}

#[test]
fn host_terminal_color_cache_generation_changes_only_when_colors_differ() {
    let runner = AppRunner::new(
        App::new().live_host_terminal_colors(true),
        RunnerKeymapSmoke,
        (),
    );
    let env = runner.core.ctx.env();
    let first = host_colors(Color::rgb(1, 2, 3));

    assert_eq!(runner.core.ctx.host_terminal_colors(), None);
    assert_eq!(runner.core.ctx.host_terminal_color_generation(), 0);

    assert!(env.set_host_terminal_colors(Some(first)));
    assert_eq!(runner.core.ctx.host_terminal_colors(), Some(first));
    assert_eq!(runner.core.ctx.host_terminal_color_generation(), 1);

    assert!(!env.set_host_terminal_colors(Some(first)));
    assert_eq!(runner.core.ctx.host_terminal_color_generation(), 1);

    let second = host_colors(Color::rgb(4, 5, 6));
    assert!(env.set_host_terminal_colors(Some(second)));
    assert_eq!(runner.core.ctx.host_terminal_colors(), Some(second));
    assert_eq!(runner.core.ctx.host_terminal_color_generation(), 2);
}

#[test]
fn host_terminal_color_refresh_request_is_opt_in() {
    let disabled = AppRunner::new(App::new(), RunnerKeymapSmoke, ());
    disabled.core.ctx.request_host_terminal_color_refresh();
    assert!(
        !disabled
            .core
            .ctx
            .env()
            .take_host_terminal_color_refresh_request()
    );

    let enabled = AppRunner::new(
        App::new().live_host_terminal_colors(true),
        RunnerKeymapSmoke,
        (),
    );
    enabled.core.ctx.request_host_terminal_color_refresh();
    assert!(
        enabled
            .core
            .ctx
            .env()
            .take_host_terminal_color_refresh_request()
    );

    let system_theme = AppRunner::new(App::new().system_theme(), RunnerKeymapSmoke, ());
    system_theme.core.ctx.request_host_terminal_color_refresh();
    assert!(
        system_theme
            .core
            .ctx
            .env()
            .take_host_terminal_color_refresh_request()
    );
}

#[cfg(unix)]
#[test]
fn termina_live_input_is_unix_fullscreen_and_opt_in_only() {
    let fullscreen = super::SurfaceDriver::new(SurfaceMode::Fullscreen);
    let inline = super::SurfaceDriver::new(SurfaceMode::InlineEphemeral {
        height: crate::app::InlineHeight::Fixed(4),
    });

    assert!(super::uses_termina_live_input(&fullscreen, true));
    assert!(!super::uses_termina_live_input(&fullscreen, false));
    assert!(!super::uses_termina_live_input(&inline, true));
}

#[test]
fn focus_gained_host_color_refresh_request_is_opt_in() {
    let disabled = AppRunner::new(App::new(), RunnerKeymapSmoke, ());
    disabled.request_host_terminal_color_refresh_from_event();
    assert!(!disabled.take_host_terminal_color_refresh_request());

    let enabled = AppRunner::new(
        App::new().live_host_terminal_colors(true),
        RunnerKeymapSmoke,
        (),
    );
    enabled.request_host_terminal_color_refresh_from_event();
    assert!(enabled.take_host_terminal_color_refresh_request());

    let system_theme = AppRunner::new(App::new().system_theme(), RunnerKeymapSmoke, ());
    system_theme.request_host_terminal_color_refresh_from_event();
    assert!(system_theme.take_host_terminal_color_refresh_request());
}

#[test]
fn host_terminal_color_change_updates_terminal_bg_and_requests_repaint() {
    let mut runner = AppRunner::new(
        App::new()
            .live_host_terminal_colors(true)
            .terminal_bg(Some(Color::rgb(9, 9, 9))),
        RunnerKeymapSmoke,
        (),
    );
    let fallback = runner.core.theme.clone();
    let colors = host_colors(Color::rgb(10, 11, 12));

    assert!(runner.apply_host_terminal_colors(colors, true));
    assert_eq!(runner.terminal_bg, Some(colors.bg));
    assert_eq!(runner.core.theme, fallback);
    assert_eq!(runner.core.ctx.theme(), fallback);
    assert_eq!(runner.core.ctx.host_terminal_colors(), Some(colors));
    assert_eq!(runner.core.ctx.host_terminal_color_generation(), 1);
    assert!(runner.core.take_full_repaint_request());

    assert!(!runner.apply_host_terminal_colors(colors, true));
    assert!(!runner.core.take_full_repaint_request());
}

#[test]
fn system_theme_keeps_fallback_until_host_colors_apply() {
    let fallback = Theme::dracula();
    let runner = AppRunner::new(
        App::new().theme(fallback.clone()).system_theme(),
        RunnerKeymapSmoke,
        (),
    );

    assert_eq!(runner.core.theme, fallback);
    assert_eq!(runner.core.ctx.theme(), fallback);
    assert_eq!(runner.terminal_bg, None);
}

#[test]
fn system_theme_host_color_change_updates_terminal_bg_core_and_context_theme() {
    let mut runner = AppRunner::new(
        App::new()
            .theme(Theme::dracula())
            .system_theme()
            .terminal_bg(Some(Color::rgb(9, 9, 9))),
        RunnerKeymapSmoke,
        (),
    );
    let colors = host_colors(Color::rgb(10, 11, 12));
    let expected = Theme::from_host_colors(colors);

    assert!(runner.apply_host_terminal_colors(colors, true));
    assert_eq!(runner.terminal_bg, Some(colors.bg));
    assert_eq!(runner.core.theme, expected);
    assert_eq!(runner.core.ctx.theme(), expected);
    assert_eq!(runner.core.ctx.host_terminal_colors(), Some(colors));
    assert_eq!(runner.core.ctx.host_terminal_color_generation(), 1);
    assert!(runner.core.take_full_repaint_request());

    assert!(!runner.apply_host_terminal_colors(colors, true));
    assert_eq!(runner.core.theme, expected);
    assert_eq!(runner.core.ctx.theme(), expected);
    assert!(!runner.core.take_full_repaint_request());
}

#[test]
fn system_theme_startup_host_color_apply_does_not_request_repaint() {
    let mut runner = AppRunner::new(App::new().system_theme(), RunnerKeymapSmoke, ());
    let colors = host_colors(Color::rgb(10, 11, 12));
    let expected = Theme::from_host_colors(colors);

    assert!(runner.apply_host_terminal_colors(colors, false));

    assert_eq!(runner.terminal_bg, Some(colors.bg));
    assert_eq!(runner.core.theme, expected);
    assert_eq!(runner.core.ctx.theme(), expected);
    assert!(!runner.core.take_full_repaint_request());
}

#[test]
fn system_theme_palette_change_with_same_background_updates_theme() {
    let mut runner = AppRunner::new(App::new().system_theme(), RunnerKeymapSmoke, ());
    let bg = Color::rgb(10, 11, 12);
    let mut first = host_colors(bg);
    first.ansi[4] = Color::rgb(20, 80, 180);
    let mut second = host_colors(bg);
    second.ansi[4] = Color::rgb(180, 80, 20);

    assert!(runner.apply_host_terminal_colors(first, true));
    let first_theme = runner.core.theme.clone();
    assert!(runner.core.take_full_repaint_request());

    assert!(runner.apply_host_terminal_colors(second, true));

    assert_eq!(runner.terminal_bg, Some(bg));
    assert_eq!(runner.core.theme, Theme::from_host_colors(second));
    assert_eq!(runner.core.ctx.theme(), Theme::from_host_colors(second));
    assert_ne!(runner.core.theme, first_theme);
    assert!(runner.core.take_full_repaint_request());
}

#[test]
fn termina_runtime_refresh_ignores_equal_resolved_colors() {
    let mut runner = AppRunner::new(App::new().system_theme(), RunnerKeymapSmoke, ());
    let colors = host_colors(Color::rgb(10, 11, 12));

    assert!(runner.apply_host_terminal_colors(colors, true));
    assert_eq!(runner.core.ctx.host_terminal_color_generation(), 1);
    assert!(runner.core.take_full_repaint_request());

    assert!(!runner.apply_host_terminal_colors(colors, true));
    assert_eq!(runner.core.ctx.host_terminal_color_generation(), 1);
    assert!(!runner.core.take_full_repaint_request());
}

#[test]
fn previous_frame_invalidation_repaints_without_physically_clearing_first() {
    let mut terminal = RatatuiTerminal::new(TestBackend::new(6, 1)).unwrap();
    let draw = |frame: &mut ratatui::Frame<'_>| {
        frame.render_widget(Paragraph::new("ok"), frame.area());
    };
    terminal.draw(draw).unwrap();

    let overwritten = RatatuiCell::new("x");
    let updates: Vec<_> = (0..6).map(|x| (x, 0, &overwritten)).collect();
    ratatui::backend::Backend::draw(terminal.backend_mut(), updates.into_iter()).unwrap();
    assert_eq!(
        terminal.backend().buffer().cell((5, 0)).unwrap().symbol(),
        "x"
    );

    super::invalidate_previous_frame(&mut terminal);
    assert_eq!(
        terminal.backend().buffer().cell((5, 0)).unwrap().symbol(),
        "x",
        "invalidation must not expose an empty intermediate frame"
    );

    terminal.draw(draw).unwrap();
    assert_eq!(
        terminal.backend().buffer().cell((0, 0)).unwrap().symbol(),
        "o"
    );
    assert_eq!(
        terminal.backend().buffer().cell((1, 0)).unwrap().symbol(),
        "k"
    );
    assert_eq!(
        terminal.backend().buffer().cell((5, 0)).unwrap().symbol(),
        " "
    );

    let draw_noncharacter = |frame: &mut ratatui::Frame<'_>| {
        frame.render_widget(Paragraph::new("\u{fdd0}"), frame.area());
    };
    terminal.draw(draw_noncharacter).unwrap();
    let updates: Vec<_> = (0..6).map(|x| (x, 0, &overwritten)).collect();
    ratatui::backend::Backend::draw(terminal.backend_mut(), updates.into_iter()).unwrap();

    super::invalidate_previous_frame(&mut terminal);
    terminal.draw(draw_noncharacter).unwrap();
    assert_eq!(
        terminal.backend().buffer().cell((0, 0)).unwrap().symbol(),
        "\u{fdd0}",
        "forced repaint must not collide with drawable application content"
    );
}

#[test]
fn host_terminal_color_refresh_waits_for_quiet_window() {
    let now = Instant::now();
    let deadline = super::deferred_host_color_refresh_deadline(now);

    let remaining = super::host_color_refresh_wait_remaining(Some(deadline), now)
        .expect("deadline should block refresh");
    assert!(remaining > Duration::ZERO);
    assert!(remaining <= super::HOST_COLOR_REFRESH_QUIET_WINDOW);
    assert_eq!(
        super::host_color_refresh_wait_remaining(Some(deadline), deadline),
        None
    );
    assert_eq!(super::host_color_refresh_wait_remaining(None, now), None);
}

#[test]
fn resize_burst_followed_by_key_preserves_key_as_pending_event() {
    let runner = AppRunner::new(App::new(), RunnerKeymapSmoke, ());
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(super::RunnerEvent::Terminal(CEvent::Resize(81, 24)))
        .unwrap();
    tx.send(super::RunnerEvent::Terminal(CEvent::Resize(82, 24)))
        .unwrap();
    tx.send(super::RunnerEvent::Terminal(c_key('x'))).unwrap();

    let mut pending_event = None;
    while let Some(next_ev) = runner.try_recv_event(Some(&rx)).unwrap() {
        if matches!(next_ev, super::RunnerEvent::Terminal(CEvent::Resize(_, _))) {
            continue;
        }
        super::preserve_pending_event(&mut pending_event, next_ev);
        break;
    }

    assert!(matches!(
        pending_event,
        Some(super::RunnerEvent::Terminal(CEvent::Key(_)))
    ));
    assert!(runner.try_recv_event(Some(&rx)).unwrap().is_none());
}

#[test]
fn mouse_move_burst_followed_by_key_or_resize_preserves_non_mouse_event() {
    for trailing in [c_key('m'), CEvent::Resize(100, 30)] {
        let runner = AppRunner::new(App::new(), RunnerKeymapSmoke, ());
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(super::RunnerEvent::Terminal(c_mouse(
            CMouseEventKind::Moved,
            2,
            3,
        )))
        .unwrap();
        tx.send(super::RunnerEvent::Terminal(c_mouse(
            CMouseEventKind::Moved,
            3,
            4,
        )))
        .unwrap();
        tx.send(super::RunnerEvent::Terminal(trailing.clone()))
            .unwrap();

        let mut pending_event = None;
        while let Some(next_ev) = runner.try_recv_event(Some(&rx)).unwrap() {
            if let super::RunnerEvent::Terminal(CEvent::Mouse(next_m)) = next_ev {
                if let Some(next_mouse) = runner.convert_mouse_event(next_m)
                    && matches!(next_mouse.kind, MouseKind::Moved)
                {
                    continue;
                }
            } else {
                super::preserve_pending_event(&mut pending_event, next_ev);
                break;
            }
        }

        assert_eq!(pending_event, Some(super::RunnerEvent::Terminal(trailing)));
        assert!(runner.try_recv_event(Some(&rx)).unwrap().is_none());
    }
}

#[test]
fn scroll_burst_followed_by_non_scroll_event_preserves_event() {
    let runner = AppRunner::new(App::new(), RunnerKeymapSmoke, ());
    let (tx, rx) = std::sync::mpsc::channel();
    tx.send(super::RunnerEvent::Terminal(c_mouse(
        CMouseEventKind::ScrollDown,
        2,
        3,
    )))
    .unwrap();
    tx.send(super::RunnerEvent::Terminal(c_mouse(
        CMouseEventKind::ScrollDown,
        2,
        4,
    )))
    .unwrap();
    tx.send(super::RunnerEvent::Terminal(c_key('s'))).unwrap();

    let mut pending_event = None;
    while let Some(next_ev) = runner.try_recv_event(Some(&rx)).unwrap() {
        if let super::RunnerEvent::Terminal(CEvent::Mouse(next_m)) = next_ev {
            if let Some(next_mouse) = runner.convert_mouse_event(next_m)
                && next_mouse.kind == MouseKind::ScrollDown
            {
                continue;
            }
        } else {
            super::preserve_pending_event(&mut pending_event, next_ev);
            break;
        }
    }

    assert!(matches!(
        pending_event,
        Some(super::RunnerEvent::Terminal(CEvent::Key(_)))
    ));
    assert!(runner.try_recv_event(Some(&rx)).unwrap().is_none());
}

#[test]
fn frame_skip_preserve_does_not_overwrite_existing_pending_event() {
    let mut pending_event = Some(super::RunnerEvent::Terminal(c_key('p')));
    super::preserve_pending_event(
        &mut pending_event,
        super::RunnerEvent::Terminal(CEvent::Resize(120, 40)),
    );

    assert!(matches!(
        pending_event,
        Some(super::RunnerEvent::Terminal(CEvent::Key(_)))
    ));
}

#[test]
fn host_color_refresh_event_does_not_drop_queued_ordinary_input() {
    let runner = AppRunner::new(App::new(), RunnerKeymapSmoke, ());
    let (tx, rx) = std::sync::mpsc::channel();
    let colors = host_colors(Color::rgb(3, 4, 5));
    tx.send(super::RunnerEvent::HostTerminalColors(colors))
        .unwrap();
    tx.send(super::RunnerEvent::Terminal(CEvent::Resize(90, 30)))
        .unwrap();
    tx.send(super::RunnerEvent::Terminal(c_mouse(
        CMouseEventKind::ScrollDown,
        2,
        3,
    )))
    .unwrap();
    tx.send(super::RunnerEvent::Terminal(CEvent::Paste("paste".into())))
        .unwrap();
    tx.send(super::RunnerEvent::Terminal(c_key('k'))).unwrap();

    assert_eq!(
        runner.try_recv_event(Some(&rx)).unwrap(),
        Some(super::RunnerEvent::HostTerminalColors(colors))
    );
    assert!(matches!(
        runner.try_recv_event(Some(&rx)).unwrap(),
        Some(super::RunnerEvent::Terminal(CEvent::Resize(90, 30)))
    ));
    assert!(matches!(
        runner.try_recv_event(Some(&rx)).unwrap(),
        Some(super::RunnerEvent::Terminal(CEvent::Mouse(_)))
    ));
    assert!(matches!(
        runner.try_recv_event(Some(&rx)).unwrap(),
        Some(super::RunnerEvent::Terminal(CEvent::Paste(_)))
    ));
    assert!(matches!(
        runner.try_recv_event(Some(&rx)).unwrap(),
        Some(super::RunnerEvent::Terminal(CEvent::Key(_)))
    ));
}

#[test]
fn fullscreen_input_channel_disconnect_is_an_error() {
    let runner = AppRunner::new(App::new(), RunnerKeymapSmoke, ());
    let (tx, rx) = std::sync::mpsc::channel();
    drop(tx);

    let recv_error = runner
        .recv_event(Duration::ZERO, Some(&rx))
        .expect_err("disconnected blocking receiver should fail");
    assert!(matches!(
        recv_error,
        crate::Error::Io(error) if error.kind() == std::io::ErrorKind::BrokenPipe
    ));

    let (tx, rx) = std::sync::mpsc::channel();
    drop(tx);
    let try_error = runner
        .try_recv_event(Some(&rx))
        .expect_err("disconnected non-blocking receiver should fail");
    assert!(matches!(
        try_error,
        crate::Error::Io(error) if error.kind() == std::io::ErrorKind::BrokenPipe
    ));
}

struct ScrollHoverSmoke;

struct ScrollMultiplierSmoke;

struct ScrollViewMultiplierSmoke;

struct TextAreaMultiplierSmoke;

struct DocumentViewMultiplierSmoke;

struct AutoSpinnerSmoke;

struct ManualSpinnerSmoke;

struct PaintOnlyMessageProbe;

#[derive(Clone)]
enum PaintOnlyMessage {
    Tick,
}

impl Component for ScrollHoverSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        List::new()
            .items([
                ListItem::new("one"),
                ListItem::new("two"),
                ListItem::new("three"),
            ])
            .item_hover_style(Style::new().bg(Color::Blue))
            .into()
    }
}

impl Component for ScrollMultiplierSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        List::new()
            .items((0..10).map(|i| ListItem::new(format!("item-{i}"))))
            .into()
    }
}

impl Component for ScrollViewMultiplierSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        ScrollView::new()
            .height(Length::Px(1))
            .scroll_wheel_multiplier(2)
            .children((0..10).map(|i| Text::new(format!("row-{i}")).into()))
            .into()
    }
}

impl Component for TextAreaMultiplierSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        TextArea::new(
            (0..10)
                .map(|i| format!("line-{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .border(false)
        .height(Length::Px(1))
        .scroll_wheel_multiplier(2)
        .into()
    }
}

impl Component for DocumentViewMultiplierSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        DocumentView::new(
            (0..10)
                .map(|i| format!("line-{i}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .border(false)
        .height(Length::Px(1))
        .scroll_wheel_multiplier(2)
        .into()
    }
}

impl Component for AutoSpinnerSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Spinner::new().into()
    }
}

impl Component for ManualSpinnerSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Spinner::new().frame(7).into()
    }
}

impl Component for PaintOnlyMessageProbe {
    type Message = PaintOnlyMessage;
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        match msg {
            PaintOnlyMessage::Tick => Update::paint(),
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        Text::new("paint-probe").into()
    }
}

struct StationaryAutoscrollSmoke;

struct StandaloneAutoscrollTextAreaSmoke;

struct StandaloneAutoscrollDocumentViewSmoke;

struct ScrollSelectionTextAreaSmoke;

struct ScrollSelectionDocumentViewSmoke;

struct NestedScrollSelectionTextAreaSmoke;

struct NestedScrollSelectionDocumentViewSmoke;

#[derive(Clone, Debug)]
struct DragTestPayload {
    label: Arc<str>,
}

#[derive(Clone)]
struct DragDropSmoke {
    log: Rc<RefCell<Vec<String>>>,
    source_group: Option<Arc<str>>,
    left_group: Option<Arc<str>>,
    right_group: Option<Arc<str>>,
}

#[derive(Clone)]
struct AnimatedPairSmoke {
    hidden: Rc<Cell<bool>>,
}

#[derive(Clone)]
struct AnimatedHeightSmoke {
    open: Rc<Cell<bool>>,
}

#[derive(Clone)]
struct AnimatedColorSmoke {
    active: Rc<Cell<bool>>,
}

#[derive(Clone)]
struct AnimatedPositionSmoke {
    shifted: Rc<Cell<bool>>,
    ended: Rc<Cell<u32>>,
    duration: Duration,
}

#[derive(Clone)]
struct AnimatedPositionRetargetSmoke {
    leading_width: Rc<Cell<u16>>,
    ended: Rc<Cell<u32>>,
    duration: Duration,
}

struct SmoothScrollViewSmoke;

#[derive(Clone)]
struct SmoothWheelScrollSmoke {
    offsets: Rc<RefCell<Vec<usize>>>,
}

struct SmoothDocumentViewSmoke;

struct SmoothTextAreaSmoke;

impl Component for StationaryAutoscrollSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        let body = (0..24)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        ScrollView::new()
            .child(DocumentView::new(body).border(false))
            .into()
    }
}

impl Component for StandaloneAutoscrollTextAreaSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        let body = (0..24)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        TextArea::new(body).line_numbers(false).border(false).into()
    }
}

impl Component for StandaloneAutoscrollDocumentViewSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        let body = (0..24)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        DocumentView::new(body).border(false).into()
    }
}

impl Component for ScrollSelectionTextAreaSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        TextArea::new("zero\none\ntwo")
            .line_numbers(false)
            .border(false)
            .into()
    }
}

impl Component for ScrollSelectionDocumentViewSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        DocumentView::new("zero\none\ntwo").border(false).into()
    }
}

impl Component for NestedScrollSelectionTextAreaSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        ScrollView::new()
            .child(
                TextArea::new("zero\none\ntwo")
                    .line_numbers(false)
                    .border(false)
                    .scroll_wheel(false),
            )
            .into()
    }
}

impl Component for NestedScrollSelectionDocumentViewSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
        ScrollView::new()
            .child(
                DocumentView::new("zero\none\ntwo")
                    .border(false)
                    .scroll_wheel(false),
            )
            .into()
    }
}

impl Component for DragDropSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let log = self.log.clone();
        let mut source = DragSource::new()
            .child(
                Frame::new()
                    .header_left("Source")
                    .border(true)
                    .padding(1)
                    .height(crate::style::Length::Px(3))
                    .child(Text::new("Drag card")),
            )
            .threshold(3)
            .preview_label("Card A")
            .dragging_style(Style::new().bg(Color::Blue))
            .on_drag_start(move |_| {
                log.borrow_mut().push("start".to_string());
                Some(Box::new(DragTestPayload {
                    label: Arc::from("Card A"),
                }) as Box<dyn DragPayload>)
            });
        if let Some(group) = self.source_group.clone() {
            source = source.drag_group(group);
        }

        let cancel_log = self.log.clone();
        source = source.on_drag_cancel(Callback::new(
            move |event: crate::widgets::DragCancelEvent| {
                let payload = event
                    .payload
                    .downcast_ref::<DragTestPayload>()
                    .expect("payload should downcast");
                cancel_log
                    .borrow_mut()
                    .push(format!("cancel:{}", payload.label));
            },
        ));

        VStack::new()
            .gap(1)
            .child(
                Element::from(source)
                    .min_height(crate::style::Length::Px(3))
                    .max_height(crate::style::Length::Px(3))
                    .key("source"),
            )
            .child(
                HStack::new()
                    .gap(1)
                    .child(make_drop_target(
                        self.log.clone(),
                        "left",
                        self.left_group.clone(),
                    ))
                    .child(make_drop_target(
                        self.log.clone(),
                        "right",
                        self.right_group.clone(),
                    ))
                    .min_height(crate::style::Length::Px(4))
                    .max_height(crate::style::Length::Px(4)),
            )
            .into()
    }
}

impl Component for AnimatedPairSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let hidden = self.hidden.get();
        let transition = TransitionConfig {
            duration: Duration::from_millis(100),
            easing: Easing::Linear,
        };

        VStack::new()
            .child(
                Animated::new(Text::new("left"))
                    .opacity(if hidden { 0.0 } else { 1.0 })
                    .transition(transition)
                    .key("left"),
            )
            .child(
                Animated::new(Text::new("right"))
                    .opacity(if hidden { 0.0 } else { 1.0 })
                    .transition(transition)
                    .key("right"),
            )
            .into()
    }
}

impl Component for AnimatedHeightSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let open = self.open.get();
        let transition = TransitionConfig {
            duration: Duration::from_millis(100),
            easing: Easing::Linear,
        };

        VStack::new()
            .child(Text::new("before"))
            .child(
                Animated::new(
                    Frame::new().border(true).child(
                        VStack::new()
                            .child(Text::new("alpha"))
                            .child(Text::new("beta"))
                            .child(Text::new("gamma")),
                    ),
                )
                .height(if open { Length::Auto } else { Length::Px(0) })
                .transition(transition)
                .key("panel"),
            )
            .child(Text::new("after"))
            .into()
    }
}

impl Component for AnimatedColorSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let transition = TransitionConfig {
            duration: Duration::from_millis(100),
            easing: Easing::Linear,
        };
        let active = self.active.get();

        Animated::new(Text::new("color"))
            .fg(if active {
                Color::rgb(255, 128, 32)
            } else {
                Color::rgb(16, 24, 32)
            })
            .bg(if active {
                Color::rgb(4, 8, 12)
            } else {
                Color::rgb(180, 120, 60)
            })
            .transition(transition)
            .key("color")
    }
}

impl Component for AnimatedPositionSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let transition = TransitionConfig {
            duration: self.duration,
            easing: Easing::Linear,
        };
        let ended = self.ended.clone();
        let animated = Animated::new(Text::new("move"))
            .position_transition(true)
            .transition(transition)
            .on_position_transition_end(Callback::new(move |_| {
                ended.set(ended.get() + 1);
            }))
            .key("moving");
        let spacer = Spacer::new().width(Length::Px(10)).height(Length::Px(1));

        if self.shifted.get() {
            HStack::new().child(spacer).child(animated).into()
        } else {
            HStack::new().child(animated).child(spacer).into()
        }
    }
}

impl Component for AnimatedPositionRetargetSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let transition = TransitionConfig {
            duration: self.duration,
            easing: Easing::Linear,
        };
        let ended = self.ended.clone();

        HStack::new()
            .child(
                Spacer::new()
                    .width(Length::Px(self.leading_width.get()))
                    .height(Length::Px(1)),
            )
            .child(
                Animated::new(Text::new("move"))
                    .position_transition(true)
                    .transition(transition)
                    .on_position_transition_end(Callback::new(move |_| {
                        ended.set(ended.get() + 1);
                    }))
                    .key("moving"),
            )
            .into()
    }
}

fn smooth_test_transition() -> TransitionConfig {
    TransitionConfig {
        duration: Duration::from_millis(100),
        easing: Easing::Linear,
    }
}

impl Component for SmoothScrollViewSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Element::from(
            ScrollView::new()
                .height(Length::Px(4))
                .children(
                    (0..10).map(|idx| Text::new(format!("row {idx}")).key(format!("row-{idx}"))),
                )
                .scroll_to_key("row-8")
                .scroll_transition(smooth_test_transition()),
        )
        .key("smooth-scroll")
    }
}

impl Component for SmoothWheelScrollSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let offsets = self.offsets.clone();
        Element::from(
            ScrollView::new()
                .height(Length::Px(4))
                .smooth_wheel_scroll(true)
                .scroll_acceleration(100.0)
                .on_scroll(Callback::new(move |event: crate::widgets::ScrollEvent| {
                    offsets.borrow_mut().push(event.offset);
                }))
                .children((0..10).map(|idx| Text::new(format!("row {idx}")).into())),
        )
        .key("smooth-wheel-scroll")
    }
}

impl Component for SmoothDocumentViewSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Element::from(
            DocumentView::new(
                (0..10)
                    .map(|idx| format!("line {idx}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .border(false)
            .height(Length::Px(4))
            .scroll_to_source_line(8)
            .scroll_transition(smooth_test_transition()),
        )
        .key("smooth-doc")
    }
}

impl Component for SmoothTextAreaSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Element::from(
            TextArea::new(
                (0..10)
                    .map(|idx| format!("line {idx}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
            .border(false)
            .height(Length::Px(4))
            .scroll_to_line(8)
            .scroll_transition(smooth_test_transition()),
        )
        .key("smooth-text-area")
    }
}

fn make_drop_target(
    log: Rc<RefCell<Vec<String>>>,
    name: &'static str,
    group: Option<Arc<str>>,
) -> Element {
    let mut target = DropTarget::new()
        .child(
            Element::from(
                Frame::new()
                    .header_left(name)
                    .border(true)
                    .padding(1)
                    .width(crate::style::Length::Px(14))
                    .height(crate::style::Length::Px(4))
                    .child(Text::new(format!("Target {name}"))),
            )
            .key(format!("{name}-frame")),
        )
        .highlight_fill(Style::new().bg(Color::DarkGray))
        .on_drag_over(Callback::new({
            let log = log.clone();
            move |_| {
                log.borrow_mut().push(format!("over:{name}"));
            }
        }))
        .on_drag_leave(Callback::new({
            let log = log.clone();
            move |_| {
                log.borrow_mut().push(format!("leave:{name}"));
            }
        }))
        .on_drop(Callback::new(move |event: crate::widgets::DropEvent| {
            let payload = event
                .payload
                .downcast_ref::<DragTestPayload>()
                .expect("payload should downcast");
            log.borrow_mut()
                .push(format!("drop:{name}:{}", payload.label));
        }));
    if let Some(group) = group {
        target = target.accept_group(group);
    }
    Element::from(target).key(name)
}

fn init_runner<C: Component>(runner: &mut AppRunner<C>, component: C, viewport: Rect)
where
    C::Properties: Default,
{
    runner.core = RuntimeCore::new_test(
        component,
        Default::default(),
        viewport,
        Theme::default(),
        SurfaceMode::Fullscreen,
        Rc::new(Cell::new(false)),
    );
    runner.core.init();
    runner.core.render_element(viewport, None, None, None);
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        mods: KeyMods::default(),
    }
}

fn ctrl_char(ch: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(ch),
        mods: KeyMods {
            ctrl: true,
            ..KeyMods::default()
        },
    }
}

fn node_id_by_key(tree: &NodeTree, key: &str) -> NodeId {
    tree.iter()
        .find(|node| {
            node.key
                .as_ref()
                .is_some_and(|node_key| node_key.as_ref() == key)
        })
        .map(|node| node.id)
        .expect("node with key should exist")
}

fn node_center<C: Component>(runner: &AppRunner<C>, key: &str) -> (u16, u16) {
    let id = node_id_by_key(&runner.core.tree, key);
    let rect = runner.core.tree.node(id).rect;
    (
        rect.x.saturating_add((rect.w / 2) as i16) as u16,
        rect.y.saturating_add((rect.h / 2) as i16) as u16,
    )
}

fn arm_drag_from_source(runner: &mut AppRunner<DragDropSmoke>) -> (u16, u16) {
    let source_id = node_id_by_key(&runner.core.tree, "source");
    let (sx, sy) = node_center(runner, "source");
    runner.mouse.pending_drag_source = Some(source_id);
    runner.mouse.left_down_pos = Some((sx, sy));
    (sx, sy)
}

fn point_has_drop_target<C: Component>(runner: &AppRunner<C>, x: u16, y: u16) -> bool {
    let mut cur = runner.core.tree.hit_test(x as i16, y as i16);
    while let Some(id) = cur {
        let node = runner.core.tree.node(id);
        if matches!(node.kind, NodeKind::DropTarget(_)) {
            return true;
        }
        cur = node.parent;
    }
    false
}

fn first_spinner_node<C: Component>(runner: &AppRunner<C>) -> (usize, bool, SpinnerSpeed) {
    runner
        .core
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::Spinner(spinner) => Some((spinner.frame, spinner.auto_frame, spinner.speed)),
            _ => None,
        })
        .expect("spinner node should exist")
}

fn drag_source_dragging<C: Component>(runner: &AppRunner<C>) -> bool {
    let id = node_id_by_key(&runner.core.tree, "source");
    match &runner.core.tree.node(id).kind {
        NodeKind::DragSource(node) => node.is_dragging,
        _ => panic!("expected DragSource node"),
    }
}

#[test]
fn custom_spinner_speed_can_land_between_normal_and_slow() {
    assert_eq!(spinner_frame_for_speed(12, SpinnerSpeed::Normal), 6);
    assert_eq!(
        spinner_frame_for_speed(12, SpinnerSpeed::Custom { frame_ms: 150 }),
        4
    );
    assert_eq!(spinner_frame_for_speed(12, SpinnerSpeed::Slow), 3);
}

#[test]
fn auto_spinner_syncs_to_current_runtime_frame() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), AutoSpinnerSmoke, ());
    init_runner(&mut runner, AutoSpinnerSmoke, viewport);

    runner.animation.spinner_frame = 9;
    runner.update_spinner_frames();

    let (frame, auto_frame, speed) = first_spinner_node(&runner);
    assert!(auto_frame);
    assert_eq!(frame, spinner_frame_for_speed(9, speed));
}

#[test]
fn manual_spinner_frame_is_not_overwritten_by_runtime_tick() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), ManualSpinnerSmoke, ());
    init_runner(&mut runner, ManualSpinnerSmoke, viewport);

    runner.animation.spinner_frame = 9;
    runner.update_spinner_frames();

    let (frame, auto_frame, _) = first_spinner_node(&runner);
    assert!(!auto_frame);
    assert_eq!(frame, 7);
}

fn drop_target_highlighted<C: Component>(runner: &AppRunner<C>, key: &str) -> bool {
    let id = node_id_by_key(&runner.core.tree, key);
    match &runner.core.tree.node(id).kind {
        NodeKind::DropTarget(node) => node.dnd_highlighted,
        _ => panic!("expected DropTarget node"),
    }
}

fn animated_child_height<C: Component>(runner: &AppRunner<C>, key: &str) -> u16 {
    let id = node_id_by_key(&runner.core.tree, key);
    let node = runner.core.tree.node(id);
    let child_id = *node
        .children
        .first()
        .expect("animated node should have child");
    runner.core.tree.node(child_id).rect.h
}

fn animated_node_by_key<'a, C: Component>(runner: &'a AppRunner<C>, key: &str) -> &'a AnimatedNode {
    let id = node_id_by_key(&runner.core.tree, key);
    match &runner.core.tree.node(id).kind {
        NodeKind::Animated(node) => node,
        _ => panic!("expected animated node"),
    }
}

#[test]
fn smooth_scroll_tracking_registers_active_nodes_and_drops_completed_transitions() {
    let mut runner = AppRunner::new(App::new().mouse(false), SmoothDocumentViewSmoke, ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    };
    init_runner(&mut runner, SmoothDocumentViewSmoke, viewport);

    assert!(runner.core.tree.has_animated_scrolls());
    assert_eq!(runner.core.tree.animated_scroll_ids().len(), 1);

    let (changed, needs_paint, needs_layout) =
        runner.update_smooth_scrolls(Duration::from_millis(100));

    assert!(changed);
    assert!(needs_paint);
    assert!(!needs_layout);
    assert!(!runner.core.tree.has_animated_scrolls());
    assert!(runner.core.tree.animated_scroll_ids().is_empty());
}

#[test]
fn smooth_scroll_view_tick_marks_layout_dirty_and_updates_offset() {
    let mut runner = AppRunner::new(App::new().mouse(false), SmoothScrollViewSmoke, ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    };
    init_runner(&mut runner, SmoothScrollViewSmoke, viewport);

    let id = node_id_by_key(&runner.core.tree, "smooth-scroll");
    assert!(runner.core.tree.has_animated_scrolls());

    let (changed, needs_paint, needs_layout) =
        runner.update_smooth_scrolls(Duration::from_millis(50));

    assert!(changed);
    assert!(!needs_paint);
    assert!(needs_layout);
    match &runner.core.tree.node(id).kind {
        NodeKind::ScrollView(node) => {
            assert!(node.offset > 0);
            assert_eq!(node.scroll_offset as usize, node.offset);
            assert_eq!(node.scroll_override, Some(node.offset));
        }
        _ => panic!("expected ScrollView node"),
    }
}

#[test]
fn smooth_wheel_scroll_tick_marks_layout_dirty_and_emits_scroll() {
    let offsets = Rc::new(RefCell::new(Vec::new()));
    let component = SmoothWheelScrollSmoke {
        offsets: offsets.clone(),
    };
    let mut runner = AppRunner::new(App::new().mouse(false), component.clone(), ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    };
    init_runner(&mut runner, component, viewport);

    let id = node_id_by_key(&runner.core.tree, "smooth-wheel-scroll");
    assert!(crate::app::input::handlers::scroll_view::handle_scroll(
        &mut runner.core.tree,
        id,
        crate::widgets::internal::ScrollAction::LineDown(1),
    ));
    assert!(runner.core.tree.has_animated_scrolls());

    let (changed, needs_paint, needs_layout) =
        runner.update_smooth_scrolls(Duration::from_millis(50));

    assert!(changed);
    assert!(!needs_paint);
    assert!(needs_layout);
    match &runner.core.tree.node(id).kind {
        NodeKind::ScrollView(node) => {
            assert!(node.offset > 0);
            assert_eq!(node.scroll_override, Some(node.offset));
            assert_eq!(offsets.borrow().as_slice(), &[node.offset]);
        }
        _ => panic!("expected ScrollView node"),
    }
}

#[test]
fn smooth_document_view_tick_marks_paint_dirty_and_updates_offset() {
    let mut runner = AppRunner::new(App::new().mouse(false), SmoothDocumentViewSmoke, ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    };
    init_runner(&mut runner, SmoothDocumentViewSmoke, viewport);

    let id = node_id_by_key(&runner.core.tree, "smooth-doc");
    let (changed, needs_paint, needs_layout) =
        runner.update_smooth_scrolls(Duration::from_millis(50));

    assert!(changed);
    assert!(needs_paint);
    assert!(!needs_layout);
    match &runner.core.tree.node(id).kind {
        NodeKind::DocumentView(node) => {
            assert!(node.scroll_offset > 0);
            assert_eq!(node.scroll_override, Some(node.scroll_offset));
        }
        _ => panic!("expected DocumentView node"),
    }
}

#[test]
fn smooth_text_area_tick_marks_paint_dirty_and_updates_offset() {
    let mut runner = AppRunner::new(App::new().mouse(false), SmoothTextAreaSmoke, ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    };
    init_runner(&mut runner, SmoothTextAreaSmoke, viewport);

    let id = node_id_by_key(&runner.core.tree, "smooth-text-area");
    let (changed, needs_paint, needs_layout) =
        runner.update_smooth_scrolls(Duration::from_millis(50));

    assert!(changed);
    assert!(needs_paint);
    assert!(!needs_layout);
    match &runner.core.tree.node(id).kind {
        NodeKind::TextArea(node) => {
            assert!(node.scroll_offset > 0);
            assert_eq!(node.scroll_override, Some(node.scroll_offset));
        }
        _ => panic!("expected TextArea node"),
    }
}

#[test]
fn update_animated_widgets_registers_and_ticks_all_animated_nodes_in_one_epoch() {
    let hidden = Rc::new(Cell::new(false));
    let mut runner = AppRunner::new(
        App::new().mouse(false),
        AnimatedPairSmoke {
            hidden: hidden.clone(),
        },
        (),
    );
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    };
    init_runner(
        &mut runner,
        AnimatedPairSmoke {
            hidden: hidden.clone(),
        },
        viewport,
    );

    hidden.set(true);
    runner.core.render_element(viewport, None, None, None);

    assert!(runner.core.tree.has_animated_widgets());
    assert_eq!(runner.core.tree.animated_widget_ids().len(), 2);

    let left_id = node_id_by_key(&runner.core.tree, "left");
    let right_id = node_id_by_key(&runner.core.tree, "right");

    match (
        &runner.core.tree.node(left_id).kind,
        &runner.core.tree.node(right_id).kind,
    ) {
        (NodeKind::Animated(left), NodeKind::Animated(right)) => {
            assert!(left.opacity_anim.is_some());
            assert!(right.opacity_anim.is_some());
            assert_eq!(left.opacity, 1.0);
            assert_eq!(right.opacity, 1.0);
        }
        _ => panic!("expected animated nodes"),
    }

    let (changed, needs_paint, needs_layout) =
        runner.update_animated_widgets(Duration::from_millis(50));

    assert!(changed);
    assert!(needs_paint);
    assert!(!needs_layout);
    assert_eq!(runner.core.tree.animated_widget_ids().len(), 2);

    match (
        &runner.core.tree.node(left_id).kind,
        &runner.core.tree.node(right_id).kind,
    ) {
        (NodeKind::Animated(left), NodeKind::Animated(right)) => {
            assert!((left.opacity - 0.5).abs() < 0.01);
            assert!((right.opacity - 0.5).abs() < 0.01);
        }
        _ => panic!("expected animated nodes"),
    }
}

#[test]
fn animated_height_transition_moves_through_intermediate_sizes_instead_of_snapping() {
    let open = Rc::new(Cell::new(false));
    let mut runner = AppRunner::new(
        App::new().mouse(false),
        AnimatedHeightSmoke { open: open.clone() },
        (),
    );
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 16,
    };
    init_runner(
        &mut runner,
        AnimatedHeightSmoke { open: open.clone() },
        viewport,
    );

    assert_eq!(animated_child_height(&runner, "panel"), 0);

    open.set(true);
    runner.core.render_element(viewport, None, None, None);
    assert_eq!(animated_child_height(&runner, "panel"), 0);

    let (_, _, needs_layout) = runner.update_animated_widgets(Duration::from_millis(50));
    assert!(needs_layout);
    runner.core.render_element(viewport, None, None, None);

    let mid_open = animated_child_height(&runner, "panel");
    assert!(mid_open > 0);
    assert!(mid_open < 5);

    let (_, _, needs_layout) = runner.update_animated_widgets(Duration::from_millis(50));
    assert!(needs_layout);
    runner.core.render_element(viewport, None, None, None);

    assert_eq!(animated_child_height(&runner, "panel"), 5);

    open.set(false);
    runner.core.render_element(viewport, None, None, None);
    assert_eq!(animated_child_height(&runner, "panel"), 5);

    let (_, _, needs_layout) = runner.update_animated_widgets(Duration::from_millis(50));
    assert!(needs_layout);
    runner.core.render_element(viewport, None, None, None);

    let mid_closed = animated_child_height(&runner, "panel");
    assert!(mid_closed > 0);
    assert!(mid_closed < 5);

    let (_, _, needs_layout) = runner.update_animated_widgets(Duration::from_millis(50));
    assert!(needs_layout);
    runner.core.render_element(viewport, None, None, None);

    assert_eq!(animated_child_height(&runner, "panel"), 0);
}

#[test]
fn animated_color_transition_marks_paint_dirty_without_layout_dirty() {
    let active = Rc::new(Cell::new(false));
    let mut runner = AppRunner::new(
        App::new().mouse(false),
        AnimatedColorSmoke {
            active: active.clone(),
        },
        (),
    );
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 4,
    };
    init_runner(
        &mut runner,
        AnimatedColorSmoke {
            active: active.clone(),
        },
        viewport,
    );

    active.set(true);
    runner.core.render_element(viewport, None, None, None);

    let color_id = node_id_by_key(&runner.core.tree, "color");
    match &runner.core.tree.node(color_id).kind {
        NodeKind::Animated(animated) => {
            assert!(animated.fg_anim.is_some());
            assert!(animated.bg_anim.is_some());
        }
        _ => panic!("expected animated node"),
    }

    let (changed, needs_paint, needs_layout) =
        runner.update_animated_widgets(Duration::from_millis(50));

    assert!(changed);
    assert!(needs_paint);
    assert!(!needs_layout);

    match &runner.core.tree.node(color_id).kind {
        NodeKind::Animated(animated) => {
            assert!(animated.current_fg.is_some());
            assert!(animated.current_bg.is_some());
            assert!(animated.fg_anim.is_some());
            assert!(animated.bg_anim.is_some());
        }
        _ => panic!("expected animated node"),
    }
}

#[test]
fn animated_position_transition_ticks_as_paint_only_and_keeps_final_rect() {
    let shifted = Rc::new(Cell::new(false));
    let ended = Rc::new(Cell::new(0));
    let component = AnimatedPositionSmoke {
        shifted: shifted.clone(),
        ended: ended.clone(),
        duration: Duration::from_millis(100),
    };
    let mut runner = AppRunner::new(App::new().mouse(false), component.clone(), ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    };
    init_runner(&mut runner, component.clone(), viewport);

    let moving_id = node_id_by_key(&runner.core.tree, "moving");
    let old_rect = runner.core.tree.node(moving_id).rect;
    assert_eq!(old_rect.x, 0);
    assert!(!runner.core.tree.has_animated_widgets());

    shifted.set(true);
    runner.core.render_element(viewport, None, None, None);

    let final_rect = runner.core.tree.node(moving_id).rect;
    assert_eq!(final_rect.x, 10);
    assert_eq!(final_rect.y, old_rect.y);
    assert_eq!(
        animated_node_by_key(&runner, "moving").visual_position_offset_cells(),
        (-10, 0)
    );
    assert!(runner.core.tree.has_animated_widgets());

    let (changed, needs_paint, needs_layout) =
        runner.update_animated_widgets(Duration::from_millis(50));

    assert!(changed);
    assert!(needs_paint);
    assert!(!needs_layout);
    assert_eq!(runner.core.tree.node(moving_id).rect, final_rect);
    assert_eq!(
        animated_node_by_key(&runner, "moving").visual_position_offset_cells(),
        (-5, 0)
    );
    assert_eq!(ended.get(), 0);

    let (changed, needs_paint, needs_layout) =
        runner.update_animated_widgets(Duration::from_millis(50));

    assert!(changed);
    assert!(needs_paint);
    assert!(!needs_layout);
    assert_eq!(runner.core.tree.node(moving_id).rect, final_rect);
    assert_eq!(
        animated_node_by_key(&runner, "moving").visual_position_offset_cells(),
        (0, 0)
    );
    assert!(!runner.core.tree.has_animated_widgets());
    assert_eq!(ended.get(), 1);
}

#[test]
fn animated_position_transition_is_not_reseeded_by_unchanged_reconcile() {
    let shifted = Rc::new(Cell::new(false));
    let ended = Rc::new(Cell::new(0));
    let component = AnimatedPositionSmoke {
        shifted: shifted.clone(),
        ended: ended.clone(),
        duration: Duration::from_millis(100),
    };
    let mut runner = AppRunner::new(App::new().mouse(false), component.clone(), ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    };
    init_runner(&mut runner, component.clone(), viewport);

    shifted.set(true);
    runner.core.render_element(viewport, None, None, None);
    let moving_id = node_id_by_key(&runner.core.tree, "moving");
    let final_rect = runner.core.tree.node(moving_id).rect;

    let (_, needs_paint, needs_layout) = runner.update_animated_widgets(Duration::from_millis(50));
    assert!(needs_paint);
    assert!(!needs_layout);
    assert_eq!(
        animated_node_by_key(&runner, "moving").visual_position_offset_cells(),
        (-5, 0)
    );

    runner.core.render_element(viewport, None, None, None);
    assert_eq!(runner.core.tree.node(moving_id).rect, final_rect);
    assert_eq!(
        animated_node_by_key(&runner, "moving").visual_position_offset_cells(),
        (-5, 0)
    );

    let (_, needs_paint, needs_layout) = runner.update_animated_widgets(Duration::from_millis(50));
    assert!(needs_paint);
    assert!(!needs_layout);
    assert_eq!(
        animated_node_by_key(&runner, "moving").visual_position_offset_cells(),
        (0, 0)
    );
    assert_eq!(ended.get(), 1);
}

#[test]
fn animated_position_retarget_to_current_visual_origin_clears_animation() {
    let leading_width = Rc::new(Cell::new(0));
    let ended = Rc::new(Cell::new(0));
    let component = AnimatedPositionRetargetSmoke {
        leading_width: leading_width.clone(),
        ended: ended.clone(),
        duration: Duration::from_millis(100),
    };
    let mut runner = AppRunner::new(App::new().mouse(false), component.clone(), ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    };
    init_runner(&mut runner, component.clone(), viewport);

    leading_width.set(10);
    runner.core.render_element(viewport, None, None, None);
    let (_, needs_paint, needs_layout) = runner.update_animated_widgets(Duration::from_millis(50));
    assert!(needs_paint);
    assert!(!needs_layout);
    assert_eq!(
        animated_node_by_key(&runner, "moving").visual_position_offset_cells(),
        (-5, 0)
    );
    assert_eq!(ended.get(), 0);

    leading_width.set(5);
    runner.core.render_element(viewport, None, None, None);
    let moving_id = node_id_by_key(&runner.core.tree, "moving");

    assert_eq!(runner.core.tree.node(moving_id).rect.x, 5);
    assert_eq!(
        animated_node_by_key(&runner, "moving").visual_position_offset_cells(),
        (0, 0)
    );
    assert!(!runner.core.tree.has_animated_widgets());
    assert_eq!(ended.get(), 1);
}

#[test]
fn animated_position_zero_duration_snaps_and_emits_callback() {
    let shifted = Rc::new(Cell::new(false));
    let ended = Rc::new(Cell::new(0));
    let component = AnimatedPositionSmoke {
        shifted: shifted.clone(),
        ended: ended.clone(),
        duration: Duration::ZERO,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), component.clone(), ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 4,
    };
    init_runner(&mut runner, component.clone(), viewport);

    shifted.set(true);
    runner.core.render_element(viewport, None, None, None);

    let moving_id = node_id_by_key(&runner.core.tree, "moving");
    assert_eq!(runner.core.tree.node(moving_id).rect.x, 10);
    assert_eq!(
        animated_node_by_key(&runner, "moving").visual_position_offset_cells(),
        (0, 0)
    );
    assert!(!runner.core.tree.has_animated_widgets());
    assert_eq!(ended.get(), 1);
}

struct AnimatedChromeSmoke {
    focused: Rc<Cell<bool>>,
    views: Rc<Cell<usize>>,
}

impl Component for AnimatedChromeSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        self.views.set(self.views.get() + 1);
        let target = if self.focused.get() {
            Color::Rgb(255, 255, 255)
        } else {
            Color::Rgb(0, 0, 0)
        };
        let paint = ctx.animated_color(
            "chrome",
            target,
            crate::animation::TransitionConfig {
                duration: Duration::from_millis(100),
                easing: crate::animation::Easing::Linear,
            },
        );
        Text::new("chrome").style(Style::new().fg(paint)).into()
    }
}

/// A colour that only feeds a style must fade without re-running `view()`: the element names the
/// transition, and the renderer resolves it while painting. This is what makes a focus fade cost
/// repaints instead of one rebuild of the window per frame of it.
#[test]
fn an_animated_colour_fades_without_a_view_pass() {
    let focused = Rc::new(Cell::new(false));
    let views = Rc::new(Cell::new(0));
    let mut backend = crate::TestBackend::new(AnimatedChromeSmoke {
        focused: focused.clone(),
        views: views.clone(),
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 1,
    });
    backend.render();

    // Retarget the fade, then let the view run once so the new target is registered.
    focused.set(true);
    backend.render();
    let views_at_start = views.get();
    let start = backend.capture_frame().cell(0, 0).fg;

    // Advancing the fade is a repaint: the ticker reports no view work, and the painted colour still
    // moves because the renderer resolves the slot.
    backend.advance(Duration::from_millis(50));
    assert_eq!(
        views.get(),
        views_at_start,
        "advancing a style-only fade must not run view()"
    );
    let midway = backend.capture_frame().cell(0, 0).fg;
    assert_ne!(midway, start, "the painted colour moved anyway");

    backend.advance(Duration::from_millis(60));
    let settled = backend.capture_frame().cell(0, 0).fg;
    assert_ne!(settled, midway, "and keeps moving to the target");
    assert_eq!(
        views.get(),
        views_at_start,
        "the whole fade cost no view passes"
    );
}

#[test]
fn moved_mouse_events_only_need_paint() {
    assert_eq!(
        mouse_dispatch_dirty_level(MouseKind::Moved, None, None, DirtyLevel::PaintOnly),
        DirtyLevel::PaintOnly
    );
}

/// Motion carries whatever the hover change costs, so a crossing that invalidates a view refreshes
/// that view rather than rebuilding the tree.
#[test]
fn moved_mouse_events_refresh_hover_dependent_views() {
    assert_eq!(
        mouse_dispatch_dirty_level(MouseKind::Moved, None, None, DirtyLevel::LayoutOnly),
        DirtyLevel::LayoutOnly
    );
}

#[test]
fn mouse_down_without_widget_dirty_level_requests_full_render() {
    assert_eq!(
        mouse_dispatch_dirty_level(
            MouseKind::Down(MouseButton::Left),
            None,
            None,
            DirtyLevel::PaintOnly
        ),
        DirtyLevel::Full
    );
}

#[test]
fn drag_drop_does_not_start_before_threshold() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    };
    let log = Rc::new(RefCell::new(Vec::new()));
    let component = DragDropSmoke {
        log: log.clone(),
        source_group: Some(Arc::from("cards")),
        left_group: Some(Arc::from("cards")),
        right_group: Some(Arc::from("cards")),
    };
    let mut runner = AppRunner::new(App::new().mouse(false), component.clone(), ());
    init_runner(&mut runner, component, viewport);

    let (sx, sy) = arm_drag_from_source(&mut runner);

    assert_eq!(runner.dispatch_active_drag(sx.saturating_add(1), sy), None);
    assert!(!runner.drag.is_active());
    assert!(!drag_source_dragging(&runner));

    assert_eq!(runner.handle_drag_release(sx.saturating_add(1), sy), None);
    assert!(log.borrow().is_empty());
}

#[test]
fn drag_drop_switches_hover_targets_and_drops() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    };
    let log = Rc::new(RefCell::new(Vec::new()));
    let component = DragDropSmoke {
        log: log.clone(),
        source_group: Some(Arc::from("cards")),
        left_group: Some(Arc::from("cards")),
        right_group: Some(Arc::from("cards")),
    };
    let mut runner = AppRunner::new(App::new().mouse(false), component.clone(), ());
    init_runner(&mut runner, component, viewport);

    let (_sx, _sy) = arm_drag_from_source(&mut runner);
    let (lx, ly) = node_center(&runner, "left-frame");
    let (rx, ry) = node_center(&runner, "right-frame");

    assert!(point_has_drop_target(&runner, lx, ly));
    let mut result = runner.dispatch_active_drag(lx, ly);
    if result == Some(false) {
        result = runner.dispatch_active_drag(lx, ly);
    }
    assert_eq!(result, Some(true));
    assert_eq!(
        log.borrow().clone(),
        vec!["start".to_string(), "over:left".to_string()]
    );
    assert!(drag_source_dragging(&runner));
    assert!(drop_target_highlighted(&runner, "left"));
    assert!(!drop_target_highlighted(&runner, "right"));

    assert!(point_has_drop_target(&runner, rx, ry));
    let mut result = runner.dispatch_active_drag(rx, ry);
    if result == Some(false) {
        result = runner.dispatch_active_drag(rx, ry);
    }
    assert_eq!(result, Some(true));
    assert_eq!(
        log.borrow().clone(),
        vec![
            "start".to_string(),
            "over:left".to_string(),
            "leave:left".to_string(),
            "over:right".to_string(),
        ]
    );
    assert!(drop_target_highlighted(&runner, "right"));
    assert!(!drop_target_highlighted(&runner, "left"));

    assert_eq!(runner.handle_drag_release(rx, ry), Some(true));
    assert_eq!(
        log.borrow().clone(),
        vec![
            "start".to_string(),
            "over:left".to_string(),
            "leave:left".to_string(),
            "over:right".to_string(),
            "drop:right:Card A".to_string(),
            "leave:right".to_string(),
        ]
    );
    assert!(!runner.drag.is_active());
    assert!(!drag_source_dragging(&runner));
    assert!(!drop_target_highlighted(&runner, "left"));
    assert!(!drop_target_highlighted(&runner, "right"));
}

#[test]
fn drag_drop_target_without_group_accepts_any_source_group() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    };
    let log = Rc::new(RefCell::new(Vec::new()));
    let component = DragDropSmoke {
        log: log.clone(),
        source_group: Some(Arc::from("cards")),
        left_group: None,
        right_group: Some(Arc::from("other")),
    };
    let mut runner = AppRunner::new(App::new().mouse(false), component.clone(), ());
    init_runner(&mut runner, component, viewport);

    let (_sx, _sy) = arm_drag_from_source(&mut runner);
    let (lx, ly) = node_center(&runner, "left-frame");

    assert!(point_has_drop_target(&runner, lx, ly));
    let mut result = runner.dispatch_active_drag(lx, ly);
    if result == Some(false) {
        result = runner.dispatch_active_drag(lx, ly);
    }
    assert_eq!(result, Some(true));
    assert_eq!(runner.handle_drag_release(lx, ly), Some(true));

    assert_eq!(
        log.borrow().clone(),
        vec![
            "start".to_string(),
            "over:left".to_string(),
            "drop:left:Card A".to_string(),
            "leave:left".to_string(),
        ]
    );
}

#[test]
fn cancel_drag_drop_emits_leave_and_cancel() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    };
    let log = Rc::new(RefCell::new(Vec::new()));
    let component = DragDropSmoke {
        log: log.clone(),
        source_group: Some(Arc::from("cards")),
        left_group: Some(Arc::from("cards")),
        right_group: Some(Arc::from("cards")),
    };
    let mut runner = AppRunner::new(App::new().mouse(false), component.clone(), ());
    init_runner(&mut runner, component, viewport);

    let (_sx, _sy) = arm_drag_from_source(&mut runner);
    let (lx, ly) = node_center(&runner, "left-frame");

    assert!(point_has_drop_target(&runner, lx, ly));
    let mut result = runner.dispatch_active_drag(lx, ly);
    if result == Some(false) {
        result = runner.dispatch_active_drag(lx, ly);
    }
    assert_eq!(result, Some(true));
    assert!(runner.cancel_drag_drop());

    assert_eq!(
        log.borrow().clone(),
        vec![
            "start".to_string(),
            "over:left".to_string(),
            "leave:left".to_string(),
            "cancel:Card A".to_string(),
        ]
    );
    assert!(!runner.drag.is_active());
    assert!(!drag_source_dragging(&runner));
    assert!(!drop_target_highlighted(&runner, "left"));
}

#[test]
fn mouse_up_preserves_active_drag_dirty_level() {
    assert_eq!(
        mouse_dispatch_dirty_level(
            MouseKind::Up(MouseButton::Left),
            Some(DirtyLevel::PaintOnly),
            None,
            DirtyLevel::PaintOnly,
        ),
        DirtyLevel::PaintOnly
    );
}

#[test]
fn effective_drag_dirty_level_promotes_autoscroll_to_layout() {
    let mut drag = DragState::default();
    drag.active =
        crate::app::runner::ActiveDrag::DocumentView(crate::app::input::drag::DocumentViewDrag {
            id: crate::core::node::NodeId::INVALID,
            anchor: crate::app::input::drag::DocumentViewDragAnchor::Linear(0),
            shared_selection_id: None,
            scroll_view_id: None,
            shared_drag_anchor: None,
        });

    assert_eq!(
        effective_active_drag_dirty_level(&drag),
        Some(DirtyLevel::PaintOnly)
    );

    drag.autoscroll_layout_dirty = true;
    assert_eq!(
        effective_active_drag_dirty_level(&drag),
        Some(DirtyLevel::LayoutOnly)
    );
}

#[test]
fn active_splitter_drag_requests_layout() {
    let mut drag = DragState::default();
    drag.active = crate::app::runner::ActiveDrag::Splitter(crate::app::input::drag::SplitterDrag {
        id: NodeId::INVALID,
        handle: 0,
        start_pos: 0,
        start_sizes: vec![10, 10],
        secondary: None,
    });

    assert_eq!(
        effective_active_drag_dirty_level(&drag),
        Some(DirtyLevel::LayoutOnly)
    );
}

#[test]
fn corner_click_finds_perpendicular_junction_splitter() {
    use crate::widgets::{Orientation, Spacer, Splitter};

    let inner = Splitter::horizontal()
        .weights(vec![0.5, 0.5])
        .child(Spacer::new())
        .child(Spacer::new());
    let root: crate::core::element::Element = Splitter::vertical()
        .weights(vec![0.5, 0.5])
        .child(Spacer::new())
        .child(inner)
        .into();

    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 41,
            h: 21,
        },
        None,
    );

    let mut vertical = None;
    let mut horizontal = None;
    for node in tree.iter() {
        if let crate::core::node::NodeKind::Splitter(s) = &node.kind {
            let entry = Some((node.id, s.handle_rects[0]));
            match s.orientation {
                Orientation::Vertical => vertical = entry,
                Orientation::Horizontal => horizontal = entry,
            }
        }
    }
    let (vertical_id, v_handle) = vertical.expect("outer vertical splitter");
    let (horizontal_id, h_handle) = horizontal.expect("inner horizontal splitter");

    // Junction: the vertical handle column cell on the horizontal handle row.
    let jx = v_handle.x as u16;
    let jy = h_handle.y as u16;

    let target = crate::app::input::drag::find_junction_splitter(
        &tree,
        vertical_id,
        Orientation::Vertical,
        jx,
        jy,
    )
    .expect("junction click grabs the perpendicular splitter");
    assert_eq!(target.id, horizontal_id);

    // Same vertical handle far from the junction stays single-axis.
    assert!(
        crate::app::input::drag::find_junction_splitter(
            &tree,
            vertical_id,
            Orientation::Vertical,
            jx,
            0,
        )
        .is_none()
    );

    // Symmetric: grabbing the horizontal handle beside the junction finds the
    // vertical splitter.
    let target = crate::app::input::drag::find_junction_splitter(
        &tree,
        horizontal_id,
        Orientation::Horizontal,
        jx + 1,
        jy,
    )
    .expect("junction click from the horizontal handle finds the vertical one");
    assert_eq!(target.id, vertical_id);
}

#[test]
fn text_area_gets_first_shot_at_tab_keys() {
    let root = TextArea::new("").into();
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
        None,
    );

    assert!(should_dispatch_text_area_tab_first(
        &tree,
        Some(tree.root),
        key(KeyCode::Tab),
    ));
    assert!(should_dispatch_text_area_tab_first(
        &tree,
        Some(tree.root),
        key(KeyCode::BackTab),
    ));
}

#[test]
fn non_text_area_widgets_keep_default_tab_focus_traversal() {
    let root = Input::new("").into();
    let mut tree = NodeTree::new();
    LayoutEngine::reconcile_with_focus(
        &mut tree,
        &root,
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 3,
        },
        None,
    );

    assert!(!should_dispatch_text_area_tab_first(
        &tree,
        Some(tree.root),
        key(KeyCode::Tab),
    ));
}

#[test]
fn manual_focus_policy_leaves_native_tab_unhandled() {
    let mut runner = App::new()
        .focus_policy(FocusPolicy::Manual)
        .mount(RunnerKeymapSmoke);
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    runner.core.ctx.set_viewport(viewport);
    runner.core.render_element(viewport, None, None, None);

    let result = runner.dispatch_layered_key(KeyEvent {
        code: KeyCode::Tab,
        mods: KeyMods::NONE,
    });

    assert!(!result.consumed);
    assert_eq!(runner.focus.focused, None);
}

#[test]
fn manual_focus_policy_ignores_queued_framework_traversal() {
    let mut runner = App::new()
        .focus_policy(FocusPolicy::Manual)
        .mount(RunnerKeymapSmoke);
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    };
    runner.core.ctx.set_viewport(viewport);
    runner.core.render_element(viewport, None, None, None);
    runner
        .framework_command_queue
        .borrow_mut()
        .push(FrameworkCommandAction::FocusNext);

    assert!(!runner.apply_framework_commands());
    assert_eq!(runner.focus.focused, None);
}

#[test]
fn default_runner_keymap_keeps_ctrl_q_quit_without_ctrl_c() {
    let runner = App::new().mouse(false).mount(RunnerKeymapSmoke);

    let ctrl_c = runner.keymap.matches(KeyEvent {
        code: KeyCode::Char('c'),
        mods: KeyMods {
            ctrl: true,
            ..KeyMods::default()
        },
    });
    assert!(
        ctrl_c
            .iter()
            .any(|binding| binding.action == crate::app::input::keymap::Action::Copy)
    );
    assert!(
        !ctrl_c
            .iter()
            .any(|binding| binding.action == crate::app::input::keymap::Action::Quit)
    );

    let ctrl_q = runner.keymap.matches(KeyEvent {
        code: KeyCode::Char('q'),
        mods: KeyMods {
            ctrl: true,
            ..KeyMods::default()
        },
    });
    assert!(
        ctrl_q
            .iter()
            .any(|binding| binding.action == crate::app::input::keymap::Action::Quit)
    );
}

#[test]
fn selected_document_view_copy_inside_scroll_view_is_paint_only() {
    let writes = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .clipboard_config(ClipboardConfig {
            enable_osc52: false,
            ..Default::default()
        })
        .clipboard_provider(RecordingClipboardProvider {
            writes: writes.clone(),
        });
    let mut runner = AppRunner::new(app, NestedScrollSelectionDocumentViewSmoke, ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 5,
    };
    init_runner(
        &mut runner,
        NestedScrollSelectionDocumentViewSmoke,
        viewport,
    );

    let document_view_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
        .map(|node| node.id)
        .expect("document view exists");
    if let NodeKind::DocumentView(node) = &mut runner.core.tree.node_mut(document_view_id).kind {
        node.selection_cursor = 4;
        node.selection_anchor = Some(0);
    }

    let dispatch = runner.dispatch_selection_clipboard_shortcut(ctrl_char('c'));

    assert!(dispatch.handled);
    assert_eq!(dispatch.dirty_override, Some(DirtyLevel::PaintOnly));
    assert_eq!(writes.borrow().as_slice(), &["zero"]);
    assert!(runner.copy_feedback.is_active(document_view_id));
}

#[test]
fn focused_document_view_copy_is_paint_only() {
    let writes = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .clipboard_config(ClipboardConfig {
            enable_osc52: false,
            ..Default::default()
        })
        .clipboard_provider(RecordingClipboardProvider {
            writes: writes.clone(),
        });
    let mut runner = AppRunner::new(app, ScrollSelectionDocumentViewSmoke, ());
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 5,
    };
    init_runner(&mut runner, ScrollSelectionDocumentViewSmoke, viewport);

    let document_view_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
        .map(|node| node.id)
        .expect("document view exists");
    runner.focus.focused = Some(document_view_id);
    if let NodeKind::DocumentView(node) = &mut runner.core.tree.node_mut(document_view_id).kind {
        node.selection_cursor = 4;
        node.selection_anchor = Some(0);
    }

    let dispatch = runner.dispatch_focused_key(ctrl_char('c'));

    assert!(dispatch.handled);
    assert_eq!(dispatch.dirty_override, Some(DirtyLevel::PaintOnly));
    assert_eq!(writes.borrow().as_slice(), &["zero"]);
    assert!(runner.copy_feedback.is_active(document_view_id));
}

#[test]
fn copy_feedback_expiry_marks_paint_only() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), RunnerKeymapSmoke, ());
    init_runner(&mut runner, RunnerKeymapSmoke, viewport);
    runner
        .copy_feedback
        .trigger(runner.core.tree.root, Duration::from_millis(2));

    let mut dirty = DirtyTracker::default();
    let poll_timeout = runner.update_animation_cycle(&mut dirty);

    assert_eq!(dirty.level(), DirtyLevel::None);
    assert!(poll_timeout <= Duration::from_millis(2));

    std::thread::sleep(Duration::from_millis(8));
    let mut dirty = DirtyTracker::default();
    runner.update_animation_cycle(&mut dirty);

    assert_eq!(dirty.level(), DirtyLevel::PaintOnly);
}

/// The reveal frame has to reconcile, not just repaint. A panel that appears on reveal is a subtree
/// the view did not previously return, and `DirtyLevel::PaintOnly` redraws the existing tree without
/// re-running `view` - so a paint-only reveal renders nothing at all, forever.
#[test]
fn a_chord_reveal_marks_a_full_frame_so_the_view_runs_again() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    };
    let delay = Duration::from_millis(20);
    let mut runner = AppRunner::new(
        App::new()
            .mouse(false)
            .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst)
            .command_chord_reveal_delay(delay),
        RunnerKeymapSmoke,
        (),
    );
    // The production path: `AppRunner::new` puts the app's delay in the env it built.
    assert_eq!(
        runner.core.ctx.env().command_chord_reveal_delay.get(),
        delay,
        "AppRunner::new must carry the configured delay into the runtime env"
    );
    // `init_runner` then swaps in a fresh `RuntimeCore`, and with it a fresh env, so the delay has
    // to be re-applied to the env actually under test below.
    init_runner(&mut runner, RunnerKeymapSmoke, viewport);
    runner.core.ctx.env().command_chord_reveal_delay.set(delay);
    runner.core.ctx.command_registry().register(
        crate::CommandEntry::builder("test.chord")
            .shortcut(crate::KeyBinding::from_str("ctrl-x q").expect("binding"))
            .handler(Callback::new(|_| {}))
            .build(),
    );

    runner.dispatch_layered_key(KeyEvent {
        code: KeyCode::Char('x'),
        mods: KeyMods::CTRL,
    });
    assert!(runner.core.ctx.command_chord_pending());

    // Before the delay: nothing to redraw, but the loop must wake in time to draw the reveal.
    let mut dirty = DirtyTracker::default();
    let poll_timeout = runner.update_animation_cycle(&mut dirty);
    assert_eq!(dirty.level(), DirtyLevel::None);
    assert!(poll_timeout <= delay);

    std::thread::sleep(delay + Duration::from_millis(10));
    let mut dirty = DirtyTracker::default();
    runner.update_animation_cycle(&mut dirty);
    assert_eq!(dirty.level(), DirtyLevel::Full);

    // Idempotent once revealed - the reveal is an edge, not a per-frame repaint.
    let mut dirty = DirtyTracker::default();
    runner.update_animation_cycle(&mut dirty);
    assert_eq!(dirty.level(), DirtyLevel::None);
}

#[cfg(feature = "devtools")]
#[test]
fn devtools_stats_panel_uses_compact_default_size() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 80,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), RunnerKeymapSmoke, ());
    runner.devtools_state.borrow_mut().set_visible(true);
    runner
        .core
        .set_extra_root_element(Some(crate::devtools::panel_element(Rc::clone(
            &runner.devtools_state,
        ))));
    runner.core.render_element(viewport, None, None, None);

    let panel_node = runner
        .core
        .tree
        .iter()
        .find(|node| {
            node.key
                .as_ref()
                .is_some_and(|key| key.as_ref() == crate::devtools::DEVTOOLS_KEY)
        })
        .expect("devtools panel node should exist");

    assert_eq!(panel_node.rect.w, 48);
    assert_eq!(panel_node.rect.h, 15);
    assert_eq!(panel_node.rect.y, 65); // anchored to bottom
}

#[cfg(feature = "devtools")]
#[test]
fn hidden_log_ingestion_keeps_snapshot_stale_until_devtools_visible() {
    let mut runner = AppRunner::new(App::new().mouse(false), RunnerKeymapSmoke, ());
    assert!(!runner.devtools_state.borrow().visible);

    {
        let mut queue = runner
            .devtools_log_queue
            .lock()
            .expect("devtools log queue should lock");
        queue.push_back(crate::debug::DevLogEntry {
            message: "hidden-entry".to_string(),
            source: crate::debug::LogSource::App,
        });
    }

    runner.ingest_pending_devtools_logs();

    {
        let state = runner.devtools_state.borrow();
        assert_eq!(state.log_buffer.len(), 1);
        assert_eq!(state.log_entries.len(), 0);
    }

    assert!(runner.set_devtools_visible(true));
    let state = runner.devtools_state.borrow();
    assert_eq!(state.log_entries.len(), 1);
}

#[cfg(feature = "devtools")]
#[test]
fn showing_devtools_does_not_sync_logs_when_paused() {
    let mut runner = AppRunner::new(App::new().mouse(false), RunnerKeymapSmoke, ());
    runner.devtools_state.borrow_mut().toggle_log_paused();

    {
        let mut queue = runner
            .devtools_log_queue
            .lock()
            .expect("devtools log queue should lock");
        queue.push_back(crate::debug::DevLogEntry {
            message: "paused-hidden-entry".to_string(),
            source: crate::debug::LogSource::App,
        });
    }
    runner.ingest_pending_devtools_logs();
    assert_eq!(runner.devtools_state.borrow().log_entries.len(), 0);

    assert!(runner.set_devtools_visible(true));
    assert_eq!(runner.devtools_state.borrow().log_entries.len(), 0);
}

#[cfg(feature = "devtools")]
#[test]
fn disabled_log_ingestion_drains_queue_without_touching_state() {
    let mut runner = AppRunner::new(
        App::new().mouse(false).devtools_config(DevToolsConfig {
            logs: false,
            metrics: true,
            show_framework_logs: true,
        }),
        RunnerKeymapSmoke,
        (),
    );

    {
        let mut queue = runner
            .devtools_log_queue
            .lock()
            .expect("devtools log queue should lock");
        queue.push_back(crate::debug::DevLogEntry {
            message: "discard-me".to_string(),
            source: crate::debug::LogSource::App,
        });
    }

    runner.ingest_pending_devtools_logs();

    let queue = runner
        .devtools_log_queue
        .lock()
        .expect("devtools log queue should lock");
    assert!(queue.is_empty());
    let state = runner.devtools_state.borrow();
    assert!(state.log_buffer.is_empty());
    assert!(state.log_entries.is_empty());
}

#[cfg(feature = "devtools")]
#[test]
fn devtools_logs_panel_uses_full_width_default_size() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 80,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), RunnerKeymapSmoke, ());
    {
        let mut state = runner.devtools_state.borrow_mut();
        state.set_visible(true);
        state.set_active_tab(1); // Logs tab
        for i in 0..500 {
            state.push_log_entry(crate::devtools::DevLogEntry {
                timestamp: std::time::SystemTime::now(),
                message: format!("log entry number {i} with some extra text to make it longer"),
                source: crate::debug::LogSource::App,
            });
        }
    }
    runner
        .core
        .set_extra_root_element(Some(crate::devtools::panel_element(Rc::clone(
            &runner.devtools_state,
        ))));
    runner.core.render_element(viewport, None, None, None);

    let panel_node = runner
        .core
        .tree
        .iter()
        .find(|node| {
            node.key
                .as_ref()
                .is_some_and(|key| key.as_ref() == crate::devtools::DEVTOOLS_KEY)
        })
        .expect("devtools panel node should exist");

    assert_eq!(panel_node.rect.w, 80);
    assert_eq!(panel_node.rect.h, 26);
    assert_eq!(panel_node.rect.y, 54);
}

#[cfg(feature = "devtools")]
#[test]
fn fill_background_uses_app_root_theme_when_devtools_extra_root_is_present() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 20,
    };
    let stale = Color::Rgb(1, 2, 3);
    let live = Color::Rgb(9, 8, 7);
    let mut app_theme = Theme::default();
    app_theme.surface.backdrop = stale;
    let backdrop = Rc::new(Cell::new(live));
    let mut runner = AppRunner::new(
        App::new().mouse(false).theme(app_theme).fill_background(),
        RunnerDynamicThemeSmoke {
            backdrop: Rc::clone(&backdrop),
        },
        (),
    );
    runner.devtools_state.borrow_mut().set_visible(true);
    runner
        .core
        .set_extra_root_element(Some(crate::devtools::panel_element(Rc::clone(
            &runner.devtools_state,
        ))));

    runner.core.render_element(viewport, None, None, None);

    let resolved = runner
        .resolved_screen_background()
        .expect("fill background should resolve");
    assert_eq!(
        resolved.bg,
        Some(crate::backend::ratatui_backend::common::to_ratatui_color(
            live
        ))
    );
    assert_ne!(
        resolved.bg,
        Some(crate::backend::ratatui_backend::common::to_ratatui_color(
            stale
        ))
    );
}

#[cfg(feature = "devtools")]
#[test]
fn devtools_catch_up_frame_records_no_metrics_and_terminates() {
    use super::render_service::DrawMode;

    let dur = Duration::from_millis(1);
    let mut runner = AppRunner::new(App::new().mouse(false), RunnerKeymapSmoke, ());
    runner.devtools_state.borrow_mut().set_visible(true);

    // An app frame records metrics and arms exactly one catch-up refresh.
    assert!(!runner.devtools_refresh_pending);
    runner.record_devtools_frame_metrics(DrawMode::Full, dur, dur, dur);
    assert_eq!(runner.devtools_state.borrow().frame_history.len(), 1);
    assert!(
        runner.devtools_refresh_pending,
        "recording a frame should request one catch-up refresh"
    );

    // The loop consumes the flag and runs a suppressed catch-up frame.
    let refresh = std::mem::take(&mut runner.devtools_refresh_pending);
    assert!(refresh);
    runner.devtools_metrics_suppressed = true;

    // The catch-up frame rebuilds the panel (with the metric above) but records
    // nothing and does not re-arm the flag, so the refresh stops after one frame.
    runner.record_devtools_frame_metrics(DrawMode::Full, dur, dur, dur);
    assert_eq!(
        runner.devtools_state.borrow().frame_history.len(),
        1,
        "catch-up frame must not push a metric"
    );
    assert!(
        !runner.devtools_refresh_pending,
        "catch-up frame must not re-arm the refresh (no infinite loop)"
    );
}

#[cfg(feature = "devtools")]
#[test]
fn attribution_records_component_full_update() {
    use super::render_service::DrawMode;
    use crate::devtools::state::UpdateSource;

    #[derive(Clone)]
    enum Msg {
        Bump,
    }

    struct AttributionFullProbe;

    impl Component for AttributionFullProbe {
        type Message = Msg;
        type Properties = ();
        type State = u32;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Bump => {
                    ctx.state = ctx.state.wrapping_add(1);
                    Update::full()
                }
            }
        }

        fn view(&self, ctx: &Context<Self>) -> crate::core::element::Element {
            Text::new(format!("count={}", ctx.state)).into()
        }
    }

    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    };
    let mut runner = AppRunner::new(
        App::new().mouse(false).devtools_config(DevToolsConfig {
            logs: false,
            metrics: true,
            show_framework_logs: false,
        }),
        AttributionFullProbe,
        (),
    );
    init_runner(&mut runner, AttributionFullProbe, viewport);
    runner.devtools_state.borrow_mut().set_visible(true);

    runner
        .core
        .queue
        .borrow_mut()
        .push_back((ScopeId(1), Box::new(Msg::Bump)));

    let mut dirty = DirtyTracker::default();
    runner
        .process_pending_messages(&mut dirty)
        .expect("full update message should process");
    assert_eq!(dirty.level(), DirtyLevel::Full);

    let dur = Duration::from_millis(1);
    runner.record_devtools_frame_metrics(DrawMode::Full, dur, dur, dur);

    let frame = runner
        .devtools_state
        .borrow()
        .latest_frame()
        .cloned()
        .expect("frame metrics should be recorded");
    assert!(
        frame.attributions.iter().any(|entry| {
            matches!(
                &entry.source,
                UpdateSource::Component { name, .. } if name.as_ref() == "AttributionFullProbe"
            ) && entry.level == DirtyLevel::Full
                && entry.count >= 1
        }),
        "expected AttributionFullProbe Full attribution, got {:?}",
        frame.attributions
    );
}

struct HoverPaintSmoke;

impl Component for HoverPaintSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        HStack::new()
            .children([
                // Click-only: hoverable, but nothing about it paints differently.
                MouseRegion::new()
                    .on_click(ctx.link().callback(|_: MouseEvent| ()))
                    .child(Text::new("aaaaa"))
                    .key("click-only-a"),
                MouseRegion::new()
                    .on_click(ctx.link().callback(|_: MouseEvent| ()))
                    .child(Text::new("bbbbb"))
                    .key("click-only-b"),
                MouseRegion::new()
                    .on_click(ctx.link().callback(|_: MouseEvent| ()))
                    .hover_style(Style::new().bg(Color::Blue))
                    .child(Text::new("ccccc"))
                    .key("hover-styled"),
            ])
            .into()
    }
}

/// Motion across click-only mouse regions must not repaint. The hover node
/// changes on every boundary crossing, but neither side renders differently
/// while hovered, so a repaint there is pure waste — and with a masked
/// `hit_test` the hover flaps on every gap, repainting many times per second.
#[test]
fn hover_transition_between_click_only_regions_is_not_dirty() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 15,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), HoverPaintSmoke, ());
    init_runner(&mut runner, HoverPaintSmoke, viewport);

    assert!(!runner.update_hover(1, 0), "entering click-only a");
    assert!(!runner.update_hover(6, 0), "crossing into click-only b");
    assert!(
        runner.update_hover(11, 0),
        "entering the hover-styled region"
    );
    assert!(runner.update_hover(1, 0), "leaving the hover-styled region");
}

/// A tab strip beside a keyed region whose look depends on hover — the shape of an app with a tab
/// bar across the top and a sidebar whose rows reveal a hover-only affordance.
struct KeyedHoverDependencySmoke;

impl Component for KeyedHoverDependencySmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        // Reading hover for one keyed region must not price motion everywhere else in the window.
        let row_hovered = ctx.has_hover_within_key("row");
        HStack::new()
            .children([
                crate::widgets::Tabs::new()
                    .tabs(["one", "two"].map(crate::widgets::Tab::new))
                    .tab_hover_style(Style::new().bg(Color::Blue))
                    .width(Length::Px(11))
                    .into(),
                MouseRegion::new()
                    .on_click(ctx.link().callback(|_: MouseEvent| ()))
                    .child(Text::new(if row_hovered { "row x" } else { "row  " }))
                    .key("row"),
            ])
            .into()
    }
}

/// Motion the views do not care about must stay a repaint even when some view reads hover.
///
/// The blunt version of this check asked only whether *any* view reads hover, so one keyed
/// `has_hover_within_key` call promoted every pointer crossing in the window to a full rebuild —
/// visible in a multiplexer as "opening the sidebar makes hovering the tab bar expensive".
#[test]
fn a_hover_change_outside_every_queried_key_stays_a_repaint() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), KeyedHoverDependencySmoke, ());
    init_runner(&mut runner, KeyedHoverDependencySmoke, viewport);

    // Crossing between tabs: the strip repaints its own highlight, and the keyed region's answer is
    // unchanged, so no view has to run.
    assert!(runner.update_hover(1, 0), "entering the first tab");
    assert_eq!(
        runner.motion_hover_dirty_level(),
        DirtyLevel::PaintOnly,
        "a crossing outside every queried key needs no view pass"
    );
    assert!(runner.update_hover(7, 0), "crossing onto the second tab");
    assert_eq!(
        runner.motion_hover_dirty_level(),
        DirtyLevel::PaintOnly,
        "and neither does the next one"
    );
    assert!(
        runner.dirty_component_scopes.is_empty(),
        "a repaint marks no scope for refresh"
    );

    // Reaching the keyed region does change what its view would produce - but only that view, so
    // the frame is a layout pass over the asking scope rather than a rebuild of everything.
    let row = node_id_by_key(&runner.core.tree, "row");
    let row_x = runner.core.tree.node(row).rect.x as u16;
    assert!(runner.update_hover(row_x, 0), "entering the keyed region");
    assert_eq!(
        runner.motion_hover_dirty_level(),
        DirtyLevel::LayoutOnly,
        "the queried key now answers differently, so its view must run"
    );
    assert_eq!(
        runner.dirty_component_scopes.as_slice(),
        &[ScopeId(1)],
        "and the scope that asked is marked, which is what makes the layout pass legal"
    );
}

/// The text rendered under the node carrying `key`.
fn text_under_key<C: Component>(runner: &AppRunner<C>, key: &str) -> String {
    let tree = &runner.core.tree;
    let mut out = String::new();
    let mut stack = vec![node_id_by_key(tree, key)];
    while let Some(id) = stack.pop() {
        if !tree.is_valid(id) {
            continue;
        }
        let node = tree.node(id);
        if let NodeKind::Text(text) = &node.kind {
            for span in &text.spans {
                out.push_str(&span.content);
            }
        }
        stack.extend(node.children.iter().rev().copied());
    }
    out
}

/// The frame a `LayoutOnly` hover crossing asks for: refresh the marked scopes, then reconcile.
fn render_layout_only_frame<C: Component>(runner: &mut AppRunner<C>, viewport: Rect) {
    let scopes = std::mem::take(&mut runner.dirty_component_scopes);
    runner.dirty_scope_set.clear();
    assert!(
        runner
            .core
            .refresh_cached_scopes(&scopes, viewport, runner.mouse.hovered),
        "the marked scopes should refresh"
    );
    assert!(
        runner.core.reconcile_cached_element(
            viewport,
            runner.focus.focused,
            None,
            runner.mouse.hovered,
        ),
        "the refreshed element should reconcile"
    );
}

/// The layout pass a crossing asks for has to actually land, twice over: the view it re-runs must
/// see the hover position that made it dirty, and the questions asked during that partial pass must
/// outlive the frame so the *next* crossing is priced as a view pass too.
///
/// Losing either half leaves the tree describing where the pointer used to be — in a multiplexer,
/// a sidebar row whose hover styling drops the moment the pointer reaches the close button nested
/// inside it, and never comes back until something else forces a full render.
#[test]
fn a_hover_layout_frame_sees_the_new_hover_and_keeps_its_questions() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), KeyedHoverDependencySmoke, ());
    init_runner(&mut runner, KeyedHoverDependencySmoke, viewport);

    let row = node_id_by_key(&runner.core.tree, "row");
    let row_x = runner.core.tree.node(row).rect.x as u16;
    assert_eq!(text_under_key(&runner, "row"), "row  ", "resting");

    assert!(runner.update_hover(row_x, 0), "entering the keyed region");
    assert_eq!(runner.motion_hover_dirty_level(), DirtyLevel::LayoutOnly);
    render_layout_only_frame(&mut runner, viewport);
    assert_eq!(
        text_under_key(&runner, "row"),
        "row x",
        "the refreshed view builds against the hover that dirtied it"
    );

    assert!(runner.update_hover(1, 0), "leaving for the tab strip");
    assert_eq!(
        runner.motion_hover_dirty_level(),
        DirtyLevel::LayoutOnly,
        "the question asked during the partial pass still prices the next crossing"
    );
    render_layout_only_frame(&mut runner, viewport);
    assert_eq!(
        text_under_key(&runner, "row"),
        "row  ",
        "and leaving takes the affordance back off"
    );
}

struct TabStripHoverSmoke;

impl Component for TabStripHoverSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        // " one " | " two " | " three ": 5, 5 and 7 columns with single-column dividers between.
        crate::widgets::Tabs::new()
            .tabs(["one", "two", "three"].map(crate::widgets::Tab::new))
            .tab_hover_style(Style::new().bg(Color::Blue))
            .into()
    }
}

/// A tab strip must report hover dirt when the pointer moves to a *different* tab, not on every
/// motion event that happens to be over it. A strip spanning the top of a window otherwise makes
/// every pointer movement anywhere along it repaint the whole tree.
#[test]
fn tab_hover_repaints_across_dividers_in_both_directions() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), TabStripHoverSmoke, ());
    init_runner(&mut runner, TabStripHoverSmoke, viewport);

    assert!(runner.update_hover(1, 0), "entering the first tab");
    assert_eq!(runner.mouse.hovered_item_index, Some(0));
    assert!(
        !runner.update_hover(2, 0),
        "sliding within the first tab paints nothing new"
    );
    assert!(
        !runner.update_hover(3, 0),
        "still within the first tab: no repaint"
    );

    assert!(runner.update_hover(5, 0), "crossing onto the first divider");
    assert_eq!(runner.mouse.hovered_item_index, None);
    assert!(
        runner.update_hover(6, 0),
        "crossing from the first divider onto the second tab"
    );
    assert_eq!(runner.mouse.hovered_item_index, Some(1));
    assert!(!runner.update_hover(8, 0), "sliding within the second tab");

    assert!(
        runner.update_hover(11, 0),
        "crossing onto the second divider"
    );
    assert_eq!(runner.mouse.hovered_item_index, None);
    assert!(
        runner.update_hover(12, 0),
        "crossing from the second divider onto the third tab"
    );
    assert_eq!(runner.mouse.hovered_item_index, Some(2));
    assert!(
        !runner.update_hover(17, 0),
        "sliding across the whole third tab paints nothing new"
    );

    assert!(
        runner.update_hover(11, 0),
        "crossing back onto the second divider"
    );
    assert_eq!(runner.mouse.hovered_item_index, None);
    assert!(
        runner.update_hover(10, 0),
        "crossing back from the second divider onto the second tab"
    );
    assert_eq!(runner.mouse.hovered_item_index, Some(1));
    assert!(
        runner.update_hover(5, 0),
        "crossing back onto the first divider"
    );
    assert_eq!(runner.mouse.hovered_item_index, None);
    assert!(
        runner.update_hover(4, 0),
        "crossing back from the first divider onto the first tab"
    );
    assert_eq!(runner.mouse.hovered_item_index, Some(0));
}

struct DraggableTabStripHoverSmoke;

impl Component for DraggableTabStripHoverSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        crate::widgets::DraggableTabBar::new()
            .tabs([
                crate::widgets::DraggableTab::new("one").closeable(true),
                crate::widgets::DraggableTab::new("two").closeable(true),
                crate::widgets::DraggableTab::new("three").closeable(true),
            ])
            .show_close_buttons(true)
            .tab_hover_style(Style::new().bg(Color::Blue))
            .close_hover_style(Style::new().fg(Color::Red))
            .into()
    }
}

#[test]
fn draggable_tab_motion_is_dirty_only_when_the_painted_target_changes() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), DraggableTabStripHoverSmoke, ());
    init_runner(&mut runner, DraggableTabStripHoverSmoke, viewport);

    let bar_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DraggableTabBar(_)))
        .map(|node| node.id)
        .expect("draggable tab bar node");
    let node = runner.core.tree.node(bar_id);
    let NodeKind::DraggableTabBar(bar) = &node.kind else {
        unreachable!();
    };
    let inner = node.rect.inner(bar.border, bar.padding);
    let layout = crate::widgets::DraggableTabBar::viewport_layout(
        &bar.tabs,
        &bar.display_options(),
        &bar.viewport_options(inner.w as usize),
    );
    let first = &layout.visible_tabs[0];
    let second = &layout.visible_tabs[1];
    let first_close_local = first.metrics.close_start.expect("first close slot");
    let first_body_x = inner.x as u16 + first.start as u16;
    let first_close_x = inner.x as u16
        + first.start as u16
        + first_close_local.saturating_sub(first.clip_left) as u16;
    let second_body_x = inner.x as u16 + second.start as u16;

    assert!(runner.update_hover(first_body_x, inner.y as u16));
    assert_eq!(
        runner.mouse.hover_paint_target,
        Some(
            crate::app::interaction_state::HoverPaintTarget::DraggableTabBar(
                crate::widgets::draggable_tab_bar::DraggableTabHitTarget::Tab(
                    crate::widgets::draggable_tab_bar::DraggableTabHit {
                        index: 0,
                        part: crate::widgets::DraggableTabHitPart::Body,
                    },
                ),
            ),
        )
    );
    assert!(
        !runner.update_hover(first_body_x + 1, inner.y as u16),
        "motion within the first tab body changes no painted state"
    );

    assert!(
        runner.update_hover(first_close_x, inner.y as u16),
        "the close affordance has its own hover style"
    );
    assert_eq!(
        runner.mouse.hover_paint_target,
        Some(
            crate::app::interaction_state::HoverPaintTarget::DraggableTabBar(
                crate::widgets::draggable_tab_bar::DraggableTabHitTarget::Tab(
                    crate::widgets::draggable_tab_bar::DraggableTabHit {
                        index: 0,
                        part: crate::widgets::DraggableTabHitPart::Close,
                    },
                ),
            ),
        )
    );

    assert!(runner.update_hover(second_body_x, inner.y as u16));
    assert_eq!(
        runner.mouse.hover_paint_target,
        Some(
            crate::app::interaction_state::HoverPaintTarget::DraggableTabBar(
                crate::widgets::draggable_tab_bar::DraggableTabHitTarget::Tab(
                    crate::widgets::draggable_tab_bar::DraggableTabHit {
                        index: 1,
                        part: crate::widgets::DraggableTabHitPart::Body,
                    },
                ),
            ),
        )
    );
}

#[test]
fn draggable_tab_width_lock_clears_when_pointer_leaves_bar() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 2,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), DraggableTabStripHoverSmoke, ());
    init_runner(&mut runner, DraggableTabStripHoverSmoke, viewport);

    let bar_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DraggableTabBar(_)))
        .map(|node| node.id)
        .expect("draggable tab bar node");
    assert!(runner.update_hover(1, 0));
    let NodeKind::DraggableTabBar(bar) = &mut runner.core.tree.node_mut(bar_id).kind else {
        unreachable!();
    };
    bar.width_lock = Some(crate::widgets::draggable_tab_bar::TabWidthLock {
        index: 0,
        width: 12,
    });

    assert!(runner.update_hover(1, 1));
    let NodeKind::DraggableTabBar(bar) = &runner.core.tree.node(bar_id).kind else {
        unreachable!();
    };
    assert_eq!(bar.width_lock, None);
}

struct TextAreaSentinelHoverSmoke;

impl Component for TextAreaSentinelHoverSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let mut sentinels = Vec::new();
        let (value, cursor) = crate::widgets::insert_sentinel(
            "",
            0,
            &mut sentinels,
            crate::widgets::TextAreaSentinel::new("FIRST")
                .hover_style(Style::new().bg(Color::Blue)),
        );
        let (value, _) = crate::widgets::insert_sentinel(
            &value,
            cursor,
            &mut sentinels,
            crate::widgets::TextAreaSentinel::new("SECOND")
                .hover_style(Style::new().bg(Color::Red)),
        );
        TextArea::new(value)
            .sentinels(sentinels)
            .read_only(true)
            .into()
    }
}

#[test]
fn text_area_sentinel_motion_is_dirty_only_between_placeholders() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), TextAreaSentinelHoverSmoke, ());
    init_runner(&mut runner, TextAreaSentinelHoverSmoke, viewport);

    let text_area_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .expect("text area node");
    let node = runner.core.tree.node(text_area_id);
    let NodeKind::TextArea(text_area) = &node.kind else {
        unreachable!();
    };
    let content = text_area.metrics(node.rect).content_rect;
    let second_byte = text_area
        .value
        .char_indices()
        .nth(1)
        .map(|(byte, _)| byte)
        .expect("second sentinel byte");

    assert!(runner.update_hover(content.x as u16, content.y as u16));
    assert_eq!(
        runner.mouse.hover_paint_target,
        Some(crate::app::interaction_state::HoverPaintTarget::TextArea {
            sentinel_byte: Some(0),
            diff_separator_source_line: None,
        },)
    );
    assert!(
        !runner.update_hover(content.x as u16 + 3, content.y as u16),
        "motion within the wide FIRST placeholder changes nothing"
    );

    assert!(
        runner.update_hover(content.x as u16 + 5, content.y as u16),
        "crossing to SECOND changes the painted placeholder"
    );
    assert_eq!(
        runner.mouse.hover_paint_target,
        Some(crate::app::interaction_state::HoverPaintTarget::TextArea {
            sentinel_byte: Some(second_byte),
            diff_separator_source_line: None,
        },)
    );
    assert!(
        !runner.update_hover(content.x as u16 + 8, content.y as u16),
        "motion within SECOND is quiet"
    );
}

#[cfg(feature = "diff-view")]
struct DocumentDiffSeparatorHoverSmoke;

#[cfg(feature = "diff-view")]
impl Component for DocumentDiffSeparatorHoverSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let before: String = (1..=30).map(|line| format!("line {line}\n")).collect();
        let mut after_lines: Vec<String> = (1..=30).map(|line| format!("line {line}\n")).collect();
        after_lines[14] = "changed\n".to_owned();
        crate::widgets::DiffView::with_content(before, after_lines.concat())
            .mode(crate::widgets::DiffViewMode::Unified)
            .backend(crate::widgets::DiffViewBackend::DocumentView)
            .context_lines(1)
            .context_separator_hover_style(Style::new().bg(Color::Blue))
            .into()
    }
}

#[cfg(feature = "diff-view")]
#[test]
fn document_diff_separator_motion_is_dirty_only_between_separator_rows() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 80,
        h: 30,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), DocumentDiffSeparatorHoverSmoke, ());
    init_runner(&mut runner, DocumentDiffSeparatorHoverSmoke, viewport);

    let document_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
        .map(|node| node.id)
        .expect("document view node");
    let node = runner.core.tree.node(document_id);
    let NodeKind::DocumentView(document) = &node.kind else {
        unreachable!();
    };
    let config = document
        .diff_context_separator_click
        .as_ref()
        .expect("separator hover config");
    let separator_rows: Vec<_> = document
        .visual_cache
        .source_line_map
        .iter()
        .enumerate()
        .filter(|(_, source_line)| {
            config
                .events_by_source_line
                .get(**source_line)
                .is_some_and(Option::is_some)
        })
        .map(|(visual_row, source_line)| (visual_row, *source_line))
        .collect();
    assert!(separator_rows.len() >= 2, "expected two context separators");
    let inner = node.rect.inner(document.border, document.padding);
    let content = document.content_layout(inner);
    let first = separator_rows[0];
    let second = separator_rows[1];
    let first_y = inner.y as u16 + first.0 as u16;
    let second_y = inner.y as u16 + second.0 as u16;
    let x = content.content_x as u16;

    assert!(runner.update_hover(x, first_y));
    assert_eq!(
        runner.mouse.hover_paint_target,
        Some(
            crate::app::interaction_state::HoverPaintTarget::DocumentViewDiffSeparator {
                source_line: first.1,
            },
        )
    );
    assert!(
        !runner.update_hover(x + 5, first_y),
        "motion within one separator row is quiet"
    );

    assert!(runner.update_hover(x, second_y));
    assert_eq!(
        runner.mouse.hover_paint_target,
        Some(
            crate::app::interaction_state::HoverPaintTarget::DocumentViewDiffSeparator {
                source_line: second.1,
            },
        )
    );
}

#[test]
fn refresh_hover_tracks_item_hover_during_scroll() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), ScrollHoverSmoke, ());
    init_runner(&mut runner, ScrollHoverSmoke, viewport);

    assert!(runner.update_hover(1, 0));
    assert!(runner.refresh_hover_from_last_mouse());
    assert_eq!(runner.mouse.hovered_item_index, Some(0));

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 1,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        1,
    ));

    assert_eq!(runner.mouse.hovered_item_index, Some(1));
}

#[test]
fn dispatch_mouse_scroll_applies_configured_multiplier() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(
        App::new().mouse(false).scroll_wheel_multiplier(3),
        ScrollMultiplierSmoke,
        (),
    );
    init_runner(&mut runner, ScrollMultiplierSmoke, viewport);

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 1,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        1,
    ));

    let offset = runner
        .core
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::List(list) => Some(list.offset),
            _ => None,
        })
        .expect("list exists");
    assert_eq!(
        offset, 3,
        "single wheel tick should apply the configured multiplier"
    );
}

#[test]
fn dispatch_mouse_scroll_uses_scroll_view_multiplier_override() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(
        App::new().mouse(false).scroll_wheel_multiplier(5),
        ScrollViewMultiplierSmoke,
        (),
    );
    init_runner(&mut runner, ScrollViewMultiplierSmoke, viewport);

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 1,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        1,
    ));

    let offset = runner
        .core
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::ScrollView(scroll) => Some(scroll.offset),
            _ => None,
        })
        .expect("scroll view exists");
    assert_eq!(offset, 2);
}

#[test]
fn dispatch_mouse_scroll_uses_text_area_multiplier_override() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(
        App::new().mouse(false).scroll_wheel_multiplier(5),
        TextAreaMultiplierSmoke,
        (),
    );
    init_runner(&mut runner, TextAreaMultiplierSmoke, viewport);

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 1,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        1,
    ));

    let offset = runner
        .core
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::TextArea(text_area) => Some(text_area.scroll_offset),
            _ => None,
        })
        .expect("text area exists");
    assert_eq!(offset, 2);
}

#[test]
fn dispatch_mouse_scroll_uses_document_view_multiplier_override() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(
        App::new().mouse(false).scroll_wheel_multiplier(5),
        DocumentViewMultiplierSmoke,
        (),
    );
    init_runner(&mut runner, DocumentViewMultiplierSmoke, viewport);

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 1,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        1,
    ));

    let offset = runner
        .core
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::DocumentView(document_view) => Some(document_view.scroll_offset),
            _ => None,
        })
        .expect("document view exists");
    assert_eq!(offset, 2);
}

#[test]
fn scroll_wheel_refreshes_textarea_drag_selection_without_mouse_motion() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), ScrollSelectionTextAreaSmoke, ());
    init_runner(&mut runner, ScrollSelectionTextAreaSmoke, viewport);

    let text_area_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .expect("text area exists");

    runner.drag.active =
        crate::app::runner::ActiveDrag::TextArea(crate::app::input::drag::TextAreaDrag {
            id: text_area_id,
            anchor: 0,
        });

    if let NodeKind::TextArea(node) = &mut runner.core.tree.node_mut(text_area_id).kind {
        node.cursor = 0;
        node.anchor = Some(0);
    }

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 0,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        1,
    ));

    let (scroll_offset, cursor, anchor) = match &runner.core.tree.node(text_area_id).kind {
        NodeKind::TextArea(node) => (node.scroll_offset, node.cursor, node.anchor),
        _ => unreachable!(),
    };

    assert_eq!(scroll_offset, 1);
    assert_eq!(cursor, 5);
    assert_eq!(anchor, Some(0));
}

#[test]
fn dispatch_mouse_scroll_without_active_drag_does_not_seed_textarea_drag_pointer() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), ScrollSelectionTextAreaSmoke, ());
    init_runner(&mut runner, ScrollSelectionTextAreaSmoke, viewport);

    let text_area_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .expect("text area exists");

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 0,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        1,
    ));

    let scroll_offset = match &runner.core.tree.node(text_area_id).kind {
        NodeKind::TextArea(node) => node.scroll_offset,
        _ => unreachable!(),
    };

    assert_eq!(scroll_offset, 1);
    assert_eq!(runner.drag.last_pointer_pos, None);
}

#[test]
fn scroll_wheel_refreshes_document_view_drag_selection_without_mouse_motion() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(
        App::new().mouse(false),
        ScrollSelectionDocumentViewSmoke,
        (),
    );
    init_runner(&mut runner, ScrollSelectionDocumentViewSmoke, viewport);

    let document_view_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
        .map(|node| node.id)
        .expect("document view exists");

    runner.drag.active =
        crate::app::runner::ActiveDrag::DocumentView(crate::app::input::drag::DocumentViewDrag {
            id: document_view_id,
            anchor: crate::app::input::drag::DocumentViewDragAnchor::Linear(0),
            shared_selection_id: None,
            scroll_view_id: None,
            shared_drag_anchor: None,
        });

    if let NodeKind::DocumentView(node) = &mut runner.core.tree.node_mut(document_view_id).kind {
        node.selection_cursor = 0;
        node.selection_anchor = Some(0);
    }

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 0,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        1,
    ));

    let (scroll_offset, cursor, anchor) = match &runner.core.tree.node(document_view_id).kind {
        NodeKind::DocumentView(node) => (
            node.scroll_offset,
            node.selection_cursor,
            node.selection_anchor,
        ),
        _ => unreachable!(),
    };

    assert_eq!(scroll_offset, 1);
    assert_eq!(cursor, 5);
    assert_eq!(anchor, Some(0));
}

#[test]
fn nested_scroll_view_refreshes_textarea_drag_selection_without_mouse_motion() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(
        App::new().mouse(false),
        NestedScrollSelectionTextAreaSmoke,
        (),
    );
    init_runner(&mut runner, NestedScrollSelectionTextAreaSmoke, viewport);

    let text_area_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .expect("text area exists");

    runner.drag.active =
        crate::app::runner::ActiveDrag::TextArea(crate::app::input::drag::TextAreaDrag {
            id: text_area_id,
            anchor: 0,
        });
    runner.drag.last_pointer_pos = Some((0, 0));

    if let NodeKind::TextArea(node) = &mut runner.core.tree.node_mut(text_area_id).kind {
        node.cursor = 0;
        node.anchor = Some(0);
    }

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 0,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        1,
    ));

    runner.core.render_element(viewport, None, None, None);
    assert!(runner.refresh_active_selection_drag_from_last_pointer());

    let cursor = match &runner.core.tree.node(text_area_id).kind {
        NodeKind::TextArea(node) => node.cursor,
        _ => unreachable!(),
    };

    assert_eq!(cursor, 5);
}

#[test]
fn nested_scroll_view_refreshes_document_view_drag_selection_without_mouse_motion() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    };
    let mut runner = AppRunner::new(
        App::new().mouse(false),
        NestedScrollSelectionDocumentViewSmoke,
        (),
    );
    init_runner(
        &mut runner,
        NestedScrollSelectionDocumentViewSmoke,
        viewport,
    );

    let document_view_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
        .map(|node| node.id)
        .expect("document view exists");

    runner.drag.active =
        crate::app::runner::ActiveDrag::DocumentView(crate::app::input::drag::DocumentViewDrag {
            id: document_view_id,
            anchor: crate::app::input::drag::DocumentViewDragAnchor::Linear(0),
            shared_selection_id: None,
            scroll_view_id: None,
            shared_drag_anchor: None,
        });
    runner.drag.last_pointer_pos = Some((0, 0));

    if let NodeKind::DocumentView(node) = &mut runner.core.tree.node_mut(document_view_id).kind {
        node.selection_cursor = 0;
        node.selection_anchor = Some(0);
    }

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 0,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        1,
    ));

    runner.core.render_element(viewport, None, None, None);
    assert!(runner.refresh_active_selection_drag_from_last_pointer());

    let cursor = match &runner.core.tree.node(document_view_id).kind {
        NodeKind::DocumentView(node) => node.selection_cursor,
        _ => unreachable!(),
    };

    assert_eq!(cursor, 5);
}

#[test]
fn standalone_text_widget_drag_edges_autoscroll() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 4,
    };

    {
        let mut runner = AppRunner::new(
            App::new().mouse(false),
            StandaloneAutoscrollTextAreaSmoke,
            (),
        );
        init_runner(&mut runner, StandaloneAutoscrollTextAreaSmoke, viewport);

        let text_area_id = runner
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("text area exists");
        let rect = runner.core.tree.node(text_area_id).rect;
        let x = rect.x.max(0) as u16;
        let second_from_bottom_y = rect.y.saturating_add(rect.h as i16).saturating_sub(2) as u16;
        let edge_y = rect.y.saturating_add(rect.h as i16).saturating_sub(1) as u16;

        runner.drag.active =
            crate::app::runner::ActiveDrag::TextArea(crate::app::input::drag::TextAreaDrag {
                id: text_area_id,
                anchor: 0,
            });
        if let NodeKind::TextArea(node) = &mut runner.core.tree.node_mut(text_area_id).kind {
            node.cursor = 0;
            node.anchor = Some(0);
        }

        assert_eq!(
            runner.dispatch_active_drag(x, second_from_bottom_y),
            Some(true)
        );

        let NodeKind::TextArea(node) = &runner.core.tree.node(text_area_id).kind else {
            unreachable!()
        };
        assert_eq!(node.scroll_offset, 0);

        assert_eq!(runner.dispatch_active_drag(x, edge_y), Some(true));

        let NodeKind::TextArea(node) = &runner.core.tree.node(text_area_id).kind else {
            unreachable!()
        };
        assert!(node.scroll_offset > 0);
        assert!(node.cursor > 0);
        assert_eq!(node.anchor, Some(0));
    }

    {
        let mut runner = AppRunner::new(
            App::new().mouse(false),
            StandaloneAutoscrollDocumentViewSmoke,
            (),
        );
        init_runner(&mut runner, StandaloneAutoscrollDocumentViewSmoke, viewport);

        let document_view_id = runner
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
            .map(|node| node.id)
            .expect("document view exists");
        let rect = runner.core.tree.node(document_view_id).rect;
        let x = rect.x.max(0) as u16;
        let second_from_bottom_y = rect.y.saturating_add(rect.h as i16).saturating_sub(2) as u16;
        let edge_y = rect.y.saturating_add(rect.h as i16).saturating_sub(1) as u16;

        runner.drag.active = crate::app::runner::ActiveDrag::DocumentView(
            crate::app::input::drag::DocumentViewDrag {
                id: document_view_id,
                anchor: crate::app::input::drag::DocumentViewDragAnchor::Linear(0),
                shared_selection_id: None,
                scroll_view_id: None,
                shared_drag_anchor: None,
            },
        );
        if let NodeKind::DocumentView(node) = &mut runner.core.tree.node_mut(document_view_id).kind
        {
            node.selection_cursor = 0;
            node.selection_anchor = Some(0);
        }

        assert_eq!(
            runner.dispatch_active_drag(x, second_from_bottom_y),
            Some(true)
        );

        let NodeKind::DocumentView(node) = &runner.core.tree.node(document_view_id).kind else {
            unreachable!()
        };
        assert_eq!(node.scroll_offset, 0);

        assert_eq!(runner.dispatch_active_drag(x, edge_y), Some(true));

        let NodeKind::DocumentView(node) = &runner.core.tree.node(document_view_id).kind else {
            unreachable!()
        };
        assert!(node.scroll_offset > 0);
        assert!(node.selection_cursor > 0);
        assert_eq!(node.selection_anchor, Some(0));
    }
}

#[test]
fn standalone_text_widget_stationary_drag_autoscroll_ticks_without_mouse_motion() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 4,
    };

    {
        let mut runner = AppRunner::new(
            App::new().mouse(false),
            StandaloneAutoscrollTextAreaSmoke,
            (),
        );
        init_runner(&mut runner, StandaloneAutoscrollTextAreaSmoke, viewport);

        let text_area_id = runner
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("text area exists");
        let rect = runner.core.tree.node(text_area_id).rect;
        let x = rect.x.max(0) as u16;
        let y = rect.y.saturating_add(rect.h as i16).saturating_sub(1) as u16;

        runner.drag.active =
            crate::app::runner::ActiveDrag::TextArea(crate::app::input::drag::TextAreaDrag {
                id: text_area_id,
                anchor: 0,
            });
        runner.drag.last_pointer_pos = Some((x, y));
        runner.drag.last_autoscroll_tick = Some(
            Instant::now()
                - runner.stationary_drag_autoscroll_interval()
                - Duration::from_millis(1),
        );
        if let NodeKind::TextArea(node) = &mut runner.core.tree.node_mut(text_area_id).kind {
            node.cursor = 0;
            node.anchor = Some(0);
        }

        assert!(runner.stationary_drag_autoscroll_pending());
        let mut dirty = DirtyTracker::default();
        runner.update_animation_cycle(&mut dirty);

        let NodeKind::TextArea(node) = &runner.core.tree.node(text_area_id).kind else {
            unreachable!()
        };
        assert!(node.scroll_offset > 0);
        assert!(node.cursor > 0);
        assert_eq!(dirty.level(), DirtyLevel::PaintOnly);
    }

    {
        let mut runner = AppRunner::new(
            App::new().mouse(false),
            StandaloneAutoscrollDocumentViewSmoke,
            (),
        );
        init_runner(&mut runner, StandaloneAutoscrollDocumentViewSmoke, viewport);

        let document_view_id = runner
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
            .map(|node| node.id)
            .expect("document view exists");
        let rect = runner.core.tree.node(document_view_id).rect;
        let x = rect.x.max(0) as u16;
        let y = rect.y.saturating_add(rect.h as i16).saturating_sub(1) as u16;

        runner.drag.active = crate::app::runner::ActiveDrag::DocumentView(
            crate::app::input::drag::DocumentViewDrag {
                id: document_view_id,
                anchor: crate::app::input::drag::DocumentViewDragAnchor::Linear(0),
                shared_selection_id: None,
                scroll_view_id: None,
                shared_drag_anchor: None,
            },
        );
        runner.drag.last_pointer_pos = Some((x, y));
        runner.drag.last_autoscroll_tick = Some(
            Instant::now()
                - runner.stationary_drag_autoscroll_interval()
                - Duration::from_millis(1),
        );
        if let NodeKind::DocumentView(node) = &mut runner.core.tree.node_mut(document_view_id).kind
        {
            node.selection_cursor = 0;
            node.selection_anchor = Some(0);
        }

        assert!(runner.stationary_drag_autoscroll_pending());
        let mut dirty = DirtyTracker::default();
        runner.update_animation_cycle(&mut dirty);

        let NodeKind::DocumentView(node) = &runner.core.tree.node(document_view_id).kind else {
            unreachable!()
        };
        assert!(node.scroll_offset > 0);
        assert!(node.selection_cursor > 0);
        assert_eq!(dirty.level(), DirtyLevel::PaintOnly);
    }
}

#[test]
fn standalone_text_area_stationary_drag_keeps_scrolling_after_anchor_leaves_view() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 4,
    };
    let mut runner = AppRunner::new(
        App::new().mouse(false),
        StandaloneAutoscrollTextAreaSmoke,
        (),
    );
    init_runner(&mut runner, StandaloneAutoscrollTextAreaSmoke, viewport);

    let text_area_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .expect("text area exists");
    let rect = runner.core.tree.node(text_area_id).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.saturating_add(rect.h as i16).saturating_sub(1) as u16;

    runner.drag.active =
        crate::app::runner::ActiveDrag::TextArea(crate::app::input::drag::TextAreaDrag {
            id: text_area_id,
            anchor: 0,
        });
    runner.drag.last_pointer_pos = Some((x, y));
    if let NodeKind::TextArea(node) = &mut runner.core.tree.node_mut(text_area_id).kind {
        node.cursor = 0;
        node.anchor = Some(0);
    }

    for _ in 0..3 {
        runner.drag.last_autoscroll_tick = Some(
            Instant::now()
                - runner.stationary_drag_autoscroll_interval()
                - Duration::from_millis(1),
        );
        let mut dirty = DirtyTracker::default();
        runner.update_animation_cycle(&mut dirty);
        if matches!(dirty.level(), DirtyLevel::LayoutOnly | DirtyLevel::Full) {
            runner.core.render_element(viewport, None, None, None);
        }
    }

    let NodeKind::TextArea(node) = &runner.core.tree.node(text_area_id).kind else {
        unreachable!()
    };
    assert!(
        node.scroll_offset >= 3,
        "scroll should continue after the anchor line leaves the viewport"
    );
    assert!(node.cursor > 0);
    assert_eq!(node.anchor, Some(0));
}

#[test]
fn stationary_document_drag_autoscroll_ticks_without_mouse_motion() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 4,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), StationaryAutoscrollSmoke, ());
    init_runner(&mut runner, StationaryAutoscrollSmoke, viewport);

    let scroll_view_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::ScrollView(_)))
        .map(|node| node.id)
        .expect("scroll view exists");
    let document_view_id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
        .map(|node| node.id)
        .expect("document view exists");

    let scroll_rect = runner.core.tree.node(scroll_view_id).rect;
    let x = scroll_rect.x.saturating_add(1) as u16;
    let y = scroll_rect
        .y
        .saturating_add(scroll_rect.h as i16)
        .saturating_sub(1) as u16;

    runner.drag.active =
        crate::app::runner::ActiveDrag::DocumentView(crate::app::input::drag::DocumentViewDrag {
            id: document_view_id,
            anchor: crate::app::input::drag::DocumentViewDragAnchor::Linear(0),
            shared_selection_id: None,
            scroll_view_id: None,
            shared_drag_anchor: None,
        });
    runner.drag.last_pointer_pos = Some((x, y));
    runner.drag.last_autoscroll_tick = Some(
        Instant::now() - runner.stationary_drag_autoscroll_interval() - Duration::from_millis(1),
    );

    assert!(runner.stationary_drag_autoscroll_pending());

    let start_offset = match &runner.core.tree.node(scroll_view_id).kind {
        NodeKind::ScrollView(node) => node.offset,
        _ => unreachable!(),
    };
    let start_cursor = match &runner.core.tree.node(document_view_id).kind {
        NodeKind::DocumentView(node) => node.selection_cursor,
        _ => unreachable!(),
    };

    let mut dirty = DirtyTracker::default();
    let poll_timeout = runner.update_animation_cycle(&mut dirty);

    let end_offset = match &runner.core.tree.node(scroll_view_id).kind {
        NodeKind::ScrollView(node) => node.offset,
        _ => unreachable!(),
    };
    let end_cursor = match &runner.core.tree.node(document_view_id).kind {
        NodeKind::DocumentView(node) => node.selection_cursor,
        _ => unreachable!(),
    };

    assert!(end_offset > start_offset);
    assert!(end_cursor > start_cursor);
    assert_eq!(dirty.level(), DirtyLevel::LayoutOnly);
    assert!(poll_timeout <= runner.stationary_drag_autoscroll_interval());
}

#[test]
fn process_pending_messages_routes_paint_only_without_scope_enqueue() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), PaintOnlyMessageProbe, ());
    init_runner(&mut runner, PaintOnlyMessageProbe, viewport);

    runner
        .core
        .queue
        .borrow_mut()
        .push_back((ScopeId(1), Box::new(PaintOnlyMessage::Tick)));

    let mut dirty = DirtyTracker::default();
    runner
        .process_pending_messages(&mut dirty)
        .expect("pending paint message should process");

    assert_eq!(dirty.level(), DirtyLevel::PaintOnly);
    assert!(runner.dirty_component_scopes.is_empty());
}

#[test]
fn process_pending_messages_routes_root_layout_to_layout_scope_refresh() {
    #[derive(Clone)]
    enum Msg {
        RootLayout,
    }

    struct RootLayoutMessageProbe;

    impl Component for RootLayoutMessageProbe {
        type Message = Msg;
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::RootLayout => Update::layout(),
            }
        }

        fn view(&self, _ctx: &Context<Self>) -> crate::core::element::Element {
            Text::new("root-layout").into()
        }
    }

    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), RootLayoutMessageProbe, ());
    init_runner(&mut runner, RootLayoutMessageProbe, viewport);

    runner
        .core
        .queue
        .borrow_mut()
        .push_back((ScopeId(1), Box::new(Msg::RootLayout)));

    let mut dirty = DirtyTracker::default();
    runner
        .process_pending_messages(&mut dirty)
        .expect("pending root layout message should process");

    assert_eq!(dirty.level(), DirtyLevel::LayoutOnly);
    assert_eq!(runner.dirty_component_scopes, vec![ScopeId(1)]);
}

#[test]
fn root_window_focus_lifecycle_applies_each_update_level_once_per_transition() {
    #[derive(Clone, Copy)]
    enum RequestedUpdate {
        None,
        Paint,
        Layout,
        Full,
    }

    struct FocusLifecycleProbe {
        requested: RequestedUpdate,
        calls: Rc<Cell<usize>>,
    }

    impl Component for FocusLifecycleProbe {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn on_window_focus_changed(&mut self, _focused: bool, _ctx: &mut Context<Self>) -> Update {
            self.calls.set(self.calls.get() + 1);
            match self.requested {
                RequestedUpdate::None => Update::none(),
                RequestedUpdate::Paint => Update::paint(),
                RequestedUpdate::Layout => Update::layout(),
                RequestedUpdate::Full => Update::full(),
            }
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("focus").into()
        }
    }

    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    };
    for (requested, expected) in [
        (RequestedUpdate::None, DirtyLevel::None),
        (RequestedUpdate::Paint, DirtyLevel::PaintOnly),
        (RequestedUpdate::Layout, DirtyLevel::LayoutOnly),
        (RequestedUpdate::Full, DirtyLevel::Full),
    ] {
        let calls = Rc::new(Cell::new(0));
        let probe = FocusLifecycleProbe {
            requested,
            calls: Rc::clone(&calls),
        };
        let mut runner = AppRunner::new(App::new().mouse(false), probe, ());
        init_runner(
            &mut runner,
            FocusLifecycleProbe {
                requested,
                calls: Rc::clone(&calls),
            },
            viewport,
        );

        let mut dirty = DirtyTracker::default();
        assert!(runner.set_window_focused(false, &mut dirty));
        assert_eq!(dirty.level(), expected);
        assert_eq!(calls.get(), 1);
        assert!(!runner.set_window_focused(false, &mut dirty));
        assert_eq!(calls.get(), 1);
        assert_eq!(
            runner.dirty_component_scopes,
            if matches!(expected, DirtyLevel::LayoutOnly) {
                vec![ScopeId(1)]
            } else {
                Vec::new()
            }
        );
    }
}

#[test]
fn controlled_text_area_input_remains_layout_only() {
    #[derive(Clone)]
    enum Msg {
        Changed(TextAreaEvent),
        SentinelsChanged,
        VimModeChanged,
        AsyncAutocomplete,
    }

    struct TextAreaLayoutProbe;

    impl Component for TextAreaLayoutProbe {
        type Message = Msg;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("")
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Changed(event) => event.apply_to(&mut ctx.state),
                Msg::SentinelsChanged | Msg::VimModeChanged => return Update::layout(),
                Msg::AsyncAutocomplete => {
                    return Update::layout_with_command(crate::Command::new(|| {}));
                }
            }
            Update::layout()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Element::from(TextArea::bound(&ctx.state).on_change(ctx.link().callback(Msg::Changed)))
                .key("editor")
        }
    }

    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), TextAreaLayoutProbe, ());
    init_runner(&mut runner, TextAreaLayoutProbe, viewport);
    runner.focus.focused = Some(node_id_by_key(&runner.core.tree, "editor"));

    let key_result = runner.dispatch_focused_key(key(KeyCode::Char('x')));
    assert!(key_result.handled);

    let mut dirty = DirtyTracker::default();
    dirty.mark_layout();
    runner
        .process_pending_messages(&mut dirty)
        .expect("TextArea callback should process");

    assert_eq!(dirty.level(), DirtyLevel::LayoutOnly);
    assert_eq!(runner.dirty_component_scopes, vec![ScopeId(1)]);

    for msg in [
        Msg::SentinelsChanged,
        Msg::VimModeChanged,
        Msg::AsyncAutocomplete,
    ] {
        runner
            .core
            .queue
            .borrow_mut()
            .push_back((ScopeId(1), Box::new(msg)));
        let mut dirty = DirtyTracker::default();
        runner
            .process_pending_messages(&mut dirty)
            .expect("prompt-local callback should process");
        assert_eq!(dirty.level(), DirtyLevel::LayoutOnly);
    }
}

/// Shared fixture for scroll view-dependency tests: a controlled TextArea
/// whose view optionally reads scroll data through the `Context` accessors.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ScrollReadMode {
    None,
    Scrollbars,
    Metrics,
}

fn scroll_dependency_probe_stale_after_edits(mode: ScrollReadMode, edits: &[KeyCode]) -> bool {
    #[derive(Clone)]
    enum Msg {
        Changed(TextAreaEvent),
    }

    struct ScrollReadProbe {
        mode: ScrollReadMode,
    }

    impl Component for ScrollReadProbe {
        type Message = Msg;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("")
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Changed(event) => event.apply_to(&mut ctx.state),
            }
            Update::layout()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            match self.mode {
                ScrollReadMode::None => {}
                ScrollReadMode::Scrollbars => {
                    let _ = ctx.text_area_scrollbars("editor");
                }
                ScrollReadMode::Metrics => {
                    let _ = ctx.text_area_metrics("editor");
                }
            }
            Element::from(TextArea::bound(&ctx.state).on_change(ctx.link().callback(Msg::Changed)))
                .key("editor")
        }
    }

    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), ScrollReadProbe { mode }, ());
    init_runner(&mut runner, ScrollReadProbe { mode }, viewport);
    runner.focus.focused = Some(node_id_by_key(&runner.core.tree, "editor"));

    let scroll_generations = runner.core.scroll.view_generations();
    let mut stale = false;
    for code in edits {
        let key_result = runner.dispatch_focused_key(key(*code));
        assert!(key_result.handled);
        let mut dirty = DirtyTracker::default();
        runner
            .process_pending_messages(&mut dirty)
            .expect("TextArea callback should process");
        assert_eq!(dirty.level(), DirtyLevel::LayoutOnly);

        // Mirror the layout-only render path for the frame: refresh dirty
        // scopes' views, reconcile the cached element, then ask whether
        // cached views became stale.
        let scopes = std::mem::take(&mut runner.dirty_component_scopes);
        runner.dirty_scope_set.clear();
        assert!(
            runner
                .core
                .refresh_cached_scopes(&scopes, viewport, runner.mouse.hovered)
        );
        assert!(runner.core.reconcile_cached_element(
            viewport,
            runner.focus.focused,
            runner.focus.focused_key.as_ref().cloned().as_ref(),
            None,
        ));
        stale |= runner
            .core
            .scroll
            .view_dependencies_stale(&scroll_generations);
    }
    stale
}

#[test]
fn text_area_edit_keeps_layout_only_without_scroll_readers() {
    // No view reads scroll data: a keystroke changes TextArea metrics but must
    // not invalidate cached views (the pre-fix behavior escalated to full).
    assert!(!scroll_dependency_probe_stale_after_edits(
        ScrollReadMode::None,
        &[KeyCode::Char('x')],
    ));
}

#[test]
fn scrollbar_reader_ignores_edits_that_keep_visibility() {
    // The view reads only scrollbar visibility; a single typed char cannot
    // flip it, so cached views stay valid.
    assert!(!scroll_dependency_probe_stale_after_edits(
        ScrollReadMode::Scrollbars,
        &[KeyCode::Char('x')],
    ));
}

#[test]
fn scrollbar_reader_staled_by_visibility_flip() {
    // Enough newlines to overflow the 4-row area flip vertical scrollbar
    // visibility, which the reading view must observe via a full rebuild.
    assert!(scroll_dependency_probe_stale_after_edits(
        ScrollReadMode::Scrollbars,
        &[
            KeyCode::Enter,
            KeyCode::Enter,
            KeyCode::Enter,
            KeyCode::Enter,
            KeyCode::Enter,
            KeyCode::Enter,
        ],
    ));
}

#[test]
fn metrics_reader_staled_by_any_edit() {
    // Full-metrics readers observe cursor/offset changes, so any edit stales
    // their cached views.
    assert!(scroll_dependency_probe_stale_after_edits(
        ScrollReadMode::Metrics,
        &[KeyCode::Char('x')],
    ));
}

struct UiSnapshotRoot;

impl Component for UiSnapshotRoot {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Text::new("snapshot probe").into()
    }
}

#[test]
fn ui_snapshot_slot_delivered_after_render() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 24,
        h: 5,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), UiSnapshotRoot, ());
    let slot = crate::ui_snapshot::UiSnapshotSlot::new();
    runner.core.ctx.request_ui_snapshot_to_slot(&slot);
    runner.core.render_element(viewport, None, None, None);
    runner
        .apply_pending_ui_snapshot_request()
        .expect("snapshot delivery should succeed");
    assert!(slot.take().is_some());
}

struct HScrollOffsetSmoke;

impl Component for HScrollOffsetSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        ScrollView::new()
            .axis(crate::widgets::ScrollAxis::Both)
            .h_scrollbar(true)
            .children((0..6).map(|i| {
                Text::new("x".repeat(80))
                    .width(Length::Auto)
                    .height(Length::Px(1))
                    .key(format!("row-{i}"))
            }))
            .into()
    }
}

// Regression: the runner's drag redraw gate (`dispatch_active_drag`) compares the
// scrollbar offset before/after `handle_drag` to decide whether to repaint. The
// horizontal arm of `get_scrollbar_offset` had no `ScrollView` case and always
// returned 0, so a horizontal scrollbar drag never reported a change and the thumb
// froze until release.
#[test]
fn horizontal_scrollbar_offset_reads_scroll_view_h_offset() {
    use crate::core::node::ScrollbarAxis;

    let viewport = Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 6,
    };
    let mut runner = AppRunner::new(App::new().mouse(false), HScrollOffsetSmoke, ());
    init_runner(&mut runner, HScrollOffsetSmoke, viewport);

    let id = runner
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::ScrollView(_)))
        .map(|node| node.id)
        .expect("scroll view node should exist");

    {
        let NodeKind::ScrollView(sv) = &mut runner.core.tree.node_mut(id).kind else {
            panic!("expected scroll view");
        };
        sv.h_offset = 4;
    }
    assert_eq!(
        runner.get_scrollbar_offset(id, ScrollbarAxis::Horizontal),
        4
    );

    // A live drag pins the position via `h_scroll_override`, which must take priority.
    {
        let NodeKind::ScrollView(sv) = &mut runner.core.tree.node_mut(id).kind else {
            panic!("expected scroll view");
        };
        sv.h_scroll_override = Some(7);
    }
    assert_eq!(
        runner.get_scrollbar_offset(id, ScrollbarAxis::Horizontal),
        7
    );
}

struct HScrollMultiplierSmoke;

impl Component for HScrollMultiplierSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        ScrollView::new()
            .axis(crate::widgets::ScrollAxis::Both)
            .h_scroll_wheel_multiplier(8)
            .children((0..4).map(|i| {
                Text::new("x".repeat(60))
                    .width(Length::Auto)
                    .height(Length::Px(1))
                    .key(format!("row-{i}"))
            }))
            .into()
    }
}

// Shift+wheel panning uses the dedicated horizontal multiplier, not the app-wide
// vertical one, so wide content can be scrolled comfortably (columns are finer
// than rows).
#[test]
fn dispatch_mouse_scroll_uses_scroll_view_horizontal_multiplier() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 4,
    };
    let mut runner = AppRunner::new(
        App::new().mouse(false).scroll_wheel_multiplier(1),
        HScrollMultiplierSmoke,
        (),
    );
    init_runner(&mut runner, HScrollMultiplierSmoke, viewport);

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 1,
            y: 0,
            kind: MouseKind::ScrollDown,
            mods: KeyMods {
                shift: true,
                ..KeyMods::default()
            },
        },
        1,
    ));

    let h_offset = runner
        .core
        .tree
        .iter()
        .find_map(|node| match &node.kind {
            NodeKind::ScrollView(scroll) => Some(scroll.h_offset),
            _ => None,
        })
        .expect("scroll view exists");
    assert_eq!(
        h_offset, 8,
        "shift+wheel should apply the horizontal multiplier, not the vertical default"
    );
}

#[test]
fn app_runner_and_test_backend_share_global_quit_unbind_behavior() {
    let runner = AppRunner::new(
        App::new().mouse(false).global_quit(None),
        RunnerKeymapSmoke,
        (),
    );
    assert!(runner.keymap.matches(ctrl_char('q')).is_empty());

    let mut backend = crate::TestBackend::new_with_app(
        App::new().mouse(false).global_quit(None),
        RunnerKeymapSmoke,
        (),
    );
    assert!(!backend.send_key(ctrl_char('q')).expect("send_key succeeds"));
    assert!(!backend.core.ctx.should_quit());
}

struct CommandShortcutSmoke {
    _hit: Rc<Cell<bool>>,
}

impl Component for CommandShortcutSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Input::new("").key("input")
    }
}

#[test]
fn command_shortcut_runs_in_test_backend_same_as_runner_policy() {
    let hit = Rc::new(Cell::new(false));
    let app = App::new()
        .mouse(false)
        .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst);
    let mut backend =
        crate::TestBackend::new_with_app(app, CommandShortcutSmoke { _hit: hit.clone() }, ());
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("app.test")
            .shortcut(crate::KeyBinding::from_str("ctrl-k").expect("binding"))
            .handler(Callback::new({
                let hit = hit.clone();
                move |_| hit.set(true)
            }))
            .build(),
    );

    assert!(backend.send_key(ctrl_char('k')).expect("send_key succeeds"));
    assert!(hit.get());
}

#[test]
fn app_command_highest_priority_policy_affects_real_dispatch() {
    let low_hit = Rc::new(Cell::new(false));
    let high_hit = Rc::new(Cell::new(false));
    let app = App::new()
        .mouse(false)
        .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst)
        .command_conflict_policy(crate::CommandConflictPolicy::HighestPriority);
    let mut backend = crate::TestBackend::new_with_app(
        app,
        CommandShortcutSmoke {
            _hit: Rc::new(Cell::new(false)),
        },
        (),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("low")
            .priority(0)
            .shortcut(crate::KeyBinding::from_str("ctrl-k").expect("binding"))
            .handler(Callback::new({
                let low_hit = low_hit.clone();
                move |_| low_hit.set(true)
            }))
            .build(),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("high")
            .priority(10)
            .shortcut(crate::KeyBinding::from_str("ctrl-k").expect("binding"))
            .handler(Callback::new({
                let high_hit = high_hit.clone();
                move |_| high_hit.set(true)
            }))
            .build(),
    );

    assert!(backend.send_key(ctrl_char('k')).expect("send_key succeeds"));
    assert!(!low_hit.get());
    assert!(high_hit.get());
}

#[cfg(feature = "terminal")]
struct TerminalKeyPolicySmoke {
    keys: Rc<RefCell<Vec<KeyEvent>>>,
}

#[cfg(feature = "terminal")]
impl Component for TerminalKeyPolicySmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let keys = self.keys.clone();
        Terminal::new()
            .focusable(true)
            .on_key(crate::callback::KeyHandler::new(move |key| {
                keys.borrow_mut().push(key);
                true
            }))
            .into()
    }
}

#[cfg(feature = "terminal")]
#[test]
fn chord_mismatch_policy_affects_real_dispatch() {
    let terminal_keys = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal)
        .chord_mismatch_policy(crate::ChordMismatchPolicy::ForwardPrefixAndCurrent);
    let mut backend = crate::TestBackend::new_with_app(
        app,
        TerminalKeyPolicySmoke {
            keys: terminal_keys.clone(),
        },
        (),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.detach")
            .shortcut(crate::KeyBinding::from_str("ctrl-a d").expect("binding"))
            .build(),
    );
    let terminal_id = backend
        .core
        .tree
        .iter()
        .find_map(|node| matches!(node.kind, NodeKind::Terminal(_)).then_some(node.id))
        .expect("terminal widget");
    backend.set_focused(terminal_id);

    assert!(backend.send_key(ctrl_char('a')).expect("prefix succeeds"));
    assert!(
        backend
            .send_key(KeyEvent {
                code: KeyCode::Char('x'),
                mods: KeyMods::default(),
            })
            .expect("mismatch succeeds")
    );
    assert_eq!(
        terminal_keys.borrow().as_slice(),
        &[
            ctrl_char('a'),
            KeyEvent {
                code: KeyCode::Char('x'),
                mods: KeyMods::default(),
            }
        ]
    );
}

#[cfg(feature = "terminal")]
#[test]
fn forward_prefix_replays_swallowed_prefix_on_pending_chord_mismatch() {
    let terminal_keys = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal)
        .chord_mismatch_policy(crate::ChordMismatchPolicy::ForwardPrefixAndCurrent);
    let mut backend = crate::TestBackend::new_with_app(
        app,
        TerminalKeyPolicySmoke {
            keys: terminal_keys.clone(),
        },
        (),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.detach")
            .shortcut(crate::KeyBinding::from_str("ctrl-a d").expect("binding"))
            .build(),
    );
    let terminal_id = backend
        .core
        .tree
        .iter()
        .find_map(|node| matches!(node.kind, NodeKind::Terminal(_)).then_some(node.id))
        .expect("terminal widget");
    backend.set_focused(terminal_id);

    backend.send_key(ctrl_char('a')).expect("prefix");
    backend
        .send_key(KeyEvent {
            code: KeyCode::Char('x'),
            mods: KeyMods::default(),
        })
        .expect("mismatch");
    // Documented ForwardPrefixAndCurrent: terminal should receive BOTH the
    // swallowed prefix (ctrl-a) and the mismatching key (x).
    assert_eq!(
        terminal_keys.borrow().as_slice(),
        &[
            ctrl_char('a'),
            KeyEvent {
                code: KeyCode::Char('x'),
                mods: KeyMods::default(),
            }
        ],
        "prefix should be forwarded on mismatch"
    );
}

#[cfg(feature = "terminal")]
struct TerminalWheelForwardSmoke {
    forwarded: Rc<RefCell<Vec<Vec<u8>>>>,
}

#[cfg(feature = "terminal")]
impl Component for TerminalWheelForwardSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let forwarded = self.forwarded.clone();
        Terminal::new()
            .snapshot(crate::widgets::TerminalRenderSnapshot {
                mouse_mode: crate::widgets::MouseModeState {
                    mode: crate::widgets::MouseMode::Normal,
                    encoding: crate::widgets::MouseEncoding::Sgr,
                    focus_events_enabled: false,
                },
                ..Default::default()
            })
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .on_mouse_forward(Callback::new(move |bytes: Vec<u8>| {
                forwarded.borrow_mut().push(bytes);
            }))
            .into()
    }
}

#[cfg(feature = "terminal")]
fn wheel_forward_runner(
    forwarded: &Rc<RefCell<Vec<Vec<u8>>>>,
    app: App,
) -> AppRunner<TerminalWheelForwardSmoke> {
    let mut runner = AppRunner::new(
        app,
        TerminalWheelForwardSmoke {
            forwarded: forwarded.clone(),
        },
        (),
    );
    init_runner(
        &mut runner,
        TerminalWheelForwardSmoke {
            forwarded: forwarded.clone(),
        },
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
    );
    runner
}

#[cfg(feature = "terminal")]
#[test]
fn coalesced_wheel_ticks_forward_one_sequence_per_tick() {
    let forwarded = Rc::new(RefCell::new(Vec::new()));
    let mut runner = wheel_forward_runner(&forwarded, App::new().mouse(false));

    // The runner batches a burst of same-direction wheel events into one dispatch. A child in
    // mouse-tracking mode must still see every raw tick, or a host that emits several events per
    // physical notch scrolls the child a fraction of what the user asked for.
    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 2,
            y: 1,
            kind: MouseKind::ScrollDown,
            mods: KeyMods::default(),
        },
        6,
    ));

    let emitted = forwarded.borrow();
    assert_eq!(emitted.len(), 1, "a batch should cost one callback");
    assert_eq!(
        emitted[0],
        "\x1b[<65;3;2M".repeat(6).into_bytes(),
        "six coalesced ticks should reach the child as six wheel reports"
    );
}

#[cfg(feature = "terminal")]
#[test]
fn forwarded_wheel_ticks_ignore_app_scroll_wheel_multiplier() {
    let forwarded = Rc::new(RefCell::new(Vec::new()));
    let mut runner = wheel_forward_runner(
        &forwarded,
        App::new().mouse(false).scroll_wheel_multiplier(3),
    );

    assert!(runner.dispatch_mouse_scroll(
        MouseEvent {
            x: 2,
            y: 1,
            kind: MouseKind::ScrollUp,
            mods: KeyMods::default(),
        },
        1,
    ));

    // Forwarding is a passthrough of what the host sent; the child applies its own scroll step, so
    // scaling here would multiply the two together.
    let emitted = forwarded.borrow();
    assert_eq!(emitted.len(), 1);
    assert_eq!(emitted[0], b"\x1b[<64;3;2M".to_vec());
}

/// End-to-end check of the tmux-style leader model a terminal mux (hyprmux)
/// relies on: `AppCommandsFirst` + `AppCommandsThenTerminal` + the default
/// `SwallowPrefixReplayCurrent`. The leader prefix is swallowed, a completing
/// key runs the command, a mismatching key is replayed to the PTY while the
/// prefix is dropped, and plain typing reaches the PTY untouched.
#[cfg(feature = "terminal")]
#[test]
fn mux_leader_chord_model_matches_prefix_semantics() {
    let terminal_keys = Rc::new(RefCell::new(Vec::new()));
    let detached = Rc::new(Cell::new(0u32));
    let app = App::new()
        .mouse(false)
        .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal);
    let mut backend = crate::TestBackend::new_with_app(
        app,
        TerminalKeyPolicySmoke {
            keys: terminal_keys.clone(),
        },
        (),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.detach")
            .shortcut(crate::KeyBinding::from_str("ctrl-a d").expect("binding"))
            .handler(Callback::new({
                let detached = detached.clone();
                move |_| detached.set(detached.get() + 1)
            }))
            .build(),
    );
    let terminal_id = backend
        .core
        .tree
        .iter()
        .find_map(|node| matches!(node.kind, NodeKind::Terminal(_)).then_some(node.id))
        .expect("terminal widget");
    backend.set_focused(terminal_id);

    let plain = |ch: char| KeyEvent {
        code: KeyCode::Char(ch),
        mods: KeyMods::default(),
    };

    // Completed leader chord runs the command; nothing leaks to the PTY.
    backend.send_key(ctrl_char('a')).expect("prefix");
    backend.send_key(plain('d')).expect("completion");
    assert_eq!(detached.get(), 1, "ctrl-a d should run mux.detach");
    assert!(
        terminal_keys.borrow().is_empty(),
        "leader chord must not leak keys to the PTY"
    );

    // Mismatch replays only the current key to the PTY; prefix is dropped.
    backend.send_key(ctrl_char('a')).expect("prefix");
    backend.send_key(plain('x')).expect("mismatch");
    assert_eq!(detached.get(), 1, "mismatch must not run the command");
    assert_eq!(
        terminal_keys.borrow().as_slice(),
        &[plain('x')],
        "mismatch forwards current key only, prefix swallowed"
    );

    // Plain typing flows straight to the PTY.
    terminal_keys.borrow_mut().clear();
    backend.send_key(plain('l')).expect("plain");
    backend.send_key(plain('s')).expect("plain");
    assert_eq!(
        terminal_keys.borrow().as_slice(),
        &[plain('l'), plain('s')],
        "plain keys reach the PTY untouched"
    );
}

#[cfg(feature = "terminal")]
fn ctrl_shift_char(ch: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(ch),
        mods: KeyMods {
            ctrl: true,
            shift: true,
            ..KeyMods::default()
        },
    }
}

#[cfg(feature = "terminal")]
struct StaticReadClipboardProvider(&'static str);

#[cfg(feature = "terminal")]
impl ClipboardProvider for StaticReadClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
        Ok(self.0.to_string())
    }

    fn write_clipboard_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
        Ok(())
    }
}

#[cfg(feature = "terminal")]
struct ClassifiedClipboardProvider {
    content: crate::ClipboardPasteContent,
    text_fallback: Option<&'static str>,
}

#[cfg(feature = "terminal")]
impl ClipboardProvider for ClassifiedClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
        self.text_fallback.map(str::to_string).ok_or_else(|| {
            ClipboardError::provider(
                crate::clipboard::error::ClipboardOperation::ReadClipboard,
                "text representation unavailable",
            )
        })
    }

    fn write_clipboard_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
        Ok(())
    }

    fn read_terminal_paste(&mut self) -> Result<crate::ClipboardPasteContent, ClipboardError> {
        Ok(self.content.clone())
    }
}

#[cfg(feature = "terminal")]
struct FailingClipboardProvider;

#[cfg(feature = "terminal")]
impl ClipboardProvider for FailingClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
        Err(ClipboardError::provider(
            crate::clipboard::error::ClipboardOperation::ReadClipboard,
            "clipboard unavailable",
        ))
    }

    fn write_clipboard_text(&mut self, _text: &str) -> Result<(), ClipboardError> {
        Ok(())
    }
}

#[cfg(feature = "terminal")]
struct CapturedTerminalPasteSmoke {
    keys: Rc<RefCell<Vec<KeyEvent>>>,
    inputs: Rc<RefCell<Vec<crate::widgets::TerminalInputEvent>>>,
    behavior: crate::widgets::TerminalPasteShortcutBehavior,
    key_modes: crate::widgets::TerminalKeyModes,
}

#[cfg(feature = "terminal")]
impl Component for CapturedTerminalPasteSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let keys = self.keys.clone();
        let inputs = self.inputs.clone();
        Terminal::new()
            .key_modes(self.key_modes)
            .paste_shortcut_behavior(self.behavior)
            .on_key(crate::callback::KeyHandler::new(move |key| {
                keys.borrow_mut().push(key);
                true
            }))
            .on_input(Callback::new(
                move |event: crate::widgets::TerminalInputEvent| {
                    inputs.borrow_mut().push(event);
                },
            ))
            .key("terminal")
    }
}

#[cfg(feature = "terminal")]
struct PasteDispatchResult {
    keys: Rc<RefCell<Vec<KeyEvent>>>,
    inputs: Rc<RefCell<Vec<crate::widgets::TerminalInputEvent>>>,
    command_hit: Rc<Cell<bool>>,
    errors: Rc<RefCell<Vec<ClipboardError>>>,
}

#[cfg(feature = "terminal")]
fn dispatch_ctrl_v(
    provider: impl ClipboardProvider + 'static,
    behavior: crate::widgets::TerminalPasteShortcutBehavior,
    key_modes: crate::widgets::TerminalKeyModes,
) -> PasteDispatchResult {
    let keys = Rc::new(RefCell::new(Vec::new()));
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let command_hit = Rc::new(Cell::new(false));
    let errors = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal)
        .clipboard_provider(provider)
        .clipboard_reporter({
            let errors = errors.clone();
            move |error| errors.borrow_mut().push(error)
        });
    let mut backend = crate::TestBackend::new_with_app(
        app,
        CapturedTerminalPasteSmoke {
            keys: keys.clone(),
            inputs: inputs.clone(),
            behavior,
            key_modes,
        },
        (),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.paste")
            .shortcut(crate::KeyBinding::from_str("ctrl-v").expect("binding"))
            .handler(Callback::new({
                let command_hit = command_hit.clone();
                move |_| command_hit.set(true)
            }))
            .build(),
    );
    let terminal_id = node_id_by_key(&backend.core.tree, "terminal");
    backend.set_focused(terminal_id);
    assert!(backend.send_key(ctrl_char('v')).expect("send_key succeeds"));

    PasteDispatchResult {
        keys,
        inputs,
        command_hit,
        errors,
    }
}

#[cfg(feature = "terminal")]
struct TerminalDispatchSmoke {
    keys: Rc<RefCell<Vec<KeyEvent>>>,
    inputs: Rc<RefCell<Vec<crate::widgets::TerminalInputKind>>>,
}

#[cfg(feature = "terminal")]
impl Component for TerminalDispatchSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let keys = self.keys.clone();
        let inputs = self.inputs.clone();
        Terminal::new()
            .focusable(true)
            .paste_shortcut_behavior(crate::widgets::TerminalPasteShortcutBehavior::Performable)
            .on_key(crate::callback::KeyHandler::new(move |key| {
                keys.borrow_mut().push(key);
                true
            }))
            .on_input(Callback::new(
                move |event: crate::widgets::TerminalInputEvent| {
                    inputs.borrow_mut().push(event.kind);
                },
            ))
            .key("terminal")
    }
}

#[cfg(feature = "terminal")]
fn set_terminal_selection(
    backend: &mut crate::TestBackend<TerminalDispatchSmoke>,
    terminal_id: NodeId,
    line: &str,
    end_col: usize,
) {
    if let NodeKind::Terminal(term) = &mut backend.core.tree.node_mut(terminal_id).kind {
        term.lines = vec![vec![Span::new(line)]].into();
        let mut selection =
            crate::widgets::TerminalSelection::new(crate::widgets::TerminalPos { line: 0, col: 0 });
        selection.extend_to(crate::widgets::TerminalPos {
            line: 0,
            col: end_col,
        });
        term.selection = Some(selection);
    }
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_ctrl_c_with_selection_copies_before_app_command() {
    let command_hit = Rc::new(Cell::new(false));
    let keys = Rc::new(RefCell::new(Vec::new()));
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal);
    let mut backend = crate::TestBackend::new_with_app(
        app,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: inputs.clone(),
        },
        (),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.copy")
            .shortcut(crate::KeyBinding::from_str("ctrl-c").expect("binding"))
            .handler(Callback::new({
                let command_hit = command_hit.clone();
                move |_| command_hit.set(true)
            }))
            .build(),
    );
    let terminal_id = node_id_by_key(&backend.core.tree, "terminal");
    set_terminal_selection(&mut backend, terminal_id, "hello", 5);
    backend.set_focused(terminal_id);

    assert!(backend.send_key(ctrl_char('c')).expect("send_key succeeds"));
    assert!(
        !command_hit.get(),
        "app command must not run when copy preflight applies"
    );
    assert!(
        keys.borrow().is_empty(),
        "terminal on_key must not receive ctrl-c"
    );
}

#[cfg(feature = "terminal")]
struct LiveScreenSmoke {
    screen: Rc<RefCell<crate::widgets::TerminalScreen>>,
    views: Rc<Cell<usize>>,
}

#[cfg(feature = "terminal")]
impl Component for LiveScreenSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        self.views.set(self.views.get() + 1);
        Terminal::new()
            .screen(crate::widgets::TerminalScreenHandle::new(
                self.screen.clone(),
            ))
            .key("terminal")
    }
}

/// Output written to a screen the widget holds a handle to must reach the node without `view()`
/// running again. That is what lets a streaming pane cost a repaint instead of a rebuild of every
/// pane, workbar segment and sidebar row in the window.
#[cfg(feature = "terminal")]
#[test]
fn live_screen_output_reaches_the_node_without_a_view_pass() {
    let screen = Rc::new(RefCell::new(crate::widgets::TerminalScreen::new(
        4, 20, 100,
    )));
    let views = Rc::new(Cell::new(0));
    let mut backend = crate::TestBackend::new(LiveScreenSmoke {
        screen: screen.clone(),
        views: views.clone(),
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 4,
    });
    screen.borrow_mut().process_bytes(b"first");
    backend.render();

    let views_after_first_frame = views.get();
    assert!(
        backend.capture_frame().to_fixed_grid_lines()[0].contains("first"),
        "the first frame shows what the screen already had"
    );

    // The child program writes again. No message, no `view()` — just the refresh the runtime does
    // on its way to the buffer.
    screen.borrow_mut().process_bytes(b"\r\nsecond");
    assert!(
        backend.refresh_live_terminals(),
        "the refresh reports that the screen moved"
    );

    let lines = backend.capture_frame().to_fixed_grid_lines();
    assert!(
        lines[1].contains("second"),
        "new output is painted without a view pass: {lines:#?}"
    );
    assert_eq!(
        views.get(),
        views_after_first_frame,
        "and `view()` was not run to get it there"
    );

    assert!(
        !backend.refresh_live_terminals(),
        "a refresh that finds the screen unmoved does nothing"
    );
}

/// Chord mismatch under `AppCommandsThenTerminal` forwards the key (no frame for the key itself)
/// but must still claim a frame so PREFIX chrome can clear. Without that, a busy child TUI keeps
/// painting the pane while the stale badge stays until an unrelated full `view()` pass.
#[cfg(feature = "terminal")]
#[test]
fn chord_mismatch_while_terminal_focused_claims_a_frame_for_prefix_chrome() {
    let keys = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal);
    let mut runner = AppRunner::new(
        app,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        },
        (),
    );
    init_runner(
        &mut runner,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        },
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
    );
    runner.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.detach")
            .shortcut(crate::KeyBinding::from_str("ctrl-a d").expect("binding"))
            .handler(Callback::new(|_| {}))
            .build(),
    );
    let terminal = node_id_by_key(&runner.core.tree, "terminal");
    runner.focus.focused = Some(terminal);

    let prefix = runner.dispatch_layered_key(ctrl_char('a'));
    assert!(prefix.consumed);
    assert!(
        runner
            .core
            .ctx
            .env()
            .command_chord_pending_since
            .get()
            .is_some()
    );
    assert!(
        prefix.mark_full && prefix.dirty_override.is_none(),
        "entering a leader chord must claim a frame for PREFIX chrome"
    );

    let mismatch = runner.dispatch_layered_key(key(KeyCode::Char('x')));
    assert!(mismatch.consumed);
    assert_eq!(keys.borrow().as_slice(), &[key(KeyCode::Char('x'))]);
    assert!(
        runner
            .core
            .ctx
            .env()
            .command_chord_pending_since
            .get()
            .is_none()
    );
    assert!(
        mismatch.mark_full && mismatch.dirty_override.is_none(),
        "clearing PREFIX after a forwarded mismatch must claim a frame: {mismatch:?}"
    );
}

/// A key forwarded to a terminal changes nothing on screen: it becomes bytes for the child program,
/// and whatever the child draws in response arrives later as output that asks for its own frame.
/// Claiming a frame here too means every keystroke costs two — one speculative, one real.
#[cfg(feature = "terminal")]
#[test]
fn a_key_forwarded_to_a_terminal_claims_no_frame() {
    let keys = Rc::new(RefCell::new(Vec::new()));
    let mut runner = AppRunner::new(
        App::new().mouse(false),
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        },
        (),
    );
    init_runner(
        &mut runner,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        },
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
    );
    let terminal = node_id_by_key(&runner.core.tree, "terminal");
    runner.focus.focused = Some(terminal);

    let result = runner.dispatch_layered_key(key(KeyCode::Char('a')));

    assert!(result.consumed, "the terminal claims the key");
    assert_eq!(keys.borrow().len(), 1, "and forwards it to the child");
    assert_eq!(
        result.dirty_override,
        Some(DirtyLevel::None),
        "forwarding asks for no frame of its own"
    );
    assert!(
        !result.mark_full,
        "the override is what the event loop honors, so it must not also ask for a full frame"
    );
}

/// Dropping a live selection *is* a visible change, so that key does owe a frame.
#[cfg(feature = "terminal")]
#[test]
fn a_key_that_clears_a_terminal_selection_still_claims_a_frame() {
    let keys = Rc::new(RefCell::new(Vec::new()));
    let mut runner = AppRunner::new(
        App::new().mouse(false),
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        },
        (),
    );
    init_runner(
        &mut runner,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        },
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
    );
    let terminal = node_id_by_key(&runner.core.tree, "terminal");
    runner.focus.focused = Some(terminal);
    if let NodeKind::Terminal(term) = &mut runner.core.tree.node_mut(terminal).kind {
        term.lines = vec![vec![Span::new("selected")]].into();
        let mut selection =
            crate::widgets::TerminalSelection::new(crate::widgets::TerminalPos { line: 0, col: 0 });
        selection.extend_to(crate::widgets::TerminalPos { line: 0, col: 8 });
        term.selection = Some(selection);
    }

    let result = runner.dispatch_layered_key(key(KeyCode::Char('a')));

    assert!(result.consumed);
    assert_eq!(
        result.dirty_override, None,
        "a cleared selection leaves the generic handled-key level in place"
    );
    assert!(result.mark_full, "so the frame still happens");
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_ctrl_c_without_selection_runs_app_command_before_forwarding_in_mux_policy() {
    let command_hit = Rc::new(Cell::new(false));
    let keys = Rc::new(RefCell::new(Vec::new()));
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal);
    let mut backend = crate::TestBackend::new_with_app(
        app,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs,
        },
        (),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.copy")
            .shortcut(crate::KeyBinding::from_str("ctrl-c").expect("binding"))
            .handler(Callback::new({
                let command_hit = command_hit.clone();
                move |_| command_hit.set(true)
            }))
            .build(),
    );
    let terminal_id = node_id_by_key(&backend.core.tree, "terminal");
    backend.set_focused(terminal_id);

    assert!(backend.send_key(ctrl_char('c')).expect("send_key succeeds"));
    assert!(command_hit.get());
    assert!(keys.borrow().is_empty());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_ctrl_shift_v_still_pastes_to_terminal_before_command() {
    let command_hit = Rc::new(Cell::new(false));
    let keys = Rc::new(RefCell::new(Vec::new()));
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal)
        .clipboard_provider(StaticReadClipboardProvider("pasted-text"));
    let mut backend = crate::TestBackend::new_with_app(
        app,
        TerminalDispatchSmoke {
            keys,
            inputs: inputs.clone(),
        },
        (),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.paste")
            .shortcut(crate::KeyBinding::from_str("ctrl-shift-v").expect("binding"))
            .handler(Callback::new({
                let command_hit = command_hit.clone();
                move |_| command_hit.set(true)
            }))
            .build(),
    );
    let terminal_id = node_id_by_key(&backend.core.tree, "terminal");
    backend.set_focused(terminal_id);

    assert!(
        backend
            .send_key(ctrl_shift_char('v'))
            .expect("send_key succeeds")
    );
    assert_eq!(
        inputs.borrow().as_slice(),
        &[crate::widgets::TerminalInputKind::Paste]
    );
    assert!(!command_hit.get());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_pastes_text_before_app_command() {
    let command_hit = Rc::new(Cell::new(false));
    let keys = Rc::new(RefCell::new(Vec::new()));
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal)
        .clipboard_provider(StaticReadClipboardProvider("pasted-text"));
    let mut backend = crate::TestBackend::new_with_app(
        app,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: inputs.clone(),
        },
        (),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.paste")
            .shortcut(crate::KeyBinding::from_str("ctrl-v").expect("binding"))
            .handler(Callback::new({
                let command_hit = command_hit.clone();
                move |_| command_hit.set(true)
            }))
            .build(),
    );
    let terminal_id = node_id_by_key(&backend.core.tree, "terminal");
    backend.set_focused(terminal_id);

    assert!(backend.send_key(ctrl_char('v')).expect("send_key succeeds"));
    assert_eq!(
        inputs.borrow().as_slice(),
        &[crate::widgets::TerminalInputKind::Paste]
    );
    assert!(keys.borrow().is_empty());
    assert!(!command_hit.get());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_native_ctrl_v_clears_a_pending_command_chord_and_prefix_state() {
    let command_hit = Rc::new(Cell::new(false));
    let keys = Rc::new(RefCell::new(Vec::new()));
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal)
        .clipboard_provider(StaticReadClipboardProvider("pasted-text"));
    let mut backend = crate::TestBackend::new_with_app(
        app,
        TerminalDispatchSmoke {
            keys,
            inputs: inputs.clone(),
        },
        (),
    );
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.detach")
            .shortcut(crate::KeyBinding::from_str("ctrl-a d").expect("binding"))
            .handler(Callback::new({
                let command_hit = command_hit.clone();
                move |_| command_hit.set(true)
            }))
            .build(),
    );
    let terminal_id = node_id_by_key(&backend.core.tree, "terminal");
    backend.set_focused(terminal_id);

    assert!(backend.send_key(ctrl_char('a')).expect("prefix succeeds"));
    assert!(
        backend
            .core
            .ctx
            .env()
            .command_chord_pending_since
            .get()
            .is_some(),
        "the prefix indicator must be visible while the chord waits"
    );

    assert!(
        backend
            .send_key(ctrl_char('v'))
            .expect("native paste succeeds")
    );
    assert_eq!(
        inputs.borrow().as_slice(),
        &[crate::widgets::TerminalInputKind::Paste]
    );
    assert!(
        !command_hit.get(),
        "native paste must not run the pending command"
    );
    assert!(
        backend
            .core
            .ctx
            .env()
            .command_chord_pending_since
            .get()
            .is_none(),
        "native paste must clear the visible PREFIX state"
    );
}

#[cfg(feature = "terminal")]
fn assert_performable_forwarded(result: &PasteDispatchResult, modes: crate::TerminalKeyModes) {
    let key = ctrl_char('v');
    assert_eq!(result.keys.borrow().as_slice(), &[key]);
    let inputs = result.inputs.borrow();
    assert_eq!(inputs.len(), 1);
    assert_eq!(inputs[0].kind, crate::widgets::TerminalInputKind::Key);
    assert_eq!(inputs[0].key, Some(key));
    assert_eq!(
        inputs[0].bytes.as_ref(),
        crate::widgets::key_event_to_bytes(key, modes)
            .expect("ctrl-v is encodable")
            .as_slice()
    );
    assert!(!result.command_hit.get());
    assert!(result.errors.borrow().is_empty());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_emits_plain_text_without_bracketed_mode() {
    let result = dispatch_ctrl_v(
        ClassifiedClipboardProvider {
            content: crate::ClipboardPasteContent::Text("plain text".into()),
            text_fallback: Some("plain text"),
        },
        crate::TerminalPasteShortcutBehavior::Performable,
        crate::TerminalKeyModes::default(),
    );

    assert!(result.keys.borrow().is_empty());
    assert_eq!(
        result.inputs.borrow()[0].kind,
        crate::widgets::TerminalInputKind::Paste
    );
    assert_eq!(result.inputs.borrow()[0].bytes.as_ref(), b"plain text");
    assert!(!result.command_hit.get());
    assert!(result.errors.borrow().is_empty());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_wraps_text_in_bracketed_mode() {
    let modes = crate::TerminalKeyModes {
        bracketed_paste: true,
        ..crate::TerminalKeyModes::default()
    };
    let result = dispatch_ctrl_v(
        ClassifiedClipboardProvider {
            content: crate::ClipboardPasteContent::Text("plain text".into()),
            text_fallback: Some("plain text"),
        },
        crate::TerminalPasteShortcutBehavior::Performable,
        modes,
    );

    assert!(result.keys.borrow().is_empty());
    assert_eq!(
        result.inputs.borrow()[0].kind,
        crate::widgets::TerminalInputKind::Paste
    );
    assert_eq!(
        result.inputs.borrow()[0].bytes.as_ref(),
        b"\x1b[200~plain text\x1b[201~"
    );
    assert!(!result.command_hit.get());
    assert!(result.errors.borrow().is_empty());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_prefers_file_list_over_text_fallback() {
    let result = dispatch_ctrl_v(
        ClassifiedClipboardProvider {
            content: crate::ClipboardPasteContent::Rich,
            text_fallback: Some("file:///tmp/photo.png"),
        },
        crate::TerminalPasteShortcutBehavior::Performable,
        crate::TerminalKeyModes::default(),
    );
    assert_performable_forwarded(&result, crate::TerminalKeyModes::default());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_prefers_image_over_text_fallback() {
    let result = dispatch_ctrl_v(
        ClassifiedClipboardProvider {
            content: crate::ClipboardPasteContent::Rich,
            text_fallback: Some("image fallback"),
        },
        crate::TerminalPasteShortcutBehavior::Performable,
        crate::TerminalKeyModes::default(),
    );
    assert_performable_forwarded(&result, crate::TerminalKeyModes::default());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_forwards_file_list_without_text() {
    let result = dispatch_ctrl_v(
        ClassifiedClipboardProvider {
            content: crate::ClipboardPasteContent::Rich,
            text_fallback: None,
        },
        crate::TerminalPasteShortcutBehavior::Performable,
        crate::TerminalKeyModes::default(),
    );
    assert_performable_forwarded(&result, crate::TerminalKeyModes::default());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_forwards_image_without_text() {
    let result = dispatch_ctrl_v(
        ClassifiedClipboardProvider {
            content: crate::ClipboardPasteContent::Rich,
            text_fallback: None,
        },
        crate::TerminalPasteShortcutBehavior::Performable,
        crate::TerminalKeyModes::default(),
    );
    assert_performable_forwarded(&result, crate::TerminalKeyModes::default());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_forwards_empty_clipboard() {
    let result = dispatch_ctrl_v(
        ClassifiedClipboardProvider {
            content: crate::ClipboardPasteContent::Unavailable,
            text_fallback: None,
        },
        crate::TerminalPasteShortcutBehavior::Performable,
        crate::TerminalKeyModes::default(),
    );
    assert_performable_forwarded(&result, crate::TerminalKeyModes::default());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_forwards_unknown_unsupported_format() {
    let result = dispatch_ctrl_v(
        ClassifiedClipboardProvider {
            content: crate::ClipboardPasteContent::Unavailable,
            text_fallback: None,
        },
        crate::TerminalPasteShortcutBehavior::Performable,
        crate::TerminalKeyModes::default(),
    );
    assert_performable_forwarded(&result, crate::TerminalKeyModes::default());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_consumes_and_reports_provider_failure() {
    let result = dispatch_ctrl_v(
        FailingClipboardProvider,
        crate::TerminalPasteShortcutBehavior::Performable,
        crate::TerminalKeyModes::default(),
    );

    assert!(result.keys.borrow().is_empty());
    assert!(result.inputs.borrow().is_empty());
    assert!(!result.command_hit.get());
    assert_eq!(result.errors.borrow().len(), 1);
    assert!(
        result.errors.borrow()[0]
            .to_string()
            .contains("clipboard unavailable")
    );
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_performable_ctrl_v_forwards_through_negotiated_key_encoder() {
    let modes = crate::TerminalKeyModes {
        kitty_keyboard: crate::KittyKeyboardFlags {
            disambiguate_escape_codes: true,
            ..crate::KittyKeyboardFlags::default()
        },
        ..crate::TerminalKeyModes::default()
    };
    let result = dispatch_ctrl_v(
        ClassifiedClipboardProvider {
            content: crate::ClipboardPasteContent::Rich,
            text_fallback: Some("image fallback"),
        },
        crate::TerminalPasteShortcutBehavior::Performable,
        modes,
    );

    assert_performable_forwarded(&result, modes);
    assert_ne!(result.inputs.borrow()[0].bytes.as_ref(), &[0x16]);
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_forward_behavior_leaves_ctrl_v_to_the_existing_app_command() {
    let result = dispatch_ctrl_v(
        ClassifiedClipboardProvider {
            content: crate::ClipboardPasteContent::Text("plain text".into()),
            text_fallback: Some("plain text"),
        },
        crate::TerminalPasteShortcutBehavior::Forward,
        crate::TerminalKeyModes::default(),
    );

    assert!(result.keys.borrow().is_empty());
    assert!(result.inputs.borrow().is_empty());
    assert!(result.command_hit.get());
    assert!(result.errors.borrow().is_empty());
}

#[test]
fn non_terminal_focus_keeps_normal_app_command_routing_for_ctrl_v() {
    let command_hit = Rc::new(Cell::new(false));
    let app = App::new()
        .mouse(false)
        .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst);
    let mut backend = crate::TestBackend::new_with_app(app, RunnerKeymapSmoke, ());
    backend.core.ctx.command_registry().register(
        crate::CommandEntry::builder("app.paste")
            .shortcut(crate::KeyBinding::from_str("ctrl-v").expect("binding"))
            .handler(Callback::new({
                let command_hit = command_hit.clone();
                move |_| command_hit.set(true)
            }))
            .build(),
    );
    backend.set_focused(node_id_by_key(&backend.core.tree, "input"));

    assert!(backend.send_key(ctrl_char('v')).expect("send_key succeeds"));
    assert!(command_hit.get());
}

#[cfg(feature = "terminal")]
#[test]
fn terminal_only_forwards_ctrl_q_and_f12_without_framework_actions() {
    let keys = Rc::new(RefCell::new(Vec::new()));
    let inputs = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .terminal_key_policy(crate::TerminalKeyPolicy::TerminalOnly);
    let mut backend = crate::TestBackend::new_with_app(
        app,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs,
        },
        (),
    );
    let terminal_id = node_id_by_key(&backend.core.tree, "terminal");
    backend.set_focused(terminal_id);

    assert!(backend.send_key(ctrl_char('q')).expect("ctrl-q succeeds"));
    assert!(
        backend
            .send_key(KeyEvent {
                code: KeyCode::F(12),
                mods: KeyMods::default(),
            })
            .expect("f12 succeeds")
    );
    assert!(!backend.core.ctx.should_quit());
    assert_eq!(
        keys.borrow().as_slice(),
        &[
            ctrl_char('q'),
            KeyEvent {
                code: KeyCode::F(12),
                mods: KeyMods::default(),
            }
        ]
    );
}

/// Component with two focusables, so focus stepping has somewhere to land.
struct HeadlessSnapshotSmoke;

impl Component for HeadlessSnapshotSmoke {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        VStack::new()
            .child(Text::new("headless capture"))
            .child(Input::new("").key("first"))
            .child(Input::new("").key("second"))
            .into()
    }
}

fn headless_snapshot_config(
    path: std::path::PathBuf,
    focus_steps: usize,
) -> super::headless_snapshot::HeadlessSnapshotConfig {
    super::headless_snapshot::HeadlessSnapshotConfig {
        format: crate::ui_snapshot::UiSnapshotFileFormat::from_path(&path),
        path,
        viewport: Rect {
            x: 0,
            y: 0,
            w: 48,
            h: 12,
        },
        frames: 1,
        focus_steps,
        actions: Vec::new(),
        diagnostic: false,
    }
}

#[test]
fn headless_snapshot_writes_markdown_at_the_requested_viewport() {
    let dir = std::env::temp_dir().join(format!("tui-lipan-headless-md-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("snapshot.md");

    let runner = AppRunner::new(App::new(), HeadlessSnapshotSmoke, ());
    runner
        .run_headless_snapshot(headless_snapshot_config(path.clone(), 0))
        .expect("headless snapshot succeeds");

    let written = std::fs::read_to_string(&path).expect("snapshot file exists");
    assert!(
        written.contains("headless capture"),
        "rendered text should reach the report: {written}"
    );
    assert!(
        written.contains("48x12") || written.contains("w: 48"),
        "report should record the requested viewport: {written}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn headless_snapshot_focus_steps_move_focus_off_the_first_widget() {
    let dir = std::env::temp_dir().join(format!("tui-lipan-headless-focus-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");

    let first = dir.join("first.md");
    AppRunner::new(App::new(), HeadlessSnapshotSmoke, ())
        .run_headless_snapshot(headless_snapshot_config(first.clone(), 0))
        .expect("headless snapshot succeeds");

    let stepped = dir.join("stepped.md");
    AppRunner::new(App::new(), HeadlessSnapshotSmoke, ())
        .run_headless_snapshot(headless_snapshot_config(stepped.clone(), 1))
        .expect("headless snapshot succeeds");

    let first = std::fs::read_to_string(&first).expect("first report");
    let stepped = std::fs::read_to_string(&stepped).expect("stepped report");
    assert_ne!(
        first, stepped,
        "advancing focus should change the captured focus state"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn headless_snapshot_writes_a_real_png() {
    let dir = std::env::temp_dir().join(format!("tui-lipan-headless-png-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("snapshot.png");

    AppRunner::new(App::new(), HeadlessSnapshotSmoke, ())
        .run_headless_snapshot(headless_snapshot_config(path.clone(), 0))
        .expect("headless snapshot succeeds");

    let bytes = std::fs::read(&path).expect("png file exists");
    assert!(
        bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]),
        "expected a PNG signature, got {} bytes",
        bytes.len()
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Appends typed characters to state, so a key script's effect is observable.
struct KeyScriptSmoke;

impl Component for KeyScriptSmoke {
    type Message = ();
    type Properties = ();
    type State = String;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        String::new()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> crate::KeyUpdate {
        if let KeyCode::Char(ch) = key.code {
            ctx.state.push(ch);
            return crate::KeyUpdate::handled(Update::full());
        }
        crate::KeyUpdate::unhandled(Update::none())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Text::new(format!("typed:{}", ctx.state)).into()
    }
}

fn key_script_config(
    path: std::path::PathBuf,
    keys: Vec<KeyEvent>,
) -> super::headless_snapshot::HeadlessSnapshotConfig {
    let actions = keys
        .into_iter()
        .map(crate::ui_snapshot::Action::Key)
        .collect();
    super::headless_snapshot::HeadlessSnapshotConfig {
        format: crate::ui_snapshot::UiSnapshotFileFormat::from_path(&path),
        path,
        viewport: Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 6,
        },
        frames: 1,
        focus_steps: 0,
        actions,
        diagnostic: false,
    }
}

fn char_key(ch: char) -> KeyEvent {
    KeyEvent {
        code: KeyCode::Char(ch),
        mods: KeyMods::default(),
    }
}

#[test]
fn headless_snapshot_key_script_reaches_the_component() {
    let dir = std::env::temp_dir().join(format!("tui-lipan-keyscript-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("typed.md");

    AppRunner::new(App::new(), KeyScriptSmoke, ())
        .run_headless_snapshot(key_script_config(
            path.clone(),
            vec![char_key('h'), char_key('i')],
        ))
        .expect("headless snapshot succeeds");

    let written = std::fs::read_to_string(&path).expect("snapshot file exists");
    assert!(
        written.contains("typed:hi"),
        "every scripted key should reach the component in order: {written}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn headless_snapshot_key_script_settles_between_keys() {
    // Regression guard: dispatching the whole script before re-rendering makes
    // each key act on a stale tree, collapsing typed text to its last character.
    let dir = std::env::temp_dir().join(format!("tui-lipan-keysettle-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("typed.md");

    AppRunner::new(App::new(), KeyScriptSmoke, ())
        .run_headless_snapshot(key_script_config(
            path.clone(),
            "abcd".chars().map(char_key).collect(),
        ))
        .expect("headless snapshot succeeds");

    let written = std::fs::read_to_string(&path).expect("snapshot file exists");
    assert!(
        written.contains("typed:abcd"),
        "expected the full typed string, not just the last key: {written}"
    );
    std::fs::remove_dir_all(&dir).ok();
}

/// Component with a clickable button, for exercising control commands.
struct ControlSmoke;

impl Component for ControlSmoke {
    type Message = ();
    type Properties = ();
    type State = bool;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        false
    }

    fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state = true;
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let label = if ctx.state { "clicked" } else { "idle" };
        VStack::new()
            .child(Text::new(label).key("label"))
            .child(
                crate::widgets::Button::new("Go")
                    .on_click(ctx.link().callback(|_| ()))
                    .key("go"),
            )
            .into()
    }
}

/// Drive one control command against a laid-out runner and return the reply.
fn control_exchange(
    runner: &mut AppRunner<ControlSmoke>,
    command: &str,
) -> std::result::Result<String, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    runner.handle_control(super::control::ControlRequest {
        command: command.to_owned(),
        reply: tx,
    });
    match rx.recv().expect("a reply is always sent") {
        super::control::ControlReply::Ok(payload) => Ok(payload),
        super::control::ControlReply::Err(message) => Err(message),
    }
}

/// A runner with an initialised, laid-out tree, as the live loop would have.
fn control_runner() -> AppRunner<ControlSmoke> {
    let mut runner = AppRunner::new(App::new(), ControlSmoke, ());
    let bounds = Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 8,
    };
    runner.core.ctx.set_viewport(bounds);
    runner.last_bounds = bounds;
    runner.core.init();
    runner.headless_render(bounds);
    runner
}

#[test]
fn control_ping_answers_pong() {
    let mut runner = control_runner();
    assert_eq!(control_exchange(&mut runner, "ping"), Ok("pong".to_owned()));
}

#[test]
fn control_keys_lists_rendered_widget_keys() {
    let mut runner = control_runner();
    let keys = control_exchange(&mut runner, "keys").expect("keys");
    let listed: Vec<&str> = keys.lines().collect();
    assert!(listed.contains(&"go"), "{keys}");
    assert!(listed.contains(&"label"), "{keys}");
}

#[test]
fn control_snapshot_reflects_the_live_viewport() {
    let mut runner = control_runner();
    let markdown = control_exchange(&mut runner, "snapshot").expect("snapshot");
    assert!(
        markdown.contains("40x8"),
        "snapshot should use the laid-out viewport, not 0x0: {markdown}"
    );
    assert!(markdown.contains("idle"), "{markdown}");
}

#[test]
fn control_act_clicks_a_widget_by_key() {
    let mut runner = control_runner();
    assert_eq!(
        control_exchange(&mut runner, "act click:#go"),
        Ok(String::new())
    );

    let markdown = control_exchange(&mut runner, "snapshot").expect("snapshot");
    assert!(
        markdown.contains("clicked"),
        "the click should have reached the button: {markdown}"
    );
}

#[test]
fn control_act_on_a_missing_key_reports_an_error() {
    let mut runner = control_runner();
    let err = control_exchange(&mut runner, "act click:#nope").expect_err("must fail");
    assert!(err.contains("nope"), "{err}");
}

#[test]
fn control_rejects_unknown_commands_without_touching_the_ui() {
    let mut runner = control_runner();
    let err = control_exchange(&mut runner, "frobnicate").expect_err("must fail");
    assert!(err.contains("expected"), "{err}");
    // The UI must be untouched by a rejected command.
    let markdown = control_exchange(&mut runner, "snapshot").expect("snapshot");
    assert!(markdown.contains("idle"), "{markdown}");
}

#[test]
fn control_quit_asks_the_app_to_exit() {
    let mut runner = control_runner();
    assert!(!runner.core.ctx.should_quit());
    assert_eq!(control_exchange(&mut runner, "quit"), Ok(String::new()));
    assert!(runner.core.ctx.should_quit());
}

#[test]
fn control_highlight_by_key_reports_the_widget_rect() {
    let mut runner = control_runner();
    let rect = control_exchange(&mut runner, "highlight go").expect("highlight");
    assert!(rect.contains('x'), "should report a rect, got {rect}");
    assert!(runner.highlight().is_some());
}

#[test]
fn control_highlight_by_cell_picks_the_smallest_widget() {
    let mut runner = control_runner();
    // The button is a small leaf inside a full-width stack; an inspector should
    // land on the leaf, not the container that happens to contain the cell.
    let button = runner
        .rect_for_key(&crate::core::element::Key::from("go".to_owned()))
        .expect("button is rendered");
    let cell = format!("highlight {},{}", button.x, button.y);

    control_exchange(&mut runner, &cell).expect("highlight by cell");
    assert_eq!(
        runner.highlight(),
        Some(button),
        "cell targeting should resolve to the smallest covering widget"
    );
}

#[test]
fn control_highlight_clear_removes_the_outline() {
    let mut runner = control_runner();
    control_exchange(&mut runner, "highlight go").expect("highlight");
    assert!(runner.highlight().is_some());

    control_exchange(&mut runner, "highlight clear").expect("clear");
    assert!(runner.highlight().is_none());
}

#[test]
fn control_highlight_on_a_missing_key_reports_an_error() {
    let mut runner = control_runner();
    let err = control_exchange(&mut runner, "highlight nope").expect_err("must fail");
    assert!(err.contains("nope"), "{err}");
    assert!(
        runner.highlight().is_none(),
        "a failed highlight sets nothing"
    );
}

#[test]
fn control_highlight_marks_cells_in_the_captured_frame() {
    // The outline has to reach captures, not just the live paint, or an agent
    // cannot see what it highlighted.
    let mut runner = control_runner();
    let plain = runner.headless_frame(runner.last_bounds);

    control_exchange(&mut runner, "highlight go").expect("highlight");
    let marked = runner.headless_frame(runner.last_bounds);

    assert_ne!(
        plain.cells, marked.cells,
        "highlighting should change the rendered frame"
    );
}

/// Esc cancels a pending chord, and a keystroke that cancels a chord must not also reach the pane.
/// A leaked `Esc` is not a harmless extra byte: terminals read `ESC` followed by a key as `Alt+key`,
/// so cancelling the prefix silently turns the *next* keystroke into a meta chord.
#[cfg(feature = "terminal")]
#[test]
fn esc_cancelling_a_pending_chord_is_not_forwarded_to_the_terminal() {
    let keys = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal);
    let mut runner = AppRunner::new(
        app,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        },
        (),
    );
    init_runner(
        &mut runner,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        },
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
    );
    runner.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.detach")
            .shortcut(crate::KeyBinding::from_str("ctrl-a d").expect("binding"))
            .handler(Callback::new(|_| {}))
            .build(),
    );
    let terminal = node_id_by_key(&runner.core.tree, "terminal");
    runner.focus.focused = Some(terminal);

    assert!(runner.dispatch_layered_key(ctrl_char('a')).consumed);
    let cancel = runner.dispatch_layered_key(key(KeyCode::Esc));
    assert!(cancel.consumed, "the cancelling Esc is spent on the cancel");
    assert!(
        runner
            .core
            .ctx
            .env()
            .command_chord_pending_since
            .get()
            .is_none()
    );
    assert!(
        keys.borrow().is_empty(),
        "Esc must not reach the pane: {:?}",
        keys.borrow()
    );

    // With no chord pending, Esc is an ordinary key and must still reach the pane - a vim user in a
    // pane presses it constantly.
    let plain = runner.dispatch_layered_key(key(KeyCode::Esc));
    assert!(
        plain.consumed,
        "forwarded to the terminal counts as handled"
    );
    assert_eq!(keys.borrow().as_slice(), &[key(KeyCode::Esc)]);
}

/// `CancelOnly` must actually swallow the mismatching key, not merely stop replaying the prefix:
/// once the prefix has been pressed the user is in the app's command state, and an unbound key
/// there means "nothing", not "type this into the pane".
#[cfg(feature = "terminal")]
#[test]
fn cancel_only_swallows_an_unbound_key_after_the_prefix() {
    let keys = Rc::new(RefCell::new(Vec::new()));
    let app = App::new()
        .mouse(false)
        .key_dispatch_policy(crate::KeyDispatchPolicy::AppCommandsFirst)
        .terminal_key_policy(crate::TerminalKeyPolicy::AppCommandsThenTerminal)
        .chord_mismatch_policy(crate::ChordMismatchPolicy::CancelOnly);
    let mut runner = AppRunner::new(
        app,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        },
        (),
    );
    init_runner(
        &mut runner,
        TerminalDispatchSmoke {
            keys: keys.clone(),
            inputs: Rc::new(RefCell::new(Vec::new())),
        },
        Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 5,
        },
    );
    runner.core.ctx.command_registry().register(
        crate::CommandEntry::builder("mux.detach")
            .shortcut(crate::KeyBinding::from_str("ctrl-a d").expect("binding"))
            .handler(Callback::new(|_| {}))
            .build(),
    );
    let terminal = node_id_by_key(&runner.core.tree, "terminal");
    runner.focus.focused = Some(terminal);

    assert!(runner.dispatch_layered_key(ctrl_char('a')).consumed);
    runner.dispatch_layered_key(key(KeyCode::Char('x')));
    assert!(
        keys.borrow().is_empty(),
        "an unbound key after the prefix must not reach the pane: {:?}",
        keys.borrow()
    );
    assert!(
        runner
            .core
            .ctx
            .env()
            .command_chord_pending_since
            .get()
            .is_none(),
        "and the chord must end"
    );

    // The following key is ordinary again.
    runner.dispatch_layered_key(key(KeyCode::Char('x')));
    assert_eq!(keys.borrow().as_slice(), &[key(KeyCode::Char('x'))]);
}
