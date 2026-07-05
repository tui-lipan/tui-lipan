use crate::core::node::WidgetNode;
use crate::style::Style;

#[derive(Clone, Debug, Default)]
pub struct CenterPinNode {
    pub style: Style,
}

impl WidgetNode for CenterPinNode {}
