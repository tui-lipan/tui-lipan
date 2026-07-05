//! Drag handling for sliders, progress bars, text areas, and inputs.

use crate::core::element::Key;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::Rect;
use crate::widgets::OffscreenDocSelection;

use super::*;
pub(crate) fn nearest_ancestor_scroll_view(tree: &NodeTree, start: NodeId) -> Option<NodeId> {
    let mut cur = Some(start);
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if matches!(&node.kind, NodeKind::ScrollView(_)) {
            return Some(id);
        }
        cur = node.parent;
    }
    None
}

#[cfg(feature = "diff-view")]
fn split_diff_shared_base(id: &str) -> (&str, bool) {
    if let Some(base) = id.strip_suffix(":left") {
        return (base, true);
    }
    if let Some(base) = id.strip_suffix(":right") {
        return (base, true);
    }
    (id, false)
}

pub(crate) fn shared_selection_id_matches(candidate: Option<&str>, target: &str) -> bool {
    let Some(candidate) = candidate else {
        return false;
    };
    if candidate == target {
        return true;
    }

    #[cfg(feature = "diff-view")]
    {
        let (candidate_base, candidate_is_split) = split_diff_shared_base(candidate);
        let (target_base, target_is_split) = split_diff_shared_base(target);
        candidate_base == target_base && (!candidate_is_split || !target_is_split)
    }

    #[cfg(not(feature = "diff-view"))]
    {
        false
    }
}

pub(crate) fn nearest_ancestor_document_view(tree: &NodeTree, start: NodeId) -> Option<NodeId> {
    let mut cur = Some(start);
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if matches!(&node.kind, NodeKind::DocumentView(_)) {
            return Some(id);
        }
        cur = node.parent;
    }
    None
}

fn collect_document_views_in_subtree(tree: &NodeTree, root: NodeId, out: &mut Vec<NodeId>) {
    if !tree.is_valid(root) {
        return;
    }

    let node = tree.node(root);
    if matches!(&node.kind, NodeKind::DocumentView(_)) {
        out.push(root);
    }

    for &child in &node.children {
        collect_document_views_in_subtree(tree, child, out);
    }
}

fn scroll_view_child_with_key(
    tree: &NodeTree,
    scroll_view_id: NodeId,
    key: &Key,
) -> Option<NodeId> {
    tree.node(scroll_view_id)
        .children
        .iter()
        .copied()
        .find(|&cid| tree.is_valid(cid) && tree.node(cid).key.as_ref() == Some(key))
}

struct AppendVisibleSharedDocsParams<'a> {
    tree: &'a NodeTree,
    subtree_root: NodeId,
    shared_selection_id: &'a str,
    virtual_child_index: usize,
    child_key: Key,
}

fn append_visible_shared_docs_for_child(
    params: AppendVisibleSharedDocsParams<'_>,
    next_doc_slot: &mut usize,
    global: &mut usize,
    first_item_in_scroll: &mut bool,
    out: &mut Vec<DocumentViewSharedLinearItem>,
) {
    let AppendVisibleSharedDocsParams {
        tree,
        subtree_root,
        shared_selection_id,
        virtual_child_index,
        child_key,
    } = params;
    if !tree.is_valid(subtree_root) {
        return;
    }
    let node = tree.node(subtree_root);
    if let NodeKind::DocumentView(doc) = &node.kind {
        let slot = *next_doc_slot;
        *next_doc_slot += 1;
        if shared_selection_id_matches(doc.shared_selection_id.as_deref(), shared_selection_id) {
            let len = doc.visual_cache.flat_text.len();
            if len > 0 {
                if *first_item_in_scroll {
                    *first_item_in_scroll = false;
                } else {
                    *global = global.saturating_add(1);
                }
                let start = *global;
                let end = start.saturating_add(len);
                out.push(DocumentViewSharedLinearItem {
                    node_id: Some(subtree_root),
                    virtual_child_index,
                    child_key: child_key.clone(),
                    doc_slot: slot,
                    global_start: start,
                    global_end: end,
                    text_len: len,
                    rect: node.rect,
                    phantom_flat_text: None,
                });
                *global = end;
            }
        }
        return;
    }
    let children = node.children.clone();
    for child_id in &children {
        append_visible_shared_docs_for_child(
            AppendVisibleSharedDocsParams {
                tree,
                subtree_root: *child_id,
                shared_selection_id,
                virtual_child_index,
                child_key: child_key.clone(),
            },
            next_doc_slot,
            global,
            first_item_in_scroll,
            out,
        );
    }
}

fn append_offscreen_shared_docs_for_child(
    off: &OffscreenDocSelection,
    shared_selection_id: &str,
    virtual_child_index: usize,
    child_key: Key,
    global: &mut usize,
    first_item_in_scroll: &mut bool,
    out: &mut Vec<DocumentViewSharedLinearItem>,
) {
    for (slot, sel) in off.docs.iter().enumerate() {
        if shared_selection_id_matches(sel.shared_selection_id.as_deref(), shared_selection_id) {
            let len = sel.flat_text.len();
            if len > 0 {
                if *first_item_in_scroll {
                    *first_item_in_scroll = false;
                } else {
                    *global = global.saturating_add(1);
                }
                let start = *global;
                let end = start.saturating_add(len);
                out.push(DocumentViewSharedLinearItem {
                    node_id: None,
                    virtual_child_index,
                    child_key: child_key.clone(),
                    doc_slot: slot,
                    global_start: start,
                    global_end: end,
                    text_len: len,
                    rect: Rect {
                        x: 0,
                        y: 0,
                        w: 0,
                        h: 0,
                    },
                    phantom_flat_text: Some(sel.flat_text.clone()),
                });
                *global = end;
            }
        }
    }
}

pub(crate) fn scroll_view_has_multiple_shared_linear_docs(
    tree: &NodeTree,
    scroll_view_id: NodeId,
    shared_selection_id: &str,
) -> bool {
    shared_linear_items_for_scroll_view_full(tree, scroll_view_id, shared_selection_id).len() >= 2
}

/// Ordered shared-linear items including virtualized rows (off-screen snapshots).
pub(crate) fn shared_linear_items_for_scroll_view_full(
    tree: &NodeTree,
    scroll_view_id: NodeId,
    shared_selection_id: &str,
) -> Vec<DocumentViewSharedLinearItem> {
    if !tree.is_valid(scroll_view_id) {
        return Vec::new();
    }
    let NodeKind::ScrollView(sv) = &tree.node(scroll_view_id).kind else {
        return Vec::new();
    };

    if sv.virtual_cache.entries.is_empty() {
        return shared_linear_items_tree_only(tree, scroll_view_id, shared_selection_id);
    }

    let mut out = Vec::new();
    let mut global = 0usize;
    let mut first_item_in_scroll = true;

    for i in 0..sv.virtual_cache.entries.len() {
        let Some(child_key) = sv.virtual_cache.entries[i]
            .as_ref()
            .and_then(|e| e.key.clone())
        else {
            continue;
        };

        if let Some(cid) = scroll_view_child_with_key(tree, scroll_view_id, &child_key) {
            let mut slot = 0usize;
            append_visible_shared_docs_for_child(
                AppendVisibleSharedDocsParams {
                    tree,
                    subtree_root: cid,
                    shared_selection_id,
                    virtual_child_index: i,
                    child_key,
                },
                &mut slot,
                &mut global,
                &mut first_item_in_scroll,
                &mut out,
            );
        } else if let Some(off) = sv.offscreen_doc_selections.get(&child_key) {
            append_offscreen_shared_docs_for_child(
                off,
                shared_selection_id,
                i,
                child_key,
                &mut global,
                &mut first_item_in_scroll,
                &mut out,
            );
        }
    }

    if out.is_empty() {
        shared_linear_items_tree_only(tree, scroll_view_id, shared_selection_id)
    } else {
        out
    }
}

fn shared_linear_items_tree_only(
    tree: &NodeTree,
    scroll_view_id: NodeId,
    shared_selection_id: &str,
) -> Vec<DocumentViewSharedLinearItem> {
    let mut doc_ids = Vec::new();
    collect_document_views_in_subtree(tree, scroll_view_id, &mut doc_ids);

    let valid_doc_ids: Vec<NodeId> = doc_ids
        .into_iter()
        .filter(|id| {
            if !tree.is_valid(*id) {
                return false;
            }
            match &tree.node(*id).kind {
                NodeKind::DocumentView(doc) => shared_selection_id_matches(
                    doc.shared_selection_id.as_deref(),
                    shared_selection_id,
                ),
                _ => false,
            }
        })
        .collect();

    let mut items = Vec::with_capacity(valid_doc_ids.len());
    let mut cursor = 0usize;
    for id in valid_doc_ids {
        let node = tree.node(id);
        let NodeKind::DocumentView(doc) = &node.kind else {
            continue;
        };

        let len = doc.visual_cache.flat_text.len();
        if len == 0 {
            continue;
        }

        if !items.is_empty() {
            cursor = cursor.saturating_add(1);
        }

        let Some((child_root, child_key)) =
            scroll_direct_child_of_scroll_view(tree, scroll_view_id, id)
        else {
            continue;
        };
        let Some(virtual_child_index) = tree
            .node(scroll_view_id)
            .children
            .iter()
            .position(|&c| c == child_root)
        else {
            continue;
        };
        let mut slot_c = 0usize;
        let Some(doc_slot) = document_view_slot_dfs(tree, child_root, id, &mut slot_c) else {
            continue;
        };

        let start = cursor;
        let end = start.saturating_add(len);
        items.push(DocumentViewSharedLinearItem {
            node_id: Some(id),
            virtual_child_index,
            child_key,
            doc_slot,
            global_start: start,
            global_end: end,
            text_len: len,
            rect: node.rect,
            phantom_flat_text: None,
        });
        cursor = end;
    }

    items
}

fn scroll_direct_child_of_scroll_view(
    tree: &NodeTree,
    scroll_view_id: NodeId,
    doc_id: NodeId,
) -> Option<(NodeId, Key)> {
    let mut cur = doc_id;
    loop {
        let parent = tree.node(cur).parent?;
        if parent == scroll_view_id {
            let key = tree.node(cur).key.clone().unwrap_or_else(|| Key::from(""));
            return Some((cur, key));
        }
        cur = parent;
    }
}

fn is_descendant_of_subtree_root(tree: &NodeTree, mut node: NodeId, subtree_root: NodeId) -> bool {
    loop {
        if node == subtree_root {
            return true;
        }
        let Some(p) = tree.node(node).parent else {
            return false;
        };
        node = p;
    }
}

fn document_view_slot_dfs(
    tree: &NodeTree,
    node_id: NodeId,
    target: NodeId,
    slot: &mut usize,
) -> Option<usize> {
    if !tree.is_valid(node_id) {
        return None;
    }
    if let NodeKind::DocumentView(_) = &tree.node(node_id).kind {
        let s = *slot;
        *slot += 1;
        return if node_id == target { Some(s) } else { None };
    }
    for &cid in &tree.node(node_id).children.clone() {
        if let Some(found) = document_view_slot_dfs(tree, cid, target, slot) {
            return Some(found);
        }
    }
    None
}

/// Stable anchor for shared linear drag (virtual-scroll safe).
pub(crate) fn shared_document_drag_anchor_for_hit(
    tree: &NodeTree,
    scroll_view_id: NodeId,
    hit_doc_id: NodeId,
    local_byte: usize,
) -> Option<SharedDocumentDragAnchor> {
    let NodeKind::ScrollView(sv) = &tree.node(scroll_view_id).kind else {
        return None;
    };

    if !sv.virtual_cache.entries.is_empty() {
        for i in 0..sv.virtual_cache.entries.len() {
            let Some(child_key) = sv.virtual_cache.entries[i]
                .as_ref()
                .and_then(|e| e.key.clone())
            else {
                continue;
            };
            let Some(cid) = scroll_view_child_with_key(tree, scroll_view_id, &child_key) else {
                continue;
            };
            if !is_descendant_of_subtree_root(tree, hit_doc_id, cid) {
                continue;
            }
            let mut slot = 0usize;
            if let Some(ds) = document_view_slot_dfs(tree, cid, hit_doc_id, &mut slot) {
                return Some(SharedDocumentDragAnchor {
                    virtual_child_index: i,
                    doc_slot: ds,
                    local_byte,
                });
            }
        }
        return None;
    }

    for (i, &cid) in tree.node(scroll_view_id).children.iter().enumerate() {
        if !tree.is_valid(cid) {
            continue;
        }
        if !is_descendant_of_subtree_root(tree, hit_doc_id, cid) {
            continue;
        }
        let mut slot = 0usize;
        if let Some(ds) = document_view_slot_dfs(tree, cid, hit_doc_id, &mut slot) {
            return Some(SharedDocumentDragAnchor {
                virtual_child_index: i,
                doc_slot: ds,
                local_byte,
            });
        }
    }
    None
}

pub(crate) fn anchor_global_for_shared_drag(
    items: &[DocumentViewSharedLinearItem],
    anchor: &SharedDocumentDragAnchor,
) -> Option<usize> {
    let item = items.iter().find(|it| {
        it.virtual_child_index == anchor.virtual_child_index && it.doc_slot == anchor.doc_slot
    })?;
    let local = anchor.local_byte.min(item.text_len);
    Some(item.global_start.saturating_add(local))
}

fn item_for_live_doc(
    items: &[DocumentViewSharedLinearItem],
    id: NodeId,
) -> Option<&DocumentViewSharedLinearItem> {
    items.iter().find(|item| item.node_id == Some(id))
}

pub(crate) fn global_cursor_from_local(
    items: &[DocumentViewSharedLinearItem],
    id: NodeId,
    local: usize,
) -> Option<usize> {
    let item = item_for_live_doc(items, id)?;
    Some(item.global_start.saturating_add(local.min(item.text_len)))
}

pub(crate) fn global_cursor_from_pointer(
    tree: &NodeTree,
    items: &[DocumentViewSharedLinearItem],
    x: u16,
    y: u16,
) -> Option<usize> {
    if items.is_empty() {
        return None;
    }

    let pointer_doc = tree
        .hit_test(x as i16, y as i16)
        .and_then(|hit| nearest_ancestor_document_view(tree, hit))
        .and_then(|id| item_for_live_doc(items, id));

    if let Some(item) = pointer_doc {
        let nid = item.node_id?;
        let local = document_view_cursor_from_coords(tree, x, y, nid)?;
        return Some(item.global_start.saturating_add(local.min(item.text_len)));
    }

    let y_i16 = y as i16;
    if let Some(item) = items.iter().find(|item| {
        item.node_id.is_some() && {
            let bottom = item
                .rect
                .y
                .saturating_add(item.rect.h as i16)
                .saturating_sub(1);
            y_i16 >= item.rect.y && y_i16 <= bottom
        }
    }) {
        let nid = item.node_id?;
        let local = document_view_cursor_from_coords(tree, x, y, nid)?;
        return Some(item.global_start.saturating_add(local.min(item.text_len)));
    }

    let first_vis = items.iter().find(|it| it.node_id.is_some())?;
    let last_vis = items.iter().rfind(|it| it.node_id.is_some())?;

    let last_bottom = last_vis
        .rect
        .y
        .saturating_add(last_vis.rect.h as i16)
        .saturating_sub(1);

    if y_i16 < first_vis.rect.y {
        return Some(items.first()?.global_start);
    }
    if y_i16 > last_bottom {
        return Some(items.last()?.global_end);
    }

    let visible: Vec<&DocumentViewSharedLinearItem> =
        items.iter().filter(|it| it.node_id.is_some()).collect();
    for window in visible.windows(2) {
        let prev = window[0];
        let next = window[1];
        let prev_bottom = prev
            .rect
            .y
            .saturating_add(prev.rect.h as i16)
            .saturating_sub(1);
        let next_top = next.rect.y;
        if y_i16 > prev_bottom && y_i16 < next_top {
            let dist_prev = (y_i16 - prev_bottom) as usize;
            let dist_next = (next_top - y_i16) as usize;
            if dist_prev <= dist_next {
                return Some(prev.global_end);
            }
            return Some(next.global_start);
        }
    }

    Some(last_vis.global_end)
}
