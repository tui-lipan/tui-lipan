//! Text widgets.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_text_constrained;
pub(crate) use layout::split_spans_on_newlines;
pub use node::TextNode;
pub use reconcile::reconcile_text;

use std::sync::Arc;

use crate::core::element::{Element, ElementKind};
use crate::style::{Span, Style};

/// Overflow behavior when content doesn't fit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Overflow {
    /// Choose behavior based on available space.
    #[default]
    Auto,
    /// Clip overflowing content.
    Clip,
    /// Clip from the start, keeping the tail visible.
    ClipStart,
    /// Truncate overflowing lines with `…`.
    Ellipsis,
    /// Soft-wrap lines to fit available width.
    Wrap,
}

/// A text element.
#[derive(Clone, Debug)]
pub struct Text {
    /// Text segments.
    pub spans: Vec<Span>,
    /// Base style for all spans.
    pub style: Style,
    /// Overflow strategy.
    pub overflow: Overflow,
    /// Requested width.
    pub width: crate::style::Length,
    /// Requested height.
    pub height: crate::style::Length,
}

impl Text {
    /// Create a new text element.
    pub fn new(content: impl Into<Arc<str>>) -> Self {
        Self {
            spans: vec![Span::new(content)],
            style: Style::default(),
            overflow: Overflow::Auto,
            width: crate::style::Length::Auto,
            height: crate::style::Length::Auto,
        }
    }

    /// Create text from multiple spans.
    pub fn from_spans(spans: impl IntoIterator<Item = Span>) -> Self {
        Self {
            spans: spans.into_iter().collect(),
            style: Style::default(),
            overflow: Overflow::Auto,
            width: crate::style::Length::Auto,
            height: crate::style::Length::Auto,
        }
    }

    /// Create text from an ANSI-escaped string.
    ///
    /// SGR escape sequences (colors, bold, italic, etc.) are converted to
    /// styled spans. Non-SGR sequences are silently stripped.
    pub fn from_ansi(input: &str) -> Self {
        Self::from_spans(crate::style::ansi::parse_ansi(input))
    }

    /// Add a span.
    pub fn span(mut self, span: impl Into<Span>) -> Self {
        self.spans.push(span.into());
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set overflow behavior.
    pub fn overflow(mut self, overflow: Overflow) -> Self {
        self.overflow = overflow;
        self
    }

    /// Set width.
    pub fn width(mut self, width: crate::style::Length) -> Self {
        self.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: crate::style::Length) -> Self {
        self.height = height;
        self
    }

    /// Returns the concatenated plain text content.
    pub fn plain_content(&self) -> String {
        let mut s = String::new();
        for span in &self.spans {
            s.push_str(&span.content);
        }
        s
    }
}

// Implement From<TextNode> back to Text to facilitate shared render code or debugging if needed.
impl From<TextNode> for Text {
    fn from(node: TextNode) -> Self {
        Self {
            spans: node.spans,
            style: node.style,
            overflow: node.overflow,
            width: node.widget_key.width,
            height: node.widget_key.height,
        }
    }
}

impl From<Text> for Element {
    fn from(value: Text) -> Self {
        Element::new(ElementKind::Text(value))
    }
}

impl Default for Text {
    fn default() -> Self {
        Self {
            spans: Vec::new(),
            style: Style::default(),
            overflow: Overflow::Auto,
            width: crate::style::Length::Auto,
            height: crate::style::Length::Auto,
        }
    }
}

impl crate::layout::hash::LayoutHash for Text {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.overflow.hash(hasher);
        crate::layout::hash::hash_spans_content(&self.spans, hasher);
        Some(())
    }
}
