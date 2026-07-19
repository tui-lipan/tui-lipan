use std::time::Duration;

use crate::app::focus_service::{self, OverlayKey};
use crate::app::input::focus::FocusDirection;
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
            self.restore_focus_from_stack(OverlayKey::of(overlay));
        }
        dismissed
    }

    pub(crate) fn restore_focus_from_stack(&mut self, overlay: OverlayKey) {
        let restored = focus_service::restore_focus_from_stack(
            &self.core.tree,
            &mut self.focus.refs(),
            overlay,
        );
        if restored {
            self.animation.reset_blink();
        }
    }

    pub(crate) fn ensure_overlay_focus(&mut self) {
        let moved = focus_service::ensure_overlay_focus(&self.core.tree, &mut self.focus.refs());
        if moved {
            self.animation.reset_blink();
        }
    }

    pub(crate) fn top_capturing_overlay_is_empty(&self) -> bool {
        focus_service::top_capturing_overlay_is_empty(&self.core.tree)
    }

    pub(crate) fn focus_overlay_next(&mut self) -> bool {
        self.focus_overlay_step(FocusDirection::Next)
    }

    pub(crate) fn focus_overlay_prev(&mut self) -> bool {
        self.focus_overlay_step(FocusDirection::Prev)
    }

    fn focus_overlay_step(&mut self, direction: FocusDirection) -> bool {
        let before = self.focus.focused;
        let handled =
            focus_service::overlay_step(&self.core.tree, &mut self.focus.refs(), direction);
        if handled && self.focus.focused != before {
            self.animation.reset_blink();
        }
        handled
    }
}
