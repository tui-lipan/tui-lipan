use crate::core::component::FocusContext;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::axis::{Axis, requested_main_axis};
use crate::layout::measure::min_size_constrained;
use crate::layout::reconcile::{
    ElementReconcile, OverlayState, ReconcileCtx, SingleChildReconcile, reconcile_element,
    reconcile_single_child,
};
use crate::layout::tag::can_reuse;
use crate::style::{Length, Rect};

use super::drag_source::DragSource;
use super::drag_source_node::DragSourceNode;

pub(crate) fn reconcile_drag_source(
    tree: &mut NodeTree,
    epoch: u32,
    id: NodeId,
    source: &DragSource,
    rect: Rect,
    focus: Option<&FocusContext>,
    overlay_state: &mut OverlayState,
) -> NodeId {
    let (is_dragging, old_preview_child, mut old_children) = {
        let node = tree.node_mut(id);
        let (is_dragging, old_preview_child) = if let NodeKind::DragSource(existing) = &node.kind {
            (existing.is_dragging, existing.preview_child)
        } else {
            (false, None)
        };
        node.rect = rect;
        node.kind = NodeKind::DragSource(DragSourceNode {
            on_drag_start: source.on_drag_start.clone(),
            on_drag_cancel: source.on_drag_cancel.clone(),
            on_drag_started: source.on_drag_started.clone(),
            drag_group: source.drag_group.clone(),
            preview: source.preview.clone(),
            preview_child: None,
            preview_anchor: source.preview_anchor,
            hide_source_while_dragging: source.hide_source_while_dragging,
            dragging_style: source.dragging_style,
            drag_slot: source.drag_slot,
            preview_max_width: source.preview_max_width,
            preview_max_height: source.preview_max_height,
            threshold: source.threshold,
            enabled: source.enabled,
            is_dragging,
        });
        (
            is_dragging,
            old_preview_child,
            std::mem::take(&mut node.children),
        )
    };

    if let Some(prev_id) = old_preview_child {
        old_children.retain(|&child| child != prev_id);
    }

    let mut reconcile_ctx = ReconcileCtx {
        tree,
        epoch,
        focus,
        overlay_state,
    };
    let mut new_children = reconcile_single_child(
        &mut reconcile_ctx,
        SingleChildReconcile {
            parent_id: id,
            child: source.child.as_deref(),
            rect,
            old_children,
        },
    );

    let preview_child = if is_dragging && let Some(preview_el) = source.preview_element.as_deref() {
        let requested_w = requested_main_axis(preview_el, Axis::Horizontal, None);
        let width_cap = source.preview_max_width.or_else(|| {
            if matches!(requested_w, Length::Auto) {
                None
            } else {
                (rect.w > 0).then_some(rect.w)
            }
        });
        let (mw, mh) = min_size_constrained(preview_el, width_cap, source.preview_max_height);
        let preview_w = match requested_w {
            Length::Px(px) => source.preview_max_width.map_or(px, |max| px.min(max)),
            Length::Auto => source.preview_max_width.map_or(mw, |max| mw.min(max)),
            _ if rect.w > 0 => source
                .preview_max_width
                .map_or(rect.w, |max| rect.w.min(max)),
            _ => mw,
        };
        let preview_h = if mh > 0 { mh } else { rect.h };
        let preview_h = source
            .preview_max_height
            .map_or(preview_h, |max| preview_h.min(max));
        let preview_rect = Rect {
            x: 0,
            y: 0,
            w: preview_w,
            h: preview_h,
        };
        let reuse = old_preview_child.filter(|&prev_id| {
            reconcile_ctx.tree.is_valid(prev_id)
                && can_reuse(reconcile_ctx.tree.node(prev_id), preview_el)
        });
        let preview_id = reconcile_element(
            &mut reconcile_ctx,
            ElementReconcile {
                reuse,
                parent: Some(id),
                el: preview_el,
                rect: preview_rect,
            },
        );
        mark_subtree_inert(reconcile_ctx.tree, preview_id);
        new_children.push(preview_id);
        Some(preview_id)
    } else {
        None
    };

    let node = tree.node_mut(id);
    node.children = new_children;
    if let NodeKind::DragSource(ds) = &mut node.kind {
        ds.preview_child = preview_child;
    }

    id
}

fn mark_subtree_inert(tree: &mut NodeTree, id: NodeId) {
    let mut stack = vec![id];
    while let Some(current) = stack.pop() {
        if !tree.is_valid(current) {
            continue;
        }
        let node = tree.node_mut(current);
        node.inert = true;
        stack.extend(node.children.iter().copied());
    }
}

#[cfg(test)]
mod tests {
    use crate::TestBackend;
    use crate::app::interaction_state::ActiveDrag;
    use crate::core::event::{MouseButton, MouseEvent, MouseKind};
    use crate::core::node::NodeId;
    use crate::prelude::*;
    use crate::style::Rect;

    #[derive(Debug)]
    struct Payload;

    struct App;

    impl Component for App {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            let preview = VStack::new()
                .child(Button::new("preview-btn").key("preview-btn"))
                .child(
                    Popover::new()
                        .open(true)
                        .trigger(Text::new("pop-trigger"))
                        .content(Button::new("portal-btn").key("portal-btn")),
                );
            DragSource::new()
                .child(Text::new("source").height(Length::Px(1)))
                .threshold(1)
                .preview_element(preview)
                .on_drag_start(|_| Some(Box::new(Payload) as Box<dyn DragPayload>))
                .key("src")
        }
    }

    fn subtree(tree: &crate::core::node::NodeTree, root: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![root];
        while let Some(id) = stack.pop() {
            if !tree.is_valid(id) {
                continue;
            }
            let node = tree.node(id);
            out.push(id);
            stack.extend(node.children.iter().copied());
        }
        out
    }

    #[test]
    fn element_preview_marks_nested_focusables_and_portals_inert() {
        let mut backend = TestBackend::new(App);
        backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: 40,
            h: 8,
        });
        backend.render();

        for (kind, x, y) in [
            (MouseKind::Down(MouseButton::Left), 1, 0),
            (MouseKind::Drag(MouseButton::Left), 4, 1),
        ] {
            backend
                .send_mouse(MouseEvent {
                    kind,
                    x,
                    y,
                    mods: KeyMods::NONE,
                })
                .unwrap();
            backend.render();
        }

        let ActiveDrag::DragDrop(drag) = &backend.drag.active else {
            panic!("drag should be active");
        };
        let preview = backend
            .core
            .tree
            .node(drag.source_id)
            .kind
            .drag_preview_child()
            .expect("element preview should be mounted");
        let nodes = subtree(&backend.core.tree, preview);
        assert!(
            nodes.len() > 1,
            "preview should contain the button and popover"
        );
        assert!(
            nodes.iter().all(|id| backend.core.tree.node(*id).inert),
            "every preview descendant must be inert"
        );
        let focused = backend.focused();
        assert!(
            focused.is_none_or(|id| !nodes.contains(&id)),
            "focus must not land inside the preview"
        );
        assert!(
            backend.core.tree.overlay_roots().is_empty(),
            "an open popover inside the preview must not become an overlay"
        );
    }
}
