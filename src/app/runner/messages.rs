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
            if self.sync_theme_request(true) {
                dirty.mark_full();
            }
            if scope == ScopeId(1) {
                self.apply_root_update(dirty, update_level);
            } else {
                self.apply_component_update(dirty, scope, update_level);
            }
        }
        self.apply_lifecycle_updates(dirty);
        self.sync_runtime_keymap_config();
        Ok(())
    }

    pub(super) fn apply_lifecycle_updates(&mut self, dirty: &mut DirtyTracker) {
        while let Some((scope, level)) = self.core.take_lifecycle_update() {
            if self.core.components.is_mounted_scope(scope) {
                self.apply_component_update(dirty, scope, level);
            }
        }
        if self.sync_theme_request(true) {
            dirty.mark_full();
        }
    }

    /// Apply an [`UpdateLevel`] returned by a root lifecycle or message callback.
    pub(super) fn apply_root_update(
        &mut self,
        dirty: &mut DirtyTracker,
        update_level: UpdateLevel,
    ) {
        if self.sync_theme_request(true) {
            dirty.mark_full();
        }
        self.apply_component_update(dirty, ScopeId(1), update_level);
    }

    pub(super) fn apply_component_update(
        &mut self,
        dirty: &mut DirtyTracker,
        scope: ScopeId,
        update_level: UpdateLevel,
    ) {
        // The DevTools panel's own state changed. Only its view reads that state, so it is rebuilt
        // on its own, at once, and nothing about it is the app's frame.
        #[cfg(feature = "devtools")]
        if !matches!(update_level, UpdateLevel::None) && self.devtools_owns_scope(scope) {
            self.devtools_panel_stale = true;
            self.devtools_panel_refresh_now = true;
            return;
        }
        match update_level {
            UpdateLevel::None => {}
            UpdateLevel::Paint => {
                #[cfg(debug_assertions)]
                if scope == ScopeId(1) {
                    self.debug_paint_claim_root = true;
                }
                dirty.mark_paint();
            }
            #[cfg(feature = "terminal")]
            UpdateLevel::TerminalGraphics => dirty.mark_terminal_graphics(),
            #[cfg(feature = "terminal")]
            UpdateLevel::TerminalPaint => {
                #[cfg(debug_assertions)]
                if scope == ScopeId(1) {
                    self.debug_paint_claim_root = true;
                }
                dirty.mark_terminal_paint();
            }
            UpdateLevel::Render => {
                self.mark_dirty_scope(scope);
                dirty.mark_layout();
            }
            UpdateLevel::Full => {
                if scope == ScopeId(1) {
                    self.clear_dirty_scopes();
                }
                dirty.mark_full();
            }
        }

        #[cfg(feature = "devtools")]
        if !matches!(update_level, UpdateLevel::None)
            && self.devtools_config.metrics
            && self.devtools_state.borrow().visible
            && !self.devtools_metrics_suppressed
        {
            let level = match update_level {
                UpdateLevel::Paint => super::DirtyLevel::PaintOnly,
                #[cfg(feature = "terminal")]
                UpdateLevel::TerminalGraphics => super::DirtyLevel::TerminalGraphicsOnly,
                #[cfg(feature = "terminal")]
                UpdateLevel::TerminalPaint => super::DirtyLevel::TerminalPaintOnly,
                UpdateLevel::Render => super::DirtyLevel::LayoutOnly,
                UpdateLevel::Full => super::DirtyLevel::Full,
                UpdateLevel::None => unreachable!(),
            };
            let name = if scope == ScopeId(1) {
                self.root_component_display_name
                    .get_or_insert_with(|| {
                        std::sync::Arc::from(
                            crate::core::nested::short_type_name(self.core.root_component_name())
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

    /// Whether `scope` is the DevTools panel or a component inside it, such as its log view.
    #[cfg(feature = "devtools")]
    pub(super) fn devtools_owns_scope(&self, scope: ScopeId) -> bool {
        let Some(panel) = self.core.extra_root_scope() else {
            return false;
        };
        let mut current = scope;
        while current != ScopeId(1) {
            if current == panel {
                return true;
            }
            match self.core.components.parent_scope(current) {
                Some(parent) => current = parent,
                None => return false,
            }
        }
        false
    }

    /// Record a scope once. A queued application root covers both expanded trees.
    pub(super) fn mark_dirty_scope(&mut self, scope: ScopeId) {
        let root = ScopeId(1);
        if self.dirty_scope_set.contains(&root) {
            return;
        }
        if scope == root {
            self.clear_dirty_scopes();
        }
        if self.dirty_scope_set.insert(scope) {
            self.dirty_component_scopes.push(scope);
        }
    }

    /// Take the next batch in insertion order, keeping the outermost dirty scopes.
    pub(super) fn take_dirty_scopes(&mut self) -> Vec<ScopeId> {
        let mut scopes = std::mem::take(&mut self.dirty_component_scopes);
        scopes.retain(|&scope| {
            !self
                .core
                .components
                .ancestor_scopes(scope)
                .any(|ancestor| self.dirty_scope_set.contains(&ancestor))
        });
        self.dirty_scope_set.clear();
        scopes
    }

    pub(super) fn clear_dirty_scopes(&mut self) {
        self.dirty_component_scopes.clear();
        self.dirty_scope_set.clear();
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
                #[cfg(feature = "devtools")]
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
    pub(super) fn framework_focus_step(&mut self, direction: focus::FocusDirection) -> bool {
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
            crate::app::focus_service::release_closing_saves(
                &self.core.tree,
                &mut self.focus.focus_stack,
            );
            self.animation.reset_blink();
            return true;
        }
        false
    }
}
