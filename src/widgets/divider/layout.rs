use crate::layout::measure::min_size_constrained;

use super::{Divider, Orientation};
pub fn measure_divider(divider: &Divider) -> (u16, u16) {
    if divider.orientation == Orientation::Horizontal
        && let Some(label) = divider.label.as_deref()
    {
        let (label_w, _) = min_size_constrained(label, None, Some(1));
        let padding = divider.label_padding.saturating_mul(2);
        return (label_w.saturating_add(padding).max(1), 1);
    }

    (1, 1)
}
