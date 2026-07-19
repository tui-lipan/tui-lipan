mod layout;
mod node;
mod reconcile;

use std::sync::Arc;

pub(crate) use layout::measure_input;
pub use node::InputNode;
pub(crate) use reconcile::reconcile_input;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::core::event::MouseEvent;
use crate::style::{
    BorderStyle, CaretShape, Color, LayoutConstraints, Length, Padding, Style, StyleSlot,
};
use crate::text::edit::TextEditEvent;
use crate::text::input::TextInput;

/// A single-line text input.
///
/// Recommended state binding:
/// `Input::bound(&state).on_change(ctx.link().callback(Msg::Changed))`
/// and then `ev.apply_to(&mut state)` in the handler.
/// Storing only the raw text value breaks cursor navigation and selection across rerenders.
#[derive(Clone)]
pub struct Input {
    pub(crate) value: Arc<str>,
    pub(crate) cursor: usize,
    pub(crate) anchor: Option<usize>,
    pub(crate) placeholder: Option<Arc<str>>,
    pub(crate) prefix: Option<Arc<str>>,
    pub(crate) suffix: Option<Arc<str>>,
    pub(crate) truncate_head: bool,
    pub(crate) style: Style,
    pub(crate) hover_style: StyleSlot,
    pub(crate) focus_style: StyleSlot,
    pub(crate) focus_content_style: Style,
    pub(crate) hover_border_style: Option<BorderStyle>,
    pub(crate) placeholder_style: Style,
    pub(crate) focus_placeholder_style: Style,
    pub(crate) prefix_style: Style,
    pub(crate) focus_prefix_style: Style,
    pub(crate) suffix_style: Style,
    pub(crate) focus_suffix_style: Style,
    pub(crate) caret_shape: CaretShape,
    pub(crate) caret_color: Option<Color>,
    pub(crate) selection_style: StyleSlot,
    pub(crate) border: bool,
    /// Border style.
    /// Default: `BorderStyle::Plain`.
    pub(crate) border_style: BorderStyle,
    /// Padding.
    /// Default: `Padding { left: 1, right: 1, top: 0, bottom: 0 }`.
    pub(crate) padding: Padding,
    pub(crate) mask: Option<char>,
    pub(crate) disabled: bool,
    pub(crate) disabled_style: Style,
    pub(crate) read_only: bool,
    pub(crate) error: Option<Arc<str>>,
    pub(crate) error_style: Style,
    pub(crate) reserve_error_row: bool,
    /// Requested width.
    /// Default: `Length::Flex(1)`.
    pub(crate) width: Length,
    /// Requested height.
    /// Default: `Length::Auto`.
    pub(crate) height: Length,
    pub(crate) on_change: Option<Callback<InputEvent>>,
    pub(crate) on_edit: Option<Callback<TextEditEvent>>,
    pub(crate) on_click: Option<Callback<MouseEvent>>,
    pub(crate) on_key: Option<KeyHandler>,
    pub(crate) key_interceptor: Option<KeyHandler>,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) on_focus: Option<Callback<()>>,
    pub(crate) on_blur: Option<Callback<()>>,
}

impl Input {
    /// Create a new input.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        let value = value.into();
        let cursor = value.len();
        Self {
            value,
            cursor,
            anchor: None,
            placeholder: None,
            prefix: None,
            suffix: None,
            truncate_head: false,
            style: Style::default(),
            hover_style: StyleSlot::Inherit,
            focus_style: StyleSlot::Inherit,
            focus_content_style: Style::default(),
            hover_border_style: None,
            placeholder_style: Style::default(),
            focus_placeholder_style: Style::default(),
            prefix_style: Style::default(),
            focus_prefix_style: Style::default(),
            suffix_style: Style::default(),
            focus_suffix_style: Style::default(),
            caret_shape: CaretShape::default(),
            caret_color: None,
            selection_style: StyleSlot::Inherit,
            border: true,
            border_style: BorderStyle::Plain,
            padding: Padding {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0,
            },
            mask: None,
            disabled: false,
            disabled_style: Style::default(),
            read_only: false,
            error: None,
            error_style: Style::default(),
            reserve_error_row: false,
            width: Length::Flex(1),
            height: Length::Auto,
            on_change: None,
            on_edit: None,
            on_click: None,
            on_key: None,
            key_interceptor: None,
            focusable: true,
            tab_stop: true,
            on_focus: None,
            on_blur: None,
        }
    }

    /// Create a new input bound to a [`TextInput`] state bundle.
    pub fn bound(state: &TextInput) -> Self {
        Self::new("").bind(state)
    }

    /// Set the cursor position (byte index).
    pub fn cursor(mut self, cursor: usize) -> Self {
        self.cursor = cursor;
        self
    }

    /// Set the selection anchor position (byte index).
    /// When set, text between anchor and cursor is selected.
    pub fn anchor(mut self, anchor: Option<usize>) -> Self {
        self.anchor = anchor;
        self
    }

    /// Bind the input's value, cursor, and anchor from a [`TextInput`] state bundle.
    pub fn bind(mut self, state: &TextInput) -> Self {
        self.value = state.text().into();
        self.cursor = state.cursor();
        self.anchor = state.anchor();
        self
    }

    /// Set the placeholder (shown when empty and not focused).
    pub fn placeholder(mut self, placeholder: impl Into<Arc<str>>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Set the prefix displayed before the input content.
    pub fn prefix(mut self, prefix: impl Into<Arc<str>>) -> Self {
        self.prefix = Some(prefix.into());
        self
    }

    /// Set the suffix displayed after the input content.
    pub fn suffix(mut self, suffix: impl Into<Arc<str>>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    /// Toggle leading truncation when content overflows.
    pub fn truncate_head(mut self, truncate_head: bool) -> Self {
        self.truncate_head = truncate_head;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set style when hovered.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's hover style with additional fields.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit hover style from the active theme.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hover style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_style = slot;
        self
    }

    /// Set chrome/surface style when focused.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's focus style with additional fields.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit focus style from the active theme.
    pub fn inherit_focus_style(mut self) -> Self {
        self.focus_style = StyleSlot::Inherit;
        self
    }

    /// Set focus style slot directly for composite forwarding.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_style = slot;
        self
    }

    /// Set content text style when focused.
    pub fn focus_content_style(mut self, style: Style) -> Self {
        self.focus_content_style = style;
        self
    }

    /// Set border style when hovered.
    pub fn hover_border_style(mut self, border_style: BorderStyle) -> Self {
        self.hover_border_style = Some(border_style);
        self
    }

    /// Set placeholder style.
    pub fn placeholder_style(mut self, style: Style) -> Self {
        self.placeholder_style = style;
        self
    }

    /// Set placeholder style when focused.
    pub fn focus_placeholder_style(mut self, style: Style) -> Self {
        self.focus_placeholder_style = style;
        self
    }

    /// Set prefix style.
    pub fn prefix_style(mut self, style: Style) -> Self {
        self.prefix_style = style;
        self
    }

    /// Set prefix style when focused.
    pub fn focus_prefix_style(mut self, style: Style) -> Self {
        self.focus_prefix_style = style;
        self
    }

    /// Set suffix style.
    pub fn suffix_style(mut self, style: Style) -> Self {
        self.suffix_style = style;
        self
    }

    /// Set suffix style when focused.
    pub fn focus_suffix_style(mut self, style: Style) -> Self {
        self.focus_suffix_style = style;
        self
    }

    /// Set caret shape (bar, block, or underline).
    pub fn caret_shape(mut self, shape: CaretShape) -> Self {
        self.caret_shape = shape;
        self
    }

    /// Set caret color (only used for block caret rendering).
    pub fn caret_color(mut self, color: Color) -> Self {
        self.caret_color = Some(color);
        self
    }

    /// Set selection highlight style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's selection style with additional fields.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit selection style from the active theme.
    pub fn inherit_selection_style(mut self) -> Self {
        self.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set selection style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.selection_style = slot;
        self
    }

    /// Set border.
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

    /// Set mask character (e.g. '*' for passwords).
    pub fn mask(mut self, mask: Option<char>) -> Self {
        self.mask = mask;
        self
    }

    /// Override requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Override requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Callback fired when the input value or cursor changes.
    pub fn on_change(mut self, cb: Callback<InputEvent>) -> Self {
        self.on_change = Some(cb);
        self
    }

    /// Callback fired with incremental edit information.
    pub fn on_edit(mut self, cb: Callback<TextEditEvent>) -> Self {
        self.on_edit = Some(cb);
        self
    }

    /// Set on-click handler.
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set on-key handler.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }

    /// Set a pre-insertion key interceptor.
    ///
    /// This handler runs **before** text insertion in the editable path (Phase 3).
    /// If it returns `true`, the key is consumed and neither text insertion nor
    /// `on_key` will fire. Use this to intercept character keys (e.g. spacebar)
    /// that would otherwise be inserted into the input.
    pub fn key_interceptor(mut self, handler: KeyHandler) -> Self {
        self.key_interceptor = Some(handler);
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Set read-only mode. Allows mouse selection but blocks keyboard input.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Set error message to display below the input.
    pub fn error<S>(mut self, message: Option<S>) -> Self
    where
        S: Into<Arc<str>>,
    {
        self.error = message.map(Into::into);
        self
    }

    /// Set style for the error message text.
    pub fn error_style(mut self, style: Style) -> Self {
        self.error_style = style;
        self
    }

    /// Reserve a dedicated row for error text even when no error is present.
    pub fn reserve_error_row(mut self, reserve_error_row: bool) -> Self {
        self.reserve_error_row = reserve_error_row;
        self
    }

    /// Control whether the node is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// When `false`, the input stays focusable but is skipped by Tab / Shift+Tab traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the input gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the input loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
        self
    }
}

impl From<Input> for Element {
    fn from(value: Input) -> Self {
        let mut layout = LayoutConstraints::default();
        if value.focusable {
            let (min_w, min_h) = measure_input(&value);
            layout.focus_min_w = min_w;
            // Also set strict minimum height constraints to prevent collapse
            // in tight layouts (especially Flex containers).
            layout.min_h = Length::Px(min_h);
        } else {
            let (_, min_h) = measure_input(&value);
            layout.min_h = Length::Px(min_h);
        }
        Element::new(ElementKind::Input(Box::new(value))).with_layout(layout)
    }
}

/// An input change event.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InputEvent {
    /// Updated value.
    pub value: Arc<str>,
    /// Updated cursor position (byte index).
    pub cursor: usize,
    /// Selection anchor position (byte index), if any.
    pub anchor: Option<usize>,
}

impl InputEvent {
    /// Apply this event to a [`TextInput`] state bundle.
    pub fn apply_to(&self, state: &mut TextInput) {
        state.core.text = self.value.to_string();
        state.core.cursor = crate::utils::text::clamp_cursor(&state.core.text, self.cursor);
        state.core.anchor = self
            .anchor
            .map(|anchor| crate::utils::text::clamp_cursor(&state.core.text, anchor));
    }
}

impl crate::layout::hash::LayoutHash for Input {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.border.hash(hasher);
        self.padding.hash(hasher);
        self.focusable.hash(hasher);
        self.value.hash(hasher);
        self.placeholder.hash(hasher);
        self.prefix.hash(hasher);
        self.suffix.hash(hasher);
        self.error.hash(hasher);
        self.reserve_error_row.hash(hasher);
        Some(())
    }
}
