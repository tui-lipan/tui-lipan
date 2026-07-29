//! Animation primitives.

pub mod easing;
pub mod exit_animation;
pub mod exit_queue;
pub(crate) mod registry;
pub mod transition;

pub use easing::{
    Easing, EasingFn, ease_in_out_cubic, ease_in_out_sine, ease_in_quad, ease_out_elastic,
    ease_out_quad, linear,
};
pub use exit_animation::ExitAnimation;
pub use exit_queue::{ExitQueue, ExitTransfer};
pub(crate) use registry::AnimationRegistry;
pub use transition::{Lerp, Transition, TransitionConfig};
