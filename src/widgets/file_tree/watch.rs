//! Filesystem watching for [`FileTreeRefreshPolicy::Watch`](super::FileTreeRefreshPolicy::Watch).
//!
//! The watch set is the root plus every expanded directory, one level each, so it grows with what
//! is on screen rather than with the repository. It is reconciled from tree state after every
//! update instead of being maintained by expand and collapse alone: a renamed, deleted, or replaced
//! directory then cannot leave a stale watch behind.
//!
//! Events are invalidations, never mutations. The watcher thread only records which directory
//! changed and how; the component rereads those directories. That absorbs rename pairs, atomic
//! saves through temporary files, coalesced events, and platform differences without interpreting
//! any of them.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::fs::FsNode;

/// How long changes collect before the dirty directories are reread. A batch window rather than a
/// quiet period, so a process that never stops writing still refreshes several times a second.
pub(crate) const WATCH_BATCH: Duration = Duration::from_millis(100);
/// Git status costs a subprocess per run, so file changes coalesce over a longer window.
pub(crate) const GIT_BATCH: Duration = Duration::from_millis(400);
/// The full reread that repairs missed events while watching works.
pub(crate) const REPAIR_INTERVAL: Duration = Duration::from_secs(30);
/// The full reread while some directory cannot be watched, or watching is unavailable.
pub(crate) const DEGRADED_INTERVAL: Duration = Duration::from_secs(5);
/// The shortest interval [`FileTreeRefreshPolicy::Polling`](super::FileTreeRefreshPolicy) honours.
pub(crate) const MIN_POLL_INTERVAL: Duration = Duration::from_millis(100);
/// Distinct directories recorded before a batch gives up and asks for a full reread instead.
#[cfg_attr(
    not(all(feature = "file-tree-watch", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
const MAX_PENDING_DIRS: usize = 4096;

/// What an event says about one directory.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ChangeMask {
    /// An entry was created, removed, or renamed: the listing itself changed.
    pub(crate) entries: bool,
    /// A file's contents changed: the listing is the same, Git status may not be.
    pub(crate) content: bool,
}

#[cfg_attr(
    not(all(feature = "file-tree-watch", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
impl ChangeMask {
    pub(crate) const ENTRIES: Self = Self {
        entries: true,
        content: true,
    };
    pub(crate) const CONTENT: Self = Self {
        entries: false,
        content: true,
    };

    fn merge(&mut self, other: Self) {
        self.entries |= other.entries;
        self.content |= other.content;
    }
}

/// Changes recorded by the watcher thread and taken by the component on flush.
#[derive(Debug, Default)]
pub(crate) struct PendingChanges {
    dirs: HashMap<Arc<str>, ChangeMask>,
    rescan: bool,
    /// Whether the component has been told about this batch. Only the first change after a flush
    /// sends a message, so an event storm costs one message per batch instead of one per event.
    notified: bool,
}

pub(crate) type SharedPending = Arc<Mutex<PendingChanges>>;

#[cfg_attr(
    not(all(feature = "file-tree-watch", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
impl PendingChanges {
    /// Record changes to directories. Returns whether the component needs waking.
    pub(crate) fn record(
        &mut self,
        changes: impl IntoIterator<Item = (Arc<str>, ChangeMask)>,
    ) -> bool {
        if !self.rescan {
            for (dir, mask) in changes {
                self.dirs.entry(dir).or_default().merge(mask);
            }
            if self.dirs.len() > MAX_PENDING_DIRS {
                self.dirs.clear();
                self.rescan = true;
            }
        }
        self.wake()
    }

    /// Record that events were lost and only a full reread is trustworthy.
    pub(crate) fn record_rescan(&mut self) -> bool {
        self.dirs.clear();
        self.rescan = true;
        self.wake()
    }

    /// Take the batch and rearm the wake-up for the next change.
    pub(crate) fn take(&mut self) -> (HashMap<Arc<str>, ChangeMask>, bool) {
        self.notified = false;
        (
            std::mem::take(&mut self.dirs),
            std::mem::take(&mut self.rescan),
        )
    }

    fn wake(&mut self) -> bool {
        !std::mem::replace(&mut self.notified, true)
    }
}

/// What one flush has to do.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct FlushPlan {
    /// Visible directories whose listing must be reread, sorted.
    pub(crate) dirs: Vec<Arc<str>>,
    /// Whether Git status may have changed.
    pub(crate) git: bool,
    /// Whether events were lost, so the whole visible tree must be reread.
    pub(crate) rescan: bool,
}

/// Sort a batch into listing rereads and a Git refresh.
///
/// A change in a Git metadata directory only touches Git status. A change in a visible directory
/// rereads it when entries changed, and touches Git status either way. Changes to directories no
/// longer visible are dropped: they were watched when the event fired and collapsed since.
pub(crate) fn plan_flush(
    changes: HashMap<Arc<str>, ChangeMask>,
    rescan: bool,
    entry_dirs: &HashSet<Arc<str>>,
    git_dirs: &HashSet<Arc<str>>,
) -> FlushPlan {
    let mut plan = FlushPlan {
        rescan,
        git: rescan,
        ..FlushPlan::default()
    };
    for (dir, mask) in changes {
        if git_dirs.contains(&dir) {
            plan.git = true;
        }
        if entry_dirs.contains(&dir) {
            plan.git |= mask.content || mask.entries;
            if mask.entries {
                plan.dirs.push(dir);
            }
        }
    }
    plan.dirs.sort();
    plan
}

/// The directories a watching tree should observe: the root and every expanded directory reachable
/// through expanded ancestors. A directory that failed to list is left out; it cannot be watched
/// either, and the periodic reread retries it.
pub(crate) fn visible_directories(
    root: &FsNode,
    expanded: &HashSet<Arc<str>>,
) -> HashSet<Arc<str>> {
    let mut dirs = HashSet::new();
    collect_visible_directories(root, expanded, &mut dirs);
    dirs
}

fn collect_visible_directories(
    node: &FsNode,
    expanded: &HashSet<Arc<str>>,
    dirs: &mut HashSet<Arc<str>>,
) {
    if !node.is_dir() || node.error.is_some() || !expanded.contains(&node.path) {
        return;
    }
    dirs.insert(node.path.clone());
    for child in &node.children {
        collect_visible_directories(child, expanded, dirs);
    }
}

/// Live watching state, kept beside the tree.
#[derive(Clone, Debug, Default)]
pub(crate) struct WatchState {
    /// Bumped whenever watching restarts from scratch. Messages from an older session are dropped.
    pub(crate) generation: u64,
    /// Bumped whenever the periodic reread is rearmed, so only one chain is ever alive.
    pub(crate) poll_generation: u64,
    pub(crate) session: Option<Arc<WatchSession>>,
    pub(crate) pending: SharedPending,
    /// Watching could not start at all.
    pub(crate) unavailable: bool,
    /// Directories currently watched, visible and Git metadata alike.
    pub(crate) watched: HashSet<Arc<str>>,
    /// Directories whose watch was refused. They are retried on the periodic reread.
    pub(crate) failed: HashSet<Arc<str>>,
    /// Resolved Git metadata directories for the current root.
    pub(crate) git_dirs: HashSet<Arc<str>>,
    /// Visible directories waiting for a reread.
    pub(crate) dirty: HashSet<Arc<str>>,
    pub(crate) flush_scheduled: bool,
    pub(crate) reload_in_flight: bool,
    pub(crate) git_scheduled: bool,
}

impl WatchState {
    /// Drop the session and everything tied to it, and start a new generation.
    pub(crate) fn reset(&mut self) {
        *self = Self {
            generation: self.generation.wrapping_add(1),
            poll_generation: self.poll_generation.wrapping_add(1),
            ..Self::default()
        };
    }

    /// Whether some directory is not covered by a working watch.
    pub(crate) fn degraded(&self) -> bool {
        self.unavailable || !self.failed.is_empty()
    }

    /// Bring the watch set to `desired`, retrying nothing that already failed.
    pub(crate) fn reconcile(&mut self, desired: &HashSet<Arc<str>>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        self.watched.retain(|path| {
            let keep = desired.contains(path);
            if !keep {
                session.unwatch(path);
            }
            keep
        });
        self.failed.retain(|path| desired.contains(path));
        for path in desired {
            if self.watched.contains(path) || self.failed.contains(path) {
                continue;
            }
            if session.watch(path) {
                self.watched.insert(path.clone());
            } else {
                self.failed.insert(path.clone());
            }
        }
    }
}

#[cfg(all(feature = "file-tree-watch", not(target_arch = "wasm32")))]
pub(crate) use backend::WatchSession;
#[cfg(not(all(feature = "file-tree-watch", not(target_arch = "wasm32"))))]
pub(crate) use disabled::WatchSession;

#[cfg(all(feature = "file-tree-watch", not(target_arch = "wasm32")))]
mod backend {
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use notify::event::{AccessKind, AccessMode, EventKind, ModifyKind};
    use notify::{RecursiveMode, Watcher};

    use super::{ChangeMask, SharedPending};

    /// A running watcher. Dropping the last handle stops it.
    pub(crate) struct WatchSession {
        watcher: Mutex<notify::RecommendedWatcher>,
    }

    impl std::fmt::Debug for WatchSession {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("WatchSession").finish_non_exhaustive()
        }
    }

    impl WatchSession {
        /// Start a watcher that records into `pending` and calls `wake` once per batch.
        pub(crate) fn start(
            pending: SharedPending,
            wake: impl Fn() + Send + 'static,
        ) -> Result<Self, String> {
            let watcher =
                notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                    let Ok(mut pending) = pending.lock() else {
                        return;
                    };
                    let needs_wake = match result {
                        Ok(event) if event.need_rescan() => pending.record_rescan(),
                        Ok(event) => {
                            let Some(mask) = classify(&event.kind) else {
                                return;
                            };
                            pending.record(changed_directories(&event.paths, mask))
                        }
                        Err(_) => pending.record_rescan(),
                    };
                    drop(pending);
                    if needs_wake {
                        wake();
                    }
                })
                .map_err(|err| err.to_string())?;
            Ok(Self {
                watcher: Mutex::new(watcher),
            })
        }

        /// Watch one directory, one level deep. Returns whether the backend accepted it.
        pub(crate) fn watch(&self, path: &str) -> bool {
            self.watcher.lock().is_ok_and(|mut watcher| {
                watcher
                    .watch(Path::new(path), RecursiveMode::NonRecursive)
                    .is_ok()
            })
        }

        pub(crate) fn unwatch(&self, path: &str) {
            if let Ok(mut watcher) = self.watcher.lock() {
                // A directory that was deleted has already lost its watch; that error is expected.
                let _ = watcher.unwatch(Path::new(path));
            }
        }
    }

    /// Map an event to what it can have changed, or `None` when it changes nothing visible.
    ///
    /// Reads are dropped first: listing a watched directory or running `git status` opens files,
    /// and treating that as a change would refresh in a loop. Metadata-only changes are dropped for
    /// the same reason, since reading can bump access times. Closing a file opened for writing is
    /// the one access that is a change.
    pub(super) fn classify(kind: &EventKind) -> Option<ChangeMask> {
        match kind {
            EventKind::Access(AccessKind::Close(AccessMode::Write))
            | EventKind::Modify(ModifyKind::Data(_)) => Some(ChangeMask::CONTENT),
            EventKind::Access(_) | EventKind::Modify(ModifyKind::Metadata(_)) => None,
            _ => Some(ChangeMask::ENTRIES),
        }
    }

    /// The directories an event touches: each path's parent, and the path itself in case it is a
    /// watched directory that was removed or renamed.
    fn changed_directories(
        paths: &[std::path::PathBuf],
        mask: ChangeMask,
    ) -> Vec<(Arc<str>, ChangeMask)> {
        let mut dirs = Vec::with_capacity(paths.len() * 2);
        for path in paths {
            if let Some(parent) = path.parent() {
                dirs.push((Arc::from(parent.to_string_lossy().as_ref()), mask));
            }
            dirs.push((Arc::from(path.to_string_lossy().as_ref()), mask));
        }
        dirs
    }

    #[cfg(test)]
    mod tests {
        use notify::event::{CreateKind, DataChange, MetadataKind, RenameMode};

        use super::*;

        #[test]
        fn reads_and_metadata_changes_are_not_changes() {
            assert_eq!(classify(&EventKind::Access(AccessKind::Any)), None);
            assert_eq!(
                classify(&EventKind::Access(AccessKind::Open(AccessMode::Read))),
                None
            );
            assert_eq!(
                classify(&EventKind::Access(AccessKind::Close(AccessMode::Read))),
                None
            );
            assert_eq!(
                classify(&EventKind::Modify(ModifyKind::Metadata(
                    MetadataKind::AccessTime
                ))),
                None
            );
        }

        #[test]
        fn content_changes_only_touch_git_status() {
            assert_eq!(
                classify(&EventKind::Modify(ModifyKind::Data(DataChange::Content))),
                Some(ChangeMask::CONTENT)
            );
            assert_eq!(
                classify(&EventKind::Access(AccessKind::Close(AccessMode::Write))),
                Some(ChangeMask::CONTENT)
            );
            assert_eq!(
                classify(&EventKind::Create(CreateKind::File)),
                Some(ChangeMask::ENTRIES)
            );
            assert_eq!(
                classify(&EventKind::Modify(ModifyKind::Name(RenameMode::Both))),
                Some(ChangeMask::ENTRIES)
            );
        }

        #[test]
        fn a_created_file_is_reported_against_its_directory() {
            let dir = std::env::temp_dir().join(format!(
                "tui-lipan-file-tree-watch-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            let dir_key: Arc<str> = Arc::from(dir.to_string_lossy().as_ref());
            let pending = SharedPending::default();
            let (tx, rx) = std::sync::mpsc::channel();
            let session = WatchSession::start(pending.clone(), move || {
                let _ = tx.send(());
            })
            .unwrap();
            assert!(session.watch(&dir_key));

            std::fs::write(dir.join("new.rs"), "").unwrap();

            rx.recv_timeout(std::time::Duration::from_secs(5))
                .expect("watcher did not report the new file");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                let (dirs, _) = pending.lock().unwrap().take();
                if dirs.get(&dir_key).is_some_and(|mask| mask.entries) {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the new file was not recorded against its directory"
                );
                rx.recv_timeout(std::time::Duration::from_millis(100)).ok();
            }

            drop(session);
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(not(all(feature = "file-tree-watch", not(target_arch = "wasm32"))))]
mod disabled {
    use super::SharedPending;

    /// Stand-in when the `file-tree-watch` feature is off: watching never starts, and the tree
    /// falls back to the degraded polling interval.
    #[derive(Debug)]
    pub(crate) enum WatchSession {}

    impl WatchSession {
        pub(crate) fn start(
            _pending: SharedPending,
            _wake: impl Fn() + Send + 'static,
        ) -> Result<Self, String> {
            Err("the file-tree-watch feature is disabled".to_string())
        }

        pub(crate) fn watch(&self, _path: &str) -> bool {
            match *self {}
        }

        pub(crate) fn unwatch(&self, _path: &str) {
            match *self {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::widgets::file_tree::FileKind;

    fn dir(path: &str, children: Vec<FsNode>) -> FsNode {
        FsNode {
            name: Arc::from(path.rsplit('/').next().unwrap_or(path)),
            path: Arc::from(path),
            kind: FileKind::Directory,
            loaded: true,
            loading: false,
            error: None,
            children,
        }
    }

    fn set(paths: &[&str]) -> HashSet<Arc<str>> {
        paths.iter().map(|path| Arc::from(*path)).collect()
    }

    #[test]
    fn only_the_first_change_in_a_batch_wakes_the_component() {
        let mut pending = PendingChanges::default();
        assert!(pending.record([(Arc::from("/repo"), ChangeMask::ENTRIES)]));
        assert!(!pending.record([(Arc::from("/repo/src"), ChangeMask::CONTENT)]));
        assert!(!pending.record_rescan());

        let (dirs, rescan) = pending.take();
        assert!(
            dirs.is_empty() && rescan,
            "a rescan supersedes recorded directories"
        );
        assert!(pending.record([(Arc::from("/repo"), ChangeMask::ENTRIES)]));
    }

    #[test]
    fn an_overflowing_batch_turns_into_a_rescan() {
        let mut pending = PendingChanges::default();
        pending.record(
            (0..=MAX_PENDING_DIRS).map(|n| (Arc::from(format!("/repo/{n}")), ChangeMask::ENTRIES)),
        );
        let (dirs, rescan) = pending.take();
        assert!(dirs.is_empty());
        assert!(rescan);
    }

    #[test]
    fn a_flush_rereads_visible_listings_and_routes_git_metadata_to_status() {
        let changes = HashMap::from([
            (Arc::from("/repo/src"), ChangeMask::ENTRIES),
            (Arc::from("/repo"), ChangeMask::CONTENT),
            (Arc::from("/repo/.git"), ChangeMask::ENTRIES),
            (Arc::from("/repo/collapsed"), ChangeMask::ENTRIES),
        ]);
        let plan = plan_flush(
            changes,
            false,
            &set(&["/repo", "/repo/src"]),
            &set(&["/repo/.git"]),
        );
        assert_eq!(
            plan,
            FlushPlan {
                dirs: vec![Arc::from("/repo/src")],
                git: true,
                rescan: false,
            }
        );
    }

    #[test]
    fn git_metadata_alone_does_not_reread_listings() {
        let changes = HashMap::from([(Arc::from("/repo/.git"), ChangeMask::ENTRIES)]);
        let plan = plan_flush(changes, false, &set(&["/repo"]), &set(&["/repo/.git"]));
        assert!(plan.dirs.is_empty());
        assert!(plan.git);
    }

    #[test]
    fn visible_directories_follow_expansion_and_skip_unreadable_ones() {
        let mut unreadable = dir("/repo/locked", Vec::new());
        unreadable.error = Some(Arc::from("permission denied"));
        let root = dir(
            "/repo",
            vec![
                dir("/repo/src", vec![dir("/repo/src/update", Vec::new())]),
                dir("/repo/target", vec![dir("/repo/target/debug", Vec::new())]),
                unreadable,
            ],
        );
        let expanded = set(&[
            "/repo",
            "/repo/src",
            "/repo/src/update",
            "/repo/target/debug",
            "/repo/locked",
        ]);
        assert_eq!(
            visible_directories(&root, &expanded),
            set(&["/repo", "/repo/src", "/repo/src/update"]),
            "a directory under a collapsed parent is not visible"
        );
    }
}
