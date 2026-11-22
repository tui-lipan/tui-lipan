//! Input widget keyboard handler.

use std::sync::Arc;

use crate::app::input::handlers::KeyCtx;
use crate::callback::KeyHandler;
use crate::core::event::KeyEvent;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::text::input::TextInput;
use crate::ui::capabilities::{InputClipboardContext, ReadOnlyClipboardContext, selection_range};
use crate::ui::router;
use crate::widgets::InputEvent;

use super::handle_key_interceptor;

/// Handle keyboard input for a focused Input node.
pub(crate) fn handle_key(
    tree: &mut NodeTree,
    id: NodeId,
    key: KeyEvent,
    ctx: &mut KeyCtx<'_>,
) -> bool {
    let from_chord = ctx.chord_action.is_some();
    let handle_on_key = |handler: &KeyHandler| -> bool { !from_chord && handler.handle(key) };

    // ── Phase 1: immutable borrow – read fields & handle clipboard ──────
    let node = tree.node(id);
    let NodeKind::Input(node) = &node.kind else {
        return false;
    };

    let disabled = node.disabled;
    let read_only = node.read_only;
    let cursor = node.cursor;
    let anchor = node.anchor;
    let value = node.value.clone();
    let has_on_change = node.on_change.is_some();
    let is_masked = node.mask.is_some();
    let on_change = node.on_change.clone();
    let on_submit = node.on_submit.clone();
    let history_scope = node.history_scope.clone();
    let on_edit = node.on_edit.clone();
    let on_key = node.on_key.clone();
    let key_interceptor = node.key_interceptor.clone();

    let mut clipboard_handled = false;

    if !disabled {
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
            let mut context =
                ReadOnlyClipboardContext::new(value.as_ref(), selection, !is_masked, is_masked);
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
            let input = ctx.input_history.entry(id).or_insert_with(|| {
                let mut input = TextInput::new(value.as_ref().to_string());
                input.set_cursor(cursor);
                input.set_anchor(anchor);
                input
            });
            input.sync_history_scope(history_scope.as_ref());
            input.sync_from(value.as_ref(), cursor, anchor);

            let mut context = InputClipboardContext::new(
                input,
                on_change.as_ref(),
                on_edit.as_ref(),
                !is_masked,
                !disabled,
                is_masked,
            );
            let dispatch = crate::app::copy_feedback::dispatch_clipboard_with_feedback_result(
                key,
                ctx.keymap,
                &mut context,
                ctx.clipboard,
                ctx.clipboard_config,
                ctx.copy_feedback,
                id,
            );
            let reported = dispatch.mutated.then(|| {
                (
                    Arc::from(input.text().to_owned()),
                    input.cursor(),
                    input.anchor(),
                )
            });
            clipboard_handled = ctx.record_copy_feedback_dispatch(dispatch);
            if let Some((reported_value, reported_cursor, reported_anchor)) = reported
                && let NodeKind::Input(node) = &mut tree.node_mut(id).kind
            {
                node.uncontrolled
                    .record_reported(reported_value, reported_cursor, reported_anchor);
            }
        }
    }

    if clipboard_handled {
        return true;
    }

    // ── Phase 2: read-only fast path ────────────────────────────────────
    if disabled || read_only {
        return on_key.as_ref().map(handle_on_key).unwrap_or(false);
    }

    // ── Phase 2.5: pre-insertion key interceptor ────────────────────────
    if !from_chord && handle_key_interceptor(key_interceptor.as_ref(), key) {
        return true;
    }

    // ── Phase 2.75: plain Enter submit ──────────────────────────────────
    if !from_chord
        && key.is(crate::core::event::KeyCode::Enter)
        && let Some(cb) = on_submit.as_ref()
    {
        cb.emit(InputEvent {
            value,
            cursor,
            anchor,
        });
        return true;
    }

    if !has_on_change {
        return on_key.as_ref().map(handle_on_key).unwrap_or(false);
    }

    // ── Phase 3: editable – forward key to TextInput ────────────────────
    if let Some(cb) = on_change.as_ref() {
        let input = ctx.input_history.entry(id).or_insert_with(|| {
            let mut input = TextInput::new(value.as_ref().to_string());
            input.set_cursor(cursor);
            input.set_anchor(anchor);
            input
        });
        input.sync_history_scope(history_scope.as_ref());
        input.sync_from(value.as_ref(), cursor, anchor);

        let action = ctx
            .chord_action
            .unwrap_or_else(|| ctx.keymap.resolve_action(key));
        if input.handle_action_masked(action, is_masked) {
            if let Some(cb) = on_edit.as_ref()
                && let Some(edit) = input.take_last_edit()
            {
                cb.emit(edit);
            }
            let next_value: Arc<str> = Arc::from(input.text().to_owned());
            let next_cursor = input.cursor();
            let next_anchor = input.anchor();
            if let NodeKind::Input(node) = &mut tree.node_mut(id).kind {
                node.uncontrolled
                    .record_reported(next_value.clone(), next_cursor, next_anchor);
            }
            cb.emit(InputEvent {
                value: next_value,
                cursor: next_cursor,
                anchor: next_anchor,
            });
            true
        } else if let Some(cb) = on_key.as_ref() {
            handle_on_key(cb)
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
    let NodeKind::Input(node) = &node.kind else {
        return false;
    };

    if node.disabled {
        return false;
    }

    let read_only = node.read_only;
    let cursor = node.cursor;
    let anchor = node.anchor;
    let value = node.value.clone();
    let has_on_change = node.on_change.is_some();
    let is_masked = node.mask.is_some();
    let on_change = node.on_change.clone();
    let history_scope = node.history_scope.clone();
    let on_edit = node.on_edit.clone();

    if read_only || !has_on_change {
        return false;
    }

    let input = ctx.input_history.entry(id).or_insert_with(|| {
        let mut input = TextInput::new(value.as_ref().to_string());
        input.set_cursor(cursor);
        input.set_anchor(anchor);
        input
    });
    input.sync_history_scope(history_scope.as_ref());
    input.sync_from(value.as_ref(), cursor, anchor);

    let handled = {
        let mut context = InputClipboardContext::new(
            input,
            on_change.as_ref(),
            on_edit.as_ref(),
            !is_masked,
            true,
            is_masked,
        );

        router::dispatch_text_paste(text, &mut context, ctx.clipboard_config.paste_max_bytes)
    };
    if handled {
        let reported_value: Arc<str> = Arc::from(input.text().to_owned());
        let reported_cursor = input.cursor();
        let reported_anchor = input.anchor();
        if let NodeKind::Input(node) = &mut tree.node_mut(id).kind {
            node.uncontrolled
                .record_reported(reported_value, reported_cursor, reported_anchor);
        }
    }
    handled
}
