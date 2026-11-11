//! A drag from the host terminal reaches the root component with the keyed widgets under it, each
//! with the pointer in its own cells, and what the app sends back reaches the host only when the
//! host speaks the protocol.

use tui_lipan::TestBackend;
use tui_lipan::dnd::{
    DragPosition, DropOperations, HostEvent, ProgramCommand, TaggedEvent, TerminalEvent,
};
use tui_lipan::prelude::*;

#[derive(Default)]
struct State {
    seen: Vec<HostEvent>,
}

struct Host;

impl Component for Host {
    type Message = ();
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> State {
        State::default()
    }

    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn on_host_drag_and_drop(&mut self, event: HostEvent, ctx: &mut Context<Self>) -> Update {
        ctx.host_drag_and_drop()
            .send(&ProgramCommand::StopAcceptingDrops);
        ctx.state.seen.push(event);
        Update::none()
    }

    fn view(&self, _: &Context<Self>) -> Element {
        HStack::new()
            .child(Button::new("left").key("left"))
            .child(Button::new("right").key("right"))
            .key("row")
    }
}

fn over(x: u32, y: u32) -> TaggedEvent {
    TaggedEvent {
        client_id: None,
        event: TerminalEvent::DragOver {
            position: DragPosition {
                cell_x: x,
                cell_y: y,
                pixel_x: x as i32 * 10 + 4,
                pixel_y: y as i32 * 20 + 7,
            },
            operations: DropOperations::COPY,
            mimes: None,
        },
    }
}

#[test]
fn the_root_hears_the_widgets_under_a_drag_in_their_own_cells() {
    let mut backend = TestBackend::new(Host);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 6,
    });
    backend.render();
    let right = backend
        .rect_of_key(&Key::from("right"))
        .expect("right button");
    backend
        .host_drag_and_drop(over(right.x as u32 + 2, right.y as u32))
        .unwrap();
    let event = backend.state().seen.last().expect("an event").clone();
    let keys: Vec<_> = event.targets.iter().map(|t| t.key.clone()).collect();
    assert_eq!(keys.first(), Some(&Key::from("right")), "{keys:?}");
    assert!(
        keys.contains(&Key::from("row")),
        "keyed ancestors follow: {keys:?}"
    );
    let target = event.target(|key| key == &Key::from("right")).unwrap();
    assert_eq!((target.position.cell_x, target.position.cell_y), (2, 0));
    assert_eq!(
        target.position.pixel_x,
        2 * 10 + 4,
        "pixels from the widget's corner"
    );
    assert_eq!(target.rect, right);

    backend
        .host_drag_and_drop(TaggedEvent {
            client_id: None,
            event: TerminalEvent::DragLeave,
        })
        .unwrap();
    assert!(backend.state().seen.last().unwrap().targets.is_empty());
}

#[test]
fn commands_reach_only_a_host_that_speaks_the_protocol() {
    let mut backend = TestBackend::new(Host);
    backend.render();
    backend.host_drag_and_drop(over(0, 0)).unwrap();
    assert!(backend.take_host_drag_and_drop_output().is_empty());
    backend.set_host_drag_and_drop_supported(true);
    backend.host_drag_and_drop(over(0, 0)).unwrap();
    assert_eq!(
        backend.take_host_drag_and_drop_output(),
        b"\x1b]72;t=A\x1b\\"
    );
}
