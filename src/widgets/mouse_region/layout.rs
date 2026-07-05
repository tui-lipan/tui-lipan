use crate::layout::measure::min_size_constrained;

use super::MouseRegion;

pub(crate) fn measure_mouse_region(
    region: &MouseRegion,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    if let Some(child) = region.child.as_deref() {
        min_size_constrained(child, max_w, max_h)
    } else {
        (0, 0)
    }
}
