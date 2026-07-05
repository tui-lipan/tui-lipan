use std::collections::HashMap;

use crossterm::cursor::SetCursorStyle;
use crossterm::execute;
use crossterm::style::Print;

use crate::Result;
use crate::app::input::text_area_vim::TextAreaVimState;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::{CaretShape, Color};
use crate::widgets::TextAreaVimMode;

pub(crate) struct TerminalManager {
    pub last_cursor_color: Option<(u8, u8, u8)>,
    pub osc12_supported: bool,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self {
            last_cursor_color: None,
            osc12_supported: supports_osc12_cursor_color(),
        }
    }
}

impl TerminalManager {
    pub fn update_cursor<B: std::io::Write>(
        &mut self,
        backend: &mut B,
        tree: &NodeTree,
        focused: Option<NodeId>,
        text_area_vim_state: &HashMap<NodeId, TextAreaVimState>,
    ) -> Result<()> {
        let mut target_style = SetCursorStyle::DefaultUserShape;
        let mut desired_cursor_color: Option<(u8, u8, u8)> = None;

        if let Some(id) = focused
            && tree.is_valid(id)
        {
            let node = tree.node(id);
            let caret = match &node.kind {
                NodeKind::TextArea(node) => {
                    if node.read_only {
                        None
                    } else {
                        if self.osc12_supported {
                            desired_cursor_color = node.caret_color.and_then(Color::to_rgb);
                        }
                        Some(
                            if node.vim_motions && node.caret_shape == CaretShape::Block {
                                match text_area_vim_state
                                    .get(&id)
                                    .map(|state| state.mode)
                                    .unwrap_or_default()
                                {
                                    TextAreaVimMode::Insert => CaretShape::Bar,
                                    TextAreaVimMode::Normal
                                    | TextAreaVimMode::Visual
                                    | TextAreaVimMode::VisualLine => CaretShape::Block,
                                }
                            } else {
                                node.caret_shape
                            },
                        )
                    }
                }
                NodeKind::Input(node) => {
                    if node.read_only {
                        None
                    } else {
                        if self.osc12_supported {
                            desired_cursor_color = node.caret_color.and_then(Color::to_rgb);
                        }
                        Some(node.caret_shape)
                    }
                }
                #[cfg(feature = "terminal")]
                NodeKind::Terminal(node) => {
                    if node.cursor_visible {
                        Some(CaretShape::Block)
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some(caret_shape) = caret {
                match caret_shape {
                    CaretShape::Bar => target_style = SetCursorStyle::SteadyBar,
                    CaretShape::Block => target_style = SetCursorStyle::SteadyBlock,
                    CaretShape::Underline => target_style = SetCursorStyle::SteadyUnderScore,
                }
            }
        }

        if desired_cursor_color != self.last_cursor_color {
            if let Some((r, g, b)) = desired_cursor_color {
                execute!(
                    backend,
                    Print(format!("\x1b]12;#{r:02x}{g:02x}{b:02x}\x07"))
                )?;
            } else if self.last_cursor_color.is_some() {
                execute!(backend, Print("\x1b]112\x07"))?;
            }
            self.last_cursor_color = desired_cursor_color;
        }

        execute!(backend, target_style)?;
        Ok(())
    }

    pub fn reset_cursor<B: std::io::Write>(&mut self, backend: &mut B) -> Result<()> {
        if self.last_cursor_color.is_some() {
            execute!(backend, Print("\x1b]112\x07"))?;
            self.last_cursor_color = None;
        }
        Ok(())
    }
}

fn supports_osc12_cursor_color() -> bool {
    if let Ok(value) = std::env::var("TUI_LIPAN_OSC12") {
        let value = value.trim().to_ascii_lowercase();
        if matches!(value.as_str(), "0" | "false" | "off" | "no") {
            return false;
        }
        if matches!(value.as_str(), "1" | "true" | "on" | "yes") {
            return true;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::app::input::text_area_vim::TextAreaVimState;
    use crate::core::node::NodeTree;
    use crate::layout::LayoutEngine;
    use crate::style::{CaretShape, Rect};
    use crate::widgets::{TextArea, TextAreaVimMode};

    use super::TerminalManager;

    fn text_area_tree() -> NodeTree {
        text_area_tree_with(TextArea::new("abc").vim_motions(true))
    }

    fn text_area_tree_with(text_area: TextArea) -> NodeTree {
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &text_area.into(),
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 3,
            },
            None,
        );
        tree
    }

    #[test]
    fn vim_text_area_cursor_shape_defaults_to_block() {
        let tree = text_area_tree();
        let mut manager = TerminalManager {
            osc12_supported: false,
            ..Default::default()
        };
        let mut out = Vec::new();

        manager
            .update_cursor(&mut out, &tree, Some(tree.root), &HashMap::new())
            .unwrap();

        assert!(String::from_utf8_lossy(&out).contains("\u{1b}[2 q"));
    }

    #[test]
    fn vim_text_area_insert_cursor_shape_is_bar() {
        let tree = text_area_tree();
        let mut manager = TerminalManager {
            osc12_supported: false,
            ..Default::default()
        };
        let mut state = HashMap::new();
        state.insert(
            tree.root,
            TextAreaVimState {
                mode: TextAreaVimMode::Insert,
                ..Default::default()
            },
        );
        let mut out = Vec::new();

        manager
            .update_cursor(&mut out, &tree, Some(tree.root), &state)
            .unwrap();

        assert!(String::from_utf8_lossy(&out).contains("\u{1b}[6 q"));
    }

    #[test]
    fn vim_text_area_non_default_caret_shape_overrides_mode_shape() {
        let tree = text_area_tree_with(
            TextArea::new("abc")
                .vim_motions(true)
                .caret_shape(CaretShape::Underline),
        );
        let mut manager = TerminalManager {
            osc12_supported: false,
            ..Default::default()
        };
        let mut out = Vec::new();

        manager
            .update_cursor(&mut out, &tree, Some(tree.root), &HashMap::new())
            .unwrap();

        assert!(String::from_utf8_lossy(&out).contains("\u{1b}[4 q"));
    }
}
