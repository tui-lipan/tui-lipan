//! Tree widget types.

use crate::callback::{Callback, KeyHandler};
use crate::core::event::KeyCode;
use crate::core::event::KeyEvent;
use crate::style::{Length, Style, StyleSlot};
use crate::utils::gradient::ColorGradient;
use crate::widgets::ScrollKeymap;
use crate::widgets::{FocusAccordion, ListItem};
use std::sync::Arc;

/// A path to a tree node (index-based).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TreePath(pub(crate) Arc<[usize]>);

impl TreePath {
    /// Access the path segments.
    pub fn segments(&self) -> &[usize] {
        &self.0
    }
}

impl From<Vec<usize>> for TreePath {
    fn from(value: Vec<usize>) -> Self {
        Self(value.into())
    }
}

impl AsRef<[usize]> for TreePath {
    fn as_ref(&self) -> &[usize] {
        &self.0
    }
}

/// Tree selection event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEvent {
    /// Visible row index.
    pub index: usize,
    /// Path to the selected node.
    pub path: TreePath,
}

/// Tree expand/collapse event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeToggleEvent {
    /// Visible row index.
    pub index: usize,
    /// Path to the toggled node.
    pub path: TreePath,
    /// New expanded state.
    pub expanded: bool,
}

/// Keyboard shortcuts for tree navigation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TreeKeymap(u8);

impl TreeKeymap {
    /// Disable key handling.
    pub const NONE: Self = Self(0);
    /// Arrow keys (Left/Right).
    pub const ARROWS: Self = Self(1 << 0);
    /// Vim-style h/l.
    pub const VIM: Self = Self(1 << 1);
    /// Toggle with Space.
    pub const TOGGLE: Self = Self(1 << 2);
    /// Default key set.
    pub const DEFAULT: Self = Self(Self::ARROWS.0 | Self::VIM.0 | Self::TOGGLE.0);

    /// Check if this keymap includes another set.
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl std::ops::BitOr for TreeKeymap {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for TreeKeymap {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for TreeKeymap {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::BitAndAssign for TreeKeymap {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl std::ops::Not for TreeKeymap {
    type Output = Self;

    fn not(self) -> Self {
        Self(!self.0)
    }
}

impl Default for TreeKeymap {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TreeAction {
    Expand,
    Collapse,
    Toggle,
}

pub(crate) fn tree_action_from_key(key: &KeyEvent, keymap: TreeKeymap) -> Option<TreeAction> {
    match key.code {
        KeyCode::Left if keymap.contains(TreeKeymap::ARROWS) => Some(TreeAction::Collapse),
        KeyCode::Right if keymap.contains(TreeKeymap::ARROWS) => Some(TreeAction::Expand),
        KeyCode::Char('h') if keymap.contains(TreeKeymap::VIM) => Some(TreeAction::Collapse),
        KeyCode::Char('l') if keymap.contains(TreeKeymap::VIM) => Some(TreeAction::Expand),
        KeyCode::Char(' ') if keymap.contains(TreeKeymap::TOGGLE) => Some(TreeAction::Toggle),
        _ => None,
    }
}

/// A node in a tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub(crate) item: ListItem,
    pub(crate) children: Vec<TreeNode>,
    pub(crate) expanded: bool,
    pub(crate) indent: u16,
    pub(crate) leading_guide_fill_cells: u16,
}

impl TreeNode {
    /// Create a new tree node.
    pub fn new(item: impl Into<ListItem>) -> Self {
        Self {
            item: item.into(),
            children: Vec::new(),
            expanded: false,
            indent: 2,
            leading_guide_fill_cells: 0,
        }
    }

    /// Add a child node.
    pub fn child(mut self, child: TreeNode) -> Self {
        self.children.push(child);
        self
    }

    /// Replace all children, discarding anything already added with
    /// [`child`](Self::child). Call `child` repeatedly to append instead.
    pub fn children(mut self, children: impl IntoIterator<Item = TreeNode>) -> Self {
        self.children = children.into_iter().collect();
        self
    }

    /// Set expanded state (initial).
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Set indentation per level (default 2).
    pub fn indent(mut self, indent: u16) -> Self {
        self.indent = indent;
        self
    }

    pub(crate) fn leading_guide_fill_cells(mut self, cells: u16) -> Self {
        self.leading_guide_fill_cells = cells;
        self
    }
}

/// Style of indentation guides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IndentStyle {
    /// No guides.
    #[default]
    None,
    /// Vertical lines only (│).
    Line,
    /// Short branch connectors (├, └).
    Short,
    /// Short branch connectors with rounded terminal elbows (├, ╰).
    ShortRounded,
    /// Long branch connectors (├─, └─).
    Long,
    /// Long branch connectors with rounded terminal elbows (├─, ╰─).
    LongRounded,
}

#[derive(Clone, PartialEq)]
pub(crate) struct TreeProps {
    pub root: TreeNode,
    pub selected: Option<usize>,
    pub clear_selection: bool,
    pub force_scroll_to_selected: bool,
    pub gap: u16,
    pub icon_gap: u16,
    pub show_icons: bool,
    pub expanded_icon: Arc<str>,
    pub collapsed_icon: Arc<str>,
    pub leaf_icon: Option<Arc<str>>,
    pub icon_style: Style,
    pub width: Length,
    pub height: Length,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub item_hover_style: StyleSlot,
    pub selection_style: StyleSlot,
    pub unfocused_selection_style: StyleSlot,
    pub selection_symbol: Option<Arc<str>>,
    pub selection_symbol_style: Option<Style>,
    pub unfocused_selection_symbol_style: Option<Style>,
    pub scrollbar: bool,
    pub scrollbar_config: crate::style::ScrollbarConfig,
    pub scroll_keys: ScrollKeymap,
    pub scroll_wheel: bool,
    pub show_scroll_indicators: bool,
    pub scroll_indicator_style: Style,
    pub empty_text: Option<Arc<str>>,
    pub empty_text_style: Style,
    pub activate_on_click: bool,
    pub focusable: bool,
    pub tab_stop: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
    pub on_select: Option<Callback<TreeEvent>>,
    pub on_activate: Option<Callback<TreeEvent>>,
    pub on_toggle: Option<Callback<TreeToggleEvent>>,
    pub keymap: TreeKeymap,
    pub focus_policy: Option<FocusAccordion>,
    pub indent_style: IndentStyle,
    pub indent_guide_style: Style,
    pub indent_gradient: Option<ColorGradient>,
    pub(crate) solid_indent_connector_gap: bool,
    pub selection_full_width: bool,
    pub unselected_symbol: Option<Arc<str>>,
    pub key_interceptor: Option<KeyHandler>,
}
