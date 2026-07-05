use unicode_width::UnicodeWidthStr;

use crate::widgets::Input;

pub(crate) fn measure_input(input: &Input) -> (u16, u16) {
    let value = input.value.as_ref();
    let value_w = UnicodeWidthStr::width(value.lines().next().unwrap_or(""));
    let ph_w = input
        .placeholder
        .as_ref()
        .map(|ph| UnicodeWidthStr::width(ph.lines().next().unwrap_or("")))
        .unwrap_or(0);

    let cursor_w = if input.focusable { 1 } else { 0 };
    let mut w = ph_w.max(value_w.saturating_add(cursor_w));

    if let Some(prefix) = &input.prefix {
        w = w.saturating_add(UnicodeWidthStr::width(prefix.as_ref()));
    }
    if let Some(suffix) = &input.suffix {
        w = w.saturating_add(UnicodeWidthStr::width(suffix.as_ref()));
    }

    let mut h = 1usize;

    w = w.saturating_add(input.padding.horizontal() as usize);
    h = h.saturating_add(input.padding.vertical() as usize);

    if input.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }

    if input.reserve_error_row || input.error.is_some() {
        h = h.saturating_add(1);
    }

    let w = w.min(u16::MAX as usize) as u16;
    let h = h.min(u16::MAX as usize) as u16;
    (w, h)
}
