//! Animation primitives.

pub mod easing;
pub mod exit_animation;
pub mod exit_queue;
pub(crate) mod registry;
pub mod transition;

pub use easing::{
    Easing, EasingFn, MAX_BACK_OVERSHOOT_PERMILLE, STANDARD_BACK_OVERSHOOT_PERMILLE,
    ease_in_out_cubic, ease_in_out_sine, ease_in_quad, ease_out_back, ease_out_elastic,
    ease_out_quad, linear,
};
pub use exit_animation::ExitAnimation;
pub use exit_queue::{ExitQueue, ExitTransfer};
pub(crate) use registry::AnimationRegistry;
pub use transition::{Lerp, Transition, TransitionConfig};

#[cfg(test)]
mod tests {
    #[test]
    fn ease_out_back_is_available_from_animation_facade() {
        let curve = super::ease_out_back(0.5, super::STANDARD_BACK_OVERSHOOT_PERMILLE);
        assert!(curve > 1.0);
        assert_eq!(super::MAX_BACK_OVERSHOOT_PERMILLE, 500);
    }
}
