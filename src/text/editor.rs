//! Multi-line text editor with selection support.

use crate::clipboard::ImageContent;
use crate::core::event::KeyEvent;
use crate::text::buffer::{CoreEdit, SharedHistory, SharedHistoryHandle, TextBufferCore};
use crate::text::edit::{TextEditEvent, TextEditKind};
use crate::utils::text::clamp_cursor;
use crate::widgets::{SentinelEvent, SentinelId, TextAreaEvent, TextAreaSentinel};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;
use undo::Record;

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
    /// Sticky visual column used for wrap-aware vertical navigation.
    /// Cleared whenever the cursor moves due to anything other than
    /// visual up/down (including external cursor sync, edits, horizontal
    /// movement, clicks). Set by the textarea key handler before performing
    /// visual navigation.
    pub(crate) visual_nav_col: Option<usize>,
    /// Set after moving to the end of a soft-wrapped row. That byte is also where the next row
    /// starts, so this records that the caret still counts as on the row it came from.
    pub(crate) visual_row_end: Option<usize>,
    /// Pre-edit snapshot and optional bound history target active during a single runtime
    /// `TextArea` key/paste/clipboard operation.
    active_tx: ActiveEditTransaction,
}

#[derive(Debug, Default)]
struct ActiveEditTransaction(Option<PendingEditTransaction>);

impl Clone for ActiveEditTransaction {
    fn clone(&self) -> Self {
        Self(None)
    }
}

#[derive(Debug)]
struct PendingEditTransaction {
    pre_text: String,
    pre_cursor: usize,
    pre_anchor: Option<usize>,
    /// Copy-on-write pre-edit record captured only when needed for sentinel remapping.
    pre_record: Option<Arc<Record<CoreEdit>>>,
    pre_generation: u64,
    bound: Option<(SharedHistoryHandle, u64)>,
}

/// RAII guard that scopes a single runtime [`TextArea`](crate::widgets::TextArea) edit
/// transaction against an optional bound [`TextEditor`]'s shared undo history.
///
/// Bound edits run on a copy-on-write snapshot and are staged on the bound history handle right
/// before emitting `on_change`, so rejected or transformed controlled events can never corrupt
/// the bound `TextEditor`'s committed undo stack, and non-mutating keys avoid deep clones.
pub(crate) struct BoundEditorGuard<'a> {
    editor: &'a mut TextEditor,
}

impl Deref for BoundEditorGuard<'_> {
    type Target = TextEditor;

    fn deref(&self) -> &Self::Target {
        self.editor
    }
}

impl DerefMut for BoundEditorGuard<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.editor
    }
}

impl Drop for BoundEditorGuard<'_> {
    fn drop(&mut self) {
        self.editor.active_tx.0 = None;
    }
}

#[derive(Clone, Debug)]
struct PendingExternalPaste {
    text_before: String,
    cursor_before: usize,
    anchor_before: Option<usize>,
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
                history: SharedHistory::fresh(),
                last_edit: None,
            },
            pending_external_paste: None,
            visual_nav_col: None,
            visual_row_end: None,
            active_tx: ActiveEditTransaction::default(),
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
            self.visual_row_end = None;
        }
    }

    /// Reconcile the runtime editor with a [`TextArea`](crate::widgets::TextArea) node's current
    /// `(text, cursor, anchor)`.
    ///
    /// When `bound_history` is `Some`, checks out an O(1) copy-on-write snapshot of the bound
    /// [`TextEditor`]'s authoritative history so runtime edits remain transactional until
    /// accepted via [`TextAreaEvent::apply_to`] or [`TextEditor::apply`], while non-mutating
    /// operations (such as cursor navigation) never deep-clone the undo record.
    ///
    /// When `track_sentinel_remap` is `true`, also retains a copy-on-write pre-edit checkpoint so
    /// sentinel renumbering can rewrite the active edit transaction atomically.
    pub(crate) fn sync_for_widget<'a>(
        &'a mut self,
        text: &str,
        cursor: usize,
        anchor: Option<usize>,
        bound_history: Option<&SharedHistoryHandle>,
        track_sentinel_remap: bool,
    ) -> BoundEditorGuard<'a> {
        if let Some(handle) = bound_history {
            let prev_cursor = self.core.cursor;
            let text_changed = self.core.text != text;
            self.pending_external_paste = None;
            if text_changed {
                self.core.text = text.to_string();
            }
            self.core.cursor = clamp_cursor(&self.core.text, cursor);
            self.core.anchor = anchor.map(|a| clamp_cursor(&self.core.text, a));
            if text_changed || self.core.cursor != prev_cursor {
                self.visual_nav_col = None;
                self.visual_row_end = None;
            }
            let (starting_record, base_generation) = handle.checkout_for_text(&self.core.text);
            let pre_record = track_sentinel_remap.then(|| Arc::clone(&starting_record));
            self.core.history = SharedHistory::from_arc_record(starting_record);
            self.active_tx.0 = Some(PendingEditTransaction {
                pre_text: if track_sentinel_remap {
                    self.core.text.clone()
                } else {
                    String::new()
                },
                pre_cursor: self.core.cursor,
                pre_anchor: self.core.anchor,
                pre_record,
                pre_generation: 0,
                bound: Some((handle.clone(), base_generation)),
            });
            BoundEditorGuard { editor: self }
        } else {
            self.sync_from(text, cursor, anchor);
            let pre_record = track_sentinel_remap.then(|| self.core.history.snapshot());
            let pre_generation = self.core.history.generation();
            self.active_tx.0 = Some(PendingEditTransaction {
                pre_text: if track_sentinel_remap {
                    self.core.text.clone()
                } else {
                    String::new()
                },
                pre_cursor: self.core.cursor,
                pre_anchor: self.core.anchor,
                pre_record,
                pre_generation,
                bound: None,
            });
            BoundEditorGuard { editor: self }
        }
    }

    /// Replace the active edit transaction's history entry with a single atomic replacement from
    /// the pre-edit text to `final_text` (e.g. after sentinel renumbering).
    pub(crate) fn rewrite_pending_edit_to(
        &mut self,
        final_text: &str,
        final_cursor: usize,
        final_anchor: Option<usize>,
    ) -> Option<TextEditEvent> {
        let pre = self.active_tx.0.as_ref()?;
        let pre_record = pre.pre_record.as_ref().map(Arc::clone)?;
        let pre_text = pre.pre_text.clone();
        let pre_cursor = pre.pre_cursor;
        let pre_anchor = pre.pre_anchor;
        let (image_snapshots, sentinel_snapshots) = self.core.history.clone_metadata_snapshots();

        self.core.text = pre_text;
        self.core.cursor = pre_cursor;
        self.core.anchor = pre_anchor;
        self.core.history = SharedHistory::from_arc_record(pre_record);
        for snapshot in image_snapshots {
            self.core
                .history
                .remember_images(&snapshot.text, &snapshot.images);
        }
        for snapshot in sentinel_snapshots {
            self.core
                .history
                .remember_sentinels(&snapshot.text, &snapshot.sentinels);
        }
        let event = self.core.record_full_replace_edit(
            final_text,
            pre_cursor,
            pre_anchor,
            final_cursor,
            final_anchor,
        );
        self.core.last_edit = None;
        if let Some(tx) = self.active_tx.0.as_mut() {
            // `from_arc_record` starts local generation at 0 and `record_full_replace_edit`
            // bumped it to 1, so `pre_generation` must be 0 to mark the transaction as mutated.
            tx.pre_generation = 0;
        }
        Some(event)
    }

    /// Stage the working history snapshot onto the bound handle right before emitting `on_change`
    /// so `TextAreaEvent::apply_to` / `TextEditor::apply` can commit it atomically if accepted.
    pub(crate) fn stage_bound_history(&self, expected_text: &Arc<str>) {
        let Some(tx) = self.active_tx.0.as_ref() else {
            return;
        };
        let Some((handle, base_generation)) = tx.bound.as_ref() else {
            return;
        };
        if self.core.history.generation() == tx.pre_generation {
            return;
        }
        let (image_snapshots, sentinel_snapshots) = self.core.history.clone_metadata_snapshots();
        handle.stage_step(
            *base_generation,
            expected_text.clone(),
            self.core.history.snapshot(),
            image_snapshots,
            sentinel_snapshots,
        );
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
        self.core.history.remember_sentinels(text, sentinels);
    }

    pub(crate) fn remembered_text_area_sentinels(
        &self,
        text: &str,
    ) -> Option<Vec<TextAreaSentinel>> {
        self.core.history.remembered_sentinels(text).or_else(|| {
            self.active_tx
                .0
                .as_ref()
                .and_then(|tx| tx.bound.as_ref())
                .and_then(|(handle, _)| handle.remembered_sentinels(text))
        })
    }

    pub(crate) fn remember_text_area_images(&mut self, text: &str, images: &[ImageContent]) {
        self.core.history.remember_images(text, images);
    }

    pub(crate) fn remembered_text_area_images(&self, text: &str) -> Option<Vec<ImageContent>> {
        self.core.history.remembered_images(text).or_else(|| {
            self.active_tx
                .0
                .as_ref()
                .and_then(|tx| tx.bound.as_ref())
                .and_then(|(handle, _)| handle.remembered_images(text))
        })
    }

    /// Undo the most recent edit and synchronize any tracked inline image metadata with the
    /// restored text.
    pub fn undo_with_images(&mut self, images: &mut Vec<ImageContent>) -> bool {
        self.undo_with_metadata(Some(images), None)
    }

    /// Redo the most recently undone edit and synchronize any tracked inline image metadata with
    /// the restored text.
    pub fn redo_with_images(&mut self, images: &mut Vec<ImageContent>) -> bool {
        self.redo_with_metadata(Some(images), None)
    }

    /// Undo the most recent edit and synchronize any tracked custom sentinel metadata with the
    /// restored text.
    pub fn undo_with_sentinels(&mut self, sentinels: &mut Vec<TextAreaSentinel>) -> bool {
        self.undo_with_metadata(None, Some(sentinels))
    }

    /// Redo the most recently undone edit and synchronize any tracked custom sentinel metadata
    /// with the restored text.
    pub fn redo_with_sentinels(&mut self, sentinels: &mut Vec<TextAreaSentinel>) -> bool {
        self.redo_with_metadata(None, Some(sentinels))
    }

    /// Undo the most recent edit and synchronize any tracked inline image and/or custom sentinel
    /// metadata with the restored text.
    pub fn undo_with_metadata(
        &mut self,
        images: Option<&mut Vec<ImageContent>>,
        sentinels: Option<&mut Vec<TextAreaSentinel>>,
    ) -> bool {
        self.replay_history_with_metadata(true, images, sentinels)
    }

    /// Redo the most recently undone edit and synchronize any tracked inline image and/or custom
    /// sentinel metadata with the restored text.
    pub fn redo_with_metadata(
        &mut self,
        images: Option<&mut Vec<ImageContent>>,
        sentinels: Option<&mut Vec<TextAreaSentinel>>,
    ) -> bool {
        self.replay_history_with_metadata(false, images, sentinels)
    }

    fn replay_history_with_metadata(
        &mut self,
        is_undo: bool,
        images: Option<&mut Vec<ImageContent>>,
        sentinels: Option<&mut Vec<TextAreaSentinel>>,
    ) -> bool {
        if let Some(imgs) = images.as_deref() {
            self.core.history.remember_images(&self.core.text, imgs);
        }
        if let Some(sents) = sentinels.as_deref() {
            self.core.history.remember_sentinels(&self.core.text, sents);
        }
        let changed = if is_undo {
            self.core.undo()
        } else {
            self.core.redo()
        };
        if !changed {
            return false;
        }
        let restored_text = self.text().to_owned();
        if let Some(imgs) = images {
            let required = image_sentinel_required_len(&restored_text);
            let source = if required == 0 {
                (!imgs.is_empty()).then(|| imgs.clone())
            } else {
                self.remembered_text_area_images(&restored_text)
                    .filter(|saved| required <= saved.len())
                    .or_else(|| (required <= imgs.len()).then(|| imgs.clone()))
            };
            if let Some(source) = source {
                let (pruned, _) = prune_images_for_value(&restored_text, &source);
                *imgs = pruned;
                self.remember_text_area_images(&restored_text, imgs);
            }
        }
        if let Some(sents) = sentinels {
            let required = custom_sentinel_required_len(&restored_text);
            let source = if required == 0 {
                (!sents.is_empty()).then(|| sents.clone())
            } else {
                self.remembered_text_area_sentinels(&restored_text)
                    .filter(|saved| required <= saved.len())
                    .or_else(|| (required <= sents.len()).then(|| sents.clone()))
            };
            if let Some(source) = source {
                let (pruned, _, _) = prune_sentinels_for_value(&restored_text, &source);
                *sents = pruned;
                self.remember_text_area_sentinels(&restored_text, sents);
            }
        }
        true
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

    /// Delete from the start of the current line to the cursor, or the selection if any.
    ///
    /// At the start of a line this changes nothing rather than joining the previous line.
    pub fn delete_to_line_start(&mut self) -> bool {
        self.core.normalize_cursor_and_anchor();
        if self.core.delete_selection() {
            return true;
        }
        let cursor = self.core.cursor;
        let start = self.core.text[..cursor].rfind('\n').map_or(0, |i| i + 1);
        self.core
            .replace_range(start, cursor, "", TextEditKind::DeleteBackspace)
    }

    /// Delete from the cursor to the end of the current line, or the selection if any.
    ///
    /// At the end of a line this changes nothing rather than joining the next line.
    pub fn delete_to_line_end(&mut self) -> bool {
        self.core.normalize_cursor_and_anchor();
        if self.core.delete_selection() {
            return true;
        }
        let cursor = self.core.cursor;
        let end = self.core.text[cursor..]
            .find('\n')
            .map_or(self.core.text.len(), |i| cursor + i);
        self.core
            .replace_range(cursor, end, "", TextEditKind::DeleteForward)
    }

    /// Delete the current line with its line break, or the selection if any.
    ///
    /// The cursor lands at the start of the following line, or at the end of the new last line
    /// when the last line was deleted.
    pub fn delete_line(&mut self) -> bool {
        self.core.normalize_cursor_and_anchor();
        if self.core.delete_selection() {
            return true;
        }
        let text = &self.core.text;
        let cursor = self.core.cursor;
        let line_start = text[..cursor].rfind('\n').map_or(0, |i| i + 1);
        let (start, end) = match text[cursor..].find('\n') {
            Some(i) => (line_start, cursor + i + 1),
            None => (line_start.saturating_sub(1), text.len()),
        };
        self.core
            .replace_range(start, end, "", TextEditKind::Replace)
    }

    /// Move to the start of the text, clearing selection.
    pub fn move_buffer_home(&mut self) -> bool {
        self.move_to_buffer_edge(0)
    }

    /// Move to the end of the text, clearing selection.
    pub fn move_buffer_end(&mut self) -> bool {
        self.move_to_buffer_edge(self.core.text.len())
    }

    /// Extend the selection to the start of the text.
    pub fn select_buffer_home(&mut self) -> bool {
        self.select_to_buffer_edge(0)
    }

    /// Extend the selection to the end of the text.
    pub fn select_buffer_end(&mut self) -> bool {
        self.select_to_buffer_edge(self.core.text.len())
    }

    fn move_to_buffer_edge(&mut self, target: usize) -> bool {
        self.core.normalize_cursor_and_anchor();
        let changed = self.core.cursor != target || self.core.anchor.is_some();
        self.core.anchor = None;
        self.core.cursor = target;
        changed
    }

    fn select_to_buffer_edge(&mut self, target: usize) -> bool {
        self.core.normalize_cursor_and_anchor();
        if self.core.cursor == target {
            return false;
        }
        if self.core.anchor.is_none() {
            self.core.anchor = Some(self.core.cursor);
        }
        self.core.cursor = target;
        self.core.collapse_empty_selection();
        true
    }

    /// Apply a [`TextAreaEvent`] to this state bundle.
    pub fn apply(&mut self, ev: &TextAreaEvent) {
        ev.apply_to(self);
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
        self.handle_action(keymap.resolve_action(key))
    }

    pub(crate) fn handle_action(&mut self, action: crate::app::input::keymap::Action) -> bool {
        use crate::app::input::keymap::Action;

        match action {
            Action::MoveLeft => self.core.move_left(),
            Action::MoveRight => self.core.move_right(),
            Action::MoveUp => self.move_up(),
            Action::MoveDown => self.move_down(),
            Action::MoveWordLeft => self.core.move_word_left(),
            Action::MoveWordRight => self.core.move_word_right(),
            Action::MoveHome | Action::MoveVisualLineHome => self.move_home(),
            Action::MoveEnd | Action::MoveVisualLineEnd => self.move_end(),
            Action::MoveBufferHome => self.move_buffer_home(),
            Action::MoveBufferEnd => self.move_buffer_end(),

            Action::SelectLeft => self.core.select_left(),
            Action::SelectRight => self.core.select_right(),
            Action::SelectUp => self.select_up(),
            Action::SelectDown => self.select_down(),
            Action::SelectWordLeft => self.core.select_word_left(),
            Action::SelectWordRight => self.core.select_word_right(),
            Action::SelectHome | Action::SelectVisualLineHome => self.select_home(),
            Action::SelectEnd | Action::SelectVisualLineEnd => self.select_end(),
            Action::SelectBufferHome => self.select_buffer_home(),
            Action::SelectBufferEnd => self.select_buffer_end(),
            Action::SelectAll => self.core.select_all(),

            Action::Backspace => self.core.backspace(),
            Action::Clear => self.clear(),
            Action::Delete => self.core.delete(),
            Action::DeleteWordLeft => self.core.delete_word_left(),
            Action::DeleteWordRight => self.core.delete_word_right(),
            Action::DeleteToLineStart => self.delete_to_line_start(),
            Action::DeleteToLineEnd => self.delete_to_line_end(),
            Action::DeleteLine => self.delete_line(),

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

pub(crate) fn custom_sentinel_required_len(value: &str) -> usize {
    let base = crate::widgets::SENTINEL_BASE as u32;
    let private_use_end = 0xF900;
    value
        .chars()
        .filter_map(|ch| {
            let cp = ch as u32;
            if cp >= base && cp < private_use_end {
                Some((cp - base) as usize + 1)
            } else {
                None
            }
        })
        .max()
        .unwrap_or(0)
}

pub(crate) fn image_sentinel_required_len(value: &str) -> usize {
    let base = crate::widgets::IMAGE_SENTINEL_BASE as u32;
    let private_use_end = 0xE100;
    value
        .chars()
        .filter_map(|ch| {
            let cp = ch as u32;
            if cp >= base && cp < private_use_end {
                Some((cp - base) as usize + 1)
            } else {
                None
            }
        })
        .max()
        .unwrap_or(0)
}

/// Return a pruned images list that only keeps images still referenced by sentinel
/// characters present in `new_value`.
///
/// In inline mode each image at index `i` is represented by the sentinel character
/// `U+E000 + i`. When the user deletes one of those characters the corresponding
/// image must be removed from the list.
///
/// Returns `(pruned_images, Option<remapped_value>)`. When a middle sentinel is deleted,
/// the remaining sentinel chars in the value are remapped to sequential indices so that
/// the pruned list and the value stay consistent.
pub(crate) fn prune_images_for_value(
    new_value: &str,
    images: &[ImageContent],
) -> (Vec<ImageContent>, Option<String>) {
    if images.is_empty() {
        return (Vec::new(), None);
    }
    let sentinel_base = crate::IMAGE_SENTINEL_BASE as u32;
    let max_sentinel = sentinel_base + images.len() as u32;
    let mut present = vec![false; images.len()];
    for ch in new_value.chars() {
        let cp = ch as u32;
        if cp >= sentinel_base && cp < max_sentinel {
            let idx = (cp - sentinel_base) as usize;
            present[idx] = true;
        }
    }
    let pruned: Vec<ImageContent> = images
        .iter()
        .enumerate()
        .filter(|(i, _)| present[*i])
        .map(|(_, img)| img.clone())
        .collect();

    let mut remap: Vec<Option<usize>> = vec![None; images.len()];
    let mut new_idx = 0usize;
    for (old_idx, was_present) in present.iter().enumerate() {
        if *was_present {
            remap[old_idx] = Some(new_idx);
            new_idx += 1;
        }
    }

    let needs_remap = remap
        .iter()
        .enumerate()
        .any(|(i, r)| r.is_some_and(|n| n != i));
    let remapped_value = if needs_remap {
        let mut out = String::with_capacity(new_value.len());
        for ch in new_value.chars() {
            let cp = ch as u32;
            if cp >= sentinel_base && cp < max_sentinel {
                let old_idx = (cp - sentinel_base) as usize;
                if let Some(Some(new_i)) = remap.get(old_idx) {
                    out.push(char::from_u32(sentinel_base + *new_i as u32).unwrap_or(ch));
                } else {
                    out.push(ch);
                }
            } else {
                out.push(ch);
            }
        }
        Some(out)
    } else {
        None
    };

    (pruned, remapped_value)
}

/// Prune the custom sentinels list when sentinel chars are removed from the value.
///
/// Returns `(pruned_sentinels, Option<remapped_value>, deleted_events)`. When a middle sentinel
/// is deleted, the remaining sentinel chars in the value are remapped to sequential indices.
pub(crate) fn prune_sentinels_for_value(
    new_value: &str,
    sentinels: &[TextAreaSentinel],
) -> (Vec<TextAreaSentinel>, Option<String>, Vec<SentinelEvent>) {
    if sentinels.is_empty() {
        return (Vec::new(), None, Vec::new());
    }
    let sentinel_base = crate::widgets::SENTINEL_BASE as u32;
    let max_sentinel = sentinel_base + sentinels.len() as u32;
    let mut present = vec![false; sentinels.len()];
    for ch in new_value.chars() {
        let cp = ch as u32;
        if cp >= sentinel_base && cp < max_sentinel {
            let idx = (cp - sentinel_base) as usize;
            present[idx] = true;
        }
    }
    let mut events = Vec::new();
    for (i, was_present) in present.iter().enumerate() {
        if !*was_present {
            let s = &sentinels[i];
            events.push(SentinelEvent::Deleted {
                id: s.sentinel_id().unwrap_or(SentinelId::UNKNOWN),
                sentinel: s.clone(),
            });
        }
    }
    let pruned: Vec<TextAreaSentinel> = sentinels
        .iter()
        .enumerate()
        .filter(|(i, _)| present[*i])
        .map(|(_, s)| s.clone())
        .collect();

    let mut remap: Vec<Option<usize>> = vec![None; sentinels.len()];
    let mut new_idx = 0usize;
    for (old_idx, was_present) in present.iter().enumerate() {
        if *was_present {
            remap[old_idx] = Some(new_idx);
            new_idx += 1;
        }
    }

    let needs_remap = remap
        .iter()
        .enumerate()
        .any(|(i, r)| r.is_some_and(|n| n != i));
    let remapped_value = if needs_remap {
        let mut out = String::with_capacity(new_value.len());
        for ch in new_value.chars() {
            let cp = ch as u32;
            if cp >= sentinel_base && cp < max_sentinel {
                let old_idx = (cp - sentinel_base) as usize;
                if let Some(Some(new_i)) = remap.get(old_idx) {
                    out.push(char::from_u32(sentinel_base + *new_i as u32).unwrap_or(ch));
                } else {
                    out.push(ch);
                }
            } else {
                out.push(ch);
            }
        }
        Some(out)
    } else {
        None
    };

    (pruned, remapped_value, events)
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
    fn delete_to_line_start_stops_at_the_current_line() {
        let mut editor = TextEditor::new("abc\nhelloworld\nxyz");
        editor.set_cursor(9);

        assert!(editor.delete_to_line_start());
        assert_eq!(editor.text(), "abc\nworld\nxyz");
        assert_eq!(editor.cursor(), 4);

        assert!(editor.undo());
        assert_eq!(editor.text(), "abc\nhelloworld\nxyz");
        assert_eq!(editor.cursor(), 9);
    }

    #[test]
    fn delete_to_line_start_at_a_line_start_changes_nothing() {
        let mut editor = TextEditor::new("abc\ndef");
        editor.set_cursor(4);

        assert!(!editor.delete_to_line_start());
        assert_eq!(editor.text(), "abc\ndef");
        assert_eq!(editor.cursor(), 4);
        assert!(!editor.can_undo());
    }

    #[test]
    fn delete_to_line_start_removes_a_selection_across_lines() {
        let mut editor = TextEditor::new("abc\ndef");
        editor.set_cursor(5);
        editor.set_anchor(Some(1));

        assert!(editor.delete_to_line_start());
        assert_eq!(editor.text(), "aef");
        assert_eq!(editor.cursor(), 1);
        assert_eq!(editor.anchor(), None);
    }

    #[test]
    fn delete_to_line_end_stops_at_the_current_line() {
        let mut editor = TextEditor::new("abc\nhelloworld\nxyz");
        editor.set_cursor(9);

        assert!(editor.delete_to_line_end());
        assert_eq!(editor.text(), "abc\nhello\nxyz");
        assert_eq!(editor.cursor(), 9);

        assert!(!editor.delete_to_line_end(), "no join with the next line");

        assert!(editor.undo());
        assert_eq!(editor.text(), "abc\nhelloworld\nxyz");
    }

    #[test]
    fn delete_to_line_end_keeps_multibyte_text_intact() {
        let mut editor = TextEditor::new("żółć🦀\ngęś");
        editor.set_cursor("żó".len());

        assert!(editor.delete_to_line_end());
        assert_eq!(editor.text(), "żó\ngęś");
    }

    #[test]
    fn delete_line_removes_the_line_and_its_break() {
        let mut editor = TextEditor::new("abc\ndef\nghi");
        editor.set_cursor(5);

        assert!(editor.delete_line());
        assert_eq!(editor.text(), "abc\nghi");
        assert_eq!(editor.cursor(), 4);

        assert!(editor.undo());
        assert_eq!(editor.text(), "abc\ndef\nghi");
        assert_eq!(editor.cursor(), 5);
    }

    #[test]
    fn delete_line_on_the_last_line_removes_the_break_before_it() {
        let mut editor = TextEditor::new("abc\ndef");
        editor.set_cursor(5);

        assert!(editor.delete_line());
        assert_eq!(editor.text(), "abc");
        assert_eq!(editor.cursor(), 3);

        assert!(editor.delete_line());
        assert_eq!(editor.text(), "");
        assert!(!editor.delete_line(), "nothing left to delete");
    }

    #[test]
    fn delete_line_on_an_empty_line_removes_only_that_line() {
        let mut editor = TextEditor::new("abc\n\ndef");
        editor.set_cursor(4);

        assert!(editor.delete_line());
        assert_eq!(editor.text(), "abc\ndef");
        assert_eq!(editor.cursor(), 4);
    }

    #[test]
    fn delete_line_removes_only_a_selection() {
        let mut editor = TextEditor::new("abc\ndef");
        editor.set_cursor(5);
        editor.set_anchor(Some(1));

        assert!(editor.delete_line());
        assert_eq!(editor.text(), "aef");
    }

    #[test]
    fn buffer_motions_cross_lines_and_extend_selections() {
        let mut editor = TextEditor::new("abc\ndef");
        editor.set_cursor(5);

        assert!(editor.select_buffer_home());
        assert_eq!((editor.cursor(), editor.anchor()), (0, Some(5)));
        assert!(editor.select_buffer_end());
        assert_eq!((editor.cursor(), editor.anchor()), (7, Some(5)));
        assert!(editor.move_buffer_home());
        assert_eq!((editor.cursor(), editor.anchor()), (0, None));
        assert!(!editor.move_buffer_home());
        assert!(editor.move_buffer_end());
        assert_eq!(editor.cursor(), 7);
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

    #[test]
    fn cloning_editor_creates_independent_undo_history() {
        let mut original = TextEditor::new("");
        original.insert_str("hello");

        let mut cloned = original.clone();
        cloned.insert_str(" world");
        assert!(cloned.undo());
        assert_eq!(cloned.text(), "hello");
        assert!(cloned.undo());
        assert_eq!(cloned.text(), "");

        assert_eq!(original.text(), "hello");
        assert!(original.can_undo());
        assert!(!original.can_redo());
        assert!(original.undo());
        assert_eq!(original.text(), "");
    }
}
