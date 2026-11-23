use tui_lipan::prelude::*;
use tui_lipan::{App, ComboBox, ContextMenu, Popover, TestBackend, Tooltip};

const TARGETS: &[&str] = &[
    "Local sandbox",
    "Staging cluster",
    "Production canary",
    "Production full",
];

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent {
        code,
        mods: KeyMods::default(),
    }
}

#[derive(Default)]
struct SelectState {
    selected: Option<usize>,
    expanded: bool,
    select_events: Vec<(usize, String)>,
}

#[derive(Clone, Debug)]
enum SelectMsg {
    Toggle(bool),
    Picked(SelectEvent),
}

struct InlineSelectHarness {
    top_spacer_rows: u16,
    list_height: u16,
    with_leading_button: bool,
}

impl InlineSelectHarness {
    fn new(top_spacer_rows: u16, list_height: u16, with_leading_button: bool) -> Self {
        Self {
            top_spacer_rows,
            list_height,
            with_leading_button,
        }
    }
}

impl Component for InlineSelectHarness {
    type Message = SelectMsg;
    type Properties = ();
    type State = SelectState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        SelectState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            SelectMsg::Toggle(expanded) => {
                ctx.state.expanded = expanded;
                Update::render()
            }
            SelectMsg::Picked(ev) => {
                ctx.state.selected = Some(ev.index);
                ctx.state.expanded = false;
                ctx.state
                    .select_events
                    .push((ev.index, ev.label.to_string()));
                Update::render()
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut stack = VStack::new();
        if self.top_spacer_rows > 0 {
            stack = stack.child(Spacer::new().height(Length::Px(self.top_spacer_rows)));
        }
        if self.with_leading_button {
            stack = stack.child(Button::new("Before").focusable(true));
        }
        stack
            .child(
                Select::new()
                    .options(TARGETS.iter().copied())
                    .selected(ctx.state.selected)
                    .placeholder("Choose target")
                    .expanded(ctx.state.expanded)
                    .list_height(Length::Px(self.list_height))
                    .on_toggle(ctx.link().callback(SelectMsg::Toggle))
                    .on_select(ctx.link().callback(SelectMsg::Picked))
                    .width(Length::Px(28)),
            )
            .into()
    }
}

fn assert_inline_select_lifecycle(app: App, viewport_h: u16) {
    let mut backend = TestBackend::new_with_app_and_viewport(
        app,
        InlineSelectHarness::new(0, 6, true),
        (),
        Rect {
            x: 0,
            y: 0,
            w: 60,
            h: viewport_h,
        },
    );

    // Move focus onto the leading button, then onto the Select trigger.
    backend.send_key(key(KeyCode::Tab)).unwrap();
    backend.send_key(key(KeyCode::Tab)).unwrap();
    assert!(!backend.state().expanded);

    // Press Enter on the focused Select trigger to open the popup.
    backend.send_key(key(KeyCode::Enter)).unwrap();
    assert!(
        backend.state().expanded,
        "Enter on Select trigger should set expanded(true)"
    );

    let open_text = backend.capture_frame().plain_text();
    assert!(
        open_text.contains("Local sandbox") && open_text.contains("Staging cluster"),
        "open Select popup must paint its options in inline mode, got:\n{open_text}"
    );

    // Navigate from unset selection -> first option (0) -> second option (1), then commit.
    backend.send_key(key(KeyCode::Down)).unwrap();
    backend.send_key(key(KeyCode::Down)).unwrap();
    backend.send_key(key(KeyCode::Enter)).unwrap();

    assert!(
        !backend.state().expanded,
        "picking an option should close the popup"
    );
    assert_eq!(
        backend.state().select_events,
        vec![(1, "Staging cluster".to_string())],
        "on_select should fire exactly once for the picked option"
    );

    let closed_text = backend.capture_frame().plain_text();
    assert!(
        closed_text.contains("Staging cluster"),
        "trigger should show the selected option after closing"
    );
    assert!(
        !closed_text.contains("Production canary"),
        "popup options should be gone once closed, got:\n{closed_text}"
    );

    // Re-open with Space and dismiss with Esc.
    backend.send_key(key(KeyCode::Char(' '))).unwrap();
    assert!(backend.state().expanded);
    let reopened = backend.capture_frame().plain_text();
    assert!(
        reopened.contains("Production canary"),
        "Space should reopen the inline Select popup, got:\n{reopened}"
    );
    backend.send_key(key(KeyCode::Esc)).unwrap();
    assert!(!backend.state().expanded);
    assert_eq!(
        backend.state().select_events.len(),
        1,
        "Esc dismiss must not emit another on_select"
    );
}

#[test]
fn inline_ephemeral_select_opens_paints_options_picks_and_closes() {
    assert_inline_select_lifecycle(App::new().inline_ephemeral(24), 24);
}

#[test]
fn inline_transcript_select_opens_paints_options_picks_and_closes() {
    assert_inline_select_lifecycle(App::new().inline_transcript(24), 24);
}

#[test]
fn inline_fixed_viewport_flips_select_popup_above_when_bottom_overflows() {
    let mut backend = TestBackend::new_with_app_and_viewport(
        App::new().inline_ephemeral(10),
        InlineSelectHarness::new(7, 5, false),
        (),
        Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 10,
        },
    );

    backend.send_key(key(KeyCode::Tab)).unwrap();
    backend.send_key(key(KeyCode::Enter)).unwrap();
    assert!(backend.state().expanded);

    let text = backend.capture_frame().plain_text();
    assert!(
        text.contains("Local sandbox") && text.contains("Staging cluster"),
        "flipped inline popup should still render visible options, got:\n{text}"
    );

    // Trigger occupies rows 7..10; flipped popup must appear above row 7.
    let option_row = text
        .lines()
        .position(|line| line.contains("Local sandbox"))
        .expect("Local sandbox should appear on screen");
    assert!(
        option_row < 7,
        "expected flipped popup above trigger row 7, got row {option_row}:\n{text}"
    );
}

#[test]
fn inline_fixed_viewport_keeps_popup_below_when_above_has_less_space() {
    // Viewport height 7, trigger at rows 0..3 -> 0 rows above, 4 rows below.
    // A 6-row dropdown overflows below, but must stay below and shrink to 4 rows
    // rather than flipping above into 0 rows.
    let mut backend = TestBackend::new_with_app_and_viewport(
        App::new().inline_ephemeral(7),
        InlineSelectHarness::new(0, 6, false),
        (),
        Rect {
            x: 0,
            y: 0,
            w: 60,
            h: 7,
        },
    );

    backend.send_key(key(KeyCode::Tab)).unwrap();
    backend.send_key(key(KeyCode::Enter)).unwrap();
    assert!(backend.state().expanded);

    let text = backend.capture_frame().plain_text();
    assert!(
        text.contains("Local sandbox"),
        "popup should remain below and shrink to fit 4 rows instead of collapsing above, got:\n{text}"
    );
}

struct SharedPopoverWidgets;

impl Component for SharedPopoverWidgets {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        HStack::new()
            .gap(1)
            .child(
                ComboBox::new()
                    .items(["Alpha choice", "Beta choice"])
                    .open(true)
                    .width(Length::Px(20)),
            )
            .child(
                ContextMenu::new(Text::new("Menu trigger"))
                    .items(["Inspect", "Delete"])
                    .open(true),
            )
            .child(
                Popover::new()
                    .trigger(Text::new("Pop trigger"))
                    .content(Frame::new().border(true).child(Text::new("Popover body")))
                    .open(true),
            )
            .child(
                Tooltip::new("Tooltip hint")
                    .open(true)
                    .child(Text::new("Tip trigger")),
            )
            .child(Modal::new().child(Text::new("Suppressed modal")))
            .into()
    }
}

#[test]
fn inline_shared_popover_widgets_render_while_modal_stays_suppressed() {
    for app in [
        App::new().inline_ephemeral(24),
        App::new().inline_transcript(24),
    ] {
        let backend = TestBackend::new_with_app_and_viewport(
            app,
            SharedPopoverWidgets,
            (),
            Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 24,
            },
        );
        let text = backend.capture_frame().plain_text();
        assert!(
            text.contains("Alpha choice"),
            "ComboBox popup should paint in inline mode:\n{text}"
        );
        assert!(
            text.contains("Inspect"),
            "ContextMenu popup should paint in inline mode:\n{text}"
        );
        assert!(
            text.contains("Popover body"),
            "Popover content should paint in inline mode:\n{text}"
        );
        assert!(
            text.contains("Tooltip hint"),
            "Tooltip content should paint in inline mode:\n{text}"
        );
        assert!(
            !text.contains("Suppressed modal"),
            "Modal root portal should remain suppressed in inline mode:\n{text}"
        );
    }
}
