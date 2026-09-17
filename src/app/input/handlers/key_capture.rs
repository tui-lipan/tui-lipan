//! KeyCapture keyboard handler.

use crate::core::event::KeyEvent;
use crate::core::node::{NodeId, NodeKind, NodeTree};

/// Hand a key to a focused KeyCapture's handler. Declined keys bubble.
pub(crate) fn handle_key(tree: &NodeTree, id: NodeId, key: KeyEvent) -> bool {
    let NodeKind::KeyCapture(capture) = &tree.node(id).kind else {
        return false;
    };
    capture
        .on_key
        .as_ref()
        .is_some_and(|handler| handler.handle(key))
}
