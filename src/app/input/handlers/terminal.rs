//! Terminal keyboard and scroll-wheel handlers.

use crate::callback::KeyHandler;
use crate::clipboard::{ClipboardConfig, ClipboardService, write_osc52};
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::internal::apply_scroll_action;
use crate::widgets::{ScrollEvent, ScrollMetrics};
use crate::widgets::{TerminalInputKind, wrap_bracketed_paste};

/// Handle keyboard input for a focused Terminal node.
pub(crate) fn handle_key(
    tree: &mut NodeTree,
    id: NodeId,
    key: KeyEvent,
    clipboard: &ClipboardService,
    clipboard_config: &ClipboardConfig,
) -> bool {
    let node = tree.node(id);
    let NodeKind::Terminal(node) = &node.kind else {
        return false;
    };

    let handle_key = |handler: &KeyHandler| -> bool { handler.handle(key) };

    // Copy selection: Ctrl+C (when selection exists) or Ctrl+Shift+C
    let is_ctrl_c = key.mods.ctrl && matches!(key.code, KeyCode::Char('C') | KeyCode::Char('c'));
    let has_selection = node.selection.as_ref().is_some_and(|sel| !sel.is_empty());

    // Ctrl+C copies if selection exists; Ctrl+Shift+C always copies
    if is_ctrl_c && (has_selection || key.mods.shift) {
        if let Some(sel) = node.selection.as_ref()
            && !sel.is_empty()
        {
            let line_texts: Vec<String> = node
                .lines
                .iter()
                .map(|spans| {
                    let mut line = String::new();
                    for span in spans {
                        line.push_str(span.content.as_ref());
                    }
                    line
                })
                .collect();
            let text = sel.extract_text(&line_texts);
            if !text.is_empty() {
                if let Err(err) = clipboard.write_clipboard_text(&text) {
                    clipboard.report_error(err);
                }
                if clipboard_config.enable_osc52 {
                    write_osc52(&text);
                }
                if clipboard_config.enable_primary_selection
                    && clipboard.supports_primary_selection()
                    && let Err(err) = clipboard.write_primary_selection_text(&text)
                    && !matches!(err, crate::clipboard::ClipboardError::Unsupported { .. })
                {
                    clipboard.report_error(err);
                }
            }
        }
        return true;
    }

    // Ctrl+Shift+V paste
    if key.mods.ctrl
        && key.mods.shift
        && matches!(key.code, KeyCode::Char('V') | KeyCode::Char('v'))
        && let Some(on_input) = node.on_input.as_ref()
    {
        match clipboard.read_clipboard_text() {
            Ok(text) => {
                let text = truncate_paste(&text, clipboard_config.paste_max_bytes);
                let bytes = wrap_bracketed_paste(&text);
                on_input.emit(crate::widgets::TerminalInputEvent {
                    kind: TerminalInputKind::Paste,
                    key: Some(key),
                    bytes: bytes.into(),
                });
            }
            Err(err) => clipboard.report_error(err),
        }
        return true;
    }

    // Clone callbacks before mutating
    let on_key_cb = node.on_key.clone();
    let on_selection_cb = node.on_selection.clone();

    // Clear selection when user types (any key that will be forwarded to PTY)
    if has_selection && on_key_cb.is_some() {
        // Clear selection in the node
        if let NodeKind::Terminal(term) = &mut tree.node_mut(id).kind {
            term.selection = None;
        }
        // Notify about selection clear
        if let Some(cb) = on_selection_cb {
            cb.emit(crate::widgets::TerminalSelectionEvent {
                selection: None,
                text: None,
            });
        }
    }

    on_key_cb.as_ref().map(&handle_key).unwrap_or(false)
}

pub(crate) fn handle_paste(tree: &mut NodeTree, id: NodeId, text: &str) -> bool {
    let node = tree.node(id);
    let NodeKind::Terminal(node) = &node.kind else {
        return false;
    };

    let Some(on_input) = node.on_input.as_ref() else {
        return false;
    };

    let bytes = wrap_bracketed_paste(text);
    on_input.emit(crate::widgets::TerminalInputEvent {
        kind: TerminalInputKind::Paste,
        key: None,
        bytes: bytes.into(),
    });
    true
}

/// Handle scroll-wheel events for a Terminal node.
pub(crate) fn handle_scroll(
    tree: &mut NodeTree,
    id: NodeId,
    action: crate::widgets::internal::ScrollAction,
) -> bool {
    let NodeKind::Terminal(term) = &mut tree.node_mut(id).kind else {
        return false;
    };

    let total = term.viewport_rows + term.total_scrollback_rows;
    let visible = term.viewport_rows;
    let can_scroll =
        term.scroll_wheel && term.total_scrollback_rows > 0 && visible > 0 && total > visible;

    if !can_scroll {
        return false;
    }

    // Terminal scrollback is inverted: offset 0 = live view (bottom),
    // higher values = scrolled into history.
    let metrics = ScrollMetrics {
        len: total,
        visible,
        max_offset: term.total_scrollback_rows,
    };
    let std_offset = term
        .total_scrollback_rows
        .saturating_sub(term.scrollback_offset);
    let next_std = apply_scroll_action(std_offset, metrics, action).min(metrics.max_offset);
    let next_scrollback = term.total_scrollback_rows.saturating_sub(next_std);

    if next_scrollback == term.scrollback_offset {
        return false;
    }

    term.scrollback_offset = next_scrollback;
    term.scroll_override = Some(next_scrollback);
    if let Some(cb) = term.on_scroll_to.as_ref() {
        cb.emit(next_scrollback);
    } else if let Some(cb) = term.on_scroll.as_ref() {
        cb.emit(ScrollEvent {
            offset: next_scrollback,
            metrics,
        });
    }
    true
}

/// Truncate a paste string to at most `max_bytes`, ensuring we don't split a
/// multi-byte UTF-8 character.
fn truncate_paste(text: &str, max_bytes: usize) -> String {
    if max_bytes == 0 || text.len() <= max_bytes {
        return text.to_string();
    }

    let mut end = max_bytes.min(text.len());
    while end > 0 && !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }

    text[..end].to_string()
}
