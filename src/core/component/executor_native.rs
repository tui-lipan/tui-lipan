use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};

use super::TaskPolicy;
use super::task_policy::{KeyedTaskState, Task};

const DEFAULT_OVERFLOW_THREAD_CAP: usize = 32;

pub(super) struct TaskExecutor {
    tx: mpsc::SyncSender<Task>,
    keyed: Arc<Mutex<KeyedTaskState>>,
    overflow_threads: Arc<AtomicUsize>,
    overflow_thread_cap: usize,
}

impl TaskExecutor {
    pub(super) fn global() -> &'static Self {
        static EXECUTOR: OnceLock<TaskExecutor> = OnceLock::new();
        EXECUTOR.get_or_init(Self::new)
    }

    fn new() -> Self {
        let worker_count = std::thread::available_parallelism()
            .map(|n| n.get().clamp(2, 8))
            .unwrap_or(4);
        let queue_cap = worker_count.saturating_mul(64).max(128);
        Self::with_config(worker_count, queue_cap, DEFAULT_OVERFLOW_THREAD_CAP)
    }

    fn with_config(worker_count: usize, queue_cap: usize, overflow_thread_cap: usize) -> Self {
        let queue_cap = queue_cap.max(1);
        let (tx, rx) = mpsc::sync_channel::<Task>(queue_cap);
        let shared_rx = Arc::new(Mutex::new(rx));

        for idx in 0..worker_count {
            let rx = Arc::clone(&shared_rx);
            let _ = std::thread::Builder::new()
                .name(format!("tui-lipan-cmd-worker-{idx}"))
                .spawn(move || {
                    loop {
                        let task = match rx.lock() {
                            Ok(guard) => match guard.recv() {
                                Ok(task) => task,
                                Err(_) => break,
                            },
                            Err(_) => break,
                        };
                        task.run();
                    }
                });
        }

        Self {
            tx,
            keyed: Arc::new(Mutex::new(KeyedTaskState::default())),
            overflow_threads: Arc::new(AtomicUsize::new(0)),
            overflow_thread_cap: overflow_thread_cap.max(1),
        }
    }

    pub(super) fn execute(&self, task: Task) {
        self.enqueue(task);
    }

    pub(super) fn execute_keyed(&self, key: Arc<str>, policy: TaskPolicy, task: Task) {
        match policy {
            TaskPolicy::QueueAll => {
                self.enqueue(task);
            }
            TaskPolicy::DropIfRunning => {
                let to_enqueue = match self.keyed.lock() {
                    Ok(mut keyed) => keyed.submit_drop_if_running(&key, task),
                    Err(_) => Some(task),
                };
                if let Some(task) = to_enqueue {
                    let wrapped = self.wrap_keyed_task(Arc::clone(&key), task);
                    if !self.enqueue(wrapped) {
                        Self::clear_keyed_state_with(&self.keyed, &key);
                    }
                }
            }
            TaskPolicy::LatestOnly => {
                let to_enqueue: Option<Task> = match self.keyed.lock() {
                    Ok(mut keyed) => keyed
                        .submit_latest_only(&key, task)
                        .map(|task| self.wrap_keyed_task(Arc::clone(&key), task)),
                    Err(_) => Some(task),
                };

                if let Some(task) = to_enqueue
                    && !self.enqueue(task)
                {
                    Self::clear_keyed_state_with(&self.keyed, &key);
                }
            }
        }
    }

    fn wrap_keyed_task(&self, key: Arc<str>, task: Task) -> Task {
        let tx = self.tx.clone();
        let keyed = Arc::clone(&self.keyed);
        let overflow_threads = Arc::clone(&self.overflow_threads);
        let overflow_thread_cap = self.overflow_thread_cap;
        let token = task.cancellation_token();
        Task::with_token(
            move || {
                let _guard = KeyedCompletionGuard::new(
                    keyed,
                    tx,
                    overflow_threads,
                    overflow_thread_cap,
                    key,
                );
                task.run();
            },
            token,
        )
    }

    fn on_keyed_task_complete_with(
        keyed_state: Arc<Mutex<KeyedTaskState>>,
        tx: mpsc::SyncSender<Task>,
        overflow_threads: Arc<AtomicUsize>,
        overflow_thread_cap: usize,
        key: Arc<str>,
    ) {
        let next = if let Ok(mut keyed) = keyed_state.lock() {
            keyed.on_keyed_task_complete(&key)
        } else {
            None
        };

        if let Some(task) = next {
            let next_key = Arc::clone(&key);
            let next_keyed = Arc::clone(&keyed_state);
            let next_tx = tx.clone();
            let next_overflow_threads = Arc::clone(&overflow_threads);
            let token = task.cancellation_token();
            let wrapped = Task::with_token(
                move || {
                    let _guard = KeyedCompletionGuard::new(
                        next_keyed,
                        next_tx,
                        next_overflow_threads,
                        overflow_thread_cap,
                        next_key,
                    );
                    task.run();
                },
                token,
            );

            if !Self::enqueue_with(&tx, &overflow_threads, overflow_thread_cap, wrapped) {
                Self::clear_keyed_state_with(&keyed_state, &key);
            }
        }
    }

    fn clear_keyed_state_with(keyed_state: &Arc<Mutex<KeyedTaskState>>, key: &Arc<str>) {
        if let Ok(mut keyed) = keyed_state.lock() {
            keyed.clear_for_enqueue_failure(key);
        }
    }

    fn enqueue(&self, task: Task) -> bool {
        Self::enqueue_with(
            &self.tx,
            &self.overflow_threads,
            self.overflow_thread_cap,
            task,
        )
    }

    fn enqueue_with(
        tx: &mpsc::SyncSender<Task>,
        overflow_threads: &Arc<AtomicUsize>,
        overflow_thread_cap: usize,
        task: Task,
    ) -> bool {
        match tx.try_send(task) {
            Ok(()) => true,
            Err(mpsc::TrySendError::Full(task)) | Err(mpsc::TrySendError::Disconnected(task)) => {
                let active = overflow_threads.fetch_add(1, Ordering::AcqRel);
                if active >= overflow_thread_cap {
                    overflow_threads.fetch_sub(1, Ordering::AcqRel);
                    task.cancel();
                    crate::debug::internal_log!(
                        "[tui-lipan] dropping command task: overflow thread cap reached ({overflow_thread_cap})"
                    );
                    return false;
                }

                crate::debug::internal_log!(
                    "[tui-lipan] command queue saturated, spawning overflow worker ({}/{})",
                    active.saturating_add(1),
                    overflow_thread_cap
                );

                let counter = Arc::clone(overflow_threads);
                let token = task.cancellation_token();
                let spawn_result = std::thread::Builder::new()
                    .name("tui-lipan-cmd-overflow".to_string())
                    .spawn(move || {
                        let _guard = OverflowThreadGuard(counter);
                        task.run();
                    });

                if let Err(err) = spawn_result {
                    overflow_threads.fetch_sub(1, Ordering::AcqRel);
                    token.cancel();
                    crate::debug::internal_log!(
                        "[tui-lipan] failed to spawn overflow worker, dropping task: {}",
                        err
                    );
                    return false;
                }

                true
            }
        }
    }
}

struct KeyedCompletionGuard {
    keyed_state: Option<Arc<Mutex<KeyedTaskState>>>,
    tx: mpsc::SyncSender<Task>,
    overflow_threads: Arc<AtomicUsize>,
    overflow_thread_cap: usize,
    key: Option<Arc<str>>,
}

impl KeyedCompletionGuard {
    fn new(
        keyed_state: Arc<Mutex<KeyedTaskState>>,
        tx: mpsc::SyncSender<Task>,
        overflow_threads: Arc<AtomicUsize>,
        overflow_thread_cap: usize,
        key: Arc<str>,
    ) -> Self {
        Self {
            keyed_state: Some(keyed_state),
            tx,
            overflow_threads,
            overflow_thread_cap,
            key: Some(key),
        }
    }
}

impl Drop for KeyedCompletionGuard {
    fn drop(&mut self) {
        let Some(keyed_state) = self.keyed_state.take() else {
            return;
        };
        let Some(key) = self.key.take() else {
            return;
        };
        TaskExecutor::on_keyed_task_complete_with(
            keyed_state,
            self.tx.clone(),
            Arc::clone(&self.overflow_threads),
            self.overflow_thread_cap,
            key,
        );
    }
}

struct OverflowThreadGuard(Arc<AtomicUsize>);

impl Drop for OverflowThreadGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn task(f: impl FnOnce() + Send + 'static) -> Task {
        Task::new(f)
    }

    #[test]
    fn task_executor_runs_basic_task() {
        let executor = TaskExecutor::with_config(1, 8, 32);
        let (tx, rx) = mpsc::channel();

        executor.execute(task(move || {
            let _ = tx.send(());
        }));

        rx.recv_timeout(Duration::from_secs(1))
            .expect("task should execute");
    }

    #[test]
    fn queue_all_executes_all_tasks_for_same_key() {
        let executor = TaskExecutor::with_config(1, 8, 32);
        let key: Arc<str> = Arc::from("search");
        let (tx, rx) = mpsc::channel();

        for label in ["A", "B", "C"] {
            let tx = tx.clone();
            executor.execute_keyed(
                Arc::clone(&key),
                TaskPolicy::QueueAll,
                task(move || {
                    let _ = tx.send(label);
                }),
            );
        }

        let mut ran = vec![
            rx.recv_timeout(Duration::from_secs(1))
                .expect("first task should execute"),
            rx.recv_timeout(Duration::from_secs(1))
                .expect("second task should execute"),
            rx.recv_timeout(Duration::from_secs(1))
                .expect("third task should execute"),
        ];
        ran.sort_unstable();
        assert_eq!(ran, vec!["A", "B", "C"]);
    }

    #[test]
    fn drop_if_running_drops_new_task_when_key_active() {
        let executor = TaskExecutor::with_config(1, 8, 32);
        let key: Arc<str> = Arc::from("search");

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (ran_tx, ran_rx) = mpsc::channel();

        let ran_first = ran_tx.clone();
        executor.execute_keyed(
            Arc::clone(&key),
            TaskPolicy::DropIfRunning,
            task(move || {
                let _ = started_tx.send(());
                let _ = release_rx.recv();
                let _ = ran_first.send("A");
            }),
        );

        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first task should start");

        let ran_second = ran_tx.clone();
        executor.execute_keyed(
            key,
            TaskPolicy::DropIfRunning,
            task(move || {
                let _ = ran_second.send("B");
            }),
        );

        release_tx.send(()).expect("release signal should be sent");

        assert_eq!(
            ran_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("first task should complete"),
            "A"
        );
        assert!(
            matches!(
                ran_rx.recv_timeout(Duration::from_millis(200)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "second task should be dropped"
        );
    }

    #[test]
    fn drop_if_running_does_not_cancel_active_task() {
        let executor = TaskExecutor::with_config(1, 8, 32);
        let key: Arc<str> = Arc::from("search");

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let active = task(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        });
        let active_token = active.cancellation_token();

        executor.execute_keyed(Arc::clone(&key), TaskPolicy::DropIfRunning, active);
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("active task should start");

        let dropped = task(|| {});
        let dropped_token = dropped.cancellation_token();
        executor.execute_keyed(key, TaskPolicy::DropIfRunning, dropped);

        assert!(!active_token.is_cancelled());
        assert!(dropped_token.is_cancelled());
        release_tx.send(()).expect("release signal should be sent");
    }

    #[test]
    fn latest_only_keeps_only_latest_pending_task() {
        let executor = TaskExecutor::with_config(1, 8, 32);
        let key: Arc<str> = Arc::from("search");

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (ran_tx, ran_rx) = mpsc::channel();

        let ran_first = ran_tx.clone();
        executor.execute_keyed(
            Arc::clone(&key),
            TaskPolicy::LatestOnly,
            task(move || {
                let _ = started_tx.send(());
                let _ = ran_first.send("A");
                let _ = release_rx.recv();
            }),
        );

        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("first task should start");

        let ran_second = ran_tx.clone();
        executor.execute_keyed(
            Arc::clone(&key),
            TaskPolicy::LatestOnly,
            task(move || {
                let _ = ran_second.send("B");
            }),
        );

        let ran_third = ran_tx.clone();
        executor.execute_keyed(
            key,
            TaskPolicy::LatestOnly,
            task(move || {
                let _ = ran_third.send("C");
            }),
        );

        release_tx.send(()).expect("release signal should be sent");

        assert_eq!(
            ran_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("A should run"),
            "A"
        );
        assert_eq!(
            ran_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("C should run"),
            "C"
        );
        assert!(
            matches!(
                ran_rx.recv_timeout(Duration::from_millis(200)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "intermediate task should be replaced"
        );
    }

    #[test]
    fn latest_only_cancels_active_and_replaced_pending_tokens() {
        let executor = TaskExecutor::with_config(1, 8, 32);
        let key: Arc<str> = Arc::from("search");

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let active = task(move || {
            let _ = started_tx.send(());
            let _ = release_rx.recv();
        });
        let active_token = active.cancellation_token();
        executor.execute_keyed(Arc::clone(&key), TaskPolicy::LatestOnly, active);
        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("active task should start");

        let pending = task(|| {});
        let pending_token = pending.cancellation_token();
        executor.execute_keyed(Arc::clone(&key), TaskPolicy::LatestOnly, pending);
        assert!(active_token.is_cancelled());
        assert!(!pending_token.is_cancelled());

        let replacement = task(|| {});
        let replacement_token = replacement.cancellation_token();
        executor.execute_keyed(key, TaskPolicy::LatestOnly, replacement);
        assert!(pending_token.is_cancelled());
        assert!(!replacement_token.is_cancelled());

        release_tx.send(()).expect("release signal should be sent");
    }

    #[test]
    fn queue_overflow_still_executes_task_via_overflow_worker() {
        let executor = TaskExecutor::with_config(0, 1, 32);
        let (tx, rx) = mpsc::channel();

        executor.execute(task(|| {
            std::thread::sleep(Duration::from_millis(10));
        }));

        executor.execute(task(move || {
            let _ = tx.send(());
        }));

        rx.recv_timeout(Duration::from_secs(1))
            .expect("overflow task should execute");
    }

    #[test]
    fn overflow_worker_spawns_are_rate_limited() {
        let executor = TaskExecutor::with_config(1, 1, 1);

        let (worker_started_tx, worker_started_rx) = mpsc::channel();
        let (release_worker_tx, release_worker_rx) = mpsc::channel();
        let (ran_tx, ran_rx) = mpsc::channel();

        executor.execute(task(move || {
            let _ = worker_started_tx.send(());
            let _ = release_worker_rx.recv();
        }));

        worker_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker task should start");

        executor.execute(task(|| {
            std::thread::sleep(Duration::from_millis(25));
        }));

        let (overflow_started_tx, overflow_started_rx) = mpsc::channel();
        let (release_overflow_tx, release_overflow_rx) = mpsc::channel();
        let ran_overflow = ran_tx.clone();
        executor.execute(task(move || {
            let _ = overflow_started_tx.send(());
            let _ = release_overflow_rx.recv();
            let _ = ran_overflow.send("overflow");
        }));

        overflow_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("overflow worker should start");

        let ran_dropped = ran_tx.clone();
        executor.execute(task(move || {
            let _ = ran_dropped.send("dropped");
        }));

        release_overflow_tx
            .send(())
            .expect("overflow release signal should be sent");
        release_worker_tx
            .send(())
            .expect("worker release signal should be sent");

        assert_eq!(
            ran_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("overflow task should finish"),
            "overflow"
        );
        assert!(
            matches!(
                ran_rx.recv_timeout(Duration::from_millis(200)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "task beyond overflow cap should be dropped"
        );
    }

    #[test]
    fn latest_only_recovers_after_dropped_active_task() {
        let executor = TaskExecutor::with_config(1, 1, 1);
        let key: Arc<str> = Arc::from("search");

        let (worker_started_tx, worker_started_rx) = mpsc::channel();
        let (release_worker_tx, release_worker_rx) = mpsc::channel();
        let (overflow_started_tx, overflow_started_rx) = mpsc::channel();
        let (release_overflow_tx, release_overflow_rx) = mpsc::channel();
        let (ran_tx, ran_rx) = mpsc::channel();

        executor.execute(task(move || {
            let _ = worker_started_tx.send(());
            let _ = release_worker_rx.recv();
        }));

        worker_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker task should start");

        executor.execute(task(|| {
            std::thread::sleep(Duration::from_millis(25));
        }));

        executor.execute(task(move || {
            let _ = overflow_started_tx.send(());
            let _ = release_overflow_rx.recv();
        }));

        overflow_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("overflow worker should start");

        let ran_first = ran_tx.clone();
        let dropped = task(move || {
            let _ = ran_first.send("first");
        });
        let dropped_token = dropped.cancellation_token();
        executor.execute_keyed(Arc::clone(&key), TaskPolicy::LatestOnly, dropped);
        assert!(dropped_token.is_cancelled());

        release_overflow_tx
            .send(())
            .expect("overflow release signal should be sent");
        release_worker_tx
            .send(())
            .expect("worker release signal should be sent");

        std::thread::sleep(Duration::from_millis(60));

        let ran_second = ran_tx.clone();
        executor.execute_keyed(
            key,
            TaskPolicy::LatestOnly,
            task(move || {
                let _ = ran_second.send("second");
            }),
        );

        assert_eq!(
            ran_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("follow-up task should execute"),
            "second"
        );
        assert!(
            matches!(
                ran_rx.recv_timeout(Duration::from_millis(200)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "dropped task must not execute later"
        );
    }

    #[test]
    fn drop_if_running_recovers_after_dropped_active_task() {
        let executor = TaskExecutor::with_config(1, 1, 1);
        let key: Arc<str> = Arc::from("search");

        let (worker_started_tx, worker_started_rx) = mpsc::channel();
        let (release_worker_tx, release_worker_rx) = mpsc::channel();
        let (overflow_started_tx, overflow_started_rx) = mpsc::channel();
        let (release_overflow_tx, release_overflow_rx) = mpsc::channel();
        let (ran_tx, ran_rx) = mpsc::channel();

        executor.execute(task(move || {
            let _ = worker_started_tx.send(());
            let _ = release_worker_rx.recv();
        }));

        worker_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("worker task should start");

        executor.execute(task(|| {
            std::thread::sleep(Duration::from_millis(25));
        }));

        executor.execute(task(move || {
            let _ = overflow_started_tx.send(());
            let _ = release_overflow_rx.recv();
        }));

        overflow_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("overflow worker should start");

        let ran_first = ran_tx.clone();
        let dropped = task(move || {
            let _ = ran_first.send("first");
        });
        let dropped_token = dropped.cancellation_token();
        executor.execute_keyed(Arc::clone(&key), TaskPolicy::DropIfRunning, dropped);
        assert!(dropped_token.is_cancelled());

        release_overflow_tx
            .send(())
            .expect("overflow release signal should be sent");
        release_worker_tx
            .send(())
            .expect("worker release signal should be sent");

        std::thread::sleep(Duration::from_millis(60));

        let ran_second = ran_tx.clone();
        executor.execute_keyed(
            key,
            TaskPolicy::DropIfRunning,
            task(move || {
                let _ = ran_second.send("second");
            }),
        );

        assert_eq!(
            ran_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("follow-up task should execute"),
            "second"
        );
        assert!(
            matches!(
                ran_rx.recv_timeout(Duration::from_millis(200)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ),
            "dropped task must not execute later"
        );
    }

    #[test]
    fn latest_only_cleans_up_keyed_state_after_task_panic() {
        let executor = TaskExecutor::with_config(1, 8, 32);
        let key: Arc<str> = Arc::from("panic-cleanup");
        let (panicking_started_tx, panicking_started_rx) = mpsc::channel();
        let (ran_tx, ran_rx) = mpsc::channel();
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));

        executor.execute_keyed(
            Arc::clone(&key),
            TaskPolicy::LatestOnly,
            task(move || {
                let _ = panicking_started_tx.send(());
                panic!("intentional keyed task panic");
            }),
        );

        panicking_started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("panicking task should start");
        std::thread::sleep(Duration::from_millis(60));
        std::panic::set_hook(previous_hook);

        executor.execute_keyed(
            key,
            TaskPolicy::LatestOnly,
            task(move || {
                let _ = ran_tx.send("recovered");
            }),
        );

        assert_eq!(
            ran_rx
                .recv_timeout(Duration::from_secs(1))
                .expect("follow-up task should execute"),
            "recovered"
        );
    }
}
