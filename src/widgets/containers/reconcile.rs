use std::borrow::Borrow;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

use crate::core::element::{Element, ElementKind};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::axis::{Axis, is_focus_protected};
use crate::layout::hash::{layout_hasher, stack_layout_hash};
use crate::layout::measure::min_size_constrained;
use crate::layout::reconcile::{ElementReconcile, ReconcileCtx, reconcile_element};
use crate::layout::stack::{layout_hstack, layout_vstack};
use crate::style::{Length, Rect};
use crate::utils::diff::reuse_plan;
#[cfg(feature = "diff-view")]
use crate::widgets::SplitWrapDualPass;
use crate::widgets::containers::FocusSizing;
use crate::widgets::frame::box_metrics::frame_inner_max_size;
use crate::widgets::internal::StackProps;

use super::super::text_area::{text_area_auto_height_for_width, text_area_pending_vim_search_row};
use super::node::StackLayoutCache;

pub(crate) fn stack_reuse_plan<T>(
    tree: &NodeTree,
    old_children: &[NodeId],
    new_children: &[T],
) -> Vec<Option<NodeId>>
where
    T: Borrow<crate::core::element::Element>,
{
    // Reuse policy for stacks:
    // - keyed children are matched by key (stable under reorder/insertions)
    // - unkeyed children are matched positionally among unkeyed old children
    #[cfg(debug_assertions)]
    {
        let mut seen = std::collections::HashSet::new();
        for child in new_children {
            if let Some(key) = child.borrow().key.clone()
                && !seen.insert(key.clone())
            {
                crate::debug::internal_log!(
                    "[tui-lipan] duplicate sibling key `{}` in multi-child container reconciliation",
                    key
                );
            }
        }
    }

    reuse_plan(
        old_children,
        new_children,
        |id| tree.node(*id).key.clone(),
        |child| crate::layout::tag::reuse_key_of_element(child.borrow()),
        |id| tree.is_valid(*id),
        |id, child| can_reuse(tree.node(*id), child.borrow()),
    )
}

/// Shared stack reconciliation used by both [`reconcile_vstack`] and
/// [`reconcile_hstack`].
///
/// * `axis` - drives the layout direction and whether split-wrap dual-pass
///   reconciliation is attempted (horizontal only).
/// * `pinned_key` - sticky accordion hint; only meaningful for vertical stacks.
pub(crate) struct StackReconcile<'a> {
    pub parent: NodeId,
    pub old_children: &'a [NodeId],
    pub props: &'a StackProps,
    pub children: &'a [Element],
    pub axis: Axis,
    pub bounds: Rect,
    pub pinned_key: Option<&'a str>,
}

pub(crate) fn reconcile_stack(
    ctx: &mut ReconcileCtx<'_>,
    args: StackReconcile<'_>,
) -> (Vec<NodeId>, Rc<Vec<Rect>>) {
    let StackReconcile {
        parent,
        old_children,
        props,
        children,
        axis,
        bounds,
        pinned_key,
    } = args;
    let focus = ctx.focus;
    let plan = stack_reuse_plan(ctx.tree, old_children, children);

    let pending_search_hash = pending_vim_search_layout_hash(ctx.tree, &plan, children, axis);
    let layout_hash =
        stack_layout_hash(props, children, axis, bounds, focus, pinned_key).map(|hash| {
            if let Some(pending_search_hash) = pending_search_hash {
                let mut hasher = layout_hasher();
                hash.hash(&mut hasher);
                pending_search_hash.hash(&mut hasher);
                hasher.finish()
            } else {
                hash
            }
        });
    let rects = match layout_hash {
        Some(hash) => {
            let cached = {
                let node = ctx.tree.node(parent);
                match &node.kind {
                    NodeKind::VStack(node) | NodeKind::HStack(node) => node
                        .layout_cache
                        .as_ref()
                        .filter(|cache| {
                            cache.bounds == bounds
                                && cache.layout_hash == hash
                                && cache.child_rects.len() == children.len()
                        })
                        .map(|cache| Rc::clone(&cache.child_rects)),
                    _ => None,
                }
            };
            cached.unwrap_or_else(|| {
                let mut rects = match axis {
                    Axis::Vertical => layout_vstack(props, children, bounds, focus, pinned_key),
                    Axis::Horizontal => layout_hstack(props, children, bounds, focus),
                };
                apply_pending_vim_search_layout(
                    ctx.tree, &plan, children, axis, bounds, &mut rects,
                );
                Rc::new(rects)
            })
        }
        None => {
            let mut rects = match axis {
                Axis::Vertical => layout_vstack(props, children, bounds, focus, pinned_key),
                Axis::Horizontal => layout_hstack(props, children, bounds, focus),
            };
            apply_pending_vim_search_layout(ctx.tree, &plan, children, axis, bounds, &mut rects);
            Rc::new(rects)
        }
    };

    #[cfg(feature = "diff-view")]
    let split_wrap_dual_pass = if axis == Axis::Horizontal {
        SplitWrapDualPass::begin_reconcile(
            children
                .iter()
                .zip(rects.iter().copied().map(|rect| rect.w)),
        )
    } else {
        None
    };
    #[cfg(feature = "diff-view")]
    let reconcile_passes: &[u8] = if split_wrap_dual_pass.is_some() {
        &SplitWrapDualPass::PASSES
    } else {
        &[0]
    };
    #[cfg(not(feature = "diff-view"))]
    let reconcile_passes: &[u8] = &[0];

    let mut child_ids: Vec<NodeId> = Vec::new();
    for (pass_idx, _) in reconcile_passes.iter().enumerate() {
        #[cfg(feature = "diff-view")]
        if let Some(ref dual_pass) = split_wrap_dual_pass {
            dual_pass.set_pass(reconcile_passes[pass_idx]);
        }

        let mut next_ids = Vec::with_capacity(children.len());
        for (i, child) in children.iter().enumerate() {
            let reuse_id = if pass_idx == 0 {
                plan[i]
            } else {
                Some(child_ids[i])
            };
            let rect = rects[i];
            let child_id = reconcile_element(
                ctx,
                ElementReconcile {
                    reuse: reuse_id,
                    parent: Some(parent),
                    el: child,
                    rect,
                },
            );
            next_ids.push(child_id);
        }
        child_ids = next_ids;
    }

    let node = ctx.tree.node_mut(parent);
    match &mut node.kind {
        NodeKind::VStack(node) | NodeKind::HStack(node) => {
            node.layout_cache = layout_hash.map(|hash| StackLayoutCache {
                bounds,
                layout_hash: hash,
                child_rects: Rc::clone(&rects),
            });
        }
        _ => {}
    }

    (child_ids, rects)
}

fn pending_vim_search_layout_hash(
    tree: &NodeTree,
    plan: &[Option<NodeId>],
    children: &[Element],
    axis: Axis,
) -> Option<u64> {
    if axis != Axis::Vertical {
        return None;
    }

    let mut hasher = layout_hasher();
    let mut found = false;
    for (idx, child) in children.iter().enumerate() {
        if child_has_pending_vim_search_runtime_height(
            tree,
            child,
            plan.get(idx).copied().flatten(),
        ) {
            idx.hash(&mut hasher);
            found = true;
        }
    }

    found.then(|| hasher.finish())
}

fn apply_pending_vim_search_layout(
    tree: &NodeTree,
    plan: &[Option<NodeId>],
    children: &[Element],
    axis: Axis,
    bounds: Rect,
    rects: &mut [Rect],
) {
    if axis != Axis::Vertical {
        return;
    }

    let mut y_delta = 0i16;
    let bottom = i32::from(bounds.y).saturating_add(i32::from(bounds.h));

    for (idx, child) in children.iter().enumerate() {
        if matches!(child.kind, ElementKind::Portal(_)) {
            continue;
        }

        if y_delta != 0 {
            rects[idx].y = rects[idx].y.saturating_add(y_delta);
        }

        let remaining = bottom
            .saturating_sub(i32::from(rects[idx].y))
            .max(0)
            .min(i32::from(u16::MAX)) as u16;
        if rects[idx].h > remaining {
            let delta = i32::from(remaining).saturating_sub(i32::from(rects[idx].h));
            rects[idx].h = remaining;
            y_delta = y_delta
                .saturating_add(delta.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
        }

        let Some(desired_h) = child_pending_vim_search_auto_height(
            tree,
            child,
            plan.get(idx).copied().flatten(),
            rects[idx].w,
        ) else {
            continue;
        };

        let mut remaining = remaining;
        if desired_h > remaining {
            let reclaimed = reclaim_previous_vertical_space(rects, idx, desired_h - remaining);
            if reclaimed > 0 {
                remaining = remaining.saturating_add(reclaimed);
            }
        }

        let desired_h = desired_h.min(remaining);
        if desired_h == rects[idx].h {
            continue;
        }

        let delta = i32::from(desired_h).saturating_sub(i32::from(rects[idx].h));
        rects[idx].h = desired_h;
        y_delta =
            y_delta.saturating_add(delta.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16);
    }
}

fn reclaim_previous_vertical_space(rects: &mut [Rect], idx: usize, mut needed: u16) -> u16 {
    let mut reclaimed = 0u16;
    for donor_idx in (0..idx).rev() {
        if needed == 0 {
            break;
        }

        let available = rects[donor_idx].h.saturating_sub(1);
        if available == 0 {
            continue;
        }

        let take = available.min(needed);
        rects[donor_idx].h = rects[donor_idx].h.saturating_sub(take);
        for shifted in rects.iter_mut().take(idx + 1).skip(donor_idx + 1) {
            shifted.y = shifted.y.saturating_sub(take as i16);
        }
        needed = needed.saturating_sub(take);
        reclaimed = reclaimed.saturating_add(take);
    }

    reclaimed
}

fn child_has_pending_vim_search_runtime_height(
    tree: &NodeTree,
    child: &Element,
    reuse_id: Option<NodeId>,
) -> bool {
    match &child.kind {
        ElementKind::TextArea(text_area) => {
            if !matches!(text_area.height, Length::Auto) {
                return false;
            }
            let Some(reuse_id) = reuse_id.filter(|id| tree.is_valid(*id)) else {
                return false;
            };
            text_area_pending_vim_search_row(text_area, Some(&tree.node(reuse_id).kind))
        }
        ElementKind::Frame(frame) => {
            if !matches!(frame.props.height, Length::Auto) {
                return false;
            }
            let Some(child) = frame.child.as_deref() else {
                return false;
            };
            let Some(reuse_child) = frame_reuse_child_id(tree, reuse_id, frame, child) else {
                return false;
            };
            child_has_pending_vim_search_runtime_height(tree, child, Some(reuse_child))
        }
        ElementKind::Popover(popover) => {
            let Some(reuse_trigger) = popover_reuse_trigger_id(tree, reuse_id, popover) else {
                return false;
            };
            child_has_pending_vim_search_runtime_height(
                tree,
                popover.trigger.as_ref(),
                Some(reuse_trigger),
            )
        }
        ElementKind::Animated(animated) => {
            if !animated_allows_runtime_auto_height(animated) {
                return false;
            }
            let Some(reuse_child) = single_reuse_child_id(tree, reuse_id, animated.child.as_ref())
            else {
                return false;
            };
            child_has_pending_vim_search_runtime_height(
                tree,
                animated.child.as_ref(),
                Some(reuse_child),
            )
        }
        ElementKind::Group(group) => {
            let Some(reuse_child) = single_reuse_child_id(tree, reuse_id, group.child.as_ref())
            else {
                return false;
            };
            child_has_pending_vim_search_runtime_height(
                tree,
                group.child.as_ref(),
                Some(reuse_child),
            )
        }
        ElementKind::EffectScope(scope) => {
            let Some(child) = scope.child.as_deref() else {
                return false;
            };
            let Some(reuse_child) = single_reuse_child_id(tree, reuse_id, child) else {
                return false;
            };
            child_has_pending_vim_search_runtime_height(tree, child, Some(reuse_child))
        }
        ElementKind::ThemeProvider(provider) => {
            let Some(reuse_child) = single_reuse_child_id(tree, reuse_id, &provider.child) else {
                return false;
            };
            child_has_pending_vim_search_runtime_height(tree, &provider.child, Some(reuse_child))
        }
        ElementKind::ContextProvider(provider) => {
            let Some(reuse_child) = single_reuse_child_id(tree, reuse_id, &provider.child) else {
                return false;
            };
            child_has_pending_vim_search_runtime_height(tree, &provider.child, Some(reuse_child))
        }
        ElementKind::DragSource(source) => {
            let Some(child) = source.child.as_deref() else {
                return false;
            };
            let Some(reuse_child) = single_reuse_child_id(tree, reuse_id, child) else {
                return false;
            };
            child_has_pending_vim_search_runtime_height(tree, child, Some(reuse_child))
        }
        ElementKind::DropTarget(target) => {
            let Some(child) = target.child.as_deref() else {
                return false;
            };
            let Some(reuse_child) = single_reuse_child_id(tree, reuse_id, child) else {
                return false;
            };
            child_has_pending_vim_search_runtime_height(tree, child, Some(reuse_child))
        }
        ElementKind::MouseRegion(region) => {
            let Some(child) = region.child.as_deref() else {
                return false;
            };
            let Some(reuse_child) = single_reuse_child_id(tree, reuse_id, child) else {
                return false;
            };
            child_has_pending_vim_search_runtime_height(tree, child, Some(reuse_child))
        }
        ElementKind::VStack(stack) => {
            if !matches!(stack.props.height, Length::Auto) {
                return false;
            }
            let Some(reuse_id) = reuse_id.filter(|id| tree.is_valid(*id)) else {
                return false;
            };
            let plan = stack_reuse_plan(tree, &tree.node(reuse_id).children, &stack.children);
            stack.children.iter().enumerate().any(|(idx, child)| {
                child_has_pending_vim_search_runtime_height(
                    tree,
                    child,
                    plan.get(idx).copied().flatten(),
                )
            })
        }
        _ => false,
    }
}

fn child_pending_vim_search_auto_height(
    tree: &NodeTree,
    child: &Element,
    reuse_id: Option<NodeId>,
    width: u16,
) -> Option<u16> {
    match &child.kind {
        ElementKind::TextArea(text_area) => {
            if !child_has_pending_vim_search_runtime_height(tree, child, reuse_id) {
                return None;
            }
            let reuse_id = reuse_id.filter(|id| tree.is_valid(*id))?;
            let parent_h_edge = tree
                .parent_frame_integrated_h_edge(reuse_id)
                .unwrap_or(false);
            let height = text_area_auto_height_for_width(text_area, width, true, parent_h_edge);
            Some(clamp_runtime_height(child, height))
        }
        ElementKind::Frame(frame) => {
            if !matches!(frame.props.height, Length::Auto) {
                return None;
            }
            let frame_child = frame.child.as_deref()?;
            let reuse_child = frame_reuse_child_id(tree, reuse_id, frame, frame_child)?;
            let (inner_w, _) = frame_inner_max_size(frame, Some(width), None);
            let child_runtime_h = child_pending_vim_search_auto_height(
                tree,
                frame_child,
                Some(reuse_child),
                inner_w.unwrap_or(width),
            )?;
            let (_, child_base_h) = min_size_constrained(frame_child, inner_w, None);
            let delta = child_runtime_h.saturating_sub(child_base_h);
            if delta == 0 {
                return None;
            }
            let (_, base_h) = min_size_constrained(child, Some(width), None);
            Some(clamp_runtime_height(child, base_h.saturating_add(delta)))
        }
        ElementKind::Popover(popover) => {
            let reuse_trigger = popover_reuse_trigger_id(tree, reuse_id, popover)?;
            let trigger = popover.trigger.as_ref();
            let trigger_runtime_h =
                child_pending_vim_search_auto_height(tree, trigger, Some(reuse_trigger), width)?;
            let (_, trigger_base_h) = min_size_constrained(trigger, Some(width), None);
            let delta = trigger_runtime_h.saturating_sub(trigger_base_h);
            if delta == 0 {
                return None;
            }
            let (_, base_h) = min_size_constrained(child, Some(width), None);
            Some(clamp_runtime_height(child, base_h.saturating_add(delta)))
        }
        ElementKind::Animated(animated) => {
            if !animated_allows_runtime_auto_height(animated) {
                return None;
            }
            let animated_child = animated.child.as_ref();
            let reuse_child = single_reuse_child_id(tree, reuse_id, animated_child)?;
            let child_runtime_h = child_pending_vim_search_auto_height(
                tree,
                animated_child,
                Some(reuse_child),
                width,
            )?;
            let (_, child_base_h) = min_size_constrained(animated_child, Some(width), None);
            let delta = child_runtime_h.saturating_sub(child_base_h);
            if delta == 0 {
                return None;
            }
            let (_, base_h) = min_size_constrained(child, Some(width), None);
            Some(clamp_runtime_height(child, base_h.saturating_add(delta)))
        }
        ElementKind::Group(group) => child_pending_vim_search_transparent_wrapper_height(
            tree,
            child,
            reuse_id,
            group.child.as_ref(),
            width,
        ),
        ElementKind::EffectScope(scope) => {
            let wrapped = scope.child.as_deref()?;
            child_pending_vim_search_transparent_wrapper_height(
                tree, child, reuse_id, wrapped, width,
            )
        }
        ElementKind::ThemeProvider(provider) => {
            child_pending_vim_search_transparent_wrapper_height(
                tree,
                child,
                reuse_id,
                &provider.child,
                width,
            )
        }
        ElementKind::ContextProvider(provider) => {
            child_pending_vim_search_transparent_wrapper_height(
                tree,
                child,
                reuse_id,
                &provider.child,
                width,
            )
        }
        ElementKind::DragSource(source) => {
            let wrapped = source.child.as_deref()?;
            child_pending_vim_search_transparent_wrapper_height(
                tree, child, reuse_id, wrapped, width,
            )
        }
        ElementKind::DropTarget(target) => {
            let wrapped = target.child.as_deref()?;
            child_pending_vim_search_transparent_wrapper_height(
                tree, child, reuse_id, wrapped, width,
            )
        }
        ElementKind::MouseRegion(region) => {
            let wrapped = region.child.as_deref()?;
            child_pending_vim_search_transparent_wrapper_height(
                tree, child, reuse_id, wrapped, width,
            )
        }
        ElementKind::VStack(stack) => {
            if !matches!(stack.props.height, Length::Auto) {
                return None;
            }
            let reuse_id = reuse_id.filter(|id| tree.is_valid(*id))?;
            let inner_width = width
                .saturating_sub(if stack.props.border { 2 } else { 0 })
                .saturating_sub(stack.props.padding.horizontal());
            let plan = stack_reuse_plan(tree, &tree.node(reuse_id).children, &stack.children);
            let mut delta = 0u16;
            for (idx, child) in stack.children.iter().enumerate() {
                let Some(runtime_h) = child_pending_vim_search_auto_height(
                    tree,
                    child,
                    plan.get(idx).copied().flatten(),
                    inner_width,
                ) else {
                    continue;
                };
                let (_, base_h) = min_size_constrained(child, Some(inner_width), None);
                delta = delta.saturating_add(runtime_h.saturating_sub(base_h));
            }
            if delta == 0 {
                return None;
            }
            let (_, base_h) = min_size_constrained(child, Some(width), None);
            Some(clamp_runtime_height(child, base_h.saturating_add(delta)))
        }
        _ => None,
    }
}

fn child_pending_vim_search_transparent_wrapper_height(
    tree: &NodeTree,
    wrapper: &Element,
    reuse_id: Option<NodeId>,
    wrapped: &Element,
    width: u16,
) -> Option<u16> {
    let reuse_child = single_reuse_child_id(tree, reuse_id, wrapped)?;
    let child_runtime_h =
        child_pending_vim_search_auto_height(tree, wrapped, Some(reuse_child), width)?;
    let (_, child_base_h) = min_size_constrained(wrapped, Some(width), None);
    let delta = child_runtime_h.saturating_sub(child_base_h);
    if delta == 0 {
        return None;
    }
    let (_, base_h) = min_size_constrained(wrapper, Some(width), None);
    Some(clamp_runtime_height(wrapper, base_h.saturating_add(delta)))
}

fn clamp_runtime_height(child: &Element, height: u16) -> u16 {
    child.layout_constraints().clamp_height(height, u16::MAX)
}

fn animated_allows_runtime_auto_height(animated: &crate::widgets::Animated) -> bool {
    matches!(
        animated.layout_height.or(animated.height),
        None | Some(Length::Auto)
    )
}

fn single_reuse_child_id(
    tree: &NodeTree,
    reuse_id: Option<NodeId>,
    child: &Element,
) -> Option<NodeId> {
    let reuse_id = reuse_id.filter(|id| tree.is_valid(*id))?;
    tree.node(reuse_id)
        .children
        .iter()
        .copied()
        .find(|id| tree.is_valid(*id) && can_reuse(tree.node(*id), child))
}

fn popover_reuse_trigger_id(
    tree: &NodeTree,
    reuse_id: Option<NodeId>,
    popover: &crate::widgets::Popover,
) -> Option<NodeId> {
    let reuse_id = reuse_id.filter(|id| tree.is_valid(*id))?;
    let trigger = popover.trigger.as_ref();
    tree.node(reuse_id)
        .children
        .first()
        .copied()
        .filter(|id| tree.is_valid(*id) && can_reuse(tree.node(*id), trigger))
}

fn frame_reuse_child_id(
    tree: &NodeTree,
    reuse_id: Option<NodeId>,
    frame: &crate::widgets::Frame,
    child: &Element,
) -> Option<NodeId> {
    let reuse_id = reuse_id.filter(|id| tree.is_valid(*id))?;
    let old_children = &tree.node(reuse_id).children;
    let reuse_header = frame.header.as_deref().and_then(|header| {
        old_children
            .iter()
            .copied()
            .find(|id| tree.is_valid(*id) && can_reuse(tree.node(*id), header))
    });
    old_children.iter().copied().find(|id| {
        tree.is_valid(*id) && Some(*id) != reuse_header && can_reuse(tree.node(*id), child)
    })
}

pub(crate) struct VStackReconcile<'a> {
    pub parent: NodeId,
    pub old_children: Vec<NodeId>,
    pub vs: &'a crate::widgets::VStack,
    pub bounds: Rect,
}

pub(crate) fn reconcile_vstack(
    ctx: &mut ReconcileCtx<'_>,
    args: VStackReconcile<'_>,
) -> Vec<NodeId> {
    let VStackReconcile {
        parent,
        old_children,
        vs,
        bounds,
    } = args;
    let focus = ctx.focus;
    // Determine sticky pinned key from persistent node state.
    // Only active when the accordion policy has sticky=true and no child
    // currently has real focus (real focus always takes priority).
    let is_sticky = matches!(
        &vs.props.focus_sizing,
        FocusSizing::Accordion(acc) if acc.sticky
    );
    let has_real_focus = is_sticky && vs.children.iter().any(|c| is_focus_protected(c, focus));
    let pinned_key: Option<crate::core::element::Key> = if is_sticky && !has_real_focus {
        let node = ctx.tree.node(parent);
        if let NodeKind::VStack(node) = &node.kind {
            node.last_focused_key.clone()
        } else {
            None
        }
    } else {
        None
    };
    let pinned_str: Option<&str> = pinned_key.as_ref().map(|k| k.as_ref());

    let (new_children, _rects) = reconcile_stack(
        ctx,
        StackReconcile {
            parent,
            old_children: &old_children,
            props: &vs.props,
            children: &vs.children,
            axis: Axis::Vertical,
            bounds,
            pinned_key: pinned_str,
        },
    );

    // Update last_focused_key when a child has real focus, so the
    // sticky accordion can preserve it after focus moves elsewhere.
    if is_sticky {
        let node = ctx.tree.node_mut(parent);
        if let NodeKind::VStack(node) = &mut node.kind
            && let Some(focused) = vs
                .children
                .iter()
                .find(|c| is_focus_protected(c, focus))
                .and_then(|c| c.key.clone())
        {
            node.last_focused_key = Some(focused);
        }
    }

    new_children
}

pub(crate) struct HStackReconcile<'a> {
    pub parent: NodeId,
    pub old_children: Vec<NodeId>,
    pub hs: &'a crate::widgets::HStack,
    pub bounds: Rect,
}

pub(crate) fn reconcile_hstack(
    ctx: &mut ReconcileCtx<'_>,
    args: HStackReconcile<'_>,
) -> Vec<NodeId> {
    let HStackReconcile {
        parent,
        old_children,
        hs,
        bounds,
    } = args;
    reconcile_stack(
        ctx,
        StackReconcile {
            parent,
            old_children: &old_children,
            props: &hs.props,
            children: &hs.children,
            axis: Axis::Horizontal,
            bounds,
            pinned_key: None,
        },
    )
    .0
}

pub(crate) fn can_reuse(
    node: &crate::core::node::Node,
    el: &crate::core::element::Element,
) -> bool {
    crate::layout::tag::can_reuse(node, el)
}
