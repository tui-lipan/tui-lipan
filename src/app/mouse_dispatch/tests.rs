use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::app::interaction_state::{DragState, MouseRegionDragState, MouseTrackingState};
use crate::callback::Callback;
use crate::core::component::{Component, Context, Update};
use crate::core::element::{Element, IntoElement, Key};
use crate::core::event::{
    KeyCode, KeyEvent, KeyMods, MouseButton, MouseEvent, MouseKind, MouseMoveEvent,
};
use crate::core::node::{NodeId, NodeKind, NodeTree, ScrollbarTarget};
use crate::style::{Align, Color, Length, Rect, Span};
use crate::widgets::document_view::FormattedLink;
use crate::widgets::{
    AsciiCanvas, Button, CenterPin, ContentFormatter, DocumentClickEvent, DocumentView,
    EffectScope, FormatInput, FormattedBlock, FormattedDocument, FormattedLine, Frame, MouseRegion,
    ScrollEvent, ScrollView, Spacer, StatusBar, Text, TextArea, TextAreaEvent, TextAreaVimMode,
    VStack,
};
use crate::{CellMask, TextEditor};

struct MockComponent;

impl Component for MockComponent {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Text::new("").into()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
}

struct MockCtx {
    mouse_state: MouseTrackingState,
    drag_state: DragState,
    overlay_click_result: bool,
    drag_release_result: Option<bool>,
}

impl MockCtx {
    fn new() -> Self {
        Self {
            mouse_state: MouseTrackingState::default(),
            drag_state: DragState::default(),
            overlay_click_result: false,
            drag_release_result: None,
        }
    }
}

impl MouseDispatchCtx<MockComponent> for MockCtx {
    fn adjust_mouse(&mut self, mouse: MouseEvent) -> MouseEvent {
        mouse
    }

    fn mouse_state(&mut self) -> &mut MouseTrackingState {
        &mut self.mouse_state
    }

    fn drag_state(&mut self) -> &mut DragState {
        &mut self.drag_state
    }

    fn tree(&self) -> &NodeTree {
        unimplemented!()
    }

    fn tree_mut(&mut self) -> &mut NodeTree {
        unimplemented!()
    }

    fn drag_is_active(&self) -> bool {
        false
    }

    fn active_drag(&self) -> ActiveDrag {
        ActiveDrag::None
    }

    fn dispatch_mouse_move(&mut self, _mouse: MouseEvent) -> bool {
        false
    }

    fn update_hover(&mut self, _x: u16, _y: u16) -> bool {
        false
    }

    fn update_hover_impl(&mut self, _x: u16, _y: u16, _force: bool) -> bool {
        false
    }

    fn dispatch_active_drag(&mut self, _x: u16, _y: u16) -> Option<bool> {
        None
    }

    fn handle_drag_release(&mut self, _x: u16, _y: u16) -> Option<bool> {
        self.drag_release_result
    }

    fn handle_overlay_click(&mut self, _button: MouseButton, _x: u16, _y: u16) -> bool {
        self.overlay_click_result
    }

    fn handle_right_click_textarea(&mut self, _hit: NodeId, _mouse: MouseEvent) -> bool {
        false
    }

    fn selection_owner_for_node(&self, _start: NodeId) -> Option<SelectionOwner> {
        None
    }

    fn clear_selectable_widget_selections(&mut self, _keep: Option<SelectionOwner>) -> bool {
        false
    }

    fn focus_for_node(&mut self, _id: NodeId) -> bool {
        false
    }

    fn handle_scrollbar_click(&mut self, _target: ScrollbarTarget, _x: u16, _y: u16) -> bool {
        false
    }

    fn handle_slider_click(
        &mut self,
        _hit: NodeId,
        _change: mouse::SliderChange,
        _x: u16,
        _y: u16,
    ) -> bool {
        false
    }

    fn handle_progress_click(&mut self, _change: mouse::ProgressChange) -> bool {
        false
    }

    fn handle_draggable_tab_bar_click(
        &mut self,
        _action: mouse::DraggableTabBarAction,
        _x: u16,
        _dirty: bool,
    ) -> bool {
        false
    }

    fn handle_splitter_click(&mut self, _grab: mouse::SplitterGrab, _x: u16, _y: u16) -> bool {
        false
    }

    fn handle_list_click(
        &mut self,
        _hit: NodeId,
        _select: mouse::ListSelect,
        _x: u16,
        _y: u16,
    ) -> bool {
        false
    }

    fn handle_table_click(
        &mut self,
        _hit: NodeId,
        _select: mouse::TableSelect,
        _x: u16,
        _y: u16,
    ) -> bool {
        false
    }

    fn handle_textarea_click(&mut self, _change: mouse::TextAreaChange, _x: u16, _y: u16) -> bool {
        false
    }

    fn handle_document_view_click(
        &mut self,
        _hit: NodeId,
        _mouse: MouseEvent,
        _x: u16,
        _y: u16,
    ) -> (bool, bool) {
        (false, false)
    }

    fn handle_input_click(&mut self, _change: mouse::InputChange, _x: u16) -> bool {
        false
    }

    fn handle_hex_area_click(
        &mut self,
        _hit: NodeId,
        _mouse: MouseEvent,
        _x: u16,
        _y: u16,
    ) -> bool {
        false
    }

    fn handle_tabs_click(&mut self, _change: mouse::TabsChange) -> bool {
        false
    }

    fn handle_checkbox_click(&mut self, _toggle: mouse::CheckboxToggle) -> bool {
        false
    }

    fn handle_document_click(&mut self, _click: mouse::DocumentClick) -> bool {
        false
    }

    fn handle_graph_node_click(&mut self, _click: mouse::GraphNodeClick) -> bool {
        false
    }

    fn handle_sequence_item_click(&mut self, _click: mouse::SequenceItemClick) -> bool {
        false
    }

    fn handle_flowchart_item_click(&mut self, _click: mouse::FlowchartItemClick) -> bool {
        false
    }

    fn handle_fallback_click(&mut self, _cb: Callback<MouseEvent>, _mouse: MouseEvent) -> bool {
        false
    }
}

#[test]
fn overlay_click_precedes_underlying_widget() {
    let mut ctx = MockCtx::new();
    ctx.overlay_click_result = true;

    let mouse = MouseEvent {
        x: 5,
        y: 5,
        kind: MouseKind::Down(MouseButton::Left),
        mods: Default::default(),
    };

    let result = dispatch_mouse_shared(&mut ctx, mouse);
    assert!(result, "overlay click should consume the event");
}

#[test]
fn drag_threshold_tracks_movement() {
    let mut ctx = MockCtx::new();

    let down = MouseEvent {
        x: 10,
        y: 10,
        kind: MouseKind::Down(MouseButton::Left),
        mods: Default::default(),
    };
    transition_drag_threshold(&mut ctx, &down, 10, 10);
    assert!(!ctx.mouse_state.drag_threshold_exceeded);

    let small_drag = MouseEvent {
        x: 11,
        y: 10,
        kind: MouseKind::Drag(MouseButton::Left),
        mods: Default::default(),
    };
    transition_drag_threshold(&mut ctx, &small_drag, 11, 10);
    assert!(!ctx.mouse_state.drag_threshold_exceeded);

    let big_drag = MouseEvent {
        x: 14,
        y: 10,
        kind: MouseKind::Drag(MouseButton::Left),
        mods: Default::default(),
    };
    transition_drag_threshold(&mut ctx, &big_drag, 14, 10);
    assert!(ctx.mouse_state.drag_threshold_exceeded);
}

#[test]
fn drag_release_consumed_when_threshold_exceeded() {
    let mut ctx = MockCtx::new();
    ctx.mouse_state.drag_threshold_exceeded = true;
    ctx.drag_release_result = Some(true);

    let up = MouseEvent {
        x: 10,
        y: 10,
        kind: MouseKind::Up(MouseButton::Left),
        mods: Default::default(),
    };

    let (dirty, consumed) = transition_drag_release(&mut ctx, &up, 10, 10);
    assert!(dirty);
    assert!(consumed);
    assert!(!ctx.drag_state.autoscroll_layout_dirty);
}

#[test]
fn drag_threshold_prevents_button_click() {
    struct ButtonRoot {
        clicked: Rc<RefCell<bool>>,
    }

    impl Component for ButtonRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Button::new("Click")
                .on_click({
                    let clicked = Rc::clone(&self.clicked);
                    Callback::new(move |_ev: MouseEvent| {
                        *clicked.borrow_mut() = true;
                    })
                })
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let clicked = Rc::new(RefCell::new(false));
    let mut backend = TestBackend::new(ButtonRoot {
        clicked: Rc::clone(&clicked),
    });

    let button_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Button(_)))
        .map(|node| node.id)
        .expect("expected a button node");
    let rect = backend.core.tree.node(button_id).rect;
    let x = rect.x as u16;
    let y = rect.y as u16;

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        },
    );

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x: x + 5,
            y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        },
    );

    assert!(backend.mouse.drag_threshold_exceeded);

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x: x + 5,
            y,
            kind: MouseKind::Up(MouseButton::Left),
            mods: Default::default(),
        },
    );

    assert!(
        !*clicked.borrow(),
        "click should not fire when drag threshold exceeded"
    );
}

#[test]
fn modifier_gated_ancestor_mouse_region_drag_starts_over_child() {
    struct RegionRoot {
        starts: Rc<RefCell<u32>>,
    }

    impl Component for RegionRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            MouseRegion::new()
                .drag_requires_mods(KeyMods::ALT)
                .on_drag_start({
                    let starts = Rc::clone(&self.starts);
                    Callback::new(move |_| {
                        *starts.borrow_mut() += 1;
                    })
                })
                .child(Button::new("child"))
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let starts = Rc::new(RefCell::new(0));
    let mut backend = TestBackend::new(RegionRoot {
        starts: Rc::clone(&starts),
    });

    let button_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Button(_)))
        .map(|node| node.id)
        .expect("expected a button node");
    let rect = backend.core.tree.node(button_id).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::NONE,
        },
    );
    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x: x + 5,
            y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: KeyMods::NONE,
        },
    );
    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x: x + 5,
            y,
            kind: MouseKind::Up(MouseButton::Left),
            mods: KeyMods::NONE,
        },
    );
    assert_eq!(
        *starts.borrow(),
        0,
        "plain child drag should stay with the child"
    );

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::ALT,
        },
    );
    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x: x + 5,
            y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: KeyMods::ALT,
        },
    );

    assert_eq!(
        *starts.borrow(),
        1,
        "Alt-drag should bubble to ancestor region"
    );
}

#[test]
fn bubbling_mouse_down_does_not_steal_child_click() {
    struct RegionRoot {
        downs: Rc<RefCell<u32>>,
        clicks: Rc<RefCell<u32>>,
    }

    impl Component for RegionRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            MouseRegion::new()
                .bubble_mouse_down(true)
                .on_mouse_down({
                    let downs = Rc::clone(&self.downs);
                    Callback::new(move |_| {
                        *downs.borrow_mut() += 1;
                    })
                })
                .child(Button::new("child").on_click({
                    let clicks = Rc::clone(&self.clicks);
                    Callback::new(move |_| {
                        *clicks.borrow_mut() += 1;
                    })
                }))
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let downs = Rc::new(RefCell::new(0));
    let clicks = Rc::new(RefCell::new(0));
    let mut backend = TestBackend::new(RegionRoot {
        downs: Rc::clone(&downs),
        clicks: Rc::clone(&clicks),
    });

    let button_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Button(_)))
        .map(|node| node.id)
        .expect("expected a button node");
    let rect = backend.core.tree.node(button_id).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::NONE,
        },
    );
    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Up(MouseButton::Left),
            mods: KeyMods::NONE,
        },
    );

    assert_eq!(*downs.borrow(), 1, "ancestor should see mouse down");
    assert_eq!(*clicks.borrow(), 1, "child click should still fire");
}

#[test]
fn mouse_drag_event_keeps_captured_local_origin_after_rect_shift() {
    struct RegionRoot;

    impl Component for RegionRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            MouseRegion::new()
                .on_drag(Callback::new(|_| {}))
                .child(
                    Text::new("drag me")
                        .width(Length::Px(20))
                        .height(Length::Px(3)),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let mut backend = TestBackend::new(RegionRoot);
    let region_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::MouseRegion(_)))
        .map(|node| node.id)
        .expect("expected a MouseRegion node");
    let rect = backend.core.tree.node(region_id).rect;
    let origin = (
        rect.x.saturating_add(2).max(0) as u16,
        rect.y.saturating_add(1).max(0) as u16,
    );
    let state = MouseRegionDragState {
        node_id: region_id,
        button: MouseButton::Left,
        origin,
        origin_local: (2, 1),
        last_pos: origin,
        started: false,
    };

    backend.core.tree.node_mut(region_id).rect.x += 8;
    let event = mouse_region_drag_event(
        &backend.core.tree,
        state,
        origin.0.saturating_add(4),
        origin.1,
        KeyMods::ALT,
    )
    .expect("drag event");

    assert_eq!(event.from_local_x, 2);
    assert_eq!(event.from_local_y, 1);
}

#[test]
fn mouse_region_wrapping_frame_receives_border_clicks() {
    struct FramedRegionRoot {
        clicked: Rc<RefCell<bool>>,
    }

    impl Component for FramedRegionRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            MouseRegion::new()
                .on_click({
                    let clicked = Rc::clone(&self.clicked);
                    Callback::new(move |_ev: MouseEvent| {
                        *clicked.borrow_mut() = true;
                    })
                })
                .child(
                    Frame::new()
                        .title(" hit ")
                        .border(true)
                        .width(Length::Px(11))
                        .height(Length::Px(5))
                        .child(Text::new("mole")),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let clicked = Rc::new(RefCell::new(false));
    let mut backend = TestBackend::new(FramedRegionRoot {
        clicked: Rc::clone(&clicked),
    });

    let frame_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Frame(_)))
        .map(|node| node.id)
        .expect("expected a frame node");
    let rect = backend.core.tree.node(frame_id).rect;
    let x = rect.x.saturating_add(1).max(0) as u16;
    let y = rect.y.max(0) as u16;

    let hit = backend.core.tree.hit_test(x as i16, y as i16);
    assert!(hit.is_some(), "border point should hit MouseRegion subtree");
    let hit = hit.unwrap();
    let target = mouse::resolve_left_click_target(&backend.core.tree, hit, KeyMods::NONE);
    assert!(
        matches!(
            backend.core.tree.node(target).kind,
            NodeKind::MouseRegion(_)
        ),
        "border point should resolve to wrapping MouseRegion"
    );

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        },
    );
    assert_eq!(backend.mouse.left_down_node, Some(target));
    assert!(
        mouse::gather_hit_actions(&backend.core.tree, target, x, y)
            .on_click
            .is_some()
    );
    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Up(MouseButton::Left),
            mods: Default::default(),
        },
    );

    assert!(
        *clicked.borrow(),
        "MouseRegion should receive clicks on the wrapped Frame border"
    );
}

#[test]
fn mouse_region_wrapping_frame_receives_border_mouse_down() {
    struct FramedRegionRoot {
        pressed: Rc<RefCell<bool>>,
    }

    impl Component for FramedRegionRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            MouseRegion::new()
                .on_mouse_down({
                    let pressed = Rc::clone(&self.pressed);
                    Callback::new(move |_ev: MouseEvent| {
                        *pressed.borrow_mut() = true;
                    })
                })
                .child(
                    Frame::new()
                        .title(" hit ")
                        .border(true)
                        .width(Length::Px(11))
                        .height(Length::Px(5))
                        .child(Text::new("mole")),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let pressed = Rc::new(RefCell::new(false));
    let mut backend = TestBackend::new(FramedRegionRoot {
        pressed: Rc::clone(&pressed),
    });

    let frame_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Frame(_)))
        .map(|node| node.id)
        .expect("expected a frame node");
    let rect = backend.core.tree.node(frame_id).rect;
    let x = rect.x.saturating_add(1).max(0) as u16;
    let y = rect.y.max(0) as u16;

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        },
    );

    assert!(
        *pressed.borrow(),
        "MouseRegion should receive mouse-down on the wrapped Frame border"
    );
}

#[test]
fn mouse_region_with_only_mouse_down_receives_inner_content_press() {
    struct FramedRegionRoot {
        pressed: Rc<RefCell<bool>>,
    }

    impl Component for FramedRegionRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            MouseRegion::new()
                .on_mouse_down({
                    let pressed = Rc::clone(&self.pressed);
                    Callback::new(move |_ev: MouseEvent| {
                        *pressed.borrow_mut() = true;
                    })
                })
                .child(
                    Frame::new()
                        .title(" hit ")
                        .border(true)
                        .width(Length::Px(11))
                        .height(Length::Px(5))
                        .child(Text::new("mole")),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let pressed = Rc::new(RefCell::new(false));
    let mut backend = TestBackend::new(FramedRegionRoot {
        pressed: Rc::clone(&pressed),
    });

    let frame_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Frame(_)))
        .map(|node| node.id)
        .expect("expected a frame node");
    let rect = backend.core.tree.node(frame_id).rect;
    let x = rect.x.saturating_add(2).max(0) as u16;
    let y = rect.y.saturating_add(2).max(0) as u16;

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        },
    );

    assert!(
        *pressed.borrow(),
        "MouseRegion should receive mouse-down inside the wrapped Frame content"
    );
}

#[test]
fn mouse_region_mouse_up_fires_after_drag_threshold() {
    struct RegionRoot {
        released: Rc<RefCell<bool>>,
        clicked: Rc<RefCell<bool>>,
    }

    impl Component for RegionRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            MouseRegion::new()
                .capture_click(true)
                .on_mouse_up({
                    let released = Rc::clone(&self.released);
                    Callback::new(move |_ev: MouseEvent| {
                        *released.borrow_mut() = true;
                    })
                })
                .on_click({
                    let clicked = Rc::clone(&self.clicked);
                    Callback::new(move |_ev: MouseEvent| {
                        *clicked.borrow_mut() = true;
                    })
                })
                .child(Text::new("Click target"))
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let released = Rc::new(RefCell::new(false));
    let clicked = Rc::new(RefCell::new(false));
    let mut backend = TestBackend::new(RegionRoot {
        released: Rc::clone(&released),
        clicked: Rc::clone(&clicked),
    });

    let region_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::MouseRegion(_)))
        .map(|node| node.id)
        .expect("expected a MouseRegion node");
    let rect = backend.core.tree.node(region_id).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        },
    );
    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x: x + 5,
            y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        },
    );
    assert!(backend.mouse.drag_threshold_exceeded);

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x: x + 5,
            y,
            kind: MouseKind::Up(MouseButton::Left),
            mods: Default::default(),
        },
    );

    assert!(*released.borrow(), "mouse-up should fire after a drag");
    assert!(
        !*clicked.borrow(),
        "click should still require click semantics"
    );
}

#[test]
fn center_pin_slots_preserve_nested_component_state_across_mouse_rerender() {
    #[derive(Clone)]
    struct LogoProbeProps {
        pressed: Rc<RefCell<u32>>,
        clicked: Rc<RefCell<u32>>,
    }

    impl PartialEq for LogoProbeProps {
        fn eq(&self, other: &Self) -> bool {
            Rc::ptr_eq(&self.pressed, &other.pressed) && Rc::ptr_eq(&self.clicked, &other.clicked)
        }
    }

    struct LogoProbe;

    #[derive(Clone)]
    enum LogoProbeMsg {
        Move,
        Down,
        Click,
    }

    #[derive(Default)]
    struct LogoProbeState {
        moved: bool,
        pressed: bool,
    }

    impl Component for LogoProbe {
        type Message = LogoProbeMsg;
        type Properties = LogoProbeProps;
        type State = LogoProbeState;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            LogoProbeState::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let lines = if ctx.state.pressed {
                vec!["OPEN".to_string(), "CODE".to_string()]
            } else {
                vec!["open".to_string(), "code".to_string()]
            };
            let (_, mask) = CellMask::from_char_lines(&lines).expect("logo mask");
            let content: Element = EffectScope::new()
                .tint_by(
                    Color::hex("#558EDF"),
                    u8::from(ctx.state.pressed) as f32 * 0.5,
                )
                .child(AsciiCanvas::new(lines))
                .into();

            let region: Element = MouseRegion::new()
                .capture_click(true)
                .cell_mask(Arc::new(mask))
                .on_mouse_move(ctx.link().callback(|_: MouseMoveEvent| LogoProbeMsg::Move))
                .on_mouse_down(ctx.link().callback(|_: MouseEvent| LogoProbeMsg::Down))
                .on_click(ctx.link().callback(|_: MouseEvent| LogoProbeMsg::Click))
                .child(content)
                .into();
            region.key("logo-probe-region")
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                LogoProbeMsg::Move => {
                    ctx.state.moved = true;
                    Update::none()
                }
                LogoProbeMsg::Down => {
                    if ctx.state.moved {
                        *ctx.props.pressed.borrow_mut() += 1;
                    }
                    ctx.state.pressed = true;
                    Update::full()
                }
                LogoProbeMsg::Click => {
                    *ctx.props.clicked.borrow_mut() += 1;
                    Update::full()
                }
            }
        }
    }

    struct CenteredHomeShape {
        props: LogoProbeProps,
    }

    impl Component for CenteredHomeShape {
        type Message = ();
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let center: Element = CenterPin::new()
                .center(
                    VStack::new()
                        .align(Align::Center)
                        .child(
                            crate::child::<LogoProbe, _>(|| LogoProbe, self.props.clone())
                                .key("home-logo"),
                        )
                        .child(Spacer::new().height(Length::Px(2)))
                        .child(
                            VStack::new().align(Align::Center).child(
                                TextArea::bound(&ctx.state)
                                    .focusable(false)
                                    .border(false)
                                    .width(Length::Px(20))
                                    .height(Length::Auto),
                            ),
                        ),
                )
                .bottom(VStack::new().align(Align::Center))
                .into();

            VStack::new()
                .child(center.key("screen:home"))
                .child(
                    StatusBar::new()
                        .left(Text::new("~/Work/Projects/tui-lipan:main"))
                        .right(Text::new("1.1.53"))
                        .padding((0, 2, 1, 2)),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let pressed = Rc::new(RefCell::new(0));
    let clicked = Rc::new(RefCell::new(0));
    let props = LogoProbeProps {
        pressed: Rc::clone(&pressed),
        clicked: Rc::clone(&clicked),
    };
    let mut backend = TestBackend::new(CenteredHomeShape { props });

    let region_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::MouseRegion(_)))
        .map(|node| node.id)
        .expect("expected logo MouseRegion");
    let rect = backend.core.tree.node(region_id).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;

    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Moved,
            mods: Default::default(),
        })
        .unwrap();

    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    assert_eq!(*pressed.borrow(), 1, "mouse-down should reach logo");

    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Up(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    assert_eq!(
        *clicked.borrow(),
        1,
        "click should survive the dirty mouse-down rerender"
    );
}

#[test]
fn mouse_move_callback_with_no_update_does_not_report_dirty() {
    #[derive(Clone)]
    struct MoveProbe {
        moves: Rc<RefCell<u32>>,
    }

    impl Component for MoveProbe {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, ctx: &Context<Self>) -> Element {
            MouseRegion::new()
                .on_mouse_move(ctx.link().callback(|_: MouseMoveEvent| ()))
                .child(Text::new("hover me"))
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            *self.moves.borrow_mut() += 1;
            Update::none()
        }
    }

    let moves = Rc::new(RefCell::new(0));
    let mut backend = TestBackend::new(MoveProbe {
        moves: Rc::clone(&moves),
    });
    let region_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::MouseRegion(_)))
        .map(|node| node.id)
        .expect("expected MouseRegion");
    let rect = backend.core.tree.node(region_id).rect;

    let dirty = backend
        .send_mouse(MouseEvent {
            x: rect.x.max(0) as u16,
            y: rect.y.max(0) as u16,
            kind: MouseKind::Moved,
            mods: Default::default(),
        })
        .unwrap();

    assert_eq!(*moves.borrow(), 1, "move callback should still run");
    assert!(!dirty, "Update::none() move callback should not repaint");
}

#[test]
fn test_backend_textarea_drag_syncs_controlled_state() {
    struct EditorRoot;

    #[derive(Clone)]
    enum Msg {
        Changed(TextAreaEvent),
    }

    impl Component for EditorRoot {
        type Message = Msg;
        type Properties = ();
        type State = TextEditor;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            TextEditor::new("abcdef")
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state)
                .border(false)
                .padding(0)
                .focusable(false)
                .width(Length::Px(12))
                .height(Length::Px(1))
                .on_change(ctx.link().callback(Msg::Changed))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Changed(ev) => ev.apply_to(&mut ctx.state),
            }
            Update::full()
        }
    }

    let mut backend = TestBackend::new(EditorRoot);
    let text_area_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .expect("expected a textarea node");
    let rect = backend.core.tree.node(text_area_id).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;

    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: x + 4,
            y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();

    let NodeKind::TextArea(node) = &backend.core.tree.node(text_area_id).kind else {
        panic!("expected a textarea node");
    };
    assert_eq!(node.cursor, 4);
    assert_eq!(node.anchor, Some(0));
}

#[test]
fn textarea_click_drag_rebases_anchor_after_selection_scrolls_out_of_view() {
    struct EditorRoot;

    #[derive(Clone)]
    enum Msg {
        Changed(TextAreaEvent),
    }

    struct State {
        value: Arc<str>,
        cursor: usize,
        anchor: Option<usize>,
        scroll_offset: usize,
    }

    impl Component for EditorRoot {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State {
                value: Arc::from(
                    "row0\nrow1\nrow2\nrow3\nrow4\nrow5\nrow6\nrow7\nrow8\nrow9\nrow10\nrow11",
                ),
                cursor: 4,
                anchor: Some(0),
                scroll_offset: 8,
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::new(ctx.state.value.clone())
                .cursor(ctx.state.cursor)
                .anchor(ctx.state.anchor)
                .scroll_offset(ctx.state.scroll_offset)
                .border(false)
                .padding(0)
                .focusable(false)
                .wrap(false)
                .scrollbar(false)
                .width(Length::Px(12))
                .height(Length::Px(3))
                .on_change(ctx.link().callback(Msg::Changed))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Changed(ev) => {
                    ctx.state.value = ev.value;
                    ctx.state.cursor = ev.cursor;
                    ctx.state.anchor = ev.anchor;
                }
            }
            Update::full()
        }
    }

    let mut backend = TestBackend::new(EditorRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 12,
        h: 3,
    });
    backend.render();
    let text_area_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .expect("expected a textarea node");
    let rect = backend.core.tree.node(text_area_id).rect;
    let x = rect.x.saturating_add(1).max(0) as u16;
    let y = rect.y.max(0) as u16;
    let expected_click_cursor = "row0\nrow1\nrow2\nrow3\nrow4\nrow5\nrow6\nrow7\n".len() + 1;
    let NodeKind::TextArea(node) = &backend.core.tree.node(text_area_id).kind else {
        panic!("expected a textarea node");
    };
    assert_eq!(rect.h, 3);
    assert_eq!(node.visual_lines_count, 12);
    assert_eq!(node.scroll_offset, 8);

    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();

    match &backend.drag.active {
        ActiveDrag::TextArea(drag) => assert_eq!(drag.anchor, expected_click_cursor),
        other => panic!("expected textarea drag, got {other:?}"),
    }

    backend
        .send_mouse(MouseEvent {
            x: x + 3,
            y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();

    let NodeKind::TextArea(node) = &backend.core.tree.node(text_area_id).kind else {
        panic!("expected a textarea node");
    };
    assert_eq!(node.anchor, Some(expected_click_cursor));
    assert_eq!(node.cursor, expected_click_cursor + 3);
}

#[test]
fn textarea_vim_double_click_selection_enters_visual_mode() {
    struct EditorRoot;

    #[derive(Clone)]
    enum Msg {
        Edited(TextAreaEvent),
        Mode(TextAreaVimMode),
    }

    struct State {
        editor: TextEditor,
        modes: Vec<TextAreaVimMode>,
    }

    impl Component for EditorRoot {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State {
                editor: TextEditor::new("alpha beta"),
                modes: Vec::new(),
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state.editor)
                .border(false)
                .width(Length::Px(20))
                .height(Length::Px(1))
                .vim_motions(true)
                .on_change(ctx.link().callback(Msg::Edited))
                .on_vim_mode_change(ctx.link().callback(Msg::Mode))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Edited(event) => event.apply_to(&mut ctx.state.editor),
                Msg::Mode(mode) => ctx.state.modes.push(mode),
            }
            Update::full()
        }
    }

    let mut backend = TestBackend::new(EditorRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    });
    backend.render();
    let text_area_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .expect("expected a textarea node");
    backend.set_focused(text_area_id);

    for _ in 0..2 {
        backend
            .send_mouse(MouseEvent {
                x: 1,
                y: 0,
                kind: MouseKind::Down(MouseButton::Left),
                mods: Default::default(),
            })
            .unwrap();
    }

    assert_eq!(backend.state().editor.anchor(), Some(0));
    assert_eq!(backend.state().editor.cursor(), 5);
    assert_eq!(backend.state().modes.as_slice(), [TextAreaVimMode::Visual]);
    let state = backend.text_area_vim_state.get(&text_area_id).unwrap();
    assert_eq!(state.mode, TextAreaVimMode::Visual);
    assert_eq!(state.visual_anchor, Some(0));
}

#[test]
fn textarea_vim_mouse_selection_clears_pending_search_feedback() {
    struct EditorRoot;

    #[derive(Clone)]
    enum Msg {
        Edited(TextAreaEvent),
        Mode(TextAreaVimMode),
    }

    struct State {
        editor: TextEditor,
        modes: Vec<TextAreaVimMode>,
    }

    impl Component for EditorRoot {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State {
                editor: TextEditor::new("alpha beta"),
                modes: Vec::new(),
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            TextArea::bound(&ctx.state.editor)
                .border(false)
                .width(Length::Px(20))
                .height(Length::Px(1))
                .vim_motions(true)
                .on_change(ctx.link().callback(Msg::Edited))
                .on_vim_mode_change(ctx.link().callback(Msg::Mode))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Edited(event) => event.apply_to(&mut ctx.state.editor),
                Msg::Mode(mode) => ctx.state.modes.push(mode),
            }
            Update::full()
        }
    }

    let char_key = |ch| KeyEvent {
        code: KeyCode::Char(ch),
        mods: Default::default(),
    };
    let mut backend = TestBackend::new(EditorRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    });
    backend.render();
    let text_area_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .expect("expected a textarea node");
    backend.set_focused(text_area_id);

    backend
        .send_key(char_key('/'))
        .expect("search key succeeds");
    backend.send_key(char_key('a')).expect("query key succeeds");
    let NodeKind::TextArea(node) = &backend.core.tree.node(text_area_id).kind else {
        panic!("expected a textarea node");
    };
    assert!(
        node.vim_search_feedback
            .as_ref()
            .is_some_and(|feedback| feedback.pending)
    );

    for _ in 0..2 {
        backend
            .send_mouse(MouseEvent {
                x: 1,
                y: 0,
                kind: MouseKind::Down(MouseButton::Left),
                mods: Default::default(),
            })
            .unwrap();
    }

    assert_eq!(backend.state().editor.anchor(), Some(0));
    assert_eq!(backend.state().editor.cursor(), 5);
    assert_eq!(backend.state().modes.as_slice(), [TextAreaVimMode::Visual]);
    let state = backend.text_area_vim_state.get(&text_area_id).unwrap();
    assert_eq!(state.mode, TextAreaVimMode::Visual);
    assert_eq!(state.pending, None);
    let NodeKind::TextArea(node) = &backend.core.tree.node(text_area_id).kind else {
        panic!("expected a textarea node");
    };
    assert_eq!(node.vim_mode, TextAreaVimMode::Visual);
    assert!(node.vim_search_feedback.is_none());
}

#[test]
fn textarea_click_hold_rebases_after_selection_then_wheel_scroll() {
    struct EditorRoot;

    #[derive(Clone)]
    enum Msg {
        Edited(TextAreaEvent),
        Scrolled(ScrollEvent),
    }

    struct State {
        editor: TextEditor,
        editor_scroll: Option<usize>,
    }

    impl Component for EditorRoot {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            let text = (0..80)
                .map(|i| format!("row {i:02} text"))
                .collect::<Vec<_>>()
                .join("\n");
            State {
                editor: TextEditor::new(text),
                editor_scroll: None,
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let mut area = TextArea::bound(&ctx.state.editor)
                .line_numbers(true)
                .insert_tab(true)
                .wrap(false)
                .h_scrollbar(true)
                .scrollbar(true)
                .border(false)
                .height(Length::Flex(1))
                .width(Length::Flex(1))
                .on_change(ctx.link().callback(Msg::Edited))
                .on_scroll(ctx.link().callback(Msg::Scrolled));
            if let Some(offset) = ctx.state.editor_scroll {
                area = area.scroll_offset(offset);
            }
            Frame::new()
                .title("Editor")
                .border(true)
                .height(Length::Flex(1))
                .width(Length::Flex(1))
                .child(area.key("active-editor"))
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Edited(ev) => ev.apply_to(&mut ctx.state.editor),
                Msg::Scrolled(_ev) => ctx.state.editor_scroll = None,
            }
            Update::full()
        }
    }

    fn textarea_hit_point(backend: &TestBackend<EditorRoot>) -> (NodeId, u16, u16, u16) {
        let text_area_id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
            .map(|node| node.id)
            .expect("expected a textarea node");
        let node = backend.core.tree.node(text_area_id);
        let NodeKind::TextArea(text_area) = &node.kind else {
            unreachable!()
        };
        let inner = node.rect.inner(text_area.border, text_area.padding);
        let x = inner
            .x
            .saturating_add(text_area.geometry.gutter_width as i16)
            .saturating_add(1)
            .max(0) as u16;
        let top_y = inner.y.max(0) as u16;
        let content_h = text_area.geometry.content_viewport_h(false).max(1);
        let lower_y = inner
            .y
            .saturating_add(content_h.saturating_sub(4).max(1) as i16)
            .max(0) as u16;
        (text_area_id, x, top_y, lower_y)
    }

    let mut backend = TestBackend::new(EditorRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 48,
        h: 16,
    });
    backend.render();

    let (text_area_id, x, top_y, lower_y) = textarea_hit_point(&backend);
    let start_y = top_y.saturating_add(1);
    backend
        .send_mouse(MouseEvent {
            x,
            y: start_y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x,
            y: lower_y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x,
            y: lower_y,
            kind: MouseKind::Up(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    let old_cursor = backend.state().editor.cursor();

    for _ in 0..4 {
        backend
            .send_mouse(MouseEvent {
                x,
                y: lower_y,
                kind: MouseKind::ScrollDown,
                mods: Default::default(),
            })
            .unwrap();
    }
    assert_eq!(backend.drag.last_pointer_pos, None);
    backend.drag.last_pointer_pos = Some((x, lower_y));
    backend.drag.autoscroll_layout_dirty = true;

    let (_, x, top_y, _) = textarea_hit_point(&backend);
    backend
        .send_mouse(MouseEvent {
            x,
            y: top_y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();

    let NodeKind::TextArea(node) = &backend.core.tree.node(text_area_id).kind else {
        panic!("expected a textarea node");
    };
    let clicked_cursor = node.cursor;
    assert_ne!(clicked_cursor, old_cursor);
    assert_eq!(node.anchor, None);
    assert_eq!(backend.drag.last_pointer_pos, None);
    assert!(!backend.drag.autoscroll_layout_dirty);
    match &backend.drag.active {
        ActiveDrag::TextArea(drag) => assert_eq!(drag.anchor, clicked_cursor),
        other => panic!("expected textarea drag, got {other:?}"),
    }

    backend
        .send_mouse(MouseEvent {
            x,
            y: top_y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();

    let NodeKind::TextArea(node) = &backend.core.tree.node(text_area_id).kind else {
        panic!("expected a textarea node");
    };
    assert_eq!(node.cursor, clicked_cursor);
    assert_eq!(node.anchor, Some(clicked_cursor));
}

#[test]
fn scroll_wheel_survives_keyed_scroll_view_remount() {
    struct RemountingScrollRoot;

    #[derive(Default)]
    struct State {
        offset: usize,
        remount_epoch: usize,
    }

    #[derive(Clone, Debug)]
    enum Msg {
        Scroll(ScrollEvent),
    }

    impl Component for RemountingScrollRoot {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let scroll = ScrollView::new()
                .border(true)
                .scrollbar(true)
                .offset(ctx.state.offset)
                .on_scroll(ctx.link().callback(Msg::Scroll))
                .children(
                    (0..80).map(|i| Text::new(format!("row {i}")).height(Length::Px(1)).into()),
                )
                .key("timeline-scroll");

            Frame::new()
                .child(scroll)
                .key(format!("host-{}", ctx.state.remount_epoch))
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Scroll(ev) => {
                    ctx.state.offset = ev.offset;
                    ctx.state.remount_epoch += 1;
                }
            }
            Update::full()
        }
    }

    let mut backend = TestBackend::new(RemountingScrollRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 20,
    });
    backend.render();

    let scroll_key = Key::from("timeline-scroll");
    let rect = backend
        .core
        .tree
        .iter()
        .find(|node| node.key.as_ref() == Some(&scroll_key))
        .map(|node| node.rect)
        .expect("expected keyed scroll view");
    let x = rect.x.saturating_add(2).max(0) as u16;
    let y = rect.y.saturating_add(2).max(0) as u16;

    for expected in 1..=3 {
        backend
            .send_mouse(MouseEvent {
                x,
                y,
                kind: MouseKind::ScrollDown,
                mods: Default::default(),
            })
            .unwrap();
        assert_eq!(backend.state().offset, expected);
    }
}

#[test]
fn document_view_scroll_wheel_false_bubbles_to_parent_scroll_view() {
    struct NestedDocRoot;

    #[derive(Default)]
    struct State {
        offset: usize,
    }

    #[derive(Clone, Debug)]
    enum Msg {
        Scroll(ScrollEvent),
    }

    impl Component for NestedDocRoot {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let long_doc = (0..40)
                .map(|i| format!("inner line {i}"))
                .collect::<Vec<_>>()
                .join("\n");

            ScrollView::new()
                .offset(ctx.state.offset)
                .on_scroll(ctx.link().callback(Msg::Scroll))
                .children(
                    std::iter::once(
                        DocumentView::new(long_doc)
                            .height(Length::Px(4))
                            .scrollbar(true)
                            .scroll_wheel(false)
                            .key("inner-doc"),
                    )
                    .chain((0..40).map(|i| {
                        Text::new(format!("outer row {i}"))
                            .height(Length::Px(1))
                            .into()
                    })),
                )
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Scroll(ev) => ctx.state.offset = ev.offset,
            }
            Update::full()
        }
    }

    let mut backend = TestBackend::new(NestedDocRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 10,
    });
    backend.render();

    let doc_key = Key::from("inner-doc");
    let rect = backend
        .core
        .tree
        .iter()
        .find(|node| node.key.as_ref() == Some(&doc_key))
        .map(|node| node.rect)
        .expect("expected inner document view");

    backend
        .send_mouse(MouseEvent {
            x: rect.x.saturating_add(1).max(0) as u16,
            y: rect.y.saturating_add(1).max(0) as u16,
            kind: MouseKind::ScrollDown,
            mods: Default::default(),
        })
        .unwrap();

    assert_eq!(backend.state().offset, 1);
}

#[test]
fn unfocusable_unscrollable_document_view_selects_text_in_scroll_view() {
    struct UnfocusableDocRoot;

    impl Component for UnfocusableDocRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ScrollView::new()
                .children([
                    DocumentView::new("abcdef")
                        .border(false)
                        .scrollbar(false)
                        .h_scrollbar(false)
                        .scroll_wheel(false)
                        .focusable(false)
                        .height(Length::Auto)
                        .key("doc"),
                    Text::new("tail").height(Length::Px(1)).into(),
                ])
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let mut backend = TestBackend::new(UnfocusableDocRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 5,
    });
    backend.render();

    let doc_key = Key::from("doc");
    let doc_id = backend
        .core
        .tree
        .iter()
        .find(|node| node.key.as_ref() == Some(&doc_key))
        .map(|node| node.id)
        .expect("expected document view");
    let rect = backend.core.tree.node(doc_id).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;

    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: x + 4,
            y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();

    let NodeKind::DocumentView(doc) = &backend.core.tree.node(doc_id).kind else {
        panic!("expected document view");
    };
    assert_eq!(doc.selection_anchor, Some(0));
    assert_eq!(doc.selection_cursor, 4);
}

#[test]
fn document_view_passthrough_with_on_click_keeps_links_and_bubbles_plain_clicks() {
    struct PassthroughDocRoot;

    #[derive(Clone)]
    struct LinkFormatter;

    impl ContentFormatter for LinkFormatter {
        fn format(&self, _input: FormatInput<'_>) -> FormattedDocument {
            FormattedDocument {
                blocks: vec![FormattedBlock::Lines(vec![FormattedLine {
                    spans: vec![Span::new("plain link")],
                    source_line: 0,
                    indent: 0,
                    links: vec![FormattedLink {
                        start: 6,
                        end: 10,
                        url: Arc::from("https://example.test"),
                    }],
                }])],
            }
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }

        fn clone_box(&self) -> Box<dyn ContentFormatter> {
            Box::new(self.clone())
        }
    }

    #[derive(Default)]
    struct State {
        document_clicks: usize,
        region_clicks: usize,
        last_link: Option<Arc<str>>,
    }

    #[derive(Clone, Debug)]
    enum Msg {
        Document(DocumentClickEvent),
        Region,
    }

    impl Component for PassthroughDocRoot {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            MouseRegion::new()
                .on_click(ctx.link().callback(|_: MouseEvent| Msg::Region))
                .child(
                    DocumentView::new("plain link")
                        .formatter(LinkFormatter)
                        .border(false)
                        .scrollbar(false)
                        .h_scrollbar(false)
                        .scroll_wheel(false)
                        .focusable(false)
                        .height(Length::Auto)
                        .passthrough_clicks(true)
                        .on_click(ctx.link().callback(Msg::Document))
                        .key("doc"),
                )
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Document(ev) => {
                    ctx.state.document_clicks += 1;
                    ctx.state.last_link = ev.link;
                }
                Msg::Region => ctx.state.region_clicks += 1,
            }
            Update::full()
        }
    }

    fn click<C: Component>(backend: &mut TestBackend<C>, x: u16, y: u16) {
        backend
            .send_mouse(MouseEvent {
                x,
                y,
                kind: MouseKind::Down(MouseButton::Left),
                mods: Default::default(),
            })
            .unwrap();
        backend
            .send_mouse(MouseEvent {
                x,
                y,
                kind: MouseKind::Up(MouseButton::Left),
                mods: Default::default(),
            })
            .unwrap();
    }

    let mut backend = TestBackend::new(PassthroughDocRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 5,
    });
    backend.render();

    let doc_key = Key::from("doc");
    let rect = backend
        .core
        .tree
        .iter()
        .find(|node| node.key.as_ref() == Some(&doc_key))
        .map(|node| node.rect)
        .expect("expected document view");
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;

    click(&mut backend, x, y);
    assert_eq!(backend.state().region_clicks, 1);
    assert_eq!(backend.state().document_clicks, 0);

    click(&mut backend, x + 6, y);
    assert_eq!(backend.state().region_clicks, 1);
    assert_eq!(backend.state().document_clicks, 1);
    assert_eq!(
        backend.state().last_link.as_deref(),
        Some("https://example.test")
    );
}

#[test]
fn unscrollable_document_view_wheel_bubbles_to_parent_scroll_view() {
    struct UnscrollableDocWheelRoot;

    #[derive(Default)]
    struct State {
        offset: usize,
    }

    #[derive(Clone, Debug)]
    enum Msg {
        Scroll(ScrollEvent),
    }

    impl Component for UnscrollableDocWheelRoot {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            ScrollView::new()
                .offset(ctx.state.offset)
                .on_scroll(ctx.link().callback(Msg::Scroll))
                .children(
                    std::iter::once(
                        DocumentView::new("abcdef")
                            .border(false)
                            .scrollbar(false)
                            .h_scrollbar(false)
                            .scroll_wheel(true)
                            .focusable(false)
                            .height(Length::Auto)
                            .key("doc"),
                    )
                    .chain((0..20).map(|i| {
                        Text::new(format!("outer row {i}"))
                            .height(Length::Px(1))
                            .into()
                    })),
                )
                .into()
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Scroll(ev) => ctx.state.offset = ev.offset,
            }
            Update::full()
        }
    }

    let mut backend = TestBackend::new(UnscrollableDocWheelRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 30,
        h: 5,
    });
    backend.render();

    let doc_key = Key::from("doc");
    let rect = backend
        .core
        .tree
        .iter()
        .find(|node| node.key.as_ref() == Some(&doc_key))
        .map(|node| node.rect)
        .expect("expected document view");

    backend
        .send_mouse(MouseEvent {
            x: rect.x.max(0) as u16,
            y: rect.y.max(0) as u16,
            kind: MouseKind::ScrollDown,
            mods: Default::default(),
        })
        .unwrap();

    assert_eq!(backend.state().offset, 1);
}

#[cfg(feature = "diff-view")]
#[test]
fn unfocusable_document_backend_diff_view_selects_text_in_scroll_view() {
    struct DiffDocRoot;

    impl Component for DiffDocRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ScrollView::new()
                .child(
                    crate::widgets::DiffView::with_content("fn old() {}\n", "fn new() {}\n")
                        .backend(crate::widgets::DiffViewBackend::DocumentView)
                        .document_view(DocumentView::new("").scroll_wheel(false))
                        .mode(crate::widgets::DiffViewMode::Unified)
                        .line_numbers(false)
                        .wrap(true)
                        .scrollbar(false)
                        .h_scrollbar(false)
                        .focusable(false)
                        .border(false)
                        .panels_border(false)
                        .height(Length::Auto),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let mut backend = TestBackend::new(DiffDocRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 6,
    });
    backend.render();

    let doc_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DocumentView(_)))
        .map(|node| node.id)
        .expect("expected diff document pane");
    let rect = backend.core.tree.node(doc_id).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;

    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: x + 4,
            y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();

    let NodeKind::DocumentView(doc) = &backend.core.tree.node(doc_id).kind else {
        panic!("expected document view");
    };
    assert_eq!(doc.selection_anchor, Some(0));
    assert!(doc.selection_cursor > 0);
}

#[cfg(feature = "diff-view")]
#[test]
fn split_document_backend_diff_view_drag_selects_across_panes() {
    struct SplitDiffDocRoot;

    impl Component for SplitDiffDocRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            crate::widgets::DiffView::with_content(
                "fn old() {}\nlet before = 1;\n",
                "fn new() {}\nlet after = 2;\n",
            )
            .backend(crate::widgets::DiffViewBackend::DocumentView)
            .document_view(DocumentView::new("").scroll_wheel(false))
            .mode(crate::widgets::DiffViewMode::Split)
            .line_numbers(false)
            .wrap(true)
            .scrollbar(false)
            .h_scrollbar(false)
            .focusable(false)
            .border(false)
            .panels_border(false)
            .height(Length::Auto)
            .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let mut backend = TestBackend::new(SplitDiffDocRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 6,
    });
    backend.render();

    let mut left_id = None;
    let mut right_id = None;
    for node in backend.core.tree.iter() {
        let NodeKind::DocumentView(doc) = &node.kind else {
            continue;
        };
        match doc.diff_split_pane {
            Some(crate::widgets::DiffPane::Left) => left_id = Some(node.id),
            Some(crate::widgets::DiffPane::Right) => right_id = Some(node.id),
            _ => {}
        }
    }
    let left_id = left_id.expect("left diff pane exists");
    let right_id = right_id.expect("right diff pane exists");
    let left_rect = backend.core.tree.node(left_id).rect;
    let right_rect = backend.core.tree.node(right_id).rect;
    let y = right_rect.y.max(0) as u16;

    backend
        .send_mouse(MouseEvent {
            x: right_rect.x.saturating_add(2).max(0) as u16,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    match &backend.drag.active {
        ActiveDrag::DocumentView(drag) => {
            assert!(backend.core.tree.is_valid(drag.id));
        }
        other => panic!("expected document view drag, got {other:?}"),
    }
    backend
        .send_mouse(MouseEvent {
            x: left_rect.x.saturating_add(1).max(0) as u16,
            y,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();

    let NodeKind::DocumentView(left) = &backend.core.tree.node(left_id).kind else {
        panic!("expected left document view");
    };
    let NodeKind::DocumentView(right) = &backend.core.tree.node(right_id).kind else {
        panic!("expected right document view");
    };
    assert!(left.selection_anchor.is_some());
    assert!(right.selection_anchor.is_some());
    assert_eq!(left.selection_cursor, left.visual_cache.line_lengths[0]);
    assert_eq!(right.selection_cursor, right.visual_cache.line_lengths[0]);
}

#[cfg(feature = "diff-view")]
#[test]
fn shared_document_drag_enters_split_diff_view_pane() {
    struct SharedDocIntoDiffRoot;

    impl Component for SharedDocIntoDiffRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ScrollView::new()
                .children([
                    DocumentView::new("alpha beta gamma")
                        .border(false)
                        .scrollbar(false)
                        .h_scrollbar(false)
                        .scroll_wheel(false)
                        .focusable(false)
                        .shared_selection_id("shared")
                        .height(Length::Auto)
                        .key("doc-above"),
                    crate::widgets::DiffView::with_content(
                        "fn old() {}\nlet before = 1;\n",
                        "fn new() {}\nlet after = 2;\n",
                    )
                    .backend(crate::widgets::DiffViewBackend::DocumentView)
                    .document_view(DocumentView::new("").scroll_wheel(false))
                    .shared_selection_id("shared")
                    .mode(crate::widgets::DiffViewMode::Split)
                    .line_numbers(false)
                    .wrap(true)
                    .scrollbar(false)
                    .h_scrollbar(false)
                    .focusable(false)
                    .border(false)
                    .panels_border(false)
                    .height(Length::Auto)
                    .into(),
                ])
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let mut backend = TestBackend::new(SharedDocIntoDiffRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 10,
    });
    backend.render();

    let above_key = Key::from("doc-above");
    let above_id = backend
        .core
        .tree
        .iter()
        .find(|node| node.key.as_ref() == Some(&above_key))
        .map(|node| node.id)
        .expect("shared document above diff exists");

    let mut left_id = None;
    for node in backend.core.tree.iter() {
        let NodeKind::DocumentView(doc) = &node.kind else {
            continue;
        };
        if matches!(doc.diff_split_pane, Some(crate::widgets::DiffPane::Left)) {
            left_id = Some(node.id);
            break;
        }
    }
    let left_id = left_id.expect("left diff pane exists");

    let above_rect = backend.core.tree.node(above_id).rect;
    let left_rect = backend.core.tree.node(left_id).rect;
    let y_above = above_rect.y.max(0) as u16;
    let x_left = left_rect.x.saturating_add((left_rect.w / 2) as i16).max(0) as u16;
    let y_left = left_rect
        .y
        .saturating_add((left_rect.h.saturating_sub(1) / 2) as i16)
        .max(0) as u16;

    backend
        .send_mouse(MouseEvent {
            x: above_rect.x.saturating_add(1).max(0) as u16,
            y: y_above,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    backend
        .send_mouse(MouseEvent {
            x: x_left,
            y: y_left,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();

    let left_id_after = backend
        .core
        .tree
        .iter()
        .find_map(|node| {
            let NodeKind::DocumentView(doc) = &node.kind else {
                return None;
            };
            matches!(doc.diff_split_pane, Some(crate::widgets::DiffPane::Left)).then_some(node.id)
        })
        .expect("left diff pane exists after drag");
    let NodeKind::DocumentView(left) = &backend.core.tree.node(left_id_after).kind else {
        panic!("expected left document view");
    };
    assert!(
        left.selection_anchor.is_some(),
        "dragging from shared document into split diff should select left pane"
    );
}

#[test]
fn scrollbar_drag_survives_keyed_scroll_view_remount() {
    struct RemountingScrollRoot;

    #[derive(Default)]
    struct State {
        offset: usize,
        remount_epoch: usize,
    }

    #[derive(Clone, Debug)]
    enum Msg {
        Scroll(ScrollEvent),
    }

    impl Component for RemountingScrollRoot {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let scroll = ScrollView::new()
                .border(true)
                .scrollbar(true)
                .offset(ctx.state.offset)
                .on_scroll(ctx.link().callback(Msg::Scroll))
                .children(
                    (0..80).map(|i| Text::new(format!("row {i}")).height(Length::Px(1)).into()),
                )
                .key("timeline-scroll");

            Frame::new()
                .child(scroll)
                .key(format!("host-{}", ctx.state.remount_epoch))
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                Msg::Scroll(ev) => {
                    ctx.state.offset = ev.offset;
                    ctx.state.remount_epoch += 1;
                }
            }
            Update::full()
        }
    }

    let mut backend = TestBackend::new(RemountingScrollRoot);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 60,
        h: 20,
    });
    backend.render();

    let scroll_key = Key::from("timeline-scroll");
    let scroll_id = backend
        .core
        .tree
        .iter()
        .find(|node| node.key.as_ref() == Some(&scroll_key))
        .map(|node| node.id)
        .expect("expected keyed scroll view");
    let zone =
        crate::core::node::scrollbar_zones(&backend.core.tree, backend.core.tree.node(scroll_id))
            .into_iter()
            .next()
            .expect("expected scrollbar zone")
            .rect;

    backend
        .send_mouse(MouseEvent {
            x: zone.x as u16,
            y: zone.y as u16,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();
    assert!(
        !backend.core.tree.is_valid(scroll_id),
        "test setup should remount the original scroll node"
    );

    backend
        .send_mouse(MouseEvent {
            x: zone.x as u16,
            y: zone.y.saturating_add(zone.h.saturating_sub(2) as i16) as u16,
            kind: MouseKind::Drag(MouseButton::Left),
            mods: Default::default(),
        })
        .unwrap();

    assert!(
        backend.state().offset > 0,
        "drag should rebind to the remounted keyed ScrollView"
    );
}

#[test]
fn document_view_shared_selection_cleared_on_click() {
    struct SharedDocRoot;

    impl Component for SharedDocRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            ScrollView::new()
                .children([
                    DocumentView::new("alpha")
                        .focusable(false)
                        .shared_selection_id("shared")
                        .into(),
                    DocumentView::new("beta")
                        .focusable(false)
                        .shared_selection_id("shared")
                        .into(),
                ])
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let mut backend = TestBackend::new(SharedDocRoot);

    let doc_ids: Vec<_> = backend
        .core
        .tree
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::DocumentView(_)))
        .map(|node| node.id)
        .collect();
    assert_eq!(doc_ids.len(), 2);

    let first = doc_ids[0];
    let second = doc_ids[1];

    // Set selection on the first document view.
    if let NodeKind::DocumentView(doc) = &mut backend.core.tree.node_mut(first).kind {
        doc.selection_cursor = doc.visual_cache.flat_text.len();
        doc.selection_anchor = Some(0);
    }

    // Click inside the second document view.
    let rect = backend.core.tree.node(second).rect;
    let x = rect.x as u16;
    let y = rect.y as u16;

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x,
            y,
            kind: MouseKind::Down(MouseButton::Left),
            mods: Default::default(),
        },
    );

    // The first document view's selection should have been cleared.
    if let NodeKind::DocumentView(doc) = &backend.core.tree.node(first).kind {
        assert!(
            doc.selection_anchor.is_none(),
            "shared selection anchor should be cleared"
        );
        assert!(
            doc.table_rect_selection.is_none(),
            "shared table rect selection should be cleared"
        );
    } else {
        panic!("expected DocumentView");
    }
}
