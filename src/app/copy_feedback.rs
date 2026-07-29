//! Brief visual feedback after a successful selection copy.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use crate::app::input::keymap::Keymap;
use crate::app::interaction_state::DirtyLevel;
use crate::clipboard::{ClipboardConfig, ClipboardService};
use crate::core::event::KeyEvent;
use crate::core::node::{NodeId, NodeTree};
use crate::style::Span;
use crate::ui::capabilities::ClipboardContext;
use crate::ui::router::{self, ClipboardDispatchOutcome};
use crate::utils::GridSelection;

#[derive(Clone)]
enum CopyFeedbackPhase {
    /// The selection has been copied and should render as active on the next
    /// paint. The wall-clock deadline is intentionally not armed until the
    /// frame after that first paint, so queued input cannot eat into the visible
    /// flash duration before users see it.
    Pending {
        duration: Duration,
    },
    Active {
        deadline: Instant,
    },
}

#[derive(Clone)]
struct CopyFeedbackFlash {
    phase: CopyFeedbackPhase,
    /// The range to paint, for callers that copied a range they are no longer
    /// keeping selected. `None` flashes whatever the node currently has selected.
    #[cfg_attr(not(feature = "terminal"), allow(dead_code))]
    range: Option<CopyFeedbackRange>,
}

/// A copied range paired with the rows that were visible when it was copied.
#[derive(Clone)]
#[cfg_attr(not(feature = "terminal"), allow(dead_code))]
pub(crate) struct CopyFeedbackRange {
    pub(crate) selection: GridSelection,
    pub(crate) first_row: usize,
    pub(crate) lines: Arc<[Vec<Span>]>,
}

impl CopyFeedbackRange {
    #[cfg_attr(not(feature = "terminal"), allow(dead_code))]
    pub(crate) fn line(&self, row: usize) -> Option<&[Span]> {
        row.checked_sub(self.first_row)
            .and_then(|row| self.lines.get(row))
            .map(Vec::as_slice)
    }
}

pub(crate) fn capture_terminal_range(
    tree: &NodeTree,
    id: NodeId,
    selection: GridSelection,
) -> Option<CopyFeedbackRange> {
    #[cfg(feature = "terminal")]
    {
        let crate::core::node::NodeKind::Terminal(node) = &tree.node(id).kind else {
            return None;
        };
        let (start, end) = selection.normalized();
        let first_row = start.row.min(node.lines.len());
        let end_exclusive = end.row.saturating_add(1).min(node.lines.len());
        let lines = if first_row < end_exclusive {
            Arc::from(node.lines[first_row..end_exclusive].to_vec())
        } else {
            Arc::from([])
        };
        Some(CopyFeedbackRange {
            selection,
            first_row,
            lines,
        })
    }
    #[cfg(not(feature = "terminal"))]
    {
        let _ = (tree, id, selection);
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct CopyFeedbackTick {
    pub(crate) needs_paint: bool,
    pub(crate) next_due: Option<Duration>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CopyFeedbackDispatch {
    pub(crate) handled: bool,
    pub(crate) mutated: bool,
    pub(crate) dirty_override: Option<DirtyLevel>,
}

#[derive(Default)]
pub(crate) struct CopyFeedbackState {
    flashes: HashMap<NodeId, CopyFeedbackFlash>,
}

impl CopyFeedbackState {
    pub fn trigger(&mut self, id: NodeId, duration: Duration) {
        self.trigger_range(id, duration, None);
    }

    /// Arm a flash that paints `range` regardless of what the node has selected.
    ///
    /// `None` keeps the historical behavior of flashing the live selection.
    pub fn trigger_range(
        &mut self,
        id: NodeId,
        duration: Duration,
        range: Option<CopyFeedbackRange>,
    ) {
        if duration.is_zero() {
            return;
        }
        self.flashes.insert(
            id,
            CopyFeedbackFlash {
                phase: CopyFeedbackPhase::Pending { duration },
                range,
            },
        );
    }

    pub fn is_active(&self, id: NodeId) -> bool {
        let now = Instant::now();
        self.flashes
            .get(&id)
            .is_some_and(|flash| match flash.phase {
                CopyFeedbackPhase::Pending { .. } => true,
                CopyFeedbackPhase::Active { deadline } => now < deadline,
            })
    }

    /// The explicit range of an active flash, if one was supplied.
    #[cfg_attr(not(feature = "terminal"), allow(dead_code))]
    pub fn active_range(&self, id: NodeId) -> Option<CopyFeedbackRange> {
        if !self.is_active(id) {
            return None;
        }
        self.flashes.get(&id).and_then(|flash| flash.range.clone())
    }

    pub fn tick(&mut self) -> CopyFeedbackTick {
        if self.flashes.is_empty() {
            return CopyFeedbackTick::default();
        }

        let now = Instant::now();
        let mut tick = CopyFeedbackTick::default();

        self.flashes.retain(|_, flash| match flash.phase {
            CopyFeedbackPhase::Pending { duration } => {
                flash.phase = CopyFeedbackPhase::Active {
                    deadline: now + duration,
                };
                tick.next_due = min_due(tick.next_due, duration);
                true
            }
            CopyFeedbackPhase::Active { deadline } if now < deadline => {
                tick.next_due = min_due(tick.next_due, deadline.saturating_duration_since(now));
                true
            }
            CopyFeedbackPhase::Active { .. } => {
                tick.needs_paint = true;
                false
            }
        });

        tick
    }
}

fn min_due(current: Option<Duration>, candidate: Duration) -> Option<Duration> {
    Some(current.map_or(candidate, |current| current.min(candidate)))
}

pub(crate) fn register_copy_feedback(
    feedback: &mut CopyFeedbackState,
    config: &ClipboardConfig,
    node_id: NodeId,
    outcome: ClipboardDispatchOutcome,
) -> bool {
    if !outcome.copied || config.copy_feedback_duration_ms == 0 {
        return false;
    }
    feedback.trigger(
        node_id,
        Duration::from_millis(config.copy_feedback_duration_ms as u64),
    );
    true
}

pub(crate) fn dispatch_clipboard_with_feedback_result(
    key: KeyEvent,
    keymap: &Keymap,
    context: &mut dyn ClipboardContext,
    clipboard: &ClipboardService,
    config: &ClipboardConfig,
    feedback: &mut CopyFeedbackState,
    node_id: NodeId,
) -> CopyFeedbackDispatch {
    let outcome = router::dispatch_clipboard(key, keymap, context, clipboard, config);
    dispatch_result_from_outcome(feedback, config, node_id, outcome)
}

fn dispatch_result_from_outcome(
    feedback: &mut CopyFeedbackState,
    config: &ClipboardConfig,
    node_id: NodeId,
    outcome: ClipboardDispatchOutcome,
) -> CopyFeedbackDispatch {
    let feedback_registered = register_copy_feedback(feedback, config, node_id, outcome);
    let dirty_override = if outcome.handled && !outcome.mutated {
        Some(if feedback_registered {
            DirtyLevel::PaintOnly
        } else {
            DirtyLevel::None
        })
    } else {
        None
    };

    CopyFeedbackDispatch {
        handled: outcome.handled,
        mutated: outcome.mutated,
        dirty_override,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::node::NodeId;

    #[test]
    fn register_copy_feedback_tracks_active_node() {
        let mut feedback = CopyFeedbackState::default();
        let config = ClipboardConfig::default();
        let id = NodeId::new(1, 0);
        register_copy_feedback(
            &mut feedback,
            &config,
            id,
            ClipboardDispatchOutcome {
                handled: true,
                copied: true,
                mutated: false,
            },
        );
        assert!(feedback.is_active(id));
    }

    #[test]
    fn pending_feedback_does_not_expire_before_first_tick() {
        let mut feedback = CopyFeedbackState::default();
        let id = NodeId::new(1, 0);

        feedback.trigger(id, Duration::from_millis(1));
        std::thread::sleep(Duration::from_millis(5));

        assert!(feedback.is_active(id));
        let tick = feedback.tick();
        assert!(!tick.needs_paint);
        assert!(tick.next_due.is_some());
        assert!(feedback.is_active(id));
    }
}
