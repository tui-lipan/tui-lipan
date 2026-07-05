use std::{borrow::Cow, sync::Arc};

use super::{Paint, theme::Style};

/// A styled segment of text.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    /// Text content.
    pub content: Arc<str>,
    /// Style.
    pub style: Style,
    /// Whether row-level hover/selection/active styling may override this span.
    pub allow_row_style: bool,
}

impl Default for Span {
    fn default() -> Self {
        Self {
            content: Arc::from(""),
            style: Style::default(),
            allow_row_style: true,
        }
    }
}

impl Span {
    /// Create a new span.
    pub fn new(content: impl Into<Arc<str>>) -> Self {
        Self {
            content: content.into(),
            style: Style::default(),
            allow_row_style: true,
        }
    }

    /// Set style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set foreground paint.
    pub fn fg(mut self, paint: impl Into<Paint>) -> Self {
        self.style.fg = Some(paint.into());
        self
    }

    /// Set background paint.
    pub fn bg(mut self, paint: impl Into<Paint>) -> Self {
        self.style.bg = Some(paint.into());
        self
    }

    /// Control whether row-level hover/selection/active styling may override this span.
    pub fn allow_row_style(mut self, allow: bool) -> Self {
        self.allow_row_style = allow;
        self
    }

    /// Enable bold.
    pub fn bold(mut self) -> Self {
        self.style.bold = Some(true);
        self
    }
}

impl From<&'static str> for Span {
    fn from(s: &'static str) -> Self {
        Self::new(s)
    }
}

impl From<String> for Span {
    fn from(s: String) -> Self {
        Self::new(s)
    }
}

impl From<Arc<str>> for Span {
    fn from(s: Arc<str>) -> Self {
        Self {
            content: s,
            style: Style::default(),
            allow_row_style: true,
        }
    }
}

/// A sequence of styled text spans for rich text rendering.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct RichText {
    /// The styled spans that make up this rich text.
    pub spans: Vec<Span>,
}

impl RichText {
    /// Create an empty rich text.
    pub fn new() -> Self {
        Self { spans: Vec::new() }
    }

    /// Parse an ANSI-escaped string into rich text.
    ///
    /// SGR escape sequences (colors, bold, italic, etc.) are converted to
    /// styled [`Span`]s. Non-SGR sequences are silently stripped.
    pub fn from_ansi(input: &str) -> Self {
        Self {
            spans: crate::style::ansi::parse_ansi(input),
        }
    }

    /// Append a styled span.
    pub fn span(mut self, span: impl Into<Span>) -> Self {
        self.spans.push(span.into());
        self
    }

    /// Get the plain text content (without styling).
    pub fn plain_content(&self) -> Cow<'_, str> {
        match self.spans.as_slice() {
            [] => Cow::Borrowed(""),
            [span] => Cow::Borrowed(span.content.as_ref()),
            spans => {
                let total_len = spans.iter().map(|span| span.content.len()).sum();
                let mut content = String::with_capacity(total_len);
                for span in spans {
                    content.push_str(span.content.as_ref());
                }
                Cow::Owned(content)
            }
        }
    }

    /// Check if the rich text is empty.
    pub fn is_empty(&self) -> bool {
        self.spans.is_empty() || self.spans.iter().all(|s| s.content.is_empty())
    }

    /// Get the display width of the rich text.
    pub fn width(&self) -> usize {
        self.spans
            .iter()
            .map(|s| unicode_width::UnicodeWidthStr::width(s.content.as_ref()))
            .sum()
    }
}

impl<S: Into<Span>> From<S> for RichText {
    fn from(s: S) -> Self {
        Self {
            spans: vec![s.into()],
        }
    }
}

impl From<Vec<Span>> for RichText {
    fn from(spans: Vec<Span>) -> Self {
        Self { spans }
    }
}

impl<S: Into<Span>> FromIterator<S> for RichText {
    fn from_iter<I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self {
            spans: iter.into_iter().map(Into::into).collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{RichText, Span};

    #[test]
    fn plain_content_borrows_single_span() {
        let text = RichText::from("hello");
        assert!(matches!(text.plain_content(), Cow::Borrowed("hello")));
    }

    #[test]
    fn plain_content_allocates_for_multiple_spans() {
        let text = RichText::new()
            .span(Span::from("hel"))
            .span(Span::from("lo"));
        assert!(matches!(text.plain_content(), Cow::Owned(ref s) if s == "hello"));
    }
}
