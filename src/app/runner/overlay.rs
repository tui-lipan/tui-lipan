use std::time::Duration;

use crate::app::input::focus;
use crate::clipboard::ClipboardHandle;
use crate::core::component::Component;
use crate::core::event::MouseButton;
use crate::core::node::OverlayRoot;

use super::AppRunner;

impl<C: Component> AppRunner<C> {
    pub(crate) fn handle_overlay_click(&mut self, button: MouseButton, x: u16, y: u16) -> bool {
        let overlays: Vec<_> = self.core.tree.overlay_roots().to_vec();
        for overlay in overlays.iter().rev() {
            if !self.core.tree.is_valid(overlay.id) {
                continue;
            }
            let rect = self.core.tree.node(overlay.id).rect;
            let inside = rect.contains(x as i16, y as i16);

            if inside {
                if button == MouseButton::Right
                    && let Some(text) = overlay.copy_text.as_deref()
                {
                    self.copy_overlay_text(overlay, text);
                    return true;
                }

                if let (Some(text), Some(copy_zone)) =
                    (overlay.copy_text.as_deref(), overlay.copy_zone)
                    && button == MouseButton::Left
                    && copy_zone.contains(x as i16, y as i16)
                {
                    self.copy_overlay_text(overlay, text);
                    return true;
                }

                if button == MouseButton::Left && overlay.dismiss_policy.dismiss_on_click_inside() {
                    return self.dismiss_overlay(overlay);
                }
                return false;
            }

            if button == MouseButton::Left && overlay.dismiss_policy.dismiss_on_click_outside() {
                return self.dismiss_overlay(overlay);
            }

            if overlay.captures_focus {
                return true;
            }
        }

        false
    }

    fn copy_overlay_text(&self, overlay: &OverlayRoot, text: &str) {
        let clipboard = ClipboardHandle::new(self.clipboard.clone(), self.clipboard_config.clone());
        if let Err(err) = clipboard.copy(text) {
            self.clipboard.report_error(err);
            return;
        }
        if let Some(id) = overlay.overlay_id {
            let duration =
                Duration::from_millis(self.clipboard_config.copy_feedback_duration_ms as u64);
            self.core
                .overlay_manager
                .borrow_mut()
                .trigger_copy_feedback(id, duration);
        }
    }

    pub(crate) fn handle_overlay_escape(&mut self) -> bool {
        let overlays: Vec<_> = self.core.tree.overlay_roots().to_vec();
        for overlay in overlays.iter().rev() {
            if overlay.dismiss_policy.dismiss_on_escape() {
                return self.dismiss_overlay(overlay);
            }
        }

        overlays.iter().any(|overlay| overlay.captures_focus)
    }

    pub(crate) fn dismiss_overlay(&mut self, overlay: &OverlayRoot) -> bool {
        let dismissed = if let Some(id) = overlay.overlay_id {
            self.core.overlay_manager.borrow_mut().dismiss(id)
        } else if let Some(cb) = &overlay.on_dismiss {
            cb.emit(());
            true
        } else {
            false
        };

        if dismissed && overlay.captures_focus {
            self.restore_focus_from_stack();
        }
        dismissed
    }

    pub(crate) fn restore_focus_from_stack(&mut self) {
        if let Some(saved_key) = self.focus.focus_stack.pop() {
            self.focus.focused_key = saved_key;
            self.focus.focused = None;
            self.focus.focused_tag = None;
            focus::restore_focus(
                &self.core.tree,
                &mut self.focus.focused,
                &mut self.focus.focused_key,
                &mut self.focus.focused_tag,
                self.focus.policy,
            );
            // Reset blink state when focus changes
            self.animation.reset_blink();
        }
    }

    pub(crate) fn ensure_overlay_focus(&mut self) {
        let Some((overlay_id, auto_focus)) = self
            .core
            .tree
            .top_capturing_overlay()
            .map(|overlay| (overlay.id, overlay.auto_focus))
        else {
            return;
        };
        if !auto_focus {
            self.suspend_focus_for_empty_overlay();
            return;
        }
        let focused_in_overlay = self
            .focus
            .focused
            .filter(|id| self.core.tree.is_descendant(overlay_id, *id))
            .is_some();
        if focused_in_overlay {
            return;
        }

        let focusables = self.core.tree.focusables_in_subtree(overlay_id);
        if focusables.is_empty() {
            self.suspend_focus_for_empty_overlay();
            return;
        }

        self.push_focus_stack_for_overlay();
        let next = focusables[0];
        self.set_focus(next);
    }

    pub(crate) fn top_capturing_overlay_is_empty(&self) -> bool {
        self.core
            .tree
            .top_capturing_overlay()
            .is_some_and(|overlay| self.core.tree.focusables_in_subtree(overlay.id).is_empty())
    }

    fn push_focus_stack_for_overlay(&mut self) {
        // Deduplicate: don't push if the same key is already on top of the stack.
        let should_push = self
            .focus
            .focus_stack
            .last()
            .is_none_or(|top| *top != self.focus.focused_key);
        if !should_push {
            return;
        }

        // Cap maximum stack depth to prevent unbounded growth.
        const MAX_FOCUS_STACK_DEPTH: usize = 32;
        if self.focus.focus_stack.len() >= MAX_FOCUS_STACK_DEPTH {
            self.focus.focus_stack.remove(0);
        }
        self.focus.focus_stack.push(self.focus.focused_key.clone());
    }

    fn suspend_focus_for_empty_overlay(&mut self) {
        if self.focus.focused.is_none()
            && self.focus.focused_key.is_none()
            && self.focus.focused_tag.is_none()
        {
            return;
        }

        self.push_focus_stack_for_overlay();
        self.focus.focused = None;
        self.focus.focused_tag = None;
        self.animation.reset_blink();
    }

    pub(crate) fn focus_overlay_next(&mut self) -> bool {
        let Some(overlay) = self.core.tree.top_capturing_overlay() else {
            return false;
        };
        let mut focusables = self.core.tree.focusables_in_subtree(overlay.id);
        if focusables.is_empty() {
            return true;
        }
        focusables.sort_by_key(|id| id.index());
        let next = if let Some(curr) = self.focus.focused
            && let Some(idx) = focusables.iter().position(|id| *id == curr)
        {
            focusables[(idx + 1) % focusables.len()]
        } else {
            focusables[0]
        };
        self.set_focus(next);
        true
    }

    pub(crate) fn focus_overlay_prev(&mut self) -> bool {
        let Some(overlay) = self.core.tree.top_capturing_overlay() else {
            return false;
        };
        let mut focusables = self.core.tree.focusables_in_subtree(overlay.id);
        if focusables.is_empty() {
            return true;
        }
        focusables.sort_by_key(|id| id.index());
        let prev = if let Some(curr) = self.focus.focused
            && let Some(idx) = focusables.iter().position(|id| *id == curr)
        {
            focusables[(idx + focusables.len().saturating_sub(1)) % focusables.len()]
        } else {
            focusables[focusables.len().saturating_sub(1)]
        };
        self.set_focus(prev);
        true
    }
}
