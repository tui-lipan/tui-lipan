//! Diff view types.

use super::render::{DiffRender, build_diff_data};
use crate::callback::Callback;
use crate::core::element::Element;
use crate::core::event::KeyMods;
use crate::style::Style;
use crate::widgets::ScrollEvent;
use std::sync::Arc;

/// Diff presentation mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiffViewMode {
    /// Split view with left and right panes.
    Split,
    /// Unified view with a single pane.
    Unified,
}

/// Rendering backend used by [`DiffView`](super::DiffView).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DiffViewBackend {
    /// Render using [`TextArea`](crate::widgets::TextArea).
    ///
    /// Supports editing (`DiffView::editable(true)`) and syntax strategy passthrough.
    #[default]
    TextArea,
    /// Render using [`DocumentView`](crate::widgets::DocumentView).
    ///
    /// Optimized for read-only review and selection workflows.
    DocumentView,
}

/// Identifies which pane produced a diff event.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiffPane {
    /// Left pane in split mode (before side).
    Left,
    /// Right pane in split mode (after side).
    Right,
    /// Unified pane in unified mode.
    Unified,
}

/// Source side preferred when resolving an anchored row across diff modes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DiffLineSide {
    /// Resolve through the original/source line.
    Old,
    /// Resolve through the modified/source line.
    #[default]
    New,
}

/// Stable source-line identity for one rendered diff row.
///
/// Line numbers are git-style and 1-based. Added rows have only [`Self::new_line`],
/// removed rows have only [`Self::old_line`], and unchanged rows normally have both.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiffLineAnchor {
    /// Original/source line represented by the row.
    pub old_line: Option<usize>,
    /// Modified/source line represented by the row.
    pub new_line: Option<usize>,
    /// Zero-based patch hunk index when the diff came from a unified patch.
    ///
    /// This disambiguates repeated line numbers across files. Before/after
    /// content diffs do not have parsed patch hunks and report `None`.
    pub hunk_index: Option<usize>,
    /// Side used when a split replacement becomes separate removed/added rows
    /// in unified mode.
    pub preferred_side: DiffLineSide,
}

impl DiffLineAnchor {
    /// Create a source-line anchor.
    pub const fn new(old_line: Option<usize>, new_line: Option<usize>) -> Self {
        Self {
            old_line,
            new_line,
            hunk_index: None,
            preferred_side: if new_line.is_some() {
                DiffLineSide::New
            } else {
                DiffLineSide::Old
            },
        }
    }

    /// Associate this anchor with a zero-based parsed patch hunk.
    pub const fn with_hunk_index(mut self, hunk_index: usize) -> Self {
        self.hunk_index = Some(hunk_index);
        self
    }

    /// Prefer one source side when resolving this anchor across view modes.
    pub const fn with_preferred_side(mut self, side: DiffLineSide) -> Self {
        self.preferred_side = side;
        self
    }

    /// Source line on the preferred side, falling back to the other side.
    pub const fn preferred_line(self) -> Option<usize> {
        match self.preferred_side {
            DiffLineSide::Old => match self.old_line {
                Some(line) => Some(line),
                None => self.new_line,
            },
            DiffLineSide::New => match self.new_line {
                Some(line) => Some(line),
                None => self.old_line,
            },
        }
    }

    pub(crate) fn matches_source_row(
        self,
        old_line: Option<usize>,
        new_line: Option<usize>,
        hunk_index: Option<usize>,
    ) -> bool {
        if self.hunk_index != hunk_index {
            return false;
        }
        if self.old_line == old_line && self.new_line == new_line {
            return true;
        }
        match self.preferred_side {
            DiffLineSide::Old => self.old_line.is_some() && self.old_line == old_line,
            DiffLineSide::New => self.new_line.is_some() && self.new_line == new_line,
        }
    }
}

/// Stable inclusive source-row range for a multiline diff comment.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiffLineRange {
    /// First selected source row.
    pub start: DiffLineAnchor,
    /// Last selected source row.
    pub end: DiffLineAnchor,
}

impl DiffLineRange {
    /// Create an inclusive range, ordered by source line when both endpoints
    /// use the same preferred side.
    pub fn new(mut start: DiffLineAnchor, mut end: DiffLineAnchor) -> Self {
        if start.hunk_index == end.hunk_index
            && start.preferred_side == end.preferred_side
            && start.preferred_line() > end.preferred_line()
        {
            std::mem::swap(&mut start, &mut end);
        }
        Self { start, end }
    }

    /// Create a one-row range.
    pub const fn single(anchor: DiffLineAnchor) -> Self {
        Self {
            start: anchor,
            end: anchor,
        }
    }

    /// Whether both endpoints identify the same source row.
    pub fn is_single_line(self) -> bool {
        self.start.old_line == self.end.old_line
            && self.start.new_line == self.end.new_line
            && self.start.hunk_index == self.end.hunk_index
            && self.start.preferred_side == self.end.preferred_side
    }

    /// Whether both endpoints belong to the same before/after diff or patch hunk.
    pub fn is_valid(self) -> bool {
        self.start.hunk_index == self.end.hunk_index
    }
}

/// Event emitted when a source row in a [`DiffView`](super::DiffView) is clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiffLineClickEvent {
    /// Pane that received the click.
    pub pane: DiffPane,
    /// Stable source-line identity shared by the aligned split row.
    pub anchor: DiffLineAnchor,
    /// Zero-based rendered logical row before soft wrapping.
    pub logical_line: usize,
    /// Keyboard modifiers held for application-specific click actions.
    pub mods: KeyMods,
}

/// Event emitted after dragging through a diff pane's line-number gutter.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiffLineRangeEvent {
    /// Pane where the drag started and ended.
    pub pane: DiffPane,
    /// Inclusive source range selected by the drag.
    pub range: DiffLineRange,
    /// Zero-based logical row where the drag started.
    pub start_logical_line: usize,
    /// Zero-based logical row where the drag ended.
    pub end_logical_line: usize,
}

/// An arbitrary element inserted after a source row in a [`DiffView`](super::DiffView).
///
/// Inline blocks span the complete diff width in both unified and split modes,
/// making them suitable for comment editors and review controls.
#[derive(Clone)]
pub struct DiffInlineBlock {
    pub(crate) range: DiffLineRange,
    pub(crate) content: Element,
}

impl DiffInlineBlock {
    /// Insert `content` after the row identified by `anchor`.
    pub fn after(anchor: DiffLineAnchor, content: impl Into<Element>) -> Self {
        Self::after_range(DiffLineRange::single(anchor), content)
    }

    /// Insert `content` after the final row in an inclusive source range.
    pub fn after_range(range: DiffLineRange, content: impl Into<Element>) -> Self {
        Self {
            range,
            content: content.into(),
        }
    }

    /// Source row after which this block is inserted.
    pub const fn anchor(&self) -> DiffLineAnchor {
        self.range.end
    }

    /// Inclusive source range associated with this block.
    pub const fn range(&self) -> DiffLineRange {
        self.range
    }
}

/// Scroll event emitted by [`DiffView`](super::DiffView) with pane metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiffScrollEvent {
    /// Pane that emitted the event.
    pub pane: DiffPane,
    /// Scroll payload from the underlying viewer.
    pub scroll: ScrollEvent,
}

/// Direction of a collapsed context separator in a [`DiffView`](super::DiffView).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiffContextSeparatorDirection {
    /// Hidden lines are above the visible diff hunk.
    Above,
    /// Hidden lines are below the visible diff hunk.
    Below,
    /// Hidden lines are between two visible diff hunks.
    Between,
}

/// Stable identifier for a collapsed unchanged range in a [`DiffView`](super::DiffView).
///
/// Line numbers are git-style, 1-based, and inclusive when present. They are
/// `None` for patch metadata or malformed context that has no source mapping.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct DiffContextRange {
    /// First hidden original/source line, inclusive.
    pub old_start: Option<usize>,
    /// Last hidden original/source line, inclusive.
    pub old_end: Option<usize>,
    /// First hidden modified/source line, inclusive.
    pub new_start: Option<usize>,
    /// Last hidden modified/source line, inclusive.
    pub new_end: Option<usize>,
}

/// Controlled expansion state for one collapsed context range.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiffContextExpansion {
    /// Hidden source range represented by the separator.
    pub range: DiffContextRange,
    /// Number of previously hidden lines now revealed.
    ///
    /// Use [`usize::MAX`] (via [`Self::full`]) to reveal the entire collapsed run.
    pub lines_revealed: usize,
}

impl DiffContextExpansion {
    /// Expand the entire collapsed range identified by `range`.
    pub fn full(range: DiffContextRange) -> Self {
        Self {
            range,
            lines_revealed: usize::MAX,
        }
    }
}

impl From<DiffContextRange> for DiffContextExpansion {
    fn from(range: DiffContextRange) -> Self {
        Self::full(range)
    }
}

/// Event emitted when a collapsed context separator is clicked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiffContextSeparatorEvent {
    /// Pane that received the click.
    pub pane: DiffPane,
    /// Hidden source range represented by the separator.
    pub range: DiffContextRange,
    /// Number of hidden logical lines still represented by the rendered separator.
    pub hidden_lines: usize,
    /// Position of the hidden range relative to visible diff hunks.
    pub direction: DiffContextSeparatorDirection,
    /// Number of additional hidden lines revealed by [`Self::next_expansion`].
    pub expand_lines: usize,
}

impl DiffContextSeparatorEvent {
    /// Compute the next controlled expansion after one click.
    ///
    /// `current` is the existing entry for this separator's [`Self::range`], if any.
    pub fn next_expansion(&self, current: Option<&DiffContextExpansion>) -> DiffContextExpansion {
        self.next_expansion_by(current, self.expand_lines)
    }

    /// Compute the next controlled expansion using a custom per-click reveal size.
    pub fn next_expansion_by(
        &self,
        current: Option<&DiffContextExpansion>,
        step: usize,
    ) -> DiffContextExpansion {
        let current_revealed = current
            .filter(|expansion| expansion.range == self.range)
            .map(|expansion| expansion.lines_revealed)
            .unwrap_or(0);
        let max_revealed = current_revealed.saturating_add(self.hidden_lines);
        let revealed = current_revealed.saturating_add(step).min(max_revealed);
        DiffContextExpansion {
            range: self.range,
            lines_revealed: revealed,
        }
    }
}

/// Logical row anchor for one parsed patch hunk in a [`DiffView`](super::DiffView).
///
/// `logical_line` is a zero-based rendered source row before soft wrapping. Pass it
/// to `TextArea::scroll_to_line` or `DocumentView::scroll_to_source_line` to let the
/// backend resolve the final visual row after layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DiffHunkAnchor {
    /// Pane this anchor belongs to.
    pub pane: DiffPane,
    /// Zero-based hunk index in patch order.
    pub index: usize,
    /// Original/source start line from the `@@` header, when present.
    pub old_start: Option<usize>,
    /// Modified/source start line from the `@@` header, when present.
    pub new_start: Option<usize>,
    /// Zero-based logical row in the rendered pane.
    pub logical_line: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DiffHunk {
    pub index: usize,
    pub old_start: Option<usize>,
    pub new_start: Option<usize>,
}

impl DiffHunk {
    pub(crate) fn anchor(self, pane: DiffPane, logical_line: usize) -> DiffHunkAnchor {
        DiffHunkAnchor {
            pane,
            index: self.index,
            old_start: self.old_start,
            new_start: self.new_start,
            logical_line,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct DiffContextSeparator {
    pub range: DiffContextRange,
    pub hidden_lines: usize,
    pub direction: DiffContextSeparatorDirection,
}

impl DiffContextSeparator {
    pub(crate) fn event(&self, pane: DiffPane, expand_lines: usize) -> DiffContextSeparatorEvent {
        DiffContextSeparatorEvent {
            pane,
            range: self.range,
            hidden_lines: self.hidden_lines,
            direction: self.direction,
            expand_lines,
        }
    }
}

#[derive(Clone)]
pub(crate) struct DiffContextSeparatorClickConfig {
    pub events_by_source_line: Arc<[Option<DiffContextSeparatorEvent>]>,
    pub on_click: Option<Callback<DiffContextSeparatorEvent>>,
    pub hover_style: Option<Style>,
}

#[derive(Clone)]
pub(crate) struct DiffLineClickConfig {
    pub events_by_source_line: Arc<[Option<DiffLineClickEvent>]>,
    pub on_click: Option<Callback<DiffLineClickEvent>>,
    pub on_range_select: Option<Callback<DiffLineRangeEvent>>,
    pub range_style: Style,
}

/// Prefixes used for diff lines.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DiffPrefixes {
    /// Prefix for unchanged lines.
    pub context: Arc<str>,
    /// Prefix for added lines.
    pub added: Arc<str>,
    /// Prefix for removed lines.
    pub removed: Arc<str>,
}

impl DiffPrefixes {
    /// Create a new prefix set.
    pub fn new(
        context: impl Into<Arc<str>>,
        added: impl Into<Arc<str>>,
        removed: impl Into<Arc<str>>,
    ) -> Self {
        Self {
            context: context.into(),
            added: added.into(),
            removed: removed.into(),
        }
    }
}

impl Default for DiffPrefixes {
    fn default() -> Self {
        Self {
            context: "  ".into(),
            added: "+ ".into(),
            removed: "- ".into(),
        }
    }
}

impl crate::style::DiffPalette {
    pub(crate) fn line_style(self, kind: super::render::DiffLineKind) -> crate::style::Style {
        use super::render::DiffLineKind;
        match kind {
            DiffLineKind::Context => self.context,
            DiffLineKind::PatchHeader => self.patch_header,
            DiffLineKind::Added => self.added,
            DiffLineKind::Removed => self.removed,
            DiffLineKind::Empty => self.empty,
            DiffLineKind::Separator => self.context_separator_style,
        }
    }

    pub(crate) fn word_style(
        self,
        kind: super::render::DiffLineKind,
    ) -> Option<crate::style::Style> {
        use super::render::DiffLineKind;
        match kind {
            DiffLineKind::Added => Some(self.added_word),
            DiffLineKind::Removed => Some(self.removed_word),
            _ => None,
        }
    }

    pub(crate) fn marker_style(
        self,
        kind: super::render::DiffLineKind,
        line_style: crate::style::Style,
    ) -> Option<crate::style::Style> {
        use super::render::DiffLineKind;
        match kind {
            DiffLineKind::Added => Some(line_style.patch(self.added_marker)),
            DiffLineKind::Removed => Some(line_style.patch(self.removed_marker)),
            _ => None,
        }
    }

    pub(crate) fn line_number_style(
        self,
        kind: super::render::DiffLineKind,
        line_style: crate::style::Style,
    ) -> Option<crate::style::Style> {
        use super::render::DiffLineKind;
        match kind {
            DiffLineKind::Context => Some(line_style.patch(self.context_line_number)),
            DiffLineKind::PatchHeader => None,
            DiffLineKind::Added => Some(line_style.patch(self.added_line_number)),
            DiffLineKind::Removed => Some(line_style.patch(self.removed_line_number)),
            _ => None,
        }
    }
}

pub(crate) fn default_context_separator_min_lines() -> usize {
    2
}

pub(crate) fn default_context_expand_lines() -> usize {
    20
}

pub(crate) fn default_context_separator_text() -> Arc<str> {
    "{arrow} {count} hidden {line_word} {direction} {arrow}".into()
}

/// Diff data configuration used for precomputing a diff.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DiffDataConfig {
    /// Prefixes for diff lines.
    pub prefixes: DiffPrefixes,
    /// Whether to include prefixes in the rendered text.
    pub show_prefixes: bool,
    /// Whether to compute word-level diffs.
    pub word_diff: bool,
    /// Number of unchanged context lines to keep around each change.
    ///
    /// `None` (the default) shows all lines.  `Some(4)` keeps at most 4
    /// unchanged lines above and below every changed region and inserts `DiffLineKind::Separator`
    /// lines in place of the hidden ranges.
    pub context_lines: Option<usize>,
    /// Whether to insert a visible separator line when collapsing context.
    ///
    /// When `true` (the default) a context separator line is shown in place of
    /// each collapsed region. When `false` the hidden lines are simply omitted
    /// without any visual placeholder.
    pub show_context_separator: bool,
    /// Template text for context separators.
    ///
    /// Supported placeholders:
    /// - `{count}`: number of hidden lines
    /// - `{line_word}`: `line` or `lines`
    /// - `{direction}`: `above`, `below`, or `between`
    /// - `{arrow}`: `↑`, `↓`, or `↑↓`
    pub context_separator_text: Arc<str>,
}

impl Default for DiffDataConfig {
    fn default() -> Self {
        Self {
            prefixes: DiffPrefixes::default(),
            show_prefixes: true,
            word_diff: true,
            context_lines: None,
            show_context_separator: true,
            context_separator_text: default_context_separator_text(),
        }
    }
}

/// Precomputed diff data for reuse across renders.
#[derive(Clone)]
pub struct DiffData {
    pub(crate) left: DiffRender,
    pub(crate) right: DiffRender,
    pub(crate) unified: DiffRender,
}

impl DiffData {
    /// Build diff data with default config.
    pub fn new(before: &str, after: &str) -> Self {
        Self::with_config(before, after, DiffDataConfig::default())
    }

    /// Build diff data with a custom config.
    pub fn with_config(before: &str, after: &str, config: DiffDataConfig) -> Self {
        build_diff_data(before, after, config)
    }

    /// Build diff data from a raw unified diff (patch) string.
    pub fn from_patch(patch: &str) -> Self {
        Self::from_patch_with_config(patch, DiffDataConfig::default())
    }

    /// Build diff data from a raw unified diff (patch) string with custom config.
    pub fn from_patch_with_config(patch: &str, config: DiffDataConfig) -> Self {
        super::render::build_patch_data(patch, config)
    }

    /// Return hunk anchors for the pane normally used by `mode`.
    ///
    /// Unified mode returns anchors for the unified pane. Split mode returns the
    /// left-pane anchors; split panes are row-aligned, so the same logical row can
    /// be used to scroll both sides.
    pub fn hunk_anchors(&self, mode: DiffViewMode) -> Vec<DiffHunkAnchor> {
        match mode {
            DiffViewMode::Unified => self.hunk_anchors_for_pane(DiffPane::Unified),
            DiffViewMode::Split => self.hunk_anchors_for_pane(DiffPane::Left),
        }
    }

    /// Return hunk anchors for one rendered pane.
    pub fn hunk_anchors_for_pane(&self, pane: DiffPane) -> Vec<DiffHunkAnchor> {
        let render = match pane {
            DiffPane::Left => &self.left,
            DiffPane::Right => &self.right,
            DiffPane::Unified => &self.unified,
        };
        super::render::hunk_anchors_for_render(render, pane)
    }

    pub(crate) fn new_internal(left: DiffRender, right: DiffRender, unified: DiffRender) -> Self {
        Self {
            left,
            right,
            unified,
        }
    }
}
