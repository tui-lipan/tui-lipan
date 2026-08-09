//! Slider keyboard handler.

use crate::callback::KeyHandler;
use crate::core::event::{KeyCode, KeyEvent};
use crate::core::node::{NodeId, NodeKind, NodeTree};

/// Handle keyboard input for a focused Slider node.
pub(crate) fn handle_key(tree: &NodeTree, id: NodeId, key: KeyEvent) -> bool {
    let node = tree.node(id);
    let NodeKind::Slider(slider) = &node.kind else {
        return false;
    };

    if slider.disabled {
        return false;
    }

    let handle_on_key = |handler: &KeyHandler| -> bool { handler.handle(key) };

    if slider.on_key.as_ref().is_some_and(handle_on_key) {
        return true;
    }

    let delta = match key.code {
        KeyCode::Left | KeyCode::Down => -slider.step,
        KeyCode::Right | KeyCode::Up => slider.step,
        KeyCode::Home => slider.min - slider.value,
        KeyCode::End => slider.max - slider.value,
        KeyCode::Enter | KeyCode::Char(' ') => {
            if let Some(cb) = slider.on_click.as_ref() {
                cb.emit(slider.value);
                return true;
            }
            return false;
        }
        _ => return false,
    };

    let new_value = (slider.value + delta).clamp(slider.min, slider.max);
    if (new_value - slider.value).abs() < f64::EPSILON {
        return false;
    }

    if let Some(cb) = slider.on_change.as_ref() {
        cb.emit(new_value);
        return true;
    }

    false
}
