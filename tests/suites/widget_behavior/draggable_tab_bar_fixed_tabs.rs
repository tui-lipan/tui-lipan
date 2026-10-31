//! Selectable tabs can opt out of reordering without becoming action tabs.
//!
//! `reorderable(false)` keeps selection, hover, and close, and it is a barrier:
//! the tab cannot start a drag, and other tabs cannot cross it.

use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

#[derive(Clone, Copy)]
enum Scenario {
    Reorder(DragReorderMode),
    Interior(DragReorderMode),
    Transfer(DragReorderMode),
    Select,
    Close,
    Action,
}

struct FixedTabs {
    scenario: Scenario,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum TabKind {
    Normal,
    Fixed,
    Action,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Tab {
    label: String,
    kind: TabKind,
    closeable: bool,
}

struct State {
    tabs: Vec<Tab>,
    incoming: Vec<Tab>,
    selected: usize,
    reorders: Vec<(usize, usize)>,
    closed: Option<usize>,
    actions: Vec<usize>,
}

#[derive(Clone)]
enum Msg {
    Select(TabsEvent),
    Reorder(DraggableTabReorderEvent),
    Transfer(DraggableTabTransferEvent),
    Close(DraggableTabCloseEvent),
    Action(DraggableTabActionEvent),
}

impl Component for FixedTabs {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let tab = |label: &str, kind: TabKind, closeable: bool| Tab {
            label: label.to_string(),
            kind,
            closeable,
        };
        let (tabs, incoming) = match self.scenario {
            Scenario::Reorder(_) | Scenario::Select => (
                vec![
                    tab("Home", TabKind::Fixed, false),
                    tab("Alpha", TabKind::Normal, false),
                    tab("Beta", TabKind::Normal, false),
                ],
                Vec::new(),
            ),
            Scenario::Interior(_) => (
                vec![
                    tab("Left", TabKind::Normal, false),
                    tab("Pin", TabKind::Fixed, false),
                    tab("Right", TabKind::Normal, false),
                ],
                Vec::new(),
            ),
            Scenario::Transfer(_) => (
                vec![
                    tab("Home", TabKind::Fixed, false),
                    tab("Alpha", TabKind::Normal, false),
                    tab("Beta", TabKind::Normal, false),
                ],
                vec![tab("Session", TabKind::Normal, false)],
            ),
            Scenario::Close => (
                vec![
                    tab("Home", TabKind::Fixed, true),
                    tab("Alpha", TabKind::Normal, true),
                ],
                Vec::new(),
            ),
            Scenario::Action => (
                vec![
                    tab("Alpha", TabKind::Normal, false),
                    tab("+", TabKind::Action, true),
                ],
                Vec::new(),
            ),
        };
        State {
            tabs,
            incoming,
            selected: match self.scenario {
                Scenario::Select => 1,
                _ => 0,
            },
            reorders: Vec::new(),
            closed: None,
            actions: Vec::new(),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Select(event) => ctx.state.selected = event.index,
            Msg::Reorder(event) => {
                ctx.state.reorders.push((event.from, event.to));
                move_tab(&mut ctx.state.tabs, event.from, event.to);
                ctx.state.selected = event.to;
            }
            Msg::Transfer(event) => {
                let tab = ctx.state.incoming.remove(event.from);
                let at = event.to.min(ctx.state.tabs.len());
                ctx.state.tabs.insert(at, tab);
            }
            Msg::Close(event) => {
                ctx.state.closed = Some(event.index);
                if event.index < ctx.state.tabs.len() {
                    ctx.state.tabs.remove(event.index);
                }
            }
            Msg::Action(event) => ctx.state.actions.push(event.index),
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mode = match self.scenario {
            Scenario::Reorder(mode) | Scenario::Interior(mode) | Scenario::Transfer(mode) => mode,
            _ => DragReorderMode::Live,
        };
        let bar = |id: &str, tabs: &[Tab]| {
            DraggableTabBar::new()
                .tabs(tabs.iter().map(to_tab))
                .active(ctx.state.selected)
                .bar_id(id)
                .drag_group("editors")
                .reorder_mode(mode)
                .drag_threshold(1)
                .divider('|')
                .close_symbol("x")
                .show_overflow_controls(false)
                .focusable(false)
                .height(Length::Px(1))
                .on_change(ctx.link().callback(Msg::Select))
                .on_reorder(ctx.link().callback(Msg::Reorder))
                .on_transfer(ctx.link().callback(Msg::Transfer))
                .on_close(ctx.link().callback(Msg::Close))
                .on_action(ctx.link().callback(Msg::Action))
        };
        match self.scenario {
            Scenario::Transfer(_) => VStack::new()
                .child(bar("top", &ctx.state.incoming))
                .child(Spacer::new().height(Length::Px(1)))
                .child(bar("bottom", &ctx.state.tabs))
                .into(),
            _ => bar("only", &ctx.state.tabs).into(),
        }
    }
}

fn to_tab(tab: &Tab) -> DraggableTab {
    let built = match tab.kind {
        TabKind::Action => DraggableTab::action(tab.label.as_str()),
        TabKind::Fixed => DraggableTab::new(tab.label.as_str()).reorderable(false),
        TabKind::Normal => DraggableTab::new(tab.label.as_str()),
    };
    built.closeable(tab.closeable)
}

fn move_tab(tabs: &mut Vec<Tab>, from: usize, to: usize) {
    if from >= tabs.len() || to >= tabs.len() || from == to {
        return;
    }
    let moved = tabs.remove(from);
    tabs.insert(to, moved);
}

fn labels(tabs: &[Tab]) -> Vec<&str> {
    tabs.iter().map(|tab| tab.label.as_str()).collect()
}

fn mouse(x: u16, y: u16, kind: MouseKind) -> MouseEvent {
    MouseEvent {
        x,
        y,
        kind,
        mods: KeyMods::NONE,
    }
}

fn backend(scenario: Scenario, height: u16) -> TestBackend<FixedTabs> {
    let mut backend = TestBackend::new(FixedTabs { scenario });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 80,
        h: height,
    });
    backend.render();
    backend
}

fn column(backend: &mut TestBackend<FixedTabs>, row: usize, label: &str) -> u16 {
    backend.render();
    let lines = backend.capture_frame().to_fixed_grid_lines();
    lines[row].find(label).expect(label) as u16
}

/// A cell on `label` that is past the tab midpoint, so a Live drag commits the move.
fn column_past_midpoint(backend: &mut TestBackend<FixedTabs>, row: usize, label: &str) -> u16 {
    column(backend, row, label) + label.len().saturating_sub(1) as u16
}

#[test]
fn fixed_home_selects_and_does_not_start_a_drag() {
    let mut backend = backend(Scenario::Select, 2);
    let home = column(&mut backend, 0, "Home");

    assert!(
        backend
            .send_mouse(mouse(home, 0, MouseKind::Down(MouseButton::Left)))
            .unwrap()
    );
    assert_eq!(backend.state().selected, 0);
    assert_eq!(labels(&backend.state().tabs), ["Home", "Alpha", "Beta"]);

    backend
        .send_mouse(mouse(
            home.saturating_add(6),
            1,
            MouseKind::Drag(MouseButton::Left),
        ))
        .unwrap();

    assert!(backend.state().reorders.is_empty());
    assert_eq!(labels(&backend.state().tabs), ["Home", "Alpha", "Beta"]);
    let lines = backend.capture_frame().to_fixed_grid_lines();
    assert!(
        !lines[1].contains("Home"),
        "dragging Home must not float a preview, got {:?}",
        lines
    );
}

#[test]
fn sessions_reorder_around_home_but_cannot_cross_it() {
    for mode in [DragReorderMode::Live, DragReorderMode::OnDrop] {
        let mut backend = backend(Scenario::Reorder(mode), 1);
        let alpha = column(&mut backend, 0, "Alpha");
        let beta = column_past_midpoint(&mut backend, 0, "Beta");
        let home = column(&mut backend, 0, "Home");

        drag(&mut backend, alpha, beta, mode);
        assert_eq!(
            labels(&backend.state().tabs),
            ["Home", "Beta", "Alpha"],
            "{mode:?}"
        );

        let beta = column(&mut backend, 0, "Beta");
        drag(&mut backend, beta, home, mode);
        assert_eq!(
            labels(&backend.state().tabs)[0],
            "Home",
            "{mode:?} crossed the fixed tab: {:?}",
            labels(&backend.state().tabs)
        );
    }
}

#[test]
fn an_interior_fixed_tab_separates_reorder_runs() {
    for mode in [DragReorderMode::Live, DragReorderMode::OnDrop] {
        let mut backend = backend(Scenario::Interior(mode), 1);
        let left = column(&mut backend, 0, "Left");
        let right = column_past_midpoint(&mut backend, 0, "Right");
        drag(&mut backend, left, right, mode);
        assert_eq!(
            labels(&backend.state().tabs),
            ["Left", "Pin", "Right"],
            "{mode:?}"
        );
    }
}

#[test]
fn an_incoming_tab_cannot_land_before_home() {
    for mode in [DragReorderMode::Live, DragReorderMode::OnDrop] {
        let mut backend = backend(Scenario::Transfer(mode), 3);
        let session = column(&mut backend, 0, "Session");
        let home = column(&mut backend, 2, "Home");
        drag_at(&mut backend, session, 0, home, 2, mode);
        assert_eq!(
            labels(&backend.state().tabs),
            ["Home", "Session", "Alpha", "Beta"],
            "{mode:?}"
        );
        assert!(backend.state().incoming.is_empty(), "{mode:?}");
    }
}

#[test]
fn a_fixed_tab_can_still_be_closed() {
    let mut backend = backend(Scenario::Close, 1);
    let close = column(&mut backend, 0, "x");
    assert!(
        backend
            .send_mouse(mouse(close, 0, MouseKind::Down(MouseButton::Left)))
            .unwrap()
    );
    assert_eq!(backend.state().closed, Some(0));
    assert_eq!(labels(&backend.state().tabs), ["Alpha"]);
}

#[test]
fn an_action_tab_still_does_not_select_or_reorder() {
    let mut backend = backend(Scenario::Action, 1);
    let action = column(&mut backend, 0, "+");
    let alpha = column(&mut backend, 0, "Alpha");
    assert!(
        backend
            .send_mouse(mouse(action, 0, MouseKind::Down(MouseButton::Left)))
            .unwrap(),
        "row {:?}",
        backend.capture_frame().to_fixed_grid_lines()
    );
    assert_eq!(backend.state().actions, [1]);
    assert_eq!(backend.state().selected, 0);
    assert!(backend.state().reorders.is_empty());

    backend
        .send_mouse(mouse(alpha, 0, MouseKind::Drag(MouseButton::Left)))
        .unwrap();
    assert_eq!(labels(&backend.state().tabs), ["Alpha", "+"]);
    assert!(backend.state().reorders.is_empty());
}

fn drag(backend: &mut TestBackend<FixedTabs>, from_x: u16, to_x: u16, mode: DragReorderMode) {
    drag_at(backend, from_x, 0, to_x, 0, mode);
}

fn drag_at(
    backend: &mut TestBackend<FixedTabs>,
    from_x: u16,
    from_y: u16,
    to_x: u16,
    to_y: u16,
    mode: DragReorderMode,
) {
    assert!(
        backend
            .send_mouse(mouse(from_x, from_y, MouseKind::Down(MouseButton::Left)))
            .unwrap()
    );
    backend
        .send_mouse(mouse(to_x, to_y, MouseKind::Drag(MouseButton::Left)))
        .unwrap();
    // Live commits during the move. OnDrop commits on release. Releasing either
    // way ends the gesture so the next press is not swallowed by an active drag.
    assert!(
        backend
            .send_mouse(mouse(to_x, to_y, MouseKind::Up(MouseButton::Left)))
            .unwrap(),
        "release should finish {mode:?}"
    );
}
