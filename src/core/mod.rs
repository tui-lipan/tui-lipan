//! Core framework definitions.

/// Component trait and related types.
pub mod component;
/// Context value marker trait.
pub mod context_value;
/// Element (VDOM) and key types.
pub mod element;
#[cfg(debug_assertions)]
pub(crate) mod element_debug;
/// Event handling types.
pub mod event;
/// Cell bitmask shared by visual clipping and pointer hit testing.
pub mod mask;
/// Element-level memoization wrapper.
pub mod memo;
/// Nested component support.
pub(crate) mod nested;
/// Render node tree and layout node types.
pub mod node;
/// Shared runtime environment passed to every component.
pub(crate) mod runtime_env;
