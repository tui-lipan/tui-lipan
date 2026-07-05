//! Spacer widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_spacer;
pub use node::SpacerNode;
pub use reconcile::reconcile_spacer;

use crate::core::element::{Element, ElementKind};
use crate::style::Length;

/// Flexible empty space.
#[derive(Clone, Debug)]
pub struct Spacer {
    pub(crate) width: Length,
    pub(crate) height: Length,
}

impl Default for Spacer {
    fn default() -> Self {
        Self {
            width: Length::Flex(1),
            height: Length::Flex(1),
        }
    }
}

impl Spacer {
    /// Create a spacer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }
}

impl From<Spacer> for Element {
    fn from(value: Spacer) -> Self {
        Element::new(ElementKind::Spacer(value))
    }
}

impl crate::layout::hash::LayoutHash for Spacer {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        Some(())
    }
}
