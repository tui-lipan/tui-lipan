//! Application runtime and event loop.

pub(crate) mod animation;
pub(crate) mod caret;
pub mod context;
pub(crate) mod copy_feedback;
pub mod cursor_trail;
pub(crate) mod focus_service;
pub mod input;
pub(crate) mod interaction_state;
pub(crate) mod job_control;
pub(crate) mod mouse_dispatch;
pub(crate) mod pointer;
#[cfg(not(target_arch = "wasm32"))]
pub mod runner;
pub(crate) mod selection_clear;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web_runner;

#[cfg(feature = "devtools")]
pub use context::DevToolsConfig;
pub use context::{
    App, ContrastPolicy, DEFAULT_COLOR_ANIMATION_FRAME_RATE, DEFAULT_FRAME_RATE, DevToolsMetric,
    FocusChanged, FocusEntry, FocusPolicy, InlineHeight, InlineStartupPolicy, ScreenBackground,
    SurfaceMode, TextAreaNewlineBinding,
};
pub use cursor_trail::{
    CursorTrail, CursorTrailFill, CursorTrailGlyphs, CursorTrailMotion, CursorTrailParticles,
    CursorTrailSettle, CursorTrailSpring, CursorTrailTrigger,
};
#[cfg(not(target_arch = "wasm32"))]
pub use runner::AppRunner;
