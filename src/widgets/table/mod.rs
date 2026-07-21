//! Table widget.

use std::sync::Arc;

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, ElementKind};
use crate::core::event::{KeyEvent, MouseEvent};
use crate::style::{BorderStyle, Length, Padding, ScrollbarConfig, Style, StyleSlot};
use crate::utils::gradient::{ColorGradient, GradientRange};
use crate::widgets::scroll::{ScrollKeymap, scroll_action_from_key};

/// A table selection event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TableEvent {
    /// Selected row index.
    pub index: usize,
}

/// Semantic role of a table row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TableRowRole {
    /// Regular data row.
    #[default]
    Normal,
    /// Section header row, usually spanning key/value groups.
    Section,
    /// Visual separator row.
    Separator,
}

/// Disclosure marker state for inspector-like rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TableDisclosureState {
    /// Collapsed branch marker.
    Collapsed,
    /// Expanded branch marker.
    Expanded,
}

/// A cell in a table.
#[derive(Clone, Debug, Default)]
pub struct TableCell {
    pub(crate) content: Arc<str>,
    pub(crate) style: Style,
}

impl TableCell {
    /// Create a new table cell.
    pub fn new(content: impl Into<Arc<str>>) -> Self {
        Self {
            content: content.into(),
            style: Style::default(),
        }
    }

    /// Set cell style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Map a numeric value to foreground color using a gradient.
    pub fn heat_fg(
        mut self,
        value: u64,
        gradient: ColorGradient,
        range: impl Into<GradientRange>,
    ) -> Self {
        let color = gradient.color_for(value, range);
        self.style = self.style.patch(Style::new().fg(color));
        self
    }

    /// Map a numeric value to background color using a gradient.
    pub fn heat_bg(
        mut self,
        value: u64,
        gradient: ColorGradient,
        range: impl Into<GradientRange>,
    ) -> Self {
        let color = gradient.color_for(value, range);
        self.style = self.style.patch(Style::new().bg(color));
        self
    }
}

impl<T: Into<Arc<str>>> From<T> for TableCell {
    fn from(value: T) -> Self {
        Self::new(value)
    }
}

/// A row in a table.
#[derive(Clone, Debug, Default)]
pub struct TableRow {
    pub(crate) cells: Vec<TableCell>,
    pub(crate) style: Style,
    pub(crate) height: u16,
    pub(crate) bottom_margin: u16,
    pub(crate) role: TableRowRole,
    pub(crate) depth: u16,
    pub(crate) disclosure: Option<TableDisclosureState>,
}

impl TableRow {
    /// Create a new table row.
    pub fn new(cells: impl IntoIterator<Item = impl Into<TableCell>>) -> Self {
        Self {
            cells: cells.into_iter().map(Into::into).collect(),
            style: Style::default(),
            height: 1,
            bottom_margin: 0,
            role: TableRowRole::Normal,
            depth: 0,
            disclosure: None,
        }
    }

    /// Create a key/value row optimized for inspector-style tables.
    pub fn key_value(key: impl Into<TableCell>, value: impl Into<TableCell>) -> Self {
        Self::new([key.into(), value.into()])
    }

    /// Create a section row.
    pub fn section(title: impl Into<TableCell>) -> Self {
        Self::new([title.into()]).role(TableRowRole::Section)
    }

    /// Create a separator row.
    pub fn separator() -> Self {
        Self::new(std::iter::empty::<TableCell>()).role(TableRowRole::Separator)
    }

    /// Set row style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set row height.
    pub fn height(mut self, height: u16) -> Self {
        self.height = height;
        self
    }

    /// Automatically size the row height based on content line count.
    pub fn auto_height(mut self) -> Self {
        self.height = 0;
        self
    }

    /// Set bottom margin.
    pub fn bottom_margin(mut self, margin: u16) -> Self {
        self.bottom_margin = margin;
        self
    }

    /// Set semantic row role.
    pub fn role(mut self, role: TableRowRole) -> Self {
        self.role = role;
        self
    }

    /// Set indentation depth for inspector rendering.
    pub fn depth(mut self, depth: u16) -> Self {
        self.depth = depth;
        self
    }

    /// Set disclosure marker state for inspector rendering.
    pub fn disclosure(mut self, disclosure: TableDisclosureState) -> Self {
        self.disclosure = Some(disclosure);
        self
    }
}

pub(crate) fn resolved_row_height(row: &TableRow) -> u16 {
    if row.height > 0 {
        return row.height;
    }
    let mut max_lines = 1u16;
    for cell in &row.cells {
        let lines = cell.content.as_ref().lines().count().max(1) as u16;
        max_lines = max_lines.max(lines);
    }
    max_lines
}

pub(crate) fn resolved_row_total_height(row: &TableRow) -> u16 {
    resolved_row_height(row).saturating_add(row.bottom_margin)
}

pub(crate) fn table_header_reserved_height(
    header: Option<&TableRow>,
    rows_len: usize,
    row_gap: u16,
) -> u16 {
    header
        .map(resolved_row_total_height)
        .unwrap_or(0)
        .saturating_add(if header.is_some() && rows_len > 0 {
            row_gap
        } else {
            0
        })
}

pub(crate) fn row_index_at_visual_offset(
    rows: &[TableRow],
    offset: usize,
    visual_y: u16,
    row_gap: u16,
) -> Option<usize> {
    if rows.is_empty() || offset >= rows.len() {
        return None;
    }

    let mut remaining = visual_y;
    for (index, row) in rows.iter().enumerate().skip(offset) {
        let row_h = resolved_row_total_height(row).max(1);
        if remaining < row_h {
            return Some(index);
        }
        remaining = remaining.saturating_sub(row_h);

        if index + 1 < rows.len() {
            if remaining < row_gap {
                return None;
            }
            remaining = remaining.saturating_sub(row_gap);
        }
    }

    None
}

pub(crate) fn visible_rows_for_height(
    rows: &[TableRow],
    offset: usize,
    available_height: u16,
    row_gap: u16,
) -> usize {
    if available_height == 0 || rows.is_empty() || offset >= rows.len() {
        return 0;
    }

    let mut used = 0u16;
    let mut count = 0usize;
    for (idx, row) in rows.iter().enumerate().skip(offset) {
        let row_h = resolved_row_total_height(row).max(1);
        let gap_before = if count > 0 && idx > offset {
            row_gap
        } else {
            0
        };
        let needed = gap_before.saturating_add(row_h);
        if used.saturating_add(needed) > available_height {
            break;
        }
        used = used.saturating_add(needed);
        count = count.saturating_add(1);
    }

    if count == 0 { 1 } else { count }
}

impl<I, C> From<I> for TableRow
where
    I: IntoIterator<Item = C>,
    C: Into<TableCell>,
{
    fn from(iter: I) -> Self {
        Self::new(iter)
    }
}

/// Column width constraint.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ColumnWidth {
    /// Fixed width in cells.
    Fixed(u16),
    /// Percentage of total width.
    Percent(u16),
    /// Minimum width (Auto).
    Min(u16),
    /// Maximum width.
    Max(u16),
    /// Proportional fill.
    Fill(u16),
}

/// A table widget.
#[derive(Clone)]
pub struct Table {
    pub(crate) rows: Arc<[TableRow]>,
    pub(crate) header: Option<TableRow>,
    pub(crate) widths: Vec<ColumnWidth>,
    pub(crate) column_styles: Vec<Style>,
    pub(crate) row_styles: Vec<Style>,
    pub(crate) selected: Option<usize>,
    pub(crate) column_spacing: u16,
    pub(crate) row_gap: u16,
    pub(crate) style: Style,
    pub(crate) hover_style: StyleSlot,
    pub(crate) item_hover_style: StyleSlot,
    pub(crate) alternating_row_style: Option<Style>,
    pub(crate) row_style_full_width: bool,
    pub(crate) selection_style: StyleSlot,
    pub(crate) selection_symbol: Option<Arc<str>>,
    pub(crate) selection_symbol_style: Option<Style>,
    pub(crate) unselected_symbol: Option<Arc<str>>,
    pub(crate) border: bool,
    pub(crate) border_style: BorderStyle,
    pub(crate) padding: Padding,

    // Scrolling support
    pub(crate) scrollbar: bool,
    pub(crate) scrollbar_config: ScrollbarConfig,
    pub(crate) scroll_keys: ScrollKeymap,
    pub(crate) scroll_wheel: bool,

    // Layout
    pub(crate) width: Length,
    pub(crate) height: Length,

    // Events
    pub(crate) on_select: Option<Callback<TableEvent>>,
    pub(crate) on_activate: Option<Callback<TableEvent>>,
    pub(crate) on_click: Option<Callback<MouseEvent>>,
    pub(crate) on_scroll_to: Option<Callback<usize>>,
    pub(crate) on_key: Option<KeyHandler>,

    pub(crate) disabled: bool,
    pub(crate) disabled_style: Style,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) on_focus: Option<Callback<()>>,
    pub(crate) on_blur: Option<Callback<()>>,
    pub(crate) show_scroll_indicators: bool,
    pub(crate) scroll_indicator_style: Style,

    // Inspector-style configuration.
    pub(crate) inspector: bool,
    pub(crate) inspector_key_style: Style,
    pub(crate) inspector_value_style: Style,
    pub(crate) inspector_section_style: Style,
    pub(crate) inspector_separator_style: Style,
    pub(crate) inspector_indent_size: u16,
    pub(crate) inspector_collapsed_symbol: Arc<str>,
    pub(crate) inspector_expanded_symbol: Arc<str>,
    pub(crate) inspector_separator_char: char,
}

impl Default for Table {
    fn default() -> Self {
        Self {
            rows: Arc::new([]),
            header: None,
            widths: Vec::new(),
            column_styles: Vec::new(),
            row_styles: Vec::new(),
            selected: Some(0),
            column_spacing: 1,
            row_gap: 0,
            style: Style::default(),
            hover_style: StyleSlot::Inherit,
            item_hover_style: StyleSlot::Inherit,
            alternating_row_style: None,
            row_style_full_width: false,
            selection_style: StyleSlot::Inherit,
            selection_symbol: None,
            selection_symbol_style: None,
            unselected_symbol: None,
            border: false,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            scrollbar: false,
            scrollbar_config: ScrollbarConfig::default(),
            scroll_keys: ScrollKeymap::default(),
            scroll_wheel: true,
            width: Length::Flex(1),
            height: Length::Flex(1),
            on_select: None,
            on_activate: None,
            on_click: None,
            on_scroll_to: None,
            on_key: None,
            disabled: false,
            disabled_style: Style::default(),
            focusable: true,
            tab_stop: true,
            on_focus: None,
            on_blur: None,
            show_scroll_indicators: false,
            scroll_indicator_style: Style::default(),
            inspector: false,
            inspector_key_style: Style::default(),
            inspector_value_style: Style::default(),
            inspector_section_style: Style::default(),
            inspector_separator_style: Style::default(),
            inspector_indent_size: 2,
            inspector_collapsed_symbol: Arc::from("▸"),
            inspector_expanded_symbol: Arc::from("▾"),
            inspector_separator_char: '─',
        }
    }
}

impl Table {
    /// Create a new table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set table rows.
    pub fn rows(mut self, rows: impl IntoIterator<Item = impl Into<TableRow>>) -> Self {
        self.rows = rows.into_iter().map(Into::into).collect::<Vec<_>>().into();
        self
    }

    /// Set rows from a shared slice.
    pub fn rows_arc(mut self, rows: Arc<[TableRow]>) -> Self {
        self.rows = rows;
        self
    }

    /// Add a row.
    pub fn row(mut self, row: impl Into<TableRow>) -> Self {
        let mut rows = self.rows.to_vec();
        rows.push(row.into());
        self.rows = rows.into();
        self
    }

    /// Set header row.
    pub fn header(mut self, header: impl Into<TableRow>) -> Self {
        self.header = Some(header.into());
        self
    }

    /// Set header row style.
    pub fn header_style(mut self, style: Style) -> Self {
        if let Some(header) = &mut self.header {
            header.style = header.style.patch(style);
        }
        self
    }

    /// Set base style for all rows.
    pub fn row_style(mut self, style: Style) -> Self {
        let mut rows = self.rows.to_vec();
        for row in &mut rows {
            row.style = row.style.patch(style);
        }
        self.rows = rows.into();
        self
    }

    /// Patch the style for one zero-based column.
    ///
    /// Missing entries are filled with `Style::default()`. The supplied style is patched over any
    /// existing style at `index` and applies to header and data cells in that column.
    pub fn column_style(mut self, index: usize, style: Style) -> Self {
        if self.column_styles.len() <= index {
            self.column_styles
                .resize(index.saturating_add(1), Style::default());
        }
        self.column_styles[index] = self.column_styles[index].patch(style);
        self
    }

    /// Replace the positional column style list.
    ///
    /// Styles are matched by zero-based column index and apply to header and data cells.
    pub fn column_styles(mut self, styles: impl IntoIterator<Item = Style>) -> Self {
        self.column_styles = styles.into_iter().collect();
        self
    }

    /// Patch the style for one zero-based data row by absolute row index.
    ///
    /// Missing entries are filled with `Style::default()`. The supplied style is patched over any
    /// existing style at `index`; header rows are not affected.
    pub fn row_style_at(mut self, index: usize, style: Style) -> Self {
        if self.row_styles.len() <= index {
            self.row_styles
                .resize(index.saturating_add(1), Style::default());
        }
        self.row_styles[index] = self.row_styles[index].patch(style);
        self
    }

    /// Replace the positional data-row style list.
    ///
    /// Styles are matched by zero-based absolute row index and do not affect the header row.
    pub fn row_styles(mut self, styles: impl IntoIterator<Item = Style>) -> Self {
        self.row_styles = styles.into_iter().collect();
        self
    }

    /// Set column widths.
    pub fn widths(mut self, widths: impl IntoIterator<Item = ColumnWidth>) -> Self {
        self.widths = widths.into_iter().collect();
        self
    }

    /// Set selected row index.
    ///
    /// Pass `None` for no current row (no selection highlight). Bare integers
    /// still work via `From<T> for Option<T>` (`table.selected(0)`).
    pub fn selected(mut self, selected: impl Into<Option<usize>>) -> Self {
        self.selected = selected.into();
        self
    }

    /// Set column spacing.
    pub fn column_spacing(mut self, spacing: u16) -> Self {
        self.column_spacing = spacing;
        self
    }

    /// Set blank terminal rows inserted between rendered table rows.
    ///
    /// The gap is additive with `TableRow::bottom_margin`, applies between the
    /// header and first data row when both are present, and is not added after
    /// the final data row or after a header-only table.
    pub fn row_gap(mut self, gap: u16) -> Self {
        self.row_gap = gap;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set style when table is hovered.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's hover style with additional fields.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit hover style from the active theme.
    pub fn inherit_hover_style(mut self) -> Self {
        self.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set style for hovered rows.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.item_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's item hover style with additional fields.
    pub fn extend_item_hover_style(mut self, style: Style) -> Self {
        self.item_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit item hover style from the active theme.
    pub fn inherit_item_hover_style(mut self) -> Self {
        self.item_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set alternating style for odd data rows.
    pub fn alternating_row_style(mut self, style: Style) -> Self {
        self.alternating_row_style = Some(style);
        self
    }

    /// Set whether row-level styles span the full content width.
    ///
    /// When enabled, alternating row style, hover style, and selected-row style
    /// are rendered across the entire row width, not just table cell content.
    pub fn row_style_full_width(mut self, full_width: bool) -> Self {
        self.row_style_full_width = full_width;
        self
    }

    /// Set highlight style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's selection style with additional fields.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit selection style from the active theme.
    pub fn inherit_selection_style(mut self) -> Self {
        self.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set highlight symbol.
    pub fn selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.selection_symbol = symbol.map(Into::into);
        self
    }

    /// Set style for the highlight symbol.
    pub fn selection_symbol_style(mut self, style: Style) -> Self {
        self.selection_symbol_style = Some(style);
        self
    }

    /// Set symbol for unselected rows.
    pub fn unselected_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.unselected_symbol = symbol.map(Into::into);
        self
    }

    /// Enable border.
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

    /// Enable scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.scrollbar_config = config;
        self
    }

    /// Set scroll keys.
    pub fn scroll_keys(mut self, keys: ScrollKeymap) -> Self {
        self.scroll_keys = keys;
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

    /// Set on-select callback.
    pub fn on_select(mut self, cb: Callback<TableEvent>) -> Self {
        self.on_select = Some(cb);
        self
    }

    /// Set on-activate callback.
    pub fn on_activate(mut self, cb: Callback<TableEvent>) -> Self {
        self.on_activate = Some(cb);
        self
    }

    /// Set on-click callback.
    pub fn on_click(mut self, cb: Callback<MouseEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set on-scroll-to callback.
    pub fn on_scroll_to(mut self, cb: Callback<usize>) -> Self {
        self.on_scroll_to = Some(cb);
        self
    }

    /// Set on-key handler.
    pub fn on_key(mut self, handler: KeyHandler) -> Self {
        self.on_key = Some(handler);
        self
    }

    /// Set disabled.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Set focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Control whether the table participates in sequential focus navigation.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the table receives focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the table loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
        self
    }

    /// Enable scroll indicators when rows are hidden.
    pub fn show_scroll_indicators(mut self, show: bool) -> Self {
        self.show_scroll_indicators = show;
        self
    }

    /// Set style for scroll indicators.
    pub fn scroll_indicator_style(mut self, style: Style) -> Self {
        self.scroll_indicator_style = style;
        self
    }

    /// Enable inspector-style row rendering conventions.
    pub fn inspector(mut self, enabled: bool) -> Self {
        self.inspector = enabled;
        self
    }

    /// Apply inspector defaults and key/value-friendly column widths.
    pub fn inspector_preset(mut self) -> Self {
        self.inspector = true;
        if self.widths.is_empty() {
            self.widths = vec![ColumnWidth::Min(24), ColumnWidth::Fill(1)];
        }
        self
    }

    /// Style for the key column when inspector mode is enabled.
    pub fn inspector_key_style(mut self, style: Style) -> Self {
        self.inspector_key_style = style;
        self
    }

    /// Style for value columns when inspector mode is enabled.
    pub fn inspector_value_style(mut self, style: Style) -> Self {
        self.inspector_value_style = style;
        self
    }

    /// Style for section rows when inspector mode is enabled.
    pub fn inspector_section_style(mut self, style: Style) -> Self {
        self.inspector_section_style = style;
        self
    }

    /// Style for separator rows when inspector mode is enabled.
    pub fn inspector_separator_style(mut self, style: Style) -> Self {
        self.inspector_separator_style = style;
        self
    }

    /// Set indentation width (in cells) for inspector mode.
    pub fn inspector_indent_size(mut self, size: u16) -> Self {
        self.inspector_indent_size = size.max(1);
        self
    }

    /// Set disclosure symbols used by inspector rows with disclosure metadata.
    pub fn inspector_disclosure_symbols(
        mut self,
        collapsed: impl Into<Arc<str>>,
        expanded: impl Into<Arc<str>>,
    ) -> Self {
        self.inspector_collapsed_symbol = collapsed.into();
        self.inspector_expanded_symbol = expanded.into();
        self
    }

    /// Set separator character used by inspector separator rows.
    pub fn inspector_separator_char(mut self, separator_char: char) -> Self {
        self.inspector_separator_char = separator_char;
        self
    }

    pub(crate) fn next_selection(
        selected: usize,
        len: usize,
        key: &KeyEvent,
        scroll_keys: ScrollKeymap,
    ) -> Option<usize> {
        let action = scroll_action_from_key(key, scroll_keys)?;
        crate::widgets::list::List::selection_for_action_in_len(selected, len, action)
    }
}

impl From<Table> for Element {
    fn from(value: Table) -> Self {
        Element::new(ElementKind::Table(Box::new(value)))
    }
}

impl crate::layout::hash::LayoutHash for Table {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.border.hash(hasher);
        self.border_style.hash(hasher);
        self.padding.hash(hasher);
        self.row_gap.hash(hasher);

        let needs_content = matches!(self.height, Length::Auto);
        if needs_content {
            self.rows.len().hash(hasher);
            if let Some(header) = &self.header {
                header.height.hash(hasher);
                header.bottom_margin.hash(hasher);
            }
            for row in self.rows.iter() {
                row.height.hash(hasher);
                row.bottom_margin.hash(hasher);
            }
        }

        self.header.is_some().hash(hasher);
        self.column_spacing.hash(hasher);
        self.scrollbar.hash(hasher);
        self.scrollbar_config.gap.hash(hasher);
        self.show_scroll_indicators.hash(hasher);
        self.widths.hash(hasher);
        self.selected.hash(hasher);
        Some(())
    }
}

mod layout;
mod node;
mod reconcile;
mod shared;

pub(crate) use layout::measure_table;
pub(crate) use node::TableNode;
pub(crate) use reconcile::reconcile_table;
pub(crate) use shared::{
    TableBorderLineKind, distribute_extra_width, shrink_widths_to_fit, table_border_glyphs,
    table_border_line, table_fixed_chars, table_render_width,
};

#[cfg(test)]
mod arc_setter_tests {
    use super::{Table, TableRow};
    use std::sync::Arc;

    #[test]
    fn rows_arc_preserves_shared_slice() {
        let rows: Arc<[TableRow]> = Arc::from([TableRow::new(vec!["a", "b"])]);
        let table = Table::new().rows_arc(Arc::clone(&rows));
        assert!(Arc::ptr_eq(&table.rows, &rows));
    }
}
