use super::*;

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
    let edit = editor.take_last_edit();
    if let (Some(cb), Some(edit)) = (emission.on_edit, edit.clone()) {
        cb.emit(edit);
    }
    let mut new_value: Arc<str> = Arc::from(editor.text().to_owned());
    let cursor = editor.cursor();
    let anchor = editor.anchor();
    emission.on_change.emit(TextAreaEvent {
        value: new_value.clone(),
        cursor,
        anchor,
    });
    emit_editor_state_change(
        emission,
        new_value.clone(),
        cursor,
        anchor,
        edit.clone(),
        None,
    );

    // Prune or restore images list if inline image sentinel chars changed.
    // In attachment mode, sentinels are never inserted into the value,
    // so sentinel-based pruning would incorrectly clear the images list.
    if emission.image_mode == TextAreaImageMode::Inline
        && let Some(on_img_cb) = emission.on_images_change
    {
        editor.remember_text_area_images(emission.old_value, emission.images);
        let required_images = image_sentinel_required_len(&new_value);
        let image_source = if required_images == 0 {
            (!emission.images.is_empty()).then(|| emission.images.to_vec())
        } else if required_images <= emission.images.len() {
            Some(emission.images.to_vec())
        } else {
            editor
                .remembered_text_area_images(&new_value)
                .filter(|images| required_images <= images.len())
        };
        if let Some(image_source) = image_source {
            let (pruned, remapped) =
                prune_images_for_value(emission.old_value, &new_value, &image_source);
            if let Some(remapped_val) = remapped {
                // Middle sentinel was deleted; remap chars and emit updated value.
                let remapped_arc: Arc<str> = Arc::from(remapped_val);
                emission.on_change.emit(TextAreaEvent {
                    value: remapped_arc.clone(),
                    cursor: editor.cursor(),
                    anchor: editor.anchor(),
                });
                new_value = remapped_arc;
            }
            if pruned.as_slice() != emission.images {
                on_img_cb.emit(pruned.clone());
            }
            editor.remember_text_area_images(&new_value, &pruned);
        }
    }

    // Prune or restore custom sentinels list.
    let required_sentinels = custom_sentinel_required_len(&new_value);
    let sentinel_source = if required_sentinels == 0 {
        (!emission.sentinels.is_empty()).then(|| emission.sentinels.to_vec())
    } else if required_sentinels <= emission.sentinels.len() {
        Some(emission.sentinels.to_vec())
    } else {
        editor
            .remembered_text_area_sentinels(&new_value)
            .filter(|sentinels| required_sentinels <= sentinels.len())
    };
    if let Some(sentinel_source) = sentinel_source
        && (emission.on_sentinels_change.is_some() || emission.on_sentinel_event.is_some())
    {
        let (pruned, remapped, events) = prune_sentinels_for_value(&new_value, &sentinel_source);
        if let Some(remapped_val) = remapped {
            let remapped_arc: Arc<str> = Arc::from(remapped_val);
            emission.on_change.emit(TextAreaEvent {
                value: remapped_arc.clone(),
                cursor: editor.cursor(),
                anchor: editor.anchor(),
            });
            new_value = remapped_arc;
        }
        if pruned.as_slice() != emission.sentinels {
            if let Some(on_sent_cb) = emission.on_sentinels_change {
                on_sent_cb.emit(pruned.clone());
            }
            if !events.is_empty()
                && let Some(ev_cb) = emission.on_sentinel_event
            {
                ev_cb.emit(events);
            }
        }
        editor.remember_text_area_sentinels(&new_value, &pruned);
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

fn custom_sentinel_required_len(value: &str) -> usize {
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

fn image_sentinel_required_len(value: &str) -> usize {
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
/// `U+E000 + i`.  When the user deletes one of those characters the corresponding
/// image must be removed from the list.
///
/// Returns `(pruned_images, Option<remapped_value>)`. When a middle sentinel is deleted,
/// the remaining sentinel chars in the value are remapped to sequential indices so that
/// the pruned list and the value stay consistent.
pub(super) fn prune_images_for_value(
    _old_value: &str,
    new_value: &str,
    images: &[crate::clipboard::ImageContent],
) -> (Vec<crate::clipboard::ImageContent>, Option<String>) {
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
    let pruned: Vec<crate::clipboard::ImageContent> = images
        .iter()
        .enumerate()
        .filter(|(i, _)| present[*i])
        .map(|(_, img)| img.clone())
        .collect();

    // Build old_index → new_index mapping for remapping sentinel chars.
    let mut remap: Vec<Option<usize>> = vec![None; images.len()];
    let mut new_idx = 0usize;
    for (old_idx, was_present) in present.iter().enumerate() {
        if *was_present {
            remap[old_idx] = Some(new_idx);
            new_idx += 1;
        }
    }

    // Check if any char needs remapping (gap was created).
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
/// Returns `(pruned_sentinels, Option<remapped_value>)`. When a middle sentinel is deleted,
/// the remaining sentinel chars in the value are remapped to sequential indices.
pub(super) fn prune_sentinels_for_value(
    new_value: &str,
    sentinels: &[crate::widgets::TextAreaSentinel],
) -> (
    Vec<crate::widgets::TextAreaSentinel>,
    Option<String>,
    Vec<crate::widgets::SentinelEvent>,
) {
    use crate::widgets::{SentinelEvent, SentinelId};

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
    let pruned: Vec<crate::widgets::TextAreaSentinel> = sentinels
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
