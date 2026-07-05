use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::event::MouseEvent;
use crate::core::node::WidgetNode;
use crate::style::{BorderStyle, FileIconPalette, Padding, Style, StyleSlot, Theme, ThemeRole};
use crate::widgets::TabsEvent;
use std::collections::HashMap;

use super::{
    DragReorderMode, DraggableTab, DraggableTabActionEvent, DraggableTabBar,
    DraggableTabBarOverflow, DraggableTabBarVariant, DraggableTabCloseEvent,
    DraggableTabReorderEvent, DraggableTabTransferEvent, TabDisplayOptions, TabViewportOptions,
};
use crate::utils::file_icons::FileIconOverride;
use crate::widgets::file_tree::FileIconStyle;

#[derive(Clone)]
pub struct DraggableTabBarNode {
    pub tabs: Arc<[DraggableTab]>,
    pub active: usize,
    pub style: Style,
    pub focus_style: StyleSlot,
    pub hover_style: StyleSlot,
    pub tab_hover_style: StyleSlot,
    pub active_style: StyleSlot,
    pub close_style: Style,
    pub close_hover_style: Style,
    pub divider: char,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub variant: DraggableTabBarVariant,
    pub accent_symbol: char,
    pub active_accent_symbol: char,
    pub accent_style: Style,
    pub active_accent_style: Style,
    pub close_symbol: Arc<str>,
    pub show_close_buttons: bool,
    pub close_on_hover_only: bool,
    pub tab_max_width: Option<u16>,
    pub overflow: DraggableTabBarOverflow,
    pub scroll_wheel: bool,
    pub show_overflow_controls: bool,
    pub overflow_style: Style,
    pub overflow_hover_style: Style,
    pub scroll_offset: usize,
    pub scroll_override: Option<usize>,
    pub previous_active: usize,
    pub show_file_icons: bool,
    pub file_icon_style: FileIconStyle,
    pub file_icon_palette: FileIconPalette,
    pub file_icon_overrides: HashMap<Arc<str>, FileIconOverride>,
    pub bar_id: Option<Arc<str>>,
    pub drag_group: Option<Arc<str>>,
    pub draggable: bool,
    pub drag_preview: bool,
    pub reorder_mode: DragReorderMode,
    pub drag_threshold: u16,
    pub on_change: Option<Callback<TabsEvent>>,
    pub on_action: Option<Callback<DraggableTabActionEvent>>,
    pub on_close: Option<Callback<DraggableTabCloseEvent>>,
    pub on_reorder: Option<Callback<DraggableTabReorderEvent>>,
    pub on_transfer: Option<Callback<DraggableTabTransferEvent>>,
    pub on_click: Option<Callback<MouseEvent>>,
    pub on_key: Option<KeyHandler>,
    pub disabled: bool,
    pub disabled_style: Style,
    pub focusable: bool,
}

impl DraggableTabBarNode {
    pub(crate) fn display_options(&self) -> TabDisplayOptions<'_> {
        TabDisplayOptions {
            variant: self.variant,
            divider: self.divider,
            accent_symbol: self.accent_symbol,
            close_symbol: &self.close_symbol,
            show_close_buttons: self.show_close_buttons,
            tab_max_width: self.tab_max_width,
            overflow: self.overflow,
            show_file_icons: self.show_file_icons,
            file_icon_style: self.file_icon_style,
            file_icon_palette: &self.file_icon_palette,
            file_icon_overrides: &self.file_icon_overrides,
        }
    }

    pub(crate) fn viewport_options(&self, viewport_width: usize) -> TabViewportOptions {
        TabViewportOptions {
            scroll_offset: self.scroll_offset,
            viewport_width,
            show_overflow_controls: self.show_overflow_controls,
        }
    }
}

impl WidgetNode for DraggableTabBarNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn has_on_click(&self) -> bool {
        !self.disabled
            && (self.on_click.is_some()
                || self.on_change.is_some()
                || self.on_action.is_some()
                || self.on_close.is_some()
                || self.on_reorder.is_some()
                || self.on_transfer.is_some()
                || self.show_overflow_controls
                || self.scroll_wheel)
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        if self.disabled {
            return false;
        }
        self.on_click.is_some()
            || self.on_action.is_some()
            || self.show_overflow_controls
            || self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
            || self
                .tab_hover_style
                .resolves_non_empty(theme, ThemeRole::ItemHover)
            || !self.close_hover_style.is_empty()
    }
}

impl From<DraggableTabBar> for DraggableTabBarNode {
    fn from(value: DraggableTabBar) -> Self {
        Self {
            tabs: value.tabs,
            active: value.active,
            style: value.style,
            focus_style: value.focus_style,
            hover_style: value.hover_style,
            tab_hover_style: value.tab_hover_style,
            active_style: value.active_style,
            close_style: value.close_style,
            close_hover_style: value.close_hover_style,
            divider: value.divider,
            border: value.border,
            border_style: value.border_style,
            padding: value.padding,
            variant: value.variant,
            accent_symbol: value.accent_symbol,
            active_accent_symbol: value.active_accent_symbol,
            accent_style: value.accent_style,
            active_accent_style: value.active_accent_style,
            close_symbol: value.close_symbol,
            show_close_buttons: value.show_close_buttons,
            close_on_hover_only: value.close_on_hover_only,
            tab_max_width: value.tab_max_width,
            overflow: value.overflow,
            scroll_wheel: value.scroll_wheel,
            show_overflow_controls: value.show_overflow_controls,
            overflow_style: value.overflow_style,
            overflow_hover_style: value.overflow_hover_style,
            scroll_offset: value.scroll_offset,
            scroll_override: None,
            previous_active: value.active,
            show_file_icons: value.show_file_icons,
            file_icon_style: value.file_icon_style,
            file_icon_palette: value.file_icon_palette,
            file_icon_overrides: value.file_icon_overrides,
            bar_id: value.bar_id,
            drag_group: value.drag_group,
            draggable: value.draggable,
            drag_preview: value.drag_preview,
            reorder_mode: value.reorder_mode,
            drag_threshold: value.drag_threshold,
            on_change: value.on_change,
            on_action: value.on_action,
            on_close: value.on_close,
            on_reorder: value.on_reorder,
            on_transfer: value.on_transfer,
            on_click: value.on_click,
            on_key: value.on_key,
            disabled: value.disabled,
            disabled_style: value.disabled_style,
            focusable: value.focusable,
        }
    }
}
