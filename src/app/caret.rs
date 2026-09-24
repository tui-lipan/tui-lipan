//! The caret the focused widget asks for, shared by the runtime and headless captures.

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
                framework_blinks: false,
            })
        }
        NodeKind::Input(node) if !node.read_only => Some(FocusedCaret {
            shape: node.caret_shape.unwrap_or(theme.caret.shape),
            blinking: node.caret_blinking.unwrap_or(theme.caret.blinking),
            color: node.caret_color.or(theme.caret.color),
            framework_blinks: false,
        }),
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
