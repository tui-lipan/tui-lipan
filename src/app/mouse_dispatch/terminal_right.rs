use crate::app::input::mouse::{ancestor_mouse_region_captures_mods, mods_contain};
use crate::app::runner::events::{
    TerminalMouseForward, terminal_mouse_cell_size, terminal_mouse_forward_plan,
    terminal_mouse_forward_plan_at,
};
use crate::callback::Callback;
use crate::core::component::Component;
use crate::core::element::Key;
use crate::core::event::{MouseButton, MouseEvent, MouseKind};
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::style::Rect;
use crate::widgets::internal::{TerminalNode, terminal_mouse_content_rect};
use crate::widgets::{
    ForwardedMouseEvent, MouseEncoding, MouseMode, TerminalCellSize, TerminalContextMenuPolicy,
    TerminalScreenHandle,
};

use super::MouseDispatchCtx;

#[derive(Clone)]
pub(crate) struct TerminalRightPress {
    node: NodeId,
    target: MouseInputTarget,
    mode: MouseMode,
}

#[derive(Clone)]
enum MouseInputTarget {
    Explicit(Key),
    Screen(TerminalScreenHandle),
    SnapshotCallback(Callback<ForwardedMouseEvent>),
}

impl MouseInputTarget {
    fn capture(terminal: &TerminalNode, callback: &Callback<ForwardedMouseEvent>) -> Self {
        if let Some(key) = &terminal.mouse_input_target {
            Self::Explicit(key.clone())
        } else if let Some(screen) = &terminal.screen {
            Self::Screen(screen.clone())
        } else {
            Self::SnapshotCallback(callback.clone())
        }
    }

    fn matches(&self, terminal: &TerminalNode) -> bool {
        if let Some(key) = &terminal.mouse_input_target {
            return matches!(self, Self::Explicit(captured) if captured == key);
        }
        if let Some(screen) = &terminal.screen {
            return matches!(self, Self::Screen(captured) if captured == screen);
        }
        matches!(self, Self::SnapshotCallback(captured) if terminal.on_mouse_forward.as_ref() == Some(captured))
    }
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

    if !tracking || ancestor_capture || mouse.mods.shift {
        return RightPressRoute::Fallback;
    }

    // The child owns the press even if its wire encoding cannot represent this cell.
    let sub_cell = ctx.mouse_state().sub_cell.get();
    if let Some(plan) = terminal_mouse_forward_plan_at(ctx.tree(), hit, mouse, sub_cell) {
        capture_press(ctx, plan);
    }
    RightPressRoute::Handled
}

fn terminal_content_hit(tree: &NodeTree, hit: NodeId, mouse: MouseEvent) -> Option<&TerminalNode> {
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
        target: MouseInputTarget::capture(terminal, &plan.callback),
        mode: plan.event.mouse_mode.mode,
    });
    ctx.focus_for_node(plan.hit);
    plan.emit(1);
}

pub(super) fn consume_terminal_right_gesture<C: Component, T: MouseDispatchCtx<C>>(
    ctx: &mut T,
    mouse: MouseEvent,
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
    if let Some(plan) = captured_report(ctx.tree(), &owner, mouse, sub_cell) {
        plan.emit(1);
    }
    true
}

fn captured_report(
    tree: &NodeTree,
    owner: &TerminalRightPress,
    mouse: MouseEvent,
    sub_cell: Option<(u16, u16)>,
) -> Option<TerminalMouseForward> {
    if !owner.mode.allows(mouse.kind) || !tree.is_valid(owner.node) {
        return None;
    }
    let NodeKind::Terminal(terminal) = &tree.node(owner.node).kind else {
        return None;
    };
    if !owner.target.matches(terminal) {
        return None;
    }
    let rect = terminal_mouse_content_rect(tree, owner.node).filter(|rect| !rect.is_empty())?;
    let cell_size = terminal_mouse_cell_size(terminal);
    let pixel_mode = terminal.input_mouse_mode().encoding == MouseEncoding::SgrPixels;
    let (mouse, sub_cell) = clamp_pointer(mouse, sub_cell, rect, cell_size, pixel_mode);
    terminal_mouse_forward_plan_at(tree, owner.node, mouse, sub_cell)
}

fn clamp_pointer(
    mouse: MouseEvent,
    sub_cell: Option<(u16, u16)>,
    rect: Rect,
    cell_size: TerminalCellSize,
    pixel_mode: bool,
) -> (MouseEvent, Option<(u16, u16)>) {
    let (sub_x, sub_y) = sub_cell.unwrap_or_default();
    let (x, sub_x) = clamp_axis(mouse.x, sub_x, rect.x, rect.w, cell_size.width);
    let (y, sub_y) = clamp_axis(mouse.y, sub_y, rect.y, rect.h, cell_size.height);
    let report_pixels = sub_cell.is_some() || (pixel_mode && (x, y) != (mouse.x, mouse.y));
    (
        MouseEvent { x, y, ..mouse },
        report_pixels.then_some((sub_x, sub_y)),
    )
}

fn clamp_axis(cell: u16, offset: u16, origin: i16, length: u16, size: u16) -> (u16, u16) {
    let first = i32::from(origin.max(0));
    let last = (i32::from(origin) + i32::from(length) - 1).clamp(first, i32::from(i16::MAX));
    if i32::from(cell) < first {
        (first as u16, 0)
    } else if i32::from(cell) > last {
        (last as u16, size.saturating_sub(1))
    } else {
        (cell, offset)
    }
}
