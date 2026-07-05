//! Button keyboard handler.

use crate::core::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseKind};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::Rect;

/// Handle keyboard input for a focused Button node.
pub(crate) fn handle_key(tree: &NodeTree, id: NodeId, key: KeyEvent) -> bool {
    let node = tree.node(id);
    let NodeKind::Button(button) = &node.kind else {
        return false;
    };

    let rect = node.rect;
    let disabled = button.disabled;
    let on_click = button.on_click.clone();
    let on_key = button.on_key.clone();

    if on_key.as_ref().is_some_and(|handler| handler.handle(key)) {
        return true;
    }

    if disabled || !(key.is(KeyCode::Enter) || key.is(KeyCode::Char(' '))) {
        return false;
    }

    let Some(on_click) = on_click else {
        return false;
    };

    on_click.emit(keyboard_activation_event(rect, key));
    true
}

fn keyboard_activation_event(rect: Rect, key: KeyEvent) -> MouseEvent {
    let (x, y) = rect_center(rect).unwrap_or((0, 0));
    MouseEvent {
        x,
        y,
        kind: MouseKind::Up(MouseButton::Left),
        mods: key.mods,
    }
}

fn rect_center(rect: Rect) -> Option<(u16, u16)> {
    if rect.is_empty() {
        return None;
    }

    let x = i32::from(rect.x) + i32::from(rect.w / 2);
    let y = i32::from(rect.y) + i32::from(rect.h / 2);
    Some((clamp_to_u16(x), clamp_to_u16(y)))
}

fn clamp_to_u16(value: i32) -> u16 {
    value.clamp(0, i32::from(u16::MAX)) as u16
}
