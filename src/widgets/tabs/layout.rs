use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::Tabs;

pub fn measure_tabs(tabs: &Tabs) -> (u16, u16) {
    let mut w = 0usize;
    for (i, tab) in tabs.tabs.iter().enumerate() {
        w = w.saturating_add(UnicodeWidthStr::width(tab.label.as_ref()).saturating_add(2));
        if i + 1 < tabs.tabs.len() {
            w = w.saturating_add(UnicodeWidthChar::width(tabs.divider).unwrap_or(1));
        }
    }

    let mut h = 1usize;

    w = w.saturating_add(tabs.padding.horizontal() as usize);
    h = h.saturating_add(tabs.padding.vertical() as usize);

    if tabs.border {
        w = w.saturating_add(2);
        h = h.saturating_add(2);
    }

    let w = w.min(u16::MAX as usize) as u16;
    let h = h.min(u16::MAX as usize) as u16;
    (w, h)
}
