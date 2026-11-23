//! Regression tests for selection API consistency across list-like widgets.

use tui_lipan::core::event::{MouseButton, MouseKind};
use tui_lipan::prelude::*;
use tui_lipan::{RadioEvent as RootRadioEvent, SelectEvent as RootSelectEvent, TestBackend};

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        mods: KeyMods::NONE,
    }
}

fn left_click(x: u16, y: u16) -> MouseEvent {
    MouseEvent {
        x,
        y,
        kind: MouseKind::Down(MouseButton::Left),
        mods: KeyMods::NONE,
    }
}

// ── Select ───────────────────────────────────────────────────────────────────

#[derive(Clone)]
enum SelectMsg {
    Toggle(bool),
    Pick(SelectEvent),
}

struct SelectState {
    selected: Option<usize>,
    expanded: bool,
    picks: Vec<SelectEvent>,
}

struct SelectApp {
    initial: Option<usize>,
    initially_expanded: bool,
}

impl Component for SelectApp {
    type Message = SelectMsg;
    type Properties = ();
    type State = SelectState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        SelectState {
            selected: self.initial,
            expanded: self.initially_expanded,
            picks: Vec::new(),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            SelectMsg::Toggle(open) => ctx.state.expanded = open,
            SelectMsg::Pick(ev) => {
                ctx.state.selected = Some(ev.index);
                ctx.state.picks.push(ev);
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Select::new()
            .options(["Alpha", "Beta", "Gamma"])
            .selected(ctx.state.selected)
            .expanded(ctx.state.expanded)
            .on_toggle(ctx.link().callback(SelectMsg::Toggle))
            .on_select(ctx.link().callback(SelectMsg::Pick))
            .key("select")
    }
}

fn press<C: Component>(backend: &mut TestBackend<C>, code: KeyCode) {
    backend.send_key(key(code)).expect("key dispatches");
    backend.pump().expect("messages pump");
    backend.render();
}

#[test]
fn select_emits_once_per_keyboard_pick_and_none_on_highlight() {
    let mut backend = TestBackend::new(SelectApp {
        initial: Some(0),
        initially_expanded: false,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 12,
    });
    backend.render();
    assert!(backend.focus_key(&Key::from("select")));
    backend.render();

    // Open dropdown, move highlight, then dismiss with Esc: no on_select.
    press(&mut backend, KeyCode::Enter);
    assert!(backend.state().expanded);
    press(&mut backend, KeyCode::Down);
    assert!(
        backend.state().picks.is_empty(),
        "mere highlight movement must not fire on_select"
    );
    press(&mut backend, KeyCode::Esc);
    assert!(!backend.state().expanded);
    assert!(backend.state().picks.is_empty());

    // Re-open (highlight resets to selected=0), move Down twice, commit with Enter.
    press(&mut backend, KeyCode::Enter);
    press(&mut backend, KeyCode::Down);
    press(&mut backend, KeyCode::Down);
    assert!(backend.state().picks.is_empty());
    press(&mut backend, KeyCode::Enter);

    assert!(!backend.state().expanded);
    assert_eq!(backend.state().picks.len(), 1);
    let pick: &RootSelectEvent = &backend.state().picks[0];
    assert_eq!(pick.index, 2);
    assert_eq!(pick.label.as_ref(), "Gamma");
}

#[test]
fn select_emits_once_per_mouse_pick() {
    let mut backend = TestBackend::new(SelectApp {
        initial: Some(0),
        initially_expanded: true,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 12,
    });
    backend.render();

    let list_rect = backend
        .capture_ui_snapshot()
        .widgets
        .iter()
        .find(|w| w.kind == tui_lipan::UiWidgetKind::List)
        .expect("expanded select renders inner list")
        .rect;

    // Border is 1 cell; row 0 is at list_rect.y + 1, row 1 ("Beta") is at list_rect.y + 2.
    backend
        .send_mouse(left_click(
            (list_rect.x + 2) as u16,
            (list_rect.y + 2) as u16,
        ))
        .expect("mouse click dispatches");
    backend.pump().expect("messages pump");
    backend.render();

    assert!(!backend.state().expanded);
    assert_eq!(backend.state().picks.len(), 1);
    assert_eq!(backend.state().picks[0].index, 1);
    assert_eq!(backend.state().picks[0].label.as_ref(), "Beta");
}

#[test]
fn select_none_clears_selection_and_requires_highlight_before_enter() {
    let mut backend = TestBackend::new(SelectApp {
        initial: None,
        initially_expanded: true,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 12,
    });
    backend.render();

    let snapshot = backend.capture_ui_snapshot();
    let list = snapshot
        .widgets
        .iter()
        .find(|w| w.kind == tui_lipan::UiWidgetKind::List)
        .expect("expanded select renders an inner list");
    assert_eq!(list.selected_index, None);

    // Enter with no highlight must not pick anything.
    press(&mut backend, KeyCode::Enter);
    assert!(backend.state().picks.is_empty());

    // First Down seeds highlight to 0 without emitting on_select; Enter then picks 0.
    press(&mut backend, KeyCode::Down);
    assert!(backend.state().picks.is_empty());
    press(&mut backend, KeyCode::Enter);
    assert_eq!(backend.state().picks.len(), 1);
    assert_eq!(backend.state().picks[0].index, 0);
    assert_eq!(backend.state().picks[0].label.as_ref(), "Alpha");
}

// ── Tabs & DraggableTabBar ───────────────────────────────────────────────────

struct TabsNoneApp {
    draggable: bool,
}

#[derive(Default)]
struct TabsNoneState {
    selected: Option<usize>,
    events: Vec<TabsEvent>,
}

impl Component for TabsNoneApp {
    type Message = TabsEvent;
    type Properties = ();
    type State = TabsNoneState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        TabsNoneState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.selected = Some(msg.index);
        ctx.state.events.push(msg);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        if self.draggable {
            DraggableTabBar::new()
                .tabs([DraggableTab::new("One"), DraggableTab::new("Two")])
                .selected(ctx.state.selected)
                .show_close_buttons(false)
                .focusable(true)
                .style(Style::new().bg(Color::Black))
                .active_style(Style::new().bg(Color::Cyan))
                .on_select(ctx.link().callback(|e| e))
                .key("tabs")
        } else {
            Tabs::new()
                .tabs([Tab::new("One"), Tab::new("Two")])
                .selected(ctx.state.selected)
                .focusable(true)
                .style(Style::new().bg(Color::Black))
                .active_style(Style::new().bg(Color::Cyan))
                .on_select(ctx.link().callback(|e| e))
                .key("tabs")
        }
    }
}

#[test]
fn tabs_none_clears_highlight_and_allows_selecting_tab_zero() {
    for draggable in [false, true] {
        let mut backend = TestBackend::new(TabsNoneApp { draggable });
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 1,
        });
        backend.render();

        let frame = backend.capture_frame();
        for x in 0..20 {
            assert_ne!(
                frame.cell(x, 0).bg,
                Color::Cyan,
                "draggable={draggable}: no tab should be painted with active_style at col {x}"
            );
        }

        let snapshot = backend.capture_ui_snapshot();
        let kind = if draggable {
            tui_lipan::UiWidgetKind::DraggableTabBar
        } else {
            tui_lipan::UiWidgetKind::Tabs
        };
        let widget = snapshot
            .widgets
            .iter()
            .find(|w| w.kind == kind)
            .expect("tab bar in snapshot");
        assert_eq!(widget.selected_index, None);

        // Clicking tab 0 when selected is None must emit TabsEvent { index: 0 }.
        backend
            .send_mouse(left_click(1, 0))
            .expect("mouse down tab 0 dispatches");
        backend
            .send_mouse(MouseEvent {
                x: 1,
                y: 0,
                kind: MouseKind::Up(MouseButton::Left),
                mods: KeyMods::NONE,
            })
            .expect("mouse up tab 0 dispatches");
        backend.pump().expect("pump");
        backend.render();

        assert_eq!(backend.state().events.len(), 1);
        assert_eq!(backend.state().events[0].index, 0);
        assert_eq!(backend.state().selected, Some(0));
    }
}

#[test]
fn tabs_none_seeds_keyboard_navigation_from_ends() {
    for (draggable, code, expected) in [
        (false, KeyCode::Right, 0),
        (false, KeyCode::Left, 1),
        (true, KeyCode::Right, 0),
        (true, KeyCode::Left, 1),
    ] {
        let mut backend = TestBackend::new(TabsNoneApp { draggable });
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 1,
        });
        backend.render();
        assert!(backend.focus_key(&Key::from("tabs")));
        backend.render();

        press(&mut backend, code);
        assert_eq!(backend.state().events.len(), 1);
        assert_eq!(backend.state().events[0].index, expected);
    }
}

// ── Tree ─────────────────────────────────────────────────────────────────────

struct TreeSelectionApp {
    selected: Option<usize>,
}

impl Component for TreeSelectionApp {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}
    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _ctx: &Context<Self>) -> Element {
        let root = TreeNode::new("root")
            .expanded(true)
            .child(TreeNode::new("child-a"))
            .child(TreeNode::new("child-b"));
        Tree::new(root).selected(self.selected).into()
    }
}

#[test]
fn tree_selected_none_clears_selection() {
    let mut cleared = TestBackend::new(TreeSelectionApp { selected: None });
    cleared.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 8,
    });
    cleared.render();
    let list = cleared
        .capture_ui_snapshot()
        .widgets
        .into_iter()
        .find(|w| w.kind == tui_lipan::UiWidgetKind::List)
        .expect("tree renders inner list");
    assert_eq!(list.selected_index, None);

    let mut picked = TestBackend::new(TreeSelectionApp { selected: Some(2) });
    picked.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 8,
    });
    picked.render();
    let list = picked
        .capture_ui_snapshot()
        .widgets
        .into_iter()
        .find(|w| w.kind == tui_lipan::UiWidgetKind::List)
        .expect("tree renders inner list");
    assert_eq!(list.selected_index, Some(2));
}

// ── Radio ────────────────────────────────────────────────────────────────────

#[derive(Default)]
struct RadioAppState {
    selected: Option<usize>,
    events: Vec<RadioEvent>,
}

struct RadioApp;

impl Component for RadioApp {
    type Message = RadioEvent;
    type Properties = ();
    type State = RadioAppState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        RadioAppState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.selected = Some(msg.index);
        ctx.state.events.push(msg);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Radio::new(["Light", "Dark", "System"])
            .selected(ctx.state.selected)
            .on_select(ctx.link().callback(|e| e))
            .into()
    }
}

#[test]
fn radio_none_clears_selection_and_on_select_emits_radio_event() {
    let mut backend = TestBackend::new(RadioApp);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 6,
    });
    backend.render();

    // No checkbox is checked when selected is None.
    let snapshot = backend.capture_ui_snapshot();
    let checkboxes: Vec<_> = snapshot
        .widgets
        .iter()
        .filter(|w| w.kind == tui_lipan::UiWidgetKind::Checkbox)
        .collect();
    assert_eq!(checkboxes.len(), 3);
    assert!(
        checkboxes
            .iter()
            .all(|cb| cb.checkbox_state == Some(CheckboxState::Unchecked))
    );

    // Focus the radio group and move Down -> emits RadioEvent with index + label.
    backend.focus_next();
    backend.render();
    press(&mut backend, KeyCode::Down);

    assert_eq!(backend.state().events.len(), 1);
    let ev: &RootRadioEvent = &backend.state().events[0];
    assert_eq!(ev.index, 1);
    assert_eq!(ev.label.as_ref(), "Dark");
}

// ── ComboBox & MultiSelect ───────────────────────────────────────────────────

struct ComboBoxNoneApp {
    query: &'static str,
    selected: Option<usize>,
    highlighted: Option<usize>,
}

#[derive(Default)]
struct ComboBoxNoneState {
    highlights: Vec<Option<usize>>,
}

impl Component for ComboBoxNoneApp {
    type Message = Option<usize>;
    type Properties = ();
    type State = ComboBoxNoneState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        ComboBoxNoneState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.highlights.push(msg);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        ComboBox::new()
            .items(["Alpha", "Beta", "Gamma"])
            .query(self.query)
            .open(true)
            .selected(self.selected)
            .highlighted(self.highlighted)
            .on_highlight(ctx.link().callback(|i| i))
            .key("combo")
    }
}

#[test]
fn combo_box_selected_and_highlighted_support_none_and_on_highlight() {
    // Explicit selected(2) drives list highlight to 2; selected(None) clears it.
    let mut with_sel = TestBackend::new(ComboBoxNoneApp {
        query: "",
        selected: Some(2),
        highlighted: None,
    });
    with_sel.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 12,
    });
    with_sel.render();
    let list = with_sel
        .capture_ui_snapshot()
        .widgets
        .into_iter()
        .find(|w| w.kind == tui_lipan::UiWidgetKind::List)
        .expect("open combo renders inner list");
    assert_eq!(list.selected_index, Some(2));

    let mut cleared = TestBackend::new(ComboBoxNoneApp {
        query: "zzz",
        selected: None,
        highlighted: None,
    });
    cleared.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 12,
    });
    cleared.render();
    let list = cleared
        .capture_ui_snapshot()
        .widgets
        .into_iter()
        .find(|w| w.kind == tui_lipan::UiWidgetKind::List)
        .expect("open combo renders inner list");
    assert_eq!(list.selected_index, None);

    // Explicit highlighted overrides selected and on_highlight fires on Down.
    let mut highlighted = TestBackend::new(ComboBoxNoneApp {
        query: "",
        selected: Some(0),
        highlighted: Some(1),
    });
    highlighted.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 12,
    });
    highlighted.render();
    assert!(highlighted.focus_key(&Key::from("combo")));
    highlighted.render();
    press(&mut highlighted, KeyCode::Down);
    assert_eq!(highlighted.state().highlights, vec![Some(2)]);
}

struct MultiSelectHighlightApp {
    highlighted: Option<usize>,
}

#[derive(Default)]
struct MultiSelectHighlightState {
    highlights: Vec<usize>,
    toggles: Vec<MultiSelectToggleEvent>,
}

impl Component for MultiSelectHighlightApp {
    type Message = MultiMsg;
    type Properties = ();
    type State = MultiSelectHighlightState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        MultiSelectHighlightState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            MultiMsg::Highlight(i) => ctx.state.highlights.push(i),
            MultiMsg::Toggle(ev) => ctx.state.toggles.push(ev),
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        MultiSelect::new()
            .items(["One", "Two", "Three"])
            .highlighted(self.highlighted)
            .on_highlight(ctx.link().callback(MultiMsg::Highlight))
            .on_toggle(ctx.link().callback(MultiMsg::Toggle))
            .key("multi")
    }
}

#[derive(Clone)]
enum MultiMsg {
    Highlight(usize),
    Toggle(MultiSelectToggleEvent),
}

#[test]
fn multi_select_highlighted_none_clears_cursor_and_on_highlight_fires() {
    let mut cleared = TestBackend::new(MultiSelectHighlightApp { highlighted: None });
    cleared.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 8,
    });
    cleared.render();
    let list = cleared
        .capture_ui_snapshot()
        .widgets
        .into_iter()
        .find(|w| w.kind == tui_lipan::UiWidgetKind::List)
        .expect("multi_select renders inner list");
    assert_eq!(list.selected_index, None);

    assert!(cleared.focus_key(&Key::from("multi")));
    cleared.render();
    // Space with no highlight must not toggle any row.
    press(&mut cleared, KeyCode::Char(' '));
    assert!(cleared.state().toggles.is_empty());

    // Down seeds highlight to 0 via on_highlight.
    press(&mut cleared, KeyCode::Down);
    assert_eq!(cleared.state().highlights, vec![0]);

    // Bare usize highlighted(1) works via Into<Option<usize>> and toggles row 1 on Space.
    let mut active = TestBackend::new(MultiSelectHighlightApp {
        highlighted: Some(1),
    });
    active.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 8,
    });
    active.render();
    assert!(active.focus_key(&Key::from("multi")));
    active.render();
    press(&mut active, KeyCode::Char(' '));
    assert_eq!(active.state().toggles.len(), 1);
    assert_eq!(active.state().toggles[0].index, 1);
    assert!(active.state().toggles[0].selected);
}
