//! Theme provider widget.

use crate::core::element::{Element, ElementKind};
use crate::style::Theme;

/// Provide a theme for a subtree.
#[derive(Clone)]
pub struct ThemeProvider {
    theme: Theme,
    child: Element,
}

impl ThemeProvider {
    /// Create a new theme provider.
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            child: crate::widgets::Spacer::new().into(),
        }
    }

    /// Set child content.
    pub fn child(mut self, child: impl Into<Element>) -> Self {
        self.child = child.into();
        self
    }
}

impl From<ThemeProvider> for Element {
    fn from(provider: ThemeProvider) -> Self {
        use crate::core::element::ThemeProviderElement;
        Element::new(ElementKind::ThemeProvider(Box::new(ThemeProviderElement {
            theme: provider.theme,
            child: provider.child,
        })))
    }
}
