mod layout;
mod node;
mod reconcile;

pub(crate) use self::layout::measure_center;
pub use self::node::CenterNode;
pub(crate) use self::reconcile::reconcile_center;

use crate::core::element::{Element, ElementKind};
use crate::style::{LayoutConstraints, Length, Size, Style};

/// Center a single child within the available area.
#[derive(Clone, Default)]
pub struct Center {
    pub(crate) child: Option<Box<Element>>,
    pub(crate) style: Style,
    pub(crate) width: Size,
    pub(crate) height: Size,
}

impl Center {
    /// Create an empty Center.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the centered child.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = Some(Box::new(child.into()));
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the centered width constraint.
    pub fn width(mut self, width: Size) -> Self {
        self.width = width;
        self
    }

    /// Set the centered height constraint.
    pub fn height(mut self, height: Size) -> Self {
        self.height = height;
        self
    }
}

impl From<Center> for Element {
    fn from(value: Center) -> Self {
        let (min_w, min_h) = measure_center(&value, None, None);
        Element::new(ElementKind::Center(value)).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}

impl crate::layout::hash::LayoutHash for Center {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        if let Some(child) = self.child.as_ref() {
            recurse(child.as_ref())?.hash(hasher);
        } else {
            0u8.hash(hasher);
        }
        Some(())
    }
}
