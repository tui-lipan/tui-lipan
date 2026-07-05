//! Diff view widget.

mod formatter;
mod render;
mod strategy;
mod types;
pub(crate) mod wrap_sync;

pub(crate) use formatter::*;
pub(crate) use render::*;
pub(crate) use strategy::*;
pub use types::*;
pub(crate) use wrap_sync::*;

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use crate::callback::Callback;
use crate::core::element::Element;
use crate::style::{Color, DiffPalette, Length, Style};
use crate::widgets::{Divider, DocumentView, Frame, HStack, ScrollEvent, TextArea};
use rustc_hash::FxHasher;
use std::rc::Rc;
use std::sync::Arc;

// ── DiffData cache ───────────────────────────────────────────────────────────

const PANE_DATA_CACHE_LIMIT: usize = 2048;
const DIFF_GUTTER_SPANS_CACHE_LIMIT: usize = 4096;
/// Bound for the content- and pointer-keyed `DiffData` caches. The live working
/// set is the currently visible diffs (small), so a long session that scrolls
/// through many distinct files would otherwise grow these unbounded. Clear-on-
/// overflow (matching `PANE_DATA_CACHE`) caps memory; the only cost on overflow
/// is re-parsing a diff on next view, which happens off the resize hot path.
const DIFF_DATA_CACHE_LIMIT: usize = 2048;

// Global cache for `DiffData` results.  Keyed on source contents + config so
// repeated element rebuilds (e.g. during scroll/viewport metadata updates) skip
// expensive diff/patch parsing without risking stale collision hits.
thread_local! {
    static DIFF_DATA_CACHE: RefCell<HashMap<u64, Arc<DiffData>>> =
        RefCell::new(HashMap::new());
    static PATCH_DIFF_DATA_PTR_CACHE: RefCell<HashMap<PatchDiffDataPtrKey, Arc<DiffData>>> =
        RefCell::new(HashMap::new());
    static PANE_DATA_CACHE: RefCell<HashMap<(PaneCacheKey, u64), PaneCacheEntry>> =
        RefCell::new(HashMap::new());
}

fn diff_data_cache_key(before: &str, after: &str, config: &DiffDataConfig) -> u64 {
    let mut h = FxHasher::default();
    0u8.hash(&mut h);
    before.hash(&mut h);
    after.hash(&mut h);
    config.hash(&mut h);
    h.finish()
}

fn patch_diff_data_cache_key(patch: &str, config: &DiffDataConfig) -> u64 {
    let mut h = FxHasher::default();
    1u8.hash(&mut h);
    patch.hash(&mut h);
    config.hash(&mut h);
    h.finish()
}

#[derive(Clone)]
struct PatchDiffDataPtrKey {
    patch: Arc<str>,
    config: DiffDataConfig,
}

impl PartialEq for PatchDiffDataPtrKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.patch, &other.patch) && self.config == other.config
    }
}

impl Eq for PatchDiffDataPtrKey {}

impl Hash for PatchDiffDataPtrKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.patch.as_ptr().hash(state);
        self.patch.len().hash(state);
        self.config.hash(state);
    }
}

fn cached_diff_data(before: &str, after: &str, config: DiffDataConfig) -> Arc<DiffData> {
    let key = diff_data_cache_key(before, after, &config);
    DIFF_DATA_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if let Some(data) = map.get(&key) {
            return Arc::clone(data);
        }
        let data = Arc::new(DiffData::with_config(before, after, config));
        if map.len() >= DIFF_DATA_CACHE_LIMIT {
            map.clear();
        }
        map.insert(key, Arc::clone(&data));
        data
    })
}

fn cached_patch_diff_data(patch: Arc<str>, config: DiffDataConfig) -> Arc<DiffData> {
    let ptr_key = PatchDiffDataPtrKey {
        patch: Arc::clone(&patch),
        config: config.clone(),
    };
    if let Some(data) =
        PATCH_DIFF_DATA_PTR_CACHE.with(|cache| cache.borrow().get(&ptr_key).cloned())
    {
        return data;
    }

    let content_key = patch_diff_data_cache_key(patch.as_ref(), &config);
    let data = DIFF_DATA_CACHE.with(|cache| {
        let mut map = cache.borrow_mut();
        if let Some(data) = map.get(&content_key) {
            return Arc::clone(data);
        }
        let data = Arc::new(DiffData::from_patch_with_config(patch.as_ref(), config));
        if map.len() >= DIFF_DATA_CACHE_LIMIT {
            map.clear();
        }
        map.insert(content_key, Arc::clone(&data));
        data
    });
    PATCH_DIFF_DATA_PTR_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.len() >= DIFF_DATA_CACHE_LIMIT && !cache.contains_key(&ptr_key) {
            cache.clear();
        }
        cache.insert(ptr_key, Arc::clone(&data));
    });
    data
}

#[cfg(feature = "syntax-syntect")]
use crate::widgets::SyntectStrategy;
use crate::widgets::TextAreaColorStrategy;

/// Cache key for per-pane derived data (numbered render, gutter, excluded lines).
#[derive(Clone, PartialEq, Eq, Hash)]
pub(crate) struct PaneCacheKey {
    diff_hash: u64,
    mode: DiffViewMode,
    pane: DiffPane,
    line_numbers: bool,
    min_digits: usize,
}

/// Cached per-pane data to avoid recomputing on every frame.
#[derive(Clone)]
pub(crate) struct PaneCacheEntry {
    key: PaneCacheKey,
    gutter_style_hash: u64,
    numbered_render: DiffRender,
    gutter_spans: DiffGutterSpans,
    gutter_col_width: u16,
    excluded_source_lines: Arc<Vec<usize>>,
    excluded_bytes: Arc<Vec<(usize, usize)>>,
}

#[derive(Clone, Copy)]
pub(crate) struct PaneRenderOptions {
    mode: DiffViewMode,
    line_numbers: bool,
    min_digits: usize,
    style: DiffPalette,
    gutter_style_hash: u64,
}

/// A diff view widget with selectable rendering backends.
#[derive(Clone)]
pub struct DiffView {
    before: Arc<str>,
    after: Arc<str>,
    patch: Option<Arc<str>>,
    mode: DiffViewMode,
    backend: DiffViewBackend,
    backend_explicit: bool,
    width_override: Option<Length>,
    height_override: Option<Length>,
    editable: bool,
    wrap_override: Option<bool>,
    line_numbers_override: Option<bool>,
    min_line_number_width_override: Option<u8>,
    gutter_inset_override: Option<u16>,
    scrollbar_override: Option<bool>,
    h_scrollbar_override: Option<bool>,
    focusable_override: Option<bool>,
    outer_border: bool,
    pane_border: bool,
    highlight_full_width: bool,
    single_scrollbar: bool,
    join_frame: bool,
    vertical_separator: bool,
    vertical_separator_char: char,
    vertical_separator_style: Style,
    scroll_offset: Option<usize>,
    scroll_to_hunk: Option<usize>,
    on_scroll: Option<Callback<DiffScrollEvent>>,
    on_context_separator_click: Option<Callback<DiffContextSeparatorEvent>>,
    context_separator_hover_style: Option<Style>,
    text_area: TextArea,
    document_view: DocumentView,
    diff_style: DiffPalette,
    prefixes: DiffPrefixes,
    show_prefixes: bool,
    word_diff: bool,
    trim_common_indent: bool,
    shared_selection_id: Option<Arc<str>>,
    language: Option<Arc<str>>,
    theme: Option<Arc<str>>,
    base_color_strategy: Option<Rc<dyn TextAreaColorStrategy>>,
    diff_data: Option<Arc<DiffData>>,
    context_lines: Option<usize>,
    show_context_separator: bool,
    context_separator_text: Arc<str>,
    context_separator_min_lines: usize,
    context_expand_lines: usize,
    expanded_contexts: Vec<DiffContextExpansion>,
    /// Per-pane data cache (avoids recomputing numbered render, gutter, excluded
    /// lines every frame). Keyed by (diff_hash, mode, pane, line_numbers, min_digits, style).
    pane_cache: RefCell<Vec<PaneCacheEntry>>,
}

impl DiffView {
    /// Create a new, empty diff view.
    pub fn new() -> Self {
        Self::new_internal("".into(), "".into(), None)
    }

    /// Set the "before" content of the diff.
    pub fn before(mut self, before: impl Into<Arc<str>>) -> Self {
        self.before = before.into();
        self
    }

    /// Set the "after" content of the diff.
    pub fn after(mut self, after: impl Into<Arc<str>>) -> Self {
        self.after = after.into();
        self
    }

    /// Set the diff content from a raw unified diff (patch) string.
    pub fn patch(mut self, patch: impl Into<Arc<str>>) -> Self {
        let patch = patch.into();
        self.text_area = self.text_area.value("");
        self.document_view = self.document_view.value("");
        self.before = "".into();
        self.after = "".into();
        self.diff_data = None;
        self.patch = Some(patch);
        self
    }

    /// Create a diff view for the given before/after text.
    pub fn with_content(before: impl Into<Arc<str>>, after: impl Into<Arc<str>>) -> Self {
        Self::new_internal(before.into(), after.into(), None)
    }

    /// Create a diff view from a raw unified diff (patch) string.
    ///
    /// This constructor is useful when you have a pre-computed patch (e.g. from
    /// git or an API) and want to display it with `DiffView` colors and
    /// formatting.
    pub fn from_patch(patch: impl Into<Arc<str>>) -> Self {
        Self::new().patch(patch)
    }

    fn new_internal(before: Arc<str>, after: Arc<str>, diff_data: Option<DiffData>) -> Self {
        let text_area = TextArea::new("")
            .read_only(true)
            .line_numbers(true)
            .wrap(false)
            .scrollbar(true)
            .h_scrollbar(true);
        let document_view = DocumentView::new("")
            .line_numbers(true)
            .wrap(false)
            .scrollbar(true)
            .h_scrollbar(true);
        Self {
            before,
            after,
            patch: None,
            mode: DiffViewMode::Split,
            backend: DiffViewBackend::TextArea,
            backend_explicit: false,
            width_override: None,
            height_override: None,
            editable: false,
            wrap_override: None,
            line_numbers_override: None,
            min_line_number_width_override: None,
            gutter_inset_override: None,
            scrollbar_override: None,
            h_scrollbar_override: None,
            focusable_override: None,
            outer_border: false,
            pane_border: true,
            highlight_full_width: false,
            single_scrollbar: false,
            join_frame: false,
            vertical_separator: false,
            vertical_separator_char: '│',
            vertical_separator_style: Style::default(),
            scroll_offset: None,
            scroll_to_hunk: None,
            on_scroll: None,
            on_context_separator_click: None,
            context_separator_hover_style: None,
            text_area,
            document_view,
            diff_style: DiffPalette::default(),
            prefixes: DiffPrefixes::default(),
            show_prefixes: true,
            word_diff: true,
            trim_common_indent: true,
            shared_selection_id: None,
            language: None,
            theme: None,
            base_color_strategy: None,
            diff_data: diff_data.map(Arc::new),
            context_lines: None,
            show_context_separator: true,
            context_separator_text: default_context_separator_text(),
            context_separator_min_lines: default_context_separator_min_lines(),
            context_expand_lines: default_context_expand_lines(),
            expanded_contexts: Vec::new(),
            pane_cache: RefCell::new(Vec::new()),
        }
    }

    /// Set rendering backend explicitly.
    ///
    /// Not required in most cases: calling [`Self::text_area`] or
    /// [`Self::document_view`] also infers backend when this isn't set.
    pub fn backend(mut self, backend: DiffViewBackend) -> Self {
        self.backend = backend;
        self.backend_explicit = true;
        self
    }

    /// Enable/disable editing when using the `TextArea` backend.
    ///
    /// This flag is ignored by the `DocumentView` backend.
    pub fn editable(mut self, editable: bool) -> Self {
        self.editable = editable;
        self
    }

    /// Set controlled vertical scroll offset for rendered pane(s).
    pub fn scroll_offset(mut self, offset: usize) -> Self {
        self.scroll_offset = Some(offset);
        self
    }

    /// Scroll rendered pane(s) to the first visible row for a parsed patch hunk.
    ///
    /// `index` is zero-based patch order. The target is resolved after
    /// indentation trimming and context collapse, then delegated to the active
    /// backend so soft wrapping maps to the final visual row during layout.
    /// A controlled [`Self::scroll_offset`] takes precedence if both are set.
    pub fn scroll_to_hunk(mut self, index: usize) -> Self {
        self.scroll_to_hunk = Some(index);
        self
    }

    /// Receive pane-aware scroll events from rendered pane(s).
    pub fn on_scroll(mut self, cb: Callback<DiffScrollEvent>) -> Self {
        self.on_scroll = Some(cb);
        self
    }

    /// Set the callback fired when a visible context separator is clicked.
    ///
    /// Use [`Self::expanded_contexts`] with the clicked event's range on the
    /// next render to expand the hidden lines represented by that separator.
    pub fn on_context_separator_click(mut self, cb: Callback<DiffContextSeparatorEvent>) -> Self {
        self.on_context_separator_click = Some(cb);
        self
    }

    /// Set the style patched over a context separator while the pointer hovers it.
    pub fn context_separator_hover_style(mut self, style: Style) -> Self {
        self.context_separator_hover_style = Some(style);
        self
    }

    /// Set the diff presentation mode.
    pub fn mode(mut self, mode: DiffViewMode) -> Self {
        self.mode = mode;
        self
    }

    /// Override the outer diff view width.
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width_override = Some(width.into());
        self
    }

    /// Override the outer diff view height.
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height_override = Some(height.into());
        self
    }

    /// Provide a base `TextArea` configuration.
    ///
    /// When backend is not explicitly set, this also selects the `TextArea`
    /// backend.
    pub fn text_area(mut self, text_area: TextArea) -> Self {
        if self.base_color_strategy.is_none() {
            self.base_color_strategy = text_area.color_strategy.clone();
        }
        if self.language.is_none() {
            self.language = text_area.language.clone();
        }
        if self.theme.is_none() {
            self.theme = text_area.theme.clone();
        }
        self.text_area = text_area;
        if let Some(v) = self.wrap_override {
            self.text_area = self.text_area.wrap(v);
        }
        if let Some(v) = self.gutter_inset_override {
            self.text_area = self.text_area.gutter_inset(v);
        }
        if let Some(v) = self.scrollbar_override {
            self.text_area = self.text_area.scrollbar(v);
        }
        if let Some(v) = self.h_scrollbar_override {
            self.text_area = self.text_area.h_scrollbar(v);
        }
        if let Some(v) = self.focusable_override {
            self.text_area = self.text_area.focusable(v);
        }
        if !self.backend_explicit {
            self.backend = DiffViewBackend::TextArea;
        }
        self
    }

    /// Provide a base `DocumentView` configuration.
    ///
    /// When backend is not explicitly set, this also selects the
    /// `DocumentView` backend.
    pub fn document_view(mut self, document_view: DocumentView) -> Self {
        self.document_view = document_view;
        if let Some(v) = self.wrap_override {
            self.document_view = self.document_view.wrap(v);
        }
        if let Some(v) = self.gutter_inset_override {
            self.document_view = self.document_view.gutter_inset(v);
        }
        if let Some(v) = self.scrollbar_override {
            self.document_view = self.document_view.scrollbar(v);
        }
        if let Some(v) = self.h_scrollbar_override {
            self.document_view = self.document_view.h_scrollbar(v);
        }
        if let Some(v) = self.focusable_override {
            self.document_view = self.document_view.focusable(v);
        }
        if !self.backend_explicit {
            self.backend = DiffViewBackend::DocumentView;
        }
        self
    }

    /// Enable/disable wrapping in both backends.
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap_override = Some(wrap);
        self.text_area = self.text_area.wrap(wrap);
        self.document_view = self.document_view.wrap(wrap);
        self
    }

    /// Enable/disable line numbers in both backends.
    pub fn line_numbers(mut self, show: bool) -> Self {
        self.line_numbers_override = Some(show);
        self
    }

    /// Set minimum line-number digit width in both backends.
    pub fn min_line_number_width(mut self, width: u8) -> Self {
        self.min_line_number_width_override = Some(width);
        self
    }

    /// Show or hide an outer border around the whole `DiffView`.
    pub fn border(mut self, border: bool) -> Self {
        self.outer_border = border;
        self
    }

    /// Show or hide per-pane borders in split/unified pane wrappers.
    pub fn panels_border(mut self, border: bool) -> Self {
        self.pane_border = border;
        self
    }

    /// Highlight changed-line backgrounds to full row width.
    pub fn highlight_full_width(mut self, enabled: bool) -> Self {
        self.highlight_full_width = enabled;
        self
    }

    /// Show or hide vertical scrollbars in both backends.
    pub fn scrollbar(mut self, show: bool) -> Self {
        self.scrollbar_override = Some(show);
        self.text_area = self.text_area.scrollbar(show);
        self.document_view = self.document_view.scrollbar(show);
        self
    }

    /// Reserve empty cells before the gutter / line numbers in both backends.
    pub fn gutter_inset(mut self, inset: u16) -> Self {
        self.gutter_inset_override = Some(inset);
        self.text_area = self.text_area.gutter_inset(inset);
        self.document_view = self.document_view.gutter_inset(inset);
        self
    }

    /// Show or hide horizontal scrollbars in both backends.
    pub fn h_scrollbar(mut self, show: bool) -> Self {
        self.h_scrollbar_override = Some(show);
        self.text_area = self.text_area.h_scrollbar(show);
        self.document_view = self.document_view.h_scrollbar(show);
        self
    }

    /// Control focusability in both backends.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.focusable_override = Some(focusable);
        self.text_area = self.text_area.focusable(focusable);
        self.document_view = self.document_view.focusable(focusable);
        self
    }

    /// In split mode, render a single vertical scrollbar on the right pane only.
    pub fn single_scrollbar(mut self, enabled: bool) -> Self {
        self.single_scrollbar = enabled;
        self
    }

    /// Join pane frames when adjacent (uses `Frame::join_frame`).
    pub fn join_frame(mut self, join: bool) -> Self {
        self.join_frame = join;
        self
    }

    /// Show a vertical separator between split panes.
    pub fn vertical_separator(mut self, enabled: bool) -> Self {
        self.vertical_separator = enabled;
        self
    }

    /// Set the split-pane vertical separator character.
    pub fn vertical_separator_char(mut self, ch: char) -> Self {
        self.vertical_separator_char = ch;
        self
    }

    /// Set style for the split-pane vertical separator.
    pub fn vertical_separator_style(mut self, style: Style) -> Self {
        self.vertical_separator_style = style;
        self
    }

    /// Configure diff line and word styles.
    pub fn diff_style(mut self, style: DiffPalette) -> Self {
        self.diff_style = style;
        self
    }

    /// Set background color for unchanged/context lines.
    ///
    /// Note: split filler lines (`DiffLineKind::Empty`) are not affected.
    pub fn neutral_bg(mut self, color: Color) -> Self {
        self.diff_style.context = self.diff_style.context.bg(color);
        self
    }

    /// Enable cross-widget drag selection for this diff view's internal panels.
    ///
    /// In **unified** mode the id is used as-is on the single panel.
    /// In **split** mode the id is suffixed with `:left` and `:right` so that
    /// only same-side panels share selection across multiple `DiffView`s.
    ///
    /// Multiple `DiffView`s (and plain `DocumentView`s) under the same
    /// `ScrollView` that share the same id participate in unified drag
    /// selection and copy.
    pub fn shared_selection_id(mut self, id: impl Into<Arc<str>>) -> Self {
        self.shared_selection_id = Some(id.into());
        self
    }

    /// Configure diff prefixes.
    pub fn prefixes(mut self, prefixes: DiffPrefixes) -> Self {
        self.prefixes = prefixes;
        self
    }

    /// Toggle prefix rendering.
    pub fn show_prefixes(mut self, show: bool) -> Self {
        self.show_prefixes = show;
        self
    }

    /// Toggle word-level diff highlighting.
    pub fn word_diff(mut self, enabled: bool) -> Self {
        self.word_diff = enabled;
        self
    }

    /// Trim shared leading indentation from diff line content.
    ///
    /// When enabled (the default), `DiffView` computes the smallest common
    /// leading indent across visible non-empty diff lines and removes that many
    /// leading spaces/tabs from line content before rendering. In split mode,
    /// both panes share the same trim amount so rows stay aligned.
    pub fn trim_common_indent(mut self, enabled: bool) -> Self {
        self.trim_common_indent = enabled;
        self
    }

    /// Set the number of unchanged context lines to keep around each change.
    ///
    /// When set, unchanged regions farther than `n` lines from any change are
    /// collapsed into a single separator line.  The default (`None`) shows
    /// all lines.
    ///
    /// ```ignore
    /// DiffView::with_content(before, after).context_lines(4)
    /// ```
    pub fn context_lines(mut self, n: usize) -> Self {
        self.context_lines = Some(n);
        self
    }

    /// Show or hide the placeholder separator line when collapsing context.
    ///
    /// When `true` (the default) a context separator line is rendered in place
    /// of each collapsed region. When `false` the hidden lines are simply
    /// omitted without any visual placeholder. Only meaningful when
    /// [`Self::context_lines`] is set.
    pub fn show_context_separator(mut self, show: bool) -> Self {
        self.show_context_separator = show;
        self
    }

    /// Set the template text used for context separators.
    ///
    /// Supported placeholders:
    /// - `{count}`: number of hidden lines
    /// - `{line_word}`: `line` or `lines`
    /// - `{direction}`: `above`, `below`, or `between`
    /// - `{arrow}`: `↑`, `↓`, or `↑↓`
    pub fn context_separator_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.context_separator_text = text.into();
        self
    }

    /// Minimum number of hidden lines required before a context separator is shown.
    ///
    /// Shorter collapsed runs render as normal context lines instead of a
    /// separator placeholder (default: `2`).
    pub fn context_separator_min_lines(mut self, min_lines: usize) -> Self {
        self.context_separator_min_lines = min_lines.max(1);
        self
    }

    /// Default number of hidden lines revealed per context-separator click.
    ///
    /// This value is included in [`DiffContextSeparatorEvent`] and used by
    /// [`DiffContextSeparatorEvent::next_expansion`] (default: `20`).
    pub fn context_expand_lines(mut self, lines: usize) -> Self {
        self.context_expand_lines = lines.max(1);
        self
    }

    /// Set collapsed context ranges that should render fully expanded.
    ///
    /// Pass ranges received from [`DiffContextSeparatorEvent::range`] to turn
    /// individual separators back into their hidden lines. This is controlled
    /// by the app so expansion state survives normal component rerenders.
    pub fn expanded_contexts(mut self, ranges: impl IntoIterator<Item = DiffContextRange>) -> Self {
        self.expanded_contexts = ranges.into_iter().map(DiffContextExpansion::full).collect();
        self
    }

    /// Set controlled partial or full context expansions.
    pub fn expanded_context_expansions(
        mut self,
        expansions: impl IntoIterator<Item = DiffContextExpansion>,
    ) -> Self {
        self.expanded_contexts = expansions.into_iter().collect();
        self
    }

    /// Expand one collapsed context range fully.
    pub fn expanded_context(mut self, range: DiffContextRange) -> Self {
        self.expanded_contexts
            .push(DiffContextExpansion::full(range));
        self
    }

    /// Expand one collapsed context range by a specific number of lines.
    pub fn expanded_context_lines(
        mut self,
        range: DiffContextRange,
        lines_revealed: usize,
    ) -> Self {
        self.expanded_contexts.push(DiffContextExpansion {
            range,
            lines_revealed,
        });
        self
    }

    /// Provide a base color strategy (syntax highlighting).
    ///
    /// Applied to both backends:
    /// - `TextArea` backend via `TextArea::color_strategy`
    /// - `DocumentView` backend via the internal diff formatter
    pub fn base_color_strategy(mut self, strategy: impl TextAreaColorStrategy + 'static) -> Self {
        self.base_color_strategy = Some(Rc::new(strategy));
        self
    }

    /// Set the language identifier for syntax strategies.
    pub fn language(mut self, language: impl Into<Arc<str>>) -> Self {
        let language = language.into();
        self.language = Some(language.clone());
        self.text_area = self.text_area.language(language);
        self
    }

    /// Set language identifier by resolving from a file path's extension or name.
    ///
    /// Uses the default syntect syntax definitions. If no syntax matches the
    /// path, the language remains unset (plain text fallback). TypeScript/TSX
    /// paths fall back to JavaScript/JSX-compatible syntaxes when the default
    /// set does not provide exact grammars.
    #[cfg(feature = "syntax-syntect")]
    pub fn language_from_path(self, path: impl AsRef<std::path::Path>) -> Self {
        if let Some(lang) = crate::widgets::language_from_path(path) {
            self.language(lang)
        } else {
            self
        }
    }

    /// Set the theme identifier for syntax strategies.
    pub fn theme(mut self, theme: impl Into<Arc<str>>) -> Self {
        let theme = theme.into();
        self.theme = Some(theme.clone());
        self.text_area = self.text_area.theme(theme);
        self
    }

    /// Enable syntect-based syntax highlighting.
    #[cfg(feature = "syntax-syntect")]
    pub fn with_syntax(self, language: impl Into<Arc<str>>, theme: impl Into<Arc<str>>) -> Self {
        self.base_color_strategy(SyntectStrategy::default())
            .language(language)
            .theme(theme)
    }

    /// Enable syntect-based syntax highlighting with background colors.
    #[cfg(feature = "syntax-syntect")]
    pub fn with_syntax_bg(self, language: impl Into<Arc<str>>, theme: impl Into<Arc<str>>) -> Self {
        self.base_color_strategy(SyntectStrategy::default().use_background(true))
            .language(language)
            .theme(theme)
    }

    /// Use precomputed diff data.
    pub fn with_diff(mut self, data: DiffData) -> Self {
        self.patch = None;
        self.diff_data = Some(Arc::new(data));
        self
    }

    /// Use shared precomputed diff data.
    pub fn with_shared_diff(mut self, data: Arc<DiffData>) -> Self {
        self.patch = None;
        self.diff_data = Some(data);
        self
    }

    fn resolved_width(&self) -> Length {
        self.width_override.unwrap_or(match self.backend {
            DiffViewBackend::TextArea => self.text_area.width,
            DiffViewBackend::DocumentView => self.document_view.width,
        })
    }

    fn effective_wrap(&self) -> bool {
        self.wrap_override.unwrap_or(match self.backend {
            DiffViewBackend::TextArea => self.text_area.wrap,
            DiffViewBackend::DocumentView => self.document_view.wrap,
        })
    }

    fn effective_scrollbar(&self) -> bool {
        self.scrollbar_override.unwrap_or(match self.backend {
            DiffViewBackend::TextArea => self.text_area.scrollbar,
            DiffViewBackend::DocumentView => self.document_view.scrollbar,
        })
    }

    fn backend_height(&self) -> Length {
        match self.backend {
            DiffViewBackend::TextArea => self.text_area.height,
            DiffViewBackend::DocumentView => self.document_view.height,
        }
    }

    fn should_use_implicit_auto_height(&self) -> bool {
        self.height_override.is_none()
            && self.effective_wrap()
            && !self.effective_scrollbar()
            && matches!(self.backend_height(), Length::Flex(1))
    }

    fn resolved_height(&self) -> Length {
        self.height_override.unwrap_or_else(|| {
            if self.should_use_implicit_auto_height() {
                Length::Auto
            } else {
                self.backend_height()
            }
        })
    }
}

impl Default for DiffView {
    fn default() -> Self {
        Self::new()
    }
}

fn separator_click_config(
    render: &DiffRender,
    pane: DiffPane,
    on_click: Option<Callback<DiffContextSeparatorEvent>>,
    hover_style: Option<Style>,
    expand_lines: usize,
) -> Option<DiffContextSeparatorClickConfig> {
    let has_interaction = on_click.is_some() || hover_style.is_some_and(|style| !style.is_empty());
    if !has_interaction {
        return None;
    }
    let events = render
        .lines
        .iter()
        .map(|line| {
            line.context_separator
                .as_ref()
                .map(|separator| separator.event(pane, expand_lines))
        })
        .collect::<Vec<_>>();
    events
        .iter()
        .any(Option::is_some)
        .then(|| DiffContextSeparatorClickConfig {
            events_by_source_line: events.into(),
            on_click,
            hover_style,
        })
}

impl From<DiffView> for Element {
    fn from(view: DiffView) -> Self {
        let outer_width = view.resolved_width();
        let outer_height = view.resolved_height();
        let use_auto_pane_height = matches!(outer_height, Length::Auto);
        let config = DiffDataConfig {
            prefixes: view.prefixes.clone(),
            show_prefixes: view.show_prefixes,
            word_diff: view.word_diff,
            context_lines: None,
            ..DiffDataConfig::default()
        };
        let diff_data = if let Some(data) = view.diff_data.clone() {
            data
        } else if let Some(patch) = view.patch.as_ref() {
            cached_patch_diff_data(Arc::clone(patch), config)
        } else {
            cached_diff_data(&view.before, &view.after, config)
        };
        let (left_render, right_render, unified_render) = if view.trim_common_indent {
            match view.mode {
                DiffViewMode::Split => {
                    let trim = common_indent_across_lines(
                        diff_data
                            .left
                            .lines
                            .iter()
                            .chain(diff_data.right.lines.iter()),
                    );
                    (
                        trim_render_common_indent(&diff_data.left, trim),
                        trim_render_common_indent(&diff_data.right, trim),
                        trim_render_common_indent(
                            &diff_data.unified,
                            common_indent_across_lines(diff_data.unified.lines.iter()),
                        ),
                    )
                }
                DiffViewMode::Unified => {
                    let trim = common_indent_across_lines(diff_data.unified.lines.iter());
                    (
                        trim_render_common_indent(&diff_data.left, trim),
                        trim_render_common_indent(&diff_data.right, trim),
                        trim_render_common_indent(&diff_data.unified, trim),
                    )
                }
            }
        } else {
            (
                diff_data.left.clone(),
                diff_data.right.clone(),
                diff_data.unified.clone(),
            )
        };

        let (left_render, right_render, unified_render) =
            apply_runtime_context_collapse_to_diff_renders(
                left_render,
                right_render,
                unified_render,
                view.context_lines,
                render::context_collapse_options(
                    view.show_context_separator,
                    view.context_separator_text.as_ref(),
                    view.context_separator_min_lines,
                    &view.expanded_contexts,
                ),
            );

        let base_strategy = view
            .base_color_strategy
            .clone()
            .or_else(|| view.text_area.color_strategy.clone());

        let line_numbers_enabled = view.line_numbers_override.unwrap_or(match view.backend {
            DiffViewBackend::TextArea => view.text_area.line_numbers,
            DiffViewBackend::DocumentView => view.document_view.line_numbers,
        });
        let min_line_digits = view
            .min_line_number_width_override
            .map(usize::from)
            .unwrap_or(match view.backend {
                DiffViewBackend::TextArea => view.text_area.min_line_number_width as usize,
                DiffViewBackend::DocumentView => view.document_view.min_line_number_width as usize,
            });

        let language = view.language.clone();
        let theme = view.theme.clone();
        let split_wrap_sync = matches!(view.mode, DiffViewMode::Split) && view.effective_wrap();
        let split_wrap_state = split_wrap_sync.then(new_split_wrap_sync_state);
        if let Some(ref sync) = split_wrap_state {
            wrap_sync::set_split_wrap_scrollbar_cols(
                sync,
                split_pane_standalone_scrollbar_cols(&view, DiffPane::Left),
                split_pane_standalone_scrollbar_cols(&view, DiffPane::Right),
            );
        }
        let split_wrap_padding_gutter_style = split_wrap_sync.then_some(
            view.diff_style
                .empty
                .patch(view.diff_style.context_line_number),
        );
        let split_wrap_padding_style = split_wrap_sync.then_some(view.diff_style.empty);
        let left_peer_source_lines = if split_wrap_sync {
            Some(Arc::new(
                right_render
                    .lines
                    .iter()
                    .map(|line| Arc::clone(&line.text))
                    .collect::<Vec<_>>(),
            ))
        } else {
            None
        };
        let right_peer_source_lines = if split_wrap_sync {
            Some(Arc::new(
                left_render
                    .lines
                    .iter()
                    .map(|line| Arc::clone(&line.text))
                    .collect::<Vec<_>>(),
            ))
        } else {
            None
        };

        // Pre-compute per-pane shared selection ids.  Unified uses the id
        // as-is; split suffixes `:left` / `:right` so only same-side panels
        // share selection across multiple DiffViews.
        let pane_shared_selection_id = |pane: DiffPane| -> Option<Arc<str>> {
            let base = view.shared_selection_id.as_ref()?;
            Some(match (view.mode, pane) {
                (DiffViewMode::Unified, _) | (_, DiffPane::Unified) => Arc::clone(base),
                (DiffViewMode::Split, DiffPane::Left) => Arc::from(format!("{}:left", base)),
                (DiffViewMode::Split, DiffPane::Right) => Arc::from(format!("{}:right", base)),
            })
        };

        let gutter_style_hash = diff_style_hash(&view.diff_style);
        let pane_options = PaneRenderOptions {
            mode: view.mode,
            line_numbers: line_numbers_enabled,
            min_digits: min_line_digits,
            style: view.diff_style,
            gutter_style_hash,
        };

        let build_text_area = |render: &DiffRender, pane: DiffPane| {
            let pane_data = get_pane_data(&view.pane_cache, render, pane, pane_options);
            let render = pane_data.numbered_render;
            let gutter_spans = pane_data.gutter_spans;
            let gutter_col_width = pane_data.gutter_col_width;
            let excluded_source_lines = pane_data.excluded_source_lines;
            let excluded_bytes = pane_data.excluded_bytes;
            let diff_strategy = DiffColorStrategy::new(
                render.clone(),
                base_strategy.clone(),
                view.diff_style,
                view.highlight_full_width,
                false,
            );
            let separator_click = separator_click_config(
                &render,
                pane,
                view.on_context_separator_click.clone(),
                view.context_separator_hover_style,
                view.context_expand_lines,
            );
            let scroll_to_hunk_line = view
                .scroll_to_hunk
                .and_then(|hunk_index| hunk_logical_line(&render, hunk_index));
            let mut area = view
                .text_area
                .clone()
                .value(render.raw_text.clone())
                .read_only(!view.editable)
                .color_strategy(diff_strategy)
                .gutter_lines(gutter_spans, gutter_col_width)
                .copy_excluded_bytes(excluded_bytes)
                .selection_excluded_lines(excluded_source_lines);

            area = area.line_numbers(false);

            area.peer_source_lines = match pane {
                DiffPane::Left => left_peer_source_lines.clone(),
                DiffPane::Right => right_peer_source_lines.clone(),
                DiffPane::Unified => None,
            };
            area.split_wrap_sync = split_wrap_state.clone();
            area.split_wrap_side = match pane {
                DiffPane::Left => Some(SplitPaneSide::Left),
                DiffPane::Right => Some(SplitPaneSide::Right),
                DiffPane::Unified => None,
            };
            area.split_wrap_padding_gutter_style = split_wrap_padding_gutter_style;
            area.split_wrap_padding_style = split_wrap_padding_style;
            area.diff_context_separator_click = separator_click;

            if let Some(v) = view.wrap_override {
                area = area.wrap(v);
            }
            if let Some(v) = view.scrollbar_override {
                area = area.scrollbar(v);
            }
            if let Some(v) = view.h_scrollbar_override {
                area = area.h_scrollbar(v);
            }
            if let Some(v) = view.focusable_override {
                area = area.focusable(v);
            }

            if view.single_scrollbar && matches!(view.mode, DiffViewMode::Split) {
                let is_right = matches!(pane, DiffPane::Right);
                area = area.scrollbar(is_right);
                area.pin_scrollbar_focus_style = is_right;
            }

            // Border is handled by pane wrapper frames.
            area = area.border(false);

            if use_auto_pane_height {
                area = area.height(Length::Auto);
            }

            if let Some(offset) = view.scroll_offset {
                area = area.scroll_offset(offset);
            }
            if let Some(line) = scroll_to_hunk_line {
                area = area.scroll_to_line(line);
            }

            let existing_on_scroll = area.on_scroll.clone();
            let view_on_scroll = view.on_scroll.clone();
            if existing_on_scroll.is_some() || view_on_scroll.is_some() {
                area = area.on_scroll(Callback::new(move |event: ScrollEvent| {
                    if let Some(cb) = &existing_on_scroll {
                        cb.emit(event);
                    }
                    if let Some(cb) = &view_on_scroll {
                        cb.emit(DiffScrollEvent {
                            pane,
                            scroll: event,
                        });
                    }
                }));
            }

            area.into()
        };

        let build_document_view = |render: &DiffRender, pane: DiffPane| {
            let pane_data = get_pane_data(&view.pane_cache, render, pane, pane_options);
            let render = pane_data.numbered_render;
            let gutter_spans = pane_data.gutter_spans;
            let gutter_col_width = pane_data.gutter_col_width;
            let excluded_source_lines = pane_data.excluded_source_lines;
            let formatter = DiffDocumentFormatter::new(
                render.clone(),
                base_strategy.clone(),
                view.diff_style,
                view.highlight_full_width,
                false,
                language.clone(),
                theme.clone(),
            );
            let separator_click = separator_click_config(
                &render,
                pane,
                view.on_context_separator_click.clone(),
                view.context_separator_hover_style,
                view.context_expand_lines,
            );
            let scroll_to_hunk_line = view
                .scroll_to_hunk
                .and_then(|hunk_index| hunk_logical_line(&render, hunk_index));
            let mut doc = view.document_view.clone();
            doc.value = render.raw_text.clone();
            doc.content_type = Some("diff".into());
            doc.formatter = Some(Rc::new(formatter));
            doc.highlight_full_width = view.highlight_full_width;
            doc.gutter_lines = Some(gutter_spans);
            doc.gutter_col_width = gutter_col_width;
            doc.copy_excluded_source_lines = Some(excluded_source_lines);
            doc.shared_selection_id = pane_shared_selection_id(pane);
            doc.peer_source_lines = match pane {
                DiffPane::Left => left_peer_source_lines.clone(),
                DiffPane::Right => right_peer_source_lines.clone(),
                DiffPane::Unified => None,
            };
            doc.split_wrap_sync = split_wrap_state.clone();
            doc.split_wrap_side = match pane {
                DiffPane::Left => Some(SplitPaneSide::Left),
                DiffPane::Right => Some(SplitPaneSide::Right),
                DiffPane::Unified => None,
            };
            doc.diff_split_pane = if matches!(view.mode, DiffViewMode::Split) {
                Some(pane)
            } else {
                None
            };
            doc.split_wrap_padding_gutter_style = split_wrap_padding_gutter_style;
            doc.split_wrap_padding_style = split_wrap_padding_style;
            doc.diff_context_separator_click = separator_click;

            doc = doc.line_numbers(false);

            if let Some(v) = view.wrap_override {
                doc = doc.wrap(v);
            }
            if let Some(v) = view.scrollbar_override {
                doc = doc.scrollbar(v);
            }
            if let Some(v) = view.h_scrollbar_override {
                doc = doc.h_scrollbar(v);
            }
            if let Some(v) = view.focusable_override {
                doc = doc.focusable(v);
            }

            if view.single_scrollbar && matches!(view.mode, DiffViewMode::Split) {
                let is_right = matches!(pane, DiffPane::Right);
                doc = doc.scrollbar(is_right);
                doc.pin_scrollbar_focus_style = is_right;
            }

            // Border is handled by pane wrapper frames.
            doc = doc.border(false);

            if use_auto_pane_height {
                doc.height = Length::Auto;
            }

            if let Some(offset) = view.scroll_offset {
                doc.scroll_offset = Some(offset);
            }
            if let Some(line) = scroll_to_hunk_line {
                doc = doc.scroll_to_source_line(line);
            }

            let existing_on_scroll = doc.on_scroll.clone();
            let view_on_scroll = view.on_scroll.clone();
            if existing_on_scroll.is_some() || view_on_scroll.is_some() {
                doc.on_scroll = Some(Callback::new(move |event: ScrollEvent| {
                    if let Some(cb) = &existing_on_scroll {
                        cb.emit(event);
                    }
                    if let Some(cb) = &view_on_scroll {
                        cb.emit(DiffScrollEvent {
                            pane,
                            scroll: event,
                        });
                    }
                }));
            }

            doc.into()
        };

        let build_pane = |render: &DiffRender, pane: DiffPane, is_left: bool| -> Element {
            let inner: Element = match view.backend {
                DiffViewBackend::TextArea => build_text_area(render, pane),
                DiffViewBackend::DocumentView => build_document_view(render, pane),
            };

            let right_pad = if is_left && view.single_scrollbar && view.effective_scrollbar() {
                1
            } else {
                0
            };

            let mut pane = Frame::new()
                .border(view.pane_border)
                .join_frame(view.join_frame)
                .padding((0, right_pad, 0, 0))
                .child(inner);

            if use_auto_pane_height {
                pane = pane.height(Length::Auto);
            }

            pane.into()
        };

        let content = match view.mode {
            DiffViewMode::Split => {
                let left = build_pane(&left_render, DiffPane::Left, true);
                let right = build_pane(&right_render, DiffPane::Right, false);
                if view.vertical_separator {
                    HStack::new()
                        .even_flex(true)
                        .child(left)
                        .child(
                            Divider::vertical()
                                .ch(view.vertical_separator_char)
                                .style(view.vertical_separator_style)
                                .join_frame(view.join_frame),
                        )
                        .child(right)
                        .into()
                } else {
                    HStack::new()
                        .even_flex(true)
                        .child(left)
                        .child(right)
                        .into()
                }
            }
            DiffViewMode::Unified => build_pane(&unified_render, DiffPane::Unified, false),
        };

        Frame::new()
            .border(view.outer_border)
            .width(outer_width)
            .height(outer_height)
            .padding(0)
            .child(content)
            .into()
    }
}

mod gutter;
pub(crate) use gutter::*;

#[cfg(test)]
mod tests;
