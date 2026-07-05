use crate::layout::measure::min_size_constrained;

use super::EffectScope;

pub(crate) fn measure_effect_scope(
    scope: &EffectScope,
    max_w: Option<u16>,
    max_h: Option<u16>,
) -> (u16, u16) {
    if let Some(child) = scope.child.as_deref() {
        min_size_constrained(child, max_w, max_h)
    } else {
        (0, 0)
    }
}
