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

trait DynEntry: Any {
    fn entry_type_id(&self) -> TypeId;
    fn tick(&mut self, dt: Duration) -> bool;
    fn is_animating(&self) -> bool;
    fn touched(&self) -> bool;
    fn reset_touched(&self);
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

struct TypedEntry<T: Lerp + PartialEq + 'static> {
    current: T,
    target: T,
    transition: Option<Transition<T>>,
    touched: Cell<bool>,
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

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Registry of per-key property transitions.
///
/// Owned by [`crate::core::runtime_env::RuntimeEnv`] and shared across all
/// component contexts in a runtime.
#[derive(Default)]
pub(crate) struct AnimationRegistry {
    entries: RefCell<HashMap<Key, Box<dyn DynEntry>>>,
    generation: Cell<u64>,
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
        let mut entries = self.entries.borrow_mut();
        let entry = entries.entry(key).or_insert_with(|| {
            Box::new(TypedEntry::<T> {
                current: target.clone(),
                target: target.clone(),
                transition: None,
                touched: Cell::new(true),
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

    /// Advance all in-flight transitions by `dt`. Returns `true` if any
    /// interpolated value changed (callers should mark a full re-render so the
    /// view function re-reads the new value).
    pub(crate) fn tick(&self, dt: Duration) -> bool {
        let mut entries = self.entries.borrow_mut();
        let mut any_changed = false;
        for entry in entries.values_mut() {
            if entry.tick(dt) {
                any_changed = true;
            }
        }
        if any_changed {
            self.generation
                .set(self.generation.get().wrapping_add(1).max(1));
        }
        any_changed
    }

    /// Drop entries that were not read during the most recent view. Called once
    /// per frame after `Component::view` returns.
    pub(crate) fn end_frame_gc(&self) {
        let mut entries = self.entries.borrow_mut();
        let before = entries.len();
        entries.retain(|_, e| e.touched());
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
        assert!(changed);

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
        assert!(!reg.tick(Duration::from_millis(16)));
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
