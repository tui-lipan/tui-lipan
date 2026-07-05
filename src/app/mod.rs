//! Application runtime and event loop.

pub mod context;
pub(crate) mod copy_feedback;
pub mod input;
pub(crate) mod interaction_state;
pub(crate) mod mouse_dispatch;
#[cfg(not(target_arch = "wasm32"))]
pub mod runner;
#[cfg(all(target_arch = "wasm32", feature = "web"))]
pub mod web_runner;

#[cfg(feature = "devtools")]
pub use context::DevToolsConfig;
pub use context::{
    App, ContrastPolicy, InlineStartupPolicy, ScreenBackground, SurfaceMode, TextAreaNewlineBinding,
};
#[cfg(not(target_arch = "wasm32"))]
pub use runner::AppRunner;
