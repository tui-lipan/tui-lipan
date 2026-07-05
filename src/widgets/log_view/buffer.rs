use std::collections::VecDeque;
use std::sync::Arc;

/// Severity level for a log line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LogLevel {
    /// Very verbose diagnostic information.
    Trace,
    /// Debug-level information.
    Debug,
    /// Informational message.
    Info,
    /// Warning message.
    Warn,
    /// Error message.
    Error,
}

impl LogLevel {
    /// Uppercase label used by the default renderer.
    pub fn label(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }
}

/// One log line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogEntry {
    /// Log severity.
    pub level: LogLevel,
    /// Line text.
    pub message: Arc<str>,
}

impl LogEntry {
    /// Create a new log entry.
    pub fn new(level: LogLevel, message: impl Into<Arc<str>>) -> Self {
        Self {
            level,
            message: message.into(),
        }
    }

    /// Convenience constructor for `INFO`.
    pub fn info(message: impl Into<Arc<str>>) -> Self {
        Self::new(LogLevel::Info, message)
    }

    /// Convenience constructor for `WARN`.
    pub fn warn(message: impl Into<Arc<str>>) -> Self {
        Self::new(LogLevel::Warn, message)
    }

    /// Convenience constructor for `ERROR`.
    pub fn error(message: impl Into<Arc<str>>) -> Self {
        Self::new(LogLevel::Error, message)
    }
}

/// Efficient bounded store for streaming logs.
///
/// The buffer keeps at most `capacity` newest entries and can be paused, freezing
/// snapshots without dropping incoming lines.
#[derive(Clone, Debug)]
pub struct LogBuffer {
    entries: VecDeque<LogEntry>,
    capacity: usize,
    paused: bool,
    pause_len: usize,
}

impl LogBuffer {
    /// Create a new bounded log buffer.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            capacity: capacity.max(1),
            paused: false,
            pause_len: 0,
        }
    }

    /// Return the configured max number of entries.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Return the number of currently retained entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` when the buffer has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Push one log entry, dropping the oldest one when full.
    pub fn push(&mut self, entry: LogEntry) {
        self.entries.push_back(entry);
        while self.entries.len() > self.capacity {
            self.entries.pop_front();
        }
        if self.paused {
            self.pause_len = self.pause_len.min(self.entries.len());
        }
    }

    /// Extend with multiple entries.
    pub fn extend<I>(&mut self, entries: I)
    where
        I: IntoIterator<Item = LogEntry>,
    {
        for entry in entries {
            self.push(entry);
        }
    }

    /// Remove all entries.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.pause_len = 0;
    }

    /// Enable or disable pause mode.
    ///
    /// While paused, `snapshot()` returns only the lines visible at the moment of
    /// pausing. Incoming lines are still retained in the ring buffer.
    pub fn set_paused(&mut self, paused: bool) {
        if paused && !self.paused {
            self.pause_len = self.entries.len();
        }
        if !paused {
            self.pause_len = 0;
        }
        self.paused = paused;
    }

    /// Returns current pause state.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// Return a shareable snapshot for widgets.
    pub fn snapshot(&self) -> Arc<[LogEntry]> {
        let visible_len = if self.paused {
            self.pause_len.min(self.entries.len())
        } else {
            self.entries.len()
        };
        self.entries
            .iter()
            .take(visible_len)
            .cloned()
            .collect::<Vec<_>>()
            .into()
    }
}
