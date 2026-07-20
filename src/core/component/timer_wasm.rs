use std::time::Duration;

use super::task_policy::Task;

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
}
