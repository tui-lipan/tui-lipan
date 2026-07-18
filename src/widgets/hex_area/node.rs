use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{BorderStyle, Padding, Style, StyleSlot, Theme, ThemeRole};
use crate::widgets::ScrollEvent;

use super::{HexArea, HexAreaChangeEvent, HexAreaCursorEvent, HexAreaEditEvent};

#[derive(Clone)]
pub struct HexAreaNode {
    pub bytes: Arc<[u8]>,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub read_only: bool,
    pub bytes_per_row: u16,
    pub show_ascii: bool,
    pub show_offsets: bool,
    pub uppercase_hex: bool,
    pub scroll_offset: Option<usize>,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub focus_style: StyleSlot,
    pub focus_content_style: Style,
    pub selection_style: StyleSlot,
    pub cursor_style: Style,
    pub pending_edit_style: Style,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub disabled: bool,
    pub disabled_style: Style,
    pub focusable: bool,
    pub tab_stop: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
    pub on_cursor_change: Option<Callback<HexAreaCursorEvent>>,
    pub on_change: Option<Callback<HexAreaChangeEvent>>,
    pub on_edit: Option<Callback<HexAreaEditEvent>>,
    pub on_scroll: Option<Callback<ScrollEvent>>,
    pub pending_high_nibble: Option<u8>,
    pub on_key: Option<KeyHandler>,
}

impl WidgetNode for HexAreaNode {
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

    fn is_hoverable(&self) -> bool {
        !self.disabled && self.hover_style.has_explicit_style()
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        !self.disabled && self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
    }
}

impl From<HexArea> for HexAreaNode {
    fn from(value: HexArea) -> Self {
        Self {
            bytes: value.bytes,
            cursor: value.cursor,
            anchor: value.anchor,
            read_only: value.read_only,
            bytes_per_row: value.bytes_per_row.max(1),
            show_ascii: value.show_ascii,
            show_offsets: value.show_offsets,
            uppercase_hex: value.uppercase_hex,
            scroll_offset: value.scroll_offset,
            style: value.style,
            hover_style: value.hover_style,
            focus_style: value.focus_style,
            focus_content_style: value.focus_content_style,
            selection_style: value.selection_style,
            cursor_style: value.cursor_style,
            pending_edit_style: value.pending_edit_style,
            border: value.border,
            border_style: value.border_style,
            padding: value.padding,
            disabled: value.disabled,
            disabled_style: value.disabled_style,
            focusable: value.focusable,
            tab_stop: value.tab_stop,
            on_focus: value.on_focus,
            on_blur: value.on_blur,
            on_cursor_change: value.on_cursor_change,
            on_change: value.on_change,
            on_edit: value.on_edit,
            on_scroll: value.on_scroll,
            pending_high_nibble: None,
            on_key: value.on_key,
        }
    }
}

impl From<HexAreaNode> for NodeKind {
    fn from(value: HexAreaNode) -> Self {
        NodeKind::HexArea(value)
    }
}
