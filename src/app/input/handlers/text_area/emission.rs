use super::*;
use crate::text::buffer::{
    DeferredMetadataCallbacks, StagedMetadata, register_deferred_metadata_callbacks,
};
use crate::text::edit::EditMetadata;
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
    let mut edit = editor.take_last_edit();
    let restored_metadata = editor.take_last_restored_metadata();
    let is_history_replay = edit.is_none() && editor.text() != emission.old_value;
    let mut current_value = editor.text().to_owned();
    let cursor = editor.cursor();
    let anchor = editor.anchor();

    // Prune or restore images list if inline image sentinel chars changed.
    // In attachment mode, sentinels are never inserted into the value,
    // so sentinel-based pruning would incorrectly clear the images list.
    let mut pending_images: Option<Vec<ImageContent>> = None;
    let mut images_already_recorded_on_edit = false;
    if emission.image_mode == TextAreaImageMode::Inline && emission.on_images_change.is_some() {
        if is_history_replay {
            if let Some(restored_images) = restored_metadata
                .as_ref()
                .and_then(|meta| meta.images.clone())
            {
                if restored_images.as_slice() != emission.images {
                    pending_images = Some(restored_images);
                }
            } else {
                let required_images = image_sentinel_required_len(&current_value);
                if required_images == 0 && !emission.images.is_empty() {
                    pending_images = Some(Vec::new());
                }
            }
        } else if let Some(edit_images) = restored_metadata
            .as_ref()
            .and_then(|meta| meta.images.clone())
        {
            images_already_recorded_on_edit = true;
            if edit_images.as_slice() != emission.images {
                pending_images = Some(edit_images);
            }
        } else {
            let required_images = image_sentinel_required_len(&current_value);
            if required_images == 0 {
                if !emission.images.is_empty() {
                    pending_images = Some(Vec::new());
                }
            } else if required_images <= emission.images.len() {
                let (pruned, remapped) = prune_images_for_value(&current_value, emission.images);
                if let Some(remapped_val) = remapped {
                    current_value = remapped_val;
                }
                if pruned.as_slice() != emission.images {
                    pending_images = Some(pruned);
                }
            }
        }
    }

    // Prune or restore custom sentinels list.
    let mut pending_sentinels: Option<Vec<TextAreaSentinel>> = None;
    let mut pending_sentinel_events: Vec<SentinelEvent> = Vec::new();
    let mut sentinels_already_recorded_on_edit = false;
    if emission.on_sentinels_change.is_some() || emission.on_sentinel_event.is_some() {
        if is_history_replay {
            if let Some(restored_sentinels) = restored_metadata.and_then(|meta| meta.sentinels) {
                if restored_sentinels.as_slice() != emission.sentinels {
                    pending_sentinels = Some(restored_sentinels);
                }
            } else {
                let required_sentinels = custom_sentinel_required_len(&current_value);
                if required_sentinels == 0 && !emission.sentinels.is_empty() {
                    pending_sentinels = Some(Vec::new());
                }
            }
        } else if let Some(edit_sentinels) = restored_metadata.and_then(|meta| meta.sentinels) {
            sentinels_already_recorded_on_edit = true;
            if edit_sentinels.as_slice() != emission.sentinels {
                pending_sentinels = Some(edit_sentinels);
            }
        } else {
            let required_sentinels = custom_sentinel_required_len(&current_value);
            if required_sentinels == 0 {
                if !emission.sentinels.is_empty() {
                    let (pruned, _, events) =
                        prune_sentinels_for_value(&current_value, emission.sentinels);
                    pending_sentinels = Some(pruned);
                    pending_sentinel_events = events;
                }
            } else if required_sentinels <= emission.sentinels.len() {
                let (pruned, remapped, events) =
                    prune_sentinels_for_value(&current_value, emission.sentinels);
                if let Some(remapped_val) = remapped {
                    current_value = remapped_val;
                }
                if pruned.as_slice() != emission.sentinels {
                    pending_sentinels = Some(pruned);
                    pending_sentinel_events = events;
                }
            }
        }
    }

    // For new edits that remapped sentinels or changed image/sentinel metadata, rewrite the
    // pending undo entry atomically so the recorded `TextEdit` owns both the final emitted text
    // and its before/after metadata.
    let needs_metadata_rewrite = !is_history_replay
        && (current_value != editor.text()
            || (pending_images.is_some() && !images_already_recorded_on_edit)
            || (pending_sentinels.is_some() && !sentinels_already_recorded_on_edit));
    if needs_metadata_rewrite {
        let metadata =
            (pending_images.is_some() || pending_sentinels.is_some()).then(|| EditMetadata {
                images_before: pending_images.is_some().then(|| emission.images.to_vec()),
                images_after: pending_images.clone(),
                sentinels_before: pending_sentinels
                    .is_some()
                    .then(|| emission.sentinels.to_vec()),
                sentinels_after: pending_sentinels.clone(),
            });
        if let Some(rewritten_edit) =
            editor.rewrite_pending_edit_to(&current_value, cursor, anchor, metadata)
        {
            edit = Some(rewritten_edit);
        }
    }

    let new_value: Arc<str> = Arc::from(editor.text().to_owned());
    let cursor = editor.cursor();
    let anchor = editor.anchor();

    if editor.is_bound() {
        let staged_metadata =
            (pending_images.is_some() || pending_sentinels.is_some()).then(|| StagedMetadata {
                images: pending_images.clone(),
                sentinels: pending_sentinels.clone(),
            });
        let deferred_token = register_deferred_metadata_callbacks(DeferredMetadataCallbacks {
            images: emission
                .on_images_change
                .cloned()
                .zip(pending_images.take()),
            sentinels: emission
                .on_sentinels_change
                .cloned()
                .zip(pending_sentinels.take()),
            sentinel_events: (!pending_sentinel_events.is_empty())
                .then(|| {
                    emission
                        .on_sentinel_event
                        .cloned()
                        .map(|cb| (cb, std::mem::take(&mut pending_sentinel_events)))
                })
                .flatten(),
        });
        editor.stage_bound_history(&new_value, deferred_token, staged_metadata);
    }

    if let (Some(cb), Some(edit)) = (emission.on_edit, edit.clone()) {
        cb.emit(edit);
    }
    emission.on_change.emit(TextAreaEvent {
        value: new_value.clone(),
        cursor,
        anchor,
    });
    emit_editor_state_change(emission, new_value.clone(), cursor, anchor, edit, None);

    // Unbound TextAreas emit metadata callbacks immediately; bound TextAreas emit them only when
    // the controlled text change is accepted via `apply_to` / `apply` / `apply_with_metadata`.
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
