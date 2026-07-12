//! DocumentView keyboard and scroll-wheel handlers.

use crate::app::input::drag::document_view_selected_text_from_node;
use crate::callback::KeyHandler;
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::ui::capabilities::{ReadOnlyClipboardContext, selection_range};
use crate::widgets::internal::{ScrollAction, apply_scroll_action};
use crate::widgets::{ScrollEvent, ScrollMetrics};

// ── Keyboard ────────────────────────────────────────────────────────────────

/// Handle keyboard input for a focused DocumentView node.
///
/// Handles clipboard (copy-only) first, then scroll keyboard navigation
/// (Up/Down/j/k/PageUp/PageDown/Home/End), and falls back to `on_key`.
pub(crate) fn handle_key(
    tree: &mut NodeTree,
    id: NodeId,
    key: KeyEvent,
    ctx: &mut super::KeyCtx<'_>,
) -> bool {
    let handle_key_cb = |handler: &KeyHandler| -> bool { handler.handle(key) };

    let NodeKind::DocumentView(dv) = &tree.node(id).kind else {
        return false;
    };

    // ── Clipboard (copy only, read-only widget) ─────────────────────────
    let clipboard_handled;
    {
        let selected_text = if let Some(table_sel) = &dv.table_rect_selection {
            Some(table_sel.tsv_text.to_string())
        } else {
            let selection = selection_range(
                dv.selection_cursor,
                dv.selection_anchor,
                dv.visual_cache.flat_text.len(),
            );
            selection.and_then(|(start, end)| {
                document_view_selected_text_from_node(dv, start, end, true)
            })
        };

        let selection = selected_text.as_ref().map(|text| (0, text.len()));
        let text = selected_text.as_deref().unwrap_or("");

        let mut context = if let Some(table_sel) = &dv.table_rect_selection {
            ReadOnlyClipboardContext::new(
                table_sel.tsv_text.as_ref(),
                Some((0, table_sel.tsv_text.len())),
                true,  // copy_allowed
                false, // paste_allowed
            )
        } else {
            ReadOnlyClipboardContext::new(text, selection, true, false)
        };
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
    }

    if clipboard_handled {
        return true;
    }

    // Re-borrow immutably after clipboard handling.
    let NodeKind::DocumentView(dv) = &tree.node(id).kind else {
        return false;
    };

    // ── Scroll keyboard navigation ──────────────────────────────────────
    let mut handled = false;
    let current_offset = dv.scroll_offset;
    let total = dv.total_visual_lines;
    let on_key = dv.on_key.clone();
    let on_scroll = dv.on_scroll.clone();
    let source_target_cancel_pending = dv.scroll_to_source_line.is_some()
        && dv.cancelled_scroll_to_source_line != dv.scroll_to_source_line;
    let node_rect = tree.node(id).rect;
    let NodeKind::DocumentView(dv) = &tree.node(id).kind else {
        return false;
    };
    let inner = node_rect.inner(dv.border, dv.padding);
    let cl = dv.content_layout(inner);
    let viewport_h = cl.content_height as usize;
    let max_offset = total.saturating_sub(viewport_h);

    let new_offset = match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(current_offset.saturating_sub(1)),
        KeyCode::Down | KeyCode::Char('j') => Some((current_offset + 1).min(max_offset)),
        KeyCode::PageUp => Some(current_offset.saturating_sub(viewport_h)),
        KeyCode::PageDown => Some((current_offset + viewport_h).min(max_offset)),
        KeyCode::Home => Some(0),
        KeyCode::End => Some(max_offset),
        _ => None,
    };

    if let Some(offset) = new_offset {
        let offset_changed = offset != current_offset;
        if offset_changed || source_target_cancel_pending {
            if let NodeKind::DocumentView(dv) = &mut tree.node_mut(id).kind {
                dv.scroll_offset = offset;
                dv.smooth_scroll.cancel_at(offset);
                dv.cancelled_scroll_to_source_line = dv.scroll_to_source_line;
            }
            if offset_changed && let Some(cb) = on_scroll.as_ref() {
                cb.emit(ScrollEvent {
                    offset,
                    metrics: ScrollMetrics {
                        len: total,
                        visible: viewport_h,
                        max_offset,
                    },
                });
            }
            handled = true;
        }
    }

    if !handled && let Some(cb) = on_key.as_ref() {
        handled = handle_key_cb(cb);
    }

    handled
}

// ── Scroll wheel ────────────────────────────────────────────────────────────

/// Handle scroll-wheel events for a DocumentView node.
pub(crate) fn handle_scroll(tree: &mut NodeTree, id: NodeId, action: ScrollAction) -> bool {
    // Immutable read phase.
    let node = tree.node(id);
    let rect = node.rect;
    let NodeKind::DocumentView(dv) = &node.kind else {
        return false;
    };

    let inner = rect.inner(dv.border, dv.padding);
    let cl = dv.content_layout(inner);
    let viewport_h = cl.content_height as usize;
    let total = dv.total_visual_lines;
    let max_offset = total.saturating_sub(viewport_h);
    let current_offset = dv.scroll_offset;
    let on_scroll = dv.on_scroll.clone();
    let source_target_cancel_pending = dv.scroll_to_source_line.is_some()
        && dv.cancelled_scroll_to_source_line != dv.scroll_to_source_line;

    if !dv.scroll_wheel {
        return false;
    }

    if viewport_h == 0 || total <= viewport_h {
        if source_target_cancel_pending {
            if let NodeKind::DocumentView(dv) = &mut tree.node_mut(id).kind {
                dv.smooth_scroll.cancel_at(current_offset);
                dv.cancelled_scroll_to_source_line = dv.scroll_to_source_line;
                dv.scroll_override = Some(current_offset);
            }
            return true;
        }
        return false;
    }

    let metrics = ScrollMetrics {
        len: total,
        visible: viewport_h,
        max_offset,
    };
    let next = apply_scroll_action(current_offset, metrics, action).min(max_offset);

    // Mutable write phase.
    let offset_changed = next != current_offset;
    if offset_changed || source_target_cancel_pending {
        if let NodeKind::DocumentView(dv) = &mut tree.node_mut(id).kind {
            dv.scroll_offset = next;
            dv.scroll_override = Some(next);
            dv.smooth_scroll.cancel_at(next);
            dv.cancelled_scroll_to_source_line = dv.scroll_to_source_line;
        }
        if offset_changed && let Some(cb) = on_scroll.as_ref() {
            cb.emit(ScrollEvent {
                offset: next,
                metrics,
            });
        }
        true
    } else {
        false
    }
}
