use std::sync::Arc;

use crate::callback::Callback;
use crate::clipboard::ImageContent;
use crate::text::edit::TextEditEvent;
use crate::text::editor::TextEditor;
use crate::text::input::TextInput;
use crate::utils::text::{SentinelInfo, replace_sentinels};
use crate::widgets::{
    IMAGE_SENTINEL_BASE, TextAreaClipboardTransform, TextAreaClipboardTransformEvent,
    TextAreaImageMode, TextAreaStateChangeEvent, TextAreaStateChangeReason, sentinel_info_for,
};
use crate::widgets::{InputEvent, TextAreaEvent, TextAreaPasteEvent};

pub(crate) trait ClipboardContext {
    fn selection_text(&self) -> Option<String>;
    fn can_copy(&self) -> bool;
    fn can_cut(&self) -> bool;
    fn can_paste(&self) -> bool;
    fn block_copy_cut(&self) -> bool {
        false
    }
    fn delete_selection(&mut self) -> bool;
    fn insert_text(&mut self, text: &str) -> bool;
    fn handle_text_paste(&mut self, _text: &str) -> bool {
        false
    }
    /// Insert image content. Returns `false` by default (most widgets don't support images).
    fn insert_image(&mut self, _content: &ImageContent) -> bool {
        false
    }
    /// Get the current image content to copy, if the widget supports image copy.
    /// Returns `None` by default.
    fn get_image(&self) -> Option<ImageContent> {
        None
    }
    /// Returns `true` when the widget can handle image paste (i.e., has an `on_image_paste`
    /// callback). Used by the `Paste` command to auto-detect image content. Defaults to `false`.
    fn accepts_image(&self) -> bool {
        false
    }
}

/// Remove bytes whose positions (in the *original* text, starting at `slice_offset`)
/// fall within any of the given excluded ranges. Ranges are in original-text coordinates.
fn filter_excluded_bytes(slice: &str, excluded: &[(usize, usize)], slice_offset: usize) -> String {
    let mut result = String::with_capacity(slice.len());
    let mut pos = slice_offset;
    for ch in slice.chars() {
        let ch_end = pos + ch.len_utf8();
        let keep = !excluded.iter().any(|(s, e)| pos >= *s && pos < *e);
        if keep {
            result.push(ch);
        }
        pos = ch_end;
    }
    result
}

pub(crate) fn selection_range(
    cursor: usize,
    anchor: Option<usize>,
    len: usize,
) -> Option<(usize, usize)> {
    let anchor = anchor?;
    let start = cursor.min(anchor);
    let end = cursor.max(anchor);
    if start < end && end <= len {
        Some((start, end))
    } else {
        None
    }
}

pub(crate) struct ReadOnlyClipboardContext<'a> {
    text: &'a str,
    selection: Option<(usize, usize)>,
    copy_allowed: bool,
    block_copy_cut: bool,
    sentinel: Option<SentinelInfo>,
    sentinel_placeholder: Option<&'a str>,
    excluded_bytes: &'a [(usize, usize)],
    clipboard_transform: Option<TextAreaClipboardTransform>,
}

impl<'a> ReadOnlyClipboardContext<'a> {
    pub(crate) fn new(
        text: &'a str,
        selection: Option<(usize, usize)>,
        copy_allowed: bool,
        block_copy_cut: bool,
    ) -> Self {
        Self {
            text,
            selection,
            copy_allowed,
            block_copy_cut,
            sentinel: None,
            sentinel_placeholder: None,
            excluded_bytes: &[],
            clipboard_transform: None,
        }
    }

    pub(crate) fn with_sentinel(
        mut self,
        sentinel: Option<SentinelInfo>,
        placeholder: &'a str,
    ) -> Self {
        self.sentinel = sentinel;
        self.sentinel_placeholder = Some(placeholder);
        self
    }

    pub(crate) fn with_excluded_bytes(mut self, ranges: &'a [(usize, usize)]) -> Self {
        self.excluded_bytes = ranges;
        self
    }

    pub(crate) fn with_clipboard_transform(
        mut self,
        transform: Option<TextAreaClipboardTransform>,
    ) -> Self {
        self.clipboard_transform = transform;
        self
    }
}

impl ClipboardContext for ReadOnlyClipboardContext<'_> {
    fn selection_text(&self) -> Option<String> {
        let (start, end) = self.selection?;
        let slice = self.text.get(start..end)?;
        let placeholder = self.sentinel_placeholder.unwrap_or("[Image]");
        let text = replace_sentinels(slice, self.sentinel.as_ref(), placeholder);
        let text = if self.excluded_bytes.is_empty() {
            text.into_owned()
        } else {
            filter_excluded_bytes(&text, self.excluded_bytes, start)
        };
        Some(match &self.clipboard_transform {
            Some(transform) => transform(TextAreaClipboardTransformEvent {
                text: &text,
                raw_text: slice,
            }),
            None => text,
        })
    }

    fn can_copy(&self) -> bool {
        self.copy_allowed
    }

    fn can_cut(&self) -> bool {
        false
    }

    fn can_paste(&self) -> bool {
        false
    }

    fn block_copy_cut(&self) -> bool {
        self.block_copy_cut
    }

    fn delete_selection(&mut self) -> bool {
        false
    }

    fn insert_text(&mut self, _text: &str) -> bool {
        false
    }
}

pub(crate) struct InputClipboardContext<'a> {
    input: &'a mut TextInput,
    on_change: Option<&'a Callback<InputEvent>>,
    on_edit: Option<&'a Callback<TextEditEvent>>,
    copy_allowed: bool,
    editable: bool,
    block_copy_cut: bool,
}

impl<'a> InputClipboardContext<'a> {
    pub(crate) fn new(
        input: &'a mut TextInput,
        on_change: Option<&'a Callback<InputEvent>>,
        on_edit: Option<&'a Callback<TextEditEvent>>,
        copy_allowed: bool,
        editable: bool,
        block_copy_cut: bool,
    ) -> Self {
        Self {
            input,
            on_change,
            on_edit,
            copy_allowed,
            editable,
            block_copy_cut,
        }
    }

    fn emit_change(&self) {
        let Some(cb) = self.on_change else {
            return;
        };
        cb.emit(InputEvent {
            value: Arc::from(self.input.text().to_owned()),
            cursor: self.input.cursor(),
            anchor: self.input.anchor(),
        });
    }

    fn emit_edit(&mut self) {
        let Some(cb) = self.on_edit else {
            return;
        };
        if let Some(edit) = self.input.take_last_edit() {
            cb.emit(edit);
        }
    }
}

impl ClipboardContext for InputClipboardContext<'_> {
    fn selection_text(&self) -> Option<String> {
        self.input.selected_text().map(|text| text.to_string())
    }

    fn can_copy(&self) -> bool {
        self.copy_allowed
    }

    fn can_cut(&self) -> bool {
        self.editable && self.copy_allowed
    }

    fn can_paste(&self) -> bool {
        self.editable
    }

    fn block_copy_cut(&self) -> bool {
        self.block_copy_cut
    }

    fn delete_selection(&mut self) -> bool {
        let changed = self.input.delete_selection();
        if changed {
            self.emit_edit();
            self.emit_change();
        }
        changed
    }

    fn insert_text(&mut self, text: &str) -> bool {
        let changed = self.input.insert_text(text);
        if changed {
            self.emit_edit();
            self.emit_change();
        }
        changed
    }
}

pub(crate) struct TextAreaClipboardParams<'a> {
    pub on_change: Option<&'a Callback<TextAreaEvent>>,
    pub on_edit: Option<&'a Callback<TextEditEvent>>,
    pub on_editor_state_change: Option<&'a Callback<TextAreaStateChangeEvent>>,
    pub on_image_paste: Option<&'a Callback<ImageContent>>,
    pub on_text_paste: Option<&'a Callback<TextAreaPasteEvent>>,
    pub images: &'a [ImageContent],
    pub on_images_change: Option<&'a Callback<Vec<ImageContent>>>,
    pub image_mode: TextAreaImageMode,
    pub image_placeholder: &'a str,
    pub sentinels: &'a [crate::widgets::TextAreaSentinel],
    pub clipboard_transform: Option<TextAreaClipboardTransform>,
    pub editable: bool,
}

pub(crate) struct TextAreaClipboardContext<'a> {
    editor: &'a mut TextEditor,
    on_change: Option<&'a Callback<TextAreaEvent>>,
    on_edit: Option<&'a Callback<TextEditEvent>>,
    on_editor_state_change: Option<&'a Callback<TextAreaStateChangeEvent>>,
    on_image_paste: Option<&'a Callback<ImageContent>>,
    on_text_paste: Option<&'a Callback<TextAreaPasteEvent>>,
    images: &'a [ImageContent],
    on_images_change: Option<&'a Callback<Vec<ImageContent>>>,
    image_mode: TextAreaImageMode,
    image_placeholder: &'a str,
    sentinels: &'a [crate::widgets::TextAreaSentinel],
    clipboard_transform: Option<TextAreaClipboardTransform>,
    editable: bool,
}

impl<'a> TextAreaClipboardContext<'a> {
    pub(crate) fn new(editor: &'a mut TextEditor, params: TextAreaClipboardParams<'a>) -> Self {
        Self {
            editor,
            on_change: params.on_change,
            on_edit: params.on_edit,
            on_editor_state_change: params.on_editor_state_change,
            on_image_paste: params.on_image_paste,
            on_text_paste: params.on_text_paste,
            images: params.images,
            on_images_change: params.on_images_change,
            image_mode: params.image_mode,
            image_placeholder: params.image_placeholder,
            sentinels: params.sentinels,
            clipboard_transform: params.clipboard_transform,
            editable: params.editable,
        }
    }

    fn emit_change(&self, edit: Option<TextEditEvent>) {
        let value: Arc<str> = Arc::from(self.editor.text().to_owned());
        let cursor = self.editor.cursor();
        let anchor = self.editor.anchor();
        if let Some(cb) = self.on_change {
            cb.emit(TextAreaEvent {
                value: value.clone(),
                cursor,
                anchor,
            });
        }
        if let Some(cb) = self.on_editor_state_change {
            cb.emit(TextAreaStateChangeEvent {
                reason: if edit.is_some() {
                    TextAreaStateChangeReason::Edit
                } else if anchor.is_some() {
                    TextAreaStateChangeReason::SelectionChange
                } else {
                    TextAreaStateChangeReason::CursorMove
                },
                value,
                cursor,
                anchor,
                edit,
                vim_mode: None,
            });
        }
    }

    fn emit_edit(&mut self) -> Option<TextEditEvent> {
        let edit = self.editor.take_last_edit();
        if let (Some(cb), Some(edit)) = (self.on_edit, edit.clone()) {
            cb.emit(edit);
        }
        edit
    }
}

impl ClipboardContext for TextAreaClipboardContext<'_> {
    fn selection_text(&self) -> Option<String> {
        let raw = self.editor.selected_text()?;
        let sentinel = sentinel_info_for(
            self.image_mode,
            self.images.len(),
            self.image_placeholder,
            self.sentinels,
        );
        let text = replace_sentinels(raw, sentinel.as_ref(), self.image_placeholder).into_owned();
        Some(match &self.clipboard_transform {
            Some(transform) => transform(TextAreaClipboardTransformEvent {
                text: &text,
                raw_text: raw,
            }),
            None => text,
        })
    }

    fn can_copy(&self) -> bool {
        true
    }

    fn can_cut(&self) -> bool {
        self.editable
    }

    fn can_paste(&self) -> bool {
        self.editable
    }

    fn delete_selection(&mut self) -> bool {
        let changed = self.editor.delete_selection();
        if changed {
            let edit = self.emit_edit();
            self.emit_change(edit);
        }
        changed
    }

    fn insert_text(&mut self, text: &str) -> bool {
        let changed = self.editor.insert_text(text);
        if changed {
            let edit = self.emit_edit();
            self.emit_change(edit);
        }
        changed
    }

    fn handle_text_paste(&mut self, text: &str) -> bool {
        let Some(cb) = self.on_text_paste else {
            return false;
        };
        self.editor.expect_external_paste_change();
        cb.emit(TextAreaPasteEvent {
            text: Arc::from(text.to_owned()),
            cursor: self.editor.cursor(),
            anchor: self.editor.anchor(),
        });
        true
    }

    fn insert_image(&mut self, content: &ImageContent) -> bool {
        // Always fire the legacy on_image_paste callback if set.
        if let Some(cb) = self.on_image_paste {
            cb.emit(content.clone());
        }

        // If there is no on_images_change callback, we cannot manage the images list.
        let Some(on_images_change) = self.on_images_change else {
            return self.on_image_paste.is_some();
        };

        let mut new_images = self.images.to_vec();
        let new_index = new_images.len();
        new_images.push(content.clone());

        match self.image_mode {
            TextAreaImageMode::Inline => {
                // Insert the sentinel character at the current cursor position.
                let sentinel = char::from_u32(IMAGE_SENTINEL_BASE as u32 + new_index as u32)
                    .unwrap_or(IMAGE_SENTINEL_BASE);
                let insert_end = self
                    .editor
                    .selection()
                    .map(|(_, end)| end)
                    .unwrap_or_else(|| self.editor.cursor());
                let tail_starts_with_space = self
                    .editor
                    .text()
                    .get(insert_end..)
                    .is_some_and(|tail| tail.starts_with(' '));
                let mut text = sentinel.to_string();
                if !tail_starts_with_space {
                    text.push(' ');
                }
                if self.editor.insert_text(&text) {
                    let edit = self.emit_edit();
                    self.emit_change(edit);
                }
            }
            TextAreaImageMode::Attachment => {
                // Attachment mode: do not modify the editor value; just update the list.
            }
        }

        on_images_change.emit(new_images);
        true
    }

    fn accepts_image(&self) -> bool {
        self.on_image_paste.is_some() || self.on_images_change.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_clipboard_context_inserts_and_emits() {
        let mut input = TextInput::new("hello");
        input.set_cursor(5);

        let last = std::rc::Rc::new(std::cell::RefCell::new(None));
        let last_ref = last.clone();
        let cb = Callback::new(move |event: InputEvent| {
            *last_ref.borrow_mut() = Some(event);
        });

        let mut ctx = InputClipboardContext::new(&mut input, Some(&cb), None, true, true, false);
        assert!(ctx.insert_text(" world"));

        let event = last.borrow().clone().expect("event emitted");
        assert_eq!(event.value.as_ref(), "hello world");
    }

    #[test]
    fn text_area_image_insert_adds_trailing_space() {
        use crate::clipboard::ImageFormat;

        let mut editor = TextEditor::new("hello");
        editor.set_cursor(5);
        let image = ImageContent::from_bytes(b"fake-png", ImageFormat::Png);
        let changed_images = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let changed_images_ref = changed_images.clone();
        let images_cb = Callback::new(move |images: Vec<ImageContent>| {
            *changed_images_ref.borrow_mut() = images;
        });
        let change = std::rc::Rc::new(std::cell::RefCell::new(None));
        let change_ref = change.clone();
        let change_cb = Callback::new(move |event: TextAreaEvent| {
            *change_ref.borrow_mut() = Some(event);
        });

        {
            let mut ctx = TextAreaClipboardContext::new(
                &mut editor,
                TextAreaClipboardParams {
                    on_change: Some(&change_cb),
                    on_edit: None,
                    on_editor_state_change: None,
                    on_image_paste: None,
                    on_text_paste: None,
                    images: &[],
                    on_images_change: Some(&images_cb),
                    image_mode: TextAreaImageMode::Inline,
                    image_placeholder: "[Image X]",
                    sentinels: &[],
                    clipboard_transform: None,
                    editable: true,
                },
            );
            assert!(ctx.insert_image(&image));
        }

        let expected = format!("hello{} ", IMAGE_SENTINEL_BASE);
        assert_eq!(editor.text(), expected);
        assert_eq!(editor.cursor(), expected.len());
        assert_eq!(
            changed_images.borrow().as_slice(),
            std::slice::from_ref(&image)
        );
        let event = change.borrow().clone().expect("change event emitted");
        assert_eq!(event.value.as_ref(), expected);
        assert_eq!(event.cursor, expected.len());
    }

    #[test]
    fn text_area_image_insert_reuses_existing_trailing_space() {
        use crate::clipboard::ImageFormat;

        let mut editor = TextEditor::new("hello world");
        editor.set_cursor(5);
        let image = ImageContent::from_bytes(b"fake-png", ImageFormat::Png);
        let images_cb = Callback::new(|_: Vec<ImageContent>| {});

        {
            let mut ctx = TextAreaClipboardContext::new(
                &mut editor,
                TextAreaClipboardParams {
                    on_change: None,
                    on_edit: None,
                    on_editor_state_change: None,
                    on_image_paste: None,
                    on_text_paste: None,
                    images: &[],
                    on_images_change: Some(&images_cb),
                    image_mode: TextAreaImageMode::Inline,
                    image_placeholder: "[Image X]",
                    sentinels: &[],
                    clipboard_transform: None,
                    editable: true,
                },
            );
            assert!(ctx.insert_image(&image));
        }

        let expected = format!("hello{} world", IMAGE_SENTINEL_BASE);
        assert_eq!(editor.text(), expected);
        assert_eq!(
            editor.cursor(),
            "hello".len() + IMAGE_SENTINEL_BASE.len_utf8()
        );
    }

    #[test]
    fn read_only_context_copies_selection() {
        let text = "hello";
        let selection = selection_range(1, Some(4), text.len());
        let ctx = ReadOnlyClipboardContext::new(text, selection, true, false);
        assert_eq!(ctx.selection_text().as_deref(), Some("ell"));
    }

    #[test]
    fn text_area_selection_copy_is_unchanged_without_transform() {
        let mut editor = TextEditor::new("hello world");
        editor.set_cursor(11);
        editor.set_anchor(Some(6));

        let ctx = TextAreaClipboardContext::new(
            &mut editor,
            TextAreaClipboardParams {
                on_change: None,
                on_edit: None,
                on_editor_state_change: None,
                on_image_paste: None,
                on_text_paste: None,
                images: &[],
                on_images_change: None,
                image_mode: TextAreaImageMode::Inline,
                image_placeholder: "[Image]",
                sentinels: &[],
                clipboard_transform: None,
                editable: true,
            },
        );

        assert_eq!(ctx.selection_text().as_deref(), Some("world"));
    }

    #[test]
    fn text_area_selection_copy_applies_opt_in_transform() {
        let mut editor = TextEditor::new("hello world");
        editor.set_cursor(11);
        editor.set_anchor(Some(6));

        let ctx = TextAreaClipboardContext::new(
            &mut editor,
            TextAreaClipboardParams {
                on_change: None,
                on_edit: None,
                on_editor_state_change: None,
                on_image_paste: None,
                on_text_paste: None,
                images: &[],
                on_images_change: None,
                image_mode: TextAreaImageMode::Inline,
                image_placeholder: "[Image]",
                sentinels: &[],
                clipboard_transform: Some(std::sync::Arc::new(|event| event.text.to_uppercase())),
                editable: true,
            },
        );

        assert_eq!(ctx.selection_text().as_deref(), Some("WORLD"));
    }
}
