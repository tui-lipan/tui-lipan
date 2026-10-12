//! A modal playing its exit animation has already let go of focus. A focus request made in the
//! update that closed it must not be undone when the animation ends and the focus the modal saved
//! on opening is restored.

use std::time::Duration;

use tui_lipan::prelude::*;
use tui_lipan::{TestBackend, VisibilityAnimation};

#[derive(Default)]
struct State {
    open: bool,
}

#[derive(Clone, Copy)]
enum Msg {
    Open,
    Close,
    CloseAndFocusAfter,
}

struct Host;

impl Component for Host {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> State {
        State::default()
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Open => ctx.state.open = true,
            Msg::Close => ctx.state.open = false,
            Msg::CloseAndFocusAfter => {
                ctx.state.open = false;
                ctx.request_focus("after");
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let buttons = VStack::new()
            .child(Button::new("before").key("before"))
            .child(Button::new("after").key("after"));
        let mut root = ZStack::new().child(buttons);
        if ctx.state.open {
            let modal: Element = Modal::new()
                .width(Length::Px(20))
                .height(Length::Px(5))
                .animation(
                    VisibilityAnimation::new()
                        .enter(timing(0))
                        .exit(timing(200)),
                )
                .child(Button::new("inside").key("inside"))
                .into();
            root = root.child(modal.key("modal"));
        }
        root.into()
    }
}

fn timing(ms: u64) -> TransitionConfig {
    TransitionConfig {
        duration: Duration::from_millis(ms),
        easing: Easing::Linear,
    }
}

/// A modal opened over the focused `before` button, which it took focus from.
fn opened() -> TestBackend<Host> {
    let mut backend = TestBackend::new(Host);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 12,
    });
    backend.render();
    assert!(backend.focus_key(&Key::from("before")));
    backend.dispatch(Msg::Open).expect("open");
    backend.render();
    assert_eq!(backend.focused_key(), Some(&Key::from("inside")));
    backend
}

#[test]
fn a_focus_request_made_while_closing_outlasts_the_exit_animation() {
    let mut backend = opened();

    backend
        .dispatch(Msg::CloseAndFocusAfter)
        .expect("close and focus");
    backend.render();
    assert_eq!(backend.focused_key(), Some(&Key::from("after")));

    backend.advance(Duration::from_millis(250));
    backend.render();
    assert_eq!(
        backend.focused_key(),
        Some(&Key::from("after")),
        "the saved focus must not come back once the modal is gone"
    );
}

#[test]
fn closing_without_a_request_still_returns_to_the_saved_focus() {
    let mut backend = opened();

    backend.dispatch(Msg::Close).expect("close");
    backend.render();
    assert_eq!(backend.focused_key(), Some(&Key::from("before")));

    backend.advance(Duration::from_millis(250));
    backend.render();
    assert_eq!(backend.focused_key(), Some(&Key::from("before")));
}
