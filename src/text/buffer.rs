use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use crate::clipboard::ImageContent;
use crate::text::edit::{TextEdit, TextEditEvent, TextEditFields, TextEditKind, TextTarget};
use crate::utils::text::clamp_cursor;
use crate::utils::text::{
    next_char_boundary, prev_char_boundary, word_boundary_left, word_boundary_right,
};
use crate::widgets::TextAreaSentinel;
use undo::Record;

const DEFAULT_HISTORY_LIMIT: usize = 1000;
const MAX_STAGED_HISTORY_STEPS: usize = 64;
const SENTINEL_SNAPSHOT_LIMIT: usize = 1000;

pub(crate) type CoreEdit = TextEdit<TextBufferCore>;

#[derive(Clone, Debug)]
pub(crate) struct ImageSnapshot {
    pub(crate) text: String,
    pub(crate) images: Vec<ImageContent>,
}

#[derive(Clone, Debug)]
pub(crate) struct SentinelSnapshot {
    pub(crate) text: String,
    pub(crate) sentinels: Vec<TextAreaSentinel>,
}

#[derive(Debug)]
struct StagedHistoryStep {
    expected_text: Arc<str>,
    record: Arc<Record<CoreEdit>>,
    image_snapshots: Vec<ImageSnapshot>,
    sentinel_snapshots: Vec<SentinelSnapshot>,
}

#[derive(Debug)]
struct StagedHistory {
    base_generation: u64,
    steps: VecDeque<StagedHistoryStep>,
}

#[derive(Debug)]
struct SharedHistoryInner {
    /// Copy-on-write undo/redo record so snapshots and non-mutating operations avoid deep clones.
    record: Arc<Record<CoreEdit>>,
    generation: u64,
    staged: Option<StagedHistory>,
    image_snapshots: Vec<ImageSnapshot>,
    sentinel_snapshots: Vec<SentinelSnapshot>,
}

fn push_image_snapshot(snapshots: &mut Vec<ImageSnapshot>, text: &str, images: &[ImageContent]) {
    if images.is_empty() {
        return;
    }
    if let Some(pos) = snapshots.iter().position(|snapshot| snapshot.text == text) {
        snapshots.remove(pos);
    }
    snapshots.push(ImageSnapshot {
        text: text.to_owned(),
        images: images.to_vec(),
    });
    if snapshots.len() > SENTINEL_SNAPSHOT_LIMIT {
        snapshots.remove(0);
    }
}

fn push_sentinel_snapshot(
    snapshots: &mut Vec<SentinelSnapshot>,
    text: &str,
    sentinels: &[TextAreaSentinel],
) {
    if sentinels.is_empty() {
        return;
    }
    if let Some(pos) = snapshots.iter().position(|snapshot| snapshot.text == text) {
        snapshots.remove(pos);
    }
    snapshots.push(SentinelSnapshot {
        text: text.to_owned(),
        sentinels: sentinels.to_vec(),
    });
    if snapshots.len() > SENTINEL_SNAPSHOT_LIMIT {
        snapshots.remove(0);
    }
}

/// Undo history backing a [`TextBufferCore`].
///
/// Cloning a `SharedHistory` creates a new independent handle sharing only the immutable
/// copy-on-write [`Record`] until mutation, so cloning a [`TextEditor`](super::TextEditor) or
/// [`TextInput`](super::TextInput) is O(1) while preserving independent undo stacks. Sharing the
/// same history with a bound [`TextArea`](crate::widgets::TextArea) goes explicitly through
/// [`SharedHistoryHandle`], where runtime edits are staged until accepted via
/// [`TextAreaEvent::apply_to`](crate::widgets::TextAreaEvent::apply_to) or
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
            image_snapshots: Vec::new(),
            sentinel_snapshots: Vec::new(),
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

    pub(crate) fn remember_images(&self, text: &str, images: &[ImageContent]) {
        let mut inner = self.lock();
        push_image_snapshot(&mut inner.image_snapshots, text, images);
    }

    pub(crate) fn remembered_images(&self, text: &str) -> Option<Vec<ImageContent>> {
        self.lock()
            .image_snapshots
            .iter()
            .rev()
            .find(|snapshot| snapshot.text == text)
            .map(|snapshot| snapshot.images.clone())
    }

    pub(crate) fn remember_sentinels(&self, text: &str, sentinels: &[TextAreaSentinel]) {
        let mut inner = self.lock();
        push_sentinel_snapshot(&mut inner.sentinel_snapshots, text, sentinels);
    }

    pub(crate) fn remembered_sentinels(&self, text: &str) -> Option<Vec<TextAreaSentinel>> {
        self.lock()
            .sentinel_snapshots
            .iter()
            .rev()
            .find(|snapshot| snapshot.text == text)
            .map(|snapshot| snapshot.sentinels.clone())
    }

    pub(crate) fn clone_metadata_snapshots(&self) -> (Vec<ImageSnapshot>, Vec<SentinelSnapshot>) {
        let inner = self.lock();
        (
            inner.image_snapshots.clone(),
            inner.sentinel_snapshots.clone(),
        )
    }

    pub(crate) fn clear(&self) {
        let mut inner = self.lock();
        inner.record = Arc::new(TextBufferCore::fresh_record());
        inner.generation = inner.generation.wrapping_add(1);
        inner.staged = None;
        inner.image_snapshots.clear();
        inner.sentinel_snapshots.clear();
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
            inner.staged = None;
        }
    }

    pub(crate) fn commit_staged(&self, expected_text: &str) -> bool {
        let mut inner = self.lock();
        let current_generation = inner.generation;
        let Some(staged) = inner.staged.as_mut() else {
            return false;
        };
        if staged.base_generation != current_generation {
            inner.staged = None;
            return false;
        }
        let Some(pos) = staged
            .steps
            .iter()
            .position(|step| step.expected_text.as_ref() == expected_text)
        else {
            inner.staged = None;
            return false;
        };
        for _ in 0..pos {
            staged.steps.pop_front();
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
        for snapshot in step.image_snapshots {
            push_image_snapshot(&mut inner.image_snapshots, &snapshot.text, &snapshot.images);
        }
        for snapshot in step.sentinel_snapshots {
            push_sentinel_snapshot(
                &mut inner.sentinel_snapshots,
                &snapshot.text,
                &snapshot.sentinels,
            );
        }
        true
    }
}

impl Clone for SharedHistory {
    fn clone(&self) -> Self {
        let inner = self.lock();
        Self(Arc::new(Mutex::new(SharedHistoryInner {
            record: Arc::clone(&inner.record),
            generation: 0,
            staged: None,
            image_snapshots: inner.image_snapshots.clone(),
            sentinel_snapshots: inner.sentinel_snapshots.clone(),
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
        inner.staged = None;
        (Arc::clone(&inner.record), inner.generation)
    }

    pub(crate) fn remembered_images(&self, text: &str) -> Option<Vec<ImageContent>> {
        let inner = self.lock();
        if let Some(staged) = inner.staged.as_ref()
            && staged.base_generation == inner.generation
            && let Some(found) = staged.steps.iter().rev().find_map(|step| {
                step.image_snapshots
                    .iter()
                    .rev()
                    .find(|snapshot| snapshot.text == text)
                    .map(|snapshot| snapshot.images.clone())
            })
        {
            return Some(found);
        }
        inner
            .image_snapshots
            .iter()
            .rev()
            .find(|snapshot| snapshot.text == text)
            .map(|snapshot| snapshot.images.clone())
    }

    pub(crate) fn remembered_sentinels(&self, text: &str) -> Option<Vec<TextAreaSentinel>> {
        let inner = self.lock();
        if let Some(staged) = inner.staged.as_ref()
            && staged.base_generation == inner.generation
            && let Some(found) = staged.steps.iter().rev().find_map(|step| {
                step.sentinel_snapshots
                    .iter()
                    .rev()
                    .find(|snapshot| snapshot.text == text)
                    .map(|snapshot| snapshot.sentinels.clone())
            })
        {
            return Some(found);
        }
        inner
            .sentinel_snapshots
            .iter()
            .rev()
            .find(|snapshot| snapshot.text == text)
            .map(|snapshot| snapshot.sentinels.clone())
    }

    /// Stage a runtime history checkpoint to be committed when the matching `TextAreaEvent` is
    /// accepted by `TextAreaEvent::apply_to` or `TextEditor::apply`.
    pub(crate) fn stage_step(
        &self,
        base_generation: u64,
        expected_text: Arc<str>,
        record: Arc<Record<CoreEdit>>,
        image_snapshots: Vec<ImageSnapshot>,
        sentinel_snapshots: Vec<SentinelSnapshot>,
    ) {
        let mut inner = self.lock();
        if inner.generation != base_generation {
            return;
        }
        let step = StagedHistoryStep {
            expected_text,
            record,
            image_snapshots,
            sentinel_snapshots,
        };
        if let Some(staged) = inner.staged.as_mut()
            && staged.base_generation == base_generation
        {
            if staged.steps.len() >= MAX_STAGED_HISTORY_STEPS {
                staged.steps.pop_front();
            }
            staged.steps.push_back(step);
        } else {
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
}

impl Default for TextBufferCore {
    fn default() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            anchor: None,
            history: SharedHistory::fresh(),
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
        let changed = history.undo(self).is_some();
        self.history.put_back_committed(history, changed);
        changed
    }

    /// Redo the most recently undone edit.
    pub fn redo(&mut self) -> bool {
        let mut history = self.history.take();
        let changed = history.redo(self).is_some();
        self.history.put_back_committed(history, changed);
        changed
    }

    /// Clear all undo/redo history without changing the text.
    pub fn clear_history(&mut self) {
        self.history.clear();
    }

    pub(crate) fn reset_history(&mut self) {
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
        });
        let event = TextEditEvent::from(&edit);
        self.record_edit(edit);
        event
    }

    /// Apply a controlled `TextAreaEvent` back to a bound buffer, committing the matching staged
    /// runtime history step when the text was accepted verbatim, or recording a clean replacement
    /// edit when the application transformed the text.
    pub(crate) fn apply_controlled_change(
        &mut self,
        value: &str,
        cursor: usize,
        anchor: Option<usize>,
    ) {
        if self.history.commit_staged(value) {
            self.text = value.to_string();
            self.cursor = clamp_cursor(&self.text, cursor);
            self.anchor = anchor.map(|a| clamp_cursor(&self.text, a));
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
        history.edit(self, edit);
        self.history.put_back_committed(history, true);
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
