//! Clearing one widget's selection, shared by every backend.

use crate::core::node::{NodeId, NodeKind, NodeTree};
#[cfg(feature = "terminal")]
use crate::widgets::internal::apply_terminal_selection_input;

/// Clear the selection `id` holds, the way a click elsewhere does: through the widget's change
/// callback when it has one, so a controlled widget hears about it. Returns whether anything was
/// cleared.
pub(crate) fn clear_node_selection(tree: &mut NodeTree, id: NodeId) -> bool {
    if !tree.is_valid(id) {
        return false;
    }
    let mut dirty = false;
    let mut clear_input = None;
    let mut clear_text_area = None;
    let mut clear_hex_area = None;
    let mut clear_document_view = false;
    #[cfg(feature = "terminal")]
    let mut clear_terminal = None;

    {
        let node = tree.node(id);
        match &node.kind {
            NodeKind::Input(input) if input.anchor.is_some_and(|anchor| anchor != input.cursor) => {
                clear_input = Some((input.on_change.clone(), input.value.clone(), input.cursor));
            }
            NodeKind::TextArea(text_area)
                if text_area
                    .anchor
                    .is_some_and(|anchor| anchor != text_area.cursor) =>
            {
                clear_text_area = Some((
                    text_area.on_change.clone(),
                    text_area.value.clone(),
                    text_area.cursor,
                ));
            }
            NodeKind::HexArea(hex_area)
                if hex_area
                    .anchor
                    .is_some_and(|anchor| anchor != hex_area.cursor) =>
            {
                clear_hex_area = Some((hex_area.on_cursor_change.clone(), hex_area.cursor));
            }
            NodeKind::DocumentView(doc)
                if (doc.table_rect_selection.is_some()
                    || doc
                        .selection_anchor
                        .is_some_and(|anchor| anchor != doc.selection_cursor)) =>
            {
                clear_document_view = true;
            }
            #[cfg(feature = "terminal")]
            NodeKind::Terminal(term) if term.selection.is_some() => {
                clear_terminal = Some(term.on_selection.clone());
            }
            _ => {}
        }
    }

    if let Some((on_change, value, cursor)) = clear_input {
        if let Some(cb) = on_change {
            cb.emit(crate::widgets::InputEvent {
                value,
                cursor,
                anchor: None,
            });
        } else if let NodeKind::Input(node) = &mut tree.node_mut(id).kind {
            node.anchor = None;
        }
        dirty = true;
    }

    if let Some((on_change, value, cursor)) = clear_text_area {
        if let Some(cb) = on_change {
            cb.emit(crate::widgets::TextAreaEvent {
                value,
                cursor,
                anchor: None,
            });
        } else if let NodeKind::TextArea(node) = &mut tree.node_mut(id).kind {
            node.anchor = None;
        }
        dirty = true;
    }

    if let Some((on_cursor_change, cursor)) = clear_hex_area {
        if let Some(cb) = on_cursor_change {
            cb.emit(crate::widgets::HexAreaCursorEvent {
                cursor,
                anchor: None,
            });
        } else if let NodeKind::HexArea(node) = &mut tree.node_mut(id).kind {
            node.anchor = None;
        }
        dirty = true;
    }

    if clear_document_view {
        if let NodeKind::DocumentView(node) = &mut tree.node_mut(id).kind {
            node.selection_anchor = None;
            node.table_rect_selection = None;
        }
        dirty = true;
    }

    #[cfg(feature = "terminal")]
    if let Some(on_selection) = clear_terminal {
        if let NodeKind::Terminal(node) = &mut tree.node_mut(id).kind {
            apply_terminal_selection_input(&mut node.selection, node.selection_controlled, None);
        }
        if let Some(cb) = on_selection {
            cb.emit(crate::widgets::TerminalSelectionEvent {
                selection: None,
                text: None,
                pointer: None,
            });
        }
        dirty = true;
    }
    dirty
}
