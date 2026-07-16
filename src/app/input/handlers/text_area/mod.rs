//! TextArea keyboard and scroll-wheel handlers.

use std::sync::Arc;
use std::time::Duration;

use crate::app::context::TextAreaNewlineBinding;
use crate::app::input::handlers::KeyCtx;
use crate::app::input::keymap::Action;
use crate::app::input::text_area_vim::{
    TextAreaVimPending, TextAreaVimState, VimInsertKind, VimInsertOrigin, VimInsertSession,
    VimMotion, VimOperator, VimRegisterValue, VimRepeatChange, VimRepeatTarget, VimTextObject,
    first_nonblank_in_line, line_bounds_at, line_count, line_end_at, line_end_including_newline,
    line_index_at, line_start_at, line_start_by_index, line_start_by_one_based_count,
    text_area_vim_search_feedback_for_text, vim_big_word_backward_start, vim_big_word_end,
    vim_big_word_forward_start, vim_find_search, vim_word_backward_start, vim_word_end,
    vim_word_forward_start,
};
use crate::app::interaction_state::DirtyLevel;
use crate::callback::{Callback, KeyHandler};
use crate::clipboard::ImageContent;
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::ScrollbarVariant;
use crate::text::editor::TextEditor;
use crate::ui::capabilities::{
    ReadOnlyClipboardContext, TextAreaClipboardContext, TextAreaClipboardParams, selection_range,
};
use crate::ui::router;
use crate::utils::text::{
    SentinelInfo, byte_at_col_sentinel_tabs, clamp_cursor, next_char_boundary, prev_char_boundary,
    str_visual_width_with_tabs, visual_col_with_virtual,
};
use crate::widgets::internal::{apply_scroll_action, scroll_metrics};
use crate::widgets::{
    IMAGE_SENTINEL_BASE, ScrollEvent, SentinelEvent, TextAreaEvent, TextAreaImageMode,
    TextAreaSentinel, TextAreaVimMode, TextAreaVirtualText, TextAreaVisualLine,
    inline_virtual_insertions_for_line, sentinel_info_for, text_area_visual_line_for_cursor,
};
use crate::widgets::{text_area_cursor_reserve, text_area_total_gutter_width};

use super::handle_key_interceptor;

mod emission;
mod newline;
mod scroll;
#[cfg(test)]
mod tests;
mod vim;
mod visual_nav;

use emission::{
    TextAreaEmission, emit_editor_state_change, emit_text_area_editor_change,
    finish_text_area_edit_if_handled,
};
use newline::{
    effective_text_area_newline_binding, text_area_should_block_enter,
    text_area_should_insert_newline,
};
#[cfg(test)]
pub(crate) use newline::{
    test_effective_text_area_newline_binding, test_text_area_should_block_enter,
    test_text_area_should_insert_newline,
};
use scroll::cancel_text_area_smooth_scroll;
pub(crate) use scroll::handle_scroll;
use vim::{
    VimClipboardCtx, VimKeyOutcome, VimLayoutCtx, clear_text_area_vim_render_feedback,
    dispatch_text_area_vim_key, exit_text_area_visual_mode_if_needed,
    handle_text_area_vim_edit_command, remap_vim_marks_for_last_edit,
    sync_text_area_vim_render_feedback, text_area_clipboard_action_may_mutate,
};
use visual_nav::perform_visual_vertical_nav;
// ── Keyboard handler ────────────────────────────────────────────────────────

fn sync_text_area_vim_after_key(
    tree: &mut NodeTree,
    id: NodeId,
    editor: &TextEditor,
    state: &mut TextAreaVimState,
    copy_feedback: &mut crate::app::copy_feedback::CopyFeedbackState,
    clipboard_config: &crate::clipboard::ClipboardConfig,
    dirty_override: &mut Option<DirtyLevel>,
) {
    let pending_yank_feedback = state.take_pending_yank_feedback();
    sync_text_area_vim_render_feedback(tree, id, state, editor);

    if !pending_yank_feedback {
        return;
    }

    if let NodeKind::TextArea(ta) = &mut tree.node_mut(id).kind {
        ta.cursor = editor.cursor();
        ta.anchor = editor.anchor();
    }

    if clipboard_config.copy_feedback_duration_ms > 0 {
        copy_feedback.trigger(
            id,
            Duration::from_millis(clipboard_config.copy_feedback_duration_ms as u64),
        );
        *dirty_override = Some(DirtyLevel::PaintOnly);
    }
}

fn sync_text_area_node_from_editor(tree: &mut NodeTree, id: NodeId, editor: &TextEditor) {
    if let NodeKind::TextArea(ta) = &mut tree.node_mut(id).kind {
        ta.value = Arc::from(editor.text().to_owned());
        ta.cursor = editor.cursor();
        ta.anchor = editor.anchor();
    }
}

/// Handle keyboard input for a focused TextArea node.
pub(crate) fn handle_key(
    tree: &mut NodeTree,
    id: NodeId,
    key: KeyEvent,
    ctx: &mut KeyCtx<'_>,
) -> bool {
    let handle_on_key = |handler: &KeyHandler| -> bool { handler.handle(key) };

    // ── Phase 1: immutable borrow – read fields & handle clipboard ──────
    let node = tree.node(id);
    let NodeKind::TextArea(ta) = &node.kind else {
        return false;
    };

    let disabled = ta.disabled;
    let read_only = ta.read_only;
    let cursor = ta.cursor;
    let anchor = ta.anchor;
    let value = ta.value.clone();
    let has_on_change = ta.on_change.is_some();
    let on_change = ta.on_change.clone();
    let on_edit = ta.on_edit.clone();
    let on_editor_state_change = ta.on_editor_state_change.clone();
    let on_key = ta.on_key.clone();
    let key_interceptor = ta.key_interceptor.clone();
    let clear_bindings = ta.clear_bindings.clone();
    let on_image_paste = ta.on_image_paste.clone();
    let on_text_paste = ta.on_text_paste.clone();
    let on_images_change = ta.on_images_change.clone();
    let images = ta.images.clone();
    let image_mode = ta.image_mode;
    let image_placeholder = ta.image_placeholder.clone();
    let copy_excluded_bytes = ta.copy_excluded_bytes.clone();
    let clipboard_transform = ta.clipboard_transform.clone();
    let sentinels = ta.sentinels.clone();
    let on_sentinels_change = ta.on_sentinels_change.clone();
    let on_sentinel_event = ta.on_sentinel_event.clone();
    let vim_motions = ta.vim_motions;
    let vim_keymap = ta.vim_keymap.clone();
    let on_vim_mode_change = ta.on_vim_mode_change.clone();
    let newline_binding_widget = ta.newline_binding;
    let tab_width = ta.tab_width;
    let insert_tab = ta.insert_tab;
    let tab_stop = ta.tab_stop as usize;
    let wrap = ta.wrap;
    let virtual_texts = ta.virtual_texts.clone();
    let visual_lines: Option<Vec<TextAreaVisualLine>> = if wrap {
        ta.visual_cache.latest_lines().map(|s| s.to_vec())
    } else {
        None
    };
    let is_masked = false; // TextArea never has mask
    let clear_binding_matches = !disabled
        && !read_only
        && has_on_change
        && clear_bindings.as_ref().is_some_and(|bindings| {
            bindings
                .iter()
                .any(|binding| binding.matches_sequence(&[key]))
        });

    let mut clipboard_handled = false;

    if !vim_motions || read_only || !has_on_change {
        ctx.text_area_vim_state.remove(&id);
        clear_text_area_vim_render_feedback(tree, id);
    }

    if !disabled && !clear_binding_matches {
        if read_only || !has_on_change {
            let (eff_cursor, eff_anchor) = if !has_on_change {
                ctx.read_only_selection
                    .and_then(|m| m.get(&id))
                    .map(|(c, a)| (*c, *a))
                    .unwrap_or((cursor, anchor))
            } else {
                (cursor, anchor)
            };
            let selection = selection_range(eff_cursor, eff_anchor, value.len());
            let sentinel =
                sentinel_info_for(image_mode, images.len(), &image_placeholder, &sentinels);
            let excluded: &[(usize, usize)] = copy_excluded_bytes
                .as_deref()
                .map(|v| v.as_slice())
                .unwrap_or(&[]);
            let mut context =
                ReadOnlyClipboardContext::new(value.as_ref(), selection, true, is_masked)
                    .with_sentinel(sentinel, &image_placeholder)
                    .with_excluded_bytes(excluded)
                    .with_clipboard_transform(clipboard_transform.clone());
            let dispatch = crate::app::copy_feedback::dispatch_clipboard_with_feedback_result(
                key,
                ctx.keymap,
                &mut context,
                ctx.clipboard,
                ctx.clipboard_config,
                ctx.copy_feedback,
                id,
            );
            clipboard_handled = ctx.record_copy_feedback_dispatch(dispatch);
        } else {
            let editor = ctx.textarea_history.entry(id).or_insert_with(|| {
                let mut editor = TextEditor::new(value.as_ref().to_string());
                editor.set_cursor(cursor);
                editor.set_anchor(anchor);
                editor
            });
            editor.sync_from(value.as_ref(), cursor, anchor);

            let dispatch = {
                let mut context = TextAreaClipboardContext::new(
                    editor,
                    TextAreaClipboardParams {
                        on_change: on_change.as_ref(),
                        on_edit: on_edit.as_ref(),
                        on_editor_state_change: on_editor_state_change.as_ref(),
                        on_image_paste: on_image_paste.as_ref(),
                        on_text_paste: on_text_paste.as_ref(),
                        images: images.as_slice(),
                        on_images_change: on_images_change.as_ref(),
                        image_mode,
                        image_placeholder: &image_placeholder,
                        sentinels: sentinels.as_slice(),
                        clipboard_transform: clipboard_transform.clone(),
                        editable: !disabled,
                    },
                );
                crate::app::copy_feedback::dispatch_clipboard_with_feedback_result(
                    key,
                    ctx.keymap,
                    &mut context,
                    ctx.clipboard,
                    ctx.clipboard_config,
                    ctx.copy_feedback,
                    id,
                )
            };
            if dispatch.mutated {
                sync_text_area_node_from_editor(tree, id, editor);
            }
            clipboard_handled = ctx.record_copy_feedback_dispatch(dispatch);
        }
    }

    if clipboard_handled {
        if vim_motions && text_area_clipboard_action_may_mutate(ctx.keymap.resolve_action(key)) {
            if let Some(state) = ctx.text_area_vim_state.get_mut(&id) {
                exit_text_area_visual_mode_if_needed(state, on_vim_mode_change.as_ref());
            }
            if let (Some(state), Some(editor)) = (
                ctx.text_area_vim_state.get_mut(&id),
                ctx.textarea_history.get(&id),
            ) {
                remap_vim_marks_for_last_edit(state, editor);
                sync_text_area_vim_render_feedback(tree, id, state, editor);
            }
        }
        return true;
    }

    // ── Phase 2: read-only / no on_change fast path ─────────────────────
    if read_only || !has_on_change {
        return on_key.as_ref().map(&handle_on_key).unwrap_or(false);
    }

    if handle_key_interceptor(key_interceptor.as_ref(), key) {
        return true;
    }

    // ── Phase 3: editable – forward key to TextEditor ───────────────────
    if let Some(cb) = on_change.as_ref() {
        let editor = ctx.textarea_history.entry(id).or_insert_with(|| {
            let mut editor = TextEditor::new(value.as_ref().to_string());
            editor.set_cursor(cursor);
            editor.set_anchor(anchor);
            editor
        });
        editor.sync_from(value.as_ref(), cursor, anchor);

        let newline_binding = effective_text_area_newline_binding(
            newline_binding_widget,
            ctx.text_area_newline_binding,
        );

        // Snapshot old value for sentinel-pruning after the key is handled.
        let old_value = value.clone();
        let images_snapshot = images.clone();

        let emission = TextAreaEmission {
            on_change: cb,
            on_edit: on_edit.as_ref(),
            on_editor_state_change: on_editor_state_change.as_ref(),
            old_value: &old_value,
            images: &images_snapshot,
            image_mode,
            sentinels: &sentinels,
            on_images_change: on_images_change.as_ref(),
            on_sentinels_change: on_sentinels_change.as_ref(),
            on_sentinel_event: on_sentinel_event.as_ref(),
        };

        let action = ctx.keymap.resolve_action(key);
        let is_vert = matches!(
            action,
            Action::MoveUp | Action::MoveDown | Action::SelectUp | Action::SelectDown
        );

        if clear_binding_matches {
            if vim_motions && let Some(state) = ctx.text_area_vim_state.get_mut(&id) {
                exit_text_area_visual_mode_if_needed(state, on_vim_mode_change.as_ref());
            }
            let handled = editor.clear();
            if handled
                && vim_motions
                && let Some(state) = ctx.text_area_vim_state.get_mut(&id)
            {
                remap_vim_marks_for_last_edit(state, editor);
                sync_text_area_vim_render_feedback(tree, id, state, editor);
            }
            return finish_text_area_edit_if_handled(tree, id, editor, &emission, handled)
                || on_key.as_ref().map(&handle_on_key).unwrap_or(false);
        }

        if vim_motions {
            let sentinel =
                sentinel_info_for(image_mode, images.len(), &image_placeholder, &sentinels);
            let state = ctx.text_area_vim_state.entry(id).or_default();
            let vim_key = if matches!(state.mode, TextAreaVimMode::Insert) {
                key
            } else {
                vim_keymap
                    .as_ref()
                    .map(|keymap| keymap.translate_key(key))
                    .unwrap_or(key)
            };
            let vim_clipboard = VimClipboardCtx {
                params: TextAreaClipboardParams {
                    on_change: on_change.as_ref(),
                    on_edit: on_edit.as_ref(),
                    on_editor_state_change: on_editor_state_change.as_ref(),
                    on_image_paste: on_image_paste.as_ref(),
                    on_text_paste: on_text_paste.as_ref(),
                    images: images.as_slice(),
                    on_images_change: on_images_change.as_ref(),
                    image_mode,
                    image_placeholder: &image_placeholder,
                    sentinels: sentinels.as_slice(),
                    clipboard_transform: clipboard_transform.clone(),
                    editable: true,
                },
                clipboard: ctx.clipboard,
                config: ctx.clipboard_config,
            };
            let vim_layout = VimLayoutCtx {
                wrap,
                visual_lines: visual_lines.as_deref(),
                sentinel: sentinel.as_ref(),
                tab_stop,
                virtual_texts: &virtual_texts,
            };
            if let Some(outcome) = handle_text_area_vim_edit_command(
                editor,
                state,
                vim_key,
                action,
                &vim_clipboard,
                on_vim_mode_change.as_ref(),
            ) {
                sync_text_area_vim_after_key(
                    tree,
                    id,
                    editor,
                    state,
                    ctx.copy_feedback,
                    ctx.clipboard_config,
                    &mut ctx.dirty_override,
                );
                match outcome {
                    VimKeyOutcome::Unhandled => {
                        return on_key.as_ref().map(&handle_on_key).unwrap_or(false);
                    }
                    VimKeyOutcome::ConsumedUnchanged => return true,
                    VimKeyOutcome::EditorChanged {
                        vertical,
                        mode_changed,
                    } => {
                        remap_vim_marks_for_last_edit(state, editor);
                        if let Some(mode) = mode_changed
                            && let Some(cb) = on_vim_mode_change.as_ref()
                        {
                            cb.emit(mode);
                        }
                        if !vertical {
                            editor.set_visual_nav_col(None);
                        }
                        return emit_text_area_editor_change(tree, id, editor, &emission);
                    }
                    VimKeyOutcome::PassThrough | VimKeyOutcome::ModeChanged(_) => {}
                }
            }
            let outcome = dispatch_text_area_vim_key(editor, state, vim_key, action, &vim_layout);
            sync_text_area_vim_render_feedback(tree, id, state, editor);
            match outcome {
                VimKeyOutcome::Unhandled => {
                    return on_key.as_ref().map(&handle_on_key).unwrap_or(false);
                }
                VimKeyOutcome::PassThrough => {}
                VimKeyOutcome::ConsumedUnchanged => return true,
                VimKeyOutcome::ModeChanged(mode) => {
                    if let Some(cb) = on_vim_mode_change.as_ref() {
                        cb.emit(mode);
                    }
                    emit_editor_state_change(
                        &emission,
                        Arc::from(editor.text().to_owned()),
                        editor.cursor(),
                        editor.anchor(),
                        None,
                        Some(mode),
                    );
                    return true;
                }
                VimKeyOutcome::EditorChanged {
                    vertical,
                    mode_changed,
                } => {
                    remap_vim_marks_for_last_edit(state, editor);
                    if let Some(mode) = mode_changed
                        && let Some(cb) = on_vim_mode_change.as_ref()
                    {
                        cb.emit(mode);
                    }
                    if !vertical {
                        editor.set_visual_nav_col(None);
                    }
                    return emit_text_area_editor_change(tree, id, editor, &emission);
                }
            }
        }

        let mut handled = if text_area_should_insert_newline(key, newline_binding) {
            editor.insert_char('\n')
        } else if text_area_should_block_enter(key, newline_binding) {
            false
        } else if insert_tab
            && key.code == KeyCode::Tab
            && key.mods == crate::core::event::KeyMods::default()
        {
            editor.insert_char('\t')
        } else if tab_width > 0
            && key.code == KeyCode::Tab
            && key.mods == crate::core::event::KeyMods::default()
        {
            let tab = tab_width as usize;
            let text = editor.text();
            let cursor = crate::utils::text::clamp_cursor(text, editor.cursor());
            let line_start = text[..cursor].rfind('\n').map_or(0, |i| i + 1);
            let sentinel =
                sentinel_info_for(image_mode, images.len(), &image_placeholder, &sentinels);
            let col = str_visual_width_with_tabs(
                &text[line_start..cursor],
                sentinel.as_ref(),
                0,
                tab_stop,
            );
            let count = tab - (col % tab);
            let spaces = " ".repeat(count);
            editor.insert_str(&spaces)
        } else if let Some(lines) = visual_lines.as_deref().filter(|_| is_vert && wrap) {
            let sentinel =
                sentinel_info_for(image_mode, images.len(), &image_placeholder, &sentinels);
            perform_visual_vertical_nav(
                editor,
                action,
                lines,
                sentinel.as_ref(),
                tab_stop,
                &virtual_texts,
            )
        } else {
            editor.handle_key_with(key, ctx.keymap)
        };

        if !is_vert {
            editor.set_visual_nav_col(None);
        }

        if handled
            && vim_motions
            && let Some(state) = ctx.text_area_vim_state.get_mut(&id)
        {
            remap_vim_marks_for_last_edit(state, editor);
            sync_text_area_vim_render_feedback(tree, id, state, editor);
        }

        if finish_text_area_edit_if_handled(tree, id, editor, &emission, handled) {
            true
        } else if let Some(cb) = on_key.as_ref() {
            handled = handle_on_key(cb);
            handled
        } else {
            false
        }
    } else if let Some(cb) = on_key.as_ref() {
        handle_on_key(cb)
    } else {
        false
    }
}

pub(crate) fn handle_paste(
    tree: &mut NodeTree,
    id: NodeId,
    text: &str,
    ctx: &mut KeyCtx<'_>,
) -> bool {
    let node = tree.node(id);
    let NodeKind::TextArea(ta) = &node.kind else {
        return false;
    };

    if ta.disabled {
        return false;
    }

    let read_only = ta.read_only;
    let cursor = ta.cursor;
    let anchor = ta.anchor;
    let value = ta.value.clone();
    let has_on_change = ta.on_change.is_some();
    let on_change = ta.on_change.clone();
    let on_edit = ta.on_edit.clone();
    let on_image_paste = ta.on_image_paste.clone();
    let on_text_paste = ta.on_text_paste.clone();
    let on_images_change = ta.on_images_change.clone();
    let images = ta.images.clone();
    let image_mode = ta.image_mode;
    let image_placeholder = ta.image_placeholder.clone();
    let sentinels = ta.sentinels.clone();
    let vim_motions = ta.vim_motions;
    let on_vim_mode_change = ta.on_vim_mode_change.clone();
    let on_editor_state_change = ta.on_editor_state_change.clone();

    if !vim_motions || read_only || !has_on_change {
        ctx.text_area_vim_state.remove(&id);
        clear_text_area_vim_render_feedback(tree, id);
    }

    if read_only || !has_on_change {
        return false;
    }

    let editor = ctx.textarea_history.entry(id).or_insert_with(|| {
        let mut editor = TextEditor::new(value.as_ref().to_string());
        editor.set_cursor(cursor);
        editor.set_anchor(anchor);
        editor
    });
    editor.sync_from(value.as_ref(), cursor, anchor);

    let before_text = editor.text().to_owned();
    let before_cursor = editor.cursor();
    let before_anchor = editor.anchor();
    let handled = {
        let mut context = TextAreaClipboardContext::new(
            editor,
            TextAreaClipboardParams {
                on_change: on_change.as_ref(),
                on_edit: on_edit.as_ref(),
                on_editor_state_change: on_editor_state_change.as_ref(),
                on_image_paste: on_image_paste.as_ref(),
                on_text_paste: on_text_paste.as_ref(),
                images: images.as_slice(),
                on_images_change: on_images_change.as_ref(),
                image_mode,
                image_placeholder: &image_placeholder,
                sentinels: sentinels.as_slice(),
                clipboard_transform: None,
                editable: true,
            },
        );
        router::dispatch_text_paste(text, &mut context, ctx.clipboard_config.paste_max_bytes)
    };
    let mutated = editor.text() != before_text
        || editor.cursor() != before_cursor
        || editor.anchor() != before_anchor;
    if handled {
        if mutated {
            sync_text_area_node_from_editor(tree, id, editor);
        }
        if vim_motions && let Some(state) = ctx.text_area_vim_state.get_mut(&id) {
            exit_text_area_visual_mode_if_needed(state, on_vim_mode_change.as_ref());
            remap_vim_marks_for_last_edit(state, editor);
            sync_text_area_vim_render_feedback(tree, id, state, editor);
        }
        cancel_text_area_smooth_scroll(tree, id);
    }
    handled
}
