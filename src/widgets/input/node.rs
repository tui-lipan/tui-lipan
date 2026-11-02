use std::sync::Arc;

use unicode_width::UnicodeWidthStr;

use crate::callback::{Callback, KeyHandler};
use crate::core::event::MouseEvent;
use crate::core::node::WidgetNode;
use crate::style::{
    BorderStyle, CaretShape, Color, Padding, Rect, Style, StyleSlot, Theme, ThemeRole,
};
use crate::text::edit::TextEditEvent;
use crate::widgets::InputEvent;

/// A realized input node.
#[derive(Clone)]
pub struct InputNode {
    pub value: Arc<str>,
    pub cursor: usize,
    pub anchor: Option<usize>,
    pub placeholder: Option<Arc<str>>,
    pub prefix: Option<Arc<str>>,
    pub prefix_style: Style,
    pub focus_prefix_style: Style,
    pub suffix: Option<Arc<str>>,
    pub suffix_style: Style,
    pub focus_suffix_style: Style,
    pub truncate_head: bool,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub focus_style: StyleSlot,
    pub focus_content_style: Style,
    pub hover_border_style: Option<BorderStyle>,
    pub placeholder_style: Style,
    pub focus_placeholder_style: Style,
    pub caret_shape: Option<CaretShape>,
    pub caret_blinking: Option<bool>,
    pub caret_color: Option<Color>,
    pub selection_style: StyleSlot,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub mask: Option<char>,
    pub disabled: bool,
    pub disabled_style: Style,
    pub read_only: bool,
    pub error: Option<Arc<str>>,
    pub error_style: Style,
    pub reserve_error_row: bool,
    pub on_change: Option<Callback<InputEvent>>,
    pub on_edit: Option<Callback<TextEditEvent>>,
    pub on_click: Option<Callback<MouseEvent>>,
    pub on_key: Option<KeyHandler>,
    /// Pre-insertion key interceptor. Runs before text insertion in the editable
    /// path. If it returns `true` the key is consumed and text insertion is
    /// skipped entirely. `on_key` is NOT called when this returns `true`.
    pub key_interceptor: Option<KeyHandler>,
    pub focusable: bool,
    pub tab_stop: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
    pub history_scope: Option<Arc<str>>,
}

impl InputNode {
    /// Whether a mouse press can place a caret or start a selection in this input.
    pub(crate) fn accepts_pointer_text(&self) -> bool {
        !self.disabled && (self.on_change.is_some() || self.read_only)
    }

    /// The cells of the field that take text: inside the border, between any prefix and suffix.
    ///
    /// Padding counts unless an affix stands between it and the value, as in a browser's input box.
    pub(crate) fn pointer_text_rect(&self, rect: Rect) -> Rect {
        let field = rect.inner(self.border, Padding::default());
        let inner = rect.inner(self.border, self.padding);
        let width = |text: &Option<Arc<str>>| {
            text.as_deref()
                .filter(|text| !text.is_empty())
                .map(|text| UnicodeWidthStr::width(text).min(u16::MAX as usize) as i16)
        };
        let inner_right = inner.x.saturating_add(inner.w as i16);
        let left = width(&self.prefix).map_or(field.x, |w| inner.x.saturating_add(w));
        let right = width(&self.suffix).map_or_else(
            || field.x.saturating_add(field.w as i16),
            |w| inner_right.saturating_sub(w),
        );
        Rect {
            x: left,
            w: right.saturating_sub(left).max(0) as u16,
            ..field
        }
    }
}

impl WidgetNode for InputNode {
    fn is_disabled(&self) -> bool {
        self.disabled
    }
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
            && (self.on_click.is_some() || self.on_change.is_some() || self.on_key.is_some())
    }
    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        // Only hoverable if explicitly styled for hover, or has an on_click handler.
        // Having on_change/on_key does not make the widget hoverable since there's
        // no visual feedback for those interactions.
        !self.disabled
            && (self.on_click.is_some()
                || self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
                || self.hover_border_style.is_some())
    }
}
