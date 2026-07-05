use crate::layout::measure::min_size_constrained;

use super::drop_target::DropTarget;

pub(crate) fn measure_drop_target(
    target: &DropTarget,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    if let Some(child) = target.child.as_deref() {
        min_size_constrained(child, max_w, max_h)
    } else {
        (0, 0)
    }
}
