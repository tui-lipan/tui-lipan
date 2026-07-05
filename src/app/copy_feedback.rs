//! Brief visual feedback after a successful selection copy.

use std::collections::HashMap;
use std::time::Duration;

use web_time::Instant;

use crate::app::input::keymap::Keymap;
use crate::app::interaction_state::DirtyLevel;
use crate::clipboard::{ClipboardConfig, ClipboardService};
use crate::core::event::KeyEvent;
use crate::core::node::NodeId;
use crate::ui::capabilities::ClipboardContext;
use crate::ui::router::{self, ClipboardDispatchOutcome};

#[derive(Clone, Copy)]
enum CopyFeedbackFlash {
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
        if duration.is_zero() {
            return;
        }
        self.flashes
            .insert(id, CopyFeedbackFlash::Pending { duration });
    }

    pub fn is_active(&self, id: NodeId) -> bool {
        let now = Instant::now();
        self.flashes.get(&id).is_some_and(|flash| match *flash {
            CopyFeedbackFlash::Pending { .. } => true,
            CopyFeedbackFlash::Active { deadline } => now < deadline,
        })
    }

    pub fn tick(&mut self) -> CopyFeedbackTick {
        if self.flashes.is_empty() {
            return CopyFeedbackTick::default();
        }

        let now = Instant::now();
        let mut tick = CopyFeedbackTick::default();

        self.flashes.retain(|_, flash| match *flash {
            CopyFeedbackFlash::Pending { duration } => {
                *flash = CopyFeedbackFlash::Active {
                    deadline: now + duration,
                };
                tick.next_due = min_due(tick.next_due, duration);
                true
            }
            CopyFeedbackFlash::Active { deadline } if now < deadline => {
                tick.next_due = min_due(tick.next_due, deadline.saturating_duration_since(now));
                true
            }
            CopyFeedbackFlash::Active { .. } => {
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
