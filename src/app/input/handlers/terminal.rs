//! Terminal keyboard and scroll-wheel handlers.

use crate::callback::KeyHandler;
use crate::clipboard::{ClipboardConfig, ClipboardPasteContent, ClipboardService, write_osc52};
use crate::core::event::{KeyCode, KeyEvent, KeyMods, MouseEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::utils::SelectionEnd;
use crate::utils::hints::HintScan;
use crate::widgets::internal::{
    apply_scroll_action, terminal_mouse_content_rect, terminal_node_selection_text,
};
use crate::widgets::{ScrollEvent, ScrollMetrics, TerminalLinkEvent};
use crate::widgets::{TerminalInputKind, TerminalPasteShortcutBehavior, encode_paste};

/// A link activation resolved from one terminal cell.
pub(crate) struct TerminalLinkHit {
    pub node_id: NodeId,
    pub event: TerminalLinkEvent,
    pub callback: crate::callback::Callback<TerminalLinkEvent>,
}

/// Resolve a link press, including modifier and ancestor-capture policy.
pub(crate) fn modified_link_hit(tree: &NodeTree, mouse: MouseEvent) -> Option<TerminalLinkHit> {
    let hit = tree.hit_test(mouse.x as i16, mouse.y as i16)?;
    let NodeKind::Terminal(term) = &tree.node(hit).kind else {
        return None;
    };
    term.on_link_activate.as_ref()?;
    if !mods_contain(term.link_activation_mods, mouse.mods)
        || crate::app::input::mouse::ancestor_mouse_region_captures_mods(tree, hit, mouse.mods)
    {
        return None;
    }
    link_hit_at_node(tree, hit, mouse)
}

/// Resolve a release against the same terminal that owned the press.
pub(crate) fn link_hit_at_node(
    tree: &NodeTree,
    node_id: NodeId,
    mouse: MouseEvent,
) -> Option<TerminalLinkHit> {
    if tree.hit_test(mouse.x as i16, mouse.y as i16)? != node_id {
        return None;
    }
    let NodeKind::Terminal(term) = &tree.node(node_id).kind else {
        return None;
    };
    let callback = term.on_link_activate.clone()?;
    let content = terminal_mouse_content_rect(tree, node_id)?;
    if !content.contains(mouse.x as i16, mouse.y as i16) {
        return None;
    }
    let row = usize::from(mouse.y.saturating_sub(content.y.max(0) as u16));
    let col = usize::from(mouse.x.saturating_sub(content.x.max(0) as u16));
    let uri = terminal_link_at(term, row, col)?;
    Some(TerminalLinkHit {
        node_id,
        event: TerminalLinkEvent { uri, row, col },
        callback,
    })
}

fn terminal_link_at(
    term: &crate::widgets::internal::TerminalNode,
    row: usize,
    col: usize,
) -> Option<std::sync::Arc<str>> {
    if let Some(link) = term.hyperlinks.iter().find(|link| link.contains(row, col)) {
        return Some(link.uri.clone());
    }

    HintScan::new()
        .paths(false)
        .git_shas(false)
        .scan_wrapped(&term.text, &term.wrapped_rows)
        .into_iter()
        .find(|matched| {
            matched
                .spans
                .iter()
                .any(|span| span.row == row && (span.start_col..span.end_col).contains(&col))
        })
        .map(|matched| matched.text.into())
}

fn mods_contain(required: KeyMods, actual: KeyMods) -> bool {
    (!required.ctrl || actual.ctrl)
        && (!required.alt || actual.alt)
        && (!required.shift || actual.shift)
        && (!required.super_key || actual.super_key)
}

/// Handle keyboard input for a focused Terminal node.
pub(crate) fn handle_key(
    tree: &mut NodeTree,
    id: NodeId,
    key: KeyEvent,
    ctx: &mut super::KeyCtx<'_>,
) -> bool {
    if preflight_key(tree, id, key, ctx.clipboard, ctx.clipboard_config).is_consumed() {
        return true;
    }
    let forward = forward_key(tree, id, key);
    if let Some(level) = forward.dirty_override() {
        ctx.dirty_override = Some(level);
    }
    forward.handled
}

/// Performable terminal-local clipboard/paste handling before app commands.
pub(crate) fn preflight_key(
    tree: &mut NodeTree,
    id: NodeId,
    key: KeyEvent,
    clipboard: &ClipboardService,
    clipboard_config: &ClipboardConfig,
) -> TerminalPreflightResult {
    let node = tree.node(id);
    let NodeKind::Terminal(node) = &node.kind else {
        return TerminalPreflightResult::NotApplicable;
    };

    let is_ctrl_c = key.mods.ctrl && matches!(key.code, KeyCode::Char('C') | KeyCode::Char('c'));
    let is_plain_ctrl_v = key.mods.ctrl
        && !key.mods.shift
        && !key.mods.alt
        && !key.mods.super_key
        && matches!(key.code, KeyCode::Char('V') | KeyCode::Char('v'));
    let has_selection = node.selection.as_ref().is_some_and(|sel| !sel.is_empty());

    if is_plain_ctrl_v && node.paste_shortcut_behavior == TerminalPasteShortcutBehavior::Performable
    {
        let Some(on_input) = node.on_input.as_ref() else {
            return TerminalPreflightResult::Forward;
        };
        match clipboard.read_terminal_paste() {
            Ok(ClipboardPasteContent::Text(text)) => {
                let text = truncate_paste(&text, clipboard_config.paste_max_bytes);
                let bytes = encode_paste(&text, node.key_modes);
                on_input.emit(crate::widgets::TerminalInputEvent {
                    kind: TerminalInputKind::Paste,
                    key: Some(key),
                    bytes: bytes.into(),
                });
                return TerminalPreflightResult::Consumed;
            }
            Ok(ClipboardPasteContent::Rich | ClipboardPasteContent::Unavailable) => {
                return TerminalPreflightResult::Forward;
            }
            Err(err) => {
                clipboard.report_error(err);
                return TerminalPreflightResult::Consumed;
            }
        }
    }

    if is_ctrl_c && (has_selection || key.mods.shift) {
        if let Some(sel) = node.selection.as_ref()
            && !sel.is_empty()
        {
            let text = terminal_node_selection_text(node, sel, SelectionEnd::Exclusive, false);
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
        return TerminalPreflightResult::Consumed;
    }

    if key.mods.ctrl
        && key.mods.shift
        && matches!(key.code, KeyCode::Char('V') | KeyCode::Char('v'))
    {
        let NodeKind::Terminal(node) = &tree.node(id).kind else {
            return TerminalPreflightResult::NotApplicable;
        };
        if let Some(on_input) = node.on_input.as_ref() {
            match clipboard.read_clipboard_text() {
                Ok(text) => {
                    let text = truncate_paste(&text, clipboard_config.paste_max_bytes);
                    let bytes = encode_paste(&text, node.key_modes);
                    on_input.emit(crate::widgets::TerminalInputEvent {
                        kind: TerminalInputKind::Paste,
                        key: Some(key),
                        bytes: bytes.into(),
                    });
                }
                Err(err) => clipboard.report_error(err),
            }
            return TerminalPreflightResult::Consumed;
        }
        return TerminalPreflightResult::NotConsumed;
    }

    if is_ctrl_c || (key.mods.ctrl && key.mods.shift) {
        return TerminalPreflightResult::NotConsumed;
    }

    TerminalPreflightResult::NotApplicable
}

/// What forwarding a key to a terminal did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalKeyForward {
    /// The terminal had an `on_key` handler and it claimed the key.
    pub handled: bool,
    /// Node state the renderer reads changed here — only a live selection being dropped.
    ///
    /// The key itself changes nothing on screen: it becomes bytes for the child program, and
    /// whatever the child draws in response arrives later as output. So a forwarded key must not
    /// claim a frame on its own; the snapshot the output produces is what asks for one.
    pub mutated: bool,
}

impl TerminalKeyForward {
    /// The dirty level a forwarded key deserves, or [`None`] to leave the generic handled-key
    /// level in place. Mirrors [`crate::app::copy_feedback`]: handled without mutating means
    /// handled without repainting.
    pub(crate) fn dirty_override(self) -> Option<crate::app::runner::DirtyLevel> {
        (self.handled && !self.mutated).then_some(crate::app::runner::DirtyLevel::None)
    }
}

/// Forward unmatched keys to terminal callbacks.
pub(crate) fn forward_key(tree: &mut NodeTree, id: NodeId, key: KeyEvent) -> TerminalKeyForward {
    let node = tree.node(id);
    let NodeKind::Terminal(node) = &node.kind else {
        return TerminalKeyForward::default();
    };

    let handle_key = |handler: &KeyHandler| -> bool { handler.handle(key) };
    let has_selection = node.selection.as_ref().is_some_and(|sel| !sel.is_empty());
    let on_key_cb = node.on_key.clone();
    let on_selection_cb = node.on_selection.clone();

    let mut mutated = false;
    if has_selection && on_key_cb.is_some() {
        if let NodeKind::Terminal(term) = &mut tree.node_mut(id).kind {
            term.selection = None;
        }
        mutated = true;
        if let Some(cb) = on_selection_cb {
            cb.emit(crate::widgets::TerminalSelectionEvent {
                selection: None,
                text: None,
            });
        }
    }

    TerminalKeyForward {
        handled: on_key_cb.as_ref().map(&handle_key).unwrap_or(false),
        mutated,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalPreflightResult {
    Consumed,
    Forward,
    NotApplicable,
    NotConsumed,
}

impl TerminalPreflightResult {
    pub(crate) fn is_consumed(self) -> bool {
        matches!(self, Self::Consumed)
    }
}

pub(crate) fn handle_paste(tree: &mut NodeTree, id: NodeId, text: &str) -> bool {
    let node = tree.node(id);
    let NodeKind::Terminal(node) = &node.kind else {
        return false;
    };

    let Some(on_input) = node.on_input.as_ref() else {
        return false;
    };

    let bytes = encode_paste(text, node.key_modes);
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

    let Some((next_scrollback, metrics)) = terminal_scroll_target(term, action) else {
        return false;
    };

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

pub(crate) fn terminal_scroll_target(
    term: &crate::widgets::internal::TerminalNode,
    action: crate::widgets::internal::ScrollAction,
) -> Option<(usize, ScrollMetrics)> {
    let total = term.viewport_rows + term.total_scrollback_rows;
    let visible = term.viewport_rows;
    if !term.scroll_wheel || term.total_scrollback_rows == 0 || visible == 0 || total <= visible {
        return None;
    }
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
    (next_scrollback != term.scrollback_offset).then_some((next_scrollback, metrics))
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
