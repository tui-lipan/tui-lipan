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

/// A drop target offset from the left edge, fed by a source whose drag
/// activates away from the press.
struct OffsetApp {
    starts: std::sync::Arc<std::sync::Mutex<Vec<DragStartEvent>>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Local {
    local_x: u16,
    local_y: u16,
    local_width: u16,
    local_height: u16,
}

impl Component for OffsetApp {
    type Message = Local;
    type Properties = ();
    type State = Vec<Local>;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        Vec::new()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state.push(msg);
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let starts = self.starts.clone();
        let source = DragSource::new()
            .child(Text::new("item"))
            .threshold(3)
            .on_drag_start(move |ev| {
                starts.lock().unwrap().push(ev);
                Some(Box::new(ItemPayload { id: 1 }) as Box<dyn DragPayload>)
            });
        let target = DropTarget::new()
            .child(Text::new("t").width(Length::Px(10)).height(Length::Px(2)))
            .on_drag_over(ctx.link().callback(|ev: DragOverEvent| Local {
                local_x: ev.local_x,
                local_y: ev.local_y,
                local_width: ev.local_width,
                local_height: ev.local_height,
            }))
            .on_drop(ctx.link().callback(|ev: DropEvent| Local {
                local_x: ev.local_x,
                local_y: ev.local_y,
                local_width: ev.local_width,
                local_height: ev.local_height,
            }));
        VStack::new()
            .child(
                HStack::new()
                    .height(Length::Px(1))
                    .child(Text::new("").width(Length::Px(2)))
                    .child(source),
            )
            .child(
                HStack::new()
                    .align(Align::Start)
                    .child(Text::new("pad").width(Length::Px(5)))
                    .child(target),
            )
            .into()
    }
}

#[test]
fn drag_events_report_press_origin_and_target_local_x() {
    let starts = std::sync::Arc::default();
    let mut b = TestBackend::new(OffsetApp {
        starts: std::sync::Arc::clone(&starts),
    });
    b.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    });
    b.render();

    // The source starts at column 2; the target spans columns 5..15 and rows 1..3.
    let down = MouseKind::Down(MouseButton::Left);
    let drag = MouseKind::Drag(MouseButton::Left);
    mouse_offset(&mut b, down, 3, 0);
    mouse_offset(&mut b, drag, 8, 2);
    mouse_offset(&mut b, drag, 9, 2);
    mouse_offset(&mut b, MouseKind::Up(MouseButton::Left), 14, 1);

    assert_eq!(
        *starts.lock().unwrap(),
        vec![DragStartEvent {
            x: 8,
            y: 2,
            from_x: 3,
            from_y: 0,
            from_local_x: 1,
            from_local_y: 0,
        }]
    );
    let over = |local_x, local_y| Local {
        local_x,
        local_y,
        local_width: 10,
        local_height: 2,
    };
    assert_eq!(b.state().last(), Some(&over(9, 0)), "drop");
    assert!(b.state().contains(&over(4, 1)), "drag over");
}

fn mouse_offset(backend: &mut TestBackend<OffsetApp>, kind: MouseKind, x: u16, y: u16) {
    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind,
            mods: KeyMods::NONE,
        })
        .unwrap();
}
