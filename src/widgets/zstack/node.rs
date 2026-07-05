use crate::core::node::WidgetNode;
use crate::style::Style;

#[derive(Clone, Debug, Default)]
pub struct ZStackNode {
    pub style: Style,
    pub passthrough: bool,
}

impl WidgetNode for ZStackNode {}
