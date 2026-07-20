use crate::core::element::Key;
use crate::core::node::{NodeId, NodeKind};
use crate::layout::measure::min_size_constrained;
use crate::layout::reconcile::state::resolve_rect_with_auto;
use crate::layout::reconcile::{ElementReconcile, ReconcileCtx, reconcile_element};
use crate::style::{LayoutConstraints, Rect};
use crate::widgets::PanView;

use super::{bound_pan_offset, pan_metrics};

pub(crate) struct PanViewReconcile<'a> {
    pub id: NodeId,
    pub pan: &'a PanView,
    pub pan_key: Option<Key>,
    pub rect: Rect,
    pub constraints: &'a LayoutConstraints,
}

pub(crate) fn reconcile_pan_view(ctx: &mut ReconcileCtx<'_>, args: PanViewReconcile<'_>) -> NodeId {
    let PanViewReconcile {
        id,
        pan,
        pan_key,
        rect,
        constraints,
    } = args;
    let child_size = pan
        .child
        .as_deref()
        .map(|child| min_size_constrained(child, None, None))
        .unwrap_or((0, 0));
    let rect = resolve_rect_with_auto(
        rect,
        constraints,
        pan.width,
        pan.height,
        child_size.0,
        child_size.1,
    );
    let metrics = pan_metrics(child_size.0, child_size.1, rect.w, rect.h);

    let (old_override, old_children) = if let NodeKind::PanView(node) = &ctx.tree.node(id).kind {
        (node.input_override, ctx.tree.node(id).children.clone())
    } else {
        (None, Vec::new())
    };

    let remembered = pan_key
        .as_ref()
        .and_then(|key| ctx.tree.pan_input_offset_by_key.get(key).copied());
    let centered_offset = || {
        (
            (i32::from(child_size.0) - i32::from(rect.w)) / 2,
            (i32::from(child_size.1) - i32::from(rect.h)) / 2,
        )
    };
    let raw_offset = pan
        .offset
        .or(old_override)
        .or(remembered)
        .unwrap_or_else(|| {
            if pan.center_content {
                centered_offset()
            } else {
                (0, 0)
            }
        });
    let offset = bound_pan_offset(raw_offset, metrics, pan.clamp, pan.free_pan_margin);

    {
        let node = ctx.tree.node_mut(id);
        node.rect = rect;
        node.children.clear();
        node.kind = NodeKind::PanView(crate::widgets::internal::PanViewNode {
            offset_x: offset.0,
            offset_y: offset.1,
            content_w: child_size.0,
            content_h: child_size.1,
            viewport_w: rect.w,
            viewport_h: rect.h,
            keymap: pan.keymap,
            clamp: pan.clamp,
            free_pan_margin: pan.free_pan_margin,
            drag_to_pan: pan.drag_to_pan,
            wheel_to_pan: pan.wheel_to_pan,
            focusable: pan.focusable,
            tab_stop: pan.tab_stop,
            on_focus: pan.on_focus.clone(),
            on_blur: pan.on_blur.clone(),
            key_step: pan.key_step,
            input_override: pan.offset.or(old_override),
            input_dirty: false,
            state_key: pan_key.clone(),
            on_pan: pan.on_pan.clone(),
        });
        ctx.tree.note_kind_set(id);
    }

    if let Some(child) = pan.child.as_deref() {
        let reuse = old_children.first().copied();
        let child_rect = Rect {
            x: (i32::from(rect.x) - offset.0).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            y: (i32::from(rect.y) - offset.1).clamp(i16::MIN as i32, i16::MAX as i32) as i16,
            w: child_size.0,
            h: child_size.1,
        };
        let child_id = reconcile_element(
            ctx,
            ElementReconcile {
                reuse,
                parent: Some(id),
                el: child,
                rect: child_rect,
            },
        );
        ctx.tree.node_mut(id).children = vec![child_id];
    }

    if let Some(key) = pan_key {
        ctx.tree.pan_input_offset_by_key.insert(key, offset);
    }

    id
}
