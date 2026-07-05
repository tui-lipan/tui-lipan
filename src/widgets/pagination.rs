//! Pagination helpers.

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::Element;
use crate::style::{BorderStyle, Style, StyleSlot};
use crate::widgets::button::ButtonVariant;
use crate::widgets::{Button, HStack, Text};

/// Controlled pagination state helper for composition-based UIs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaginationState {
    page: usize,
    per_page: usize,
    total_items: usize,
}

impl PaginationState {
    /// Create pagination state.
    pub fn new(total_items: usize, per_page: usize) -> Self {
        let per_page = per_page.max(1);
        let mut state = Self {
            page: 0,
            per_page,
            total_items,
        };
        state.clamp_page();
        state
    }

    /// Current zero-based page index.
    pub fn page(&self) -> usize {
        self.page
    }

    /// Items per page.
    pub fn per_page(&self) -> usize {
        self.per_page
    }

    /// Total items.
    pub fn total_items(&self) -> usize {
        self.total_items
    }

    /// Total page count (at least 1).
    pub fn total_pages(&self) -> usize {
        self.total_items.max(1).div_ceil(self.per_page)
    }

    /// Whether this is the first page.
    pub fn is_first_page(&self) -> bool {
        self.page == 0
    }

    /// Whether this is the last page.
    pub fn is_last_page(&self) -> bool {
        self.page + 1 >= self.total_pages()
    }

    /// Set zero-based page index (clamped to bounds).
    pub fn set_page(&mut self, page: usize) {
        self.page = page;
        self.clamp_page();
    }

    /// Move to previous page.
    pub fn prev_page(&mut self) {
        self.page = self.page.saturating_sub(1);
    }

    /// Move to next page.
    pub fn next_page(&mut self) {
        if !self.is_last_page() {
            self.page += 1;
        }
    }

    /// Move to first page.
    pub fn first_page(&mut self) {
        self.page = 0;
    }

    /// Move to last page.
    pub fn last_page(&mut self) {
        self.page = self.total_pages().saturating_sub(1);
    }

    /// Set items per page and re-clamp the current page.
    pub fn set_per_page(&mut self, per_page: usize) {
        self.per_page = per_page.max(1);
        self.clamp_page();
    }

    /// Set total items and re-clamp the current page.
    pub fn set_total_items(&mut self, total_items: usize) {
        self.total_items = total_items;
        self.clamp_page();
    }

    /// Current item range as `(start, end_exclusive)`.
    pub fn range(&self) -> (usize, usize) {
        let start = self.page.saturating_mul(self.per_page);
        let end = start.saturating_add(self.per_page).min(self.total_items);
        (start.min(self.total_items), end)
    }

    fn clamp_page(&mut self) {
        self.page = self.page.min(self.total_pages().saturating_sub(1));
    }
}

impl Default for PaginationState {
    fn default() -> Self {
        Self::new(0, 10)
    }
}

/// Navigation action emitted by [`PaginationBar`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PaginationAction {
    /// Jump to first page.
    First,
    /// Move to previous page.
    Prev,
    /// Move to next page.
    Next,
    /// Jump to last page.
    Last,
}

/// Navigation labels for [`PaginationBar`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PaginationLabels {
    /// Label for first-page button.
    pub first: Arc<str>,
    /// Label for previous-page button.
    pub prev: Arc<str>,
    /// Label for next-page button.
    pub next: Arc<str>,
    /// Label for last-page button.
    pub last: Arc<str>,
}

impl Default for PaginationLabels {
    fn default() -> Self {
        Self {
            first: "<<".into(),
            prev: "<".into(),
            next: ">".into(),
            last: ">>".into(),
        }
    }
}

/// Per-button style overrides for [`PaginationBar`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct PaginationButtonOverrides {
    /// Optional variant override.
    pub variant: Option<ButtonVariant>,
    /// Optional border style override (for outlined variant).
    pub border_style: Option<BorderStyle>,
    /// Optional base style override.
    pub style: Option<Style>,
    /// Optional hover style override.
    pub hover_style: Option<StyleSlot>,
    /// Optional focus style override.
    pub focus_style: Option<StyleSlot>,
    /// Optional disabled style override.
    pub disabled_style: Option<Style>,
}

impl PaginationButtonOverrides {
    /// Create empty per-button overrides.
    pub fn new() -> Self {
        Self::default()
    }

    /// Override variant.
    pub fn variant(mut self, variant: ButtonVariant) -> Self {
        self.variant = Some(variant);
        self
    }

    /// Override border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = Some(border_style);
        self
    }

    /// Override base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }

    /// Override hover style.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed hover style.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit the themed hover style.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = Some(StyleSlot::Inherit);
        self
    }

    /// Override hover style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_style = Some(slot);
        self
    }

    /// Override focus style.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed focus style.
    pub fn extend_focus_style(mut self, style: Style) -> Self {
        self.focus_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit the themed focus style.
    pub fn inherit_focus_style(mut self) -> Self {
        self.focus_style = Some(StyleSlot::Inherit);
        self
    }

    /// Override focus style slot directly for composite forwarding.
    pub fn focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.focus_style = Some(slot);
        self
    }

    /// Override disabled style.
    pub fn disabled_style(mut self, style: Style) -> Self {
        self.disabled_style = Some(style);
        self
    }
}

/// Structured values available to custom pagination info formatters.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PaginationInfo {
    /// Current zero-based page index.
    pub page_index: usize,
    /// Current one-based page number.
    pub page_number: usize,
    /// Total number of pages.
    pub total_pages: usize,
    /// Total number of items.
    pub total_items: usize,
    /// Items per page.
    pub per_page: usize,
    /// Current range start (zero-based, inclusive).
    pub start: usize,
    /// Current range end (zero-based, exclusive).
    pub end: usize,
}

type PaginationInfoFormatter = Arc<dyn Fn(PaginationInfo) -> Arc<str>>;

/// Composable pagination controls with style personalization.
#[derive(Clone)]
pub struct PaginationBar {
    state: PaginationState,
    labels: PaginationLabels,
    show_first_last: bool,
    show_range_info: bool,
    gap: u16,
    button_variant: ButtonVariant,
    button_border_style: BorderStyle,
    button_style: Style,
    button_hover_style: StyleSlot,
    button_focus_style: StyleSlot,
    button_disabled_style: Style,
    button_overrides: [PaginationButtonOverrides; 4],
    info_style: Style,
    info_formatter: Option<PaginationInfoFormatter>,
    on_action: Option<Callback<PaginationAction>>,
}

impl PaginationBar {
    /// Create a new pagination bar from controlled state.
    pub fn new(state: PaginationState) -> Self {
        Self {
            state,
            labels: PaginationLabels::default(),
            show_first_last: true,
            show_range_info: true,
            gap: 1,
            button_variant: ButtonVariant::Outlined,
            button_border_style: BorderStyle::Plain,
            button_style: Style::default(),
            button_hover_style: StyleSlot::Inherit,
            button_focus_style: StyleSlot::Inherit,
            button_disabled_style: Style::default(),
            button_overrides: [PaginationButtonOverrides::default(); 4],
            info_style: Style::default(),
            info_formatter: None,
            on_action: None,
        }
    }

    /// Set all navigation labels at once.
    pub fn labels(mut self, labels: PaginationLabels) -> Self {
        self.labels = labels;
        self
    }

    /// Set first-page button label.
    pub fn first_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.labels.first = label.into();
        self
    }

    /// Set previous-page button label.
    pub fn prev_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.labels.prev = label.into();
        self
    }

    /// Set next-page button label.
    pub fn next_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.labels.next = label.into();
        self
    }

    /// Set last-page button label.
    pub fn last_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.labels.last = label.into();
        self
    }

    /// Show or hide first/last navigation buttons.
    pub fn show_first_last(mut self, show: bool) -> Self {
        self.show_first_last = show;
        self
    }

    /// Show or hide row-range information in the center label.
    pub fn show_range_info(mut self, show: bool) -> Self {
        self.show_range_info = show;
        self
    }

    /// Set horizontal gap between controls.
    pub fn gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }

    /// Set button variant for all nav buttons.
    pub fn button_variant(mut self, variant: ButtonVariant) -> Self {
        self.button_variant = variant;
        self
    }

    /// Set border style for outlined nav buttons.
    pub fn button_border_style(mut self, style: BorderStyle) -> Self {
        self.button_border_style = style;
        self
    }

    /// Set base style for nav buttons.
    pub fn button_style(mut self, style: Style) -> Self {
        self.button_style = style;
        self
    }

    /// Set hover style for nav buttons.
    pub fn button_hover_style(mut self, style: Style) -> Self {
        self.button_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hover style for nav buttons.
    pub fn extend_button_hover_style(mut self, style: Style) -> Self {
        self.button_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed hover style for nav buttons.
    pub fn inherit_button_hover_style(mut self) -> Self {
        self.button_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hover style slot for nav buttons directly for composite forwarding.
    pub fn button_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.button_hover_style = slot;
        self
    }

    /// Set focus style for nav buttons.
    pub fn button_focus_style(mut self, style: Style) -> Self {
        self.button_focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed focus style for nav buttons.
    pub fn extend_button_focus_style(mut self, style: Style) -> Self {
        self.button_focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed focus style for nav buttons.
    pub fn inherit_button_focus_style(mut self) -> Self {
        self.button_focus_style = StyleSlot::Inherit;
        self
    }

    /// Set focus style slot for nav buttons directly for composite forwarding.
    pub fn button_focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.button_focus_style = slot;
        self
    }

    /// Set disabled style for nav buttons.
    pub fn button_disabled_style(mut self, style: Style) -> Self {
        self.button_disabled_style = style;
        self
    }

    /// Set per-button style overrides for one action.
    pub fn button_overrides_for(
        mut self,
        action: PaginationAction,
        overrides: PaginationButtonOverrides,
    ) -> Self {
        self.button_overrides[action_index(action)] = overrides;
        self
    }

    /// Set style overrides for first-page button.
    pub fn first_button_overrides(mut self, overrides: PaginationButtonOverrides) -> Self {
        self.button_overrides[action_index(PaginationAction::First)] = overrides;
        self
    }

    /// Set style overrides for previous-page button.
    pub fn prev_button_overrides(mut self, overrides: PaginationButtonOverrides) -> Self {
        self.button_overrides[action_index(PaginationAction::Prev)] = overrides;
        self
    }

    /// Set style overrides for next-page button.
    pub fn next_button_overrides(mut self, overrides: PaginationButtonOverrides) -> Self {
        self.button_overrides[action_index(PaginationAction::Next)] = overrides;
        self
    }

    /// Set style overrides for last-page button.
    pub fn last_button_overrides(mut self, overrides: PaginationButtonOverrides) -> Self {
        self.button_overrides[action_index(PaginationAction::Last)] = overrides;
        self
    }

    /// Set style for pagination info text.
    pub fn info_style(mut self, style: Style) -> Self {
        self.info_style = style;
        self
    }

    /// Set a custom formatter for the center info label.
    pub fn info_formatter<F>(mut self, formatter: F) -> Self
    where
        F: Fn(PaginationInfo) -> Arc<str> + 'static,
    {
        self.info_formatter = Some(Arc::new(formatter));
        self
    }

    /// Callback fired when a nav button is clicked.
    pub fn on_action(mut self, cb: Callback<PaginationAction>) -> Self {
        self.on_action = Some(cb);
        self
    }

    fn nav_button(&self, label: Arc<str>, disabled: bool, action: PaginationAction) -> Button {
        let overrides = self.button_overrides[action_index(action)];
        let variant = overrides.variant.unwrap_or(self.button_variant);
        let border_style = overrides.border_style.unwrap_or(self.button_border_style);
        let style = overrides.style.unwrap_or(self.button_style);
        let hover_style = overrides.hover_style.unwrap_or(self.button_hover_style);
        let focus_style = overrides.focus_style.unwrap_or(self.button_focus_style);
        let disabled_style = overrides
            .disabled_style
            .unwrap_or(self.button_disabled_style);

        let mut button = Button::new(label)
            .variant(variant)
            .style(style)
            .hover_style_slot(hover_style)
            .focus_style_slot(focus_style)
            .disabled_style(disabled_style)
            .disabled(disabled);

        if matches!(variant, ButtonVariant::Outlined) {
            button = button.border_style(border_style);
        }

        if let Some(cb) = self.on_action.clone() {
            button = button.on_click(Callback::new(move |_| cb.emit(action)));
        }

        button
    }
}

impl From<PaginationBar> for Element {
    fn from(bar: PaginationBar) -> Self {
        let mut row = HStack::new().gap(bar.gap);

        if bar.show_first_last {
            row = row.child(bar.nav_button(
                bar.labels.first.clone(),
                bar.state.is_first_page(),
                PaginationAction::First,
            ));
        }

        row = row.child(bar.nav_button(
            bar.labels.prev.clone(),
            bar.state.is_first_page(),
            PaginationAction::Prev,
        ));

        let page = bar.state.page() + 1;
        let total_pages = bar.state.total_pages();
        let total = bar.state.total_items();
        let (start, end) = bar.state.range();
        let first_row = if total == 0 {
            0
        } else {
            start.saturating_add(1)
        };
        let info_data = PaginationInfo {
            page_index: bar.state.page(),
            page_number: page,
            total_pages,
            total_items: total,
            per_page: bar.state.per_page(),
            start,
            end,
        };
        let info = if let Some(formatter) = bar.info_formatter.as_ref() {
            formatter(info_data)
        } else if bar.show_range_info {
            Arc::from(format!(
                "Page {}/{}  (rows {}-{} of {})",
                page, total_pages, first_row, end, total
            ))
        } else {
            Arc::from(format!("Page {}/{}", page, total_pages))
        };
        row = row.child(Text::new(info).style(bar.info_style));

        row = row.child(bar.nav_button(
            bar.labels.next.clone(),
            bar.state.is_last_page(),
            PaginationAction::Next,
        ));

        if bar.show_first_last {
            row = row.child(bar.nav_button(
                bar.labels.last.clone(),
                bar.state.is_last_page(),
                PaginationAction::Last,
            ));
        }

        row.into()
    }
}

fn action_index(action: PaginationAction) -> usize {
    match action {
        PaginationAction::First => 0,
        PaginationAction::Prev => 1,
        PaginationAction::Next => 2,
        PaginationAction::Last => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::{PaginationLabels, PaginationState};

    #[test]
    fn clamps_page_to_last_after_total_change() {
        let mut state = PaginationState::new(120, 10);
        state.set_page(9);
        state.set_total_items(15);
        assert_eq!(state.page(), 1);
    }

    #[test]
    fn range_matches_page_window() {
        let mut state = PaginationState::new(53, 10);
        state.set_page(2);
        assert_eq!(state.range(), (20, 30));

        state.last_page();
        assert_eq!(state.range(), (50, 53));
    }

    #[test]
    fn default_labels_are_ascii_navigation_arrows() {
        let labels = PaginationLabels::default();
        assert_eq!(labels.first.as_ref(), "<<");
        assert_eq!(labels.prev.as_ref(), "<");
        assert_eq!(labels.next.as_ref(), ">");
        assert_eq!(labels.last.as_ref(), ">>");
    }
}
