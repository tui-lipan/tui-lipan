use tui_lipan::TestBackend;
use tui_lipan::core::event::{MouseButton, MouseEvent, MouseKind};
use tui_lipan::prelude::*;

struct BorderSplitter;

struct JunctionSplitters {
    inner_on_right: bool,
}

#[derive(Clone)]
enum Msg {
    Resizing(SplitterResizeEvent),
    Resized(SplitterResizeEvent),
}

#[derive(Default)]
struct ResizeState {
    live_events: usize,
    committed: bool,
}

impl Component for BorderSplitter {
    type State = ResizeState;
    type Message = Msg;
    type Properties = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        ResizeState::default()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Resizing(event) => {
                assert_eq!(event.weights.len(), 2);
                ctx.state.live_events += 1;
            }
            Msg::Resized(event) => {
                assert_eq!(event.weights.len(), 2);
                ctx.state.committed = true;
            }
        }
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Splitter::vertical()
            .handle_mode(SplitterHandleMode::Border)
            .weights(vec![0.5, 0.5])
            .on_resize_live(ctx.link().callback(Msg::Resizing))
            .on_resize(ctx.link().callback(Msg::Resized))
            .child(Frame::new().border(true).child(Text::new("left")))
            .child(Frame::new().border(true).child(Text::new("right")))
            .into()
    }
}

impl Component for JunctionSplitters {
    type State = ();
    type Message = ();
    type Properties = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let inner: Element = Splitter::horizontal()
            .weights(vec![0.5, 0.5])
            .child(Spacer::new())
            .child(Spacer::new())
            .into();
        let outer = Splitter::vertical().weights(vec![0.5, 0.5]);
        if self.inner_on_right {
            outer.child(Spacer::new()).child(inner).into()
        } else {
            outer.child(inner).child(Spacer::new()).into()
        }
    }
}

fn mouse(x: u16, y: u16, kind: MouseKind) -> MouseEvent {
    MouseEvent {
        x,
        y,
        kind,
        mods: KeyMods::NONE,
    }
}

#[test]
fn border_riding_splitter_drag_works_through_child_hit_and_test_backend() {
    let mut backend = TestBackend::new(BorderSplitter);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 5,
    });
    backend.render();

    assert!(
        backend
            .send_mouse(mouse(9, 2, MouseKind::Down(MouseButton::Left)))
            .unwrap()
    );
    assert!(
        backend
            .send_mouse(mouse(12, 2, MouseKind::Drag(MouseButton::Left)))
            .unwrap()
    );
    backend.render();

    assert!(backend.state().live_events > 0);
    assert!(!backend.state().committed);

    let frame = backend.capture_frame();
    assert_eq!(frame.cell(12, 2).symbol, "│");
    assert!(
        backend
            .send_mouse(mouse(12, 2, MouseKind::Up(MouseButton::Left)))
            .unwrap()
    );
    assert!(backend.state().committed);
}

/// A sidebar-shaped split: the leading pane is bounded, the trailing one takes
/// whatever is left.
struct LimitedSplitter;

impl Component for LimitedSplitter {
    type State = Vec<f32>;
    type Message = SplitterResizeEvent;
    type Properties = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        Vec::new()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        ctx.state = msg.weights;
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Splitter::vertical()
            .weights(vec![10.0, 29.0])
            .pane_limits(vec![
                SplitterPaneLimits::range(5, 15),
                SplitterPaneLimits::UNBOUNDED,
            ])
            .on_resize(ctx.link().callback(|event| event))
            .child(Spacer::new())
            .child(Spacer::new())
            .into()
    }
}

fn drag_handle(backend: &mut TestBackend<LimitedSplitter>, from: u16, to: u16) {
    assert!(
        backend
            .send_mouse(mouse(from, 2, MouseKind::Down(MouseButton::Left)))
            .unwrap()
    );
    backend
        .send_mouse(mouse(to, 2, MouseKind::Drag(MouseButton::Left)))
        .unwrap();
    backend.render();
    backend
        .send_mouse(mouse(to, 2, MouseKind::Up(MouseButton::Left)))
        .unwrap();
    backend.render();
}

/// Dragging a bounded pane past its limits stops at them, and the split still
/// covers the splitter: no gap opens beside the pane that stopped.
#[test]
fn pane_limits_stop_the_drag_at_the_bound() {
    let handle_column = |backend: &TestBackend<LimitedSplitter>| {
        let frame = backend.capture_frame();
        (0..40)
            .find(|x| frame.cell(*x, 2).symbol == "│")
            .expect("splitter handle")
    };

    let mut backend = TestBackend::new(LimitedSplitter);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    });
    backend.render();
    assert_eq!(handle_column(&backend), 10);

    // Past the ceiling: the handle stops at 15 columns, not at the pointer.
    drag_handle(&mut backend, 10, 30);
    assert_eq!(handle_column(&backend), 15);
    // 39 columns of panes with the handle taking the fortieth.
    assert_eq!(backend.state()[0], 15.0 / 39.0);

    // And past the floor, from the other side.
    drag_handle(&mut backend, 15, 1);
    assert_eq!(handle_column(&backend), 5);
    assert_eq!(backend.state()[0], 5.0 / 39.0);
}

#[test]
fn touching_nested_splitters_render_directional_tees() {
    let viewport = Rect {
        x: 0,
        y: 0,
        w: 21,
        h: 9,
    };

    let mut left = TestBackend::new(JunctionSplitters {
        inner_on_right: false,
    });
    left.set_viewport(viewport);
    left.render();
    assert_eq!(left.capture_frame().cell(10, 4).symbol, "┤");

    let mut right = TestBackend::new(JunctionSplitters {
        inner_on_right: true,
    });
    right.set_viewport(viewport);
    right.render();
    assert_eq!(right.capture_frame().cell(10, 4).symbol, "├");
}

struct FocusSplitter {
    horizontal: bool,
    border: bool,
}

impl Component for FocusSplitter {
    type State = ();
    type Message = ();
    type Properties = ();

    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let splitter = if self.horizontal {
            Splitter::horizontal()
        } else {
            Splitter::vertical()
        };
        splitter
            .handle_mode(if self.border {
                SplitterHandleMode::Border
            } else {
                SplitterHandleMode::Gutter
            })
            .child(
                Frame::new()
                    .border(self.border)
                    .child(Input::new("first").key("first")),
            )
            .child(
                Frame::new()
                    .border(self.border)
                    .child(Input::new("second").key("second")),
            )
            .into()
    }
}

#[test]
fn splitter_handle_preserves_focus_and_content_clicks_still_focus() {
    for horizontal in [false, true] {
        for border in [false, true] {
            let mut backend = TestBackend::new(FocusSplitter { horizontal, border });
            backend.set_viewport(Rect {
                x: 0,
                y: 0,
                w: 21,
                h: 11,
            });
            backend.render();
            backend.focus_next();
            backend.focus_next();
            assert_eq!(backend.focused_key().unwrap().as_ref(), "second");
            let (x, y, to_x, to_y) = if horizontal {
                (10, 5, 10, 7)
            } else {
                (10, 5, 12, 5)
            };
            for (x, y, kind) in [
                (x, y, MouseKind::Down(MouseButton::Left)),
                (to_x, to_y, MouseKind::Drag(MouseButton::Left)),
                (to_x, to_y, MouseKind::Up(MouseButton::Left)),
            ] {
                backend.send_mouse(mouse(x, y, kind)).unwrap();
                assert_eq!(
                    backend.focused_key().unwrap().as_ref(),
                    "second",
                    "horizontal={horizontal}, border={border}, {kind:?}"
                );
            }
            for kind in [
                MouseKind::Down(MouseButton::Left),
                MouseKind::Up(MouseButton::Left),
            ] {
                backend.send_mouse(mouse(2, 1, kind)).unwrap();
            }
            assert_eq!(backend.focused_key().unwrap().as_ref(), "first");
        }
    }
}

struct PointerSplitter {
    horizontal: bool,
    border: bool,
    hover: Option<PointerShape>,
    active: Option<PointerShape>,
}

impl Component for PointerSplitter {
    type State = bool;
    type Message = bool;
    type Properties = ();
    fn create_state(&self, _: &()) -> bool {
        true
    }
    fn update(&mut self, visible: bool, ctx: &mut Context<Self>) -> Update {
        ctx.state = visible;
        Update::full()
    }
    fn view(&self, ctx: &Context<Self>) -> Element {
        if !ctx.state {
            return Text::new("replacement")
                .width(Length::Flex(1))
                .height(Length::Flex(1))
                .pointer_shape(PointerShape::Help);
        }
        let mut splitter = if self.horizontal {
            Splitter::horizontal()
        } else {
            Splitter::vertical()
        };
        splitter = splitter.handle_mode(if self.border {
            SplitterHandleMode::Border
        } else {
            SplitterHandleMode::Gutter
        });
        if let Some(shape) = self.hover {
            splitter = splitter.handle_pointer_shape(shape);
        }
        if let Some(shape) = self.active {
            splitter = splitter.handle_active_pointer_shape(shape);
        }
        let child = || {
            Frame::new().border(self.border).child(
                Text::new("pane")
                    .width(Length::Flex(1))
                    .height(Length::Flex(1))
                    .pointer_shape(PointerShape::Text),
            )
        };
        splitter.child(child()).child(child()).into()
    }
}

fn pointer_backend(
    horizontal: bool,
    border: bool,
    hover: Option<PointerShape>,
    active: Option<PointerShape>,
) -> TestBackend<PointerSplitter> {
    let mut backend = TestBackend::new(PointerSplitter {
        horizontal,
        border,
        hover,
        active,
    });
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 21,
        h: 11,
    });
    backend.render();
    backend
}

#[test]
fn splitter_handle_pointers_are_opt_in_and_exclude_child_content() {
    for horizontal in [false, true] {
        for border in [false, true] {
            let shape = if horizontal {
                PointerShape::NsResize
            } else {
                PointerShape::EwResize
            };
            let default = pointer_backend(horizontal, border, None, None);
            assert_eq!(default.pointer_shape_at(10, 5), None);
            let opted_in = pointer_backend(horizontal, border, Some(shape), None);
            assert_eq!(opted_in.pointer_shape_at(10, 5), Some(shape));
            assert_eq!(opted_in.pointer_shape_at(1, 1), Some(PointerShape::Text));
        }
    }
}

#[test]
fn splitter_drag_retains_active_pointer_across_children_and_releases_it() {
    for horizontal in [false, true] {
        for border in [false, true] {
            for active in [None, Some(PointerShape::Grabbing)] {
                let hover = if horizontal {
                    PointerShape::NsResize
                } else {
                    PointerShape::EwResize
                };
                let mut backend = pointer_backend(horizontal, border, Some(hover), active);
                backend.send_mouse(mouse(10, 5, MouseKind::Moved)).unwrap();
                assert_eq!(backend.pointer_shape(), Some(hover));
                backend
                    .send_mouse(mouse(10, 5, MouseKind::Down(MouseButton::Left)))
                    .unwrap();
                backend
                    .send_mouse(mouse(1, 1, MouseKind::Drag(MouseButton::Left)))
                    .unwrap();
                backend.render();
                assert_eq!(backend.pointer_shape(), active.or(Some(hover)));
                backend
                    .send_mouse(mouse(1, 1, MouseKind::Up(MouseButton::Left)))
                    .unwrap();
                assert_eq!(backend.pointer_shape(), Some(PointerShape::Text));
            }
        }
    }
}

#[test]
fn splitter_pointer_capture_handles_active_only_focus_loss_and_removal() {
    let mut backend = pointer_backend(false, false, None, Some(PointerShape::Grabbing));
    backend.send_mouse(mouse(10, 5, MouseKind::Moved)).unwrap();
    assert_eq!(backend.pointer_shape(), None);
    backend
        .send_mouse(mouse(10, 5, MouseKind::Down(MouseButton::Left)))
        .unwrap();
    assert_eq!(backend.pointer_shape(), Some(PointerShape::Grabbing));
    backend.set_window_focused(false).unwrap();
    assert_eq!(backend.pointer_shape(), None);

    let mut backend = pointer_backend(
        false,
        false,
        Some(PointerShape::EwResize),
        Some(PointerShape::Grabbing),
    );
    backend
        .send_mouse(mouse(10, 5, MouseKind::Down(MouseButton::Left)))
        .unwrap();
    backend.update_level(false).unwrap();
    backend.render();
    assert_eq!(backend.pointer_shape(), Some(PointerShape::Help));
}

#[test]
fn interactive_splitter_pointers_follow_the_handle_axis_during_hover_and_drag() {
    struct InteractiveSplitter(bool);
    impl Component for InteractiveSplitter {
        type State = ();
        type Message = ();
        type Properties = ();
        fn create_state(&self, _: &()) {}
        fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
            Update::none()
        }
        fn view(&self, ctx: &Context<Self>) -> Element {
            ctx.set_pointer_mode(PointerMode::Interactive);
            let splitter = if self.0 {
                Splitter::horizontal()
            } else {
                Splitter::vertical()
            };
            splitter
                .child(Text::new("one"))
                .child(Text::new("two"))
                .into()
        }
    }
    for horizontal in [false, true] {
        let expected = if horizontal {
            PointerShape::NsResize
        } else {
            PointerShape::EwResize
        };
        let mut backend = TestBackend::new(InteractiveSplitter(horizontal));
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 21,
            h: 11,
        });
        backend.render();
        backend.send_mouse(mouse(10, 5, MouseKind::Moved)).unwrap();
        assert_eq!(backend.pointer_shape(), Some(expected));
        backend
            .send_mouse(mouse(10, 5, MouseKind::Down(MouseButton::Left)))
            .unwrap();
        backend
            .send_mouse(mouse(1, 1, MouseKind::Drag(MouseButton::Left)))
            .unwrap();
        assert_eq!(backend.pointer_shape(), Some(expected));
        backend
            .send_mouse(mouse(1, 1, MouseKind::Up(MouseButton::Left)))
            .unwrap();
        assert_eq!(backend.pointer_shape(), None);
    }
}
