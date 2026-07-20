use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use super::task_policy::Task;

/// Delayed-task scheduler backing [`Command::after`](super::Command::after).
///
/// One thread owns every pending timer. Without it the only way to delay work is to sleep inside a
/// task, which parks one of the executor's 2-8 workers for the whole delay: two recurring timers
/// are enough to starve the pool on a low-core machine and stall unrelated background work behind
/// them. Sleeping here costs no worker, and firing hands the task to the normal executor so a slow
/// callback cannot delay later timers either.
pub(super) struct TimerService {
    state: Arc<TimerState>,
}

struct TimerState {
    queue: Mutex<BinaryHeap<Reverse<Entry>>>,
    wakeup: Condvar,
}

struct Entry {
    due: Instant,
    /// Tie-break so equal deadlines keep submission order and `Ord` stays total.
    seq: u64,
    task: Task,
}

impl PartialEq for Entry {
    fn eq(&self, other: &Self) -> bool {
        self.due == other.due && self.seq == other.seq
    }
}

impl Eq for Entry {}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.due.cmp(&other.due).then(self.seq.cmp(&other.seq))
    }
}

impl TimerService {
    pub(super) fn global() -> &'static Self {
        static TIMER: OnceLock<TimerService> = OnceLock::new();
        TIMER.get_or_init(Self::new)
    }

    fn new() -> Self {
        let state = Arc::new(TimerState {
            queue: Mutex::new(BinaryHeap::new()),
            wakeup: Condvar::new(),
        });
        let worker = Arc::clone(&state);
        let _ = std::thread::Builder::new()
            .name("tui-lipan-timer".to_string())
            .spawn(move || run_timer(&worker));
        Self { state }
    }

    /// Queue `task` to be handed to the executor once `delay` has elapsed.
    ///
    /// A zero delay still goes through the queue rather than running inline, so callers cannot
    /// accidentally run a "delayed" task synchronously inside `update()`.
    pub(super) fn schedule(&self, delay: Duration, task: Task) {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let due = Instant::now()
            .checked_add(delay)
            .unwrap_or_else(Instant::now);
        let Ok(mut queue) = self.state.queue.lock() else {
            // A poisoned queue means the timer thread is gone; cancelling is closer to the caller's
            // intent than silently dropping a task it believes is pending.
            task.cancel();
            return;
        };
        queue.push(Reverse(Entry { due, seq, task }));
        drop(queue);
        // The sleeping thread may be waiting on a later deadline than this one.
        self.state.wakeup.notify_one();
    }
}

fn run_timer(state: &Arc<TimerState>) {
    loop {
        let Ok(mut queue) = state.queue.lock() else {
            return;
        };
        loop {
            let now = Instant::now();
            let wait = match queue.peek() {
                Some(Reverse(entry)) if entry.due <= now => break,
                Some(Reverse(entry)) => entry.due.saturating_duration_since(now),
                // Nothing pending: park until something is scheduled.
                None => Duration::from_secs(3600),
            };
            let Ok((next, _)) = state.wakeup.wait_timeout(queue, wait) else {
                return;
            };
            queue = next;
        }
        // Drain everything already due in this wakeup rather than reacquiring per entry.
        let mut due = Vec::new();
        let now = Instant::now();
        while matches!(queue.peek(), Some(Reverse(entry)) if entry.due <= now) {
            if let Some(Reverse(entry)) = queue.pop() {
                due.push(entry.task);
            }
        }
        drop(queue);
        for task in due {
            super::TaskExecutor::global().execute(task);
        }
    }
}
