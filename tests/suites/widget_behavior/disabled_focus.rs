//! Disabled widgets are excluded from focus across every widget type.
//!
//! The rule lives in `Node::is_focusable`/`Node::is_tab_stop`, so these tests cover the
//! traversal ring, pointer focus, and the widget that becomes disabled while focused.

use tui_lipan::HexArea;
use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct Row;

#[derive(Clone, Copy)]
enum Msg {
    ToggleMiddle,
}

#[derive(Default)]
struct State {
    middle_disabled: bool,
}

impl Component for Row {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::ToggleMiddle => ctx.state.middle_disabled = !ctx.state.middle_disabled,
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        VStack::new()
            .child(Button::new("first").key("first"))
            .child(
                Button::new("middle")
                    .disabled(ctx.state.middle_disabled)
                    .key("middle"),
            )
            .child(Button::new("last").key("last"))
            .into()
    }
}

fn row_backend() -> TestBackend<Row> {
    let mut backend = TestBackend::new(Row);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    backend.render();
    backend
}

fn focused(backend: &TestBackend<Row>) -> Option<String> {
    backend.focused_key().map(|key| key.to_string())
}

#[test]
fn tab_traversal_skips_a_disabled_widget() {
    let mut backend = row_backend();
    backend.focus_key(&Key::from("first"));
    backend.render();

    backend.focus_next();
    backend.render();
    assert_eq!(focused(&backend), Some("middle".to_string()));

    backend.dispatch(Msg::ToggleMiddle).unwrap();
    backend.focus_key(&Key::from("first"));
    backend.render();

    backend.focus_next();
    backend.render();
    assert_eq!(
        focused(&backend),
        Some("last".to_string()),
        "a disabled widget cannot act on a key, so it must not hold a tab stop"
    );
}

#[test]
fn shift_tab_traversal_skips_a_disabled_widget() {
    let mut backend = row_backend();
    backend.dispatch(Msg::ToggleMiddle).unwrap();
    backend.focus_key(&Key::from("last"));
    backend.render();

    backend.focus_prev();
    backend.render();
    assert_eq!(focused(&backend), Some("first".to_string()));
}

#[test]
fn a_widget_disabled_while_focused_gives_up_focus() {
    let mut backend = row_backend();
    backend.focus_key(&Key::from("middle"));
    backend.render();
    assert_eq!(focused(&backend), Some("middle".to_string()));

    backend.dispatch(Msg::ToggleMiddle).unwrap();
    backend.render();

    assert_ne!(
        focused(&backend),
        Some("middle".to_string()),
        "focus must not be stranded on a widget that just became disabled"
    );
}

#[test]
fn re_enabling_restores_the_tab_stop() {
    let mut backend = row_backend();
    backend.dispatch(Msg::ToggleMiddle).unwrap();
    backend.render();
    backend.dispatch(Msg::ToggleMiddle).unwrap();
    backend.focus_key(&Key::from("first"));
    backend.render();

    backend.focus_next();
    backend.render();
    assert_eq!(focused(&backend), Some("middle".to_string()));
}

#[test]
fn request_focus_cannot_target_a_disabled_widget() {
    let mut backend = row_backend();
    backend.dispatch(Msg::ToggleMiddle).unwrap();
    backend.render();

    assert!(
        !backend.focus_key(&Key::from("middle")),
        "programmatic focus must respect the same rule as traversal"
    );
}

/// Every widget kind that carries `disabled` has to honor the rule, not just Button.
mod every_widget_kind {
    use super::*;

    macro_rules! disabled_widget_case {
        ($name:ident, $widget:expr) => {
            #[test]
            fn $name() {
                struct Harness;

                impl Component for Harness {
                    type Message = ();
                    type Properties = ();
                    type State = ();

                    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

                    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
                        Update::none()
                    }

                    fn view(&self, _ctx: &Context<Self>) -> Element {
                        VStack::new()
                            .child($widget)
                            .child(Button::new("after").key("after"))
                            .into()
                    }
                }

                let mut backend = TestBackend::new(Harness);
                backend.set_viewport(Rect {
                    x: 0,
                    y: 0,
                    w: 40,
                    h: 20,
                });
                backend.render();

                assert!(
                    !backend.focus_key(&Key::from("subject")),
                    "a disabled widget must not be focusable"
                );

                backend.focus_next();
                backend.render();
                assert_eq!(
                    backend.focused_key().map(|key| key.to_string()),
                    Some("after".to_string()),
                    "the disabled widget must not hold the first tab stop"
                );
            }
        };
    }

    disabled_widget_case!(button, Button::new("subject").disabled(true).key("subject"));
    disabled_widget_case!(
        checkbox,
        Checkbox::new(false)
            .label("subject")
            .disabled(true)
            .key("subject")
    );
    disabled_widget_case!(
        input,
        Input::new("subject".to_string())
            .disabled(true)
            .key("subject")
    );
    disabled_widget_case!(
        list,
        List::new()
            .items([ListItem::new("one"), ListItem::new("two")])
            .disabled(true)
            .key("subject")
    );
    disabled_widget_case!(slider, Slider::new(0.5).disabled(true).key("subject"));
    disabled_widget_case!(
        tabs,
        Tabs::new()
            .tab("one")
            .tab("two")
            .focusable(true)
            .disabled(true)
            .key("subject")
    );
    disabled_widget_case!(
        text_area,
        TextArea::new("subject".to_string())
            .disabled(true)
            .key("subject")
    );
    disabled_widget_case!(
        table,
        Table::new()
            .rows([TableRow::new(["one"]), TableRow::new(["two"])])
            .disabled(true)
            .key("subject")
    );
    disabled_widget_case!(
        hex_area,
        HexArea::new(vec![0u8, 1, 2, 3])
            .disabled(true)
            .key("subject")
    );
    disabled_widget_case!(
        draggable_tab_bar,
        DraggableTabBar::new()
            .tab(DraggableTab::new("one"))
            .tab(DraggableTab::new("two"))
            .focusable(true)
            .disabled(true)
            .key("subject")
    );
}
