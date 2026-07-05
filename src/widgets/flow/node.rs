use crate::core::node::WidgetNode;
use crate::style::{BorderStyle, Padding, Style};

#[derive(Clone, Debug, Default)]
pub struct FlowNode {
    pub style: Style,
    pub padding: Padding,
    pub border: bool,
    pub border_style: BorderStyle,
}

impl WidgetNode for FlowNode {}
