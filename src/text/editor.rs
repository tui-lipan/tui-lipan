//! Multi-line text editor with selection support.

use crate::clipboard::ImageContent;
use crate::core::event::KeyEvent;
use crate::text::buffer::TextBufferCore;
use crate::text::edit::TextEditKind;
use crate::utils::text::clamp_cursor;
use crate::widgets::{TextAreaEvent, TextAreaSentinel};
use std::ops::{Deref, DerefMut};

const SENTINEL_SNAPSHOT_LIMIT: usize = 1000;

/// A multi-line text editor with selection support.
///
/// The cursor is stored as a byte index into the UTF-8 string and is always kept on a
/// character boundary. Selection is represented by an optional anchor position.
///
/// All shared text-buffer operations (cursor movement, selection, word
/// boundaries, undo/redo, etc.) are provided via [`Deref`]/[`DerefMut`] to the
/// inner `TextBufferCore`.
#[derive(Clone, Debug)]
pub struct TextEditor {
    pub(crate) core: TextBufferCore,
    pending_external_paste: Option<PendingExternalPaste>,
    image_snapshots: Vec<ImageSnapshot>,
    sentinel_snapshots: Vec<SentinelSnapshot>,
    /// Sticky visual column used for wrap-aware vertical navigation.
    /// Cleared whenever the cursor moves due to anything other than
    /// visual up/down (including external cursor sync, edits, horizontal
    /// movement, clicks). Set by the textarea key handler before performing
    /// visual navigation.
    pub(crate) visual_nav_col: Option<usize>,
}

#[derive(Clone, Debug)]
struct PendingExternalPaste {
    text_before: String,
    cursor_before: usize,
    anchor_before: Option<usize>,
}

#[derive(Clone, Debug)]
struct ImageSnapshot {
    text: String,
    images: Vec<ImageContent>,
}

#[derive(Clone, Debug)]
struct SentinelSnapshot {
    text: String,
    sentinels: Vec<TextAreaSentinel>,
}

impl Default for TextEditor {
    fn default() -> Self {
        Self::new("")
    }
}

impl PartialEq for TextEditor {
    fn eq(&self, other: &Self) -> bool {
        self.core == other.core
    }
}

impl Eq for TextEditor {}

impl Deref for TextEditor {
    type Target = TextBufferCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for TextEditor {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl TextEditor {
    /// Create a new editor with the cursor at the start.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            core: TextBufferCore {
                text,
                cursor: 0,
                anchor: None,
                history: TextBufferCore::fresh_history(),
                last_edit: None,
            },
            pending_external_paste: None,
            image_snapshots: Vec::new(),
            sentinel_snapshots: Vec::new(),
            visual_nav_col: None,
        }
    }

    /// Sticky visual column used for wrap-aware vertical navigation.
    pub fn visual_nav_col(&self) -> Option<usize> {
        self.visual_nav_col
    }

    /// Set or clear the sticky visual column for wrap-aware vertical navigation.
    pub fn set_visual_nav_col(&mut self, col: Option<usize>) {
        self.visual_nav_col = col;
    }

    /// Reconcile the editor with externally-supplied text/cursor/anchor.
    ///
    /// Clears the sticky visual column when the externally-supplied cursor
    /// differs from our last known position, so a subsequent up/down arrow
    /// recomputes the column instead of using a stale one.
    pub(crate) fn sync_from(&mut self, text: &str, cursor: usize, anchor: Option<usize>) {
        let prev_cursor = self.core.cursor;
        let text_changed = self.core.text != text;
        let pending_external_paste = self.pending_external_paste.take();
        if text_changed
            && let Some(pending) = pending_external_paste
            && self.core.text == pending.text_before
        {
            self.core.sync_external_edit_from(
                text,
                pending.cursor_before,
                pending.anchor_before,
                cursor,
                anchor,
            );
        } else {
            self.core.sync_from(text, cursor, anchor);
        }
        if text_changed || self.core.cursor != prev_cursor {
            self.visual_nav_col = None;
        }
    }

    pub(crate) fn expect_external_paste_change(&mut self) {
        self.core.normalize_cursor_and_anchor();
        self.pending_external_paste = Some(PendingExternalPaste {
            text_before: self.core.text().to_owned(),
            cursor_before: self.core.cursor(),
            anchor_before: self.core.anchor(),
        });
    }

    pub(crate) fn remember_text_area_sentinels(
        &mut self,
        text: &str,
        sentinels: &[TextAreaSentinel],
    ) {
        if sentinels.is_empty() {
            return;
        }

        if let Some(pos) = self
            .sentinel_snapshots
            .iter()
            .position(|snapshot| snapshot.text == text)
        {
            self.sentinel_snapshots.remove(pos);
        }
        self.sentinel_snapshots.push(SentinelSnapshot {
            text: text.to_owned(),
            sentinels: sentinels.to_vec(),
        });
        if self.sentinel_snapshots.len() > SENTINEL_SNAPSHOT_LIMIT {
            self.sentinel_snapshots.remove(0);
        }
    }

    pub(crate) fn remembered_text_area_sentinels(
        &self,
        text: &str,
    ) -> Option<Vec<TextAreaSentinel>> {
        self.sentinel_snapshots
            .iter()
            .rev()
            .find(|snapshot| snapshot.text == text)
            .map(|snapshot| snapshot.sentinels.clone())
    }

    pub(crate) fn remember_text_area_images(&mut self, text: &str, images: &[ImageContent]) {
        if images.is_empty() {
            return;
        }

        if let Some(pos) = self
            .image_snapshots
            .iter()
            .position(|snapshot| snapshot.text == text)
        {
            self.image_snapshots.remove(pos);
        }
        self.image_snapshots.push(ImageSnapshot {
            text: text.to_owned(),
            images: images.to_vec(),
        });
        if self.image_snapshots.len() > SENTINEL_SNAPSHOT_LIMIT {
            self.image_snapshots.remove(0);
        }
    }

    pub(crate) fn remembered_text_area_images(&self, text: &str) -> Option<Vec<ImageContent>> {
        self.image_snapshots
            .iter()
            .rev()
            .find(|snapshot| snapshot.text == text)
            .map(|snapshot| snapshot.images.clone())
    }

    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }
        let (start, end) = self
            .core
            .selection()
            .unwrap_or((self.core.cursor, self.core.cursor));
        self.core
            .replace_range(start, end, text, TextEditKind::Replace)
    }

    /// Clear all text while preserving undo/redo history.
    pub fn clear(&mut self) -> bool {
        let changed = self.core.clear_preserving_history();
        if changed {
            self.visual_nav_col = None;
        }
        changed
    }

    /// Apply a [`TextAreaEvent`] to this state bundle.
    pub fn apply(&mut self, ev: &TextAreaEvent) {
        self.core.text = ev.value.to_string();
        self.core.cursor = clamp_cursor(&self.core.text, ev.cursor);
        self.core.anchor = ev
            .anchor
            .map(|anchor| clamp_cursor(&self.core.text, anchor));
    }

    /// Handle a key event.
    ///
    /// Returns `true` if the state changed (text, cursor, or selection).
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.handle_key_with(key, crate::app::input::keymap::default_keymap())
    }

    pub(crate) fn handle_key_with(
        &mut self,
        key: KeyEvent,
        keymap: &crate::app::input::keymap::Keymap,
    ) -> bool {
        use crate::app::input::keymap::Action;

        match keymap.resolve_action(key) {
            Action::MoveLeft => self.core.move_left(),
            Action::MoveRight => self.core.move_right(),
            Action::MoveUp => self.move_up(),
            Action::MoveDown => self.move_down(),
            Action::MoveWordLeft => self.core.move_word_left(),
            Action::MoveWordRight => self.core.move_word_right(),
            Action::MoveHome => self.move_home(),
            Action::MoveEnd => self.move_end(),

            Action::SelectLeft => self.core.select_left(),
            Action::SelectRight => self.core.select_right(),
            Action::SelectUp => self.select_up(),
            Action::SelectDown => self.select_down(),
            Action::SelectWordLeft => self.core.select_word_left(),
            Action::SelectWordRight => self.core.select_word_right(),
            Action::SelectHome => self.select_home(),
            Action::SelectEnd => self.select_end(),
            Action::SelectAll => self.core.select_all(),

            Action::Backspace => self.core.backspace(),
            Action::Clear => self.clear(),
            Action::Delete => self.core.delete(),
            Action::DeleteWordLeft => self.core.delete_word_left(),
            Action::DeleteWordRight => self.core.delete_word_right(),

            Action::Copy
            | Action::Cut
            | Action::Paste
            | Action::PasteFromSelection
            | Action::CopyImage
            | Action::PasteImage
            | Action::ToggleDevTools
            | Action::Quit => false,
            Action::Undo => self.core.undo(),
            Action::Redo => self.core.redo(),

            Action::InsertChar(c) => self.core.insert_char(c),
            Action::InsertNewline => self.core.insert_char('\n'),

            Action::DismissOverlay | Action::FocusNext | Action::FocusPrev | Action::None => false,
        }
    }

    /// Insert a string at the cursor, replacing any selection.
    pub fn insert_str(&mut self, s: &str) -> bool {
        let (start, end, kind) = if let Some((start, end)) = self.core.selection() {
            (start, end, TextEditKind::Replace)
        } else {
            (self.core.cursor, self.core.cursor, TextEditKind::Insert)
        };
        self.core.replace_range(start, end, s, kind)
    }

    // ── Line-aware home/end ──────────────────────────────────────────────

    /// Move to the start of the current line, clearing selection.
    pub fn move_home(&mut self) -> bool {
        self.core.anchor = None;
        self.core.normalize_cursor_and_anchor();
        let start = self.core.text[..self.core.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        if self.core.cursor == start {
            return false;
        }
        self.core.cursor = start;
        true
    }

    /// Move to the end of the current line, clearing selection.
    pub fn move_end(&mut self) -> bool {
        self.core.anchor = None;
        self.core.normalize_cursor_and_anchor();
        let end = self.core.text[self.core.cursor..]
            .find('\n')
            .map(|i| self.core.cursor + i)
            .unwrap_or(self.core.text.len());
        if self.core.cursor == end {
            return false;
        }
        self.core.cursor = end;
        true
    }

    /// Extend selection to the start of the current line.
    pub fn select_home(&mut self) -> bool {
        self.core.normalize_cursor_and_anchor();
        let start = self.core.text[..self.core.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        if self.core.cursor == start {
            return false;
        }
        if self.core.anchor.is_none() {
            self.core.anchor = Some(self.core.cursor);
        }
        self.core.cursor = start;
        self.core.collapse_empty_selection();
        true
    }

    /// Extend selection to the end of the current line.
    pub fn select_end(&mut self) -> bool {
        self.core.normalize_cursor_and_anchor();
        let end = self.core.text[self.core.cursor..]
            .find('\n')
            .map(|i| self.core.cursor + i)
            .unwrap_or(self.core.text.len());
        if self.core.cursor == end {
            return false;
        }
        if self.core.anchor.is_none() {
            self.core.anchor = Some(self.core.cursor);
        }
        self.core.cursor = end;
        self.core.collapse_empty_selection();
        true
    }

    // ── Vertical movement ────────────────────────────────────────────────

    /// Move up one line, clearing selection.
    pub fn move_up(&mut self) -> bool {
        self.core.anchor = None;
        self.move_up_internal()
    }

    /// Move down one line, clearing selection.
    pub fn move_down(&mut self) -> bool {
        self.core.anchor = None;
        self.move_down_internal()
    }

    /// Extend selection up one line.
    pub fn select_up(&mut self) -> bool {
        if self.core.anchor.is_none() {
            self.core.anchor = Some(self.core.cursor);
        }
        let moved = self.move_up_internal();
        self.core.collapse_empty_selection();
        moved
    }

    /// Extend selection down one line.
    pub fn select_down(&mut self) -> bool {
        if self.core.anchor.is_none() {
            self.core.anchor = Some(self.core.cursor);
        }
        let moved = self.move_down_internal();
        self.core.collapse_empty_selection();
        moved
    }

    fn move_up_internal(&mut self) -> bool {
        self.core.normalize_cursor_and_anchor();
        let line_start = self.core.text[..self.core.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        if line_start == 0 {
            if self.core.cursor == 0 {
                return false;
            }
            self.core.cursor = 0;
            return true;
        }

        let col = self.core.text[line_start..self.core.cursor].chars().count();

        let prev_line_end = line_start - 1;
        let prev_line_start = self.core.text[..prev_line_end]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);

        let prev_line = &self.core.text[prev_line_start..prev_line_end];
        let new_col = col.min(prev_line.chars().count());

        let offset: usize = prev_line.chars().take(new_col).map(|c| c.len_utf8()).sum();
        self.core.cursor = prev_line_start + offset;
        true
    }

    fn move_down_internal(&mut self) -> bool {
        self.core.normalize_cursor_and_anchor();
        let len = self.core.text.len();
        if self.core.cursor >= len {
            return false;
        }

        let line_end = self.core.text[self.core.cursor..]
            .find('\n')
            .map(|i| self.core.cursor + i);

        match line_end {
            Some(end_idx) => {
                let line_start = self.core.text[..self.core.cursor]
                    .rfind('\n')
                    .map(|i| i + 1)
                    .unwrap_or(0);
                let col = self.core.text[line_start..self.core.cursor].chars().count();

                let next_line_start = end_idx + 1;
                if next_line_start >= len {
                    self.core.cursor = len;
                    return true;
                }

                let next_line_end = self.core.text[next_line_start..]
                    .find('\n')
                    .map(|i| next_line_start + i)
                    .unwrap_or(len);

                let next_line = &self.core.text[next_line_start..next_line_end];
                let new_col = col.min(next_line.chars().count());

                let offset: usize = next_line.chars().take(new_col).map(|c| c.len_utf8()).sum();
                self.core.cursor = next_line_start + offset;
                true
            }
            None => {
                if self.core.cursor == len {
                    return false;
                }
                self.core.cursor = len;
                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::widgets::TextAreaEvent;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods::default(),
        }
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        }
    }

    #[test]
    fn undo_redo_multiline() {
        let mut editor = TextEditor::new("");
        assert!(editor.handle_key(key(KeyCode::Char('a'))));
        assert!(editor.handle_key(key(KeyCode::Enter)));
        assert!(editor.handle_key(key(KeyCode::Char('b'))));
        assert_eq!(editor.text(), "a\nb");

        assert!(editor.handle_key(ctrl(KeyCode::Char('z'))));
        assert_eq!(editor.text(), "a\n");

        assert!(editor.handle_key(ctrl(KeyCode::Char('y'))));
        assert_eq!(editor.text(), "a\nb");
    }

    #[test]
    fn undo_redo_word_merges() {
        let mut editor = TextEditor::new("");
        assert!(editor.handle_key(key(KeyCode::Char('a'))));
        assert!(editor.handle_key(key(KeyCode::Char('b'))));
        assert!(editor.handle_key(key(KeyCode::Char('c'))));
        assert_eq!(editor.text(), "abc");

        assert!(editor.handle_key(ctrl(KeyCode::Char('z'))));
        assert_eq!(editor.text(), "");

        assert!(editor.handle_key(ctrl(KeyCode::Char('y'))));
        assert_eq!(editor.text(), "abc");
    }

    #[test]
    fn undo_merges_trailing_space() {
        let mut editor = TextEditor::new("");
        assert!(editor.handle_key(key(KeyCode::Char('a'))));
        assert!(editor.handle_key(key(KeyCode::Char('b'))));
        assert!(editor.handle_key(key(KeyCode::Char(' '))));
        assert_eq!(editor.text(), "ab ");

        assert!(editor.handle_key(ctrl(KeyCode::Char('z'))));
        assert_eq!(editor.text(), "");

        assert!(editor.handle_key(ctrl(KeyCode::Char('y'))));
        assert_eq!(editor.text(), "ab ");
    }

    #[test]
    fn move_down_and_up_through_lines() {
        let mut editor = TextEditor::new("abc\ndef\nghi");
        // cursor starts at 0
        assert_eq!(editor.cursor(), 0);

        // move_down twice -> line 2 (start of "ghi")
        assert!(editor.move_down());
        assert!(editor.move_down());

        // move_up twice -> back to line 0, col 0
        assert!(editor.move_up());
        assert!(editor.move_up());
        assert_eq!(editor.cursor(), 0);
    }

    #[test]
    fn move_down_clamps_column_to_shorter_line() {
        // "abcde" (len 5), "ab" (len 2), "abcde" (len 5)
        let mut editor = TextEditor::new("abcde\nab\nabcde");
        // Place cursor at end of first line (byte 5)
        editor.set_cursor(5);
        assert_eq!(editor.cursor(), 5);

        // move_down -> line "ab", col should clamp to 2 -> byte 6 + 2 = 8
        assert!(editor.move_down());
        assert_eq!(editor.cursor(), 8); // byte index of end of "ab"

        // move_down again -> line "abcde", col should restore to 2 (clamped) -> byte 9 + 2 = 11
        assert!(editor.move_down());
        assert_eq!(editor.cursor(), 11);
    }

    #[test]
    fn move_up_on_first_line_moves_to_line_start() {
        let mut editor = TextEditor::new("abc\ndef");
        editor.set_cursor(2); // middle of first line
        assert!(editor.move_up());
        assert_eq!(editor.cursor(), 0);
        assert!(!editor.move_up());
    }

    #[test]
    fn move_down_on_last_line_moves_to_line_end() {
        let mut editor = TextEditor::new("abc\ndef");
        // Place cursor on second line
        editor.set_cursor(5); // middle of "def"
        assert!(editor.move_down());
        assert_eq!(editor.cursor(), 7);
        assert!(!editor.move_down());
    }

    #[test]
    fn select_up_down_at_boundaries_select_line_edge() {
        let mut editor = TextEditor::new("abc\ndef");
        editor.set_cursor(2);
        assert!(editor.select_up());
        assert_eq!(editor.cursor(), 0);
        assert_eq!(editor.selection(), Some((0, 2)));

        editor.set_cursor(5);
        assert!(editor.select_down());
        assert_eq!(editor.cursor(), 7);
        assert_eq!(editor.selection(), Some((5, 7)));
    }

    #[test]
    fn move_home_goes_to_line_start() {
        let mut editor = TextEditor::new("abc\ndef");
        // Place cursor in middle of "def" (byte 5 = 'd','e' -> byte 5 is 'e')
        editor.set_cursor(5);
        assert!(editor.move_home());
        // Start of "def" is byte 4
        assert_eq!(editor.cursor(), 4);
    }

    #[test]
    fn move_end_goes_to_line_end() {
        let mut editor = TextEditor::new("abc\ndef");
        // Place cursor at start of "def" (byte 4)
        editor.set_cursor(4);
        assert!(editor.move_end());
        // End of "def" is byte 7
        assert_eq!(editor.cursor(), 7);
    }

    #[test]
    fn backspace_at_line_start_joins_lines() {
        let mut editor = TextEditor::new("abc\ndef");
        // Place cursor at start of "def" (byte 4)
        editor.set_cursor(4);
        assert!(editor.backspace());
        assert_eq!(editor.text(), "abcdef");
        assert_eq!(editor.cursor(), 3); // cursor at join point
    }

    #[test]
    fn delete_at_line_end_joins_lines() {
        let mut editor = TextEditor::new("abc\ndef");
        // Place cursor at end of "abc" (byte 3, which is the '\n')
        editor.set_cursor(3);
        assert!(editor.delete());
        assert_eq!(editor.text(), "abcdef");
        assert_eq!(editor.cursor(), 3); // cursor stays
    }

    #[test]
    fn insert_newline_splits_line() {
        let mut editor = TextEditor::new("abcdef");
        editor.set_cursor(3);
        assert!(editor.insert_char('\n'));
        assert_eq!(editor.text(), "abc\ndef");
        assert_eq!(editor.cursor(), 4); // cursor at start of new line
    }

    #[test]
    fn select_down_creates_vertical_selection() {
        let mut editor = TextEditor::new("abc\ndef");
        // cursor at byte 1 (after 'a')
        editor.set_cursor(1);
        assert!(editor.select_down());
        // anchor should be original position
        assert_eq!(editor.anchor(), Some(1));
        // cursor should be at col 1 on line "def" -> byte 4 + 1 = 5
        assert_eq!(editor.cursor(), 5);
    }

    #[test]
    fn empty_lines_traversal() {
        let mut editor = TextEditor::new("a\n\nb");
        // "a" at byte 0, '\n' at byte 1, empty line '\n' at byte 2, "b" at byte 3
        assert_eq!(editor.cursor(), 0);

        // move_down -> empty line, cursor at byte 2 (start of empty line)
        assert!(editor.move_down());
        assert_eq!(editor.cursor(), 2);

        // move_down -> line "b", cursor at byte 3
        assert!(editor.move_down());
        assert_eq!(editor.cursor(), 3);

        // move_up back to empty line
        assert!(editor.move_up());
        assert_eq!(editor.cursor(), 2);

        // move_up back to "a"
        assert!(editor.move_up());
        assert_eq!(editor.cursor(), 0);
    }

    #[test]
    fn toggle_devtools_action_is_non_editing() {
        let mut editor = TextEditor::new("hello");
        let changed = editor.handle_key(KeyEvent {
            code: KeyCode::F(12),
            mods: KeyMods::default(),
        });

        assert!(!changed);
        assert_eq!(editor.text(), "hello");
        assert_eq!(editor.cursor(), 0);
    }

    #[test]
    fn multi_line_selection_delete() {
        let mut editor = TextEditor::new("abc\ndef\nghi");
        // Select from byte 1 to byte 8 (across "bc\ndef\n")
        editor.set_cursor(1);
        editor.set_anchor(Some(8));
        assert!(editor.delete_selection());
        assert_eq!(editor.text(), "aghi");
        assert_eq!(editor.cursor(), 1);
        assert_eq!(editor.anchor(), None);
    }

    #[test]
    fn apply_updates_editor_after_insert() {
        let mut editor = TextEditor::new("abc");
        TextAreaEvent {
            value: "abc\ndef".into(),
            cursor: 7,
            anchor: None,
        }
        .apply_to(&mut editor);

        assert_eq!(editor.text(), "abc\ndef");
        assert_eq!(editor.cursor(), 7);
        assert_eq!(editor.anchor(), None);
    }

    #[test]
    fn apply_updates_editor_after_delete() {
        let mut editor = TextEditor::new("abc\ndef");
        TextAreaEvent {
            value: "abcdef".into(),
            cursor: 3,
            anchor: None,
        }
        .apply_to(&mut editor);

        assert_eq!(editor.text(), "abcdef");
        assert_eq!(editor.cursor(), 3);
        assert_eq!(editor.anchor(), None);
    }

    #[test]
    fn apply_updates_cursor_navigation_without_text_change() {
        let mut editor = TextEditor::new("abc\ndef");
        TextAreaEvent {
            value: "abc\ndef".into(),
            cursor: 5,
            anchor: None,
        }
        .apply_to(&mut editor);

        assert_eq!(editor.text(), "abc\ndef");
        assert_eq!(editor.cursor(), 5);
        assert_eq!(editor.anchor(), None);
    }

    #[test]
    fn apply_updates_selection_anchor() {
        let mut editor = TextEditor::new("abc\ndef");
        TextAreaEvent {
            value: "abc\ndef".into(),
            cursor: 6,
            anchor: Some(1),
        }
        .apply_to(&mut editor);

        assert_eq!(editor.selection(), Some((1, 6)));
    }

    #[test]
    fn apply_clamps_cursor_and_anchor_inside_unicode_characters() {
        let value = "części\newaluacyjnej";
        let cursor_inside_s = 5;
        assert!(!value.is_char_boundary(cursor_inside_s));

        let mut editor = TextEditor::new("");
        TextAreaEvent {
            value: value.into(),
            cursor: cursor_inside_s,
            anchor: Some(cursor_inside_s),
        }
        .apply_to(&mut editor);

        assert_eq!(editor.cursor(), 4);
        assert_eq!(editor.anchor(), Some(4));
        assert!(editor.text().is_char_boundary(editor.cursor()));
    }

    #[test]
    fn line_navigation_tolerates_unicode_boundary_drift() {
        let mut editor = TextEditor::new("abc\nczęści\nxyz");
        editor.core.cursor = 9;
        assert!(!editor.text().is_char_boundary(editor.core.cursor));

        assert!(editor.move_end());
        assert_eq!(editor.cursor(), "abc\nczęści".len());

        editor.core.cursor = 9;
        assert!(editor.move_home());
        assert_eq!(editor.cursor(), "abc\n".len());

        editor.core.cursor = 9;
        assert!(editor.move_down());
        assert!(editor.text().is_char_boundary(editor.cursor()));
    }

    #[test]
    fn clear_is_undoable_and_redoable_with_cursor_and_anchor() {
        let mut editor = TextEditor::new("abc\ndef");
        editor.set_cursor(6);
        editor.set_anchor(Some(1));
        editor.set_visual_nav_col(Some(3));

        assert!(editor.clear());
        assert_eq!(editor.text(), "");
        assert_eq!(editor.cursor(), 0);
        assert_eq!(editor.anchor(), None);
        assert_eq!(editor.visual_nav_col(), None);

        assert!(editor.undo());
        assert_eq!(editor.text(), "abc\ndef");
        assert_eq!(editor.cursor(), 6);
        assert_eq!(editor.anchor(), Some(1));

        assert!(editor.redo());
        assert_eq!(editor.text(), "");
        assert_eq!(editor.cursor(), 0);
        assert_eq!(editor.anchor(), None);
    }

    #[test]
    fn clear_empty_text_only_normalizes_cursor_and_anchor() {
        let mut editor = TextEditor::new("");
        editor.set_cursor_keep_anchor(10);
        editor.set_anchor(Some(10));

        assert!(editor.clear());
        assert_eq!(editor.cursor(), 0);
        assert_eq!(editor.anchor(), None);
        assert!(!editor.can_undo());
        assert!(!editor.clear());
    }
}
