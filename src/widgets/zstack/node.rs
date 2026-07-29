use crate::core::node::WidgetNode;
use crate::style::Style;
use crate::widgets::containers::exit_retention::ExitingChild;

#[derive(Clone, Debug, Default)]
pub struct ZStackNode {
    pub style: Style,
    pub passthrough: bool,
    /// Children retained past removal so they can finish an
    /// [`Animated::auto_exit`](crate::widgets::Animated::auto_exit) collapse.
    ///
    /// Every layer receives the same rectangle, so like a `Canvas` there is no reflow to
    /// preserve: a retained layer collapses in place and the others are untouched.
    pub exiting: Vec<ExitingChild>,
}

impl WidgetNode for ZStackNode {}
