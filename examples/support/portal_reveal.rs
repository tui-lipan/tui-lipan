//! A reusable application-defined reveal effect, independent of widget lifecycle.
use tui_lipan::prelude::*;

#[derive(Debug)]
pub struct PortalReveal {
    pub progress: f32,
}
impl CellEffect for PortalReveal {
    fn apply(&self, _: &mut EffectCell, _: &EffectContext) {}
    fn uses_backdrop(&self) -> bool {
        true
    }
    fn apply_with_backdrop(
        &self,
        cell: &mut EffectCell,
        backdrop: &EffectCell,
        ctx: &EffectContext,
    ) {
        let half_w = f32::from(ctx.bounds.w.saturating_sub(1)) / 2.0;
        let half_h = f32::from(ctx.bounds.h.saturating_sub(1));
        let x = f32::from(ctx.x - ctx.bounds.x) - half_w;
        let y = f32::from(ctx.y - ctx.bounds.y) * 2.0 - half_h;
        let radius = half_w.hypot(half_h) * self.progress;
        if self.progress > 0.0 && x.hypot(y) <= radius {
            return;
        }
        *cell = backdrop.clone();
    }
}
