#[cfg(feature = "devtools")]
use std::mem;
#[cfg(feature = "devtools")]
use std::rc::Rc;
#[cfg(feature = "devtools")]
use web_time::Instant;

use crate::core::component::Component;

use super::{AppRunner, DrawMode};

#[cfg(feature = "devtools")]
fn dirty_level_label(draw_mode: DrawMode) -> &'static str {
    match draw_mode {
        DrawMode::Full => "full",
        DrawMode::LayoutOnly => "layout",
        DrawMode::PaintOnly => "paint",
    }
}

impl<C: Component> AppRunner<C> {
    #[cfg(feature = "devtools")]
    pub(super) fn update_devtools_focus_metrics(&mut self) {
        let focused = self.focus.focused.filter(|id| self.core.tree.is_valid(*id));
        let (key, tag) = focused
            .map(|id| {
                let node = self.core.tree.node(id);
                (
                    node.key.clone(),
                    Some(crate::layout::tag::tag_of_node(node)),
                )
            })
            .unwrap_or_default();
        let ring_len = self.core.tree.top_capturing_overlay().map_or_else(
            || crate::app::input::focus::traversal_focusables(&self.core.tree, focused).len(),
            |overlay| {
                if overlay.auto_focus {
                    crate::app::focus_service::overlay_ring(&self.core.tree, overlay.id).len()
                } else {
                    0
                }
            },
        );
        self.devtools_state.borrow_mut().focus = crate::devtools::state::FocusMetrics {
            policy: self.focus.policy,
            node_id: focused,
            key,
            tag,
            ring_len,
            stack_depth: self.focus.focus_stack.len(),
        };
    }

    #[cfg(feature = "devtools")]
    pub(crate) fn record_devtools_frame_metrics(
        &mut self,
        draw_mode: DrawMode,
        total_duration: std::time::Duration,
        reconcile_duration: std::time::Duration,
        draw_duration: std::time::Duration,
    ) {
        // Drain first so pending attributions never leak across early returns
        // (suppressed catch-up frames, metrics disabled, panel hidden).
        let pending = mem::take(&mut self.pending_attributions);

        // A catch-up refresh frame only re-renders the panel with the metrics
        // recorded by the previous app frame. Recording here would both mislead
        // (it would time the refresh, not app work) and re-arm the refresh flag,
        // looping forever. Draw the panel, record nothing.
        if self.devtools_metrics_suppressed {
            return;
        }
        if !self.devtools_config.metrics {
            return;
        }
        if !self.devtools_state.borrow().visible {
            return;
        }

        let attributions = crate::devtools::state::finalize_frame_attributions(pending);

        let (memo_hits, memo_misses) = crate::core::nested::take_memo_counters();
        let mut node_count = self.core.tree.iter().count();
        if let Some(devtools_root) = self
            .core
            .tree
            .iter()
            .find(|node| {
                node.key
                    .as_ref()
                    .is_some_and(|key| key.as_ref() == crate::devtools::DEVTOOLS_KEY)
            })
            .map(|node| node.id)
        {
            let mut stack = vec![devtools_root];
            let mut devtools_nodes = 0usize;
            while let Some(id) = stack.pop() {
                if !self.core.tree.is_valid(id) {
                    continue;
                }
                devtools_nodes = devtools_nodes.saturating_add(1);
                for &child in &self.core.tree.node(id).children {
                    stack.push(child);
                }
            }
            node_count = node_count.saturating_sub(devtools_nodes);
        }

        self.devtools_state
            .borrow_mut()
            .push_frame_metrics(crate::devtools::FrameMetrics {
                timestamp: Instant::now(),
                dirty_level: dirty_level_label(draw_mode).to_string(),
                total_duration,
                reconcile_duration,
                draw_duration,
                node_count,
                overlay_count: self.core.tree.overlay_roots().len(),
                memo_hits: u64::from(memo_hits),
                memo_misses: u64::from(memo_misses),
                attributions,
            });

        // The panel just drawn shows the *previous* frame's metrics (it was built
        // before this record). Request one idle catch-up frame to surface these.
        self.devtools_refresh_pending = true;
    }

    #[cfg(feature = "devtools")]
    pub(super) fn install_devtools_overlay(&mut self) {
        if self.devtools_state.borrow().visible {
            self.core
                .set_extra_root_element(Some(crate::devtools::panel_element(Rc::clone(
                    &self.devtools_state,
                ))));
            return;
        }
        self.core.set_extra_root_element(None);
    }
}
