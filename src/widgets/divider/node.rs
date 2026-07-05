use crate::core::node::{NodeKind, WidgetNode};
use crate::style::Style;

use super::{Divider, Orientation};

#[derive(Clone, Debug)]
pub struct DividerNode {
    pub orientation: Orientation,
    pub ch: char,
    pub style: Style,
    pub join_frame: bool,
    pub label_padding: u16,
}

impl WidgetNode for DividerNode {}

impl From<Divider> for DividerNode {
    fn from(divider: Divider) -> Self {
        Self {
            orientation: divider.orientation,
            ch: divider.ch,
            style: divider.style,
            join_frame: divider.join_frame,
            label_padding: divider.label_padding,
        }
    }
}

impl From<DividerNode> for NodeKind {
    fn from(node: DividerNode) -> Self {
        NodeKind::Divider(node)
    }
}
