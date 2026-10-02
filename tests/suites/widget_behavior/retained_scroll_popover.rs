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
                                        Input::new("nested-only").border(false).key("nested-input"),
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
