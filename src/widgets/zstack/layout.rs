use crate::layout::measure::min_size_constrained;

pub(crate) fn measure_zstack(
    zstack: &super::ZStack,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    let mut w = 0u16;
    let mut h = 0u16;
    for child in &zstack.children {
        let (cw, ch) = min_size_constrained(child, max_w, max_h);
        w = w.max(cw);
        h = h.max(ch);
    }
    (w, h)
}
