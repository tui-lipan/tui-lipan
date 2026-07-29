//! Automatic retention of keyed container children that are playing an exit animation.
//!
//! The reconciler frees any node that the application stops describing, which is what makes exit
//! animations awkward: the element has to stay in the app's own state until the animation ends.
//! [`Animated::auto_exit`](crate::widgets::Animated::auto_exit) moves that into the container.
//!
//! The subtree is *not* re-derived. Nothing is cloned. The container keeps the node it already
//! reconciled and adopts it back into the tree each frame until the collapse ends, at which point
//! the slot stops being emitted and the ordinary epoch sweep frees it.
//!
//! How the vacated space is handled depends on how the container places children:
//!
//! - **Reflowing containers** (`VStack`, `HStack`) go through [`plan_exits`], which substitutes a
//!   [`Spacer`] collapsing on the stack's main axis at the child's former index. Layout therefore
//!   still sees a box shrinking to zero and siblings reflow exactly as they would have. The
//!   collapse starts from the child's laid-out rectangle, which is the only honest measure of what
//!   it occupied.
//! - **Positioned containers** (`Canvas`, `ZStack`) go through [`plan_positioned_exits`]. Children
//!   carry their own rectangle and never reflow around each other, so there is no slot to reserve
//!   and no space to reclaim: the departing child keeps its rectangle and fades. A collapse is
//!   available there as a pure effect, but only when the widget asks for it with
//!   [`ExitAnimation::with_collapse`](crate::animation::ExitAnimation::with_collapse), because the
//!   retained subtree is clipped rather than re-laid out and its bottom edge is cut away instead
//!   of travelling up.
//!
//! The two families also differ in depth. A `ZStack` is explicit layering, so an exiting layer
//! keeps the depth it had and fades over whatever is beneath it. A `Canvas` places children in
//! space, where overlap is incidental and a ghost covering live content is always wrong, so
//! exiting children are drawn beneath every live one.
//!
//! Containers outside both lists have no retention. `auto_exit` under one of them would silently do
//! nothing, so [`warn_if_auto_exit_inert`] reports it in debug builds. When adding a container,
//! update [`supports_auto_exit`] alongside it or the diagnostic will misreport.

use web_time::Instant;

use crate::core::element::Element;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::axis::Axis;
use crate::style::Length;
use crate::widgets::Spacer;

/// A child retained past its removal so it can finish its exit.
#[derive(Clone, Debug)]
pub(crate) struct ExitingChild {
    /// Root of the retained subtree.
    pub id: NodeId,
    /// Index the child occupied when it was removed. Retained slots are re-emitted here so the
    /// element collapses in place rather than jumping to the end of the container.
    pub index: usize,
    /// Backstop. A container that stops being reconciled mid-exit never reaches
    /// [`AnimatedNode::auto_exit_finished`], so retention must also expire on wall-clock time or
    /// the subtree would be held forever.
    ///
    /// [`AnimatedNode::auto_exit_finished`]: crate::widgets::internal::AnimatedNode
    pub deadline: Instant,
}

/// One position in the augmented child list handed to layout and reconciliation.
#[derive(Clone, Copy)]
pub(crate) enum ExitSlot {
    /// An element the application described, at its index in the original list.
    Live(usize),
    /// A retained subtree. Layout sees a spacer; reconciliation adopts the existing node.
    Exiting(NodeId),
}

/// Retention plan for one stack reconciliation.
pub(crate) struct ExitPlan {
    pub slots: Vec<ExitSlot>,
    pub elements: Vec<Element>,
    pub retained: Vec<ExitingChild>,
}

/// Grace added to the collapse duration before the wall-clock backstop fires, so an ordinary
/// animation is never cut short by a slow frame.
const DEADLINE_GRACE: std::time::Duration = std::time::Duration::from_millis(250);

/// Build the retention plan for a stack, or `None` when nothing is exiting.
///
/// Returning `None` is the overwhelmingly common case and costs one pass over the previous
/// children plus, when any child opted in, one pass over the new ones.
pub(crate) fn plan_exits(
    tree: &mut NodeTree,
    parent: NodeId,
    old_children: &[NodeId],
    children: &[Element],
    reuse: &[Option<NodeId>],
    axis: Axis,
) -> Option<ExitPlan> {
    let previously_retained = retained_of(tree, parent);
    let departed = departed_children(tree, old_children, reuse, &previously_retained);
    if departed.is_empty() && previously_retained.is_empty() {
        return None;
    }

    let now = Instant::now();
    let mut retained: Vec<ExitingChild> = Vec::new();

    // Carry forward anything still collapsing. A key the application described again is
    // resurrected instead: the plan already reused its node, so dropping it here is enough.
    for entry in previously_retained {
        if reuse.contains(&Some(entry.id)) {
            revive(tree, entry.id);
            continue;
        }
        if !tree.is_valid(entry.id) || now >= entry.deadline || exit_finished(tree, entry.id) {
            continue;
        }
        retained.push(entry);
    }

    for (index, id) in departed {
        if retained.iter().any(|entry| entry.id == id) {
            continue;
        }
        // A stack reclaims space on its main axis, starting from the size the child actually
        // occupied on screen.
        let rect = tree.node(id).rect;
        let from = match axis {
            Axis::Horizontal => rect.w,
            Axis::Vertical => rect.h,
        };
        let Some(duration) = begin_exit(tree, id, Some((axis, from))) else {
            continue;
        };
        retained.push(ExitingChild {
            id,
            index,
            deadline: now + duration + DEADLINE_GRACE,
        });
    }

    if retained.is_empty() {
        store_retained(tree, parent, Vec::new());
        return None;
    }

    refresh_retained_indices(&mut retained, old_children);
    let slots = interleave_exits(old_children, reuse, children.len(), &retained);
    let mut elements = Vec::with_capacity(children.len() + retained.len());
    for slot in &slots {
        match *slot {
            ExitSlot::Live(index) => elements.push(children[index].clone()),
            ExitSlot::Exiting(id) => elements.push(exit_spacer(tree, id, axis)),
        }
    }

    Some(ExitPlan {
        slots,
        elements,
        retained,
    })
}

/// Adopt a retained subtree into `rect` without re-deriving it.
///
/// Re-stamps the epoch across the subtree so the sweep spares it, re-registers every node so the
/// animation keeps ticking, and marks it inert so no input can reach callbacks whose owning
/// component may already be gone.
pub(crate) fn adopt_exiting(
    tree: &mut NodeTree,
    id: NodeId,
    parent: NodeId,
    rect: crate::style::Rect,
    epoch: u32,
) {
    if !tree.is_valid(id) {
        return;
    }
    {
        let node = tree.node_mut(id);
        node.parent = Some(parent);
        node.rect = rect;
    }

    let mut stack = vec![id];
    while let Some(current) = stack.pop() {
        if !tree.is_valid(current) {
            continue;
        }
        {
            let node = tree.node_mut(current);
            node.epoch = epoch;
            node.inert = true;
            if let NodeKind::Animated(animated) = &mut node.kind {
                animated.callbacks_suppressed = true;
            }
            stack.extend(node.children.iter().copied());
        }
        // Same registration the normal reconcile path uses, so animated widgets, spinners, and
        // animated scrolls inside a collapsing subtree keep being ticked.
        tree.note_kind_set(current);
    }
}

pub(crate) fn store_retained(tree: &mut NodeTree, parent: NodeId, retained: Vec<ExitingChild>) {
    match &mut tree.node_mut(parent).kind {
        NodeKind::VStack(stack) | NodeKind::HStack(stack) => stack.exiting = retained,
        NodeKind::Canvas(canvas) => canvas.exiting = retained,
        NodeKind::ZStack(zstack) => zstack.exiting = retained,
        _ => {}
    }
}

fn retained_of(tree: &NodeTree, parent: NodeId) -> Vec<ExitingChild> {
    match &tree.node(parent).kind {
        NodeKind::VStack(stack) | NodeKind::HStack(stack) => stack.exiting.clone(),
        NodeKind::Canvas(canvas) => canvas.exiting.clone(),
        NodeKind::ZStack(zstack) => zstack.exiting.clone(),
        _ => Vec::new(),
    }
}

/// Retention for containers that place children at explicit rectangles.
///
/// A [`Canvas`](crate::widgets::Canvas) or [`ZStack`](crate::widgets::ZStack) child owns its
/// placement, so none of the stack machinery applies: there is no slot to reserve and no sibling
/// to reflow. A retained child keeps exactly the rectangle it already had and fades out, and the
/// other children never move. Returns the children still exiting, ordered by their former index.
pub(crate) fn plan_positioned_exits(
    tree: &mut NodeTree,
    parent: NodeId,
    old_children: &[NodeId],
    reuse: &[Option<NodeId>],
) -> Vec<ExitingChild> {
    let previously_retained = retained_of(tree, parent);
    let departed = departed_children(tree, old_children, reuse, &previously_retained);
    if departed.is_empty() && previously_retained.is_empty() {
        return Vec::new();
    }

    let now = Instant::now();
    let mut retained: Vec<ExitingChild> = Vec::new();

    for entry in previously_retained {
        if reuse.contains(&Some(entry.id)) {
            revive(tree, entry.id);
            continue;
        }
        if !tree.is_valid(entry.id) || now >= entry.deadline || exit_finished(tree, entry.id) {
            continue;
        }
        retained.push(entry);
    }

    for (index, id) in departed {
        if retained.iter().any(|entry| entry.id == id) {
            continue;
        }
        // Nothing reflows around a positioned child, so there is no space to reclaim and the
        // collapse is purely an effect the widget has to ask for. Without it, fade only.
        let collapse_from = match &tree.node(id).kind {
            NodeKind::Animated(animated) if animated.auto_exit.is_some_and(|e| e.collapse) => {
                Some((Axis::Vertical, tree.node(id).rect.h))
            }
            _ => None,
        };
        let Some(duration) = begin_exit(tree, id, collapse_from) else {
            continue;
        };
        retained.push(ExitingChild {
            id,
            index,
            deadline: now + duration + DEADLINE_GRACE,
        });
    }

    refresh_retained_indices(&mut retained, old_children);
    retained
}

/// Merge retained children into the new live order while anchoring each exit to its nearest
/// surviving old sibling. This preserves simultaneous removals without letting stale old indices
/// reorder live children.
pub(crate) fn interleave_exits(
    old_children: &[NodeId],
    reuse: &[Option<NodeId>],
    live_len: usize,
    retained: &[ExitingChild],
) -> Vec<ExitSlot> {
    let mut live_by_id = std::collections::HashMap::new();
    for (index, id) in reuse.iter().copied().enumerate() {
        if let Some(id) = id {
            live_by_id.insert(id, index);
        }
    }

    let mut before = vec![Vec::new(); live_len];
    let mut after = vec![Vec::new(); live_len];
    let mut trailing = Vec::new();
    for entry in retained {
        let old_index = old_children
            .iter()
            .position(|id| *id == entry.id)
            .unwrap_or(entry.index.min(old_children.len()));
        let following = old_children
            .get(old_index.saturating_add(1)..)
            .into_iter()
            .flatten()
            .find_map(|id| live_by_id.get(id).copied());
        if let Some(index) = following {
            before[index].push(entry.id);
            continue;
        }
        let preceding = old_children
            .get(..old_index)
            .into_iter()
            .flatten()
            .rev()
            .find_map(|id| live_by_id.get(id).copied());
        if let Some(index) = preceding {
            after[index].push(entry.id);
        } else if live_len > 0 {
            before[entry.index.min(live_len - 1)].push(entry.id);
        } else {
            trailing.push(entry.id);
        }
    }

    let mut slots = Vec::with_capacity(live_len + retained.len());
    for index in 0..live_len {
        slots.extend(before[index].iter().copied().map(ExitSlot::Exiting));
        slots.push(ExitSlot::Live(index));
        slots.extend(after[index].iter().copied().map(ExitSlot::Exiting));
    }
    slots.extend(trailing.into_iter().map(ExitSlot::Exiting));
    slots
}

fn refresh_retained_indices(retained: &mut [ExitingChild], old_children: &[NodeId]) {
    for entry in retained.iter_mut() {
        if let Some(index) = old_children.iter().position(|id| *id == entry.id) {
            entry.index = index;
        }
    }
    retained.sort_by_key(|entry| entry.index);
}

/// Whether `kind` is a container that implements automatic exit retention.
///
/// Keep this in step with the containers that call [`plan_exits`] or
/// [`plan_positioned_exits`]; it is what the unsupported-container diagnostic checks against.
pub(crate) fn supports_auto_exit(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::VStack(_) | NodeKind::HStack(_) | NodeKind::Canvas(_) | NodeKind::ZStack(_)
    )
}

/// Report an `auto_exit` that can never fire, once per node.
///
/// `auto_exit` is implemented by the container, so a wrapper whose parent has no retention, or
/// which carries no key for the container to track it by, silently behaves as if the option were
/// never set. Failing loudly in debug builds turns "the feature is broken" into a five-second fix.
#[cfg(debug_assertions)]
pub(crate) fn warn_if_auto_exit_inert(tree: &mut NodeTree, id: NodeId) {
    let (needs_warning, reason) = {
        let node = tree.node(id);
        let NodeKind::Animated(animated) = &node.kind else {
            return;
        };
        if animated.auto_exit.is_none() || animated.auto_exit_warned {
            return;
        }
        let parent_supported = node.parent.is_some_and(|parent| {
            tree.is_valid(parent) && supports_auto_exit(&tree.node(parent).kind)
        });
        if !parent_supported {
            (
                true,
                "its parent container does not implement retention (supported: VStack, HStack, ZStack, Canvas)",
            )
        } else if node.key.is_none() {
            (true, "it has no key for the container to track it by")
        } else {
            (false, "")
        }
    };

    if !needs_warning {
        return;
    }
    if let NodeKind::Animated(animated) = &mut tree.node_mut(id).kind {
        animated.auto_exit_warned = true;
    }
    crate::debug::internal_log!(
        "[tui-lipan] Animated::auto_exit has no effect here: {reason}. The element will disappear \
         immediately instead of animating out. Wrap it in a supported container and give it a key, \
         or drive the exit yourself with Animated::exit and ExitQueue."
    );
}

/// Rectangle a positioned child occupies while it exits.
///
/// Unchanged by default. Only a child whose [`ExitAnimation`](crate::animation::ExitAnimation)
/// asked to collapse gets its height driven down, and that is opt-in precisely because of what
/// shortening this rectangle does:
/// children are clipped to it but are not re-laid out inside it, so the element keeps full size
/// while its lower rows, bottom border included, are sliced away.
pub(crate) fn positioned_exit_rect(tree: &NodeTree, id: NodeId) -> crate::style::Rect {
    let node = tree.node(id);
    let mut rect = node.rect;
    if let NodeKind::Animated(animated) = &node.kind
        && animated.auto_exit.is_some_and(|exit| exit.collapse)
        && animated.auto_exit_active
    {
        rect.h = rect.h.min(animated.auto_exit_height());
    }
    rect
}

/// Keyed children with an `auto_exit` wrapper that the new element list no longer describes.
fn departed_children(
    tree: &NodeTree,
    old_children: &[NodeId],
    reuse: &[Option<NodeId>],
    already_retained: &[ExitingChild],
) -> Vec<(usize, NodeId)> {
    old_children
        .iter()
        .enumerate()
        .filter(|(_, id)| {
            tree.is_valid(**id)
                && tree.node(**id).key.is_some()
                && !reuse.iter().any(|reused| reused.as_ref() == Some(*id))
                && !already_retained.iter().any(|entry| entry.id == **id)
                && matches!(&tree.node(**id).kind, NodeKind::Animated(a) if a.auto_exit.is_some())
        })
        .map(|(index, id)| (index, *id))
        .collect()
}

fn begin_exit(
    tree: &mut NodeTree,
    id: NodeId,
    collapse_from: Option<(Axis, u16)>,
) -> Option<std::time::Duration> {
    let NodeKind::Animated(animated) = &mut tree.node_mut(id).kind else {
        return None;
    };
    let duration = animated.auto_exit?.duration();
    animated.begin_auto_exit(collapse_from).then_some(duration)
}

fn exit_finished(tree: &NodeTree, id: NodeId) -> bool {
    match &tree.node(id).kind {
        NodeKind::Animated(animated) => animated.auto_exit_finished(),
        _ => true,
    }
}

/// Clear the inert mark when a key is described again mid-exit. The reconciler reuses the node
/// normally from here, which restarts its height and opacity transitions toward the live values.
fn revive(tree: &mut NodeTree, id: NodeId) {
    if !tree.is_valid(id) {
        return;
    }
    if let NodeKind::Animated(animated) = &mut tree.node_mut(id).kind {
        animated.cancel_auto_exit();
    }
    let mut stack = vec![id];
    while let Some(current) = stack.pop() {
        if !tree.is_valid(current) {
            continue;
        }
        let node = tree.node_mut(current);
        node.inert = false;
        if let NodeKind::Animated(animated) = &mut node.kind {
            animated.callbacks_suppressed = false;
        }
        stack.extend(node.children.iter().copied());
    }
}

/// Layout stand-in for a collapsing child: a spacer of its current animated main-axis size.
fn exit_spacer(tree: &NodeTree, id: NodeId, axis: Axis) -> Element {
    let (width, height) = match (&tree.node(id).kind, axis) {
        (NodeKind::Animated(animated), Axis::Horizontal) => {
            (Length::Px(animated.auto_exit_width()), Length::Flex(1))
        }
        (NodeKind::Animated(animated), Axis::Vertical) => {
            (Length::Flex(1), Length::Px(animated.auto_exit_height()))
        }
        (_, Axis::Horizontal) => (Length::Px(0), Length::Flex(1)),
        (_, Axis::Vertical) => (Length::Flex(1), Length::Px(0)),
    };
    Spacer::new().width(width).height(height).into()
}

#[cfg(test)]
mod tests {
    use crate::animation::ExitAnimation;
    use crate::core::component::{Component, Context, Update};
    use crate::core::element::{Element, IntoElement};
    use crate::style::Length;
    use crate::test_backend::TestBackend;
    use crate::widgets::animated::AnimatedNode;
    use crate::widgets::{Animated, HStack, Text, VStack};

    #[derive(Clone, Default)]
    struct Rows {
        removed: bool,
        duration_ms: u64,
    }

    struct RowList;

    impl Component for RowList {
        type Message = ();
        type Properties = u64;
        type State = Rows;

        fn create_state(&self, props: &Self::Properties) -> Self::State {
            Rows {
                removed: false,
                duration_ms: *props,
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let duration = ctx.state.duration_ms;
            let mut stack = VStack::new().height(Length::Flex(1));
            for row in ["alpha", "beta", "gamma"] {
                if row == "beta" && ctx.state.removed {
                    continue;
                }
                stack = stack.child(Animated::new(Text::new(row)).auto_exit(duration).key(row));
            }
            stack.into()
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state.removed = true;
            Update::full()
        }
    }

    #[test]
    fn a_removed_child_keeps_rendering_while_it_collapses() {
        let mut backend = TestBackend::new_with_props(RowList, 200);
        backend.render();
        assert!(backend.capture_frame().plain_text().contains("beta"));

        backend.dispatch(()).unwrap();
        backend.render();

        // The application stopped describing this row, yet the already-rendered subtree is
        // retained so the collapse has something to show.
        let text = backend.capture_frame().plain_text();
        assert!(
            text.contains("beta"),
            "retained row should still paint: {text}"
        );
        assert!(text.contains("alpha") && text.contains("gamma"));
    }

    #[derive(Clone, Default)]
    struct Columns {
        removed: bool,
    }

    struct ColumnList;

    impl Component for ColumnList {
        type Message = ();
        type Properties = ();
        type State = Columns;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            Columns::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let mut stack = HStack::new().width(Length::Auto).height(Length::Px(1));
            for column in ["aa", "bbbb", "cc"] {
                if column == "bbbb" && ctx.state.removed {
                    continue;
                }
                stack = stack.child(Animated::new(Text::new(column)).auto_exit(200).key(column));
            }
            stack.into()
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state.removed = true;
            Update::full()
        }
    }

    #[test]
    fn an_hstack_exit_collapses_the_departed_childs_width() {
        use crate::core::node::NodeKind;

        let mut backend = TestBackend::new(ColumnList);
        backend.render();
        let exiting = backend
            .core
            .tree
            .iter()
            .find(|node| node.key.as_ref().is_some_and(|key| key.as_ref() == "bbbb"))
            .map(|node| node.id)
            .expect("middle column");
        let trailing = backend
            .core
            .tree
            .iter()
            .find(|node| node.key.as_ref().is_some_and(|key| key.as_ref() == "cc"))
            .map(|node| node.id)
            .expect("trailing column");
        let original_width = backend.core.tree.node(exiting).rect.w;
        let original_trailing_x = backend.core.tree.node(trailing).rect.x;
        assert_eq!(original_width, 4);

        backend.dispatch(()).unwrap();
        backend.render();

        let NodeKind::Animated(animated) = &backend.core.tree.node(exiting).kind else {
            panic!("exiting column should remain animated");
        };
        assert_eq!(animated.auto_exit_width(), original_width);
        assert!(animated.width_anim.is_some());
        assert!(animated.height_anim.is_none());
        assert_eq!(backend.core.tree.node(trailing).rect.x, original_trailing_x);

        backend.advance(std::time::Duration::from_millis(50));
        let collapsed_width = match &backend.core.tree.node(exiting).kind {
            NodeKind::Animated(animated) => animated.auto_exit_width(),
            _ => unreachable!(),
        };
        assert!(collapsed_width < original_width);
        assert_eq!(
            backend.core.tree.node(trailing).rect.x,
            original_trailing_x - (original_width - collapsed_width) as i16
        );
    }

    #[derive(Clone, Default)]
    struct ReorderedRows {
        changed: bool,
    }

    struct ReorderingRowList;

    impl Component for ReorderingRowList {
        type Message = ();
        type Properties = ();
        type State = ReorderedRows;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            ReorderedRows::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            let order: &[&str] = if ctx.state.changed {
                &["d", "a"]
            } else {
                &["a", "b", "c", "d"]
            };
            order
                .iter()
                .fold(VStack::new(), |stack, key| {
                    stack.child(Animated::new(Text::new(*key)).auto_exit(200).key(*key))
                })
                .into()
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state.changed = true;
            Update::full()
        }
    }

    #[test]
    fn simultaneous_stack_exits_keep_their_order_without_reordering_live_children() {
        use crate::core::node::NodeKind;

        let mut backend = TestBackend::new(ReorderingRowList);
        backend.render();
        backend.dispatch(()).unwrap();
        backend.render();

        let stack = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::VStack(_)))
            .expect("row stack");
        let keys: Vec<&str> = stack
            .children
            .iter()
            .map(|id| {
                backend
                    .core
                    .tree
                    .node(*id)
                    .key
                    .as_ref()
                    .map_or("", |key| key.as_ref())
            })
            .collect();
        assert_eq!(keys, ["b", "c", "d", "a"]);
    }

    #[test]
    fn a_zero_duration_exit_releases_the_subtree_immediately() {
        let mut backend = TestBackend::new_with_props(RowList, 0);
        backend.render();
        assert!(backend.capture_frame().plain_text().contains("beta"));

        backend.dispatch(()).unwrap();
        backend.render();
        backend.render();

        let text = backend.capture_frame().plain_text();
        assert!(
            !text.contains("beta"),
            "collapsed row should be gone: {text}"
        );
        assert!(text.contains("alpha") && text.contains("gamma"));
    }

    #[derive(Clone, Default)]
    struct Panes {
        removed: bool,
    }

    struct PaneCanvas;

    impl Component for PaneCanvas {
        type Message = ();
        type Properties = ();
        type State = Panes;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            Panes::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            use crate::style::Rect;
            use crate::widgets::Canvas;

            let mut canvas = Canvas::new().width(Length::Flex(1)).height(Length::Flex(1));
            for (index, label) in ["left", "middle", "right"].iter().enumerate() {
                if *label == "middle" && ctx.state.removed {
                    continue;
                }
                canvas = canvas.child_at(
                    Rect {
                        x: (index as i16) * 10,
                        y: 0,
                        w: 9,
                        h: 3,
                    },
                    Animated::new(Text::new(*label)).auto_exit(200).key(*label),
                );
            }
            canvas.into()
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state.removed = true;
            Update::full()
        }
    }

    /// Canvas children own their placement, so a departing one must not disturb the others.
    #[test]
    fn a_removed_canvas_child_collapses_in_place_without_moving_siblings() {
        let mut backend = TestBackend::new(PaneCanvas);
        backend.render();
        let before = backend.capture_frame().to_fixed_grid_lines();

        backend.dispatch(()).unwrap();
        backend.render();
        let after = backend.capture_frame().to_fixed_grid_lines();

        assert!(
            after.iter().any(|line| line.contains("middle")),
            "the retained pane should still paint while it collapses: {after:?}"
        );
        // Absolutely positioned siblings never reflow, so their columns are untouched.
        for (before_line, after_line) in before.iter().zip(after.iter()) {
            let left = |line: &str| line.find("left");
            let right = |line: &str| line.find("right");
            assert_eq!(left(before_line), left(after_line));
            assert_eq!(right(before_line), right(after_line));
        }
    }

    #[test]
    fn a_retained_canvas_child_is_released_once_its_collapse_ends() {
        use crate::core::node::NodeKind;

        let mut backend = TestBackend::new(PaneCanvas);
        backend.render();
        backend.dispatch(()).unwrap();
        backend.render();

        // Force the collapse to completion the way the animation ticker would.
        let ids = backend.core.tree.animated_widget_ids().to_vec();
        for id in ids {
            if let NodeKind::Animated(node) = &mut backend.core.tree.node_mut(id).kind {
                node.tick(std::time::Duration::from_millis(400));
            }
        }
        backend.render();

        let text = backend.capture_frame().plain_text();
        assert!(
            !text.contains("middle"),
            "collapsed pane should be gone: {text}"
        );
        assert!(text.contains("left") && text.contains("right"));
    }

    /// A positioned child is clipped to its rectangle but is *not* re-laid out inside it, so
    /// shrinking that rectangle slices rows off the bottom instead of collapsing the element.
    /// Nothing reflows around a Canvas child, so the exit must leave the rectangle alone.
    #[test]
    fn an_exiting_canvas_child_keeps_its_whole_rectangle_while_it_fades() {
        use crate::core::node::NodeKind;
        use crate::style::Rect;
        use crate::widgets::Canvas;

        #[derive(Clone, Default)]
        struct Removed(bool);
        struct Rows;

        impl Component for Rows {
            type Message = ();
            type Properties = ();
            type State = Removed;

            fn create_state(&self, _props: &Self::Properties) -> Self::State {
                Removed::default()
            }

            fn view(&self, ctx: &Context<Self>) -> Element {
                let mut canvas = Canvas::new().width(Length::Flex(1)).height(Length::Flex(1));
                if !ctx.state.0 {
                    canvas = canvas.child_at(
                        Rect {
                            x: 0,
                            y: 0,
                            w: 6,
                            h: 3,
                        },
                        Animated::new(
                            VStack::new()
                                .child(Text::new("top"))
                                .child(Text::new("mid"))
                                .child(Text::new("bot")),
                        )
                        .auto_exit(200)
                        .key("pane"),
                    );
                }
                canvas.into()
            }

            fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
                ctx.state.0 = true;
                Update::full()
            }
        }

        let mut backend = TestBackend::new(Rows);
        backend.render();
        backend.dispatch(()).unwrap();
        backend.render();

        // Halfway through the exit: still fading, so every row it was drawing must still be drawn.
        let ids = backend.core.tree.animated_widget_ids().to_vec();
        for id in ids {
            if let NodeKind::Animated(node) = &mut backend.core.tree.node_mut(id).kind {
                node.tick(std::time::Duration::from_millis(100));
            }
        }
        backend.render();

        let text = backend.capture_frame().plain_text();
        for row in ["top", "mid", "bot"] {
            assert!(
                text.contains(row),
                "a fading pane must keep its whole rectangle, but {row:?} was clipped away: {text}"
            );
        }
    }

    /// The collapse a positioned parent will not do on its own, opted into per widget.
    #[test]
    fn an_opted_in_collapse_shrinks_a_positioned_child_from_its_real_height() {
        use crate::core::node::NodeKind;
        use crate::style::Rect;
        use crate::widgets::Canvas;

        #[derive(Clone, Default)]
        struct Removed(bool);
        struct Rows;

        impl Component for Rows {
            type Message = ();
            type Properties = ();
            type State = Removed;

            fn create_state(&self, _props: &Self::Properties) -> Self::State {
                Removed::default()
            }

            fn view(&self, ctx: &Context<Self>) -> Element {
                let mut canvas = Canvas::new().width(Length::Flex(1)).height(Length::Flex(1));
                if !ctx.state.0 {
                    canvas = canvas.child_at(
                        Rect {
                            x: 0,
                            y: 0,
                            w: 6,
                            h: 4,
                        },
                        Animated::new(
                            VStack::new()
                                .child(Text::new("r0"))
                                .child(Text::new("r1"))
                                .child(Text::new("r2"))
                                .child(Text::new("r3")),
                        )
                        .auto_exit(ExitAnimation::collapse(200))
                        .key("pane"),
                    );
                }
                canvas.into()
            }

            fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
                ctx.state.0 = true;
                Update::full()
            }
        }

        let mut backend = TestBackend::new(Rows);
        backend.render();
        backend.dispatch(()).unwrap();
        backend.render();

        // The collapse must start from the four rows the pane actually occupied, so the first
        // exiting frame still shows all of them rather than jumping part-way down.
        let text = backend.capture_frame().plain_text();
        for row in ["r0", "r1", "r2", "r3"] {
            assert!(
                text.contains(row),
                "the collapse must start at the pane's real height, but {row:?} was already \
                 clipped on the first exiting frame: {text}"
            );
        }

        let ids = backend.core.tree.animated_widget_ids().to_vec();
        for id in ids {
            if let NodeKind::Animated(node) = &mut backend.core.tree.node_mut(id).kind {
                node.tick(std::time::Duration::from_millis(120));
            }
        }
        backend.render();

        let text = backend.capture_frame().plain_text();
        assert!(
            text.contains("r0") && !text.contains("r3"),
            "the pane should be collapsing from the bottom by now: {text}"
        );
    }

    /// Canvas children are layered in child order. An element the application has stopped
    /// describing must not cover one it is still describing, which is what a closing pane would do
    /// to the neighbour expanding into its space.
    #[test]
    fn an_exiting_canvas_child_is_drawn_beneath_the_live_ones() {
        use crate::style::Rect;
        use crate::widgets::Canvas;

        #[derive(Clone, Default)]
        struct Removed(bool);
        struct Overlap;

        impl Component for Overlap {
            type Message = ();
            type Properties = ();
            type State = Removed;

            fn create_state(&self, _props: &Self::Properties) -> Self::State {
                Removed::default()
            }

            fn view(&self, ctx: &Context<Self>) -> Element {
                let spot = Rect {
                    x: 0,
                    y: 0,
                    w: 5,
                    h: 1,
                };
                let mut canvas = Canvas::new()
                    .width(Length::Flex(1))
                    .height(Length::Flex(1))
                    .child_at(spot, Animated::new(Text::new("aaaaa")).key("under"));
                if !ctx.state.0 {
                    // Described last, so it covers "under" while it is alive.
                    canvas = canvas.child_at(
                        spot,
                        Animated::new(Text::new("bbbbb")).auto_exit(200).key("over"),
                    );
                }
                canvas.into()
            }

            fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
                ctx.state.0 = true;
                Update::full()
            }
        }

        let mut backend = TestBackend::new(Overlap);
        backend.render();
        assert_eq!(backend.capture_frame().cell(0, 0).symbol, "b");

        backend.dispatch(()).unwrap();
        backend.render();

        assert_eq!(
            backend.capture_frame().cell(0, 0).symbol,
            "a",
            "the exiting layer must drop beneath the live one instead of covering it"
        );
    }

    /// Opting into an exit must not change anything about how the element lays out while it is
    /// alive. An earlier version forced `height` to `Length::Auto` to give the collapse a value to
    /// start from, which both changed live layout and started the collapse from the child's
    /// natural height rather than the height it actually occupied.
    #[test]
    fn auto_exit_does_not_change_how_a_live_element_lays_out() {
        struct Plain(bool);

        impl Component for Plain {
            type Message = ();
            type Properties = bool;
            type State = ();

            fn create_state(&self, _props: &Self::Properties) -> Self::State {}

            fn view(&self, _ctx: &Context<Self>) -> Element {
                let inner = VStack::new()
                    .child(Text::new("alpha"))
                    .child(Text::new("beta"));
                let animated = Animated::new(inner).height(Length::Flex(1));
                let animated = if self.0 {
                    animated.auto_exit(200)
                } else {
                    animated
                };
                VStack::new()
                    .height(Length::Flex(1))
                    .child(animated.key("row"))
                    .child(Text::new("footer"))
                    .into()
            }

            fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
                Update::none()
            }
        }

        let mut without = TestBackend::new_with_props(Plain(false), false);
        without.render();
        let mut with = TestBackend::new_with_props(Plain(true), true);
        with.render();

        assert_eq!(
            with.capture_frame().to_fixed_grid_lines(),
            without.capture_frame().to_fixed_grid_lines()
        );
    }

    /// Release must wait on the transitions the *exit* started, not on whatever else happens to
    /// be animating. A color settling from a hover, or a movement the element was part of when it
    /// was removed, would otherwise hold the subtree past its own exit.
    #[test]
    fn unrelated_animation_does_not_hold_an_exit_open() {
        use crate::core::node::NodeKind;
        use crate::style::Color;

        let mut backend = TestBackend::new_with_props(RowList, 60);
        backend.render();

        // Put a long, unrelated color transition in flight on the row that is about to leave.
        let ids = backend.core.tree.animated_widget_ids().to_vec();
        for id in ids.iter().copied() {
            if let NodeKind::Animated(node) = &mut backend.core.tree.node_mut(id).kind {
                node.bg_anim = Some(crate::animation::Transition::new(
                    Color::rgb(0, 0, 0),
                    Color::rgb(255, 255, 255),
                    std::time::Duration::from_secs(10),
                    node.transition_easing,
                ));
            }
        }

        backend.dispatch(()).unwrap();
        backend.render();
        for id in backend.core.tree.animated_widget_ids().to_vec() {
            if let NodeKind::Animated(node) = &mut backend.core.tree.node_mut(id).kind {
                node.tick(std::time::Duration::from_millis(120));
            }
        }
        backend.render();

        let text = backend.capture_frame().plain_text();
        assert!(
            !text.contains("beta"),
            "the exit finished, so a ten-second color fade must not keep the row alive: {text}"
        );
    }

    /// An exit translation is defined against the position the element had when it was removed,
    /// and it settles there. The FLIP machinery pulls offsets back to zero on completion, which
    /// would drag a leaving element back to its layout rectangle.
    #[test]
    fn an_exit_translation_starts_where_the_element_was_and_stays_where_it_went() {
        use crate::core::node::NodeKind;

        #[derive(Clone, Default)]
        struct Removed(bool);
        struct Slider;

        impl Component for Slider {
            type Message = ();
            type Properties = ();
            type State = Removed;

            fn create_state(&self, _props: &Self::Properties) -> Self::State {
                Removed::default()
            }

            fn view(&self, ctx: &Context<Self>) -> Element {
                let mut stack = VStack::new().height(Length::Flex(1));
                for row in ["alpha", "beta"] {
                    if row == "beta" && ctx.state.0 {
                        continue;
                    }
                    stack = stack.child(
                        Animated::new(Text::new(row))
                            .auto_exit(ExitAnimation::slide(200, 6, 0).keep_opacity())
                            .key(row),
                    );
                }
                stack.into()
            }

            fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
                ctx.state.0 = true;
                Update::full()
            }
        }

        let mut backend = TestBackend::new(Slider);
        backend.render();

        // Removal happens mid-move: the row already carries a visual offset.
        let exiting = {
            let tree = &backend.core.tree;
            let mut found = None;
            let mut stack = vec![tree.root];
            while let Some(id) = stack.pop() {
                if !tree.is_valid(id) {
                    continue;
                }
                let node = tree.node(id);
                if matches!(node.kind, NodeKind::Animated(_))
                    && node.key.as_ref().is_some_and(|key| key.as_ref() == "beta")
                {
                    found = Some(id);
                }
                stack.extend(node.children.iter().copied());
            }
            found.expect("the row to remove")
        };
        if let NodeKind::Animated(node) = &mut backend.core.tree.node_mut(exiting).kind {
            node.current_x_offset = 3.0;
        }

        backend.dispatch(()).unwrap();
        backend.render();

        // The exit takes that offset as its baseline instead of snapping back to zero first.
        let start = match &backend.core.tree.node(exiting).kind {
            NodeKind::Animated(node) => node.current_x_offset,
            _ => unreachable!(),
        };
        assert!(
            start >= 3.0,
            "the translation must start from where the element actually was, got {start}"
        );

        if let NodeKind::Animated(node) = &mut backend.core.tree.node_mut(exiting).kind {
            node.tick(std::time::Duration::from_millis(300));
            assert!(
                (node.current_x_offset - 9.0).abs() < 0.5,
                "it should settle at baseline + delta, not be pulled back to zero, got {}",
                node.current_x_offset
            );
        }
    }

    /// A retained subtree's component scope is already disposed, so a transition finishing on a
    /// descendant Animated node must not call back into it.
    #[test]
    fn descendant_transition_end_callbacks_are_suppressed_after_scope_disposal() {
        use crate::core::node::NodeKind;
        use std::cell::Cell;
        use std::rc::Rc;

        let fired = Rc::new(Cell::new(false));

        struct CallbackChild;
        impl Component for CallbackChild {
            type Message = ();
            type Properties = Rc<Cell<bool>>;
            type State = ();

            fn create_state(&self, _props: &Self::Properties) {}

            fn view(&self, ctx: &Context<Self>) -> Element {
                let fired = ctx.props.clone();
                Animated::new(Text::new("child"))
                    .on_opacity_transition_end(crate::callback::Callback::new(move |()| {
                        fired.set(true);
                    }))
                    .into()
            }

            fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
                Update::none()
            }
        }

        #[derive(Clone, Default)]
        struct Removed(bool);
        struct Parent;
        impl Component for Parent {
            type Message = ();
            type Properties = Rc<Cell<bool>>;
            type State = Removed;

            fn create_state(&self, _props: &Self::Properties) -> Self::State {
                Removed::default()
            }

            fn view(&self, ctx: &Context<Self>) -> Element {
                let mut stack = VStack::new();
                if !ctx.state.0 {
                    stack = stack.child(
                        Animated::new(crate::child(|| CallbackChild, ctx.props.clone()))
                            .auto_exit(100)
                            .key("row"),
                    );
                }
                stack.into()
            }

            fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
                ctx.state.0 = true;
                Update::full()
            }
        }

        let mut backend = TestBackend::new_with_props(Parent, fired.clone());
        backend.render();
        let descendant = backend
            .core
            .tree
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Animated(_)) && node.key.is_none())
            .map(|node| node.id)
            .expect("descendant Animated node");
        if let NodeKind::Animated(node) = &mut backend.core.tree.node_mut(descendant).kind {
            node.target_opacity = 0.0;
            node.opacity_anim = Some(crate::animation::Transition::new(
                1.0,
                0.0,
                std::time::Duration::from_millis(50),
                node.transition_easing,
            ));
        }

        backend.dispatch(()).unwrap();
        backend.render();
        let NodeKind::Animated(node) = &backend.core.tree.node(descendant).kind else {
            panic!("retained descendant should still exist");
        };
        assert!(node.callbacks_suppressed);
        if let NodeKind::Animated(node) = &mut backend.core.tree.node_mut(descendant).kind {
            node.tick(std::time::Duration::from_millis(100));
        }
        assert!(
            !fired.get(),
            "an exiting subtree must not call back into a scope that has already been swept"
        );

        // The same callbacks still fire for an ordinary, app-driven transition.
        let mut live = AnimatedNode::from(Animated::new(Text::new("x")));
        live.on_opacity_transition_end = Some({
            let fired = fired.clone();
            crate::callback::Callback::new(move |()| fired.set(true))
        });
        live.opacity_anim = Some(crate::animation::Transition::new(
            1.0,
            0.0,
            std::time::Duration::from_millis(50),
            live.transition_easing,
        ));
        live.tick(std::time::Duration::from_millis(100));
        assert!(fired.get(), "a live widget still reports its transitions");
        let _ = NodeKind::from(live);
    }

    struct LayerStack;

    impl Component for LayerStack {
        type Message = ();
        type Properties = ();
        type State = Panes;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            Panes::default()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            use crate::widgets::ZStack;

            let mut layers = ZStack::new();
            for label in ["base", "overlay"] {
                if label == "overlay" && ctx.state.removed {
                    continue;
                }
                layers = layers.child(Animated::new(Text::new(label)).auto_exit(200).key(label));
            }
            layers.into()
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state.removed = true;
            Update::full()
        }
    }

    #[test]
    fn a_removed_zstack_layer_collapses_in_place_then_reveals_the_layer_beneath() {
        use crate::core::node::NodeKind;

        let mut backend = TestBackend::new(LayerStack);
        backend.render();
        // Layers share a rectangle, so the top one covers the one beneath it.
        assert!(backend.capture_frame().plain_text().contains("overlay"));

        backend.dispatch(()).unwrap();
        backend.render();
        assert!(
            backend.capture_frame().plain_text().contains("overlay"),
            "the retained layer should still paint while it collapses"
        );

        let ids = backend.core.tree.animated_widget_ids().to_vec();
        for id in ids {
            if let NodeKind::Animated(node) = &mut backend.core.tree.node_mut(id).kind {
                node.tick(std::time::Duration::from_millis(400));
            }
        }
        backend.render();

        let text = backend.capture_frame().plain_text();
        assert!(
            !text.contains("overlay"),
            "collapsed layer should be gone: {text}"
        );
        assert!(
            text.contains("base"),
            "the layer beneath should be revealed: {text}"
        );
    }

    /// `auto_exit` is implemented per container, so an unsupported parent silently does nothing.
    /// The diagnostic is what stops that from reading as a broken feature.
    #[test]
    fn unsupported_parents_and_missing_keys_are_reported() {
        use crate::core::node::NodeKind;
        use crate::widgets::Grid;

        struct Unsupported;
        impl Component for Unsupported {
            type Message = ();
            type Properties = ();
            type State = ();
            fn create_state(&self, _props: &Self::Properties) {}
            fn view(&self, _ctx: &Context<Self>) -> Element {
                Grid::new()
                    .columns([Length::Flex(1)])
                    .child(Animated::new(Text::new("row")).auto_exit(200).key("row"))
                    .into()
            }
            fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
                Update::none()
            }
        }

        let mut backend = TestBackend::new(Unsupported);
        backend.render();

        // The wrapper reconciles, opts in, and is flagged rather than silently doing nothing.
        let tree = &backend.core.tree;
        let mut warned = false;
        let mut stack = vec![tree.root];
        while let Some(id) = stack.pop() {
            if !tree.is_valid(id) {
                continue;
            }
            let node = tree.node(id);
            if let NodeKind::Animated(animated) = &node.kind
                && animated.auto_exit.is_some()
                && animated.auto_exit_warned
            {
                warned = true;
            }
            stack.extend(node.children.iter().copied());
        }
        assert!(
            warned,
            "an auto_exit under an unsupported container must be reported"
        );
    }

    #[test]
    fn a_supported_parent_with_a_key_is_not_reported() {
        use crate::core::node::NodeKind;

        let mut backend = TestBackend::new_with_props(RowList, 200);
        backend.render();

        let tree = &backend.core.tree;
        let mut stack = vec![tree.root];
        while let Some(id) = stack.pop() {
            if !tree.is_valid(id) {
                continue;
            }
            let node = tree.node(id);
            if let NodeKind::Animated(animated) = &node.kind {
                assert!(
                    !animated.auto_exit_warned,
                    "a keyed child of a VStack is fully supported and must not warn"
                );
            }
            stack.extend(node.children.iter().copied());
        }
    }

    #[test]
    fn a_retained_subtree_is_inert_and_holds_no_focusables() {
        let mut backend = TestBackend::new_with_props(RowList, 200);
        backend.render();
        backend.dispatch(()).unwrap();
        backend.render();

        // The retained node is still in the tree but must not be reachable by pointer or focus,
        // because its callbacks may close over a component scope that is already gone.
        let tree = &backend.core.tree;
        let mut inert_found = false;
        let mut stack = vec![tree.root];
        while let Some(id) = stack.pop() {
            if !tree.is_valid(id) {
                continue;
            }
            let node = tree.node(id);
            if node.inert {
                inert_found = true;
            }
            stack.extend(node.children.iter().copied());
        }
        assert!(inert_found, "the collapsing subtree should be marked inert");
    }
}
