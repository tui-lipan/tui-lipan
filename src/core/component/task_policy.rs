use std::collections::HashMap;
use std::sync::Arc;

use crate::callback::CancellationToken;

pub(super) struct Task {
    action: Box<dyn FnOnce() + Send + 'static>,
    token: CancellationToken,
}

impl Task {
    #[cfg(test)]
    pub(super) fn new(action: impl FnOnce() + Send + 'static) -> Self {
        Self::with_token(action, CancellationToken::default())
    }

    pub(super) fn with_token(
        action: impl FnOnce() + Send + 'static,
        token: CancellationToken,
    ) -> Self {
        Self {
            action: Box::new(action),
            token,
        }
    }

    pub(super) fn cancellation_token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub(super) fn cancel(&self) {
        self.token.cancel();
    }

    pub(super) fn run(self) {
        (self.action)();
    }
}

#[derive(Default)]
pub(super) struct KeyedTaskState {
    active: HashMap<Arc<str>, CancellationToken>,
    latest_pending: HashMap<Arc<str>, Task>,
}

impl KeyedTaskState {
    pub(super) fn submit_drop_if_running(&mut self, key: &Arc<str>, task: Task) -> Option<Task> {
        if self.active.contains_key(key) {
            task.cancel();
            None
        } else {
            self.active
                .insert(Arc::clone(key), task.cancellation_token());
            Some(task)
        }
    }

    pub(super) fn submit_latest_only(&mut self, key: &Arc<str>, task: Task) -> Option<Task> {
        if let Some(active) = self.active.get(key) {
            active.cancel();
            if let Some(replaced) = self.latest_pending.insert(Arc::clone(key), task) {
                replaced.cancel();
            }
            None
        } else {
            self.active
                .insert(Arc::clone(key), task.cancellation_token());
            Some(task)
        }
    }

    pub(super) fn on_keyed_task_complete(&mut self, key: &Arc<str>) -> Option<Task> {
        if let Some(task) = self.latest_pending.remove(key) {
            self.active
                .insert(Arc::clone(key), task.cancellation_token());
            Some(task)
        } else {
            self.active.remove(key);
            None
        }
    }

    pub(super) fn clear_for_enqueue_failure(&mut self, key: &Arc<str>) {
        if let Some(active) = self.active.remove(key) {
            active.cancel();
        }
        if let Some(pending) = self.latest_pending.remove(key) {
            pending.cancel();
        }
    }
}
