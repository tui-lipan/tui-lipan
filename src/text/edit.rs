use std::marker::PhantomData;

use std::sync::Arc;

use crate::clipboard::ImageContent;
use crate::widgets::TextAreaSentinel;
use undo::{Edit, Merged};

/// Inline image and custom sentinel metadata attached to a [`TextEdit`] so undo and redo restore
/// the exact metadata for that history transition even when multiple history positions share
/// identical text.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct EditMetadata {
    pub(crate) images_before: Option<Vec<ImageContent>>,
    pub(crate) images_after: Option<Vec<ImageContent>>,
    pub(crate) sentinels_before: Option<Vec<TextAreaSentinel>>,
    pub(crate) sentinels_after: Option<Vec<TextAreaSentinel>>,
}

/// Metadata produced when a [`TextEdit`] is applied or undone.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct RestoredMetadata {
    pub(crate) images: Option<Vec<ImageContent>>,
    pub(crate) sentinels: Option<Vec<TextAreaSentinel>>,
}

pub(crate) trait TextTarget {
    fn with_parts_mut<R>(
        &mut self,
        f: impl FnOnce(&mut String, &mut usize, &mut Option<usize>) -> R,
    ) -> R;
}

/// The kind of text edit that occurred.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextEditKind {
    /// Inserted text at the cursor.
    Insert,
    /// Deleted text using backspace.
    DeleteBackspace,
    /// Deleted text using forward delete.
    DeleteForward,
    /// Replaced a selection or range with new text.
    Replace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InsertGroup {
    Whitespace,
    NonWhitespace,
}

fn insert_group_for_char(ch: char) -> Option<InsertGroup> {
    if ch == '\n' || ch == '\r' {
        return None;
    }
    if ch.is_whitespace() {
        return Some(InsertGroup::Whitespace);
    }
    Some(InsertGroup::NonWhitespace)
}

fn insert_group_last(text: &str) -> Option<InsertGroup> {
    text.chars().last().and_then(insert_group_for_char)
}

fn insert_group_single(text: &str) -> Option<InsertGroup> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    insert_group_for_char(ch)
}

#[derive(Clone, Debug)]
pub(crate) struct TextEdit<T> {
    pub(crate) start: usize,
    pub(crate) deleted: String,
    pub(crate) inserted: String,
    pub(crate) cursor_before: usize,
    pub(crate) anchor_before: Option<usize>,
    pub(crate) cursor_after: usize,
    pub(crate) anchor_after: Option<usize>,
    pub(crate) kind: TextEditKind,
    pub(crate) metadata: Option<Box<EditMetadata>>,
    _target: PhantomData<T>,
}

/// A text edit description for external consumers (e.g. LSP clients).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct TextEditEvent {
    /// Start byte index of the edit.
    pub start: usize,
    /// Deleted text.
    pub deleted: Arc<str>,
    /// Inserted text.
    pub inserted: Arc<str>,
    /// Cursor before the edit.
    pub cursor_before: usize,
    /// Selection anchor before the edit.
    pub anchor_before: Option<usize>,
    /// Cursor after the edit.
    pub cursor_after: usize,
    /// Selection anchor after the edit.
    pub anchor_after: Option<usize>,
    /// Edit kind.
    pub kind: TextEditKind,
}

impl<T> From<&TextEdit<T>> for TextEditEvent {
    fn from(edit: &TextEdit<T>) -> Self {
        Self {
            start: edit.start,
            deleted: Arc::from(edit.deleted.as_str()),
            inserted: Arc::from(edit.inserted.as_str()),
            cursor_before: edit.cursor_before,
            anchor_before: edit.anchor_before,
            cursor_after: edit.cursor_after,
            anchor_after: edit.anchor_after,
            kind: edit.kind,
        }
    }
}

pub(crate) struct TextEditFields {
    pub start: usize,
    pub deleted: String,
    pub inserted: String,
    pub cursor_before: usize,
    pub anchor_before: Option<usize>,
    pub cursor_after: usize,
    pub anchor_after: Option<usize>,
    pub kind: TextEditKind,
}

impl<T> TextEdit<T> {
    pub(crate) fn new(fields: TextEditFields) -> Self {
        Self {
            start: fields.start,
            deleted: fields.deleted,
            inserted: fields.inserted,
            cursor_before: fields.cursor_before,
            anchor_before: fields.anchor_before,
            cursor_after: fields.cursor_after,
            anchor_after: fields.anchor_after,
            kind: fields.kind,
            metadata: None,
            _target: PhantomData,
        }
    }

    pub(crate) fn with_metadata(mut self, metadata: Option<EditMetadata>) -> Self {
        self.metadata = metadata.map(Box::new);
        self
    }

    pub(crate) fn apply_to(
        &self,
        text: &mut String,
        cursor: &mut usize,
        anchor: &mut Option<usize>,
    ) {
        let end = self.start.saturating_add(self.deleted.len());
        text.replace_range(self.start..end, &self.inserted);
        *cursor = self.cursor_after;
        *anchor = self.anchor_after;
    }

    pub(crate) fn undo_on(
        &self,
        text: &mut String,
        cursor: &mut usize,
        anchor: &mut Option<usize>,
    ) {
        let end = self.start.saturating_add(self.inserted.len());
        text.replace_range(self.start..end, &self.deleted);
        *cursor = self.cursor_before;
        *anchor = self.anchor_before;
    }

    pub(crate) fn merge_with(&mut self, other: Self) -> Merged<Self> {
        if self.metadata.is_some() || other.metadata.is_some() {
            return Merged::No(other);
        }
        if self.kind == TextEditKind::DeleteBackspace
            && other.kind == TextEditKind::DeleteBackspace
            && self.inserted.is_empty()
            && other.inserted.is_empty()
            && self.anchor_before.is_none()
            && self.anchor_after.is_none()
            && other.anchor_before.is_none()
            && other.anchor_after.is_none()
            && other.cursor_before == self.cursor_after
            && other.start.saturating_add(other.deleted.len()) == self.start
        {
            self.start = other.start;
            self.deleted.reserve(other.deleted.len());
            self.deleted.insert_str(0, &other.deleted);
            self.cursor_after = other.cursor_after;
            self.anchor_after = other.anchor_after;
            return Merged::Yes;
        }

        if self.kind == TextEditKind::DeleteForward
            && other.kind == TextEditKind::DeleteForward
            && self.inserted.is_empty()
            && other.inserted.is_empty()
            && self.anchor_before.is_none()
            && self.anchor_after.is_none()
            && other.anchor_before.is_none()
            && other.anchor_after.is_none()
            && self.start == other.start
            && self.cursor_before == other.cursor_before
            && self.cursor_after == other.cursor_after
        {
            self.deleted.push_str(&other.deleted);
            return Merged::Yes;
        }

        if self.kind == TextEditKind::Insert
            && other.kind == TextEditKind::Insert
            && self.deleted.is_empty()
            && other.deleted.is_empty()
            && self.anchor_before.is_none()
            && self.anchor_after.is_none()
            && other.anchor_before.is_none()
            && other.anchor_after.is_none()
            && other.cursor_before == self.cursor_after
            && other.start == self.start.saturating_add(self.inserted.len())
        {
            let group = insert_group_last(&self.inserted);
            let other_group = insert_group_single(&other.inserted);
            let should_merge = matches!(
                (group, other_group),
                (Some(InsertGroup::Whitespace), Some(InsertGroup::Whitespace))
                    | (
                        Some(InsertGroup::NonWhitespace),
                        Some(InsertGroup::NonWhitespace)
                    )
                    | (
                        Some(InsertGroup::NonWhitespace),
                        Some(InsertGroup::Whitespace)
                    )
            );
            if should_merge {
                self.inserted.push_str(&other.inserted);
                self.cursor_after = other.cursor_after;
                self.anchor_after = other.anchor_after;
                return Merged::Yes;
            }
        }

        Merged::No(other)
    }
}

impl<T: TextTarget> Edit for TextEdit<T> {
    type Target = T;
    type Output = Option<RestoredMetadata>;

    fn edit(&mut self, target: &mut Self::Target) -> Self::Output {
        target.with_parts_mut(|text, cursor, anchor| {
            self.apply_to(text, cursor, anchor);
        });
        self.metadata.as_deref().map(|meta| RestoredMetadata {
            images: meta.images_after.clone(),
            sentinels: meta.sentinels_after.clone(),
        })
    }

    fn undo(&mut self, target: &mut Self::Target) -> Self::Output {
        target.with_parts_mut(|text, cursor, anchor| {
            self.undo_on(text, cursor, anchor);
        });
        self.metadata.as_deref().map(|meta| RestoredMetadata {
            images: meta.images_before.clone(),
            sentinels: meta.sentinels_before.clone(),
        })
    }

    fn merge(&mut self, other: Self) -> Merged<Self> {
        self.merge_with(other)
    }
}
