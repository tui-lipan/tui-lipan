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
            let theme = node.active_theme();
            let caret = match &node.kind {
                NodeKind::TextArea(node) => {
                    if node.read_only {
                        None
                    } else {
                        let caret_shape = node.caret_shape.unwrap_or(theme.caret.shape);
                        let blinking = node.caret_blinking.unwrap_or(theme.caret.blinking);
                        if self.osc12_supported {
                            desired_cursor_color = node
                                .caret_color
                                .or(theme.caret.color)
                                .and_then(Color::to_rgb);
                        }
                        let caret_shape = if node.vim_motions && caret_shape == CaretShape::Block {
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
                            caret_shape
                        };
                        Some((caret_shape, blinking))
                    }
                }
                NodeKind::Input(node) => {
                    if node.read_only {
                        None
                    } else {
                        if self.osc12_supported {
                            desired_cursor_color = node
                                .caret_color
                                .or(theme.caret.color)
                                .and_then(Color::to_rgb);
                        }
                        Some((
                            node.caret_shape.unwrap_or(theme.caret.shape),
                            node.caret_blinking.unwrap_or(theme.caret.blinking),
                        ))
                    }
                }
                #[cfg(feature = "terminal")]
                NodeKind::Terminal(node) => {
                    if self.osc12_supported {
                        desired_cursor_color = node.caret_color.and_then(Color::to_rgb);
                    }
                    // Honor the child program's DECSCUSR shape. Blinking is driven
                    // by the framework blink timer in the terminal renderer, so the
                    // hardware cursor stays a steady shape here to avoid double blink.
                    if node.cursor_visible {
                        Some((node.cursor_shape, false))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            if let Some((caret_shape, blinking)) = caret
                && let Some(style) = cursor_style_for(caret_shape, blinking)
            {
                target_style = style;
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

/// Resolve a caret shape and blink preference to a `DECSCUSR` style.
///
/// Returns `None` for [`CaretShape::TerminalDefault`], which deliberately
/// declines to pick a style so the caller keeps `DefaultUserShape` and the
/// terminal's own cursor configuration stands. Blink is part of that
/// configuration, so `blinking` is ignored in that case.
fn cursor_style_for(shape: CaretShape, blinking: bool) -> Option<SetCursorStyle> {
    Some(match (shape, blinking) {
        (CaretShape::TerminalDefault, _) => return None,
        (CaretShape::Bar, false) => SetCursorStyle::SteadyBar,
        (CaretShape::Bar, true) => SetCursorStyle::BlinkingBar,
        (CaretShape::Block, false) => SetCursorStyle::SteadyBlock,
        (CaretShape::Block, true) => SetCursorStyle::BlinkingBlock,
        (CaretShape::Underline, false) => SetCursorStyle::SteadyUnderScore,
        (CaretShape::Underline, true) => SetCursorStyle::BlinkingUnderScore,
    })
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
    use crate::core::element::Element;
    use crate::core::node::NodeTree;
    use crate::layout::LayoutEngine;
    use crate::style::{CaretShape, Color, Rect, Theme};
    use crate::widgets::{Input, TextArea, TextAreaVimMode, ThemeProvider};

    use super::TerminalManager;

    fn text_area_tree() -> NodeTree {
        text_area_tree_with(TextArea::new("abc").vim_motions(true))
    }

    fn text_area_tree_with(text_area: TextArea) -> NodeTree {
        tree_with_theme(text_area, Theme::default())
    }

    fn tree_with_theme(child: impl Into<Element>, theme: Theme) -> NodeTree {
        let root: Element = ThemeProvider::new(theme).child(child).into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
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

    fn emitted(tree: &NodeTree) -> String {
        let mut manager = TerminalManager {
            osc12_supported: false,
            ..Default::default()
        };
        let mut out = Vec::new();
        manager
            .update_cursor(&mut out, tree, Some(tree.root), &HashMap::new())
            .unwrap();
        String::from_utf8_lossy(&out).into_owned()
    }

    #[test]
    fn caret_blinking_selects_the_blinking_decscusr_variant() {
        // CSI 1/3/5 q are the blinking block/underline/bar forms; their steady
        // counterparts are CSI 2/4/6 q.
        for (shape, expected) in [
            (CaretShape::Block, "\u{1b}[1 q"),
            (CaretShape::Underline, "\u{1b}[3 q"),
            (CaretShape::Bar, "\u{1b}[5 q"),
        ] {
            let tree = tree_with_theme(
                Input::new("abc").caret_shape(shape).caret_blinking(true),
                Theme::default(),
            );
            assert!(
                emitted(&tree).contains(expected),
                "{shape:?} blinking should emit {expected:?}"
            );
        }
    }

    #[test]
    fn caret_blinking_defaults_to_steady() {
        let tree = tree_with_theme(
            Input::new("abc").caret_shape(CaretShape::Bar),
            Theme::default(),
        );
        assert!(emitted(&tree).contains("\u{1b}[6 q"));
    }

    #[test]
    fn theme_caret_blinking_applies_when_the_widget_does_not_override_it() {
        let tree = tree_with_theme(
            Input::new("abc"),
            Theme::default()
                .caret_shape(CaretShape::Bar)
                .caret_blinking(true),
        );
        assert!(emitted(&tree).contains("\u{1b}[5 q"));
    }

    #[test]
    fn widget_caret_blinking_overrides_the_theme() {
        let tree = tree_with_theme(
            Input::new("abc").caret_blinking(false),
            Theme::default()
                .caret_shape(CaretShape::Bar)
                .caret_blinking(true),
        );
        assert!(emitted(&tree).contains("\u{1b}[6 q"));
    }

    #[test]
    fn terminal_default_shape_emits_the_user_shape_reset() {
        // CSI 0 q restores whatever the user configured in their terminal, and
        // blink is part of that configuration, so it must not be overridden.
        let tree = tree_with_theme(
            Input::new("abc")
                .caret_shape(CaretShape::TerminalDefault)
                .caret_blinking(true),
            Theme::default(),
        );
        let out = emitted(&tree);
        assert!(out.contains("\u{1b}[0 q"), "expected CSI 0 q, got {out:?}");
        for overridden in ["\u{1b}[1 q", "\u{1b}[5 q", "\u{1b}[6 q"] {
            assert!(
                !out.contains(overridden),
                "{overridden:?} should not be sent"
            );
        }
    }

    #[test]
    fn terminal_default_shape_from_the_theme_also_defers_to_the_terminal() {
        let tree = tree_with_theme(
            TextArea::new("abc"),
            Theme::default().caret_shape(CaretShape::TerminalDefault),
        );
        assert!(emitted(&tree).contains("\u{1b}[0 q"));
    }

    #[test]
    fn vim_block_swap_is_skipped_for_terminal_default() {
        // The insert-mode Block -> Bar swap keys off an explicit Block shape, so
        // deferring to the terminal must survive vim motions untouched.
        let tree = text_area_tree_with(
            TextArea::new("abc")
                .vim_motions(true)
                .caret_shape(CaretShape::TerminalDefault),
        );
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

        assert!(String::from_utf8_lossy(&out).contains("\u{1b}[0 q"));
    }

    #[test]
    fn theme_caret_shape_applies_to_input() {
        let tree = tree_with_theme(
            Input::new("abc"),
            Theme::default().caret_shape(CaretShape::Underline),
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

    #[test]
    fn theme_caret_shape_applies_to_text_area() {
        let tree = tree_with_theme(
            TextArea::new("abc"),
            Theme::default().caret_shape(CaretShape::Underline),
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

    #[test]
    fn explicit_caret_shape_overrides_theme_for_text_area() {
        let tree = tree_with_theme(
            TextArea::new("abc").caret_shape(CaretShape::Underline),
            Theme::default().caret_shape(CaretShape::Bar),
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

    #[test]
    fn theme_caret_color_applies_and_explicit_color_overrides_it() {
        let theme_color = Color::rgb(0x12, 0x34, 0x56);
        let explicit_color = Color::rgb(0xAB, 0xCD, 0xEF);
        let theme = Theme::default().caret_color(theme_color);
        let tree = tree_with_theme(Input::new("abc"), theme.clone());
        let mut manager = TerminalManager {
            osc12_supported: true,
            ..Default::default()
        };
        let mut out = Vec::new();

        manager
            .update_cursor(&mut out, &tree, Some(tree.root), &HashMap::new())
            .unwrap();
        assert!(String::from_utf8_lossy(&out).contains("\u{1b}]12;#123456\u{7}"));

        let tree = tree_with_theme(Input::new("abc").caret_color(explicit_color), theme);
        out.clear();
        manager
            .update_cursor(&mut out, &tree, Some(tree.root), &HashMap::new())
            .unwrap();
        assert!(String::from_utf8_lossy(&out).contains("\u{1b}]12;#abcdef\u{7}"));
    }

    #[cfg(feature = "terminal")]
    #[test]
    fn terminal_cursor_shape_follows_child_request() {
        use crate::widgets::Terminal;

        let term = Terminal::new()
            .cursor_shape(CaretShape::Bar)
            .cursor_blinking(false);
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &term.into(),
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 3,
            },
            None,
        );
        let mut manager = TerminalManager {
            osc12_supported: false,
            ..Default::default()
        };
        let mut out = Vec::new();

        manager
            .update_cursor(&mut out, &tree, Some(tree.root), &HashMap::new())
            .unwrap();

        // SteadyBar: blinking is handled by the framework blink timer, so the
        // hardware cursor style stays steady regardless of the blink request.
        assert!(String::from_utf8_lossy(&out).contains("\u{1b}[6 q"));
    }

    #[cfg(feature = "terminal")]
    #[test]
    fn terminal_cursor_color_uses_osc12() {
        use crate::style::Color;
        use crate::widgets::Terminal;

        let term = Terminal::new().caret_color(Color::rgb(0x12, 0x34, 0x56));
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &term.into(),
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 3,
            },
            None,
        );
        let mut manager = TerminalManager {
            osc12_supported: true,
            ..Default::default()
        };
        let mut out = Vec::new();

        manager
            .update_cursor(&mut out, &tree, Some(tree.root), &HashMap::new())
            .unwrap();

        assert!(String::from_utf8_lossy(&out).contains("\u{1b}]12;#123456\u{7}"));
    }
}
