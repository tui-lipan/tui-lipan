use unicode_width::UnicodeWidthStr;

use super::mod_private::Terminal;
use crate::style::{Rect, ScrollbarVariant};

#[derive(Clone, Copy, Debug)]
pub(crate) struct TerminalContentLayout {
    pub content_rect: Rect,
    pub scrollbar_visible: bool,
    pub scrollbar_integrated: bool,
}

pub(crate) fn terminal_content_layout(
    inner: Rect,
    border: bool,
    scrollbar: bool,
    scrollbar_variant: ScrollbarVariant,
    scrollbar_gap: u16,
    total_scrollback_rows: usize,
    parent_integrated_v_edge: bool,
) -> TerminalContentLayout {
    let scrollbar_visible = scrollbar && total_scrollback_rows > 0 && inner.w > 0 && inner.h > 0;
    let scrollbar_integrated = scrollbar_visible
        && matches!(scrollbar_variant, ScrollbarVariant::Integrated)
        && (border || parent_integrated_v_edge);

    let mut content_rect = inner;
    if scrollbar_visible && !scrollbar_integrated {
        content_rect.w = content_rect
            .w
            .saturating_sub(1u16.saturating_add(scrollbar_gap));
    }

    TerminalContentLayout {
        content_rect,
        scrollbar_visible,
        scrollbar_integrated,
    }
}

/// Compute the terminal content rect for mouse event handling.
///
/// Performs the shared sequence: match `NodeKind::Terminal`, compute whether the parent
/// frame exposes a vertical integrated scrollbar edge, then `terminal_content_layout`.
/// Returns `None` when `id` is not a valid terminal node.
pub(crate) fn terminal_mouse_content_rect(
    tree: &crate::core::node::NodeTree,
    id: crate::core::node::NodeId,
) -> Option<Rect> {
    use crate::core::node::NodeKind;

    if !tree.is_valid(id) {
        return None;
    }
    let node = tree.node(id);
    let NodeKind::Terminal(term) = &node.kind else {
        return None;
    };

    let parent_v_edge = tree.parent_frame_integrated_v_edge(id);
    let inner = node.rect.inner(term.border, term.padding);
    let layout = terminal_content_layout(
        inner,
        term.border,
        term.scrollbar,
        term.scrollbar_variant,
        term.scrollbar_gap,
        term.total_scrollback_rows,
        parent_v_edge.unwrap_or(false),
    );

    Some(layout.content_rect)
}

pub(crate) fn measure_terminal(terminal: &Terminal) -> (u16, u16) {
    let lines = terminal
        .color_lines
        .clone()
        .unwrap_or_else(|| terminal_lines(terminal.content.as_ref()));

    let scrollbar_visible = terminal.scrollbar && terminal.total_scrollback_rows > 0;
    let scrollbar_integrated = scrollbar_visible
        && matches!(terminal.scrollbar_variant, ScrollbarVariant::Integrated)
        && terminal.border;
    let scrollbar_cols = if scrollbar_visible && !scrollbar_integrated {
        1u16.saturating_add(terminal.scrollbar_gap)
    } else {
        0
    };

    let width = lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
                .sum::<usize>()
        })
        .max()
        .unwrap_or(0)
        .saturating_add(terminal.padding.horizontal() as usize)
        .saturating_add(if terminal.border { 2 } else { 0 })
        .saturating_add(scrollbar_cols as usize) as u16;

    let height = lines
        .len()
        .saturating_add(terminal.padding.vertical() as usize)
        .saturating_add(if terminal.border { 2 } else { 0 }) as u16;

    (width.max(1), height.max(1))
}

use crate::style::Span;
use std::sync::Arc;

pub(crate) fn terminal_lines(value: &str) -> Arc<[Vec<Span>]> {
    if value.is_empty() {
        return Arc::new([vec![Span::new("")]]);
    }

    value
        .split('\n')
        .map(|line| vec![Span::new(line.to_string())])
        .collect::<Vec<_>>()
        .into()
}
