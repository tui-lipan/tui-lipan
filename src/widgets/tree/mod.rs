//! Tree widget.

mod component;
mod types;

pub(crate) use component::*;
pub use types::*;

use crate::callback::Callback;
use crate::core::element::Element;
use crate::style::{Length, ScrollbarConfig, Style, StyleSlot};
use crate::utils::gradient::ColorGradient;
use crate::widgets::FocusAccordion;
use crate::widgets::ScrollKeymap;
use std::sync::Arc;

/// A hierarchical tree view.
#[derive(Clone)]
pub struct Tree {
    props: TreeProps,
}

impl Tree {
    /// Create a new tree with the given root node.
    pub fn new(root: TreeNode) -> Self {
        Self {
            props: TreeProps {
                root,
                selected: None,
                force_scroll_to_selected: false,
                gap: 0,
                icon_gap: 1,
                show_icons: true,
                expanded_icon: "▼".into(),
                collapsed_icon: "▶".into(),
                leaf_icon: None,
                icon_style: Style::default(),
                width: Length::Flex(1),
                height: Length::Flex(1),
                style: Style::default(),
                hover_style: StyleSlot::Inherit,
                item_hover_style: StyleSlot::Inherit,
                selection_style: StyleSlot::Inherit,
                unfocused_selection_style: StyleSlot::Inherit,
                selection_symbol: None,
                selection_symbol_style: None,
                unfocused_selection_symbol_style: None,
                scrollbar: false,
                scrollbar_config: ScrollbarConfig::default(),
                scroll_keys: ScrollKeymap::default(),
                scroll_wheel: true,
                show_scroll_indicators: false,
                scroll_indicator_style: Style::default(),
                empty_text: None,
                empty_text_style: Style::default(),
                activate_on_click: true,
                focusable: true,
                tab_stop: true,
                on_focus: None,
                on_blur: None,
                on_select: None,
                on_activate: None,
                on_toggle: None,
                keymap: TreeKeymap::default(),
                focus_policy: None,
                indent_style: IndentStyle::None,
                indent_guide_style: Style::default(),
                indent_gradient: None,
                solid_indent_connector_gap: false,
                selection_full_width: false,
                unselected_symbol: None,
                key_interceptor: None,
            },
        }
    }

    /// Set style of indentation guides.
    pub fn indent_style(mut self, style: IndentStyle) -> Self {
        self.props.indent_style = style;
        self
    }

    /// Set style for indent guides.
    pub fn indent_guide_style(mut self, style: Style) -> Self {
        self.props.indent_guide_style = style;
        self
    }

    /// Set a depth-based gradient for indentation guides.
    pub fn indent_gradient(mut self, gradient: ColorGradient) -> Self {
        self.props.indent_gradient = Some(gradient);
        self
    }

    pub(crate) fn solid_indent_connector_gap(mut self, solid: bool) -> Self {
        self.props.solid_indent_connector_gap = solid;
        self
    }

    /// Set whether the highlight should span the full width of the tree.
    pub fn selection_full_width(mut self, full_width: bool) -> Self {
        self.props.selection_full_width = full_width;
        self
    }

    /// Set symbol for unselected items.
    pub fn unselected_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.props.unselected_symbol = symbol.map(Into::into);
        self
    }

    /// Set vertical gap between rows.
    pub fn gap(mut self, gap: u16) -> Self {
        self.props.gap = gap;
        self
    }

    /// Set the selected visible row index.
    pub fn selected(mut self, selected: usize) -> Self {
        self.props.selected = Some(selected);
        self
    }

    /// Force scroll to make the selected item visible on next render.
    pub fn force_scroll_to_selected(mut self, force: bool) -> Self {
        self.props.force_scroll_to_selected = force;
        self
    }

    /// Set horizontal gap between icon and content.
    pub fn icon_gap(mut self, gap: u16) -> Self {
        self.props.icon_gap = gap;
        self
    }

    /// Toggle showing expand/collapse icons.
    pub fn show_icons(mut self, show: bool) -> Self {
        self.props.show_icons = show;
        self
    }

    /// Set expanded icon.
    pub fn expanded_icon(mut self, icon: impl Into<Arc<str>>) -> Self {
        self.props.expanded_icon = icon.into();
        self
    }

    /// Set collapsed icon.
    pub fn collapsed_icon(mut self, icon: impl Into<Arc<str>>) -> Self {
        self.props.collapsed_icon = icon.into();
        self
    }

    /// Set leaf icon.
    pub fn leaf_icon(mut self, icon: Option<impl Into<Arc<str>>>) -> Self {
        self.props.leaf_icon = icon.map(Into::into);
        self
    }

    /// Set icon style.
    pub fn icon_style(mut self, style: Style) -> Self {
        self.props.icon_style = style;
        self
    }

    /// Set base style for the list.
    pub fn style(mut self, style: Style) -> Self {
        self.props.style = style;
        self
    }

    /// Set hover style for the list.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.props.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hover style for the list.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.props.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed hover style for the list.
    pub fn inherit_hover_style(mut self) -> Self {
        self.props.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hover style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.hover_style = slot;
        self
    }

    /// Set item hover style.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.props.item_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed item-hover style.
    pub fn extend_item_hover_style(mut self, style: Style) -> Self {
        self.props.item_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed item-hover style.
    pub fn inherit_item_hover_style(mut self) -> Self {
        self.props.item_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set item-hover style slot directly for composite forwarding.
    pub fn item_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.item_hover_style = slot;
        self
    }

    /// Set highlight style for the selected row.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.props.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed highlight style for the selected row.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.props.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed highlight style for the selected row.
    pub fn inherit_selection_style(mut self) -> Self {
        self.props.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set selection style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.selection_style = slot;
        self
    }

    /// Set highlight style while the tree is not focused.
    pub fn unfocused_selection_style(mut self, style: Style) -> Self {
        self.props.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed highlight style while the tree is not focused.
    pub fn extend_unfocused_selection_style(mut self, style: Style) -> Self {
        self.props.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed highlight style while the tree is not focused.
    pub fn inherit_unfocused_selection_style(mut self) -> Self {
        self.props.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set unfocused-selection style slot directly for composite forwarding.
    pub fn unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.unfocused_selection_style = slot;
        self
    }

    /// Set highlight symbol.
    pub fn selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.props.selection_symbol = symbol.map(Into::into);
        self
    }

    /// Set highlight symbol style.
    pub fn selection_symbol_style(mut self, style: Option<Style>) -> Self {
        self.props.selection_symbol_style = style;
        self
    }

    /// Set highlight symbol style while the tree is not focused.
    pub fn unfocused_selection_symbol_style(mut self, style: Option<Style>) -> Self {
        self.props.unfocused_selection_symbol_style = style;
        self
    }

    /// Set requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.props.width = width;
        self
    }

    /// Set requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.props.height = height;
        self
    }

    /// Draw a scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.props.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.props.scrollbar_config = config;
        self
    }

    /// Configure which keys move selection.
    pub fn scroll_keys(mut self, keys: ScrollKeymap) -> Self {
        self.props.scroll_keys = keys;
        self
    }

    /// Enable mouse wheel scrolling.
    pub fn scroll_wheel(mut self, enabled: bool) -> Self {
        self.props.scroll_wheel = enabled;
        self
    }

    /// Enable "N more" scroll indicators when items are hidden.
    pub fn show_scroll_indicators(mut self, show: bool) -> Self {
        self.props.show_scroll_indicators = show;
        self
    }

    /// Set style for scroll indicators.
    pub fn scroll_indicator_style(mut self, style: Style) -> Self {
        self.props.scroll_indicator_style = style;
        self
    }

    /// Set text to display when the tree is empty.
    pub fn empty_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.props.empty_text = Some(text.into());
        self
    }

    /// Set style for empty text.
    pub fn empty_text_style(mut self, style: Style) -> Self {
        self.props.empty_text_style = style;
        self
    }

    /// Control activation on mouse click.
    pub fn activate_on_click(mut self, activate: bool) -> Self {
        self.props.activate_on_click = activate;
        self
    }

    /// Control whether the tree is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.props.focusable = focusable;
        self
    }

    /// Control whether the tree participates in tab focus traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.props.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the tree gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.props.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the tree loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.props.on_blur = Some(cb);
        self
    }

    /// Set selection callback.
    pub fn on_select(mut self, cb: Callback<TreeEvent>) -> Self {
        self.props.on_select = Some(cb);
        self
    }

    /// Set activation callback (Enter or click when `activate_on_click` is true).
    pub fn on_activate(mut self, cb: Callback<TreeEvent>) -> Self {
        self.props.on_activate = Some(cb);
        self
    }

    /// Set toggle callback.
    pub fn on_toggle(mut self, cb: Callback<TreeToggleEvent>) -> Self {
        self.props.on_toggle = Some(cb);
        self
    }

    /// Configure keymap for expand/collapse.
    pub fn keymap(mut self, keymap: TreeKeymap) -> Self {
        self.props.keymap = keymap;
        self
    }

    /// Configure focus-aware collapsing for large trees.
    pub fn focus_policy(mut self, policy: FocusAccordion) -> Self {
        self.props.focus_policy = Some(policy);
        self
    }

    /// Set a key interceptor that runs before internal tree key handling.
    ///
    /// If the interceptor returns `true`, the key is consumed and the tree's
    /// built-in expand/collapse handling is skipped.
    pub fn key_interceptor(mut self, handler: crate::callback::KeyHandler) -> Self {
        self.props.key_interceptor = Some(handler);
        self
    }
}

impl From<Tree> for Element {
    fn from(tree: Tree) -> Self {
        crate::child(TreeComponent::new, tree.props)
    }
}
