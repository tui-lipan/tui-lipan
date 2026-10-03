//! Visibility transitions for declarative root overlays.

use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use super::{Easing, Transition, TransitionConfig};
use crate::style::VisualEffect;

/// Direction of an overlay's visibility transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum OverlayAnimationPhase {
    /// Becoming visible, including reversal of an unfinished exit.
    Entering,
    /// Fully visible with no lifecycle transition running.
    Visible,
    /// Retained after removal, becoming hidden.
    Exiting,
}

/// Inputs to an overlay animation's effect factory.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct OverlayAnimationContext {
    /// Current visibility, clamped to `[0, 1]`, independent of direction.
    pub progress: f32,
    /// Whether the overlay is entering, visible, or exiting.
    pub phase: OverlayAnimationPhase,
}

impl OverlayAnimationContext {
    /// Construct inputs for testing an application's effect factory.
    pub fn new(progress: f32, phase: OverlayAnimationPhase) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            phase,
        }
    }
}

/// An overlay's enter and exit timing, with an optional application-defined paint effect.
///
/// Progress describes visibility: `0.0` is hidden and `1.0` is fully visible. Enter runs toward
/// one, exit toward zero. Reopening during exit reverses from the current value without snapping.
/// The framework retains closing content and removes its focus and input handlers.
///
/// Without a custom effect, the overlay fades. A custom effect replaces the content fade, and
/// receives the current progress each draw. It can return any [`VisualEffect`], including a
/// backdrop-aware [`CellEffect`](crate::style::CellEffect) that reveals the live layer beneath it.
/// The backdrop dim still follows visibility independently.
///
/// Give the modal a stable element key and place it directly in a `ZStack`, `Canvas`, `VStack`,
/// or `HStack` so the framework can retain it when removed.
#[derive(Clone)]
pub struct OverlayAnimation {
    pub(crate) enter: TransitionConfig,
    pub(crate) exit: TransitionConfig,
    effect: Option<Rc<dyn Fn(OverlayAnimationContext) -> VisualEffect>>,
}

impl Default for OverlayAnimation {
    fn default() -> Self {
        Self {
            enter: TransitionConfig {
                duration: Duration::from_millis(150),
                easing: Easing::EaseOutQuad,
            },
            exit: TransitionConfig {
                duration: Duration::from_millis(100),
                easing: Easing::EaseInQuad,
            },
            effect: None,
        }
    }
}

impl fmt::Debug for OverlayAnimation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OverlayAnimation")
            .field("enter", &self.enter)
            .field("exit", &self.exit)
            .field("custom_effect", &self.effect.is_some())
            .finish()
    }
}

impl OverlayAnimation {
    /// Fade in over 150 ms and out over 100 ms.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the duration and easing used to become visible.
    pub fn enter(mut self, config: TransitionConfig) -> Self {
        self.enter = config;
        self
    }

    /// Set the duration and easing used to disappear.
    pub fn exit(mut self, config: TransitionConfig) -> Self {
        self.exit = config;
        self
    }

    /// Replace the content fade with an effect evaluated at the current visibility each draw.
    ///
    /// The effect applies to the complete overlay frame, including title and border, after
    /// painting its children. A backdrop-reading custom effect sees the live cells beneath the
    /// overlay, after its backdrop dim. The factory runs only while visibility is below one;
    /// at one, content paints normally. Factories run on the UI thread and may capture `Rc` data.
    pub fn effect(
        mut self,
        effect: impl Fn(OverlayAnimationContext) -> VisualEffect + 'static,
    ) -> Self {
        self.effect = Some(Rc::new(effect));
        self
    }

    pub(crate) fn paints_effect(&self) -> bool {
        self.effect.is_some()
    }

    pub(crate) fn effect_at(&self, context: OverlayAnimationContext) -> Option<VisualEffect> {
        if context.progress >= 1.0 {
            return None;
        }
        self.effect.as_ref().map(|effect| effect(context))
    }
}

/// Node-owned timing survives view rebuilds and freezes the active recipe until it settles.
#[derive(Clone)]
pub(crate) struct OverlayAnimationState {
    pub recipe: OverlayAnimation,
    progress: f32,
    transition: Option<Transition<f32>>,
    closing: bool,
}

impl OverlayAnimationState {
    pub fn new(recipe: OverlayAnimation) -> Self {
        let mut state = Self {
            recipe,
            progress: 0.0,
            transition: None,
            closing: false,
        };
        state.start(false);
        state
    }

    pub fn reconcile(&mut self, recipe: &OverlayAnimation) {
        if self.closing || self.transition.is_none() {
            self.recipe = recipe.clone();
        }
        if self.closing {
            self.start(false);
        }
    }

    fn start(&mut self, closing: bool) {
        self.closing = closing;
        let config = if closing {
            self.recipe.exit
        } else {
            self.recipe.enter
        };
        let target = if closing { 0.0 } else { 1.0 };
        if config.duration.is_zero() {
            self.progress = target;
            self.transition = None;
        } else {
            self.transition = Some(Transition::new(
                self.progress,
                target,
                config.duration,
                config.easing,
            ));
        }
    }

    pub fn begin_exit(&mut self) -> Duration {
        self.start(true);
        self.recipe.exit.duration
    }

    pub fn progress(&self) -> f32 {
        self.progress.clamp(0.0, 1.0)
    }
    pub fn context(&self) -> OverlayAnimationContext {
        let phase = if self.closing {
            OverlayAnimationPhase::Exiting
        } else if self.is_animating() {
            OverlayAnimationPhase::Entering
        } else {
            OverlayAnimationPhase::Visible
        };
        OverlayAnimationContext::new(self.progress(), phase)
    }

    pub fn is_animating(&self) -> bool {
        self.transition.is_some()
    }
    pub fn is_closing(&self) -> bool {
        self.closing
    }
    pub fn exit_finished(&self) -> bool {
        self.closing && self.transition.is_none()
    }

    pub fn tick(&mut self, delta: Duration) -> bool {
        let Some(transition) = &mut self.transition else {
            return false;
        };
        let finished = transition.tick(delta);
        self.progress = transition.current();
        if finished {
            self.transition = None;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timing(ms: u64) -> TransitionConfig {
        TransitionConfig {
            duration: Duration::from_millis(ms),
            easing: Easing::Linear,
        }
    }

    #[test]
    fn closing_before_entry_finishes_preserves_visibility() {
        let mut state = OverlayAnimationState::new(
            OverlayAnimation::new().enter(timing(200)).exit(timing(100)),
        );
        state.tick(Duration::from_millis(50));
        assert_eq!(state.progress(), 0.25);
        state.begin_exit();
        assert_eq!(state.context().phase, OverlayAnimationPhase::Exiting);
        assert_eq!(state.progress(), 0.25);
        state.tick(Duration::from_millis(50));
        assert_eq!(state.progress(), 0.125);
        state.tick(Duration::from_millis(50));
        assert!(state.exit_finished());
        assert_eq!(state.progress(), 0.0);
    }

    #[test]
    fn rebuilt_recipes_cannot_replace_an_active_transition() {
        let mut state = OverlayAnimationState::new(OverlayAnimation::new().enter(timing(200)));
        state.tick(Duration::from_millis(50));
        let replacement = OverlayAnimation::new().enter(timing(0)).exit(timing(20));
        state.reconcile(&replacement);
        assert_eq!(state.progress(), 0.25);
        assert!(state.is_animating());
        state.tick(Duration::from_millis(150));
        assert_eq!(state.progress(), 1.0);
        state.reconcile(&replacement);
        assert_eq!(state.begin_exit(), Duration::from_millis(20));
        state.tick(Duration::from_millis(10));
        assert_eq!(state.progress(), 0.5);
    }
}
