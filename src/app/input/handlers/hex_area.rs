//! HexArea keyboard and scroll-wheel handlers.

use std::sync::Arc;

use crate::app::input::handlers::KeyCtx;
use crate::app::input::hex_history::HexHistory;
use crate::app::input::keymap::Action;
use crate::app::interaction_state::HexPendingEdit;
use crate::callback::KeyHandler;
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::internal::apply_scroll_action;
use crate::widgets::{
    HexAreaChangeEvent, HexAreaCursorEvent, HexAreaEditEvent, HexAreaEditKind, ScrollEvent,
    ScrollMetrics,
};

/// Handle keyboard input for a focused HexArea node.
pub(crate) fn handle_key(
    tree: &mut NodeTree,
    id: NodeId,
    key: KeyEvent,
    ctx: &mut KeyCtx<'_>,
) -> bool {
    let handle_key = |handler: &KeyHandler| -> bool { handler.handle(key) };

    // ── 1. Immutable borrow: read node state and clone callbacks ────────
    let NodeKind::HexArea(node) = &tree.node(id).kind else {
        return false;
    };

    let mut handled = false;
    let len = node.bytes.len();
    let step = node.bytes_per_row.max(1) as usize;
    let old_cursor = if len == 0 {
        0
    } else {
        node.cursor.min(len.saturating_sub(1))
    };
    let mut next_cursor = old_cursor;
    let mut next_anchor = node.anchor;
    let on_cursor_change = node.on_cursor_change.clone();
    let on_change = node.on_change.clone();
    let on_edit = node.on_edit.clone();
    let on_scroll = node.on_scroll.clone();
    let on_key = node.on_key.clone();

    // ── 2. Read rect for page calculation ───────────────────────────────
    let page_rows = tree.node(id).rect.inner(node.border, node.padding).h.max(1) as usize;
    let page_step = page_rows.saturating_mul(step).max(1);

    // We need several more fields from the node for editing logic below.
    let disabled = node.disabled;
    let read_only = node.read_only;
    let scroll_offset = node.scroll_offset;
    let node_anchor = node.anchor;
    let bytes = node.bytes.clone();

    // ── 3. Get pending_edit from ctx ────────────────────────────────────
    let mut pending_edit = ctx.hex_pending_edit.get(&id).copied();

    // ── 4. Process key ──────────────────────────────────────────────────
    if !disabled {
        let mut moved = false;
        if len > 0 {
            match key.code {
                KeyCode::Left => {
                    next_cursor = next_cursor.saturating_sub(1);
                    moved = true;
                }
                KeyCode::Right => {
                    next_cursor = (next_cursor + 1).min(len.saturating_sub(1));
                    moved = true;
                }
                KeyCode::Up => {
                    next_cursor = next_cursor.saturating_sub(step);
                    moved = true;
                }
                KeyCode::Down => {
                    next_cursor = (next_cursor + step).min(len.saturating_sub(1));
                    moved = true;
                }
                KeyCode::PageUp => {
                    next_cursor = next_cursor.saturating_sub(page_step);
                    moved = true;
                }
                KeyCode::PageDown => {
                    next_cursor = (next_cursor + page_step).min(len.saturating_sub(1));
                    moved = true;
                }
                KeyCode::Home => {
                    next_cursor = (next_cursor / step).saturating_mul(step);
                    moved = true;
                }
                KeyCode::End => {
                    let row_start = (next_cursor / step).saturating_mul(step);
                    next_cursor = row_start
                        .saturating_add(step.saturating_sub(1))
                        .min(len.saturating_sub(1));
                    moved = true;
                }
                _ => {}
            }
        }

        if moved {
            pending_edit = None;

            if key.mods.shift && !key.mods.ctrl && !key.mods.alt && !key.mods.super_key {
                next_anchor = next_anchor.or(Some(old_cursor));
            } else {
                next_anchor = None;
            }

            if (next_cursor != old_cursor || next_anchor != node_anchor)
                && let Some(cb) = on_cursor_change.as_ref()
            {
                cb.emit(HexAreaCursorEvent {
                    cursor: next_cursor,
                    anchor: next_anchor,
                });
            }

            if let Some(cb) = on_scroll.as_ref() {
                emit_hex_scroll_for_cursor(
                    cb,
                    len,
                    step,
                    page_rows,
                    scroll_offset,
                    old_cursor,
                    next_cursor,
                );
            }

            handled = true;
        } else if !read_only && on_change.is_some() {
            let history = ctx
                .hex_history
                .entry(id)
                .or_insert_with(|| HexHistory::new(bytes.clone(), old_cursor, node_anchor));
            history.sync_from(bytes.clone(), old_cursor, node_anchor);

            match ctx.keymap.resolve_action(key) {
                Action::Undo => {
                    if let Some((undo_bytes, cursor, anchor)) = history.undo() {
                        if let Some(cb) = on_change.as_ref() {
                            cb.emit(HexAreaChangeEvent {
                                bytes: undo_bytes,
                                cursor,
                                anchor,
                            });
                        }
                        if let Some(cb) = on_cursor_change.as_ref()
                            && (cursor != old_cursor || anchor != node_anchor)
                        {
                            cb.emit(HexAreaCursorEvent { cursor, anchor });
                        }
                        pending_edit = None;
                        handled = true;
                    }
                }
                Action::Redo => {
                    if let Some((redo_bytes, cursor, anchor)) = history.redo() {
                        if let Some(cb) = on_change.as_ref() {
                            cb.emit(HexAreaChangeEvent {
                                bytes: redo_bytes,
                                cursor,
                                anchor,
                            });
                        }
                        if let Some(cb) = on_cursor_change.as_ref()
                            && (cursor != old_cursor || anchor != node_anchor)
                        {
                            cb.emit(HexAreaCursorEvent { cursor, anchor });
                        }
                        pending_edit = None;
                        handled = true;
                    }
                }
                _ => {
                    if matches!(key.code, KeyCode::Esc) && pending_edit.is_some() {
                        if let Some(pending) = pending_edit.take()
                            && pending.index < len
                        {
                            let mut raw = bytes.to_vec();
                            let current = raw[pending.index];
                            if current != pending.before_byte {
                                raw[pending.index] = pending.before_byte;
                                let new_bytes: Arc<[u8]> = Arc::from(raw.into_boxed_slice());
                                history.apply_change(new_bytes.clone(), pending.index, None);
                                if let Some(cb) = on_edit.as_ref() {
                                    cb.emit(HexAreaEditEvent {
                                        index: pending.index,
                                        before: Some(current),
                                        after: Some(pending.before_byte),
                                        kind: HexAreaEditKind::Replace,
                                    });
                                }
                                if let Some(cb) = on_change.as_ref() {
                                    cb.emit(HexAreaChangeEvent {
                                        bytes: new_bytes,
                                        cursor: pending.index,
                                        anchor: None,
                                    });
                                }
                                if let Some(cb) = on_cursor_change.as_ref()
                                    && (pending.index != old_cursor || node_anchor.is_some())
                                {
                                    cb.emit(HexAreaCursorEvent {
                                        cursor: pending.index,
                                        anchor: None,
                                    });
                                }
                            }
                        }
                        handled = true;
                    } else if let Some(nibble) = hex_nibble_for_key(key) {
                        if len > 0 {
                            if let Some(pending) = pending_edit.take() {
                                // Second nibble - complete the byte edit.
                                let index = pending.index.min(len.saturating_sub(1));
                                let mut raw = bytes.to_vec();
                                let before = raw[index];
                                let after = (pending.high_nibble << 4) | nibble;
                                raw[index] = after;

                                let next = (index + 1).min(len.saturating_sub(1));
                                let new_bytes: Arc<[u8]> = Arc::from(raw.into_boxed_slice());
                                history.apply_change(new_bytes.clone(), next, None);
                                if let Some(cb) = on_edit.as_ref() {
                                    cb.emit(HexAreaEditEvent {
                                        index,
                                        before: Some(before),
                                        after: Some(after),
                                        kind: HexAreaEditKind::Replace,
                                    });
                                }
                                if let Some(cb) = on_change.as_ref() {
                                    cb.emit(HexAreaChangeEvent {
                                        bytes: new_bytes,
                                        cursor: next,
                                        anchor: None,
                                    });
                                }
                                if let Some(cb) = on_cursor_change.as_ref()
                                    && next != old_cursor
                                {
                                    cb.emit(HexAreaCursorEvent {
                                        cursor: next,
                                        anchor: None,
                                    });
                                }
                            } else {
                                // First nibble - start pending edit.
                                let mut raw = bytes.to_vec();
                                let index = old_cursor.min(raw.len().saturating_sub(1));
                                let before = raw[index];
                                let after = nibble << 4;
                                raw[index] = after;

                                let new_bytes: Arc<[u8]> = Arc::from(raw.into_boxed_slice());
                                history.apply_change(new_bytes.clone(), index, None);
                                if let Some(cb) = on_edit.as_ref() {
                                    cb.emit(HexAreaEditEvent {
                                        index,
                                        before: Some(before),
                                        after: Some(after),
                                        kind: HexAreaEditKind::Replace,
                                    });
                                }
                                if let Some(cb) = on_change.as_ref() {
                                    cb.emit(HexAreaChangeEvent {
                                        bytes: new_bytes,
                                        cursor: index,
                                        anchor: None,
                                    });
                                }
                                pending_edit = Some(HexPendingEdit {
                                    index,
                                    high_nibble: nibble,
                                    before_byte: before,
                                });
                            }
                            handled = true;
                        }
                    } else {
                        match key.code {
                            KeyCode::Insert => {
                                let mut raw = bytes.to_vec();
                                let index = old_cursor.min(raw.len());
                                raw.insert(index, 0);
                                let new_bytes: Arc<[u8]> = Arc::from(raw.into_boxed_slice());
                                history.apply_change(new_bytes.clone(), index, None);
                                if let Some(cb) = on_edit.as_ref() {
                                    cb.emit(HexAreaEditEvent {
                                        index,
                                        before: None,
                                        after: Some(0),
                                        kind: HexAreaEditKind::Insert,
                                    });
                                }
                                if let Some(cb) = on_change.as_ref() {
                                    cb.emit(HexAreaChangeEvent {
                                        bytes: new_bytes,
                                        cursor: index,
                                        anchor: None,
                                    });
                                }
                                pending_edit = None;
                                handled = true;
                            }
                            KeyCode::Delete if len > 0 => {
                                let mut raw = bytes.to_vec();
                                let index = old_cursor.min(raw.len().saturating_sub(1));
                                let before = raw.remove(index);
                                let next = if raw.is_empty() {
                                    0
                                } else {
                                    index.min(raw.len().saturating_sub(1))
                                };
                                let new_bytes: Arc<[u8]> = Arc::from(raw.into_boxed_slice());
                                history.apply_change(new_bytes.clone(), next, None);
                                if let Some(cb) = on_edit.as_ref() {
                                    cb.emit(HexAreaEditEvent {
                                        index,
                                        before: Some(before),
                                        after: None,
                                        kind: HexAreaEditKind::Delete,
                                    });
                                }
                                if let Some(cb) = on_change.as_ref() {
                                    cb.emit(HexAreaChangeEvent {
                                        bytes: new_bytes,
                                        cursor: next,
                                        anchor: None,
                                    });
                                }
                                pending_edit = None;
                                handled = true;
                            }
                            KeyCode::Backspace if len > 0 && old_cursor > 0 => {
                                let mut raw = bytes.to_vec();
                                let index = old_cursor.saturating_sub(1);
                                let before = raw.remove(index);
                                let next = if raw.is_empty() {
                                    0
                                } else {
                                    index.min(raw.len().saturating_sub(1))
                                };
                                let new_bytes: Arc<[u8]> = Arc::from(raw.into_boxed_slice());
                                history.apply_change(new_bytes.clone(), next, None);
                                if let Some(cb) = on_edit.as_ref() {
                                    cb.emit(HexAreaEditEvent {
                                        index,
                                        before: Some(before),
                                        after: None,
                                        kind: HexAreaEditKind::Delete,
                                    });
                                }
                                if let Some(cb) = on_change.as_ref() {
                                    cb.emit(HexAreaChangeEvent {
                                        bytes: new_bytes,
                                        cursor: next,
                                        anchor: None,
                                    });
                                }
                                pending_edit = None;
                                handled = true;
                            }
                            _ => {}
                        }
                    }
                }
            }
        }
    }

    // ── 5. Update pending_edit back to ctx ──────────────────────────────
    if let Some(edit) = pending_edit {
        ctx.hex_pending_edit.insert(id, edit);
    } else {
        ctx.hex_pending_edit.remove(&id);
    }

    // ── 6. Mutably update node's pending_high_nibble ────────────────────
    if let NodeKind::HexArea(hex_node) = &mut tree.node_mut(id).kind {
        hex_node.pending_high_nibble = pending_edit.map(|edit| edit.high_nibble);
    }

    // ── 7. Fallback to on_key ───────────────────────────────────────────
    if !handled {
        handled = on_key.as_ref().map(&handle_key).unwrap_or(false);
    }

    handled
}

/// Handle scroll-wheel events for a HexArea node.
pub(crate) fn handle_scroll(
    tree: &mut NodeTree,
    id: NodeId,
    action: crate::widgets::internal::ScrollAction,
) -> bool {
    let node = tree.node(id);
    let NodeKind::HexArea(hex) = &node.kind else {
        return false;
    };

    if hex.disabled || hex.on_scroll.is_none() {
        return false;
    }

    let len = hex.bytes.len();
    let step = hex.bytes_per_row.max(1) as usize;
    let total_rows = len.div_ceil(step).max(1);
    let inner = node.rect.inner(hex.border, hex.padding);
    let visible = (inner.h as usize).max(1).min(total_rows);
    let metrics = ScrollMetrics {
        len: total_rows,
        visible,
        max_offset: total_rows.saturating_sub(visible),
    };

    let current = hex.scroll_offset.unwrap_or_else(|| {
        if len == 0 {
            0
        } else {
            (hex.cursor / step).saturating_sub(visible.saturating_sub(1))
        }
    });
    let current = current.min(metrics.max_offset);
    let next = apply_scroll_action(current, metrics, action).min(metrics.max_offset);

    if next != current
        && let Some(cb) = hex.on_scroll.as_ref()
    {
        cb.emit(ScrollEvent {
            offset: next,
            metrics,
        });
        true
    } else {
        false
    }
}

// ── Private helpers ─────────────────────────────────────────────────────────

fn hex_nibble_for_key(key: KeyEvent) -> Option<u8> {
    if key.mods.ctrl || key.mods.alt || key.mods.super_key {
        return None;
    }

    let KeyCode::Char(ch) = key.code else {
        return None;
    };

    ch.to_digit(16).map(|digit| digit as u8)
}

fn emit_hex_scroll_for_cursor(
    cb: &crate::callback::Callback<ScrollEvent>,
    len: usize,
    bytes_per_row: usize,
    visible_rows: usize,
    controlled_offset: Option<usize>,
    old_cursor: usize,
    new_cursor: usize,
) {
    if len == 0 || visible_rows == 0 {
        return;
    }

    let total_rows = len.div_ceil(bytes_per_row).max(1);
    let visible = visible_rows.min(total_rows);
    let max_offset = total_rows.saturating_sub(visible);

    let old_row = old_cursor / bytes_per_row;
    let new_row = new_cursor / bytes_per_row;
    let current_offset = controlled_offset.unwrap_or_else(|| old_row.saturating_sub(visible - 1));
    let current_offset = current_offset.min(max_offset);

    let mut next_offset = current_offset;
    if new_row < next_offset {
        next_offset = new_row;
    } else if new_row >= next_offset.saturating_add(visible) {
        next_offset = new_row.saturating_sub(visible.saturating_sub(1));
    }
    next_offset = next_offset.min(max_offset);

    if next_offset != current_offset {
        cb.emit(ScrollEvent {
            offset: next_offset,
            metrics: ScrollMetrics {
                len: total_rows,
                visible,
                max_offset,
            },
        });
    }
}
