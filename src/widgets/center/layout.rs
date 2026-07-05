use crate::layout::measure::min_size_constrained;

pub(crate) fn measure_center(
    center: &super::Center,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    center
        .child
        .as_deref()
        .map(|c| min_size_constrained(c, max_w, max_h))
        .unwrap_or((0, 0))
}
