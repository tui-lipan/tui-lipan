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

            #[cfg(feature = "devtools")]
            if !matches!(update_level, UpdateLevel::None) {
                let level = match update_level {
                    UpdateLevel::Paint => super::DirtyLevel::PaintOnly,
                    UpdateLevel::Layout => super::DirtyLevel::LayoutOnly,
                    UpdateLevel::Full => super::DirtyLevel::Full,
                    UpdateLevel::None => unreachable!(),
                };
                let name = if scope == ScopeId(1) {
                    self.root_component_display_name
                        .get_or_insert_with(|| {
                            std::sync::Arc::from(
                                crate::core::nested::short_type_name(
                                    self.core.root_component_name(),
                                )
                                .as_str(),
                            )
                        })
                        .clone()
                } else {
                    self.core
                        .components
                        .display_name_for_scope(scope)
                        .unwrap_or_else(|| std::sync::Arc::from("?"))
                };
                self.note_attribution(
                    crate::devtools::state::UpdateSource::Component { scope, name },
                    level,
                );
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
                    dirty |= self.framework_focus_step(focus::FocusDirection::Next);
                }
                FrameworkCommandAction::FocusPrev => {
                    dirty |= self.framework_focus_step(focus::FocusDirection::Prev);
                }
                FrameworkCommandAction::DismissOverlay => {
                    if self.handle_overlay_escape() {
                        dirty = true;
                    }
                }
            }
        }

        self.notify_focus_change();

        dirty
    }

    /// Tab traversal issued as a framework command (`Command::focus_next()` etc.).
    ///
    /// Returns whether a repaint is needed: an overlay consuming the step, or
    /// focus actually moving. [`FocusPolicy::Manual`] suppresses the framework
    /// step, matching key dispatch.
    ///
    /// [`FocusPolicy::Manual`]: crate::FocusPolicy::Manual
    fn framework_focus_step(&mut self, direction: focus::FocusDirection) -> bool {
        let before = self.focus.focused;
        let overlay_handled = match direction {
            focus::FocusDirection::Next => self.focus_overlay_next(),
            focus::FocusDirection::Prev => self.focus_overlay_prev(),
        };
        if overlay_handled {
            return true;
        }
        if !focus::step_for_policy(
            &self.core.tree,
            &mut self.focus.focused,
            &mut self.focus.focused_key,
            &mut self.focus.focused_tag,
            self.focus.policy,
            direction,
        ) {
            return false;
        }
        if self.focus.focused != before {
            self.animation.reset_blink();
            return true;
        }
        false
    }
}
