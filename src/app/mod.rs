//! Application runtime and event loop.

pub(crate) mod animation;
pub mod context;
pub(crate) mod copy_feedback;
pub(crate) mod focus_service;
pub mod input;
pub(crate) mod interaction_state;
pub(crate) mod job_control;
pub(crate) mod mouse_dispatch;
#[cfg(not(target_arch = "wasm32"))]
pub mod runner;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web_runner;

#[cfg(feature = "devtools")]
pub use context::DevToolsConfig;
pub use context::{
    App, ContrastPolicy, DEFAULT_COLOR_ANIMATION_FRAME_RATE, DEFAULT_FRAME_RATE, DevToolsMetric,
    FocusChanged, FocusEntry, FocusPolicy, InlineHeight, InlineStartupPolicy, ScreenBackground,
    SurfaceMode, TextAreaNewlineBinding,
};
#[cfg(not(target_arch = "wasm32"))]
pub use runner::AppRunner;
