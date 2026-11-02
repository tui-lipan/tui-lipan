use tui_lipan::{
    TestBackend,
    core::event::{MouseButton, MouseKind},
    prelude::*,
};

struct TriggerApp {
    tooltip: bool,
    draggable: bool,
    lower: Option<bool>,
}
#[derive(Default)]
struct State {
    hovered: bool,
    clicks: usize,
    popup_clicks: usize,
    closed: bool,
}
enum Msg {
    Hover(bool),
    Click,
    PopupClick,
    Close,
}
impl Component for TriggerApp {
    type Properties = ();
    type State = State;
    type Message = Msg;
    fn create_state(&self, _: &()) -> State {
        State::default()
    }
    fn update(&mut self, message: Msg, ctx: &mut Context<Self>) -> Update {
        match message {
            Msg::Hover(hovered) => ctx.state.hovered = hovered,
            Msg::Click => ctx.state.clicks += 1,
            Msg::PopupClick => ctx.state.popup_clicks += 1,
            Msg::Close => ctx.state.closed = true,
        }
        Update::layout()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let body: Element = Frame::new()
            .border(false)
            .width(Length::Px(20))
            .height(Length::Px(2))
            .child(Text::new("Trigger"))
            .into();
        let body = if self.tooltip {
            Tooltip::new("Details")
                .auto(false)
                .open(ctx.state.hovered)
                .placement(PopoverPlacement::RightStart)
                .child(body)
                .into()
        } else {
            body
        };
        let region: Element = MouseRegion::new()
            .on_hover_change(ctx.link().callback(Msg::Hover))
            .on_click(ctx.link().callback(|_| Msg::Click))
            .child(body)
            .key("region");
        let region = if self.draggable {
            DropTarget::new()
                .accept_group("rows")
                .child(
                    DragSource::new()
                        .drag_group("rows")
                        .on_drag_start(|_| Some(Box::new(1usize)))
                        .child(region),
                )
                .into()
        } else {
            region
        };
        with_lower_popover(region, self.lower, ctx)
    }
}

fn with_lower_popover<C: Component<Message = Msg, State = State>>(
    trigger: Element,
    dismiss: Option<bool>,
    ctx: &Context<C>,
) -> Element {
    let Some(dismiss) = dismiss else {
        return trigger;
    };
    let mut popover = Popover::new()
        .trigger(trigger)
        .content(Text::new("Lower").key("lower"))
        .anchor(Some((10, 10)))
        .min_trigger_width(false)
        .open(!ctx.state.closed);
    if dismiss {
        popover = popover.on_close(ctx.link().callback(|_| Msg::Close));
    }
    popover.into()
}

fn assert_details_covers_click<C: Component>(backend: &TestBackend<C>) {
    let snapshot = backend.capture_ui_snapshot();
    let details = snapshot
        .widgets
        .iter()
        .find(|widget| widget.label.as_deref() == Some("Details"))
        .expect("tooltip text must be present");
    assert!(
        details.rect.contains(3, 0),
        "tooltip must cover the click: {:?}",
        details.rect
    );
}

fn mouse(kind: MouseKind) -> MouseEvent {
    MouseEvent {
        x: 3,
        y: 0,
        kind,
        mods: KeyMods::NONE,
    }
}
#[test]
fn tooltip_trigger_click_survives_hover_layout_with_and_without_drag_source() {
    for tooltip in [false, true] {
        for draggable in [false, true] {
            for hover_first in [false, true] {
                let mut backend = TestBackend::new(TriggerApp {
                    tooltip,
                    draggable,
                    lower: None,
                });
                backend.set_viewport(Rect {
                    x: 0,
                    y: 0,
                    w: 20,
                    h: 20,
                });
                backend.render();
                if hover_first {
                    backend.send_mouse(mouse(MouseKind::Moved)).unwrap();
                }
                backend
                    .send_mouse(mouse(MouseKind::Down(MouseButton::Left)))
                    .unwrap();
                if tooltip {
                    assert!(backend.state().hovered);
                    assert_details_covers_click(&backend);
                }
                backend
                    .send_mouse(mouse(MouseKind::Up(MouseButton::Left)))
                    .unwrap();
                assert_eq!(
                    backend.state().clicks,
                    1,
                    "tooltip={tooltip}, draggable={draggable}, hover_first={hover_first}, hovered={}, region={:?}\n{}",
                    backend.state().hovered,
                    backend.rect_of_key(&"region".into()),
                    backend.capture_frame().plain_text()
                );
            }
        }
    }
}

struct PopupApp {
    capture: bool,
    interactive: bool,
    lower: Option<bool>,
}
impl Component for PopupApp {
    type Properties = ();
    type State = State;
    type Message = Msg;
    fn create_state(&self, _: &()) -> State {
        State::default()
    }
    fn update(&mut self, message: Msg, ctx: &mut Context<Self>) -> Update {
        match message {
            Msg::Click => ctx.state.clicks += 1,
            Msg::PopupClick => ctx.state.popup_clicks += 1,
            Msg::Close => ctx.state.closed = true,
            Msg::Hover(_) => {}
        }
        Update::none()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let content: Element = if self.interactive {
            Button::new("Popup")
                .on_click(ctx.link().callback(|_| Msg::PopupClick))
                .into()
        } else {
            Text::new("Popup").into()
        };
        let region = MouseRegion::new()
            .on_click(ctx.link().callback(|_| Msg::Click))
            .child(
                Popover::new()
                    .trigger(Frame::new().border(false).child(Text::new("Trigger")))
                    .content(content)
                    .open(true)
                    .capture_focus(false)
                    .capture_pointer(self.capture)
                    .placement(PopoverPlacement::RightStart)
                    .min_trigger_width(false),
            )
            .into();
        with_lower_popover(region, self.lower, ctx)
    }
}

#[test]
fn passive_popover_space_passes_through_but_interactive_content_still_activates() {
    for capture in [false, true] {
        for interactive in [false, true] {
            let mut backend = TestBackend::new(PopupApp {
                capture,
                interactive,
                lower: None,
            });
            backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 20,
            });
            backend.render();
            backend
                .send_mouse(mouse(MouseKind::Down(MouseButton::Left)))
                .unwrap();
            backend
                .send_mouse(mouse(MouseKind::Up(MouseButton::Left)))
                .unwrap();
            assert_eq!(
                backend.state().clicks,
                usize::from(!capture && !interactive),
                "capture={capture}, interactive={interactive}"
            );
            assert_eq!(
                backend.state().popup_clicks,
                usize::from(interactive),
                "capture={capture}, interactive={interactive}"
            );
        }
    }
}

#[test]
fn tooltip_preserves_lower_popover_capture_and_outside_dismissal() {
    for dismiss in [false, true] {
        let mut backend = TestBackend::new(TriggerApp {
            tooltip: true,
            draggable: false,
            lower: Some(dismiss),
        });
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 20,
            h: 20,
        });
        backend.render();
        backend.send_mouse(mouse(MouseKind::Moved)).unwrap();
        assert_details_covers_click(&backend);
        assert!(!backend.rect_of_key(&"lower".into()).unwrap().contains(3, 0));
        backend
            .send_mouse(mouse(MouseKind::Down(MouseButton::Left)))
            .unwrap();
        backend
            .send_mouse(mouse(MouseKind::Up(MouseButton::Left)))
            .unwrap();
        assert_eq!(backend.state().closed, dismiss);
        assert_eq!(backend.state().clicks, 0, "dismiss={dismiss}");
    }
}

#[test]
fn interactive_upper_popover_receives_click_above_capturing_popover() {
    for dismiss in [false, true] {
        let mut backend = TestBackend::new(PopupApp {
            capture: false,
            interactive: true,
            lower: Some(dismiss),
        });
        backend.render();
        assert!(!backend.rect_of_key(&"lower".into()).unwrap().contains(3, 0));
        backend
            .send_mouse(mouse(MouseKind::Down(MouseButton::Left)))
            .unwrap();
        backend
            .send_mouse(mouse(MouseKind::Up(MouseButton::Left)))
            .unwrap();
        assert_eq!(backend.state().popup_clicks, 1);
        assert!(!backend.state().closed);
        assert_eq!(backend.state().clicks, 0);
    }
}
