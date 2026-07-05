use crate::app::input::mouse;
use crate::core::component::Component;
use crate::core::event::{MouseEvent, MouseKind};

use super::MouseDispatchCtx;

pub(crate) fn transition_mouse_move<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    adjusted_mouse: MouseEvent,
    x: u16,
    y: u16,
) -> Option<bool> {
    if matches!(adjusted_mouse.kind, MouseKind::Moved) {
        if ctx.forward_terminal_mouse(adjusted_mouse) {
            // Forwarded motion only writes bytes to the PTY; nothing in our
            // own tree changed, so a repaint per motion event is pure waste.
            return Some(false);
        }
        let move_dirty = ctx.dispatch_mouse_move(adjusted_mouse);
        let hover_dirty = ctx.update_hover(x, y);
        return Some(move_dirty || hover_dirty);
    }
    None
}

pub(crate) fn transition_scroll_wheel<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    adjusted_mouse: MouseEvent,
    x: u16,
    y: u16,
) -> Option<bool> {
    if matches!(
        adjusted_mouse.kind,
        MouseKind::ScrollUp | MouseKind::ScrollDown
    ) {
        if ctx.forward_terminal_mouse(adjusted_mouse) {
            return Some(true);
        }

        let fallback_multiplier = ctx.scroll_wheel_multiplier();
        let scroll_dirty =
            mouse::handle_scroll_wheel_n(ctx.tree_mut(), adjusted_mouse, 1, fallback_multiplier);
        let selection_dirty = if scroll_dirty && ctx.drag_is_active() {
            ctx.drag_state().remember_pointer(x, y);
            ctx.refresh_active_selection_drag_after_scroll(x, y)
        } else {
            false
        };
        let hover_dirty = ctx.update_hover_impl(x, y, true);
        return Some(hover_dirty || scroll_dirty || selection_dirty);
    }
    None
}
