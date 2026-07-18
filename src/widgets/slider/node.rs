use crate::callback::Callback;
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{Padding, Style, StyleSlot, Theme, ThemeRole};
use crate::utils::gradient::ColorGradient;

use super::Slider;

#[derive(Clone)]
pub struct SliderNode {
    pub value: f64,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub on_change: Option<Callback<f64>>,
    pub on_click: Option<Callback<f64>>,
    pub style: Style,
    pub filled_track_style: Style,
    pub filled_track_gradient: Option<ColorGradient>,
    pub thumb_style: Style,
    pub thumb_gradient: Option<ColorGradient>,
    pub label: Option<String>,
    pub label_style: Style,
    pub show_value: bool,
    pub padding: Padding,
    pub focusable: bool,
    pub tab_stop: bool,
    pub on_focus: Option<Callback<()>>,
    pub on_blur: Option<Callback<()>>,
    pub focus_style: StyleSlot,
    pub focus_thumb_style: StyleSlot,
    pub hover_thumb_style: StyleSlot,
    pub thumb_symbol: String,
    pub track_symbol: String,
    pub filled_track_symbol: String,
    pub hover_thumb_symbol: Option<String>,
}

impl WidgetNode for SliderNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }
    fn is_tab_stop(&self) -> bool {
        self.focusable && self.tab_stop
    }
    fn on_focus_callback(&self) -> Option<&Callback<()>> {
        self.on_focus.as_ref()
    }
    fn on_blur_callback(&self) -> Option<&Callback<()>> {
        self.on_blur.as_ref()
    }
    fn has_on_click(&self) -> bool {
        self.on_click.is_some() || self.on_change.is_some()
    }
    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        // Only hoverable if explicitly styled for hover, or has an on_click handler.
        // Having on_change alone does not make the widget hoverable since clicking
        // changes the value without needing visual hover feedback.
        if self.on_click.is_some() {
            return true;
        }
        self.hover_thumb_style
            .resolves_non_empty(theme, ThemeRole::Hover)
            || self.hover_thumb_symbol.is_some()
    }
}

impl From<Slider> for SliderNode {
    fn from(value: Slider) -> Self {
        Self {
            value: value.value,
            min: value.min,
            max: value.max,
            step: value.step,
            on_change: value.on_change,
            on_click: value.on_click,
            style: value.style,
            filled_track_style: value.filled_track_style,
            filled_track_gradient: value.filled_track_gradient,
            thumb_style: value.thumb_style,
            thumb_gradient: value.thumb_gradient,
            label: value.label,
            label_style: value.label_style,
            show_value: value.show_value,
            padding: value.padding,
            focusable: value.focusable,
            tab_stop: value.tab_stop,
            on_focus: value.on_focus,
            on_blur: value.on_blur,
            focus_style: value.focus_style,
            focus_thumb_style: value.focus_thumb_style,
            hover_thumb_style: value.hover_thumb_style,
            thumb_symbol: value.thumb_symbol,
            track_symbol: value.track_symbol,
            filled_track_symbol: value.filled_track_symbol,
            hover_thumb_symbol: value.hover_thumb_symbol,
        }
    }
}

impl From<SliderNode> for NodeKind {
    fn from(node: SliderNode) -> Self {
        NodeKind::Slider(node)
    }
}
