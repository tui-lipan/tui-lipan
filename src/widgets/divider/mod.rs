//! Divider widget.

mod layout;
mod node;
mod reconcile;

pub(crate) use self::reconcile::{DividerReconcile, reconcile_divider};
pub use layout::measure_divider;
pub use node::DividerNode;

use crate::core::element::{Element, ElementKind};
use crate::style::{Align, Length, Style};

/// Layout orientation (horizontal vs vertical).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Orientation {
    /// Horizontal orientation.
    Horizontal,
    /// Vertical orientation.
    Vertical,
}

/// A 1-cell-thick divider.
#[derive(Clone)]
pub struct Divider {
    pub(crate) orientation: Orientation,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) ch: char,
    pub(crate) style: Style,
    pub(crate) label: Option<Box<Element>>,
    pub(crate) label_alignment: Align,
    /// Left and right inset of a horizontal label, in cells.
    ///
    /// The divider character still paints in the inset cells, so a left inset of 1 yields
    /// `─title` rather than a blank before the title. Only the label's own cells are left clear.
    pub(crate) label_padding: (u16, u16),
    pub(crate) join_frame: bool,
}

impl Divider {
    /// Create a divider with the given orientation.
    pub fn new(orientation: Orientation) -> Self {
        match orientation {
            Orientation::Horizontal => Self::horizontal(),
            Orientation::Vertical => Self::vertical(),
        }
    }

    /// Create a horizontal divider.
    pub fn horizontal() -> Self {
        Self {
            orientation: Orientation::Horizontal,
            width: Length::Flex(1),
            height: Length::Px(1),
            ch: '─',
            style: Style::default(),
            label: None,
            label_alignment: Align::Start,
            label_padding: (1, 1),
            join_frame: false,
        }
    }

    /// Create a vertical divider.
    pub fn vertical() -> Self {
        Self {
            orientation: Orientation::Vertical,
            width: Length::Px(1),
            height: Length::Flex(1),
            ch: '│',
            style: Style::default(),
            label: None,
            label_alignment: Align::Start,
            label_padding: (1, 1),
            join_frame: false,
        }
    }

    /// Set style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set an inline label (horizontal only).
    pub fn label(mut self, label: impl Into<Element>) -> Self {
        self.label = Some(Box::new(label.into()));
        self
    }

    /// Set label alignment (horizontal only).
    pub fn label_alignment(mut self, alignment: Align) -> Self {
        self.label_alignment = alignment;
        self
    }

    /// Set equal left and right label insets (horizontal only).
    ///
    /// Prefer [`Self::label_padding_axes`] when the sides should differ - for example a border
    /// title that wants one leading `─` and no trailing gap before the line continues.
    pub fn label_padding(mut self, padding: u16) -> Self {
        self.label_padding = (padding, padding);
        self
    }

    /// Set independent left/right label insets (horizontal only).
    ///
    /// Inset cells still show the divider character; only the label itself clears the line.
    pub fn label_padding_axes(mut self, left: u16, right: u16) -> Self {
        self.label_padding = (left, right);
        self
    }

    /// Join the divider with the nearest frame border.
    pub fn join_frame(mut self, join: bool) -> Self {
        self.join_frame = join;
        self
    }

    /// Override the divider character.
    pub fn ch(mut self, ch: char) -> Self {
        self.ch = ch;
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
}

impl From<Divider> for Element {
    fn from(value: Divider) -> Self {
        Element::new(ElementKind::Divider(value))
    }
}

impl crate::layout::hash::LayoutHash for Divider {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.orientation.hash(hasher);
        self.label_alignment.hash(hasher);
        self.label_padding.hash(hasher);
        self.join_frame.hash(hasher);

        if let Some(label) = self.label.as_deref() {
            recurse(label)?.hash(hasher);
        }
        Some(())
    }
}
