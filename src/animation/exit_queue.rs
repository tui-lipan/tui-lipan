//! State-owned storage for keyed entries that are leaving with an [`Animated`](crate::widgets::Animated)
//! wrapper.

use std::time::Duration;

use web_time::Instant;

struct Entry<K, T> {
    key: K,
    value: T,
    visible: bool,
    /// When this entry stops being retained even if [`ExitQueue::finish`] never arrives.
    ///
    /// Exit animations only complete while their host is actually being rendered. A collection
    /// living in a hidden tab, an inactive workspace, or a collapsed panel never ticks, so an
    /// entry waiting purely on completion would be retained forever. Set from
    /// [`ExitQueue::with_exit_timeout`].
    deadline: Option<Instant>,
}

/// An entry removed from one [`ExitQueue`] to be handed to another, preserving its exit progress.
pub struct ExitTransfer<K, T> {
    key: K,
    value: T,
    visible: bool,
    deadline: Option<Instant>,
}

impl<K, T> ExitTransfer<K, T> {
    /// The key this entry was stored under.
    pub fn key(&self) -> &K {
        &self.key
    }

    /// The transferred value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Whether the entry was still live rather than exiting.
    pub fn is_visible(&self) -> bool {
        self.visible
    }

    /// Consume the transfer, yielding its key and value.
    pub fn into_parts(self) -> (K, T) {
        (self.key, self.value)
    }
}

/// A keyed collection that keeps removed entries available for exit animations.
///
/// Call [`ExitQueue::sync`] with the current live `(key, value)` pairs after each state update.
/// Entries absent from the live set remain in the collection with `visible == false` until
/// [`ExitQueue::finish`] removes them. If a key is added again before its exit completes, the
/// existing entry is updated and resurrected instead of being inserted a second time.
#[derive(Default)]
pub struct ExitQueue<K: Eq, T> {
    entries: Vec<Entry<K, T>>,
    exit_timeout: Option<Duration>,
}

impl<K: Eq, T> ExitQueue<K, T> {
    /// Create an empty keyed exit queue with no timeout.
    ///
    /// Entries are then retained until [`ExitQueue::finish`] is called for them. Prefer
    /// [`ExitQueue::with_exit_timeout`] unless the host is guaranteed to stay rendered for the
    /// whole exit, since an animation that never runs never completes.
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            exit_timeout: None,
        }
    }

    /// Create a queue that stops retaining an entry `timeout` after it began exiting.
    ///
    /// The timeout is a backstop, not the animation duration: [`ExitQueue::finish`] still removes
    /// entries as soon as their animation reports completion. Set it slightly longer than the exit
    /// animation so a slow frame cannot cut one short. Without it, a collection that stops being
    /// rendered mid-exit (a hidden tab, an inactive workspace) retains those entries indefinitely.
    pub fn with_exit_timeout(timeout: Duration) -> Self {
        Self {
            entries: Vec::new(),
            exit_timeout: Some(timeout),
        }
    }

    /// Drop entries whose exit timeout has elapsed, returning how many were released.
    ///
    /// [`ExitQueue::sync`] calls this, so it is only needed when a queue can go many frames
    /// without syncing and you still want its memory released.
    pub fn expire(&mut self) -> usize {
        if self.exit_timeout.is_none() {
            return 0;
        }
        let now = Instant::now();
        let before = self.entries.len();
        self.entries
            .retain(|entry| entry.visible || entry.deadline.is_none_or(|deadline| now < deadline));
        before - self.entries.len()
    }

    /// Remove an entry so another queue can adopt it, preserving its exit progress.
    ///
    /// Use when an item migrates between collections while leaving, for example a row that moves
    /// to another group as it is being deleted. Re-inserting it with [`ExitQueue::sync`] on the
    /// target would restart its exit; [`ExitQueue::adopt`] preserves the original deadline.
    pub fn transfer_out(&mut self, key: &K) -> Option<ExitTransfer<K, T>> {
        let index = self.entries.iter().position(|entry| entry.key == *key)?;
        let entry = self.entries.remove(index);
        Some(ExitTransfer {
            key: entry.key,
            value: entry.value,
            visible: entry.visible,
            deadline: entry.deadline,
        })
    }

    /// Adopt an entry handed over by [`ExitQueue::transfer_out`].
    ///
    /// An existing entry under the same key is replaced. The adopted deadline is kept, so an item
    /// cannot extend its own retention by hopping between collections.
    pub fn adopt(&mut self, transfer: ExitTransfer<K, T>) {
        self.entries.retain(|entry| entry.key != transfer.key);
        self.entries.push(Entry {
            key: transfer.key,
            value: transfer.value,
            visible: transfer.visible,
            deadline: transfer.deadline,
        });
    }

    /// Synchronize the queue with the current live `(key, value)` pairs.
    ///
    /// Existing keys are updated in place. Keys not present in `live` become exiting, while new
    /// keys are inserted as visible. Duplicate keys in one input are updated rather than doubled.
    pub fn sync<I>(&mut self, live: I)
    where
        K: Clone,
        I: IntoIterator<Item = (K, T)>,
    {
        let now = Instant::now();
        let deadline = self.exit_timeout.map(|timeout| now + timeout);
        for entry in &mut self.entries {
            if entry.visible {
                // Only entries that are newly leaving get a fresh deadline; one already exiting
                // keeps its original so repeated syncs cannot extend its retention indefinitely.
                entry.deadline = deadline;
            }
            entry.visible = false;
        }

        for (key, value) in live {
            if let Some(entry) = self.entries.iter_mut().find(|entry| entry.key == key) {
                entry.value = value;
                entry.visible = true;
                entry.deadline = None;
            } else {
                self.entries.push(Entry {
                    key,
                    value,
                    visible: true,
                    deadline: None,
                });
            }
        }

        self.expire();
    }

    /// Iterate over `(key, value, visible)` entries in insertion order.
    ///
    /// `visible` is `true` for current live entries and `false` for entries retained only for an
    /// exit animation.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &T, bool)> + '_ {
        self.entries
            .iter()
            .map(|entry| (&entry.key, &entry.value, entry.visible))
    }

    /// Finish an entry's exit animation and remove it from the queue.
    ///
    /// Returns `true` when an exiting entry was removed. Calling this for a live entry or an
    /// unknown key is a no-op and returns `false`.
    pub fn finish(&mut self, key: &K) -> bool {
        let Some(index) = self
            .entries
            .iter()
            .position(|entry| !entry.visible && entry.key == *key)
        else {
            return false;
        };
        self.entries.remove(index);
        true
    }

    /// Return whether a key currently exists only for its exit animation.
    pub fn is_exiting(&self, key: &K) -> bool {
        self.entries
            .iter()
            .any(|entry| !entry.visible && entry.key == *key)
    }

    /// Return the number of live and exiting entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return whether the collection has no live or exiting entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::ExitQueue;

    fn entries(queue: &ExitQueue<u32, &'static str>) -> Vec<(u32, &'static str, bool)> {
        queue
            .iter()
            .map(|(key, value, visible)| (*key, *value, visible))
            .collect()
    }

    #[test]
    fn sync_marks_removed_entries_exiting_without_dropping_them() {
        let mut queue = ExitQueue::new();
        queue.sync([(1, "one"), (2, "two")]);
        queue.sync([(2, "updated")]);

        assert_eq!(
            entries(&queue),
            vec![(1, "one", false), (2, "updated", true)]
        );
        assert!(queue.is_exiting(&1));
        assert!(!queue.is_exiting(&2));
        assert_eq!(queue.len(), 2);
    }

    #[test]
    fn readding_an_exiting_key_resurrects_the_existing_entry() {
        let mut queue = ExitQueue::new();
        queue.sync([(7, "old")]);
        queue.sync([]);
        queue.sync([(7, "new")]);

        assert_eq!(entries(&queue), vec![(7, "new", true)]);
        assert!(!queue.is_exiting(&7));
        assert_eq!(queue.len(), 1);
    }

    #[test]
    fn duplicate_live_keys_do_not_double_insert() {
        let mut queue = ExitQueue::new();
        queue.sync([(3, "first"), (3, "last")]);

        assert_eq!(entries(&queue), vec![(3, "last", true)]);
    }

    #[test]
    fn an_exit_timeout_releases_entries_whose_animation_never_completes() {
        // A collection in a hidden tab never ticks, so `finish` never arrives.
        let mut queue: ExitQueue<u32, &'static str> =
            ExitQueue::with_exit_timeout(std::time::Duration::from_millis(10));
        queue.sync([(1, "one"), (2, "two")]);
        queue.sync([(2, "two")]);
        assert!(queue.is_exiting(&1));

        std::thread::sleep(std::time::Duration::from_millis(25));
        queue.sync([(2, "two")]);

        assert!(!queue.is_exiting(&1));
        assert_eq!(entries(&queue), vec![(2, "two", true)]);
    }

    #[test]
    fn repeated_syncs_do_not_extend_an_exiting_entrys_deadline() {
        let mut queue: ExitQueue<u32, &'static str> =
            ExitQueue::with_exit_timeout(std::time::Duration::from_millis(20));
        queue.sync([(1, "one")]);
        queue.sync([]);

        for _ in 0..4 {
            std::thread::sleep(std::time::Duration::from_millis(8));
            queue.sync([]);
        }

        assert!(queue.is_empty(), "deadline must not be pushed forward");
    }

    #[test]
    fn transfer_preserves_exit_progress_across_collections() {
        let mut source: ExitQueue<u32, &'static str> =
            ExitQueue::with_exit_timeout(std::time::Duration::from_millis(500));
        let mut target: ExitQueue<u32, &'static str> =
            ExitQueue::with_exit_timeout(std::time::Duration::from_millis(500));
        source.sync([(1, "one")]);
        source.sync([]);
        assert!(source.is_exiting(&1));

        let moved = source.transfer_out(&1).expect("entry exists");
        assert!(!moved.is_visible());
        target.adopt(moved);

        assert!(source.is_empty());
        // Still exiting rather than restarted as a live entry.
        assert!(target.is_exiting(&1));
        assert_eq!(entries(&target), vec![(1, "one", false)]);
    }

    #[test]
    fn finish_removes_only_the_exiting_entry() {
        let mut queue = ExitQueue::new();
        queue.sync([(1, "one"), (2, "two")]);
        queue.sync([(2, "two")]);

        assert!(!queue.finish(&2));
        assert!(queue.finish(&1));
        assert!(!queue.finish(&1));
        assert_eq!(entries(&queue), vec![(2, "two", true)]);
    }
}
