use crate::layout::measure::min_size_constrained;
use crate::style::Length;

use super::Animated;

pub(crate) fn measure_animated(
    animated: &Animated,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    let (w, natural_h) = min_size_constrained(animated.child.as_ref(), max_w, max_h);
    let available_h = max_h.unwrap_or(natural_h);
    let spec = animated.layout_height.as_ref().or(animated.height.as_ref());
    let height = match spec {
        None => natural_h,
        Some(Length::Auto) => natural_h,
        Some(Length::Px(px)) => *px,
        Some(Length::Percent(percent)) => Length::Percent(*percent).resolve(available_h, natural_h),
        Some(Length::Flex(_)) => available_h,
    };
    (w, height)
}
