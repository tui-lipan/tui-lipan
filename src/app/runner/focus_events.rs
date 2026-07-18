use crate::app::{FocusChanged, FocusEntry};
use crate::core::component::Component;
use crate::layout::tag::tag_of_node;

use super::AppRunner;

impl<C: Component> AppRunner<C> {
    pub(super) fn notify_focus_change(&mut self) {
        let current = self
            .focus
            .focused
            .filter(|id| self.core.tree.is_valid(*id))
            .map(|id| {
                let node = self.core.tree.node(id);
                (
                    id,
                    FocusEntry {
                        key: node.key.clone(),
                        tag: tag_of_node(node),
                    },
                )
            });

        let unchanged = match (&self.focus.last_notified, &current) {
            (None, None) => true,
            (Some((old_id, old)), Some((new_id, new))) => {
                old_id == new_id
                    || old
                        .key
                        .as_ref()
                        .zip(new.key.as_ref())
                        .is_some_and(|(old, new)| old == new)
            }
            _ => false,
        };
        if unchanged {
            // Keep the live node id after a keyed remount so a later blur can
            // still reach the remounted widget's callback.
            self.focus.last_notified = current;
            return;
        }

        let previous = self.focus.last_notified.take();
        self.focus.last_notified = current.clone();

        let on_blur = previous
            .as_ref()
            .filter(|(id, _)| self.core.tree.is_valid(*id))
            .and_then(|(id, _)| self.core.tree.node(*id).on_blur_callback().cloned());
        let on_focus = current
            .as_ref()
            .and_then(|(id, _)| self.core.tree.node(*id).on_focus_callback().cloned());

        if let Some(callback) = on_blur {
            callback.emit(());
        }
        if let Some(callback) = on_focus {
            callback.emit(());
        }
        if let Some(hook) = &self.on_focus_changed {
            hook(&FocusChanged {
                old: previous.map(|(_, entry)| entry),
                new: current.map(|(_, entry)| entry),
            });
        }
    }
}
