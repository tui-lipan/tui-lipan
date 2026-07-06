use std::sync::Arc;

use crate::callback::Callback;
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{Rect, Style};
use crate::widgets::Orientation;

use super::{Splitter, SplitterHandleMode, SplitterResizeEvent, sizes_to_weights};

#[derive(Clone)]
pub struct SplitterNode {
    pub orientation: Orientation,
    pub weights: Vec<f32>,
    pub weights_nonce: u32,
    pub split_id: Option<Arc<str>>,
    pub on_resize: Option<Callback<SplitterResizeEvent>>,
    pub min_size: u16,
    pub handle_mode: SplitterHandleMode,
    pub handle_symbol: char,
    pub handle_style: Style,
    pub handle_hover_style: Style,
    pub handle_active_style: Style,
    pub handle_rects: Vec<Rect>,
    pub pane_sizes: Vec<u16>,
    pub active_handle: Option<usize>,
}

impl SplitterNode {
    /// Whether handles ride the pane border seam instead of a reserved gutter.
    pub(crate) fn rides_border(&self) -> bool {
        matches!(self.handle_mode, SplitterHandleMode::Border)
    }

    pub(crate) fn handle_at(&self, x: i16, y: i16) -> Option<usize> {
        self.handle_rects
            .iter()
            .position(|rect| rect.contains(x, y))
    }

    pub(crate) fn set_drag_sizes(&mut self, sizes: Vec<u16>) {
        self.pane_sizes = sizes;
        self.weights = sizes_to_weights(&self.pane_sizes);
    }
}

impl WidgetNode for SplitterNode {
    fn has_on_click(&self) -> bool {
        !self.handle_rects.is_empty()
    }

    fn is_hoverable(&self) -> bool {
        !self.handle_rects.is_empty()
    }

    fn hit_test_refinement(&self, x: i16, y: i16, _rect: Rect) -> Option<bool> {
        let hit = self.handle_at(x, y).is_some();
        Some(hit)
    }
}

impl From<Splitter> for SplitterNode {
    fn from(value: Splitter) -> Self {
        Self {
            orientation: value.orientation,
            weights: value.weights,
            weights_nonce: value.weights_nonce,
            split_id: value.split_id.clone(),
            on_resize: value.on_resize.clone(),
            min_size: value.min_size,
            handle_mode: value.handle_mode,
            handle_symbol: value.handle_symbol,
            handle_style: value.handle_style,
            handle_hover_style: value.handle_hover_style,
            handle_active_style: value.handle_active_style,
            handle_rects: Vec::new(),
            pane_sizes: Vec::new(),
            active_handle: None,
        }
    }
}

impl From<SplitterNode> for NodeKind {
    fn from(node: SplitterNode) -> Self {
        NodeKind::Splitter(node)
    }
}
