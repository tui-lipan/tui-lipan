//! Brief visual feedback after a successful selection copy.
//!
//! A copy arms a flash on the widget it copied from. The flash knows how far through its
//! [`CopyFeedbackEffect`] it is, and renderers ask it for the style each selected cell should wear
//! this frame. When the flash ends it can also clear the selection it was drawn on, provided the
//! user has not changed that selection in the meantime.

use std::collections::HashMap;
use std::f32::consts::PI;
use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use crate::app::input::keymap::Keymap;
use crate::app::interaction_state::DirtyLevel;
use crate::clipboard::{ClipboardConfig, ClipboardService, CopyFeedback, CopyFeedbackEffect};
use crate::core::element::Key;
use crate::core::event::KeyEvent;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::{ColorTransform, Span, Style};
use crate::ui::capabilities::ClipboardContext;
use crate::ui::router::{self, ClipboardDispatchOutcome};
use crate::utils::GridSelection;
use crate::widgets::document_view::node::DocumentTableRectSelection;

/// How often an animated effect repaints. Smooth enough for a sweep, and the effect lasts well
/// under half a second, so the cost is a handful of paint-only frames.
const FRAME_INTERVAL: Duration = Duration::from_millis(16);

/// How far behind the head of a [`CopyFeedbackEffect::Scan`] its trail reaches, as a share of
/// the selection.
const SCAN_TRAIL: f32 = 0.35;

/// How many strengths an effect is rounded to. Neighbouring cells at the same strength then share
/// one style, so a sweep splits a row into a few runs instead of one per cell.
const INTENSITY_STEPS: f32 = 16.0;

#[derive(Clone)]
enum CopyFeedbackPhase {
    /// The selection has been copied and should render as active on the next
    /// paint. The wall-clock deadline is intentionally not armed until the
    /// frame after that first paint, so queued input cannot eat into the visible
    /// flash duration before users see it.
    Pending,
    Active {
        started: Instant,
        deadline: Instant,
    },
}

/// Whether a flash clears the selection it was drawn on when it ends.
#[derive(Clone)]
enum ClearOnEnd {
    Never,
    /// Not yet known what the selection was: the backend records it on its next pass.
    Unrecorded,
    Recorded(CopiedSelection),
}

#[derive(Clone)]
struct CopyFeedbackFlash {
    phase: CopyFeedbackPhase,
    feedback: CopyFeedback,
    /// The range to paint, for callers that copied a range they are no longer
    /// keeping selected. `None` flashes whatever the node currently has selected.
    #[cfg_attr(not(feature = "terminal"), allow(dead_code))]
    range: Option<CopyFeedbackRange>,
    clear: ClearOnEnd,
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
        let NodeKind::Terminal(node) = &tree.node(id).kind else {
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

/// What a widget had selected, compared before a flash clears it so a selection the user made
/// after the copy survives.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SelectionSnapshot {
    /// An input, text area, or hex area: its cursor and anchor.
    Text { cursor: usize, anchor: usize },
    /// A read-only input whose selection the runtime keeps on the app's behalf.
    ReadOnly { cursor: usize, anchor: usize },
    /// A document's text range and table rectangle. A table drag moves only the rectangle, so the
    /// rectangle itself is compared, not just whether there is one.
    Document {
        cursor: usize,
        anchor: Option<usize>,
        table: Option<DocumentTableRectSelection>,
    },
    #[cfg(feature = "terminal")]
    Terminal(crate::widgets::TerminalSelection),
}

impl SelectionSnapshot {
    fn document(
        cursor: usize,
        anchor: Option<usize>,
        table: Option<&DocumentTableRectSelection>,
    ) -> Option<Self> {
        let ranged = anchor.is_some_and(|anchor| anchor != cursor);
        (ranged || table.is_some()).then(|| Self::Document {
            cursor,
            anchor,
            table: table.cloned(),
        })
    }
}

/// Where a `DocumentView` sits under one of its `ScrollView` ancestors, in terms that outlive its
/// node.
///
/// A virtualized scroll view sweeps a child that scrolls out of view and stashes its documents'
/// selections under the child's key, in subtree order, until it scrolls back as a new node. Any
/// ancestor scroll view can do this, including one that sweeps a nested scroll view whole.
#[derive(Clone, Debug, PartialEq, Eq)]
struct ScrolledDocument {
    scroll_view: NodeId,
    child_key: Key,
    index: usize,
}

/// A copied selection a flash will clear when it ends.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CopiedSelection {
    id: NodeId,
    snapshot: SelectionSnapshot,
    /// Where the document sits under each `ScrollView` above it, nearest first.
    scrolled: Vec<ScrolledDocument>,
}

/// What `id` has selected right now, or `None` when it has no selection to clear.
pub(crate) fn selection_snapshot(
    tree: &NodeTree,
    read_only_selection: &HashMap<NodeId, (usize, Option<usize>)>,
    id: NodeId,
) -> Option<SelectionSnapshot> {
    if !tree.is_valid(id) {
        return None;
    }
    if let Some((cursor, Some(anchor))) = read_only_selection.get(&id).copied()
        && anchor != cursor
    {
        return Some(SelectionSnapshot::ReadOnly { cursor, anchor });
    }
    let text = |cursor: usize, anchor: Option<usize>| {
        anchor
            .filter(|anchor| *anchor != cursor)
            .map(|anchor| SelectionSnapshot::Text { cursor, anchor })
    };
    match &tree.node(id).kind {
        NodeKind::Input(node) => text(node.cursor, node.anchor),
        NodeKind::TextArea(node) => text(node.cursor, node.anchor),
        NodeKind::HexArea(node) => text(node.cursor, node.anchor),
        NodeKind::DocumentView(node) => SelectionSnapshot::document(
            node.selection_cursor,
            node.selection_anchor,
            node.table_rect_selection.as_ref(),
        ),
        #[cfg(feature = "terminal")]
        NodeKind::Terminal(node) => node
            .selection
            .filter(|selection| selection.anchor != selection.cursor)
            .map(SelectionSnapshot::Terminal),
        _ => None,
    }
}

/// Record what `id` has selected, and where to find it again if a scroll view sweeps its node.
pub(crate) fn copied_selection(
    tree: &NodeTree,
    read_only_selection: &HashMap<NodeId, (usize, Option<usize>)>,
    id: NodeId,
) -> Option<CopiedSelection> {
    let snapshot = selection_snapshot(tree, read_only_selection, id)?;
    let scrolled = if matches!(snapshot, SelectionSnapshot::Document { .. }) {
        scrolled_document(tree, id)
    } else {
        Vec::new()
    };
    Some(CopiedSelection {
        id,
        snapshot,
        scrolled,
    })
}

/// Where `id` sits under every `ScrollView` above it, nearest first. A level whose scroll-view
/// child has no key is left out: such a child is never stashed.
fn scrolled_document(tree: &NodeTree, id: NodeId) -> Vec<ScrolledDocument> {
    let mut found = Vec::new();
    let mut child = id;
    while let Some(parent) = tree.node(child).parent {
        if matches!(tree.node(parent).kind, NodeKind::ScrollView(_))
            && let Some(child_key) = tree.node(child).key.clone()
            && let Some(index) = documents_in_subtree(tree, child)
                .iter()
                .position(|doc| *doc == id)
        {
            found.push(ScrolledDocument {
                scroll_view: parent,
                child_key,
                index,
            });
        }
        child = parent;
    }
    found
}

/// The `DocumentView`s under `root`, in the order a scroll view stashes their selections.
fn documents_in_subtree(tree: &NodeTree, root: NodeId) -> Vec<NodeId> {
    fn walk(tree: &NodeTree, id: NodeId, out: &mut Vec<NodeId>) {
        if !tree.is_valid(id) {
            return;
        }
        if matches!(tree.node(id).kind, NodeKind::DocumentView(_)) {
            out.push(id);
            return;
        }
        for child in &tree.node(id).children {
            walk(tree, *child, out);
        }
    }
    let mut out = Vec::new();
    walk(tree, root, &mut out);
    out
}

/// Clear a copied selection, provided it is still what was copied. Returns whether anything
/// changed.
///
/// A document that a scroll view swept since the copy is found where it went: in a scroll view's
/// stash, or as the new node it was restored to. Each recorded scroll view that still exists is
/// asked in turn, nearest first, so a document swept with a whole nested scroll view is found in
/// the outer one.
pub(crate) fn clear_copied_selection(
    tree: &mut NodeTree,
    read_only_selection: &mut HashMap<NodeId, (usize, Option<usize>)>,
    copied: &CopiedSelection,
) -> bool {
    let expected = &copied.snapshot;
    let id = if tree.is_valid(copied.id) {
        copied.id
    } else {
        let mut remounted = None;
        for scrolled in &copied.scrolled {
            if !tree.is_valid(scrolled.scroll_view) {
                continue;
            }
            if let Some(id) = remounted_document(tree, scrolled) {
                remounted = Some(id);
                break;
            }
            if let Some(cleared) = clear_stashed_document(tree, scrolled, expected) {
                return cleared;
            }
        }
        let Some(id) = remounted else {
            return false;
        };
        id
    };
    if selection_snapshot(tree, read_only_selection, id).as_ref() != Some(expected) {
        return false;
    }
    if matches!(expected, SelectionSnapshot::ReadOnly { .. }) {
        return read_only_selection.remove(&id).is_some();
    }
    crate::app::selection_clear::clear_node_selection(tree, id)
}

/// The node a swept document was restored to, if its scroll-view child is mounted again.
fn remounted_document(tree: &NodeTree, scrolled: &ScrolledDocument) -> Option<NodeId> {
    if !tree.is_valid(scrolled.scroll_view) {
        return None;
    }
    let child = tree
        .node(scrolled.scroll_view)
        .children
        .iter()
        .copied()
        .find(|child| {
            tree.is_valid(*child) && tree.node(*child).key.as_ref() == Some(&scrolled.child_key)
        })?;
    documents_in_subtree(tree, child)
        .get(scrolled.index)
        .copied()
}

/// Clear a document selection a scroll view is holding for a child that is out of view.
///
/// `None` when this scroll view holds nothing for the document, so another one may; otherwise
/// whether the stashed selection was still the copied one and got cleared.
fn clear_stashed_document(
    tree: &mut NodeTree,
    scrolled: &ScrolledDocument,
    expected: &SelectionSnapshot,
) -> Option<bool> {
    let NodeKind::ScrollView(sv) = &mut tree.node_mut(scrolled.scroll_view).kind else {
        return None;
    };
    let doc = sv
        .offscreen_doc_selections
        .get_mut(&scrolled.child_key)?
        .docs
        .get_mut(scrolled.index)?;
    let stashed = SelectionSnapshot::document(
        doc.selection_cursor,
        doc.selection_anchor,
        doc.table_rect_selection.as_ref(),
    );
    if stashed.as_ref() != Some(expected) {
        return Some(false);
    }
    doc.selection_anchor = None;
    doc.table_rect_selection = None;
    Some(true)
}

/// The effect one flash shows on the frame being painted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FeedbackFrame {
    effect: CopyFeedbackEffect,
    /// How far through the effect, from `0.0` at the first paint to `1.0` at the end.
    progress: f32,
    style: Style,
}

impl FeedbackFrame {
    /// Whether cells need their own position to know their style.
    #[cfg_attr(not(feature = "terminal"), allow(dead_code))]
    pub(crate) fn is_positional(&self) -> bool {
        self.effect == CopyFeedbackEffect::Scan
    }

    /// How strongly to highlight a cell `position` of the way through the selection, in reading
    /// order. `None` means the widget cannot say, and positional effects fall back to a fade.
    pub(crate) fn intensity(&self, position: Option<f32>) -> f32 {
        let t = self.progress.clamp(0.0, 1.0);
        let fade = || {
            // Hold briefly at full strength so the copy registers, then ease out.
            const HOLD: f32 = 0.15;
            if t < HOLD {
                1.0
            } else {
                let rest = 1.0 - (t - HOLD) / (1.0 - HOLD);
                rest * rest
            }
        };
        match self.effect {
            CopyFeedbackEffect::Off => 0.0,
            CopyFeedbackEffect::Flash => 1.0,
            CopyFeedbackEffect::Fade => fade(),
            // Bright at the start, dark a third of the way, bright again at two thirds, and dark
            // when the time runs out, so the effect never ends on a jump.
            CopyFeedbackEffect::Pulse => (1.5 * PI * t).cos().powi(2),
            CopyFeedbackEffect::Scan => match position {
                None => fade(),
                Some(position) => {
                    let head = t * (1.0 + SCAN_TRAIL);
                    let behind = head - position.clamp(0.0, 1.0);
                    if (0.0..=SCAN_TRAIL).contains(&behind) {
                        1.0 - behind / SCAN_TRAIL
                    } else {
                        0.0
                    }
                }
            },
        }
    }

    /// The style to merge onto the selection for a cell at `position`, or `None` when the effect
    /// leaves it plain this frame.
    pub(crate) fn style_at(&self, position: Option<f32>) -> Option<Style> {
        let intensity = (self.intensity(position) * INTENSITY_STEPS).round() / INTENSITY_STEPS;
        (intensity > 0.0).then(|| scale_feedback_style(self.style, intensity))
    }
}

/// `style` at `intensity` of its strength.
///
/// Color transforms and tints scale smoothly. Absolute colors and text modifiers cannot be
/// partly applied, so they hold for the brighter half of the effect.
pub(crate) fn scale_feedback_style(style: Style, intensity: f32) -> Style {
    if intensity >= 1.0 {
        return style;
    }
    let intensity = intensity.max(0.0);
    let scale_transform = |transform: Option<ColorTransform>| {
        let transform = transform?;
        let Some(amount) = transform.amount().as_fixed() else {
            return (intensity >= 0.5).then_some(transform);
        };
        let scaled = match transform {
            ColorTransform::Opacity(_) | ColorTransform::OpacityToward { .. } => {
                1.0 - (1.0 - amount) * intensity
            }
            _ => amount * intensity,
        };
        Some(transform.with_amount(scaled))
    };
    let mut scaled = if intensity >= 0.5 {
        style
    } else {
        Style {
            contrast_policy: style.contrast_policy,
            ..Style::default()
        }
    };
    scaled.fg_transform = scale_transform(style.fg_transform);
    scaled.bg_transform = scale_transform(style.bg_transform);
    scaled.dim_amount = style.dim_amount.map(|amount| amount * intensity);
    scaled.tint = style.tint.map(|(color, alpha)| (color, alpha * intensity));
    scaled
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct CopyFeedbackTick {
    pub(crate) needs_paint: bool,
    pub(crate) next_due: Option<Duration>,
    /// Selections whose flash just ended and asked for them to be cleared.
    pub(crate) clear: Vec<CopiedSelection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CopyFeedbackDispatch {
    pub(crate) handled: bool,
    pub(crate) mutated: bool,
    pub(crate) dirty_override: Option<DirtyLevel>,
}

pub(crate) struct CopyFeedbackState {
    clock: crate::core::runtime_env::SessionClock,
    flashes: HashMap<NodeId, CopyFeedbackFlash>,
    /// When an animated flash next needs a repaint.
    next_frame: Option<Instant>,
}

impl CopyFeedbackState {
    pub(crate) fn new(clock: crate::core::runtime_env::SessionClock) -> Self {
        Self {
            clock,
            flashes: HashMap::new(),
            next_frame: None,
        }
    }

    /// Arm `feedback` on the selection `id` just copied.
    ///
    /// Nothing is armed when the feedback is invisible and leaves the selection alone.
    pub fn trigger(&mut self, id: NodeId, feedback: CopyFeedback) {
        if !feedback.is_visible() && !feedback.clear_selection {
            return;
        }
        let clear = if feedback.clear_selection {
            ClearOnEnd::Unrecorded
        } else {
            ClearOnEnd::Never
        };
        self.insert(id, feedback, None, clear);
    }

    /// Arm a flash that paints `range` regardless of what the node has selected.
    ///
    /// `None` flashes the live selection. A ranged flash never clears a selection: the caller
    /// copied a range of its own, and whatever the node still has selected is unrelated.
    pub fn trigger_range(
        &mut self,
        id: NodeId,
        feedback: CopyFeedback,
        range: Option<CopyFeedbackRange>,
    ) {
        if !feedback.is_visible() {
            return;
        }
        self.insert(id, feedback, range, ClearOnEnd::Never);
    }

    fn insert(
        &mut self,
        id: NodeId,
        feedback: CopyFeedback,
        range: Option<CopyFeedbackRange>,
        clear: ClearOnEnd,
    ) {
        self.flashes.insert(
            id,
            CopyFeedbackFlash {
                phase: CopyFeedbackPhase::Pending,
                feedback,
                range,
                clear,
            },
        );
    }

    /// Record what each newly copied selection is, so the flash can tell later whether the user
    /// has replaced it. Backends call this before [`Self::tick_at`], while the tree still holds
    /// the selection exactly as it was copied.
    pub(crate) fn record_selections(
        &mut self,
        mut snapshot: impl FnMut(NodeId) -> Option<CopiedSelection>,
    ) {
        for (id, flash) in &mut self.flashes {
            if matches!(flash.clear, ClearOnEnd::Unrecorded) {
                flash.clear = snapshot(*id).map_or(ClearOnEnd::Never, ClearOnEnd::Recorded);
            }
        }
    }

    pub fn is_active(&self, id: NodeId) -> bool {
        let now = self.clock.now();
        self.flashes.get(&id).is_some_and(|flash| {
            flash.feedback.is_visible()
                && match flash.phase {
                    CopyFeedbackPhase::Pending => true,
                    CopyFeedbackPhase::Active { deadline, .. } => now < deadline,
                }
        })
    }

    /// What the flash on `id` shows this frame, if it shows anything.
    pub(crate) fn frame(&self, id: NodeId) -> Option<FeedbackFrame> {
        if !self.is_active(id) {
            return None;
        }
        let flash = self.flashes.get(&id)?;
        let progress = match flash.phase {
            CopyFeedbackPhase::Pending => 0.0,
            CopyFeedbackPhase::Active { started, deadline } => {
                let total = deadline.saturating_duration_since(started).as_secs_f32();
                let elapsed = self
                    .clock
                    .now()
                    .saturating_duration_since(started)
                    .as_secs_f32();
                if total > 0.0 { elapsed / total } else { 1.0 }
            }
        };
        Some(FeedbackFrame {
            effect: flash.feedback.effect,
            progress,
            style: flash.feedback.style,
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

    pub fn tick_at(&mut self, now: Instant) -> CopyFeedbackTick {
        if self.flashes.is_empty() {
            self.next_frame = None;
            return CopyFeedbackTick::default();
        }
        let mut tick = CopyFeedbackTick::default();
        let mut animating = false;

        self.flashes.retain(|_, flash| match flash.phase {
            // Nothing to show: the flash only exists to clear the selection, which it can do now.
            CopyFeedbackPhase::Pending if !flash.feedback.is_visible() => {
                if let ClearOnEnd::Recorded(copied) = &flash.clear {
                    tick.clear.push(copied.clone());
                }
                false
            }
            CopyFeedbackPhase::Pending => {
                let duration = flash.feedback.duration;
                flash.phase = CopyFeedbackPhase::Active {
                    started: now,
                    deadline: now + duration,
                };
                animating |= flash.feedback.effect.is_animated() && !duration.is_zero();
                tick.next_due = min_due(tick.next_due, duration);
                true
            }
            CopyFeedbackPhase::Active { deadline, .. } if now < deadline => {
                animating |= flash.feedback.effect.is_animated();
                tick.next_due = min_due(tick.next_due, deadline.saturating_duration_since(now));
                true
            }
            CopyFeedbackPhase::Active { .. } => {
                tick.needs_paint |= flash.feedback.is_visible();
                if let ClearOnEnd::Recorded(copied) = &flash.clear {
                    tick.clear.push(copied.clone());
                }
                false
            }
        });

        if animating {
            match self.next_frame {
                Some(next_frame) if now < next_frame => {
                    tick.next_due =
                        min_due(tick.next_due, next_frame.saturating_duration_since(now));
                }
                _ => {
                    tick.needs_paint = true;
                    self.next_frame = Some(now + FRAME_INTERVAL);
                    tick.next_due = min_due(tick.next_due, FRAME_INTERVAL);
                }
            }
        } else {
            self.next_frame = None;
        }

        tick
    }
}

impl Default for CopyFeedbackState {
    fn default() -> Self {
        Self::new(crate::core::runtime_env::SessionClock::new(
            crate::automation::ClockMode::Realtime,
        ))
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
    if !outcome.copied {
        return false;
    }
    feedback.trigger(node_id, config.copy_feedback);
    config.copy_feedback.is_visible() || config.copy_feedback.clear_selection
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

pub(crate) fn dispatch_result_from_outcome(
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

    fn copied() -> ClipboardDispatchOutcome {
        ClipboardDispatchOutcome {
            handled: true,
            copied: true,
            mutated: false,
        }
    }

    fn frame(effect: CopyFeedbackEffect, progress: f32) -> FeedbackFrame {
        FeedbackFrame {
            effect,
            progress,
            style: CopyFeedback::default_style(),
        }
    }

    #[test]
    fn register_copy_feedback_tracks_active_node() {
        let mut feedback = CopyFeedbackState::default();
        let config = ClipboardConfig::default();
        let id = NodeId::new(1, 0);
        register_copy_feedback(&mut feedback, &config, id, copied());
        assert!(feedback.is_active(id));
    }

    #[test]
    fn off_feedback_registers_nothing_unless_it_clears() {
        let mut feedback = CopyFeedbackState::default();
        let mut config = ClipboardConfig {
            copy_feedback: CopyFeedback::off(),
            ..ClipboardConfig::default()
        };
        let id = NodeId::new(1, 0);
        assert!(!register_copy_feedback(
            &mut feedback,
            &config,
            id,
            copied()
        ));
        assert!(feedback.flashes.is_empty());

        config.copy_feedback = CopyFeedback::off().clear_selection(true);
        assert!(register_copy_feedback(&mut feedback, &config, id, copied()));
        assert!(!feedback.is_active(id), "an invisible flash paints nothing");
        assert_eq!(feedback.frame(id), None);
    }

    #[test]
    fn pending_feedback_does_not_expire_before_first_tick() {
        let mut feedback = CopyFeedbackState::default();
        let id = NodeId::new(1, 0);

        feedback.trigger(
            id,
            CopyFeedback::default().duration(Duration::from_millis(1)),
        );
        std::thread::sleep(Duration::from_millis(5));

        assert!(feedback.is_active(id));
        let tick = feedback.tick_at(Instant::now());
        assert!(!tick.needs_paint);
        assert!(tick.next_due.is_some());
        assert!(feedback.is_active(id));
    }

    #[test]
    fn animated_effects_request_frames_until_they_end() {
        let mut feedback = CopyFeedbackState::default();
        let id = NodeId::new(1, 0);
        feedback.trigger(id, CopyFeedback::new(CopyFeedbackEffect::Fade));
        let start = Instant::now();

        let first = feedback.tick_at(start);
        assert!(first.needs_paint);
        assert_eq!(first.next_due, Some(FRAME_INTERVAL));

        let early = feedback.tick_at(start + Duration::from_millis(4));
        assert!(
            !early.needs_paint,
            "no repaint before the next frame is due"
        );

        let next = feedback.tick_at(start + FRAME_INTERVAL);
        assert!(next.needs_paint);

        let end = feedback.tick_at(start + Duration::from_secs(1));
        assert!(end.needs_paint, "the final paint removes the highlight");
        assert!(feedback.flashes.is_empty());
        assert_eq!(
            feedback.tick_at(start + Duration::from_secs(2)).next_due,
            None
        );
    }

    #[test]
    fn a_plain_flash_only_wakes_at_its_deadline() {
        let mut feedback = CopyFeedbackState::default();
        let id = NodeId::new(1, 0);
        feedback.trigger(id, CopyFeedback::default());
        let tick = feedback.tick_at(Instant::now());
        assert!(!tick.needs_paint);
        assert_eq!(tick.next_due, Some(Duration::from_millis(150)));
    }

    fn text_selection(id: NodeId) -> CopiedSelection {
        CopiedSelection {
            id,
            snapshot: SelectionSnapshot::Text {
                cursor: 5,
                anchor: 0,
            },
            scrolled: Vec::new(),
        }
    }

    #[test]
    fn a_recorded_selection_is_handed_back_when_the_flash_ends() {
        let mut feedback = CopyFeedbackState::default();
        let id = NodeId::new(1, 0);
        let other = NodeId::new(2, 0);
        let copied = text_selection(id);
        let clearing = CopyFeedback::default().clear_selection(true);
        feedback.trigger(id, clearing);
        feedback.trigger(other, clearing);
        feedback.record_selections(|node| (node == id).then(|| copied.clone()));

        let start = Instant::now();
        assert!(feedback.tick_at(start).clear.is_empty());
        let end = feedback.tick_at(start + Duration::from_secs(1));
        assert_eq!(end.clear, vec![copied]);
    }

    #[test]
    fn ranged_flashes_never_clear() {
        let mut feedback = CopyFeedbackState::default();
        let id = NodeId::new(1, 0);
        feedback.trigger_range(id, CopyFeedback::default().clear_selection(true), None);
        feedback.record_selections(|node| Some(text_selection(node)));
        let start = Instant::now();
        feedback.tick_at(start);
        assert!(
            feedback
                .tick_at(start + Duration::from_secs(1))
                .clear
                .is_empty()
        );
    }

    #[test]
    fn effect_intensities_follow_their_shape() {
        assert_eq!(frame(CopyFeedbackEffect::Flash, 0.9).intensity(None), 1.0);
        assert_eq!(frame(CopyFeedbackEffect::Off, 0.0).intensity(None), 0.0);

        let fade = frame(CopyFeedbackEffect::Fade, 0.0);
        assert_eq!(fade.intensity(None), 1.0);
        assert!(frame(CopyFeedbackEffect::Fade, 0.6).intensity(None) < 0.5);
        assert!(frame(CopyFeedbackEffect::Fade, 1.0).intensity(None) < 1e-6);

        let pulse = |t| frame(CopyFeedbackEffect::Pulse, t).intensity(None);
        assert!(pulse(0.0) > 0.99);
        assert!(pulse(1.0 / 3.0) < 1e-3);
        assert!(pulse(2.0 / 3.0) > 0.99);
        assert!(pulse(1.0) < 1e-3);
    }

    #[test]
    fn scan_sweeps_in_reading_order() {
        let scan = frame(CopyFeedbackEffect::Scan, 0.4);
        let head = 0.4 * (1.0 + SCAN_TRAIL);
        assert_eq!(scan.intensity(Some(head + 0.05)), 0.0, "ahead of the head");
        assert!(scan.intensity(Some(head)) > 0.99, "at the head");
        assert!(
            scan.intensity(Some(head - SCAN_TRAIL / 2.0)) < 0.6,
            "in the trail"
        );
        assert_eq!(scan.intensity(Some(0.0)), 0.0, "behind the trail");
        assert!(
            frame(CopyFeedbackEffect::Scan, 1.0)
                .style_at(Some(1.0))
                .is_none()
        );
        assert!(scan.is_positional());
        assert_eq!(
            scan.intensity(None),
            frame(CopyFeedbackEffect::Fade, 0.4).intensity(None),
            "without positions a scan fades"
        );
    }

    #[test]
    fn scaling_a_style_weakens_its_transforms() {
        let full = CopyFeedback::default_style();
        assert_eq!(scale_feedback_style(full, 1.0), full);
        let half = scale_feedback_style(full, 0.5);
        let amount = half.bg_transform.unwrap().amount().as_fixed().unwrap();
        assert!((amount - 0.175).abs() < 1e-6);

        let solid = Style::new().bg(crate::style::Color::Red).bold();
        assert_eq!(scale_feedback_style(solid, 0.75), solid);
        assert_eq!(scale_feedback_style(solid, 0.25), Style::default());
    }
}
