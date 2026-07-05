use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::event::MouseEvent;
use crate::core::node::WidgetNode;
use crate::style::{BorderStyle, CaretShape, Color, Padding, Style, StyleSlot, Theme, ThemeRole};
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
    pub caret_shape: CaretShape,
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
    pub tab_order: bool,
}

impl WidgetNode for InputNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn in_tab_order(&self) -> bool {
        self.focusable && self.tab_order
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
