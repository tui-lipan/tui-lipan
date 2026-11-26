//! The caret the focused widget asks for, shared by the runtime and headless captures.

use crate::app::focus_service::{FocusStackEntry, OverlayKey};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::{CaretShape, Color};
use crate::widgets::TextAreaVimMode;

/// The caret the focused widget asks for, before the terminal's capabilities are considered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FocusedCaret {
    pub shape: CaretShape,
    pub blinking: bool,
    pub color: Option<Color>,
    /// The widget blinks the caret itself, by placing it only on lit frames.
    pub framework_blinks: bool,
}

/// Resolve the caret of the focused widget, or `None` when it shows none.
///
/// `vim_mode` is the focused text area's Vim mode, which turns a block caret into a bar in
/// insert mode.
pub(crate) fn focused_caret(
    tree: &NodeTree,
    focused: Option<NodeId>,
    vim_mode: Option<TextAreaVimMode>,
) -> Option<FocusedCaret> {
    let id = focused.filter(|&id| tree.is_valid(id))?;
    let node = tree.node(id);
    let theme = node.active_theme();
    match &node.kind {
        NodeKind::TextArea(node) if !node.read_only => {
            let shape = node.caret_shape.unwrap_or(theme.caret.shape);
            let shape = if node.vim_motions && shape == CaretShape::Block {
                match vim_mode.unwrap_or_default() {
                    TextAreaVimMode::Insert => CaretShape::Bar,
                    TextAreaVimMode::Normal
                    | TextAreaVimMode::Visual
                    | TextAreaVimMode::VisualLine => CaretShape::Block,
                }
            } else {
                shape
            };
            Some(FocusedCaret {
                shape,
                blinking: node.caret_blinking.unwrap_or(theme.caret.blinking),
                color: node.caret_color.or(theme.caret.color),
                framework_blinks: !shape.is_terminal_default(),
            })
        }
        NodeKind::Input(node) if !node.read_only => {
            let shape = node.caret_shape.unwrap_or(theme.caret.shape);
            Some(FocusedCaret {
                shape,
                blinking: node.caret_blinking.unwrap_or(theme.caret.blinking),
                color: node.caret_color.or(theme.caret.color),
                framework_blinks: !shape.is_terminal_default(),
            })
        }
        // Honor the child program's DECSCUSR shape.
        #[cfg(feature = "terminal")]
        NodeKind::Terminal(node) if node.cursor_visible => Some(FocusedCaret {
            shape: node.cursor_shape,
            blinking: node.cursor_blinking,
            color: node.caret_color,
            framework_blinks: true,
        }),
        _ => None,
    }
}

/// Static carets must remain lit even if another widget left the blink timer in its off phase.
/// TerminalDefault explicitly leaves both shape and blinking to the host terminal.
pub(crate) fn caret_is_lit(tree: &NodeTree, id: NodeId, blink_visible: bool) -> bool {
    focused_caret(tree, Some(id), None)
        .is_none_or(|caret| !caret.framework_blinks || !caret.blinking || blink_visible)
}

/// If keyboard focus currently sits inside a selection-preserving popover (such as a
/// `ContextMenu`) that suspended focus from an editable text widget (`TextArea` or `Input`)
/// with no active selection, return that underlying widget's `NodeId` so it can keep a
/// static (non-blinking) insertion caret visible while the menu is open.
pub(crate) fn preserved_caret_node(
    tree: &NodeTree,
    focused: Option<NodeId>,
    focus_stack: &[FocusStackEntry],
) -> Option<NodeId> {
    if focused_caret(tree, focused, None).is_some() {
        return None;
    }
    for entry in focus_stack.iter().rev() {
        if !overlay_preserves_selection(tree, entry.overlay) {
            return None;
        }
        let candidate = entry.focused.filter(|&id| tree.is_valid(id)).or_else(|| {
            let key = entry.key.as_ref()?;
            tree.iter()
                .find(|node| node.key.as_ref() == Some(key))
                .map(|node| node.id)
        });
        if let Some(id) = candidate
            && is_editable_text_without_selection(tree, id)
        {
            return Some(id);
        }
    }
    None
}

/// Resolve the node that owns the active caret: the focused node when it displays a caret,
/// otherwise the suspended underlying editor (`static_caret_node`) when present.
pub(crate) fn caret_owner_node(
    tree: &NodeTree,
    focused: Option<NodeId>,
    static_caret_node: Option<NodeId>,
) -> Option<NodeId> {
    focused
        .filter(|&id| tree.is_valid(id) && focused_caret(tree, Some(id), None).is_some())
        .or_else(|| static_caret_node.filter(|&id| tree.is_valid(id)))
}

/// Resolve the active caret, falling back to `static_caret_node` (with blinking forced off)
/// when the focused widget itself shows no caret.
///
/// Returns `(caret, is_static)`.
pub(crate) fn active_caret(
    tree: &NodeTree,
    focused: Option<NodeId>,
    static_caret_node: Option<NodeId>,
    vim_mode: Option<TextAreaVimMode>,
) -> Option<(FocusedCaret, bool)> {
    if let Some(caret) = focused_caret(tree, focused, vim_mode) {
        return Some((caret, false));
    }
    let id = static_caret_node.filter(|&id| tree.is_valid(id))?;
    let mut caret = focused_caret(tree, Some(id), vim_mode)?;
    caret.blinking = false;
    Some((caret, true))
}

fn overlay_preserves_selection(tree: &NodeTree, key: OverlayKey) -> bool {
    tree.overlay_roots().iter().any(|overlay| {
        if !overlay.captures_focus || OverlayKey::of(overlay) != key || !tree.is_valid(overlay.id) {
            return false;
        }
        let Some(parent_id) = tree.node(overlay.id).parent else {
            return false;
        };
        if !tree.is_valid(parent_id) {
            return false;
        }
        matches!(
            &tree.node(parent_id).kind,
            NodeKind::Popover(popover) if popover.preserve_selection && *popover.content == overlay.id
        )
    })
}

fn is_editable_text_without_selection(tree: &NodeTree, id: NodeId) -> bool {
    if !tree.is_valid(id) {
        return false;
    }
    match &tree.node(id).kind {
        NodeKind::TextArea(node) => {
            !node.disabled
                && !node.read_only
                && node.anchor.is_none_or(|anchor| anchor == node.cursor)
        }
        NodeKind::Input(node) => {
            !node.disabled
                && !node.read_only
                && node.anchor.is_none_or(|anchor| anchor == node.cursor)
        }
        _ => false,
    }
}
