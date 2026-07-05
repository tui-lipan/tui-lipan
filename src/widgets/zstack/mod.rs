mod layout;
mod node;
mod reconcile;

pub(crate) use self::layout::measure_zstack;
pub use self::node::ZStackNode;
pub(crate) use self::reconcile::reconcile_zstack;

use crate::core::element::{Element, ElementKind};
use crate::style::{LayoutConstraints, Length, Style};

/// Overlay container.
///
/// Unlike `VStack`/`HStack`, `ZStack` does not split space: every child receives the full
/// available rectangle.
///
/// Children are rendered in order (painter's algorithm). The last child is on top.
#[derive(Clone, Default)]
pub struct ZStack {
    pub(crate) style: Style,
    pub(crate) passthrough: bool,
    pub(crate) children: Vec<Element>,
}

impl ZStack {
    /// Create an empty ZStack.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Allow pointer events to pass through non-interactive layers.
    pub fn passthrough(mut self, passthrough: bool) -> Self {
        self.passthrough = passthrough;
        self
    }

    /// Add a child.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.children.push(child.into());
        self
    }
}

impl From<ZStack> for Element {
    fn from(value: ZStack) -> Self {
        let (min_w, min_h) = measure_zstack(&value, None, None);
        Element::new(ElementKind::ZStack(value)).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}

impl crate::layout::hash::LayoutHash for ZStack {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.passthrough.hash(hasher);
        crate::layout::hash::hash_children(&self.children, hasher, recurse)
    }
}
