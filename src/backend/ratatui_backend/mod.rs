pub(crate) mod capture_render;
pub(crate) mod common;
pub(crate) mod glyph_paint_cache;
#[cfg(feature = "image")]
pub(crate) mod image_support;
pub(crate) mod render;
pub(crate) mod renderers;

#[cfg(not(target_arch = "wasm32"))]
mod native_terminal;
#[cfg(not(target_arch = "wasm32"))]
pub mod terminal_handoff;
#[cfg(not(target_arch = "wasm32"))]
mod terminal_transition;

#[cfg(not(target_arch = "wasm32"))]
pub(crate) use native_terminal::{
    Terminal, TerminalGuard, create_inline_terminal, restore_terminal_on_panic,
    set_mouse_all_motion_enabled, set_mouse_capture_enabled,
};

pub(crate) use render::{RenderContext, render, render_regions};

#[cfg(all(test, not(target_arch = "wasm32")))]
pub(crate) fn assert_inline_surface_internal_wrap_policy_is_opaque() {
    native_terminal::assert_inline_surface_internal_wrap_policy_is_opaque();
}
