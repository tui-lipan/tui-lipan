//! Combo box widget.

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::Element;
use crate::core::event::{KeyCode, KeyEvent};
use crate::style::{BorderStyle, Length, Padding, ScrollbarConfig, Style, StyleSlot};
use crate::widgets::{
    Input, InputEvent, List, ListConfig, ListEvent, ListItem, Popover, PopoverOffset,
    PopoverPlacement,
};

/// Commit event emitted by [`ComboBox`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComboBoxCommitEvent {
    /// Index in the source `items` list when an existing item is committed.
    pub index: Option<usize>,
    /// Committed value.
    pub value: Arc<str>,
    /// `true` when the committed value comes from free-form query text.
    pub from_custom_value: bool,
}

/// A controlled input + dropdown list widget.
#[derive(Clone)]
pub struct ComboBox {
    items: Vec<Arc<str>>,
    query: Arc<str>,
    placeholder: Option<Arc<str>>,
    open: bool,
    active_index: Option<usize>,
    selected: Option<usize>,
    allow_custom_value: bool,
    width: Length,
    list_width: Option<Length>,
    list_height: Length,
    match_input_width: bool,
    disabled: bool,
    placement: PopoverPlacement,
    offset: PopoverOffset,
    clamp: bool,
    auto_flip: bool,
    input_style: Style,
    input_hover_style: StyleSlot,
    input_focus_style: StyleSlot,
    input_focus_content_style: Style,
    input_disabled_style: Style,
    input_hover_border_style: Option<BorderStyle>,
    input_placeholder_style: Style,
    input_focus_placeholder_style: Style,
    input_suffix_open: Arc<str>,
    input_suffix_closed: Arc<str>,
    input_suffix_style: Style,
    input_focus_suffix_style: Style,
    list_config: ListConfig,
    empty_text: Option<Arc<str>>,
    on_query_change: Option<Callback<Arc<str>>>,
    on_open_change: Option<Callback<bool>>,
    on_active_index_change: Option<Callback<Option<usize>>>,
    on_commit: Option<Callback<ComboBoxCommitEvent>>,
}

impl Default for ComboBox {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            query: Arc::from(""),
            placeholder: Some("Type to filter...".into()),
            open: false,
            active_index: None,
            selected: None,
            allow_custom_value: false,
            width: Length::Flex(1),
            list_width: None,
            list_height: Length::Px(8),
            match_input_width: false,
            disabled: false,
            placement: PopoverPlacement::BelowStart,
            offset: PopoverOffset::ZERO,
            clamp: true,
            auto_flip: true,
            input_style: Style::default(),
            input_hover_style: StyleSlot::Inherit,
            input_focus_style: StyleSlot::Inherit,
            input_focus_content_style: Style::default(),
            input_disabled_style: Style::default(),
            input_hover_border_style: None,
            input_placeholder_style: Style::default(),
            input_focus_placeholder_style: Style::default(),
            input_suffix_open: " ▲".into(),
            input_suffix_closed: " ▼".into(),
            input_suffix_style: Style::default(),
            input_focus_suffix_style: Style::default(),
            list_config: ListConfig {
                border: true,
                border_style: BorderStyle::Plain,
                padding: Padding::default(),
                style: Style::default(),
                selection_style: StyleSlot::Inherit,
                unfocused_selection_style: StyleSlot::Inherit,
                selection_full_width: false,
                selection_symbol: None,
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
            empty_text: Some("No matches".into()),
            on_query_change: None,
            on_open_change: None,
            on_active_index_change: None,
            on_commit: None,
        }
    }
}

impl ComboBox {
    /// Create a new combo box.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set source items.
    pub fn items(mut self, items: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        self.items = items.into_iter().map(Into::into).collect();
        self
    }

    /// Set current query.
    pub fn query(mut self, query: impl Into<Arc<str>>) -> Self {
        self.query = query.into();
        self
    }

    /// Set placeholder text.
    pub fn placeholder(mut self, placeholder: impl Into<Arc<str>>) -> Self {
        self.placeholder = Some(placeholder.into());
        self
    }

    /// Set controlled open state.
    pub fn open(mut self, open: bool) -> Self {
        self.open = open;
        self
    }

    /// Set currently active_index source index.
    pub fn active_index(mut self, active_index: Option<usize>) -> Self {
        self.active_index = active_index;
        self
    }

    /// Set selected source index.
    pub fn selected(mut self, selected: Option<usize>) -> Self {
        self.selected = selected;
        self
    }

    /// Allow Enter to commit free-form query text when no item is chosen.
    pub fn allow_custom_value(mut self, allow_custom_value: bool) -> Self {
        self.allow_custom_value = allow_custom_value;
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set dropdown width override.
    pub fn list_width(mut self, width: Length) -> Self {
        self.list_width = Some(width);
        self
    }

    /// Set dropdown height.
    pub fn list_height(mut self, height: Length) -> Self {
        self.list_height = height;
        self
    }

    /// Force dropdown width to exactly match rendered input width.
    pub fn match_input_width(mut self, match_input_width: bool) -> Self {
        self.match_input_width = match_input_width;
        self
    }

    /// Set disabled state.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set popover placement.
    pub fn placement(mut self, placement: PopoverPlacement) -> Self {
        self.placement = placement;
        self
    }

    /// Set popover offset.
    pub fn offset(mut self, offset: impl Into<PopoverOffset>) -> Self {
        self.offset = offset.into();
        self
    }

    /// Clamp dropdown to viewport bounds.
    pub fn clamp(mut self, clamp: bool) -> Self {
        self.clamp = clamp;
        self
    }

    /// Auto-flip dropdown placement when overflowing viewport.
    pub fn auto_flip(mut self, auto_flip: bool) -> Self {
        self.auto_flip = auto_flip;
        self
    }

    /// Set input base style.
    pub fn input_style(mut self, style: Style) -> Self {
        self.input_style = style;
        self
    }

    /// Set input hover style.
    pub fn input_hover_style(mut self, style: Style) -> Self {
        self.input_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed input hover style.
    pub fn extend_input_hover_style(mut self, style: Style) -> Self {
        self.input_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed input hover style.
    pub fn inherit_input_hover_style(mut self) -> Self {
        self.input_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set input hover style slot directly for composite forwarding.
    pub fn input_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.input_hover_style = slot;
        self
    }

    /// Set input focus chrome style.
    pub fn input_focus_style(mut self, style: Style) -> Self {
        self.input_focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed input focus style.
    pub fn extend_input_focus_style(mut self, style: Style) -> Self {
        self.input_focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed input focus style.
    pub fn inherit_input_focus_style(mut self) -> Self {
        self.input_focus_style = StyleSlot::Inherit;
        self
    }

    /// Set input focus style slot directly for composite forwarding.
    pub fn input_focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.input_focus_style = slot;
        self
    }

    /// Set focused input content text style.
    pub fn input_focus_content_style(mut self, style: Style) -> Self {
        self.input_focus_content_style = style;
        self
    }

    /// Set input disabled style.
    pub fn input_disabled_style(mut self, style: Style) -> Self {
        self.input_disabled_style = style;
        self
    }

    /// Set input border style while hovered.
    pub fn input_hover_border_style(mut self, border_style: BorderStyle) -> Self {
        self.input_hover_border_style = Some(border_style);
        self
    }

    /// Set input placeholder style.
    pub fn input_placeholder_style(mut self, style: Style) -> Self {
        self.input_placeholder_style = style;
        self
    }

    /// Set input placeholder style when focused.
    pub fn input_focus_placeholder_style(mut self, style: Style) -> Self {
        self.input_focus_placeholder_style = style;
        self
    }

    /// Set suffix displayed when dropdown is open.
    pub fn input_open_suffix(mut self, suffix: impl Into<Arc<str>>) -> Self {
        self.input_suffix_open = suffix.into();
        self
    }

    /// Set suffix displayed when dropdown is closed.
    pub fn input_closed_suffix(mut self, suffix: impl Into<Arc<str>>) -> Self {
        self.input_suffix_closed = suffix.into();
        self
    }

    /// Set input suffix style.
    pub fn input_suffix_style(mut self, style: Style) -> Self {
        self.input_suffix_style = style;
        self
    }

    /// Set input suffix style when focused.
    pub fn input_focus_suffix_style(mut self, style: Style) -> Self {
        self.input_focus_suffix_style = style;
        self
    }

    /// Set list config.
    pub fn list_config(mut self, config: ListConfig) -> Self {
        self.list_config = config;
        self
    }

    /// Set dropdown border visibility.
    pub fn list_border(mut self, list_border: bool) -> Self {
        self.list_config.border = list_border;
        self
    }

    /// Set dropdown border style.
    pub fn list_border_style(mut self, border_style: BorderStyle) -> Self {
        self.list_config.border_style = border_style;
        self
    }

    /// Set dropdown padding.
    pub fn list_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.list_config.padding = padding.into();
        self
    }

    /// Set dropdown base style.
    pub fn list_style(mut self, style: Style) -> Self {
        self.list_config.style = style;
        self
    }

    /// Set dropdown active_index-item style.
    pub fn list_selection_style(mut self, style: Style) -> Self {
        self.list_config.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed dropdown active_index-item style.
    pub fn extend_list_selection_style(mut self, style: Style) -> Self {
        self.list_config.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed dropdown active_index-item style.
    pub fn inherit_list_selection_style(mut self) -> Self {
        self.list_config.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set dropdown active_index-item style slot directly for composite forwarding.
    pub fn list_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.selection_style = slot;
        self
    }

    /// Set dropdown active_index-item style while the list is not focused.
    pub fn list_unfocused_selection_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed dropdown active_index-item style while the list is not focused.
    pub fn extend_list_unfocused_selection_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed dropdown active_index-item style while the list is not focused.
    pub fn inherit_list_unfocused_selection_style(mut self) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set dropdown unfocused active_index-item style slot directly for composite forwarding.
    pub fn list_unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.unfocused_selection_style = slot;
        self
    }

    /// Set dropdown hovered-item style.
    pub fn list_hover_style(mut self, style: Style) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed dropdown hovered-item style.
    pub fn extend_list_hover_style(mut self, style: Style) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit the themed dropdown hovered-item style.
    pub fn inherit_list_hover_style(mut self) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Inherit);
        self
    }

    /// Set dropdown hovered-item style slot directly for composite forwarding.
    pub fn list_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.item_hover_style = Some(slot);
        self
    }

    /// Set dropdown active_index-item symbol.
    pub fn list_selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.list_config.selection_symbol = symbol.map(Into::into);
        self
    }

    /// Set the trailing dropdown selection symbol (right "pill" cap). Pairs with
    /// [`Self::list_selection_symbol`] and shares the selection symbol style.
    pub fn list_selection_symbol_right(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.list_config.selection_symbol_right = symbol.map(Into::into);
        self
    }

    /// Set dropdown active_index-item symbol style.
    pub fn list_selection_symbol_style(mut self, style: Style) -> Self {
        self.list_config.selection_symbol_style = Some(style);
        self
    }

    /// Set dropdown active_index-item symbol style while the list is not focused.
    pub fn list_unfocused_selection_symbol_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_symbol_style = Some(style);
        self
    }

    /// Enable dropdown scrollbar.
    pub fn list_scrollbar(mut self, scrollbar: bool) -> Self {
        self.list_config.scrollbar = scrollbar;
        self
    }

    /// Set dropdown scrollbar configuration.
    pub fn list_scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.list_config.scrollbar_config = config;
        self
    }

    /// Set empty-list text.
    pub fn empty_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.empty_text = Some(text.into());
        self
    }

    /// Set empty-list text style.
    pub fn empty_text_style(mut self, style: Style) -> Self {
        self.list_config.empty_text_style = style;
        self
    }

    /// Callback fired when query changes.
    pub fn on_query_change(mut self, cb: Callback<Arc<str>>) -> Self {
        self.on_query_change = Some(cb);
        self
    }

    /// Callback fired when open state should change.
    pub fn on_open_change(mut self, cb: Callback<bool>) -> Self {
        self.on_open_change = Some(cb);
        self
    }

    /// Callback fired when active_index source index changes.
    pub fn on_active_index_change(mut self, cb: Callback<Option<usize>>) -> Self {
        self.on_active_index_change = Some(cb);
        self
    }

    /// Callback fired when an item or custom value is committed.
    pub fn on_commit(mut self, cb: Callback<ComboBoxCommitEvent>) -> Self {
        self.on_commit = Some(cb);
        self
    }
}

impl From<ComboBox> for Element {
    fn from(combo: ComboBox) -> Self {
        let filtered_indices = filtered_item_indices(&combo.items, combo.query.as_ref());
        let effective_highlight = normalized_active_index(
            combo.active_index,
            combo.selected,
            filtered_indices.as_slice(),
        );
        let list_selected = effective_highlight
            .and_then(|source_index| {
                filtered_indices
                    .iter()
                    .position(|&candidate| candidate == source_index)
            })
            .unwrap_or(0);

        let mut input = Input::new(combo.query.clone())
            .width(combo.width)
            .style(combo.input_style)
            .hover_style_slot(combo.input_hover_style)
            .focus_style_slot(combo.input_focus_style)
            .focus_content_style(combo.input_focus_content_style)
            .disabled_style(combo.input_disabled_style)
            .placeholder_style(combo.input_placeholder_style)
            .focus_placeholder_style(combo.input_focus_placeholder_style)
            .suffix(if combo.open {
                combo.input_suffix_open.clone()
            } else {
                combo.input_suffix_closed.clone()
            })
            .suffix_style(combo.input_suffix_style)
            .focus_suffix_style(combo.input_focus_suffix_style)
            .read_only(combo.disabled)
            .disabled(combo.disabled);

        if let Some(hover_border_style) = combo.input_hover_border_style {
            input = input.hover_border_style(hover_border_style);
        }

        if let Some(placeholder) = combo.placeholder.clone() {
            input = input.placeholder(placeholder);
        }

        if combo.on_query_change.is_some() || combo.on_open_change.is_some() {
            let on_query_change = combo.on_query_change.clone();
            let on_open_change = combo.on_open_change.clone();
            input = input.on_change(Callback::new(move |event: InputEvent| {
                if let Some(cb) = on_query_change.as_ref() {
                    cb.emit(event.value.clone());
                }
                if let Some(cb) = on_open_change.as_ref() {
                    cb.emit(true);
                }
            }));
        }

        {
            let filtered_indices = filtered_indices.clone();
            let items = combo.items.clone();
            let query = combo.query.clone();
            let selected = combo.selected;
            let allow_custom_value = combo.allow_custom_value;
            let open = combo.open;
            let on_open_change = combo.on_open_change.clone();
            let on_active_index_change = combo.on_active_index_change.clone();
            let on_commit = combo.on_commit.clone();
            input = input.on_key(KeyHandler::new(move |key: KeyEvent| match key.code {
                KeyCode::Esc if open => {
                    if let Some(cb) = on_open_change.as_ref() {
                        cb.emit(false);
                        true
                    } else {
                        false
                    }
                }
                KeyCode::Down | KeyCode::Up => {
                    if filtered_indices.is_empty() {
                        return true;
                    }

                    let current_pos = effective_highlight
                        .and_then(|source_index| {
                            filtered_indices
                                .iter()
                                .position(|&candidate| candidate == source_index)
                        })
                        .unwrap_or(0);
                    let next_pos = if key.code == KeyCode::Down {
                        (current_pos + 1).min(filtered_indices.len().saturating_sub(1))
                    } else {
                        current_pos.saturating_sub(1)
                    };
                    if let Some(cb) = on_active_index_change.as_ref() {
                        cb.emit(Some(filtered_indices[next_pos]));
                    }
                    if let Some(cb) = on_open_change.as_ref() {
                        cb.emit(true);
                    }
                    true
                }
                KeyCode::Enter => {
                    let mut handled = false;

                    let active_index = effective_highlight
                        .filter(|source_index| filtered_indices.contains(source_index));
                    let selected =
                        selected.filter(|source_index| filtered_indices.contains(source_index));
                    let picked_index = active_index.or(selected);

                    if let Some(cb) = on_commit.as_ref() {
                        if let Some(index) = picked_index {
                            cb.emit(ComboBoxCommitEvent {
                                index: Some(index),
                                value: items[index].clone(),
                                from_custom_value: false,
                            });
                            handled = true;
                        } else if allow_custom_value && !query.is_empty() {
                            cb.emit(ComboBoxCommitEvent {
                                index: None,
                                value: query.clone(),
                                from_custom_value: true,
                            });
                            handled = true;
                        }
                    }

                    if open && let Some(cb) = on_open_change.as_ref() {
                        cb.emit(false);
                        handled = true;
                    }

                    handled
                }
                _ => false,
            }));
        }

        let mut list = List::new()
            .items(
                filtered_indices
                    .iter()
                    .map(|&index| ListItem::new(combo.items[index].clone())),
            )
            .selected(list_selected)
            .border(combo.list_config.border)
            .border_style(combo.list_config.border_style)
            .padding(combo.list_config.padding)
            .style(combo.list_config.style)
            .selection_symbol(combo.list_config.selection_symbol)
            .selection_symbol_right(combo.list_config.selection_symbol_right)
            .selection_symbol_style(
                combo.list_config.selection_symbol_style.unwrap_or(
                    combo
                        .list_config
                        .selection_style
                        .explicit_style()
                        .unwrap_or_default(),
                ),
            )
            .unfocused_selection_symbol_style(
                combo
                    .list_config
                    .unfocused_selection_symbol_style
                    .or_else(|| combo.list_config.unfocused_selection_style.explicit_style())
                    .unwrap_or(
                        combo
                            .list_config
                            .selection_style
                            .explicit_style()
                            .unwrap_or_default(),
                    ),
            )
            .symbol_column(combo.list_config.symbol_column)
            .gutter_gap(combo.list_config.gutter_gap)
            .gutter_for_non_selectable(combo.list_config.gutter_for_non_selectable)
            .scrollbar(combo.list_config.scrollbar)
            .scrollbar_config(combo.list_config.scrollbar_config)
            .width(combo.list_width.unwrap_or(combo.width))
            .height(combo.list_height)
            .disabled(combo.disabled)
            .item_horizontal_padding(combo.list_config.item_horizontal_padding)
            .header_horizontal_padding(combo.list_config.header_horizontal_padding)
            .empty_text_style(combo.list_config.empty_text_style);
        list = list
            .selection_style_slot(combo.list_config.selection_style)
            .unfocused_selection_style_slot(combo.list_config.unfocused_selection_style)
            .item_hover_style_slot(
                combo
                    .list_config
                    .item_hover_style
                    .unwrap_or(combo.list_config.selection_style),
            );

        let fit_trigger_width = combo.match_input_width && combo.list_width.is_none();

        if let Some(empty_text) = combo.empty_text {
            list = list.empty_text(empty_text);
        }

        if let Some(cb) = combo.on_active_index_change.clone() {
            let filtered_indices = filtered_indices.clone();
            list = list.on_select(Callback::new(move |event: ListEvent| {
                if let Some(source_index) = filtered_indices.get(event.index).copied() {
                    cb.emit(Some(source_index));
                }
            }));
        }

        if combo.on_commit.is_some() || combo.on_open_change.is_some() {
            let filtered_indices = filtered_indices.clone();
            let on_commit = combo.on_commit.clone();
            let on_open_change = combo.on_open_change.clone();
            let items = combo.items.clone();
            list = list.on_activate(Callback::new(move |event: ListEvent| {
                if let Some(source_index) = filtered_indices.get(event.index).copied()
                    && let Some(cb) = on_commit.as_ref()
                {
                    cb.emit(ComboBoxCommitEvent {
                        index: Some(source_index),
                        value: items[source_index].clone(),
                        from_custom_value: false,
                    });
                }
                if let Some(cb) = on_open_change.as_ref() {
                    cb.emit(false);
                }
            }));
        }

        let on_close = combo
            .on_open_change
            .unwrap_or_else(|| Callback::new(|_| {}));

        Popover::new()
            .trigger(input)
            .content(list)
            .open(combo.open && !combo.disabled)
            .placement(combo.placement)
            .offset(combo.offset)
            .clamp(combo.clamp)
            .auto_flip(combo.auto_flip)
            .fit_trigger_width(fit_trigger_width)
            .min_trigger_width(false)
            .on_close(Callback::new(move |_| on_close.emit(false)))
            .into()
    }
}

fn filtered_item_indices(items: &[Arc<str>], query: &str) -> Vec<usize> {
    if query.is_empty() {
        return (0..items.len()).collect();
    }

    let query = query.to_ascii_lowercase();
    items
        .iter()
        .enumerate()
        .filter_map(|(index, item)| {
            item.to_ascii_lowercase()
                .contains(query.as_str())
                .then_some(index)
        })
        .collect()
}

fn normalized_active_index(
    active_index: Option<usize>,
    selected: Option<usize>,
    filtered_indices: &[usize],
) -> Option<usize> {
    if filtered_indices.is_empty() {
        return None;
    }

    active_index
        .filter(|index| filtered_indices.contains(index))
        .or_else(|| selected.filter(|index| filtered_indices.contains(index)))
        .or_else(|| filtered_indices.first().copied())
}

#[cfg(test)]
mod tests {
    use super::{filtered_item_indices, normalized_active_index};

    #[test]
    fn filter_returns_all_for_empty_query() {
        let items = ["Alpha".into(), "Beta".into(), "Gamma".into()];
        let filtered = filtered_item_indices(&items, "");
        assert_eq!(filtered, vec![0, 1, 2]);
    }

    #[test]
    fn filter_is_case_insensitive() {
        let items = ["Alpha".into(), "Beta".into(), "Gamma".into()];
        let filtered = filtered_item_indices(&items, "AL");
        assert_eq!(filtered, vec![0]);
    }

    #[test]
    fn normalized_active_index_prefers_explicit_highlight() {
        let filtered = vec![2, 4, 6];
        let resolved = normalized_active_index(Some(4), Some(2), filtered.as_slice());
        assert_eq!(resolved, Some(4));
    }

    #[test]
    fn normalized_active_index_falls_back_to_selected_then_first() {
        let filtered = vec![1, 3, 5];
        let resolved = normalized_active_index(Some(2), Some(3), filtered.as_slice());
        assert_eq!(resolved, Some(3));

        let resolved = normalized_active_index(Some(2), Some(4), filtered.as_slice());
        assert_eq!(resolved, Some(1));
    }
}
