use std::sync::Arc;

use crate::core::node::{NodeKind, WidgetNode};
use crate::style::Style;

use super::{Spinner, SpinnerSpeed, SpinnerStyle};

#[derive(Clone)]
pub struct SpinnerNode {
    pub spinner_style: SpinnerStyle,
    pub speed: SpinnerSpeed,
    pub frame: usize,
    pub auto_frame: bool,
    pub label: Option<Arc<str>>,
    pub gap: u16,
    pub style: Style,
    pub label_style: Style,
}

impl WidgetNode for SpinnerNode {}

impl From<Spinner> for SpinnerNode {
    fn from(spinner: Spinner) -> Self {
        Self {
            spinner_style: spinner.spinner_style,
            speed: spinner.speed,
            frame: spinner.frame.unwrap_or(0),
            auto_frame: spinner.frame.is_none(),
            label: spinner.label,
            gap: spinner.gap,
            style: spinner.style,
            label_style: spinner.label_style,
        }
    }
}

impl From<SpinnerNode> for NodeKind {
    fn from(node: SpinnerNode) -> Self {
        NodeKind::Spinner(node)
    }
}
