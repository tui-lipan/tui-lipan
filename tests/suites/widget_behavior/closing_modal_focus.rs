//! A modal playing its exit animation has already let go of focus. Focus moved on purpose before
//! the animation ends, by the app or by the user, must not be undone when it ends and the focus the
//! modal saved on opening is restored.

use std::time::Duration;

use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
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
    Blur,
    FocusNext,
    FocusPrev,
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
            Msg::Blur => ctx.blur(),
            Msg::FocusNext => ctx.focus_next(),
            Msg::FocusPrev => ctx.focus_prev(),
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

/// The modal from [`opened`], closed and still playing its exit animation.
fn closing() -> TestBackend<Host> {
    let mut backend = opened();
    backend.dispatch(Msg::Close).expect("close");
    backend.render();
    assert_eq!(backend.focused_key(), Some(&Key::from("before")));
    backend
}

/// Let the exit animation finish, which is when the modal's saved focus would be restored.
fn finish(backend: &mut TestBackend<Host>) {
    backend.advance(Duration::from_millis(250));
    backend.render();
}

#[test]
fn a_focus_request_made_while_closing_outlasts_the_exit_animation() {
    let mut backend = opened();

    backend
        .dispatch(Msg::CloseAndFocusAfter)
        .expect("close and focus");
    backend.render();
    assert_eq!(backend.focused_key(), Some(&Key::from("after")));

    finish(&mut backend);
    assert_eq!(
        backend.focused_key(),
        Some(&Key::from("after")),
        "the saved focus must not come back once the modal is gone"
    );
}

#[test]
fn closing_without_a_request_still_returns_to_the_saved_focus() {
    let mut backend = closing();

    finish(&mut backend);
    assert_eq!(backend.focused_key(), Some(&Key::from("before")));
}

#[test]
fn traversal_requested_during_the_exit_animation_outlasts_it() {
    for msg in [Msg::FocusNext, Msg::FocusPrev] {
        let mut backend = closing();

        backend.dispatch(msg).expect("traverse");
        backend.render();
        assert_eq!(backend.focused_key(), Some(&Key::from("after")));

        finish(&mut backend);
        assert_eq!(backend.focused_key(), Some(&Key::from("after")));
    }
}

#[test]
fn a_blur_requested_during_the_exit_animation_outlasts_it() {
    let mut backend = closing();

    backend.dispatch(Msg::Blur).expect("blur");
    backend.render();
    assert_eq!(backend.focused(), None);

    finish(&mut backend);
    assert_eq!(backend.focused(), None);
    assert_eq!(backend.focused_key(), None);
}

#[test]
fn tab_pressed_during_the_exit_animation_outlasts_it() {
    let mut backend = closing();

    backend
        .send_key(KeyEvent {
            code: KeyCode::Tab,
            mods: KeyMods::NONE,
        })
        .expect("tab");
    backend.render();
    assert_eq!(backend.focused_key(), Some(&Key::from("after")));

    finish(&mut backend);
    assert_eq!(backend.focused_key(), Some(&Key::from("after")));
}

#[test]
fn a_click_during_the_exit_animation_outlasts_it() {
    let mut backend = closing();
    let after = backend
        .rect_of_key(&Key::from("after"))
        .expect("after laid out");

    backend
        .send_mouse(MouseEvent {
            x: u16::try_from(after.x).expect("on screen"),
            y: u16::try_from(after.y).expect("on screen"),
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::NONE,
        })
        .expect("click");
    backend.render();
    assert_eq!(backend.focused_key(), Some(&Key::from("after")));

    finish(&mut backend);
    assert_eq!(backend.focused_key(), Some(&Key::from("after")));
}
