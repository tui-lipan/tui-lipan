//! Slider widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_slider;
pub use node::SliderNode;
pub use reconcile::reconcile_slider;

use unicode_width::UnicodeWidthStr;

use crate::callback::Callback;
use crate::core::element::Element;
use crate::style::{Length, Padding, Style, StyleSlot};
use crate::utils::gradient::ColorGradient;

/// A slider for numeric selection.
#[derive(Clone)]
pub struct Slider {
    /// Current value.
    pub value: f64,
    /// Minimum value.
    pub min: f64,
    /// Maximum value.
    pub max: f64,
    /// Step size.
    pub step: f64,
    /// Callback when value changes.
    pub on_change: Option<Callback<f64>>,
    /// Callback when clicked.
    pub on_click: Option<Callback<f64>>,
    /// Track style.
    pub style: Style,
    /// Style for the filled portion of the track.
    pub filled_track_style: Style,
    /// Optional gradient for the filled portion of the track (left -> right).
    pub filled_track_gradient: Option<ColorGradient>,
    /// Thumb style.
    pub thumb_style: Style,
    /// Optional gradient for thumb color based on current value.
    pub thumb_gradient: Option<ColorGradient>,
    /// Label text.
    pub label: Option<String>,
    /// Label style.
    pub label_style: Style,
    /// Whether to show value text.
    pub show_value: bool,
    /// Requested width.
    /// Default: `Length::Flex(1)`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Px(1)`.
    pub height: Length,
    /// Padding.
    /// Default: `Padding::default()`.
    pub padding: Padding,
    /// Whether the slider is focusable.
    pub focusable: bool,
    /// Style when focused.
    pub focus_style: StyleSlot,
    /// Thumb style when focused.
    pub focus_thumb_style: StyleSlot,
    /// Style for the thumb when hovered.
    pub hover_thumb_style: StyleSlot,
    /// Symbol for the thumb.
    pub thumb_symbol: String,
    /// Symbol for the unfilled track.
    pub track_symbol: String,
    /// Symbol for the filled track.
    pub filled_track_symbol: String,
    /// Symbol for the thumb when hovered.
    pub hover_thumb_symbol: Option<String>,
}

impl Slider {
    /// Create a new slider.
    pub fn new(value: f64) -> Self {
        Self {
            value,
            min: 0.0,
            max: 100.0,
            step: 1.0,
            on_change: None,
            on_click: None,
            style: Style::default(),
            filled_track_style: Style::default(),
            filled_track_gradient: None,
            thumb_style: Style::default(),
            thumb_gradient: None,
            label: None,
            label_style: Style::default(),
            show_value: true,
            width: Length::Flex(1),
            height: Length::Px(1),
            padding: Padding::default(),
            focusable: true,
            focus_style: StyleSlot::Inherit,
            focus_thumb_style: StyleSlot::Inherit,
            hover_thumb_style: StyleSlot::Inherit,
            thumb_symbol: "●".to_string(),
            track_symbol: "─".to_string(),
            filled_track_symbol: "━".to_string(),
            hover_thumb_symbol: None,
        }
    }

    /// Set minimum value.
    pub fn min(mut self, min: f64) -> Self {
        self.min = min;
        self
    }

    /// Set maximum value.
    pub fn max(mut self, max: f64) -> Self {
        self.max = max;
        self
    }

    /// Set step value.
    pub fn step(mut self, step: f64) -> Self {
        self.step = step;
        self
    }

    /// Set label.
    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set on-change callback.
    pub fn on_change(mut self, cb: Callback<f64>) -> Self {
        self.on_change = Some(cb);
        self
    }

    /// Set on-click callback.
    pub fn on_click(mut self, cb: Callback<f64>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set label style.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Set whether to show value text.
    pub fn show_value(mut self, show: bool) -> Self {
        self.show_value = show;
        self
    }

    /// Set style for the track.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set style for the filled portion of the track.
    pub fn filled_track_style(mut self, style: Style) -> Self {
        self.filled_track_style = style;
        self
    }

    /// Set gradient for the filled portion of the track.
    pub fn filled_track_gradient(mut self, gradient: ColorGradient) -> Self {
        self.filled_track_gradient = Some(gradient);
        self
    }

    /// Set style for the thumb.
    pub fn thumb_style(mut self, style: Style) -> Self {
        self.thumb_style = style;
        self
    }

    /// Set gradient for the thumb based on current value.
    pub fn thumb_gradient(mut self, gradient: ColorGradient) -> Self {
        self.thumb_gradient = Some(gradient);
        self
    }

    /// Set requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set whether the slider is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Set style when focused.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed focus style with the given style.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit focus style from the active theme.
    pub fn inherit_focus_style(mut self) -> Self {
        self.focus_style = StyleSlot::Inherit;
        self
    }

    /// Set the focus style slot directly.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_style = slot;
        self
    }

    /// Set thumb style when focused.
    pub fn focus_thumb_style(mut self, style: Style) -> Self {
        self.focus_thumb_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed focused thumb style with the given style.
    pub fn extend_focus_thumb_style(mut self, style: Style) -> Self {
        self.focus_thumb_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit focused thumb style from the active theme.
    pub fn inherit_focus_thumb_style(mut self) -> Self {
        self.focus_thumb_style = StyleSlot::Inherit;
        self
    }

    /// Set the focused thumb style slot directly.
    pub fn focus_thumb_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_thumb_style = slot;
        self
    }

    /// Set thumb style when hovered.
    pub fn hover_thumb_style(mut self, style: Style) -> Self {
        self.hover_thumb_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hovered thumb style with the given style.
    pub fn extend_hover_thumb_style(mut self, style: Style) -> Self {
        self.hover_thumb_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit hovered thumb style from the active theme.
    pub fn inherit_hover_thumb_style(mut self) -> Self {
        self.hover_thumb_style = StyleSlot::Inherit;
        self
    }

    /// Set the hovered thumb style slot directly.
    pub fn hover_thumb_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_thumb_style = slot;
        self
    }

    /// Set thumb symbol.
    pub fn thumb_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.thumb_symbol = symbol.into();
        self
    }

    /// Set track symbol.
    pub fn track_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.track_symbol = symbol.into();
        self
    }

    /// Set filled track symbol.
    pub fn filled_track_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.filled_track_symbol = symbol.into();
        self
    }

    /// Set thumb symbol when hovered.
    pub fn hover_thumb_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.hover_thumb_symbol = Some(symbol.into());
        self
    }
}

impl From<Slider> for Element {
    fn from(mut slider: Slider) -> Self {
        // Validate and fix inverted ranges to prevent NaN/Inf in layout and rendering.
        if slider.min > slider.max {
            std::mem::swap(&mut slider.min, &mut slider.max);
        }
        // Ensure min != max to avoid division by zero.
        if (slider.max - slider.min).abs() < f64::EPSILON {
            slider.max = slider.min + 1.0;
        }
        // Clamp value to valid range.
        slider.value = slider.value.clamp(slider.min, slider.max);
        Element::new(crate::core::element::ElementKind::Slider(slider))
    }
}

pub(crate) fn value_slot_width(min: f64, max: f64) -> u16 {
    let min_text = format!("{:.1}", min);
    let max_text = format!("{:.1}", max);
    let width = UnicodeWidthStr::width(min_text.as_str())
        .max(UnicodeWidthStr::width(max_text.as_str()))
        .min(u16::MAX as usize) as u16;
    width.max(1)
}

impl crate::layout::hash::LayoutHash for Slider {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&crate::core::element::Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.label.hash(hasher);
        self.padding.hash(hasher);
        Some(())
    }
}
