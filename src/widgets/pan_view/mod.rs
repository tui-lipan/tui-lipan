//! Two-dimensional pan viewport widget.

pub mod layout;
pub mod node;
pub mod reconcile;

pub(crate) use self::layout::measure_pan_view;
pub use self::node::PanViewNode;

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind, Key};
use crate::core::event::{KeyCode, KeyEvent};
use crate::style::Length;

/// Pan viewport metrics for a single axis pair.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PanMetrics {
    /// Child content width in cells.
    pub content_w: u16,
    /// Child content height in cells.
    pub content_h: u16,
    /// Viewport width in cells.
    pub viewport_w: u16,
    /// Viewport height in cells.
    pub viewport_h: u16,
    /// Maximum horizontal offset when clamped.
    pub max_x: i32,
    /// Maximum vertical offset when clamped.
    pub max_y: i32,
}

/// Event emitted after a `PanView` offset changes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PanEvent {
    /// New horizontal offset. May be negative when clamping is disabled.
    pub x: i32,
    /// New vertical offset. May be negative when clamping is disabled.
    pub y: i32,
    /// Current content/viewport bounds.
    pub metrics: PanMetrics,
}

/// Key bindings for `PanView` keyboard panning.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PanKeymap(u8);

impl PanKeymap {
    /// Disable key handling.
    pub const NONE: Self = Self(0);
    /// Arrow keys.
    pub const ARROWS: Self = Self(1 << 0);
    /// Vim-style h/j/k/l keys.
    pub const VIM: Self = Self(1 << 1);
    /// Default key set (arrows and vim keys).
    pub const DEFAULT: Self = Self(Self::ARROWS.0 | Self::VIM.0);

    /// Check if this keymap includes another set.
    pub fn contains(self, other: Self) -> bool {
        (self.0 & other.0) == other.0
    }
}

impl std::ops::BitOr for PanKeymap {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl std::ops::BitOrAssign for PanKeymap {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl std::ops::BitAnd for PanKeymap {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self {
        Self(self.0 & rhs.0)
    }
}

impl std::ops::BitAndAssign for PanKeymap {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl std::ops::Not for PanKeymap {
    type Output = Self;

    fn not(self) -> Self {
        Self(!self.0)
    }
}

impl Default for PanKeymap {
    fn default() -> Self {
        Self::DEFAULT
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PanAction {
    Delta(i16, i16),
}

fn signed_step(step: u16) -> i16 {
    step.min(i16::MAX as u16) as i16
}

pub(crate) fn pan_action_from_key(
    key: &KeyEvent,
    keymap: PanKeymap,
    key_step: (u16, u16),
) -> Option<PanAction> {
    let x = signed_step(key_step.0);
    let y = signed_step(key_step.1);
    match key.code {
        KeyCode::Left if keymap.contains(PanKeymap::ARROWS) => Some(PanAction::Delta(-x, 0)),
        KeyCode::Right if keymap.contains(PanKeymap::ARROWS) => Some(PanAction::Delta(x, 0)),
        KeyCode::Up if keymap.contains(PanKeymap::ARROWS) => Some(PanAction::Delta(0, -y)),
        KeyCode::Down if keymap.contains(PanKeymap::ARROWS) => Some(PanAction::Delta(0, y)),
        KeyCode::Char('h') if keymap.contains(PanKeymap::VIM) => Some(PanAction::Delta(-x, 0)),
        KeyCode::Char('l') if keymap.contains(PanKeymap::VIM) => Some(PanAction::Delta(x, 0)),
        KeyCode::Char('k') if keymap.contains(PanKeymap::VIM) => Some(PanAction::Delta(0, -y)),
        KeyCode::Char('j') if keymap.contains(PanKeymap::VIM) => Some(PanAction::Delta(0, y)),
        _ => None,
    }
}

pub(crate) fn pan_metrics(
    content_w: u16,
    content_h: u16,
    viewport_w: u16,
    viewport_h: u16,
) -> PanMetrics {
    PanMetrics {
        content_w,
        content_h,
        viewport_w,
        viewport_h,
        max_x: i32::from(content_w.saturating_sub(viewport_w)),
        max_y: i32::from(content_h.saturating_sub(viewport_h)),
    }
}

pub(crate) fn clamp_pan_offset((x, y): (i32, i32), metrics: PanMetrics, clamp: bool) -> (i32, i32) {
    if clamp {
        (x.clamp(0, metrics.max_x), y.clamp(0, metrics.max_y))
    } else {
        (x, y)
    }
}

fn free_axis_bounds(content: u16, viewport: u16, margin: u16) -> (i32, i32) {
    let margin = margin.min(content.max(1)).min(viewport.max(1));
    (
        -(i32::from(viewport) - i32::from(margin)),
        i32::from(content) - i32::from(margin),
    )
}

pub(crate) fn bound_pan_offset(
    (x, y): (i32, i32),
    metrics: PanMetrics,
    clamp: bool,
    free_pan_margin: Option<(u16, u16)>,
) -> (i32, i32) {
    if clamp {
        return clamp_pan_offset((x, y), metrics, true);
    }

    let Some((margin_x, margin_y)) = free_pan_margin else {
        return (x, y);
    };

    let (min_x, max_x) = free_axis_bounds(metrics.content_w, metrics.viewport_w, margin_x);
    let (min_y, max_y) = free_axis_bounds(metrics.content_h, metrics.viewport_h, margin_y);
    (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
}

pub(crate) fn apply_pan_delta(
    (x, y): (i32, i32),
    dx: i16,
    dy: i16,
    metrics: PanMetrics,
    clamp: bool,
    free_pan_margin: Option<(u16, u16)>,
) -> (i32, i32) {
    let next_x = x.saturating_add(i32::from(dx));
    let next_y = y.saturating_add(i32::from(dy));
    bound_pan_offset((next_x, next_y), metrics, clamp, free_pan_margin)
}

pub(crate) fn apply_pan_action(
    offset: (i32, i32),
    action: PanAction,
    metrics: PanMetrics,
    clamp: bool,
    free_pan_margin: Option<(u16, u16)>,
) -> (i32, i32) {
    match action {
        PanAction::Delta(dx, dy) => {
            apply_pan_delta(offset, dx, dy, metrics, clamp, free_pan_margin)
        }
    }
}

/// A single-child two-dimensional panning viewport.
#[derive(Clone)]
pub struct PanView {
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) offset: Option<(i32, i32)>,
    pub(crate) on_pan: Option<Callback<PanEvent>>,
    pub(crate) clamp: bool,
    pub(crate) center_content: bool,
    pub(crate) free_pan_margin: Option<(u16, u16)>,
    pub(crate) drag_to_pan: bool,
    pub(crate) keymap: PanKeymap,
    pub(crate) key_step: (u16, u16),
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) on_focus: Option<Callback<()>>,
    pub(crate) on_blur: Option<Callback<()>>,
    pub(crate) pan_state_key: Option<Key>,
    pub(crate) child: Option<Box<Element>>,
}

impl Default for PanView {
    fn default() -> Self {
        Self {
            width: Length::Flex(1),
            height: Length::Flex(1),
            offset: None,
            on_pan: None,
            clamp: true,
            center_content: false,
            free_pan_margin: None,
            drag_to_pan: true,
            keymap: PanKeymap::default(),
            key_step: (4, 2),
            focusable: false,
            tab_stop: true,
            on_focus: None,
            on_blur: None,
            pan_state_key: None,
            child: None,
        }
    }
}

impl PanView {
    /// Create an empty pan view.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the single child element.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = Some(Box::new(child.into()));
        self
    }

    /// Set requested viewport width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested viewport height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set a controlled viewport offset.
    pub fn offset(mut self, offset: (i32, i32)) -> Self {
        self.offset = Some(offset);
        self
    }

    /// Callback fired when input changes the viewport offset.
    pub fn on_pan(mut self, cb: Callback<PanEvent>) -> Self {
        self.on_pan = Some(cb);
        self
    }

    /// Enable or disable clamping to content bounds.
    pub fn clamp(mut self, clamp: bool) -> Self {
        self.clamp = clamp;
        self
    }

    /// Center the child in the viewport until input or a persisted offset takes over.
    pub fn center_content(mut self, center: bool) -> Self {
        self.center_content = center;
        self
    }

    /// Limit unclamped panning so at least `margin` cells of content remain reachable.
    ///
    /// This only applies when `.clamp(false)` is set. It gives free-canvas previews
    /// room to pull content around without allowing the child to drift infinitely far away.
    pub fn free_pan_margin(mut self, margin: u16) -> Self {
        self.free_pan_margin = Some((margin, margin));
        self
    }

    /// Set independent horizontal and vertical margins for unclamped panning limits.
    pub fn free_pan_margins(mut self, margins: (u16, u16)) -> Self {
        self.free_pan_margin = Some(margins);
        self
    }

    /// Enable or disable pointer-drag panning.
    pub fn drag_to_pan(mut self, enabled: bool) -> Self {
        self.drag_to_pan = enabled;
        self
    }

    /// Configure keys that move the viewport.
    pub fn keymap(mut self, keymap: PanKeymap) -> Self {
        self.keymap = keymap;
        if keymap != PanKeymap::NONE {
            self.focusable = true;
        }
        self
    }

    /// Alias matching `ScrollView::scroll_keys` style.
    pub fn pan_keys(self, keymap: PanKeymap) -> Self {
        self.keymap(keymap)
    }

    /// Set keyboard panning step as `(horizontal_cells, vertical_cells)`.
    ///
    /// The default is `(4, 2)` because terminal cells are taller than they are wide.
    pub fn key_step(mut self, step: (u16, u16)) -> Self {
        self.key_step = (step.0.max(1), step.1.max(1));
        self
    }

    /// Allow the pan view to receive focus.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Control whether the pan view participates in sequential focus navigation.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the pan view receives focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the pan view loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
        self
    }

    /// Set a stable key for persisting uncontrolled pan offset across remounts.
    pub fn pan_state_key(mut self, key: impl Into<Key>) -> Self {
        self.pan_state_key = Some(key.into());
        self
    }

    /// Convenience overload for callers that already have an `Arc<str>`.
    pub fn state_key(self, key: impl Into<Arc<str>>) -> Self {
        self.pan_state_key(Key::from(key.into()))
    }
}

impl From<PanView> for Element {
    fn from(value: PanView) -> Self {
        Element::new(ElementKind::PanView(value))
    }
}

impl crate::layout::hash::LayoutHash for PanView {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;

        self.width.hash(hasher);
        self.height.hash(hasher);
        self.clamp.hash(hasher);
        self.center_content.hash(hasher);
        self.free_pan_margin.hash(hasher);
        self.drag_to_pan.hash(hasher);
        self.keymap.hash(hasher);
        self.key_step.hash(hasher);
        self.focusable.hash(hasher);
        if let Some(child) = &self.child {
            recurse(child)?.hash(hasher);
        }
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::node::{NodeKind, NodeTree};
    use crate::layout::LayoutEngine;
    use crate::style::Rect;
    use crate::widgets::Text;

    fn wide_tall_child() -> Element {
        Text::new("0123456789\n0123456789\n0123456789").into()
    }

    fn small_child() -> Element {
        Text::new("tiny").into()
    }

    #[test]
    fn pan_view_clamps_offsets_to_content_bounds() {
        let metrics = pan_metrics(20, 10, 8, 4);

        assert_eq!(clamp_pan_offset((99, 99), metrics, true), (12, 6));
        assert_eq!(clamp_pan_offset((99, 99), metrics, false), (99, 99));
        assert_eq!(clamp_pan_offset((-4, -2), metrics, false), (-4, -2));
    }

    #[test]
    fn pan_view_drag_direction_decreases_offset_when_dragging_right_down() {
        let metrics = pan_metrics(20, 10, 8, 4);

        assert_eq!(apply_pan_delta((5, 3), -2, -1, metrics, true, None), (3, 2));
        assert_eq!(apply_pan_delta((5, 3), 2, 1, metrics, true, None), (7, 4));
        assert_eq!(
            apply_pan_delta((0, 0), -2, -1, metrics, false, None),
            (-2, -1)
        );
    }

    #[test]
    fn pan_view_free_pan_margin_bounds_unclamped_offsets() {
        let metrics = pan_metrics(20, 10, 8, 4);

        assert_eq!(
            bound_pan_offset((-99, -99), metrics, false, Some((1, 1))),
            (-7, -3)
        );
        assert_eq!(
            bound_pan_offset((99, 99), metrics, false, Some((1, 1))),
            (19, 9)
        );
    }

    #[test]
    fn pan_view_keymap_matches_arrows_and_vim() {
        let key = |code| KeyEvent {
            code,
            mods: crate::core::event::KeyMods::default(),
        };

        assert_eq!(
            pan_action_from_key(&key(KeyCode::Right), PanKeymap::ARROWS, (4, 2)),
            Some(PanAction::Delta(4, 0))
        );
        assert_eq!(
            pan_action_from_key(&key(KeyCode::Char('h')), PanKeymap::VIM, (4, 2)),
            Some(PanAction::Delta(-4, 0))
        );
        assert_eq!(
            pan_action_from_key(&key(KeyCode::Char('h')), PanKeymap::ARROWS, (4, 2)),
            None
        );
        assert_eq!(
            pan_action_from_key(&key(KeyCode::Char('u')), PanKeymap::VIM, (4, 2)),
            None
        );
    }

    #[test]
    fn pan_view_reconcile_clamps_and_offsets_child_rect() {
        let root: Element = PanView::new()
            .width(Length::Px(5))
            .height(Length::Px(2))
            .offset((99, 99))
            .child(wide_tall_child())
            .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 5,
                h: 2,
            },
            None,
        );

        let node = tree.node(tree.root);
        let NodeKind::PanView(pan) = &node.kind else {
            panic!("expected PanView root");
        };
        assert_eq!((pan.content_w, pan.content_h), (10, 3));
        assert_eq!((pan.viewport_w, pan.viewport_h), (5, 2));
        assert_eq!((pan.offset_x, pan.offset_y), (5, 1));

        let child = tree.node(node.children[0]);
        assert_eq!(child.rect.x, -5);
        assert_eq!(child.rect.y, -1);
        assert_eq!(child.rect.w, 10);
        assert_eq!(child.rect.h, 3);
    }

    #[test]
    fn pan_view_keyboard_updates_uncontrolled_offset_and_persists_key() {
        use std::cell::RefCell;
        use std::rc::Rc;

        let events = Rc::new(RefCell::new(Vec::new()));
        let events_cb = events.clone();
        let root: Element = PanView::new()
            .width(Length::Px(5))
            .height(Length::Px(2))
            .pan_state_key("pan-test")
            .on_pan(Callback::new(move |event| {
                events_cb.borrow_mut().push(event);
            }))
            .child(wide_tall_child())
            .into();
        let mut tree = NodeTree::new();
        let bounds = Rect {
            x: 0,
            y: 0,
            w: 5,
            h: 2,
        };
        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);

        let root_id = tree.root;
        let handled = crate::app::input::handlers::pan_view::handle_key(
            &mut tree,
            root_id,
            &KeyEvent {
                code: KeyCode::Right,
                mods: crate::core::event::KeyMods::default(),
            },
        );
        assert!(handled);
        let NodeKind::PanView(pan) = &tree.node(tree.root).kind else {
            panic!("expected PanView root");
        };
        assert_eq!((pan.offset_x, pan.offset_y), (4, 0));
        assert_eq!(events.borrow()[0].x, 4);

        LayoutEngine::reconcile_with_focus(&mut tree, &root, bounds, None);
        let NodeKind::PanView(pan) = &tree.node(tree.root).kind else {
            panic!("expected PanView root");
        };
        assert_eq!((pan.offset_x, pan.offset_y), (4, 0));
    }

    #[test]
    fn pan_view_can_center_smaller_content_with_negative_offset() {
        let root: Element = PanView::new()
            .width(Length::Px(20))
            .height(Length::Px(10))
            .clamp(false)
            .center_content(true)
            .child(small_child())
            .into();
        let mut tree = NodeTree::new();
        LayoutEngine::reconcile_with_focus(
            &mut tree,
            &root,
            Rect {
                x: 0,
                y: 0,
                w: 20,
                h: 10,
            },
            None,
        );

        let node = tree.node(tree.root);
        let NodeKind::PanView(pan) = &node.kind else {
            panic!("expected PanView root");
        };
        assert_eq!((pan.offset_x, pan.offset_y), (-8, -4));
        let child = tree.node(node.children[0]);
        assert_eq!(child.rect.x, 8);
        assert_eq!(child.rect.y, 4);
    }
}
