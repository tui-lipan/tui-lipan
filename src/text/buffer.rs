use crate::text::edit::{TextEdit, TextEditEvent, TextEditFields, TextEditKind, TextTarget};
use crate::utils::text::clamp_cursor;
use crate::utils::text::{
    next_char_boundary, prev_char_boundary, word_boundary_left, word_boundary_right,
};
use undo::Record;

const DEFAULT_HISTORY_LIMIT: usize = 1000;

type CoreEdit = TextEdit<TextBufferCore>;

/// Shared text buffer backing both `TextInput` (single-line) and `TextEditor` (multi-line).
///
/// Owns the UTF-8 text, byte-index cursor, optional selection anchor, undo/redo history,
/// and last-edit event. All cursor-movement, selection, editing, and history operations
/// that are identical across single-line and multi-line modes live here.
#[derive(Clone, Debug)]
pub struct TextBufferCore {
    pub(crate) text: String,
    pub(crate) cursor: usize,
    pub(crate) anchor: Option<usize>,
    pub(crate) history: Record<CoreEdit>,
    pub(crate) last_edit: Option<TextEditEvent>,
}

impl Default for TextBufferCore {
    fn default() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            anchor: None,
            history: Self::fresh_history(),
            last_edit: None,
        }
    }
}

impl PartialEq for TextBufferCore {
    fn eq(&self, other: &Self) -> bool {
        self.text == other.text && self.cursor == other.cursor && self.anchor == other.anchor
    }
}

impl Eq for TextBufferCore {}

impl TextTarget for TextBufferCore {
    fn with_parts_mut<R>(
        &mut self,
        f: impl FnOnce(&mut String, &mut usize, &mut Option<usize>) -> R,
    ) -> R {
        f(&mut self.text, &mut self.cursor, &mut self.anchor)
    }
}

impl TextBufferCore {
    pub(crate) fn fresh_history() -> Record<CoreEdit> {
        Record::builder().limit(DEFAULT_HISTORY_LIMIT).build()
    }

    // ── Accessors ────────────────────────────────────────────────────────

    /// Return the current text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Return the cursor byte index.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// Return the selection anchor, if any.
    pub fn anchor(&self) -> Option<usize> {
        self.anchor
    }

    /// Return the selection range as (start, end) byte indices, if there is a selection.
    pub fn selection(&self) -> Option<(usize, usize)> {
        self.anchor.map(|anchor| {
            let anchor = clamp_cursor(&self.text, anchor);
            let cursor = clamp_cursor(&self.text, self.cursor);
            if anchor <= cursor {
                (anchor, cursor)
            } else {
                (cursor, anchor)
            }
        })
    }

    /// Return the selected text, if any.
    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|(start, end)| &self.text[start..end])
    }

    // ── History ──────────────────────────────────────────────────────────

    /// Returns true if there is something to undo.
    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    /// Returns true if there is something to redo.
    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Undo the most recent edit.
    pub fn undo(&mut self) -> bool {
        let mut history = std::mem::take(&mut self.history);
        let changed = history.undo(self).is_some();
        self.history = history;
        changed
    }

    /// Redo the most recently undone edit.
    pub fn redo(&mut self) -> bool {
        let mut history = std::mem::take(&mut self.history);
        let changed = history.redo(self).is_some();
        self.history = history;
        changed
    }

    /// Clear all undo/redo history without changing the text.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub(crate) fn reset_history(&mut self) {
        self.history = Self::fresh_history();
    }

    // ── Sync / setters ───────────────────────────────────────────────────

    pub(crate) fn sync_from(&mut self, text: &str, cursor: usize, anchor: Option<usize>) {
        let text_changed = self.text != text;
        if text_changed {
            self.text = text.to_string();
        }
        self.cursor = clamp_cursor(&self.text, cursor);
        self.anchor = anchor.map(|a| clamp_cursor(&self.text, a));
        if text_changed {
            self.reset_history();
        }
    }

    pub(crate) fn sync_external_edit_from(
        &mut self,
        text: &str,
        cursor_before: usize,
        anchor_before: Option<usize>,
        cursor_after: usize,
        anchor_after: Option<usize>,
    ) {
        let deleted = self.text.clone();
        let cursor_before = clamp_cursor(&deleted, cursor_before);
        let anchor_before = anchor_before.map(|anchor| clamp_cursor(&deleted, anchor));
        let cursor_after = clamp_cursor(text, cursor_after);
        let anchor_after = anchor_after.map(|anchor| clamp_cursor(text, anchor));
        let edit = TextEdit::new(TextEditFields {
            start: 0,
            deleted,
            inserted: text.to_string(),
            cursor_before,
            anchor_before,
            cursor_after,
            anchor_after,
            kind: TextEditKind::Replace,
        });
        self.record_edit(edit);
        self.last_edit = None;
    }

    /// Set the text, clamping the cursor and clearing selection.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = clamp_cursor(&self.text, self.cursor);
        self.anchor = self.anchor.map(|a| clamp_cursor(&self.text, a));
    }

    /// Set the cursor position (byte index), clearing selection.
    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = clamp_cursor(&self.text, cursor);
        self.anchor = None;
    }

    /// Set the cursor position without clearing selection.
    pub fn set_cursor_keep_anchor(&mut self, cursor: usize) {
        self.cursor = clamp_cursor(&self.text, cursor);
    }

    /// Set the selection anchor.
    pub fn set_anchor(&mut self, anchor: Option<usize>) {
        self.anchor = anchor.map(|a| clamp_cursor(&self.text, a));
    }

    // ── Selection ────────────────────────────────────────────────────────

    /// Select all text.
    pub fn select_all(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }
        self.anchor = Some(0);
        self.cursor = self.text.len();
        true
    }

    /// Clear selection without moving cursor.
    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    /// If selection is empty (anchor == cursor), clear the anchor.
    pub(crate) fn collapse_empty_selection(&mut self) {
        if self.anchor == Some(self.cursor) {
            self.anchor = None;
        }
    }

    pub(crate) fn normalize_cursor_and_anchor(&mut self) {
        self.cursor = clamp_cursor(&self.text, self.cursor);
        self.anchor = self.anchor.map(|anchor| clamp_cursor(&self.text, anchor));
    }

    // ── Edit recording ───────────────────────────────────────────────────

    pub(crate) fn record_edit(&mut self, edit: CoreEdit) {
        self.last_edit = Some(TextEditEvent::from(&edit));
        let mut history = std::mem::take(&mut self.history);
        history.edit(self, edit);
        self.history = history;
    }

    pub(crate) fn take_last_edit(&mut self) -> Option<TextEditEvent> {
        self.last_edit.take()
    }

    pub(crate) fn replace_range(
        &mut self,
        start: usize,
        end: usize,
        inserted: &str,
        kind: TextEditKind,
    ) -> bool {
        self.normalize_cursor_and_anchor();
        let start = clamp_cursor(&self.text, start);
        let end = clamp_cursor(&self.text, end);
        let (start, end) = if start <= end {
            (start, end)
        } else {
            (end, start)
        };

        if start == end && inserted.is_empty() {
            return false;
        }

        let deleted = self.text[start..end].to_string();
        let cursor_after = start.saturating_add(inserted.len());
        let edit = TextEdit::new(TextEditFields {
            start,
            deleted,
            inserted: inserted.to_owned(),
            cursor_before: self.cursor,
            anchor_before: self.anchor,
            cursor_after,
            anchor_after: None,
            kind,
        });
        self.record_edit(edit);
        true
    }

    pub(crate) fn clear_preserving_history(&mut self) -> bool {
        if self.text.is_empty() {
            let changed = self.cursor != 0 || self.anchor.is_some();
            self.cursor = 0;
            self.anchor = None;
            return changed;
        }

        let len = self.text.len();
        self.replace_range(0, len, "", TextEditKind::Replace)
    }

    /// Delete the current selection if any, returning true if something was deleted.
    pub(crate) fn delete_selection(&mut self) -> bool {
        if let Some((start, end)) = self.selection() {
            self.replace_range(start, end, "", TextEditKind::Replace)
        } else {
            false
        }
    }

    // ── Character-level editing ──────────────────────────────────────────

    /// Insert a character at the cursor, replacing any selection.
    pub fn insert_char(&mut self, c: char) -> bool {
        let (start, end, kind) = if let Some((start, end)) = self.selection() {
            (start, end, TextEditKind::Replace)
        } else {
            (self.cursor, self.cursor, TextEditKind::Insert)
        };
        let mut buf = [0u8; 4];
        let s = c.encode_utf8(&mut buf);
        self.replace_range(start, end, s, kind)
    }

    /// Delete the previous character or selection.
    pub fn backspace(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        if self.delete_selection() {
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        let start = prev_char_boundary(&self.text, self.cursor);
        self.replace_range(start, self.cursor, "", TextEditKind::DeleteBackspace)
    }

    /// Delete the character at the cursor or selection.
    pub fn delete(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        if self.delete_selection() {
            return true;
        }
        if self.cursor >= self.text.len() {
            return false;
        }
        let end = next_char_boundary(&self.text, self.cursor);
        self.replace_range(self.cursor, end, "", TextEditKind::DeleteForward)
    }

    /// Delete the previous word.
    pub fn delete_word_left(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        if self.delete_selection() {
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        let target = word_boundary_left(&self.text, self.cursor);
        self.replace_range(target, self.cursor, "", TextEditKind::DeleteBackspace)
    }

    /// Delete the next word.
    pub fn delete_word_right(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        if self.delete_selection() {
            return true;
        }
        if self.cursor >= self.text.len() {
            return false;
        }
        let target = word_boundary_right(&self.text, self.cursor);
        self.replace_range(self.cursor, target, "", TextEditKind::DeleteForward)
    }

    // ── Cursor movement ──────────────────────────────────────────────────

    /// Move one character left, clearing selection.
    pub fn move_left(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        // If there's a selection, move to the start of it
        if let Some((start, _)) = self.selection() {
            self.cursor = start;
            self.anchor = None;
            return true;
        }
        if self.cursor == 0 {
            return false;
        }
        let new = prev_char_boundary(&self.text, self.cursor);
        if new == self.cursor {
            return false;
        }
        self.cursor = new;
        true
    }

    /// Move one character right, clearing selection.
    pub fn move_right(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        // If there's a selection, move to the end of it
        if let Some((_, end)) = self.selection() {
            self.cursor = end;
            self.anchor = None;
            return true;
        }
        if self.cursor >= self.text.len() {
            return false;
        }
        let new = next_char_boundary(&self.text, self.cursor);
        if new == self.cursor {
            return false;
        }
        self.cursor = new;
        true
    }

    /// Extend selection one character left.
    pub fn select_left(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        if self.cursor == 0 {
            return false;
        }
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        let new = prev_char_boundary(&self.text, self.cursor);
        if new == self.cursor {
            return false;
        }
        self.cursor = new;
        self.collapse_empty_selection();
        true
    }

    /// Extend selection one character right.
    pub fn select_right(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        if self.cursor >= self.text.len() {
            return false;
        }
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        let new = next_char_boundary(&self.text, self.cursor);
        if new == self.cursor {
            return false;
        }
        self.cursor = new;
        self.collapse_empty_selection();
        true
    }

    /// Move to the beginning of the previous word, clearing selection.
    pub fn move_word_left(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        self.anchor = None;
        let old = self.cursor;
        self.cursor = word_boundary_left(&self.text, self.cursor);
        self.cursor != old
    }

    /// Move to the end of the next word, clearing selection.
    pub fn move_word_right(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        self.anchor = None;
        let old = self.cursor;
        self.cursor = word_boundary_right(&self.text, self.cursor);
        self.cursor != old
    }

    /// Extend selection to the beginning of the previous word.
    pub fn select_word_left(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        let old = self.cursor;
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = word_boundary_left(&self.text, self.cursor);
        self.collapse_empty_selection();
        self.cursor != old
    }

    /// Extend selection to the end of the next word.
    pub fn select_word_right(&mut self) -> bool {
        self.normalize_cursor_and_anchor();
        let old = self.cursor;
        if self.anchor.is_none() {
            self.anchor = Some(self.cursor);
        }
        self.cursor = word_boundary_right(&self.text, self.cursor);
        self.collapse_empty_selection();
        self.cursor != old
    }
}
