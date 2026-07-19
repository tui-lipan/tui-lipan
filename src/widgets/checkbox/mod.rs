//! Checkbox widget.

mod layout;
mod node;
mod reconcile;

pub use layout::measure_checkbox;
pub use node::CheckboxNode;
pub use reconcile::reconcile_checkbox;

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::core::event::MouseEvent;
use crate::style::{Length, Padding, Style, StyleSlot};

/// Visual variant for a [`Checkbox`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CheckboxVariant {
    /// Rendered as `[x]` / `[ ]`.
    #[default]
    Bracket,
    /// Rendered as `◉` / `○`.
    Circle,
    /// Rendered as `☐` with `✓` when checked - modern box style.
    Box,
    /// Custom variant with user-defined strings.
    Custom {
        /// String for checked state.
        checked: &'static str,
        /// String for unchecked state.
        unchecked: &'static str,
        /// String for indeterminate state.
        indeterminate: &'static str,
    },
}

impl CheckboxVariant {
    /// Get the display string for checked state.
    pub fn checked_str(self) -> &'static str {
        match self {
            Self::Bracket => "[x]",
            Self::Circle => "◉",
            Self::Box => "✓",
            Self::Custom { checked, .. } => checked,
        }
    }

    /// Get the display string for unchecked state.
    pub fn unchecked_str(self) -> &'static str {
        match self {
            Self::Bracket => "[ ]",
            Self::Circle => "○",
            Self::Box => "☐",
            Self::Custom { unchecked, .. } => unchecked,
        }
    }

    /// Get the display string for indeterminate state.
    pub fn indeterminate_str(self) -> &'static str {
        match self {
            Self::Bracket => "[-]",
            Self::Circle => "◍",
            Self::Box => "▣",
            Self::Custom { indeterminate, .. } => indeterminate,
        }
    }

    /// Get the width of the checkbox symbol (in cells).
    pub fn width(self) -> u16 {
        use unicode_width::UnicodeWidthStr;
        match self {
            Self::Bracket => 3,
            Self::Circle | Self::Box => 1,
            Self::Custom {
                checked,
                unchecked,
                indeterminate,
            } => UnicodeWidthStr::width(checked)
                .max(UnicodeWidthStr::width(unchecked))
                .max(UnicodeWidthStr::width(indeterminate)) as u16,
        }
    }
}

/// Checkbox state.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CheckboxState {
    /// Unchecked state.
    Unchecked,
    /// Checked state.
    Checked,
    /// Indeterminate state.
    Indeterminate,
}

impl CheckboxState {
    /// Return true if checked.
    pub fn is_checked(self) -> bool {
        matches!(self, Self::Checked)
    }

    /// Return true if indeterminate.
    pub fn is_indeterminate(self) -> bool {
        matches!(self, Self::Indeterminate)
    }

    /// Toggle the state (indeterminate -> checked).
    pub fn toggle(self) -> Self {
        match self {
            Self::Checked => Self::Unchecked,
            Self::Unchecked | Self::Indeterminate => Self::Checked,
        }
    }
}

/// A checkbox toggle event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CheckboxEvent {
    /// New checkbox state after toggle.
    pub state: CheckboxState,
}

/// A checkbox widget for boolean values.
#[derive(Clone)]
pub struct Checkbox {
    /// Checkbox state.
    pub state: CheckboxState,
    /// Optional label displayed next to the checkbox.
    pub label: Option<Arc<str>>,
    /// Visual variant.
    pub variant: CheckboxVariant,
    /// Gap between checkbox symbol and label.
    pub gap: u16,
    /// Base style.
    pub style: Style,
    /// Style applied when hovered.
    pub hover_style: StyleSlot,
    /// Style applied when focused.
    pub focus_style: StyleSlot,
    /// Style for the checked state symbol.
    pub checked_style: Style,
    /// Style for the unchecked state symbol.
    pub unchecked_style: Style,
    /// Style for the indeterminate state symbol.
    pub indeterminate_style: Style,
    /// Label style.
    pub label_style: Style,
    /// Padding.
    /// Default: `Padding::default()`.
    pub padding: Padding,
    /// Whether the checkbox is disabled.
    pub disabled: bool,
    /// Style applied when disabled.
    pub disabled_style: Style,
    /// Requested width.
    /// Default: `Length::Auto`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Auto`.
    pub height: Length,
    /// Toggle handler.
    pub on_toggle: Option<Callback<CheckboxEvent>>,
    /// Mouse click handler.
    pub on_click: Option<Callback<MouseEvent>>,
    /// Keyboard handler.
    pub on_key: Option<KeyHandler>,
    /// Whether the checkbox participates in focus traversal.
    pub focusable: bool,
    /// Whether the checkbox participates in tab traversal when focusable.
    pub tab_stop: bool,
    /// Callback fired when the checkbox gains focus.
    pub on_focus: Option<Callback<()>>,
    /// Callback fired when the checkbox loses focus.
    pub on_blur: Option<Callback<()>>,
}

impl Checkbox {
    /// Create a new checkbox.
    pub fn new(checked: bool) -> Self {
        Self {
            state: if checked {
                CheckboxState::Checked
            } else {
                CheckboxState::Unchecked
            },
            label: None,
            variant: CheckboxVariant::Bracket,
            gap: 1,
            style: Style::default(),
            hover_style: StyleSlot::Inherit,
            focus_style: StyleSlot::Inherit,
            checked_style: Style::default(),
            unchecked_style: Style::default(),
            indeterminate_style: Style::default(),
            label_style: Style::default(),
            padding: Padding::default(),
            disabled: false,
            disabled_style: Style::default(),
            width: Length::Auto,
            height: Length::Auto,
            on_toggle: None,
            on_click: None,
            on_key: None,
            focusable: true,
            tab_stop: true,
            on_focus: None,
            on_blur: None,
        }
    }

    /// Set the checkbox state.
    pub fn state(mut self, state: CheckboxState) -> Self {
        self.state = state;
        self
    }

    /// Set the checked state.
    pub fn checked(mut self, checked: bool) -> Self {
        self.state = if checked {
            CheckboxState::Checked
        } else {
            CheckboxState::Unchecked
        };
        self
    }

    /// Set indeterminate state.
    pub fn indeterminate(mut self, indeterminate: bool) -> Self {
        if indeterminate {
            self.state = CheckboxState::Indeterminate;
        } else if self.state.is_indeterminate() {
            self.state = CheckboxState::Unchecked;
        }
        self
    }

    /// Set the label.
    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Set the visual variant.
    pub fn variant(mut self, variant: CheckboxVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set the gap between checkbox and label.
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set hover style.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hover style with the given style.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit hover style from the active theme.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set the hover style slot directly.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_style = slot;
        self
    }

    /// Set focus style.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed focus style with the given style.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit focus style from the active theme.
    pub fn inherit_focus_style(mut self) -> Self {
        self.focus_style = StyleSlot::Inherit;
        self
    }

    /// Set the focus style slot directly.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_style = slot;
        self
    }

    /// Set checked state symbol style.
    pub fn checked_style(mut self, style: Style) -> Self {
        self.checked_style = style;
        self
    }

    /// Set unchecked state symbol style.
    pub fn unchecked_style(mut self, style: Style) -> Self {
        self.unchecked_style = style;
        self
    }

    /// Set indeterminate state symbol style.
    pub fn indeterminate_style(mut self, style: Style) -> Self {
        self.indeterminate_style = style;
        self
    }

    /// Set label style.
    pub fn label_style(mut self, style: Style) -> Self {
        self.label_style = style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }

    /// Set requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set toggle handler.
    pub fn on_toggle(mut self, cb: Callback<CheckboxEvent>) -> Self {
        self.on_toggle = Some(cb);
        self
    }

    /// Set mouse click handler.
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set keyboard handler.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }

    /// Control whether the node is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Control whether the checkbox participates in tab traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the checkbox gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the checkbox loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
        self
    }
}

impl From<Checkbox> for Element {
    fn from(value: Checkbox) -> Self {
        Element::new(ElementKind::Checkbox(value))
    }
}

impl crate::layout::hash::LayoutHash for Checkbox {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.variant.hash(hasher);
        self.gap.hash(hasher);
        self.padding.hash(hasher);
        self.label.hash(hasher);
        Some(())
    }
}
