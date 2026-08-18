//! The hardware caret belongs to the widget on top: a focused widget covered by a later layer must
//! not keep blinking through it.

use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

fn editor() -> Element {
    Input::new("hi").border(false).key("editor")
}

struct FocusedEditor;

impl Component for FocusedEditor {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        ZStack::new().child(editor()).into()
    }
}

struct FocusedEditorUnderCover;

impl Component for FocusedEditorUnderCover {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        ZStack::new()
            .child(editor())
            .child(Input::new("cover").border(false).key("cover"))
            .into()
    }
}

fn viewport() -> Rect {
    Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 1,
    }
}

#[test]
fn focused_widget_places_the_caret_when_nothing_covers_it() {
    let mut backend = TestBackend::new(FocusedEditor);
    backend.set_viewport(viewport());
    backend.render();
    assert!(backend.focus_key(&Key::from("editor")), "editor focusable");
    backend.render();

    assert!(
        backend.capture_frame().cursor.is_some(),
        "the focused input owns the caret"
    );
}

#[test]
fn a_layer_drawn_over_the_focused_widget_withholds_the_caret() {
    let mut backend = TestBackend::new(FocusedEditorUnderCover);
    backend.set_viewport(viewport());
    backend.render();
    assert!(backend.focus_key(&Key::from("editor")), "editor focusable");
    backend.render();

    assert!(
        backend.capture_frame().cursor.is_none(),
        "the later layer is painted over the caret cell, so the caret is withheld"
    );
}
