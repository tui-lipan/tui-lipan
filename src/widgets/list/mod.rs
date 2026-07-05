//! List widget.

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::core::event::{KeyEvent, MouseEvent};
use crate::style::{BorderStyle, Length, Padding, ScrollbarConfig, Span, Style, StyleSlot};
use unicode_width::UnicodeWidthStr;

use super::{Spinner, SpinnerSpeed, SpinnerStyle};
use crate::widgets::scroll::{ScrollAction, ScrollKeymap, scroll_action_from_key};

/// Shared configuration for the inner [`List`] used by composite widgets
/// like [`Select`](super::Select), [`ComboBox`](super::ComboBox),
/// [`MultiSelect`](super::MultiSelect), and [`SearchPalette`](super::SearchPalette).
///
/// Each of these widgets composes a `List` and proxies many list-related style
/// fields. `ListConfig` deduplicates the common subset so that a single struct
/// can carry them all.
///
/// Individual convenience setters (e.g. `.list_style(...)`) are still available
/// on each widget; they delegate to the embedded `ListConfig`.
#[derive(Clone, Debug, PartialEq)]
pub struct ListConfig {
    /// Whether to draw a border around the list.
    pub border: bool,
    /// Border style.
    /// Default: `BorderStyle::Plain`.
    pub border_style: BorderStyle,
    /// Inner padding.
    /// Default: `Padding::default()`.
    pub padding: Padding,
    /// Base style.
    pub style: Style,
    /// Style applied to the active_index (selected) item.
    pub selection_style: StyleSlot,
    /// Style applied to the selected item while the list is not focused.
    ///
    /// When unset, `selection_style` is used regardless of focus state.
    pub unfocused_selection_style: StyleSlot,
    /// Per-row hover style. `None` lets each host widget apply its own default
    /// (several fall back to `selection_style`).
    pub item_hover_style: Option<StyleSlot>,
    /// Whether the highlight spans the full row width.
    pub selection_full_width: bool,
    /// Symbol shown on the active_index item.
    pub selection_symbol: Option<Arc<str>>,
    /// Trailing symbol after the selected item's label (pairs with
    /// `selection_symbol` for "pill" caps). Shares `selection_symbol_style`.
    pub selection_symbol_right: Option<Arc<str>>,
    /// Style for the highlight symbol.
    pub selection_symbol_style: Option<Style>,
    /// Style for the highlight symbol while the list is not focused.
    ///
    /// When unset, `selection_symbol_style` is used regardless of focus state.
    pub unfocused_selection_symbol_style: Option<Style>,
    /// Whether to enable the left symbol/status column.
    pub symbol_column: bool,
    /// Explicit gap after the widest item gutter.
    pub gutter_gap: u16,
    /// Whether non-selectable rows participate in gutter alignment.
    pub gutter_for_non_selectable: bool,
    /// Left/right padding for normal rows (interior to the selection highlight).
    pub item_horizontal_padding: Padding,
    /// Left/right padding for header rows.
    pub header_horizontal_padding: Padding,
    /// Style for the empty-state placeholder text.
    pub empty_text_style: Style,
    /// Whether to show a vertical scrollbar when content overflows.
    pub scrollbar: bool,
    /// Scrollbar configuration.
    pub scrollbar_config: ScrollbarConfig,
}

impl Default for ListConfig {
    fn default() -> Self {
        Self {
            border: true,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            style: Style::default(),
            selection_style: StyleSlot::Inherit,
            unfocused_selection_style: StyleSlot::Inherit,
            item_hover_style: None,
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
            scrollbar: false,
            scrollbar_config: ScrollbarConfig::default(),
        }
    }
}

impl ListConfig {
    /// Create a new `ListConfig` with defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set border visibility.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, style: BorderStyle) -> Self {
        self.border_style = style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set highlight style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the theme selection style with this partial overlay.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the active theme selection style.
    pub fn inherit_selection_style(mut self) -> Self {
        self.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set highlight style while the list is not focused.
    pub fn unfocused_selection_style(mut self, style: Style) -> Self {
        self.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the theme unfocused-selection style with this partial overlay.
    pub fn extend_unfocused_selection_style(mut self, style: Style) -> Self {
        self.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the active theme unfocused-selection style.
    pub fn inherit_unfocused_selection_style(mut self) -> Self {
        self.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set whether highlight spans the full row width.
    pub fn selection_full_width(mut self, full_width: bool) -> Self {
        self.selection_full_width = full_width;
        self
    }

    /// Set highlight symbol.
    pub fn selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.selection_symbol = symbol.map(Into::into);
        self
    }

    /// Set the trailing selection symbol (right "pill" cap). Pairs with
    /// [`Self::selection_symbol`] and shares [`Self::selection_symbol_style`].
    pub fn selection_symbol_right(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.selection_symbol_right = symbol.map(Into::into);
        self
    }

    /// Set highlight symbol style.
    pub fn selection_symbol_style(mut self, style: Style) -> Self {
        self.selection_symbol_style = Some(style);
        self
    }

    /// Set highlight symbol style while the list is not focused.
    pub fn unfocused_selection_symbol_style(mut self, style: Style) -> Self {
        self.unfocused_selection_symbol_style = Some(style);
        self
    }

    /// Set whether the left symbol/status column is enabled.
    pub fn symbol_column(mut self, enabled: bool) -> Self {
        self.symbol_column = enabled;
        self
    }

    /// Set the explicit gap after item gutters.
    pub fn gutter_gap(mut self, gap: u16) -> Self {
        self.gutter_gap = gap;
        self
    }

    /// Set whether headers/spacers participate in gutter column alignment.
    pub fn gutter_for_non_selectable(mut self, enabled: bool) -> Self {
        self.gutter_for_non_selectable = enabled;
        self
    }

    /// Set the per-row hover style.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.item_hover_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Set the per-row hover style slot directly.
    pub fn item_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.item_hover_style = Some(slot);
        self
    }

    /// Set left/right padding for normal rows (interior to the highlight).
    pub fn item_horizontal_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.item_horizontal_padding = padding.into();
        self
    }

    /// Set left/right padding for header rows.
    pub fn header_horizontal_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.header_horizontal_padding = padding.into();
        self
    }

    /// Set the empty-state placeholder text style.
    pub fn empty_text_style(mut self, style: Style) -> Self {
        self.empty_text_style = style;
        self
    }

    /// Set scrollbar visibility.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.scrollbar_config = config;
        self
    }
}

/// A list selection event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ListEvent {
    /// Selected item index.
    pub index: usize,
}

/// Semantic role of a list row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ListItemRole {
    /// Regular selectable row.
    #[default]
    Normal,
    /// Non-selectable section/header row.
    Header,
    /// Non-selectable blank spacer row.
    Spacer,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ListItemPrefixKind {
    Plain,
    Numbered(usize),
}

/// Placement of list symbols relative to the main label content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ListSymbolPosition {
    /// Render the symbol in the left symbol column.
    #[default]
    Left,
    /// Render the symbol immediately after the label content.
    Right,
}

/// A fixed-width left gutter rendered before a list row label.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItemGutter {
    pub(crate) kind: ListItemGutterKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ListItemGutterKind {
    Text(Vec<Span>),
    Spinner(ListItemSpinnerGutter),
}

/// Per-row status content rendered in the list symbol column.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItemStatus {
    pub(crate) kind: ListItemStatusKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ListItemStatusKind {
    Text(Vec<Span>),
    Spinner(ListItemSpinnerGutter),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ListItemSpinnerGutter {
    pub spinner_style: SpinnerStyle,
    pub speed: SpinnerSpeed,
    pub frame: usize,
    pub auto_frame: bool,
    pub label: Option<Arc<str>>,
    pub gap: u16,
    pub style: Style,
    pub label_style: Style,
}

impl ListItemGutter {
    /// Create a plain text gutter.
    pub fn text(text: impl Into<Arc<str>>) -> Self {
        Self::from_spans([Span::new(text)])
    }

    /// Create a rich-text gutter.
    pub fn from_spans(spans: impl IntoIterator<Item = Span>) -> Self {
        Self {
            kind: ListItemGutterKind::Text(spans.into_iter().collect()),
        }
    }

    /// Create a spinner gutter.
    pub fn spinner(spinner: Spinner) -> Self {
        spinner.into()
    }

    pub(crate) fn width(&self) -> u16 {
        match &self.kind {
            ListItemGutterKind::Text(spans) => spans
                .iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum::<usize>()
                .min(u16::MAX as usize) as u16,
            ListItemGutterKind::Spinner(spinner) => {
                let label_width = spinner
                    .label
                    .as_ref()
                    .map(|label| UnicodeWidthStr::width(label.as_ref()) as u16)
                    .unwrap_or(0);
                let gap = if label_width > 0 { spinner.gap } else { 0 };
                spinner
                    .spinner_style
                    .width()
                    .saturating_add(gap)
                    .saturating_add(label_width)
            }
        }
    }

    pub(crate) fn has_spinner(&self) -> bool {
        matches!(self.kind, ListItemGutterKind::Spinner(_))
    }

    pub(crate) fn spinner_mut(&mut self) -> Option<&mut ListItemSpinnerGutter> {
        match &mut self.kind {
            ListItemGutterKind::Spinner(spinner) => Some(spinner),
            ListItemGutterKind::Text(_) => None,
        }
    }
}

impl From<Spinner> for ListItemGutter {
    fn from(spinner: Spinner) -> Self {
        Self {
            kind: ListItemGutterKind::Spinner(ListItemSpinnerGutter {
                spinner_style: spinner.spinner_style,
                speed: spinner.speed,
                frame: spinner.frame.unwrap_or(0),
                auto_frame: spinner.frame.is_none(),
                label: spinner.label,
                gap: spinner.gap,
                style: spinner.style,
                label_style: spinner.label_style,
            }),
        }
    }
}

impl ListItemStatus {
    /// Create a plain-text status symbol.
    pub fn text(text: impl Into<Arc<str>>) -> Self {
        Self::from_spans([Span::new(text)])
    }

    /// Create a rich-text status symbol.
    pub fn from_spans(spans: impl IntoIterator<Item = Span>) -> Self {
        Self {
            kind: ListItemStatusKind::Text(spans.into_iter().collect()),
        }
    }

    /// Create an animated spinner status symbol.
    pub fn spinner(spinner: Spinner) -> Self {
        spinner.into()
    }

    pub(crate) fn width(&self) -> u16 {
        match &self.kind {
            ListItemStatusKind::Text(spans) => spans
                .iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum::<usize>()
                .min(u16::MAX as usize) as u16,
            ListItemStatusKind::Spinner(spinner) => spinner.spinner_style.width(),
        }
    }

    pub(crate) fn has_spinner(&self) -> bool {
        matches!(self.kind, ListItemStatusKind::Spinner(_))
    }

    pub(crate) fn spinner_mut(&mut self) -> Option<&mut ListItemSpinnerGutter> {
        match &mut self.kind {
            ListItemStatusKind::Spinner(spinner) => Some(spinner),
            ListItemStatusKind::Text(_) => None,
        }
    }
}

impl From<Spinner> for ListItemStatus {
    fn from(spinner: Spinner) -> Self {
        Self {
            kind: ListItemStatusKind::Spinner(ListItemSpinnerGutter {
                spinner_style: spinner.spinner_style,
                speed: spinner.speed,
                frame: spinner.frame.unwrap_or(0),
                auto_frame: spinner.frame.is_none(),
                label: None,
                gap: 0,
                style: spinner.style,
                label_style: spinner.label_style,
            }),
        }
    }
}

/// A list item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem {
    pub(crate) spans: Vec<Span>,
    pub(crate) description_spans: Vec<Span>,
    pub(crate) extra_lines: Vec<ListItemLine>,
    pub(crate) status: Option<ListItemStatus>,
    pub(crate) gutter: Option<ListItemGutter>,
    pub(crate) gutter_line: usize,
    pub(crate) prefix: Option<Arc<str>>,
    pub(crate) prefix_kind: ListItemPrefixKind,
    pub(crate) prefix_style: Option<Style>,
    pub(crate) extra_line_indent: u16,
    pub(crate) style: Style,
    pub(crate) role: ListItemRole,
    pub(crate) active: bool,
    pub(crate) primary_selection_label: bool,
    pub(crate) primary_selection_description: bool,
    pub(crate) primary_hover_label: bool,
    pub(crate) primary_hover_description: bool,
    pub(crate) primary_truncate_description_first: bool,
    pub(crate) primary_wrap_label: bool,
    pub(crate) primary_wrap_description: bool,
    pub(crate) primary_max_label_width: Option<u16>,
    pub(crate) primary_max_description_width: Option<u16>,
    pub(crate) symbol_line: usize,
}

/// An additional rendered line for a [`ListItem`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItemLine {
    pub(crate) spans: Vec<Span>,
    pub(crate) description_spans: Vec<Span>,
    pub(crate) style: Style,
    pub(crate) selection_label: bool,
    pub(crate) selection_description: bool,
    pub(crate) hover_label: bool,
    pub(crate) hover_description: bool,
    pub(crate) truncate_description_first: bool,
    pub(crate) wrap_label: bool,
    pub(crate) wrap_description: bool,
    pub(crate) max_label_width: Option<u16>,
    pub(crate) max_description_width: Option<u16>,
}

impl ListItemLine {
    /// Create a new line.
    pub fn new(content: impl Into<Arc<str>>) -> Self {
        Self {
            spans: vec![Span::new(content)],
            description_spans: Vec::new(),
            style: Style::default(),
            selection_label: true,
            selection_description: true,
            hover_label: true,
            hover_description: true,
            truncate_description_first: false,
            wrap_label: false,
            wrap_description: false,
            max_label_width: None,
            max_description_width: None,
        }
    }

    /// Create from multiple spans.
    pub fn from_spans(spans: impl IntoIterator<Item = Span>) -> Self {
        Self {
            spans: spans.into_iter().collect(),
            description_spans: Vec::new(),
            style: Style::default(),
            selection_label: true,
            selection_description: true,
            hover_label: true,
            hover_description: true,
            truncate_description_first: false,
            wrap_label: false,
            wrap_description: false,
            max_label_width: None,
            max_description_width: None,
        }
    }

    /// Add description (right-aligned) content.
    pub fn description_spans(mut self, spans: impl IntoIterator<Item = Span>) -> Self {
        self.description_spans = spans.into_iter().collect();
        self
    }

    /// Set description as plain text (creates a single unstyled span).
    pub fn description(mut self, text: impl Into<Arc<str>>) -> Self {
        self.description_spans = vec![Span::new(text)];
        self
    }

    /// Apply a style to all current description spans.
    pub fn description_style(mut self, style: Style) -> Self {
        for span in &mut self.description_spans {
            span.style = style;
        }
        self
    }

    /// Set line style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Control whether selection highlight applies to the label side.
    pub fn selection_label(mut self, highlight: bool) -> Self {
        self.selection_label = highlight;
        self
    }

    /// Control whether selection highlight applies to the description side.
    pub fn selection_description(mut self, highlight: bool) -> Self {
        self.selection_description = highlight;
        self
    }

    /// Control whether hover style applies to the label side.
    pub fn hover_label(mut self, hover: bool) -> Self {
        self.hover_label = hover;
        self
    }

    /// Control whether hover style applies to the description side.
    pub fn hover_description(mut self, hover: bool) -> Self {
        self.hover_description = hover;
        self
    }

    /// Prefer truncating description content before label content.
    pub fn truncate_description_first(mut self, truncate: bool) -> Self {
        self.truncate_description_first = truncate;
        self
    }

    /// Wrap label content onto additional lines when needed.
    pub fn wrap_label(mut self, wrap: bool) -> Self {
        self.wrap_label = wrap;
        self
    }

    /// Wrap description content onto extra lines when needed.
    pub fn wrap_description(mut self, wrap: bool) -> Self {
        self.wrap_description = wrap;
        self
    }

    /// Limit the label to at most this many columns. Surplus space goes to the description.
    pub fn max_label_width(mut self, width: u16) -> Self {
        self.max_label_width = Some(width);
        self
    }

    /// Limit the description to at most this many columns. Surplus space goes to the label.
    pub fn max_description_width(mut self, width: u16) -> Self {
        self.max_description_width = Some(width);
        self
    }
}

impl ListItem {
    /// Create a new list item.
    pub fn new(content: impl Into<Arc<str>>) -> Self {
        Self {
            spans: vec![Span::new(content)],
            description_spans: Vec::new(),
            extra_lines: Vec::new(),
            status: None,
            gutter: None,
            gutter_line: 0,
            prefix: None,
            prefix_kind: ListItemPrefixKind::Plain,
            prefix_style: None,
            extra_line_indent: 0,
            style: Style::default(),
            role: ListItemRole::Normal,
            active: false,
            primary_selection_label: true,
            primary_selection_description: true,
            primary_hover_label: true,
            primary_hover_description: true,
            primary_truncate_description_first: false,
            primary_wrap_label: false,
            primary_wrap_description: false,
            primary_max_label_width: None,
            primary_max_description_width: None,
            symbol_line: 0,
        }
    }

    /// Create a non-selectable section/header row.
    pub fn header(content: impl Into<Arc<str>>) -> Self {
        Self::new(content)
            .role(ListItemRole::Header)
            .style(Style::default())
    }

    /// Create a non-selectable blank spacer row.
    pub fn spacer() -> Self {
        Self {
            spans: vec![Span::new("")],
            description_spans: Vec::new(),
            extra_lines: Vec::new(),
            status: None,
            gutter: None,
            gutter_line: 0,
            prefix: None,
            prefix_kind: ListItemPrefixKind::Plain,
            prefix_style: None,
            extra_line_indent: 0,
            style: Style::default(),
            role: ListItemRole::Spacer,
            active: false,
            primary_selection_label: true,
            primary_selection_description: true,
            primary_hover_label: true,
            primary_hover_description: true,
            primary_truncate_description_first: false,
            primary_wrap_label: false,
            primary_wrap_description: false,
            primary_max_label_width: None,
            primary_max_description_width: None,
            symbol_line: 0,
        }
    }

    /// Create from multiple spans.
    pub fn from_spans(spans: impl IntoIterator<Item = Span>) -> Self {
        Self {
            spans: spans.into_iter().collect(),
            description_spans: Vec::new(),
            extra_lines: Vec::new(),
            status: None,
            gutter: None,
            gutter_line: 0,
            prefix: None,
            prefix_kind: ListItemPrefixKind::Plain,
            prefix_style: None,
            extra_line_indent: 0,
            style: Style::default(),
            role: ListItemRole::Normal,
            active: false,
            primary_selection_label: true,
            primary_selection_description: true,
            primary_hover_label: true,
            primary_hover_description: true,
            primary_truncate_description_first: false,
            primary_wrap_label: false,
            primary_wrap_description: false,
            primary_max_label_width: None,
            primary_max_description_width: None,
            symbol_line: 0,
        }
    }

    /// Add description (right-aligned) content.
    pub fn description_spans(mut self, spans: impl IntoIterator<Item = Span>) -> Self {
        self.description_spans = spans.into_iter().collect();
        self
    }

    /// Set description as plain text (creates a single unstyled span).
    pub fn description(mut self, text: impl Into<Arc<str>>) -> Self {
        self.description_spans = vec![Span::new(text)];
        self
    }

    /// Apply a style to all current description spans.
    pub fn description_style(mut self, style: Style) -> Self {
        for span in &mut self.description_spans {
            span.style = style;
        }
        self
    }

    /// Add an extra rendered line below the primary line.
    pub fn line(mut self, line: impl Into<ListItemLine>) -> Self {
        self.extra_lines.push(line.into());
        self
    }

    /// Set extra lines below the primary line.
    pub fn lines(mut self, lines: impl IntoIterator<Item = ListItemLine>) -> Self {
        self.extra_lines = lines.into_iter().collect();
        self
    }

    /// Set a fixed-width left gutter rendered before the primary label column.
    pub fn gutter(mut self, gutter: impl Into<ListItemGutter>) -> Self {
        self.gutter = Some(gutter.into());
        self
    }

    /// Set per-row status content in the existing list symbol column.
    ///
    /// The status is used only when the row is not showing an active or selected
    /// symbol. This is useful for status markers such as a busy spinner without
    /// creating an extra label gutter column.
    pub fn status(mut self, status: impl Into<ListItemStatus>) -> Self {
        self.status = Some(status.into());
        self
    }

    /// Set a plain-text per-row status symbol.
    pub fn status_symbol(self, symbol: impl Into<Arc<str>>) -> Self {
        self.status(ListItemStatus::text(symbol))
    }

    /// Set an animated per-row status spinner.
    pub fn status_spinner(self, spinner: Spinner) -> Self {
        self.status(ListItemStatus::spinner(spinner))
    }

    /// Set which visual line carries the gutter.
    ///
    /// `0` means primary line, `1` means first extra line, etc.
    pub fn gutter_line(mut self, line: usize) -> Self {
        self.gutter_line = line;
        self
    }

    /// Set a text prefix for the primary line.
    pub fn prefix(mut self, prefix: impl Into<Arc<str>>) -> Self {
        let prefix = prefix.into();
        self.extra_line_indent =
            UnicodeWidthStr::width(prefix.as_ref()).min(u16::MAX as usize) as u16;
        self.prefix = Some(prefix);
        self.prefix_kind = ListItemPrefixKind::Plain;
        self
    }

    /// Set style for the primary-line prefix.
    pub fn prefix_style(mut self, style: Style) -> Self {
        self.prefix_style = Some(style);
        self
    }

    /// Prefix the row with `"N. "`.
    pub fn numbered(mut self, n: usize) -> Self {
        let prefix = format!("{n}. ");
        self.extra_line_indent =
            UnicodeWidthStr::width(prefix.as_str()).min(u16::MAX as usize) as u16;
        self.prefix = Some(prefix.into());
        self.prefix_kind = ListItemPrefixKind::Numbered(n);
        self
    }

    /// Prefix the row with `"<ch> "`.
    pub fn bulleted(self, ch: char) -> Self {
        self.prefix(format!("{ch} "))
    }

    /// Set extra-line indentation width in terminal columns.
    pub fn extra_line_indent(mut self, indent: u16) -> Self {
        self.extra_line_indent = indent;
        self
    }

    /// Control whether selection highlight applies to primary label content.
    pub fn primary_selection_label(mut self, highlight: bool) -> Self {
        self.primary_selection_label = highlight;
        self
    }

    /// Control whether selection highlight applies to primary description content.
    pub fn primary_selection_description(mut self, highlight: bool) -> Self {
        self.primary_selection_description = highlight;
        self
    }

    /// Control whether hover style applies to primary label content.
    pub fn primary_hover_label(mut self, hover: bool) -> Self {
        self.primary_hover_label = hover;
        self
    }

    /// Control whether hover style applies to primary description content.
    pub fn primary_hover_description(mut self, hover: bool) -> Self {
        self.primary_hover_description = hover;
        self
    }

    /// Prefer truncating primary description content before primary label content.
    pub fn primary_truncate_description_first(mut self, truncate: bool) -> Self {
        self.primary_truncate_description_first = truncate;
        self
    }

    /// Wrap primary label content onto extra lines when needed.
    pub fn primary_wrap_label(mut self, wrap: bool) -> Self {
        self.primary_wrap_label = wrap;
        self
    }

    /// Wrap primary description content onto extra lines when needed.
    pub fn primary_wrap_description(mut self, wrap: bool) -> Self {
        self.primary_wrap_description = wrap;
        self
    }

    /// Limit the primary label to at most this many columns.
    pub fn primary_max_label_width(mut self, width: u16) -> Self {
        self.primary_max_label_width = Some(width);
        self
    }

    /// Limit the primary description to at most this many columns.
    pub fn primary_max_description_width(mut self, width: u16) -> Self {
        self.primary_max_description_width = Some(width);
        self
    }

    /// Set which visual line carries the symbol column.
    ///
    /// `0` means primary line, `1` means first extra line, etc.
    pub fn symbol_line(mut self, line: usize) -> Self {
        self.symbol_line = line;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set semantic row role.
    pub fn role(mut self, role: ListItemRole) -> Self {
        self.role = role;
        self
    }

    /// Mark this row as active.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Whether this row is keyboard/mouse selectable.
    pub fn is_selectable(&self) -> bool {
        matches!(self.role, ListItemRole::Normal)
    }

    /// Whether this row is active.
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Returns the concatenated plain text content.
    pub fn plain_content(&self) -> String {
        let mut s = String::new();
        for span in &self.spans {
            s.push_str(&span.content);
        }
        for line in &self.extra_lines {
            s.push('\n');
            for span in &line.spans {
                s.push_str(&span.content);
            }
        }
        s
    }

    pub(crate) fn line_count(&self) -> usize {
        1 + self.extra_lines.len()
    }
}

impl From<&'static str> for ListItemLine {
    fn from(value: &'static str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ListItemLine {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<Arc<str>> for ListItemLine {
    fn from(value: Arc<str>) -> Self {
        Self::new(value)
    }
}

impl From<&'static str> for ListItem {
    fn from(value: &'static str) -> Self {
        Self::new(value)
    }
}

impl From<String> for ListItem {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<Arc<str>> for ListItem {
    fn from(value: Arc<str>) -> Self {
        Self::new(value)
    }
}

pub(crate) fn reserved_symbol_width_for_items(
    items: &[ListItem],
    symbol_column: bool,
    active_symbol_position: ListSymbolPosition,
    active_symbol: Option<&str>,
    selection_symbol: Option<&str>,
    unselected_symbol: Option<&str>,
) -> u16 {
    if !symbol_column {
        return 0;
    }

    let status_width = items
        .iter()
        .filter(|item| item.is_selectable())
        .filter_map(|item| item.status.as_ref())
        .map(ListItemStatus::width)
        .max()
        .unwrap_or(0) as usize;

    selection_symbol
        .map(UnicodeWidthStr::width)
        .unwrap_or(0)
        .max(
            if matches!(active_symbol_position, ListSymbolPosition::Left) {
                active_symbol.map(UnicodeWidthStr::width).unwrap_or(0)
            } else {
                0
            },
        )
        .max(unselected_symbol.map(UnicodeWidthStr::width).unwrap_or(0))
        .max(status_width)
        .min(u16::MAX as usize) as u16
}

pub(crate) fn reserved_symbol_width(list: &List) -> u16 {
    reserved_symbol_width_for_items(
        &list.items,
        list.symbol_column,
        list.active_symbol_position,
        list.active_symbol.as_deref(),
        list.selection_symbol.as_deref(),
        list.unselected_symbol.as_deref(),
    )
}

pub(crate) struct ListSymbolWidthCtx<'a> {
    pub active_symbol_position: ListSymbolPosition,
    pub active_symbol: Option<&'a str>,
    pub selection_symbol: Option<&'a str>,
    pub unselected_symbol: Option<&'a str>,
}

pub(crate) fn item_symbol_width_for_reserved(
    reserved: u16,
    item: &ListItem,
    is_selected: bool,
    ctx: ListSymbolWidthCtx<'_>,
) -> u16 {
    let ListSymbolWidthCtx {
        active_symbol_position,
        active_symbol,
        selection_symbol,
        unselected_symbol,
    } = ctx;
    if reserved == 0 || !item.is_selectable() {
        return 0;
    }

    if matches!(active_symbol_position, ListSymbolPosition::Left)
        && item.is_active()
        && let Some(symbol) = active_symbol
    {
        return UnicodeWidthStr::width(symbol).min(u16::MAX as usize) as u16;
    }

    if item.status.is_some() {
        return reserved;
    }

    if is_selected {
        return selection_symbol
            .map(|symbol| UnicodeWidthStr::width(symbol).min(u16::MAX as usize) as u16)
            .unwrap_or(reserved);
    }

    unselected_symbol
        .map(|symbol| UnicodeWidthStr::width(symbol).min(u16::MAX as usize) as u16)
        .unwrap_or(reserved)
}

pub(crate) fn item_symbol_width(list: &List, item: &ListItem, is_selected: bool) -> u16 {
    item_symbol_width_for_reserved(
        reserved_symbol_width(list),
        item,
        is_selected,
        ListSymbolWidthCtx {
            active_symbol_position: list.active_symbol_position,
            active_symbol: list.active_symbol.as_deref(),
            selection_symbol: list.selection_symbol.as_deref(),
            unselected_symbol: list.unselected_symbol.as_deref(),
        },
    )
}

pub(crate) fn item_active_right_symbol_width(list: &List, item: &ListItem) -> u16 {
    if matches!(list.active_symbol_position, ListSymbolPosition::Right)
        && item.is_active()
        && let Some(symbol) = list.active_symbol.as_deref()
    {
        UnicodeWidthStr::width(symbol).min(u16::MAX as usize) as u16
    } else {
        0
    }
}

/// Width of the trailing selection symbol for a given row.
///
/// Returns 0 unless the row is the selected, selectable row and a
/// `selection_symbol_right` is configured. A right-positioned `active_symbol`
/// occupies the same trailing slot and takes priority, so this returns 0 when
/// that symbol applies to the row to avoid double-reserving width.
pub(crate) fn item_selection_right_symbol_width(
    list: &List,
    item: &ListItem,
    is_selected: bool,
) -> u16 {
    if is_selected
        && item.is_selectable()
        && item_active_right_symbol_width(list, item) == 0
        && let Some(symbol) = list.selection_symbol_right.as_deref()
    {
        UnicodeWidthStr::width(symbol).min(u16::MAX as usize) as u16
    } else {
        0
    }
}

pub(crate) fn item_uses_gutter(item: &ListItem, gutter_for_non_selectable: bool) -> bool {
    item.is_selectable() || gutter_for_non_selectable
}

pub(crate) fn reserved_gutter_width_for_items(
    items: &[ListItem],
    gutter_gap: u16,
    gutter_for_non_selectable: bool,
) -> u16 {
    let width = items
        .iter()
        .filter(|item| item_uses_gutter(item, gutter_for_non_selectable))
        .filter_map(|item| item.gutter.as_ref())
        .map(ListItemGutter::width)
        .max()
        .unwrap_or(0);
    if width > 0 {
        width.saturating_add(gutter_gap)
    } else {
        0
    }
}

pub(crate) fn reserved_gutter_width(list: &List) -> u16 {
    reserved_gutter_width_for_items(&list.items, list.gutter_gap, list.gutter_for_non_selectable)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ListLeadingMetrics {
    pub symbol_width: u16,
    pub gutter_width: u16,
    pub active_right_symbol_width: u16,
    pub selection_right_symbol_width: u16,
}

pub(crate) fn leading_metrics(
    list: &List,
    item: &ListItem,
    is_selected: bool,
) -> ListLeadingMetrics {
    ListLeadingMetrics {
        symbol_width: item_symbol_width(list, item, is_selected),
        gutter_width: if item_uses_gutter(item, list.gutter_for_non_selectable) {
            reserved_gutter_width(list)
        } else {
            0
        },
        active_right_symbol_width: item_active_right_symbol_width(list, item),
        selection_right_symbol_width: item_selection_right_symbol_width(list, item, is_selected),
    }
}

pub(crate) fn max_numbered_prefix_width(list: &List) -> u16 {
    list.items
        .iter()
        .filter_map(|item| match item.prefix_kind {
            ListItemPrefixKind::Numbered(n) => Some(n),
            ListItemPrefixKind::Plain => None,
        })
        .map(|n| UnicodeWidthStr::width(format!("{n}. ").as_str()))
        .max()
        .unwrap_or(0)
        .min(u16::MAX as usize) as u16
}

pub(crate) fn max_numbered_prefix_width_for_items(items: &[ListItem]) -> u16 {
    items
        .iter()
        .filter_map(|item| match item.prefix_kind {
            ListItemPrefixKind::Numbered(n) => Some(n),
            ListItemPrefixKind::Plain => None,
        })
        .map(|n| UnicodeWidthStr::width(format!("{n}. ").as_str()))
        .max()
        .unwrap_or(0)
        .min(u16::MAX as usize) as u16
}

pub(crate) fn effective_prefix_for_width<'a>(
    item: &'a ListItem,
    numbered_prefix_width: u16,
) -> Option<std::borrow::Cow<'a, str>> {
    match (&item.prefix, &item.prefix_kind) {
        (Some(prefix), ListItemPrefixKind::Numbered(n)) => {
            let target_width = numbered_prefix_width as usize;
            let text = format!("{n}. ");
            let width = UnicodeWidthStr::width(text.as_str());
            if target_width > width {
                Some(std::borrow::Cow::Owned(format!(
                    "{}{}",
                    " ".repeat(target_width - width),
                    text
                )))
            } else {
                Some(std::borrow::Cow::Borrowed(prefix.as_ref()))
            }
        }
        (Some(prefix), ListItemPrefixKind::Plain) => {
            Some(std::borrow::Cow::Borrowed(prefix.as_ref()))
        }
        (None, _) => None,
    }
}

pub(crate) fn effective_extra_line_indent_for_width(
    item: &ListItem,
    numbered_prefix_width: u16,
) -> u16 {
    match item.prefix_kind {
        ListItemPrefixKind::Numbered(_) => numbered_prefix_width,
        ListItemPrefixKind::Plain => item.extra_line_indent,
    }
}

pub(crate) fn effective_prefix<'a>(
    list: &List,
    item: &'a ListItem,
) -> Option<std::borrow::Cow<'a, str>> {
    effective_prefix_for_width(item, max_numbered_prefix_width(list))
}

pub(crate) fn effective_extra_line_indent(list: &List, item: &ListItem) -> u16 {
    effective_extra_line_indent_for_width(item, max_numbered_prefix_width(list))
}

/// A vertically scrolling list.
#[derive(Clone)]
pub struct List {
    pub(crate) items: Arc<[ListItem]>,
    pub(crate) selected: usize,
    pub(crate) scroll_keys: ScrollKeymap,
    pub(crate) scroll_wheel: bool,
    pub(crate) style: Style,
    pub(crate) hover_style: StyleSlot,
    pub(crate) item_hover_style: StyleSlot,
    pub(crate) active_style: StyleSlot,
    pub(crate) selection_style: StyleSlot,
    pub(crate) unfocused_selection_style: StyleSlot,
    pub(crate) active_symbol: Option<Arc<str>>,
    pub(crate) active_symbol_position: ListSymbolPosition,
    pub(crate) active_symbol_style: Option<Style>,
    pub(crate) selection_symbol: Option<Arc<str>>,
    pub(crate) selection_symbol_right: Option<Arc<str>>,
    pub(crate) selection_symbol_style: Option<Style>,
    pub(crate) unfocused_selection_symbol_style: Option<Style>,
    pub(crate) unselected_symbol: Option<Arc<str>>,
    pub(crate) symbol_column: bool,
    pub(crate) gutter_gap: u16,
    pub(crate) gutter_for_non_selectable: bool,
    pub(crate) selection_full_width: bool,
    pub(crate) item_horizontal_padding: Padding,
    pub(crate) header_horizontal_padding: Padding,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) title: Option<Arc<str>>,
    pub(crate) title_style: Style,
    pub(crate) padding: Padding,
    pub(crate) scrollbar: bool,
    pub(crate) scrollbar_config: ScrollbarConfig,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) on_select: Option<Callback<ListEvent>>,
    pub(crate) on_item_click: Option<Callback<ListEvent>>,
    pub(crate) on_activate: Option<Callback<ListEvent>>,
    pub(crate) on_click: Option<Callback<MouseEvent>>,
    pub(crate) activate_on_click: bool,
    pub(crate) on_scroll_to: Option<Callback<usize>>,
    pub(crate) on_key: Option<KeyHandler>,
    pub(crate) disabled: bool,
    pub(crate) disabled_style: Style,
    pub(crate) focusable: bool,
    pub(crate) show_scroll_indicators: bool,
    pub(crate) scroll_indicator_style: Style,
    pub(crate) empty_text: Option<Arc<str>>,
    pub(crate) empty_text_style: Style,
    pub(crate) force_scroll_to_selected: bool,
}

impl Default for List {
    fn default() -> Self {
        Self {
            items: Arc::new([]),
            selected: 0,
            scroll_keys: ScrollKeymap::default(),
            scroll_wheel: true,
            style: Style::default(),
            hover_style: StyleSlot::Inherit,
            item_hover_style: StyleSlot::Inherit,
            active_style: StyleSlot::Inherit,
            selection_style: StyleSlot::Inherit,
            unfocused_selection_style: StyleSlot::Inherit,
            active_symbol: None,
            active_symbol_position: ListSymbolPosition::Left,
            active_symbol_style: None,
            selection_symbol: None,
            selection_symbol_right: None,
            selection_symbol_style: None,
            unfocused_selection_symbol_style: None,
            unselected_symbol: None,
            symbol_column: true,
            gutter_gap: 0,
            gutter_for_non_selectable: false,
            selection_full_width: false,
            item_horizontal_padding: Padding::default(),
            header_horizontal_padding: Padding::default(),
            border: false,
            border_style: BorderStyle::Plain,
            title: None,
            title_style: Style::default(),
            padding: Padding::default(),
            scrollbar: false,
            scrollbar_config: ScrollbarConfig::default(),
            width: Length::Flex(1),
            height: Length::Flex(1),
            on_select: None,
            on_item_click: None,
            on_activate: None,
            on_click: None,
            activate_on_click: true,
            on_scroll_to: None,
            on_key: None,
            disabled: false,
            disabled_style: Style::default(),
            focusable: true,
            show_scroll_indicators: false,
            scroll_indicator_style: Style::default(),
            empty_text: None,
            empty_text_style: Style::default(),
            force_scroll_to_selected: false,
        }
    }
}

impl List {
    /// Create an empty list.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace list items.
    pub fn items<I>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = ListItem>,
    {
        self.items = items.into_iter().collect();
        self
    }

    /// Add an item.
    ///
    /// Note: This clones the internal Arc if it's shared, which might be expensive.
    /// For bulk updates, use `items()`.
    pub fn item(mut self, item: impl Into<ListItem>) -> Self {
        let mut items = self.items.to_vec();
        items.push(item.into());
        self.items = items.into();
        self
    }

    /// Set items from a shared slice.
    ///
    /// This is efficient for avoiding allocations when the list items don't change often.
    pub fn items_arc(mut self, items: Arc<[ListItem]>) -> Self {
        self.items = items;
        self
    }

    /// Set selected item index.
    pub fn selected(mut self, selected: usize) -> Self {
        self.selected = selected;
        self
    }

    /// Force scroll to make the selected item visible on next render.
    ///
    /// Use this when you want the list to jump directly to the selected item,
    /// bypassing the smart scroll logic that normally preserves scroll position.
    /// This is useful for auto-follow scenarios where you always want to show
    /// the latest item.
    pub fn force_scroll_to_selected(mut self, force: bool) -> Self {
        self.force_scroll_to_selected = force;
        self
    }

    /// Configure which keys move the selection.
    pub fn scroll_keys(mut self, keys: ScrollKeymap) -> Self {
        self.scroll_keys = keys;
        self
    }

    /// Enable mouse wheel selection changes.
    pub fn scroll_wheel(mut self, enabled: bool) -> Self {
        self.scroll_wheel = enabled;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set style when list is hovered.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the theme hover style with this partial overlay.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the active theme hover style.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hover style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_style = slot;
        self
    }

    /// Set style for hovered items.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.item_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the theme item-hover style with this partial overlay.
    pub fn extend_item_hover_style(mut self, style: Style) -> Self {
        self.item_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the active theme item-hover style.
    pub fn inherit_item_hover_style(mut self) -> Self {
        self.item_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set item-hover style slot directly for composite forwarding.
    pub fn item_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.item_hover_style = slot;
        self
    }

    /// Set style for active rows.
    pub fn active_style(mut self, style: Style) -> Self {
        self.active_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the theme active style with this partial overlay.
    pub fn extend_active_style(mut self, style: Style) -> Self {
        self.active_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the active theme active-row style.
    pub fn inherit_active_style(mut self) -> Self {
        self.active_style = StyleSlot::Inherit;
        self
    }

    /// Set active style slot directly for composite forwarding.
    pub fn active_style_slot(mut self, slot: StyleSlot) -> Self {
        self.active_style = slot;
        self
    }

    /// Set selected item style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the theme selection style with this partial overlay.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the active theme selection style.
    pub fn inherit_selection_style(mut self) -> Self {
        self.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set selection style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.selection_style = slot;
        self
    }

    /// Set selected item style while the list is not focused.
    ///
    /// When unset, [`Self::selection_style`] is used regardless of focus state.
    pub fn unfocused_selection_style(mut self, style: Style) -> Self {
        self.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the theme unfocused-selection style with this partial overlay.
    pub fn extend_unfocused_selection_style(mut self, style: Style) -> Self {
        self.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the active theme unfocused-selection style.
    pub fn inherit_unfocused_selection_style(mut self) -> Self {
        self.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set unfocused-selection style slot directly for composite forwarding.
    pub fn unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.unfocused_selection_style = slot;
        self
    }

    /// Set the symbol shown on the selected (active_index) item.
    ///
    /// Symbols are shown with a consistent left-aligned prefix column whose width
    /// is the maximum of `selection_symbol`, `unselected_symbol`, and `active_symbol`
    /// when `active_symbol_position` is [`ListSymbolPosition::Left`].
    ///
    /// Symbol rendering priority per row:
    /// 1. `active_symbol` - shown whenever the item is active (overrides highlight).
    /// 2. `selection_symbol` - shown for the selected item when not active.
    /// 3. `unselected_symbol` - shown for all other rows.
    /// 4. Spaces - auto-padding to the reserved column width when no symbol matches.
    pub fn selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.selection_symbol = symbol.map(Into::into);
        self
    }

    /// Set a trailing symbol rendered immediately after the selected row's
    /// label content.
    ///
    /// This mirrors [`Self::selection_symbol`] (the leading symbol) on the right
    /// side, enabling "pill"/capsule selection styles: pair a left cap (e.g.
    /// `""`) via `selection_symbol` with a right cap (e.g. `""`) here, and
    /// color both with [`Self::selection_symbol_style`] so their foreground
    /// equals the selection background. Combine with
    /// `selection_full_width(false)` so the highlight hugs the label and the
    /// caps sit on the surrounding background.
    ///
    /// The trailing symbol shares [`Self::selection_symbol_style`] /
    /// [`Self::unfocused_selection_symbol_style`] with the leading symbol — both
    /// caps of a pill are the same color by definition. It renders only on the
    /// selected row's symbol line, in the same position as a right-positioned
    /// [`Self::active_symbol`]; when a row is both selected and shows a
    /// right-positioned active symbol, the active symbol takes priority.
    pub fn selection_symbol_right(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.selection_symbol_right = symbol.map(Into::into);
        self
    }

    /// Set the symbol shown on active rows.
    ///
    /// When `active_symbol_position` is [`ListSymbolPosition::Left`], `active_symbol`
    /// takes priority over `selection_symbol`, even when the item is both active and
    /// selected. When set to [`ListSymbolPosition::Right`], the active symbol is rendered
    /// immediately after the label content instead.
    pub fn active_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.active_symbol = symbol.map(Into::into);
        self
    }

    /// Set where the active symbol is rendered.
    pub fn active_symbol_position(mut self, position: ListSymbolPosition) -> Self {
        self.active_symbol_position = position;
        self
    }

    /// Set style for the active symbol.
    ///
    /// If not set, it defaults to the computed row style.
    pub fn active_symbol_style(mut self, style: Style) -> Self {
        self.active_symbol_style = Some(style);
        self
    }

    /// Set style for the highlight symbol.
    ///
    /// If not set, it defaults to the computed row style (with `selection_style` applied).
    pub fn selection_symbol_style(mut self, style: Style) -> Self {
        self.selection_symbol_style = Some(style);
        self
    }

    /// Set selected item symbol style while the list is not focused.
    ///
    /// When unset, [`Self::selection_symbol_style`] is used regardless of focus state.
    pub fn unfocused_selection_symbol_style(mut self, style: Style) -> Self {
        self.unfocused_selection_symbol_style = Some(style);
        self
    }

    /// Set the symbol shown for rows that are neither selected nor active.
    ///
    /// When not set, non-selected rows are padded with spaces equal to the
    /// reserved prefix column width (the max of all three symbol widths), keeping
    /// all content aligned. To remove all left padding set this to `Some("")`.
    pub fn unselected_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.unselected_symbol = symbol.map(Into::into);
        self
    }

    /// Set whether the left symbol/status column is enabled.
    ///
    /// This does not affect right-positioned active symbols.
    pub fn symbol_column(mut self, enabled: bool) -> Self {
        self.symbol_column = enabled;
        self
    }

    /// Set the explicit gap after the widest item gutter.
    pub fn gutter_gap(mut self, gap: u16) -> Self {
        self.gutter_gap = gap;
        self
    }

    /// Set whether non-selectable rows reserve the gutter column.
    pub fn gutter_for_non_selectable(mut self, enabled: bool) -> Self {
        self.gutter_for_non_selectable = enabled;
        self
    }

    /// Set whether the highlight should span the full width of the list.
    pub fn selection_full_width(mut self, full_width: bool) -> Self {
        self.selection_full_width = full_width;
        self
    }

    /// Set row padding for normal rows.
    ///
    /// Only left/right are used by the renderer; top/bottom are ignored.
    pub fn item_horizontal_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.item_horizontal_padding = padding.into();
        self
    }

    /// Set row padding for header rows.
    ///
    /// Only left/right are used by the renderer; top/bottom are ignored.
    pub fn header_horizontal_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.header_horizontal_padding = padding.into();
        self
    }

    /// Draw a border.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.border_style = border_style;
        self
    }

    /// Set the title (only visible when `border` is true).
    pub fn title(mut self, title: impl Into<Arc<str>>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Set title style.
    pub fn title_style(mut self, style: Style) -> Self {
        self.title_style = style;
        self
    }

    /// Set padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Draw a vertical scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.scrollbar_config = config;
        self
    }

    /// Override requested width.
    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    /// Override requested height.
    pub fn height(mut self, height: Length) -> Self {
        self.height = height;
        self
    }

    /// Callback fired when selection changes (keyboard or mouse).
    pub fn on_select(mut self, cb: Callback<ListEvent>) -> Self {
        self.on_select = Some(cb);
        self
    }

    /// Callback fired when a row is clicked with the mouse.
    pub fn on_item_click(mut self, cb: Callback<ListEvent>) -> Self {
        self.on_item_click = Some(cb);
        self
    }

    /// Callback fired on activation (Enter).
    pub fn on_activate(mut self, cb: Callback<ListEvent>) -> Self {
        self.on_activate = Some(cb);
        self
    }

    /// Control whether clicking an item emits `on_activate`.
    pub fn activate_on_click(mut self, activate: bool) -> Self {
        self.activate_on_click = activate;
        self
    }

    /// Set on-click handler.
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set on-scroll-to handler.
    pub fn on_scroll_to(mut self, cb: Callback<usize>) -> Self {
        self.on_scroll_to = Some(cb);
        self
    }

    /// Set on-key handler.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
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

    /// Control whether the node is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Enable "N more" scroll indicators when items are hidden.
    pub fn show_scroll_indicators(mut self, show: bool) -> Self {
        self.show_scroll_indicators = show;
        self
    }

    /// Set style for scroll indicators.
    pub fn scroll_indicator_style(mut self, style: Style) -> Self {
        self.scroll_indicator_style = style;
        self
    }

    /// Set text to display when the list is empty.
    pub fn empty_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.empty_text = Some(text.into());
        self
    }

    /// Set style for empty text.
    pub fn empty_text_style(mut self, style: Style) -> Self {
        self.empty_text_style = style;
        self
    }

    pub(crate) fn first_selectable_index(items: &[ListItem]) -> Option<usize> {
        items.iter().position(ListItem::is_selectable)
    }

    pub(crate) fn last_selectable_index(items: &[ListItem]) -> Option<usize> {
        items.iter().rposition(ListItem::is_selectable)
    }

    pub(crate) fn is_selectable_index(items: &[ListItem], index: usize) -> bool {
        items.get(index).is_some_and(ListItem::is_selectable)
    }

    pub(crate) fn selectable_at_or_after(items: &[ListItem], start: usize) -> Option<usize> {
        if items.is_empty() {
            return None;
        }

        let from = start.min(items.len().saturating_sub(1));
        items
            .iter()
            .enumerate()
            .skip(from)
            .find_map(|(idx, item)| item.is_selectable().then_some(idx))
    }

    pub(crate) fn selectable_at_or_before(items: &[ListItem], start: usize) -> Option<usize> {
        if items.is_empty() {
            return None;
        }

        let to = start.min(items.len().saturating_sub(1));
        items
            .iter()
            .enumerate()
            .take(to.saturating_add(1))
            .rfind(|(_, item)| item.is_selectable())
            .map(|(idx, _)| idx)
    }

    pub(crate) fn nearest_selectable_index(items: &[ListItem], preferred: usize) -> Option<usize> {
        if items.is_empty() {
            return None;
        }

        let preferred = preferred.min(items.len().saturating_sub(1));
        if Self::is_selectable_index(items, preferred) {
            return Some(preferred);
        }

        let before = Self::selectable_at_or_before(items, preferred);
        let after = Self::selectable_at_or_after(items, preferred);

        match (before, after) {
            (Some(left), Some(right)) => {
                if preferred.abs_diff(right) <= preferred.abs_diff(left) {
                    Some(right)
                } else {
                    Some(left)
                }
            }
            (Some(left), None) => Some(left),
            (None, Some(right)) => Some(right),
            (None, None) => None,
        }
    }

    pub(crate) fn selection_for_action(
        selected: usize,
        items: &[ListItem],
        action: ScrollAction,
    ) -> Option<usize> {
        let len = items.len();
        if len == 0 {
            return None;
        }

        let mut selected = Self::nearest_selectable_index(items, selected)?;

        let next = match action {
            ScrollAction::LineUp(lines) => {
                for _ in 0..lines {
                    if selected == 0 {
                        break;
                    }

                    let Some(prev) = Self::selectable_at_or_before(items, selected - 1) else {
                        break;
                    };
                    selected = prev;
                }
                selected
            }
            ScrollAction::LineDown(lines) => {
                for _ in 0..lines {
                    if selected.saturating_add(1) >= len {
                        break;
                    }

                    let Some(next) = Self::selectable_at_or_after(items, selected + 1) else {
                        break;
                    };
                    selected = next;
                }
                selected
            }
            ScrollAction::LineLeft(_) | ScrollAction::LineRight(_) => return None,
            ScrollAction::Home => Self::first_selectable_index(items)?,
            ScrollAction::End => Self::last_selectable_index(items)?,
        };

        Some(next)
    }

    pub(crate) fn selection_for_action_in_len(
        selected: usize,
        len: usize,
        action: ScrollAction,
    ) -> Option<usize> {
        if len == 0 {
            return None;
        }

        let selected = selected.min(len.saturating_sub(1));
        let next = match action {
            ScrollAction::LineUp(lines) => selected.saturating_sub(lines),
            ScrollAction::LineDown(lines) => (selected + lines).min(len.saturating_sub(1)),
            ScrollAction::LineLeft(_) | ScrollAction::LineRight(_) => return None,
            ScrollAction::Home => 0,
            ScrollAction::End => len.saturating_sub(1),
        };

        Some(next)
    }

    pub(crate) fn next_selection(
        selected: usize,
        items: &[ListItem],
        key: &KeyEvent,
        scroll_keys: ScrollKeymap,
    ) -> Option<usize> {
        let action = scroll_action_from_key(key, scroll_keys)?;
        Self::selection_for_action(selected, items, action)
    }
}

impl From<List> for Element {
    fn from(value: List) -> Self {
        Element::new(ElementKind::List(Box::new(value)))
    }
}

fn hash_list_item_layout(item: &ListItem, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    item.role.hash(hasher);
    item.active.hash(hasher);
    item.status.as_ref().map(ListItemStatus::width).hash(hasher);
    item.gutter.as_ref().map(ListItemGutter::width).hash(hasher);
    item.gutter_line.hash(hasher);
    item.prefix.hash(hasher);
    item.prefix_kind.hash(hasher);
    item.extra_line_indent.hash(hasher);
    item.primary_truncate_description_first.hash(hasher);
    item.primary_wrap_label.hash(hasher);
    item.primary_wrap_description.hash(hasher);
    item.primary_max_label_width.hash(hasher);
    item.primary_max_description_width.hash(hasher);
    item.symbol_line.hash(hasher);
}

fn hash_list_item_line_layout(line: &ListItemLine, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash;

    line.truncate_description_first.hash(hasher);
    line.wrap_label.hash(hasher);
    line.wrap_description.hash(hasher);
    line.max_label_width.hash(hasher);
    line.max_description_width.hash(hasher);
}

impl crate::layout::hash::LayoutHash for List {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use crate::layout::hash::hash_spans_content;
        use std::hash::Hash;

        self.width.hash(hasher);
        self.height.hash(hasher);
        self.border.hash(hasher);
        self.scrollbar.hash(hasher);
        self.scrollbar_config.variant.hash(hasher);
        self.scrollbar_config.gap.hash(hasher);
        self.padding.hash(hasher);
        self.item_horizontal_padding.hash(hasher);
        self.header_horizontal_padding.hash(hasher);
        self.symbol_column.hash(hasher);
        self.gutter_gap.hash(hasher);
        self.gutter_for_non_selectable.hash(hasher);

        let needs_content = matches!(self.width, Length::Auto);
        let needs_len = matches!(self.height, Length::Auto) || self.scrollbar;

        if needs_len {
            self.items.len().hash(hasher);
        }

        if needs_content || needs_len {
            self.selected.hash(hasher);
            for item in self.items.iter() {
                hash_list_item_layout(item, hasher);
                hash_spans_content(&item.spans, hasher);
                hash_spans_content(&item.description_spans, hasher);
                for line in &item.extra_lines {
                    hash_list_item_line_layout(line, hasher);
                    hash_spans_content(&line.spans, hasher);
                    hash_spans_content(&line.description_spans, hasher);
                }
            }
        }

        self.title.hash(hasher);
        self.empty_text.hash(hasher);
        self.selection_symbol.hash(hasher);
        self.selection_symbol_right.hash(hasher);
        self.active_symbol.hash(hasher);
        self.active_symbol_position.hash(hasher);
        self.unselected_symbol.hash(hasher);
        Some(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::element::Element;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::layout::hash::element_layout_hash;
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods::default(),
        }
    }

    fn fixture_items() -> Vec<ListItem> {
        vec![
            ListItem::header("Group A"),
            ListItem::new("Alpha"),
            ListItem::new("Beta"),
            ListItem::spacer(),
            ListItem::header("Group B"),
            ListItem::new("Gamma"),
        ]
    }

    #[test]
    fn keyboard_navigation_skips_headers_and_spacers() {
        let items = fixture_items();
        let next = List::next_selection(1, &items, &key(KeyCode::Down), ScrollKeymap::default());
        assert_eq!(next, Some(2));

        let next = List::next_selection(2, &items, &key(KeyCode::Down), ScrollKeymap::default());
        assert_eq!(next, Some(5));
    }

    #[test]
    fn home_end_resolve_to_selectable_rows() {
        let items = fixture_items();

        let home = List::next_selection(5, &items, &key(KeyCode::Home), ScrollKeymap::default());
        assert_eq!(home, Some(1));

        let end = List::next_selection(1, &items, &key(KeyCode::End), ScrollKeymap::default());
        assert_eq!(end, Some(5));
    }

    #[test]
    fn all_non_selectable_rows_have_no_selection_target() {
        let items = vec![ListItem::header("Section"), ListItem::spacer()];

        assert_eq!(List::first_selectable_index(&items), None);
        assert_eq!(
            List::next_selection(0, &items, &key(KeyCode::Down), ScrollKeymap::default()),
            None
        );
    }

    #[test]
    fn auto_height_layout_hash_tracks_extra_line_content() {
        let base: Element = List::new()
            .items(vec![ListItem::new("Type your own answer")])
            .height(Length::Auto)
            .into();
        let with_answer: Element = List::new()
            .items(vec![
                ListItem::new("Type your own answer").line("saved answer"),
            ])
            .height(Length::Auto)
            .into();

        assert_ne!(
            element_layout_hash(&base),
            element_layout_hash(&with_answer)
        );
    }

    #[test]
    fn layout_hash_tracks_leading_column_config() {
        let base: Element = List::new()
            .items(vec![ListItem::new("Alpha").gutter(Spinner::new())])
            .width(Length::Auto)
            .into();
        let no_symbol: Element = List::new()
            .items(vec![ListItem::new("Alpha").gutter(Spinner::new())])
            .width(Length::Auto)
            .symbol_column(false)
            .into();
        let gap: Element = List::new()
            .items(vec![ListItem::new("Alpha").gutter(Spinner::new())])
            .width(Length::Auto)
            .gutter_gap(1)
            .into();

        assert_ne!(element_layout_hash(&base), element_layout_hash(&no_symbol));
        assert_ne!(element_layout_hash(&base), element_layout_hash(&gap));
    }

    #[test]
    fn layout_hash_tracks_row_leading_width_fields() {
        let base: Element = List::new()
            .items(vec![ListItem::new("A")])
            .width(Length::Auto)
            .into();
        let with_status: Element = List::new()
            .items(vec![ListItem::new("A").status_symbol("!!")])
            .width(Length::Auto)
            .into();
        let with_gutter: Element = List::new()
            .items(vec![ListItem::new("A").gutter(ListItemGutter::text(">>"))])
            .width(Length::Auto)
            .into();
        let header_with_gutter: Element = List::new()
            .items(vec![
                ListItem::header("A").gutter(ListItemGutter::text(">>")),
            ])
            .width(Length::Auto)
            .into();

        assert_ne!(
            element_layout_hash(&base),
            element_layout_hash(&with_status)
        );
        assert_ne!(
            element_layout_hash(&base),
            element_layout_hash(&with_gutter)
        );
        assert_ne!(
            element_layout_hash(&with_gutter),
            element_layout_hash(&header_with_gutter)
        );
    }

    #[test]
    fn layout_hash_tracks_wrap_flags_for_auto_height() {
        let base: Element = List::new()
            .items(vec![ListItem::new("alpha beta gamma")])
            .height(Length::Auto)
            .into();
        let wrapped: Element = List::new()
            .items(vec![
                ListItem::new("alpha beta gamma").primary_wrap_label(true),
            ])
            .height(Length::Auto)
            .into();

        assert_ne!(element_layout_hash(&base), element_layout_hash(&wrapped));
    }
}

pub(crate) mod layout;
pub(crate) mod node;
pub(crate) mod reconcile;
pub(crate) mod utils;

pub use node::ListNode;
pub(crate) use reconcile::reconcile_list;
