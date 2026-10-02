use tui_lipan::TestBackend;
use tui_lipan::prelude::*;

struct Form {
    trigger_height: u16,
}

#[derive(Default)]
struct State {
    offset: usize,
    open: bool,
}

enum Msg {
    Open,
    Close,
    Scroll(usize),
}

impl Component for Form {
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
            Msg::Scroll(offset) => ctx.state.offset = offset,
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let popover = Popover::new()
            .open(ctx.state.open)
            .on_close(ctx.link().callback(|_| Msg::Close))
            .trigger(
                Button::new("open")
                    .height(Length::Px(self.trigger_height))
                    .on_click(ctx.link().callback(|_| Msg::Open))
                    .key("trigger"),
            )
            .content(Input::new("portal-only").border(false).key("popup-input"));
        let rows = [
            Text::new("row 0").into(),
            Text::new("row 1").into(),
            popover.into(),
        ]
        .into_iter()
        .chain((3..10).map(|i| Text::new(format!("row {i}")).into()));
        VStack::new()
            .child(
                ScrollView::new()
                    .virtualize(false)
                    .height(Length::Px(4))
                    .offset(ctx.state.offset)
                    .children(rows),
            )
            .child(Button::new("background").key("background"))
            .into()
    }
}

#[test]
fn offscreen_retained_popover_is_suppressed_without_closing_or_remounting() {
    let mut backend = TestBackend::new_with_viewport(
        Form { trigger_height: 1 },
        Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 8,
        },
    );
    assert!(backend.focus_key(&Key::from("trigger")));
    let trigger = backend.focused();
    backend.dispatch(Msg::Open).unwrap();
    assert_eq!(backend.focused_key(), Some(&Key::from("popup-input")));
    let input = backend.focused();
    assert!(backend.capture_frame().plain_text().contains("portal-only"));

    backend.dispatch(Msg::Scroll(5)).unwrap();
    assert!(backend.state().open);
    assert!(!backend.capture_frame().plain_text().contains("portal-only"));
    assert!(!backend.focus_key(&Key::from("popup-input")));
    assert!(backend.focus_key(&Key::from("background")));
    backend.render();
    assert_eq!(backend.focused_key(), Some(&Key::from("background")));
    backend.focus_next();
    assert_ne!(backend.focused_key(), Some(&Key::from("popup-input")));
    assert!(backend.focus_key(&Key::from("trigger")));
    assert_eq!(backend.focused(), trigger);

    backend.dispatch(Msg::Scroll(0)).unwrap();
    assert!(backend.state().open);
    assert!(backend.capture_frame().plain_text().contains("portal-only"));
    assert!(backend.focus_key(&Key::from("popup-input")));
    assert_eq!(backend.focused(), input);
}

#[test]
fn partially_clipped_trigger_keeps_its_root_popover_visible() {
    let mut backend = TestBackend::new_with_viewport(
        Form { trigger_height: 3 },
        Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 8,
        },
    );
    backend.dispatch(Msg::Open).unwrap();
    assert!(backend.capture_frame().plain_text().contains("portal-only"));
    assert!(backend.focus_key(&Key::from("popup-input")));
}

struct NestedPopover;

impl Component for NestedPopover {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _: &()) {}

    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _: &Context<Self>) -> Element {
        VStack::new()
            .child(
                ScrollView::new()
                    .virtualize(false)
                    .height(Length::Px(3))
                    .child(Text::new("row 0"))
                    .child(Text::new("row 1"))
                    .child(
                        Popover::new()
                            .open(true)
                            .trigger(Text::new("outer trigger"))
                            .content(
                                VStack::new()
                                    .height(Length::Px(3))
                                    .child(Text::new("portal row"))
                                    .child(
                                        Popover::new()
                                            .open(true)
                                            .trigger(Text::new("nested trigger"))
                                            .content(
                                                Input::new("nested-only")
                                                    .border(false)
                                                    .key("nested-input"),
                                            ),
                                    ),
                            ),
                    ),
            )
            .into()
    }
}

#[test]
fn nested_portal_content_does_not_inherit_the_triggers_scroll_clip() {
    let mut backend = TestBackend::new_with_viewport(
        NestedPopover,
        Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 10,
        },
    );
    assert!(backend.capture_frame().plain_text().contains("nested-only"));
    assert!(backend.focus_key(&Key::from("nested-input")));
}

struct PopoverInsideModalPortal;

impl Component for PopoverInsideModalPortal {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _: &()) {}

    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _: &Context<Self>) -> Element {
        // A root Modal creates a generic Portal, not a Popover boundary.
        let modal = Modal::new()
            .border(false)
            .padding(0)
            .width(Length::Px(30))
            .height(Length::Px(3))
            .child(
                VStack::new().child(Text::new("modal row")).child(
                    Popover::new()
                        .open(true)
                        .trigger(Text::new("modal popover trigger").key("modal-trigger"))
                        .content(
                            Input::new("modal-popup-only")
                                .border(false)
                                .key("modal-input"),
                        ),
                ),
            );
        VStack::new()
            .child(
                ScrollView::new()
                    .virtualize(false)
                    .height(Length::Px(3))
                    .child(Text::new("row 0"))
                    .child(Text::new("row 1"))
                    .child(
                        Frame::new()
                            .height(Length::Px(1))
                            .border(false)
                            .padding(0)
                            .child(modal),
                    )
                    .key("declaration-scroll"),
            )
            .into()
    }
}

#[test]
fn generic_portal_content_does_not_inherit_the_declaration_scroll_clip() {
    let mut backend = TestBackend::new_with_viewport(
        PopoverInsideModalPortal,
        Rect {
            x: 0,
            y: 0,
            w: 30,
            h: 10,
        },
    );
    let snapshot = backend.capture_ui_snapshot();
    let scroll = snapshot
        .widgets
        .iter()
        .find(|widget| widget.key.as_ref() == Some(&Key::from("declaration-scroll")))
        .expect("declaration ScrollView is mounted");
    assert_eq!(scroll.rect.h, 3);
    let trigger = snapshot
        .widgets
        .iter()
        .find(|widget| widget.key.as_ref() == Some(&Key::from("modal-trigger")))
        .expect("trigger is retained inside the modal portal");
    assert!(
        trigger.rect.y >= 3,
        "trigger must be outside the ScrollView clip"
    );
    assert!(
        backend
            .capture_frame()
            .plain_text()
            .contains("modal-popup-only")
    );
    assert_eq!(backend.focused_key(), Some(&Key::from("modal-input")));
    assert!(backend.focus_key(&Key::from("modal-input")));
}
