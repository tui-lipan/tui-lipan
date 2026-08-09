//! Radio widget.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use rustc_hash::FxHasher;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, IntoElement};
use crate::core::event::{KeyCode, KeyEvent, MouseEvent};
use crate::style::{Length, Padding, Style, StyleSlot};
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
///
/// Keyboard model (WAI-ARIA radio group / roving tabindex):
/// - The selected option (or the first option when none is selected) is the
///   sole tab stop and focus entry.
/// - Other options stay mouse-activatable but are not focusable.
/// - Arrow keys move the selection (and focus follows via [`Radio::focus_key`]).
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
    hover_style: StyleSlot,
    focus_style: StyleSlot,
    checked_style: Style,
    unchecked_style: Style,
    label_style: Style,
    padding: Padding,
    width: Length,
    height: Length,
    disabled_style: Style,
    focusable: bool,
    tab_stop: bool,
    focus_key: Option<Arc<str>>,
    on_focus: Option<Callback<usize>>,
    on_blur: Option<Callback<usize>>,
    on_key: Option<KeyHandler>,
}

fn derived_focus_key(options: &[Arc<str>]) -> Arc<str> {
    let mut hasher = FxHasher::default();
    options.len().hash(&mut hasher);
    for option in options {
        option.hash(&mut hasher);
    }
    Arc::from(format!("__tui_lipan_radio:{:016x}", hasher.finish()))
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
            hover_style: StyleSlot::Inherit,
            focus_style: StyleSlot::Inherit,
            checked_style: Style::default(),
            unchecked_style: Style::default(),
            label_style: Style::default(),
            padding: Padding::default(),
            width: Length::Auto,
            height: Length::Auto,
            disabled_style: Style::default(),
            focusable: true,
            tab_stop: true,
            focus_key: None,
            on_focus: None,
            on_blur: None,
            on_key: None,
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

    /// Set hover style slot directly for composite forwarding.
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

    /// Set focus style slot directly for composite forwarding.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_style = slot;
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

    /// Control whether the active option is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Control whether the active option participates in tab traversal.
    ///
    /// Non-active options are never tab stops (roving focus).
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Key applied to the active option so focus follows arrow-driven selection.
    ///
    /// When omitted, a key is derived from the option labels so distinct groups
    /// in the same tree do not collide. Override when two groups share the same
    /// labels (or to give the group a stable app-owned key).
    pub fn focus_key(mut self, key: impl Into<Arc<str>>) -> Self {
        self.focus_key = Some(key.into());
        self
    }

    /// Set the callback fired when the active option gains focus.
    pub fn on_focus(mut self, cb: Callback<usize>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the active option loses focus.
    pub fn on_blur(mut self, cb: Callback<usize>) -> Self {
        self.on_blur = Some(cb);
        self
    }

    /// Set focused key handler on the active option (runs before built-in arrows).
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }
}

impl From<Radio> for Element {
    fn from(radio: Radio) -> Self {
        let len = radio.options.len();
        let active = radio
            .selected
            .filter(|&i| i < len)
            .or_else(|| (len > 0).then_some(0));
        let focus_key = radio
            .focus_key
            .clone()
            .unwrap_or_else(|| derived_focus_key(&radio.options));

        let items: Vec<Element> = radio
            .options
            .into_iter()
            .enumerate()
            .map(|(i, option)| {
                let is_selected = radio.selected == Some(i);
                let is_active = active == Some(i);
                let on_change = radio.on_change.clone();

                let mut checkbox = Checkbox::new(is_selected)
                    .label(option)
                    .variant(radio.variant)
                    .gap(1)
                    .style(radio.style)
                    .hover_style_slot(radio.hover_style)
                    .focus_style_slot(radio.focus_style)
                    .checked_style(radio.checked_style)
                    .unchecked_style(radio.unchecked_style)
                    .label_style(radio.label_style)
                    .padding(radio.padding)
                    .width(radio.width)
                    .height(radio.height)
                    .disabled(radio.disabled)
                    .disabled_style(radio.disabled_style)
                    .focusable(radio.focusable && is_active && !radio.disabled)
                    .tab_stop(radio.tab_stop && is_active && !radio.disabled);

                if is_active {
                    if let Some(cb) = radio.on_focus.clone() {
                        checkbox = checkbox.on_focus(Callback::new(move |_| cb.emit(i)));
                    }
                    if let Some(cb) = radio.on_blur.clone() {
                        checkbox = checkbox.on_blur(Callback::new(move |_| cb.emit(i)));
                    }
                }

                if is_active && !radio.disabled {
                    let change_cb = radio.on_change.clone();
                    let caller_on_key = radio.on_key.clone();
                    let layout = radio.layout;
                    checkbox = checkbox.on_key(KeyHandler::new(move |key: KeyEvent| {
                        if caller_on_key
                            .as_ref()
                            .is_some_and(|handler| handler.handle(key))
                        {
                            return true;
                        }
                        handle_radio_key(key, i, len, layout, &change_cb)
                    }));
                }

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

                if is_active {
                    checkbox.key(focus_key.clone())
                } else {
                    checkbox.into()
                }
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

fn handle_radio_key(
    key: KeyEvent,
    current: usize,
    len: usize,
    layout: RadioLayout,
    on_change: &Option<Callback<usize>>,
) -> bool {
    if len == 0 || key.mods.ctrl || key.mods.alt || key.mods.super_key {
        return false;
    }

    let next = match (layout, key.code) {
        (RadioLayout::Vertical, KeyCode::Up | KeyCode::Left)
        | (RadioLayout::Horizontal, KeyCode::Left | KeyCode::Up) => {
            Some(current.checked_sub(1).unwrap_or(len - 1))
        }
        (RadioLayout::Vertical, KeyCode::Down | KeyCode::Right)
        | (RadioLayout::Horizontal, KeyCode::Right | KeyCode::Down) => Some((current + 1) % len),
        (_, KeyCode::Home) => Some(0),
        (_, KeyCode::End) => Some(len - 1),
        _ => None,
    };

    let Some(next) = next else {
        return false;
    };

    if next != current
        && let Some(cb) = on_change
    {
        cb.emit(next);
    }
    true
}
