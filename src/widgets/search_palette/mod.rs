pub(crate) mod component;
mod matching;
mod render;

use std::sync::Arc;

use nucleo::pattern::{CaseMatching, Normalization};

use crate::callback::{Callback, KeyHandler};
use crate::core::element::{Element, Key};
use crate::style::{
    BorderStyle, CaretShape, Color, Length, Padding, ScrollbarConfig, Style, StyleSlot,
};
use crate::utils::gradient::{ColorGradient, GradientRange};
use crate::widgets::{ListConfig, ListItem, ListItemGutter, ListItemStatus};

pub(crate) const DEFAULT_SYNC_MATCH_LIMIT: usize = 100;

/// Structured description for a [`SearchItem`].
///
/// Supports a left segment (searchable, placed according to [`DescriptionPlacement`])
/// and a right segment (not searchable, always right-aligned). Both are styled with
/// `description_style`.
///
/// Plain strings convert to a left-only description via [`From`].
///
/// # Examples
///
/// ```
/// # use tui_lipan::prelude::ItemDescription;
/// // Left only (equivalent to a plain string):
/// let d = ItemDescription::new().left("Code editor");
///
/// // Right badge only:
/// let d = ItemDescription::new().right("Pro");
///
/// // Both:
/// let d = ItemDescription::new().left("Code editor").right("Free");
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ItemDescription {
    /// Left-aligned description text (searchable, respects [`DescriptionPlacement`]).
    pub left: Option<Arc<str>>,
    /// Right-aligned text (not searchable, always shown on the right).
    pub right: Option<Arc<str>>,
}

impl ItemDescription {
    /// Create an empty description.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the left (main) description text.
    pub fn left(mut self, text: impl Into<Arc<str>>) -> Self {
        self.left = Some(text.into());
        self
    }

    /// Set the right-aligned text (e.g. `"Free"`, `"Pro"`).
    pub fn right(mut self, text: impl Into<Arc<str>>) -> Self {
        self.right = Some(text.into());
        self
    }
}

impl From<&str> for ItemDescription {
    fn from(s: &str) -> Self {
        Self {
            left: Some(s.into()),
            right: None,
        }
    }
}

impl From<String> for ItemDescription {
    fn from(s: String) -> Self {
        Self {
            left: Some(s.into()),
            right: None,
        }
    }
}

impl From<Arc<str>> for ItemDescription {
    fn from(s: Arc<str>) -> Self {
        Self {
            left: Some(s),
            right: None,
        }
    }
}

/// A searchable item.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchItem<T> {
    /// Item label (text to search).
    pub label: Arc<str>,
    /// Optional structured description.
    pub description: Option<ItemDescription>,
    /// Alternative names matched alongside the label. Each alias is scored
    /// independently and the best alias score competes with the label score
    /// (the match takes the maximum), so a row can rank well via either its
    /// canonical label or any alias. Aliases are not displayed.
    pub aliases: Vec<Arc<str>>,
    /// Whether the row should render in the list active state.
    pub active: bool,
    /// User data.
    pub value: T,
}

impl<T> SearchItem<T> {
    /// Create a new search item.
    pub fn new(label: impl Into<Arc<str>>, value: T) -> Self {
        Self {
            label: label.into(),
            description: None,
            aliases: Vec::new(),
            active: false,
            value,
        }
    }

    /// Set description. Accepts a plain string or an [`ItemDescription`].
    pub fn description(mut self, description: impl Into<ItemDescription>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Replace the aliases list.
    pub fn aliases<I, S>(mut self, aliases: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Arc<str>>,
    {
        self.aliases = aliases.into_iter().map(Into::into).collect();
        self
    }

    /// Append a single alias.
    pub fn alias(mut self, alias: impl Into<Arc<str>>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    /// Mark this item as active.
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }
}

/// Returns indices into `items` in [`SearchPalette`] fuzzy-match order (best match first).
///
/// The query is trimmed; an empty query yields `0..items.len()` in definition order.
/// Matching uses the same nucleo settings as the widget (smart case matching and normalization).
pub fn rank_search_palette_indices<T: Clone + PartialEq>(
    items: &[SearchItem<T>],
    query: &str,
) -> Vec<usize> {
    rank_search_palette_indices_with_score(items, query, |_, _, score| score as f64)
}

/// Returns indices into `items` in [`SearchPalette`] fuzzy-match order after score adjustment.
///
/// The query is trimmed; an empty query yields `0..items.len()` in definition order and does not
/// call `score_fn`. For non-empty queries, matching uses the same nucleo settings as the widget
/// (smart case matching and normalization), then calls `score_fn` with the source item index, item,
/// and raw fuzzy score. Results are ordered by descending adjusted score, with the source item index
/// as the tie-breaker. `NaN` adjusted scores rank after finite scores, with the source item index
/// as the tie-breaker between `NaN` results.
///
/// Uses [`SearchMatchMode::Fuzzy`]; call [`rank_search_palette_indices_with_mode`] to pick a
/// different strategy such as [`SearchMatchMode::Hybrid`].
pub fn rank_search_palette_indices_with_score<T: Clone + PartialEq, F>(
    items: &[SearchItem<T>],
    query: &str,
    score_fn: F,
) -> Vec<usize>
where
    F: FnMut(usize, &SearchItem<T>, u32) -> f64,
{
    rank_search_palette_indices_with_mode(items, query, SearchMatchMode::Fuzzy, score_fn)
}

/// Returns indices into `items` in [`SearchPalette`] match order for a given
/// [`SearchMatchMode`], after score adjustment.
///
/// Identical to [`rank_search_palette_indices_with_score`] but lets the caller pick the matching
/// strategy (for example [`SearchMatchMode::Hybrid`] to rank exact/prefix/substring matches above
/// fuzzy ones and reject weak scattered fuzzy hits). The query is trimmed; an empty query yields
/// `0..items.len()` in definition order and does not call `score_fn`. The `score` passed to
/// `score_fn` is the mode's raw match score (nucleo's fuzzy score under `Fuzzy`, the composite
/// tiered score under `Hybrid`), so multiplicative adjustments such as frecency boosts compose the
/// same way in either mode. Results are ordered by descending adjusted score, with the source item
/// index as the tie-breaker; `NaN` adjusted scores rank after finite scores.
pub fn rank_search_palette_indices_with_mode<T: Clone + PartialEq, F>(
    items: &[SearchItem<T>],
    query: &str,
    match_mode: SearchMatchMode,
    mut score_fn: F,
) -> Vec<usize>
where
    F: FnMut(usize, &SearchItem<T>, u32) -> f64,
{
    let query = query.trim();
    if query.is_empty() {
        return (0..items.len()).collect();
    }
    let entries = matching::build_search_entries(items);
    let mut results: Vec<_> = matching::match_items(
        &entries,
        query,
        match_mode,
        CaseMatching::Smart,
        Normalization::Smart,
    )
    .into_iter()
    .map(|result| {
        let item_index = result.item_index;
        let adjusted_score = score_fn(item_index, &items[item_index], result.score);
        (item_index, adjusted_score)
    })
    .collect();

    results.sort_by(|(a_index, a_score), (b_index, b_score)| {
        match (a_score.is_nan(), b_score.is_nan()) {
            (true, true) => a_index.cmp(b_index),
            (true, false) => std::cmp::Ordering::Greater,
            (false, true) => std::cmp::Ordering::Less,
            (false, false) => b_score
                .partial_cmp(a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a_index.cmp(b_index)),
        }
    });

    results
        .into_iter()
        .map(|(item_index, _)| item_index)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{
        SearchItem, SearchMatchMode, rank_search_palette_indices,
        rank_search_palette_indices_with_mode, rank_search_palette_indices_with_score,
    };

    #[test]
    fn rank_search_palette_indices_uses_identity_scoring() {
        let items = vec![
            SearchItem::new("alpha", 0),
            SearchItem::new("alpine", 1),
            SearchItem::new("beta", 2),
        ];

        let ranked = rank_search_palette_indices(&items, "alp");
        let ranked_with_identity =
            rank_search_palette_indices_with_score(&items, "alp", |_, _, score| score as f64);

        assert_eq!(ranked_with_identity, ranked);
    }

    #[test]
    fn items_arc_and_entries_arc_preserve_shared_slices() {
        use super::{SearchEntry, SearchPalette};
        use std::sync::Arc;

        let items: Arc<[SearchItem<usize>]> =
            Arc::from([SearchItem::new("alpha", 0), SearchItem::new("beta", 1)]);
        let palette = SearchPalette::new().items_arc(Arc::clone(&items));
        assert!(Arc::ptr_eq(&palette.props.items, &items));
        assert!(palette.props.entries.is_empty());

        let entries: Arc<[SearchEntry<usize>]> =
            Arc::from([SearchEntry::header("Group"), SearchEntry::item("gamma", 2)]);
        let palette = SearchPalette::new().entries_arc(Arc::clone(&entries));
        assert!(Arc::ptr_eq(&palette.props.entries, &entries));
        assert_eq!(palette.props.items.len(), 1);
        assert_eq!(palette.props.items[0].label.as_ref(), "gamma");
    }

    #[test]
    fn rank_search_palette_indices_with_score_can_reorder_matches() {
        let items = vec![
            SearchItem::new("alpha", 0),
            SearchItem::new("alpine", 1),
            SearchItem::new("beta", 2),
        ];

        let ranked = rank_search_palette_indices_with_score(&items, "alp", |index, _, score| {
            if index == 1 {
                score as f64 + 1_000_000.0
            } else {
                score as f64
            }
        });

        assert_eq!(ranked, vec![1, 0]);
    }

    #[test]
    fn rank_search_palette_indices_with_score_does_not_score_empty_query() {
        let items = vec![SearchItem::new("alpha", 0), SearchItem::new("alpine", 1)];

        let ranked = rank_search_palette_indices_with_score(&items, "  ", |_, _, _| {
            panic!("empty queries must not call custom scoring")
        });

        assert_eq!(ranked, vec![0, 1]);
    }

    #[test]
    fn rank_search_palette_indices_with_score_places_nan_after_finite_scores() {
        let items = vec![
            SearchItem::new("alpha", 0),
            SearchItem::new("alpine", 1),
            SearchItem::new("atlas", 2),
        ];

        let ranked = rank_search_palette_indices_with_score(&items, "a", |index, _, _| {
            if index == 1 { 1.0 } else { f64::NAN }
        });

        assert_eq!(ranked, vec![1, 0, 2]);
    }

    #[test]
    fn rank_search_palette_indices_with_mode_applies_hybrid_gating() {
        let items = vec![
            SearchItem::new("Enable pane synchronization", 0),
            SearchItem::new("Layout", 1),
        ];

        // Hybrid rejects the weak scattered fuzzy match and keeps only the prefix match.
        let hybrid = rank_search_palette_indices_with_mode(
            &items,
            "layo",
            SearchMatchMode::Hybrid,
            |_, _, score| score as f64,
        );
        assert_eq!(hybrid, vec![1]);

        // Fuzzy (the default) still surfaces the scattered match.
        let fuzzy =
            rank_search_palette_indices_with_score(&items, "layo", |_, _, score| score as f64);
        assert!(fuzzy.contains(&0));
    }
}

/// A search event emitted when an item is selected/activated.
#[derive(Clone, Debug, PartialEq)]
pub struct SearchEvent<T> {
    /// Index in the matched list.
    pub match_index: usize,
    /// Index in the source item list.
    pub item_index: usize,
    /// The matched item.
    pub item: SearchItem<T>,
}

/// Match metadata passed to the custom renderer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchHighlight {
    /// Matching character indices in the label.
    pub label_hits: Vec<u32>,
    /// Matching character indices in [`ItemDescription::left`].
    pub description_hits: Vec<u32>,
    /// Matching character indices in [`ItemDescription::right`].
    pub description_right_hits: Vec<u32>,
    /// Score reported by the matcher.
    pub score: u32,
}

/// Custom item renderer. Return [`Some`] to replace the default render, [`None`] to fall through.
type SearchRenderer<T> = Arc<dyn Fn(&SearchItem<T>, &SearchHighlight) -> Option<ListItem>>;

/// Custom item gutter renderer. Return [`Some`] to attach a left gutter to the row.
type SearchGutterRenderer<T> =
    Arc<dyn Fn(&SearchItem<T>, &SearchHighlight) -> Option<ListItemGutter>>;

/// Custom item status renderer. Return [`Some`] to attach content in the list symbol column.
type SearchStatusRenderer<T> =
    Arc<dyn Fn(&SearchItem<T>, &SearchHighlight) -> Option<ListItemStatus>>;

/// Placement for item descriptions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DescriptionPlacement {
    /// Render as `label - description` on the primary line.
    #[default]
    Inline,
    /// Render in the right-aligned slot on the primary line.
    Right,
    /// Render as a line above the label.
    Above,
    /// Render as a line below the label.
    Below,
}

/// Overflow policy for description text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DescriptionOverflow {
    /// Keep descriptions on one visual line and truncate with ellipsis.
    #[default]
    Truncate,
    /// Wrap descriptions onto additional lines for above/below placement.
    /// Wrapping prefers word boundaries.
    Wrap,
}

/// Matching strategy used to rank [`SearchPalette`] results.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SearchMatchMode {
    /// Plain `nucleo` fuzzy matching (default). The label competes with
    /// aliases via `max()`, and description text adds to the score.
    #[default]
    Fuzzy,
    /// Evaluate exact, prefix, word-prefix, substring, and fuzzy matching
    /// together per field (label, aliases, description, right-hint) and rank
    /// by match quality in that priority order, so a real substring or
    /// prefix match always outranks a fuzzy one, and weak scattered fuzzy
    /// matches are rejected instead of polluting results.
    /// Contiguous queries may omit separators within a field, so
    /// `switchmodel` matches `Switch model`.
    ///
    /// Each whitespace-separated query term may match a different field, but
    /// characters within one term never combine across the label, an alias,
    /// the description, and the right-hand hint. Labels and aliases carry the
    /// highest weight, descriptions a lower weight, and the right-hand hint
    /// only matches via exact or substring comparison (no fuzzy/prefix
    /// matching), which suits keybinding-style hints.
    Hybrid,
}

/// An entry in the search palette item list.
///
/// Use [`SearchEntry::item`] for searchable rows, [`SearchEntry::header`] for
/// section labels, and [`SearchEntry::spacer`] for blank separator rows.
/// Headers and spacers are excluded from fuzzy matching but are shown in the
/// list when results are displayed.
#[derive(Clone, Debug, PartialEq)]
pub enum SearchEntry<T> {
    /// A searchable, selectable item.
    Item(SearchItem<T>),
    /// A non-selectable section header.
    Header(Arc<str>),
    /// A non-selectable blank spacer row.
    Spacer,
}

impl<T> SearchEntry<T> {
    /// Create a searchable item entry.
    pub fn item(label: impl Into<Arc<str>>, value: T) -> Self {
        Self::Item(SearchItem::new(label, value))
    }

    /// Create a section header entry.
    pub fn header(label: impl Into<Arc<str>>) -> Self {
        Self::Header(label.into())
    }

    /// Create a blank spacer entry.
    pub fn spacer() -> Self {
        Self::Spacer
    }

    /// Set description on an item entry. Accepts a plain string or [`ItemDescription`].
    /// No-op if called on a header or spacer.
    pub fn description(self, description: impl Into<ItemDescription>) -> Self {
        match self {
            Self::Item(item) => Self::Item(item.description(description)),
            other => other,
        }
    }

    /// Mark an item entry as active. No-op for header and spacer entries.
    pub fn active(self, active: bool) -> Self {
        match self {
            Self::Item(item) => Self::Item(item.active(active)),
            other => other,
        }
    }
}

#[derive(Clone)]
#[allow(missing_docs)]
pub(crate) struct SearchPaletteProps<T> {
    items: Arc<[SearchItem<T>]>,
    entries: Arc<[SearchEntry<T>]>,
    sync_match_limit: usize,
    sync_selection: bool,
    initial_query: Arc<str>,
    /// Index into [`SearchPaletteProps::items`]. When this item appears in the
    /// current result list, keyboard selection starts on that row; otherwise the
    /// first result is selected.
    initial_selected_item_index: Option<usize>,
    /// Controlled mode: when `Some`, the query is driven by the caller, not by
    /// an internal `TextInput`. The `Input` widget is not rendered.
    query: Option<Arc<str>>,
    placeholder: Arc<str>,
    // Layout
    width: Length,
    height: Length,
    max_width: Option<Length>,
    max_height: Option<Length>,
    // Input forwarding props
    input_prefix: Option<Arc<str>>,
    input_suffix: Option<Arc<str>>,
    input_border: bool,
    input_divider: bool,
    input_divider_style: Style,
    input_divider_join_frame: bool,
    input_caret_shape: CaretShape,
    input_caret_color: Option<Color>,
    input_border_style: BorderStyle,
    input_padding: Padding,
    input_style: Style,
    input_hover_style: StyleSlot,
    input_focus_style: StyleSlot,
    input_focus_content_style: Style,
    input_placeholder_style: Style,
    input_focus_placeholder_style: Style,
    input_prefix_style: Style,
    input_focus_prefix_style: Style,
    input_suffix_style: Style,
    input_focus_suffix_style: Style,
    // List forwarding props
    list_config: ListConfig,
    list_symbol_column: Option<bool>,
    list_hover_style: StyleSlot,
    list_active_style: StyleSlot,
    list_active_symbol: Option<Arc<str>>,
    list_active_symbol_style: Option<Style>,
    list_unselected_symbol: Option<Arc<str>>,
    list_focusable: bool,
    input_key: Option<Key>,
    tab_stop: bool,
    on_focus: Option<Callback<()>>,
    on_blur: Option<Callback<()>>,
    empty_text: Option<Arc<str>>,
    // Item rendering props
    item_style: Style,
    active_item_style: Option<Style>,
    header_style: Style,
    description_style: Style,
    active_description_style: Option<Style>,
    focused_description_style: Option<Style>,
    description_placement: DescriptionPlacement,
    description_separator: Option<Arc<str>>,
    description_selection: bool,
    description_overflow: DescriptionOverflow,
    match_style: Style,
    show_scores: bool,
    score_gradient: Option<ColorGradient>,
    score_range: Option<GradientRange>,
    /// When `true`, entries (headers/spacers) remain visible during active
    /// search instead of being hidden. Matched items stay grouped under their
    /// original headers; empty groups are suppressed.  Navigation follows
    /// visual (definition) order rather than score order so that arrow keys
    /// move sequentially through the visible rows.
    preserve_groups: bool,
    navigation_wrap: bool,
    // Matching config
    match_mode: SearchMatchMode,
    case_matching: CaseMatching,
    normalization: Normalization,
    // Input key interceptor
    input_key_interceptor: Option<KeyHandler>,
    // Callbacks
    on_query_change: Option<Callback<Arc<str>>>,
    on_select: Option<Callback<SearchEvent<T>>>,
    on_activate: Option<Callback<SearchEvent<T>>>,
    render_item: Option<SearchRenderer<T>>,
    item_status: Option<SearchStatusRenderer<T>>,
    item_gutter: Option<SearchGutterRenderer<T>>,
}

impl<T: PartialEq> PartialEq for SearchPaletteProps<T> {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
            && self.entries == other.entries
            && self.sync_match_limit == other.sync_match_limit
            && self.sync_selection == other.sync_selection
            && self.initial_query == other.initial_query
            && self.initial_selected_item_index == other.initial_selected_item_index
            && self.query == other.query
            && self.placeholder == other.placeholder
            && self.width == other.width
            && self.height == other.height
            && self.max_width == other.max_width
            && self.max_height == other.max_height
            && self.input_prefix == other.input_prefix
            && self.input_suffix == other.input_suffix
            && self.input_border == other.input_border
            && self.input_divider == other.input_divider
            && self.input_divider_style == other.input_divider_style
            && self.input_divider_join_frame == other.input_divider_join_frame
            && self.input_caret_shape == other.input_caret_shape
            && self.input_caret_color == other.input_caret_color
            && self.input_border_style == other.input_border_style
            && self.input_padding == other.input_padding
            && self.input_style == other.input_style
            && self.input_hover_style == other.input_hover_style
            && self.input_focus_style == other.input_focus_style
            && self.input_placeholder_style == other.input_placeholder_style
            && self.input_focus_placeholder_style == other.input_focus_placeholder_style
            && self.input_prefix_style == other.input_prefix_style
            && self.input_focus_prefix_style == other.input_focus_prefix_style
            && self.input_suffix_style == other.input_suffix_style
            && self.input_focus_suffix_style == other.input_focus_suffix_style
            && self.list_config == other.list_config
            && self.list_symbol_column == other.list_symbol_column
            && self.list_hover_style == other.list_hover_style
            && self.list_active_style == other.list_active_style
            && self.list_active_symbol == other.list_active_symbol
            && self.list_active_symbol_style == other.list_active_symbol_style
            && self.list_unselected_symbol == other.list_unselected_symbol
            && self.list_focusable == other.list_focusable
            && self.input_key == other.input_key
            && self.tab_stop == other.tab_stop
            && self.on_focus == other.on_focus
            && self.on_blur == other.on_blur
            && self.empty_text == other.empty_text
            && self.item_style == other.item_style
            && self.active_item_style == other.active_item_style
            && self.header_style == other.header_style
            && self.description_style == other.description_style
            && self.active_description_style == other.active_description_style
            && self.focused_description_style == other.focused_description_style
            && self.description_placement == other.description_placement
            && self.description_separator == other.description_separator
            && self.description_selection == other.description_selection
            && self.description_overflow == other.description_overflow
            && self.preserve_groups == other.preserve_groups
            && self.navigation_wrap == other.navigation_wrap
            && self.match_style == other.match_style
            && self.show_scores == other.show_scores
            && self.score_gradient == other.score_gradient
            && self.score_range == other.score_range
            && self.match_mode == other.match_mode
            && self.case_matching == other.case_matching
            && self.normalization == other.normalization
            && self.input_key_interceptor.is_some() == other.input_key_interceptor.is_some()
            && self.on_query_change == other.on_query_change
            && self.on_select == other.on_select
            && self.on_activate == other.on_activate
            && render_item_eq(&self.render_item, &other.render_item)
            && render_status_eq(&self.item_status, &other.item_status)
            && render_gutter_eq(&self.item_gutter, &other.item_gutter)
    }
}

fn render_item_eq<T>(left: &Option<SearchRenderer<T>>, right: &Option<SearchRenderer<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn render_gutter_eq<T>(
    left: &Option<SearchGutterRenderer<T>>,
    right: &Option<SearchGutterRenderer<T>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

fn render_status_eq<T>(
    left: &Option<SearchStatusRenderer<T>>,
    right: &Option<SearchStatusRenderer<T>>,
) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

/// A fuzzy search palette powered by `nucleo`.
///
/// # Modes
///
/// `SearchPalette` supports two query ownership modes:
///
/// ## Uncontrolled (default)
///
/// The palette renders its own `Input` widget and owns the query state
/// (including undo/redo history). This is the fast path for the common
/// "modal search / command palette" use case.
///
/// ```no_run
/// # use tui_lipan::prelude::*;
/// # use std::sync::Arc;
/// SearchPalette::<Arc<str>>::new()
///     .initial_query("src/")
///     .on_activate(Callback::new(|ev: SearchEvent<Arc<str>>| {
///         // handle activation
///     }));
/// ```
///
/// ## Controlled
///
/// When [`query`](Self::query) is set the palette operates in controlled mode:
///
/// - **No `Input` widget is rendered** - the caller is responsible for
///   displaying and updating the query string (e.g. inside a `Frame` divider).
/// - **No `TextInput` or undo history is allocated** - only a single
///   `Arc<str>` is stored.
/// - Changes to the `query` prop are picked up automatically via
///   `on_props_changed`, which refreshes the result list.
/// - Navigation keys (`↑↓ Enter PgUp PgDn Home End`) are handled by the
///   component's `on_key` when the results list has focus.
///
/// ```no_run
/// # use tui_lipan::prelude::*;
/// # use std::sync::Arc;
/// // The caller owns `query: Arc<str>` and passes it every render.
/// // SearchPalette only rerenders the results list.
/// SearchPalette::<Arc<str>>::new()
///     .query(Arc::from("search text"))
///     .on_activate(Callback::new(|ev: SearchEvent<Arc<str>>| {
///         // handle activation
///     }));
/// ```
///
/// See `examples/search_palette_hub.rs` (Controlled tab) for a full example embedding
/// the query input inside a `Frame` divider with `join_frame(true)`.
#[derive(Clone)]
pub struct SearchPalette<T> {
    props: SearchPaletteProps<T>,
}

impl<T: Clone + PartialEq> Default for SearchPalette<T> {
    fn default() -> Self {
        Self {
            props: SearchPaletteProps {
                items: Arc::from([]),
                entries: Arc::from([]),
                sync_match_limit: DEFAULT_SYNC_MATCH_LIMIT,
                sync_selection: false,
                initial_query: "".into(),
                initial_selected_item_index: None,
                query: None,
                placeholder: "Search...".into(),
                width: Length::Flex(1),
                height: Length::Flex(1),
                max_width: None,
                max_height: None,
                input_prefix: None,
                input_suffix: None,
                input_border: false,
                input_divider: true,
                input_divider_style: Style::default(),
                input_divider_join_frame: true,
                input_caret_shape: CaretShape::default(),
                input_caret_color: None,
                input_border_style: BorderStyle::Plain,
                input_padding: Padding {
                    left: 1,
                    right: 1,
                    top: 0,
                    bottom: 0,
                },
                input_style: Style::default(),
                input_hover_style: StyleSlot::Inherit,
                input_focus_style: StyleSlot::Inherit,
                input_focus_content_style: Style::default(),
                input_placeholder_style: Style::default(),
                input_focus_placeholder_style: Style::default(),
                input_prefix_style: Style::default(),
                input_focus_prefix_style: Style::default(),
                input_suffix_style: Style::default(),
                input_focus_suffix_style: Style::default(),
                list_config: ListConfig {
                    border: false,
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
                list_symbol_column: None,
                list_hover_style: StyleSlot::Inherit,
                list_active_style: StyleSlot::Inherit,
                list_active_symbol: None,
                list_active_symbol_style: None,
                list_unselected_symbol: None,
                list_focusable: true,
                input_key: None,
                tab_stop: true,
                on_focus: None,
                on_blur: None,
                empty_text: Some("No matches".into()),
                item_style: Style::default(),
                active_item_style: None,
                header_style: Style::default(),
                description_style: Style::default(),
                active_description_style: None,
                focused_description_style: None,
                description_placement: DescriptionPlacement::Inline,
                description_separator: None,
                description_selection: true,
                description_overflow: DescriptionOverflow::Truncate,
                match_style: Style::default(),
                show_scores: false,
                score_gradient: None,
                score_range: None,
                preserve_groups: false,
                navigation_wrap: true,
                match_mode: SearchMatchMode::default(),
                case_matching: CaseMatching::Smart,
                normalization: Normalization::Smart,
                input_key_interceptor: None,
                on_query_change: None,
                on_select: None,
                on_activate: None,
                render_item: None,
                item_status: None,
                item_gutter: None,
            },
        }
    }
}

impl<T: Clone + PartialEq> SearchPalette<T> {
    /// Create a new search palette.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set searchable items (no headers or spacers).
    pub fn items(mut self, items: impl IntoIterator<Item = SearchItem<T>>) -> Self {
        self.props.items = items.into_iter().collect::<Vec<_>>().into();
        self.props.entries = Arc::from([]);
        self
    }

    /// Set searchable items from a shared slice.
    pub fn items_arc(mut self, items: Arc<[SearchItem<T>]>) -> Self {
        self.props.items = items;
        self.props.entries = Arc::from([]);
        self
    }

    /// Set items with optional headers and spacers.
    ///
    /// Use [`SearchEntry::item`] for searchable rows, [`SearchEntry::header`]
    /// for section labels, and [`SearchEntry::spacer`] for blank separators.
    /// Headers and spacers are excluded from fuzzy matching. They are shown in
    /// the results list only while the query is empty; active search renders a
    /// flat ranked result list for more stable navigation.
    pub fn entries(mut self, entries: impl IntoIterator<Item = SearchEntry<T>>) -> Self {
        let entries: Arc<[SearchEntry<T>]> = entries.into_iter().collect::<Vec<_>>().into();
        self.props.items = entries
            .iter()
            .filter_map(|e| {
                if let SearchEntry::Item(item) = e {
                    Some(item.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .into();
        self.props.entries = entries;
        self
    }

    /// Set entries from a shared slice.
    ///
    /// Searchable items are derived from [`SearchEntry::Item`] rows in the slice.
    pub fn entries_arc(mut self, entries: Arc<[SearchEntry<T>]>) -> Self {
        self.props.items = entries
            .iter()
            .filter_map(|e| {
                if let SearchEntry::Item(item) = e {
                    Some(item.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .into();
        self.props.entries = entries;
        self
    }

    /// Set placeholder.
    pub fn placeholder(mut self, placeholder: impl Into<Arc<str>>) -> Self {
        self.props.placeholder = placeholder.into();
        self
    }

    /// Set the maximum item count that still uses synchronous matching.
    ///
    /// Lists at or below this size update immediately on each query change.
    /// Larger lists keep the previous results visible while a background fuzzy
    /// search computes the next result set.
    pub fn sync_match_limit(mut self, limit: usize) -> Self {
        self.props.sync_match_limit = limit;
        self
    }

    /// Keep `on_select` synchronized with the currently visible selection.
    ///
    /// When enabled, the palette also emits `on_select` when it establishes an
    /// initial selection or when query/result changes move the internal
    /// selection to a different row.
    pub fn sync_selection(mut self, sync: bool) -> Self {
        self.props.sync_selection = sync;
        self
    }

    /// Set an initial query to pre-populate the search field.
    ///
    /// Only used in uncontrolled mode (when [`query`](Self::query) is not set).
    pub fn initial_query(mut self, query: impl Into<Arc<str>>) -> Self {
        self.props.initial_query = query.into();
        self
    }

    /// Start with the result row that corresponds to this index in [`items`](Self::items).
    ///
    /// The item must be present in the current match list; otherwise selection falls back to the
    /// first row. Ignored when `None` (default).
    pub fn initial_selected_item_index(mut self, index: Option<usize>) -> Self {
        self.props.initial_selected_item_index = index;
        self
    }

    /// Control whether Up/Down navigation wraps at list boundaries.
    ///
    /// Enabled by default. Disable for boundary rows such as "current position"
    /// where wrapping from the last row to the first row would jump unexpectedly.
    pub fn navigation_wrap(mut self, wrap: bool) -> Self {
        self.props.navigation_wrap = wrap;
        self
    }

    /// Drive the search query from outside the widget (controlled mode).
    ///
    /// When set, the palette renders **without** an `Input` widget - the caller
    /// is responsible for displaying and updating the query elsewhere.
    /// Changes to this prop are detected via `on_props_changed` and trigger a
    /// new async search automatically.
    ///
    /// Do not combine with [`initial_query`](Self::initial_query); that prop is
    /// ignored in controlled mode.
    pub fn query(mut self, query: impl Into<Arc<str>>) -> Self {
        self.props.query = Some(query.into());
        self
    }

    /// Set requested palette width.
    pub fn width(mut self, width: Length) -> Self {
        self.props.width = width;
        self
    }

    /// Set requested palette height.
    pub fn height(mut self, height: Length) -> Self {
        self.props.height = height;
        self
    }

    /// Set maximum palette width constraint.
    pub fn max_width(mut self, width: Length) -> Self {
        self.props.max_width = Some(width);
        self
    }

    /// Set maximum palette height constraint.
    pub fn max_height(mut self, height: Length) -> Self {
        self.props.max_height = Some(height);
        self
    }

    /// Set query change callback.
    pub fn on_query_change(mut self, cb: Callback<Arc<str>>) -> Self {
        self.props.on_query_change = Some(cb);
        self
    }

    /// Set selection callback.
    ///
    /// By default this fires for explicit keyboard or mouse selection changes.
    /// Combine with [`sync_selection`](Self::sync_selection) to also receive the
    /// currently visible selection when the palette initializes or when query
    /// changes move the internal selection.
    pub fn on_select(mut self, cb: Callback<SearchEvent<T>>) -> Self {
        self.props.on_select = Some(cb);
        self
    }

    /// Set a pre-insertion key interceptor for the internal `Input` widget.
    ///
    /// This handler runs **before** text insertion. If it returns `true`, the
    /// key is consumed and no character is inserted. Use this to remap keys
    /// like spacebar to a different action (e.g. toggle) in the palette.
    ///
    /// Only effective in uncontrolled mode (when [`query`](Self::query) is not set).
    pub fn input_key_interceptor(mut self, handler: KeyHandler) -> Self {
        self.props.input_key_interceptor = Some(handler);
        self
    }

    /// Set activation callback.
    pub fn on_activate(mut self, cb: Callback<SearchEvent<T>>) -> Self {
        self.props.on_activate = Some(cb);
        self
    }

    // -- Input forwarding --

    /// Override the prefix shown before the query text (default: `" "`).
    pub fn input_prefix(mut self, prefix: impl Into<Arc<str>>) -> Self {
        self.props.input_prefix = Some(prefix.into());
        self
    }

    /// Override the suffix shown after the query text (default: `"{matches}/{total}"`).
    pub fn input_suffix(mut self, suffix: impl Into<Arc<str>>) -> Self {
        self.props.input_suffix = Some(suffix.into());
        self
    }

    /// Set input border.
    pub fn input_border(mut self, border: bool) -> Self {
        self.props.input_border = border;
        self
    }

    /// Control whether a divider is rendered below the input (uncontrolled mode).
    ///
    /// Default: `true`.
    pub fn input_divider(mut self, divider: bool) -> Self {
        self.props.input_divider = divider;
        self
    }

    /// Set the style of the divider below the input (uncontrolled mode).
    pub fn input_divider_style(mut self, style: Style) -> Self {
        self.props.input_divider_style = style;
        self
    }

    /// Control whether the divider joins the surrounding frame border.
    ///
    /// Default: `true`.
    pub fn input_divider_join_frame(mut self, join: bool) -> Self {
        self.props.input_divider_join_frame = join;
        self
    }

    /// Set input caret shape (block, bar, or underline).
    pub fn input_caret_shape(mut self, shape: CaretShape) -> Self {
        self.props.input_caret_shape = shape;
        self
    }

    /// Set input caret color (OSC 12 cursor color, terminal support required).
    pub fn input_caret_color(mut self, color: Color) -> Self {
        self.props.input_caret_color = Some(color);
        self
    }

    /// Set input border style.
    pub fn input_border_style(mut self, border_style: BorderStyle) -> Self {
        self.props.input_border_style = border_style;
        self
    }

    /// Set input padding.
    pub fn input_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.input_padding = padding.into();
        self
    }

    /// Set input style.
    pub fn input_style(mut self, style: Style) -> Self {
        self.props.input_style = style;
        self
    }

    /// Set input hover style.
    pub fn input_hover_style(mut self, style: Style) -> Self {
        self.props.input_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed input hover style.
    pub fn extend_input_hover_style(mut self, style: Style) -> Self {
        self.props.input_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed input hover style.
    pub fn inherit_input_hover_style(mut self) -> Self {
        self.props.input_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set input hover style slot directly for composite forwarding.
    pub fn input_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.input_hover_style = slot;
        self
    }

    /// Set input focus chrome style.
    pub fn input_focus_style(mut self, style: Style) -> Self {
        self.props.input_focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed input focus style.
    pub fn extend_input_focus_style(mut self, style: Style) -> Self {
        self.props.input_focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed input focus style.
    pub fn inherit_input_focus_style(mut self) -> Self {
        self.props.input_focus_style = StyleSlot::Inherit;
        self
    }

    /// Set input focus style slot directly for composite forwarding.
    pub fn input_focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.input_focus_style = slot;
        self
    }

    /// Set focused input content text style.
    pub fn input_focus_content_style(mut self, style: Style) -> Self {
        self.props.input_focus_content_style = style;
        self
    }

    /// Set input placeholder style.
    pub fn input_placeholder_style(mut self, style: Style) -> Self {
        self.props.input_placeholder_style = style;
        self
    }

    /// Set input focus placeholder style.
    pub fn input_focus_placeholder_style(mut self, style: Style) -> Self {
        self.props.input_focus_placeholder_style = style;
        self
    }

    /// Set input prefix style.
    pub fn input_prefix_style(mut self, style: Style) -> Self {
        self.props.input_prefix_style = style;
        self
    }

    /// Set input focus prefix style.
    pub fn input_focus_prefix_style(mut self, style: Style) -> Self {
        self.props.input_focus_prefix_style = style;
        self
    }

    /// Set input suffix style.
    pub fn input_suffix_style(mut self, style: Style) -> Self {
        self.props.input_suffix_style = style;
        self
    }

    /// Set input focus suffix style.
    pub fn input_focus_suffix_style(mut self, style: Style) -> Self {
        self.props.input_focus_suffix_style = style;
        self
    }

    // -- List forwarding --

    /// Set list config.
    pub fn list_config(mut self, config: ListConfig) -> Self {
        self.props.list_config = config;
        self
    }

    /// Control whether the internal list reserves and renders its built-in symbol column.
    ///
    /// This is a convenience override for search/command palettes that use custom
    /// row gutters instead of the selected-row marker column. Gutter spacing and
    /// non-selectable-row gutter participation remain available through
    /// [`Self::list_config`].
    pub fn list_symbol_column(mut self, enabled: bool) -> Self {
        self.props.list_symbol_column = Some(enabled);
        self
    }

    /// Set list border.
    pub fn list_border(mut self, border: bool) -> Self {
        self.props.list_config.border = border;
        self
    }

    /// Set list border style.
    pub fn list_border_style(mut self, border_style: BorderStyle) -> Self {
        self.props.list_config.border_style = border_style;
        self
    }

    /// Set list padding.
    pub fn list_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.list_config.padding = padding.into();
        self
    }

    /// Set list style.
    pub fn list_style(mut self, style: Style) -> Self {
        self.props.list_config.style = style;
        self
    }

    /// Set list hover style.
    pub fn list_hover_style(mut self, style: Style) -> Self {
        self.props.list_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed list hover style.
    pub fn extend_list_hover_style(mut self, style: Style) -> Self {
        self.props.list_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed list hover style.
    pub fn inherit_list_hover_style(mut self) -> Self {
        self.props.list_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set list hover style slot directly for composite forwarding.
    pub fn list_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.list_hover_style = slot;
        self
    }

    /// Set list highlight style.
    pub fn list_selection_style(mut self, style: Style) -> Self {
        self.props.list_config.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed list highlight style.
    pub fn extend_list_selection_style(mut self, style: Style) -> Self {
        self.props.list_config.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed list highlight style.
    pub fn inherit_list_selection_style(mut self) -> Self {
        self.props.list_config.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set list highlight style slot directly for composite forwarding.
    pub fn list_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.list_config.selection_style = slot;
        self
    }

    /// Set list highlight style while the list is not focused.
    pub fn list_unfocused_selection_style(mut self, style: Style) -> Self {
        self.props.list_config.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed list highlight style while the list is not focused.
    pub fn extend_list_unfocused_selection_style(mut self, style: Style) -> Self {
        self.props.list_config.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed list highlight style while the list is not focused.
    pub fn inherit_list_unfocused_selection_style(mut self) -> Self {
        self.props.list_config.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set list unfocused highlight style slot directly for composite forwarding.
    pub fn list_unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.list_config.unfocused_selection_style = slot;
        self
    }

    /// Set list highlight symbol.
    pub fn list_selection_symbol(mut self, symbol: impl Into<Arc<str>>) -> Self {
        self.props.list_config.selection_symbol = Some(symbol.into());
        self
    }

    /// Set the trailing list selection symbol (right "pill" cap). Pairs with
    /// [`Self::list_selection_symbol`] and shares the selection symbol style.
    pub fn list_selection_symbol_right(mut self, symbol: impl Into<Arc<str>>) -> Self {
        self.props.list_config.selection_symbol_right = Some(symbol.into());
        self
    }

    /// Set list highlight symbol style.
    pub fn list_selection_symbol_style(mut self, style: Style) -> Self {
        self.props.list_config.selection_symbol_style = Some(style);
        self
    }

    /// Set list highlight symbol style while the list is not focused.
    pub fn list_unfocused_selection_symbol_style(mut self, style: Style) -> Self {
        self.props.list_config.unfocused_selection_symbol_style = Some(style);
        self
    }

    /// Set the symbol shown for non-selected items (indentation alignment).
    pub fn list_unselected_symbol(mut self, symbol: impl Into<Arc<str>>) -> Self {
        self.props.list_unselected_symbol = Some(symbol.into());
        self
    }

    /// Extend the highlight style to the full width of the list.
    pub fn list_selection_full_width(mut self, full_width: bool) -> Self {
        self.props.list_config.selection_full_width = full_width;
        self
    }

    /// Set hover style applied to individual list items on mouse hover.
    pub fn list_item_hover_style(mut self, style: Style) -> Self {
        self.props.list_config.item_hover_style = Some(StyleSlot::Replace(style));
        self
    }

    /// Extend the themed list item-hover style.
    pub fn extend_list_item_hover_style(mut self, style: Style) -> Self {
        self.props.list_config.item_hover_style = Some(StyleSlot::Extend(style));
        self
    }

    /// Inherit the themed list item-hover style.
    pub fn inherit_list_item_hover_style(mut self) -> Self {
        self.props.list_config.item_hover_style = Some(StyleSlot::Inherit);
        self
    }

    /// Set list item-hover style slot directly for composite forwarding.
    pub fn list_item_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.list_config.item_hover_style = Some(slot);
        self
    }

    /// Set list active item style.
    pub fn list_active_style(mut self, style: Style) -> Self {
        self.props.list_active_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed list active style.
    pub fn extend_list_active_style(mut self, style: Style) -> Self {
        self.props.list_active_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed list active style.
    pub fn inherit_list_active_style(mut self) -> Self {
        self.props.list_active_style = StyleSlot::Inherit;
        self
    }

    /// Set list active style slot directly for composite forwarding.
    pub fn list_active_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.list_active_style = slot;
        self
    }

    /// Set list active item symbol.
    pub fn list_active_symbol(mut self, symbol: impl Into<Arc<str>>) -> Self {
        self.props.list_active_symbol = Some(symbol.into());
        self
    }

    /// Set list active item symbol style.
    pub fn list_active_symbol_style(mut self, style: Style) -> Self {
        self.props.list_active_symbol_style = Some(style);
        self
    }

    /// Set list row padding for normal rows.
    ///
    /// Only left/right are used by List; top/bottom are ignored.
    pub fn list_item_horizontal_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.list_config.item_horizontal_padding = padding.into();
        self
    }

    /// Set list row padding for header rows.
    ///
    /// Only left/right are used by List; top/bottom are ignored.
    pub fn list_header_horizontal_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.list_config.header_horizontal_padding = padding.into();
        self
    }

    /// Control whether the list can receive keyboard focus.
    pub fn list_focusable(mut self, focusable: bool) -> Self {
        self.props.list_focusable = focusable;
        self
    }

    /// Set a reconciliation key on the query input.
    ///
    /// Keying the palette element itself keys the palette's container, which is not focusable, so
    /// `Context::request_focus` on that key can only reach the input through the container's
    /// first-focusable-descendant fallback - and lands elsewhere the moment the palette gains
    /// another focusable widget. This addresses the input directly:
    ///
    /// ```ignore
    /// SearchPalette::<T>::new().input_key("command-palette-query")
    /// // ...
    /// ctx.request_focus("command-palette-query");
    /// ```
    ///
    /// Only meaningful in uncontrolled mode; a controlled palette renders no input of its own.
    pub fn input_key(mut self, key: impl Into<Key>) -> Self {
        self.props.input_key = Some(key.into());
        self
    }

    /// Control whether the palette's primary focus target participates in tab navigation.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.props.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the palette's primary focus target receives focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.props.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the palette's primary focus target loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.props.on_blur = Some(cb);
        self
    }

    /// Set list scrollbar.
    pub fn list_scrollbar(mut self, scroll: bool) -> Self {
        self.props.list_config.scrollbar = scroll;
        self
    }

    /// Set list scrollbar configuration.
    pub fn list_scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.props.list_config.scrollbar_config = config;
        self
    }

    /// Set empty text.
    pub fn empty_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.props.empty_text = Some(text.into());
        self
    }

    /// Set empty text style.
    pub fn empty_text_style(mut self, style: Style) -> Self {
        self.props.list_config.empty_text_style = style;
        self
    }

    // -- Item rendering --

    /// Set item base style.
    pub fn item_style(mut self, style: Style) -> Self {
        self.props.item_style = style;
        self
    }

    /// Set the style applied to section header rows (entries created with
    /// [`SearchEntry::header`]). Defaults to the inherited/ambient style, which
    /// makes headers look like normal items.
    pub fn header_style(mut self, style: Style) -> Self {
        self.props.header_style = style;
        self
    }

    /// Set description style.
    pub fn description_style(mut self, style: Style) -> Self {
        self.props.description_style = style;
        self
    }

    /// Set description placement.
    pub fn description_placement(mut self, placement: DescriptionPlacement) -> Self {
        self.props.description_placement = placement;
        self
    }

    /// Set the separator between label and inline description.
    ///
    /// Only applies to [`DescriptionPlacement::Inline`]. Defaults to `" - "`.
    /// Pass `" "` for a single space gap with no visible separator.
    pub fn description_separator(mut self, separator: impl Into<Arc<str>>) -> Self {
        self.props.description_separator = Some(separator.into());
        self
    }

    /// Control whether selection highlight applies to description text.
    ///
    /// For [`DescriptionPlacement::Inline`], description shares the primary line,
    /// so this setting has no effect.
    pub fn description_selection(mut self, highlight: bool) -> Self {
        self.props.description_selection = highlight;
        self
    }

    /// Control whether descriptions wrap or truncate.
    ///
    /// Wrapping applies to [`DescriptionPlacement::Above`] and
    /// [`DescriptionPlacement::Below`].
    /// [`DescriptionPlacement::Inline`] and [`DescriptionPlacement::Right`]
    /// always truncate to keep a single primary row.
    pub fn description_overflow(mut self, overflow: DescriptionOverflow) -> Self {
        self.props.description_overflow = overflow;
        self
    }

    /// Set match highlight style.
    pub fn match_style(mut self, style: Style) -> Self {
        self.props.match_style = style;
        self
    }

    /// Show a numeric score in the right slot for matched rows.
    pub fn show_scores(mut self, show: bool) -> Self {
        self.props.show_scores = show;
        self
    }

    /// Set gradient used to color score values.
    pub fn score_gradient(mut self, gradient: ColorGradient) -> Self {
        self.props.score_gradient = Some(gradient);
        self
    }

    /// Set explicit score range used by score gradient.
    pub fn score_range(mut self, min: u64, max: u64) -> Self {
        self.props.score_range = Some(GradientRange::new(min, max));
        self
    }

    /// Keep group structure (headers/spacers) visible during active search.
    ///
    /// By default, groups are only shown while the query is empty and hidden
    /// once a search term is entered. When set to `true`, matched items remain
    /// grouped under their original category headers, empty groups are
    /// suppressed automatically, and arrow-key navigation follows visual
    /// (definition) order rather than score order.
    pub fn preserve_groups(mut self, preserve: bool) -> Self {
        self.props.preserve_groups = preserve;
        self
    }

    /// Set the matching strategy used to rank results.
    ///
    /// Defaults to [`SearchMatchMode::Fuzzy`]. See [`SearchMatchMode::Hybrid`]
    /// for exact/prefix/word-prefix/substring/fuzzy tiered matching.
    pub fn match_mode(mut self, mode: SearchMatchMode) -> Self {
        self.props.match_mode = mode;
        self
    }

    /// Set case matching configuration.
    pub fn case_matching(mut self, case: CaseMatching) -> Self {
        self.props.case_matching = case;
        self
    }

    /// Set normalization configuration.
    pub fn normalization(mut self, normalization: Normalization) -> Self {
        self.props.normalization = normalization;
        self
    }

    /// Override the label style for active items in the default renderer.
    ///
    /// When set, active items' label spans use this style instead of [`item_style`](Self::item_style).
    /// Has no effect when a custom [`render_item`](Self::render_item) renderer is used.
    pub fn active_item_style(mut self, style: Style) -> Self {
        self.props.active_item_style = Some(style);
        self
    }

    /// Override the description style for active items in the default renderer.
    ///
    /// When set, active items' description spans use this style instead of
    /// [`description_style`](Self::description_style).
    /// Has no effect when a custom [`render_item`](Self::render_item) renderer is used.
    pub fn active_description_style(mut self, style: Style) -> Self {
        self.props.active_description_style = Some(style);
        self
    }

    /// Override the description style for the currently focused (selected) item
    /// in the default renderer.
    ///
    /// When set, the focused item's description spans use this style instead of
    /// [`description_style`](Self::description_style). Takes precedence over
    /// [`active_description_style`](Self::active_description_style).
    /// Has no effect when a custom [`render_item`](Self::render_item) renderer is used.
    pub fn focused_description_style(mut self, style: Style) -> Self {
        self.props.focused_description_style = Some(style);
        self
    }

    /// Set a custom renderer for matched items.
    ///
    /// Return [`Some`] to replace the default rendering for that item, or [`None`] to fall
    /// through to the built-in default renderer.
    pub fn render_item(mut self, renderer: SearchRenderer<T>) -> Self {
        self.props.render_item = Some(renderer);
        self
    }

    /// Set a custom per-row status renderer for the list symbol column.
    ///
    /// Status content replaces the selection or unselected symbol/spaces for
    /// that row, while active symbols keep priority.
    pub fn item_status(mut self, renderer: SearchStatusRenderer<T>) -> Self {
        self.props.item_status = Some(renderer);
        self
    }

    /// Set a custom left-gutter renderer for matched items.
    ///
    /// This preserves the built-in row rendering and only attaches a fixed-width
    /// gutter, which is useful for status widgets such as [`Spinner`](crate::widgets::Spinner).
    pub fn item_gutter(mut self, renderer: SearchGutterRenderer<T>) -> Self {
        self.props.item_gutter = Some(renderer);
        self
    }
}

impl<T: Clone + PartialEq + 'static> From<SearchPalette<T>> for Element {
    fn from(palette: SearchPalette<T>) -> Self {
        component::element(palette.props)
    }
}
