use crate::core::node::WidgetNode;
use crate::style::Style;

#[derive(Clone, Debug, Default)]
pub struct CanvasNode {
    pub style: Style,
    pub passthrough: bool,
}

impl WidgetNode for CanvasNode {}
