//! Drag handling for sliders, progress bars, text areas, and inputs.

use std::sync::Arc;

use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::ui::capabilities::selection_range;
use crate::widgets::SingleDocSelection;
use crate::widgets::document_view::node::{DocumentViewNode, VisualLineKind};

use super::*;
fn document_view_excluded_newline_ranges(doc: &DocumentViewNode) -> Vec<(usize, usize)> {
    let mut excluded = Vec::new();
    let Some(source_lines) = &doc.copy_excluded_source_lines else {
        return excluded;
    };

    for (index, visual) in doc.visual_cache.lines.iter().enumerate() {
        if !source_lines.contains(&visual.source_line) {
            continue;
        }
        if index + 1 >= doc.visual_cache.lines.len() {
            continue;
        }
        let Some(start) = doc.visual_cache.line_starts.get(index).copied() else {
            continue;
        };
        let Some(len) = doc.visual_cache.line_lengths.get(index).copied() else {
            continue;
        };
        excluded.push((start.saturating_add(len), start.saturating_add(len + 1)));
    }

    excluded
}

fn document_view_selected_slice(
    tree: &NodeTree,
    id: NodeId,
    local_start: usize,
    local_end: usize,
    apply_exclusions: bool,
) -> Option<String> {
    if !tree.is_valid(id) || local_start >= local_end {
        return Some(String::new());
    }

    let node = tree.node(id);
    let NodeKind::DocumentView(doc) = &node.kind else {
        return None;
    };

    let start = local_start.min(doc.visual_cache.flat_text.len());
    let end = local_end.min(doc.visual_cache.flat_text.len());
    if start >= end {
        return Some(String::new());
    }

    document_view_selected_text_from_node(doc, start, end, apply_exclusions)
}

pub(crate) fn document_view_selected_text_from_node(
    doc: &DocumentViewNode,
    start: usize,
    end: usize,
    apply_exclusions: bool,
) -> Option<String> {
    if start >= end {
        return Some(String::new());
    }

    let excluded = if apply_exclusions {
        document_view_excluded_newline_ranges(doc)
    } else {
        Vec::new()
    };

    let mut out = String::new();
    let mut emitted_diagram_ranges = Vec::<(usize, usize)>::new();

    for (index, line) in doc.visual_cache.lines.iter().enumerate() {
        let line_start = *doc.visual_cache.line_starts.get(index)?;
        let line_len = *doc.visual_cache.line_lengths.get(index)?;
        let line_end = line_start.saturating_add(line_len);
        let diagram_range = matches!(line.kind, VisualLineKind::DiagramRow { .. })
            .then_some((line_start, line_end));

        let overlap_start = start.max(line_start);
        let overlap_end = end.min(line_end);
        let duplicate_diagram_row =
            diagram_range.is_some_and(|range| emitted_diagram_ranges.contains(&range));
        if overlap_start < overlap_end && !duplicate_diagram_row {
            let text = doc.visual_cache.line_texts.get(index)?.as_ref();
            let local_start = overlap_start.saturating_sub(line_start);
            let local_end = overlap_end.saturating_sub(line_start);
            out.push_str(text.get(local_start..local_end)?);
            if let Some(range) = diagram_range {
                emitted_diagram_ranges.push(range);
            }
        }

        if index + 1 >= doc.visual_cache.lines.len() {
            continue;
        }

        let newline_start = line_end;
        let newline_end = newline_start.saturating_add(1);
        if start >= newline_end || end <= newline_start {
            continue;
        }

        if apply_exclusions
            && excluded.iter().any(|(excluded_start, excluded_end)| {
                newline_start >= *excluded_start && newline_start < *excluded_end
            })
        {
            continue;
        }

        let next = &doc.visual_cache.lines[index + 1];
        if !is_soft_document_view_line_break(line, next) {
            out.push('\n');
        }
    }

    Some(out)
}

fn is_soft_document_view_line_break(
    current: &crate::widgets::document_view::node::DocumentVisualLine,
    next: &crate::widgets::document_view::node::DocumentVisualLine,
) -> bool {
    if current.source_line != next.source_line {
        return false;
    }

    match (&current.kind, &next.kind) {
        (VisualLineKind::Text { .. }, VisualLineKind::Text { continuation, .. }) => *continuation,
        (VisualLineKind::BlockQuoteLine { .. }, VisualLineKind::BlockQuoteLine { .. }) => true,
        (VisualLineKind::CodeLine { .. }, VisualLineKind::CodeLine { .. }) => true,
        (VisualLineKind::DiagramRow { .. }, VisualLineKind::DiagramRow { .. }) => true,
        _ => false,
    }
}

fn selected_text_slice_for_item(
    tree: &NodeTree,
    item: &DocumentViewSharedLinearItem,
    local_start: usize,
    local_end: usize,
    apply_exclusions: bool,
) -> Option<String> {
    if local_start >= local_end {
        return Some(String::new());
    }
    if let Some(nid) = item.node_id {
        document_view_selected_slice(tree, nid, local_start, local_end, apply_exclusions)
    } else {
        let text = item.phantom_flat_text.as_ref()?;
        let start = local_start.min(text.len());
        let end = local_end.min(text.len());
        if start >= end {
            return Some(String::new());
        }
        Some(text.get(start..end)?.to_string())
    }
}

fn selected_text_for_global_range(
    tree: &NodeTree,
    items: &[DocumentViewSharedLinearItem],
    global_start: usize,
    global_end: usize,
    apply_exclusions: bool,
) -> Arc<str> {
    if items.is_empty() || global_start >= global_end {
        return Arc::from("");
    }

    let mut parts: Vec<String> = Vec::new();
    for item in items {
        let overlap_start = global_start.max(item.global_start);
        let overlap_end = global_end.min(item.global_end);
        if overlap_start >= overlap_end {
            continue;
        }
        let local_start = overlap_start.saturating_sub(item.global_start);
        let local_end = overlap_end.saturating_sub(item.global_start);
        let Some(slice) =
            selected_text_slice_for_item(tree, item, local_start, local_end, apply_exclusions)
        else {
            continue;
        };
        if !slice.is_empty() {
            parts.push(slice);
        }
    }

    Arc::from(parts.join("\n\n"))
}

fn selection_updates_for_global_range(
    tree: &NodeTree,
    items: &[DocumentViewSharedLinearItem],
    global_start: usize,
    global_end: usize,
) -> (
    Vec<DocumentViewSelectionUpdate>,
    Vec<OffscreenSharedSelectionPatch>,
) {
    let mut updates = Vec::with_capacity(items.len());
    let mut patches = Vec::new();
    for item in items {
        let overlap_start = global_start.max(item.global_start);
        let overlap_end = global_end.min(item.global_end);
        if overlap_start < overlap_end {
            let local_start = overlap_start.saturating_sub(item.global_start);
            let local_end = overlap_end.saturating_sub(item.global_start);
            let flat_len = item.text_len;
            let cursor = local_end.min(flat_len);
            let anchor = Some(local_start.min(flat_len));
            if let Some(nid) = item.node_id {
                if tree.is_valid(nid) {
                    updates.push(DocumentViewSelectionUpdate {
                        id: nid,
                        cursor,
                        anchor,
                    });
                }
            } else {
                patches.push(OffscreenSharedSelectionPatch {
                    child_key: item.child_key.clone(),
                    doc_slot: item.doc_slot,
                    selection_cursor: cursor,
                    selection_anchor: anchor,
                });
            }
        } else if let Some(nid) = item.node_id {
            if tree.is_valid(nid) {
                let current_cursor = match &tree.node(nid).kind {
                    NodeKind::DocumentView(doc) => doc.selection_cursor,
                    _ => 0,
                };
                updates.push(DocumentViewSelectionUpdate {
                    id: nid,
                    cursor: current_cursor,
                    anchor: None,
                });
            }
        } else {
            patches.push(OffscreenSharedSelectionPatch {
                child_key: item.child_key.clone(),
                doc_slot: item.doc_slot,
                selection_cursor: 0,
                selection_anchor: None,
            });
        }
    }
    (updates, patches)
}

pub(crate) fn apply_offscreen_shared_selection_patches(
    tree: &mut NodeTree,
    scroll_view_id: NodeId,
    patches: &[OffscreenSharedSelectionPatch],
) {
    if patches.is_empty() || !tree.is_valid(scroll_view_id) {
        return;
    }
    let NodeKind::ScrollView(sv) = &mut tree.node_mut(scroll_view_id).kind else {
        return;
    };
    for p in patches {
        let Some(off) = sv.offscreen_doc_selections.get_mut(&p.child_key) else {
            continue;
        };
        let Some(sel) = off.docs.get_mut(p.doc_slot) else {
            continue;
        };
        let flat_len = sel.flat_text.len();
        let cursor = p.selection_cursor.min(flat_len);
        let anchor = p.selection_anchor.map(|a| a.min(flat_len));
        sel.selection_cursor = cursor;
        sel.selection_anchor = anchor;
        sel.table_rect_selection = None;
    }
}

/// Compute a shared linear selection across all `DocumentView`s under the same
/// nearest ancestor `ScrollView`.
pub(crate) fn handle_document_view_shared_linear_drag(
    tree: &NodeTree,
    x: u16,
    y: u16,
    id: NodeId,
    anchor_local: usize,
    stable_anchor: Option<&SharedDocumentDragAnchor>,
) -> Option<DocumentViewSharedLinearDragResult> {
    let shared_selection_id = {
        let node = tree.node(id);
        let NodeKind::DocumentView(doc) = &node.kind else {
            return None;
        };
        doc.shared_selection_id.clone()?
    };

    let scroll_view_id = nearest_ancestor_scroll_view(tree, id)?;
    let items =
        shared_linear_items_for_scroll_view_full(tree, scroll_view_id, &shared_selection_id);
    if items.len() < 2 {
        return None;
    }

    let anchor_global = stable_anchor
        .and_then(|sa| anchor_global_for_shared_drag(&items, sa))
        .or_else(|| global_cursor_from_local(&items, id, anchor_local))?;
    shared_linear_drag_with_globals(tree, x, y, &items, anchor_global)
}

/// Shared linear drag when the anchor `DocumentView` node was swept off-screen.
pub(crate) fn handle_document_view_shared_linear_drag_offscreen(
    tree: &NodeTree,
    x: u16,
    y: u16,
    shared_selection_id: &str,
    scroll_view_id: NodeId,
    stable_anchor: &SharedDocumentDragAnchor,
) -> Option<DocumentViewSharedLinearDragResult> {
    if !tree.is_valid(scroll_view_id) {
        return None;
    }
    let items = shared_linear_items_for_scroll_view_full(tree, scroll_view_id, shared_selection_id);
    if items.len() < 2 {
        return None;
    }
    let anchor_global = anchor_global_for_shared_drag(&items, stable_anchor)?;
    shared_linear_drag_with_globals(tree, x, y, &items, anchor_global)
}

fn shared_linear_drag_with_globals(
    tree: &NodeTree,
    x: u16,
    y: u16,
    items: &[DocumentViewSharedLinearItem],
    anchor_global: usize,
) -> Option<DocumentViewSharedLinearDragResult> {
    let cursor_global = global_cursor_from_pointer(tree, items, x, y)?;

    let global_start = anchor_global.min(cursor_global);
    let global_end = anchor_global.max(cursor_global);

    let (updates, offscreen_patches) =
        selection_updates_for_global_range(tree, items, global_start, global_end);
    let selected_text =
        selected_text_for_global_range(tree, items, global_start, global_end, false);

    Some(DocumentViewSharedLinearDragResult {
        updates,
        offscreen_patches,
        selected_text,
    })
}

fn selection_range_for_shared_linear_item(
    tree: &NodeTree,
    scroll_view_id: NodeId,
    item: &DocumentViewSharedLinearItem,
) -> Option<(usize, usize)> {
    if let Some(nid) = item.node_id {
        if !tree.is_valid(nid) {
            return None;
        }
        let node = tree.node(nid);
        let NodeKind::DocumentView(doc) = &node.kind else {
            return None;
        };
        if doc.table_rect_selection.is_some() {
            return None;
        }
        let len = doc.visual_cache.flat_text.len();
        selection_range(doc.selection_cursor, doc.selection_anchor, len)
    } else {
        let NodeKind::ScrollView(sv) = &tree.node(scroll_view_id).kind else {
            return None;
        };
        let sel = sv
            .offscreen_doc_selections
            .get(&item.child_key)
            .and_then(|o| o.docs.get(item.doc_slot))?;
        if sel.table_rect_selection.is_some() {
            return None;
        }
        let len = sel.flat_text.len();
        selection_range(sel.selection_cursor, sel.selection_anchor, len)
    }
}

/// Build shared selection text for a `DocumentView` when multiple sibling
/// `DocumentView`s in the same nearest ancestor `ScrollView` are selected.
pub(crate) fn document_view_shared_selection_text(
    tree: &NodeTree,
    doc_id: NodeId,
    apply_exclusions: bool,
) -> Option<DocumentViewSharedSelectionText> {
    let shared_selection_id = {
        let node = tree.node(doc_id);
        let NodeKind::DocumentView(doc) = &node.kind else {
            return None;
        };
        doc.shared_selection_id.clone()?
    };

    let scroll_view_id = nearest_ancestor_scroll_view(tree, doc_id)?;
    document_view_shared_selection_text_for_scroll_view(
        tree,
        scroll_view_id,
        shared_selection_id,
        apply_exclusions,
    )
}

fn document_view_shared_selection_text_for_scroll_view(
    tree: &NodeTree,
    scroll_view_id: NodeId,
    shared_selection_id: Arc<str>,
    apply_exclusions: bool,
) -> Option<DocumentViewSharedSelectionText> {
    let items = shared_linear_items_for_scroll_view_full(
        tree,
        scroll_view_id,
        shared_selection_id.as_ref(),
    );
    if items.len() < 2 {
        return None;
    }

    let mut selected_ranges: Vec<Option<(usize, usize)>> = Vec::with_capacity(items.len());
    for item in &items {
        let Some((local_start, local_end)) =
            selection_range_for_shared_linear_item(tree, scroll_view_id, item)
        else {
            selected_ranges.push(None);
            continue;
        };
        selected_ranges.push(Some((
            item.global_start.saturating_add(local_start),
            item.global_start.saturating_add(local_end),
        )));
    }

    let selected_count = selected_ranges
        .iter()
        .filter(|range| range.is_some())
        .count();
    if selected_count < 2 {
        return None;
    }

    let first_selected = selected_ranges.iter().position(|range| range.is_some())?;
    let last_selected = selected_ranges.iter().rposition(|range| range.is_some())?;
    if selected_ranges[first_selected..=last_selected]
        .iter()
        .any(|range| range.is_none())
    {
        return None;
    }

    let global_start = selected_ranges
        .iter()
        .flatten()
        .map(|(start, _)| *start)
        .min()?;
    let global_end = selected_ranges
        .iter()
        .flatten()
        .map(|(_, end)| *end)
        .max()?;
    if global_start >= global_end {
        return None;
    }

    let selected_text = selected_text_for_global_range(
        tree,
        items.as_slice(),
        global_start,
        global_end,
        apply_exclusions,
    );

    Some(DocumentViewSharedSelectionText {
        scroll_view_id,
        shared_selection_id,
        selected_text,
    })
}

fn offscreen_single_selection_text(sel: &SingleDocSelection) -> Option<Arc<str>> {
    if let Some(table_sel) = &sel.table_rect_selection {
        return Some(table_sel.tsv_text.clone());
    }

    let (start, end) = selection_range(
        sel.selection_cursor,
        sel.selection_anchor,
        sel.flat_text.len(),
    )?;
    sel.flat_text.get(start..end).map(Arc::<str>::from)
}

/// Build copy text from `DocumentView` selections saved on off-screen
/// `ScrollView` children. This covers virtualized rows that are not present in
/// the live node tree while preserving shared-selection group order where
/// possible.
pub(crate) fn scroll_view_offscreen_document_selection_text(
    tree: &NodeTree,
    scroll_view_id: NodeId,
    apply_exclusions: bool,
) -> Option<Arc<str>> {
    if !tree.is_valid(scroll_view_id) {
        return None;
    }
    let NodeKind::ScrollView(sv) = &tree.node(scroll_view_id).kind else {
        return None;
    };
    if sv.offscreen_doc_selections.is_empty() {
        return None;
    }

    let mut shared_ids: Vec<Arc<str>> = Vec::new();
    for entry in &sv.virtual_cache.entries {
        let Some(child_key) = entry.as_ref().and_then(|entry| entry.key.as_ref()) else {
            continue;
        };
        let Some(off) = sv.offscreen_doc_selections.get(child_key) else {
            continue;
        };
        for sel in &off.docs {
            let has_selection = sel.table_rect_selection.is_some()
                || sel
                    .selection_anchor
                    .is_some_and(|anchor| anchor != sel.selection_cursor);
            if !has_selection {
                continue;
            }
            if let Some(shared_id) = &sel.shared_selection_id
                && !shared_ids
                    .iter()
                    .any(|existing| existing.as_ref() == shared_id.as_ref())
            {
                shared_ids.push(shared_id.clone());
            }
        }
    }

    for shared_id in shared_ids {
        if let Some(shared) = document_view_shared_selection_text_for_scroll_view(
            tree,
            scroll_view_id,
            shared_id,
            apply_exclusions,
        ) && !shared.selected_text.is_empty()
        {
            return Some(shared.selected_text);
        }
    }

    for entry in &sv.virtual_cache.entries {
        let Some(child_key) = entry.as_ref().and_then(|entry| entry.key.as_ref()) else {
            continue;
        };
        let Some(off) = sv.offscreen_doc_selections.get(child_key) else {
            continue;
        };
        for sel in &off.docs {
            if let Some(text) = offscreen_single_selection_text(sel)
                && !text.is_empty()
            {
                return Some(text);
            }
        }
    }

    None
}
