use super::*;
use crate::text::editor::{
    custom_sentinel_required_len, image_sentinel_required_len, prune_images_for_value,
    prune_sentinels_for_value,
};

pub(super) struct TextAreaEmission<'a> {
    pub(super) on_change: &'a Callback<TextAreaEvent>,
    pub(super) on_edit: Option<&'a Callback<crate::text::edit::TextEditEvent>>,
    pub(super) on_editor_state_change:
        Option<&'a Callback<crate::widgets::TextAreaStateChangeEvent>>,
    pub(super) old_value: &'a str,
    pub(super) images: &'a [ImageContent],
    pub(super) image_mode: TextAreaImageMode,
    pub(super) sentinels: &'a [TextAreaSentinel],
    pub(super) on_images_change: Option<&'a Callback<Vec<ImageContent>>>,
    pub(super) on_sentinels_change: Option<&'a Callback<Vec<TextAreaSentinel>>>,
    pub(super) on_sentinel_event: Option<&'a Callback<Vec<SentinelEvent>>>,
}

pub(super) fn finish_text_area_edit_if_handled(
    tree: &mut NodeTree,
    id: NodeId,
    editor: &mut TextEditor,
    emission: &TextAreaEmission<'_>,
    handled: bool,
) -> bool {
    if !handled {
        return false;
    }

    emit_text_area_editor_change(tree, id, editor, emission)
}

pub(super) fn emit_text_area_editor_change(
    tree: &mut NodeTree,
    id: NodeId,
    editor: &mut TextEditor,
    emission: &TextAreaEmission<'_>,
) -> bool {
    editor.remember_text_area_sentinels(emission.old_value, emission.sentinels);
    let mut edit = editor.take_last_edit();
    let is_history_replay = edit.is_none() && editor.text() != emission.old_value;
    let mut current_value = editor.text().to_owned();
    let cursor = editor.cursor();
    let anchor = editor.anchor();

    // Prune or restore images list if inline image sentinel chars changed.
    // In attachment mode, sentinels are never inserted into the value,
    // so sentinel-based pruning would incorrectly clear the images list.
    let mut pending_images: Option<Vec<ImageContent>> = None;
    if emission.image_mode == TextAreaImageMode::Inline && emission.on_images_change.is_some() {
        editor.remember_text_area_images(emission.old_value, emission.images);
        let required_images = image_sentinel_required_len(&current_value);
        let image_source = if required_images == 0 {
            (!emission.images.is_empty()).then(|| emission.images.to_vec())
        } else if is_history_replay {
            editor
                .remembered_text_area_images(&current_value)
                .filter(|images| required_images <= images.len())
                .or_else(|| {
                    (required_images <= emission.images.len()).then(|| emission.images.to_vec())
                })
        } else if required_images <= emission.images.len() {
            Some(emission.images.to_vec())
        } else {
            editor
                .remembered_text_area_images(&current_value)
                .filter(|images| required_images <= images.len())
        };
        if let Some(image_source) = image_source {
            let (pruned, remapped) = prune_images_for_value(&current_value, &image_source);
            if let Some(remapped_val) = remapped {
                current_value = remapped_val;
            }
            if pruned.as_slice() != emission.images {
                pending_images = Some(pruned.clone());
            }
            editor.remember_text_area_images(&current_value, &pruned);
        }
    }

    // Prune or restore custom sentinels list.
    let mut pending_sentinels: Option<Vec<TextAreaSentinel>> = None;
    let mut pending_sentinel_events: Vec<SentinelEvent> = Vec::new();
    let required_sentinels = custom_sentinel_required_len(&current_value);
    let sentinel_source = if required_sentinels == 0 {
        (!emission.sentinels.is_empty()).then(|| emission.sentinels.to_vec())
    } else if is_history_replay {
        editor
            .remembered_text_area_sentinels(&current_value)
            .filter(|sentinels| required_sentinels <= sentinels.len())
            .or_else(|| {
                (required_sentinels <= emission.sentinels.len())
                    .then(|| emission.sentinels.to_vec())
            })
    } else if required_sentinels <= emission.sentinels.len() {
        Some(emission.sentinels.to_vec())
    } else {
        editor
            .remembered_text_area_sentinels(&current_value)
            .filter(|sentinels| required_sentinels <= sentinels.len())
    };
    if let Some(sentinel_source) = sentinel_source
        && (emission.on_sentinels_change.is_some() || emission.on_sentinel_event.is_some())
    {
        let (pruned, remapped, events) =
            prune_sentinels_for_value(&current_value, &sentinel_source);
        if let Some(remapped_val) = remapped {
            current_value = remapped_val;
        }
        if pruned.as_slice() != emission.sentinels {
            pending_sentinels = Some(pruned.clone());
            pending_sentinel_events = events;
        }
        editor.remember_text_area_sentinels(&current_value, &pruned);
    }

    // When sentinel pruning remapped remaining sentinel indices, rewrite the active undo entry
    // atomically so history describes the final emitted text instead of an intermediate state.
    if current_value != editor.text()
        && let Some(rewritten_edit) = editor.rewrite_pending_edit_to(&current_value, cursor, anchor)
    {
        edit = Some(rewritten_edit);
    }

    let new_value: Arc<str> = Arc::from(editor.text().to_owned());
    let cursor = editor.cursor();
    let anchor = editor.anchor();
    editor.stage_bound_history(&new_value);

    if let (Some(cb), Some(edit)) = (emission.on_edit, edit.clone()) {
        cb.emit(edit);
    }
    emission.on_change.emit(TextAreaEvent {
        value: new_value.clone(),
        cursor,
        anchor,
    });
    emit_editor_state_change(emission, new_value.clone(), cursor, anchor, edit, None);

    if let (Some(on_img_cb), Some(pruned)) = (emission.on_images_change, pending_images) {
        on_img_cb.emit(pruned);
    }
    if let Some(pruned) = pending_sentinels {
        if let Some(on_sent_cb) = emission.on_sentinels_change {
            on_sent_cb.emit(pruned);
        }
        if !pending_sentinel_events.is_empty()
            && let Some(ev_cb) = emission.on_sentinel_event
        {
            ev_cb.emit(pending_sentinel_events);
        }
    }

    if let NodeKind::TextArea(ta) = &mut tree.node_mut(id).kind {
        ta.uncontrolled.record_reported(new_value, cursor, anchor);
    }

    cancel_text_area_smooth_scroll(tree, id);
    true
}

pub(super) fn emit_editor_state_change(
    emission: &TextAreaEmission<'_>,
    value: Arc<str>,
    cursor: usize,
    anchor: Option<usize>,
    edit: Option<crate::text::edit::TextEditEvent>,
    vim_mode: Option<crate::widgets::TextAreaVimMode>,
) {
    let Some(cb) = emission.on_editor_state_change else {
        return;
    };
    let reason = if edit.is_some() {
        crate::widgets::TextAreaStateChangeReason::Edit
    } else if anchor.is_some() {
        crate::widgets::TextAreaStateChangeReason::SelectionChange
    } else if vim_mode.is_some() {
        crate::widgets::TextAreaStateChangeReason::VimModeChange
    } else {
        crate::widgets::TextAreaStateChangeReason::CursorMove
    };
    cb.emit(crate::widgets::TextAreaStateChangeEvent {
        reason,
        value,
        cursor,
        anchor,
        edit,
        vim_mode,
    });
}
