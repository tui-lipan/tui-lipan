use crate::core::element::Element;
use crate::layout::measure::min_size_constrained;
use crate::style::Length;

use super::PanView;

pub(crate) fn measure_pan_view(
    pan: &PanView,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    let child_size = pan
        .child
        .as_deref()
        .map(|child: &Element| min_size_constrained(child, None, None))
        .unwrap_or((0, 0));
    let avail_w = max_w.unwrap_or(u16::MAX);
    let avail_h = max_h.unwrap_or(u16::MAX);
    let w = match pan.width {
        Length::Auto => child_size.0,
        Length::Px(px) => px,
        Length::Flex(_) if max_w.is_some() => avail_w,
        Length::Flex(_) => child_size.0,
        Length::Percent(p) => {
            let base = max_w.unwrap_or(child_size.0);
            ((u32::from(base) * u32::from(p)) / 100).min(u32::from(u16::MAX)) as u16
        }
    };
    let h = match pan.height {
        Length::Auto => child_size.1,
        Length::Px(px) => px,
        Length::Flex(_) if max_h.is_some() => avail_h,
        Length::Flex(_) => child_size.1,
        Length::Percent(p) => {
            let base = max_h.unwrap_or(child_size.1);
            ((u32::from(base) * u32::from(p)) / 100).min(u32::from(u16::MAX)) as u16
        }
    };
    (w.min(avail_w), h.min(avail_h))
}
