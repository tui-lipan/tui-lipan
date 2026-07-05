#[cfg(feature = "terminal")]
use crate::app::input::drag::TerminalDrag;
use crate::app::input::drag::{
    ClickState, DocumentViewDrag, DragDropDrag, DraggableTabBarDrag, HexAreaDrag, InputDrag,
    ProgressDrag, SliderDrag, SplitterDrag, TextAreaDrag,
};
use crate::app::input::hex_history::HexHistory;
use crate::app::input::scrollbar::ScrollbarDrag;
use crate::app::input::text_area_vim::TextAreaVimState;
use crate::core::element::Key;
use crate::core::event::MouseButton;
use crate::core::node::NodeId;
use crate::layout::tag::Tag;
use crate::style::Rect;
use crate::text::editor::TextEditor;
use crate::text::input::TextInput;
use std::collections::HashMap;
use web_time::Instant;

/// Dirty level for the current frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum DirtyLevel {
    /// Nothing to re-render.
    #[default]
    None,
    /// Draw-only update (cursor blink, spinner frame, image frame).
    PaintOnly,
    /// Reconcile/layout update without broader side effects.
    LayoutOnly,
    /// Full update (component state/input side effects).
    Full,
}

impl DirtyLevel {
    #[inline]
    pub fn merge(&mut self, other: DirtyLevel) {
        match (*self, other) {
            (_, DirtyLevel::None) => {}
            (DirtyLevel::Full, _) => {}
            (_, DirtyLevel::Full) => *self = DirtyLevel::Full,
            (DirtyLevel::LayoutOnly, DirtyLevel::PaintOnly) => {}
            (_, DirtyLevel::LayoutOnly) => *self = DirtyLevel::LayoutOnly,
            (DirtyLevel::None, DirtyLevel::PaintOnly) => *self = DirtyLevel::PaintOnly,
            (DirtyLevel::PaintOnly, DirtyLevel::PaintOnly) => {}
        }
    }

    #[inline]
    pub fn set_full(&mut self) {
        *self = DirtyLevel::Full;
    }

    #[inline]
    pub fn set_layout(&mut self) {
        self.merge(DirtyLevel::LayoutOnly);
    }

    #[inline]
    pub fn set_paint(&mut self) {
        self.merge(DirtyLevel::PaintOnly);
    }

    #[inline]
    pub fn is_dirty(&self) -> bool {
        !matches!(self, DirtyLevel::None)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct DirtyTracker {
    level: DirtyLevel,
}

impl DirtyTracker {
    #[inline]
    pub fn mark_full(&mut self) {
        self.level.set_full();
    }

    #[inline]
    pub fn mark_layout(&mut self) {
        self.level.set_layout();
    }

    #[inline]
    pub fn mark_paint(&mut self) {
        self.level.set_paint();
    }

    #[inline]
    pub fn is_dirty(&self) -> bool {
        self.level.is_dirty()
    }

    #[inline]
    pub fn level(&self) -> DirtyLevel {
        self.level
    }
}

/// Active drag operation - only one can be active at a time.
///
/// This enum enforces the invariant that at most one drag operation is active,
/// making invalid states unrepresentable.
#[derive(Clone, Debug, Default)]
pub(crate) enum ActiveDrag {
    /// No drag operation in progress.
    #[default]
    None,
    /// Scrollbar thumb drag.
    Scrollbar(ScrollbarDrag),
    /// Progress bar drag.
    Progress(ProgressDrag),
    /// Slider drag.
    Slider(SliderDrag),
    /// Draggable tab bar drag.
    DraggableTabBar(DraggableTabBarDrag),
    /// Generic drag-and-drop source/target drag.
    DragDrop(DragDropDrag),
    /// Splitter handle drag.
    Splitter(SplitterDrag),
    /// TextArea text selection drag.
    TextArea(TextAreaDrag),
    /// Input text selection drag.
    Input(InputDrag),
    /// HexArea byte selection drag.
    HexArea(HexAreaDrag),
    /// Terminal text selection drag.
    #[cfg(feature = "terminal")]
    Terminal(TerminalDrag),
    /// DocumentView text selection drag.
    DocumentView(DocumentViewDrag),
}

impl ActiveDrag {
    /// Returns true if any drag operation is active.
    pub fn is_active(&self) -> bool {
        !matches!(self, Self::None)
    }

    /// Clear the active drag, returning to None state.
    pub fn clear(&mut self) {
        *self = Self::None;
    }
}

#[derive(Default)]
pub(crate) struct DragState {
    /// The currently active drag operation.
    pub active: ActiveDrag,
    /// Scrollbar-specific: whether recalculation is needed after resize.
    pub scrollbar_recalc: bool,
    /// Scrollbar-specific: cached rect for detecting resize during drag.
    pub scrollbar_rect: Option<Rect>,
    /// Set during an active drag when edge autoscroll changed a ScrollView
    /// offset and a layout reconcile is required.
    pub autoscroll_layout_dirty: bool,
    /// Last pointer position seen for the active drag.
    pub last_pointer_pos: Option<(u16, u16)>,
    /// Last time stationary drag autoscroll advanced.
    pub last_autoscroll_tick: Option<Instant>,
}

impl DragState {
    /// Check if any drag operation is active.
    pub fn is_active(&self) -> bool {
        self.active.is_active()
    }

    /// Clear all drag state.
    pub fn clear(&mut self) {
        self.active.clear();
        self.scrollbar_recalc = false;
        self.scrollbar_rect = None;
        self.autoscroll_layout_dirty = false;
        self.last_pointer_pos = None;
        self.last_autoscroll_tick = None;
    }

    pub fn remember_pointer(&mut self, x: u16, y: u16) {
        self.last_pointer_pos = Some((x, y));
        self.last_autoscroll_tick = Some(Instant::now());
    }
}

#[derive(Default)]
pub(crate) struct WidgetState {
    /// Internal cursor/anchor state for read_only widgets without on_change callback.
    pub read_only_selection: HashMap<NodeId, (usize, Option<usize>)>,
    pub input_history: HashMap<NodeId, TextInput>,
    pub textarea_history: HashMap<NodeId, TextEditor>,
    pub text_area_vim_state: HashMap<NodeId, TextAreaVimState>,
    pub hex_history: HashMap<NodeId, HexHistory>,
    pub hex_pending_edit: HashMap<NodeId, HexPendingEdit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct HexPendingEdit {
    pub index: usize,
    pub high_nibble: u8,
    pub before_byte: u8,
}

pub(crate) struct AnimationState {
    pub last_blink: Instant,
    pub blink_visible: bool,
    pub last_spinner_tick: Instant,
    pub spinner_frame: usize,
    pub last_animated_tick: Instant,
    pub last_effect_tick: Instant,
    #[cfg(feature = "image")]
    pub last_image_tick: Instant,
    #[cfg(feature = "image")]
    pub last_image_protocol_epoch: u64,
    #[cfg(feature = "image")]
    pub image_animation_suspend_until: Option<Instant>,
    #[cfg(feature = "image")]
    pub last_image_layout_hash: Option<u64>,
    pub last_overlay_tick: Instant,
    pub effect_phase_tick: u64,
    /// Whether image rendering was suspended during the previous cycle.
    /// Used to detect the suspension-expired transition and trigger a repaint.
    #[cfg(feature = "image")]
    pub image_rendering_was_suspended: bool,
}

impl AnimationState {
    /// Reset blink to visible state and restart the blink timer.
    pub fn reset_blink(&mut self) {
        self.blink_visible = true;
        self.last_blink = Instant::now();
    }
}

impl Default for AnimationState {
    fn default() -> Self {
        Self {
            last_blink: Instant::now(),
            blink_visible: true,
            last_spinner_tick: Instant::now(),
            spinner_frame: 0,
            last_animated_tick: Instant::now(),
            last_effect_tick: Instant::now(),
            #[cfg(feature = "image")]
            last_image_tick: Instant::now(),
            #[cfg(feature = "image")]
            last_image_protocol_epoch: 0,
            #[cfg(feature = "image")]
            image_animation_suspend_until: None,
            #[cfg(feature = "image")]
            last_image_layout_hash: None,
            last_overlay_tick: Instant::now(),
            effect_phase_tick: 0,
            #[cfg(feature = "image")]
            image_rendering_was_suspended: false,
        }
    }
}

#[derive(Default)]
pub(crate) struct MouseTrackingState {
    pub hovered: Option<NodeId>,
    /// For List/Table/Tabs: track which item is hovered to avoid re-renders on same item.
    pub hovered_item_index: Option<usize>,
    /// List/Table nodes where `item_hover_style` is suppressed until the mouse moves.
    pub suppress_pointer_item_hover_nodes: std::collections::HashSet<NodeId>,
    /// Nodes whose row selection changed from a click this frame (not keyboard/programmatic).
    pub pointer_driven_item_hover_selection: std::collections::HashSet<NodeId>,
    pub last_mouse: Option<(u16, u16)>,
    /// For double/triple-click detection.
    pub last_click: Option<ClickState>,
    /// Track the node where the left mouse button was pressed to differentiate dragging from clicking.
    pub left_down_node: Option<NodeId>,
    /// Track the (x, y) position of the left mouse button press to calculate drag threshold.
    pub left_down_pos: Option<(u16, u16)>,
    /// Whether the mouse has moved far enough from the press position to cancel a logical click.
    pub drag_threshold_exceeded: bool,
    /// Whether the logical click (MouseUp) should be suppressed because a widget handled it during Down.
    pub click_consumed: bool,
    /// Pending drag source candidate captured on left button down.
    pub pending_drag_source: Option<NodeId>,
    /// Pending or active `MouseRegion` drag callback target.
    pub mouse_region_drag: Option<MouseRegionDragState>,
    /// Pending or active `PanView` drag-to-pan target.
    pub pan_view_drag: Option<PanViewDragState>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MouseRegionDragState {
    pub node_id: NodeId,
    pub button: MouseButton,
    pub origin: (u16, u16),
    pub origin_local: (u16, u16),
    pub last_pos: (u16, u16),
    pub started: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PanViewDragState {
    pub node_id: NodeId,
    pub last_pos: (u16, u16),
    pub started: bool,
}

pub(crate) struct FocusState {
    pub focused: Option<NodeId>,
    pub focused_key: Option<Key>,
    pub focused_tag: Option<Tag>,
    pub focus_stack: Vec<Option<Key>>,
    pub window_focused: bool,
    #[cfg(feature = "terminal")]
    pub last_emitted_focus: Option<NodeId>,
    #[cfg(feature = "terminal")]
    pub last_emitted_window_focused: bool,
}

impl Default for FocusState {
    fn default() -> Self {
        Self {
            focused: None,
            focused_key: None,
            focused_tag: None,
            focus_stack: Vec::new(),
            window_focused: true,
            #[cfg(feature = "terminal")]
            last_emitted_focus: None,
            #[cfg(feature = "terminal")]
            last_emitted_window_focused: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ViewportMetrics {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL_LEVELS: [DirtyLevel; 4] = [
        DirtyLevel::None,
        DirtyLevel::PaintOnly,
        DirtyLevel::LayoutOnly,
        DirtyLevel::Full,
    ];

    /// Helper: apply merge non-mutably and return the result.
    fn merged(a: DirtyLevel, b: DirtyLevel) -> DirtyLevel {
        let mut out = a;
        out.merge(b);
        out
    }

    #[test]
    fn dirty_level_merge_identity_with_none() {
        for level in ALL_LEVELS {
            assert_eq!(merged(level, DirtyLevel::None), level);
        }
    }

    #[test]
    fn dirty_level_merge_commutativity() {
        for a in ALL_LEVELS {
            for b in ALL_LEVELS {
                assert_eq!(
                    merged(a, b),
                    merged(b, a),
                    "merge is not commutative for ({a:?}, {b:?})",
                );
            }
        }
    }

    #[test]
    fn dirty_level_merge_monotonicity() {
        /// Map each level to a numeric rank for comparison.
        fn rank(l: DirtyLevel) -> u8 {
            match l {
                DirtyLevel::None => 0,
                DirtyLevel::PaintOnly => 1,
                DirtyLevel::LayoutOnly => 2,
                DirtyLevel::Full => 3,
            }
        }

        for a in ALL_LEVELS {
            for b in ALL_LEVELS {
                let result = merged(a, b);
                assert!(
                    rank(result) >= rank(a) && rank(result) >= rank(b),
                    "merge({a:?}, {b:?}) = {result:?} violates monotonicity",
                );
            }
        }
    }

    #[test]
    fn dirty_level_merge_full_absorbs_everything() {
        for level in ALL_LEVELS {
            assert_eq!(merged(level, DirtyLevel::Full), DirtyLevel::Full);
            assert_eq!(merged(DirtyLevel::Full, level), DirtyLevel::Full);
        }
    }

    #[test]
    fn dirty_level_merge_cross_level_paint_and_layout() {
        // PaintOnly and LayoutOnly are both below Full but on different
        // "axes" - the lattice join should be LayoutOnly since LayoutOnly
        // implies a repaint anyway (LayoutOnly > PaintOnly in the ordering).
        assert_eq!(
            merged(DirtyLevel::PaintOnly, DirtyLevel::LayoutOnly),
            DirtyLevel::LayoutOnly,
        );
        assert_eq!(
            merged(DirtyLevel::LayoutOnly, DirtyLevel::PaintOnly),
            DirtyLevel::LayoutOnly,
        );
    }

    #[test]
    fn dirty_level_convenience_setters_and_is_dirty() {
        // Default is None, which is not dirty.
        let mut level = DirtyLevel::default();
        assert_eq!(level, DirtyLevel::None);
        assert!(!level.is_dirty());

        // set_paint escalates None → PaintOnly.
        level.set_paint();
        assert_eq!(level, DirtyLevel::PaintOnly);
        assert!(level.is_dirty());

        // set_layout escalates PaintOnly → LayoutOnly.
        level.set_layout();
        assert_eq!(level, DirtyLevel::LayoutOnly);

        // set_full always goes to Full.
        level.set_full();
        assert_eq!(level, DirtyLevel::Full);
    }

    #[test]
    fn dirty_tracker_accumulates_marks() {
        let mut tracker = DirtyTracker::default();
        assert!(!tracker.is_dirty());
        assert_eq!(tracker.level(), DirtyLevel::None);

        tracker.mark_paint();
        assert!(tracker.is_dirty());
        assert_eq!(tracker.level(), DirtyLevel::PaintOnly);

        // Marking layout should escalate.
        tracker.mark_layout();
        assert_eq!(tracker.level(), DirtyLevel::LayoutOnly);

        // Marking paint again should not downgrade.
        tracker.mark_paint();
        assert_eq!(tracker.level(), DirtyLevel::LayoutOnly);

        // Marking full should go to Full.
        tracker.mark_full();
        assert_eq!(tracker.level(), DirtyLevel::Full);
    }

    #[test]
    fn drag_state_default_inactive_and_clear_resets() {
        let mut state = DragState::default();
        assert!(!state.is_active());

        // Simulate some scrollbar-related bookkeeping without constructing
        // a full drag variant - just verify clear() resets auxiliary fields.
        state.scrollbar_recalc = true;
        state.scrollbar_rect = Some(Rect {
            x: 0,
            y: 0,
            w: 10,
            h: 10,
        });
        state.autoscroll_layout_dirty = true;
        state.last_pointer_pos = Some((3, 4));
        state.last_autoscroll_tick = Some(Instant::now());

        state.clear();
        assert!(!state.is_active());
        assert!(!state.scrollbar_recalc);
        assert!(state.scrollbar_rect.is_none());
        assert!(!state.autoscroll_layout_dirty);
        assert!(state.last_pointer_pos.is_none());
        assert!(state.last_autoscroll_tick.is_none());
    }
}
