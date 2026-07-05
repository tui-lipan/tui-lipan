//! Breadcrumb widget.

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::Element;
use crate::core::event::MouseEvent;
use crate::style::{Align, Justify, Length, Padding, Style, StyleSlot};
use crate::widgets::{Button, HStack, Text};

/// A breadcrumb navigation widget.
#[derive(Clone)]
pub struct Breadcrumb {
    segments: Vec<Arc<str>>,
    separator: Arc<str>,
    gap: u16,
    width: Length,
    height: Length,
    padding: Padding,
    align: Align,
    justify: Justify,
    style: Style,
    active: Option<usize>,
    on_select: Option<Callback<usize>>,
    active_style: Style,
    inactive_style: Style,
    hover_style: StyleSlot,
    separator_style: Style,
}

impl Default for Breadcrumb {
    fn default() -> Self {
        Self {
            segments: Vec::new(),
            separator: "/".into(),
            gap: 1,
            width: Length::Flex(1),
            height: Length::Auto,
            padding: Padding::default(),
            align: Align::Center,
            justify: Justify::Start,
            style: Style::default(),
            active: None,
            on_select: None,
            active_style: Style::default(),
            inactive_style: Style::default(),
            hover_style: StyleSlot::Inherit,
            separator_style: Style::default(),
        }
    }
}

impl Breadcrumb {
    /// Create a new breadcrumb.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set segments.
    pub fn segments(mut self, segments: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        self.segments = segments.into_iter().map(Into::into).collect();
        self
    }

    /// Set separator text.
    pub fn separator(mut self, separator: impl Into<Arc<str>>) -> Self {
        self.separator = separator.into();
        self
    }

    /// Set gap between segments.
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set cross-axis alignment.
    pub fn align(mut self, align: Align) -> Self {
        self.align = align;
        self
    }

    /// Set main-axis alignment.
    pub fn justify(mut self, justify: Justify) -> Self {
        self.justify = justify;
        self
    }

    /// Set base style for the breadcrumb container.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set active segment index.
    pub fn active(mut self, active: Option<usize>) -> Self {
        self.active = active;
        self
    }

    /// Set selection callback.
    pub fn on_select(mut self, cb: Callback<usize>) -> Self {
        self.on_select = Some(cb);
        self
    }

    /// Set active segment style.
    pub fn active_style(mut self, style: Style) -> Self {
        self.active_style = style;
        self
    }

    /// Set inactive segment style.
    pub fn inactive_style(mut self, style: Style) -> Self {
        self.inactive_style = style;
        self
    }

    /// Set hover style.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hover style.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed hover style.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hover style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_style = slot;
        self
    }

    /// Set separator style.
    pub fn separator_style(mut self, style: Style) -> Self {
        self.separator_style = style;
        self
    }
}

impl From<Breadcrumb> for Element {
    fn from(breadcrumb: Breadcrumb) -> Self {
        let mut stack = HStack::new()
            .gap(breadcrumb.gap)
            .width(breadcrumb.width)
            .height(breadcrumb.height)
            .padding(breadcrumb.padding)
            .align(breadcrumb.align)
            .justify(breadcrumb.justify)
            .style(breadcrumb.style);
        let active_index = breadcrumb
            .active
            .unwrap_or_else(|| breadcrumb.segments.len().saturating_sub(1));

        for (i, segment) in breadcrumb.segments.into_iter().enumerate() {
            if i > 0 {
                stack = stack.child(
                    Text::new(breadcrumb.separator.clone()).style(breadcrumb.separator_style),
                );
            }

            let is_active = i == active_index;
            let style = if is_active {
                breadcrumb.active_style
            } else {
                breadcrumb.inactive_style
            };

            if let Some(cb) = breadcrumb.on_select.clone() {
                let mut button = Button::filled(segment)
                    .padding(0)
                    .style(style)
                    .hover_style_slot(breadcrumb.hover_style)
                    .width(Length::Auto);
                button = button.on_click(Callback::new(move |_: MouseEvent| cb.emit(i)));
                stack = stack.child(button);
            } else {
                stack = stack.child(Text::new(segment).style(style));
            }
        }

        stack.into()
    }
}
