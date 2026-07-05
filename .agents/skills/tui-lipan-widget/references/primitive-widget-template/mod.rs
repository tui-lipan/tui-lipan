//! #NAME# widget.
//!
//! Brief description of what this widget does.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_#NAME_SNAKE#;
pub use node::#Name#Node;
pub use reconcile::reconcile_#NAME_SNAKE#;

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::Element;
use crate::core::event::MouseEvent;
use crate::style::{Align, BorderStyle, Length, Padding, Style};

/// Visual variant for a [`#Name#`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum #Name#Variant {
    /// Default variant.
    #[default]
    Default,
    /// Alternative variant.
    Alternative,
}

/// A #NAME_SNAKE# element.
#[derive(Clone)]
pub struct #Name# {
    /// Primary content.
    pub label: Arc<str>,
    /// Base style.
    pub style: Style,
    /// Style applied when hovered.
    pub hover_style: Style,
    /// Style applied when focused.
    pub focus_style: Style,
    /// Style applied when disabled.
    pub disabled_style: Style,
    /// Label alignment.
    pub align: Align,
    /// Requested width.
    pub width: Length,
    /// Requested height.
    pub height: Length,
    /// Visual variant.
    pub variant: #Name#Variant,
    /// Border style.
    pub border_style: BorderStyle,
    /// Optional border style when hovered.
    pub hover_border_style: Option<BorderStyle>,
    /// Optional border style when focused.
    pub focus_border_style: Option<BorderStyle>,
    /// Padding inside the widget.
    pub padding: Padding,
    /// Whether the widget is disabled.
    pub disabled: bool,
    /// Whether the widget participates in focus traversal.
    pub focusable: bool,
    /// Mouse click handler.
    pub on_click: Option<Callback<MouseEvent>>,
    /// Keyboard handler (only for focused node).
    pub on_key: Option<KeyHandler>,
}

impl #Name# {
    /// Create a new #NAME_SNAKE#.
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            style: Style::default(),
            hover_style: Style {
                reverse: Some(true),
                ..Style::default()
            },
            focus_style: Style {
                reverse: Some(true),
                bold: Some(true),
                ..Style::default()
            },
            disabled_style: Style::new().dim(),
            align: Align::Center,
            width: Length::Auto,
            height: Length::Auto,
            variant: #Name#Variant::default(),
            border_style: BorderStyle::default(),
            hover_border_style: None,
            focus_border_style: None,
            padding: Padding::default(),
            disabled: false,
            focusable: true,
            on_click: None,
            on_key: None,
        }
    }

    /// Set the base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the hover style.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = style;
        self
    }

    /// Set the focus style.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Set the disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Set the alignment.
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Set the width.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Set the height.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Set the variant.
    pub fn variant(mut self, variant: #Name#Variant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the border style.
    pub fn border_style(mut self, style: BorderStyle) -> Self {
        self.border_style = style;
        self
    }

    /// Set the hover border style.
    pub fn hover_border_style(mut self, style: BorderStyle) -> Self {
        self.hover_border_style = Some(style);
        self
    }

    /// Set the focus border style.
    pub fn focus_border_style(mut self, style: BorderStyle) -> Self {
        self.focus_border_style = Some(style);
        self
    }

    /// Set the padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set whether the widget is disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set whether the widget is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Set the click handler.
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set the key handler.
    pub fn on_key(mut self, cb: KeyHandler) -> Self {
        self.on_key = Some(cb);
        self
    }
}

impl From<#Name#> for Element {
    fn from(value: #Name#) -> Self {
        Element::new(crate::core::element::ElementKind::#Name#(value))
    }
}
