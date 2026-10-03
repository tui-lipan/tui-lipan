//! Shared visibility transitions for overlays and inline content.

use std::fmt;
use std::rc::Rc;
use std::time::Duration;

use super::{Easing, Transition, TransitionConfig};
use crate::style::VisualEffect;

/// Direction of a content visibility transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum VisibilityAnimationPhase {
    /// Becoming visible, including reversal of an unfinished exit.
    Entering,
    /// Fully visible with no lifecycle transition running.
    Visible,
    /// Becoming hidden while retained for its closing transition.
    Exiting,
}

/// Inputs to a visibility animation's effect factory.
#[derive(Clone, Copy, Debug)]
#[non_exhaustive]
pub struct VisibilityAnimationContext {
    /// Current visibility, clamped to `[0, 1]`, independent of direction.
    pub progress: f32,
    /// Whether the content is entering, visible, or exiting.
    pub phase: VisibilityAnimationPhase,
}

impl VisibilityAnimationContext {
    /// Construct inputs for testing an application's effect factory.
    pub fn new(progress: f32, phase: VisibilityAnimationPhase) -> Self {
        Self {
            progress: progress.clamp(0.0, 1.0),
            phase,
        }
    }
}

/// Entry and exit timing for visible content, with an optional application-defined paint effect.
///
/// Progress describes visibility: `0.0` is hidden and `1.0` is fully visible. Enter runs toward
/// one, exit toward zero. Reopening during exit reverses from the current value without snapping.
/// The framework retains closing content and removes its focus and input handlers.
///
/// Without a custom effect, the content fades. A custom effect replaces the content fade, and
/// receives the current progress each draw. It can return any [`VisualEffect`], including a
/// backdrop-aware [`CellEffect`](crate::style::CellEffect) that reveals the live layer beneath it.
/// Overlay backdrop dim follows visibility independently. Inline hosts may reflow height as well.
///
/// For Modal removal, give the element a stable key under a supported `ZStack`, `Canvas`,
/// `VStack`, or `HStack`. For controlled hosts such as Popover and Animated, keep the host mounted
/// and toggle its visibility property.
#[derive(Clone)]
pub struct VisibilityAnimation {
    pub(crate) enter: TransitionConfig,
    pub(crate) exit: TransitionConfig,
    effect: Option<Rc<dyn Fn(VisibilityAnimationContext) -> VisualEffect>>,
}

impl PartialEq for VisibilityAnimation {
    fn eq(&self, other: &Self) -> bool {
        self.enter.duration == other.enter.duration
            && self.enter.easing == other.enter.easing
            && self.exit.duration == other.exit.duration
            && self.exit.easing == other.exit.easing
            && match (&self.effect, &other.effect) {
                (None, None) => true,
                (Some(left), Some(right)) => Rc::ptr_eq(left, right),
                _ => false,
            }
    }
}

impl Default for VisibilityAnimation {
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

impl fmt::Debug for VisibilityAnimation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VisibilityAnimation")
            .field("enter", &self.enter)
            .field("exit", &self.exit)
            .field("custom_effect", &self.effect.is_some())
            .finish()
    }
}

impl VisibilityAnimation {
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
    /// The effect applies to the complete host content, including its title and border, after
    /// painting its children. A backdrop-reading custom effect sees the live cells beneath the
    /// overlay, after its backdrop dim. The factory runs only while visibility is below one;
    /// at one, content paints normally. Factories run on the UI thread and may capture `Rc` data.
    pub fn effect(
        mut self,
        effect: impl Fn(VisibilityAnimationContext) -> VisualEffect + 'static,
    ) -> Self {
        self.effect = Some(Rc::new(effect));
        self
    }

    pub(crate) fn paints_effect(&self) -> bool {
        self.effect.is_some()
    }

    pub(crate) fn effect_at(&self, context: VisibilityAnimationContext) -> Option<VisualEffect> {
        if context.progress >= 1.0 {
            return None;
        }
        self.effect.as_ref().map(|effect| effect(context))
    }
}

/// Node-owned timing survives view rebuilds and freezes the active recipe until it settles.
#[derive(Clone)]
pub(crate) struct VisibilityAnimationState {
    pub recipe: VisibilityAnimation,
    progress: f32,
    transition: Option<Transition<f32>>,
    closing: bool,
}

impl VisibilityAnimationState {
    pub fn new(recipe: VisibilityAnimation) -> Self {
        let mut state = Self {
            recipe,
            progress: 0.0,
            transition: None,
            closing: false,
        };
        state.start(false);
        state
    }

    pub fn reconcile(&mut self, recipe: &VisibilityAnimation) {
        self.set_visible(true, recipe);
    }

    pub fn for_visibility(recipe: VisibilityAnimation, visible: bool) -> Self {
        if visible {
            return Self::new(recipe);
        }
        Self {
            recipe,
            progress: 0.0,
            transition: None,
            closing: true,
        }
    }

    pub fn set_visible(&mut self, visible: bool, recipe: &VisibilityAnimation) {
        let target_closing = !visible;
        let changing_direction = self.closing != target_closing;
        if self.transition.is_none() || changing_direction {
            self.recipe = recipe.clone();
        }
        if changing_direction {
            self.start(target_closing);
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
    pub fn context(&self) -> VisibilityAnimationContext {
        let phase = if self.closing {
            VisibilityAnimationPhase::Exiting
        } else if self.is_animating() {
            VisibilityAnimationPhase::Entering
        } else {
            VisibilityAnimationPhase::Visible
        };
        VisibilityAnimationContext::new(self.progress(), phase)
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
        let mut state = VisibilityAnimationState::new(
            VisibilityAnimation::new()
                .enter(timing(200))
                .exit(timing(100)),
        );
        state.tick(Duration::from_millis(50));
        assert_eq!(state.progress(), 0.25);
        state.begin_exit();
        assert_eq!(state.context().phase, VisibilityAnimationPhase::Exiting);
        assert_eq!(state.progress(), 0.25);
        state.tick(Duration::from_millis(50));
        assert_eq!(state.progress(), 0.125);
        state.tick(Duration::from_millis(50));
        assert!(state.exit_finished());
        assert_eq!(state.progress(), 0.0);
    }

    #[test]
    fn rebuilt_recipes_cannot_replace_an_active_transition() {
        let mut state =
            VisibilityAnimationState::new(VisibilityAnimation::new().enter(timing(200)));
        state.tick(Duration::from_millis(50));
        let replacement = VisibilityAnimation::new().enter(timing(0)).exit(timing(20));
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
