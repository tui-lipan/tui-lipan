#[cfg(feature = "terminal")]
use std::sync::Arc;

use crate::Result;
use crate::core::component::Component;
#[cfg(feature = "terminal")]
use crate::core::node::{NodeId, NodeKind};
#[cfg(feature = "terminal")]
use crate::widgets::{TerminalInputEvent, TerminalInputKind, focus_sequences};

use super::AppRunner;

impl<C: Component> AppRunner<C> {
    pub(super) fn sync_mouse_capture_preference(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> Result<bool> {
        let desired = self.mouse_capture_requested.get();
        if desired == self.mouse_enabled {
            return Ok(false);
        }

        self.mouse_enabled = desired;
        self.sync_mouse_capture_enabled(terminal)?;

        if !desired {
            self.drag.clear();
            self.mouse.hovered = None;
            self.mouse.hovered_item_index = None;
            self.mouse.last_mouse = None;
        }

        Ok(true)
    }

    pub(super) fn sync_mouse_capture_enabled(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> Result<()> {
        if self.mouse_capture_active == self.mouse_enabled {
            return Ok(());
        }

        crate::backend::ratatui_backend::set_mouse_capture_enabled(
            terminal.backend_mut(),
            self.mouse_enabled,
        )?;
        self.mouse_capture_active = self.mouse_enabled;

        if !self.mouse_capture_active {
            self.mouse_all_motion_enabled = false;
        }

        Ok(())
    }

    pub(super) fn needs_mouse_motion(&self) -> bool {
        if !self.mouse_enabled || !self.mouse_capture_active {
            return false;
        }

        self.core.tree.has_hoverables()
            || self.core.tree.has_mouse_move_handlers()
            || self.core.tree.has_terminal_any_event()
    }

    pub(super) fn sync_mouse_motion_capture(
        &mut self,
        terminal: &mut crate::backend::ratatui_backend::Terminal,
    ) -> Result<()> {
        if !self.mouse_enabled {
            if self.mouse_all_motion_enabled {
                crate::backend::ratatui_backend::set_mouse_all_motion_enabled(
                    terminal.backend_mut(),
                    false,
                )?;
                self.mouse_all_motion_enabled = false;
            }
            return Ok(());
        }

        let needed = self.needs_mouse_motion();
        if needed == self.mouse_all_motion_enabled {
            return Ok(());
        }

        crate::backend::ratatui_backend::set_mouse_all_motion_enabled(
            terminal.backend_mut(),
            needed,
        )?;
        self.mouse_all_motion_enabled = needed;
        Ok(())
    }

    #[cfg(feature = "terminal")]
    pub(super) fn emit_terminal_focus_change(&mut self) {
        let prev_focus = self.focus.last_emitted_focus;
        let prev_window = self.focus.last_emitted_window_focused;
        let current_focus = self.focus.focused;
        let current_window = self.focus.window_focused;

        if prev_focus == current_focus && prev_window == current_window {
            return;
        }

        let prev_terminal = self.terminal_focus_id(prev_focus, prev_window);
        let next_terminal = self.terminal_focus_id(current_focus, current_window);

        if prev_terminal != next_terminal {
            if let Some(id) = prev_terminal {
                self.emit_terminal_focus_sequence(id, false);
            }
            if let Some(id) = next_terminal {
                self.emit_terminal_focus_sequence(id, true);
            }
        }

        self.focus.last_emitted_focus = current_focus;
        self.focus.last_emitted_window_focused = current_window;
    }

    #[cfg(feature = "terminal")]
    fn terminal_focus_id(&self, focus: Option<NodeId>, window_focused: bool) -> Option<NodeId> {
        if !window_focused {
            return None;
        }
        let id = focus?;
        if !self.core.tree.is_valid(id) {
            return None;
        }
        match self.core.tree.node(id).kind {
            NodeKind::Terminal(_) => Some(id),
            _ => None,
        }
    }

    #[cfg(feature = "terminal")]
    fn emit_terminal_focus_sequence(&self, id: NodeId, focused: bool) {
        if !self.core.tree.is_valid(id) {
            return;
        }
        let NodeKind::Terminal(node) = &self.core.tree.node(id).kind else {
            return;
        };
        // Only send focus events if the PTY application has requested them
        // via CSI ? 1004 h (ReportFocusInOut mode).
        if !node.mouse_mode.focus_events_enabled {
            return;
        }
        let Some(cb) = node.on_input.as_ref() else {
            return;
        };

        let (focus_in, focus_out) = focus_sequences();
        let (kind, bytes) = if focused {
            (TerminalInputKind::FocusIn, focus_in)
        } else {
            (TerminalInputKind::FocusOut, focus_out)
        };

        cb.emit(TerminalInputEvent {
            kind,
            key: None,
            bytes: Arc::<[u8]>::from(bytes),
        });
    }
}
