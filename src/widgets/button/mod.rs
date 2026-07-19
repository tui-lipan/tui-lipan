//! Button widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_button;
pub use node::ButtonNode;
pub use reconcile::reconcile_button;

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::core::event::MouseEvent;
use crate::input::KeyBindings;
use crate::style::{Align, BorderStyle, LayoutConstraints, Length, Padding, Style, StyleSlot};

/// Visual variant for a [`Button`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ButtonVariant {
    /// Rendered as `[ Label ]`.
    #[default]
    Bracket,
    /// Background-filled button (no brackets or border).
    Filled,
    /// Border-only button (no background fill).
    Outlined,
}

/// A button element.
#[derive(Clone)]
pub struct Button {
    /// Button label.
    pub label: Arc<str>,
    /// Optional icon displayed before the label.
    pub icon: Option<Arc<str>>,
    /// Style applied to the icon.
    pub icon_style: Style,
    /// Gap between icon and label.
    pub icon_gap: u16,
    /// Optional shortcut hint displayed after the label.
    pub shortcut: Option<Arc<str>>,
    /// Style applied to the shortcut hint.
    pub shortcut_style: Style,
    /// Gap between label and shortcut.
    pub shortcut_gap: u16,
    /// Base style.
    pub style: Style,
    /// Style applied when the button is hovered.
    pub hover_style: StyleSlot,
    /// Style applied when the button is focused.
    pub focus_style: StyleSlot,
    /// Label alignment inside the allocated rect.
    /// Default: `Align::Center`.
    pub align: Align,
    /// Requested width.
    /// Default: `Length::Auto`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Auto`.
    pub height: Length,
    /// Visual variant.
    pub variant: ButtonVariant,
    /// Border style used by `ButtonVariant::Outlined`.
    /// Default: `BorderStyle::Plain`.
    pub border_style: BorderStyle,
    /// Optional border style override when hovered.
    pub hover_border_style: Option<BorderStyle>,
    /// Optional border style override when focused.
    pub focus_border_style: Option<BorderStyle>,
    /// Padding inside the button.
    /// Default: `Padding { left: 1, right: 1, top: 0, bottom: 0 }`.
    pub padding: Padding,
    /// Whether the button is disabled.
    pub disabled: bool,
    /// Style applied when disabled.
    pub disabled_style: Style,
    /// Activation handler for mouse clicks and focused plain Enter/Space.
    ///
    /// Keyboard activation emits a synthetic left-button mouse-up event at the
    /// button rect center. A custom [`Button::on_key`] handler runs first and
    /// can consume the key by returning `true`.
    pub on_click: Option<Callback<MouseEvent>>,
    /// Keyboard handler (only for focused node), called before default activation.
    pub on_key: Option<KeyHandler>,
    /// Whether the button participates in focus traversal.
    pub focusable: bool,
    /// Whether the button participates in tab traversal when focusable.
    pub tab_stop: bool,
    /// Callback fired when the button gains focus.
    pub on_focus: Option<Callback<()>>,
    /// Callback fired when the button loses focus.
    pub on_blur: Option<Callback<()>>,
}

impl Button {
    /// Create a new bracket button (`[ Label ]`).
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            icon_style: Style::default(),
            icon_gap: 1,
            shortcut: None,
            shortcut_style: Style::default(),
            shortcut_gap: 1,
            style: Style::default(),
            hover_style: StyleSlot::Inherit,
            focus_style: StyleSlot::Inherit,
            align: Align::Center,
            width: Length::Auto,
            height: Length::Auto,
            variant: ButtonVariant::Bracket,
            border_style: BorderStyle::Plain,
            hover_border_style: None,
            focus_border_style: None,
            padding: Padding {
                left: 1,
                right: 1,
                top: 0,
                bottom: 0,
            },
            disabled: false,
            disabled_style: Style::default(),
            on_click: None,
            on_key: None,
            focusable: true,
            tab_stop: true,
            on_focus: None,
            on_blur: None,
        }
    }

    /// Create a background-filled button.
    pub fn filled(label: impl Into<Arc<str>>) -> Self {
        let mut button = Self::new(label);
        button.variant = ButtonVariant::Filled;
        button
    }

    /// Create a border-only button.
    pub fn outlined(label: impl Into<Arc<str>>) -> Self {
        let mut button = Self::new(label);
        button.variant = ButtonVariant::Outlined;
        button.border_style = BorderStyle::Plain;
        button.hover_border_style = None;
        button.focus_border_style = None;
        button
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set icon displayed before the label.
    pub fn icon(mut self, icon: impl Into<Arc<str>>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    /// Set icon style.
    pub fn icon_style(mut self, style: Style) -> Self {
        self.icon_style = style;
        self
    }

    /// Set gap between icon and label.
    pub fn icon_gap(mut self, gap: u16) -> Self {
        self.icon_gap = gap;
        self
    }

    /// Set shortcut hint displayed after the label.
    pub fn shortcut(mut self, shortcut: impl Into<Arc<str>>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    /// Set shortcut hint from parsed alternative bindings.
    pub fn shortcut_bindings(mut self, bindings: KeyBindings) -> Self {
        self.shortcut = Some(bindings.to_string().into());
        self
    }

    /// Set shortcut style.
    pub fn shortcut_style(mut self, style: Style) -> Self {
        self.shortcut_style = style;
        self
    }

    /// Set gap between label and shortcut.
    pub fn shortcut_gap(mut self, gap: u16) -> Self {
        self.shortcut_gap = gap;
        self
    }

    /// Set hover style.
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

    /// Set focus style.
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

    /// Set label alignment.
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
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

    /// Set visual variant.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set border style (only used by `ButtonVariant::Outlined`).
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set hover border style override.
    pub fn hover_border_style(mut self, border_style: Option<BorderStyle>) -> Self {
        self.hover_border_style = border_style;
        self
    }

    /// Set focus border style override.
    pub fn focus_border_style(mut self, border_style: Option<BorderStyle>) -> Self {
        self.focus_border_style = border_style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
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

    /// Convenience for toggling full-width behavior.
    pub fn full_width(mut self, full_width: bool) -> Self {
        self.width = if full_width {
            Length::Flex(1)
        } else {
            Length::Auto
        };
        self
    }

    /// Set activation handler for mouse clicks and focused plain Enter/Space.
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set focused key handler. Returning `true` consumes the key before default activation.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }

    /// Control whether the node is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Control whether the button participates in tab traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the button gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the button loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
        self
    }
}

impl From<Button> for Element {
    fn from(value: Button) -> Self {
        let (min_w, min_h) = measure_button(&value);
        Element::new(ElementKind::Button(Box::new(value))).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}

impl crate::layout::hash::LayoutHash for Button {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.variant.hash(hasher);
        self.padding.hash(hasher);
        self.label.hash(hasher);
        self.icon.hash(hasher);
        self.icon_gap.hash(hasher);
        self.shortcut.hash(hasher);
        self.shortcut_gap.hash(hasher);
        Some(())
    }
}
