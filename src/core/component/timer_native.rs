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
    queue: Mutex<TimerQueue>,
    wakeup: Condvar,
}

#[derive(Default)]
struct TimerQueue {
    entries: BinaryHeap<Reverse<Entry>>,
}

/// Rounds [`TimerService::advance`] will resolve chained timers for. Enough for any realistic chain
/// of same-instant tasks, low enough to break a task that re-arms itself inside the advance horizon
/// rather than hang the harness driving it.
const MAX_ADVANCE_ROUNDS: usize = 64;

struct Entry {
    due: Instant,
    /// Tie-break so equal deadlines keep submission order and `Ord` stays total.
    seq: u64,
    /// Which runtime armed this timer, so a virtual advance only claims its own.
    ///
    /// One process can hold several runtimes - a test binary runs them in parallel threads - and the
    /// queue is shared. Without this, one runtime skipping virtual time would fire every other
    /// runtime's pending timers early, delivering messages into harnesses that never asked for them.
    owner: Option<super::RuntimeId>,
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
            queue: Mutex::new(TimerQueue::default()),
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
        self.schedule_owned(delay, task, None);
    }

    /// Like [`schedule`](Self::schedule), recording which runtime armed the timer so
    /// [`advance`](Self::advance) can claim only its own.
    pub(super) fn schedule_owned(
        &self,
        delay: Duration,
        task: Task,
        owner: Option<super::RuntimeId>,
    ) {
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
        queue.entries.push(Reverse(Entry {
            due,
            seq,
            owner,
            task,
        }));
        drop(queue);
        // The sleeping thread may be waiting on a later deadline than this one.
        self.state.wakeup.notify_one();
    }

    /// Run every timer `owner` armed that is due by `horizon`, and report how many ran.
    ///
    /// This is what lets a headless capture or a `TestBackend` settle work deferred through
    /// [`Command::after`](super::Command::after). Those harnesses drive a virtual clock and never
    /// sleep, so a real deadline would simply never arrive: a pane that reveals itself on a 36 ms
    /// timer stays invisible for the whole capture, and nothing in the tree explains why.
    ///
    /// `horizon` is the caller's own virtual *now*, so each runtime advances against its own clock,
    /// and `owner` scopes the sweep to timers that runtime armed. Both matter because the queue is
    /// process-wide: a test binary runs runtimes in parallel threads, and one of them skipping a
    /// second of virtual time must not fire another's pending timers early.
    ///
    /// Tasks run **inline on the calling thread**, not on the executor, so their messages are queued
    /// by the time this returns and the caller's next drain sees them. That is the whole point - a
    /// capture cannot wait for a worker - but it does mean a long-blocking `after` callback blocks the
    /// harness.
    ///
    /// Chains resolve within one call: a task that schedules another already-due task has it run too,
    /// bounded by [`MAX_ADVANCE_ROUNDS`] so a task that keeps re-arming itself inside the horizon
    /// cannot spin forever.
    ///
    /// Cancellation behaves exactly as it does on the executor: the task body runs either way, and a
    /// cancelled command drops its messages at the [`CommandLink`](crate::CommandLink) rather than
    /// being skipped here. Checking the token at this point would make a virtual advance behave
    /// differently from a real one, which is the one thing this must not do.
    pub(super) fn advance_owned(&self, horizon: Instant, owner: super::RuntimeId) -> usize {
        let mut ran = 0;
        for _ in 0..MAX_ADVANCE_ROUNDS {
            let Ok(mut queue) = self.state.queue.lock() else {
                break;
            };
            // Anything not ours is put straight back: the heap is ordered by deadline, so an earlier
            // timer belonging to another runtime would otherwise hide ours behind it.
            let mut due = Vec::new();
            let mut others = Vec::new();
            while matches!(queue.entries.peek(), Some(Reverse(entry)) if entry.due <= horizon) {
                let Some(Reverse(entry)) = queue.entries.pop() else {
                    break;
                };
                if entry.owner == Some(owner) {
                    due.push(entry.task);
                } else {
                    others.push(Reverse(entry));
                }
            }
            queue.entries.extend(others);
            // Released before running anything: a task is free to schedule another, which takes this
            // same lock.
            drop(queue);
            if due.is_empty() {
                break;
            }
            for task in due {
                task.run();
                ran += 1;
            }
        }
        ran
    }
}

fn run_timer(state: &Arc<TimerState>) {
    loop {
        let Ok(mut queue) = state.queue.lock() else {
            return;
        };
        loop {
            let now = Instant::now();
            let wait = match queue.entries.peek() {
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
        while matches!(queue.entries.peek(), Some(Reverse(entry)) if entry.due <= now) {
            if let Some(Reverse(entry)) = queue.entries.pop() {
                due.push(entry.task);
            }
        }
        drop(queue);
        for task in due {
            super::TaskExecutor::global().execute(task);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// A private service rather than the global one, so the process-wide singleton other tests share
    /// is never advanced out from under them.
    fn service() -> TimerService {
        TimerService::new()
    }

    fn owner(id: u64) -> super::super::RuntimeId {
        super::super::RuntimeId::from_raw_for_tests(id)
    }

    fn counting_task(runs: &Arc<AtomicUsize>) -> Task {
        let runs = Arc::clone(runs);
        Task::new(move || {
            runs.fetch_add(1, Ordering::SeqCst);
        })
    }

    fn horizon(after: Duration) -> Instant {
        Instant::now() + after
    }

    #[test]
    fn advance_runs_a_task_once_the_horizon_reaches_it() {
        let timer = service();
        let runs = Arc::new(AtomicUsize::new(0));
        timer.schedule_owned(
            Duration::from_millis(200),
            counting_task(&runs),
            Some(owner(1)),
        );

        assert_eq!(
            timer.advance_owned(horizon(Duration::from_millis(50)), owner(1)),
            0
        );
        assert_eq!(runs.load(Ordering::SeqCst), 0, "not due yet");

        assert_eq!(
            timer.advance_owned(horizon(Duration::from_millis(250)), owner(1)),
            1
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);

        // Already drained, so advancing further finds nothing to do.
        assert_eq!(
            timer.advance_owned(horizon(Duration::from_millis(500)), owner(1)),
            0
        );
        assert_eq!(runs.load(Ordering::SeqCst), 1);
    }

    /// The regression that made this scoping necessary: one runtime skipping virtual time must not
    /// fire another's timers. A test binary runs runtimes in parallel over this shared queue, and
    /// firing a stranger's timer delivers a message into a harness that never asked for it.
    #[test]
    fn advance_only_claims_timers_the_advancing_runtime_armed() {
        let timer = service();
        let mine = Arc::new(AtomicUsize::new(0));
        let theirs = Arc::new(AtomicUsize::new(0));

        timer.schedule_owned(
            Duration::from_millis(10),
            counting_task(&theirs),
            Some(owner(2)),
        );
        timer.schedule_owned(
            Duration::from_millis(20),
            counting_task(&mine),
            Some(owner(1)),
        );

        // A generous horizon covers both, yet only one runs - and the other runtime's earlier deadline
        // must not hide mine behind it in the heap.
        assert_eq!(
            timer.advance_owned(horizon(Duration::from_secs(5)), owner(1)),
            1
        );
        assert_eq!(mine.load(Ordering::SeqCst), 1);
        assert_eq!(theirs.load(Ordering::SeqCst), 0, "not this runtime's timer");

        // Still queued for its owner.
        assert_eq!(
            timer.advance_owned(horizon(Duration::from_secs(5)), owner(2)),
            1
        );
        assert_eq!(theirs.load(Ordering::SeqCst), 1);
    }

    /// `CommandLink::send_after` and other crate-internal paths schedule without an owner. They belong
    /// to the real timer thread, so no virtual advance may claim them.
    #[test]
    fn an_unowned_timer_is_left_to_the_timer_thread() {
        let timer = service();
        let runs = Arc::new(AtomicUsize::new(0));
        timer.schedule(Duration::from_millis(10), counting_task(&runs));

        assert_eq!(
            timer.advance_owned(horizon(Duration::from_secs(5)), owner(1)),
            0
        );
        assert_eq!(runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn advance_resolves_a_chain_within_one_call() {
        let timer = service();
        let runs = Arc::new(AtomicUsize::new(0));

        // The shape rozi hit: a task that arms a follow-up. Both fall inside this advance's horizon,
        // so both belong to it - a caller should not have to guess how many advances a chain needs.
        let state = Arc::clone(&timer.state);
        let counter = Arc::clone(&runs);
        timer.schedule_owned(
            Duration::from_millis(10),
            Task::new(move || {
                counter.fetch_add(1, Ordering::SeqCst);
                let follow_up = counting_task(&counter);
                TimerService { state }.schedule_owned(
                    Duration::from_millis(5),
                    follow_up,
                    Some(owner(1)),
                );
            }),
            Some(owner(1)),
        );

        assert_eq!(
            timer.advance_owned(horizon(Duration::from_millis(50)), owner(1)),
            2
        );
        assert_eq!(runs.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn advance_stops_rearming_at_its_round_bound_instead_of_spinning() {
        let timer = service();
        let runs = Arc::new(AtomicUsize::new(0));

        // Each run arms another task well inside the horizon, so without a bound this never returns.
        // A short real delay keeps the timer thread parked while the advance drains rounds.
        fn arm(state: Arc<TimerState>, runs: Arc<AtomicUsize>) {
            let next_state = Arc::clone(&state);
            TimerService { state }.schedule_owned(
                Duration::from_millis(2),
                Task::new(move || {
                    runs.fetch_add(1, Ordering::SeqCst);
                    arm(Arc::clone(&next_state), Arc::clone(&runs));
                }),
                Some(owner(1)),
            );
        }
        arm(Arc::clone(&timer.state), Arc::clone(&runs));

        let ran = timer.advance_owned(horizon(Duration::from_millis(50)), owner(1));
        assert_eq!(
            ran, MAX_ADVANCE_ROUNDS,
            "the advance should stop at its bound"
        );
        assert_eq!(runs.load(Ordering::SeqCst), MAX_ADVANCE_ROUNDS);
    }
}
