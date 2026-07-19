use crate::app::context::FocusPolicy;
use crate::callback::ScopeId;
use crate::core::element::Key;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::tag::{Tag, tag_of_node};
use crate::widgets::FocusScope;

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

pub(crate) fn in_excluded_scope(tree: &NodeTree, id: NodeId) -> bool {
    let mut current = Some(id);
    while let Some(id) = current {
        if !tree.is_valid(id) {
            return false;
        }
        let node = tree.node(id);
        if node.focus_scope() == FocusScope::Exclude {
            return true;
        }
        current = node.parent;
    }
    false
}

fn containing_scope(tree: &NodeTree, id: NodeId) -> Option<NodeId> {
    if in_excluded_scope(tree, id) {
        return None;
    }
    if !tree.is_valid(id) {
        return None;
    }
    // Start at the parent: a `Contain` node is the pane's *boundary*, which
    // belongs to the enclosing ring. A focused pane frame therefore Tabs onward
    // through that ring instead of falling into its own trap.
    let mut current = tree.node(id).parent;
    while let Some(id) = current {
        if !tree.is_valid(id) {
            return None;
        }
        let node = tree.node(id);
        if node.focus_scope() == FocusScope::Contain {
            return Some(id);
        }
        current = node.parent;
    }
    None
}

/// The tab ring that applies to `focused`.
///
/// Inside a `Contain` pane this is that pane's own ring. Outside every pane it is
/// the pane-opaque global ring, so Tab cannot tunnel into a pane it could not Tab
/// back out of.
pub(crate) fn traversal_focusables(tree: &NodeTree, focused: Option<NodeId>) -> Vec<NodeId> {
    if let Some(scope) = focused.and_then(|id| containing_scope(tree, id)) {
        return tree.focusables_in_subtree(scope);
    }

    let ring = tree.focusables();
    if !ring.is_empty() {
        return ring;
    }

    // Degenerate case: every tab stop lives inside a `Contain` pane (e.g. the whole
    // app is one pane). Containment has nothing to contain focus *relative to* yet,
    // so descend into panes rather than leaving Tab dead. Once focus lands inside,
    // the branch above takes over and that pane's ring applies.
    tree.focusables_unrestricted()
}

pub(crate) fn restore_focus(
    tree: &NodeTree,
    focused: &mut Option<NodeId>,
    focused_key: &mut Option<Key>,
    focused_tag: &mut Option<Tag>,
    policy: FocusPolicy,
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
            let descendant = if in_excluded_scope(tree, id) {
                find_first_focusable_descendant_unscoped(tree, id)
            } else {
                find_first_focusable_descendant(tree, id)
            };
            if let Some(focusable_id) = descendant {
                *focused = Some(focusable_id);
                *focused_tag = Some(tag_of_node(tree.node(focusable_id)));
                return;
            }
        }
    }

    // Third: try to restore by tag (handles tree structure changes where
    // the focused widget type still exists but got a new NodeId without a key).
    if policy != FocusPolicy::Manual
        && let Some(tag) = *focused_tag
        && let Some(id) = tree
            .iter_with_overlays()
            .find(|n| n.is_focusable() && !in_excluded_scope(tree, n.id) && tag_of_node(n) == tag)
            .map(|n| n.id)
    {
        *focused = Some(id);
        *focused_key = tree.node(id).key.clone();
        // focused_tag stays the same
        return;
    }

    // Fourth: only Auto chooses a fallback. OnDemand and Manual preserve the
    // remembered key so a keyed widget can reclaim focus when it remounts.
    if policy == FocusPolicy::Auto {
        *focused = first_focusable(tree);
        if let Some(id) = *focused {
            *focused_key = tree.node(id).key.clone();
            *focused_tag = Some(tag_of_node(tree.node(id)));
            return;
        }
        *focused_key = None;
    }
    *focused = None;
    *focused_tag = None;
}

/// Which way [`step`] moves through the ring.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum FocusDirection {
    Next,
    Prev,
}

/// The stop one step in `direction` from `focused`, in a ring sorted by node id.
///
/// `None` only when the ring is empty. `focused` need not be a member: its
/// position is found by insertion point rather than by membership. That matters
/// because focus is granted on `is_focusable()` while rings are built from
/// `is_tab_stop()`: a widget focused by click or `request_focus` is routinely
/// *not* in the ring (`.tab_stop(false)`, or an `Exclude`/`Contain` escape
/// hatch). Stepping from where it would sit keeps Tab moving to the true
/// neighbour instead of jumping back to the start of the ring.
pub(crate) fn ring_step(
    focusables: &[NodeId],
    focused: Option<NodeId>,
    direction: FocusDirection,
) -> Option<NodeId> {
    let (&first, &last) = (focusables.first()?, focusables.last()?);
    Some(match (focused, direction) {
        (Some(curr), FocusDirection::Next) => {
            let at = focusables.partition_point(|id| id.index() <= curr.index());
            focusables.get(at).copied().unwrap_or(first)
        }
        (Some(curr), FocusDirection::Prev) => {
            let at = focusables.partition_point(|id| id.index() < curr.index());
            at.checked_sub(1).map_or(last, |i| focusables[i])
        }
        (None, FocusDirection::Next) => first,
        (None, FocusDirection::Prev) => last,
    })
}

/// Move focus one stop through the ring that applies to the current focus.
pub(crate) fn step(
    tree: &NodeTree,
    focused: &mut Option<NodeId>,
    focused_key: &mut Option<Key>,
    focused_tag: &mut Option<Tag>,
    direction: FocusDirection,
) {
    let focusables = traversal_focusables(tree, *focused);
    let Some(target) = ring_step(&focusables, *focused, direction) else {
        return;
    };

    *focused = Some(target);
    *focused_key = tree.node(target).key.clone();
    *focused_tag = Some(tag_of_node(tree.node(target)));
}

pub(crate) fn focus_next(
    tree: &NodeTree,
    focused: &mut Option<NodeId>,
    focused_key: &mut Option<Key>,
    focused_tag: &mut Option<Tag>,
) {
    step(
        tree,
        focused,
        focused_key,
        focused_tag,
        FocusDirection::Next,
    );
}

pub(crate) fn focus_prev(
    tree: &NodeTree,
    focused: &mut Option<NodeId>,
    focused_key: &mut Option<Key>,
    focused_tag: &mut Option<Tag>,
) {
    step(
        tree,
        focused,
        focused_key,
        focused_tag,
        FocusDirection::Prev,
    );
}

/// Framework-initiated Tab traversal, which [`FocusPolicy::Manual`] suppresses.
///
/// Returns whether focus moved, so dispatch sites can report Tab as unhandled and
/// let it fall through to the widget and command layers under `Manual`. The
/// [`focus_next`]/[`focus_prev`] primitives stay policy-free because
/// `Ctx::focus_next`/`focus_prev` are explicit user initiative and must keep
/// working under every policy.
pub(crate) fn step_for_policy(
    tree: &NodeTree,
    focused: &mut Option<NodeId>,
    focused_key: &mut Option<Key>,
    focused_tag: &mut Option<Tag>,
    policy: FocusPolicy,
    direction: FocusDirection,
) -> bool {
    if policy == FocusPolicy::Manual {
        return false;
    }
    step(tree, focused, focused_key, focused_tag, direction);
    true
}

pub(crate) fn find_first_focusable_descendant(tree: &NodeTree, root: NodeId) -> Option<NodeId> {
    find_first_focusable_descendant_impl(tree, root, true)
}

fn find_first_focusable_descendant_unscoped(tree: &NodeTree, root: NodeId) -> Option<NodeId> {
    find_first_focusable_descendant_impl(tree, root, false)
}

fn find_first_focusable_descendant_impl(
    tree: &NodeTree,
    root: NodeId,
    respect_exclude: bool,
) -> Option<NodeId> {
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
            if respect_exclude && child_node.focus_scope() == FocusScope::Exclude {
                continue;
            }
            if child_node.is_focusable() {
                return Some(child);
            }
            queue.push_back(child);
        }
    }
    None
}

/// The default focus target for [`FocusPolicy::Auto`].
///
/// Prefers the first tab stop in node order so that startup focus and the first
/// Tab target agree - the ring is sorted by node id, so a child-order walk here
/// would pick a different node whenever children were reordered relative to
/// allocation. `Contain` panes are transparent to this search: picking a default
/// is not traversal, and an app that is entirely one pane must still get focus.
fn first_focusable(tree: &NodeTree) -> Option<NodeId> {
    if !tree.is_valid(tree.root) {
        return None;
    }
    if let Some(first) = tree.focusables_unrestricted().first().copied() {
        return Some(first);
    }

    // No tab stops anywhere: fall back to any focusable node so widgets that only
    // opted out of sequential traversal stay reachable under `Auto`.
    let mut candidates: Vec<NodeId> = Vec::new();
    let mut stack = vec![tree.root];
    while let Some(id) = stack.pop() {
        let node = tree.node(id);
        if node.focus_scope() == FocusScope::Exclude {
            continue;
        }
        if node.is_focusable() {
            candidates.push(id);
        }
        stack.extend(
            node.children
                .iter()
                .copied()
                .filter(|child| tree.is_valid(*child)),
        );
    }
    candidates.into_iter().min_by_key(|id| id.index())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::Button;
    use crate::widgets::internal::FrameNode;

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

    /// A node that is focusable (clickable, `request_focus`-able) but not a tab stop.
    fn alloc_focusable_not_tab_stop(tree: &mut NodeTree, parent: Option<NodeId>) -> NodeId {
        let id = alloc_node(tree, parent, false);
        tree.node_mut(id).kind = NodeKind::from(Button::new("btn").tab_stop(false));
        id
    }

    fn alloc_scope(tree: &mut NodeTree, parent: Option<NodeId>, scope: FocusScope) -> NodeId {
        let id = alloc_node(tree, parent, false);
        tree.node_mut(id).kind = NodeKind::Frame(FrameNode {
            focus_scope: scope,
            ..FrameNode::default()
        });
        id
    }

    /// A pane frame that is itself focusable: a tab stop at the pane boundary.
    fn alloc_focusable_scope(
        tree: &mut NodeTree,
        parent: Option<NodeId>,
        scope: FocusScope,
    ) -> NodeId {
        let id = alloc_node(tree, parent, false);
        tree.node_mut(id).kind = NodeKind::Frame(FrameNode {
            focus_scope: scope,
            focusable: true,
            ..FrameNode::default()
        });
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

    #[test]
    fn excluded_scope_is_skipped_by_ring_fallback_and_descendant_search() {
        let (mut tree, root, _) = build_tree_with_focusable_children(0);
        let excluded = alloc_scope(&mut tree, Some(root), FocusScope::Exclude);
        let hidden = alloc_node(&mut tree, Some(excluded), true);
        tree.node_mut(hidden).key = Some(Key::from("hidden"));
        tree.node_mut(excluded).children = vec![hidden];
        let visible = alloc_node(&mut tree, Some(root), true);
        tree.node_mut(visible).key = Some(Key::from("visible"));
        tree.node_mut(root).children = vec![excluded, visible];

        assert_eq!(tree.focusables(), vec![visible]);
        assert_eq!(find_first_focusable_descendant(&tree, root), Some(visible));

        let mut focused = None;
        let mut key = None;
        let mut tag = None;
        restore_focus(&tree, &mut focused, &mut key, &mut tag, FocusPolicy::Auto);
        assert_eq!(focused, Some(visible));

        focused = None;
        key = Some(Key::from("hidden"));
        tag = None;
        restore_focus(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusPolicy::OnDemand,
        );
        assert_eq!(focused, Some(hidden), "keyed requests bypass exclusion");

        tree.node_mut(root).key = Some(Key::from("root"));
        focused = None;
        key = Some(Key::from("root"));
        tag = None;
        restore_focus(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusPolicy::OnDemand,
        );
        assert_eq!(
            focused,
            Some(visible),
            "ancestor requests must still respect nested exclusion"
        );
    }

    #[test]
    fn nearest_containing_scope_cycles_and_wraps() {
        let (mut tree, root, _) = build_tree_with_focusable_children(0);
        let outside = alloc_node(&mut tree, Some(root), true);
        let outer = alloc_scope(&mut tree, Some(root), FocusScope::Contain);
        let outer_button = alloc_node(&mut tree, Some(outer), true);
        let inner = alloc_scope(&mut tree, Some(outer), FocusScope::Contain);
        let inner_first = alloc_node(&mut tree, Some(inner), true);
        let inner_second = alloc_node(&mut tree, Some(inner), true);
        tree.node_mut(inner).children = vec![inner_first, inner_second];
        tree.node_mut(outer).children = vec![outer_button, inner];
        tree.node_mut(root).children = vec![outside, outer];

        let mut focused = Some(inner_second);
        let mut key = None;
        let mut tag = None;
        focus_next(&tree, &mut focused, &mut key, &mut tag);
        assert_eq!(focused, Some(inner_first));
        focus_prev(&tree, &mut focused, &mut key, &mut tag);
        assert_eq!(focused, Some(inner_second));
    }

    #[test]
    fn containing_scope_uses_global_node_order_after_child_reorder() {
        let (mut tree, root, _) = build_tree_with_focusable_children(0);
        let scope = alloc_scope(&mut tree, Some(root), FocusScope::Contain);
        let first_allocated = alloc_node(&mut tree, Some(scope), true);
        let second_allocated = alloc_node(&mut tree, Some(scope), true);
        let third_allocated = alloc_node(&mut tree, Some(scope), true);
        tree.node_mut(scope).children = vec![third_allocated, second_allocated, first_allocated];
        tree.node_mut(root).children = vec![scope];

        let mut focused = Some(third_allocated);
        let mut key = None;
        let mut tag = None;
        focus_next(&tree, &mut focused, &mut key, &mut tag);

        assert_eq!(focused, Some(first_allocated));
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

        restore_focus(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusPolicy::OnDemand,
        );
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

        restore_focus(&tree, &mut focused, &mut key, &mut tag, FocusPolicy::Manual);
        assert_eq!(focused, Some(children[0]), "should restore by key");
    }

    #[test]
    fn restore_focus_falls_back_to_tag_match() {
        let (tree, _root, children) = build_tree_with_focusable_children(2);

        // Stale id, no key, but tag matches Button.
        let mut focused = Some(NodeId::INVALID);
        let mut key = None;
        let mut tag = Some(Tag::Button);

        restore_focus(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusPolicy::OnDemand,
        );
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

        restore_focus(&tree, &mut focused, &mut key, &mut tag, FocusPolicy::Auto);
        assert_eq!(focused, Some(children[0]));
    }

    #[test]
    fn restore_focus_on_demand_keeps_key_without_fallback() {
        let (tree, _root, _children) = build_tree_with_focusable_children(2);
        let remembered_key: Key = "missing".into();
        let mut focused = Some(NodeId::INVALID);
        let mut key = Some(remembered_key.clone());
        let mut tag = None;

        restore_focus(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusPolicy::OnDemand,
        );

        assert_eq!(focused, None);
        assert_eq!(key, Some(remembered_key));
        assert_eq!(tag, None);
    }

    #[test]
    fn tab_from_outside_does_not_tunnel_into_a_contain_pane() {
        // Regression: the ring used to include pane contents, so Tab from `outside`
        // landed inside the pane - and the pane's own ring then trapped it there
        // with no way back. Panes are entered by click / request_focus instead.
        let (mut tree, root, _) = build_tree_with_focusable_children(0);
        let outside = alloc_node(&mut tree, Some(root), true);
        let pane = alloc_scope(&mut tree, Some(root), FocusScope::Contain);
        let inside = alloc_node(&mut tree, Some(pane), true);
        tree.node_mut(pane).children = vec![inside];
        tree.node_mut(root).children = vec![outside, pane];

        assert_eq!(
            tree.focusables(),
            vec![outside],
            "pane is opaque to the ring"
        );

        let mut focused = Some(outside);
        let (mut key, mut tag) = (None, None);
        step(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusDirection::Next,
        );
        assert_eq!(focused, Some(outside), "Tab must not enter the pane");

        // Once inside (via click / request_focus) the pane's own ring applies.
        focused = Some(inside);
        step(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusDirection::Next,
        );
        assert_eq!(focused, Some(inside));
    }

    #[test]
    fn nested_contain_pane_is_opaque_to_the_enclosing_pane() {
        let (mut tree, root, _) = build_tree_with_focusable_children(0);
        let outer = alloc_scope(&mut tree, Some(root), FocusScope::Contain);
        let outer_button = alloc_node(&mut tree, Some(outer), true);
        let inner = alloc_scope(&mut tree, Some(outer), FocusScope::Contain);
        let inner_button = alloc_node(&mut tree, Some(inner), true);
        tree.node_mut(inner).children = vec![inner_button];
        tree.node_mut(outer).children = vec![outer_button, inner];
        tree.node_mut(root).children = vec![outer];

        assert_eq!(
            tree.focusables_in_subtree(outer),
            vec![outer_button],
            "the inner pane is not part of the outer pane's ring"
        );

        let mut focused = Some(outer_button);
        let (mut key, mut tag) = (None, None);
        step(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusDirection::Next,
        );
        assert_eq!(
            focused,
            Some(outer_button),
            "Tab must not fall into the inner pane and get stuck"
        );
    }

    #[test]
    fn tab_works_when_every_stop_lives_inside_a_pane() {
        // Degenerate app-is-one-pane shape: containment has nothing to contain
        // focus relative to yet, so Tab from nothing must still get focus in.
        let (mut tree, root, _) = build_tree_with_focusable_children(0);
        let pane = alloc_scope(&mut tree, Some(root), FocusScope::Contain);
        let first = alloc_node(&mut tree, Some(pane), true);
        let second = alloc_node(&mut tree, Some(pane), true);
        tree.node_mut(pane).children = vec![first, second];
        tree.node_mut(root).children = vec![pane];

        assert!(tree.focusables().is_empty());

        let mut focused = None;
        let (mut key, mut tag) = (None, None);
        step(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusDirection::Next,
        );
        assert_eq!(focused, Some(first), "Tab must not be dead");

        step(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusDirection::Next,
        );
        assert_eq!(focused, Some(second), "then the pane's own ring applies");
    }

    #[test]
    fn tab_steps_past_a_focused_node_that_is_not_a_tab_stop() {
        // Regression: focus is granted on `is_focusable()` but the ring is built
        // from `is_tab_stop()`, so a clicked `.tab_stop(false)` widget is not in the
        // ring. Stepping used to reset to `focusables[0]` instead of advancing.
        let (mut tree, root, _) = build_tree_with_focusable_children(0);
        let first = alloc_node(&mut tree, Some(root), true);
        let skipped = alloc_focusable_not_tab_stop(&mut tree, Some(root));
        let last = alloc_node(&mut tree, Some(root), true);
        tree.node_mut(root).children = vec![first, skipped, last];

        assert_eq!(tree.focusables(), vec![first, last]);

        let mut focused = Some(skipped);
        let (mut key, mut tag) = (None, None);
        step(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusDirection::Next,
        );
        assert_eq!(
            focused,
            Some(last),
            "advance past it, not back to the start"
        );

        focused = Some(skipped);
        step(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusDirection::Prev,
        );
        assert_eq!(focused, Some(first), "and back to the true predecessor");
    }

    #[test]
    fn focusable_contain_pane_is_a_stop_in_the_enclosing_ring() {
        // Regression: the opacity check used to drop the pane boundary node
        // itself from the outer ring, so a `.focusable(true)` Contain pane was
        // unreachable by keyboard entirely. The boundary belongs to the outer
        // ring; only the pane's *contents* are opaque.
        let (mut tree, root, _) = build_tree_with_focusable_children(0);
        let outside = alloc_node(&mut tree, Some(root), true);
        let pane = alloc_focusable_scope(&mut tree, Some(root), FocusScope::Contain);
        let inside = alloc_node(&mut tree, Some(pane), true);
        tree.node_mut(pane).children = vec![inside];
        tree.node_mut(root).children = vec![outside, pane];

        assert_eq!(
            tree.focusables(),
            vec![outside, pane],
            "the boundary is a stop in the outer ring"
        );
        assert_eq!(
            tree.focusables_in_subtree(pane),
            vec![inside],
            "the pane is not a member of its own ring"
        );

        // Tab reaches the pane, then continues the outer ring without entering.
        let mut focused = Some(outside);
        let (mut key, mut tag) = (None, None);
        step(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusDirection::Next,
        );
        assert_eq!(focused, Some(pane));
        step(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusDirection::Next,
        );
        assert_eq!(
            focused,
            Some(outside),
            "Tab passes over the pane, not into it"
        );

        // From inside, cycling cannot leak out over the boundary node.
        focused = Some(inside);
        step(
            &tree,
            &mut focused,
            &mut key,
            &mut tag,
            FocusDirection::Next,
        );
        assert_eq!(focused, Some(inside), "the trap stays airtight from inside");
    }

    #[test]
    fn auto_fallback_agrees_with_the_first_tab_target() {
        // Regression: the fallback walked children while the ring sorts by node id,
        // so after a child reorder startup focus and the first Tab target diverged.
        let (mut tree, root, _) = build_tree_with_focusable_children(0);
        let first_allocated = alloc_node(&mut tree, Some(root), true);
        let second_allocated = alloc_node(&mut tree, Some(root), true);
        tree.node_mut(root).children = vec![second_allocated, first_allocated];

        assert_eq!(first_focusable(&tree), Some(first_allocated));
        assert_eq!(first_focusable(&tree), tree.focusables().first().copied());

        let mut focused = Some(NodeId::INVALID);
        let (mut key, mut tag) = (None, None);
        restore_focus(&tree, &mut focused, &mut key, &mut tag, FocusPolicy::Auto);
        assert_eq!(focused, Some(first_allocated));
    }

    #[test]
    fn restore_focus_manual_skips_tag_restore() {
        let (tree, _root, _children) = build_tree_with_focusable_children(2);
        let mut focused = Some(NodeId::INVALID);
        let mut key = None;
        let mut tag = Some(Tag::Button);

        restore_focus(&tree, &mut focused, &mut key, &mut tag, FocusPolicy::Manual);

        assert_eq!(focused, None);
        assert_eq!(key, None);
        assert_eq!(tag, None);
    }
}
