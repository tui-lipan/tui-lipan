use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

use crate::animation::{Easing, Transition};
use crate::callback::Callback;
use crate::core::element::Element;
use crate::core::node::{NodeId, WidgetNode};
use crate::style::{Length, Padding, Style};
use crate::widgets::{Toast, ToastCopyAffordance};

/// Controls whether overlay-like widget content renders at the root or inline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OverlayScope {
    /// Render at root level using the overlay pipeline.
    #[default]
    RootPortal,
    /// Render inline at the declaration location inside the normal tree.
    Local,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// Unique identifier for an overlay entry.
pub struct OverlayId(u64);

impl OverlayId {
    pub(crate) fn value(self) -> u64 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OverlayLayer {
    Modal = 0,
    Popover = 1,
    Toast = 2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) enum PointerCapture {
    #[default]
    None,
    RectOnly,
    BackdropFullScreen,
}

/// Toast positioning on screen.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToastPlacement {
    /// Top-left corner.
    TopStart,
    /// Top-center.
    TopCenter,
    /// Top-right corner.
    TopEnd,
    /// Bottom-left corner.
    BottomStart,
    /// Bottom-center.
    BottomCenter,
    /// Bottom-right corner (default).
    #[default]
    BottomEnd,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DismissPolicy {
    None,
    #[default]
    ClickOutside,
    ClickInside,
    ClickOutsideOrEscape,
}

impl DismissPolicy {
    pub(crate) fn dismiss_on_click_inside(self) -> bool {
        matches!(self, Self::ClickInside)
    }

    pub(crate) fn dismiss_on_click_outside(self) -> bool {
        matches!(self, Self::ClickOutside | Self::ClickOutsideOrEscape)
    }

    pub(crate) fn dismiss_on_escape(self) -> bool {
        matches!(self, Self::ClickOutsideOrEscape)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OverlayPlacement {
    Center {
        /// Height to reserve when centering vertically, instead of the content's own height.
        /// The content is top-aligned within that reserved band, so its top edge stays fixed
        /// as it grows and shrinks (e.g. a filtering command palette). Content taller than the
        /// band keeps the same top edge and extends past the band's bottom; `max_height` is
        /// what bounds it. Without this, the overlay centers by its actual height and a
        /// shrinking modal drifts toward the middle.
        reserve_height: Option<Length>,
    },
    Stacked {
        placement: ToastPlacement,
        gap: u16,
        margin: Padding,
    },
}

#[derive(Clone)]
pub(crate) struct Portal {
    pub(crate) layer: OverlayLayer,
    pub(crate) content: Box<Element>,
    pub(crate) placement: OverlayPlacement,
    pub(crate) dismiss_policy: DismissPolicy,
    pub(crate) on_close: Option<Callback<()>>,
    pub(crate) backdrop: Option<Style>,
    pub(crate) captures_focus: bool,
    pub(crate) captures_pointer: PointerCapture,
}

#[derive(Clone)]
pub(crate) struct PortalNode {
    pub(crate) content: Box<NodeId>,
}

impl WidgetNode for PortalNode {}

impl crate::layout::hash::LayoutHash for Portal {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.layer.hash(hasher);
        self.captures_focus.hash(hasher);
        self.captures_pointer.hash(hasher);
        recurse(self.content.as_ref())?.hash(hasher);
        Some(())
    }
}

#[derive(Clone)]
pub(crate) struct OverlayEntry {
    pub(crate) id: OverlayId,
    pub(crate) order: u64,
    pub(crate) layer: OverlayLayer,
    pub(crate) content: Element,
    pub(crate) placement: OverlayPlacement,
    pub(crate) dismiss_policy: DismissPolicy,
    pub(crate) on_dismiss: Option<Callback<()>>,
    pub(crate) created_at: Instant,
    pub(crate) timeout: Option<Duration>,
    pub(crate) captures_focus: bool,
    pub(crate) backdrop: Option<Style>,
    pub(crate) captures_pointer: PointerCapture,
    pub(crate) opacity_transition: Option<Transition<f32>>,
    pub(crate) pending_dismiss: bool,
    pub(crate) copy_text: Option<Arc<str>>,
    pub(crate) copy_zone_right_padding: Option<u16>,
    pub(crate) copy_feedback_until: Option<Instant>,
}

impl OverlayEntry {
    pub(crate) fn opacity(&self) -> f32 {
        self.opacity_transition
            .as_ref()
            .map(Transition::current)
            .unwrap_or(if self.pending_dismiss { 0.0 } else { 1.0 })
            .clamp(0.0, 1.0)
    }

    pub(crate) fn copy_feedback_active(&self) -> bool {
        self.copy_feedback_until
            .is_some_and(|deadline| Instant::now() < deadline)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TickResult {
    pub(crate) dirty: bool,
    pub(crate) has_active_transitions: bool,
}

pub(crate) struct OverlayManager {
    entries: Vec<OverlayEntry>,
    next_id: u64,
    generation: u64,
    inline_mode: bool,
    toast_placement: ToastPlacement,
    toast_gap: u16,
    toast_margin: Padding,
    last_tick: Instant,
}

impl OverlayManager {
    pub(crate) fn new() -> Self {
        Self {
            entries: Vec::new(),
            next_id: 0,
            generation: 0,
            inline_mode: false,
            toast_placement: ToastPlacement::BottomEnd,
            toast_gap: 1,
            toast_margin: Padding::BORDER,
            last_tick: Instant::now(),
        }
    }

    fn enter_transition() -> Transition<f32> {
        Transition::new(0.0, 1.0, Duration::from_millis(150), Easing::EaseOutQuad)
    }

    fn exit_transition(from: f32) -> Transition<f32> {
        let mut transition = Transition::new(
            from.clamp(0.0, 1.0),
            0.0,
            Duration::from_millis(100),
            Easing::EaseInQuad,
        );
        transition.is_exit = true;
        transition
    }

    fn begin_dismiss(entry: &mut OverlayEntry) -> bool {
        if entry.pending_dismiss {
            return false;
        }
        entry.pending_dismiss = true;
        entry.opacity_transition = Some(Self::exit_transition(entry.opacity()));
        true
    }

    /// Monotonically increasing counter bumped on every mutation to `entries`.
    /// Callers can cache a clone of the entries and skip re-cloning while the
    /// generation stays the same.
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    fn bump_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn allocate_id(&mut self) -> OverlayId {
        let id = OverlayId(self.next_id);
        self.next_id = self.next_id.wrapping_add(1);
        id
    }

    pub(crate) fn set_inline_mode(&mut self, inline_mode: bool) {
        self.inline_mode = inline_mode;
        if inline_mode && !self.entries.is_empty() {
            self.entries.clear();
            self.bump_generation();
        }
    }

    pub(crate) fn entries(&self) -> &[OverlayEntry] {
        &self.entries
    }

    pub(crate) fn has_active_transitions(&self) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.opacity_transition.is_some())
    }

    pub(crate) fn push(&mut self, mut entry: OverlayEntry) -> OverlayId {
        let id = self.allocate_id();
        if self.inline_mode {
            return id;
        }
        entry.id = id;
        entry.order = id.value();
        entry.created_at = Instant::now();
        entry.pending_dismiss = false;
        entry.opacity_transition = Some(Self::enter_transition());
        self.entries.push(entry);
        self.bump_generation();
        id
    }

    pub(crate) fn dismiss(&mut self, id: OverlayId) -> bool {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            let changed = Self::begin_dismiss(entry);
            if changed {
                self.bump_generation();
            }
            return true;
        }
        false
    }

    pub(crate) fn dismiss_immediately(&mut self, id: OverlayId) -> bool {
        let Some(index) = self.entries.iter().position(|entry| entry.id == id) else {
            return false;
        };
        let mut entry = self.entries.remove(index);
        if let Some(callback) = entry.on_dismiss.take() {
            callback.emit(());
        }
        self.bump_generation();
        true
    }

    pub(crate) fn tick(&mut self) -> TickResult {
        let now = Instant::now();
        let delta = now.saturating_duration_since(self.last_tick);
        self.last_tick = now;

        let mut result = TickResult::default();

        self.entries.retain_mut(|entry| {
            if !entry.pending_dismiss {
                let expired = entry
                    .timeout
                    .map(|timeout| now.duration_since(entry.created_at) >= timeout)
                    .unwrap_or(false);
                if expired {
                    result.dirty |= Self::begin_dismiss(entry);
                }
            }

            if let Some(deadline) = entry.copy_feedback_until {
                if now >= deadline {
                    entry.copy_feedback_until = None;
                    result.dirty = true;
                } else {
                    result.has_active_transitions = true;
                }
            }

            if let Some(transition) = entry.opacity_transition.as_mut() {
                let before = transition.current();
                let complete = transition.tick(delta);
                let after = transition.current();
                if (after - before).abs() > f32::EPSILON {
                    result.dirty = true;
                }

                if complete {
                    if entry.pending_dismiss {
                        if let Some(cb) = entry.on_dismiss.take() {
                            cb.emit(());
                        }
                        result.dirty = true;
                        return false;
                    }
                    entry.opacity_transition = None;
                } else {
                    result.has_active_transitions = true;
                }
            }

            true
        });

        if self.has_active_transitions() {
            result.has_active_transitions = true;
        }

        if result.dirty {
            self.bump_generation();
        }
        result
    }

    pub(crate) fn trigger_copy_feedback(&mut self, id: OverlayId, duration: Duration) -> bool {
        if duration.is_zero() {
            return false;
        }
        let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) else {
            return false;
        };
        if entry.pending_dismiss || entry.copy_text.is_none() {
            return false;
        }
        entry.copy_feedback_until = Some(Instant::now() + duration);
        self.bump_generation();
        true
    }

    pub(crate) fn set_toast_placement(&mut self, placement: ToastPlacement) {
        self.toast_placement = placement;
    }

    pub(crate) fn set_toast_gap(&mut self, gap: u16) {
        self.toast_gap = gap;
    }

    pub(crate) fn set_toast_margin(&mut self, margin: Padding) {
        self.toast_margin = margin;
    }

    pub(crate) fn toast_config(&self) -> (ToastPlacement, u16, Padding) {
        (self.toast_placement, self.toast_gap, self.toast_margin)
    }

    pub(crate) fn dismiss_toasts(&mut self) {
        let mut changed = false;
        for entry in &mut self.entries {
            if entry.layer == OverlayLayer::Toast {
                changed |= Self::begin_dismiss(entry);
            }
        }
        if changed {
            self.bump_generation();
        }
    }

    pub(crate) fn push_toast(&mut self, toast: Toast) -> OverlayId {
        if self.inline_mode {
            crate::debug::internal_log!("[tui-lipan] inline mode: toast suppressed");
            return self.allocate_id();
        }

        let (placement, gap, margin) = self.toast_config();
        let duration = toast.duration;
        let copy_text = toast.copyable.then(|| toast.message.clone());
        let copy_zone_right_padding = if toast.copyable
            && toast.border
            && matches!(toast.copy_affordance, ToastCopyAffordance::BorderGlyph)
        {
            Some(toast.header_padding.right)
        } else {
            None
        };
        let dismiss_policy = if toast.dismiss_on_click {
            DismissPolicy::ClickInside
        } else {
            DismissPolicy::None
        };
        let content = toast.into_element();
        let entry = OverlayEntry {
            id: OverlayId(0),
            order: 0,
            layer: OverlayLayer::Toast,
            content,
            placement: OverlayPlacement::Stacked {
                placement,
                gap,
                margin,
            },
            dismiss_policy,
            on_dismiss: None,
            created_at: Instant::now(),
            timeout: Some(Duration::from_secs_f64(duration)),
            captures_focus: false,
            backdrop: None,
            captures_pointer: PointerCapture::None,
            opacity_transition: None,
            pending_dismiss: false,
            copy_text,
            copy_zone_right_padding,
            copy_feedback_until: None,
        };
        self.push(entry)
    }
}

/// Handle for showing toast notifications via `ctx.toast()`.
#[derive(Clone)]
pub struct ToastHandle {
    manager: Rc<RefCell<OverlayManager>>,
}

impl ToastHandle {
    pub(crate) fn new(manager: Rc<RefCell<OverlayManager>>) -> Self {
        Self { manager }
    }

    /// Push a toast and return its ID.
    pub fn push(&self, toast: Toast) -> OverlayId {
        self.manager.borrow_mut().push_toast(toast)
    }

    /// Dismiss a specific toast by ID.
    pub fn dismiss(&self, id: OverlayId) {
        let _ = self.manager.borrow_mut().dismiss(id);
    }

    /// Dismiss a specific toast synchronously, without its exit transition.
    ///
    /// Use this when replacing an existing toast in place would otherwise leave the fading toast
    /// visible beside its replacement.
    pub fn dismiss_immediately(&self, id: OverlayId) {
        let _ = self.manager.borrow_mut().dismiss_immediately(id);
    }

    /// Clear all active toasts.
    pub fn clear(&self) {
        self.manager.borrow_mut().dismiss_toasts();
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use super::*;

    #[test]
    fn toast_copy_feedback_marks_entry_active() {
        let mut manager = OverlayManager::new();
        let id = manager.push_toast(Toast::new("copy me").copyable(true));

        assert!(manager.trigger_copy_feedback(id, Duration::from_millis(150)));
        assert!(manager.entries[0].copy_feedback_active());
    }

    #[test]
    fn toast_copy_feedback_tick_clears_expired_flash() {
        let mut manager = OverlayManager::new();
        let id = manager.push_toast(Toast::new("copy me").copyable(true));

        assert!(manager.trigger_copy_feedback(id, Duration::from_millis(1)));
        thread::sleep(Duration::from_millis(5));
        let tick = manager.tick();

        assert!(tick.dirty);
        assert!(!manager.entries[0].copy_feedback_active());
        assert!(manager.entries[0].copy_feedback_until.is_none());
    }

    #[test]
    fn immediate_dismiss_removes_toast_without_an_exit_transition() {
        let mut manager = OverlayManager::new();
        let first = manager.push_toast(Toast::new("first"));
        let second = manager.push_toast(Toast::new("second"));

        assert!(manager.dismiss_immediately(first));
        assert_eq!(manager.entries.len(), 1);
        assert_eq!(manager.entries[0].id, second);
        assert!(!manager.entries[0].pending_dismiss);
    }
}
