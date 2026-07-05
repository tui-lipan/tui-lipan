//! Toast widget.

use std::sync::Arc;

use unicode_width::UnicodeWidthStr;

use crate::core::element::Element;
use crate::style::{Align, BorderStyle, Length, Padding, Rect, RichText, Span, Style};
use crate::widgets::frame::EdgeDecoration;
use crate::widgets::text::Overflow;
use crate::widgets::{Frame, Text};

pub(crate) const COPY_GLYPH: &str = "⧉";

pub(crate) fn copy_zone_with_right_padding(rect: Rect, right_padding: u16) -> Rect {
    let glyph_width = UnicodeWidthStr::width(COPY_GLYPH).max(1) as u16;
    if rect.w <= glyph_width + 1 + right_padding || rect.h == 0 {
        return Rect {
            x: rect.x,
            y: rect.y,
            w: 0,
            h: 0,
        };
    }

    Rect {
        x: rect.x + rect.w.saturating_sub(glyph_width + 1 + right_padding) as i16,
        y: rect.y,
        w: glyph_width,
        h: 1,
    }
}

/// A transient notification message.
#[derive(Clone)]
pub struct Toast {
    /// Message content.
    pub message: Arc<str>,
    /// Duration in seconds.
    pub duration: f64,
    /// Dismiss when clicked on.
    pub dismiss_on_click: bool,
    /// Show a copy affordance that copies the whole toast message when clicked.
    pub copyable: bool,
    /// Optional visual affordance for copyable toasts.
    pub copy_affordance: ToastCopyAffordance,
    pub(crate) title: Option<Arc<str>>,
    pub(crate) title_prefix: Option<RichText>,
    pub(crate) title_suffix: Option<RichText>,
    pub(crate) title_alignment: Align,
    pub(crate) title_style: Style,
    pub(crate) message_style: Style,
    pub(crate) frame_style: Style,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) header_padding: Padding,
    pub(crate) padding: Padding,
    pub(crate) decorations: Vec<EdgeDecoration>,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) max_width: Option<Length>,
    pub(crate) wrap: bool,
}

/// Visual affordance used for copyable toasts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToastCopyAffordance {
    /// Do not render an explicit copy control. The toast can still be copied by right-clicking it.
    None,
    /// Render the copy glyph in the top border when the toast has a border.
    BorderGlyph,
}

impl Toast {
    /// Create a new toast with the given message.
    pub fn new(message: impl Into<Arc<str>>) -> Self {
        Self {
            message: message.into(),
            duration: 3.0,
            dismiss_on_click: true,
            copyable: false,
            copy_affordance: ToastCopyAffordance::BorderGlyph,
            title: None,
            title_prefix: None,
            title_suffix: None,
            title_alignment: Align::Start,
            title_style: Style::default(),
            message_style: Style::default(),
            frame_style: Style::default(),
            border: true,
            border_style: BorderStyle::Rounded,
            header_padding: Padding::default(),
            padding: Padding::default(),
            decorations: Vec::new(),
            width: Length::Auto,
            height: Length::Auto,
            max_width: None,
            wrap: true,
        }
    }

    /// Set duration in seconds.
    pub fn duration(mut self, secs: f64) -> Self {
        self.duration = secs;
        self
    }

    /// Set title.
    pub fn title(mut self, title: Option<impl Into<Arc<str>>>) -> Self {
        self.title = title.map(Into::into);
        self
    }

    /// Set an optional prefix rendered before the title.
    pub fn title_prefix(mut self, prefix: impl Into<RichText>) -> Self {
        self.title_prefix = Some(prefix.into());
        self
    }

    /// Set an optional suffix rendered after the title.
    pub fn title_suffix(mut self, suffix: impl Into<RichText>) -> Self {
        self.title_suffix = Some(suffix.into());
        self
    }

    /// Set the title alignment in the top border.
    pub fn title_alignment(mut self, align: Align) -> Self {
        self.title_alignment = align;
        self
    }

    /// Set title style.
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Set message style.
    pub fn message_style(mut self, style: Style) -> Self {
        self.message_style = style;
        self
    }

    /// Set frame style.
    pub fn frame_style(mut self, style: Style) -> Self {
        self.frame_style = style;
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

    /// Set padding for the header (title). Top/bottom are ignored.
    pub fn header_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.header_padding = padding.into();
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Add an edge decoration.
    pub fn decoration(mut self, decoration: EdgeDecoration) -> Self {
        self.decorations.push(decoration);
        self
    }

    /// Replace all edge decorations.
    pub fn decorations(mut self, decorations: Vec<EdgeDecoration>) -> Self {
        self.decorations = decorations;
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

    /// Set maximum width (wraps content when exceeded).
    pub fn max_width(mut self, width: Length) -> Self {
        self.max_width = Some(width);
        self
    }

    /// Set whether clicking on the toast dismisses it.
    pub fn dismiss_on_click(mut self, dismiss: bool) -> Self {
        self.dismiss_on_click = dismiss;
        self
    }

    /// Set whether the toast can copy its message.
    ///
    /// Copyable toasts copy their message when right-clicked. When the toast has a border and
    /// [`ToastCopyAffordance::BorderGlyph`] is enabled, they also show a copy glyph that can be
    /// clicked with the left mouse button.
    pub fn copyable(mut self, copyable: bool) -> Self {
        self.copyable = copyable;
        self
    }

    /// Set the optional visual copy affordance for copyable toasts.
    pub fn copy_affordance(mut self, affordance: ToastCopyAffordance) -> Self {
        self.copy_affordance = affordance;
        self
    }

    /// Set whether message text should wrap.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Convert to element.
    pub fn into_element(self) -> Element {
        let max_width = self.max_width;

        let mut message_style = self.message_style;
        if message_style.bg.is_none() {
            message_style.bg = self.frame_style.bg;
        }

        let overflow = if self.wrap {
            Overflow::Wrap
        } else {
            Overflow::Auto
        };

        let render_copy_affordance = self.copyable
            && self.border
            && matches!(self.copy_affordance, ToastCopyAffordance::BorderGlyph);
        let title_alignment = if render_copy_affordance {
            Align::End
        } else {
            self.title_alignment
        };

        let mut frame = Frame::new()
            .title_style(self.title_style)
            .title_alignment(title_alignment)
            .border(self.border)
            .border_style(self.border_style)
            .header_padding(self.header_padding)
            .padding(self.padding)
            .style(self.frame_style)
            .child(
                Text::new(self.message.clone())
                    .style(message_style)
                    .overflow(overflow),
            )
            .width(self.width)
            .height(self.height);

        if let Some(title) = self.title {
            frame = frame.title(title);
        }
        if let Some(prefix) = self.title_prefix.clone() {
            frame = frame.title_prefix(prefix);
        }
        let title_suffix = if render_copy_affordance {
            let mut suffix = self.title_suffix.clone().unwrap_or_default();
            if !suffix.is_empty() {
                suffix.spans.push(Span::new(" "));
            }
            suffix.spans.push(Span::new(COPY_GLYPH));
            Some(suffix)
        } else {
            self.title_suffix.clone()
        };
        if let Some(suffix) = title_suffix {
            frame = frame.title_suffix(suffix);
        }
        if !self.decorations.is_empty() {
            frame = frame.decorations(self.decorations.clone());
        }

        let mut element: Element = frame.into();
        if let Some(max_width) = max_width {
            element = element.max_width(max_width);
        }
        element
    }
}

impl From<Toast> for Element {
    fn from(toast: Toast) -> Self {
        toast.into_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::element::ElementKind;

    #[test]
    fn copyable_defaults_to_false() {
        let toast = Toast::new("message");

        assert!(!toast.copyable);
        assert_eq!(toast.copy_affordance, ToastCopyAffordance::BorderGlyph);
    }

    #[test]
    fn copy_zone_tracks_top_right_title_suffix_cell() {
        let zone = copy_zone_with_right_padding(
            Rect {
                x: 10,
                y: 5,
                w: 20,
                h: 3,
            },
            0,
        );

        assert_eq!(zone.x, 28);
        assert_eq!(zone.y, 5);
        assert_eq!(zone.w, 1);
        assert_eq!(zone.h, 1);
        assert!(zone.contains(28, 5));
        assert!(!zone.contains(29, 5));
        assert!(!zone.contains(28, 6));
    }

    #[test]
    fn copy_zone_accounts_for_header_right_padding() {
        let zone = copy_zone_with_right_padding(
            Rect {
                x: 10,
                y: 5,
                w: 20,
                h: 3,
            },
            2,
        );

        assert_eq!(zone.x, 26);
        assert_eq!(zone.y, 5);
        assert_eq!(zone.w, 1);
        assert_eq!(zone.h, 1);
    }

    #[test]
    fn copyable_borderless_toast_does_not_render_hidden_copy_suffix() {
        let element = Toast::new("message")
            .copyable(true)
            .border(false)
            .into_element();

        let ElementKind::Frame(frame) = element.kind else {
            panic!("toast should lower to a frame");
        };
        assert!(frame.props.title_suffix.is_none());
    }

    #[test]
    fn copy_affordance_none_does_not_render_copy_suffix() {
        let element = Toast::new("message")
            .copyable(true)
            .copy_affordance(ToastCopyAffordance::None)
            .into_element();

        let ElementKind::Frame(frame) = element.kind else {
            panic!("toast should lower to a frame");
        };
        assert!(frame.props.title_suffix.is_none());
    }
}
