//! PanView keyboard handler.

use crate::core::event::KeyEvent;
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::widgets::PanEvent;
use crate::widgets::internal::{apply_pan_action, pan_action_from_key, pan_metrics};

#[cfg(feature = "image")]
fn suspend_image_rendering_for_pan() {
    crate::backend::ratatui_backend::image_support::suspend_image_rendering_for(
        std::time::Duration::from_millis(120),
    );
}

pub(crate) fn handle_key(tree: &mut NodeTree, node_id: NodeId, key: &KeyEvent) -> bool {
    let (next, offset, metrics, on_pan, state_key) = {
        let node = tree.node(node_id);
        let NodeKind::PanView(pan) = &node.kind else {
            return false;
        };
        let Some(action) = pan_action_from_key(key, pan.keymap, pan.key_step) else {
            return false;
        };
        let metrics = pan_metrics(pan.content_w, pan.content_h, pan.viewport_w, pan.viewport_h);
        let offset = (pan.offset_x, pan.offset_y);
        let next = apply_pan_action(offset, action, metrics, pan.clamp, pan.free_pan_margin);
        (
            next,
            offset,
            metrics,
            pan.on_pan.clone(),
            pan.state_key.clone().or_else(|| node.key.clone()),
        )
    };

    if next != offset {
        if let NodeKind::PanView(pan) = &mut tree.node_mut(node_id).kind {
            pan.offset_x = next.0;
            pan.offset_y = next.1;
            pan.input_override = Some(next);
            pan.input_dirty = true;
        }
        if let Some(key) = state_key {
            tree.pan_input_offset_by_key.insert(key, next);
        }
        if let Some(cb) = on_pan.as_ref() {
            cb.emit(PanEvent {
                x: next.0,
                y: next.1,
                metrics,
            });
        }
        #[cfg(feature = "image")]
        suspend_image_rendering_for_pan();
    }
    true
}
