//! Typed context value marker trait.

/// Values that can be provided through `ContextProvider<T>` and consumed via
/// `Context::use_context::<T>()`.
pub trait ContextValue: Clone + PartialEq + 'static {}

impl<T> ContextValue for T where T: Clone + PartialEq + 'static {}
