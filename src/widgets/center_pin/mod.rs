mod layout;
mod node;
mod reconcile;

pub(crate) use self::layout::measure_center_pin;
pub use self::node::CenterPinNode;
pub(crate) use self::reconcile::reconcile_center_pin;

use crate::core::element::{Element, ElementKind};
use crate::style::{LayoutConstraints, Length, Style};

/// A layout container that pins one child to the true center of the available
/// area, giving the remaining space above and below to `top` and `bottom`
/// children respectively.
///
/// Unlike a `ZStack` + `Center` combination, the top and bottom zones are
/// collision-aware: they receive only the space that remains after the centered
/// child is placed, so they will never overlap it regardless of how their
/// content grows or shrinks.
///
/// ```rust,ignore
/// CenterPin::new()
///     .top(VStack::new().child(header).child(nav))
///     .center(dialog)
///     .bottom(status_bar)
/// ```
///
/// The container defaults to `Flex(1)` on both axes so it expands to fill its
/// parent (typically the whole screen).
#[derive(Clone, Default)]
pub struct CenterPin {
    pub(crate) top: Option<Box<Element>>,
    pub(crate) center: Option<Box<Element>>,
    pub(crate) bottom: Option<Box<Element>>,
    pub(crate) style: Style,
}

impl CenterPin {
    /// Create an empty CenterPin.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the element displayed in the zone above the centered child.
    pub fn top(mut self, top: impl Into<Element>) -> Self {
        self.top = Some(Box::new(top.into()));
        self
    }

    /// Set the element that is always pinned to the center of the container.
    pub fn center(mut self, center: impl Into<Element>) -> Self {
        self.center = Some(Box::new(center.into()));
        self
    }

    /// Set the element displayed in the zone below the centered child.
    pub fn bottom(mut self, bottom: impl Into<Element>) -> Self {
        self.bottom = Some(Box::new(bottom.into()));
        self
    }

    /// Set base style (e.g. background color).
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }
}

impl From<CenterPin> for Element {
    fn from(value: CenterPin) -> Self {
        let (min_w, min_h) = measure_center_pin(&value, None, None);
        Element::new(ElementKind::CenterPin(value)).with_layout(
            LayoutConstraints::default()
                .min_width(Length::Px(min_w))
                .min_height(Length::Px(min_h)),
        )
    }
}
