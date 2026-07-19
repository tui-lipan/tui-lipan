use std::rc::Rc;
use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::Key;
use crate::core::node::WidgetNode;
use crate::style::{Rect, RichText, Style, StyleSlot};
use crate::widgets::internal::StackProps;
use crate::widgets::{FocusScope, TabVariant, TabsEvent};

#[derive(Clone, Debug)]
pub struct StackLayoutCache {
    pub bounds: Rect,
    pub layout_hash: u64,
    pub child_rects: Rc<Vec<Rect>>,
}

/// Node for VStack and HStack.
#[derive(Clone)]
pub struct StackNode {
    pub props: StackProps,
    pub tab_titles: Vec<RichText>,
    pub active_tab: usize,
    pub on_tab_change: Option<Callback<TabsEvent>>,
    pub active_tab_style: StyleSlot,
    pub inactive_tab_style: Style,
    pub tab_variant: TabVariant,
    pub title_prefix: Option<Arc<str>>,
    pub layout_cache: Option<StackLayoutCache>,
    /// Key of the last child that held real focus within this stack.
    /// Persists across frames; drives sticky accordion expansion automatically.
    pub last_focused_key: Option<Key>,
}

impl WidgetNode for StackNode {
    fn focus_scope(&self) -> FocusScope {
        self.props.focus_scope
    }

    fn has_on_click(&self) -> bool {
        self.on_tab_change.is_some()
    }

    fn hit_test_refinement(&self, _x: i16, y: i16, rect: Rect) -> Option<bool> {
        if self.on_tab_change.is_some() {
            // Tabs are always on the top row (y == rect.y)
            if y != rect.y {
                return Some(false);
            }
        }
        None
    }
}
