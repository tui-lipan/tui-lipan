//! Property-scoped transition registry.
//!
//! Components call [`crate::core::component::Context::transition`] to obtain an
//! interpolated value for a single style slot (color, scalar, ...). The
//! registry stores per-key transition state across frames, ticks active
//! transitions every animation frame, and drops entries that were not read
//! during a frame.
//!
//! A transition is either *view-resolved* or *render-resolved*. `transition()` hands the current
//! value to `view()`, which may have used it for anything, so advancing it needs a `view()` pass.
//! `animated_color()` and `animated_amount()` hand out a late-bound value instead - a
//! [`Paint::Animated`] or an [`EffectAmount`] naming a registry slot - which the renderer
//! resolves while painting. The value never escapes into `view()`, so advancing it is a repaint.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Duration;

use crate::animation::transition::{Lerp, Transition, TransitionConfig};
use crate::core::element::Key;
use crate::style::{Color, EffectAmount, Paint};

trait DynEntry: Any {
    fn entry_type_id(&self) -> TypeId;
    fn tick(&mut self, dt: Duration) -> bool;
    fn is_animating(&self) -> bool;
    fn touched(&self) -> bool;
    fn reset_touched(&self);
    /// Whether this entry is only ever read by the renderer, so advancing it needs no `view()` pass.
    fn render_resolved(&self) -> bool;
    /// Optional caller-selected cadence for a render-resolved transition.
    fn render_interval(&self) -> Option<Duration>;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn as_any(&self) -> &dyn Any;
}

struct TypedEntry<T: Lerp + PartialEq + 'static> {
    current: T,
    target: T,
    transition: Option<Transition<T>>,
    touched: Cell<bool>,
    /// Set when the value is handed out late-bound - as a slot the renderer resolves - rather than
    /// as a concrete value. The view then cannot have baked the value into anything but a render
    /// input, which is what makes advancing it a repaint instead of a rebuild.
    render_resolved: Cell<bool>,
    render_interval: Cell<Option<Duration>>,
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

    fn render_resolved(&self) -> bool {
        self.render_resolved.get()
    }

    fn render_interval(&self) -> Option<Duration> {
        self.render_interval.get()
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

/// What the current draw resolves late-bound values against.
struct RenderScope {
    registry: std::rc::Rc<AnimationRegistry>,
    /// The runtime clock when the draw began. Renderer-owned animations such as
    /// [`EffectPulse`](crate::style::EffectPulse) are evaluated from it.
    elapsed: Duration,
}

thread_local! {
    /// The registry and clock the current draw resolves late-bound values against.
    ///
    /// Ambient rather than threaded through every renderer for the same reason the render-time
    /// terminal background is: a late-bound paint or amount can surface anywhere in any widget,
    /// and the alternative is a parameter on every style conversion in the backend.
    static RENDER_SCOPE: RefCell<Option<RenderScope>> = const { RefCell::new(None) };
}

/// RAII guard restoring the previously installed render scope on drop.
pub(crate) struct RenderRegistryScope(Option<RenderScope>);

impl Drop for RenderRegistryScope {
    fn drop(&mut self) {
        RENDER_SCOPE.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}

/// Make `registry` the one this draw resolves late-bound values against, and `elapsed` the
/// runtime clock reading it paints at, until the guard drops.
pub(crate) fn set_render_registry(
    registry: std::rc::Rc<AnimationRegistry>,
    elapsed: Duration,
) -> RenderRegistryScope {
    let prev =
        RENDER_SCOPE.with(|slot| slot.borrow_mut().replace(RenderScope { registry, elapsed }));
    RenderRegistryScope(prev)
}

/// The colour a late-bound paint slot currently holds, if a registry is installed and still has it.
pub(crate) fn resolve_render_paint_slot(slot: u16) -> Option<Color> {
    resolve_render_slot(slot)
}

/// The amount a late-bound scalar slot currently holds, if a registry is installed and still has
/// it.
pub(crate) fn resolve_render_scalar_slot(slot: u16) -> Option<f32> {
    resolve_render_slot(slot)
}

fn resolve_render_slot<T: Lerp + PartialEq + Copy + 'static>(slot: u16) -> Option<T> {
    RENDER_SCOPE.with(|installed| {
        installed
            .borrow()
            .as_ref()
            .and_then(|scope| scope.registry.resolve_slot::<T>(slot))
    })
}

/// The runtime clock reading the current draw paints at, or zero outside a draw.
pub(crate) fn render_elapsed() -> Duration {
    RENDER_SCOPE.with(|installed| {
        installed
            .borrow()
            .as_ref()
            .map_or(Duration::ZERO, |scope| scope.elapsed)
    })
}

/// Registry of per-key property transitions.
///
/// Owned by [`crate::core::runtime_env::RuntimeEnv`] and shared across all
/// component contexts in a runtime.
#[derive(Default)]
pub(crate) struct AnimationRegistry {
    entries: RefCell<HashMap<Key, Box<dyn DynEntry>>>,
    /// Keys indexed by the slot id a late-bound value carries. [`Paint`] and [`EffectAmount`] must
    /// stay `Copy`, so they name their entry by slot rather than holding the key. One id space
    /// serves every value type: a key holds one type for its whole life, and resolving a slot as
    /// the wrong type finds nothing.
    render_slots: RefCell<Vec<Key>>,
    slot_by_key: RefCell<HashMap<Key, u16>>,
    generation: Cell<u64>,
}

/// What advancing the registry by one frame requires of the runtime.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TransitionTick {
    /// A value some `view()` read as a concrete value changed, so the view must run again for it to
    /// reach the screen.
    pub(crate) view_changed: bool,
    /// A late-bound value changed. The renderer resolves those itself, so a repaint is enough.
    pub(crate) render_changed: bool,
}

/// A render-resolved transition as handed out: the slot naming it, where it is now, and where it
/// is going. A late-bound paint falls back to the former, a late-bound amount to the latter.
struct RenderValue<T> {
    slot: Option<u16>,
    current: T,
    target: T,
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
        self.advance(key, target, config, false, None)
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
        frame_interval: Option<Duration>,
    ) -> Paint {
        let value = self.render_value(key, target, config, frame_interval);
        match value.slot {
            Some(slot) => Paint::Animated {
                slot,
                fallback: value.current,
            },
            None => Paint::Solid(value.current),
        }
    }

    /// Like [`animated_paint`](Self::animated_paint), for the strength of a render-time color
    /// transform: an [`EffectAmount`] naming the entry instead of its current value.
    pub(crate) fn animated_amount(
        &self,
        key: Key,
        target: f32,
        config: TransitionConfig,
        frame_interval: Option<Duration>,
    ) -> EffectAmount {
        let value = self.render_value(key, target, config, frame_interval);
        match value.slot {
            Some(slot) => EffectAmount::animated(slot, value.target),
            None => EffectAmount::fixed(value.current),
        }
    }

    /// Advance `key` as a render-resolved transition and name it by slot.
    fn render_value<T: Lerp + PartialEq + 'static>(
        &self,
        key: Key,
        target: T,
        config: TransitionConfig,
        frame_interval: Option<Duration>,
    ) -> RenderValue<T> {
        let current = self.advance(key.clone(), target.clone(), config, true, frame_interval);
        RenderValue {
            slot: self.slot_for(key),
            current,
            target,
        }
    }

    /// The slot id naming `key`, minting one on first use.
    ///
    /// Slots are never reused for a different key, so a value handed out earlier can never resolve
    /// to an unrelated transition. Returns [`None`] once the id space is exhausted, which asks the
    /// caller to hand out a plain value instead — the animation degrades to a snap rather than
    /// misbinding.
    fn slot_for(&self, key: Key) -> Option<u16> {
        if let Some(slot) = self.slot_by_key.borrow().get(&key) {
            return Some(*slot);
        }
        let mut slots = self.render_slots.borrow_mut();
        let slot = u16::try_from(slots.len()).ok()?;
        slots.push(key.clone());
        self.slot_by_key.borrow_mut().insert(key, slot);
        Some(slot)
    }

    /// The current value behind a late-bound slot, or [`None`] if the slot no longer resolves or
    /// holds another type.
    fn resolve_slot<T: Lerp + PartialEq + Copy + 'static>(&self, slot: u16) -> Option<T> {
        let key = self.render_slots.borrow().get(slot as usize)?.clone();
        let entries = self.entries.borrow();
        let entry = entries.get(&key)?;
        let typed = entry.as_any().downcast_ref::<TypedEntry<T>>()?;
        Some(typed.current)
    }

    /// The current colour behind a late-bound paint, or [`None`] if the slot no longer resolves.
    #[cfg(test)]
    pub(crate) fn resolve_paint_slot(&self, slot: u16) -> Option<Color> {
        self.resolve_slot(slot)
    }

    /// The current amount behind a late-bound scalar, or [`None`] if the slot no longer resolves.
    #[cfg(test)]
    pub(crate) fn resolve_scalar_slot(&self, slot: u16) -> Option<f32> {
        self.resolve_slot(slot)
    }

    fn advance<T: Lerp + PartialEq + 'static>(
        &self,
        key: Key,
        target: T,
        config: TransitionConfig,
        render_resolved: bool,
        render_interval: Option<Duration>,
    ) -> T {
        let mut entries = self.entries.borrow_mut();
        let entry = entries.entry(key).or_insert_with(|| {
            Box::new(TypedEntry::<T> {
                current: target.clone(),
                target: target.clone(),
                transition: None,
                touched: Cell::new(true),
                render_resolved: Cell::new(render_resolved),
                render_interval: Cell::new(render_interval),
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
        if !render_resolved {
            typed.render_resolved.set(false);
        }
        typed.render_interval.set(render_interval);

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
    /// Values a view read concretely need that view to run again; late-bound values only need the
    /// screen redrawn. The memo generation is bumped only for the former, so a fade does not
    /// invalidate memoized subtrees that never depended on it.
    pub(crate) fn tick(&self, dt: Duration) -> TransitionTick {
        let mut entries = self.entries.borrow_mut();
        let mut result = TransitionTick::default();
        for entry in entries.values_mut() {
            if entry.tick(dt) {
                if entry.render_resolved() {
                    result.render_changed = true;
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
        // Slot ids stay assigned for the life of the runtime: late-bound values already handed out
        // carry them, and a key that comes back must resolve to the same slot. Dropped entries make
        // `resolve_slot` fall through to the value's own fallback until they are read again.
        debug_assert!(
            self.render_slots.borrow().len() >= self.slot_by_key.borrow().len(),
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
    #[cfg(test)]
    pub(crate) fn has_active(&self) -> bool {
        self.entries.borrow().values().any(|e| e.is_animating())
    }

    /// Whether an active transition has a concrete value baked into view output.
    pub(crate) fn has_active_view_transition(&self) -> bool {
        self.entries
            .borrow()
            .values()
            .any(|entry| entry.is_animating() && !entry.render_resolved())
    }

    /// Whether an active transition is resolved by the renderer from a late-bound value.
    #[cfg(test)]
    pub(crate) fn has_active_render_transition(&self) -> bool {
        self.entries
            .borrow()
            .values()
            .any(|entry| entry.is_animating() && entry.render_resolved())
    }

    /// Fastest cadence requested by an active render-resolved transition, falling back to the app
    /// default.
    pub(crate) fn active_render_transition_interval(&self, default: Duration) -> Option<Duration> {
        self.entries
            .borrow()
            .values()
            .filter(|entry| entry.is_animating() && entry.render_resolved())
            .map(|entry| entry.render_interval().unwrap_or(default))
            .min()
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

    /// The registry slot a late-bound amount names, read back through the registry itself.
    fn amount_slot(amount: EffectAmount) -> u16 {
        (0..=u16::MAX)
            .find(|&slot| EffectAmount::animated(slot, amount.resting_value()) == amount)
            .expect("a late-bound amount names a slot")
    }

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
    fn active_transitions_report_whether_they_need_view_or_paint() {
        let reg = AnimationRegistry::default();
        let _ = reg.transition::<f32>("layout".into(), 0.0, cfg(100));
        let _ = reg.animated_paint("chrome".into(), Color::Red, cfg(100), None);
        assert!(!reg.has_active_view_transition());
        assert!(!reg.has_active_render_transition());

        let _ = reg.transition::<f32>("layout".into(), 1.0, cfg(100));
        let _ = reg.animated_paint("chrome".into(), Color::Blue, cfg(100), None);
        assert!(reg.has_active_view_transition());
        assert!(reg.has_active_render_transition());
    }

    #[test]
    fn animated_amounts_advance_as_render_changes_and_resolve_by_slot() {
        let reg = AnimationRegistry::default();
        let _ = reg.animated_amount("tint".into(), 0.0, cfg(100), None);
        let amount = reg.animated_amount("tint".into(), 0.2, cfg(100), None);
        assert!(
            amount.is_transition(),
            "expected a late-bound amount, got {amount:?}"
        );
        assert_eq!(amount.resting_value(), 0.2, "it rests at its target");
        let slot = amount_slot(amount);
        assert!(reg.has_active_render_transition());
        assert!(!reg.has_active_view_transition());

        let tick = reg.tick(Duration::from_millis(50));
        assert!(tick.render_changed);
        assert!(
            !tick.view_changed,
            "a late-bound amount never needs a view pass"
        );
        let midway = reg.resolve_scalar_slot(slot).expect("slot resolves");
        assert!((0.09..=0.11).contains(&midway), "{midway}");

        // The same key read again hands out an identical value for the whole transition.
        let again = reg.animated_amount("tint".into(), 0.2, cfg(100), None);
        assert_eq!(again, amount);
    }

    #[test]
    fn colour_and_scalar_slots_share_one_id_space_without_crosstalk() {
        let reg = AnimationRegistry::default();
        let paint = reg.animated_paint("chrome".into(), Color::Red, cfg(100), None);
        let amount = reg.animated_amount("tint".into(), 0.5, cfg(100), None);
        let Paint::Animated {
            slot: paint_slot, ..
        } = paint
        else {
            panic!("expected an animated paint");
        };
        let amount_slot = amount_slot(amount);
        assert_ne!(paint_slot, amount_slot);
        assert_eq!(reg.resolve_paint_slot(paint_slot), Some(Color::Red));
        assert_eq!(reg.resolve_scalar_slot(amount_slot), Some(0.5));
        assert_eq!(reg.resolve_scalar_slot(paint_slot), None);
        assert_eq!(reg.resolve_paint_slot(amount_slot), None);
    }

    #[test]
    fn a_scalar_read_concretely_keeps_asking_for_view_passes() {
        let reg = AnimationRegistry::default();
        let _ = reg.animated_amount("shared".into(), 0.0, cfg(100), None);
        let _ = reg.transition::<f32>("shared".into(), 0.0, cfg(100));
        let _ = reg.animated_amount("shared".into(), 1.0, cfg(100), None);
        let tick = reg.tick(Duration::from_millis(50));
        assert!(
            tick.view_changed,
            "some view baked the concrete value in, so it must run again"
        );
    }

    #[test]
    fn render_scope_resolves_slots_and_the_draw_clock() {
        let reg = std::rc::Rc::new(AnimationRegistry::default());
        let _ = reg.animated_amount("tint".into(), 0.0, cfg(100), None);
        let amount = reg.animated_amount("tint".into(), 1.0, cfg(100), None);
        let _ = reg.tick(Duration::from_millis(25));
        assert_eq!(amount.resolved(), 1.0, "outside a draw the target answers");
        assert_eq!(render_elapsed(), Duration::ZERO);
        {
            let _scope = set_render_registry(std::rc::Rc::clone(&reg), Duration::from_secs(3));
            assert!((amount.resolved() - 0.25).abs() < 1e-4);
            assert_eq!(render_elapsed(), Duration::from_secs(3));
        }
        assert_eq!(render_elapsed(), Duration::ZERO);
    }

    #[test]
    #[should_panic(expected = "different value type")]
    fn reusing_key_with_different_type_panics() {
        let reg = AnimationRegistry::default();
        let _ = reg.transition::<Color>("k".into(), Color::Red, cfg(100));
        let _ = reg.transition::<f32>("k".into(), 0.0, cfg(100));
    }
}
