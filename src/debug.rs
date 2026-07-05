//! Debug and diagnostic utilities.
//!
//! This module provides counters and metrics for debugging rendering behavior.
//! These are intended for diagnostic purposes and should not be used in production.

#[cfg(feature = "devtools")]
use std::cell::Cell;
use std::fmt;
#[cfg(not(target_arch = "wasm32"))]
use std::fs::OpenOptions;
#[cfg(not(target_arch = "wasm32"))]
use std::io::Write;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Global counter for mouse events processed by the framework.
///
/// This counter is incremented every time a mouse event is received and processed,
/// regardless of whether it triggers a render.
///
/// # Example
///
/// ```rust,ignore
/// use tui_lipan::debug;
///
/// let count = debug::mouse_events_processed();
/// ```
static MOUSE_EVENTS_PROCESSED: AtomicUsize = AtomicUsize::new(0);

const DEBUG_ENV: &str = "TUI_LIPAN_DEBUG";
const DEBUG_FILE_ENV: &str = "TUI_LIPAN_DEBUG_FILE";

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "devtools")]
static DEVTOOLS_LOGS_ENABLED: AtomicBool = AtomicBool::new(true);

#[cfg(feature = "devtools")]
thread_local! {
    static DEVTOOLS_LOG_SUPPRESSED: Cell<bool> = const { Cell::new(false) };
}

/// RAII guard that suppresses `debug_log!` → devtools-panel routing for the
/// current thread until dropped. File/stderr logging is unaffected.
///
/// Obtain via [`suppress_devtools_log`].
#[cfg(feature = "devtools")]
pub(crate) struct DevtoolsLogGuard;

#[cfg(feature = "devtools")]
impl Drop for DevtoolsLogGuard {
    fn drop(&mut self) {
        DEVTOOLS_LOG_SUPPRESSED.with(|cell| cell.set(false));
    }
}

/// Suppress routing of `debug_log!` entries to the devtools panel for the
/// current thread until the returned guard is dropped.
///
/// Use this around framework-internal hot paths (animation ticker, cursor
/// blink) so their logs don't appear in the user-facing devtools log view.
#[cfg(feature = "devtools")]
pub(crate) fn suppress_devtools_log() -> DevtoolsLogGuard {
    DEVTOOLS_LOG_SUPPRESSED.with(|cell| cell.set(true));
    DevtoolsLogGuard
}

/// Origin of a `debug_log!` line, used by devtools to optionally hide noisy
/// framework-internal logging from the user-facing log view.
///
/// Framework-internal call sites use `internal_log!` (→ [`LogSource::Framework`]);
/// application code uses the public [`debug_log!`](crate::debug_log) macro (→ [`LogSource::App`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogSource {
    /// Emitted by tui-lipan itself (input plumbing, dirty tracking, etc.).
    Framework,
    /// Emitted by the host application via `debug_log!`.
    App,
}

#[cfg(feature = "devtools")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DevLogEntry {
    pub(crate) message: String,
    pub(crate) source: LogSource,
}

#[cfg(feature = "devtools")]
impl DevLogEntry {
    fn new(message: String, source: LogSource) -> Self {
        Self { message, source }
    }
}

#[cfg(feature = "devtools")]
type DevtoolsLogSink = Box<dyn FnMut(DevLogEntry) + Send>;

#[cfg(feature = "devtools")]
static DEVTOOLS_LOG_SINK: LazyLock<Mutex<Option<DevtoolsLogSink>>> =
    LazyLock::new(|| Mutex::new(None));

struct DebugLogger {
    enabled: bool,
    #[cfg(not(target_arch = "wasm32"))]
    file: Option<std::fs::File>,
}

impl DebugLogger {
    fn from_env() -> Self {
        let enabled = std::env::var(DEBUG_ENV).as_deref() == Ok("1");
        #[cfg(not(target_arch = "wasm32"))]
        let file = if enabled {
            match std::env::var(DEBUG_FILE_ENV) {
                Ok(path) => {
                    let path = path.trim();
                    if path.is_empty() {
                        None
                    } else {
                        OpenOptions::new().create(true).append(true).open(path).ok()
                    }
                }
                Err(_) => None,
            }
        } else {
            None
        };

        #[cfg(not(target_arch = "wasm32"))]
        return Self { enabled, file };
        #[cfg(target_arch = "wasm32")]
        return Self { enabled };
    }

    fn log(&mut self, args: fmt::Arguments<'_>) {
        if !self.enabled {
            return;
        }

        let line = format!("{args}");
        eprintln!("{line}");
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(file) = self.file.as_mut() {
            let _ = writeln!(file, "{line}");
        }
        #[cfg(all(target_arch = "wasm32", feature = "web"))]
        {
            use wasm_bindgen::JsValue;
            web_sys::console::log_1(&JsValue::from_str(&line));
        }
    }
}

static DEBUG_LOGGER: LazyLock<Mutex<DebugLogger>> = LazyLock::new(|| {
    let logger = DebugLogger::from_env();
    DEBUG_ENABLED.store(logger.enabled, Ordering::Relaxed);
    Mutex::new(logger)
});

/// Initialize debug logging (reads env and opens file once).
pub(crate) fn init_logging() {
    let _ = LazyLock::force(&DEBUG_LOGGER);
}

/// Returns whether debug logging is enabled.
pub(crate) fn enabled() -> bool {
    DEBUG_ENABLED.load(Ordering::Relaxed)
}

#[cfg(feature = "devtools")]
fn push_devtools_log_entry(entry: DevLogEntry) {
    if DEVTOOLS_LOG_SUPPRESSED.with(|cell| cell.get()) {
        return;
    }
    if let Ok(mut sink) = DEVTOOLS_LOG_SINK.lock()
        && let Some(sink) = sink.as_mut()
    {
        sink(entry);
    }
}

#[cfg(feature = "devtools")]
pub(crate) fn set_devtools_logs_enabled(enabled: bool) {
    DEVTOOLS_LOGS_ENABLED.store(enabled, Ordering::Relaxed);
}

#[doc(hidden)]
pub fn __emit_debug_log(args: fmt::Arguments<'_>) {
    log(args, LogSource::App);
}

#[doc(hidden)]
pub fn __emit_internal_log(args: fmt::Arguments<'_>) {
    log(args, LogSource::Framework);
}

#[cfg(feature = "devtools")]
pub(crate) fn set_devtools_log_sink<F>(sink: F)
where
    F: FnMut(DevLogEntry) + Send + 'static,
{
    if let Ok(mut slot) = DEVTOOLS_LOG_SINK.lock() {
        *slot = Some(Box::new(sink));
    }
}

#[cfg(feature = "devtools")]
pub(crate) fn clear_devtools_log_sink() {
    if let Ok(mut slot) = DEVTOOLS_LOG_SINK.lock() {
        *slot = None;
    }
}

/// Emit a debug log line if enabled.
pub(crate) fn log(args: fmt::Arguments<'_>, source: LogSource) {
    #[cfg(not(feature = "devtools"))]
    let _ = source;

    #[cfg(feature = "devtools")]
    {
        if !DEVTOOLS_LOGS_ENABLED.load(Ordering::Relaxed) {
            if !enabled() {
                return;
            }
            if let Ok(mut logger) = DEBUG_LOGGER.lock() {
                logger.log(args);
            }
            return;
        }

        let message = format!("{args}");
        push_devtools_log_entry(DevLogEntry::new(message.clone(), source));

        if !enabled() {
            return;
        }
        if let Ok(mut logger) = DEBUG_LOGGER.lock() {
            logger.log(format_args!("{message}"));
        }
    }

    #[cfg(not(feature = "devtools"))]
    {
        if !enabled() {
            return;
        }
        if let Ok(mut logger) = DEBUG_LOGGER.lock() {
            logger.log(args);
        }
    }
}

/// Debug logging macro (enabled by TUI_LIPAN_DEBUG=1).
#[macro_export]
macro_rules! debug_log {
    ($($arg:tt)*) => {
        $crate::debug::__emit_debug_log(::std::format_args!($($arg)*))
    };
}

/// Framework-internal logging macro.
///
/// Identical to [`debug_log!`] but tags the line as [`LogSource::Framework`] so
/// the devtools log view can filter it out from host-application logs. Use this
/// (not `debug_log!`) for all logging inside tui-lipan itself.
macro_rules! internal_log {
    ($($arg:tt)*) => {
        $crate::debug::__emit_internal_log(::std::format_args!($($arg)*))
    };
}

pub(crate) use internal_log;

/// Increment the mouse events processed counter.
pub(crate) fn increment_mouse_events() {
    MOUSE_EVENTS_PROCESSED.fetch_add(1, Ordering::Relaxed);
}

/// Get the total number of mouse events processed.
///
/// This includes all mouse events (move, click, drag, scroll) that the framework
/// has received and processed, even if they didn't trigger a render.
pub fn mouse_events_processed() -> usize {
    MOUSE_EVENTS_PROCESSED.load(Ordering::Relaxed)
}

/// Reset the mouse events counter to zero.
///
/// This is useful when starting a new measurement period.
pub fn reset_mouse_events() {
    MOUSE_EVENTS_PROCESSED.store(0, Ordering::Relaxed);
}

#[cfg(all(test, feature = "devtools", not(target_arch = "wasm32")))]
mod devtools_tests {
    use std::sync::LazyLock;
    use std::sync::{Arc, Mutex};

    use super::{
        LogSource, clear_devtools_log_sink, log, set_devtools_log_sink, set_devtools_logs_enabled,
    };

    static DEVTOOLS_SINK_TEST_GUARD: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn log_pushes_to_devtools_sink_without_env_logging() {
        let _guard = DEVTOOLS_SINK_TEST_GUARD
            .lock()
            .expect("devtools sink test guard poisoned");
        set_devtools_logs_enabled(true);
        clear_devtools_log_sink();

        let seen = Arc::new(Mutex::new(Vec::new()));
        set_devtools_log_sink({
            let seen = Arc::clone(&seen);
            move |entry| {
                seen.lock().expect("log sink lock poisoned").push(entry);
            }
        });

        log(format_args!("devtools {}", 42), LogSource::App);

        let seen = seen.lock().expect("log sink lock poisoned");
        let devtools_42: Vec<_> = seen.iter().filter(|e| e.message == "devtools 42").collect();
        assert_eq!(
            devtools_42.len(),
            1,
            "parallel tests may share the global devtools sink; got entries: {seen:?}"
        );

        clear_devtools_log_sink();
    }

    #[test]
    fn clear_devtools_sink_stops_new_entries() {
        let _guard = DEVTOOLS_SINK_TEST_GUARD
            .lock()
            .expect("devtools sink test guard poisoned");
        set_devtools_logs_enabled(true);
        clear_devtools_log_sink();

        let seen = Arc::new(Mutex::new(Vec::new()));
        set_devtools_log_sink({
            let seen = Arc::clone(&seen);
            move |entry| {
                seen.lock().expect("log sink lock poisoned").push(entry);
            }
        });

        log(format_args!("first"), LogSource::App);
        clear_devtools_log_sink();
        log(format_args!("second"), LogSource::App);

        let seen = seen.lock().expect("log sink lock poisoned");
        assert!(
            seen.iter().any(|e| e.message == "first"),
            "expected first log in sink, got: {seen:?}"
        );
        assert!(
            !seen.iter().any(|e| e.message == "second"),
            "sink was cleared; second log should not be captured, got: {seen:?}"
        );
    }

    #[test]
    fn background_threads_also_push_to_devtools_sink() {
        let _guard = DEVTOOLS_SINK_TEST_GUARD
            .lock()
            .expect("devtools sink test guard poisoned");
        set_devtools_logs_enabled(true);
        clear_devtools_log_sink();

        let seen = Arc::new(Mutex::new(Vec::new()));
        set_devtools_log_sink({
            let seen = Arc::clone(&seen);
            move |entry| {
                seen.lock().expect("log sink lock poisoned").push(entry);
            }
        });

        std::thread::spawn(|| {
            log(format_args!("from worker thread"), LogSource::App);
        })
        .join()
        .expect("worker thread should log successfully");

        let seen = seen.lock().expect("log sink lock poisoned");
        let from_worker: Vec<_> = seen
            .iter()
            .filter(|e| e.message == "from worker thread")
            .collect();
        assert_eq!(
            from_worker.len(),
            1,
            "parallel tests may share the global devtools sink; got entries: {seen:?}"
        );

        clear_devtools_log_sink();
    }

    #[test]
    fn log_skips_devtools_sink_when_logs_disabled() {
        let _guard = DEVTOOLS_SINK_TEST_GUARD
            .lock()
            .expect("devtools sink test guard poisoned");
        clear_devtools_log_sink();

        let seen = Arc::new(Mutex::new(Vec::new()));
        set_devtools_log_sink({
            let seen = Arc::clone(&seen);
            move |entry| {
                seen.lock().expect("log sink lock poisoned").push(entry);
            }
        });

        set_devtools_logs_enabled(false);
        log(format_args!("should-not-hit-sink"), LogSource::App);
        set_devtools_logs_enabled(true);

        let seen = seen.lock().expect("log sink lock poisoned");
        assert!(
            !seen.iter().any(|e| e.message == "should-not-hit-sink"),
            "devtools logs disabled should skip sink, got: {seen:?}"
        );

        clear_devtools_log_sink();
    }
}
