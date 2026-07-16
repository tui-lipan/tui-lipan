//! End-to-end coverage of the generic `DragSource`/`DropTarget` pipeline
//! through `TestBackend`: activation threshold, drag-over events, drop,
//! group compatibility, and cancel on release outside a target.

use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseKind};
use tui_lipan::prelude::*;
use tui_lipan::style::Rect;

#[derive(Clone, Debug)]
struct ItemPayload {
    id: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Ev {
    Over {
        id: u32,
        local_y: u16,
        local_height: u16,
    },
    Leave,
    Drop {
        id: u32,
        local_y: u16,
    },
    Cancel {
        id: u32,
    },
}

struct DndApp;

#[derive(Default)]
struct State {
    events: Vec<Ev>,
}

impl Component for DndApp {
    type Message = Ev;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.events.push(msg);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        // Row 0: the drag source. Rows 1-3: compatible target. Rows 4-6:
        // incompatible target (different accept group).
        let source = DragSource::new()
            .child(Text::new("item"))
            .drag_group("g")
            .threshold(1)
            .on_drag_start(|_| Some(Box::new(ItemPayload { id: 7 }) as Box<dyn DragPayload>))
            .on_drag_cancel(ctx.link().callback(|ev: DragCancelEvent| {
                let p = ev.payload.downcast_ref::<ItemPayload>().unwrap();
                Ev::Cancel { id: p.id }
            }));

        let accepting = DropTarget::new()
            .child(Text::new("target-a").height(Length::Px(3)))
            .accept_group("g")
            .on_drag_over(ctx.link().callback(|ev: DragOverEvent| {
                let p = ev.payload.downcast_ref::<ItemPayload>().unwrap();
                Ev::Over {
                    id: p.id,
                    local_y: ev.local_y,
                    local_height: ev.local_height,
                }
            }))
            .on_drag_leave(ctx.link().callback(|_| Ev::Leave))
            .on_drop(ctx.link().callback(|ev: DropEvent| {
                let p = ev.payload.downcast_ref::<ItemPayload>().unwrap();
                Ev::Drop {
                    id: p.id,
                    local_y: ev.local_y,
                }
            }));

        let incompatible = DropTarget::new()
            .child(Text::new("target-b").height(Length::Px(3)))
            .accept_group("other")
            .on_drag_over(ctx.link().callback(|ev: DragOverEvent| {
                let p = ev.payload.downcast_ref::<ItemPayload>().unwrap();
                Ev::Over {
                    id: p.id,
                    local_y: ev.local_y,
                    local_height: ev.local_height,
                }
            }));

        VStack::new()
            .gap(0)
            .child(source)
            .child(accepting)
            .child(incompatible)
            .into()
    }
}

fn backend() -> TestBackend<DndApp> {
    let mut backend = TestBackend::new(DndApp);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    });
    backend.render();
    backend
}

fn mouse(backend: &mut TestBackend<DndApp>, kind: MouseKind, x: u16, y: u16) {
    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind,
            mods: KeyMods::NONE,
        })
        .unwrap();
}

#[test]
fn drag_below_threshold_does_not_activate() {
    let mut b = backend();
    mouse(&mut b, MouseKind::Down(MouseButton::Left), 2, 0);
    mouse(&mut b, MouseKind::Drag(MouseButton::Left), 2, 0);
    mouse(&mut b, MouseKind::Up(MouseButton::Left), 2, 0);
    assert!(b.state().events.is_empty(), "no drag events expected");
}

#[test]
fn drag_over_compatible_target_emits_over_then_drop() {
    let mut b = backend();
    mouse(&mut b, MouseKind::Down(MouseButton::Left), 2, 0);
    // Move onto row 2 of the accepting target (its rect spans rows 1..4).
    mouse(&mut b, MouseKind::Drag(MouseButton::Left), 2, 2);
    assert_eq!(
        b.state().events,
        vec![Ev::Over {
            id: 7,
            local_y: 1,
            local_height: 3
        }]
    );
    mouse(&mut b, MouseKind::Up(MouseButton::Left), 2, 3);
    assert_eq!(
        b.state().events[1..],
        [
            Ev::Drop { id: 7, local_y: 2 },
            // Production emits leave after drop; the mirror matches it.
            Ev::Leave,
        ]
    );
}

#[test]
fn incompatible_group_gets_no_events_and_release_cancels() {
    let mut b = backend();
    mouse(&mut b, MouseKind::Down(MouseButton::Left), 2, 0);
    // Rows 4..7 belong to the incompatible target.
    mouse(&mut b, MouseKind::Drag(MouseButton::Left), 2, 5);
    assert!(
        b.state().events.is_empty(),
        "incompatible target must not receive drag-over"
    );
    mouse(&mut b, MouseKind::Up(MouseButton::Left), 2, 5);
    assert_eq!(b.state().events, vec![Ev::Cancel { id: 7 }]);
}

#[test]
fn leaving_target_emits_leave() {
    let mut b = backend();
    mouse(&mut b, MouseKind::Down(MouseButton::Left), 2, 0);
    mouse(&mut b, MouseKind::Drag(MouseButton::Left), 2, 2);
    mouse(&mut b, MouseKind::Drag(MouseButton::Left), 2, 5);
    assert_eq!(
        b.state().events,
        vec![
            Ev::Over {
                id: 7,
                local_y: 1,
                local_height: 3
            },
            Ev::Leave,
        ]
    );
    mouse(&mut b, MouseKind::Up(MouseButton::Left), 2, 5);
    assert_eq!(b.state().events.last(), Some(&Ev::Cancel { id: 7 }));
}
