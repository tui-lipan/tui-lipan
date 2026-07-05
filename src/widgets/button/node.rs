use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::event::MouseEvent;
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{Align, BorderStyle, Padding, Style, StyleSlot, Theme, ThemeRole};

use super::{Button, ButtonVariant};

#[derive(Clone)]
pub struct ButtonNode {
    pub label: Arc<str>,
    pub icon: Option<Arc<str>>,
    pub icon_style: Style,
    pub icon_gap: u16,
    pub shortcut: Option<Arc<str>>,
    pub shortcut_style: Style,
    pub shortcut_gap: u16,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub focus_style: StyleSlot,
    pub align: Align,
    pub variant: ButtonVariant,
    pub border_style: BorderStyle,
    pub hover_border_style: Option<BorderStyle>,
    pub focus_border_style: Option<BorderStyle>,
    pub padding: Padding,
    pub disabled: bool,
    pub disabled_style: Style,
    pub on_click: Option<Callback<MouseEvent>>,
    pub on_key: Option<KeyHandler>,
    pub focusable: bool,
}

impl WidgetNode for ButtonNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }
    fn has_on_click(&self) -> bool {
        !self.disabled && self.on_click.is_some()
    }
    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        if self.has_on_click() {
            return true;
        }
        !self.disabled
            && (self
                .hover_style
                .resolves_non_empty(theme, ThemeRole::Accent)
                || self.hover_border_style.is_some())
    }
}

impl From<Button> for ButtonNode {
    fn from(button: Button) -> Self {
        Self {
            label: button.label,
            icon: button.icon,
            icon_style: button.icon_style,
            icon_gap: button.icon_gap,
            shortcut: button.shortcut,
            shortcut_style: button.shortcut_style,
            shortcut_gap: button.shortcut_gap,
            style: button.style,
            hover_style: button.hover_style,
            focus_style: button.focus_style,
            align: button.align,
            variant: button.variant,
            border_style: button.border_style,
            hover_border_style: button.hover_border_style,
            focus_border_style: button.focus_border_style,
            padding: button.padding,
            disabled: button.disabled,
            disabled_style: button.disabled_style,
            on_click: button.on_click,
            on_key: button.on_key,
            focusable: button.focusable,
        }
    }
}

impl From<ButtonNode> for NodeKind {
    fn from(node: ButtonNode) -> Self {
        NodeKind::Button(node)
    }
}
