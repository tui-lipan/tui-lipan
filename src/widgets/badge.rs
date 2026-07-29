//! Badge widget.

use std::sync::Arc;

use crate::core::element::{Element, IntoElement};
use crate::style::{BorderStyle, Color, Length, Padding, Style};
use crate::widgets::{CapSides, CapStyle, Frame, HStack, Spacer, Text, VStack, ZStack};

use super::segment_cap::segment_cap;

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
    cap_style: CapStyle,
    cap_sides: CapSides,
    cap_behind: Color,
    cap_same_color: bool,
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
            cap_style: CapStyle::Padded,
            cap_sides: CapSides::Both,
            cap_behind: Color::Reset,
            cap_same_color: false,
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

    /// Set the cap style used around the badge segment.
    ///
    /// [`CapStyle::Round`] and [`CapStyle::Arrow`] use Nerd Font Powerline glyphs;
    /// call [`CapStyle::font_safe`] when a font-independent fallback is needed.
    /// With automatic width, each rendered cap can replace one cell of padding
    /// or one edge space in the label. Caps sit outside an explicit inner width.
    pub fn cap(mut self, cap_style: CapStyle) -> Self {
        self.cap_style = cap_style;
        self
    }

    /// Set which sides of the badge segment receive caps.
    pub fn cap_sides(mut self, cap_sides: CapSides) -> Self {
        self.cap_sides = cap_sides;
        self
    }

    /// Set the color painted behind cap glyphs.
    pub fn cap_behind(mut self, color: Color) -> Self {
        self.cap_behind = color;
        self
    }

    /// Keep the left seam visible when this badge and its neighbor share a background.
    ///
    /// Arrow caps use the Powerline thin separator (`U+E0B3`); other styles use
    /// the font-safe left eighth block (`U+258F`).
    pub fn cap_same_color(mut self, same_color: bool) -> Self {
        self.cap_same_color = same_color;
        self
    }
}

impl From<Badge> for Element {
    fn from(badge: Badge) -> Self {
        let badge_style = badge.style;
        let text_style = badge_style.patch(badge.text_style);
        let badge_bg = badge_style
            .bg
            .map(crate::style::Paint::color)
            .unwrap_or(crate::style::Color::Reset);

        let (content, padding) = replace_padding_with_caps(
            badge.content,
            badge.padding,
            badge.cap_style,
            badge.cap_sides,
            badge.cap_same_color,
        );

        let badge_el = Frame::new()
            .border(badge.border)
            .border_style(badge.border_style)
            .padding(padding)
            .style(badge_style)
            .child(Text::new(content).style(text_style))
            .width(badge.width)
            .height(badge.height);

        let badge_el = segment_cap(
            badge_el.into(),
            badge.cap_style,
            badge.cap_sides,
            badge_bg,
            badge.cap_behind,
            badge.cap_same_color,
        );

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

/// Reduce auto-sized inner content so an outer cap can occupy the same measured
/// cell. Prefer frame padding, then the edge spaces used by labels like `" MAIN "`.
fn replace_padding_with_caps(
    mut content: Arc<str>,
    mut padding: Padding,
    cap_style: CapStyle,
    cap_sides: CapSides,
    same_color_left: bool,
) -> (Arc<str>, Padding) {
    let has_glyphs = cap_style.glyphs().is_some();
    let replace_left = cap_sides.has_left() && (has_glyphs || same_color_left);
    let replace_right = cap_sides.has_right() && has_glyphs;

    if replace_left {
        if padding.left > 0 {
            padding.left -= 1;
        } else if let Some(stripped) = content.strip_prefix(' ') {
            content = Arc::from(stripped);
        }
    }
    if replace_right {
        if padding.right > 0 {
            padding.right -= 1;
        } else if let Some(stripped) = content.strip_suffix(' ') {
            content = Arc::from(stripped);
        }
    }

    (content, padding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caps_replace_label_spaces_without_changing_intrinsic_width() {
        let (content, padding) = replace_padding_with_caps(
            Arc::from(" MAIN "),
            Padding::default(),
            CapStyle::Half,
            CapSides::Both,
            false,
        );

        assert_eq!(content.as_ref(), "MAIN");
        assert_eq!(padding, Padding::default());
        assert_eq!(
            2 + content.chars().count() + padding.horizontal() as usize,
            6
        );
    }

    #[test]
    fn caps_prefer_explicit_padding_without_changing_intrinsic_width() {
        let (content, padding) = replace_padding_with_caps(
            Arc::from(" MAIN "),
            Padding::from((0, 1)),
            CapStyle::Round,
            CapSides::Both,
            false,
        );

        assert_eq!(content.as_ref(), " MAIN ");
        assert_eq!(padding, Padding::default());
        assert_eq!(
            2 + content.chars().count() + padding.horizontal() as usize,
            8
        );
    }

    #[test]
    fn padded_same_color_separator_replaces_left_padding() {
        let (content, padding) = replace_padding_with_caps(
            Arc::from(" READY "),
            Padding::default(),
            CapStyle::Padded,
            CapSides::Both,
            true,
        );

        assert_eq!(content.as_ref(), "READY ");
        assert_eq!(padding, Padding::default());
        assert_eq!(
            1 + content.chars().count() + padding.horizontal() as usize,
            7
        );
    }
}
