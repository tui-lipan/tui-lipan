use crate::app::focus_service;
use crate::core::component::Component;

use super::AppRunner;

impl<C: Component> AppRunner<C> {
    pub(super) fn notify_focus_change(&mut self) {
        focus_service::notify_focus_change(
            &self.core.tree,
            self.focus.focused,
            &mut self.focus.last_notified,
            self.on_focus_changed.as_ref(),
        );
    }
}
