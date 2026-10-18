//! Filesystem watching for [`FileTreeRefreshPolicy::Watch`](super::FileTreeRefreshPolicy::Watch).
//!
//! Two questions, two watchers. *Which listings are stale* is answered by watching the root and
//! every expanded directory, one level each, so that set grows with what is on screen rather than
//! with the repository. It is reconciled from tree state after every update instead of being
//! maintained by expand and collapse alone: a renamed, deleted, or replaced directory then cannot
//! leave a stale watch behind.
//!
//! *Whether Git status is stale* is a question about the whole working tree, including directories
//! nobody has opened. A separate [`RepositoryWatch`] covers every directory whose contents can
//! change `git status` (holding tracked or non-ignored untracked files, or untracked itself), plus
//! the repository's metadata directories, and only ever invalidates Git status. Ignored output such
//! as build directories holds none of these, so it stays unwatched.
//!
//! Events are invalidations, never mutations. The watcher threads only record what changed; the
//! component rereads. That absorbs rename pairs, atomic saves through temporary files, coalesced
//! events, and platform differences without interpreting any of them.

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
/// Directories with tracked files a repository watch covers before it gives up and lets the
/// degraded interval poll Git status instead. Each is one kernel watch on Linux, and every one is
/// added again when the tracked set changes.
pub(crate) const MAX_REPOSITORY_WATCH_DIRS: usize = 4096;
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
    /// A file's contents or mode changed: the listing is the same, Git status may not be.
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

/// Changes recorded by the watcher threads and taken by the component on flush.
#[derive(Debug, Default)]
pub(crate) struct PendingChanges {
    batch: ChangeBatch,
    /// Whether the component has been told about this batch. Only the first change after a flush
    /// sends a message, so an event storm costs one message per batch instead of one per event.
    notified: bool,
}

/// One flush worth of changes.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ChangeBatch {
    /// Visible directories that changed, and how.
    pub(crate) dirs: HashMap<Arc<str>, ChangeMask>,
    /// Listing events were lost; only a full reread of the visible tree is trustworthy.
    pub(crate) rescan: bool,
    /// Something in the working tree or the repository metadata changed.
    pub(crate) git: bool,
    /// The repository metadata changed, or an entry appeared or vanished in the working tree, so
    /// the set of directories the repository watch must cover may have too.
    pub(crate) repository_changed: bool,
}

pub(crate) type SharedPending = Arc<Mutex<PendingChanges>>;

#[cfg_attr(
    not(all(feature = "file-tree-watch", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
impl PendingChanges {
    /// Record changes to visible directories. Returns whether the component needs waking.
    pub(crate) fn record(
        &mut self,
        changes: impl IntoIterator<Item = (Arc<str>, ChangeMask)>,
    ) -> bool {
        if !self.batch.rescan {
            for (dir, mask) in changes {
                self.batch.dirs.entry(dir).or_default().merge(mask);
            }
            if self.batch.dirs.len() > MAX_PENDING_DIRS {
                self.batch.dirs.clear();
                self.batch.rescan = true;
            }
        }
        self.wake()
    }

    /// Record that listing events were lost.
    pub(crate) fn record_rescan(&mut self) -> bool {
        self.batch.dirs.clear();
        self.batch.rescan = true;
        self.wake()
    }

    /// Record a change that can alter Git status, and whether it touched repository metadata.
    pub(crate) fn record_git(&mut self, repository_changed: bool) -> bool {
        self.batch.git = true;
        self.batch.repository_changed |= repository_changed;
        self.wake()
    }

    /// Take the batch and rearm the wake-up for the next change.
    pub(crate) fn take(&mut self) -> ChangeBatch {
        self.notified = false;
        std::mem::take(&mut self.batch)
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
    /// Whether listing events were lost, so the whole visible tree must be reread.
    pub(crate) rescan: bool,
    /// Whether the repository watch must re-resolve the directories it covers.
    pub(crate) repository_changed: bool,
}

/// Sort a batch into listing rereads and a Git refresh.
///
/// A change in a visible directory rereads it when entries changed, and touches Git status either
/// way. Changes to directories no longer visible are dropped from the rereads: they were watched
/// when the event fired and collapsed since.
pub(crate) fn plan_flush(batch: ChangeBatch, entry_dirs: &HashSet<Arc<str>>) -> FlushPlan {
    let mut plan = FlushPlan {
        rescan: batch.rescan,
        git: batch.git || batch.rescan,
        repository_changed: batch.repository_changed,
        ..FlushPlan::default()
    };
    for (dir, mask) in batch.dirs {
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
    /// Visible directories as of the last reconcile.
    pub(crate) visible: HashSet<Arc<str>>,
    /// Visible directories currently watched.
    pub(crate) watched: HashSet<Arc<str>>,
    /// Visible directories whose watch was refused. They are retried on the periodic reread.
    pub(crate) failed: HashSet<Arc<str>>,
    /// The Git-status watch over the whole working tree, once resolved.
    pub(crate) repository: Option<Arc<RepositoryWatch>>,
    /// The repository watch does not cover every directory with tracked files.
    pub(crate) repository_partial: bool,
    pub(crate) repository_resolving: bool,
    /// The tracked set changed again while a resolve was running.
    pub(crate) repository_resolve_again: bool,
    /// Visible directories waiting for a reread.
    pub(crate) dirty: HashSet<Arc<str>>,
    pub(crate) flush_scheduled: bool,
    pub(crate) reload_in_flight: bool,
    pub(crate) git_scheduled: bool,
}

impl WatchState {
    /// Drop both watchers and everything tied to them, and start a new generation.
    pub(crate) fn reset(&mut self) {
        *self = Self {
            generation: self.generation.wrapping_add(1),
            poll_generation: self.poll_generation.wrapping_add(1),
            ..Self::default()
        };
    }

    /// Whether some part of the tree or the repository is not covered by a working watch.
    pub(crate) fn degraded(&self) -> bool {
        self.unavailable || !self.failed.is_empty() || self.repository_partial
    }

    /// Bring the visible watch set to `desired`, retrying nothing that already failed.
    ///
    /// Returns the directories whose cached listing can no longer be trusted: those that just
    /// became visible, since nothing watched them while they were collapsed, and those that just
    /// gained a watch, since changes before it was in place went unseen.
    pub(crate) fn reconcile(&mut self, desired: HashSet<Arc<str>>) -> Vec<Arc<str>> {
        let mut stale: HashSet<Arc<str>> = desired.difference(&self.visible).cloned().collect();
        if let Some(session) = self.session.clone() {
            self.watched.retain(|path| {
                let keep = desired.contains(path);
                if !keep {
                    session.unwatch(path);
                }
                keep
            });
            self.failed.retain(|path| desired.contains(path));
            for path in &desired {
                if self.watched.contains(path) || self.failed.contains(path) {
                    continue;
                }
                if session.watch(path) {
                    self.watched.insert(path.clone());
                    stale.insert(path.clone());
                } else {
                    self.failed.insert(path.clone());
                }
            }
        }
        self.visible = desired;
        stale.into_iter().collect()
    }
}

/// The directories a repository watch covers, as resolved by Git.
#[derive(Debug, Default)]
#[cfg_attr(
    not(all(feature = "file-tree-watch", not(target_arch = "wasm32"))),
    allow(dead_code)
)]
pub(crate) struct RepositoryWatchDirs {
    /// Metadata directories: changes here can move `HEAD`, the index, or refs.
    pub(crate) metadata: HashSet<Arc<str>>,
    /// The repository root and every directory holding a tracked file, with their ancestors.
    pub(crate) worktree: HashSet<Arc<str>>,
    /// Whether `worktree` holds every such directory, rather than giving up at
    /// [`MAX_REPOSITORY_WATCH_DIRS`].
    pub(crate) complete: bool,
}

#[cfg(all(feature = "file-tree-watch", not(target_arch = "wasm32")))]
pub(crate) use backend::{RepositoryWatch, WatchSession};
#[cfg(not(all(feature = "file-tree-watch", not(target_arch = "wasm32"))))]
pub(crate) use disabled::{RepositoryWatch, WatchSession};

#[cfg(all(feature = "file-tree-watch", not(target_arch = "wasm32")))]
mod backend {
    use std::collections::HashSet;
    use std::path::Path;
    use std::sync::{Arc, Mutex};

    use notify::event::{AccessKind, AccessMode, EventKind, MetadataKind, ModifyKind};
    use notify::{RecursiveMode, Watcher};

    use super::{ChangeMask, RepositoryWatchDirs, SharedPending};

    /// A running watcher over visible directories. Dropping the last handle stops it.
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

    /// A running watcher over the working tree's tracked directories and the repository metadata.
    /// It only ever invalidates Git status. Updated from a background command, since a repository
    /// can need thousands of watches and some backends restart their stream on every one.
    pub(crate) struct RepositoryWatch {
        watcher: Mutex<notify::RecommendedWatcher>,
        /// Shared with the event handler, which needs it to tell metadata changes apart.
        metadata: Arc<Mutex<HashSet<Arc<str>>>>,
        watched: Mutex<HashSet<Arc<str>>>,
    }

    impl std::fmt::Debug for RepositoryWatch {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("RepositoryWatch").finish_non_exhaustive()
        }
    }

    impl RepositoryWatch {
        pub(crate) fn start(
            pending: SharedPending,
            wake: impl Fn() + Send + 'static,
        ) -> Result<Self, String> {
            let metadata: Arc<Mutex<HashSet<Arc<str>>>> = Arc::default();
            let handler_metadata = Arc::clone(&metadata);
            let watcher =
                notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                    let repository_changed = match &result {
                        Ok(event) if event.need_rescan() => true,
                        Ok(event) => {
                            let Some(mask) = classify(&event.kind) else {
                                return;
                            };
                            // A created or removed entry can add or retire a directory to cover,
                            // such as a new untracked one; so can any metadata change, and any
                            // edit to ignore rules, which decide what is untracked at all.
                            mask.entries || event.paths.iter().any(|path| is_ignore_file(path)) || {
                                let Ok(metadata) = handler_metadata.lock() else {
                                    return;
                                };
                                event.paths.iter().any(|path| {
                                    path.parent().is_some_and(|parent| {
                                        metadata.contains(parent.to_string_lossy().as_ref())
                                    })
                                })
                            }
                        }
                        Err(_) => true,
                    };
                    let Ok(mut pending) = pending.lock() else {
                        return;
                    };
                    let needs_wake = pending.record_git(repository_changed);
                    drop(pending);
                    if needs_wake {
                        wake();
                    }
                })
                .map_err(|err| err.to_string())?;
            Ok(Self {
                watcher: Mutex::new(watcher),
                metadata,
                watched: Mutex::default(),
            })
        }

        /// Bring the watch set to `dirs`. Returns whether every directory is watched.
        pub(crate) fn update(&self, dirs: RepositoryWatchDirs) -> bool {
            let (Ok(mut watcher), Ok(mut watched), Ok(mut metadata)) = (
                self.watcher.lock(),
                self.watched.lock(),
                self.metadata.lock(),
            ) else {
                return false;
            };
            let mut desired = dirs.worktree;
            desired.extend(dirs.metadata.iter().cloned());
            *metadata = dirs.metadata;
            drop(metadata);

            watched.retain(|path| {
                let keep = desired.contains(path);
                if !keep {
                    let _ = watcher.unwatch(Path::new(path.as_ref()));
                }
                keep
            });
            let mut complete = dirs.complete;
            for path in desired {
                if watched.contains(&path) {
                    continue;
                }
                if watcher
                    .watch(Path::new(path.as_ref()), RecursiveMode::NonRecursive)
                    .is_ok()
                {
                    watched.insert(path);
                } else {
                    complete = false;
                }
            }
            complete
        }
    }

    /// Map an event to what it can have changed, or `None` when it changes nothing visible.
    ///
    /// Reads are dropped: listing a watched directory or running `git status` opens files, and
    /// treating that as a change would refresh in a loop. Closing a file opened for writing is the
    /// one access that is a change. Metadata changes count, since Git tracks the executable bit,
    /// except access times, which reading alone can move on some backends. inotify reports every
    /// attribute change, `chmod` included, as unspecified metadata, and reading never raises one.
    pub(super) fn classify(kind: &EventKind) -> Option<ChangeMask> {
        match kind {
            EventKind::Access(AccessKind::Close(AccessMode::Write)) => Some(ChangeMask::CONTENT),
            EventKind::Access(_)
            | EventKind::Modify(ModifyKind::Metadata(MetadataKind::AccessTime)) => None,
            EventKind::Modify(ModifyKind::Data(_) | ModifyKind::Metadata(_)) => {
                Some(ChangeMask::CONTENT)
            }
            _ => Some(ChangeMask::ENTRIES),
        }
    }

    /// Whether `path` holds ignore rules that `--exclude-standard` reads from the working tree.
    /// `info/exclude` needs no check here: its directory is a metadata directory.
    fn is_ignore_file(path: &Path) -> bool {
        path.file_name().is_some_and(|name| name == ".gitignore")
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
        use notify::event::{CreateKind, DataChange, RenameMode};

        use super::*;

        fn unique_dir(label: &str) -> std::path::PathBuf {
            let dir = std::env::temp_dir().join(format!(
                "tui-lipan-file-tree-watch-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            std::fs::create_dir_all(&dir).unwrap();
            dir
        }

        /// Wait until the batch satisfies `done`, merging batches as they are taken.
        fn wait_for_batch(
            pending: &SharedPending,
            rx: &std::sync::mpsc::Receiver<()>,
            mut done: impl FnMut(&super::super::ChangeBatch) -> bool,
        ) {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                rx.recv_timeout(std::time::Duration::from_millis(100)).ok();
                let batch = pending.lock().unwrap().take();
                if done(&batch) {
                    return;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "the watcher never reported the change"
                );
            }
        }

        #[test]
        fn reads_and_access_times_are_not_changes() {
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
        fn content_and_mode_changes_only_touch_git_status() {
            assert_eq!(
                classify(&EventKind::Modify(ModifyKind::Data(DataChange::Content))),
                Some(ChangeMask::CONTENT)
            );
            assert_eq!(
                classify(&EventKind::Access(AccessKind::Close(AccessMode::Write))),
                Some(ChangeMask::CONTENT)
            );
            for metadata in [MetadataKind::Any, MetadataKind::Permissions] {
                assert_eq!(
                    classify(&EventKind::Modify(ModifyKind::Metadata(metadata))),
                    Some(ChangeMask::CONTENT),
                    "Git tracks the executable bit"
                );
            }
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
            let dir = unique_dir("create");
            let dir_key: Arc<str> = Arc::from(dir.to_string_lossy().as_ref());
            let pending = SharedPending::default();
            let (tx, rx) = std::sync::mpsc::channel();
            let session = WatchSession::start(pending.clone(), move || {
                let _ = tx.send(());
            })
            .unwrap();
            assert!(session.watch(&dir_key));

            std::fs::write(dir.join("new.rs"), "").unwrap();

            wait_for_batch(&pending, &rx, |batch| {
                batch.dirs.get(&dir_key).is_some_and(|mask| mask.entries)
            });
            drop(session);
            let _ = std::fs::remove_dir_all(dir);
        }

        #[cfg(unix)]
        #[test]
        fn making_a_file_executable_is_reported_as_a_change() {
            use std::os::unix::fs::PermissionsExt;

            let dir = unique_dir("chmod");
            let script = dir.join("script.sh");
            std::fs::write(&script, "").unwrap();
            let dir_key: Arc<str> = Arc::from(dir.to_string_lossy().as_ref());
            let pending = SharedPending::default();
            let (tx, rx) = std::sync::mpsc::channel();
            let session = WatchSession::start(pending.clone(), move || {
                let _ = tx.send(());
            })
            .unwrap();
            assert!(session.watch(&dir_key));

            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

            wait_for_batch(&pending, &rx, |batch| {
                batch.dirs.get(&dir_key).is_some_and(|mask| mask.content)
            });
            drop(session);
            let _ = std::fs::remove_dir_all(dir);
        }

        #[test]
        fn a_repository_watch_sees_changes_in_directories_nobody_opened() {
            let dir = unique_dir("repository");
            let collapsed = dir.join("src");
            let metadata = dir.join("meta");
            std::fs::create_dir_all(&collapsed).unwrap();
            std::fs::create_dir_all(&metadata).unwrap();
            std::fs::write(collapsed.join("main.rs"), "").unwrap();
            std::fs::write(dir.join(".gitignore"), "scratch/\n").unwrap();
            let key = |path: &std::path::Path| Arc::<str>::from(path.to_string_lossy().as_ref());
            let pending = SharedPending::default();
            let (tx, rx) = std::sync::mpsc::channel();
            let repository = RepositoryWatch::start(pending.clone(), move || {
                let _ = tx.send(());
            })
            .unwrap();
            assert!(repository.update(RepositoryWatchDirs {
                metadata: HashSet::from([key(&metadata)]),
                worktree: HashSet::from([key(&dir), key(&collapsed)]),
                complete: true,
            }));

            std::fs::write(collapsed.join("main.rs"), "fn main() {}\n").unwrap();
            wait_for_batch(&pending, &rx, |batch| {
                batch.git && !batch.repository_changed && batch.dirs.is_empty()
            });

            std::fs::write(metadata.join("index"), "").unwrap();
            wait_for_batch(&pending, &rx, |batch| batch.git && batch.repository_changed);

            // A new directory may need covering itself, such as an untracked one.
            std::fs::create_dir_all(dir.join("scratch")).unwrap();
            wait_for_batch(&pending, &rx, |batch| batch.git && batch.repository_changed);

            // Editing ignore rules in place is only a content change, yet it decides what is
            // untracked. Let earlier events settle so they cannot satisfy this check.
            std::thread::sleep(std::time::Duration::from_millis(200));
            pending.lock().unwrap().take();
            std::fs::write(dir.join(".gitignore"), "").unwrap();
            wait_for_batch(&pending, &rx, |batch| batch.git && batch.repository_changed);

            drop(repository);
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[cfg(not(all(feature = "file-tree-watch", not(target_arch = "wasm32"))))]
mod disabled {
    use super::{RepositoryWatchDirs, SharedPending};

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

    /// Stand-in for the repository watch when the `file-tree-watch` feature is off.
    #[derive(Debug)]
    pub(crate) enum RepositoryWatch {}

    impl RepositoryWatch {
        pub(crate) fn start(
            _pending: SharedPending,
            _wake: impl Fn() + Send + 'static,
        ) -> Result<Self, String> {
            Err("the file-tree-watch feature is disabled".to_string())
        }

        pub(crate) fn update(&self, _dirs: RepositoryWatchDirs) -> bool {
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
        assert!(!pending.record_git(false));
        assert!(!pending.record_rescan());

        let batch = pending.take();
        assert!(
            batch.dirs.is_empty() && batch.rescan && batch.git,
            "a rescan supersedes recorded directories, not the Git change"
        );
        assert!(pending.record_git(true));
        assert!(pending.take().repository_changed);
    }

    #[test]
    fn an_overflowing_batch_turns_into_a_rescan() {
        let mut pending = PendingChanges::default();
        pending.record(
            (0..=MAX_PENDING_DIRS).map(|n| (Arc::from(format!("/repo/{n}")), ChangeMask::ENTRIES)),
        );
        let batch = pending.take();
        assert!(batch.dirs.is_empty());
        assert!(batch.rescan);
    }

    #[test]
    fn a_flush_rereads_visible_listings_and_refreshes_git_status() {
        let batch = ChangeBatch {
            dirs: HashMap::from([
                (Arc::from("/repo/src"), ChangeMask::ENTRIES),
                (Arc::from("/repo"), ChangeMask::CONTENT),
                (Arc::from("/repo/collapsed"), ChangeMask::ENTRIES),
            ]),
            ..ChangeBatch::default()
        };
        let plan = plan_flush(batch, &set(&["/repo", "/repo/src"]));
        assert_eq!(
            plan,
            FlushPlan {
                dirs: vec![Arc::from("/repo/src")],
                git: true,
                rescan: false,
                repository_changed: false,
            }
        );
    }

    #[test]
    fn a_repository_change_alone_does_not_reread_listings() {
        let batch = ChangeBatch {
            git: true,
            repository_changed: true,
            ..ChangeBatch::default()
        };
        let plan = plan_flush(batch, &set(&["/repo"]));
        assert!(plan.dirs.is_empty());
        assert!(plan.git && plan.repository_changed);
    }

    #[test]
    fn a_directory_that_becomes_visible_again_is_stale() {
        let mut state = WatchState::default();
        assert_eq!(state.reconcile(set(&["/repo"])).len(), 1);
        assert!(state.reconcile(set(&["/repo", "/repo/src"])) == vec![Arc::from("/repo/src")]);
        assert!(
            state.reconcile(set(&["/repo"])).is_empty(),
            "collapsing reads nothing"
        );
        assert_eq!(
            state.reconcile(set(&["/repo", "/repo/src"])),
            vec![Arc::<str>::from("/repo/src")],
            "nothing watched it while it was collapsed"
        );
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
