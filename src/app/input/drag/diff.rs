//! Drag handling for diff-split `DocumentView` selection.

#[cfg(feature = "diff-view")]
use std::sync::Arc;

#[cfg(feature = "diff-view")]
use crate::core::node::{NodeId, NodeKind, NodeTree};
#[cfg(feature = "diff-view")]
use crate::ui::capabilities::selection_range;
#[cfg(feature = "diff-view")]
use crate::widgets::document_view::node::DocumentViewNode;

#[cfg(feature = "diff-view")]
use super::*;

#[cfg(feature = "diff-view")]
fn diff_split_pane_for_doc(tree: &NodeTree, id: NodeId) -> Option<crate::widgets::DiffPane> {
    if !tree.is_valid(id) {
        return None;
    }
    let NodeKind::DocumentView(doc) = &tree.node(id).kind else {
        return None;
    };
    match doc.diff_split_pane {
        Some(crate::widgets::DiffPane::Left | crate::widgets::DiffPane::Right) => {
            doc.diff_split_pane
        }
        _ => None,
    }
}

#[cfg(feature = "diff-view")]
fn collect_diff_split_docs_in_subtree(
    tree: &NodeTree,
    id: NodeId,
    left: &mut Option<NodeId>,
    right: &mut Option<NodeId>,
) {
    if !tree.is_valid(id) {
        return;
    }
    if let NodeKind::DocumentView(doc) = &tree.node(id).kind {
        match doc.diff_split_pane {
            Some(crate::widgets::DiffPane::Left) => *left = Some(id),
            Some(crate::widgets::DiffPane::Right) => *right = Some(id),
            _ => {}
        }
        return;
    }

    for &child in &tree.node(id).children.clone() {
        collect_diff_split_docs_in_subtree(tree, child, left, right);
    }
}

#[cfg(feature = "diff-view")]
fn diff_split_pair_for_document_view(
    tree: &NodeTree,
    doc_id: NodeId,
) -> Option<DiffSplitDocumentPair> {
    diff_split_pane_for_doc(tree, doc_id)?;

    let mut cur = tree.node(doc_id).parent;
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            return None;
        }
        if matches!(tree.node(id).kind, NodeKind::HStack(_)) {
            let mut left = None;
            let mut right = None;
            collect_diff_split_docs_in_subtree(tree, id, &mut left, &mut right);
            if let (Some(left), Some(right)) = (left, right)
                && (doc_id == left || doc_id == right)
            {
                return Some(DiffSplitDocumentPair { left, right });
            }
        }
        cur = tree.node(id).parent;
    }

    None
}

#[cfg(feature = "diff-view")]
fn diff_split_pointer_pane(
    tree: &NodeTree,
    pair: DiffSplitDocumentPair,
    anchor_pane: crate::widgets::DiffPane,
    x: u16,
    y: u16,
) -> crate::widgets::DiffPane {
    if let Some(hit_doc) = tree
        .hit_test(x as i16, y as i16)
        .and_then(|hit| nearest_ancestor_document_view(tree, hit))
    {
        if hit_doc == pair.left {
            return crate::widgets::DiffPane::Left;
        }
        if hit_doc == pair.right {
            return crate::widgets::DiffPane::Right;
        }
    }

    let left_rect = tree.node(pair.left).rect;
    let right_rect = tree.node(pair.right).rect;
    let x_i16 = x as i16;
    if x_i16 < right_rect.x {
        crate::widgets::DiffPane::Left
    } else if x_i16 >= right_rect.x {
        crate::widgets::DiffPane::Right
    } else if x_i16 <= left_rect.x.saturating_add(left_rect.w as i16) {
        crate::widgets::DiffPane::Left
    } else {
        anchor_pane
    }
}

#[cfg(feature = "diff-view")]
fn visual_index_for_document_cursor(doc: &DocumentViewNode, cursor: usize) -> usize {
    if doc.visual_cache.line_starts.is_empty() {
        return 0;
    }

    for (idx, &line_start) in doc.visual_cache.line_starts.iter().enumerate() {
        let line_len = doc.visual_cache.line_lengths.get(idx).copied().unwrap_or(0);
        let line_end = line_start.saturating_add(line_len);
        if cursor <= line_end {
            return idx;
        }
    }

    doc.visual_cache.line_starts.len().saturating_sub(1)
}

#[cfg(feature = "diff-view")]
fn full_line_range_for_visual_rows(
    doc: &DocumentViewNode,
    row_start: usize,
    row_end: usize,
) -> Option<(usize, usize)> {
    if doc.visual_cache.line_starts.is_empty() {
        return None;
    }
    let max_row = doc.visual_cache.line_starts.len().saturating_sub(1);
    let start_row = row_start.min(max_row);
    let end_row = row_end.min(max_row);
    let start = *doc.visual_cache.line_starts.get(start_row)?;
    let end = doc
        .visual_cache
        .line_starts
        .get(end_row)
        .copied()?
        .saturating_add(
            doc.visual_cache
                .line_lengths
                .get(end_row)
                .copied()
                .unwrap_or(0),
        );
    Some((start.min(end), end))
}

#[cfg(feature = "diff-view")]
fn selected_visual_row_range(doc: &DocumentViewNode) -> Option<(usize, usize)> {
    let (start, end) = selection_range(
        doc.selection_cursor,
        doc.selection_anchor,
        doc.visual_cache.flat_text.len(),
    )?;
    if start >= end {
        return None;
    }
    let start_row = visual_index_for_document_cursor(doc, start);
    let end_row = visual_index_for_document_cursor(doc, end.saturating_sub(1));
    Some((start_row.min(end_row), start_row.max(end_row)))
}

#[cfg(feature = "diff-view")]
fn selected_source_line_bounds(doc: &DocumentViewNode) -> Option<(usize, usize)> {
    let (row_start, row_end) = selected_visual_row_range(doc)?;
    let mut min_source = usize::MAX;
    let mut max_source = 0usize;
    let mut found = false;
    for row in row_start..=row_end {
        let Some(&source_line) = doc.visual_cache.source_line_map.get(row) else {
            continue;
        };
        min_source = min_source.min(source_line);
        max_source = max_source.max(source_line);
        found = true;
    }
    found.then_some((min_source, max_source))
}

#[cfg(feature = "diff-view")]
fn diff_formatter_for_doc(
    doc: &DocumentViewNode,
) -> Option<&crate::widgets::DiffDocumentFormatter> {
    doc.formatter
        .as_ref()?
        .as_any()
        .downcast_ref::<crate::widgets::DiffDocumentFormatter>()
}

#[cfg(feature = "diff-view")]
fn diff_split_text_for_source_lines(
    tree: &NodeTree,
    pair: DiffSplitDocumentPair,
    source_start: usize,
    source_end: usize,
) -> Arc<str> {
    let NodeKind::DocumentView(left) = &tree.node(pair.left).kind else {
        return Arc::from("");
    };
    let NodeKind::DocumentView(right) = &tree.node(pair.right).kind else {
        return Arc::from("");
    };
    let Some(left_formatter) = diff_formatter_for_doc(left) else {
        return Arc::from("");
    };
    let Some(right_formatter) = diff_formatter_for_doc(right) else {
        return Arc::from("");
    };

    let mut lines = Vec::new();
    for source_line in source_start..=source_end {
        let left_text = left_formatter
            .logical_line_text_for_copy(source_line)
            .flatten();
        let right_text = right_formatter
            .logical_line_text_for_copy(source_line)
            .flatten();
        if left_text.is_some() || right_text.is_some() {
            lines.push(format!(
                "{}\t{}",
                left_text.as_deref().unwrap_or(""),
                right_text.as_deref().unwrap_or(""),
            ));
        }
    }

    Arc::from(lines.join("\n"))
}

#[cfg(feature = "diff-view")]
pub(crate) fn handle_diff_split_document_view_drag(
    tree: &NodeTree,
    x: u16,
    y: u16,
    id: NodeId,
    anchor_local: usize,
) -> Option<DiffSplitSelectionResult> {
    let anchor_pane = diff_split_pane_for_doc(tree, id)?;
    let pair = diff_split_pair_for_document_view(tree, id)?;
    let peer_id = match anchor_pane {
        crate::widgets::DiffPane::Left => pair.right,
        crate::widgets::DiffPane::Right => pair.left,
        crate::widgets::DiffPane::Unified => return None,
    };
    let pointer_pane = diff_split_pointer_pane(tree, pair, anchor_pane, x, y);

    if pointer_pane == anchor_pane {
        let NodeKind::DocumentView(anchor_doc) = &tree.node(id).kind else {
            return None;
        };
        let cursor = document_view_cursor_from_coords(tree, x, y, id)?
            .min(anchor_doc.visual_cache.flat_text.len());
        let anchor = anchor_local.min(anchor_doc.visual_cache.flat_text.len());
        let peer_cursor = match &tree.node(peer_id).kind {
            NodeKind::DocumentView(peer) => peer.selection_cursor,
            _ => 0,
        };
        return Some(DiffSplitSelectionResult {
            updates: vec![
                DocumentViewSelectionUpdate {
                    id,
                    cursor,
                    anchor: Some(anchor),
                },
                DocumentViewSelectionUpdate {
                    id: peer_id,
                    cursor: peer_cursor,
                    anchor: None,
                },
            ],
        });
    }

    let pointer_id = match pointer_pane {
        crate::widgets::DiffPane::Left => pair.left,
        crate::widgets::DiffPane::Right => pair.right,
        crate::widgets::DiffPane::Unified => return None,
    };
    let pointer_cursor = document_view_cursor_from_coords(tree, x, y, pointer_id)?;

    let NodeKind::DocumentView(anchor_doc) = &tree.node(id).kind else {
        return None;
    };
    let NodeKind::DocumentView(pointer_doc) = &tree.node(pointer_id).kind else {
        return None;
    };
    let anchor_row = visual_index_for_document_cursor(anchor_doc, anchor_local);
    let pointer_row = visual_index_for_document_cursor(pointer_doc, pointer_cursor);
    let row_start = anchor_row.min(pointer_row);
    let row_end = anchor_row.max(pointer_row);

    let mut updates = Vec::new();
    for doc_id in [pair.left, pair.right] {
        let NodeKind::DocumentView(doc) = &tree.node(doc_id).kind else {
            continue;
        };
        if let Some((start, end)) = full_line_range_for_visual_rows(doc, row_start, row_end)
            && start < end
        {
            updates.push(DocumentViewSelectionUpdate {
                id: doc_id,
                cursor: end,
                anchor: Some(start),
            });
        }
    }

    if updates.is_empty() {
        return None;
    }

    Some(DiffSplitSelectionResult { updates })
}

#[cfg(feature = "diff-view")]
pub(crate) fn document_view_diff_split_selection_text(
    tree: &NodeTree,
    doc_id: NodeId,
) -> Option<DiffSplitSelectionText> {
    let pair = diff_split_pair_for_document_view(tree, doc_id)?;
    let NodeKind::DocumentView(left) = &tree.node(pair.left).kind else {
        return None;
    };
    let NodeKind::DocumentView(right) = &tree.node(pair.right).kind else {
        return None;
    };
    let left_range = selected_source_line_bounds(left)?;
    let right_range = selected_source_line_bounds(right)?;
    let source_start = left_range.0.min(right_range.0);
    let source_end = left_range.1.max(right_range.1);
    let selected_text = diff_split_text_for_source_lines(tree, pair, source_start, source_end);
    if selected_text.is_empty() {
        return None;
    }
    Some(DiffSplitSelectionText {
        left_id: pair.left,
        right_id: pair.right,
        selected_text,
    })
}
