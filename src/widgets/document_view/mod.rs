//! Document view widget.
//!
//! A read-only rich text display widget with pluggable content formatting,
//! text selection, scroll synchronization, and optional markdown rendering.
//!
//! Unlike [`TextArea`](crate::widgets::TextArea), `DocumentView` can transform
//! content (e.g. strip markdown syntax, render tables with box-drawing) rather
//! than just applying syntax highlighting.

pub mod diagram;
mod format;
#[cfg(feature = "markdown")]
mod format_markdown;
pub(crate) mod layout;
#[cfg(feature = "markdown")]
pub(crate) mod mermaid;
pub(crate) mod node;
pub(crate) mod planner;
pub(crate) mod reconcile;

pub(crate) use format::FormatCache;
pub use format::{
    ColumnAlign, ContentFormatter, DocumentStyles, FormatInput, FormattedBlock, FormattedCodeBlock,
    FormattedDiagramBlock, FormattedDocument, FormattedLine, FormattedLink, FormattedTable,
    PlainFormatter,
};
#[cfg(feature = "markdown")]
pub use format_markdown::MarkdownFormatter;
pub use layout::measure_document_view;
pub(crate) use layout::measure_document_view_constrained;
pub use reconcile::reconcile_document_view;

use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::rc::Rc;
use std::sync::Arc;

use rustc_hash::FxHasher;

use crate::animation::TransitionConfig;
use crate::callback::{Callback, KeyHandler};
use crate::core::element::Element;
use crate::style::{
    BorderStyle, Length, Padding, ScrollbarConfig, ScrollbarVariant, Style, StyleSlot,
};
use crate::widgets::scroll::{ScrollBehavior, ScrollEvent};

/// Event emitted when the user clicks within the document.
#[derive(Clone, Debug)]
pub struct DocumentClickEvent {
    /// Source line (0-indexed) that was clicked.
    pub source_line: usize,
    /// If a link span was clicked, its URL.
    pub link: Option<Arc<str>>,
}

/// Event emitted when text is selected.
#[derive(Clone, Debug)]
pub struct DocumentSelectEvent {
    /// Plain text of the selection.
    pub selected_text: Arc<str>,
}

/// Scroll metrics exposed for scroll synchronization.
#[derive(Clone, Debug, Default)]
pub struct DocumentScrollMetrics {
    /// Current scroll offset (visual lines from top).
    pub offset: usize,
    /// Total visual lines in the document.
    pub total_lines: usize,
    /// Number of visual lines visible in the viewport.
    pub viewport_lines: usize,
    /// Source line at the top of the viewport.
    pub top_source_line: usize,
    /// Source line at the bottom of the viewport.
    pub bottom_source_line: usize,
}

/// Line numbering mode for the `DocumentView` gutter.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DocumentLineNumberMode {
    /// Number by currently visible visual lines (1, 2, 3...).
    #[default]
    Visual,
    /// Number by source line mapping from the formatter.
    Source,
}

/// Width strategy for markdown/formatted tables rendered by `DocumentView`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DocumentTableWidthMode {
    /// Size table columns from content; do not stretch to fill viewport.
    #[default]
    Content,
    /// Stretch table columns to fill the available viewport width.
    Fill,
}

/// Controls horizontal separator lines between table rows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TableRowSeparators {
    /// No horizontal row separators.
    None,
    /// Separator only between header and data rows (markdown default).
    #[default]
    Header,
    /// Separator between all rows (header + between every data row).
    All,
}

/// A read-only rich text document viewer with pluggable formatting.
///
/// # Example
///
/// ```rust,no_run
/// use tui_lipan::prelude::*;
///
/// let view = DocumentView::new("# Hello\n\nWorld")
///     .wrap(true)
///     .line_numbers(true);
/// ```
#[derive(Clone)]
pub struct DocumentView {
    // ── Content ──────────────────────────────────────────────────────────
    /// The raw source text.
    pub value: Arc<str>,
    /// Lazily computed hash of [`Self::value`] for layout caches, with the
    /// `(ptr, len)` identity of the `Arc` allocation used when it was computed.
    ///
    /// [`LayoutHash`] and measurement must **not** use the raw `Arc` address
    /// alone: `view()` often rebuilds the same body into a new `Arc`, which
    /// would bust [`crate::layout::hash::element_layout_hash`] and the global
    /// measure cache on every frame (e.g. theme-only updates). We still
    /// recompute when the allocation changes (including `dv.value = ...` without
    /// going through [`Self::value`]).
    layout_content_fingerprint: Cell<Option<(u64, usize, usize)>>,
    /// Optional content-type hint passed to the formatter.
    pub content_type: Option<Arc<str>>,
    /// Pluggable content formatter. Defaults to [`PlainFormatter`].
    pub formatter: Option<Rc<dyn ContentFormatter>>,

    // ── Layout ───────────────────────────────────────────────────────────
    /// Requested width.
    /// Default: `Length::Flex(1)`.
    pub width: Length,
    /// Requested height.
    /// Default: `Length::Flex(1)`.
    pub height: Length,
    /// Word-wrap long lines.
    /// Default: `true`.
    pub wrap: bool,
    /// Show line numbers in the gutter.
    pub line_numbers: bool,
    /// Minimum digit width reserved for line numbers.
    pub min_line_number_width: u8,
    /// Show separator after built-in line numbers.
    /// Default: `true`.
    pub line_number_separator: bool,
    /// Empty cells between built-in line numbers and content.
    pub line_number_content_gap: u16,
    /// Line numbering mode.
    pub line_number_mode: DocumentLineNumberMode,
    /// Style override for built-in line-number gutter text.
    pub line_number_style: Style,
    /// Extend per-line background highlights across the full content width.
    pub highlight_full_width: bool,
    /// Show a border around the widget.
    /// Default: `true`.
    pub border: bool,
    /// Border style.
    /// Default: `BorderStyle::Plain`.
    pub border_style: BorderStyle,
    /// Padding inside the border.
    /// Default: `Padding::default()`.
    pub padding: Padding,
    /// Wrap table cell text to fit current column widths.
    pub table_wrap: bool,
    /// Table width behavior.
    pub table_width_mode: DocumentTableWidthMode,
    /// Draw table outer frame.
    pub table_outer_frame: bool,
    /// Draw vertical column separators between cells.
    pub table_column_separators: bool,
    /// Controls horizontal separator lines between rows.
    pub table_row_separators: TableRowSeparators,
    /// Horizontal table cell padding (left + right).
    pub table_cell_padding: u16,
    /// Border glyph variant used for table lines.
    /// Default: `BorderStyle::Plain`.
    pub table_border_variant: BorderStyle,

    // ── Styling ──────────────────────────────────────────────────────────
    /// Base text style.
    pub style: Style,
    /// Style applied when hovered.
    pub hover_style: StyleSlot,
    /// Chrome/surface style applied when focused.
    pub focus_style: StyleSlot,
    /// Text content style applied when focused.
    pub focus_content_style: Style,
    /// Style for selected text regions.
    pub selection_style: StyleSlot,
    /// Per-element style overrides.
    pub doc_styles: DocumentStyles,
    /// Border style override when hovered.
    pub hover_border_style: Option<BorderStyle>,

    // ── Scrolling ────────────────────────────────────────────────────────
    /// Explicit vertical scroll offset (visual lines from top).
    pub scroll_offset: Option<usize>,
    /// Scroll to this source line (for scroll sync).
    pub scroll_to_source_line: Option<usize>,
    /// Behavior used when applying [`Self::scroll_to_source_line`].
    pub scroll_behavior: ScrollBehavior,
    /// Vertical scrollbar visibility.
    pub scrollbar: bool,
    /// Scrollbar configuration.
    pub scrollbar_config: ScrollbarConfig,
    /// Show horizontal scrollbar (only effective when `wrap` is `false`).
    pub h_scrollbar: bool,
    /// Horizontal scrollbar rendering style.
    pub h_scrollbar_variant: ScrollbarVariant,
    /// Horizontal scrollbar thumb character override.
    pub h_scrollbar_thumb: Option<char>,
    #[cfg(feature = "diff-view")]
    pub(crate) pin_scrollbar_focus_style: bool,
    /// Enable mouse wheel scrolling.
    pub scroll_wheel: bool,
    /// Widget-local mouse wheel step multiplier, overriding the app default when set.
    pub scroll_wheel_multiplier: Option<u16>,

    // ── Interaction ──────────────────────────────────────────────────────
    /// Whether the widget participates in focus traversal.
    pub focusable: bool,
    /// Whether the widget participates in tab traversal when focusable.
    pub tab_stop: bool,
    /// Callback fired when the widget gains focus.
    pub on_focus: Option<Callback<()>>,
    /// Callback fired when the widget loses focus.
    pub on_blur: Option<Callback<()>>,
    /// Scroll event callback.
    pub on_scroll: Option<Callback<ScrollEvent>>,
    /// Click event callback (with source line + link info).
    pub on_click: Option<Callback<DocumentClickEvent>>,
    /// Text selection callback.
    pub on_select: Option<Callback<DocumentSelectEvent>>,
    /// Keyboard handler (when focused).
    pub on_key: Option<KeyHandler>,
    /// Optional shared selection group identifier.
    ///
    /// When multiple `DocumentView`s under the same `ScrollView` share this id,
    /// drag selection can extend across them and copy concatenates in visual order.
    pub shared_selection_id: Option<Arc<str>>,

    // ── Code block highlighting (reuses SyntectStrategy) ─────────────────
    /// Syntax highlighting strategy for code blocks.
    #[cfg(feature = "syntax-syntect")]
    pub code_syntax_strategy: Option<Rc<dyn crate::widgets::TextAreaColorStrategy>>,

    // ── Custom gutter ────────────────────────────────────────────────────
    /// Per-logical-line custom gutter spans. When set, replaces the built-in
    /// `line_numbers` gutter. Indexed by logical line (0-based); continuation
    /// visual lines render an empty gutter.
    pub gutter_lines: Option<Arc<Vec<Vec<crate::style::Span>>>>,
    /// Fixed column width reserved for the custom gutter. When > 0, overrides
    /// the `line_numbers` gutter width everywhere.
    pub gutter_col_width: u16,
    /// Fixed empty cells before the gutter / line numbers.
    pub gutter_gap: u16,
    /// Logical source-line indices (0-based) to exclude from clipboard copy.
    pub copy_excluded_source_lines: Option<Arc<Vec<usize>>>,
    /// Optional peer source lines used for split-view wrap synchronization.
    pub peer_source_lines: Option<Arc<Vec<Arc<str>>>>,
    /// Lazily computed content fingerprint of [`Self::peer_source_lines`].
    ///
    /// `LayoutHash` and measurement must use this instead of `Arc` pointer
    /// addresses: `trim_render_common_indent` creates new `Arc<str>`
    /// allocations every frame even when the text is identical.
    peer_source_fingerprint: Cell<Option<u64>>,
    /// Lazily computed base portion of [`document_measure_cache_key`](layout::document_measure_cache_key),
    /// covering all geometry fields except the mutable split-wrap sync state.
    /// Tuple: `(key, content_fingerprint_guard)`.
    pub(crate) measure_base_key_cache: Cell<Option<(u64, u64)>>,
    #[cfg(feature = "diff-view")]
    pub(crate) split_wrap_sync: Option<crate::widgets::diff_view::SharedSplitWrapSync>,
    #[cfg(feature = "diff-view")]
    pub(crate) split_wrap_side: Option<crate::widgets::diff_view::SplitPaneSide>,
    #[cfg(feature = "diff-view")]
    pub(crate) diff_split_pane: Option<crate::widgets::DiffPane>,
    #[cfg(feature = "diff-view")]
    pub(crate) diff_context_separator_click:
        Option<crate::widgets::diff_view::DiffContextSeparatorClickConfig>,
    /// Style used for synthetic wrap-padding gutter rows inserted for peer sync.
    pub(crate) split_wrap_padding_gutter_style: Option<Style>,
    /// Style used for synthetic wrap-padding content rows inserted for peer sync.
    pub(crate) split_wrap_padding_style: Option<Style>,
    /// Enable word/line selection on double/triple click.
    /// Default: `true`.
    pub multi_click_select: bool,
    /// Triple-click selection behavior.
    pub triple_click_mode: crate::widgets::TripleClickSelectionMode,
    /// Forward clicks to a wrapping [`MouseRegion`](crate::widgets::MouseRegion)
    /// while keeping drag-to-select.
    /// Default: `false`.
    ///
    /// When `true`, a click positions the cursor and sets up the drag anchor
    /// as usual, but the click is also forwarded to the nearest enabled
    /// `MouseRegion` ancestor that has an `on_click` handler - without
    /// requiring `capture_click(true)` on the region. If `on_click` is also
    /// set, link clicks still go to the document callback and non-link clicks
    /// pass through to the ancestor.
    pub passthrough_clicks: bool,

    // Shared auto-height measurement cache used by both measure and reconcile
    // passes during width-driven layout changes (e.g. terminal resize).
    pub(crate) measure_cache:
        RefCell<[Option<super::document_view::layout::DocumentMeasureCacheEntry>; 2]>,
    /// Width-independent format cache for the measurement path.
    ///
    /// During resize, `max_w` changes every frame which invalidates
    /// `measure_cache`, but the formatted document (markdown parse result)
    /// doesn't depend on width.  Caching it here avoids re-parsing markdown
    /// on every measurement when only the width changed.
    pub(crate) measure_format_cache: RefCell<Option<(u64, std::rc::Rc<format::FormattedDocument>)>>,
}

impl Default for DocumentView {
    fn default() -> Self {
        Self {
            value: "".into(),
            layout_content_fingerprint: Cell::new(None),
            content_type: None,
            formatter: None,
            width: Length::Flex(1),
            height: Length::Flex(1),
            wrap: true,
            line_numbers: false,
            min_line_number_width: 0,
            line_number_separator: true,
            line_number_content_gap: 0,
            line_number_mode: DocumentLineNumberMode::default(),
            line_number_style: Style::default(),
            highlight_full_width: false,
            border: true,
            border_style: BorderStyle::Plain,
            padding: Padding::default(),
            table_wrap: false,
            table_width_mode: DocumentTableWidthMode::default(),
            table_outer_frame: true,
            table_column_separators: true,
            table_row_separators: TableRowSeparators::default(),
            table_cell_padding: 0,
            table_border_variant: BorderStyle::Plain,
            style: Style::default(),
            hover_style: StyleSlot::Inherit,
            focus_style: StyleSlot::Inherit,
            focus_content_style: Style::default(),
            selection_style: StyleSlot::Inherit,
            doc_styles: DocumentStyles::default(),
            hover_border_style: None,
            scroll_offset: None,
            scroll_to_source_line: None,
            scroll_behavior: ScrollBehavior::default(),
            scrollbar: true,
            scrollbar_config: ScrollbarConfig::default(),
            h_scrollbar: false,
            h_scrollbar_variant: ScrollbarVariant::default(),
            h_scrollbar_thumb: None,
            #[cfg(feature = "diff-view")]
            pin_scrollbar_focus_style: false,
            scroll_wheel: true,
            scroll_wheel_multiplier: None,
            focusable: true,
            tab_stop: true,
            on_focus: None,
            on_blur: None,
            on_scroll: None,
            on_click: None,
            on_select: None,
            on_key: None,
            shared_selection_id: None,
            #[cfg(feature = "syntax-syntect")]
            code_syntax_strategy: None,
            gutter_lines: None,
            gutter_col_width: 0,
            gutter_gap: 0,
            copy_excluded_source_lines: None,
            peer_source_lines: None,
            peer_source_fingerprint: Cell::new(None),
            measure_base_key_cache: Cell::new(None),
            #[cfg(feature = "diff-view")]
            split_wrap_sync: None,
            #[cfg(feature = "diff-view")]
            split_wrap_side: None,
            #[cfg(feature = "diff-view")]
            diff_split_pane: None,
            #[cfg(feature = "diff-view")]
            diff_context_separator_click: None,
            split_wrap_padding_gutter_style: None,
            split_wrap_padding_style: None,
            multi_click_select: true,
            triple_click_mode: crate::widgets::TripleClickSelectionMode::Line,
            passthrough_clicks: false,
            measure_cache: RefCell::new([None, None]),
            measure_format_cache: RefCell::new(None),
        }
    }
}

impl DocumentView {
    pub(crate) fn should_use_implicit_auto_height(&self) -> bool {
        matches!(self.height, Length::Flex(1))
            && self.wrap
            && !self.scrollbar
            && !self.h_scrollbar
            && !self.focusable
    }

    pub(crate) fn resolved_height(&self) -> Length {
        if self.should_use_implicit_auto_height() {
            Length::Auto
        } else {
            self.height
        }
    }

    /// Create a new document view with the given source text.
    pub fn new(value: impl Into<Arc<str>>) -> Self {
        Self::default().value(value)
    }

    /// Set the text content.
    pub fn value(mut self, value: impl Into<Arc<str>>) -> Self {
        self.layout_content_fingerprint.set(None);
        self.value = value.into();
        self
    }

    pub(crate) fn layout_content_fingerprint(&self) -> u64 {
        let ptr = self.value.as_ptr() as usize;
        let len = self.value.len();
        if let Some((fp, cached_ptr, cached_len)) = self.layout_content_fingerprint.get()
            && cached_ptr == ptr
            && cached_len == len
        {
            return fp;
        }
        let mut h = FxHasher::default();
        self.value.as_ref().hash(&mut h);
        let fp = h.finish();
        self.layout_content_fingerprint.set(Some((fp, ptr, len)));
        fp
    }

    /// Content fingerprint of [`Self::peer_source_lines`] for layout caching.
    ///
    /// Hashes peer line content instead of `Arc` pointers, which change every
    /// frame when `trim_render_common_indent` creates new allocations.
    pub(crate) fn peer_source_content_fingerprint(&self) -> Option<u64> {
        let peer = self.peer_source_lines.as_ref()?;
        if let Some(fp) = self.peer_source_fingerprint.get() {
            return Some(fp);
        }
        let mut h = FxHasher::default();
        peer.len().hash(&mut h);
        for line in peer.iter() {
            line.as_ref().hash(&mut h);
        }
        let fp = h.finish();
        self.peer_source_fingerprint.set(Some(fp));
        Some(fp)
    }

    /// Set the content-type hint (e.g. `"markdown"`, `"log"`).
    pub fn content_type(mut self, ct: impl Into<Arc<str>>) -> Self {
        self.content_type = Some(ct.into());
        self
    }

    /// Set the content formatter.
    pub fn formatter(mut self, f: impl ContentFormatter + 'static) -> Self {
        self.formatter = Some(Rc::new(f));
        self
    }

    /// Set the requested width.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Set the requested height.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Enable or disable word wrapping.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Show or hide line numbers.
    pub fn line_numbers(mut self, show: bool) -> Self {
        self.line_numbers = show;
        self
    }

    /// Set minimum line-number gutter digits.
    pub fn min_line_number_width(mut self, width: u8) -> Self {
        self.min_line_number_width = width;
        self
    }

    /// Show or hide the built-in line-number separator.
    pub fn line_number_separator(mut self, show: bool) -> Self {
        self.line_number_separator = show;
        self
    }

    /// Set empty cells between built-in line numbers and content.
    pub fn line_number_content_gap(mut self, gap: u16) -> Self {
        self.line_number_content_gap = gap;
        self
    }

    /// Set line numbering mode for the gutter.
    pub fn line_number_mode(mut self, mode: DocumentLineNumberMode) -> Self {
        self.line_number_mode = mode;
        self
    }

    /// Set style override for built-in line-number gutter text.
    pub fn line_number_style(mut self, style: Style) -> Self {
        self.line_number_style = style;
        self
    }

    /// Extend line background highlights across the full content width.
    pub fn highlight_full_width(mut self, enabled: bool) -> Self {
        self.highlight_full_width = enabled;
        self
    }

    /// Show or hide a border around the widget.
    pub fn border(mut self, border: bool) -> Self {
        self.border = border;
        self
    }

    /// Set the border style.
    pub fn border_style(mut self, style: BorderStyle) -> Self {
        self.border_style = style;
        self
    }

    /// Set the border style when hovered.
    pub fn hover_border_style(mut self, style: BorderStyle) -> Self {
        self.hover_border_style = Some(style);
        self
    }

    /// Set the padding inside the border.
    pub fn padding(mut self, padding: impl Into<Padding>) -> Self {
        self.padding = padding.into();
        self
    }

    /// Enable or disable table cell wrapping.
    pub fn table_wrap(mut self, wrap: bool) -> Self {
        self.table_wrap = wrap;
        self
    }

    /// Set table width mode.
    pub fn table_width_mode(mut self, mode: DocumentTableWidthMode) -> Self {
        self.table_width_mode = mode;
        self
    }

    /// Show or hide table outer frame.
    pub fn table_outer_frame(mut self, enabled: bool) -> Self {
        self.table_outer_frame = enabled;
        self
    }

    /// Show or hide vertical column separators between table cells.
    pub fn table_column_separators(mut self, enabled: bool) -> Self {
        self.table_column_separators = enabled;
        self
    }

    /// Set horizontal row separator mode.
    pub fn table_row_separators(mut self, mode: TableRowSeparators) -> Self {
        self.table_row_separators = mode;
        self
    }

    /// Set horizontal table cell padding (left and right).
    pub fn table_cell_padding(mut self, padding: u16) -> Self {
        self.table_cell_padding = padding;
        self
    }

    /// Set border glyph variant used for table lines.
    pub fn table_border_variant(mut self, variant: BorderStyle) -> Self {
        self.table_border_variant = variant;
        self
    }

    /// Set style applied to table borders (color/emphasis).
    pub fn table_border_style(mut self, style: Style) -> Self {
        self.doc_styles.table_border_style = style;
        self
    }

    /// Set the base text style.
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// Set the hover style.
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

    /// Set hover style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.hover_style = slot;
        self
    }

    /// Set the focus chrome style.
    pub fn focus_style(mut self, style: Style) -> Self {
        self.focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the active theme's focus style with additional fields.
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

    /// Set the focused content text style.
    pub fn focus_content_style(mut self, style: Style) -> Self {
        self.focus_content_style = style;
        self
    }

    /// Set the selection style.
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

    /// Set selection style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.selection_style = slot;
        self
    }

    /// Set the document-element style overrides.
    pub fn doc_styles(mut self, styles: DocumentStyles) -> Self {
        self.doc_styles = styles;
        self
    }

    /// Set style applied to code block rows.
    ///
    /// Useful for setting code block background and default foreground when
    /// syntax highlighting is disabled.
    pub fn code_block_style(mut self, style: Style) -> Self {
        self.doc_styles.code_block_style = style;
        self
    }

    /// Set explicit scroll offset (controlled mode).
    pub fn scroll_offset(mut self, offset: usize) -> Self {
        self.scroll_offset = Some(offset);
        self
    }

    /// Scroll to the given source line (for scroll sync).
    pub fn scroll_to_source_line(mut self, line: usize) -> Self {
        self.scroll_to_source_line = Some(line);
        self
    }

    /// Set how [`Self::scroll_to_source_line`] targets are applied.
    pub fn scroll_behavior(mut self, behavior: ScrollBehavior) -> Self {
        self.scroll_behavior = behavior;
        self
    }

    /// Smoothly animate [`Self::scroll_to_source_line`] targets with `transition`.
    pub fn scroll_transition(mut self, transition: TransitionConfig) -> Self {
        self.scroll_behavior = ScrollBehavior::smooth(transition);
        self
    }

    /// Show or hide the vertical scrollbar.
    pub fn scrollbar(mut self, show: bool) -> Self {
        self.scrollbar = show;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.scrollbar_config = config;
        self
    }

    /// Show or hide the horizontal scrollbar (only effective when `wrap` is `false`).
    pub fn h_scrollbar(mut self, show: bool) -> Self {
        self.h_scrollbar = show;
        self
    }

    /// Set the horizontal scrollbar rendering style.
    pub fn h_scrollbar_variant(mut self, variant: ScrollbarVariant) -> Self {
        self.h_scrollbar_variant = variant;
        self
    }

    /// Set the horizontal scrollbar thumb character.
    pub fn h_scrollbar_thumb(mut self, c: char) -> Self {
        self.h_scrollbar_thumb = Some(c);
        self
    }

    /// Set a custom gutter column.
    ///
    /// `lines` is indexed by logical line (0-based). Continuation visual lines
    /// show an empty gutter. `col_width` is the fixed column width reserved;
    /// when > 0 it overrides the `line_numbers` gutter width everywhere.
    pub fn gutter_lines(
        mut self,
        lines: Arc<Vec<Vec<crate::style::Span>>>,
        col_width: u16,
    ) -> Self {
        self.gutter_lines = Some(lines);
        self.gutter_col_width = col_width;
        self
    }

    /// Reserve empty cells before the gutter / line numbers.
    pub fn gutter_inset(mut self, inset: u16) -> Self {
        self.gutter_gap = inset;
        self
    }

    /// Set logical source-line indices (0-based) to exclude from clipboard copy.
    pub fn copy_excluded_source_lines(mut self, indices: Arc<Vec<usize>>) -> Self {
        self.copy_excluded_source_lines = Some(indices);
        self
    }

    /// Enable or disable mouse wheel scrolling.
    pub fn scroll_wheel(mut self, enabled: bool) -> Self {
        self.scroll_wheel = enabled;
        self
    }

    /// Override the app-wide mouse wheel step multiplier for this document view.
    pub fn scroll_wheel_multiplier(mut self, multiplier: u16) -> Self {
        self.scroll_wheel_multiplier = Some(multiplier.max(1));
        self
    }

    /// Set whether the widget is focusable.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable = focusable;
        self
    }

    /// Control whether the widget participates in tab traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the widget gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the widget loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.on_blur = Some(cb);
        self
    }

    /// Enable or disable word/line selection on double/triple click.
    ///
    /// When `false`, double and triple clicks behave as single clicks
    /// (no word or line selection). Drag-to-select remains unaffected.
    pub fn multi_click_select(mut self, enabled: bool) -> Self {
        self.multi_click_select = enabled;
        self
    }

    /// Set how triple-click expands selection.
    pub fn triple_click_mode(mut self, mode: crate::widgets::TripleClickSelectionMode) -> Self {
        self.triple_click_mode = mode;
        self
    }

    /// Forward clicks to a wrapping [`MouseRegion`](crate::widgets::MouseRegion)
    /// while keeping drag-to-select.
    ///
    /// When `true`, a click positions the cursor and sets up the drag anchor
    /// as usual, and then also fires the nearest enabled `MouseRegion`
    /// ancestor's `on_click` handler - without requiring `capture_click(true)`
    /// on the region. If `on_click` is also set, link clicks still go to the
    /// document callback and non-link clicks pass through to the ancestor.
    pub fn passthrough_clicks(mut self, passthrough: bool) -> Self {
        self.passthrough_clicks = passthrough;
        self
    }

    /// Set the scroll event callback.
    pub fn on_scroll(mut self, cb: Callback<ScrollEvent>) -> Self {
        self.on_scroll = Some(cb);
        self
    }

    /// Set the click event callback.
    ///
    /// For the common "just open the URL" case when a link span was hit, see
    /// [`crate::callbacks::open_document_link`].
    pub fn on_click(mut self, cb: Callback<DocumentClickEvent>) -> Self {
        self.on_click = Some(cb);
        self
    }

    /// Set the text selection callback.
    pub fn on_select(mut self, cb: Callback<DocumentSelectEvent>) -> Self {
        self.on_select = Some(cb);
        self
    }

    /// Set the keyboard handler.
    pub fn on_key(mut self, cb: KeyHandler) -> Self {
        self.on_key = Some(cb);
        self
    }

    /// Set the shared selection group id.
    ///
    /// `DocumentView`s under the same `ScrollView` that share this value
    /// participate in cross-widget linear selection and unified copy.
    pub fn shared_selection_id(mut self, id: impl Into<Arc<str>>) -> Self {
        self.shared_selection_id = Some(id.into());
        self
    }

    /// Set the syntax highlighting strategy for code blocks.
    #[cfg(feature = "syntax-syntect")]
    pub fn code_syntax_strategy(
        mut self,
        strategy: impl crate::widgets::TextAreaColorStrategy + 'static,
    ) -> Self {
        self.code_syntax_strategy = Some(Rc::new(strategy));
        self
    }

    /// Convenience: set a [`MarkdownFormatter`] with default styles.
    #[cfg(feature = "markdown")]
    pub fn markdown(mut self) -> Self {
        self = self
            .formatter(MarkdownFormatter::default())
            .content_type("markdown");

        #[cfg(feature = "syntax-syntect")]
        if self.code_syntax_strategy.is_none() {
            self = self.code_syntax_strategy(
                crate::widgets::SyntectStrategy::default().default_theme("One Dark (Atom)"),
            );
        }

        self
    }

    /// Convenience: set markdown formatter with compact block spacing.
    ///
    /// When `compact` is `true`, blank lines and Markdown fence spacers are collapsed.
    #[cfg(feature = "markdown")]
    pub fn markdown_compact(mut self, compact: bool) -> Self {
        self = self
            .formatter(MarkdownFormatter::default().compact_blocks(compact))
            .content_type("markdown");

        #[cfg(feature = "syntax-syntect")]
        if self.code_syntax_strategy.is_none() {
            self = self.code_syntax_strategy(
                crate::widgets::SyntectStrategy::default().default_theme("One Dark (Atom)"),
            );
        }

        self
    }

    /// Toggle Mermaid diagram rendering on the active [`MarkdownFormatter`].
    ///
    /// Defaults to `true` via `.markdown()` / `.markdown_compact()`. Set to
    /// `false` to render ```mermaid fences as plain code blocks. No effect
    /// if the current formatter is not a [`MarkdownFormatter`].
    #[cfg(feature = "markdown")]
    pub fn render_diagrams(mut self, enabled: bool) -> Self {
        if let Some(formatter_rc) = self.formatter.as_mut() {
            if Rc::get_mut(formatter_rc).is_none() {
                *formatter_rc = Rc::from(formatter_rc.clone_box());
            }
            if let Some(formatter) = Rc::get_mut(formatter_rc)
                && let Some(md) = formatter.as_any_mut().downcast_mut::<MarkdownFormatter>()
            {
                md.render_diagrams = enabled;
            }
        }
        self
    }
}

impl From<DocumentView> for Element {
    fn from(value: DocumentView) -> Self {
        Element::new(crate::core::element::ElementKind::DocumentView(Box::new(
            value,
        )))
    }
}

impl crate::layout::hash::LayoutHash for DocumentView {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&crate::core::element::Element) -> Option<u64>,
    ) -> Option<()> {
        // Content identity must survive `Arc` reallocation when `view()` rebuilds
        // the same string; use a lazily cached full-text fingerprint (computed once
        // per `DocumentView` instance, not on every hash call after caching).
        self.layout_content_fingerprint().hash(hasher);
        self.content_type.hash(hasher);

        // Formatter geometry hash (stable across Rc recreations). Purely visual
        // theme changes must not invalidate layout hashing.
        self.formatter
            .as_ref()
            .map(|f| f.measure_cache_key())
            .hash(hasher);

        // Layout dimensions.
        self.width.hash(hasher);
        self.resolved_height().hash(hasher);
        self.wrap.hash(hasher);

        // Chrome that affects measured size.
        self.border.hash(hasher);
        self.padding.hash(hasher);
        self.scrollbar.hash(hasher);
        self.scrollbar_config.variant.hash(hasher);
        self.scrollbar_config.gap.hash(hasher);
        self.line_numbers.hash(hasher);
        self.min_line_number_width.hash(hasher);
        self.line_number_separator.hash(hasher);
        self.line_number_content_gap.hash(hasher);
        self.gutter_col_width.hash(hasher);
        self.gutter_gap.hash(hasher);
        self.peer_source_content_fingerprint().hash(hasher);

        #[cfg(feature = "diff-view")]
        if let Some(sync) = &self.split_wrap_sync {
            self.split_wrap_side.hash(hasher);
            self.split_wrap_side
                .and_then(|side| crate::widgets::diff_view::split_wrap_pane_widths(sync, side))
                .hash(hasher);
            crate::widgets::diff_view::split_wrap_scrollbar_cols_pair(sync).hash(hasher);
            crate::widgets::diff_view::split_wrap_layout_pass(sync).hash(hasher);
        }

        // Table layout properties that affect height.
        self.table_wrap.hash(hasher);
        self.table_width_mode.hash(hasher);
        self.table_outer_frame.hash(hasher);
        self.table_column_separators.hash(hasher);
        self.table_row_separators.hash(hasher);
        self.table_cell_padding.hash(hasher);
        self.table_border_variant.hash(hasher);

        Some(())
    }
}
