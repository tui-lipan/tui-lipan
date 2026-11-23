use crate::app::focus_service;
use crate::core::component::Component;
use crate::core::nested::ComponentEvent;

use super::AppRunner;

impl<C: Component> AppRunner<C> {
    pub(super) fn notify_focus_change(&mut self) {
        if let Some(change) = focus_service::notify_focus_change(
            &self.core.tree,
            self.focus.focused,
            &mut self.focus.last_notified,
        ) {
            self.core.focus.update_from_tree(
                &self.core.tree,
                self.focus.focused,
                self.focus.focused_key.as_ref(),
            );
            self.core
                .broadcast_component_event(ComponentEvent::Focus(&change));
        }
    }
}
