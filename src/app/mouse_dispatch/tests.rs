use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use super::*;
use crate::app::interaction_state::{DragState, MouseRegionDragState, MouseTrackingState};
use crate::callback::Callback;
use crate::clipboard::{
    ClipboardConfig, ClipboardError, ClipboardProvider, CopyOnSelect, PasteSource, RightClickAction,
};
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
    DraggableTab, DraggableTabBar, DraggableTabCloseEvent, EffectScope, FormatInput,
    FormattedBlock, FormattedDocument, FormattedLine, Frame, HStack, List, ListItem, MouseRegion,
    ScrollEvent, ScrollView, Spacer, StatusBar, Table, TableRow, TabsEvent, Text, TextArea,
    TextAreaEvent, TextAreaVimMode, VStack,
};
#[cfg(feature = "terminal")]
use crate::widgets::{MouseEncoding, MouseMode, MouseModeState, Terminal, TerminalRenderSnapshot};
use crate::{CellMask, TextEditor};

#[derive(Default)]
struct MouseClipboardState {
    clipboard: String,
    primary: String,
    clipboard_reads: usize,
    fail_clipboard_writes: bool,
}

struct MouseClipboardProvider(Rc<RefCell<MouseClipboardState>>);

impl ClipboardProvider for MouseClipboardProvider {
    fn read_clipboard_text(&mut self) -> Result<String, ClipboardError> {
        let mut state = self.0.borrow_mut();
        state.clipboard_reads += 1;
        Ok(state.clipboard.clone())
    }

    fn write_clipboard_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        let mut state = self.0.borrow_mut();
        if state.fail_clipboard_writes {
            return Err(ClipboardError::Unsupported {
                operation: crate::clipboard::error::ClipboardOperation::WriteClipboard,
            });
        }
        state.clipboard = text.to_string();
        Ok(())
    }

    fn read_primary_selection_text(&mut self) -> Result<String, ClipboardError> {
        Ok(self.0.borrow().primary.clone())
    }

    fn write_primary_selection_text(&mut self, text: &str) -> Result<(), ClipboardError> {
        self.0.borrow_mut().primary = text.to_string();
        Ok(())
    }

    fn supports_primary_selection(&self) -> bool {
        true
    }
}

struct ClipboardEditor(&'static str);

impl Component for ClipboardEditor {
    type Message = TextAreaEvent;
    type Properties = ();
    type State = TextEditor;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        TextEditor::new(self.0)
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        TextArea::bound(&ctx.state)
            .border(false)
            .padding(0)
            .width(Length::Px(20))
            .height(Length::Px(1))
            .on_change(ctx.link().callback(|event| event))
            .into()
    }

    fn update(&mut self, event: Self::Message, ctx: &mut Context<Self>) -> Update {
        event.apply_to(&mut ctx.state);
        Update::full()
    }
}

fn mouse_clipboard_backend(
    initial_text: &'static str,
    state: Rc<RefCell<MouseClipboardState>>,
    config: ClipboardConfig,
) -> TestBackend<ClipboardEditor> {
    let app = crate::App::new()
        .clipboard_provider(MouseClipboardProvider(state))
        .clipboard_config(config);
    let mut backend = TestBackend::new_with_app(app, ClipboardEditor(initial_text), ());
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 1,
    });
    backend.render();
    backend
}

fn text_area_id(backend: &TestBackend<ClipboardEditor>) -> NodeId {
    backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .expect("expected TextArea")
}

fn click_left(backend: &mut TestBackend<ClipboardEditor>, x: u16, y: u16) {
    for kind in [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        backend
            .send_mouse(MouseEvent {
                x,
                y,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }
}

/// A row of non-editable chrome above two side-by-side editors, for pointer-targeting tests.
struct PairedEditors(&'static str);

impl Component for PairedEditors {
    type Message = (usize, TextAreaEvent);
    type Properties = ();
    type State = [TextEditor; 2];

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        [TextEditor::new(self.0), TextEditor::new("")]
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let editor = |index: usize| {
            TextArea::bound(&ctx.state[index])
                .border(false)
                .padding(0)
                .width(Length::Px(10))
                .height(Length::Px(1))
                .on_change(ctx.link().callback(move |event| (index, event)))
        };
        VStack::new()
            .child(
                MouseRegion::new()
                    .on_mouse_down(Callback::new(|_| {}))
                    .child(Text::new("chrome").height(Length::Px(1))),
            )
            .child(
                HStack::new()
                    .height(Length::Px(1))
                    .child(editor(0))
                    .child(editor(1)),
            )
            .into()
    }

    fn update(&mut self, (index, event): Self::Message, ctx: &mut Context<Self>) -> Update {
        event.apply_to(&mut ctx.state[index]);
        Update::full()
    }
}

fn paired_editors_backend(
    left_text: &'static str,
    state: Rc<RefCell<MouseClipboardState>>,
    config: ClipboardConfig,
) -> TestBackend<PairedEditors> {
    let app = crate::App::new()
        .clipboard_provider(MouseClipboardProvider(state))
        .clipboard_config(config);
    let mut backend = TestBackend::new_with_app(app, PairedEditors(left_text), ());
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 2,
    });
    backend.render();
    backend
}

/// The left and right editors, in that order.
fn paired_editor_ids(backend: &TestBackend<PairedEditors>) -> [NodeId; 2] {
    let mut ids: Vec<NodeId> = backend
        .core
        .tree
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::TextArea(_)))
        .map(|node| node.id)
        .collect();
    ids.sort_by_key(|id| backend.core.tree.node(*id).rect.x);
    ids.try_into().expect("expected two TextAreas")
}

fn press<C: Component>(backend: &mut TestBackend<C>, button: MouseButton, x: u16, y: u16) {
    backend
        .send_mouse(MouseEvent {
            x,
            y,
            kind: MouseKind::Down(button),
            mods: KeyMods::NONE,
        })
        .unwrap();
}

fn node_origin<C: Component>(backend: &TestBackend<C>, id: NodeId) -> (u16, u16) {
    let rect = backend.core.tree.node(id).rect;
    (rect.x.max(0) as u16, rect.y.max(0) as u16)
}

/// Drag-select the first five cells of the editor, then press the right button.
fn select_then_right_click(backend: &mut TestBackend<ClipboardEditor>) {
    let rect = backend.core.tree.node(text_area_id(backend)).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;
    for (event_x, kind) in [
        (x, MouseKind::Down(MouseButton::Left)),
        (x + 5, MouseKind::Drag(MouseButton::Left)),
        (x + 5, MouseKind::Up(MouseButton::Left)),
        (x + 2, MouseKind::Down(MouseButton::Right)),
    ] {
        backend
            .send_mouse(MouseEvent {
                x: event_x,
                y,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }
}

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

    fn reset_command_chord(&mut self) -> bool {
        false
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
        _y: u16,
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
                    Callback::new(move |_ev: crate::core::event::MouseRegionEvent| {
                        *clicked.borrow_mut() = true;
                    })
                })
                .child(
                    Frame::new()
                        .header_left("hit")
                        .header_padding(1)
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
    let target = mouse::resolve_left_click_target(&backend.core.tree, hit, x, y, KeyMods::NONE);
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
        mouse::gather_hit_actions(&backend.core.tree, target, x, y, KeyMods::NONE)
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
                    Callback::new(move |_ev: crate::core::event::MouseRegionEvent| {
                        *pressed.borrow_mut() = true;
                    })
                })
                .child(
                    Frame::new()
                        .header_left("hit")
                        .header_padding(1)
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
                    Callback::new(move |_ev: crate::core::event::MouseRegionEvent| {
                        *pressed.borrow_mut() = true;
                    })
                })
                .child(
                    Frame::new()
                        .header_left("hit")
                        .header_padding(1)
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
                    Callback::new(move |_ev: crate::core::event::MouseRegionEvent| {
                        *released.borrow_mut() = true;
                    })
                })
                .on_click({
                    let clicked = Rc::clone(&self.clicked);
                    Callback::new(move |_ev: crate::core::event::MouseRegionEvent| {
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
                .on_mouse_down(
                    ctx.link()
                        .callback(|_: crate::core::event::MouseRegionEvent| LogoProbeMsg::Down),
                )
                .on_click(
                    ctx.link()
                        .callback(|_: crate::core::event::MouseRegionEvent| LogoProbeMsg::Click),
                )
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
    let state = backend
        .widgets
        .text_area_vim_state
        .get(&text_area_id)
        .unwrap();
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
    let state = backend
        .widgets
        .text_area_vim_state
        .get(&text_area_id)
        .unwrap();
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
                .header_left("Editor")
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
                .on_click(
                    ctx.link()
                        .callback(|_: crate::core::event::MouseRegionEvent| Msg::Region),
                )
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

#[test]
fn list_activation_without_selection_keeps_wrapper_hover() {
    struct ActivatableListRegion {
        activated: Rc<RefCell<Vec<usize>>>,
        item_clicked: Rc<RefCell<Vec<usize>>>,
        hover_changes: Rc<RefCell<Vec<bool>>>,
    }

    impl Component for ActivatableListRegion {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let activated = Rc::clone(&self.activated);
            let item_clicked = Rc::clone(&self.item_clicked);
            let hover_changes = Rc::clone(&self.hover_changes);
            MouseRegion::new()
                .on_hover_change(Callback::new(move |hovered| {
                    hover_changes.borrow_mut().push(hovered);
                }))
                .child(
                    List::new()
                        .items([ListItem::new("one"), ListItem::new("two")])
                        .on_item_click(Callback::new(move |event: crate::widgets::ListEvent| {
                            item_clicked.borrow_mut().push(event.index);
                        }))
                        .on_activate(Callback::new(move |event: crate::widgets::ListEvent| {
                            activated.borrow_mut().push(event.index);
                        }))
                        .height(Length::Px(2)),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let activated = Rc::new(RefCell::new(Vec::new()));
    let item_clicked = Rc::new(RefCell::new(Vec::new()));
    let hover_changes = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(ActivatableListRegion {
        activated: Rc::clone(&activated),
        item_clicked: Rc::clone(&item_clicked),
        hover_changes: Rc::clone(&hover_changes),
    });

    assert!(update_hover_test_backend(&mut backend, 0, 0, false));
    let hovered = backend.mouse.hovered.expect("wrapper should be hovered");
    assert!(matches!(
        backend.core.tree.node(hovered).kind,
        NodeKind::MouseRegion(_)
    ));
    assert_eq!(&*hover_changes.borrow(), &[true]);

    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x: 0,
            y: 0,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::NONE,
        },
    );
    dispatch_mouse_test_backend(
        &mut backend,
        MouseEvent {
            x: 0,
            y: 0,
            kind: MouseKind::Up(MouseButton::Left),
            mods: KeyMods::NONE,
        },
    );

    assert_eq!(&*activated.borrow(), &[0]);
    assert_eq!(&*item_clicked.borrow(), &[0]);
    assert_eq!(backend.mouse.hovered, Some(hovered));
    assert_eq!(&*hover_changes.borrow(), &[true]);
}

#[test]
fn table_activation_does_not_need_selection_callback() {
    struct ActivatableTable {
        activated: Rc<RefCell<Vec<usize>>>,
    }

    impl Component for ActivatableTable {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let activated = Rc::clone(&self.activated);
            Table::new()
                .rows([TableRow::new(["one"])])
                .on_activate(Callback::new(move |event: crate::widgets::TableEvent| {
                    activated.borrow_mut().push(event.index);
                }))
                .height(Length::Px(1))
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let activated = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(ActivatableTable {
        activated: Rc::clone(&activated),
    });
    for _ in 0..2 {
        dispatch_mouse_test_backend(
            &mut backend,
            MouseEvent {
                x: 0,
                y: 0,
                kind: MouseKind::Down(MouseButton::Left),
                mods: KeyMods::NONE,
            },
        );
        dispatch_mouse_test_backend(
            &mut backend,
            MouseEvent {
                x: 0,
                y: 0,
                kind: MouseKind::Up(MouseButton::Left),
                mods: KeyMods::NONE,
            },
        );
    }

    assert_eq!(&*activated.borrow(), &[0]);
}

#[test]
fn mouse_region_hover_includes_interactive_descendants() {
    struct ButtonRegion {
        hover_changes: Rc<RefCell<Vec<bool>>>,
    }

    impl Component for ButtonRegion {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let hover_changes = Rc::clone(&self.hover_changes);
            MouseRegion::new()
                .on_hover_change(Callback::new(move |hovered| {
                    hover_changes.borrow_mut().push(hovered);
                }))
                .child(
                    Button::new("go")
                        .width(Length::Px(4))
                        .on_click(Callback::new(|_| {})),
                )
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let hover_changes = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(ButtonRegion {
        hover_changes: Rc::clone(&hover_changes),
    });

    assert!(update_hover_test_backend(&mut backend, 0, 0, false));
    let hovered = backend.mouse.hovered.expect("button should be hovered");
    assert!(matches!(
        backend.core.tree.node(hovered).kind,
        NodeKind::Button(_)
    ));
    assert_eq!(&*hover_changes.borrow(), &[true]);

    let region_rect = backend.core.tree.node(backend.core.tree.root).rect;
    let outside_x = region_rect.x.saturating_add(region_rect.w as i16).max(0) as u16;
    assert!(update_hover_test_backend(&mut backend, outside_x, 0, false));
    assert_eq!(&*hover_changes.borrow(), &[true, false]);
}

#[test]
fn completed_mouse_selection_updates_primary_without_overwriting_clipboard() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "keep me".to_string(),
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        enable_primary_selection: true,
        enable_osc52: false,
        copy_on_mouse_select: CopyOnSelect::PrimarySelection,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("alpha beta", Rc::clone(&clipboard), config);
    let id = text_area_id(&backend);
    let rect = backend.core.tree.node(id).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;

    for kind in [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Drag(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        let event_x = if matches!(kind, MouseKind::Down(_)) {
            x
        } else {
            x + 5
        };
        backend
            .send_mouse(MouseEvent {
                x: event_x,
                y,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }

    let clipboard = clipboard.borrow();
    assert_eq!(clipboard.primary, "alpha");
    assert_eq!(clipboard.clipboard, "keep me");
}

#[test]
fn double_click_word_selection_updates_primary() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState::default()));
    let config = ClipboardConfig {
        enable_primary_selection: true,
        enable_osc52: false,
        copy_on_mouse_select: CopyOnSelect::PrimarySelection,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("alpha beta", Rc::clone(&clipboard), config);
    let rect = backend.core.tree.node(text_area_id(&backend)).rect;
    let x = rect.x.max(0) as u16 + 1;
    let y = rect.y.max(0) as u16;

    click_left(&mut backend, x, y);
    click_left(&mut backend, x, y);

    assert_eq!(clipboard.borrow().primary, "alpha");
}

#[test]
fn triple_click_line_selection_updates_primary() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState::default()));
    let config = ClipboardConfig {
        enable_primary_selection: true,
        enable_osc52: false,
        copy_on_mouse_select: CopyOnSelect::PrimarySelection,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("alpha beta", Rc::clone(&clipboard), config);
    let rect = backend.core.tree.node(text_area_id(&backend)).rect;
    let x = rect.x.max(0) as u16 + 1;
    let y = rect.y.max(0) as u16;

    click_left(&mut backend, x, y);
    click_left(&mut backend, x, y);
    click_left(&mut backend, x, y);

    assert_eq!(clipboard.borrow().primary, "alpha beta");
}

#[test]
fn middle_click_pastes_primary_selection_into_focused_editor() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "regular".to_string(),
        primary: "unix".to_string(),
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        enable_primary_selection: true,
        middle_click_paste: PasteSource::PrimarySelection,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("", clipboard, config);
    let id = text_area_id(&backend);
    backend.set_focused(id);
    let rect = backend.core.tree.node(id).rect;

    backend
        .send_mouse(MouseEvent {
            x: rect.x.max(0) as u16,
            y: rect.y.max(0) as u16,
            kind: MouseKind::Down(MouseButton::Middle),
            mods: KeyMods::NONE,
        })
        .unwrap();

    assert_eq!(backend.state().text(), "unix");
}

#[cfg(feature = "terminal")]
#[test]
fn tracked_terminal_middle_click_is_forwarded_instead_of_locally_pasted() {
    struct TrackedTerminal {
        forwarded: Rc<RefCell<Vec<Vec<u8>>>>,
    }

    impl Component for TrackedTerminal {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let forwarded = Rc::clone(&self.forwarded);
            Terminal::new()
                .snapshot(TerminalRenderSnapshot {
                    mouse_mode: MouseModeState {
                        mode: MouseMode::Normal,
                        encoding: MouseEncoding::Sgr,
                        focus_events_enabled: false,
                    },
                    ..TerminalRenderSnapshot::default()
                })
                .on_mouse_forward(Callback::new(move |bytes| {
                    forwarded.borrow_mut().push(bytes);
                }))
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let forwarded = Rc::new(RefCell::new(Vec::new()));
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "must not paste".to_string(),
        ..MouseClipboardState::default()
    }));
    let app = crate::App::new()
        .clipboard_provider(MouseClipboardProvider(Rc::clone(&clipboard)))
        .clipboard_config(ClipboardConfig {
            middle_click_paste: PasteSource::Clipboard,
            ..ClipboardConfig::default()
        });
    let mut backend = TestBackend::new_with_app(
        app,
        TrackedTerminal {
            forwarded: Rc::clone(&forwarded),
        },
        (),
    );
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    });
    backend.render();
    let terminal_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::Terminal(_)))
        .map(|node| node.id)
        .expect("expected Terminal");
    backend.set_focused(terminal_id);
    let rect = backend.core.tree.node(terminal_id).rect;

    backend
        .send_mouse(MouseEvent {
            x: rect.x.max(0) as u16,
            y: rect.y.max(0) as u16,
            kind: MouseKind::Down(MouseButton::Middle),
            mods: KeyMods::NONE,
        })
        .unwrap();

    assert_eq!(forwarded.borrow().len(), 1);
    assert_eq!(clipboard.borrow().clipboard_reads, 0);
}

#[test]
fn opt_in_right_click_pastes_regular_clipboard() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "regular".to_string(),
        primary: "unix".to_string(),
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        right_click_action: RightClickAction::PasteClipboard,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("", clipboard, config);
    let id = text_area_id(&backend);
    backend.set_focused(id);
    let rect = backend.core.tree.node(id).rect;

    backend
        .send_mouse(MouseEvent {
            x: rect.x.max(0) as u16,
            y: rect.y.max(0) as u16,
            kind: MouseKind::Down(MouseButton::Right),
            mods: KeyMods::NONE,
        })
        .unwrap();

    assert_eq!(backend.state().text(), "regular");
}

#[test]
fn right_click_copy_flashes_like_a_copy_shortcut() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState::default()));
    let config = ClipboardConfig {
        enable_osc52: false,
        copy_on_mouse_select: CopyOnSelect::Disabled,
        right_click_action: RightClickAction::CopyOrPaste,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("alpha beta", Rc::clone(&clipboard), config);
    let id = text_area_id(&backend);

    select_then_right_click(&mut backend);

    assert_eq!(clipboard.borrow().clipboard, "alpha");
    assert!(backend.copy_feedback.is_active(id));
}

#[test]
fn copy_on_select_stays_silent() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState::default()));
    let config = ClipboardConfig {
        enable_osc52: false,
        copy_on_mouse_select: CopyOnSelect::Clipboard,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("alpha beta", Rc::clone(&clipboard), config);
    let id = text_area_id(&backend);
    let rect = backend.core.tree.node(id).rect;
    let x = rect.x.max(0) as u16;
    let y = rect.y.max(0) as u16;

    for (event_x, kind) in [
        (x, MouseKind::Down(MouseButton::Left)),
        (x + 5, MouseKind::Drag(MouseButton::Left)),
        (x + 5, MouseKind::Up(MouseButton::Left)),
    ] {
        backend
            .send_mouse(MouseEvent {
                x: event_x,
                y,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }

    assert_eq!(clipboard.borrow().clipboard, "alpha");
    assert!(!backend.copy_feedback.is_active(id));
}

#[test]
fn failed_right_click_copy_does_not_flash_or_paste() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "stale".to_string(),
        fail_clipboard_writes: true,
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        enable_osc52: false,
        copy_on_mouse_select: CopyOnSelect::Disabled,
        right_click_action: RightClickAction::CopyOrPaste,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("alpha beta", Rc::clone(&clipboard), config);
    let id = text_area_id(&backend);

    select_then_right_click(&mut backend);

    assert!(!backend.copy_feedback.is_active(id));
    assert_eq!(backend.state().text(), "alpha beta");
    assert_eq!(clipboard.borrow().clipboard_reads, 0);
}

#[test]
fn right_click_pastes_into_the_editor_under_the_pointer() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "regular".to_string(),
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        right_click_action: RightClickAction::PasteClipboard,
        ..ClipboardConfig::default()
    };
    let mut backend = paired_editors_backend("", clipboard, config);
    let [left, right] = paired_editor_ids(&backend);
    backend.set_focused(left);

    let (x, y) = node_origin(&backend, right);
    press(&mut backend, MouseButton::Right, x, y);

    assert_eq!(backend.state()[0].text(), "");
    assert_eq!(backend.state()[1].text(), "regular");
    assert_eq!(backend.focused, Some(right));
}

#[test]
fn right_click_outside_an_editor_does_not_paste() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "regular".to_string(),
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        right_click_action: RightClickAction::CopyOrPaste,
        ..ClipboardConfig::default()
    };
    let mut backend = paired_editors_backend("", Rc::clone(&clipboard), config);
    let [left, _] = paired_editor_ids(&backend);
    backend.set_focused(left);
    let chrome = backend
        .core
        .tree
        .hit_test(0, 0)
        .expect("chrome should be hit-testable");
    assert!(matches!(
        backend.core.tree.node(chrome).kind,
        NodeKind::MouseRegion(_)
    ));

    press(&mut backend, MouseButton::Right, 0, 0);

    assert_eq!(backend.state()[0].text(), "");
    assert_eq!(backend.state()[1].text(), "");
    assert_eq!(backend.focused, Some(left));
    assert_eq!(clipboard.borrow().clipboard_reads, 0);
}

#[test]
fn middle_click_pastes_into_the_editor_under_the_pointer() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        primary: "unix".to_string(),
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        enable_primary_selection: true,
        middle_click_paste: PasteSource::PrimarySelection,
        ..ClipboardConfig::default()
    };
    let mut backend = paired_editors_backend("", clipboard, config);
    let [left, right] = paired_editor_ids(&backend);
    backend.set_focused(left);

    press(&mut backend, MouseButton::Middle, 0, 0);
    assert_eq!(backend.state()[0].text(), "");

    let (x, y) = node_origin(&backend, right);
    press(&mut backend, MouseButton::Middle, x, y);

    assert_eq!(backend.state()[0].text(), "");
    assert_eq!(backend.state()[1].text(), "unix");
    assert_eq!(backend.focused, Some(right));
}

#[test]
fn right_click_copy_ignores_a_selection_the_pointer_is_not_on() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "regular".to_string(),
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        enable_osc52: false,
        copy_on_mouse_select: CopyOnSelect::Disabled,
        right_click_action: RightClickAction::CopyOrPaste,
        ..ClipboardConfig::default()
    };
    let mut backend = paired_editors_backend("alpha beta", Rc::clone(&clipboard), config);
    let [left, right] = paired_editor_ids(&backend);
    let (x, y) = node_origin(&backend, left);
    for (event_x, kind) in [
        (x, MouseKind::Down(MouseButton::Left)),
        (x + 5, MouseKind::Drag(MouseButton::Left)),
        (x + 5, MouseKind::Up(MouseButton::Left)),
    ] {
        backend
            .send_mouse(MouseEvent {
                x: event_x,
                y,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }

    let (x, y) = node_origin(&backend, right);
    press(&mut backend, MouseButton::Right, x, y);

    assert_eq!(clipboard.borrow().clipboard, "regular");
    assert!(!backend.copy_feedback.is_active(left));
    assert_eq!(backend.state()[0].text(), "alpha beta");
    assert_eq!(backend.state()[1].text(), "regular");
}

#[test]
fn right_click_copy_only_copies_the_selection() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "stale".to_string(),
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        enable_osc52: false,
        copy_on_mouse_select: CopyOnSelect::Disabled,
        right_click_action: RightClickAction::CopyOnly,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("alpha beta", Rc::clone(&clipboard), config);
    let id = text_area_id(&backend);

    select_then_right_click(&mut backend);

    assert_eq!(clipboard.borrow().clipboard, "alpha");
    assert_eq!(clipboard.borrow().clipboard_reads, 0);
    assert_eq!(backend.state().text(), "alpha beta");
    assert!(backend.copy_feedback.is_active(id));
}

#[test]
fn right_click_copy_only_does_nothing_without_a_selection() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "regular".to_string(),
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        enable_osc52: false,
        right_click_action: RightClickAction::CopyOnly,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("", Rc::clone(&clipboard), config);
    let id = text_area_id(&backend);
    backend.set_focused(id);
    let (x, y) = node_origin(&backend, id);

    press(&mut backend, MouseButton::Right, x, y);

    assert_eq!(backend.state().text(), "");
    assert_eq!(clipboard.borrow().clipboard, "regular");
    assert_eq!(clipboard.borrow().clipboard_reads, 0);
    assert!(!backend.copy_feedback.is_active(id));
    assert_eq!(backend.focused, Some(id));
}

#[test]
fn right_click_copy_only_leaves_another_editors_selection_alone() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "regular".to_string(),
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        enable_osc52: false,
        copy_on_mouse_select: CopyOnSelect::Disabled,
        right_click_action: RightClickAction::CopyOnly,
        ..ClipboardConfig::default()
    };
    let mut backend = paired_editors_backend("alpha beta", Rc::clone(&clipboard), config);
    let [left, right] = paired_editor_ids(&backend);
    let (x, y) = node_origin(&backend, left);
    for (event_x, kind) in [
        (x, MouseKind::Down(MouseButton::Left)),
        (x + 5, MouseKind::Drag(MouseButton::Left)),
        (x + 5, MouseKind::Up(MouseButton::Left)),
    ] {
        backend
            .send_mouse(MouseEvent {
                x: event_x,
                y,
                kind,
                mods: KeyMods::NONE,
            })
            .unwrap();
    }

    let (x, y) = node_origin(&backend, right);
    press(&mut backend, MouseButton::Right, x, y);

    assert_eq!(clipboard.borrow().clipboard, "regular");
    assert_eq!(clipboard.borrow().clipboard_reads, 0);
    assert!(!backend.copy_feedback.is_active(left));
    assert_eq!(backend.state()[0].text(), "alpha beta");
    assert_eq!(backend.state()[1].text(), "");
    assert_ne!(backend.focused, Some(right));
}

#[test]
fn failed_right_click_copy_only_does_not_paste() {
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "stale".to_string(),
        fail_clipboard_writes: true,
        ..MouseClipboardState::default()
    }));
    let config = ClipboardConfig {
        enable_osc52: false,
        copy_on_mouse_select: CopyOnSelect::Disabled,
        right_click_action: RightClickAction::CopyOnly,
        ..ClipboardConfig::default()
    };
    let mut backend = mouse_clipboard_backend("alpha beta", Rc::clone(&clipboard), config);
    let id = text_area_id(&backend);

    select_then_right_click(&mut backend);

    assert!(!backend.copy_feedback.is_active(id));
    assert_eq!(backend.state().text(), "alpha beta");
    assert_eq!(clipboard.borrow().clipboard, "stale");
    assert_eq!(clipboard.borrow().clipboard_reads, 0);
}

#[cfg(feature = "terminal")]
#[test]
fn right_click_pastes_into_the_terminal_under_the_pointer() {
    struct PairedTerminals {
        pastes: Rc<RefCell<Vec<usize>>>,
    }

    impl Component for PairedTerminals {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let terminal = |index: usize| {
                let pastes = Rc::clone(&self.pastes);
                Terminal::new()
                    .width(Length::Px(10))
                    .on_input(Callback::new(
                        move |event: crate::widgets::TerminalInputEvent| {
                            if event.kind == crate::widgets::TerminalInputKind::Paste {
                                pastes.borrow_mut().push(index);
                            }
                        },
                    ))
            };
            HStack::new().child(terminal(0)).child(terminal(1)).into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let pastes = Rc::new(RefCell::new(Vec::new()));
    let clipboard = Rc::new(RefCell::new(MouseClipboardState {
        clipboard: "regular".to_string(),
        ..MouseClipboardState::default()
    }));
    let app = crate::App::new()
        .clipboard_provider(MouseClipboardProvider(clipboard))
        .clipboard_config(ClipboardConfig {
            right_click_action: RightClickAction::CopyOrPaste,
            ..ClipboardConfig::default()
        });
    let mut backend = TestBackend::new_with_app(
        app,
        PairedTerminals {
            pastes: Rc::clone(&pastes),
        },
        (),
    );
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 20,
        h: 3,
    });
    backend.render();
    let mut terminals: Vec<NodeId> = backend
        .core
        .tree
        .iter()
        .filter(|node| matches!(node.kind, NodeKind::Terminal(_)))
        .map(|node| node.id)
        .collect();
    terminals.sort_by_key(|id| backend.core.tree.node(*id).rect.x);
    let [left, right] = terminals.try_into().expect("expected two Terminals");
    backend.set_focused(left);

    let (x, y) = node_origin(&backend, right);
    press(&mut backend, MouseButton::Right, x, y);

    assert_eq!(&*pastes.borrow(), &[1]);
    assert_eq!(backend.focused, Some(right));
}

#[test]
fn clicking_hover_only_mouse_region_preserves_hover_state() {
    struct HoverOnlyRegion {
        hover_changes: Rc<RefCell<Vec<bool>>>,
    }

    impl Component for HoverOnlyRegion {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let hover_changes = Rc::clone(&self.hover_changes);
            MouseRegion::new()
                .on_hover_change(Callback::new(move |hovered| {
                    hover_changes.borrow_mut().push(hovered);
                }))
                .child(Text::new("foo"))
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let hover_changes = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(HoverOnlyRegion {
        hover_changes: Rc::clone(&hover_changes),
    });

    assert!(update_hover_test_backend(&mut backend, 0, 0, false));
    let hovered = backend.mouse.hovered.expect("region should be hovered");
    assert_eq!(&*hover_changes.borrow(), &[true]);

    for kind in [
        MouseKind::Down(MouseButton::Left),
        MouseKind::Up(MouseButton::Left),
    ] {
        dispatch_mouse_test_backend(
            &mut backend,
            MouseEvent {
                x: 0,
                y: 0,
                kind,
                mods: KeyMods::NONE,
            },
        );
        assert_eq!(backend.mouse.hovered, Some(hovered));
        assert_eq!(
            &*hover_changes.borrow(),
            &[true],
            "click routing must not synthesize another hover entry"
        );
    }

    let region_rect = backend.core.tree.node(backend.core.tree.root).rect;
    let outside_x = region_rect.x.saturating_add(region_rect.w as i16).max(0) as u16;
    assert!(update_hover_test_backend(&mut backend, outside_x, 0, false));
    assert_eq!(&*hover_changes.borrow(), &[true, false]);
}

#[cfg(feature = "terminal")]
#[test]
fn only_a_forwarding_sgr_pixels_terminal_wants_pixel_reports() {
    struct PixelTerminal {
        encoding: MouseEncoding,
        mode: MouseMode,
        forwards: bool,
    }

    impl Component for PixelTerminal {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let terminal = Terminal::new().snapshot(TerminalRenderSnapshot {
                mouse_mode: MouseModeState {
                    mode: self.mode,
                    encoding: self.encoding,
                    focus_events_enabled: false,
                },
                ..TerminalRenderSnapshot::default()
            });
            if self.forwards {
                terminal.on_mouse_forward(Callback::new(|_| {})).into()
            } else {
                terminal.into()
            }
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let wants_pixels = |encoding, mode, forwards| {
        let mut backend = TestBackend::new(PixelTerminal {
            encoding,
            mode,
            forwards,
        });
        backend.render();
        backend.core.tree.has_terminal_pixel_mouse()
    };

    assert!(wants_pixels(
        MouseEncoding::SgrPixels,
        MouseMode::AnyEvent,
        true
    ));
    assert!(
        !wants_pixels(MouseEncoding::Sgr, MouseMode::AnyEvent, true),
        "cell-encoded reports have no use for pixels"
    );
    assert!(
        !wants_pixels(MouseEncoding::SgrPixels, MouseMode::None, true),
        "a program that stopped tracking the mouse reads nothing"
    );
    assert!(
        !wants_pixels(MouseEncoding::SgrPixels, MouseMode::AnyEvent, false),
        "reports that are never forwarded reach no program"
    );
}

#[test]
fn middle_clicking_closeable_tab_closes_without_activating_or_closing_non_closeable_items() {
    struct TabCloseRoot {
        closed: Rc<RefCell<Vec<usize>>>,
        changed: Rc<RefCell<Vec<usize>>>,
        actions: Rc<RefCell<Vec<usize>>>,
        show_close_buttons: bool,
        disabled: bool,
    }

    impl Component for TabCloseRoot {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let closed = Rc::clone(&self.closed);
            let changed = Rc::clone(&self.changed);
            let actions = Rc::clone(&self.actions);
            DraggableTabBar::new()
                .tabs([
                    DraggableTab::new("one").closeable(true),
                    DraggableTab::new("two").closeable(true),
                    DraggableTab::new("pinned").closeable(false),
                    DraggableTab::action("+").closeable(true),
                ])
                .active(0)
                .show_close_buttons(self.show_close_buttons)
                .disabled(self.disabled)
                .on_close(Callback::new(move |ev: DraggableTabCloseEvent| {
                    closed.borrow_mut().push(ev.index);
                }))
                .on_change(Callback::new(move |ev: TabsEvent| {
                    changed.borrow_mut().push(ev.index);
                }))
                .on_action(Callback::new(
                    move |ev: crate::widgets::DraggableTabActionEvent| {
                        actions.borrow_mut().push(ev.index);
                    },
                ))
                .into()
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }
    }

    let closed = Rc::new(RefCell::new(Vec::new()));
    let changed = Rc::new(RefCell::new(Vec::new()));
    let actions = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(TabCloseRoot {
        closed: Rc::clone(&closed),
        changed: Rc::clone(&changed),
        actions: Rc::clone(&actions),
        show_close_buttons: true,
        disabled: false,
    });

    let bar_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DraggableTabBar(_)))
        .map(|node| node.id)
        .expect("expected a DraggableTabBar node");
    let (inner, layout) = {
        let node = backend.core.tree.node(bar_id);
        let NodeKind::DraggableTabBar(bar) = &node.kind else {
            unreachable!();
        };
        let inner = node.rect.inner(bar.border, bar.padding);
        let layout = DraggableTabBar::viewport_layout(
            &bar.tabs,
            &bar.display_options(),
            &bar.viewport_options(inner.w as usize),
        );
        (inner, layout)
    };

    let tab0 = &layout.visible_tabs[0];
    let tab1 = &layout.visible_tabs[1];
    let tab2 = &layout.visible_tabs[2];
    let tab3 = &layout.visible_tabs[3];
    let tab0_close_x = inner.x as u16
        + tab0.start as u16
        + tab0.metrics.close_start.expect("tab0 close slot") as u16;
    let tab1_body_x = inner.x as u16 + tab1.start as u16 + 1;
    let divider_x = inner.x as u16 + tab0.end as u16;
    let tab2_body_x = inner.x as u16 + tab2.start as u16 + 1;
    let tab3_body_x = inner.x as u16 + tab3.start as u16 + 1;
    let y = inner.y as u16;

    // Middle-click body of inactive closeable tab 1: closes tab 1 without changing active tab.
    press(&mut backend, MouseButton::Middle, tab1_body_x, y);
    assert_eq!(&*closed.borrow(), &[1]);
    assert!(changed.borrow().is_empty());
    assert!(actions.borrow().is_empty());
    assert!(!backend.drag_is_active());

    // Middle-click close button of tab 0: also closes tab 0.
    press(&mut backend, MouseButton::Middle, tab0_close_x, y);
    assert_eq!(&*closed.borrow(), &[1, 0]);

    // Middle-click divider, non-closeable tab, and action tab: none should close.
    press(&mut backend, MouseButton::Middle, divider_x, y);
    press(&mut backend, MouseButton::Middle, tab2_body_x, y);
    press(&mut backend, MouseButton::Middle, tab3_body_x, y);
    assert_eq!(&*closed.borrow(), &[1, 0]);
    assert!(changed.borrow().is_empty());
    assert!(actions.borrow().is_empty());

    // Middle-click still closes closeable tabs when `show_close_buttons(false)` is set.
    let closed_hidden = Rc::new(RefCell::new(Vec::new()));
    let mut hidden_backend = TestBackend::new(TabCloseRoot {
        closed: Rc::clone(&closed_hidden),
        changed: Rc::new(RefCell::new(Vec::new())),
        actions: Rc::new(RefCell::new(Vec::new())),
        show_close_buttons: false,
        disabled: false,
    });
    press(&mut hidden_backend, MouseButton::Middle, 1, 0);
    assert_eq!(&*closed_hidden.borrow(), &[0]);

    // Disabled bar ignores middle-click.
    let closed_disabled = Rc::new(RefCell::new(Vec::new()));
    let mut disabled_backend = TestBackend::new(TabCloseRoot {
        closed: Rc::clone(&closed_disabled),
        changed: Rc::new(RefCell::new(Vec::new())),
        actions: Rc::new(RefCell::new(Vec::new())),
        show_close_buttons: true,
        disabled: true,
    });
    press(&mut disabled_backend, MouseButton::Middle, 1, 0);
    assert!(closed_disabled.borrow().is_empty());
}

#[test]
fn middle_clicking_closeable_tab_locks_width_for_rapid_repeated_closing() {
    struct StatefulTabs;

    #[derive(Default)]
    struct State {
        tabs: Vec<&'static str>,
    }

    impl Component for StatefulTabs {
        type Message = usize;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State {
                tabs: vec!["very-long-first-tab", "b", "c"],
            }
        }

        fn update(&mut self, index: Self::Message, ctx: &mut Context<Self>) -> Update {
            if index < ctx.state.tabs.len() {
                ctx.state.tabs.remove(index);
                Update::full()
            } else {
                Update::none()
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            DraggableTabBar::new()
                .tabs(
                    ctx.state
                        .tabs
                        .iter()
                        .copied()
                        .map(|label| DraggableTab::new(label).closeable(true)),
                )
                .on_close(ctx.link().callback(|ev: DraggableTabCloseEvent| ev.index))
                .into()
        }
    }

    let mut backend = TestBackend::new(StatefulTabs);
    let bar_id = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DraggableTabBar(_)))
        .map(|node| node.id)
        .expect("expected a DraggableTabBar node");

    // Click near the right end of "very-long-first-tab" (well past the natural width of "b").
    let click_x = {
        let node = backend.core.tree.node(bar_id);
        let NodeKind::DraggableTabBar(bar) = &node.kind else {
            unreachable!();
        };
        let inner = node.rect.inner(bar.border, bar.padding);
        let layout = DraggableTabBar::viewport_layout(
            &bar.tabs,
            &bar.display_options(),
            &bar.viewport_options(inner.w as usize),
        );
        inner.x as u16 + layout.visible_tabs[0].end as u16 - 2
    };

    press(&mut backend, MouseButton::Middle, click_x, 0);
    assert_eq!(backend.state().tabs, vec!["b", "c"]);
    {
        let NodeKind::DraggableTabBar(bar) = &backend.core.tree.node(bar_id).kind else {
            unreachable!();
        };
        assert!(
            bar.width_lock.is_some(),
            "middle-click close should lock the closed tab width on the bar"
        );
    }

    // Without moving the mouse, a second middle-click at the same X hits the replacement tab "b".
    press(&mut backend, MouseButton::Middle, click_x, 0);
    assert_eq!(backend.state().tabs, vec!["c"]);
}

struct ContextHitHarness {
    tab_bar: bool,
    events: Rc<RefCell<Vec<Option<usize>>>>,
}
impl Component for ContextHitHarness {
    type Message = ();
    type Properties = ();
    type State = ();
    fn create_state(&self, _: &()) {}
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update {
        Update::none()
    }
    fn view(&self, _: &Context<Self>) -> Element {
        let events = self.events.clone();
        if self.tab_bar {
            DraggableTabBar::new()
                .tabs([DraggableTab::new("First")])
                .border(true)
                .padding((1, 2))
                .width(Length::Px(24))
                .height(Length::Px(7))
                .on_context_menu(Callback::new(move |(index, _)| {
                    events.borrow_mut().push(index)
                }))
                .into()
        } else {
            MouseRegion::new()
                .hit_test(|x, _| x == 0)
                .on_context_menu(Callback::new(move |_| events.borrow_mut().push(Some(99))))
                .child(
                    Button::new("wide button")
                        .width(Length::Px(20))
                        .on_click(Callback::new(|_| {})),
                )
                .into()
        }
    }
}

#[test]
fn context_menu_respects_ancestor_hit_test_for_interactive_children() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(ContextHitHarness {
        tab_bar: false,
        events: events.clone(),
    });
    backend.render();
    for x in [5, 0] {
        backend
            .send_mouse(MouseEvent {
                x,
                y: 0,
                kind: MouseKind::Down(MouseButton::Right),
                mods: KeyMods::NONE,
            })
            .unwrap();
    }
    assert_eq!(
        &*events.borrow(),
        &[Some(99)],
        "only the accepted point opens the menu"
    );
}

#[test]
fn context_menu_tab_border_and_padding_do_not_report_a_tab() {
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut backend = TestBackend::new(ContextHitHarness {
        tab_bar: true,
        events: events.clone(),
    });
    backend.render();
    let node = backend
        .core
        .tree
        .iter()
        .find(|node| matches!(node.kind, NodeKind::DraggableTabBar(_)))
        .unwrap();
    let NodeKind::DraggableTabBar(bar) = &node.kind else {
        unreachable!()
    };
    let inner = node.rect.inner(bar.border, bar.padding);
    let points = [
        (node.rect.x, node.rect.y),
        (inner.x - 1, inner.y),
        (inner.x, inner.y - 1),
        (inner.x + inner.w as i16, inner.y),
        (inner.x, inner.y + inner.h as i16),
        (inner.x, inner.y + 1),
        (inner.x, inner.y),
    ];
    for (x, y) in points {
        backend
            .send_mouse(MouseEvent {
                x: x as u16,
                y: y as u16,
                kind: MouseKind::Down(MouseButton::Right),
                mods: KeyMods::NONE,
            })
            .unwrap();
    }
    assert_eq!(
        &*events.borrow(),
        &[None, None, None, None, None, None, Some(0)]
    );
}

#[cfg(feature = "terminal")]
#[test]
fn context_menu_consumes_the_entire_terminal_right_button_gesture() {
    struct ContextTerminal {
        mode: MouseMode,
        forwarded: Rc<RefCell<Vec<Vec<u8>>>>,
    }
    impl Component for ContextTerminal {
        type Message = bool;
        type Properties = ();
        type State = bool;
        fn create_state(&self, _: &()) -> bool {
            true
        }
        fn update(&mut self, enabled: bool, ctx: &mut Context<Self>) -> Update {
            ctx.state = enabled;
            Update::layout()
        }
        fn view(&self, ctx: &Context<Self>) -> Element {
            let forwarded = self.forwarded.clone();
            let terminal = Terminal::new()
                .snapshot(TerminalRenderSnapshot {
                    mouse_mode: MouseModeState {
                        mode: self.mode,
                        encoding: MouseEncoding::Sgr,
                        focus_events_enabled: false,
                    },
                    ..TerminalRenderSnapshot::default()
                })
                .on_mouse_forward(Callback::new(move |bytes| {
                    forwarded.borrow_mut().push(bytes)
                }));
            let region = MouseRegion::new().child(terminal);
            if ctx.state {
                region
                    .on_context_menu(ctx.link().callback(|_| false))
                    .into()
            } else {
                region.into()
            }
        }
    }
    for mode in [MouseMode::Normal, MouseMode::AnyEvent] {
        let forwarded = Rc::new(RefCell::new(Vec::new()));
        let mut backend = TestBackend::new(ContextTerminal {
            mode,
            forwarded: forwarded.clone(),
        });
        backend.render();
        let id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Terminal(_)))
            .unwrap()
            .id;
        backend.set_focused(id);
        for kind in [
            MouseKind::Down(MouseButton::Right),
            MouseKind::Drag(MouseButton::Right),
            MouseKind::Up(MouseButton::Right),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x: 1,
                    y: 0,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
            backend.render();
        }
        assert!(
            !*backend.state(),
            "the callback disables itself before drag/release"
        );
        assert!(
            forwarded.borrow().is_empty(),
            "{mode:?}: child must receive none of a consumed gesture"
        );
        // After release, an unhandled gesture must again reach the child intact.
        for kind in [
            MouseKind::Down(MouseButton::Right),
            MouseKind::Up(MouseButton::Right),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x: 1,
                    y: 0,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
        }
        assert_eq!(
            forwarded.borrow().len(),
            2,
            "{mode:?}: suppression must end at release"
        );
        // A lost release must not suppress the next full right-button gesture.
        forwarded.borrow_mut().clear();
        *backend.state_mut() = true;
        backend.render();
        for kind in [
            MouseKind::Down(MouseButton::Right),
            MouseKind::Down(MouseButton::Right),
            MouseKind::Up(MouseButton::Right),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x: 1,
                    y: 0,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
            backend.render();
        }
        assert_eq!(
            forwarded.borrow().len(),
            2,
            "{mode:?}: a fresh right press recovers from a lost release"
        );
    }
}

#[test]
fn fixed_tab_selects_without_arming_a_drag() {
    struct App;

    struct State {
        selected: usize,
    }

    #[derive(Clone)]
    enum Msg {
        Select(crate::widgets::TabsEvent),
        Ignore,
    }

    impl Component for App {
        type Message = Msg;
        type Properties = ();
        type State = State;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            State { selected: 0 }
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            if let Msg::Select(event) = msg {
                ctx.state.selected = event.index;
            }
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            crate::widgets::DraggableTabBar::new()
                .tabs([
                    crate::widgets::DraggableTab::new("Home").reorderable(false),
                    crate::widgets::DraggableTab::new("Alpha"),
                ])
                .active(ctx.state.selected)
                .divider('|')
                .show_overflow_controls(false)
                .show_close_buttons(false)
                .focusable(false)
                .height(Length::Px(1))
                .on_change(ctx.link().callback(Msg::Select))
                .on_reorder(ctx.link().callback(|_| Msg::Ignore))
                .into()
        }
    }

    let mut backend = TestBackend::new(App);
    backend.set_viewport(Rect {
        x: 0,
        y: 0,
        w: 40,
        h: 1,
    });
    backend.render();

    backend
        .send_mouse(MouseEvent {
            x: 1,
            y: 0,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::NONE,
        })
        .unwrap();
    assert!(
        matches!(backend.drag.active, ActiveDrag::None),
        "clicking a fixed tab armed {:?}",
        backend.drag.active
    );

    backend
        .send_mouse(MouseEvent {
            x: 8,
            y: 0,
            kind: MouseKind::Down(MouseButton::Left),
            mods: KeyMods::NONE,
        })
        .unwrap();
    assert!(
        matches!(backend.drag.active, ActiveDrag::DraggableTabBar(_)),
        "clicking a normal tab should arm a drag, got {:?}",
        backend.drag.active
    );
}

#[cfg(feature = "terminal")]
#[test]
fn middle_click_consumes_the_entire_terminal_gesture() {
    struct ContextTerminal {
        mode: MouseMode,
        forwarded: Rc<RefCell<Vec<Vec<u8>>>>,
    }
    impl Component for ContextTerminal {
        type Message = bool;
        type Properties = ();
        type State = bool;
        fn create_state(&self, _: &()) -> bool {
            true
        }
        fn update(&mut self, enabled: bool, ctx: &mut Context<Self>) -> Update {
            ctx.state = enabled;
            Update::layout()
        }
        fn view(&self, ctx: &Context<Self>) -> Element {
            let forwarded = self.forwarded.clone();
            let terminal = Terminal::new()
                .snapshot(TerminalRenderSnapshot {
                    mouse_mode: MouseModeState {
                        mode: self.mode,
                        encoding: MouseEncoding::Sgr,
                        focus_events_enabled: false,
                    },
                    ..TerminalRenderSnapshot::default()
                })
                .on_mouse_forward(Callback::new(move |bytes| {
                    forwarded.borrow_mut().push(bytes)
                }));
            let region = MouseRegion::new().child(terminal);
            if ctx.state {
                region
                    .on_middle_click(ctx.link().callback(|_| false))
                    .into()
            } else {
                region.into()
            }
        }
    }
    for mode in [MouseMode::Normal, MouseMode::AnyEvent] {
        let forwarded = Rc::new(RefCell::new(Vec::new()));
        let mut backend = TestBackend::new(ContextTerminal {
            mode,
            forwarded: forwarded.clone(),
        });
        backend.render();
        let id = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Terminal(_)))
            .unwrap()
            .id;
        backend.set_focused(id);
        for kind in [
            MouseKind::Down(MouseButton::Middle),
            MouseKind::Drag(MouseButton::Middle),
            MouseKind::Up(MouseButton::Middle),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x: 1,
                    y: 0,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
            backend.render();
        }
        assert!(
            !*backend.state(),
            "the callback disables itself before drag/release"
        );
        assert!(
            forwarded.borrow().is_empty(),
            "{mode:?}: child must receive none of a consumed gesture"
        );
        // After release, an unhandled gesture must again reach the child intact.
        for kind in [
            MouseKind::Down(MouseButton::Middle),
            MouseKind::Up(MouseButton::Middle),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x: 1,
                    y: 0,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
        }
        assert_eq!(
            forwarded.borrow().len(),
            2,
            "{mode:?}: suppression must end at release"
        );
        // A lost release must not suppress the next full middle-button gesture.
        forwarded.borrow_mut().clear();
        *backend.state_mut() = true;
        backend.render();
        for kind in [
            MouseKind::Down(MouseButton::Middle),
            MouseKind::Down(MouseButton::Middle),
            MouseKind::Up(MouseButton::Middle),
        ] {
            backend
                .send_mouse(MouseEvent {
                    x: 1,
                    y: 0,
                    kind,
                    mods: KeyMods::NONE,
                })
                .unwrap();
            backend.render();
        }
        assert_eq!(
            forwarded.borrow().len(),
            2,
            "{mode:?}: a fresh middle press recovers from a lost release"
        );
    }
}
