use crate::callback::ScopeId;
use crate::core::element::Key;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::tag::{Tag, tag_of_node};

pub(crate) fn scope_for_node(tree: &NodeTree, id: NodeId) -> Option<ScopeId> {
    let mut current = Some(id);
    while let Some(id) = current {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if let NodeKind::Group(group) = &node.kind {
            return Some(group.scope);
        }
        current = node.parent;
    }
    None
}

pub(crate) fn restore_focus(
    tree: &NodeTree,
    focused: &mut Option<NodeId>,
    focused_key: &mut Option<Key>,
    focused_tag: &mut Option<Tag>,
) {
    // First: keep current focus if still valid.
    if let Some(id) = *focused
        && tree.is_valid(id)
        && tree.node(id).is_focusable()
    {
        *focused_key = tree.node(id).key.clone();
        *focused_tag = Some(tag_of_node(tree.node(id)));
        return;
    }

    // Second: try to restore by key (works across reorders).
    //
    // Search overlay subtrees too: a `request_focus(key)` issued from a
    // component mounted inside a modal/portal would otherwise silently miss
    // (since `iter()` only walks nodes reachable from `self.root`), letting
    // the fallback pick the first focusable in the main tree — which
    // `ensure_overlay_focus` then overrides to the *first* focusable in the
    // overlay, defeating the request.
    if let Some(key) = focused_key {
        // Find the node with the matching key.
        if let Some(id) = tree
            .iter_with_overlays()
            .find(|n| n.key.as_ref() == Some(key))
            .map(|n| n.id)
        {
            // If the node itself is focusable, use it.
            if tree.node(id).is_focusable() {
                *focused = Some(id);
                *focused_tag = Some(tag_of_node(tree.node(id)));
                return;
            }

            // Otherwise, look for the first focusable descendant.
            if let Some(focusable_id) = find_first_focusable_descendant(tree, id) {
                *focused = Some(focusable_id);
                *focused_tag = Some(tag_of_node(tree.node(focusable_id)));
                return;
            }
        }
    }

    // Third: try to restore by tag (handles tree structure changes where
    // the focused widget type still exists but got a new NodeId without a key).
    if let Some(tag) = *focused_tag
        && let Some(id) = tree
            .iter_with_overlays()
            .find(|n| n.is_focusable() && tag_of_node(n) == tag)
            .map(|n| n.id)
    {
        *focused = Some(id);
        *focused_key = tree.node(id).key.clone();
        // focused_tag stays the same
        return;
    }

    // Fourth: reset to first focusable.
    *focused = tree.iter().find(|n| n.is_focusable()).map(|n| n.id);
    if let Some(id) = *focused {
        *focused_key = tree.node(id).key.clone();
        *focused_tag = Some(tag_of_node(tree.node(id)));
    } else {
        *focused_key = None;
        *focused_tag = None;
    }
}

pub(crate) fn focus_next(
    tree: &NodeTree,
    focused: &mut Option<NodeId>,
    focused_key: &mut Option<Key>,
    focused_tag: &mut Option<Tag>,
) {
    let focusables = tree.focusables();
    if focusables.is_empty() {
        return;
    }

    // focusables() returns the list pre-sorted by definition order (id index).

    let next = if let Some(curr) = *focused
        && let Some(idx) = focusables.iter().position(|id| *id == curr)
    {
        focusables[(idx + 1) % focusables.len()]
    } else {
        focusables[0]
    };

    *focused = Some(next);
    *focused_key = tree.node(next).key.clone();
    *focused_tag = Some(tag_of_node(tree.node(next)));
}

pub(crate) fn focus_prev(
    tree: &NodeTree,
    focused: &mut Option<NodeId>,
    focused_key: &mut Option<Key>,
    focused_tag: &mut Option<Tag>,
) {
    let focusables = tree.focusables();
    if focusables.is_empty() {
        return;
    }

    // focusables() returns the list pre-sorted by definition order (id index).

    let prev = if let Some(curr) = *focused
        && let Some(idx) = focusables.iter().position(|id| *id == curr)
    {
        focusables[(idx + focusables.len().saturating_sub(1)) % focusables.len()]
    } else {
        focusables[focusables.len().saturating_sub(1)]
    };

    *focused = Some(prev);
    *focused_key = tree.node(prev).key.clone();
    *focused_tag = Some(tag_of_node(tree.node(prev)));
}

pub(crate) fn find_first_focusable_descendant(tree: &NodeTree, root: NodeId) -> Option<NodeId> {
    // Breadth-first search is usually better for focusable descendants in TUIs
    // to pick the "main" content rather than a deeply nested decorator.
    let mut queue = std::collections::VecDeque::new();
    queue.push_back(root);

    while let Some(current_id) = queue.pop_front() {
        let node = tree.node(current_id);
        for &child in &node.children {
            if !tree.is_valid(child) {
                continue;
            }
            let child_node = tree.node(child);
            if child_node.is_focusable() {
                return Some(child);
            }
            queue.push_back(child);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Button;

    /// Helper: allocate a node in `tree`, set its parent/kind, and return its id.
    /// The node is marked with the current epoch so `is_valid` returns true.
    fn alloc_node(tree: &mut NodeTree, parent: Option<NodeId>, focusable: bool) -> NodeId {
        let id = tree.alloc();
        let epoch = tree.node(tree.root).epoch; // grab epoch from root
        let node = tree.node_mut(id);
        node.parent = parent;
        node.epoch = epoch;
        if focusable {
            node.kind = NodeKind::from(Button::new("btn"));
        }
        // default kind is Text which is not focusable
        id
    }

    /// Build a tree with a non-focusable root and N focusable children.
    /// Returns (tree, root_id, child_ids).
    fn build_tree_with_focusable_children(n: usize) -> (NodeTree, NodeId, Vec<NodeId>) {
        let mut tree = NodeTree::new();
        let epoch = tree.begin_epoch();

        let root = tree.alloc();
        tree.root = root;
        {
            let r = tree.node_mut(root);
            r.epoch = epoch;
            // root is a plain Text node - not focusable
        }

        let mut children = Vec::new();
        for _ in 0..n {
            let child = alloc_node(&mut tree, Some(root), true);
            children.push(child);
        }
        // Wire children into root.
        tree.node_mut(root).children = children.clone();

        (tree, root, children)
    }

    // ---------------------------------------------------------------
    // focus_next / focus_prev
    // ---------------------------------------------------------------

    #[test]
    fn focus_next_wraps_around_at_end() {
        let (tree, _root, children) = build_tree_with_focusable_children(3);

        let mut focused = Some(children[2]); // last child
        let mut key = None;
        let mut tag = None;

        focus_next(&tree, &mut focused, &mut key, &mut tag);
        assert_eq!(focused, Some(children[0]), "should wrap to first focusable");
    }

    #[test]
    fn focus_prev_wraps_around_at_start() {
        let (tree, _root, children) = build_tree_with_focusable_children(3);

        let mut focused = Some(children[0]); // first child
        let mut key = None;
        let mut tag = None;

        focus_prev(&tree, &mut focused, &mut key, &mut tag);
        assert_eq!(focused, Some(children[2]), "should wrap to last focusable");
    }

    #[test]
    fn focus_next_selects_first_when_none_focused() {
        let (tree, _root, children) = build_tree_with_focusable_children(3);

        let mut focused: Option<NodeId> = None;
        let mut key = None;
        let mut tag = None;

        focus_next(&tree, &mut focused, &mut key, &mut tag);
        assert_eq!(focused, Some(children[0]));
    }

    #[test]
    fn focus_prev_selects_last_when_none_focused() {
        let (tree, _root, children) = build_tree_with_focusable_children(3);

        let mut focused: Option<NodeId> = None;
        let mut key = None;
        let mut tag = None;

        focus_prev(&tree, &mut focused, &mut key, &mut tag);
        assert_eq!(focused, Some(children[2]));
    }

    #[test]
    fn focus_next_noop_on_empty_tree() {
        let mut tree = NodeTree::new();
        tree.begin_epoch();
        // root is INVALID - no focusable nodes
        let mut focused: Option<NodeId> = None;
        let mut key = None;
        let mut tag = None;

        focus_next(&tree, &mut focused, &mut key, &mut tag);
        assert_eq!(focused, None);
    }

    // ---------------------------------------------------------------
    // find_first_focusable_descendant
    // ---------------------------------------------------------------

    #[test]
    fn find_first_focusable_descendant_breadth_first() {
        // Tree structure:
        //   root (not focusable)
        //     ├─ child_a (not focusable)
        //     │    └─ grandchild (focusable)
        //     └─ child_b (focusable)
        //
        // BFS should find child_b before grandchild because child_b is
        // at depth 1, grandchild at depth 2.
        let mut tree = NodeTree::new();
        let epoch = tree.begin_epoch();

        let root = tree.alloc();
        tree.root = root;
        tree.node_mut(root).epoch = epoch;

        let child_a = alloc_node(&mut tree, Some(root), false);
        let grandchild = alloc_node(&mut tree, Some(child_a), true);
        tree.node_mut(child_a).children = vec![grandchild];

        let child_b = alloc_node(&mut tree, Some(root), true);
        tree.node_mut(root).children = vec![child_a, child_b];

        let result = find_first_focusable_descendant(&tree, root);
        assert_eq!(
            result,
            Some(child_b),
            "BFS should find shallower child_b first"
        );
    }

    #[test]
    fn find_first_focusable_descendant_returns_none_when_no_focusable() {
        let mut tree = NodeTree::new();
        let epoch = tree.begin_epoch();

        let root = tree.alloc();
        tree.root = root;
        tree.node_mut(root).epoch = epoch;

        let child = alloc_node(&mut tree, Some(root), false);
        tree.node_mut(root).children = vec![child];

        assert_eq!(find_first_focusable_descendant(&tree, root), None);
    }

    // ---------------------------------------------------------------
    // restore_focus
    // ---------------------------------------------------------------

    #[test]
    fn restore_focus_keeps_valid_focused_node() {
        let (tree, _root, children) = build_tree_with_focusable_children(2);

        let mut focused = Some(children[1]);
        let mut key = None;
        let mut tag = None;

        restore_focus(&tree, &mut focused, &mut key, &mut tag);
        assert_eq!(
            focused,
            Some(children[1]),
            "valid focusable node should be kept"
        );
        // tag should be set after restore
        assert_eq!(tag, Some(Tag::Button));
    }

    #[test]
    fn restore_focus_falls_back_to_key_match() {
        // Build tree, set a key on children[0], then invalidate the old
        // focused id by pointing to a stale NodeId - restore should find
        // the node by key.
        let (mut tree, _root, children) = build_tree_with_focusable_children(2);

        let the_key: Key = "my-btn".into();
        tree.node_mut(children[0]).key = Some(the_key.clone());

        // Simulate stale focus: use an INVALID id but remember the key.
        let mut focused = Some(NodeId::INVALID);
        let mut key = Some(the_key);
        let mut tag = None;

        restore_focus(&tree, &mut focused, &mut key, &mut tag);
        assert_eq!(focused, Some(children[0]), "should restore by key");
    }

    #[test]
    fn restore_focus_falls_back_to_tag_match() {
        let (tree, _root, children) = build_tree_with_focusable_children(2);

        // Stale id, no key, but tag matches Button.
        let mut focused = Some(NodeId::INVALID);
        let mut key = None;
        let mut tag = Some(Tag::Button);

        restore_focus(&tree, &mut focused, &mut key, &mut tag);
        // Should find the first focusable node with Tag::Button
        assert_eq!(focused, Some(children[0]));
    }

    #[test]
    fn restore_focus_falls_back_to_first_focusable() {
        let (tree, _root, children) = build_tree_with_focusable_children(2);

        // Stale id, no key, no tag - should fall back to first focusable.
        let mut focused = Some(NodeId::INVALID);
        let mut key = None;
        let mut tag = None;

        restore_focus(&tree, &mut focused, &mut key, &mut tag);
        assert_eq!(focused, Some(children[0]));
    }
}
