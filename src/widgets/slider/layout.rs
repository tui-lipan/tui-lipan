use unicode_width::UnicodeWidthStr;

use crate::style::Length;

use super::Slider;

pub fn measure_slider(slider: &Slider) -> (u16, u16) {
    let mut h = 1u16; // Default height

    // Calculate width
    let mut w = if let Length::Px(width) = slider.width {
        width
    } else {
        20 // Default width
    };

    if let Length::Px(height) = slider.height {
        h = height;
    }

    // Add label width if present
    if let Some(label) = &slider.label {
        let label_w = UnicodeWidthStr::width(label.as_str()) as u16;
        w = w.saturating_add(label_w).saturating_add(1); // +1 for gap
    }

    // Add value width if shown
    if slider.show_value {
        let value_w = super::value_slot_width(slider.min, slider.max);
        w = w.saturating_add(value_w).saturating_add(1);
    }

    // Add padding
    w = w.saturating_add(slider.padding.horizontal());
    h = h.saturating_add(slider.padding.vertical());

    (w, h)
}
