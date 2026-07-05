//! Content formatting types and traits for [`DocumentView`](super::DocumentView).
//!
//! The [`ContentFormatter`] trait transforms raw text into styled display blocks.
//! Unlike [`TextAreaColorStrategy`](crate::widgets::TextAreaColorStrategy), formatters
//! can **rewrite** content (e.g. strip markdown syntax) and produce structured blocks
//! (tables, code blocks, lists) rather than flat styled lines.

use std::any::Any;
use std::hash::{Hash, Hasher};

use rustc_hash::FxHasher;
use std::sync::Arc;

#[cfg(feature = "profiling-tracing")]
use tracing::trace_span;

use crate::style::{DiffPalette, Span, Style, Theme};

use super::diagram::ParsedDiagram;

// ─── Formatter trait ────────────────────────────────────────────────────────

/// Input data for content formatting.
#[derive(Clone, Copy, Debug)]
pub struct FormatInput<'a> {
    /// The full source text.
    pub value: &'a str,
    /// Optional content-type hint (e.g. `"markdown"`, `"log"`, file extension).
    pub content_type: Option<&'a str>,
    /// Effective document styles for theme-aware formatters.
    pub document_styles: Option<&'a DocumentStyles>,
}

/// Transforms raw text into a structured, styled document.
///
/// Implement this trait to provide custom rendering for any text format.
/// The framework ships [`PlainFormatter`] (no transformation) and, behind the
#[cfg_attr(
    feature = "markdown",
    doc = " `markdown` feature, [`MarkdownFormatter`](super::MarkdownFormatter)."
)]
#[cfg_attr(
    not(feature = "markdown"),
    doc = " `markdown` feature, `MarkdownFormatter`."
)]
pub trait ContentFormatter: Any {
    /// Transform source text into display-ready formatted blocks.
    fn format(&self, input: FormatInput<'_>) -> FormattedDocument;

    /// Transform source text into a geometry-only formatted document for
    /// measurement.
    ///
    /// The default falls back to [`Self::format`]. Override this when you can
    /// preserve line count / wrapping behavior without doing expensive visual
    /// styling work such as syntax highlighting.
    fn measure_format(&self, input: FormatInput<'_>) -> FormattedDocument {
        self.format(input)
    }

    /// Stable hash for cache invalidation.
    ///
    /// Return a value that changes when the formatter's configuration changes
    /// (e.g. style overrides, options). The default returns `0`, meaning
    /// "always re-format when the value changes".
    fn cache_key(&self) -> u64 {
        0
    }

    /// Stable hash for geometry-only measurement cache invalidation.
    ///
    /// Unlike [`Self::cache_key`], this should exclude purely cosmetic style
    /// changes that do not alter line count, wrapping, or measured width.
    /// The default falls back to `cache_key()` for custom formatters.
    fn measure_cache_key(&self) -> u64 {
        self.cache_key()
    }

    /// Downcast support for framework theme integration.
    fn as_any(&self) -> &dyn Any;

    /// Mutable downcast support for framework theme integration.
    fn as_any_mut(&mut self) -> &mut dyn Any;

    /// Clone into an owned `Box` for theme application when the formatter `Rc` is shared.
    ///
    /// Prefer `Box::new(self.clone())` when the concrete type is [`Clone`].
    fn clone_box(&self) -> Box<dyn ContentFormatter>;

    /// Called when a [`Theme`] is applied to a [`super::DocumentView`] during theme-provider
    /// expansion. Default is a no-op.
    ///
    /// With `syntax-syntect`, forward into a shared inner strategy the same way `ThemeProvider`
    /// does for [`TextArea`](crate::widgets::TextArea): call `apply_syntect_strategy_app_theme`
    /// on the inner `Rc` (re-exported at the crate root).
    ///
    /// ```ignore
    /// fn set_app_theme_if_absent(&mut self, theme: &Theme) {
    ///     apply_syntect_strategy_app_theme(&mut self.syntax, theme);
    /// }
    /// ```
    fn set_app_theme_if_absent(&mut self, _theme: &Theme) {}
}

// ─── Document model ─────────────────────────────────────────────────────────

/// A fully formatted document, ready for layout and rendering.
#[derive(Clone, Debug, Default)]
pub struct FormattedDocument {
    /// The top-level blocks in the document.
    pub blocks: Vec<FormattedBlock>,
}

/// A structural block in a formatted document.
#[derive(Clone, Debug)]
pub enum FormattedBlock {
    /// One or more styled text lines.
    Lines(Vec<FormattedLine>),
    /// A table with headers, rows, and column alignments.
    Table(FormattedTable),
    /// A fenced code block with optional language hint.
    CodeBlock(FormattedCodeBlock),
    /// A parsed Mermaid diagram block.
    Diagram(FormattedDiagramBlock),
    /// A horizontal rule / divider.
    HorizontalRule {
        /// Source line in the original text.
        source_line: usize,
    },
    /// A blockquote (can nest other blocks).
    BlockQuote(FormattedBlockQuote),
    /// An ordered or unordered list.
    List(FormattedList),
}

/// A single styled display line.
#[derive(Clone, Debug)]
pub struct FormattedLine {
    /// Styled segments for this line.
    pub spans: Vec<Span>,
    /// Which source line (0-indexed) this display line maps to.
    pub source_line: usize,
    /// Logical indent level (rendered as leading spaces by the widget).
    pub indent: u16,
    /// Clickable link ranges for this rendered line (byte offsets in concatenated span text).
    pub links: Vec<FormattedLink>,
}

/// Clickable link range within a formatted line.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FormattedLink {
    /// Start byte offset (inclusive) in the rendered line text.
    pub start: usize,
    /// End byte offset (exclusive) in the rendered line text.
    pub end: usize,
    /// Link destination URL.
    pub url: Arc<str>,
}

/// Column alignment for table cells.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ColumnAlign {
    /// Left-aligned (default).
    #[default]
    Left,
    /// Center-aligned.
    Center,
    /// Right-aligned.
    Right,
}

/// A formatted table block.
#[derive(Clone, Debug)]
pub struct FormattedTable {
    /// Header cells (one `Vec<Span>` per column).
    pub headers: Vec<Vec<Span>>,
    /// Data rows. Each row is a `Vec` of cells, each cell is `Vec<Span>`.
    pub rows: Vec<Vec<Vec<Span>>>,
    /// Per-column alignment.
    pub alignments: Vec<ColumnAlign>,
    /// Source line (0-indexed) where this table starts in the original text.
    pub source_line_start: usize,
}

/// A fenced code block.
#[derive(Clone, Debug)]
pub struct FormattedCodeBlock {
    /// Language hint (e.g. `"rust"`, `"python"`). `None` for plain code blocks.
    pub language: Option<Arc<str>>,
    /// Raw code text.
    pub code: Arc<str>,
    /// Source line (0-indexed) where this code block starts.
    pub source_line_start: usize,
}

/// A parsed diagram block with its original source text retained for copy/selection.
#[derive(Clone, Debug)]
pub struct FormattedDiagramBlock {
    /// Parsed diagram data.
    pub diagram: ParsedDiagram,
    /// Raw Mermaid source text.
    pub source_code: Arc<str>,
    /// Source line (0-indexed) where this fenced block starts.
    pub source_line_start: usize,
}

/// A blockquote container.
#[derive(Clone, Debug)]
pub struct FormattedBlockQuote {
    /// Nested blocks within the quote.
    pub blocks: Vec<FormattedBlock>,
    /// Nesting depth (1 = single `>`, 2 = `>> `, etc.).
    pub depth: u16,
    /// Source line (0-indexed) where this blockquote starts.
    pub source_line_start: usize,
}

/// An ordered or unordered list.
#[derive(Clone, Debug)]
pub struct FormattedList {
    /// Whether this is an ordered (numbered) list.
    pub ordered: bool,
    /// Start number for ordered lists (typically 1).
    pub start: usize,
    /// List items.
    pub items: Vec<FormattedListItem>,
    /// Source line (0-indexed) where this list starts.
    pub source_line_start: usize,
}

/// A single list item (can contain nested blocks).
#[derive(Clone, Debug)]
pub struct FormattedListItem {
    /// Content blocks within this item.
    pub content: Vec<FormattedBlock>,
    /// Source line (0-indexed) in the original text.
    pub source_line: usize,
}

// ─── Plain formatter (default, no-dep) ──────────────────────────────────────

/// A no-op formatter that wraps each line as a plain unstyled span.
///
/// This is the default when no [`ContentFormatter`] is provided to
/// [`DocumentView`](super::DocumentView).
#[derive(Clone, Debug, Default)]
pub struct PlainFormatter;

impl ContentFormatter for PlainFormatter {
    fn clone_box(&self) -> Box<dyn ContentFormatter> {
        Box::new(self.clone())
    }

    fn format(&self, input: FormatInput<'_>) -> FormattedDocument {
        let lines: Vec<FormattedLine> = if input.value.is_empty() {
            vec![FormattedLine {
                spans: vec![Span::new("")],
                source_line: 0,
                indent: 0,
                links: Vec::new(),
            }]
        } else {
            input
                .value
                .split('\n')
                .enumerate()
                .map(|(i, line)| FormattedLine {
                    spans: vec![Span::new(line)],
                    source_line: i,
                    indent: 0,
                    links: Vec::new(),
                })
                .collect()
        };

        FormattedDocument {
            blocks: vec![FormattedBlock::Lines(lines)],
        }
    }

    fn measure_cache_key(&self) -> u64 {
        0
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

// ─── Format cache ───────────────────────────────────────────────────────────

/// Cache key combining hashes of all formatter inputs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FormatCacheKey {
    pub value_hash: u64,
    pub formatter_hash: u64,
    pub content_type_hash: u64,
    pub document_styles_hash: u64,
}

/// Caches the last formatting result to avoid re-parsing on every frame.
#[derive(Clone, Debug, Default)]
pub(crate) struct FormatCache {
    pub key: Option<FormatCacheKey>,
    pub document: FormattedDocument,
}

impl FormatCache {
    /// Update the cache if the inputs have changed.
    ///
    /// Returns `true` if the cache was invalidated and re-formatted.
    pub fn update(
        &mut self,
        formatter: &dyn ContentFormatter,
        value: &str,
        content_type: Option<&str>,
        document_styles: &DocumentStyles,
    ) -> bool {
        #[cfg(feature = "profiling-tracing")]
        let _span = trace_span!("document_view.format_cache_update").entered();

        let value_hash = hash_str(value);
        let formatter_hash = formatter.cache_key();
        let content_type_hash = content_type.map(hash_str).unwrap_or(0);

        let new_key = FormatCacheKey {
            value_hash,
            formatter_hash,
            content_type_hash,
            document_styles_hash: hash_document_styles(document_styles),
        };

        if self.key.as_ref() == Some(&new_key) {
            #[cfg(feature = "profiling-tracing")]
            tracing::trace!(target: "tui_lipan::perf", cache_hit = true);
            return false;
        }

        #[cfg(feature = "profiling-tracing")]
        let _format_span = trace_span!("document_view.format").entered();
        self.document = formatter.format(FormatInput {
            value,
            content_type,
            document_styles: Some(document_styles),
        });
        #[cfg(feature = "profiling-tracing")]
        tracing::trace!(
            target: "tui_lipan::perf",
            cache_hit = false,
            bytes = value.len(),
            has_content_type = content_type.is_some()
        );
        self.key = Some(new_key);
        true
    }
}

fn hash_str(s: &str) -> u64 {
    let mut hasher = FxHasher::default();
    s.hash(&mut hasher);
    hasher.finish()
}

fn hash_document_styles(styles: &DocumentStyles) -> u64 {
    let mut hasher = FxHasher::default();
    styles.heading_styles.hash(&mut hasher);
    styles.code_inline_style.hash(&mut hasher);
    styles.code_block_style.hash(&mut hasher);
    styles.emphasis_style.hash(&mut hasher);
    styles.strong_style.hash(&mut hasher);
    styles.strikethrough_style.hash(&mut hasher);
    styles.link_style.hash(&mut hasher);
    styles.blockquote_bar_style.hash(&mut hasher);
    styles.table_border_style.hash(&mut hasher);
    styles.table_header_style.hash(&mut hasher);
    styles.hr_style.hash(&mut hasher);
    styles.list_item_style.hash(&mut hasher);
    styles.list_enumeration_style.hash(&mut hasher);
    styles.diagram_node_fill_style.hash(&mut hasher);
    styles.diagram_node_border_style.hash(&mut hasher);
    styles.diagram_node_label_style.hash(&mut hasher);
    styles.diagram_edge_style.hash(&mut hasher);
    styles.diagram_muted_style.hash(&mut hasher);
    styles.diff_palette.hash(&mut hasher);
    hasher.finish()
}

// ─── Style defaults for formatted elements ──────────────────────────────────

/// Default styles for document elements, used by formatters and the renderer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DocumentStyles {
    /// Heading styles (h1 through h6).
    pub heading_styles: [Style; 6],
    /// Inline code style (e.g. `` `code` ``).
    pub code_inline_style: Style,
    /// Code block background/text style.
    pub code_block_style: Style,
    /// *Emphasis* (italic) style.
    pub emphasis_style: Style,
    /// **Strong** (bold) style.
    pub strong_style: Style,
    /// ~~Strikethrough~~ style.
    pub strikethrough_style: Style,
    /// Link style (typically underlined + colored).
    pub link_style: Style,
    /// Blockquote left-bar decoration style.
    pub blockquote_bar_style: Style,
    /// Table border (box-drawing) style.
    pub table_border_style: Style,
    /// Table header text style.
    pub table_header_style: Style,
    /// Horizontal rule style.
    pub hr_style: Style,
    /// Unordered list bullet point style.
    pub list_item_style: Style,
    /// Ordered list enumeration number style.
    pub list_enumeration_style: Style,
    /// Diagram node fill style.
    pub diagram_node_fill_style: Style,
    /// Diagram node border style.
    pub diagram_node_border_style: Style,
    /// Diagram node label style.
    pub diagram_node_label_style: Style,
    /// Diagram edge style.
    pub diagram_edge_style: Style,
    /// Diagram muted style for auxiliary glyphs (sequence lifelines, etc.).
    pub diagram_muted_style: Style,
    /// Diff palette used to colorize fenced ` ```diff ` / ` ```patch ` code blocks.
    pub diff_palette: DiffPalette,
}

impl DocumentStyles {
    pub(crate) fn from_theme(theme: &Theme) -> Self {
        Self {
            heading_styles: theme.document.heading_styles,
            code_inline_style: theme.document.code_inline,
            code_block_style: theme.document.code_block,
            emphasis_style: theme.document.emphasis,
            strong_style: theme.document.strong,
            strikethrough_style: theme.document.strikethrough,
            link_style: theme.document.link,
            blockquote_bar_style: theme.document.blockquote_bar,
            table_border_style: theme.document.table_border,
            table_header_style: theme.document.table_header,
            hr_style: theme.document.hr,
            list_item_style: theme.document.list_item,
            list_enumeration_style: theme.document.list_enumeration,
            diagram_node_fill_style: theme.document.diagram_node_fill_style,
            diagram_node_border_style: theme.document.diagram_node_border_style,
            diagram_node_label_style: theme.document.diagram_node_label_style,
            diagram_edge_style: theme.document.diagram_edge_style,
            diagram_muted_style: theme.document.diagram_muted_style,
            diff_palette: theme.diff,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use super::*;

    use crate::core::element::Element;
    use crate::style::apply_document_theme_carve_out;
    use crate::widgets::{DocumentView, ThemeProvider};

    fn as_lines(document: FormattedDocument) -> Vec<FormattedLine> {
        let mut blocks = document.blocks;
        assert_eq!(blocks.len(), 1);
        match blocks.remove(0) {
            FormattedBlock::Lines(lines) => lines,
            other => panic!("expected FormattedBlock::Lines, got {other:?}"),
        }
    }

    #[test]
    fn format_cache_update_invalidates_then_hits_then_invalidates_on_change() {
        let formatter = PlainFormatter;
        let mut cache = FormatCache::default();

        assert!(cache.update(&formatter, "hello", None, &DocumentStyles::default()));
        assert!(!cache.update(&formatter, "hello", None, &DocumentStyles::default(),));
        assert!(cache.update(&formatter, "hello world", None, &DocumentStyles::default(),));
    }

    #[derive(Clone)]
    struct RecordingFormatter {
        flag: Arc<AtomicBool>,
    }

    impl ContentFormatter for RecordingFormatter {
        fn clone_box(&self) -> Box<dyn ContentFormatter> {
            Box::new(self.clone())
        }

        fn format(&self, input: FormatInput<'_>) -> FormattedDocument {
            PlainFormatter.format(input)
        }

        fn set_app_theme_if_absent(&mut self, _theme: &Theme) {
            self.flag.store(true, Ordering::SeqCst);
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn as_any_mut(&mut self) -> &mut dyn Any {
            self
        }
    }

    #[test]
    fn theme_provider_invokes_formatter_set_app_theme_when_rc_unique() {
        let flag = Arc::new(AtomicBool::new(false));
        let el: Element = ThemeProvider::new(Theme::default())
            .child(DocumentView::new("hello").formatter(RecordingFormatter { flag: flag.clone() }))
            .into();
        let _ = apply_document_theme_carve_out(&Theme::default(), el);
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn theme_provider_invokes_formatter_set_app_theme_when_formatter_rc_shared() {
        let flag = Arc::new(AtomicBool::new(false));
        let dv = DocumentView::new("hello").formatter(RecordingFormatter { flag: flag.clone() });
        // Second strong ref to the formatter Rc: theme path must clone_box then set_app_theme.
        let _share = dv.clone();
        let el: Element = ThemeProvider::new(Theme::default()).child(dv).into();
        let _ = apply_document_theme_carve_out(&Theme::default(), el);
        assert!(flag.load(Ordering::SeqCst));
    }

    #[test]
    fn plain_formatter_empty_input_produces_single_empty_line() {
        let formatter = PlainFormatter;
        let lines = as_lines(formatter.format(FormatInput {
            value: "",
            content_type: None,
            document_styles: None,
        }));

        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].source_line, 0);
        assert_eq!(lines[0].spans.len(), 1);
        assert_eq!(lines[0].spans[0].content.as_ref(), "");
    }

    #[test]
    fn plain_formatter_splits_lines_and_keeps_empty_middle_lines() {
        let formatter = PlainFormatter;
        let lines = as_lines(formatter.format(FormatInput {
            value: "alpha\n\nbeta",
            content_type: None,
            document_styles: None,
        }));

        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].spans[0].content.as_ref(), "alpha");
        assert_eq!(lines[1].spans[0].content.as_ref(), "");
        assert_eq!(lines[2].spans[0].content.as_ref(), "beta");
        assert_eq!(lines[0].source_line, 0);
        assert_eq!(lines[1].source_line, 1);
        assert_eq!(lines[2].source_line, 2);
    }

    #[test]
    fn plain_formatter_preserves_trailing_newline_as_empty_last_line() {
        let formatter = PlainFormatter;
        let lines = as_lines(formatter.format(FormatInput {
            value: "alpha\n",
            content_type: None,
            document_styles: None,
        }));

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].spans[0].content.as_ref(), "alpha");
        assert_eq!(lines[1].spans[0].content.as_ref(), "");
        assert_eq!(lines[1].source_line, 1);
    }
}
