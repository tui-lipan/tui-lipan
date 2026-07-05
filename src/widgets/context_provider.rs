//! Typed context provider widget.

use crate::core::context_value::ContextValue;
use crate::core::element::{ContextProviderElement, Element, ElementKind};

/// Provide a typed context value for a subtree.
#[derive(Clone)]
pub struct ContextProvider<T>
where
    T: ContextValue,
{
    value: T,
    child: Element,
}

impl<T> ContextProvider<T>
where
    T: ContextValue,
{
    /// Create a new typed context provider.
    pub fn new(value: T) -> Self {
        Self {
            value,
            child: crate::widgets::Spacer::new().into(),
        }
    }

    /// Set child content.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = child.into();
        self
    }
}

impl<T> From<ContextProvider<T>> for Element
where
    T: ContextValue,
{
    fn from(provider: ContextProvider<T>) -> Self {
        Element::new(ElementKind::ContextProvider(Box::new(
            ContextProviderElement::new(provider.value, provider.child),
        )))
    }
}
