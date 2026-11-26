//! End-to-end coverage of the generic `DragSource`/`DropTarget` pipeline
//! through `TestBackend`: activation threshold, drag-over events, drop,
//! group compatibility, and cancel on release outside a target.

use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseKind};
use tui_lipan::prelude::*;
use tui_lipan::style::Rect;
use tui_lipan::{DragCancelEvent, DragOverEvent, DragPreviewAnchor, DragStartEvent, ScrollAxis};

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

    let starts: Vec<_> = starts
        .lock()
        .unwrap()
        .iter()
        .map(|ev| {
            (
                ev.x,
                ev.y,
                ev.from_x,
                ev.from_y,
                ev.from_local_x,
                ev.from_local_y,
            )
        })
        .collect();
    assert_eq!(starts, vec![(8, 2, 3, 0, 1, 0)]);
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

/// A drop target scrolled partially out of a `ScrollView` on both axes, so its
/// rect origin is negative.
struct ScrolledTargetApp;

impl Component for ScrolledTargetApp {
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
        let source = DragSource::new()
            .child(Text::new("item"))
            .threshold(1)
            .on_drag_start(|_| Some(Box::new(ItemPayload { id: 1 }) as Box<dyn DragPayload>));
        let target = DropTarget::new()
            .child(Text::new("t").width(Length::Px(40)).height(Length::Px(8)))
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
        let content = VStack::new()
            .child(Text::new("row").height(Length::Px(3)))
            .child(
                HStack::new()
                    .align(Align::Start)
                    .child(Text::new("pad").width(Length::Px(4)))
                    .child(Element::from(target).key("target")),
            );
        VStack::new()
            .child(HStack::new().height(Length::Px(1)).child(source))
            .child(
                ScrollView::new()
                    .axis(ScrollAxis::Both)
                    .width(Length::Px(20))
                    .height(Length::Px(6))
                    .offset(5)
                    .reveal_horizontal_range(30, 44)
                    .child(content),
            )
            .into()
    }
}

#[test]
fn drop_target_local_coords_are_signed_when_scrolled_off_screen() {
    let mut b = TestBackend::new(ScrolledTargetApp);
    b.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    });
    b.render();
    let target = b.rect_of_key(&Key::from("target")).expect("target rect");
    assert!(
        target.x < 0 && target.y < 0,
        "target should start off-screen: {target:?}"
    );

    let (x, y) = (5u16, 3u16);
    let down = MouseKind::Down(MouseButton::Left);
    let drag = MouseKind::Drag(MouseButton::Left);
    send(&mut b, down, 0, 0);
    send(&mut b, drag, x, y);
    send(&mut b, MouseKind::Up(MouseButton::Left), x, y);

    let expected = Local {
        local_x: (i32::from(x) - i32::from(target.x)) as u16,
        local_y: (i32::from(y) - i32::from(target.y)) as u16,
        local_width: 40,
        local_height: 8,
    };
    assert_eq!(b.state().first(), Some(&expected), "drag over");
    assert_eq!(b.state().last(), Some(&expected), "drop");
}

/// A drag source whose left edge moves between the press and drag activation.
struct ShiftingSourceApp {
    starts: std::sync::Arc<std::sync::Mutex<Vec<DragStartEvent>>>,
}

impl Component for ShiftingSourceApp {
    type Message = u16;
    type Properties = ();
    type State = u16;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        2
    }

    fn update(&mut self, indent: u16, ctx: &mut Context<Self>) -> Update {
        ctx.state = indent;
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
        HStack::new()
            .height(Length::Px(1))
            .child(Text::new("").width(Length::Px(ctx.state)))
            .child(Element::from(source).key("source"))
            .into()
    }
}

#[test]
fn drag_start_local_origin_is_captured_at_press() {
    let starts = std::sync::Arc::default();
    let mut b = TestBackend::new(ShiftingSourceApp {
        starts: std::sync::Arc::clone(&starts),
    });
    b.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 1,
    });
    b.render();

    // Press two cells into the source, then move the source before the threshold is crossed.
    send(&mut b, MouseKind::Down(MouseButton::Left), 4, 0);
    b.dispatch(5).unwrap();
    b.render();
    assert_eq!(b.rect_of_key(&Key::from("source")).map(|r| r.x), Some(5));
    send(&mut b, MouseKind::Drag(MouseButton::Left), 8, 0);

    let starts = starts.lock().unwrap();
    assert_eq!(starts.len(), 1, "drag should activate once");
    assert_eq!((starts[0].from_x, starts[0].from_local_x), (4, 2));
}

fn send<C: Component>(backend: &mut TestBackend<C>, kind: MouseKind, x: u16, y: u16) {
    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind,
            mods: KeyMods::NONE,
        })
        .unwrap();
}

#[derive(Clone, PartialEq)]
enum ActivationThreshold {
    Same(u16),
    Axes(u16, u16),
}

struct ThresholdApp;

impl Component for ThresholdApp {
    type Properties = ActivationThreshold;
    type State = usize;
    type Message = ();

    fn create_state(&self, _: &Self::Properties) -> usize {
        0
    }

    fn update(&mut self, _: (), ctx: &mut Context<Self>) -> Update {
        ctx.state += 1;
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let source = DragSource::new()
            .child(
                Text::new("drag me")
                    .width(Length::Px(20))
                    .height(Length::Px(8)),
            )
            .on_drag_start(|_| Some(Box::new("item")))
            .on_drag_started(ctx.link().callback(|_| ()));
        match ctx.props {
            ActivationThreshold::Same(cells) => source.threshold(cells),
            ActivationThreshold::Axes(columns, rows) => source.drag_threshold(columns, rows),
        }
        .into()
    }
}

#[test]
fn separate_thresholds_start_after_one_row_but_still_require_three_columns() {
    for (x, y, expected) in [
        (5, 2, 0),
        (5, 1, 1),
        (5, 3, 1),
        (3, 2, 0),
        (7, 2, 0),
        (2, 2, 1),
        (8, 2, 1),
    ] {
        let mut backend =
            TestBackend::new_with_props(ThresholdApp, ActivationThreshold::Axes(3, 1));
        backend.render();
        for (kind, x, y) in [
            (MouseKind::Down(MouseButton::Left), 5, 2),
            (MouseKind::Drag(MouseButton::Left), x, y),
        ] {
            backend
                .send_mouse(MouseEvent {
                    kind,
                    x,
                    y,
                    mods: KeyMods::NONE,
                })
                .unwrap();
        }
        assert_eq!(*backend.state(), expected, "movement to {x},{y}");
    }
}

#[test]
fn symmetric_threshold_still_requires_three_cells_on_either_axis() {
    for (x, y, expected) in [(5, 3, 0), (5, 4, 0), (5, 5, 1), (7, 2, 0), (8, 2, 1)] {
        let mut backend = TestBackend::new_with_props(ThresholdApp, ActivationThreshold::Same(3));
        backend.render();
        for (kind, x, y) in [
            (MouseKind::Down(MouseButton::Left), 5, 2),
            (MouseKind::Drag(MouseButton::Left), x, y),
        ] {
            backend
                .send_mouse(MouseEvent {
                    kind,
                    x,
                    y,
                    mods: KeyMods::NONE,
                })
                .unwrap();
        }
        assert_eq!(*backend.state(), expected, "movement to {x},{y}");
    }
}

struct LivePreviewApp;

#[derive(Default)]
struct LivePreviewState {
    dragging: bool,
    hovered_target: Option<&'static str>,
    dropped_on: Option<&'static str>,
}

enum LivePreviewMsg {
    Started,
    Hover(Option<&'static str>),
    Drop(&'static str),
    Cancel,
}

impl Component for LivePreviewApp {
    type Message = LivePreviewMsg;
    type Properties = ();
    type State = LivePreviewState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        LivePreviewState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            LivePreviewMsg::Started => ctx.state.dragging = true,
            LivePreviewMsg::Hover(target) => ctx.state.hovered_target = target,
            LivePreviewMsg::Drop(target) => {
                ctx.state.dragging = false;
                ctx.state.hovered_target = None;
                ctx.state.dropped_on = Some(target);
            }
            LivePreviewMsg::Cancel => {
                ctx.state.dragging = false;
                ctx.state.hovered_target = None;
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let badge = ctx.state.hovered_target.unwrap_or("");
        let preview = HStack::new()
            .gap(1)
            .height(Length::Px(1))
            .child(Text::new("floating-card").width(Length::Auto))
            .child(Text::new(badge).width(Length::Auto));

        let source = DragSource::new()
            .child(
                Text::new("source-card 2m")
                    .width(Length::Px(24))
                    .height(Length::Px(1)),
            )
            .drag_group("g")
            .threshold(1)
            .preview_element(preview)
            .on_drag_start(|_| Some(Box::new(ItemPayload { id: 1 }) as Box<dyn DragPayload>))
            .on_drag_started(ctx.link().callback(|_| LivePreviewMsg::Started))
            .on_drag_cancel(ctx.link().callback(|_| LivePreviewMsg::Cancel));

        let zone_a = DropTarget::new()
            .child(Text::new("zone-a").height(Length::Px(3)))
            .accept_group("g")
            .on_drag_over(ctx.link().callback(|_| LivePreviewMsg::Hover(Some("Pin"))))
            .on_drag_leave(ctx.link().callback(|_| LivePreviewMsg::Hover(None)))
            .on_drop(ctx.link().callback(|_| LivePreviewMsg::Drop("zone-a")));

        let zone_b = DropTarget::new()
            .child(Text::new("zone-b").height(Length::Px(3)))
            .accept_group("g")
            .on_drag_over(
                ctx.link()
                    .callback(|_| LivePreviewMsg::Hover(Some("Settle"))),
            )
            .on_drag_leave(ctx.link().callback(|_| LivePreviewMsg::Hover(None)))
            .on_drop(ctx.link().callback(|_| LivePreviewMsg::Drop("zone-b")));

        VStack::new()
            .gap(0)
            .child(Element::from(source).key("live-source"))
            .child(zone_a)
            .child(zone_b)
            .into()
    }
}

#[test]
fn preview_element_renders_live_updates_at_cursor_and_does_not_block_drops() {
    let mut backend = TestBackend::new(LivePreviewApp);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    });
    backend.render();

    let idle = backend.capture_frame().plain_text();
    assert!(idle.contains("source-card 2m"), "idle frame:\n{idle}");
    assert!(!idle.contains("floating-card"), "preview hidden when idle");

    let send = |backend: &mut TestBackend<LivePreviewApp>, kind: MouseKind, x: u16, y: u16| {
        backend
            .send_mouse(MouseEvent {
                kind,
                x,
                y,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    };

    // Grab row 0 and drag over zone-a (rows 1..4).
    send(&mut backend, MouseKind::Down(MouseButton::Left), 2, 0);
    send(&mut backend, MouseKind::Drag(MouseButton::Left), 2, 2);
    let over_a = backend.capture_frame().plain_text();
    assert!(
        over_a.contains("floating-card Pin"),
        "live preview should render Pin badge over zone-a:\n{over_a}"
    );
    assert!(
        !over_a.contains("source-card 2m"),
        "source child should stay hidden while DragPreview::Element floats:\n{over_a}"
    );

    // Move over zone-b (rows 4..7): preview must update dynamically to Settle.
    send(&mut backend, MouseKind::Drag(MouseButton::Left), 2, 5);
    let over_b = backend.capture_frame().plain_text();
    assert!(
        over_b.contains("floating-card Settle"),
        "live preview should update to Settle over zone-b:\n{over_b}"
    );

    // Release over zone-b: inert preview must never intercept the drop target.
    send(&mut backend, MouseKind::Up(MouseButton::Left), 2, 5);
    assert_eq!(backend.state().dropped_on, Some("zone-b"));
    let after = backend.capture_frame().plain_text();
    assert!(
        !after.contains("floating-card"),
        "preview should disappear after drop:\n{after}"
    );
}

struct CollapsedTreeDragApp;

impl Component for CollapsedTreeDragApp {
    type Message = LivePreviewMsg;
    type Properties = ();
    type State = LivePreviewState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        LivePreviewState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            LivePreviewMsg::Started => ctx.state.dragging = true,
            LivePreviewMsg::Hover(target) => ctx.state.hovered_target = target,
            LivePreviewMsg::Drop(target) => {
                ctx.state.dragging = false;
                ctx.state.hovered_target = None;
                ctx.state.dropped_on = Some(target);
            }
            LivePreviewMsg::Cancel => {
                ctx.state.dragging = false;
                ctx.state.hovered_target = None;
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let collapsed = ctx.state.hovered_target == Some("Pinned");
        let badge = ctx.state.hovered_target.unwrap_or("");
        let preview = HStack::new()
            .gap(1)
            .height(Length::Px(2))
            .style(Style::new().bg(Color::Black))
            .child(Text::new("floating-row").width(Length::Auto))
            .child(Text::new(badge).width(Length::Auto));

        let pinned_target = DropTarget::new()
            .child(Text::new("Pinned-zone").height(Length::Px(2)))
            .accept_group("g")
            .on_drag_over(
                ctx.link()
                    .callback(|_| LivePreviewMsg::Hover(Some("Pinned"))),
            )
            .on_drag_leave(ctx.link().callback(|_| LivePreviewMsg::Hover(None)))
            .on_drop(ctx.link().callback(|_| LivePreviewMsg::Drop("Pinned")));

        let source = DragSource::new()
            .child(
                HStack::new()
                    .width(Length::Flex(1))
                    .height(Length::Px(2))
                    .child(Text::new("source-row")),
            )
            .drag_group("g")
            .threshold(1)
            .preview_element(preview)
            .on_drag_start(|_| Some(Box::new(ItemPayload { id: 2 }) as Box<dyn DragPayload>))
            .on_drag_started(ctx.link().callback(|_| LivePreviewMsg::Started))
            .on_drag_cancel(ctx.link().callback(|_| LivePreviewMsg::Cancel))
            .key("tree-drag-src");

        let nodes = vec![
            TreeNode::new(ListItem::header("Pinned"))
                .content(pinned_target)
                .content_height(2)
                .show_arrow(false)
                .gap_before(0)
                .key("pinned-zone"),
            TreeNode::new(ListItem::new("source"))
                .content(source)
                .content_height(if collapsed { 0 } else { 2 })
                .show_arrow(false)
                .gap_before(0)
                .key("source-node"),
            TreeNode::new(ListItem::new("below"))
                .content(Text::new("below-row").height(Length::Px(1)))
                .content_height(1)
                .show_arrow(false)
                .gap_before(0)
                .key("below-node"),
        ];

        Tree::forest(nodes).into()
    }
}

#[test]
fn collapsed_tree_node_keeps_drag_source_and_live_preview_alive() {
    let mut backend = TestBackend::new(CollapsedTreeDragApp);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 36,
        h: 10,
    });
    backend.render();

    let idle = backend.capture_frame().plain_text();
    let idle_lines: Vec<&str> = idle.lines().collect();
    let below_idle = idle_lines
        .iter()
        .position(|l| l.contains("below-row"))
        .expect("below-row visible when idle");
    assert_eq!(below_idle, 4, "idle frame:\n{idle}");

    let send =
        |backend: &mut TestBackend<CollapsedTreeDragApp>, kind: MouseKind, x: u16, y: u16| {
            backend
                .send_mouse(MouseEvent {
                    kind,
                    x,
                    y,
                    mods: KeyMods::NONE,
                })
                .unwrap();
            backend.render();
        };

    // Grab source-row at y=2 and drag up onto Pinned-zone at y=0.
    send(&mut backend, MouseKind::Down(MouseButton::Left), 2, 2);
    send(&mut backend, MouseKind::Drag(MouseButton::Left), 2, 0);

    let dragging = backend.capture_frame().plain_text();
    let drag_lines: Vec<&str> = dragging.lines().collect();
    assert!(
        dragging.contains("floating-row Pinned"),
        "collapsed tree row must keep live preview alive at full width:\n{dragging}"
    );
    let below_drag = drag_lines
        .iter()
        .position(|l| l.contains("below-row"))
        .expect("below-row shifts up when source collapses to 0");
    assert_eq!(
        below_drag, 2,
        "below-row should collapse up from row 4 to row 2:\n{dragging}"
    );

    send(&mut backend, MouseKind::Up(MouseButton::Left), 2, 0);
    assert_eq!(backend.state().dropped_on, Some("Pinned"));
}

struct RailAndContextApp;

impl Component for RailAndContextApp {
    type Message = LivePreviewMsg;
    type Properties = ();
    type State = LivePreviewState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        LivePreviewState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            LivePreviewMsg::Started => ctx.state.dragging = true,
            LivePreviewMsg::Hover(target) => ctx.state.hovered_target = target,
            LivePreviewMsg::Drop(target) => {
                ctx.state.dragging = false;
                ctx.state.hovered_target = None;
                ctx.state.dropped_on = Some(target);
            }
            LivePreviewMsg::Cancel => {
                ctx.state.dragging = false;
                ctx.state.hovered_target = None;
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let over_main = ctx.state.hovered_target == Some("main");
        let preview: Element = if over_main {
            Text::new("chip-preview")
                .width(Length::Auto)
                .height(Length::Px(1))
                .style(Style::new().bg(Color::Blue))
                .into()
        } else {
            Text::new("rail-preview")
                .width(Length::Flex(1))
                .height(Length::Px(1))
                .style(Style::new().bg(Color::DarkGray))
                .into()
        };

        let source = DragSource::new()
            .child(
                Text::new("source-item")
                    .width(Length::Flex(1))
                    .height(Length::Px(1)),
            )
            .drag_group("g")
            .threshold(1)
            .preview_element(preview)
            .preview_anchor(if over_main {
                DragPreviewAnchor::CursorOffset(1, 0)
            } else {
                DragPreviewAnchor::VerticalRail
            })
            .hide_source_while_dragging(!over_main)
            .on_drag_start(|_| Some(Box::new(ItemPayload { id: 3 }) as Box<dyn DragPayload>))
            .on_drag_started(ctx.link().callback(|_| LivePreviewMsg::Started))
            .on_drag_cancel(ctx.link().callback(|_| LivePreviewMsg::Cancel))
            .key("rail-src");

        let rail = DropTarget::new()
            .child(
                VStack::new()
                    .width(Length::Px(16))
                    .height(Length::Flex(1))
                    .child(source)
                    .child(Spacer::new()),
            )
            .accept_group("g")
            .on_drag_over(ctx.link().callback(|_| LivePreviewMsg::Hover(Some("rail"))));

        let main = DropTarget::new()
            .child(
                VStack::new()
                    .width(Length::Flex(1))
                    .height(Length::Flex(1))
                    .child(Text::new("main-pane")),
            )
            .accept_group("g")
            .on_drag_over(ctx.link().callback(|_| LivePreviewMsg::Hover(Some("main"))))
            .on_drop(ctx.link().callback(|_| LivePreviewMsg::Drop("main")));

        HStack::new().gap(0).child(rail).child(main).into()
    }
}

#[test]
fn preview_anchor_locks_vertical_rail_then_switches_to_cursor_chip_with_visible_source() {
    let mut backend = TestBackend::new(RailAndContextApp);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 44,
        h: 8,
    });
    backend.render();

    let send = |backend: &mut TestBackend<RailAndContextApp>, kind: MouseKind, x: u16, y: u16| {
        backend
            .send_mouse(MouseEvent {
                kind,
                x,
                y,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    };

    // Grab at x=2, y=0 and drag diagonally within the rail to x=10, y=3:
    // VerticalRail must keep rail-preview locked at column 0 (not column 8!).
    send(&mut backend, MouseKind::Down(MouseButton::Left), 2, 0);
    send(&mut backend, MouseKind::Drag(MouseButton::Left), 10, 3);
    let in_rail = backend.capture_frame().plain_text();
    let row3 = in_rail.lines().nth(3).unwrap_or("");
    assert!(
        row3.starts_with("rail-preview"),
        "VerticalRail should lock X to column 0 even when mouse drifted to x=10:\n{in_rail}"
    );
    assert!(
        !in_rail.contains("source-item"),
        "source should stay hidden while hide_source_while_dragging(true):\n{in_rail}"
    );

    // Move right into main-pane (x=24, y=4):
    // preview switches to CursorOffset(1, 0) at x=25, y=4, and source-item reappears!
    send(&mut backend, MouseKind::Drag(MouseButton::Left), 24, 4);
    let in_main = backend.capture_frame().plain_text();
    assert!(
        in_main.contains("source-item"),
        "source should remain visible when hide_source_while_dragging(false):\n{in_main}"
    );
    let row4 = in_main.lines().nth(4).unwrap_or("");
    let chip_col = row4
        .find("chip-preview")
        .unwrap_or_else(|| panic!("expected chip-preview on row 4:\n{in_main}"));
    assert_eq!(
        chip_col, 25,
        "CursorOffset(1, 0) from x=24 should start at col 25"
    );

    send(&mut backend, MouseKind::Up(MouseButton::Left), 24, 4);
    assert_eq!(backend.state().dropped_on, Some("main"));
}

struct PreviewOverTableApp;

impl Component for PreviewOverTableApp {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let preview = Frame::new()
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding((0, 1))
            .style(
                Style::new()
                    .fg(Color::White)
                    .bg(Color::Rgb(0x11, 0x11, 0x11)),
            )
            .width(Length::Auto)
            .height(Length::Px(3))
            .child(
                Text::new("Print Markdown Test Text")
                    .style(
                        Style::new()
                            .fg(Color::White)
                            .bg(Color::Rgb(0x11, 0x11, 0x11)),
                    )
                    .width(Length::Auto)
                    .height(Length::Px(1)),
            );

        let source = DragSource::new()
            .child(
                Text::new("drag-me")
                    .width(Length::Px(10))
                    .height(Length::Px(1)),
            )
            .drag_group("g")
            .threshold(1)
            .preview_element(preview)
            .preview_anchor(DragPreviewAnchor::CursorOffset(1, 0))
            .on_drag_start(|_| Some(Box::new(ItemPayload { id: 1 }) as Box<dyn DragPayload>));

        let table = Table::new()
            .border(true)
            .border_style(BorderStyle::Rounded)
            .header(["Widget", "Module", "Default size"])
            .rows([
                vec!["Button", "src/widgets/", "content"],
                vec!["ScrollView", "src/widgets/", "Flex(1)"],
                vec!["Text", "src/widgets/", "Auto"],
            ]);

        HStack::new()
            .gap(0)
            .child(VStack::new().width(Length::Px(12)).child(source))
            .child(table)
            .into()
    }
}

#[test]
fn preview_element_clears_underlying_table_cells_without_bleed_through() {
    let mut backend = TestBackend::new(PreviewOverTableApp);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 10,
    });
    backend.render();

    let send = |backend: &mut TestBackend<PreviewOverTableApp>, kind: MouseKind, x: u16, y: u16| {
        backend
            .send_mouse(MouseEvent {
                kind,
                x,
                y,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    };

    // Drag over the table header + first data row so the 3-row bordered preview covers them.
    send(&mut backend, MouseKind::Down(MouseButton::Left), 2, 0);
    send(&mut backend, MouseKind::Drag(MouseButton::Left), 13, 1);

    let frame = backend.capture_frame().plain_text();
    let lines: Vec<&str> = frame.lines().collect();
    // Preview starts at x=14 (CursorOffset(1, 0) from x=13), y=1, height=3 -> rows 1..=3.
    let preview_top = lines[1];
    let preview_mid = lines[2];
    let preview_bot = lines[3];

    assert!(
        preview_mid.contains("Print Markdown Test Text"),
        "expected floating preview label on row 2:\n{frame}"
    );
    for (label, row) in [
        ("top border row", preview_top),
        ("middle content row", preview_mid),
        ("bottom border row", preview_bot),
    ] {
        assert!(
            !row.contains("Widget") && !row.contains("Module") && !row.contains("Button"),
            "underlying table text bled through {label}:\n{frame}"
        );
    }
}

struct PreviewChip;

struct PreviewChipState {
    label: &'static str,
}

impl Component for PreviewChip {
    type Message = ();
    type Properties = ();
    type State = PreviewChipState;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        PreviewChipState {
            label: "stateful-chip",
        }
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Text::new(ctx.state.label)
            .width(Length::Auto)
            .height(Length::Px(1))
            .into()
    }
}

fn source_with_chip(hide_source: bool) -> Element {
    DragSource::new()
        .child(
            Text::new("keep-source")
                .width(Length::Px(12))
                .height(Length::Px(1)),
        )
        .threshold(1)
        .preview_element(child(|| PreviewChip, ()))
        .preview_anchor(DragPreviewAnchor::CursorOffset(16, 0))
        .hide_source_while_dragging(hide_source)
        .on_drag_start(|_| Some(Box::new(ItemPayload { id: 9 }) as Box<dyn DragPayload>))
        .key("chip-src")
}

fn drag_chip_source<C: Component>(backend: &mut TestBackend<C>) {
    let send = |backend: &mut TestBackend<C>, kind: MouseKind, x: u16, y: u16| {
        backend
            .send_mouse(MouseEvent {
                kind,
                x,
                y,
                mods: KeyMods::NONE,
            })
            .unwrap();
        backend.render();
    };
    send(backend, MouseKind::Down(MouseButton::Left), 1, 0);
    send(backend, MouseKind::Drag(MouseButton::Left), 3, 0);
}

struct VisibleChipSourceApp;

impl Component for VisibleChipSourceApp {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        VStack::new()
            .gap(0)
            .child(source_with_chip(false))
            .child(Text::new("below-row").height(Length::Px(1)))
            .into()
    }
}

#[test]
fn element_preview_expands_stateful_component_and_keeps_visible_source() {
    let mut backend = TestBackend::new(VisibleChipSourceApp);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 6,
    });
    backend.render();

    drag_chip_source(&mut backend);

    let frame = backend.capture_frame().plain_text();
    let lines: Vec<&str> = frame.lines().collect();
    assert!(
        lines[0].contains("keep-source"),
        "visible source must keep its row:\n{frame}"
    );
    assert!(
        lines[1].contains("below-row"),
        "sibling must stay below a visible source:\n{frame}"
    );
    assert!(
        frame.contains("stateful-chip"),
        "stateful preview component must render:\n{frame}"
    );
}

struct CollapsedChipSourceApp;

impl Component for CollapsedChipSourceApp {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        VStack::new()
            .gap(0)
            .child(source_with_chip(true))
            .child(Text::new("below-row").height(Length::Px(1)))
            .into()
    }
}

#[test]
fn element_preview_collapses_hidden_source_in_test_backend() {
    let mut backend = TestBackend::new(CollapsedChipSourceApp);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 6,
    });
    backend.render();

    drag_chip_source(&mut backend);

    let frame = backend.capture_frame().plain_text();
    let lines: Vec<&str> = frame.lines().collect();
    assert!(
        lines[0].contains("below-row"),
        "hidden source must collapse so the sibling moves up:\n{frame}"
    );
    assert!(
        !frame.contains("keep-source"),
        "hidden source must not remain painted:\n{frame}"
    );
    assert!(
        frame.contains("stateful-chip"),
        "stateful preview component must render:\n{frame}"
    );
}
