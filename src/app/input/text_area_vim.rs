use std::collections::BTreeMap;
use std::sync::Arc;

use crate::utils::text::{clamp_cursor, next_char_boundary};
use crate::widgets::{TextAreaVimMode, TextAreaVimSearchFeedback};

const VIM_COUNT_CAP: usize = 1_000_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TextAreaVimState {
    pub mode: TextAreaVimMode,
    pub count: Option<usize>,
    pub pending: Option<TextAreaVimPending>,
    pub visual_anchor: Option<usize>,
    pub visual_line_head: Option<usize>,
    pub visual_line_preferred_col: Option<usize>,
    pub visual_line_caret: Option<usize>,
    pub active_register: Option<char>,
    pub registers: VimRegisters,
    pub last_change: Option<VimRepeatChange>,
    pub insert_session: Option<VimInsertSession>,
    pub search: VimSearchState,
    pub yank_feedback_range: Option<(usize, usize)>,
    pub pending_yank_feedback: bool,
    pub marks: BTreeMap<char, usize>,
    pub previous_jump: Option<usize>,
}

impl Default for TextAreaVimState {
    fn default() -> Self {
        Self {
            mode: TextAreaVimMode::Normal,
            count: None,
            pending: None,
            visual_anchor: None,
            visual_line_head: None,
            visual_line_preferred_col: None,
            visual_line_caret: None,
            active_register: None,
            registers: VimRegisters::default(),
            last_change: None,
            insert_session: None,
            search: VimSearchState::default(),
            yank_feedback_range: None,
            pending_yank_feedback: false,
            marks: BTreeMap::new(),
            previous_jump: None,
        }
    }
}

impl TextAreaVimState {
    pub fn push_count_digit(&mut self, digit: u8) -> usize {
        let digit = usize::from(digit.min(9));
        let next = self
            .count
            .unwrap_or(0)
            .saturating_mul(10)
            .saturating_add(digit)
            .min(VIM_COUNT_CAP);
        self.count = Some(next);
        next
    }

    pub fn clear_count_pending(&mut self) {
        self.count = None;
        self.pending = None;
        self.active_register = None;
    }

    pub fn clear_visual_anchor(&mut self) {
        self.visual_anchor = None;
        self.visual_line_head = None;
        self.visual_line_preferred_col = None;
        self.visual_line_caret = None;
    }

    pub fn set_mode(&mut self, mode: TextAreaVimMode) -> bool {
        if self.mode == mode {
            if !matches!(mode, TextAreaVimMode::Visual | TextAreaVimMode::VisualLine) {
                self.clear_visual_anchor();
            } else if !matches!(mode, TextAreaVimMode::VisualLine) {
                self.visual_line_head = None;
                self.visual_line_preferred_col = None;
                self.visual_line_caret = None;
            }
            return false;
        }

        self.mode = mode;
        self.clear_count_pending();
        if !matches!(mode, TextAreaVimMode::Visual | TextAreaVimMode::VisualLine) {
            self.clear_visual_anchor();
        } else if !matches!(mode, TextAreaVimMode::VisualLine) {
            self.visual_line_head = None;
            self.visual_line_preferred_col = None;
            self.visual_line_caret = None;
        }
        true
    }

    pub fn set_yank_feedback_range(&mut self, range: (usize, usize), copied: bool) {
        if copied && range.0 < range.1 {
            self.yank_feedback_range = Some(range);
            self.pending_yank_feedback = true;
        } else {
            self.yank_feedback_range = None;
            self.pending_yank_feedback = false;
        }
    }

    pub fn take_pending_yank_feedback(&mut self) -> bool {
        let pending = self.pending_yank_feedback;
        self.pending_yank_feedback = false;
        pending
    }
}

pub(crate) fn sync_visual_mode_for_external_selection(
    state: &mut TextAreaVimState,
    cursor: usize,
    anchor: Option<usize>,
) -> Option<TextAreaVimMode> {
    if let Some(anchor) = anchor.filter(|anchor| *anchor != cursor) {
        let changed = state.set_mode(TextAreaVimMode::Visual);
        state.visual_anchor = Some(anchor);
        state.visual_line_head = None;
        state.visual_line_preferred_col = None;
        state.visual_line_caret = None;
        changed.then_some(TextAreaVimMode::Visual)
    } else if matches!(
        state.mode,
        TextAreaVimMode::Visual | TextAreaVimMode::VisualLine
    ) {
        state
            .set_mode(TextAreaVimMode::Normal)
            .then_some(TextAreaVimMode::Normal)
    } else {
        None
    }
}

pub(crate) fn text_area_vim_search_feedback_for_text(
    state: &TextAreaVimState,
    text: &str,
    cursor: usize,
) -> Option<TextAreaVimSearchFeedback> {
    match &state.pending {
        Some(TextAreaVimPending::Search {
            forward,
            query,
            cursor: query_cursor,
        }) => {
            let (target_range, current_match_index, match_count) =
                vim_search_feedback_match_data(text, cursor, query, *forward, true);
            Some(TextAreaVimSearchFeedback {
                query: Arc::from(query.as_str()),
                cursor: clamp_cursor(query, *query_cursor),
                forward: *forward,
                pending: true,
                target_range,
                current_match_index,
                match_count,
            })
        }
        _ if state.search.visible => state.search.query.as_deref().map(|query| {
            let (target_range, current_match_index, match_count) =
                vim_search_feedback_match_data(text, cursor, query, state.search.forward, false);
            TextAreaVimSearchFeedback {
                query: Arc::from(query),
                cursor: query.len(),
                forward: state.search.forward,
                pending: false,
                target_range,
                current_match_index,
                match_count,
            }
        }),
        _ => None,
    }
}

fn vim_search_feedback_match_data(
    text: &str,
    cursor: usize,
    query: &str,
    forward: bool,
    pending: bool,
) -> (Option<(usize, usize)>, Option<usize>, usize) {
    let ranges = vim_search_match_ranges(text, query);
    let match_count = ranges.len();
    let target_range = if pending {
        vim_find_search(text, cursor, query, forward).map(|start| (start, start + query.len()))
    } else {
        let cursor = cursor.min(text.len());
        ranges.iter().copied().find(|(start, _)| *start == cursor)
    };
    let current_match_index = target_range
        .and_then(|target| ranges.iter().position(|range| *range == target))
        .map(|idx| idx + 1);

    (target_range, current_match_index, match_count)
}

fn vim_search_match_ranges(text: &str, query: &str) -> Vec<(usize, usize)> {
    if query.is_empty() || text.is_empty() {
        return Vec::new();
    }

    let mut ranges = Vec::new();
    let mut local_start = 0usize;
    while local_start < text.len() {
        let Some(offset) = text[local_start..].find(query) else {
            break;
        };
        let start = local_start + offset;
        let end = start + query.len();
        ranges.push((start, end));
        local_start = end;
    }
    ranges
}

pub(crate) fn vim_find_search(
    text: &str,
    cursor: usize,
    query: &str,
    forward: bool,
) -> Option<usize> {
    if query.is_empty() || text.is_empty() {
        return None;
    }
    if forward {
        let start = next_char_boundary(text, cursor.min(text.len()));
        text[start..]
            .find(query)
            .map(|offset| start + offset)
            .or_else(|| text[..start].find(query))
    } else {
        let start = clamp_to_char_boundary(text, cursor);
        text[..start]
            .rfind(query)
            .or_else(|| text[start..].rfind(query).map(|offset| start + offset))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum TextAreaVimPending {
    G,
    Register,
    MarkSet,
    MarkJump {
        linewise: bool,
    },
    Operator {
        op: VimOperator,
        count: usize,
        g_pending: bool,
    },
    TextObject {
        op: VimOperator,
        count: usize,
        around: bool,
    },
    Search {
        forward: bool,
        query: String,
        cursor: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VimOperator {
    Delete,
    Yank,
    Change,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VimSearchState {
    pub query: Option<String>,
    pub forward: bool,
    pub visible: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct VimRegisters {
    pub values: BTreeMap<char, VimRegisterValue>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VimRegisterValue {
    pub text: String,
    pub linewise: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VimRepeatChange {
    Operator {
        op: VimOperator,
        target: VimRepeatTarget,
        register: Option<char>,
    },
    Change {
        target: VimRepeatTarget,
        register: Option<char>,
        inserted: String,
    },
    DeleteChar {
        backward: bool,
        count: usize,
        register: Option<char>,
    },
    Paste {
        before: bool,
        register: Option<char>,
    },
    OpenLine {
        above: bool,
        inserted: String,
    },
    Insert {
        kind: VimInsertKind,
        inserted: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VimInsertSession {
    pub origin: VimInsertOrigin,
    pub text_before: String,
    pub insert_at: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VimInsertOrigin {
    Change {
        target: VimRepeatTarget,
        register: Option<char>,
    },
    OpenLine {
        above: bool,
    },
    Insert {
        kind: VimInsertKind,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VimInsertKind {
    Insert,
    Append,
    InsertLineStart,
    AppendLineEnd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum VimRepeatTarget {
    Motion {
        motion: VimMotion,
        count: usize,
    },
    Line {
        count: usize,
    },
    TextObject {
        object: VimTextObject,
        around: bool,
        count: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VimMotion {
    Left,
    Right,
    Up,
    Down,
    WordForward,
    WordBackward,
    WordEnd,
    BigWordForward,
    BigWordBackward,
    BigWordEnd,
    LineStart,
    LineEnd,
    GotoLastLine,
    GotoLine(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VimTextObject {
    Word,
    BigWord,
    Paragraph,
    SingleQuote,
    DoubleQuote,
    Backtick,
    Paren,
    Bracket,
    Brace,
    Angle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LineBounds {
    pub start: usize,
    pub end: usize,
}

pub(crate) fn line_bounds_at(text: &str, cursor: usize) -> LineBounds {
    let cursor = clamp_to_char_boundary(text, cursor);
    LineBounds {
        start: line_start_at(text, cursor),
        end: line_end_at(text, cursor),
    }
}

/// Byte offset of the start of the line containing `cursor`.
///
/// Re-exported publicly as [`crate::text_motion::line_start_at`].
pub fn line_start_at(text: &str, cursor: usize) -> usize {
    let cursor = clamp_to_char_boundary(text, cursor);
    text[..cursor].rfind('\n').map_or(0, |idx| idx + 1)
}

/// Byte offset one past the end of the line containing `cursor` (exclusive of the newline).
///
/// Re-exported publicly as [`crate::text_motion::line_end_at`].
pub fn line_end_at(text: &str, cursor: usize) -> usize {
    let cursor = clamp_to_char_boundary(text, cursor);
    let start = line_start_at(text, cursor);
    text[start..]
        .find('\n')
        .map_or(text.len(), |offset| start + offset)
}

pub(crate) fn line_end_including_newline(text: &str, cursor: usize) -> usize {
    let end = line_end_at(text, cursor);
    if end < text.len() && text[end..].starts_with('\n') {
        end + '\n'.len_utf8()
    } else {
        end
    }
}

pub(crate) fn line_index_at(text: &str, cursor: usize) -> usize {
    let start = line_start_at(text, cursor);
    text[..start].chars().filter(|&ch| ch == '\n').count()
}

/// Byte offset of the first non-blank character in `text[line_start..line_end]`, or `line_end`
/// if the range is entirely spaces/tabs.
///
/// Re-exported publicly as [`crate::text_motion::first_nonblank_in_line`].
pub fn first_nonblank_in_line(text: &str, line_start: usize, line_end: usize) -> usize {
    let start = clamp_to_char_boundary(text, line_start.min(line_end));
    let end = clamp_to_char_boundary(text, line_end.min(text.len()));
    text[start..end]
        .char_indices()
        .find_map(|(offset, ch)| (!matches!(ch, ' ' | '\t')).then_some(start + offset))
        .unwrap_or(end)
}

pub(crate) fn line_start_by_index(text: &str, zero_based_line: usize) -> usize {
    if zero_based_line == 0 {
        return 0;
    }

    let mut current_line = 0;
    let mut last_start = 0;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            current_line += 1;
            last_start = idx + ch.len_utf8();
            if current_line == zero_based_line {
                return last_start;
            }
        }
    }

    last_start
}

pub(crate) fn line_count(text: &str) -> usize {
    text.chars().filter(|&ch| ch == '\n').count() + 1
}

pub(crate) fn line_start_by_one_based_count(text: &str, one_based_line: usize) -> usize {
    let target = one_based_line.max(1).min(line_count(text));
    line_start_by_index(text, target - 1)
}

/// Move forward to the start of the next vim "word" (`w`).
///
/// Re-exported publicly as [`crate::text_motion::word_forward_start`].
pub fn vim_word_forward_start(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_to_char_boundary(text, cursor);
    if idx >= text.len() {
        return text.len();
    }

    if let Some(class) = char_class_at(text, idx).filter(|class| *class != VimCharClass::Whitespace)
    {
        idx = skip_run_forward(text, idx, class);
    }

    skip_whitespace_forward(text, idx)
}

/// Move backward to the start of the previous vim "word" (`b`).
///
/// Re-exported publicly as [`crate::text_motion::word_backward_start`].
pub fn vim_word_backward_start(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_to_char_boundary(text, cursor);
    if idx == 0 {
        return 0;
    }

    idx = skip_whitespace_backward(text, idx);
    if idx == 0 {
        return 0;
    }

    let Some((prev_idx, prev_ch)) = prev_char(text, idx) else {
        return 0;
    };
    let class = classify_char(prev_ch);
    idx = prev_idx;
    while let Some((candidate_idx, candidate_ch)) = prev_char(text, idx) {
        if classify_char(candidate_ch) != class {
            break;
        }
        idx = candidate_idx;
    }
    idx
}

/// Move to the end of the current or next vim "word" (`e`).
///
/// Re-exported publicly as [`crate::text_motion::word_end`].
pub fn vim_word_end(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_to_char_boundary(text, cursor);
    if idx >= text.len() {
        return text.len();
    }

    if matches!(char_class_at(text, idx), Some(VimCharClass::Whitespace)) {
        idx = skip_whitespace_forward(text, idx);
    }

    let Some(class) = char_class_at(text, idx) else {
        return text.len();
    };
    skip_run_forward(text, idx, class)
}

/// Move forward to the start of the next vim WORD (`W`; whitespace-delimited, punctuation
/// included in the run).
///
/// Re-exported publicly as [`crate::text_motion::big_word_forward_start`].
pub fn vim_big_word_forward_start(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_to_char_boundary(text, cursor);
    if idx >= text.len() {
        return text.len();
    }

    if char_at(text, idx).is_some_and(is_big_word_char) {
        idx = skip_big_word_forward(text, idx);
    }

    skip_whitespace_forward(text, idx)
}

/// Move backward to the start of the previous vim WORD (`B`).
///
/// Re-exported publicly as [`crate::text_motion::big_word_backward_start`].
pub fn vim_big_word_backward_start(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_to_char_boundary(text, cursor);
    if idx == 0 {
        return 0;
    }

    idx = skip_whitespace_backward(text, idx);
    if idx == 0 {
        return 0;
    }

    let Some((prev_idx, prev_ch)) = prev_char(text, idx) else {
        return 0;
    };
    if !is_big_word_char(prev_ch) {
        return prev_idx;
    }

    idx = prev_idx;
    while let Some((candidate_idx, candidate_ch)) = prev_char(text, idx) {
        if !is_big_word_char(candidate_ch) {
            break;
        }
        idx = candidate_idx;
    }
    idx
}

/// Move to the end of the current or next vim WORD (`E`).
///
/// Re-exported publicly as [`crate::text_motion::big_word_end`].
pub fn vim_big_word_end(text: &str, cursor: usize) -> usize {
    let mut idx = clamp_to_char_boundary(text, cursor);
    if idx >= text.len() {
        return text.len();
    }

    if char_at(text, idx).is_some_and(char::is_whitespace) {
        idx = skip_whitespace_forward(text, idx);
    }

    skip_big_word_forward(text, idx)
}

fn clamp_to_char_boundary(text: &str, cursor: usize) -> usize {
    let mut cursor = cursor.min(text.len());
    while !text.is_char_boundary(cursor) {
        cursor -= 1;
    }
    cursor
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VimCharClass {
    Whitespace,
    Word,
    Punctuation,
}

fn classify_char(ch: char) -> VimCharClass {
    if ch.is_whitespace() {
        VimCharClass::Whitespace
    } else if ch.is_alphanumeric() || ch == '_' {
        VimCharClass::Word
    } else {
        VimCharClass::Punctuation
    }
}

fn char_class_at(text: &str, idx: usize) -> Option<VimCharClass> {
    text[idx..].chars().next().map(classify_char)
}

fn char_at(text: &str, idx: usize) -> Option<char> {
    text[idx..].chars().next()
}

fn is_big_word_char(ch: char) -> bool {
    !ch.is_whitespace()
}

fn next_char_index(text: &str, idx: usize) -> usize {
    text[idx..]
        .chars()
        .next()
        .map_or(text.len(), |ch| idx + ch.len_utf8())
}

fn prev_char(text: &str, idx: usize) -> Option<(usize, char)> {
    text[..idx].char_indices().next_back()
}

fn skip_whitespace_forward(text: &str, mut idx: usize) -> usize {
    while idx < text.len() && matches!(char_class_at(text, idx), Some(VimCharClass::Whitespace)) {
        idx = next_char_index(text, idx);
    }
    idx
}

fn skip_whitespace_backward(text: &str, mut idx: usize) -> usize {
    while let Some((prev_idx, ch)) = prev_char(text, idx) {
        if classify_char(ch) != VimCharClass::Whitespace {
            break;
        }
        idx = prev_idx;
    }
    idx
}

fn skip_run_forward(text: &str, mut idx: usize, class: VimCharClass) -> usize {
    while idx < text.len() && char_class_at(text, idx) == Some(class) {
        idx = next_char_index(text, idx);
    }
    idx
}

fn skip_big_word_forward(text: &str, mut idx: usize) -> usize {
    while idx < text.len() && char_at(text, idx).is_some_and(is_big_word_char) {
        idx = next_char_index(text, idx);
    }
    idx
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_defaults_to_normal_and_manages_mode_transitions() {
        let mut state = TextAreaVimState::default();
        assert_eq!(state.mode, TextAreaVimMode::Normal);
        assert_eq!(state.count, None);
        assert_eq!(state.pending, None);
        assert_eq!(state.visual_anchor, None);
        assert_eq!(state.visual_line_head, None);

        assert!(!state.set_mode(TextAreaVimMode::Normal));
        state.count = Some(2);
        state.pending = Some(TextAreaVimPending::G);
        state.visual_anchor = Some(3);
        state.visual_line_head = Some(3);
        assert!(state.set_mode(TextAreaVimMode::Visual));
        assert_eq!(state.visual_anchor, Some(3));
        assert_eq!(state.visual_line_head, None);
        state.count = Some(2);
        state.pending = Some(TextAreaVimPending::G);
        state.visual_line_head = Some(4);
        assert!(state.set_mode(TextAreaVimMode::Insert));
        assert_eq!(state.count, None);
        assert_eq!(state.pending, None);
        assert_eq!(state.visual_anchor, None);
        assert_eq!(state.visual_line_head, None);

        state.visual_anchor = Some(4);
        assert!(!state.set_mode(TextAreaVimMode::Insert));
        assert_eq!(state.visual_anchor, None);
    }

    #[test]
    fn count_accumulation_is_capped_and_clearable() {
        let mut state = TextAreaVimState::default();
        assert_eq!(state.push_count_digit(1), 1);
        assert_eq!(state.push_count_digit(2), 12);
        for _ in 0..12 {
            state.push_count_digit(9);
        }
        assert_eq!(state.count, Some(VIM_COUNT_CAP));
        state.pending = Some(TextAreaVimPending::G);
        state.clear_count_pending();
        assert_eq!(state.count, None);
        assert_eq!(state.pending, None);
    }

    #[test]
    fn line_helpers_handle_empty_and_trailing_lines() {
        assert_eq!(line_count(""), 1);
        assert_eq!(line_bounds_at("", 10), LineBounds { start: 0, end: 0 });

        let text = "one\n\nthree\n";
        assert_eq!(line_count(text), 4);
        assert_eq!(line_bounds_at(text, 4), LineBounds { start: 4, end: 4 });
        assert_eq!(
            line_bounds_at(text, text.len()),
            LineBounds {
                start: text.len(),
                end: text.len()
            }
        );
        assert_eq!(line_start_by_index(text, 0), 0);
        assert_eq!(line_start_by_index(text, 1), 4);
        assert_eq!(line_start_by_index(text, 2), 5);
        assert_eq!(line_start_by_index(text, 3), text.len());
        assert_eq!(line_start_by_index(text, 99), text.len());
        assert_eq!(line_end_including_newline(text, 0), 4);
        assert_eq!(line_end_including_newline(text, 4), 5);
        assert_eq!(line_index_at(text, 0), 0);
        assert_eq!(line_index_at(text, 4), 1);
        assert_eq!(line_index_at(text, text.len()), 3);
    }

    #[test]
    fn line_helpers_are_utf8_safe() {
        let text = "αβ\n  Ж";
        let inside_alpha = 1;
        assert_eq!(line_start_at(text, inside_alpha), 0);
        assert_eq!(line_end_at(text, inside_alpha), "αβ".len());
        let second_start = "αβ\n".len();
        assert_eq!(
            first_nonblank_in_line(text, second_start, text.len()),
            "αβ\n  ".len()
        );
    }

    #[test]
    fn search_clamps_cursor_inside_unicode_characters() {
        let text = "części ewaluacyjnej części";
        let cursor_inside_s = 5;
        assert!(!text.is_char_boundary(cursor_inside_s));

        assert_eq!(vim_find_search(text, cursor_inside_s, "cz", false), Some(0));
        assert_eq!(vim_find_search(text, cursor_inside_s, "ew", true), Some(9));
    }

    #[test]
    fn counted_one_based_lines_clamp_to_existing_lines() {
        let text = "a\nb\nc";
        assert_eq!(line_start_by_one_based_count(text, 0), 0);
        assert_eq!(line_start_by_one_based_count(text, 1), 0);
        assert_eq!(line_start_by_one_based_count(text, 2), 2);
        assert_eq!(line_start_by_one_based_count(text, 99), 4);
    }

    #[test]
    fn word_forward_skips_words_punctuation_and_whitespace() {
        let text = "foo, bar  baz";
        assert_eq!(vim_word_forward_start(text, 0), 3);
        assert_eq!(vim_word_forward_start(text, 1), 3);
        assert_eq!(vim_word_forward_start(text, 3), 5);
        assert_eq!(vim_word_forward_start(text, 4), 5);
        assert_eq!(vim_word_forward_start(text, 8), 10);
        assert_eq!(vim_word_forward_start(text, text.len()), text.len());
    }

    #[test]
    fn word_backward_finds_run_starts() {
        let text = "foo, bar  baz";
        assert_eq!(vim_word_backward_start(text, 0), 0);
        assert_eq!(vim_word_backward_start(text, 2), 0);
        assert_eq!(vim_word_backward_start(text, 5), 3);
        assert_eq!(vim_word_backward_start(text, 9), 5);
        assert_eq!(vim_word_backward_start(text, text.len()), 10);
    }

    #[test]
    fn word_end_finds_current_or_next_run_end() {
        let text = " foo:: bar";
        assert_eq!(vim_word_end(text, 0), 4);
        assert_eq!(vim_word_end(text, 2), 4);
        assert_eq!(vim_word_end(text, 4), 6);
        assert_eq!(vim_word_end(text, 6), 10);
        assert_eq!(vim_word_end(text, text.len()), text.len());
    }

    #[test]
    fn word_helpers_treat_unicode_words_and_punctuation_runs_separately() {
        let text = "åβ -- γ_delta";
        assert_eq!(vim_word_forward_start(text, 0), "åβ ".len());
        assert_eq!(vim_word_forward_start(text, "åβ ".len()), "åβ -- ".len());
        assert_eq!(vim_word_backward_start(text, text.len()), "åβ -- ".len());
        assert_eq!(vim_word_end(text, "åβ ".len()), "åβ --".len());
    }

    #[test]
    fn big_word_forward_skips_non_whitespace_runs() {
        let text = "open-code next foo.bar/baz";

        assert_eq!(vim_big_word_forward_start(text, 0), "open-code ".len());
        assert_eq!(
            vim_big_word_forward_start(text, "open".len()),
            "open-code ".len()
        );
        assert_eq!(
            vim_big_word_forward_start(text, "open-code ".len()),
            "open-code next ".len()
        );
        assert_eq!(vim_big_word_forward_start(text, text.len()), text.len());
    }

    #[test]
    fn big_word_backward_finds_non_whitespace_run_starts() {
        let text = "open-code next foo.bar";

        assert_eq!(vim_big_word_backward_start(text, 0), 0);
        assert_eq!(vim_big_word_backward_start(text, "open".len()), 0);
        assert_eq!(vim_big_word_backward_start(text, "open-code ".len()), 0);
        assert_eq!(
            vim_big_word_backward_start(text, text.len()),
            "open-code next ".len()
        );
    }

    #[test]
    fn big_word_end_finds_non_whitespace_run_ends() {
        let text = " open-code next";

        assert_eq!(vim_big_word_end(text, 0), " open-code".len());
        assert_eq!(vim_big_word_end(text, 3), " open-code".len());
        assert_eq!(
            vim_big_word_end(text, " open-code".len()),
            " open-code next".len()
        );
        assert_eq!(vim_big_word_end(text, text.len()), text.len());
    }

    #[test]
    fn big_word_helpers_treat_unicode_and_punctuation_as_one_word() {
        let text = "åβ-γ --- next";

        assert_eq!(vim_big_word_forward_start(text, 0), "åβ-γ ".len());
        assert_eq!(vim_big_word_end(text, 0), "åβ-γ".len());
        assert_eq!(vim_big_word_end(text, "åβ-γ ".len()), "åβ-γ ---".len());
        assert_eq!(
            vim_big_word_backward_start(text, text.len()),
            "åβ-γ --- ".len()
        );
    }
}
