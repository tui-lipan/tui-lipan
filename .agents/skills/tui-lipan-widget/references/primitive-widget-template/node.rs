use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::event::MouseEvent;
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::{Align, BorderStyle, Padding, Style};

use super::{#Name#, #Name#Variant};

#[derive(Clone)]
pub struct #Name#Node {
    pub label: Arc<str>,
    pub style: Style,
    pub hover_style: Style,
    pub focus_style: Style,
    pub disabled_style: Style,
    pub align: Align,
    pub variant: #Name#Variant,
    pub border_style: BorderStyle,
    pub hover_border_style: Option<BorderStyle>,
    pub focus_border_style: Option<BorderStyle>,
    pub padding: Padding,
    pub disabled: bool,
    pub focusable: bool,
    pub on_click: Option<Callback<MouseEvent>>,
    pub on_key: Option<KeyHandler>,
}

impl WidgetNode for #Name#Node {
    fn is_focusable(&self) -> bool {
        self.focusable && !self.disabled
    }
    
    fn has_on_click(&self) -> bool {
        !self.disabled && self.on_click.is_some()
    }
    
    fn is_hoverable(&self) -> bool {
        if self.has_on_click() {
            return true;
        }
        !self.disabled && (!self.hover_style.is_empty() || self.hover_border_style.is_some())
    }
}

impl From<#Name#> for #Name#Node {
    fn from(widget: #Name#) -> Self {
        Self {
            label: widget.label,
            style: widget.style,
            hover_style: widget.hover_style,
            focus_style: widget.focus_style,
            disabled_style: widget.disabled_style,
            align: widget.align,
            variant: widget.variant,
            border_style: widget.border_style,
            hover_border_style: widget.hover_border_style,
            focus_border_style: widget.focus_border_style,
            padding: widget.padding,
            disabled: widget.disabled,
            focusable: widget.focusable,
            on_click: widget.on_click,
            on_key: widget.on_key,
        }
    }
}

impl From<#Name#Node> for NodeKind {
    fn from(node: #Name#Node) -> Self {
        NodeKind::#Name#(node)
    }
}
