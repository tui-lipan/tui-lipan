use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::callback::Callback;
use crate::clipboard::ImageContent;
use crate::text::edit::{
    EditMetadata, RestoredMetadata, TextEdit, TextEditEvent, TextEditFields, TextEditKind,
    TextTarget,
};
use crate::utils::text::clamp_cursor;
use crate::utils::text::{
    next_char_boundary, prev_char_boundary, word_boundary_left, word_boundary_right,
};
use crate::widgets::{SentinelEvent, TextAreaSentinel};
use undo::Record;

const DEFAULT_HISTORY_LIMIT: usize = 1000;
const MAX_STAGED_HISTORY_STEPS: usize = 64;

pub(crate) type CoreEdit = TextEdit<TextBufferCore>;

/// Proposed image/sentinel metadata staged alongside a bound runtime edit so it can be applied
/// atomically via `TextEditor::apply_with_metadata` or gated until `TextAreaEvent::apply_to`.
#[derive(Clone, Debug, Default)]
pub(crate) struct StagedMetadata {
    pub(crate) images: Option<Vec<ImageContent>>,
    pub(crate) sentinels: Option<Vec<TextAreaSentinel>>,
}

/// UI-thread metadata callbacks deferred until the matching bound `TextAreaEvent` is accepted.
///
/// Stored in a bounded thread-local queue keyed by an opaque token so [`TextBufferCore`] and
/// [`TextEditor`](super::TextEditor) remain `Send + Sync`.
pub(crate) struct DeferredMetadataCallbacks {
    pub(crate) images: Option<(Callback<Vec<ImageContent>>, Vec<ImageContent>)>,
    pub(crate) sentinels: Option<(Callback<Vec<TextAreaSentinel>>, Vec<TextAreaSentinel>)>,
    pub(crate) sentinel_events: Option<(Callback<Vec<SentinelEvent>>, Vec<SentinelEvent>)>,
}

static NEXT_DEFERRED_CALLBACK_TOKEN: AtomicU64 = AtomicU64::new(1);

thread_local! {
    static DEFERRED_METADATA_CALLBACKS: RefCell<VecDeque<(u64, DeferredMetadataCallbacks)>> =
        const { RefCell::new(VecDeque::new()) };
}

pub(crate) fn register_deferred_metadata_callbacks(
    callbacks: DeferredMetadataCallbacks,
) -> Option<u64> {
    if callbacks.images.is_none()
        && callbacks.sentinels.is_none()
        && callbacks.sentinel_events.is_none()
    {
        return None;
    }
    let token = NEXT_DEFERRED_CALLBACK_TOKEN.fetch_add(1, Ordering::Relaxed);
    DEFERRED_METADATA_CALLBACKS.with(|registry| {
        let mut queue = registry.borrow_mut();
        if queue.len() >= MAX_STAGED_HISTORY_STEPS {
            queue.pop_front();
        }
        queue.push_back((token, callbacks));
    });
    Some(token)
}

fn take_deferred_metadata_callbacks(token: u64) -> Option<DeferredMetadataCallbacks> {
    DEFERRED_METADATA_CALLBACKS.with(|registry| {
        let mut queue = registry.borrow_mut();
        let pos = queue.iter().position(|(id, _)| *id == token)?;
        queue.remove(pos).map(|(_, callbacks)| callbacks)
    })
}

fn discard_deferred_metadata_callbacks(token: u64) {
    let _ = take_deferred_metadata_callbacks(token);
}

#[derive(Debug)]
struct StagedHistoryStep {
    expected_text: Arc<str>,
    record: Arc<Record<CoreEdit>>,
    deferred_callbacks_token: Option<u64>,
    staged_metadata: Option<StagedMetadata>,
}

impl StagedHistoryStep {
    fn discard_callbacks(&mut self) {
        if let Some(token) = self.deferred_callbacks_token.take() {
            discard_deferred_metadata_callbacks(token);
        }
    }
}

#[derive(Debug)]
struct StagedHistory {
    base_generation: u64,
    steps: VecDeque<StagedHistoryStep>,
}

impl StagedHistory {
    fn discard_all_callbacks(&mut self) {
        for step in &mut self.steps {
            step.discard_callbacks();
        }
    }
}

pub(crate) struct CommittedStagedChange {
    pub(crate) deferred_callbacks_token: Option<u64>,
    pub(crate) staged_metadata: Option<StagedMetadata>,
}

#[derive(Debug)]
struct SharedHistoryInner {
    /// Copy-on-write undo/redo record so snapshots and non-mutating operations avoid deep clones.
    record: Arc<Record<CoreEdit>>,
    generation: u64,
    staged: Option<StagedHistory>,
}

impl SharedHistoryInner {
    fn clear_staged(&mut self) {
        if let Some(mut staged) = self.staged.take() {
            staged.discard_all_callbacks();
        }
    }
}

/// Undo history backing a [`TextBufferCore`].
///
/// Cloning a `SharedHistory` creates a new independent handle sharing only the immutable
/// copy-on-write [`Record`] until mutation, so cloning a [`TextEditor`](super::TextEditor) or
/// [`TextInput`](super::TextInput) is O(1) while preserving independent undo stacks. Sharing the
/// same history with a bound [`TextArea`](crate::widgets::TextArea) goes explicitly through
/// [`SharedHistoryHandle`], where runtime edits (and any associated metadata callbacks) are staged
/// until accepted via [`TextAreaEvent::apply_to`](crate::widgets::TextAreaEvent::apply_to) or
/// [`TextEditor::apply`](super::TextEditor::apply).
#[derive(Debug)]
pub(crate) struct SharedHistory(Arc<Mutex<SharedHistoryInner>>);

impl SharedHistory {
    pub(crate) fn fresh() -> Self {
        Self::from_arc_record(Arc::new(TextBufferCore::fresh_record()))
    }

    pub(crate) fn from_arc_record(record: Arc<Record<CoreEdit>>) -> Self {
        Self(Arc::new(Mutex::new(SharedHistoryInner {
            record,
            generation: 0,
            staged: None,
        })))
    }

    pub(crate) fn handle(&self) -> SharedHistoryHandle {
        SharedHistoryHandle(Arc::clone(&self.0))
    }

    fn lock(&self) -> MutexGuard<'_, SharedHistoryInner> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(crate) fn can_undo(&self) -> bool {
        self.lock().record.can_undo()
    }

    pub(crate) fn can_redo(&self) -> bool {
        self.lock().record.can_redo()
    }

    pub(crate) fn generation(&self) -> u64 {
        self.lock().generation
    }

    pub(crate) fn snapshot(&self) -> Arc<Record<CoreEdit>> {
        Arc::clone(&self.lock().record)
    }

    pub(crate) fn clear(&self) {
        let mut inner = self.lock();
        inner.record = Arc::new(TextBufferCore::fresh_record());
        inner.generation = inner.generation.wrapping_add(1);
        inner.clear_staged();
    }

    /// Take an owned mutable `Record`, reusing the existing allocation in O(1) when unshared or
    /// cloning copy-on-write when another handle holds a snapshot.
    pub(crate) fn take(&self) -> Record<CoreEdit> {
        let arc = std::mem::replace(
            &mut self.lock().record,
            Arc::new(TextBufferCore::fresh_record()),
        );
        Arc::try_unwrap(arc).unwrap_or_else(|shared| (*shared).clone())
    }

    pub(crate) fn put_back_committed(&self, record: Record<CoreEdit>, changed: bool) {
        let mut inner = self.lock();
        inner.record = Arc::new(record);
        if changed {
            inner.generation = inner.generation.wrapping_add(1);
            inner.clear_staged();
        }
    }

    pub(crate) fn commit_staged(&self, expected_text: &str) -> Option<CommittedStagedChange> {
        let mut inner = self.lock();
        let current_generation = inner.generation;
        let staged = inner.staged.as_mut()?;
        if staged.base_generation != current_generation {
            inner.clear_staged();
            return None;
        }
        let Some(pos) = staged
            .steps
            .iter()
            .position(|step| step.expected_text.as_ref() == expected_text)
        else {
            inner.clear_staged();
            return None;
        };
        for _ in 0..pos {
            if let Some(mut skipped) = staged.steps.pop_front() {
                skipped.discard_callbacks();
            }
        }
        let step = staged
            .steps
            .pop_front()
            .expect("matched staged history step must exist");
        let next_generation = current_generation.wrapping_add(1);
        let remaining_empty = staged.steps.is_empty();
        if !remaining_empty {
            staged.base_generation = next_generation;
        }
        inner.record = step.record;
        inner.generation = next_generation;
        if remaining_empty {
            inner.staged = None;
        }
        Some(CommittedStagedChange {
            deferred_callbacks_token: step.deferred_callbacks_token,
            staged_metadata: step.staged_metadata,
        })
    }
}

impl Clone for SharedHistory {
    fn clone(&self) -> Self {
        let inner = self.lock();
        Self(Arc::new(Mutex::new(SharedHistoryInner {
            record: Arc::clone(&inner.record),
            generation: 0,
            staged: None,
        })))
    }
}

/// Shared handle to a bound [`TextEditor`](super::TextEditor)'s authoritative undo history.
#[derive(Clone, Debug)]
pub(crate) struct SharedHistoryHandle(Arc<Mutex<SharedHistoryInner>>);

impl SharedHistoryHandle {
    fn lock(&self) -> MutexGuard<'_, SharedHistoryInner> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Check out a copy-on-write snapshot of the bound history for runtime `TextArea` handling.
    ///
    /// If uncommitted staged steps from the current generation end at `text` (e.g. a second
    /// runtime operation arrived before the first `on_change` event was drained), continues from
    /// that staged snapshot; otherwise drops stale staged steps and starts from the committed
    /// record.
    pub(crate) fn checkout_for_text(&self, text: &str) -> (Arc<Record<CoreEdit>>, u64) {
        let mut inner = self.lock();
        if let Some(staged) = inner.staged.as_ref()
            && staged.base_generation == inner.generation
            && let Some(last) = staged.steps.back()
            && last.expected_text.as_ref() == text
        {
            return (Arc::clone(&last.record), staged.base_generation);
        }
        inner.clear_staged();
        (Arc::clone(&inner.record), inner.generation)
    }

    /// Stage a runtime history checkpoint to be committed when the matching `TextAreaEvent` is
    /// accepted by `TextAreaEvent::apply_to` or `TextEditor::apply`.
    pub(crate) fn stage_step(
        &self,
        base_generation: u64,
        expected_text: Arc<str>,
        record: Arc<Record<CoreEdit>>,
        deferred_callbacks_token: Option<u64>,
        staged_metadata: Option<StagedMetadata>,
    ) {
        let mut inner = self.lock();
        if inner.generation != base_generation {
            if let Some(token) = deferred_callbacks_token {
                discard_deferred_metadata_callbacks(token);
            }
            return;
        }
        let step = StagedHistoryStep {
            expected_text,
            record,
            deferred_callbacks_token,
            staged_metadata,
        };
        if let Some(staged) = inner.staged.as_mut()
            && staged.base_generation == base_generation
        {
            if staged.steps.len() >= MAX_STAGED_HISTORY_STEPS
                && let Some(mut evicted) = staged.steps.pop_front()
            {
                evicted.discard_callbacks();
            }
            staged.steps.push_back(step);
        } else {
            inner.clear_staged();
            let mut steps = VecDeque::with_capacity(2);
            steps.push_back(step);
            inner.staged = Some(StagedHistory {
                base_generation,
                steps,
            });
        }
    }
}

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
    pub(crate) history: SharedHistory,
    pub(crate) last_edit: Option<TextEditEvent>,
    pub(crate) last_restored_metadata: Option<RestoredMetadata>,
}

impl Default for TextBufferCore {
    fn default() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            anchor: None,
            history: SharedHistory::fresh(),
            last_edit: None,
            last_restored_metadata: None,
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
    pub(crate) fn fresh_record() -> Record<CoreEdit> {
        Record::builder().limit(DEFAULT_HISTORY_LIMIT).build()
    }

    pub(crate) fn share_history(&self) -> SharedHistoryHandle {
        self.history.handle()
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
        let mut history = self.history.take();
        let outcome = history.undo(self);
        let changed = outcome.is_some();
        self.last_restored_metadata = outcome.flatten();
        self.history.put_back_committed(history, changed);
        changed
    }

    /// Redo the most recently undone edit.
    pub fn redo(&mut self) -> bool {
        let mut history = self.history.take();
        let outcome = history.redo(self);
        let changed = outcome.is_some();
        self.last_restored_metadata = outcome.flatten();
        self.history.put_back_committed(history, changed);
        changed
    }

    /// Clear all undo/redo history without changing the text.
    pub fn clear_history(&mut self) {
        self.last_restored_metadata = None;
        self.history.clear();
    }

    pub(crate) fn reset_history(&mut self) {
        self.last_restored_metadata = None;
        self.history.clear();
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
        self.record_full_replace_edit(
            text,
            cursor_before,
            anchor_before,
            cursor_after,
            anchor_after,
            None,
        );
        self.last_edit = None;
    }

    pub(crate) fn record_full_replace_edit(
        &mut self,
        text: &str,
        cursor_before: usize,
        anchor_before: Option<usize>,
        cursor_after: usize,
        anchor_after: Option<usize>,
        metadata: Option<EditMetadata>,
    ) -> TextEditEvent {
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
        })
        .with_metadata(metadata);
        let event = TextEditEvent::from(&edit);
        self.record_edit(edit);
        event
    }

    /// Apply a controlled `TextAreaEvent` back to a bound buffer, committing the matching staged
    /// runtime history step (and firing any deferred metadata callbacks) when the text was
    /// accepted verbatim, or recording a clean replacement edit when transformed.
    pub(crate) fn apply_controlled_change(
        &mut self,
        value: &str,
        cursor: usize,
        anchor: Option<usize>,
    ) {
        self.apply_controlled_change_with_metadata(value, cursor, anchor, None, None);
    }

    pub(crate) fn apply_controlled_change_with_metadata(
        &mut self,
        value: &str,
        cursor: usize,
        anchor: Option<usize>,
        images: Option<&mut Vec<ImageContent>>,
        sentinels: Option<&mut Vec<TextAreaSentinel>>,
    ) {
        if let Some(committed) = self.history.commit_staged(value) {
            self.text = value.to_string();
            self.cursor = clamp_cursor(&self.text, cursor);
            self.anchor = anchor.map(|a| clamp_cursor(&self.text, a));

            let mut applied_images_inline = false;
            let mut applied_sentinels_inline = false;
            if let Some(staged_metadata) = committed.staged_metadata {
                if let (Some(target), Some(next_images)) = (images, staged_metadata.images) {
                    *target = next_images;
                    applied_images_inline = true;
                }
                if let (Some(target), Some(next_sentinels)) = (sentinels, staged_metadata.sentinels)
                {
                    *target = next_sentinels;
                    applied_sentinels_inline = true;
                }
            }
            if let Some(token) = committed.deferred_callbacks_token
                && let Some(callbacks) = take_deferred_metadata_callbacks(token)
            {
                if !applied_images_inline && let Some((cb, next_images)) = callbacks.images {
                    cb.emit(next_images);
                }
                if !applied_sentinels_inline && let Some((cb, next_sentinels)) = callbacks.sentinels
                {
                    cb.emit(next_sentinels);
                }
                if let Some((cb, events)) = callbacks.sentinel_events {
                    cb.emit(events);
                }
            }
        } else if self.text != value {
            let prev_cursor = self.cursor;
            let prev_anchor = self.anchor;
            self.sync_external_edit_from(value, prev_cursor, prev_anchor, cursor, anchor);
        } else {
            self.cursor = clamp_cursor(&self.text, cursor);
            self.anchor = anchor.map(|a| clamp_cursor(&self.text, a));
        }
    }

    /// Set the text, clamping the cursor and clearing selection.
    pub fn set_text(&mut self, text: impl Into<String>) {
        self.text = text.into();
        self.cursor = clamp_cursor(&self.text, self.cursor);
        self.anchor = self.anchor.map(|a| clamp_cursor(&self.text, a));
        self.last_restored_metadata = None;
        self.history.clear();
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
        let mut history = self.history.take();
        self.last_restored_metadata = history.edit(self, edit);
        self.history.put_back_committed(history, true);
    }

    pub(crate) fn take_last_edit(&mut self) -> Option<TextEditEvent> {
        self.last_edit.take()
    }

    pub(crate) fn take_last_restored_metadata(&mut self) -> Option<RestoredMetadata> {
        self.last_restored_metadata.take()
    }

    pub(crate) fn replace_range(
        &mut self,
        start: usize,
        end: usize,
        inserted: &str,
        kind: TextEditKind,
    ) -> bool {
        self.replace_range_with_metadata(start, end, inserted, kind, None)
    }

    pub(crate) fn replace_range_with_metadata(
        &mut self,
        start: usize,
        end: usize,
        inserted: &str,
        kind: TextEditKind,
        metadata: Option<EditMetadata>,
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
        })
        .with_metadata(metadata);
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
