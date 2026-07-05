use crate::callback::Callback;
use crate::core::node::{NodeId, WidgetNode};
use crate::overlay::OverlayScope;

/// Realized popover node.
#[derive(Clone)]
pub struct PopoverNode {
    pub trigger: Box<NodeId>,
    pub content: Box<NodeId>,
    /// Handler for popover close.
    pub on_close: Option<Callback<()>>,
    /// Whether popover is open.
    pub open: bool,
    /// Whether content is rendered inline or via root overlay pipeline.
    pub scope: OverlayScope,
}

impl WidgetNode for PopoverNode {}
