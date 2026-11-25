use tui_lipan::{TestBackend, prelude::*};

const IDLE: Color = Color::Rgb(0x2a, 0x2a, 0x2a);
const ACTIVE: Color = Color::Rgb(0x34, 0x6b, 0xf1);

/// A button whose fill depends on state, inside a tooltip, as a send button that turns active.
struct SendButton;

impl Component for SendButton {
    type Properties = ();
    type State = bool;
    type Message = bool;
    fn create_state(&self, _: &()) -> bool {
        false
    }
    fn update(&mut self, active: bool, ctx: &mut Context<Self>) -> Update {
        ctx.state = active;
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        let fill = if ctx.state { ACTIVE } else { IDLE };
        Tooltip::new("Send message")
            .child(
                Button::filled("Go")
                    .padding(0)
                    .style(Style::new().fg(Color::White).bg(fill)),
            )
            .into()
    }
}

#[test]
fn tooltip_child_takes_a_style_only_change() {
    let mut backend = TestBackend::new(SendButton);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 10,
        h: 3,
    });
    backend.render();
    assert_eq!(backend.capture_frame().cell(0, 0).bg, IDLE);

    // Same label and size, so the layout is unchanged; only the fill differs.
    backend.dispatch(true).unwrap();
    backend.render();
    assert_eq!(backend.capture_frame().cell(0, 0).bg, ACTIVE);
}
