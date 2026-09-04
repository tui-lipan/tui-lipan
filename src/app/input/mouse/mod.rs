//! Mouse event dispatch logic.

mod click;
mod gather;
mod hover;
mod r#move;
mod scroll;
mod types;

pub(crate) use click::{
    click_count_at, process_input_click, process_textarea_click, process_textarea_sentinel_click,
};
#[cfg(feature = "terminal")]
pub(crate) use gather::ancestor_mouse_region_captures_mods;
#[cfg(feature = "diff-view")]
pub(crate) use gather::diff_line_event_at_node;
pub(crate) use gather::{find_ancestor_on_click, gather_hit_actions, resolve_left_click_target};
pub(crate) use hover::{
    clear_mouse_hover_state, emit_mouse_region_hover_changes, mouse_region_accepts_point,
    mouse_region_hover_chains_differ, mouse_region_hover_state,
    mouse_region_hover_transition_affects_paint, should_hover,
};
pub(crate) use r#move::gather_mouse_move_action;
pub(crate) use scroll::handle_scroll_wheel_n;
pub(crate) use types::*;
