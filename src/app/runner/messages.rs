use crate::Result;
use crate::callback::ScopeId;
use crate::core::component::{Component, UpdateLevel};

use crate::app::input::focus;

use super::{AppRunner, DirtyTracker, FrameworkCommandAction};

impl<C: Component> AppRunner<C> {
    pub(super) fn drain_messages_and_commands(&mut self, dirty: &mut DirtyTracker) -> Result<()> {
        self.core.drain_commands();
        self.process_pending_messages(dirty)?;

        self.core.drain_commands();
        self.process_pending_messages(dirty)
    }

    pub(super) fn process_pending_messages(&mut self, dirty: &mut DirtyTracker) -> Result<()> {
        while let Some((scope, msg)) = { self.core.queue.borrow_mut().pop_front() } {
            let update_level = self.core.update_from_boxed(scope, msg)?;
            match update_level {
                UpdateLevel::None => {}
                UpdateLevel::Paint => {
                    #[cfg(debug_assertions)]
                    if scope == ScopeId(1) {
                        self.debug_paint_claim_root = true;
                    }
                    dirty.mark_paint();
                }
                UpdateLevel::Layout => {
                    if scope == ScopeId(1) {
                        self.dirty_component_scopes.clear();
                        self.dirty_scope_set.clear();
                        self.dirty_scope_set.insert(ScopeId(1));
                        self.dirty_component_scopes.push(ScopeId(1));
                    } else if self.dirty_scope_set.insert(scope) {
                        self.dirty_component_scopes.push(scope);
                    }
                    dirty.mark_layout();
                }
                UpdateLevel::Full => {
                    if scope == ScopeId(1) {
                        self.dirty_component_scopes.clear();
                        self.dirty_scope_set.clear();
                    }
                    dirty.mark_full();
                }
            }
        }
        Ok(())
    }

    pub(super) fn apply_framework_commands(&mut self) -> bool {
        let mut actions = self.framework_command_queue.borrow_mut();
        if actions.is_empty() {
            return false;
        }

        let pending: Vec<FrameworkCommandAction> = actions.drain(..).collect();
        drop(actions);

        let mut dirty = false;
        for action in pending {
            match action {
                FrameworkCommandAction::Quit => {
                    self.core.ctx.quit();
                    dirty = true;
                }
                FrameworkCommandAction::ToggleDevtools => {
                    self.core.ctx.toggle_devtools();
                    dirty = true;
                }
                FrameworkCommandAction::FocusNext => {
                    let before = self.focus.focused;
                    if self.focus_overlay_next() {
                        dirty = true;
                    } else {
                        focus::focus_next(
                            &self.core.tree,
                            &mut self.focus.focused,
                            &mut self.focus.focused_key,
                            &mut self.focus.focused_tag,
                        );
                        if self.focus.focused != before {
                            self.animation.reset_blink();
                            dirty = true;
                        }
                    }
                }
                FrameworkCommandAction::FocusPrev => {
                    let before = self.focus.focused;
                    if self.focus_overlay_prev() {
                        dirty = true;
                    } else {
                        focus::focus_prev(
                            &self.core.tree,
                            &mut self.focus.focused,
                            &mut self.focus.focused_key,
                            &mut self.focus.focused_tag,
                        );
                        if self.focus.focused != before {
                            self.animation.reset_blink();
                            dirty = true;
                        }
                    }
                }
                FrameworkCommandAction::DismissOverlay => {
                    if self.handle_overlay_escape() {
                        dirty = true;
                    }
                }
            }
        }

        dirty
    }
}
