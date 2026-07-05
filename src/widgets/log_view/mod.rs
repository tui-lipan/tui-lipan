//! Log stream widgets and helpers.

mod buffer;
pub(crate) mod component;
pub(crate) mod matching;

pub use buffer::{LogBuffer, LogEntry, LogLevel};

use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::Element;
use crate::style::{BorderStyle, Length, Padding, ScrollbarConfig, Style, StyleSlot};

/// Log row event emitted from `LogView`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogViewEvent {
    /// Row index within currently visible (filtered) rows.
    pub visible_index: usize,
    /// Index in the original source entries passed to `LogView`.
    pub source_index: usize,
    /// Selected entry value.
    pub entry: LogEntry,
}

pub use crate::utils::nucleo::MatchMode as LogFilterMode;

#[derive(Clone, PartialEq)]
pub struct LogViewProps {
    pub entries: Arc<[LogEntry]>,
    pub filter: Option<Arc<str>>,
    pub filter_mode: LogFilterMode,
    pub case_sensitive: bool,
    pub show_level: bool,
    pub auto_follow: bool,
    pub paused: bool,
    pub selected: usize,
    pub style: Style,
    pub hover_style: StyleSlot,
    pub item_hover_style: StyleSlot,
    pub selection_style: StyleSlot,
    pub unfocused_selection_style: StyleSlot,
    pub border: bool,
    /// Border style.
    /// Default: `BorderStyle::Plain`.
    pub border_style: BorderStyle,
    /// Inner padding.
    /// Default: `Padding::default()`.
    pub padding: Padding,
    pub scrollbar: bool,
    pub scrollbar_config: ScrollbarConfig,
    pub show_scroll_indicators: bool,
    pub scroll_indicator_style: Style,
    /// Requested width.
    /// Default: `Length::Flex(1)`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Flex(1)`.
    pub height: Length,
    pub empty_text: Option<Arc<str>>,
    pub empty_text_style: Style,
    pub trace_style: Style,
    pub debug_style: Style,
    pub info_style: Style,
    pub warn_style: Style,
    pub error_style: Style,
    pub on_select: Option<Callback<LogViewEvent>>,
    pub on_activate: Option<Callback<LogViewEvent>>,
    /// Whether a single click activates a row (firing `on_activate`).
    ///
    /// When `false`, `on_activate` only fires on `Enter` or a double-click,
    /// while a single click still selects via `on_select`.
    /// Default: `true`.
    pub activate_on_click: bool,
}

impl LogViewProps {
    pub fn level_style(&self, level: LogLevel) -> Style {
        match level {
            LogLevel::Trace => self.trace_style,
            LogLevel::Debug => self.debug_style,
            LogLevel::Info => self.info_style,
            LogLevel::Warn => self.warn_style,
            LogLevel::Error => self.error_style,
        }
    }
}

/// High-throughput log list with nucleo-powered filtering and level highlighting.
#[derive(Clone)]
pub struct LogView {
    props: LogViewProps,
}

impl Default for LogView {
    fn default() -> Self {
        Self {
            props: LogViewProps {
                entries: Arc::new([]),
                filter: None,
                filter_mode: LogFilterMode::Fuzzy,
                case_sensitive: true,
                show_level: true,
                auto_follow: true,
                paused: false,
                selected: 0,
                style: Style::default(),
                hover_style: StyleSlot::Inherit,
                item_hover_style: StyleSlot::Inherit,
                selection_style: StyleSlot::Inherit,
                unfocused_selection_style: StyleSlot::Inherit,
                border: false,
                border_style: BorderStyle::default(),
                padding: Padding::default(),
                scrollbar: true,
                scrollbar_config: ScrollbarConfig::default(),
                show_scroll_indicators: false,
                scroll_indicator_style: Style::default(),
                width: Length::Flex(1),
                height: Length::Flex(1),
                empty_text: Some("No log lines".into()),
                empty_text_style: Style::default(),
                trace_style: Style::default(),
                debug_style: Style::default(),
                info_style: Style::default(),
                warn_style: Style::default(),
                error_style: Style::default(),
                on_select: None,
                on_activate: None,
                activate_on_click: true,
            },
        }
    }
}

impl LogView {
    /// Create an empty log view.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace source entries.
    pub fn entries<I>(mut self, entries: I) -> Self
    where
        I: IntoIterator<Item = LogEntry>,
    {
        self.props.entries = entries.into_iter().collect();
        self
    }

    /// Set source entries from a shared slice.
    pub fn entries_arc(mut self, entries: Arc<[LogEntry]>) -> Self {
        self.props.entries = entries;
        self
    }

    /// Add one entry.
    ///
    /// For large updates prefer `entries()` or `entries_arc()`.
    pub fn entry(mut self, entry: LogEntry) -> Self {
        let mut entries = self.props.entries.to_vec();
        entries.push(entry);
        self.props.entries = entries.into();
        self
    }

    /// Set row filter text.
    pub fn filter(mut self, filter: impl Into<Arc<str>>) -> Self {
        self.props.filter = Some(filter.into());
        self
    }

    /// Clear row filter text.
    pub fn clear_filter(mut self) -> Self {
        self.props.filter = None;
        self
    }

    /// Set filtering mode used by nucleo.
    pub fn filter_mode(mut self, mode: LogFilterMode) -> Self {
        self.props.filter_mode = mode;
        self
    }

    /// Use fuzzy matching for filtering (nucleo).
    pub fn fuzzy(mut self) -> Self {
        self.props.filter_mode = LogFilterMode::Fuzzy;
        self
    }

    /// Use substring matching for filtering (nucleo).
    pub fn substring(mut self) -> Self {
        self.props.filter_mode = LogFilterMode::Substring;
        self
    }

    /// Use exact matching for filtering (nucleo).
    pub fn exact(mut self) -> Self {
        self.props.filter_mode = LogFilterMode::Exact;
        self
    }

    /// Toggle case-sensitive matching for non-regex filtering.
    pub fn case_sensitive(mut self, enabled: bool) -> Self {
        self.props.case_sensitive = enabled;
        self
    }

    /// Toggle level prefix (`[INFO]`) rendering.
    pub fn show_level(mut self, show_level: bool) -> Self {
        self.props.show_level = show_level;
        self
    }

    /// Toggle auto-follow to the newest visible row.
    pub fn auto_follow(mut self, auto_follow: bool) -> Self {
        self.props.auto_follow = auto_follow;
        self
    }

    /// Toggle paused mode.
    ///
    /// When paused, `auto_follow` is ignored and explicit selection is used.
    pub fn paused(mut self, paused: bool) -> Self {
        self.props.paused = paused;
        self
    }

    /// Set selected visible row index.
    pub fn selected(mut self, selected: usize) -> Self {
        self.props.selected = selected;
        self
    }

    /// Set base list style.
    pub fn style(mut self, style: Style) -> Self {
        self.props.style = style;
        self
    }

    /// Set hovered list style.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.props.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hovered list style.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.props.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed hovered list style.
    pub fn inherit_hover_style(mut self) -> Self {
        self.props.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hovered list style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.hover_style = slot;
        self
    }

    /// Set hovered row style.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.props.item_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hovered row style.
    pub fn extend_item_hover_style(mut self, style: Style) -> Self {
        self.props.item_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed hovered row style.
    pub fn inherit_item_hover_style(mut self) -> Self {
        self.props.item_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hovered row style slot directly for composite forwarding.
    pub fn item_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.item_hover_style = slot;
        self
    }

    /// Set selected row style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.props.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed selected row style.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.props.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed selected row style.
    pub fn inherit_selection_style(mut self) -> Self {
        self.props.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set selected row style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.selection_style = slot;
        self
    }

    /// Set selected row style while the log view is not focused.
    pub fn unfocused_selection_style(mut self, style: Style) -> Self {
        self.props.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed selected row style while the log view is not focused.
    pub fn extend_unfocused_selection_style(mut self, style: Style) -> Self {
        self.props.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed selected row style while the log view is not focused.
    pub fn inherit_unfocused_selection_style(mut self) -> Self {
        self.props.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set unfocused selected row style slot directly for composite forwarding.
    pub fn unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.unfocused_selection_style = slot;
        self
    }

    /// Toggle border.
    pub fn border(mut self, border: bool) -> Self {
        self.props.border = border;
        self
    }

    /// Set border style.
    pub fn border_style(mut self, border_style: BorderStyle) -> Self {
        self.props.border_style = border_style;
        self
    }

    /// Set inner padding.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.padding = padding.into();
        self
    }

    /// Toggle vertical scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.props.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.props.scrollbar_config = config;
        self
    }

    /// Toggle hidden-row indicators (`N more`).
    pub fn show_scroll_indicators(mut self, show: bool) -> Self {
        self.props.show_scroll_indicators = show;
        self
    }

    /// Set hidden-row indicator style.
    pub fn scroll_indicator_style(mut self, style: Style) -> Self {
        self.props.scroll_indicator_style = style;
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.props.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.props.height = height;
        self
    }

    /// Set empty-state text.
    pub fn empty_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.props.empty_text = Some(text.into());
        self
    }

    /// Set empty-state style.
    pub fn empty_text_style(mut self, style: Style) -> Self {
        self.props.empty_text_style = style;
        self
    }

    /// Set style for `TRACE` prefix.
    pub fn trace_style(mut self, style: Style) -> Self {
        self.props.trace_style = style;
        self
    }

    /// Set style for `DEBUG` prefix.
    pub fn debug_style(mut self, style: Style) -> Self {
        self.props.debug_style = style;
        self
    }

    /// Set style for `INFO` prefix.
    pub fn info_style(mut self, style: Style) -> Self {
        self.props.info_style = style;
        self
    }

    /// Set style for `WARN` prefix.
    pub fn warn_style(mut self, style: Style) -> Self {
        self.props.warn_style = style;
        self
    }

    /// Set style for `ERROR` prefix.
    pub fn error_style(mut self, style: Style) -> Self {
        self.props.error_style = style;
        self
    }

    /// Set selection callback.
    pub fn on_select(mut self, cb: Callback<LogViewEvent>) -> Self {
        self.props.on_select = Some(cb);
        self
    }

    /// Set activation callback (Enter).
    pub fn on_activate(mut self, cb: Callback<LogViewEvent>) -> Self {
        self.props.on_activate = Some(cb);
        self
    }

    /// Control whether a single click activates a row (fires `on_activate`).
    ///
    /// When `false`, only `Enter` or a double-click activates; a single click
    /// still selects. Default: `true`.
    pub fn activate_on_click(mut self, activate: bool) -> Self {
        self.props.activate_on_click = activate;
        self
    }
}

impl From<LogView> for Element {
    fn from(view: LogView) -> Self {
        crate::child(component::LogViewComponent::new, view.props)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_buffer_keeps_newest_entries() {
        let mut buffer = LogBuffer::new(2);
        buffer.push(LogEntry::info("a"));
        buffer.push(LogEntry::info("b"));
        buffer.push(LogEntry::info("c"));

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[0].message.as_ref(), "b");
        assert_eq!(snapshot[1].message.as_ref(), "c");
    }

    #[test]
    fn paused_snapshot_stays_frozen() {
        let mut buffer = LogBuffer::new(8);
        buffer.push(LogEntry::info("one"));
        buffer.push(LogEntry::info("two"));
        buffer.set_paused(true);
        buffer.push(LogEntry::info("three"));

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.len(), 2);
        assert_eq!(snapshot[1].message.as_ref(), "two");
    }
}
