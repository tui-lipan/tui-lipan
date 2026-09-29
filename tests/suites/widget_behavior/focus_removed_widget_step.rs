//! A focus step requested in the same update that removes the focused widget.
//!
//! A request still pending at render applies after reconciliation but before focus is
//! restored, so the step starts from a focused id whose node is gone. It must count as no
//! focus. `update_level` leaves the request pending, so the next `render` takes that path.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct Row;

#[derive(Clone, Copy)]
enum Msg {
    RemoveMiddleThenNext,
    RemoveMiddleThenPrev,
}

#[derive(Default)]
struct State {
    middle_removed: bool,
}

impl Component for Row {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.middle_removed = true;
        match msg {
            Msg::RemoveMiddleThenNext => ctx.focus_next(),
            Msg::RemoveMiddleThenPrev => ctx.focus_prev(),
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut stack = VStack::new().child(Button::new("first").key("first"));
        if !ctx.state.middle_removed {
            stack = stack.child(Button::new("middle").key("middle"));
        }
        stack.child(Button::new("last").key("last")).into()
    }
}

fn focused_on_middle() -> TestBackend<Row> {
    let mut backend = TestBackend::new(Row);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    backend.render();
    assert!(backend.focus_key(&Key::from("middle")));
    backend.render();
    backend
}

fn focused(backend: &TestBackend<Row>) -> Option<String> {
    backend.focused_key().map(|key| key.to_string())
}

#[test]
fn focus_next_after_removing_the_focused_widget_starts_the_ring() {
    let mut backend = focused_on_middle();

    backend.update_level(Msg::RemoveMiddleThenNext).unwrap();
    backend.render();

    assert_eq!(focused(&backend), Some("first".to_string()));
}

#[test]
fn focus_prev_after_removing_the_focused_widget_ends_the_ring() {
    let mut backend = focused_on_middle();

    backend.update_level(Msg::RemoveMiddleThenPrev).unwrap();
    backend.render();

    assert_eq!(focused(&backend), Some("last".to_string()));
}
