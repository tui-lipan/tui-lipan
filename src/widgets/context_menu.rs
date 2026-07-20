//! Context menu widget.

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::{Element, IntoElement};
use crate::style::{BorderStyle, Length, Padding, ScrollbarConfig, Style, StyleSlot};
use crate::widgets::{List, ListItem, Popover, PopoverOffset, PopoverPlacement};

/// A popup menu widget.
#[derive(Clone)]
pub struct ContextMenu {
    trigger: Element,
    items: Vec<ListItem>,
    open: bool,
    on_select: Option<Callback<usize>>,
    on_close: Option<Callback<()>>,
    placement: PopoverPlacement,
    offset: PopoverOffset,
    clamp: bool,
    auto_flip: bool,
    anchor: Option<(u16, u16)>,
    width: Length,
    height: Length,
    border: bool,
    border_style: BorderStyle,
    padding: Padding,
    style: Style,
    selection_style: StyleSlot,
    unfocused_selection_style: StyleSlot,
    item_hover_style: Option<StyleSlot>,
    selection_symbol: Option<Arc<str>>,
    selection_symbol_style: Option<Style>,
    unfocused_selection_symbol_style: Option<Style>,
    scrollbar: bool,
    scrollbar_config: ScrollbarConfig,
}

impl ContextMenu {
    /// Create a new context menu.
    pub fn new(trigger: impl IntoElement) -> Self {
        Self {
            trigger: trigger.into(),
            items: Vec::new(),
            open: false,
            on_select: None,
            on_close: None,
            placement: PopoverPlacement::BelowStart,
            offset: PopoverOffset::ZERO,
            clamp: true,
            auto_flip: true,
            anchor: None,
            width: Length::Px(20),
            height: Length::Auto,
            border: true,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            style: Style::default(),
            selection_style: StyleSlot::Inherit,
            unfocused_selection_style: StyleSlot::Inherit,
            item_hover_style: None,
            selection_symbol: Some("> ".into()),
            selection_symbol_style: None,
            unfocused_selection_symbol_style: None,
            scrollbar: false,
            scrollbar_config: ScrollbarConfig::default(),
        }
    }

    /// Replace all menu items.
    pub fn items(mut self, items: impl IntoIterator<Item = impl Into<ListItem>>) -> Self {
        self.items = items.into_iter().map(Into::into).collect();
        self
    }

    /// Set open state.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Set selection callback.
    pub fn on_select(mut self, cb: Callback<usize>) -> Self {
        self.on_select = Some(cb);
        self
    }

    /// Set close callback.
    pub fn on_close(mut self, cb: Callback<()>) -> Self {
        self.on_close = Some(cb);
        self
    }

    /// Set popover placement.
    pub fn placement(mut self, placement: PopoverPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Set popover offset.
    pub fn offset(mut self, offset: impl Into<PopoverOffset>) -> Self {
        self.offset = offset.into();
        self
    }

    /// Clamp popover to the viewport bounds.
    pub fn clamp(mut self, clamp: bool) -> Self {
        self.clamp = clamp;
        self
    }

    /// Automatically flip placement when it overflows the viewport.
    pub fn auto_flip(mut self, auto_flip: bool) -> Self {
        self.auto_flip = auto_flip;
        self
    }

    /// Anchor the menu to an absolute position (content coordinates).
    pub fn anchor(mut self, anchor: Option<(u16, u16)>) -> Self {
        self.anchor = anchor;
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Draw a border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set highlight style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed highlight style.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed highlight style.
    pub fn inherit_selection_style(mut self) -> Self {
        self.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set highlight style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.selection_style = slot;
        self
    }

    /// Set highlight style while the menu list is not focused.
    pub fn unfocused_selection_style(mut self, style: Style) -> Self {
        self.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed highlight style while the menu list is not focused.
    pub fn extend_unfocused_selection_style(mut self, style: Style) -> Self {
        self.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed highlight style while the menu list is not focused.
    pub fn inherit_unfocused_selection_style(mut self) -> Self {
        self.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set unfocused highlight style slot directly for composite forwarding.
    pub fn unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.unfocused_selection_style = slot;
        self
    }

    /// Set hovered item style.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.item_hover_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed hovered item style.
    pub fn extend_item_hover_style(mut self, style: Style) -> Self {
        self.item_hover_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit the themed hovered item style.
    pub fn inherit_item_hover_style(mut self) -> Self {
        self.item_hover_style = Some(StyleSlot::Inherit);
        self
    }

    /// Set hovered item style slot directly for composite forwarding.
    pub fn item_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.item_hover_style = Some(slot);
        self
    }

    /// Set highlight symbol.
    pub fn selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.selection_symbol = symbol.map(Into::into);
        self
    }

    /// Set highlight symbol style.
    pub fn selection_symbol_style(mut self, style: Style) -> Self {
        self.selection_symbol_style = Some(style);
        self
    }

    /// Set highlight symbol style while the menu list is not focused.
    pub fn unfocused_selection_symbol_style(mut self, style: Style) -> Self {
        self.unfocused_selection_symbol_style = Some(style);
        self
    }

    /// Enable a scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.scrollbar_config = config;
        self
    }
}

impl From<ContextMenu> for Element {
    fn from(menu: ContextMenu) -> Self {
        let mut list = List::new()
            .items(menu.items)
            .border(menu.border)
            .border_style(menu.border_style)
            .padding(menu.padding)
            .style(menu.style)
            .selection_symbol_style(
                menu.selection_symbol_style
                    .unwrap_or(menu.selection_style.explicit_style().unwrap_or_default()),
            )
            .unfocused_selection_symbol_style(
                menu.unfocused_selection_symbol_style
                    .or_else(|| menu.unfocused_selection_style.explicit_style())
                    .unwrap_or(menu.selection_style.explicit_style().unwrap_or_default()),
            )
            .selection_symbol(menu.selection_symbol)
            .scrollbar(menu.scrollbar)
            .scrollbar_config(menu.scrollbar_config)
            .width(menu.width)
            .height(menu.height);
        list = list
            .selection_style_slot(menu.selection_style)
            .unfocused_selection_style_slot(menu.unfocused_selection_style)
            .item_hover_style_slot(menu.item_hover_style.unwrap_or(menu.selection_style));

        if let Some(cb) = menu.on_select {
            list = list.on_select(Callback::new(move |ev: crate::widgets::ListEvent| {
                cb.emit(ev.index)
            }));
        }

        Popover::new()
            .trigger(menu.trigger)
            .content(list)
            .open(menu.open)
            .placement(menu.placement)
            .offset(menu.offset)
            .clamp(menu.clamp)
            .auto_flip(menu.auto_flip)
            .min_trigger_width(false)
            .anchor(menu.anchor)
            .on_close(menu.on_close.unwrap_or_else(|| Callback::new(|_| {})))
            .into()
    }
}
