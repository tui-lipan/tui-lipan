use crate::app::input::mouse::{ancestor_mouse_region_captures_mods, mods_contain};
use crate::app::runner::events::{
    TerminalMouseForward, terminal_mouse_forward_plan, terminal_mouse_forward_plan_at,
};
use crate::core::component::Component;
use crate::core::event::{MouseButton, MouseEvent, MouseKind};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::internal::terminal_mouse_content_rect;
use crate::widgets::{MouseMode, TerminalContextMenuPolicy, TerminalScreenHandle};

use super::MouseDispatchCtx;

#[derive(Clone)]
pub(crate) struct TerminalRightPress {
    node: NodeId,
    screen: Option<TerminalScreenHandle>,
    mode: MouseMode,
}

pub(super) enum RightPressRoute {
    Default,
    Fallback,
    Handled,
}

pub(super) fn application_first_press<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    hit: NodeId,
    mouse: MouseEvent,
) -> RightPressRoute {
    let tree = ctx.tree();
    let Some(terminal) = terminal_content_hit(tree, hit, mouse) else {
        return RightPressRoute::Default;
    };
    let TerminalContextMenuPolicy::ApplicationFirst {
        override_mods,
        open_when_untracked,
    } = terminal.context_menu_policy
    else {
        return RightPressRoute::Default;
    };
    let tracking =
        terminal.on_mouse_forward.is_some() && terminal.input_mouse_mode().mode.allows(mouse.kind);
    let ancestor_capture = ancestor_mouse_region_captures_mods(tree, hit, mouse.mods);
    let force_menu = override_mods.is_some_and(|required| mods_contain(required, mouse.mods));
    let use_menu = force_menu || (!ancestor_capture && !tracking && open_when_untracked);
    if let Some(callback) = &terminal.on_context_menu
        && use_menu
    {
        callback.emit(mouse);
        ctx.mouse_state().context_menu_press_consumed = true;
        return RightPressRoute::Handled;
    }

    if !tracking || ancestor_capture {
        return RightPressRoute::Fallback;
    }

    // The child owns the press even if its wire encoding cannot represent this cell.
    let sub_cell = ctx.mouse_state().sub_cell.get();
    if let Some(plan) = terminal_mouse_forward_plan_at(ctx.tree(), hit, mouse, sub_cell) {
        capture_press(ctx, plan);
    }
    RightPressRoute::Handled
}

fn terminal_content_hit(
    tree: &NodeTree,
    hit: NodeId,
    mouse: MouseEvent,
) -> Option<&crate::widgets::internal::TerminalNode> {
    let NodeKind::Terminal(terminal) = &tree.node(hit).kind else {
        return None;
    };
    terminal_mouse_content_rect(tree, hit)?
        .contains(mouse.x as i16, mouse.y as i16)
        .then_some(terminal)
}

pub(super) fn forward_right_press<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    mouse: MouseEvent,
) -> bool {
    let sub_cell = ctx.mouse_state().sub_cell.get();
    let Some(plan) = terminal_mouse_forward_plan(ctx.tree(), mouse, sub_cell) else {
        return false;
    };
    capture_press(ctx, plan);
    true
}

fn capture_press<C: Component, T: MouseDispatchCtx<C>>(ctx: &mut T, plan: TerminalMouseForward) {
    let NodeKind::Terminal(terminal) = &ctx.tree().node(plan.hit).kind else {
        return;
    };
    ctx.mouse_state().terminal_right_press = Some(TerminalRightPress {
        node: plan.hit,
        screen: terminal.screen.clone(),
        mode: plan.event.mouse_mode.mode,
    });
    ctx.focus_for_node(plan.hit);
    plan.emit(1);
}

pub(super) fn consume_terminal_right_gesture<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    mut mouse: MouseEvent,
) -> bool {
    let state = ctx.mouse_state();
    let owner = match mouse.kind {
        MouseKind::Down(MouseButton::Right) => {
            state.terminal_right_press = None;
            return false;
        }
        MouseKind::Drag(MouseButton::Right) => state.terminal_right_press.clone(),
        MouseKind::Up(MouseButton::Right) => state.terminal_right_press.take(),
        _ => return false,
    };
    let Some(owner) = owner else {
        return false;
    };

    let sub_cell = state.sub_cell.get();
    let tree = ctx.tree();
    if !owner.mode.allows(mouse.kind) || !tree.is_valid(owner.node) {
        return true;
    }
    let NodeKind::Terminal(terminal) = &tree.node(owner.node).kind else {
        return true;
    };
    if terminal.screen != owner.screen {
        return true;
    }
    let Some(rect) =
        terminal_mouse_content_rect(tree, owner.node).filter(|rect| rect.w > 0 && rect.h > 0)
    else {
        return true;
    };

    let x = clamp_cell(mouse.x, rect.x, rect.w);
    let y = clamp_cell(mouse.y, rect.y, rect.h);
    let sub_cell = sub_cell.filter(|_| (x, y) == (mouse.x, mouse.y));
    mouse.x = x;
    mouse.y = y;
    if let Some(plan) = terminal_mouse_forward_plan_at(tree, owner.node, mouse, sub_cell) {
        plan.emit(1);
    }
    true
}

fn clamp_cell(cell: u16, origin: i16, length: u16) -> u16 {
    let first = i32::from(origin.max(0));
    let last = i32::from(origin) + i32::from(length) - 1;
    cell.clamp(first as u16, last.clamp(first, i32::from(i16::MAX)) as u16)
}
