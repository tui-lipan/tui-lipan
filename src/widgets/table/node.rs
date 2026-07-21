use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::event::MouseEvent;
use crate::core::node::{ScrollbarZone, ScrollbarZonesParams, WidgetNode, compute_scrollbar_zones};
use crate::style::{
    BorderStyle, Padding, Rect, ScrollbarVariant, Style, StyleSlot, Theme, ThemeRole,
};
use crate::widgets::{ColumnWidth, ScrollKeymap, Table, TableEvent, TableRow};

/// A table node.
#[derive(Clone)]
pub struct TableNode {
    pub rows: Arc<[TableRow]>,
    pub header: Option<TableRow>,
    pub widths: Vec<ColumnWidth>,
    pub column_styles: Vec<Style>,
    pub row_styles: Vec<Style>,
    pub selected: Option<usize>,
    pub column_spacing: u16,
    pub row_gap: u16,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub item_hover_style: StyleSlot,
    pub alternating_row_style: Option<Style>,
    pub row_style_full_width: bool,
    pub selection_style: StyleSlot,
    pub selection_symbol: Option<Arc<str>>,
    pub selection_symbol_style: Option<Style>,
    pub unselected_symbol: Option<Arc<str>>,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_thumb: Option<char>,
    pub scrollbar_thumb_style: Option<Style>,
    pub scrollbar_thumb_focus_style: Option<Style>,
    pub scrollbar_track_style: Option<Style>,
    pub scroll_keys: ScrollKeymap,
    pub scroll_wheel: bool,
    pub offset: usize,
    pub scroll_override: Option<usize>,
    pub show_scroll_indicators: bool,
    pub scroll_indicator_style: Style,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
    pub bottom_count: usize,
    pub disabled: bool,
    pub disabled_style: Style,
    pub on_select: Option<Callback<TableEvent>>,
    pub on_activate: Option<Callback<TableEvent>>,
    pub on_click: Option<Callback<MouseEvent>>,
    pub on_scroll_to: Option<Callback<usize>>,
    pub on_key: Option<KeyHandler>,
    pub focusable: bool,
    pub tab_stop: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
    pub inspector: bool,
    pub inspector_key_style: Style,
    pub inspector_value_style: Style,
    pub inspector_section_style: Style,
    pub inspector_separator_style: Style,
    pub inspector_indent_size: u16,
    pub inspector_collapsed_symbol: Arc<str>,
    pub inspector_expanded_symbol: Arc<str>,
    pub inspector_separator_char: char,
}

impl WidgetNode for TableNode {
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

        let header_h = crate::widgets::table::table_header_reserved_height(
            self.header.as_ref(),
            self.rows.len(),
            self.row_gap,
        );
        let header_h_i16 = header_h.min(i16::MAX as u16) as i16;
        let scrollbar_inner = Rect {
            y: inner.y.saturating_add(header_h_i16),
            h: inner.h.saturating_sub(header_h),
            ..inner
        };
        if scrollbar_inner.h == 0 {
            return Vec::new();
        }

        compute_scrollbar_zones(ScrollbarZonesParams {
            id,
            rect,
            inner: scrollbar_inner,
            border: self.border,
            scrollbar: self.scrollbar,
            scrollbar_variant: self.scrollbar_variant,
            scrollbar_gap: self.scrollbar_gap,
            h_scrollbar: false,
            h_scrollbar_variant: ScrollbarVariant::default(),
            content_x: scrollbar_inner.x,
            content_width: scrollbar_inner.w,
            max_content_width: 0,
            wrap: false,
            parent_border_x,
            parent_border_y: None,
        })
    }
}

impl From<Table> for TableNode {
    fn from(value: Table) -> Self {
        Self {
            rows: value.rows,
            header: value.header,
            widths: value.widths,
            column_styles: value.column_styles,
            row_styles: value.row_styles,
            selected: value.selected,
            column_spacing: value.column_spacing,
            row_gap: value.row_gap,
            style: value.style,
            hover_style: value.hover_style,
            item_hover_style: value.item_hover_style,
            alternating_row_style: value.alternating_row_style,
            row_style_full_width: value.row_style_full_width,
            selection_style: value.selection_style,
            selection_symbol: value.selection_symbol,
            selection_symbol_style: value.selection_symbol_style,
            unselected_symbol: value.unselected_symbol,
            border: value.border,
            border_style: value.border_style,
            padding: value.padding,
            scrollbar: value.scrollbar,
            scrollbar_variant: value.scrollbar_config.variant,
            scrollbar_gap: value.scrollbar_config.gap,
            scrollbar_thumb: value.scrollbar_config.thumb,
            scrollbar_thumb_style: value.scrollbar_config.thumb_style,
            scrollbar_thumb_focus_style: value.scrollbar_config.thumb_focus_style,
            scrollbar_track_style: value.scrollbar_config.track_style,
            scroll_keys: value.scroll_keys,
            scroll_wheel: value.scroll_wheel,
            offset: 0,
            scroll_override: None,
            show_scroll_indicators: value.show_scroll_indicators,
            scroll_indicator_style: value.scroll_indicator_style,
            top_indicator: false,
            bottom_indicator: false,
            bottom_count: 0,
            disabled: value.disabled,
            disabled_style: value.disabled_style,
            on_select: value.on_select,
            on_activate: value.on_activate,
            on_click: value.on_click,
            on_scroll_to: value.on_scroll_to,
            on_key: value.on_key,
            focusable: value.focusable,
            tab_stop: value.tab_stop,
            on_focus: value.on_focus,
            on_blur: value.on_blur,
            inspector: value.inspector,
            inspector_key_style: value.inspector_key_style,
            inspector_value_style: value.inspector_value_style,
            inspector_section_style: value.inspector_section_style,
            inspector_separator_style: value.inspector_separator_style,
            inspector_indent_size: value.inspector_indent_size,
            inspector_collapsed_symbol: value.inspector_collapsed_symbol,
            inspector_expanded_symbol: value.inspector_expanded_symbol,
            inspector_separator_char: value.inspector_separator_char,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::core::node::{NodeId, WidgetNode};
    use crate::style::Rect;
    use crate::widgets::{Table, TableRow};

    #[test]
    fn scrollbar_zone_starts_after_header_gap() {
        let node = super::TableNode::from(
            Table::new()
                .header(["H"])
                .rows([TableRow::new(["A"]), TableRow::new(["B"])])
                .row_gap(2)
                .scrollbar(true),
        );

        let zones = node.scrollbar_zones(
            NodeId::default(),
            Rect {
                x: 0,
                y: 0,
                w: 10,
                h: 8,
            },
            None,
            None,
        );

        assert_eq!(zones.len(), 1);
        assert_eq!(zones[0].rect.y, 3);
        assert_eq!(zones[0].rect.h, 5);
    }
}
