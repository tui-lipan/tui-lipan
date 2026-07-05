use std::rc::Rc;

use crate::core::node::WidgetNode;
use crate::style::Rect;

#[derive(Clone, Debug)]
pub struct GridLayoutCache {
    pub bounds: Rect,
    pub layout_hash: u64,
    pub child_rects: Rc<Vec<Rect>>,
}

#[derive(Clone, Debug, Default)]
pub struct GridNode {
    pub props: super::GridProps,
    pub layout_cache: Option<GridLayoutCache>,
}

impl WidgetNode for GridNode {}
