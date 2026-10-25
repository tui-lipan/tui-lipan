//! Pointer policy shared by the real runtime and headless tests.
use crate::core::node::{Node, NodeId, NodeKind, NodeTree, WidgetNode};
use crate::style::{PointerMode, PointerShape};

mod interactive;

pub(crate) fn has_pointer_target(node: &Node) -> bool {
    if node.pointer_shape.is_some()
        || node.kind.has_pointer_shape()
        || node.is_interactive()
        || node.is_hoverable()
    {
        return true;
    }
    match node.kind {
        NodeKind::Input(_) | NodeKind::TextArea(_) => true,
        #[cfg(feature = "terminal")]
        NodeKind::Terminal(_) => true,
        _ => false,
    }
}

/// Apply capture before hover. Both native output and TestBackend use this policy.
pub(crate) fn interaction_pointer(
    tree: &NodeTree,
    mouse: &crate::app::interaction_state::MouseTrackingState,
    drag: &crate::app::interaction_state::DragState,
    mode: PointerMode,
) -> Option<PointerShape> {
    if matches!(mode, PointerMode::Disabled | PointerMode::Terminal) {
        let (x, y) = mouse.last_mouse.get()?;
        return pointer_at(tree, x as i16, y as i16, mode);
    }
    drag_owner(mouse, drag, mode)
        .and_then(|id| captured_pointer(tree, id).or_else(|| interactive_capture(tree, id, mode)))
        .or_else(|| {
            let (x, y) = mouse.last_mouse.get()?;
            pointer_at(tree, x as i16, y as i16, mode)
        })
}

fn drag_owner(
    mouse: &crate::app::interaction_state::MouseTrackingState,
    drag: &crate::app::interaction_state::DragState,
    mode: PointerMode,
) -> Option<NodeId> {
    use crate::app::interaction_state::ActiveDrag;
    if let ActiveDrag::Splitter(drag) = &drag.active {
        return Some(drag.id);
    }
    if mode == PointerMode::Interactive
        && let Some(id) = interactive_drag_owner(&drag.active)
    {
        return Some(id);
    }
    mouse
        .mouse_region_drag
        .as_ref()
        .filter(|drag| drag.started)
        .map(|drag| drag.node_id)
}

fn interactive_drag_owner(drag: &crate::app::interaction_state::ActiveDrag) -> Option<NodeId> {
    use crate::app::interaction_state::ActiveDrag;
    match drag {
        ActiveDrag::DraggableTabBar(drag) if drag.started => Some(drag.source_id),
        ActiveDrag::DragDrop(drag) if drag.started => Some(drag.source_id),
        ActiveDrag::Slider(drag) => Some(drag.id),
        ActiveDrag::Scrollbar(drag) => Some(drag.id),
        _ => None,
    }
}

fn captured_node(tree: &NodeTree, id: NodeId) -> Option<&Node> {
    if !tree.is_valid(id) {
        return None;
    }
    let node = tree.node(id);
    if node.inert || node.portal_suppressed || node.kind.is_disabled() {
        return None;
    }
    Some(node)
}

fn interactive_capture(tree: &NodeTree, id: NodeId, mode: PointerMode) -> Option<PointerShape> {
    if mode != PointerMode::Interactive {
        return None;
    }
    Some(interactive_drag_shape(&captured_node(tree, id)?.kind))
}

fn interactive_drag_shape(kind: &NodeKind) -> PointerShape {
    match kind {
        NodeKind::Splitter(splitter) => match splitter.orientation {
            crate::widgets::Orientation::Horizontal => PointerShape::EwResize,
            crate::widgets::Orientation::Vertical => PointerShape::NsResize,
        },
        NodeKind::Slider(_) => PointerShape::EwResize,
        _ => PointerShape::Grabbing,
    }
}

/// Resolve a hover with the same policy as captured/native output.
pub(crate) fn pointer_at(
    tree: &NodeTree,
    x: i16,
    y: i16,
    mode: PointerMode,
) -> Option<PointerShape> {
    match mode {
        PointerMode::Disabled => None,
        PointerMode::Terminal => terminal_request_at(tree, tree.pointer_target_at(x, y)?, x, y),
        PointerMode::Explicit => tree.pointer_shape_at(x, y),
        PointerMode::Interactive => tree
            .pointer_shape_at(x, y)
            .or_else(|| interactive::pointer_at(tree, tree.pointer_target_at(x, y)?, x, y)),
    }
}

fn terminal_request_at(tree: &NodeTree, hit: NodeId, x: i16, y: i16) -> Option<PointerShape> {
    #[cfg(feature = "terminal")]
    {
        let mut current = Some(hit);
        while let Some(id) = current {
            let node = tree.node(id);
            if let NodeKind::Terminal(terminal) = &node.kind {
                return crate::widgets::internal::terminal_mouse_content_rect(tree, id)
                    .filter(|rect| rect.contains(x, y))
                    .and_then(|_| {
                        terminal
                            .screen
                            .as_ref()
                            .map_or(terminal.pointer_shape, |screen| screen.pointer_shape())
                    });
            }
            current = node.parent;
        }
    }
    #[cfg(not(feature = "terminal"))]
    let _ = (tree, hit, x, y);
    None
}

/// Resolve a captured drag owner's explicit or widget-defined pointer.
pub(crate) fn captured_pointer(tree: &NodeTree, owner: NodeId) -> Option<PointerShape> {
    let node = captured_node(tree, owner)?;
    tree.pointer_override(owner)
        .or_else(|| node.kind.drag_pointer_shape())
}

pub(crate) fn automatic_pointer(
    tree: &NodeTree,
    hit: NodeId,
    x: i16,
    y: i16,
) -> Option<PointerShape> {
    #[cfg(not(feature = "terminal"))]
    let _ = (x, y);
    let mut current = Some(hit);
    while let Some(id) = current {
        let node = tree.node(id);
        #[cfg(feature = "terminal")]
        if let NodeKind::Terminal(terminal) = &node.kind {
            return crate::widgets::internal::terminal_mouse_content_rect(tree, id)
                .filter(|rect| rect.contains(x, y))
                .and_then(|_| terminal_pointer(terminal));
        }
        if node.kind.has_pointer_shape() {
            return node.kind.pointer_shape_at(x, y, node.rect);
        }
        if let Some(shape) = widget_pointer(node) {
            return Some(shape);
        }
        current = node.parent;
    }
    None
}

fn widget_pointer(node: &Node) -> Option<PointerShape> {
    match &node.kind {
        NodeKind::Input(input) if !input.disabled => Some(PointerShape::Text),
        NodeKind::TextArea(textarea) if !textarea.disabled => Some(PointerShape::Text),
        _ => None,
    }
}

#[cfg(feature = "terminal")]
fn terminal_pointer(terminal: &crate::widgets::internal::TerminalNode) -> Option<PointerShape> {
    use crate::widgets::MouseMode;
    if terminal.link_hover.is_some() {
        return Some(PointerShape::Pointer);
    }
    if let Some(shape) = terminal
        .screen
        .as_ref()
        .map_or(terminal.pointer_shape, |screen| screen.pointer_shape())
    {
        return Some(shape);
    }
    let mode = terminal
        .screen
        .as_ref()
        .map_or(terminal.mouse_mode, |screen| screen.mouse_mode());
    Some(if mode.mode == MouseMode::None {
        PointerShape::Text
    } else {
        PointerShape::Default
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::LayoutEngine;
    use crate::style::Rect;
    use crate::widgets::{Button, HStack, Input, MouseRegion, Text};
    use crate::{Element, IntoElement};

    fn tree(element: impl Into<Element>) -> NodeTree {
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &element.into(),
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 8,
            },
            None,
        );
        tree
    }

    #[test]
    fn captured_widget_pointer_respects_ancestor_overrides_and_inert_sources() {
        use crate::widgets::{Splitter, VStack};
        let mut tree = tree(
            VStack::new()
                .child(
                    Splitter::vertical()
                        .handle_pointer_shape(PointerShape::EwResize)
                        .handle_active_pointer_shape(PointerShape::Grabbing)
                        .child(Text::new("left"))
                        .child(Text::new("right")),
                )
                .pointer_shape(PointerShape::Help),
        );
        let owner = tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Splitter(_)))
            .unwrap()
            .id;
        assert_eq!(captured_pointer(&tree, owner), Some(PointerShape::Help));
        tree.node_mut(owner).inert = true;
        assert_eq!(captured_pointer(&tree, owner), None);
    }

    #[test]
    fn pointer_only_elements_do_not_acquire_click_handlers() {
        let tree = tree(Text::new("drag").pointer_shape(PointerShape::EwResize));
        assert_eq!(tree.pointer_shape_at(0, 0), Some(PointerShape::EwResize));
        assert!(tree.hit_test(0, 0).is_none());
        assert!(tree.has_pointer_targets());
    }

    #[test]
    fn deepest_explicit_override_wins_over_inherited_and_automatic_shapes() {
        let tree = tree(
            HStack::new()
                .child(Input::new("text").width(crate::Length::Px(10)))
                .child(Button::new("button").pointer_shape(PointerShape::Help))
                .pointer_shape(PointerShape::Grab),
        );
        assert_eq!(tree.pointer_shape_at(0, 0), Some(PointerShape::Grab));
        let button = tree
            .iter()
            .find(|node| node.pointer_shape == Some(PointerShape::Help))
            .unwrap();
        assert_eq!(
            tree.pointer_shape_at(button.rect.x, button.rect.y),
            Some(PointerShape::Help)
        );
    }

    #[test]
    fn tab_pointer_opt_in_excludes_dividers_and_unused_strip_space() {
        use crate::widgets::{Tab, Tabs};
        let tree = tree(
            Tabs::new()
                .tabs([
                    Tab::new("a").pointer_shape(PointerShape::Pointer),
                    Tab::new("b"),
                ])
                .divider('|'),
        );
        assert_eq!(tree.pointer_shape_at(1, 0), Some(PointerShape::Pointer));
        assert_eq!(tree.pointer_shape_at(3, 0), None);
        assert_eq!(tree.pointer_shape_at(5, 0), None);
        assert_eq!(tree.pointer_shape_at(20, 0), None);
        assert_eq!(tree.pointer_shape_at(1, 1), None);
    }

    #[test]
    fn focus_and_drag_regions_do_not_advertise_a_click_action() {
        let region = MouseRegion::new()
            .on_mouse_down(crate::Callback::new(|_| {}))
            .on_drag(crate::Callback::new(|_| {}))
            .child(Text::new("chrome"));
        assert_eq!(tree(region).pointer_shape_at(0, 0), None);
        let button = MouseRegion::new()
            .on_click(crate::Callback::new(|_| {}))
            .child(Text::new("open"));
        assert_eq!(tree(button.clone()).pointer_shape_at(0, 0), None);
        assert_eq!(
            tree(button.pointer_shape(PointerShape::Pointer)).pointer_shape_at(0, 0),
            Some(PointerShape::Pointer)
        );
    }

    #[cfg(feature = "terminal")]
    #[test]
    fn child_pointer_is_limited_to_terminal_content() {
        use crate::widgets::{Terminal, TerminalScreen};
        use std::{cell::RefCell, rc::Rc};
        let screen = Rc::new(RefCell::new(TerminalScreen::new(4, 20, 0)));
        screen.borrow_mut().process_bytes(b"\x1b]22;grab\x07");
        let tree = tree(Terminal::new().screen(screen).border(true).padding(1));
        assert_eq!(tree.pointer_shape_at(0, 0), None);
        assert_eq!(tree.pointer_shape_at(1, 1), None);
        assert_eq!(tree.pointer_shape_at(2, 2), Some(PointerShape::Grab));
        assert_eq!(tree.pointer_shape_at(39, 7), None);
    }

    #[test]
    fn masks_and_disabled_regions_do_not_override_a_child_pointer() {
        let tree = tree(MouseRegion::new().enabled(false).child(Input::new("text")));
        assert_eq!(tree.pointer_shape_at(0, 0), Some(PointerShape::Text));
    }

    #[cfg(feature = "terminal")]
    #[test]
    fn hovered_terminal_uses_live_state_even_when_unfocused_and_without_a_cell_repaint() {
        use crate::widgets::{Terminal, TerminalScreen};
        use std::{cell::RefCell, rc::Rc};
        let left = Rc::new(RefCell::new(TerminalScreen::new(8, 20, 0)));
        let right = Rc::new(RefCell::new(TerminalScreen::new(8, 20, 0)));
        left.borrow_mut().process_bytes(b"\x1b]22;grab\x07");
        right.borrow_mut().process_bytes(b"\x1b]22;pointer\x07");
        let tree = tree(
            HStack::new()
                .child(
                    Terminal::new()
                        .screen(left.clone())
                        .width(crate::Length::Px(20)),
                )
                .child(
                    Terminal::new()
                        .screen(right.clone())
                        .width(crate::Length::Px(20)),
                ),
        );
        assert_eq!(tree.pointer_shape_at(1, 1), Some(PointerShape::Grab));
        assert_eq!(tree.pointer_shape_at(21, 1), Some(PointerShape::Pointer));
        right.borrow_mut().process_bytes(b"\x1b]22;\x07");
        assert_eq!(tree.pointer_shape_at(21, 1), Some(PointerShape::Text));
        right.borrow_mut().process_bytes(b"\x1b[?1000h");
        assert_eq!(tree.pointer_shape_at(21, 1), Some(PointerShape::Default));
    }

    #[test]
    fn interactive_mode_limits_hands_to_actionable_rows_and_controls() {
        use crate::widgets::{List, ListItem, VStack};
        let tree = tree(
            VStack::new()
                .gap(0)
                .child(
                    List::new()
                        .items([
                            ListItem::header("Heading"),
                            ListItem::new("Open"),
                            ListItem::spacer(),
                            ListItem::new("Unavailable").pointer_shape(PointerShape::Default),
                        ])
                        .height(crate::Length::Px(5))
                        .scrollbar(false)
                        .show_scroll_indicators(false)
                        .on_activate(crate::Callback::new(|_| {})),
                )
                .child(
                    Button::new("Disabled")
                        .disabled(true)
                        .on_click(crate::Callback::new(|_| {})),
                )
                .child(
                    MouseRegion::new()
                        .on_mouse_down(crate::Callback::new(|_| {}))
                        .child(Text::new("Focus only")),
                )
                .child(
                    MouseRegion::new()
                        .on_click(crate::Callback::new(|_| {}))
                        .child(Text::new("Action")),
                ),
        );
        for (y, expected) in [
            (0, None),
            (1, Some(PointerShape::Pointer)),
            (2, None),
            (3, Some(PointerShape::Default)),
            (4, None),
            (5, None),
            (6, None),
            (7, Some(PointerShape::Pointer)),
        ] {
            assert_eq!(
                pointer_at(&tree, 1, y, PointerMode::Interactive),
                expected,
                "row {y}"
            );
            assert_eq!(pointer_at(&tree, 1, y, PointerMode::Disabled), None);
            assert_eq!(pointer_at(&tree, 1, y, PointerMode::Terminal), None);
            assert_eq!(pointer_at(&tree, 1, y, PointerMode::Explicit), None);
        }
    }

    #[test]
    fn interactive_tabs_exclude_dividers_and_unused_strip() {
        use crate::widgets::{DraggableTab, DraggableTabBar};
        let tree = tree(
            DraggableTabBar::new()
                .tabs([DraggableTab::new("One"), DraggableTab::new("Two")])
                .show_close_buttons(false)
                .on_change(crate::Callback::new(|_| {})),
        );
        let node = tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::DraggableTabBar(_)))
            .unwrap();
        let NodeKind::DraggableTabBar(tabs) = &node.kind else {
            unreachable!()
        };
        let inner = node.rect.inner(tabs.border, tabs.padding);
        assert_eq!(
            pointer_at(&tree, inner.x, inner.y, PointerMode::Interactive),
            Some(PointerShape::Pointer)
        );
        assert_eq!(
            pointer_at(&tree, 39, inner.y, PointerMode::Interactive),
            None
        );
        let separator = (inner.x..inner.x + 20)
            .find(|&x| {
                crate::widgets::DraggableTabBar::hit_target_at_view_col(
                    &tabs.tabs,
                    &tabs.display_options(),
                    &tabs.viewport_options(inner.w as usize),
                    (x - inner.x) as usize,
                )
                .is_none()
            })
            .unwrap();
        assert_eq!(
            pointer_at(&tree, separator, inner.y, PointerMode::Interactive),
            None
        );
    }

    #[cfg(feature = "terminal")]
    #[test]
    fn pane_only_policy_ignores_chrome_and_observes_live_osc_requests() {
        use crate::widgets::{Terminal, TerminalScreen};
        use std::{cell::RefCell, rc::Rc};
        let screen = Rc::new(RefCell::new(TerminalScreen::new(6, 38, 0)));
        let tree = tree(
            Terminal::new()
                .screen(screen.clone())
                .border(true)
                .pointer_shape(PointerShape::Help),
        );
        assert_eq!(
            pointer_at(&tree, 2, 2, PointerMode::Terminal),
            None,
            "no synthetic terminal fallback in pane-only mode"
        );
        screen.borrow_mut().process_bytes(b"\x1b]22;grab\x07");
        assert_eq!(
            pointer_at(&tree, 2, 2, PointerMode::Terminal),
            Some(PointerShape::Grab)
        );
        assert_eq!(pointer_at(&tree, 0, 0, PointerMode::Terminal), None);
        assert_eq!(pointer_at(&tree, 2, 2, PointerMode::Disabled), None);
        assert_eq!(
            pointer_at(&tree, 2, 2, PointerMode::Interactive),
            Some(PointerShape::Help)
        );
        screen.borrow_mut().process_bytes(b"\x1b]22;\x07");
        assert_eq!(pointer_at(&tree, 2, 2, PointerMode::Terminal), None);
    }

    #[test]
    fn tab_capture_and_runtime_policy_switches_use_the_same_pointer_resolver() {
        use crate::core::event::{MouseButton, MouseEvent, MouseKind};
        use crate::prelude::*;
        struct App;
        impl Component for App {
            type State = PointerMode;
            type Message = PointerMode;
            type Properties = ();
            fn create_state(&self, _: &()) -> PointerMode {
                PointerMode::Interactive
            }
            fn update(&mut self, mode: PointerMode, ctx: &mut Context<Self>) -> Update {
                ctx.state = mode;
                Update::full()
            }
            fn view(&self, ctx: &Context<Self>) -> Element {
                ctx.set_pointer_mode(ctx.state);
                VStack::new()
                    .gap(0)
                    .child(
                        DraggableTabBar::new()
                            .tabs([DraggableTab::new("One"), DraggableTab::new("Two")])
                            .show_close_buttons(false)
                            .height(Length::Px(1))
                            .on_change(crate::Callback::new(|_| {}))
                            .on_reorder(crate::Callback::new(|_| {})),
                    )
                    .child(Text::new("Blank").height(Length::Flex(1)))
                    .into()
            }
        }
        let mut backend = crate::TestBackend::new(App);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        });
        backend.render();
        let mouse = |x, y, kind| MouseEvent {
            x,
            y,
            kind,
            mods: Default::default(),
        };
        backend.send_mouse(mouse(1, 0, MouseKind::Moved)).unwrap();
        assert_eq!(backend.pointer_shape(), Some(PointerShape::Pointer));
        backend
            .send_mouse(mouse(1, 0, MouseKind::Down(MouseButton::Left)))
            .unwrap();
        backend
            .send_mouse(mouse(25, 4, MouseKind::Drag(MouseButton::Left)))
            .unwrap();
        assert_eq!(backend.pointer_shape(), Some(PointerShape::Grabbing));
        for mode in [
            PointerMode::Disabled,
            PointerMode::Terminal,
            PointerMode::Explicit,
        ] {
            backend.dispatch(mode).unwrap();
            assert_eq!(
                backend.pointer_shape(),
                None,
                "mode {mode:?} suppresses automatic drag feedback"
            );
        }
        backend.dispatch(PointerMode::Interactive).unwrap();
        assert_eq!(backend.pointer_shape(), Some(PointerShape::Grabbing));
        backend
            .send_mouse(mouse(25, 4, MouseKind::Up(MouseButton::Left)))
            .unwrap();
        assert_eq!(backend.pointer_shape(), None);
        backend.send_mouse(mouse(1, 0, MouseKind::Moved)).unwrap();
        assert_eq!(backend.pointer_shape(), Some(PointerShape::Pointer));
    }
}
