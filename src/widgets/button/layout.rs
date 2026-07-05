use unicode_width::UnicodeWidthStr;

use crate::style::Length;

use super::{Button, ButtonVariant};

pub fn measure_button(button: &Button) -> (u16, u16) {
    let label_w = UnicodeWidthStr::width(button.label.as_ref()).min(u16::MAX as usize) as u16;
    let icon_w = button
        .icon
        .as_ref()
        .map(|icon| UnicodeWidthStr::width(icon.as_ref()) as u16)
        .unwrap_or(0);
    let shortcut_w = button
        .shortcut
        .as_ref()
        .map(|shortcut| UnicodeWidthStr::width(shortcut.as_ref()) as u16)
        .unwrap_or(0);
    let icon_gap = if icon_w > 0 { button.icon_gap } else { 0 };
    let shortcut_gap = if shortcut_w > 0 {
        button.shortcut_gap
    } else {
        0
    };
    let content_w = label_w
        .saturating_add(icon_w)
        .saturating_add(shortcut_w)
        .saturating_add(icon_gap)
        .saturating_add(shortcut_gap);

    let (w, h) = match button.variant {
        ButtonVariant::Bracket => {
            let w = content_w
                .saturating_add(button.padding.horizontal())
                .saturating_add(2);
            (w, 1)
        }
        ButtonVariant::Filled => {
            let w = content_w.saturating_add(button.padding.horizontal());
            let h = button.padding.vertical().saturating_add(1);
            (w, h)
        }
        ButtonVariant::Outlined => {
            let w = content_w
                .saturating_add(button.padding.horizontal())
                .saturating_add(2);
            let h = button.padding.vertical().saturating_add(3);
            (w, h)
        }
    };

    let w = match button.width {
        Length::Px(px) => px,
        _ => w,
    };
    let h = match button.height {
        Length::Px(px) => px,
        _ => h,
    };

    (w, h)
}
