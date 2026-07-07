//! Input handling module.

pub mod command_registry;
#[cfg(not(target_arch = "wasm32"))]
pub mod convert;
pub mod drag;
pub mod focus;
pub(crate) mod geometry;
pub mod handlers;
pub mod hex_history;
pub(crate) mod key_dispatch;
pub mod keyboard;
pub mod keymap;
pub mod mouse;
pub(crate) mod runtime_dispatch;
pub mod scrollbar;
pub mod text;
pub(crate) mod text_area_vim;
