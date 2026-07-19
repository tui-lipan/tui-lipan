//! Multi-select widget.

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::Element;
use crate::core::event::{KeyCode, KeyEvent};
use crate::style::{BorderStyle, Length, Padding, ScrollbarConfig, Span, Style, StyleSlot};
use crate::widgets::{List, ListConfig, ListEvent, ListItem, ListItemLine};

/// A selectable source item for [`MultiSelect`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiSelectItem {
    /// Primary label.
    pub label: Arc<str>,
    /// Optional description.
    pub description: Option<Arc<str>>,
}

impl MultiSelectItem {
    /// Create a new item with label only.
    pub fn new(label: impl Into<Arc<str>>) -> Self {
        Self {
            label: label.into(),
            description: None,
        }
    }

    /// Set optional description.
    pub fn description(mut self, description: impl Into<Arc<str>>) -> Self {
        self.description = Some(description.into());
        self
    }
}

impl From<&'static str> for MultiSelectItem {
    fn from(value: &'static str) -> Self {
        Self::new(value)
    }
}

impl From<String> for MultiSelectItem {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<Arc<str>> for MultiSelectItem {
    fn from(value: Arc<str>) -> Self {
        Self::new(value)
    }
}

/// Placement for multi-select item descriptions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MultiSelectDescriptionPlacement {
    /// Render inline: `label - description`.
    #[default]
    Inline,
    /// Render in right-aligned slot on the primary line.
    Right,
    /// Render above the label.
    Above,
    /// Render below the label.
    Below,
}

/// Overflow policy for multi-select item descriptions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MultiSelectDescriptionOverflow {
    /// Keep descriptions on one visual line and truncate with ellipsis.
    #[default]
    Truncate,
    /// Wrap descriptions onto additional lines for above/below placement.
    Wrap,
}

/// Toggle event emitted by [`MultiSelect`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MultiSelectToggleEvent {
    /// Source index that changed.
    pub index: usize,
    /// Whether the index became selected.
    pub selected: bool,
}

/// Selection change event emitted by [`MultiSelect`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiSelectChangeEvent {
    /// Sorted set of selected source indices.
    pub selected_indices: Vec<usize>,
}

/// Commit event emitted by [`MultiSelect`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiSelectCommitEvent {
    /// Sorted set of selected source indices.
    pub selected_indices: Vec<usize>,
}

/// A controlled list widget for selecting multiple rows.
///
/// Checked rows are represented via [`ListItem::active`] internally, so the
/// symbol rendering priority from [`List`] applies:
/// `active_symbol` > `selection_symbol` > `unselected_symbol` > auto-spaces.
#[derive(Clone)]
pub struct MultiSelect {
    items: Arc<[MultiSelectItem]>,
    active_index: usize,
    selected_indices: Vec<usize>,
    max_selected: Option<usize>,
    title: Option<Arc<str>>,
    title_style: Style,
    width: Length,
    height: Length,
    list_config: ListConfig,
    /// Symbol shown on checked rows (maps to `List::active_symbol`).
    active_symbol: Option<Arc<str>>,
    active_symbol_style: Option<Style>,
    active_style: StyleSlot,
    /// Symbol shown on unchecked rows (maps to `List::unselected_symbol`).
    unselected_symbol: Option<Arc<str>>,
    description_style: Style,
    description_placement: MultiSelectDescriptionPlacement,
    description_overflow: MultiSelectDescriptionOverflow,
    description_selection: bool,
    disabled: bool,
    disabled_style: Style,
    empty_text: Option<Arc<str>>,
    on_active_index_change: Option<Callback<usize>>,
    on_toggle: Option<Callback<MultiSelectToggleEvent>>,
    on_change: Option<Callback<MultiSelectChangeEvent>>,
    on_commit: Option<Callback<MultiSelectCommitEvent>>,
}

impl Default for MultiSelect {
    fn default() -> Self {
        Self {
            items: Arc::from([]),
            active_index: 0,
            selected_indices: Vec::new(),
            max_selected: None,
            title: None,
            title_style: Style::default(),
            width: Length::Flex(1),
            height: Length::Flex(1),
            list_config: ListConfig {
                border: true,
                border_style: BorderStyle::Plain,
                padding: Padding::default(),
                style: Style::default(),
                selection_style: StyleSlot::Inherit,
                unfocused_selection_style: StyleSlot::Inherit,
                selection_full_width: false,
                selection_symbol: Some("[ ] ".into()),
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
            active_symbol: Some("[x] ".into()),
            active_symbol_style: None,
            active_style: StyleSlot::Inherit,
            unselected_symbol: Some("[ ] ".into()),
            description_style: Style::default(),
            description_placement: MultiSelectDescriptionPlacement::Inline,
            description_overflow: MultiSelectDescriptionOverflow::Truncate,
            description_selection: true,
            disabled: false,
            disabled_style: Style::default(),
            empty_text: None,
            on_active_index_change: None,
            on_toggle: None,
            on_change: None,
            on_commit: None,
        }
    }
}

impl MultiSelect {
    /// Create a new multi-select list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set source items.
    pub fn items(mut self, items: impl IntoIterator<Item = impl Into<MultiSelectItem>>) -> Self {
        self.items = items.into_iter().map(Into::into).collect::<Vec<_>>().into();
        self
    }

    /// Set source items from a shared slice.
    pub fn items_arc(mut self, items: Arc<[MultiSelectItem]>) -> Self {
        self.items = items;
        self
    }

    /// Set description style.
    pub fn description_style(mut self, style: Style) -> Self {
        self.description_style = style;
        self
    }

    /// Set description placement.
    pub fn description_placement(mut self, placement: MultiSelectDescriptionPlacement) -> Self {
        self.description_placement = placement;
        self
    }

    /// Control whether descriptions wrap or truncate.
    ///
    /// Wrapping applies to [`MultiSelectDescriptionPlacement::Above`] and
    /// [`MultiSelectDescriptionPlacement::Below`].
    /// [`MultiSelectDescriptionPlacement::Inline`] and
    /// [`MultiSelectDescriptionPlacement::Right`] always truncate to keep a
    /// single primary row.
    pub fn description_overflow(mut self, overflow: MultiSelectDescriptionOverflow) -> Self {
        self.description_overflow = overflow;
        self
    }

    /// Control whether selection highlight applies to description text.
    ///
    /// For [`MultiSelectDescriptionPlacement::Inline`], description shares the
    /// primary line, so this setting has no effect.
    pub fn description_selection(mut self, highlight: bool) -> Self {
        self.description_selection = highlight;
        self
    }

    /// Set currently active_index source index.
    pub fn active_index(mut self, active_index: usize) -> Self {
        self.active_index = active_index;
        self
    }

    /// Set selected source indices.
    pub fn selected_indices(mut self, selected_indices: impl IntoIterator<Item = usize>) -> Self {
        self.selected_indices = selected_indices.into_iter().collect();
        self
    }

    /// Cap how many items can be selected.
    pub fn max_selected(mut self, max_selected: usize) -> Self {
        self.max_selected = Some(max_selected);
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Set list title (visible when border is enabled).
    pub fn title(mut self, title: impl Into<Arc<str>>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set list title style.
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Set list config.
    pub fn list_config(mut self, config: ListConfig) -> Self {
        self.list_config = config;
        self
    }

    /// Set border visibility.
    pub fn border(mut self, border: bool) -> Self {
        self.list_config.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, style: BorderStyle) -> Self {
        self.list_config.border_style = style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.list_config.padding = padding.into();
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.list_config.style = style;
        self
    }

    /// Set active_index-item style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.list_config.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed active_index-item style.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.list_config.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed active_index-item style.
    pub fn inherit_selection_style(mut self) -> Self {
        self.list_config.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set active_index-item style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.selection_style = slot;
        self
    }

    /// Set active_index-item style while the list is not focused.
    pub fn unfocused_selection_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed active_index-item style while the list is not focused.
    pub fn extend_unfocused_selection_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed active_index-item style while the list is not focused.
    pub fn inherit_unfocused_selection_style(mut self) -> Self {
        self.list_config.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set unfocused active_index-item style slot directly for composite forwarding.
    pub fn unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.unfocused_selection_style = slot;
        self
    }

    /// Set whether active_index row style should span full row width.
    pub fn selection_full_width(mut self, selection_full_width: bool) -> Self {
        self.list_config.selection_full_width = selection_full_width;
        self
    }

    /// Set hovered-item style.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed hovered-item style.
    pub fn extend_item_hover_style(mut self, style: Style) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit the themed hovered-item style.
    pub fn inherit_item_hover_style(mut self) -> Self {
        self.list_config.item_hover_style = Some(StyleSlot::Inherit);
        self
    }

    /// Set hovered-item style slot directly for composite forwarding.
    pub fn item_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.list_config.item_hover_style = Some(slot);
        self
    }

    /// Set the symbol shown on the active_index (focused) but unchecked row
    /// (default: `"[ ] "`).
    ///
    /// Defaults to the same bracket as `unselected_symbol` so the visual
    /// appearance is consistent across all unchecked rows. When the focused
    /// row is also checked, `active_symbol` takes priority and this is not shown.
    pub fn selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.list_config.selection_symbol = symbol.map(Into::into);
        self
    }

    /// Set the trailing selection symbol (right "pill" cap). Pairs with
    /// [`Self::selection_symbol`] and shares [`Self::selection_symbol_style`].
    pub fn selection_symbol_right(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.list_config.selection_symbol_right = symbol.map(Into::into);
        self
    }

    /// Set style for the highlight symbol.
    pub fn selection_symbol_style(mut self, style: Style) -> Self {
        self.list_config.selection_symbol_style = Some(style);
        self
    }

    /// Set style for the highlight symbol while the list is not focused.
    pub fn unfocused_selection_symbol_style(mut self, style: Style) -> Self {
        self.list_config.unfocused_selection_symbol_style = Some(style);
        self
    }

    /// Set the symbol shown on checked rows (default: `"[x] "`).
    ///
    /// Maps to [`List::active_symbol`] and takes priority over `selection_symbol`
    /// even when the row is also the focused cursor row.
    pub fn active_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.active_symbol = symbol.map(Into::into);
        self
    }

    /// Set style for the checked-row symbol.
    pub fn active_symbol_style(mut self, style: Style) -> Self {
        self.active_symbol_style = Some(style);
        self
    }

    /// Set style applied to checked rows.
    pub fn active_style(mut self, style: Style) -> Self {
        self.active_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed checked-row style.
    pub fn extend_active_style(mut self, style: Style) -> Self {
        self.active_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed checked-row style.
    pub fn inherit_active_style(mut self) -> Self {
        self.active_style = StyleSlot::Inherit;
        self
    }

    /// Set checked-row style slot directly for composite forwarding.
    pub fn active_style_slot(mut self, slot: StyleSlot) -> Self {
        self.active_style = slot;
        self
    }

    /// Set the symbol shown on unchecked rows (default: `"[ ] "`).
    ///
    /// Maps to [`List::unselected_symbol`]. Set to `None` to remove the
    /// prefix column entirely (items will be left-aligned without indentation).
    pub fn unselected_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.unselected_symbol = symbol.map(Into::into);
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

    /// Enable scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.list_config.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
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

    /// Callback fired when active_index row changes.
    pub fn on_active_index_change(mut self, cb: Callback<usize>) -> Self {
        self.on_active_index_change = Some(cb);
        self
    }

    /// Callback fired when the current row toggles selected/unselected.
    pub fn on_toggle(mut self, cb: Callback<MultiSelectToggleEvent>) -> Self {
        self.on_toggle = Some(cb);
        self
    }

    /// Callback fired with the full selected set after toggles.
    pub fn on_change(mut self, cb: Callback<MultiSelectChangeEvent>) -> Self {
        self.on_change = Some(cb);
        self
    }

    /// Callback fired on Enter with the selected set.
    pub fn on_commit(mut self, cb: Callback<MultiSelectCommitEvent>) -> Self {
        self.on_commit = Some(cb);
        self
    }
}

impl From<MultiSelect> for Element {
    fn from(multi: MultiSelect) -> Self {
        let selected_indices =
            normalize_selected_indices(multi.selected_indices, multi.items.len());
        let active_index = normalized_active_index(multi.active_index, multi.items.len());

        let list_items = multi
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let is_checked = selected_indices.binary_search(&index).is_ok();
                multi_select_list_item(
                    item,
                    is_checked,
                    multi.description_style,
                    multi.description_placement,
                    multi.description_overflow,
                    multi.description_selection,
                )
            })
            .collect::<Vec<_>>();

        let mut list = List::new()
            .items(list_items)
            .selected(active_index)
            .activate_on_click(false)
            .border(multi.list_config.border)
            .border_style(multi.list_config.border_style)
            .padding(multi.list_config.padding)
            .style(multi.list_config.style)
            .selection_full_width(multi.list_config.selection_full_width)
            .selection_symbol(multi.list_config.selection_symbol)
            .selection_symbol_right(multi.list_config.selection_symbol_right)
            .symbol_column(multi.list_config.symbol_column)
            .gutter_gap(multi.list_config.gutter_gap)
            .gutter_for_non_selectable(multi.list_config.gutter_for_non_selectable)
            .active_symbol(multi.active_symbol)
            .active_style_slot(multi.active_style)
            .unselected_symbol(multi.unselected_symbol)
            .scrollbar(multi.list_config.scrollbar)
            .scrollbar_config(multi.list_config.scrollbar_config)
            .title_style(multi.title_style)
            .width(multi.width)
            .height(multi.height)
            .disabled(multi.disabled)
            .disabled_style(multi.disabled_style)
            .item_horizontal_padding(multi.list_config.item_horizontal_padding)
            .header_horizontal_padding(multi.list_config.header_horizontal_padding)
            .empty_text_style(multi.list_config.empty_text_style);
        list = list
            .selection_style_slot(multi.list_config.selection_style)
            .unfocused_selection_style_slot(multi.list_config.unfocused_selection_style)
            .item_hover_style_slot(
                multi
                    .list_config
                    .item_hover_style
                    .unwrap_or(multi.list_config.selection_style),
            );

        if let Some(style) = multi.list_config.selection_symbol_style {
            list = list.selection_symbol_style(style);
        }
        if let Some(style) = multi.list_config.unfocused_selection_symbol_style {
            list = list.unfocused_selection_symbol_style(style);
        }
        if let Some(style) = multi.active_symbol_style {
            list = list.active_symbol_style(style);
        }
        if let Some(title) = multi.title {
            list = list.title(title);
        }
        if let Some(empty_text) = multi.empty_text {
            list = list.empty_text(empty_text);
        }

        if let Some(cb) = multi.on_active_index_change.clone() {
            list = list.on_select(Callback::new(move |event: ListEvent| cb.emit(event.index)));
        }

        if let Some(cb) = multi.on_commit {
            let selected_indices = selected_indices.clone();
            list = list.on_activate(Callback::new(move |_event: ListEvent| {
                cb.emit(MultiSelectCommitEvent {
                    selected_indices: selected_indices.clone(),
                });
            }));
        }

        if !multi.disabled && (multi.on_toggle.is_some() || multi.on_change.is_some()) {
            let selected_indices = selected_indices.clone();
            let max_selected = multi.max_selected;
            let on_toggle = multi.on_toggle;
            let on_change = multi.on_change;
            let selected_indices_click = selected_indices.clone();
            let on_toggle_click = on_toggle.clone();
            let on_change_click = on_change.clone();

            list = list.on_item_click(Callback::new(move |event: ListEvent| {
                let (next_selected, toggled_to_selected) = match toggle_selection(
                    selected_indices_click.as_slice(),
                    event.index,
                    max_selected,
                ) {
                    Some(result) => result,
                    None => return,
                };

                if let Some(cb) = on_toggle_click.as_ref() {
                    cb.emit(MultiSelectToggleEvent {
                        index: event.index,
                        selected: toggled_to_selected,
                    });
                }
                if let Some(cb) = on_change_click.as_ref() {
                    cb.emit(MultiSelectChangeEvent {
                        selected_indices: next_selected,
                    });
                }
            }));

            list = list.on_key(KeyHandler::new(move |key: KeyEvent| {
                if key.code != KeyCode::Char(' ') {
                    return false;
                }

                let (next_selected, toggled_to_selected) =
                    match toggle_selection(selected_indices.as_slice(), active_index, max_selected)
                    {
                        Some(result) => result,
                        None => return true,
                    };

                if let Some(cb) = on_toggle.as_ref() {
                    cb.emit(MultiSelectToggleEvent {
                        index: active_index,
                        selected: toggled_to_selected,
                    });
                }
                if let Some(cb) = on_change.as_ref() {
                    cb.emit(MultiSelectChangeEvent {
                        selected_indices: next_selected,
                    });
                }

                true
            }));
        }

        list.into()
    }
}

fn multi_select_list_item(
    item: &MultiSelectItem,
    checked: bool,
    description_style: Style,
    description_placement: MultiSelectDescriptionPlacement,
    description_overflow: MultiSelectDescriptionOverflow,
    description_selection: bool,
) -> ListItem {
    let overflow = effective_description_overflow(description_placement, description_overflow);

    let mut list_item = if let Some(description) = &item.description {
        match description_placement {
            MultiSelectDescriptionPlacement::Inline => ListItem::from_spans([
                Span::new(item.label.clone()),
                Span::new(" - ").style(description_style),
                Span::new(description.clone()).style(description_style),
            ]),
            MultiSelectDescriptionPlacement::Right => ListItem::new(item.label.clone())
                .description_spans([
                    Span::new(" ").style(description_style),
                    Span::new(description.clone()).style(description_style),
                ])
                .primary_selection_description(true)
                .primary_hover_description(true)
                .primary_truncate_description_first(true),
            MultiSelectDescriptionPlacement::Above => {
                ListItem::from_spans([Span::new(description.clone()).style(description_style)])
                    .primary_selection_label(description_selection)
                    .primary_selection_description(description_selection)
                    .primary_hover_label(description_selection)
                    .primary_hover_description(description_selection)
                    .primary_wrap_label(matches!(overflow, MultiSelectDescriptionOverflow::Wrap))
                    .symbol_line(1)
                    .line(ListItemLine::new(item.label.clone()))
            }
            MultiSelectDescriptionPlacement::Below => ListItem::new(item.label.clone()).line(
                ListItemLine::new(description.clone())
                    .style(description_style)
                    .selection_label(description_selection)
                    .selection_description(description_selection)
                    .hover_label(description_selection)
                    .hover_description(description_selection)
                    .wrap_label(matches!(overflow, MultiSelectDescriptionOverflow::Wrap)),
            ),
        }
    } else {
        ListItem::new(item.label.clone())
    };

    list_item = list_item.active(checked);
    list_item
}

fn effective_description_overflow(
    placement: MultiSelectDescriptionPlacement,
    overflow: MultiSelectDescriptionOverflow,
) -> MultiSelectDescriptionOverflow {
    match placement {
        MultiSelectDescriptionPlacement::Above | MultiSelectDescriptionPlacement::Below => overflow,
        MultiSelectDescriptionPlacement::Inline | MultiSelectDescriptionPlacement::Right => {
            MultiSelectDescriptionOverflow::Truncate
        }
    }
}

fn normalized_active_index(active_index: usize, len: usize) -> usize {
    if len == 0 {
        0
    } else {
        active_index.min(len.saturating_sub(1))
    }
}

fn normalize_selected_indices(mut selected_indices: Vec<usize>, len: usize) -> Vec<usize> {
    selected_indices.retain(|index| *index < len);
    selected_indices.sort_unstable();
    selected_indices.dedup();
    selected_indices
}

fn toggle_selection(
    selected_indices: &[usize],
    index: usize,
    max_selected: Option<usize>,
) -> Option<(Vec<usize>, bool)> {
    let mut next = selected_indices.to_vec();

    match next.binary_search(&index) {
        Ok(index) => {
            next.remove(index);
            Some((next, false))
        }
        Err(insert_index) => {
            if let Some(limit) = max_selected
                && next.len() >= limit
            {
                return None;
            }
            next.insert(insert_index, index);
            Some((next, true))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        MultiSelectDescriptionOverflow, MultiSelectDescriptionPlacement, MultiSelectItem,
        multi_select_list_item, normalize_selected_indices, toggle_selection,
    };
    use crate::style::Style;

    #[test]
    fn normalize_selected_indices_sorts_and_dedups() {
        let selected = normalize_selected_indices(vec![4, 2, 2, 9, 1], 5);
        assert_eq!(selected, vec![1, 2, 4]);
    }

    #[test]
    fn toggle_selection_adds_and_removes() {
        let (selected, added) = toggle_selection(&[1, 3], 2, None).expect("should toggle");
        assert!(added);
        assert_eq!(selected, vec![1, 2, 3]);

        let (selected, added) = toggle_selection(&[1, 2, 3], 2, None).expect("should toggle");
        assert!(!added);
        assert_eq!(selected, vec![1, 3]);
    }

    #[test]
    fn toggle_selection_respects_limit() {
        let toggled = toggle_selection(&[0, 1], 2, Some(2));
        assert!(toggled.is_none());
    }

    #[test]
    fn above_description_keeps_label_on_secondary_line() {
        let item = MultiSelectItem::new("Label").description("Desc");
        let rendered = multi_select_list_item(
            &item,
            false,
            Style::default(),
            MultiSelectDescriptionPlacement::Above,
            MultiSelectDescriptionOverflow::Truncate,
            false,
        );

        let primary: String = rendered
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        let secondary: String = rendered.extra_lines[0]
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();

        assert_eq!(primary, "Desc");
        assert_eq!(secondary, "Label");
    }

    #[test]
    fn right_description_always_highlights_and_hovers() {
        let item = MultiSelectItem::new("Label").description("Desc");
        let rendered = multi_select_list_item(
            &item,
            false,
            Style::default(),
            MultiSelectDescriptionPlacement::Right,
            MultiSelectDescriptionOverflow::Wrap,
            false,
        );

        assert!(rendered.primary_selection_description);
        assert!(rendered.primary_hover_description);
        assert!(!rendered.description_spans.is_empty());
        assert_eq!(rendered.description_spans[0].content.as_ref(), " ");
    }

    #[test]
    fn below_wrap_sets_wrap_label_flag() {
        let item = MultiSelectItem::new("Label").description("Desc");
        let rendered = multi_select_list_item(
            &item,
            false,
            Style::default(),
            MultiSelectDescriptionPlacement::Below,
            MultiSelectDescriptionOverflow::Wrap,
            true,
        );

        assert_eq!(rendered.extra_lines.len(), 1);
        assert!(rendered.extra_lines[0].wrap_label);
    }

    #[test]
    fn items_arc_preserves_shared_slice() {
        use super::MultiSelect;
        use std::sync::Arc;

        let items: Arc<[MultiSelectItem]> =
            Arc::from([MultiSelectItem::new("a"), MultiSelectItem::new("b")]);
        let multi = MultiSelect::new().items_arc(Arc::clone(&items));
        assert!(Arc::ptr_eq(&multi.items, &items));
    }
}
