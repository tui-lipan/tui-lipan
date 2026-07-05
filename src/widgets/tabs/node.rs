use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::event::MouseEvent;
use crate::core::node::WidgetNode;
use crate::style::{BorderStyle, Padding, Style, StyleSlot, Theme, ThemeRole};

use super::{Tab, Tabs, TabsEvent, TabsOverflow};

#[derive(Clone)]
pub struct TabsNode {
    pub tabs: Arc<[Tab]>,
    pub active: usize,
    pub style: Style,
    pub focus_style: StyleSlot,
    pub hover_style: StyleSlot,
    pub tab_hover_style: StyleSlot,
    pub active_style: StyleSlot,
    pub divider: char,
    pub overflow: TabsOverflow,
    pub border: bool,
    pub border_style: BorderStyle,
    pub padding: Padding,
    pub disabled: bool,
    pub disabled_style: Style,
    pub on_change: Option<Callback<TabsEvent>>,
    pub on_click: Option<Callback<MouseEvent>>,
    pub on_key: Option<KeyHandler>,
    pub focusable: bool,
}

impl WidgetNode for TabsNode {
    fn is_focusable(&self) -> bool {
        self.focusable
    }

    fn has_on_click(&self) -> bool {
        !self.disabled && (self.on_click.is_some() || self.on_change.is_some())
    }

    fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
        // Only hoverable if explicitly styled for hover, or has an on_click handler.
        // Having on_change alone does not make the widget hoverable since clicking
        // changes the active tab without needing visual hover feedback.
        if !self.disabled && self.on_click.is_some() {
            return true;
        }
        !self.disabled
            && (self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
                || self
                    .tab_hover_style
                    .resolves_non_empty(theme, ThemeRole::ItemHover))
    }
}

impl From<Tabs> for TabsNode {
    fn from(value: Tabs) -> Self {
        Self {
            tabs: value.tabs,
            active: value.active,
            style: value.style,
            focus_style: value.focus_style,
            hover_style: value.hover_style,
            tab_hover_style: value.tab_hover_style,
            active_style: value.active_style,
            divider: value.divider,
            overflow: value.overflow,
            border: value.border,
            border_style: value.border_style,
            padding: value.padding,
            disabled: value.disabled,
            disabled_style: value.disabled_style,
            on_change: value.on_change,
            on_click: value.on_click,
            on_key: value.on_key,
            focusable: value.focusable,
        }
    }
}
