//! Pointer policy shared by the real runtime and headless tests.
use crate::core::node::{Node, NodeId, NodeKind, NodeTree};
use crate::style::PointerShape;

pub(crate) fn has_pointer_target(node: &Node) -> bool {
    if node.pointer_shape.is_some() || node.is_interactive() || node.is_hoverable() {
        return true;
    }
    match node.kind {
        NodeKind::Input(_) | NodeKind::TextArea(_) => true,
        #[cfg(feature = "terminal")]
        NodeKind::Terminal(_) => true,
        _ => false,
    }
}

pub(crate) fn automatic_pointer(tree: &NodeTree, hit: NodeId) -> Option<PointerShape> {
    let mut current = Some(hit);
    while let Some(id) = current {
        let node = tree.node(id);
        if let Some(shape) = widget_pointer(node) {
            return Some(shape);
        }
        current = node.parent;
    }
    None
}

fn widget_pointer(node: &Node) -> Option<PointerShape> {
    use crate::core::node::WidgetNode;
    match &node.kind {
        NodeKind::Input(input) if !input.disabled => Some(PointerShape::Text),
        NodeKind::TextArea(textarea) if !textarea.disabled => Some(PointerShape::Text),
        NodeKind::Button(button) if !button.disabled => Some(PointerShape::Pointer),
        #[cfg(feature = "terminal")]
        NodeKind::Terminal(terminal) => terminal_pointer(terminal),
        _ if node.kind.has_on_click() => Some(PointerShape::Pointer),
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
}
