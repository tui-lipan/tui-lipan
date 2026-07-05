use crate::callback::Callback;
use crate::core::event::MouseEvent;
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{Padding, Style, StyleSlot, Theme, ThemeRole};
use crate::utils::gradient::ColorGradient;

use super::{ProgressBar, ProgressEvent, ProgressStyle, ProgressTextPosition, ProgressZone};

#[derive(Clone)]
pub struct ProgressNode {
    pub progress: f64,
    pub progress_style: ProgressStyle,
    pub show_percentage: bool,
    pub percentage_position: ProgressTextPosition,
    pub label: Option<String>,
    pub label_position: ProgressTextPosition,
    pub filled_style: Style,
    pub filled_gradient: Option<ColorGradient>,
    pub empty_style: Style,
    pub label_style: Style,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub target: Option<f64>,
    pub target_style: Style,
    pub target_symbol: char,
    pub zones: Vec<ProgressZone>,
    pub block_empty_bg_dim: f32,
    pub padding: Padding,
    pub draggable: bool,
    pub step: Option<f64>,
    pub inverted: bool,
    pub on_change: Option<Callback<ProgressEvent>>,
    pub on_click: Option<Callback<MouseEvent>>,
    pub focusable: bool,
    pub focus_style: StyleSlot,
}

impl WidgetNode for ProgressNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }
    fn has_on_click(&self) -> bool {
        self.on_click.is_some() || self.on_change.is_some() || self.draggable
    }
    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        // Only hoverable if explicitly styled for hover, or has an on_click handler.
        // Having on_change or being draggable does not make the widget hoverable
        // since those don't require visual hover feedback.
        if self.on_click.is_some() {
            return true;
        }
        self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
    }
}

impl From<ProgressBar> for ProgressNode {
    fn from(value: ProgressBar) -> Self {
        Self {
            progress: value.progress,
            progress_style: value.progress_style,
            show_percentage: value.show_percentage,
            percentage_position: value.percentage_position,
            label: value.label,
            label_position: value.label_position,
            filled_style: value.filled_style,
            filled_gradient: value.filled_gradient,
            empty_style: value.empty_style,
            label_style: value.label_style,
            style: value.style,
            hover_style: value.hover_style,
            target: value.target,
            target_style: value.target_style,
            target_symbol: value.target_symbol,
            zones: value.zones,
            block_empty_bg_dim: value.block_empty_bg_dim,
            padding: value.padding,
            draggable: value.draggable,
            step: value.step,
            inverted: value.inverted,
            on_change: value.on_change,
            on_click: value.on_click,
            focusable: value.focusable,
            focus_style: value.focus_style,
        }
    }
}

impl From<ProgressNode> for NodeKind {
    fn from(node: ProgressNode) -> Self {
        NodeKind::ProgressBar(node)
    }
}
