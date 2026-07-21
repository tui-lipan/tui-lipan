use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::event::MouseEvent;
use crate::core::node::{ScrollbarZone, ScrollbarZonesParams, WidgetNode, compute_scrollbar_zones};
use crate::style::{
    BorderStyle, Padding, Rect, ScrollbarVariant, Style, StyleSlot, Theme, ThemeRole,
};
use crate::widgets::list::{ListEvent, ListItem, ListSymbolPosition};
/// A realized list node.
#[derive(Clone)]
pub struct ListNode {
    pub items: Arc<[ListItem]>,
    pub selected: Option<usize>,
    pub scroll_keys: crate::widgets::ScrollKeymap,
    pub scroll_wheel: bool,
    /// Computed scroll offset (top visible index).
    pub offset: usize,
    /// Optional scroll offset override for scrollbar drags.
    pub scroll_override: Option<usize>,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub item_hover_style: StyleSlot,
    pub active_style: StyleSlot,
    pub selection_style: StyleSlot,
    pub unfocused_selection_style: StyleSlot,
    pub active_symbol: Option<Arc<str>>,
    pub active_symbol_position: ListSymbolPosition,
    pub active_symbol_style: Option<Style>,
    pub selection_symbol: Option<Arc<str>>,
    pub selection_symbol_right: Option<Arc<str>>,
    pub selection_symbol_style: Option<Style>,
    pub unfocused_selection_symbol_style: Option<Style>,
    pub unselected_symbol: Option<Arc<str>>,
    pub symbol_column: bool,
    pub gutter_gap: u16,
    pub gutter_for_non_selectable: bool,
    pub selection_full_width: bool,
    pub item_horizontal_padding: Padding,
    pub header_horizontal_padding: Padding,
    pub border: bool,
    pub border_style: BorderStyle,
    pub title: Option<Arc<str>>,
    pub title_style: Style,
    pub padding: Padding,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_thumb: Option<char>,
    pub scrollbar_thumb_style: Option<Style>,
    pub scrollbar_thumb_focus_style: Option<Style>,
    pub scrollbar_track_style: Option<Style>,
    pub show_scroll_indicators: bool,
    pub scroll_indicator_style: Style,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
    pub bottom_count: usize,
    pub disabled: bool,
    pub disabled_style: Style,
    pub empty_text: Option<Arc<str>>,
    pub empty_text_style: Style,
    pub on_select: Option<Callback<ListEvent>>,
    pub on_item_click: Option<Callback<ListEvent>>,
    pub on_activate: Option<Callback<ListEvent>>,
    pub on_click: Option<Callback<MouseEvent>>,
    pub activate_on_click: bool,
    pub on_scroll_to: Option<Callback<usize>>,
    pub on_key: Option<KeyHandler>,
    pub focusable: bool,
    pub tab_stop: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
}

impl WidgetNode for ListNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn is_tab_stop(&self) -> bool {
        self.focusable && self.tab_stop
    }

    fn on_focus_callback(&self) -> Option<&Callback<()>> {
        self.on_focus.as_ref()
    }

    fn on_blur_callback(&self) -> Option<&Callback<()>> {
        self.on_blur.as_ref()
    }

    fn has_on_click(&self) -> bool {
        !self.disabled
            && (self.on_click.is_some()
                || self.on_select.is_some()
                || self.on_scroll_to.is_some()
                || self.scrollbar
                || self.scroll_wheel)
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        // Only hoverable if explicitly styled for hover, or has an on_click handler.
        // Having on_select/on_scroll_to does not make the widget hoverable since there's
        // no visual feedback for those interactions.
        !self.disabled
            && (self.on_click.is_some()
                || self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
                || self
                    .item_hover_style
                    .resolves_non_empty(theme, ThemeRole::ItemHover))
    }

    fn scrollbar_zones(
        &self,
        id: crate::core::node::NodeId,
        rect: Rect,
        parent_border_x: Option<i16>,
        _parent_border_y: Option<i16>,
    ) -> Vec<ScrollbarZone> {
        if !self.scrollbar || self.disabled {
            return Vec::new();
        }

        let inner = rect.inner(self.border, self.padding);
        if inner.w == 0 || inner.h == 0 {
            return Vec::new();
        }

        compute_scrollbar_zones(ScrollbarZonesParams {
            id,
            rect,
            inner,
            border: self.border,
            scrollbar: self.scrollbar,
            scrollbar_variant: self.scrollbar_variant,
            scrollbar_gap: self.scrollbar_gap,
            h_scrollbar: false,
            h_scrollbar_variant: ScrollbarVariant::default(),
            content_x: inner.x,
            content_width: inner.w,
            max_content_width: 0,
            wrap: false,
            parent_border_x,
            parent_border_y: None,
        })
    }
}
