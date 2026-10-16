//! Tree widget types.

use crate::callback::{Callback, KeyHandler};
use crate::core::element::Element;
use crate::core::event::{KeyCode, KeyEvent, KeyMods};
use crate::style::{Length, Span, Style, StyleSlot};
use crate::utils::gradient::ColorGradient;
use crate::widgets::ScrollKeymap;
use crate::widgets::{FocusAccordion, ListItem};
use std::rc::Rc;
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
#[non_exhaustive]
pub struct TreeEvent {
    /// Visible row index.
    pub index: usize,
    /// Path to the selected node.
    pub path: TreePath,
}

/// Tree expand/collapse event.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
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
    if key.mods != KeyMods::NONE {
        return None;
    }
    match key.code {
        KeyCode::Left if keymap.contains(TreeKeymap::ARROWS) => Some(TreeAction::Collapse),
        KeyCode::Right if keymap.contains(TreeKeymap::ARROWS) => Some(TreeAction::Expand),
        KeyCode::Char('h') if keymap.contains(TreeKeymap::VIM) => Some(TreeAction::Collapse),
        KeyCode::Char('l') if keymap.contains(TreeKeymap::VIM) => Some(TreeAction::Expand),
        KeyCode::Char(' ') if keymap.contains(TreeKeymap::TOGGLE) => Some(TreeAction::Toggle),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, mods: KeyMods) -> KeyEvent {
        KeyEvent { code, mods }
    }

    #[test]
    fn tree_navigation_ignores_modified_keys() {
        let keymap = TreeKeymap::DEFAULT;
        assert_eq!(
            tree_action_from_key(&key(KeyCode::Left, KeyMods::NONE), keymap),
            Some(TreeAction::Collapse)
        );
        assert_eq!(
            tree_action_from_key(
                &key(
                    KeyCode::Left,
                    KeyMods {
                        ctrl: true,
                        ..KeyMods::NONE
                    }
                ),
                keymap
            ),
            None
        );
    }
}

/// A composed tree row's immutable visual content.
#[derive(Clone)]
pub(crate) struct TreeContent(Rc<Element>);

impl std::fmt::Debug for TreeContent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TreeContent(..)")
    }
}
impl PartialEq for TreeContent {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl Eq for TreeContent {}
impl TreeContent {
    pub(super) fn element(&self) -> Element {
        self.0.as_ref().clone()
    }
}

/// Pointer entering or leaving a tree row, including non-selectable rows.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct TreeHoverEvent {
    /// Visible row index.
    pub index: usize,
    /// Index path to the row.
    pub path: TreePath,
    /// Whether the pointer is within the row or an interactive descendant.
    pub hovered: bool,
}

/// Whether a tree node infers collapsibility, reserves it for lazy children, or stays open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TreeExpansionMode {
    /// Nodes with children are collapsible; empty nodes are leaves.
    #[default]
    Auto,
    /// Allow expansion and collapse even before children are loaded.
    Collapsible,
    /// Always show children, without a disclosure column or collapse actions.
    AlwaysOpen,
}

/// A node in a tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNode {
    pub(crate) item: ListItem,
    pub(crate) content: Option<TreeContent>,
    pub(crate) leading: Option<TreeContent>,
    pub(crate) content_height: u16,
    pub(crate) show_disclosure: bool,
    pub(crate) key: Option<Arc<str>>,
    pub(crate) children: Vec<TreeNode>,
    pub(crate) expanded: bool,
    pub(crate) expansion_mode: TreeExpansionMode,
    pub(crate) indent: u16,
    /// Unused disclosure cells, which branch guides may fill before the separator.
    pub(crate) disclosure_placeholder_width: u16,
    /// A disclosure supplied by FileTree, before item icons and labels.
    pub(crate) disclosure: Vec<Span>,
}

impl TreeNode {
    /// Create a new tree node.
    pub fn new(item: impl Into<ListItem>) -> Self {
        Self {
            item: item.into(),
            content: None,
            leading: None,
            content_height: 1,
            show_disclosure: true,
            key: None,
            children: Vec::new(),
            expanded: false,
            expansion_mode: TreeExpansionMode::Auto,
            indent: 2,
            disclosure_placeholder_width: 0,
            disclosure: Vec::new(),
        }
    }

    /// Show or reserve the node's disclosure column. Disable for structural headings.
    pub fn show_disclosure(mut self, show: bool) -> Self {
        self.show_disclosure = show;
        self
    }

    /// Supply composed row content. The tree still owns guides, disclosure, and navigation.
    /// Interactive descendants may consume clicks independently of row activation.
    pub fn content(mut self, content: impl Into<Element>) -> Self {
        self.content = Some(TreeContent(Rc::new(content.into())));
        self
    }

    /// A visual column before the tree guides, such as an active-row marker.
    pub fn leading(mut self, content: impl Into<Element>) -> Self {
        self.leading = Some(TreeContent(Rc::new(content.into())));
        self
    }

    /// Base style for the entire composed row, including its tree prefix.
    pub fn row_style(mut self, style: Style) -> Self {
        self.item.style = style;
        self
    }

    /// Height of composed content in terminal rows, including its detail lines. Default 1.
    pub fn content_height(mut self, height: u16) -> Self {
        self.content_height = height.max(1);
        self
    }

    /// Stable reconciliation identity for composed content, independent of its index path.
    pub fn key(mut self, key: impl Into<Arc<str>>) -> Self {
        self.key = Some(key.into());
        self
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

    /// Choose expansion behavior (default [`TreeExpansionMode::Auto`]).
    /// [`TreeExpansionMode::AlwaysOpen`] ignores [`Self::expanded`] and suppresses disclosure.
    pub fn expansion_mode(mut self, mode: TreeExpansionMode) -> Self {
        self.expansion_mode = mode;
        self
    }

    pub(crate) fn is_collapsible(&self) -> bool {
        match self.expansion_mode {
            TreeExpansionMode::Auto => !self.children.is_empty(),
            TreeExpansionMode::Collapsible => true,
            TreeExpansionMode::AlwaysOpen => false,
        }
    }

    /// Set indentation per level (default 2).
    pub fn indent(mut self, indent: u16) -> Self {
        self.indent = indent;
        self
    }

    pub(crate) fn disclosure_placeholder(mut self, cells: u16) -> Self {
        self.disclosure_placeholder_width = cells;
        self
    }

    pub(crate) fn disclosure(mut self, spans: Vec<Span>) -> Self {
        self.disclosure = spans;
        self
    }
}

/// Style of indentation guides.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IndentStyle {
    /// No guides; indentation still reserves its configured width.
    #[default]
    None,
    /// Vertical lines (│), including the last sibling.
    Line,
    /// Vertical lines with a square cap (└) on the last sibling.
    LineCapped,
    /// Vertical lines with a rounded cap (╰) on the last sibling.
    LineCappedRounded,
    /// Branch connectors (├, └), filled horizontally to the indentation width.
    Branch,
    /// Branch connectors with rounded terminal elbows (├, ╰).
    BranchRounded,
}

#[derive(Clone, PartialEq)]
pub(crate) struct TreeProps {
    pub root: TreeNode,
    pub forest: bool,
    pub item_hover_effect: Option<crate::style::VisualEffect>,
    pub controlled_expansion: bool,
    pub toggle_on_activate: bool,
    pub on_hover: Option<Callback<TreeHoverEvent>>,
    pub on_pointer_move: Option<Callback<TreeEvent>>,
    pub on_viewport_change: Option<Callback<crate::widgets::ScrollViewportEvent>>,
    pub selected: Option<usize>,
    pub clear_selection: bool,
    pub force_scroll_to_selected: bool,
    pub gap: u16,
    pub content_gap: u16,
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
    pub on_ctrl_activate: Option<Callback<TreeEvent>>,
    pub on_toggle: Option<Callback<TreeToggleEvent>>,
    pub keymap: TreeKeymap,
    pub focus_policy: Option<FocusAccordion>,
    pub focus_key: Option<Arc<str>>,
    pub indent_style: IndentStyle,
    pub indent_guide_style: Style,
    pub indent_gradient: Option<ColorGradient>,
    pub selection_full_width: bool,
    pub unselected_symbol: Option<Arc<str>>,
    pub key_interceptor: Option<KeyHandler>,
    pub indent_guide_start_depth: usize,
}
