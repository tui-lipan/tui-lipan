//! Checkbox keyboard handler.

use crate::callback::KeyHandler;
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::CheckboxEvent;

/// Handle keyboard input for a focused Checkbox node.
pub(crate) fn handle_key(tree: &NodeTree, id: NodeId, key: KeyEvent) -> bool {
    let node = tree.node(id);
    let NodeKind::Checkbox(node) = &node.kind else {
        return false;
    };

    let handle_key = |handler: &KeyHandler| -> bool { handler.handle(key) };

    let mut handled = false;
    if matches!(key.code, KeyCode::Enter | KeyCode::Char(' '))
        && let Some(cb) = node.on_toggle.as_ref()
    {
        cb.emit(CheckboxEvent {
            state: node.state.toggle(),
        });
        handled = true;
    }
    if !handled {
        handled = node.on_key.as_ref().map(&handle_key).unwrap_or(false);
    }
    handled
}
