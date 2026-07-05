use crate::callback::Callback;
use crate::core::event::{KeyMods, MouseMoveEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};

pub(crate) struct MouseMoveAction {
    pub cb: Callback<MouseMoveEvent>,
    pub event: MouseMoveEvent,
}

pub(crate) fn gather_mouse_move_action(
    tree: &NodeTree,
    id: NodeId,
    x: u16,
    y: u16,
    mods: KeyMods,
) -> Option<MouseMoveAction> {
    if !tree.is_valid(id) {
        return None;
    }
    let node = tree.node(id);

    match &node.kind {
        NodeKind::MouseRegion(region) if region.enabled => {
            let cb = region.on_mouse_move.clone()?;
            let local_x = ((x as i32) - (node.rect.x as i32)).max(0) as u16;
            let local_y = ((y as i32) - (node.rect.y as i32)).max(0) as u16;
            Some(MouseMoveAction {
                cb,
                event: MouseMoveEvent {
                    x,
                    y,
                    local_x,
                    local_y,
                    target_w: node.rect.w,
                    target_h: node.rect.h,
                    mods,
                },
            })
        }
        _ => None,
    }
}
