use crate::app::input::mouse;
use crate::core::component::Component;
use crate::core::event::{MouseButton, MouseEvent, MouseKind};
use crate::core::node::NodeKind;

use super::{
    MouseDispatchCtx, find_ancestor_mouse_region_drag_target, mouse_region_local_position,
};

pub(crate) fn transition_overlay_click<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    mouse: &MouseEvent,
    x: u16,
    y: u16,
) -> Option<bool> {
    if let MouseKind::Down(button @ (MouseButton::Left | MouseButton::Right)) = mouse.kind
        && ctx.handle_overlay_click(button, x, y)
    {
        return Some(true);
    }
    None
}

pub(crate) fn transition_right_click<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    adjusted_mouse: MouseEvent,
    mouse: &MouseEvent,
    x: u16,
    y: u16,
    hover_dirty: bool,
) -> Option<bool> {
    if matches!(mouse.kind, MouseKind::Down(MouseButton::Right)) {
        // A fresh button-down always begins a new interaction. Drop any in-flight
        // drag so a previous session whose button-up was lost (e.g. release outside
        // the terminal, focus loss mid-drag) cannot linger and swallow this one.
        ctx.mouse_state().mouse_region_drag = None;

        let Some(hit) = ctx.tree().hit_test(x as i16, y as i16) else {
            return Some(hover_dirty);
        };

        // Right-drag bubbles to any enabled ancestor mouse region
        // (`allow_ungated_ancestors = true`), unlike left-drag which only bubbles to
        // ancestors that gate behind a modifier. This lets right-drag-to-resize work
        // anywhere inside a window without an explicit modifier, while keeping plain
        // left-drag from being stolen by an enclosing region.
        if let Some(target) = find_ancestor_mouse_region_drag_target(
            ctx.tree(),
            hit,
            MouseButton::Right,
            mouse.mods,
            true,
        ) {
            let origin_local =
                mouse_region_local_position(ctx.tree(), target, x, y).unwrap_or((0, 0));
            ctx.mouse_state().mouse_region_drag =
                Some(crate::app::interaction_state::MouseRegionDragState {
                    node_id: target,
                    button: MouseButton::Right,
                    origin: (x, y),
                    origin_local,
                    last_pos: (x, y),
                    started: false,
                });
        }

        if ctx.forward_terminal_mouse(adjusted_mouse) {
            return Some(true);
        }

        if ctx.handle_right_click_textarea(hit, *mouse) {
            return Some(true);
        }

        return Some(hover_dirty);
    }
    None
}

pub(crate) fn transition_hit_test_and_scrollbar<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    x: u16,
    y: u16,
) -> (
    Option<crate::core::node::NodeId>,
    Option<crate::core::node::ScrollbarTarget>,
) {
    let hit_before_resolve = ctx.tree().hit_test(x as i16, y as i16);
    let scrollbar_target = ctx.tree().scrollbar_target_at(x as i16, y as i16);
    (hit_before_resolve, scrollbar_target)
}

pub(crate) fn transition_selection_clear<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    is_down: bool,
    hit_before_resolve: Option<crate::core::node::NodeId>,
    scrollbar_target: Option<crate::core::node::ScrollbarTarget>,
) -> bool {
    if !is_down {
        return false;
    }
    let keep_selection = hit_before_resolve
        .and_then(|id| ctx.selection_owner_for_node(id))
        .or_else(|| scrollbar_target.and_then(|target| ctx.selection_owner_for_node(target.id)));
    ctx.clear_selectable_widget_selections(keep_selection)
}

pub(crate) struct WidgetDownParams {
    pub hit: crate::core::node::NodeId,
    pub mouse: MouseEvent,
    pub adjusted_mouse: MouseEvent,
    pub actions: mouse::HitActions,
    pub x: u16,
    pub y: u16,
    pub hover_dirty: bool,
}

pub(crate) fn transition_widget_down<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    params: WidgetDownParams,
    dirty: &mut bool,
) -> Option<bool> {
    let WidgetDownParams {
        hit,
        mouse,
        adjusted_mouse,
        actions,
        x,
        y,
        hover_dirty,
    } = params;
    if let Some(cb) = actions.on_mouse_down {
        cb.emit(adjusted_mouse);
    }

    if let Some(change) = actions.slider_change {
        return Some(ctx.handle_slider_click(hit, change, x, y));
    }

    if let Some(change) = actions.progress_change
        && ctx.handle_progress_click(change)
    {
        return Some(true);
    }

    if let Some(action) = actions.draggable_tab_bar_action
        && ctx.handle_draggable_tab_bar_click(action, x, *dirty)
    {
        return Some(true);
    }

    if let Some(grab) = actions.splitter_grab
        && ctx.handle_splitter_click(grab, x, y)
    {
        return Some(true);
    }

    if let Some(select) = actions.list_select
        && ctx.handle_list_click(hit, select, x, y)
    {
        return Some(true);
    }

    if let Some(select) = actions.table_select
        && ctx.handle_table_click(hit, select, x, y)
    {
        return Some(true);
    }

    if ctx.handle_terminal_click(hit, mouse, x, y, hover_dirty) {
        return Some(true);
    }

    if let Some(change) = actions.textarea_change
        && ctx.handle_textarea_click(change, x, y)
    {
        return Some(true);
    }

    let (dv_handled, dv_dirty) = ctx.handle_document_view_click(hit, mouse, x, y);
    if dv_handled {
        return Some(true);
    }
    *dirty |= dv_dirty;

    if let Some(change) = actions.input_change
        && ctx.handle_input_click(change, x)
    {
        return Some(true);
    }

    if ctx.handle_hex_area_click(hit, mouse, x, y) {
        return Some(true);
    }

    None
}

pub(crate) fn emit_bubbling_mouse_down(
    tree: &crate::core::node::NodeTree,
    hit: crate::core::node::NodeId,
    mouse: MouseEvent,
) {
    let mut current = tree.node(hit).parent;
    while let Some(id) = current {
        if !tree.is_valid(id) {
            break;
        }
        let node = tree.node(id);
        if let NodeKind::MouseRegion(region) = &node.kind
            && region.enabled
            && region.bubble_mouse_down
            && let Some(cb) = region.on_mouse_down.clone()
        {
            cb.emit(mouse);
        }
        current = node.parent;
    }
}

pub(crate) fn transition_widget_up<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    hit: crate::core::node::NodeId,
    mouse: MouseEvent,
    actions: mouse::HitActions,
    x: u16,
    y: u16,
) -> Option<bool> {
    if let Some(cb) = actions.on_mouse_up.clone() {
        cb.emit(mouse);
    }

    let down_node = ctx.mouse_state().left_down_node.take();
    ctx.mouse_state().left_down_pos.take();
    if down_node == Some(hit)
        && !ctx.mouse_state().drag_threshold_exceeded
        && !ctx.mouse_state().click_consumed
    {
        if let Some(change) = actions.tabs_change
            && ctx.handle_tabs_click(change)
        {
            return Some(true);
        }

        if let Some(change) = actions.border_tabs_change
            && ctx.handle_tabs_click(change)
        {
            return Some(true);
        }

        if let Some(toggle) = actions.checkbox_toggle {
            return Some(ctx.handle_checkbox_click(toggle));
        }

        #[cfg(feature = "diff-view")]
        if let Some(click) = actions.diff_context_separator_click {
            click.cb.emit(click.event);
            return Some(true);
        }

        if let Some(click) = actions.document_click {
            if click.link.is_none()
                && matches!(&ctx.tree().node(hit).kind, NodeKind::DocumentView(doc) if doc.passthrough_clicks)
                && let Some(cb) = mouse::find_ancestor_on_click(ctx.tree(), hit)
            {
                return Some(ctx.handle_fallback_click(cb, mouse));
            }
            return Some(ctx.handle_document_click(click));
        }

        if let Some(click) = actions.graph_node_click {
            return Some(ctx.handle_graph_node_click(click));
        }

        if let Some(click) = actions.sequence_item_click {
            return Some(ctx.handle_sequence_item_click(click));
        }

        if let Some(click) = actions.flowchart_item_click {
            return Some(ctx.handle_flowchart_item_click(click));
        }

        if let Some(change) = actions.textarea_change.as_ref()
            && mouse::process_textarea_sentinel_click(change, mouse, x, y)
        {
            return Some(true);
        }

        if matches!(&ctx.tree().node(hit).kind, NodeKind::DocumentView(doc) if doc.passthrough_clicks)
            && let Some(cb) = mouse::find_ancestor_on_click(ctx.tree(), hit)
        {
            return Some(ctx.handle_fallback_click(cb, mouse));
        }

        if let Some(cb) = actions.on_click {
            return Some(ctx.handle_fallback_click(cb, mouse));
        }
    }
    None
}
