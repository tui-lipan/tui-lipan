#[cfg(feature = "devtools")]
use std::mem;
#[cfg(feature = "devtools")]
use std::rc::Rc;
#[cfg(feature = "devtools")]
use std::time::Duration;
#[cfg(feature = "devtools")]
use web_time::Instant;

use crate::core::component::Component;

use super::{AppRunner, DrawMode};

/// The most often DevTools rebuilds its panel to show what changed. Each rebuild runs only the
/// panel's view and repaints only its cells.
#[cfg(feature = "devtools")]
const DEVTOOLS_PANEL_REFRESH_INTERVAL: Duration = Duration::from_millis(250);

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
        // Drain first so pending attributions / memo stats never leak across
        // early returns (panel refreshes, metrics disabled, panel hidden).
        let pending = mem::take(&mut self.pending_attributions);
        let memo_stats = crate::core::nested::take_memo_frame_stats();
        let view_samples = crate::core::nested::take_view_timings();

        // A panel refresh only re-renders the panel with metrics app frames
        // recorded. Recording here would both mislead (it would time DevTools,
        // not the app) and mark the panel stale again, refreshing forever.
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
        let input_sourced_full = matches!(draw_mode, DrawMode::Full)
            && attributions.iter().any(|a| {
                matches!(a.source, crate::devtools::state::UpdateSource::Input(_))
                    && a.level == crate::app::interaction_state::DirtyLevel::Full
            });
        let component_timings = crate::core::nested::aggregate_view_timings(view_samples, 5)
            .into_iter()
            .map(|t| crate::devtools::state::ComponentTiming {
                name: t.name,
                scope: t.scope,
                duration: t.duration,
                calls: t.calls,
            })
            .collect();

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
                memo_hits: u64::from(memo_stats.hits),
                memo_misses: u64::from(memo_stats.misses),
                memo_miss_reasons: memo_stats.reasons,
                attributions,
                component_timings,
                input_sourced_full,
            });

        // The panel just drawn shows the *previous* frame's metrics (it was built
        // before this record), so it is behind until its next refresh.
        self.devtools_panel_stale = true;
    }

    /// How long until the panel should be rebuilt on its own: `Some(ZERO)` when due now, `None`
    /// when it shows everything there is to show and nothing needs to wake the loop for it.
    ///
    /// Folds in the staleness sources that are polled rather than pushed: app metric rows the host
    /// published and frame samples ageing out of the FPS window.
    #[cfg(feature = "devtools")]
    pub(crate) fn devtools_panel_refresh_wait(&mut self, now: Instant) -> Option<Duration> {
        let mut state = self.devtools_state.borrow_mut();
        if !state.visible {
            return None;
        }
        if state.app_metrics.take_changed() && state.is_app_tab_active() {
            self.devtools_panel_stale = true;
        }
        if state.expire_fps_samples(now) && state.is_stats_tab_active() {
            self.devtools_panel_stale = true;
        }
        if self.devtools_panel_stale {
            let due = self
                .devtools_panel_refreshed_at
                .map_or(now, |at| at + DEVTOOLS_PANEL_REFRESH_INTERVAL);
            return Some(due.saturating_duration_since(now));
        }
        state
            .next_fps_expiry()
            .filter(|_| state.is_stats_tab_active())
            .map(|expiry| expiry.saturating_duration_since(now))
    }

    /// Rebuild the DevTools panel alone and repaint only the cells it covers.
    ///
    /// Only the panel's own view runs, so the app's `view()` is not called, and the draw is
    /// confined to the panel's area before and after the rebuild, so the rest of the screen is
    /// neither repainted nor compared. Anything that rules out a partial repaint falls back to an
    /// ordinary draw of the tree. The frame is DevTools' own work: it records no metrics.
    #[cfg(feature = "devtools")]
    pub(crate) fn render_devtools_panel_refresh(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> crate::Result<()> {
        if self.surface.is_transcript() && self.surface.inline.transcript_reset_pending {
            return Ok(());
        }
        self.devtools_panel_stale = false;
        self.devtools_panel_refreshed_at = Some(Instant::now());
        // Framework log lines written while DevTools refreshes itself would mark it stale again.
        let _log_guard = crate::debug::suppress_devtools_log();
        self.devtools_metrics_suppressed = true;
        crate::core::nested::set_frame_diagnostics_enabled(false);
        let result = self.refresh_devtools_panel(terminal);
        self.devtools_metrics_suppressed = false;
        // Nothing gathered while the panel rebuilt belongs to the app's next frame.
        let _ = crate::core::nested::take_memo_frame_stats();
        let _ = crate::core::nested::take_view_timings();
        result
    }

    #[cfg(feature = "devtools")]
    fn refresh_devtools_panel(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> crate::Result<()> {
        let Some(scope) = self.core.extra_root_scope() else {
            return self.render(terminal);
        };
        let size = terminal.size()?;
        let bounds = self.content_bounds(size.width, size.height);
        let before = self.devtools_panel_rect();
        if !self
            .core
            .refresh_cached_scopes(&[scope], bounds, self.mouse.hovered)
            || !self.core.reconcile_cached_element(
                bounds,
                self.focus.focused,
                self.focus.focused_key.as_ref(),
                self.mouse.hovered,
            )
        {
            return self.render(terminal);
        }
        self.settle_after_reconcile()?;
        if self.skip_draw {
            return Ok(());
        }
        let frame_area = terminal.get_frame().area();
        let areas: Vec<_> = [before, self.devtools_panel_rect()]
            .into_iter()
            .flatten()
            .collect();
        match self.plan_area_damage(&areas, frame_area) {
            Ok(plan) => self.draw_row_damage(terminal, &plan)?,
            Err(super::row_damage::DamageRejection::NothingVisible) => {}
            Err(_) => {
                self.draw_current_tree(terminal, DrawMode::LayoutOnly)?;
            }
        }
        self.sync_mouse_motion_capture(terminal)?;
        Ok(())
    }

    /// Where the panel's frame sits, which is all of the panel that paints anything: the stack
    /// around it spans the viewport only to anchor it to the bottom.
    #[cfg(feature = "devtools")]
    fn devtools_panel_rect(&self) -> Option<crate::style::Rect> {
        self.core
            .tree
            .iter()
            .find(|node| {
                node.key
                    .as_ref()
                    .is_some_and(|key| key.as_ref() == crate::devtools::DEVTOOLS_KEY)
            })
            .map(|node| node.rect)
    }

    #[cfg(feature = "devtools")]
    pub(crate) fn install_devtools_overlay(&mut self) {
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
