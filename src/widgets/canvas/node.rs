use crate::core::node::WidgetNode;
use crate::style::Style;
use crate::widgets::containers::exit_retention::ExitingChild;

#[derive(Clone, Debug, Default)]
pub struct CanvasNode {
    pub style: Style,
    pub passthrough: bool,
    /// Children retained past removal so they can finish an
    /// [`Animated::auto_exit`](crate::widgets::Animated::auto_exit) collapse.
    ///
    /// Canvas items carry their own placement rectangle, so unlike a stack there is no reflow to
    /// preserve: a retained child simply keeps its position while it collapses.
    pub exiting: Vec<ExitingChild>,
}

impl WidgetNode for CanvasNode {}
