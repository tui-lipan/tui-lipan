//! Badge widget.

use std::sync::Arc;

use crate::core::element::{Element, IntoElement};
use crate::style::{BorderStyle, Length, Padding, Style};
use crate::widgets::{Frame, HStack, Spacer, Text, VStack, ZStack};

/// Badge position relative to its child.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum BadgePosition {
    /// Top-left corner.
    TopStart,
    /// Top-right corner.
    #[default]
    TopEnd,
    /// Bottom-left corner.
    BottomStart,
    /// Bottom-right corner.
    BottomEnd,
}

/// A badge widget.
#[derive(Clone)]
pub struct Badge {
    content: Arc<str>,
    child: Element,
    style: Style,
    text_style: Style,
    border: bool,
    border_style: BorderStyle,
    padding: Padding,
    offset: Padding,
    position: BadgePosition,
    width: Length,
    height: Length,
}

impl Badge {
    /// Create a new badge with the given content.
    pub fn new(content: impl Into<Arc<str>>) -> Self {
        Self {
            content: content.into(),
            child: crate::widgets::Spacer::new().into(),
            style: Style::default(),
            text_style: Style::default(),
            border: false,
            border_style: BorderStyle::Plain,
            padding: 0.into(),
            offset: 0.into(),
            position: BadgePosition::TopEnd,
            width: Length::Auto,
            height: Length::Auto,
        }
    }

    /// Set the child element.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.child = child.into();
        self
    }

    /// Set badge style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set badge text style.
    pub fn text_style(mut self, style: Style) -> Self {
        self.text_style = style;
        self
    }

    /// Draw a border around the badge.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set badge border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set badge padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set offset from the chosen corner.
    pub fn offset(mut self, offset: impl Into<Padding>) -> Self {
        self.offset = offset.into();
        self
    }

    /// Set badge position.
    pub fn position(mut self, position: BadgePosition) -> Self {
        self.position = position;
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

impl From<Badge> for Element {
    fn from(badge: Badge) -> Self {
        let text_style = badge.style.patch(badge.text_style);

        let badge_el = Frame::new()
            .border(badge.border)
            .border_style(badge.border_style)
            .padding(badge.padding)
            .style(badge.style)
            .child(Text::new(badge.content).style(text_style))
            .width(badge.width)
            .height(badge.height);

        let overlay_row = match badge.position {
            BadgePosition::TopStart | BadgePosition::BottomStart => {
                HStack::new().child(badge_el).child(Spacer::new())
            }
            BadgePosition::TopEnd | BadgePosition::BottomEnd => {
                HStack::new().child(Spacer::new()).child(badge_el)
            }
        };

        let overlay_column = match badge.position {
            BadgePosition::TopStart | BadgePosition::TopEnd => {
                VStack::new().child(overlay_row).child(Spacer::new())
            }
            BadgePosition::BottomStart | BadgePosition::BottomEnd => {
                VStack::new().child(Spacer::new()).child(overlay_row)
            }
        };

        let overlay = overlay_column.padding(badge.offset);

        ZStack::new()
            .passthrough(true)
            .child(badge.child)
            .child(overlay)
            .into()
    }
}
