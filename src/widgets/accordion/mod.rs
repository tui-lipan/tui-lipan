//! Accordion widget.

mod types;
pub use types::*;

use crate::callback::Callback;
use crate::core::element::Element;
use crate::core::event::MouseEvent;
use crate::style::{Align, BorderStyle, Length, Padding, Style, StyleSlot};
use crate::widgets::{Button, Frame, VStack};
use std::sync::Arc;

/// An accordion widget.
#[derive(Clone)]
pub struct Accordion {
    pub(crate) items: Vec<AccordionItem>,
    pub(crate) on_toggle: Option<Callback<usize>>,
    pub(crate) exclusive: bool,
    pub(crate) gap: u16,
    pub(crate) padding: Padding,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) style: Style,
    pub(crate) header_style: Style,
    pub(crate) header_hover_style: StyleSlot,
    pub(crate) header_focus_style: StyleSlot,
    pub(crate) header_padding: Padding,
    pub(crate) content_padding: Padding,
    pub(crate) content_border: bool,
    pub(crate) content_border_style: BorderStyle,
    pub(crate) content_style: Style,
    pub(crate) disabled_style: Style,
    pub(crate) expanded_icon: Arc<str>,
    pub(crate) collapsed_icon: Arc<str>,
    pub(crate) focusable: bool,
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl Default for Accordion {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            on_toggle: None,
            exclusive: false,
            gap: 0,
            padding: Padding::default(),
            border: false,
            border_style: BorderStyle::Plain,
            style: Style::default(),
            header_style: Style::default(),
            header_hover_style: StyleSlot::Inherit,
            header_focus_style: StyleSlot::Inherit,
            header_padding: Padding::default(),
            content_padding: Padding::default(),
            content_border: false,
            content_border_style: BorderStyle::Plain,
            content_style: Style::default(),
            disabled_style: Style::default(),
            expanded_icon: "▼ ".into(),
            collapsed_icon: "▶ ".into(),
            focusable: true,
            width: Length::Flex(1),
            height: Length::Auto,
        }
    }
}

impl Accordion {
    /// Create a new accordion.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a section.
    pub fn item(mut self, item: AccordionItem) -> Self {
        self.items.push(item);
        self
    }

    /// Set toggle callback.
    pub fn on_toggle(mut self, cb: Callback<usize>) -> Self {
        self.on_toggle = Some(cb);
        self
    }

    /// Set exclusive mode (only one section open at a time).
    pub fn exclusive(mut self, exclusive: bool) -> Self {
        self.exclusive = exclusive;
        self
    }

    /// Set gap between sections.
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Set padding around the accordion.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Draw a border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set header style.
    pub fn header_style(mut self, style: Style) -> Self {
        self.header_style = style;
        self
    }

    /// Set header hover style.
    pub fn header_hover_style(mut self, style: Style) -> Self {
        self.header_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed header hover style.
    pub fn extend_header_hover_style(mut self, style: Style) -> Self {
        self.header_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed header hover style.
    pub fn inherit_header_hover_style(mut self) -> Self {
        self.header_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set header hover style slot directly for composite forwarding.
    pub fn header_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.header_hover_style = slot;
        self
    }

    /// Set header focus style.
    pub fn header_focus_style(mut self, style: Style) -> Self {
        self.header_focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed header focus style.
    pub fn extend_header_focus_style(mut self, style: Style) -> Self {
        self.header_focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed header focus style.
    pub fn inherit_header_focus_style(mut self) -> Self {
        self.header_focus_style = StyleSlot::Inherit;
        self
    }

    /// Set header focus style slot directly for composite forwarding.
    pub fn header_focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.header_focus_style = slot;
        self
    }

    /// Set header padding.
    pub fn header_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.header_padding = padding.into();
        self
    }

    /// Set content padding.
    pub fn content_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.content_padding = padding.into();
        self
    }

    /// Draw a border around expanded content.
    pub fn content_border(mut self, border: bool) -> Self {
        self.content_border = border;
        self
    }

    /// Set content border style.
    pub fn content_border_style(mut self, border_style: BorderStyle) -> Self {
        self.content_border_style = border_style;
        self
    }

    /// Set content style.
    pub fn content_style(mut self, style: Style) -> Self {
        self.content_style = style;
        self
    }

    /// Set disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Set expanded icon.
    pub fn expanded_icon(mut self, icon: impl Into<Arc<str>>) -> Self {
        self.expanded_icon = icon.into();
        self
    }

    /// Set collapsed icon.
    pub fn collapsed_icon(mut self, icon: impl Into<Arc<str>>) -> Self {
        self.collapsed_icon = icon.into();
        self
    }

    /// Control whether section headers are focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
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
}

impl From<Accordion> for Element {
    fn from(accordion: Accordion) -> Self {
        let mut stack = VStack::new()
            .gap(accordion.gap)
            .padding(accordion.padding)
            .border(accordion.border)
            .border_style(accordion.border_style)
            .style(accordion.style)
            .width(accordion.width)
            .height(accordion.height);

        for (i, item) in accordion.items.into_iter().enumerate() {
            let icon = if item.expanded {
                accordion.expanded_icon.clone()
            } else {
                accordion.collapsed_icon.clone()
            };
            let title = format!("{}{}", icon, item.title);

            let mut header = Button::filled(title)
                .width(Length::Flex(1))
                .align(Align::Start)
                .padding(accordion.header_padding)
                .style(accordion.header_style)
                .hover_style_slot(accordion.header_hover_style)
                .focus_style_slot(accordion.header_focus_style)
                .focusable(accordion.focusable)
                .disabled(item.disabled)
                .disabled_style(accordion.disabled_style);

            if let Some(cb) = accordion.on_toggle.clone()
                && !item.disabled
            {
                header = header.on_click(Callback::new(move |_: MouseEvent| cb.emit(i)));
            }

            stack = stack.child(header);

            if item.expanded {
                let content = Frame::new()
                    .border(accordion.content_border)
                    .border_style(accordion.content_border_style)
                    .padding(accordion.content_padding)
                    .style(accordion.content_style)
                    .width(Length::Flex(1))
                    .height(Length::Auto)
                    .child(item.content);
                stack = stack.child(content);
            }
        }

        stack.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accordion_headers_are_focusable_by_default() {
        assert!(Accordion::new().focusable);
    }

    #[test]
    fn accordion_focusable_builder_updates_header_focusability() {
        assert!(!Accordion::new().focusable(false).focusable);
    }
}
