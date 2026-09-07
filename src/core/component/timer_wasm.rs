use std::cell::RefCell;
use std::time::Duration;

use super::task_policy::Task;

struct ControlledEntry {
    due: web_time::Instant,
    owner: Option<super::RuntimeId>,
    task: Task,
}

thread_local! {
    static CONTROLLED: RefCell<Vec<ControlledEntry>> = const { RefCell::new(Vec::new()) };
}

/// Delayed-task scheduler backing [`Command::after`](super::Command::after).
///
/// The web target has no thread to park, so this defers to the host event loop. Without the `web`
/// feature there is no timer source at all and the task runs immediately — the delay is dropped
/// rather than the work.
pub(super) struct TimerService;

impl TimerService {
    pub(super) fn global() -> &'static Self {
        &TimerService
    }

    pub(super) fn schedule(&self, delay: Duration, task: Task) {
        self.schedule_owned(delay, task, None);
    }

    pub(super) fn schedule_owned(
        &self,
        delay: Duration,
        task: Task,
        owner: Option<super::RuntimeId>,
    ) {
        self.schedule_session_owned(
            delay,
            web_time::Instant::now(),
            crate::automation::ClockMode::Realtime,
            task,
            owner,
        );
    }

    pub(super) fn schedule_session_owned(
        &self,
        delay: Duration,
        now: web_time::Instant,
        mode: crate::automation::ClockMode,
        task: Task,
        owner: Option<super::RuntimeId>,
    ) {
        if mode == crate::automation::ClockMode::Controlled {
            CONTROLLED.with(|entries| {
                entries.borrow_mut().push(ControlledEntry {
                    due: now.checked_add(delay).unwrap_or(now),
                    owner,
                    task,
                });
            });
            return;
        }
        #[cfg(feature = "web")]
        {
            use wasm_bindgen::JsCast as _;

            let millis = i32::try_from(delay.as_millis()).unwrap_or(i32::MAX);
            let closure = wasm_bindgen::closure::Closure::once_into_js(move || {
                super::TaskExecutor::global().execute(task);
            });
            let scheduled = web_sys::window().and_then(|window| {
                window
                    .set_timeout_with_callback_and_timeout_and_arguments_0(
                        closure.as_ref().unchecked_ref(),
                        millis,
                    )
                    .ok()
            });
            if scheduled.is_none() {
                crate::debug::internal_log!("[tui-lipan] no window for Command::after; dropping");
            }
        }
        #[cfg(not(feature = "web"))]
        {
            let _ = delay;
            super::TaskExecutor::global().execute(task);
        }
    }

    /// No-op on the web target: pending work lives in the host's `setTimeout` queue, which cannot be
    /// brought forward, and without the `web` feature nothing is ever deferred in the first place.
    ///
    /// The harnesses that drive virtual time (headless capture, `TestBackend`) are native-only, so
    /// this exists to keep the two backends interchangeable rather than to be called.
    pub(super) fn advance_owned(
        &self,
        horizon: web_time::Instant,
        owner: super::RuntimeId,
    ) -> usize {
        let mut due = Vec::new();
        CONTROLLED.with(|entries| {
            let mut entries = entries.borrow_mut();
            let mut retained = Vec::with_capacity(entries.len());
            for entry in entries.drain(..) {
                if entry.owner == Some(owner) && entry.due <= horizon {
                    due.push(entry.task);
                } else {
                    retained.push(entry);
                }
            }
            *entries = retained;
        });
        let count = due.len();
        for task in due {
            task.run();
        }
        count
    }

    pub(super) fn pending_owned(
        &self,
        horizon: web_time::Instant,
        owner: super::RuntimeId,
    ) -> (usize, usize) {
        CONTROLLED.with(|entries| {
            entries
                .borrow()
                .iter()
                .filter(|entry| entry.owner == Some(owner))
                .fold((0, 0), |(due, future), entry| {
                    if entry.due <= horizon {
                        (due + 1, future)
                    } else {
                        (due, future + 1)
                    }
                })
        })
    }

    pub(super) fn cancel_owned(&self, owner: super::RuntimeId) {
        CONTROLLED.with(|entries| {
            entries.borrow_mut().retain(|entry| {
                if entry.owner == Some(owner) {
                    entry.task.cancel();
                    false
                } else {
                    true
                }
            });
        });
    }
}
