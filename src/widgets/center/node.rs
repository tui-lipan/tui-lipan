use crate::core::node::WidgetNode;
use crate::style::Style;

#[derive(Clone, Debug, Default)]
pub struct CenterNode {
    pub style: Style,
}

impl WidgetNode for CenterNode {}
