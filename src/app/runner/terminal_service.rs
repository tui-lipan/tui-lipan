#[cfg(feature = "terminal")]
use std::sync::Arc;

use crate::Result;
use crate::backend::ratatui_backend::terminal_handoff::{
    resume_after_external_process, suspend_for_external_process,
};
use crate::core::component::Component;
#[cfg(feature = "terminal")]
use crate::core::event::{KeyMods, MouseEvent, MouseKind};
#[cfg(feature = "terminal")]
use crate::core::node::{NodeId, NodeKind};
#[cfg(feature = "terminal")]
use crate::widgets::{TerminalInputEvent, TerminalInputKind, focus_sequences};

use super::AppRunner;

impl<C: Component> AppRunner<C> {
    /// Run a pending suspend request — `Context::suspend_to_shell` or an
    /// external `SIGTSTP` — or take the terminal back after a background stop,
    /// and report whether either happened.
    ///
    /// Called between frames so the terminal is in a known state: hand it back
    /// to the shell, stop until the job is foregrounded, then take it again.
    /// `resume_after_external_process` asks for the full repaint that redraws
    /// over whatever the shell left on screen.
    ///
    /// A background stop (`SIGTTIN`/`SIGTTOU`) has already released the
    /// terminal modes and stopped from its signal handler, without the input
    /// reader or the raw-mode bookkeeping knowing, and closed the frame output
    /// behind it. Running the same release and restore puts both back in step
    /// and re-applies raw mode, which the shell may have changed while it held
    /// the terminal. Frame output reopens only once that worked; until then a
    /// failed attempt stays pending for the next frame boundary.
    pub(super) fn run_pending_suspend(&mut self) -> bool {
        let stop_requested = crate::app::job_control::take_suspend_request();
        let stopped_in_background = crate::app::job_control::take_background_stop();
        if !stop_requested && !stopped_in_background {
            return false;
        }

        let surface_mode = self.surface.mode();
        if let Err(err) = suspend_for_external_process(surface_mode) {
            // The terminal is still ours (the release rolls itself back), so
            // stopping now would strand the shell in raw mode. Skip the stop.
            crate::debug::internal_log!(
                "[tui-lipan] suspend: releasing the terminal failed, staying up: {}",
                err
            );
            crate::app::job_control::retry_background_stop();
            return false;
        }

        if stop_requested {
            crate::app::job_control::stop_until_continued();
        }

        match resume_after_external_process(surface_mode, self.mouse_enabled) {
            Ok(()) => crate::app::job_control::reclaim_terminal(),
            Err(err) => {
                crate::debug::internal_log!("[tui-lipan] suspend: resume failed: {}", err);
                crate::app::job_control::retry_background_stop();
            }
        }
        true
    }

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
            || self.core.tree.has_terminal_link_hover()
    }

    #[cfg(feature = "terminal")]
    pub(super) fn refresh_terminal_link_hover_at_pointer(&mut self, mods: KeyMods) -> bool {
        let Some((x, y)) = self.mouse.last_mouse.get() else {
            return false;
        };
        let previous = self.mouse.terminal_link_hover_node;
        let (next, dirty) = crate::app::input::handlers::terminal::update_link_hover(
            &mut self.core.tree,
            previous,
            MouseEvent {
                x,
                y,
                kind: MouseKind::Moved,
                mods,
            },
        );
        self.mouse.terminal_link_hover_node = next;
        dirty
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
        // Track the terminal that received focus, including its reporting mode. A live screen
        // can enable reporting after the widget already took focus, such as a session replay.
        let previous = self.focus.last_emitted_terminal_focus;
        let current = self.terminal_focus_id(self.focus.focused, self.focus.window_focused);
        if previous == current {
            return;
        }
        if let Some(id) = previous {
            self.emit_terminal_focus_sequence(id, false);
        }
        if let Some(id) = current {
            self.emit_terminal_focus_sequence(id, true);
        }
        self.focus.last_emitted_terminal_focus = current;
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
            NodeKind::Terminal(ref node)
                if node.mouse_mode.focus_events_enabled && node.on_input.is_some() =>
            {
                Some(id)
            }
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
