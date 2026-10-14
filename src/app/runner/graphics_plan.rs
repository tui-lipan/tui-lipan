//! Dependencies of the cached native presentation plan. Compare complete geometry values so a
//! layout animation cannot reuse a plan merely because reconciliation kept the same node IDs.
use crate::app::AppRunner;
use crate::backend::ratatui_backend::image_support;
use crate::core::component::Component;
use crate::core::node::NodeId;
use crate::style::Rect;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GraphicsPresentationEpochs {
    layout: u32,
    terminal_geometry: Vec<(NodeId, Rect, usize, usize, usize)>,
    overlay_composition: Vec<(NodeId, Rect)>,
    backdrop_effects: (u64, u64, u64),
    host_cell_geometry: Option<(u16, u16)>,
    host_capability: (ratatui_image::picker::ProtocolType, bool),
    frame_area: ratatui::layout::Rect,
}

impl<C: Component + 'static> AppRunner<C> {
    pub(super) fn graphics_presentation_epochs(
        &self,
        frame_area: ratatui::layout::Rect,
    ) -> GraphicsPresentationEpochs {
        let env = self.core.ctx.env();
        GraphicsPresentationEpochs {
            layout: self.core.tree.epoch(),
            terminal_geometry: self.core.tree.live_terminal_geometry(),
            overlay_composition: self
                .core
                .tree
                .overlay_roots()
                .iter()
                .map(|owner| (owner.id, self.core.tree.node(owner.id).rect))
                .collect(),
            backdrop_effects: (
                self.animation.effect_phase_tick,
                env.animations.generation(),
                env.host_terminal_color_generation(),
            ),
            host_cell_geometry: image_support::measured_host_cell_size(),
            host_capability: (
                image_support::picker_snapshot().protocol_type(),
                image_support::image_rendering_suspended(),
            ),
            frame_area,
        }
    }
}
