use crate::core::component::Component;
use crate::core::event::{KeyMods, MouseButton, MouseDragEvent, MouseEvent, MouseKind};
use crate::core::node::NodeKind;

use super::MouseDispatchCtx;

pub(crate) fn mouse_region_has_drag_callbacks(
    actions: &crate::app::input::mouse::HitActions,
) -> bool {
    actions.on_drag_start.is_some() || actions.on_drag.is_some() || actions.on_drag_end.is_some()
}

pub(crate) fn transition_drag_threshold<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    mouse: &MouseEvent,
    x: u16,
    y: u16,
) {
    let is_down = matches!(
        mouse.kind,
        MouseKind::Down(MouseButton::Left | MouseButton::Right)
    );
    let is_drag = matches!(
        mouse.kind,
        MouseKind::Drag(MouseButton::Left | MouseButton::Right)
    );

    if is_down {
        let state = ctx.mouse_state();
        state.left_down_pos = Some((x, y));
        state.drag_threshold_exceeded = false;
        state.click_consumed = false;
        state.pending_drag_source = None;
        state.mouse_region_drag = None;
        state.pan_view_drag = None;
    } else if is_drag {
        let threshold_hit = ctx
            .mouse_state()
            .left_down_pos
            .is_some_and(|(dx, dy)| x.abs_diff(dx) >= 3 || y.abs_diff(dy) >= 1);
        if threshold_hit {
            ctx.mouse_state().drag_threshold_exceeded = true;
        }
    }
}

pub(crate) fn drag_delta(current: u16, previous: u16) -> i16 {
    (current as i32 - previous as i32).clamp(i16::MIN as i32, i16::MAX as i32) as i16
}

#[cfg(feature = "image")]
fn suspend_image_rendering_for_pan() {
    crate::backend::ratatui_backend::image_support::suspend_image_rendering_for(
        std::time::Duration::from_millis(120),
    );
}

pub(crate) fn mouse_region_drag_event(
    tree: &crate::core::node::NodeTree,
    state: crate::app::interaction_state::MouseRegionDragState,
    x: u16,
    y: u16,
    mods: KeyMods,
) -> Option<MouseDragEvent> {
    if !tree.is_valid(state.node_id) {
        return None;
    }
    let node = tree.node(state.node_id);
    let NodeKind::MouseRegion(region) = &node.kind else {
        return None;
    };
    if !region.enabled {
        return None;
    }

    let local_x = ((x as i32) - (node.rect.x as i32)).max(0) as u16;
    let local_y = ((y as i32) - (node.rect.y as i32)).max(0) as u16;
    Some(MouseDragEvent {
        from_x: state.origin.0,
        from_y: state.origin.1,
        from_local_x: state.origin_local.0,
        from_local_y: state.origin_local.1,
        x,
        y,
        local_x,
        local_y,
        delta_x: drag_delta(x, state.last_pos.0),
        delta_y: drag_delta(y, state.last_pos.1),
        target_w: node.rect.w,
        target_h: node.rect.h,
        mods,
    })
}

pub(crate) fn mouse_region_local_position(
    tree: &crate::core::node::NodeTree,
    node_id: crate::core::node::NodeId,
    x: u16,
    y: u16,
) -> Option<(u16, u16)> {
    if !tree.is_valid(node_id) {
        return None;
    }
    let node = tree.node(node_id);
    if !matches!(node.kind, NodeKind::MouseRegion(_)) {
        return None;
    }
    Some((
        ((x as i32) - (node.rect.x as i32)).max(0) as u16,
        ((y as i32) - (node.rect.y as i32)).max(0) as u16,
    ))
}

type MouseDragCallbacks = (
    Option<crate::callback::Callback<MouseDragEvent>>,
    Option<crate::callback::Callback<MouseDragEvent>>,
    Option<crate::callback::Callback<MouseDragEvent>>,
);

fn mods_satisfy_required(required: Option<KeyMods>, actual: KeyMods) -> bool {
    required.is_none_or(|required| {
        (!required.ctrl || actual.ctrl)
            && (!required.alt || actual.alt)
            && (!required.shift || actual.shift)
            && (!required.super_key || actual.super_key)
    })
}

fn drag_required_mods_for(
    region: &crate::widgets::internal::MouseRegionNode,
    button: MouseButton,
) -> Option<KeyMods> {
    match button {
        MouseButton::Left => region.drag_required_mods,
        MouseButton::Right => region.right_drag_required_mods,
        MouseButton::Middle => None,
    }
}

pub(crate) fn mouse_region_drag_target_accepts_mods(
    tree: &crate::core::node::NodeTree,
    node_id: crate::core::node::NodeId,
    button: MouseButton,
    mods: KeyMods,
) -> bool {
    if !tree.is_valid(node_id) {
        return false;
    }
    let node = tree.node(node_id);
    let NodeKind::MouseRegion(region) = &node.kind else {
        return false;
    };
    region.enabled && mods_satisfy_required(drag_required_mods_for(region, button), mods)
}

pub(crate) fn mouse_region_drag_callbacks(
    tree: &crate::core::node::NodeTree,
    node_id: crate::core::node::NodeId,
    button: MouseButton,
) -> Option<MouseDragCallbacks> {
    if !tree.is_valid(node_id) {
        return None;
    }
    let node = tree.node(node_id);
    let NodeKind::MouseRegion(region) = &node.kind else {
        return None;
    };
    if !region.enabled {
        return None;
    }
    Some(match button {
        MouseButton::Left => (
            region.on_drag_start.clone(),
            region.on_drag.clone(),
            region.on_drag_end.clone(),
        ),
        MouseButton::Right => (
            region.on_right_drag_start.clone(),
            region.on_right_drag.clone(),
            region.on_right_drag_end.clone(),
        ),
        MouseButton::Middle => (None, None, None),
    })
}

pub(crate) fn find_ancestor_mouse_region_drag_target(
    tree: &crate::core::node::NodeTree,
    start: crate::core::node::NodeId,
    button: MouseButton,
    mods: KeyMods,
    allow_ungated_ancestors: bool,
) -> Option<crate::core::node::NodeId> {
    let mut current = Some(start);
    let mut is_start = true;
    while let Some(id) = current {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if let NodeKind::MouseRegion(region) = &node.kind
            && region.enabled
        {
            let has_callbacks = match button {
                MouseButton::Left => {
                    region.on_drag_start.is_some()
                        || region.on_drag.is_some()
                        || region.on_drag_end.is_some()
                }
                MouseButton::Right => {
                    region.on_right_drag_start.is_some()
                        || region.on_right_drag.is_some()
                        || region.on_right_drag_end.is_some()
                }
                MouseButton::Middle => false,
            };
            let required_mods = drag_required_mods_for(region, button);
            if has_callbacks
                && mods_satisfy_required(required_mods, mods)
                && (is_start || required_mods.is_some() || allow_ungated_ancestors)
            {
                return Some(id);
            }
        }
        is_start = false;
        current = node.parent;
    }
    None
}

pub(crate) fn clear_mouse_region_drag<C: Component, T: MouseDispatchCtx<C>>(ctx: &mut T) {
    let state = ctx.mouse_state();
    state.mouse_region_drag = None;
    state.left_down_node = None;
    state.left_down_pos = None;
    state.pending_drag_source = None;
}

pub(crate) fn nearest_pan_view(
    tree: &crate::core::node::NodeTree,
    start: crate::core::node::NodeId,
) -> Option<crate::core::node::NodeId> {
    let mut cur = Some(start);
    while let Some(id) = cur {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if let NodeKind::PanView(pan) = &node.kind
            && pan.drag_to_pan
        {
            return Some(id);
        }
        cur = node.parent;
    }
    None
}

pub(crate) fn pan_view_drag_step<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    node_id: crate::core::node::NodeId,
    delta_x: i16,
    delta_y: i16,
) -> bool {
    let (next, offset, metrics, on_pan, state_key) = {
        if !ctx.tree().is_valid(node_id) {
            return false;
        }
        let node = ctx.tree().node(node_id);
        let NodeKind::PanView(pan) = &node.kind else {
            return false;
        };
        let metrics = crate::widgets::internal::pan_metrics(
            pan.content_w,
            pan.content_h,
            pan.viewport_w,
            pan.viewport_h,
        );
        let offset = (pan.offset_x, pan.offset_y);
        let next = crate::widgets::internal::apply_pan_delta(
            offset,
            -delta_x,
            -delta_y,
            metrics,
            pan.clamp,
            pan.free_pan_margin,
        );
        (
            next,
            offset,
            metrics,
            pan.on_pan.clone(),
            pan.state_key.clone().or_else(|| node.key.clone()),
        )
    };

    if next == offset {
        return false;
    }

    if let NodeKind::PanView(pan) = &mut ctx.tree_mut().node_mut(node_id).kind {
        pan.offset_x = next.0;
        pan.offset_y = next.1;
        pan.input_override = Some(next);
        pan.input_dirty = true;
    }
    shift_pan_view_children(ctx.tree_mut(), node_id, offset, next);
    if let Some(key) = state_key {
        ctx.tree_mut().pan_input_offset_by_key.insert(key, next);
    }
    if let Some(cb) = on_pan.as_ref() {
        cb.emit(crate::widgets::PanEvent {
            x: next.0,
            y: next.1,
            metrics,
        });
    }
    #[cfg(feature = "image")]
    suspend_image_rendering_for_pan();
    true
}

pub(crate) fn shift_pan_view_children(
    tree: &mut crate::core::node::NodeTree,
    pan_id: crate::core::node::NodeId,
    old_offset: (i32, i32),
    new_offset: (i32, i32),
) {
    let dx = old_offset.0.saturating_sub(new_offset.0);
    let dy = old_offset.1.saturating_sub(new_offset.1);
    if dx == 0 && dy == 0 {
        return;
    }

    let children = tree.node(pan_id).children.clone();
    for child in children {
        shift_subtree_rect(tree, child, dx, dy);
    }
}

pub(crate) fn shift_subtree_rect(
    tree: &mut crate::core::node::NodeTree,
    id: crate::core::node::NodeId,
    dx: i32,
    dy: i32,
) {
    if !tree.is_valid(id) {
        return;
    }
    let children = {
        let node = tree.node_mut(id);
        node.rect.x = (i32::from(node.rect.x) + dx).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        node.rect.y = (i32::from(node.rect.y) + dy).clamp(i16::MIN as i32, i16::MAX as i32) as i16;
        node.children.clone()
    };
    for child in children {
        shift_subtree_rect(tree, child, dx, dy);
    }
}

pub(crate) fn transition_pan_view_drag<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    mouse: &MouseEvent,
    x: u16,
    y: u16,
) -> Option<bool> {
    match mouse.kind {
        MouseKind::Drag(MouseButton::Left) => {
            let state = ctx.mouse_state().pan_view_drag?;
            if !ctx.mouse_state().drag_threshold_exceeded {
                return None;
            }
            let delta_x = drag_delta(x, state.last_pos.0);
            let delta_y = drag_delta(y, state.last_pos.1);
            let pan_dirty = pan_view_drag_step(ctx, state.node_id, delta_x, delta_y);
            let hover_dirty = ctx.update_hover_impl(x, y, true);
            ctx.mouse_state().pan_view_drag =
                Some(crate::app::interaction_state::PanViewDragState {
                    last_pos: (x, y),
                    started: true,
                    ..state
                });
            Some(pan_dirty || hover_dirty)
        }
        MouseKind::Up(MouseButton::Left) => {
            let state = ctx.mouse_state().pan_view_drag?;
            ctx.mouse_state().pan_view_drag = None;
            state.started.then_some(true)
        }
        _ => None,
    }
}

pub(crate) fn transition_mouse_region_drag<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    mouse: &MouseEvent,
    x: u16,
    y: u16,
) -> Option<bool> {
    match mouse.kind {
        MouseKind::Drag(button @ (MouseButton::Left | MouseButton::Right)) => {
            let state = ctx.mouse_state().mouse_region_drag?;
            if state.button != button {
                return None;
            }
            if !ctx.mouse_state().drag_threshold_exceeded {
                return None;
            }

            let Some((on_drag_start, on_drag, _)) =
                mouse_region_drag_callbacks(ctx.tree(), state.node_id, state.button)
            else {
                ctx.mouse_state().mouse_region_drag = None;
                return None;
            };
            let Some(event) = mouse_region_drag_event(ctx.tree(), state, x, y, mouse.mods) else {
                ctx.mouse_state().mouse_region_drag = None;
                return None;
            };

            if !state.started
                && let Some(cb) = on_drag_start
            {
                cb.emit(event);
            }
            if let Some(cb) = on_drag {
                cb.emit(event);
            }
            ctx.mouse_state().mouse_region_drag =
                Some(crate::app::interaction_state::MouseRegionDragState {
                    last_pos: (x, y),
                    started: true,
                    ..state
                });
            Some(true)
        }
        MouseKind::Up(button @ (MouseButton::Left | MouseButton::Right)) => {
            let state = ctx.mouse_state().mouse_region_drag?;
            if state.button != button {
                return None;
            }
            if state.started {
                if let Some((_, _, Some(cb))) =
                    mouse_region_drag_callbacks(ctx.tree(), state.node_id, state.button)
                    && let Some(event) =
                        mouse_region_drag_event(ctx.tree(), state, x, y, mouse.mods)
                {
                    cb.emit(event);
                }
                clear_mouse_region_drag(ctx);
                Some(true)
            } else {
                ctx.mouse_state().mouse_region_drag = None;
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn transition_drag_move<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    adjusted_mouse: MouseEvent,
    x: u16,
    y: u16,
) -> Option<bool> {
    if matches!(adjusted_mouse.kind, MouseKind::Drag(_))
        && !ctx.drag_is_active()
        && ctx.mouse_state().pending_drag_source.is_none()
    {
        if ctx.forward_terminal_mouse(adjusted_mouse) {
            return Some(true);
        }

        let move_dirty = ctx.dispatch_mouse_move(MouseEvent {
            kind: MouseKind::Moved,
            ..adjusted_mouse
        });
        let hover_dirty = ctx.update_hover(x, y);
        return Some(move_dirty || hover_dirty);
    }
    None
}

pub(crate) fn transition_active_drag<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    mouse: &MouseEvent,
    x: u16,
    y: u16,
) -> Option<bool> {
    if let MouseKind::Drag(MouseButton::Left) = mouse.kind
        && let Some(result) = ctx.dispatch_active_drag(x, y)
    {
        return Some(result || ctx.drag_state().autoscroll_layout_dirty);
    }
    None
}

pub(crate) fn transition_drag_release<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    mouse: &MouseEvent,
    x: u16,
    y: u16,
) -> (bool, bool) {
    let is_up = matches!(mouse.kind, MouseKind::Up(MouseButton::Left));
    let mut drag_dirty = false;
    let mut drag_consumed_up = false;

    if is_up {
        if let Some(result) = ctx.handle_drag_release(x, y) {
            drag_dirty = result;
            if ctx.mouse_state().drag_threshold_exceeded {
                drag_consumed_up = true;
            }
        }
        ctx.mouse_state().pending_drag_source = None;
        ctx.drag_state().autoscroll_layout_dirty = false;
    }

    (drag_dirty, drag_consumed_up)
}
