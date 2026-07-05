use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::node::{
    NodeId, ScrollbarZone, ScrollbarZonesParams, WidgetNode, compute_scrollbar_zones,
};
use crate::style::{
    BorderStyle, Padding, Rect, ScrollbarVariant, Span, Style, StyleSlot, Theme, ThemeRole,
};
use crate::widgets::ScrollEvent;

use super::events::{
    MouseModeState, TerminalInputEvent, TerminalSelection, TerminalSelectionEvent,
};
use super::layout::terminal_content_layout;

/// Runtime node for terminal rendering.
#[derive(Clone)]
pub(crate) struct TerminalNode {
    pub lines: Arc<[Vec<Span>]>,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub cursor_visible: bool,
    pub selection: Option<TerminalSelection>,
    pub selection_style: StyleSlot,
    pub mouse_mode: MouseModeState,
    pub on_selection: Option<Callback<TerminalSelectionEvent>>,
    pub on_mouse_forward: Option<Callback<Vec<u8>>>,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub focus_style: StyleSlot,
    pub focus_content_style: Style,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub scrollback_offset: usize,
    pub total_scrollback_rows: usize,
    pub viewport_rows: usize,
    pub viewport_cols: usize,
    pub scroll_wheel: bool,
    pub scroll_override: Option<usize>,
    pub scrollbar: bool,
    pub scrollbar_variant: ScrollbarVariant,
    pub scrollbar_gap: u16,
    pub scrollbar_thumb: Option<char>,
    pub scrollbar_thumb_style: Option<Style>,
    pub scrollbar_thumb_focus_style: Option<Style>,
    pub scrollbar_track_style: Option<Style>,
    pub on_scroll: Option<Callback<ScrollEvent>>,
    pub on_scroll_to: Option<Callback<usize>>,
    pub focusable: bool,
    pub on_key: Option<KeyHandler>,
    pub on_input: Option<Callback<TerminalInputEvent>>,
}

impl WidgetNode for TerminalNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn has_on_click(&self) -> bool {
        self.on_scroll.is_some()
            || self.on_scroll_to.is_some()
            || self.scrollbar
            || self.scroll_wheel
    }

    fn is_hoverable(&self) -> bool {
        self.hover_style.has_explicit_style()
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
    }

    fn scrollbar_zones(
        &self,
        id: NodeId,
        rect: Rect,
        parent_border_x: Option<i16>,
        _parent_border_y: Option<i16>,
    ) -> Vec<ScrollbarZone> {
        if !self.scrollbar {
            return Vec::new();
        }

        let inner = rect.inner(self.border, self.padding);
        if inner.w == 0 || inner.h == 0 {
            return Vec::new();
        }

        let layout = terminal_content_layout(
            inner,
            self.border,
            self.scrollbar,
            self.scrollbar_variant,
            self.scrollbar_gap,
            self.total_scrollback_rows,
            parent_border_x.is_some(),
        );
        if !layout.scrollbar_visible {
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
