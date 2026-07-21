use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::{ElementReconcile, OverlayState, ReconcileCtx, reconcile_element};
use crate::overlay::OverlayScope;
use crate::style::Rect;
use crate::widgets::popover::layout::resolve_popover_rect;
use crate::widgets::popover::node::PopoverNode;

pub(crate) fn reconcile_popover(
    tree: &mut NodeTree,
    epoch: u32,
    id: NodeId,
    popover: &super::Popover,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
) -> NodeId {
    let old_children = {
        let node = tree.node_mut(id);
        node.rect = rect;
        node.kind = NodeKind::Popover(PopoverNode {
            trigger: Box::new(NodeId::INVALID),
            content: Box::new(NodeId::INVALID),
            on_close: popover.on_close.clone(),
            open: popover.open,
            scope: popover.scope,
            capture_focus: popover.capture_focus,
            auto_focus: popover.auto_focus,
        });
        std::mem::take(&mut node.children)
    };

    let reuse_trigger = old_children.first().copied();
    let reuse_content = old_children.get(1).copied();

    let mut new_children = Vec::with_capacity(2);

    let trigger_id = reconcile_element(
        &mut ReconcileCtx {
            tree,
            epoch,
            focus,
            overlay_state,
        },
        ElementReconcile {
            reuse: reuse_trigger,
            parent: Some(id),
            el: &popover.trigger,
            rect,
        },
    );
    new_children.push(trigger_id);

    let mut content_rect = None;
    if popover.open {
        let trigger_rect = tree.node(trigger_id).rect;
        let bounds = overlay_state.bounds;
        let resolved = resolve_popover_rect(popover, trigger_rect, bounds);

        let content_id = reconcile_element(
            &mut ReconcileCtx {
                tree,
                epoch,
                focus,
                overlay_state,
            },
            ElementReconcile {
                reuse: reuse_content,
                parent: Some(id),
                el: &popover.content,
                rect: resolved,
            },
        );
        new_children.push(content_id);
        if matches!(popover.scope, OverlayScope::Local) {
            content_rect = Some(resolved);
        }
    }

    let trigger_rect = tree.node(new_children[0]).rect;
    let mut popover_rect = trigger_rect;
    if let Some(content_rect) = content_rect {
        let x1 = trigger_rect.x.min(content_rect.x);
        let y1 = trigger_rect.y.min(content_rect.y);
        let x2 = (trigger_rect.x.saturating_add(trigger_rect.w as i16))
            .max(content_rect.x.saturating_add(content_rect.w as i16));
        let y2 = (trigger_rect.y.saturating_add(trigger_rect.h as i16))
            .max(content_rect.y.saturating_add(content_rect.h as i16));
        popover_rect = Rect {
            x: x1,
            y: y1,
            w: (x2.saturating_sub(x1)) as u16,
            h: (y2.saturating_sub(y1)) as u16,
        };
    }

    let node = tree.node_mut(id);
    node.children = new_children.clone();
    if let NodeKind::Popover(popover_node) = &mut node.kind {
        *popover_node.trigger = new_children[0];
        if popover.open && new_children.len() > 1 {
            *popover_node.content = new_children[1];
        }
    }

    node.rect = popover_rect;

    id
}
