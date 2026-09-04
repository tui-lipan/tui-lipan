//! Deciding whether a frame can be repainted from terminal damage alone.
//!
//! Planning is separated from drawing so eligibility can be asserted without rendering anything:
//! a test can hand this a tree and a refresh result and check *why* a case was rejected. `None`
//! is always the safe answer, and means the caller does an ordinary paint over the tree it has
//! already refreshed.

use crate::app::AppRunner;
use crate::core::component::Component;
use crate::core::node::{LiveTerminalRefresh, NodeId, NodeKind};
use crate::widgets::internal::TerminalDamage;

/// Why a frame could not be repainted from damage. Recorded rather than returned as a bare `None`
/// so the reason is testable and shows up in debug logs instead of being inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DamageRejection {
    /// No live terminal reported anything.
    NothingMoved,
    /// More than one terminal moved. Supporting several is possible and not yet worth it.
    SeveralTerminalsMoved,
    /// The terminal reported `Full` - a resize, a screen swap, decorations.
    FullDamage,
    /// The node backing the damage is gone or is no longer a terminal.
    NodeMissing,
    /// There is no retained frame to patch, or it does not match the current geometry.
    NoMatchingRetainedFrame,
    /// An overlay, devtools panel, drag preview or inline surface composites over the tree, so a
    /// terminal row is not the only thing that could occupy those cells.
    CompositeSurface,
    /// The terminal is showing scrollback rather than the live viewport.
    ScrolledBack,
    /// The terminal carries images, which are not painted from the cell grid.
    HasImages,
}

/// One terminal's damaged rows, resolved against the node that will paint them.
#[derive(Clone, Debug)]
pub(crate) struct TerminalDamagePlan {
    /// The terminal node to repaint.
    pub node: NodeId,
    /// Viewport rows that moved, ascending.
    pub rows: Vec<u16>,
}

impl<C> AppRunner<C>
where
    C: Component + 'static,
{
    /// Whether this frame can be repainted from terminal damage, and if not, why.
    ///
    /// Pure with respect to the runner: it reads the refresh result, the tree and the retained
    /// frame, and mutates nothing. Damage has already been consumed by the refresh that produced
    /// `refresh`, so rejecting here costs nothing but a full repaint.
    pub(crate) fn plan_terminal_damage(
        &self,
        refresh: &LiveTerminalRefresh,
    ) -> Result<TerminalDamagePlan, DamageRejection> {
        if self.surface.is_inline() || !self.core.tree.overlay_roots().is_empty() {
            return Err(DamageRejection::CompositeSurface);
        }
        #[cfg(feature = "devtools")]
        if self.devtools_state.borrow().visible {
            return Err(DamageRejection::CompositeSurface);
        }
        if !matches!(self.drag.active, crate::app::runner::ActiveDrag::None) {
            return Err(DamageRejection::CompositeSurface);
        }

        let mut moved = refresh.damage.iter();
        let (node, damage) = moved.next().ok_or(DamageRejection::NothingMoved)?;
        if moved.next().is_some() {
            return Err(DamageRejection::SeveralTerminalsMoved);
        }

        let rows = match damage {
            TerminalDamage::Rows(rows) => rows,
            TerminalDamage::Full => return Err(DamageRejection::FullDamage),
            TerminalDamage::None => return Err(DamageRejection::NothingMoved),
        };

        if !self.core.tree.is_valid(*node) {
            return Err(DamageRejection::NodeMissing);
        }
        let NodeKind::Terminal(terminal) = &self.core.tree.node(*node).kind else {
            return Err(DamageRejection::NodeMissing);
        };
        if terminal.scrollback_offset != 0 {
            return Err(DamageRejection::ScrolledBack);
        }
        #[cfg(feature = "terminal-images")]
        if !terminal.images.is_empty() {
            return Err(DamageRejection::HasImages);
        }

        let snapshot = self
            .last_frame_snapshot
            .as_ref()
            .ok_or(DamageRejection::NoMatchingRetainedFrame)?;
        let node_rect = self.core.tree.node(*node).rect;
        if node_rect.w == 0 || node_rect.h == 0 {
            return Err(DamageRejection::NoMatchingRetainedFrame);
        }
        let area = snapshot.area();
        if u32::from(area.width) * u32::from(area.height) == 0 {
            return Err(DamageRejection::NoMatchingRetainedFrame);
        }

        Ok(TerminalDamagePlan {
            node: *node,
            rows: rows.iter().map(|row| row.row).collect(),
        })
    }

    /// [`plan_terminal_damage`](Self::plan_terminal_damage), with the reason logged and discarded.
    pub(crate) fn prepare_terminal_damage_plan(
        &self,
        refresh: &LiveTerminalRefresh,
    ) -> Option<TerminalDamagePlan> {
        match self.plan_terminal_damage(refresh) {
            Ok(plan) => Some(plan),
            Err(reason) => {
                crate::debug::internal_log!("[tui-lipan] terminal damage fallback: {reason:?}");
                None
            }
        }
    }
}
