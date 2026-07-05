//! Radio widget.

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::Element;
use crate::core::event::MouseEvent;
use crate::style::{Length, Padding, Style};
use crate::widgets::{Checkbox, CheckboxEvent, CheckboxVariant, HStack, VStack};

/// Layout direction for radio groups.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum RadioLayout {
    /// Stack options vertically.
    #[default]
    Vertical,
    /// Stack options horizontally.
    Horizontal,
}

/// A radio button group.
#[derive(Clone)]
pub struct Radio {
    options: Vec<Arc<str>>,
    selected: Option<usize>,
    on_change: Option<Callback<usize>>,
    disabled: bool,
    gap: u16,
    layout: RadioLayout,
    variant: CheckboxVariant,
    style: Style,
    hover_style: Style,
    focus_style: Style,
    checked_style: Style,
    unchecked_style: Style,
    label_style: Style,
    padding: Padding,
    width: Length,
    height: Length,
    disabled_style: Style,
}

impl Radio {
    /// Create a new radio group.
    pub fn new(options: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        Self {
            options: options.into_iter().map(Into::into).collect(),
            selected: None,
            on_change: None,
            disabled: false,
            gap: 0,
            layout: RadioLayout::Vertical,
            variant: CheckboxVariant::Circle,
            style: Style::default(),
            hover_style: Style::default(),
            focus_style: Style::default(),
            checked_style: Style::default(),
            unchecked_style: Style::default(),
            label_style: Style::default(),
            padding: Padding::default(),
            width: Length::Auto,
            height: Length::Auto,
            disabled_style: Style::default(),
        }
    }

    /// Set selected index.
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Set on-change callback.
    pub fn on_change(mut self, cb: Callback<usize>) -> Self {
        self.on_change = Some(cb);
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set gap between options.
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Set layout direction.
    pub fn layout(mut self, layout: RadioLayout) -> Self {
        self.layout = layout;
        self
    }

    /// Set checkbox variant.
    pub fn variant(mut self, variant: CheckboxVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set hover style.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = style;
        self
    }

    /// Set focus style.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = style;
        self
    }

    /// Set checked style.
    pub fn checked_style(mut self, style: Style) -> Self {
        self.checked_style = style;
        self
    }

    /// Set unchecked style.
    pub fn unchecked_style(mut self, style: Style) -> Self {
        self.unchecked_style = style;
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

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = style;
        self
    }
}

impl From<Radio> for Element {
    fn from(radio: Radio) -> Self {
        let items: Vec<Element> = radio
            .options
            .into_iter()
            .enumerate()
            .map(|(i, option)| {
                let is_selected = radio.selected == Some(i);
                let on_change = radio.on_change.clone();

                let mut checkbox = Checkbox::new(is_selected)
                    .label(option)
                    .variant(radio.variant)
                    .gap(1)
                    .style(radio.style)
                    .hover_style(radio.hover_style)
                    .focus_style(radio.focus_style)
                    .checked_style(radio.checked_style)
                    .unchecked_style(radio.unchecked_style)
                    .label_style(radio.label_style)
                    .padding(radio.padding)
                    .width(radio.width)
                    .height(radio.height)
                    .disabled(radio.disabled)
                    .disabled_style(radio.disabled_style);

                if let Some(cb) = on_change
                    && !radio.disabled
                {
                    let cb_toggle = cb.clone();
                    checkbox = checkbox.on_toggle(Callback::new(move |ev: CheckboxEvent| {
                        if ev.state.is_checked() {
                            cb_toggle.emit(i);
                        }
                    }));

                    checkbox = checkbox.on_click(Callback::new(move |_: MouseEvent| {
                        cb.emit(i);
                    }));
                }

                checkbox.into()
            })
            .collect();

        match radio.layout {
            RadioLayout::Vertical => {
                let mut stack = VStack::new().gap(radio.gap);
                for item in items {
                    stack = stack.child(item);
                }
                stack.into()
            }
            RadioLayout::Horizontal => {
                let mut stack = HStack::new().gap(radio.gap);
                for item in items {
                    stack = stack.child(item);
                }
                stack.into()
            }
        }
    }
}
