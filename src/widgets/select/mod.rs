//! Select widget.

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::Element;
use crate::core::event::{KeyCode, KeyEvent, MouseEvent};
use crate::style::{BorderStyle, Length, Padding, ScrollbarConfig, Style, StyleSlot};
use crate::widgets::button::ButtonVariant;
use crate::widgets::internal::scroll_action_from_key;
use crate::widgets::{
    Button, List, ListConfig, ListItem, Popover, PopoverPlacement, ScrollKeymap, ZStack,
};

/// A dropdown select widget (expands in-place).
#[derive(Clone)]
pub struct Select {
    pub(crate) options: Vec<Arc<str>>,
    pub(crate) selected: Option<usize>,
    pub(crate) placeholder: Arc<str>,
    pub(crate) expanded: bool,
    pub(crate) on_toggle: Option<Callback<bool>>,
    pub(crate) on_select: Option<Callback<usize>>,
    pub(crate) on_change: Option<Callback<usize>>,
    pub(crate) width: Length,
    pub(crate) disabled: bool,
    pub(crate) button_variant: ButtonVariant,
    pub(crate) button_style: Style,
    pub(crate) button_hover_style: StyleSlot,
    pub(crate) button_focus_style: StyleSlot,
    pub(crate) button_disabled_style: Style,
    pub(crate) button_border_style: BorderStyle,
    pub(crate) button_hover_border_style: Option<BorderStyle>,
    pub(crate) button_focus_border_style: Option<BorderStyle>,
    pub(crate) button_open_suffix: Option<Arc<str>>,
    pub(crate) button_closed_suffix: Option<Arc<str>>,
    pub(crate) button_suffix_style: Style,
    pub(crate) list_title: Option<Arc<str>>,
    pub(crate) list_title_style: Style,
    pub(crate) list_config: ListConfig,

    pub(crate) list_width: Option<Length>,
    pub(crate) list_height: Length,
    pub(crate) match_button_width: bool,
    pub(crate) list_empty_text: Option<Arc<str>>,
    pub(crate) list_disabled_style: Style,
}

impl Default for Select {
    fn default() -> Self {
        Self {
            options: Vec::new(),
            selected: None,
            placeholder: "Select...".into(),
            expanded: false,
            on_toggle: None,
            on_select: None,
            on_change: None,
            width: Length::Auto,
            disabled: false,
            button_variant: ButtonVariant::Outlined,
            button_style: Style::default(),
            button_hover_style: StyleSlot::Inherit,
            button_focus_style: StyleSlot::Inherit,
            button_disabled_style: Style::default(),
            button_border_style: BorderStyle::Plain,
            button_hover_border_style: None,
            button_focus_border_style: None,
            button_open_suffix: None,
            button_closed_suffix: None,
            button_suffix_style: Style::default(),
            list_title: None,
            list_title_style: Style::default(),
            list_config: ListConfig {
                border: true,
                border_style: BorderStyle::Plain,
                padding: Padding::default(),
                style: Style::default(),
                selection_style: StyleSlot::Inherit,
                unfocused_selection_style: StyleSlot::Inherit,
                selection_full_width: false,
                selection_symbol: Some("> ".into()),
                selection_symbol_right: None,
                selection_symbol_style: None,
                unfocused_selection_symbol_style: None,
                symbol_column: true,
                gutter_gap: 0,
                gutter_for_non_selectable: false,
                item_horizontal_padding: Padding::default(),
                header_horizontal_padding: Padding::default(),
                empty_text_style: Style::default(),
                item_hover_style: None,
                scrollbar: false,
                scrollbar_config: ScrollbarConfig::default(),
            },
            list_width: None,
            list_height: Length::Px(6),
            match_button_width: false,
            list_empty_text: None,
            list_disabled_style: Style::default(),
        }
    }
}

impl Select {
    /// Create a new select.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set options.
    pub fn options(mut self, options: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        self.options = options.into_iter().map(Into::into).collect();
        self
    }

    /// Set selected index.
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Set placeholder text.
    pub fn placeholder(mut self, placeholder: impl Into<Arc<str>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Set expanded state.
    pub fn expanded(mut self, expanded: bool) -> Self {
        self.expanded = expanded;
        self
    }

    /// Set toggle callback.
    pub fn on_toggle(mut self, cb: Callback<bool>) -> Self {
        self.on_toggle = Some(cb);
        self
    }

    /// Set selection callback.
    pub fn on_select(mut self, cb: Callback<usize>) -> Self {
        self.on_select = Some(cb);
        self
    }

    /// Set callback when selection changes.
    pub fn on_change(mut self, cb: Callback<usize>) -> Self {
        self.on_change = Some(cb);
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set button variant.
    pub fn button_variant(mut self, variant: ButtonVariant) -> Self {
        self.button_variant = variant;
        self
    }

    /// Set button style.
    pub fn button_style(mut self, style: Style) -> Self {
        self.button_style = style;
        self
    }

    /// Set button hover style.
    pub fn button_hover_style(mut self, style: Style) -> Self {
        self.button_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed button hover style.
    pub fn extend_button_hover_style(mut self, style: Style) -> Self {
        self.button_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed button hover style.
    pub fn inherit_button_hover_style(mut self) -> Self {
        self.button_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set button hover style slot directly for composite forwarding.
    pub fn button_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.button_hover_style = slot;
        self
    }

    /// Set button focus style.
    pub fn button_focus_style(mut self, style: Style) -> Self {
        self.button_focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed button focus style.
    pub fn extend_button_focus_style(mut self, style: Style) -> Self {
        self.button_focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed button focus style.
    pub fn inherit_button_focus_style(mut self) -> Self {
        self.button_focus_style = StyleSlot::Inherit;
        self
    }

    /// Set button focus style slot directly for composite forwarding.
    pub fn button_focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.button_focus_style = slot;
        self
    }

    /// Set button disabled style.
    pub fn button_disabled_style(mut self, style: Style) -> Self {
        self.button_disabled_style = style;
        self
    }

    /// Set button border style (used for outlined variant).
    pub fn button_border_style(mut self, style: BorderStyle) -> Self {
        self.button_border_style = style;
        self
    }

    /// Set button border style while hovered.
    pub fn button_hover_border_style(mut self, style: BorderStyle) -> Self {
        self.button_hover_border_style = Some(style);
        self
    }

    /// Set button border style while focused.
    pub fn button_focus_border_style(mut self, style: BorderStyle) -> Self {
        self.button_focus_border_style = Some(style);
        self
    }

    /// Set suffix shown when dropdown is open.
    pub fn button_open_suffix(mut self, suffix: impl Into<Arc<str>>) -> Self {
        self.button_open_suffix = Some(suffix.into());
        self
    }

    /// Set suffix shown when dropdown is closed.
    pub fn button_closed_suffix(mut self, suffix: impl Into<Arc<str>>) -> Self {
        self.button_closed_suffix = Some(suffix.into());
        self
    }

    /// Set suffix style.
    pub fn button_suffix_style(mut self, style: Style) -> Self {
        self.button_suffix_style = style;
        self
    }

    /// Set dropdown title.
    pub fn list_title(mut self, title: impl Into<Arc<str>>) -> Self {
        self.list_title = Some(title.into());
        self
    }

    /// Set dropdown title style.
    pub fn list_title_style(mut self, style: Style) -> Self {
        self.list_title_style = style;
        self
    }

    /// Set list config.
    pub fn list_config(mut self, config: ListConfig) -> Self {
        self.list_config = config;
        self
    }

    /// Set list border.
    pub fn list_border(mut self, border: bool) -> Self {
        self.list_config.border = border;
        self
    }

    /// Set list border style.
    pub fn list_border_style(mut self, style: BorderStyle) -> Self {
        self.list_config.border_style = style;
        self
    }

    /// Set list padding.
    pub fn list_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.list_config.padding = padding.into();
        self
    }

    /// Set list style.
    pub fn list_style(mut self, style: Style) -> Self {
        self.list_config.style = style;
        self
    }

    /// Set list highlight style.
    pub fn list_selection_style(mut self, style: Style) -> Self {
        self.list_config.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed dropdown highlight style.
    pub fn extend_list_selection_style(mut self, style: Style) -> Self {
        self.list_config.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed dropdown highlight style.
    pub fn inherit_list_selection_style(mut self) -> Self {
        self.list_config.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set list highlight style slot directly for composite forwarding.
    pub fn list_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.selection_style = slot;
        self
    }

    /// Set dropdown highlight style while the list is not focused.
    pub fn list_unfocused_selection_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed dropdown highlight style while the list is not focused.
    pub fn extend_list_unfocused_selection_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed dropdown highlight style while the list is not focused.
    pub fn inherit_list_unfocused_selection_style(mut self) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set list unfocused highlight style slot directly for composite forwarding.
    pub fn list_unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.unfocused_selection_style = slot;
        self
    }

    /// Set whether active_index style spans full list row width.
    pub fn list_selection_full_width(mut self, full_width: bool) -> Self {
        self.list_config.selection_full_width = full_width;
        self
    }

    /// Set list highlight symbol.
    pub fn list_selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.list_config.selection_symbol = symbol.map(Into::into);
        self
    }

    /// Set the trailing list selection symbol (right "pill" cap). Pairs with
    /// [`Self::list_selection_symbol`] and shares the selection symbol style.
    pub fn list_selection_symbol_right(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.list_config.selection_symbol_right = symbol.map(Into::into);
        self
    }

    /// Set list highlight symbol style.
    pub fn list_selection_symbol_style(mut self, style: Style) -> Self {
        self.list_config.selection_symbol_style = Some(style);
        self
    }

    /// Set list highlight symbol style while the list is not focused.
    pub fn list_unfocused_selection_symbol_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_symbol_style = Some(style);
        self
    }

    /// Set list hover style.
    pub fn list_hover_style(mut self, style: Style) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed list hover style.
    pub fn extend_list_hover_style(mut self, style: Style) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit the themed list hover style.
    pub fn inherit_list_hover_style(mut self) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Inherit);
        self
    }

    /// Set list hover style slot directly for composite forwarding.
    pub fn list_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.item_hover_style = Some(slot);
        self
    }

    /// Set list width.
    pub fn list_width(mut self, width: Length) -> Self {
        self.list_width = Some(width);
        self
    }

    /// Set list height.
    pub fn list_height(mut self, height: Length) -> Self {
        self.list_height = height;
        self
    }

    /// Force dropdown width to exactly match trigger button width.
    pub fn match_button_width(mut self, match_button_width: bool) -> Self {
        self.match_button_width = match_button_width;
        self
    }

    /// Enable list scrollbar.
    pub fn list_scrollbar(mut self, scrollbar: bool) -> Self {
        self.list_config.scrollbar = scrollbar;
        self
    }

    /// Set list scrollbar configuration.
    pub fn list_scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.list_config.scrollbar_config = config;
        self
    }

    /// Set dropdown empty text.
    pub fn list_empty_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.list_empty_text = Some(text.into());
        self
    }

    /// Set dropdown empty text style.
    pub fn list_empty_text_style(mut self, style: Style) -> Self {
        self.list_config.empty_text_style = style;
        self
    }

    /// Set dropdown disabled style.
    pub fn list_disabled_style(mut self, style: Style) -> Self {
        self.list_disabled_style = style;
        self
    }
}

impl From<Select> for Element {
    fn from(select: Select) -> Self {
        let label = if let Some(idx) = select.selected {
            select
                .options
                .get(idx)
                .cloned()
                .unwrap_or(select.placeholder.clone())
        } else {
            select.placeholder.clone()
        };

        let mut button = Button::new(label)
            .variant(select.button_variant)
            .width(select.width)
            .style(select.button_style)
            .hover_style_slot(select.button_hover_style)
            .focus_style_slot(select.button_focus_style)
            .disabled_style(select.button_disabled_style)
            .disabled(select.disabled);

        if matches!(select.button_variant, ButtonVariant::Outlined) {
            button = button.border_style(select.button_border_style);
        }
        if let Some(style) = select.button_hover_border_style {
            button = button.hover_border_style(Some(style));
        }
        if let Some(style) = select.button_focus_border_style {
            button = button.focus_border_style(Some(style));
        }

        let suffix = if select.expanded {
            select.button_open_suffix.clone()
        } else {
            select.button_closed_suffix.clone()
        };
        if let Some(suffix) = suffix {
            button = button
                .shortcut(suffix)
                .shortcut_style(select.button_suffix_style);
        }

        if let Some(cb) = select.on_toggle.clone()
            && !select.disabled
        {
            let expanded = select.expanded;
            button = button.on_click(Callback::new(move |_: MouseEvent| cb.emit(!expanded)));
        }

        if select.expanded && !select.disabled {
            let options_len = select.options.len();
            let selected = select
                .selected
                .unwrap_or(0)
                .min(options_len.saturating_sub(1));
            let change_cb = select.on_change.clone().or(select.on_select.clone());
            let on_select = select.on_select.clone();
            let on_toggle = select.on_toggle.clone();
            button = button.on_key(KeyHandler::new(move |key: KeyEvent| {
                if key.code == KeyCode::Esc
                    && let Some(toggle) = &on_toggle
                {
                    toggle.emit(false);
                    return true;
                }

                if key.code == KeyCode::Enter {
                    let mut handled = false;
                    if let Some(cb) = &on_select {
                        cb.emit(selected);
                        handled = true;
                    }
                    if let Some(toggle) = &on_toggle {
                        toggle.emit(false);
                        handled = true;
                    }
                    return handled;
                }

                let Some(action) = scroll_action_from_key(&key, ScrollKeymap::default()) else {
                    return false;
                };

                if options_len == 0 {
                    return true;
                }

                if let Some(next) = List::selection_for_action_in_len(selected, options_len, action)
                    && next != selected
                    && let Some(cb) = &change_cb
                {
                    cb.emit(next);
                }

                true
            }));
        }

        let list_hover_slot = select
            .list_config
            .item_hover_style
            .unwrap_or(select.list_config.selection_style);

        let mut list = List::new()
            .items(select.options.iter().map(|s| ListItem::new(s.clone())))
            .selected(select.selected.unwrap_or(0))
            .title_style(select.list_title_style)
            .border(select.list_config.border)
            .border_style(select.list_config.border_style)
            .padding(select.list_config.padding)
            .style(select.list_config.style)
            .selection_full_width(select.list_config.selection_full_width)
            .selection_symbol_style(
                select.list_config.selection_symbol_style.unwrap_or(
                    select
                        .list_config
                        .selection_style
                        .explicit_style()
                        .unwrap_or_default(),
                ),
            )
            .unfocused_selection_symbol_style(
                select
                    .list_config
                    .unfocused_selection_symbol_style
                    .or_else(|| {
                        select
                            .list_config
                            .unfocused_selection_style
                            .explicit_style()
                    })
                    .unwrap_or(
                        select
                            .list_config
                            .selection_style
                            .explicit_style()
                            .unwrap_or_default(),
                    ),
            )
            .selection_symbol(select.list_config.selection_symbol)
            .selection_symbol_right(select.list_config.selection_symbol_right)
            .symbol_column(select.list_config.symbol_column)
            .gutter_gap(select.list_config.gutter_gap)
            .gutter_for_non_selectable(select.list_config.gutter_for_non_selectable)
            .scrollbar(select.list_config.scrollbar)
            .scrollbar_config(select.list_config.scrollbar_config)
            .width(select.list_width.unwrap_or(select.width))
            .height(select.list_height)
            .item_horizontal_padding(select.list_config.item_horizontal_padding)
            .header_horizontal_padding(select.list_config.header_horizontal_padding)
            .empty_text_style(select.list_config.empty_text_style)
            .disabled_style(select.list_disabled_style)
            .disabled(select.disabled);
        list = list
            .selection_style_slot(select.list_config.selection_style)
            .unfocused_selection_style_slot(select.list_config.unfocused_selection_style)
            .item_hover_style_slot(list_hover_slot);

        if let Some(title) = select.list_title {
            list = list.title(title);
        }
        if let Some(empty_text) = select.list_empty_text {
            list = list.empty_text(empty_text);
        }

        let change_cb = select.on_change.clone().or(select.on_select.clone());
        if let Some(cb) = change_cb {
            list = list.on_select(Callback::new(move |ev: crate::widgets::ListEvent| {
                cb.emit(ev.index);
            }));
        }

        let emit_on_activate = select.on_change.is_some();
        if (emit_on_activate && select.on_select.is_some()) || select.on_toggle.is_some() {
            let on_select = select.on_select.clone();
            let on_toggle = select.on_toggle.clone();
            list = list.on_activate(Callback::new(move |ev: crate::widgets::ListEvent| {
                if emit_on_activate && let Some(cb) = &on_select {
                    cb.emit(ev.index);
                }
                if let Some(toggle) = &on_toggle {
                    toggle.emit(false);
                }
            }));
        }

        let overlay = ZStack::new().child(list);
        let mut popover = Popover::new()
            .trigger(button)
            .content(overlay)
            .open(select.expanded && !select.disabled)
            .fit_trigger_width(select.match_button_width && select.list_width.is_none())
            .placement(PopoverPlacement::BelowStart);

        if let Some(cb) = select.on_toggle.clone() {
            popover = popover.on_close(Callback::new(move |_| cb.emit(false)));
        }

        popover.into()
    }
}
