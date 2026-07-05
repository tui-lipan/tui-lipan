use crate::core::layout::LayoutContext;
use crate::core::node::NodeTree;
use crate::style::{Rect, Size};
use crate::widgets::#NAME_SNAKE#::#Name#Node;

pub fn measure_#NAME_SNAKE#(
    _tree: &NodeTree,
    node: &#Name#Node,
    _ctx: &LayoutContext,
    available: Size,
) -> Size {
    // Calculate content size
    let content_width = node.label.len() as u16;
    let content_height = 1u16;
    
    // Add padding
    let total_width = content_width + node.padding.horizontal();
    let total_height = content_height + node.padding.vertical();
    
    // Constrain to available space
    Size {
        width: total_width.min(available.width),
        height: total_height.min(available.height),
    }
}
