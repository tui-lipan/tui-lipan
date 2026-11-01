use std::{cell::RefCell, rc::Rc};

use tui_lipan::{TestBackend, prelude::*};

#[derive(Clone)]
struct Form {
    name: &'static str,
    handled: bool,
    events: Rc<RefCell<Vec<String>>>,
}

impl Component for Form {
    type Properties = ();
    type State = String;
    type Message = String;

    fn create_state(&self, _: &()) -> String {
        String::new()
    }

    fn update(&mut self, text: String, ctx: &mut Context<Self>) -> Update {
        ctx.state = text;
        Update::layout()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        if !ctx.state.is_empty() {
            return Input::new(ctx.state.clone())
                .on_change(
                    ctx.link()
                        .callback(|event: InputEvent| event.value.to_string()),
                )
                .key("custom");
        }
        VStack::new()
            .child(Button::new(self.name).key(self.name))
            .child(
                Input::new("")
                    .on_change(
                        ctx.link()
                            .callback(|event: InputEvent| event.value.to_string()),
                    )
                    .key(format!("{}-input", self.name)),
            )
            .into()
    }

    fn on_paste(&mut self, text: &str, origin: PasteOrigin, ctx: &mut Context<Self>) -> KeyUpdate {
        assert_eq!(origin, PasteOrigin::Terminal);
        assert!(ctx.has_focus_within_key(self.name));
        self.events
            .borrow_mut()
            .push(format!("{}: {text}", self.name));
        if self.handled {
            ctx.state = text.to_owned();
            ctx.request_focus("custom");
            KeyUpdate::handled(Update::layout())
        } else {
            KeyUpdate::unhandled(Update::none())
        }
    }
}

struct Root {
    events: Rc<RefCell<Vec<String>>>,
    handled: bool,
}

impl Component for Root {
    type Properties = ();
    type State = ();
    type Message = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let first = Form {
            name: "first",
            handled: self.handled,
            events: self.events.clone(),
        };
        let sibling = Form {
            name: "sibling",
            handled: true,
            events: self.events.clone(),
        };
        VStack::new()
            .child(child(move || first.clone(), ()).key("first-scope"))
            .child(child(move || sibling.clone(), ()).key("sibling-scope"))
            .into()
    }
    fn on_paste(&mut self, text: &str, _: PasteOrigin, _: &mut Context<Self>) -> KeyUpdate {
        self.events.borrow_mut().push(format!("root: {text}"));
        KeyUpdate::handled(Update::none())
    }
}

#[test]
fn focused_form_opens_editor_with_paste_and_preserves_editor_pasting() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(Root {
        events: events.clone(),
        handled: true,
    });
    assert!(backend.focus_key(&Key::from("first")));
    assert!(backend.send_paste("custom answer").unwrap());
    assert_eq!(&*events.borrow(), &["first: custom answer"]);
    assert_eq!(backend.focused_key(), Some(&Key::from("custom")));
    assert!(
        backend
            .capture_frame()
            .plain_text()
            .contains("custom answer")
    );
    assert!(backend.send_paste(" more").unwrap());
    assert_eq!(events.borrow().len(), 1);
    assert!(backend.capture_frame().plain_text().contains("more"));
}

#[test]
fn unhandled_paste_bubbles_to_parent_without_visiting_sibling() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(Root {
        events: events.clone(),
        handled: false,
    });
    assert!(backend.focus_key(&Key::from("first")));
    assert!(backend.send_paste("hello\nworld").unwrap());
    assert_eq!(
        &*events.borrow(),
        &["first: hello\nworld", "root: hello\nworld"]
    );
}

#[test]
fn editable_child_gets_paste_before_component_hook() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(Root {
        events: events.clone(),
        handled: true,
    });
    assert!(backend.focus_key(&Key::from("first-input")));
    assert!(backend.send_paste("editor text").unwrap());
    assert!(events.borrow().is_empty());
    assert!(backend.capture_frame().plain_text().contains("editor text"));
}
