//! Property-scoped transition registry.
//!
//! Components call [`crate::core::component::Context::transition`] to obtain an
//! interpolated value for a single style slot (color, scalar, ...). The
//! registry stores per-key transition state across frames, ticks active
//! transitions every animation frame, and drops entries that were not read
//! during a frame.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Duration;

use crate::animation::transition::{Lerp, Transition, TransitionConfig};
use crate::core::element::Key;
use crate::style::{Color, Paint};

trait DynEntry: Any {
    fn entry_type_id(&self) -> TypeId;
    fn tick(&mut self, dt: Duration) -> bool;
    fn is_animating(&self) -> bool;
    fn touched(&self) -> bool;
    fn reset_touched(&self);
    /// Whether this entry is only ever read while painting, so advancing it needs no `view()` pass.
    fn paint_resolved(&self) -> bool;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn as_any(&self) -> &dyn Any;
}

struct TypedEntry<T: Lerp + PartialEq + 'static> {
    current: T,
    target: T,
    transition: Option<Transition<T>>,
    touched: Cell<bool>,
    /// Set when the value is handed out as a late-bound [`Paint`](crate::style::Paint) rather than a
    /// concrete value. The view then cannot have baked the value into anything but a style, which is
    /// what makes advancing it a repaint instead of a rebuild.
    paint_resolved: Cell<bool>,
}

impl<T: Lerp + PartialEq + 'static> DynEntry for TypedEntry<T> {
    fn entry_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn tick(&mut self, dt: Duration) -> bool {
        let Some(transition) = self.transition.as_mut() else {
            return false;
        };
        transition.tick(dt);
        let new_current = transition.current();
        let changed = new_current != self.current;
        self.current = new_current;
        if transition.is_complete() {
            self.current = self.target.clone();
            self.transition = None;
        }
        changed
    }

    fn is_animating(&self) -> bool {
        self.transition.is_some()
    }

    fn touched(&self) -> bool {
        self.touched.get()
    }

    fn reset_touched(&self) {
        self.touched.set(false);
    }

    fn paint_resolved(&self) -> bool {
        self.paint_resolved.get()
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

thread_local! {
    /// The registry the current draw resolves late-bound paints against.
    ///
    /// Ambient rather than threaded through every renderer for the same reason the render-time
    /// terminal background is: a `Paint` can surface anywhere in any widget, and the alternative is
    /// a parameter on every style conversion in the backend.
    static RENDER_REGISTRY: RefCell<Option<std::rc::Rc<AnimationRegistry>>> =
        const { RefCell::new(None) };
}

/// RAII guard restoring the previously installed registry on drop.
pub(crate) struct RenderRegistryScope(Option<std::rc::Rc<AnimationRegistry>>);

impl Drop for RenderRegistryScope {
    fn drop(&mut self) {
        RENDER_REGISTRY.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

/// Make `registry` the one this draw resolves [`Paint::Animated`] against, until the guard drops.
pub(crate) fn set_render_registry(registry: std::rc::Rc<AnimationRegistry>) -> RenderRegistryScope {
    let prev = RENDER_REGISTRY.with(|slot| slot.borrow_mut().replace(registry));
    RenderRegistryScope(prev)
}

/// The colour a late-bound paint slot currently holds, if a registry is installed and still has it.
pub(crate) fn resolve_render_paint_slot(slot: u16) -> Option<Color> {
    RENDER_REGISTRY.with(|installed| {
        installed
            .borrow()
            .as_ref()
            .and_then(|registry| registry.resolve_paint_slot(slot))
    })
}

/// Registry of per-key property transitions.
///
/// Owned by [`crate::core::runtime_env::RuntimeEnv`] and shared across all
/// component contexts in a runtime.
#[derive(Default)]
pub(crate) struct AnimationRegistry {
    entries: RefCell<HashMap<Key, Box<dyn DynEntry>>>,
    /// Keys indexed by the slot id a late-bound [`Paint`](crate::style::Paint) carries. `Paint` must
    /// stay `Copy`, so it names its entry by slot rather than holding the key.
    color_slots: RefCell<Vec<Key>>,
    slot_by_key: RefCell<HashMap<Key, u16>>,
    generation: Cell<u64>,
}

/// What advancing the registry by one frame requires of the runtime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TransitionTick {
    /// A value some `view()` read as a concrete value changed, so the view must run again for it to
    /// reach the screen.
    pub(crate) view_changed: bool,
    /// A late-bound paint changed. The renderer resolves those itself, so a repaint is enough.
    pub(crate) paint_changed: bool,
}

impl AnimationRegistry {
    /// Read or update the transition entry keyed by `key`, returning the current
    /// interpolated value.
    ///
    /// Behavior:
    /// - First call for a key: stores `target` as the resting value, returns `target` unchanged.
    /// - Subsequent calls with the same `target`: returns the entry's current value
    ///   (interpolated by tick).
    /// - Subsequent calls with a different `target`: starts a transition from the
    ///   current value to the new target using `config`.
    /// - Zero-duration transitions snap immediately.
    ///
    /// # Panics
    /// Panics if `key` was previously used with a different value type — the
    /// registry stores a fixed type per key.
    pub(crate) fn transition<T: Lerp + PartialEq + 'static>(
        &self,
        key: Key,
        target: T,
        config: TransitionConfig,
    ) -> T {
        self.advance(key, target, config, false)
    }

    /// Like [`transition`](Self::transition), but hands back a [`Paint`] that names the entry instead
    /// of its current colour.
    ///
    /// The renderer resolves the slot while painting, so the element tree holds still for the whole
    /// fade and the runtime can answer each frame with a repaint. Because the caller never sees the
    /// interpolated colour, it cannot have used it for anything but a style — which is exactly the
    /// property that makes skipping `view()` sound.
    pub(crate) fn animated_paint(
        &self,
        key: Key,
        target: Color,
        config: TransitionConfig,
    ) -> Paint {
        let current = self.advance(key.clone(), target, config, true);
        match self.slot_for(key) {
            Some(slot) => Paint::Animated {
                slot,
                fallback: current,
            },
            None => Paint::Solid(current),
        }
    }

    /// The slot id naming `key`, minting one on first use.
    ///
    /// Slots are never reused for a different key, so a `Paint` handed out earlier can never resolve
    /// to an unrelated transition. Returns [`None`] once the id space is exhausted, which asks the
    /// caller to hand out a plain colour instead — the fade degrades to a snap rather than misbinding.
    fn slot_for(&self, key: Key) -> Option<u16> {
        if let Some(slot) = self.slot_by_key.borrow().get(&key) {
            return Some(*slot);
        }
        let mut slots = self.color_slots.borrow_mut();
        let slot = u16::try_from(slots.len()).ok()?;
        slots.push(key.clone());
        self.slot_by_key.borrow_mut().insert(key, slot);
        Some(slot)
    }

    /// The current colour behind a late-bound paint, or [`None`] if the slot no longer resolves.
    pub(crate) fn resolve_paint_slot(&self, slot: u16) -> Option<Color> {
        let key = self.color_slots.borrow().get(slot as usize)?.clone();
        let entries = self.entries.borrow();
        let entry = entries.get(&key)?;
        let typed = entry.as_any().downcast_ref::<TypedEntry<Color>>()?;
        Some(typed.current)
    }

    fn advance<T: Lerp + PartialEq + 'static>(
        &self,
        key: Key,
        target: T,
        config: TransitionConfig,
        paint_resolved: bool,
    ) -> T {
        let mut entries = self.entries.borrow_mut();
        let entry = entries.entry(key).or_insert_with(|| {
            Box::new(TypedEntry::<T> {
                current: target.clone(),
                target: target.clone(),
                transition: None,
                touched: Cell::new(true),
                paint_resolved: Cell::new(paint_resolved),
            })
        });

        if entry.entry_type_id() != TypeId::of::<T>() {
            panic!(
                "Ctx::transition called with a different value type for the same key (existing type id mismatch)"
            );
        }

        let typed: &mut TypedEntry<T> = entry
            .as_any_mut()
            .downcast_mut()
            .expect("type id checked above");

        typed.touched.set(true);
        // A key read as a concrete value even once must keep asking for view passes: some view has
        // baked that value into something the renderer cannot re-derive.
        if !paint_resolved {
            typed.paint_resolved.set(false);
        }

        if typed.target != target {
            let from = typed.current.clone();
            typed.target = target.clone();
            if config.duration.is_zero() {
                typed.current = target.clone();
                typed.transition = None;
            } else {
                typed.transition = Some(Transition::new(
                    from,
                    target.clone(),
                    config.duration,
                    config.easing,
                ));
            }
        }

        typed.current.clone()
    }

    /// Advance all in-flight transitions by `dt`, reporting what the change requires.
    ///
    /// Values a view read concretely need that view to run again; late-bound paints only need the
    /// screen redrawn. The memo generation is bumped only for the former, so a colour fade does not
    /// invalidate memoized subtrees that never depended on it.
    pub(crate) fn tick(&self, dt: Duration) -> TransitionTick {
        let mut entries = self.entries.borrow_mut();
        let mut result = TransitionTick::default();
        for entry in entries.values_mut() {
            if entry.tick(dt) {
                if entry.paint_resolved() {
                    result.paint_changed = true;
                } else {
                    result.view_changed = true;
                }
            }
        }
        if result.view_changed {
            self.generation
                .set(self.generation.get().wrapping_add(1).max(1));
        }
        result
    }

    /// Drop entries that were not read during the most recent view. Called once
    /// per frame after `Component::view` returns.
    pub(crate) fn end_frame_gc(&self) {
        let mut entries = self.entries.borrow_mut();
        let before = entries.len();
        entries.retain(|_, e| e.touched());
        // Slot ids stay assigned for the life of the runtime: `Paint` values already handed out
        // carry them, and a key that comes back must resolve to the same slot. Dropped entries make
        // `resolve_paint_slot` fall through to the paint's own fallback until they are read again.
        debug_assert!(
            self.color_slots.borrow().len() >= self.slot_by_key.borrow().len(),
            "slot table and reverse map must stay consistent"
        );
        if entries.len() != before {
            self.generation
                .set(self.generation.get().wrapping_add(1).max(1));
        }
        for e in entries.values() {
            e.reset_touched();
        }
    }

    /// Whether any transition currently has a non-zero remaining duration.
    pub(crate) fn has_active(&self) -> bool {
        self.entries.borrow().values().any(|e| e.is_animating())
    }

    /// Generation counter for memo invalidation. Bumped whenever an active
    /// transition advances or an entry is dropped.
    pub(crate) fn generation(&self) -> u64 {
        self.generation.get()
    }

    #[cfg(test)]
    pub(crate) fn entry_count(&self) -> usize {
        self.entries.borrow().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::easing::Easing;
    use crate::style::Color;

    fn cfg(ms: u64) -> TransitionConfig {
        TransitionConfig {
            duration: Duration::from_millis(ms),
            easing: Easing::Linear,
        }
    }

    #[test]
    fn first_call_returns_target_with_no_transition() {
        let reg = AnimationRegistry::default();
        let v = reg.transition::<Color>("k".into(), Color::Red, cfg(100));
        assert_eq!(v, Color::Red);
        assert!(!reg.has_active());
    }

    #[test]
    fn changing_target_starts_transition_and_ticks_toward_it() {
        let reg = AnimationRegistry::default();
        // Frame 1: anchor at red.
        let v0 = reg.transition::<Color>("k".into(), Color::Red, cfg(100));
        assert_eq!(v0, Color::Red);

        // Frame 2: target becomes blue. Should return current (red) and start a transition.
        let v1 = reg.transition::<Color>("k".into(), Color::Blue, cfg(100));
        assert_eq!(v1, Color::Red);
        assert!(reg.has_active());

        // Tick halfway. Value should change.
        let changed = reg.tick(Duration::from_millis(50));
        assert!(changed.view_changed);

        // Read again with same target — should return the interpolated current,
        // not red and not blue.
        let v2 = reg.transition::<Color>("k".into(), Color::Blue, cfg(100));
        assert!(v2 != Color::Red && v2 != Color::Blue);

        // Tick to completion.
        let _ = reg.tick(Duration::from_millis(60));
        assert!(!reg.has_active());
        let v3 = reg.transition::<Color>("k".into(), Color::Blue, cfg(100));
        assert_eq!(v3, Color::Blue);
    }

    #[test]
    fn zero_duration_snaps_immediately() {
        let reg = AnimationRegistry::default();
        let _ = reg.transition::<Color>("k".into(), Color::Red, cfg(0));
        let v = reg.transition::<Color>("k".into(), Color::Blue, cfg(0));
        // First call after target change still returns previous current, but the
        // transition completes the moment we tick.
        // For zero-duration, our implementation snaps `current = target` immediately:
        assert_eq!(v, Color::Blue);
        assert!(!reg.has_active());
    }

    #[test]
    fn end_frame_gc_drops_untouched_keys() {
        let reg = AnimationRegistry::default();
        let _ = reg.transition::<Color>("a".into(), Color::Red, cfg(100));
        let _ = reg.transition::<Color>("b".into(), Color::Red, cfg(100));
        assert_eq!(reg.entry_count(), 2);
        reg.end_frame_gc();
        // After GC, since end_frame_gc resets touched flags, the next gc would
        // drop everything. But within a frame both were touched, so both remain.
        assert_eq!(reg.entry_count(), 2);

        // Simulate a frame where only "a" was read.
        let _ = reg.transition::<Color>("a".into(), Color::Red, cfg(100));
        reg.end_frame_gc();
        assert_eq!(reg.entry_count(), 1);
    }

    #[test]
    fn tick_with_no_active_returns_false() {
        let reg = AnimationRegistry::default();
        let _ = reg.transition::<Color>("k".into(), Color::Red, cfg(100));
        assert_eq!(
            reg.tick(Duration::from_millis(16)),
            TransitionTick::default()
        );
    }

    #[test]
    fn f32_transitions_supported() {
        let reg = AnimationRegistry::default();
        let _ = reg.transition::<f32>("scalar".into(), 0.0, cfg(100));
        let _ = reg.transition::<f32>("scalar".into(), 1.0, cfg(100));
        let _ = reg.tick(Duration::from_millis(50));
        let v = reg.transition::<f32>("scalar".into(), 1.0, cfg(100));
        assert!((0.4..=0.6).contains(&v));
    }

    #[test]
    #[should_panic(expected = "different value type")]
    fn reusing_key_with_different_type_panics() {
        let reg = AnimationRegistry::default();
        let _ = reg.transition::<Color>("k".into(), Color::Red, cfg(100));
        let _ = reg.transition::<f32>("k".into(), 0.0, cfg(100));
    }
}
