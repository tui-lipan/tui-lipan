use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::event::MouseEvent;
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{Padding, Style, StyleSlot, Theme, ThemeRole};

use super::{Checkbox, CheckboxEvent, CheckboxState, CheckboxVariant};

#[derive(Clone)]
pub struct CheckboxNode {
    pub state: CheckboxState,
    pub label: Option<Arc<str>>,
    pub variant: CheckboxVariant,
    pub gap: u16,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub focus_style: StyleSlot,
    pub checked_style: Style,
    pub unchecked_style: Style,
    pub indeterminate_style: Style,
    pub label_style: Style,
    pub padding: Padding,
    pub disabled: bool,
    pub disabled_style: Style,
    pub on_toggle: Option<Callback<CheckboxEvent>>,
    pub on_click: Option<Callback<MouseEvent>>,
    pub on_key: Option<KeyHandler>,
    pub focusable: bool,
}

impl WidgetNode for CheckboxNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }
    fn has_on_click(&self) -> bool {
        !self.disabled && (self.on_click.is_some() || self.on_toggle.is_some())
    }
    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        // Only hoverable if explicitly styled for hover, or has an on_click handler.
        // Having on_toggle alone does not make the widget hoverable since clicking
        // toggles the checkbox without needing visual hover feedback.
        if !self.disabled && self.on_click.is_some() {
            return true;
        }
        !self.disabled && self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
    }
}

impl From<Checkbox> for CheckboxNode {
    fn from(checkbox: Checkbox) -> Self {
        Self {
            state: checkbox.state,
            label: checkbox.label,
            variant: checkbox.variant,
            gap: checkbox.gap,
            style: checkbox.style,
            hover_style: checkbox.hover_style,
            focus_style: checkbox.focus_style,
            checked_style: checkbox.checked_style,
            unchecked_style: checkbox.unchecked_style,
            indeterminate_style: checkbox.indeterminate_style,
            label_style: checkbox.label_style,
            padding: checkbox.padding,
            disabled: checkbox.disabled,
            disabled_style: checkbox.disabled_style,
            on_toggle: checkbox.on_toggle,
            on_click: checkbox.on_click,
            on_key: checkbox.on_key,
            focusable: checkbox.focusable,
        }
    }
}

impl From<CheckboxNode> for NodeKind {
    fn from(node: CheckboxNode) -> Self {
        NodeKind::Checkbox(node)
    }
}
