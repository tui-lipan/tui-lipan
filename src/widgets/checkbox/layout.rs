use unicode_width::UnicodeWidthStr;

use super::Checkbox;

pub fn measure_checkbox(checkbox: &Checkbox) -> (u16, u16) {
    let symbol_w = checkbox.variant.width() as usize;
    let label_w = checkbox
        .label
        .as_ref()
        .map(|l| UnicodeWidthStr::width(l.as_ref()))
        .unwrap_or(0);

    let mut w = symbol_w;
    if label_w > 0 {
        w = w
            .saturating_add(checkbox.gap as usize)
            .saturating_add(label_w);
    }

    w = w.saturating_add(checkbox.padding.horizontal() as usize);
    let h = 1usize.saturating_add(checkbox.padding.vertical() as usize);

    let w = w.min(u16::MAX as usize) as u16;
    let h = h.min(u16::MAX as usize) as u16;
    (w, h)
}
