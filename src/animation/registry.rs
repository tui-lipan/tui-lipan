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
//! `animated_color()`, `animated_amount()`, and `pulsing_amount()` hand out a late-bound value
//! instead - a [`Paint::Animated`] or an [`EffectAmount`] carrying an [`AnimationHandle`] - which
//! the renderer resolves while painting. The value never escapes into `view()`, so advancing it is
//! a repaint.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::time::Duration;

use crate::animation::AnimationHandle;
use crate::animation::transition::{Lerp, Transition, TransitionConfig};
use crate::callback::ScopeId;
use crate::core::element::Key;
use crate::style::{Color, EffectAmount, EffectPulse, Paint};

/// The identity of a registry entry: the component instance that asked for it, and its key.
///
/// Scoped like every other piece of component-local keyed state, so two instances of one component
/// using the same literal key animate independently.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct AnimationKey {
    pub(crate) scope: ScopeId,
    pub(crate) key: Key,
}

impl AnimationKey {
    pub(crate) fn new(scope: ScopeId, key: impl Into<Key>) -> Self {
        Self {
            scope,
            key: key.into(),
        }
    }
}

/// How much of the screen a draw paints, which decides what it proves about who reads a slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaintExtent {
    /// The whole tree is painted, so a slot nothing resolved has no consumer on screen.
    Full,
    /// Only part of it - a few damaged terminal rows - so an unresolved slot proves nothing.
    Partial,
}

trait DynEntry: Any {
    fn entry_type_id(&self) -> TypeId;
    /// Advance by `dt` of capped animation time, with `now` the uncapped runtime clock.
    fn tick(&mut self, dt: Duration, now: Duration) -> bool;
    /// Bring a pulse's sample up to `now` at once, as it becomes visible again.
    fn wake(&mut self, now: Duration);
    fn is_animating(&self) -> bool;
    /// Whether this entry animates until it is dropped, rather than settling.
    fn is_perpetual(&self) -> bool;
    fn state(&self) -> &EntryState;
    fn as_any_mut(&mut self) -> &mut dyn Any;
    fn as_any(&self) -> &dyn Any;
}

/// Bookkeeping every entry carries, whatever its value type.
struct EntryState {
    touched: Cell<bool>,
    /// Set when the value is handed out late-bound - as a handle the renderer resolves - rather
    /// than as a concrete value. The view then cannot have baked the value into anything but a
    /// render input, which is what makes advancing it a repaint instead of a rebuild.
    render_resolved: Cell<bool>,
    render_interval: Cell<Option<Duration>>,
    /// Paint epoch in which a renderer last read this entry. See
    /// [`AnimationRegistry::begin_paint`].
    resolved_epoch: Cell<u64>,
    /// A perpetual animation the latest full paint did not read: it neither advances nor asks for
    /// paints until a paint reads it again.
    suspended: Cell<bool>,
}

impl EntryState {
    fn new(render_resolved: bool, render_interval: Option<Duration>, epoch: u64) -> Self {
        Self {
            touched: Cell::new(true),
            render_resolved: Cell::new(render_resolved),
            render_interval: Cell::new(render_interval),
            resolved_epoch: Cell::new(epoch),
            suspended: Cell::new(false),
        }
    }
}

struct TypedEntry<T: Lerp + PartialEq + 'static> {
    current: T,
    target: T,
    animation: Option<RenderAnimation<T>>,
    state: EntryState,
}

/// What moves an entry's value between frames.
enum RenderAnimation<T: Lerp> {
    /// Toward the entry's target, then done.
    Transition(Transition<T>),
    /// Back and forth forever, sampled on the pulse's own timeline.
    Pulse(PulseState<T>),
}

/// A running pulse: its shape, and how far along its own timeline it is.
struct PulseState<T> {
    from: T,
    to: T,
    period: Duration,
    easing: crate::animation::Easing,
    /// Time between samples. The value only changes on a sample, so every paint between two of
    /// them - a partial terminal-damage repaint included - sees the same value.
    interval: Duration,
    /// Runtime clock reading the pulse started at. The timeline is measured against the real
    /// clock, not summed from capped animation steps, so a stalled loop never slows the pulse.
    started: Duration,
    /// Time since the pulse started, as of its latest sample.
    elapsed: Duration,
}

impl<T: Lerp> PulseState<T> {
    /// The value at the latest sample point at or before `elapsed`.
    fn sample(&self) -> T {
        let interval = self.interval.as_nanos().max(1);
        let sampled = self.elapsed.as_nanos() / interval * interval;
        let period = self.period.as_nanos().max(1);
        let t = (sampled % period) as f64 / period as f64;
        // Triangle wave: 0 at the start of the cycle, 1 halfway, back to 0 at the end.
        let rise = (1.0 - (2.0 * t - 1.0).abs()) as f32;
        T::lerp(&self.from, &self.to, self.easing.apply(rise))
    }
}

impl<T: Lerp + PartialEq + 'static> DynEntry for TypedEntry<T> {
    fn entry_type_id(&self) -> TypeId {
        TypeId::of::<T>()
    }

    fn tick(&mut self, dt: Duration, now: Duration) -> bool {
        let new_current = match self.animation.as_mut() {
            None => return false,
            Some(RenderAnimation::Transition(transition)) => {
                transition.tick(dt);
                if transition.is_complete() {
                    let settled = self.target.clone();
                    self.animation = None;
                    settled
                } else {
                    transition.current()
                }
            }
            Some(RenderAnimation::Pulse(pulse)) => {
                pulse.elapsed = now.saturating_sub(pulse.started);
                pulse.sample()
            }
        };
        let changed = new_current != self.current;
        self.current = new_current;
        changed
    }

    fn wake(&mut self, now: Duration) {
        if let Some(RenderAnimation::Pulse(pulse)) = self.animation.as_mut() {
            pulse.elapsed = now.saturating_sub(pulse.started);
            self.current = pulse.sample();
        }
    }

    fn is_animating(&self) -> bool {
        self.animation.is_some()
    }

    fn is_perpetual(&self) -> bool {
        matches!(self.animation, Some(RenderAnimation::Pulse(_)))
    }

    fn state(&self) -> &EntryState {
        &self.state
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
    /// The runtime clock when the draw began. Custom effects read it as
    /// [`EffectContext::elapsed`](crate::style::EffectContext::elapsed); registry animations only
    /// read it to catch a suspended pulse up as it becomes visible again. Otherwise they change
    /// only when the registry ticks.
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
///
/// Dropping it also ends the draw: if it was a full paint, perpetual animations it did not read
/// are suspended. See [`AnimationRegistry::begin_paint`].
pub(crate) struct RenderRegistryScope(Option<RenderScope>);

impl Drop for RenderRegistryScope {
    fn drop(&mut self) {
        let ending =
            RENDER_SCOPE.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), self.0.take()));
        if let Some(scope) = ending {
            scope.registry.finish_paint();
        }
    }
}

/// Make `registry` the one this draw resolves late-bound values against, and `elapsed` the
/// runtime clock reading it paints at, until the guard drops.
///
/// A [`PaintExtent::Full`] draw starts a new paint epoch: see [`AnimationRegistry::begin_paint`].
pub(crate) fn set_render_registry(
    registry: std::rc::Rc<AnimationRegistry>,
    elapsed: Duration,
    extent: PaintExtent,
) -> RenderRegistryScope {
    if extent == PaintExtent::Full {
        registry.begin_paint();
    }
    let prev =
        RENDER_SCOPE.with(|slot| slot.borrow_mut().replace(RenderScope { registry, elapsed }));
    RenderRegistryScope(prev)
}

/// The colour a late-bound paint currently holds, if a registry is installed and still has it.
pub(crate) fn resolve_render_paint(handle: AnimationHandle) -> Option<Color> {
    resolve_render_handle(handle)
}

/// The amount a late-bound scalar currently holds, if a registry is installed and still has it.
pub(crate) fn resolve_render_scalar(handle: AnimationHandle) -> Option<f32> {
    resolve_render_handle(handle)
}

fn resolve_render_handle<T: Lerp + PartialEq + Copy + 'static>(
    handle: AnimationHandle,
) -> Option<T> {
    RENDER_SCOPE.with(|installed| {
        installed
            .borrow()
            .as_ref()
            .and_then(|scope| scope.registry.resolve::<T>(handle, scope.elapsed))
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

/// One entry of the handle table: who holds the slot now, and its current generation.
struct RenderSlot {
    key: Option<AnimationKey>,
    generation: u16,
}

/// Registry of per-key property transitions.
///
/// Owned by [`crate::core::runtime_env::RuntimeEnv`] and shared across all
/// component contexts in a runtime.
#[derive(Default)]
pub(crate) struct AnimationRegistry {
    entries: RefCell<HashMap<AnimationKey, Box<dyn DynEntry>>>,
    /// The handle table. [`Paint`] and [`EffectAmount`] must stay small and `Copy`, so they name
    /// their entry by an [`AnimationHandle`] - slot plus generation - rather than holding the key.
    /// One table serves every value type: a key holds one type for its whole life, and resolving a
    /// handle as the wrong type finds nothing.
    ///
    /// A slot is released when its entry is dropped and reused with the next generation, so the
    /// table is bounded by the animations alive at once, not by every key a long session has
    /// ever used.
    render_slots: RefCell<Vec<RenderSlot>>,
    free_slots: RefCell<Vec<u16>>,
    handle_by_key: RefCell<HashMap<AnimationKey, AnimationHandle>>,
    generation: Cell<u64>,
    /// Counts full paints. A perpetual animation whose slot no full paint resolved has nothing on
    /// screen reading it - a hover effect nobody hovers, a scope scrolled away - and is suspended
    /// until one does, rather than repainting an idle app forever.
    paint_epoch: Cell<u64>,
    /// Whether a full paint has begun and not yet finished.
    painting: Cell<bool>,
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

/// Whether `entry` is a render-resolved animation that should keep asking for paints.
fn is_live_render_animation(entry: &dyn DynEntry) -> bool {
    let state = entry.state();
    entry.is_animating() && state.render_resolved.get() && !state.suspended.get()
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
        key: AnimationKey,
        target: T,
        config: TransitionConfig,
    ) -> T {
        self.advance(key, target, config, false, None)
    }

    /// Like [`transition`](Self::transition), but hands back a [`Paint`] that names the entry instead
    /// of its current colour.
    ///
    /// The renderer resolves the handle while painting, so the element tree holds still for the
    /// whole fade and the runtime can answer each frame with a repaint. Because the caller never
    /// sees the interpolated colour, it cannot have used it for anything but a style — which is
    /// exactly the property that makes skipping `view()` sound.
    pub(crate) fn animated_paint(
        &self,
        key: AnimationKey,
        target: Color,
        config: TransitionConfig,
        frame_interval: Option<Duration>,
    ) -> Paint {
        let current = self.advance(key.clone(), target, config, true, frame_interval);
        match self.handle_or_forget(key) {
            Some(handle) => Paint::Animated {
                handle,
                fallback: current,
            },
            None => Paint::Solid(current),
        }
    }

    /// Like [`animated_paint`](Self::animated_paint), for the strength of a render-time color
    /// transform: an [`EffectAmount`] naming the entry instead of its current value.
    pub(crate) fn animated_amount(
        &self,
        key: AnimationKey,
        target: f32,
        config: TransitionConfig,
        frame_interval: Option<Duration>,
    ) -> EffectAmount {
        let current = self.advance(key.clone(), target, config, true, frame_interval);
        match self.handle_or_forget(key) {
            Some(handle) => EffectAmount::animated(handle, target),
            None => EffectAmount::fixed(current),
        }
    }

    /// Like [`animated_amount`](Self::animated_amount), for an amount that pulses instead of
    /// settling. The registry owns the pulse's timeline and samples it at the pulse's frame rate.
    pub(crate) fn pulsing_amount(
        &self,
        key: AnimationKey,
        pulse: EffectPulse,
        now: Duration,
    ) -> EffectAmount {
        let current = self.advance_pulse(key.clone(), pulse, now);
        match self.handle_or_forget(key) {
            Some(handle) => EffectAmount::pulsing(handle, pulse.from),
            None => EffectAmount::fixed(current),
        }
    }

    /// The handle naming `key`, minting one on first use - or, when every slot is taken, drop the
    /// entry just advanced and return [`None`].
    ///
    /// The caller then hands out a plain value. Dropping the entry keeps that honest: an entry no
    /// handle names could only keep animating - and scheduling paints - for nothing on screen. The
    /// animation degrades to a static value until a slot frees up, rather than misbinding.
    fn handle_or_forget(&self, key: AnimationKey) -> Option<AnimationHandle> {
        let handle = self.handle_for(key.clone());
        if handle.is_none() {
            self.entries.borrow_mut().remove(&key);
        }
        handle
    }

    /// The handle naming `key`, minting one on first use from a released slot if there is one.
    fn handle_for(&self, key: AnimationKey) -> Option<AnimationHandle> {
        if let Some(handle) = self.handle_by_key.borrow().get(&key) {
            return Some(*handle);
        }
        let mut slots = self.render_slots.borrow_mut();
        let slot = match self.free_slots.borrow_mut().pop() {
            Some(slot) => slot,
            None => {
                let slot = u16::try_from(slots.len()).ok()?;
                slots.push(RenderSlot {
                    key: None,
                    generation: 0,
                });
                slot
            }
        };
        let entry = &mut slots[usize::from(slot)];
        entry.key = Some(key.clone());
        let handle = AnimationHandle::new(slot, entry.generation);
        self.handle_by_key.borrow_mut().insert(key, handle);
        Some(handle)
    }

    /// Give `key`'s slot back, bumping its generation so no handle minted for `key` resolves again.
    fn release_handle(&self, key: &AnimationKey) {
        let Some(handle) = self.handle_by_key.borrow_mut().remove(key) else {
            return;
        };
        let mut slots = self.render_slots.borrow_mut();
        let slot = &mut slots[usize::from(handle.slot())];
        slot.key = None;
        slot.generation = AnimationHandle::next_generation(slot.generation);
        self.free_slots.borrow_mut().push(handle.slot());
    }

    /// The current value behind a late-bound handle, or [`None`] if it no longer resolves or names
    /// another type.
    ///
    /// Stamps the entry as read in this paint. A suspended pulse read here is visible again: it
    /// is caught up to its latest sample at `now` before its value is returned, so the paint that
    /// wakes it shows where the pulse is rather than where it was when it was hidden. A pulse that
    /// is already running returns its stored sample, whatever `now` says.
    fn resolve<T: Lerp + PartialEq + Copy + 'static>(
        &self,
        handle: AnimationHandle,
        now: Duration,
    ) -> Option<T> {
        let key = {
            let slots = self.render_slots.borrow();
            let slot = slots.get(usize::from(handle.slot()))?;
            if slot.generation != handle.generation() {
                return None;
            }
            slot.key.clone()?
        };
        let mut entries = self.entries.borrow_mut();
        let entry = entries.get_mut(&key)?;
        if entry.state().suspended.replace(false) {
            entry.wake(now);
        }
        entry.state().resolved_epoch.set(self.paint_epoch.get());
        let typed = entry.as_any().downcast_ref::<TypedEntry<T>>()?;
        Some(typed.current)
    }

    /// The current colour behind a late-bound paint, or [`None`] if the handle no longer resolves.
    #[cfg(test)]
    pub(crate) fn resolve_paint(&self, handle: AnimationHandle) -> Option<Color> {
        self.resolve(handle, Duration::ZERO)
    }

    /// The current amount behind a late-bound scalar, or [`None`] if the handle no longer
    /// resolves.
    #[cfg(test)]
    pub(crate) fn resolve_scalar(&self, handle: AnimationHandle) -> Option<f32> {
        self.resolve(handle, Duration::ZERO)
    }

    fn advance<T: Lerp + PartialEq + 'static>(
        &self,
        key: AnimationKey,
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
                animation: None,
                state: EntryState::new(render_resolved, render_interval, self.paint_epoch.get()),
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

        typed.state.touched.set(true);
        // A key read as a concrete value even once must keep asking for view passes: some view has
        // baked that value into something the renderer cannot re-derive.
        if !render_resolved {
            typed.state.render_resolved.set(false);
        }
        typed.state.render_interval.set(render_interval);

        // A pulsing key asked for a target instead settles from wherever the pulse is. A finite
        // transition always runs to its end, so it is never suspended.
        let was_pulsing = matches!(typed.animation, Some(RenderAnimation::Pulse(_)));
        typed.state.suspended.set(false);
        if typed.target != target || was_pulsing {
            let from = typed.current.clone();
            typed.target = target.clone();
            if config.duration.is_zero() {
                typed.current = target.clone();
                typed.animation = None;
            } else {
                typed.animation = Some(RenderAnimation::Transition(Transition::new(
                    from,
                    target.clone(),
                    config.duration,
                    config.easing,
                )));
            }
        }

        typed.current.clone()
    }

    /// Read or start the pulse keyed by `key`, returning its current sample.
    ///
    /// A new key starts at `pulse.from`. A key that keeps the same shape keeps its timeline; a new
    /// shape keeps the timeline too and resamples, so changing a pulse's amplitude does not restart
    /// its phase. A key that was transitioning starts pulsing from the beginning of the cycle.
    fn advance_pulse(&self, key: AnimationKey, pulse: EffectPulse, now: Duration) -> f32 {
        let interval = pulse.interval();
        let mut entries = self.entries.borrow_mut();
        let entry = entries.entry(key).or_insert_with(|| {
            Box::new(TypedEntry::<f32> {
                current: pulse.from,
                target: pulse.from,
                animation: None,
                state: EntryState::new(true, Some(interval), self.paint_epoch.get()),
            })
        });

        if entry.entry_type_id() != TypeId::of::<f32>() {
            panic!(
                "Ctx::transition called with a different value type for the same key (existing type id mismatch)"
            );
        }

        let typed: &mut TypedEntry<f32> = entry
            .as_any_mut()
            .downcast_mut()
            .expect("type id checked above");
        typed.state.touched.set(true);
        typed.state.render_interval.set(Some(interval));

        let (started, elapsed) = match &typed.animation {
            Some(RenderAnimation::Pulse(running)) => (running.started, running.elapsed),
            // A pulse that is (re)starting has a consumer in mind: count it as live until a full
            // paint says otherwise.
            _ => {
                typed.state.suspended.set(false);
                (now, Duration::ZERO)
            }
        };
        let state = PulseState {
            from: pulse.from,
            to: pulse.to,
            period: pulse.period,
            easing: pulse.easing,
            interval,
            started,
            elapsed,
        };
        typed.current = state.sample();
        typed.target = pulse.from;
        typed.animation = Some(RenderAnimation::Pulse(state));
        typed.current
    }

    /// Advance all in-flight transitions by `dt`, reporting what the change requires.
    ///
    /// Values a view read concretely need that view to run again; late-bound values only need the
    /// screen redrawn. The memo generation is bumped only for the former, so a fade does not
    /// invalidate memoized subtrees that never depended on it.
    ///
    /// `dt` is the capped animation step, which keeps a finite transition from skipping to its end
    /// after a stall. `now` is the uncapped runtime clock: a pulse samples its timeline against it,
    /// so a stalled loop never slows a pulse down. A suspended pulse is not advanced at all.
    pub(crate) fn tick(&self, dt: Duration, now: Duration) -> TransitionTick {
        let mut entries = self.entries.borrow_mut();
        let mut result = TransitionTick::default();
        for entry in entries.values_mut() {
            if entry.state().suspended.get() {
                continue;
            }
            if entry.tick(dt, now) {
                if entry.state().render_resolved.get() {
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
    ///
    /// A dropped entry's handle slot is released for reuse under a new generation. Handles still
    /// held by an old element tree - a memoized subtree, say - then fail to resolve and fall back
    /// to their own resting value, rather than naming whatever animation takes the slot next.
    pub(crate) fn end_frame_gc(&self) {
        let mut dropped = Vec::new();
        {
            let mut entries = self.entries.borrow_mut();
            entries.retain(|key, entry| {
                let keep = entry.state().touched.get();
                if !keep {
                    dropped.push(key.clone());
                }
                keep
            });
            for entry in entries.values() {
                entry.state().touched.set(false);
            }
        }
        for key in &dropped {
            self.release_handle(key);
        }
        if !dropped.is_empty() {
            self.generation
                .set(self.generation.get().wrapping_add(1).max(1));
        }
    }

    /// Whether any transition or pulse is still moving.
    #[cfg(test)]
    pub(crate) fn has_active(&self) -> bool {
        self.entries.borrow().values().any(|e| e.is_animating())
    }

    /// Start a full paint: a new paint epoch that every entry the paint resolves is stamped with.
    /// When the paint ends ([`finish_paint`](Self::finish_paint)), a perpetual render animation
    /// stamped with an older epoch had no reader on screen and is suspended.
    pub(crate) fn begin_paint(&self) {
        self.paint_epoch.set(self.paint_epoch.get().wrapping_add(1));
        self.painting.set(true);
    }

    /// End the current draw. After a full paint, suspend the perpetual render animations it did
    /// not read. A partial paint proves nothing about who reads what, so it changes nothing.
    pub(crate) fn finish_paint(&self) {
        if !self.painting.replace(false) {
            return;
        }
        let epoch = self.paint_epoch.get();
        for entry in self.entries.borrow().values() {
            let state = entry.state();
            if entry.is_perpetual()
                && state.render_resolved.get()
                && state.resolved_epoch.get() != epoch
            {
                state.suspended.set(true);
            }
        }
    }

    /// Whether an active transition has a concrete value baked into view output.
    pub(crate) fn has_active_view_transition(&self) -> bool {
        self.entries
            .borrow()
            .values()
            .any(|entry| entry.is_animating() && !entry.state().render_resolved.get())
    }

    /// Whether an active transition is resolved by the renderer from a late-bound value.
    #[cfg(test)]
    pub(crate) fn has_active_render_transition(&self) -> bool {
        self.entries
            .borrow()
            .values()
            .any(|entry| is_live_render_animation(entry.as_ref()))
    }

    /// Fastest cadence requested by an active render-resolved transition, falling back to the app
    /// default.
    pub(crate) fn active_render_transition_interval(&self, default: Duration) -> Option<Duration> {
        self.entries
            .borrow()
            .values()
            .filter(|entry| is_live_render_animation(entry.as_ref()))
            .map(|entry| entry.state().render_interval.get().unwrap_or(default))
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

    /// Slots in the handle table, taken or free.
    #[cfg(test)]
    pub(crate) fn slot_count(&self) -> usize {
        self.render_slots.borrow().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::easing::Easing;
    use crate::style::Color;

    /// The registry handle a late-bound amount names.
    fn amount_slot(amount: EffectAmount) -> AnimationHandle {
        amount.handle().expect("a late-bound amount names a handle")
    }

    impl From<&'static str> for AnimationKey {
        fn from(key: &'static str) -> Self {
            AnimationKey::new(ScopeId(0), key)
        }
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
        let changed = reg.tick(Duration::from_millis(50), Duration::ZERO);
        assert!(changed.view_changed);

        // Read again with same target — should return the interpolated current,
        // not red and not blue.
        let v2 = reg.transition::<Color>("k".into(), Color::Blue, cfg(100));
        assert!(v2 != Color::Red && v2 != Color::Blue);

        // Tick to completion.
        let _ = reg.tick(Duration::from_millis(60), Duration::ZERO);
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
            reg.tick(Duration::from_millis(16), Duration::ZERO),
            TransitionTick::default()
        );
    }

    #[test]
    fn f32_transitions_supported() {
        let reg = AnimationRegistry::default();
        let _ = reg.transition::<f32>("scalar".into(), 0.0, cfg(100));
        let _ = reg.transition::<f32>("scalar".into(), 1.0, cfg(100));
        let _ = reg.tick(Duration::from_millis(50), Duration::ZERO);
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

        let tick = reg.tick(Duration::from_millis(50), Duration::ZERO);
        assert!(tick.render_changed);
        assert!(
            !tick.view_changed,
            "a late-bound amount never needs a view pass"
        );
        let midway = reg.resolve_scalar(slot).expect("slot resolves");
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
            handle: paint_slot, ..
        } = paint
        else {
            panic!("expected an animated paint");
        };
        let amount_slot = amount_slot(amount);
        assert_ne!(paint_slot, amount_slot);
        assert_eq!(reg.resolve_paint(paint_slot), Some(Color::Red));
        assert_eq!(reg.resolve_scalar(amount_slot), Some(0.5));
        assert_eq!(reg.resolve_scalar(paint_slot), None);
        assert_eq!(reg.resolve_paint(amount_slot), None);
    }

    #[test]
    fn a_scalar_read_concretely_keeps_asking_for_view_passes() {
        let reg = AnimationRegistry::default();
        let _ = reg.animated_amount("shared".into(), 0.0, cfg(100), None);
        let _ = reg.transition::<f32>("shared".into(), 0.0, cfg(100));
        let _ = reg.animated_amount("shared".into(), 1.0, cfg(100), None);
        let tick = reg.tick(Duration::from_millis(50), Duration::ZERO);
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
        let _ = reg.tick(Duration::from_millis(25), Duration::ZERO);
        assert_eq!(amount.resolved(), 1.0, "outside a draw the target answers");
        assert_eq!(render_elapsed(), Duration::ZERO);
        {
            let _scope = set_render_registry(
                std::rc::Rc::clone(&reg),
                Duration::from_secs(3),
                PaintExtent::Full,
            );
            assert!((amount.resolved() - 0.25).abs() < 1e-4);
            assert_eq!(render_elapsed(), Duration::from_secs(3));
        }
        assert_eq!(render_elapsed(), Duration::ZERO);
    }

    fn pulse(frame_rate: u16) -> EffectPulse {
        EffectPulse::new(0.0, 1.0)
            .period(Duration::from_millis(1000))
            .easing(crate::animation::Easing::Linear)
            .frame_rate(frame_rate)
    }

    fn ms(ms: u64) -> Duration {
        Duration::from_millis(ms)
    }

    #[test]
    fn a_pulse_starts_at_from_and_moves_only_when_ticked() {
        let reg = AnimationRegistry::default();
        let amount = reg.pulsing_amount("breath".into(), pulse(10), ms(5_000));
        assert!(amount.is_pulse());
        let slot = amount_slot(amount);
        assert_eq!(reg.resolve_scalar(slot), Some(0.0), "it starts at from");
        assert!(reg.has_active_render_transition());
        assert!(!reg.has_active_view_transition());
        assert_eq!(
            reg.active_render_transition_interval(ms(33)),
            Some(ms(100)),
            "it asks for its own cadence"
        );

        let tick = reg.tick(ms(100), ms(5_100));
        assert!(tick.render_changed && !tick.view_changed);
        assert!((reg.resolve_scalar(slot).unwrap() - 0.2).abs() < 1e-6);
        assert_eq!(
            reg.pulsing_amount("breath".into(), pulse(10), ms(5_100)),
            amount,
            "the tree holds one amount for the whole pulse"
        );
        assert!(
            (reg.resolve_scalar(slot).unwrap() - 0.2).abs() < 1e-6,
            "re-reading the key keeps the pulse's timeline"
        );
    }

    /// A pulse's timeline is the runtime clock, not the capped step the ticker hands transitions.
    #[test]
    fn a_stalled_tick_samples_the_pulse_at_the_real_elapsed_time() {
        let reg = AnimationRegistry::default();
        let slot = amount_slot(reg.pulsing_amount("breath".into(), pulse(10), ms(0)));
        // A 350 ms stall arrives as one capped 100 ms step.
        let _ = reg.tick(ms(100), ms(350));
        assert!(
            (reg.resolve_scalar(slot).unwrap() - 0.6).abs() < 1e-6,
            "sampled at 300 ms, the latest sample point the real clock has passed"
        );
    }

    /// Two pulses ticked on the fastest shared cadence each change value only at their own rate.
    #[test]
    fn each_pulse_keeps_its_own_sampling_cadence() {
        let reg = AnimationRegistry::default();
        let slow = amount_slot(reg.pulsing_amount("slow".into(), pulse(10), ms(0)));
        let fast = amount_slot(reg.pulsing_amount("fast".into(), pulse(30), ms(0)));
        let shared = reg
            .active_render_transition_interval(ms(33))
            .expect("pulses are active");
        assert_eq!(shared, crate::app::context::frame_interval(30));

        let (mut slow_changes, mut fast_changes) = (0, 0);
        let (mut slow_prev, mut fast_prev) = (0.0, 0.0);
        let mut now = Duration::ZERO;
        // Half a period, so the linear rise never turns around onto an equal value.
        for _ in 0..15 {
            now += shared;
            let _ = reg.tick(shared, now);
            let (s, f) = (
                reg.resolve_scalar(slow).unwrap(),
                reg.resolve_scalar(fast).unwrap(),
            );
            slow_changes += usize::from(s != slow_prev);
            fast_changes += usize::from(f != fast_prev);
            (slow_prev, fast_prev) = (s, f);
        }
        assert_eq!(
            fast_changes, 15,
            "the 30 fps pulse moves on every shared frame"
        );
        assert!(
            (4..=5).contains(&slow_changes),
            "the 10 fps pulse moves about every third frame: {slow_changes}"
        );
    }

    #[test]
    fn a_pulse_handed_a_target_settles_from_where_it_is() {
        let reg = AnimationRegistry::default();
        let _ = reg.pulsing_amount("alert".into(), pulse(30), ms(0));
        let _ = reg.tick(ms(50), ms(300));
        let settling = reg.animated_amount("alert".into(), 0.0, cfg(100), None);
        let slot = amount_slot(settling);
        let from = reg.resolve_scalar(slot).unwrap();
        assert!(from > 0.5, "the fade starts mid-breath: {from}");
        let _ = reg.tick(ms(50), ms(350));
        let midway = reg.resolve_scalar(slot).unwrap();
        assert!(midway < from && midway > 0.0, "{midway}");
        let _ = reg.tick(ms(60), ms(410));
        assert_eq!(reg.resolve_scalar(slot), Some(0.0));
        assert!(!reg.has_active(), "and then the key is at rest");
    }

    #[test]
    fn a_pulse_nobody_reads_is_collected() {
        let reg = AnimationRegistry::default();
        let _ = reg.pulsing_amount("alert".into(), pulse(10), ms(0));
        reg.end_frame_gc();
        reg.end_frame_gc();
        assert_eq!(reg.entry_count(), 0);
        assert!(
            reg.active_render_transition_interval(ms(33)).is_none(),
            "so it stops asking for paints"
        );
    }

    /// A pulse that a full paint did not read has no consumer on screen, so it stops asking for
    /// paints until a paint reads it again. A partial paint proves nothing either way.
    #[test]
    fn a_pulse_no_full_paint_reads_is_suspended_until_one_does() {
        let reg = std::rc::Rc::new(AnimationRegistry::default());
        let slot = amount_slot(reg.pulsing_amount("hover".into(), pulse(10), ms(0)));
        assert!(
            reg.has_active_render_transition(),
            "live until proven unread"
        );

        {
            let _paint = set_render_registry(std::rc::Rc::clone(&reg), ms(0), PaintExtent::Full);
        }
        assert!(
            !reg.has_active_render_transition(),
            "the full paint never read it"
        );
        assert_eq!(reg.active_render_transition_interval(ms(33)), None);
        assert_eq!(
            reg.tick(ms(100), ms(100)),
            TransitionTick::default(),
            "a suspended pulse does not advance or ask for a paint"
        );

        {
            let _paint = set_render_registry(std::rc::Rc::clone(&reg), ms(150), PaintExtent::Full);
            let _ = EffectAmount::pulsing(slot, 0.0).resolved();
        }
        assert!(
            reg.has_active_render_transition(),
            "a paint that reads it wakes it"
        );
        let _ = reg.tick(ms(100), ms(250));
        assert!(
            (reg.resolve_scalar(slot).unwrap() - 0.4).abs() < 1e-6,
            "on its unbroken timeline"
        );

        {
            let _damage =
                set_render_registry(std::rc::Rc::clone(&reg), ms(260), PaintExtent::Partial);
        }
        assert!(
            reg.has_active_render_transition(),
            "a partial repaint that skipped its rows does not suspend it"
        );
    }

    #[test]
    fn a_finite_transition_runs_to_its_end_without_a_reader() {
        let reg = std::rc::Rc::new(AnimationRegistry::default());
        let _ = reg.animated_amount("fade".into(), 0.0, cfg(100), None);
        let _ = reg.animated_amount("fade".into(), 1.0, cfg(100), None);
        {
            let _paint = set_render_registry(std::rc::Rc::clone(&reg), ms(0), PaintExtent::Full);
        }
        assert!(reg.has_active_render_transition());
        assert!(reg.tick(ms(50), ms(50)).render_changed);
    }

    /// Two instances of one component using the same literal key must not share an entry.
    #[test]
    fn the_same_key_in_two_scopes_names_two_animations() {
        let reg = AnimationRegistry::default();
        let a = AnimationKey::new(ScopeId(1), "alert");
        let b = AnimationKey::new(ScopeId(2), "alert");
        let a_amount = reg.pulsing_amount(a.clone(), pulse(10), ms(0));
        let _ = reg.tick(ms(100), ms(700));
        let b_amount = reg.pulsing_amount(b.clone(), pulse(10), ms(700));
        assert_ne!(amount_slot(a_amount), amount_slot(b_amount));
        assert_eq!(
            reg.resolve_scalar(amount_slot(b_amount)),
            Some(0.0),
            "B starts at from"
        );
        assert!(reg.resolve_scalar(amount_slot(a_amount)).unwrap() > 0.5);

        let _ = reg.animated_amount(a, 0.0, cfg(100), None);
        let _ = reg.tick(ms(100), ms(800));
        assert!(
            (reg.resolve_scalar(amount_slot(b_amount)).unwrap() - 0.2).abs() < 1e-6,
            "settling A leaves B pulsing on its own timeline"
        );
    }

    /// Scoped keys make every mounted instance a new key. Slots must be recycled as instances go,
    /// or a long session would exhaust the handle table one pane at a time.
    #[test]
    fn handle_slots_are_recycled_as_animations_are_dropped() {
        let reg = AnimationRegistry::default();
        // More instances over the session's life than a u16 could ever name at once.
        for scope in 0..70_000u32 {
            let key = AnimationKey::new(ScopeId(scope), "pane-alert-tint");
            let amount = reg.pulsing_amount(key, pulse(10), ms(0));
            assert!(
                amount.is_pulse(),
                "instance {scope} still gets a live pulse"
            );
            reg.end_frame_gc();
            reg.end_frame_gc();
        }
        assert_eq!(
            reg.slot_count(),
            1,
            "one slot, reused by every instance in turn"
        );
        assert_eq!(reg.entry_count(), 0);
    }

    /// A handle left in an old tree must not resolve to whatever animation reuses its slot.
    #[test]
    fn a_stale_handle_never_resolves_to_the_slots_next_occupant() {
        let reg = AnimationRegistry::default();
        let old = amount_slot(reg.animated_amount("old".into(), 0.25, cfg(100), None));
        reg.end_frame_gc();
        reg.end_frame_gc();
        let new = amount_slot(reg.animated_amount("new".into(), 0.75, cfg(100), None));
        assert_eq!(new.slot(), old.slot(), "the slot was reused");
        assert_ne!(new, old, "under a new generation");
        assert_eq!(reg.resolve_scalar(new), Some(0.75));
        assert_eq!(
            reg.resolve_scalar(old),
            None,
            "the old handle falls back to its own resting value"
        );
    }

    /// With every slot taken by a live animation, a new one gets a plain value - and no entry, so
    /// it cannot schedule paints nothing on screen could show.
    #[test]
    fn an_exhausted_handle_table_hands_out_plain_values_without_animating() {
        let reg = AnimationRegistry::default();
        for scope in 0..=u32::from(u16::MAX) {
            let key = AnimationKey::new(ScopeId(scope), "held");
            let _ = reg.animated_amount(key, 0.0, cfg(100), None);
        }
        assert_eq!(reg.slot_count(), usize::from(u16::MAX) + 1);
        let overflow = AnimationKey::new(ScopeId(u32::MAX), "overflow");
        let amount = reg.pulsing_amount(overflow.clone(), pulse(10), ms(0));
        assert_eq!(
            amount,
            EffectAmount::fixed(0.0),
            "the pulse's resting value"
        );
        assert!(
            !reg.entries.borrow().contains_key(&overflow),
            "and no entry left animating for nothing"
        );
        assert_eq!(reg.active_render_transition_interval(ms(33)), None);
    }

    /// A suspended pulse becomes visible on its latest sample, not on the one it was hidden at.
    /// A pulse that never stopped being read keeps its stored sample whatever the draw clock says.
    #[test]
    fn a_waking_pulse_catches_up_but_a_running_one_keeps_its_sample() {
        let reg = std::rc::Rc::new(AnimationRegistry::default());
        let hidden = amount_slot(reg.pulsing_amount("hidden".into(), pulse(10), ms(0)));
        let shown = amount_slot(reg.pulsing_amount("shown".into(), pulse(10), ms(0)));
        let _ = reg.tick(ms(100), ms(200));
        let paint = |now, read_hidden: bool| {
            let _paint = set_render_registry(std::rc::Rc::clone(&reg), now, PaintExtent::Full);
            let shown = EffectAmount::pulsing(shown, 0.0).resolved();
            let hidden = read_hidden.then(|| EffectAmount::pulsing(hidden, 0.0).resolved());
            (shown, hidden)
        };

        // Only `shown` is read: `hidden` is suspended at its 200 ms sample.
        let (shown_value, _) = paint(ms(250), false);
        assert!((shown_value - 0.4).abs() < 1e-6);

        // 550 ms later, with no tick in between, a full paint reads both.
        let (shown_value, hidden_value) = paint(ms(750), true);
        assert!(
            (hidden_value.unwrap() - 0.6).abs() < 1e-6,
            "the waking paint shows the 700 ms sample (0.6 on a linear 0-1-0 rise), not 0.4"
        );
        assert!(
            (shown_value - 0.4).abs() < 1e-6,
            "a running pulse only moves on ticks, so paints between them agree"
        );
    }

    #[test]
    #[should_panic(expected = "different value type")]
    fn reusing_key_with_different_type_panics() {
        let reg = AnimationRegistry::default();
        let _ = reg.transition::<Color>("k".into(), Color::Red, cfg(100));
        let _ = reg.transition::<f32>("k".into(), 0.0, cfg(100));
    }
}
