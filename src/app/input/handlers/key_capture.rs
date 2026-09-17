//! KeyCapture keyboard handler.

use crate::core::event::KeyEvent;
use crate::core::node::{NodeId, NodeKind, NodeTree};

/// Offers a key to the focused KeyCapture, if one holds focus.
///
/// Dispatchers call this before any other key handling (pending chords, clipboard shortcuts,
/// overlay dismissal, app commands) so a shortcut recorder can record keys the framework would
/// otherwise claim. When it returns `true` the key is spent and the caller must drop any chord
/// state; declined keys continue through the normal pipeline, which does not offer them to the
/// KeyCapture a second time.
pub(crate) fn preflight(tree: &NodeTree, focused: Option<NodeId>, key: KeyEvent) -> bool {
    let Some(id) = focused.filter(|id| tree.is_valid(*id)) else {
        return false;
    };
    let NodeKind::KeyCapture(capture) = &tree.node(id).kind else {
        return false;
    };
    !capture.disabled
        && capture
            .on_key
            .as_ref()
            .is_some_and(|handler| handler.handle(key))
}
