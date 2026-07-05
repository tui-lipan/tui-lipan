use unicode_width::UnicodeWidthStr;

use super::Spinner;

pub fn measure_spinner(spinner: &Spinner) -> (u16, u16) {
    let symbol_w = spinner.spinner_style.width() as usize;
    let label_w = spinner
        .label
        .as_ref()
        .map(|l| UnicodeWidthStr::width(l.as_ref()))
        .unwrap_or(0);

    let mut w = symbol_w;
    if label_w > 0 {
        w = w
            .saturating_add(spinner.gap as usize)
            .saturating_add(label_w);
    }

    let w = w.min(u16::MAX as usize) as u16;
    (w, 1)
}
