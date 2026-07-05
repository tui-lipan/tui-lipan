use crate::core::node::{NodeKind, WidgetNode};

#[derive(Clone, Debug)]
pub struct SpacerNode;

impl WidgetNode for SpacerNode {}

impl From<super::Spacer> for SpacerNode {
    fn from(_: super::Spacer) -> Self {
        Self
    }
}

impl From<SpacerNode> for NodeKind {
    fn from(node: SpacerNode) -> Self {
        NodeKind::Spacer(node)
    }
}
