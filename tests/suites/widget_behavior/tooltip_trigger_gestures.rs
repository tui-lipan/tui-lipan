use tui_lipan::{
    TestBackend,
    core::event::{MouseButton, MouseKind},
    prelude::*,
};

struct TriggerApp {
    tooltip: bool,
    draggable: bool,
}
#[derive(Default)]
struct State {
    hovered: bool,
    clicks: usize,
    popup_clicks: usize,
}
enum Msg {
    Hover(bool),
    Click,
    PopupClick,
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
        if self.draggable {
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
        }
    }
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
                let mut backend = TestBackend::new(TriggerApp { tooltip, draggable });
                backend.set_viewport(Rect {
                    x: 0,
                    y: 0,
                    w: 80,
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
                    assert!(backend.capture_frame().plain_text().contains("Details"));
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
        MouseRegion::new()
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
            .into()
    }
}

#[test]
fn passive_popover_space_passes_through_but_interactive_content_still_activates() {
    for capture in [false, true] {
        for interactive in [false, true] {
            let mut backend = TestBackend::new(PopupApp {
                capture,
                interactive,
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
