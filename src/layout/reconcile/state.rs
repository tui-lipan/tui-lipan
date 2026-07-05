use crate::core::component::FocusContext;
use crate::core::element::Element;
use crate::core::node::{NodeId, NodeKind, NodeTree, OverlayRoot};
use crate::layout::tag::can_reuse;
use crate::style::{LayoutConstraints, Length, Rect};

use super::element::{ElementReconcile, reconcile_element};

pub(crate) struct ReconcileCtx<'a> {
    pub tree: &'a mut NodeTree,
    pub epoch: u32,
    pub focus: Option<&'a FocusContext>,
    pub overlay_state: &'a mut OverlayState,
}

pub(crate) struct SimpleLeafReconcile<'a> {
    pub id: NodeId,
    pub rect: Rect,
    pub constraints: &'a LayoutConstraints,
    pub width: Length,
    pub height: Length,
    pub measured: (u16, u16),
}

pub(crate) struct SingleChildReconcile<'a> {
    pub parent_id: NodeId,
    pub child: Option<&'a Element>,
    pub rect: Rect,
    pub old_children: Vec<NodeId>,
}

pub(crate) fn apply_constraints(
    rect: &mut Rect,
    constraints: &LayoutConstraints,
    avail_w: u16,
    avail_h: u16,
) {
    rect.w = constraints.clamp_width(rect.w, avail_w);
    rect.h = constraints.clamp_height(rect.h, avail_h);
}

pub(crate) fn resolve_rect_with_auto(
    rect: Rect,
    constraints: &LayoutConstraints,
    width: Length,
    height: Length,
    measured_w: u16,
    measured_h: u16,
) -> Rect {
    // Capture the parent-allocated dimensions before auto-resolution - these
    // are the "available" values used to resolve Percent min/max constraints.
    let avail_w = rect.w;
    let avail_h = rect.h;
    let mut resolved = rect;
    if matches!(width, Length::Auto) {
        resolved.w = measured_w.min(resolved.w);
    }
    if matches!(height, Length::Auto) {
        resolved.h = measured_h.min(resolved.h);
    }
    apply_constraints(&mut resolved, constraints, avail_w, avail_h);
    resolved
}

/// Shared reconcile path for simple leaf widgets.
///
/// Covers widgets whose reconcile is: measure → resolve rect with auto →
/// set node kind → return id.  No child reconciliation, no state
/// preservation from the existing node.
pub(crate) fn reconcile_simple_leaf(
    tree: &mut NodeTree,
    args: SimpleLeafReconcile<'_>,
    build_kind: impl FnOnce() -> NodeKind,
) -> NodeId {
    let rect = resolve_rect_with_auto(
        args.rect,
        args.constraints,
        args.width,
        args.height,
        args.measured.0,
        args.measured.1,
    );

    let node = tree.node_mut(args.id);
    node.rect = rect;
    node.children.clear();
    node.kind = build_kind();

    args.id
}

pub(crate) fn reuse_or_replace_kind(
    kind: &mut NodeKind,
    reconcile_in_place: impl FnOnce(&mut NodeKind) -> bool,
    build_new: impl FnOnce() -> NodeKind,
) {
    if !reconcile_in_place(kind) {
        *kind = build_new();
    }
}

/// Reconcile a single optional child widget, reusing an old child when possible.
///
/// This is the standard pattern for one-child wrapper widgets:
/// 1. Find a reusable child via `can_reuse`.
/// 2. Reconcile the child element into the reused or fresh node.
/// 3. Return a new children vec containing exactly 0 or 1 child.
pub(crate) fn reconcile_single_child(
    ctx: &mut ReconcileCtx<'_>,
    args: SingleChildReconcile<'_>,
) -> Vec<NodeId> {
    let reuse_child = args.child.and_then(|child| {
        args.old_children
            .iter()
            .copied()
            .find(|id| ctx.tree.is_valid(*id) && can_reuse(ctx.tree.node(*id), child))
    });

    let mut new_children = args.old_children;
    new_children.clear();

    if let Some(child) = args.child {
        let child_id = reconcile_element(
            ctx,
            ElementReconcile {
                reuse: reuse_child,
                parent: Some(args.parent_id),
                el: child,
                rect: args.rect,
            },
        );
        new_children.push(child_id);
    }

    new_children
}

/// Reconcile a single required child widget, reusing an old child when possible.
///
/// Like [`reconcile_single_child`] but the child is guaranteed to exist.
pub(crate) fn reconcile_single_child_required(
    ctx: &mut ReconcileCtx<'_>,
    args: SingleChildReconcile<'_>,
) -> Vec<NodeId> {
    reconcile_single_child(ctx, args)
}

pub(crate) struct OverlayState {
    pub(crate) bounds: Rect,
    pub(crate) allow_root_overlays: bool,
    pub(crate) roots: Vec<OverlayRoot>,
    pub(crate) order: u64,
}

impl OverlayState {
    pub(crate) fn new(bounds: Rect, allow_root_overlays: bool) -> Self {
        Self {
            bounds,
            allow_root_overlays,
            roots: Vec::new(),
            order: 0,
        }
    }

    pub(crate) fn next_order(&mut self) -> u64 {
        let order = self.order;
        self.order = self.order.saturating_add(1);
        order
    }
}
