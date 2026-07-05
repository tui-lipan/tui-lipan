//! Accordion types.

use crate::core::element::Element;
use std::sync::Arc;

/// An accordion section.
#[derive(Clone)]
pub struct AccordionItem {
    pub(crate) title: Arc<str>,
    pub(crate) content: Element,
    pub(crate) expanded: bool,
    pub(crate) disabled: bool,
}

impl AccordionItem {
    /// Create a new accordion item.
    pub fn new(title: impl Into<Arc<str>>, content: impl Into<Element>) -> Self {
        Self {
            title: title.into(),
            content: content.into(),
            expanded: false,
            disabled: false,
        }
    }

    /// Set content element.
    pub fn content(mut self, content: impl Into<crate::core::element::Element>) -> Self {
        self.content = content.into();
        self
    }

    /// Set expanded state.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}
