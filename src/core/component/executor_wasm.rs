use std::cell::RefCell;
use std::sync::Arc;

#[cfg(feature = "web")]
use wasm_bindgen_futures::spawn_local;

use super::TaskPolicy;
use super::task_policy::{KeyedTaskState, Task};

pub(super) struct TaskExecutor {
    keyed: RefCell<KeyedTaskState>,
}

thread_local! {
    static EXECUTOR: &'static TaskExecutor = Box::leak(Box::new(TaskExecutor::new()));
}

impl TaskExecutor {
    pub(super) fn global() -> &'static Self {
        EXECUTOR.with(|executor| *executor)
    }

    fn new() -> Self {
        Self {
            keyed: RefCell::new(KeyedTaskState::default()),
        }
    }

    pub(super) fn execute(&self, task: Task) {
        #[cfg(feature = "web")]
        {
            spawn_local(async move {
                task.run();
            });
        }
        #[cfg(not(feature = "web"))]
        {
            task.run();
        }
    }

    pub(super) fn execute_keyed(&self, key: Arc<str>, policy: TaskPolicy, task: Task) {
        match policy {
            TaskPolicy::QueueAll => self.execute(task),
            TaskPolicy::DropIfRunning => {
                if let Some(task) = self.keyed.borrow_mut().submit_drop_if_running(&key, task) {
                    let wrapped = self.wrap_keyed_task(key, task);
                    self.execute(wrapped);
                }
            }
            TaskPolicy::LatestOnly => {
                if let Some(task) = self.keyed.borrow_mut().submit_latest_only(&key, task) {
                    let wrapped = self.wrap_keyed_task(key, task);
                    self.execute(wrapped);
                }
            }
        }
    }

    fn wrap_keyed_task(&self, key: Arc<str>, task: Task) -> Task {
        let token = task.cancellation_token();
        Task::with_token(
            move || {
                task.run();
                TaskExecutor::global().on_keyed_task_complete(key);
            },
            token,
        )
    }

    fn on_keyed_task_complete(&self, key: Arc<str>) {
        if let Some(task) = self.keyed.borrow_mut().on_keyed_task_complete(&key) {
            let wrapped = self.wrap_keyed_task(key, task);
            self.execute(wrapped);
        }
    }
}
