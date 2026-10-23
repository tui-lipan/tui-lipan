use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::rc::Rc;
use std::sync::Arc;

use super::fs::FileKind;
use super::mod_private::GitStyles;
use super::provided_path::ProvidedPath;
use super::{FileTreeChange, FileTreeChangeStatus};
use crate::style::Style;

const DEFAULT_STATUS_CACHE_CAPACITY: usize = 8;

/// App-owned cache for retaining local Git status across [`super::FileTree`] mounts.
///
/// Clone and pass the same cache to trees that may replace one another. A newly mounted tree can
/// then render the last successful snapshot immediately while it revalidates in the background.
/// Entries are least-recently-used and bounded by the configured repository capacity.
#[derive(Clone)]
pub struct FileTreeGitStatusCache {
    inner: Rc<RefCell<GitStatusCacheState>>,
}

impl FileTreeGitStatusCache {
    /// Create a cache retaining up to eight repository/mode snapshots.
    pub fn new() -> Self {
        Self::with_capacity(DEFAULT_STATUS_CACHE_CAPACITY)
    }

    /// Create a cache with a repository/mode snapshot capacity of at least one.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            inner: Rc::new(RefCell::new(GitStatusCacheState {
                capacity: capacity.max(1),
                ..GitStatusCacheState::default()
            })),
        }
    }

    /// Remove all retained snapshots and in-flight request ordering state.
    pub fn clear(&self) {
        let mut state = self.inner.borrow_mut();
        state.entries.clear();
        state.pending.clear();
    }

    /// Number of retained repository/mode snapshots.
    pub fn len(&self) -> usize {
        self.inner.borrow().entries.len()
    }

    /// Whether the cache currently retains no snapshots.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn get(&self, key: &GitStatusCacheKey) -> Option<GitStatusSnapshot> {
        let mut state = self.inner.borrow_mut();
        state.access_clock = state.access_clock.wrapping_add(1);
        let access = state.access_clock;
        let entry = state.entries.get_mut(key)?;
        entry.last_access = access;
        Some(entry.snapshot.clone())
    }

    pub(crate) fn begin_request(&self, key: GitStatusCacheKey) -> GitStatusCacheRequest {
        let mut state = self.inner.borrow_mut();
        state.request_clock = state.request_clock.wrapping_add(1);
        let generation = state.request_clock;
        state.pending.insert(key.clone(), generation);
        while state.pending.len() > state.capacity {
            let Some(oldest) = state
                .pending
                .iter()
                .min_by_key(|(_, generation)| *generation)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            state.pending.remove(&oldest);
        }
        GitStatusCacheRequest { key, generation }
    }

    /// Commit only the newest shared request for this key. A failed refresh leaves the last good
    /// snapshot intact, so a transient `git` failure cannot turn future mounts falsely clean.
    pub(crate) fn finish_request(
        &self,
        request: &GitStatusCacheRequest,
        snapshot: Option<&GitStatusSnapshot>,
    ) -> bool {
        let mut state = self.inner.borrow_mut();
        if state.pending.get(&request.key) != Some(&request.generation) {
            return false;
        }
        state.pending.remove(&request.key);
        let Some(snapshot) = snapshot else {
            return true;
        };

        state.access_clock = state.access_clock.wrapping_add(1);
        let access = state.access_clock;
        state.entries.insert(
            request.key.clone(),
            GitStatusCacheEntry {
                snapshot: snapshot.clone(),
                last_access: access,
            },
        );
        while state.entries.len() > state.capacity {
            let Some(oldest) = state
                .entries
                .iter()
                .min_by_key(|(_, entry)| entry.last_access)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            state.entries.remove(&oldest);
        }
        true
    }
}

impl Default for FileTreeGitStatusCache {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for FileTreeGitStatusCache {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.inner.borrow();
        formatter
            .debug_struct("FileTreeGitStatusCache")
            .field("len", &state.entries.len())
            .field("capacity", &state.capacity)
            .finish()
    }
}

impl PartialEq for FileTreeGitStatusCache {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for FileTreeGitStatusCache {}

#[derive(Default)]
struct GitStatusCacheState {
    capacity: usize,
    access_clock: u64,
    request_clock: u64,
    entries: HashMap<GitStatusCacheKey, GitStatusCacheEntry>,
    pending: HashMap<GitStatusCacheKey, u64>,
}

struct GitStatusCacheEntry {
    snapshot: GitStatusSnapshot,
    last_access: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct GitStatusCacheKey {
    repo_root: Arc<str>,
    include_diff_stats: bool,
}

impl GitStatusCacheKey {
    pub(crate) fn new(repo_root: &Path, include_diff_stats: bool) -> Self {
        Self {
            repo_root: super::fs::normalize_path(repo_root),
            include_diff_stats,
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct GitStatusCacheRequest {
    key: GitStatusCacheKey,
    generation: u64,
}

/// Icon style for git status indicators.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum GitIconStyle {
    /// Text labels like "M", "A", "D"
    Text,
    /// Nerd font icons
    #[default]
    NerdFont,
}

/// Individual git change state (staged or unstaged).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GitChangeState {
    /// File is modified.
    Modified,
    /// File is added.
    Added,
    /// File is deleted.
    Deleted,
    /// File is renamed.
    Renamed,
    /// File is untracked.
    Untracked,
    /// File has merge conflict.
    Conflicted,
}

impl GitChangeState {
    /// Get the marker/icon for this state based on the git icon style.
    /// Note: Icons do not include spacing; row rendering inserts separators.
    pub(crate) fn marker(self, style: GitIconStyle, is_staged: bool) -> &'static str {
        match style {
            GitIconStyle::Text => match self {
                Self::Modified => "M",
                Self::Added => "A",
                Self::Deleted => "D",
                Self::Renamed => "R",
                Self::Untracked => "?",
                Self::Conflicted => "!",
            },
            GitIconStyle::NerdFont => {
                // Simplified icons: different icons for staged vs unstaged
                match (self, is_staged) {
                    (Self::Modified, false) => "", // Unstaged modified
                    (Self::Modified, true) => "",  // Staged modified
                    (Self::Added, false) => "",    // Unstaged added (shows as untracked)
                    (Self::Added, true) => "",     // Staged added
                    (Self::Deleted, false) => "",  // Unstaged deleted
                    (Self::Deleted, true) => "",   // Staged deleted
                    (Self::Renamed, false) => "󰁕",  // Unstaged renamed
                    (Self::Renamed, true) => "󰛂",   // Staged renamed
                    (Self::Untracked, _) => "",
                    (Self::Conflicted, _) => "",
                }
            }
        }
    }

    pub(super) fn style(self, styles: &GitStyles) -> Style {
        match self {
            Self::Modified => styles.modified,
            Self::Added => styles.added,
            Self::Deleted => styles.deleted,
            Self::Renamed => styles.renamed,
            Self::Untracked => styles.untracked,
            Self::Conflicted => styles.conflicted,
        }
    }

    pub(crate) fn priority(self) -> u8 {
        match self {
            Self::Conflicted => 6,
            Self::Deleted => 5,
            Self::Renamed => 4,
            Self::Added => 3,
            Self::Modified => 2,
            Self::Untracked => 1,
        }
    }
}

/// Git file status entry holding both staged and unstaged states.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GitFileStatus {
    /// Staged changes (index/worktree staging area).
    pub staged: Option<GitChangeState>,
    /// Unstaged changes (worktree modifications).
    pub unstaged: Option<GitChangeState>,
}

impl GitFileStatus {
    /// Decode the two status columns of Git porcelain v1 or v2, without reading paths or
    /// running Git. `X` describes the index and `Y` the worktree.
    ///
    /// All seven unmerged pairs become one staged `Conflicted` state, since those columns
    /// describe merge sides rather than index/worktree changes. `??` is unstaged `Untracked`;
    /// `!!`, a clean pair, and invalid columns return `None`.
    /// A space or porcelain v2 dot denotes an unchanged column.
    /// Copies share the `Renamed` presentation category; type changes are `Modified`.
    /// Ordinary pairs retain both columns. [`Self::primary_state`] prefers staged changes.
    pub fn from_porcelain_xy(x: char, y: char) -> Option<Self> {
        if matches!(
            (x, y),
            ('D', 'D')
                | ('A', 'U')
                | ('U', 'D')
                | ('U', 'A')
                | ('D', 'U')
                | ('A', 'A')
                | ('U', 'U')
        ) {
            return Some(Self::new(Some(GitChangeState::Conflicted), None));
        }
        if (x, y) == ('?', '?') {
            return Some(Self::new(None, Some(GitChangeState::Untracked)));
        }
        let valid = |c| matches!(c, ' ' | '.' | 'M' | 'T' | 'A' | 'D' | 'R' | 'C');
        if !valid(x) || !valid(y) {
            return None;
        }
        let staged = char_to_change_state(x);
        let unstaged = char_to_change_state(y);
        (staged.is_some() || unstaged.is_some()).then(|| Self::new(staged, unstaged))
    }

    /// Create a new status entry from staged and unstaged states.
    pub fn new(staged: Option<GitChangeState>, unstaged: Option<GitChangeState>) -> Self {
        Self { staged, unstaged }
    }

    /// Get the primary status for styling (prioritizes staged over unstaged).
    pub fn primary_state(self) -> Option<GitChangeState> {
        self.staged.or(self.unstaged)
    }

    /// Get style for the primary state.
    /// Staged non-conflicts use the added role; conflicts keep their own role.
    pub(super) fn style(self, styles: &GitStyles) -> Style {
        if self.primary_state() == Some(GitChangeState::Conflicted) {
            return styles.conflicted;
        }
        if self.staged.is_some() {
            return styles.added;
        }
        match self.unstaged {
            Some(state) => state.style(styles),
            None => Style::default(),
        }
    }
}

/// Numeric line-change summary from `git diff --numstat`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct GitDiffStat {
    pub(crate) added: usize,
    pub(crate) removed: usize,
}

impl GitDiffStat {
    pub(crate) fn is_empty(self) -> bool {
        self.added == 0 && self.removed == 0
    }

    fn saturating_add(self, other: Self) -> Self {
        Self {
            added: self.added.saturating_add(other.added),
            removed: self.removed.saturating_add(other.removed),
        }
    }
}

/// Git decorations for a file-tree row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct GitFileDecorations {
    pub(crate) status: GitFileStatus,
    pub(crate) diff_stat: Option<GitDiffStat>,
    pub(crate) direct: bool,
}

impl GitFileDecorations {
    pub(crate) fn from_status(status: GitFileStatus, direct: bool) -> Self {
        Self {
            status,
            diff_stat: None,
            direct,
        }
    }

    pub(crate) fn from_diff_stat(diff_stat: GitDiffStat, direct: bool) -> Self {
        Self {
            status: GitFileStatus::new(None, None),
            diff_stat: Some(diff_stat),
            direct,
        }
    }
}

/// Snapshot of git-derived decorations for a repository.
#[derive(Clone, Debug, Default)]
pub(crate) struct GitStatusSnapshot {
    pub(crate) entries: HashMap<Arc<str>, GitFileDecorations>,
    pub(crate) changed_paths: Vec<Arc<str>>,
    pub(crate) kinds: HashMap<Arc<str>, FileKind>,
    pub(crate) virtual_changes: bool,
    /// Paths that ignore rules exclude. A directory stands for everything beneath it: git reports
    /// `target/` once rather than descending into it.
    pub(crate) ignored_paths: HashSet<Arc<str>>,
}

impl GitDiffStat {
    pub(crate) fn new(added: usize, removed: usize) -> Self {
        Self { added, removed }
    }
}

pub(crate) fn provided_change_snapshot(
    root: &str,
    changes: &[FileTreeChange],
) -> GitStatusSnapshot {
    let root_path = ProvidedPath::root(root);
    let mut snapshot = GitStatusSnapshot {
        entries: HashMap::new(),
        changed_paths: Vec::new(),
        kinds: HashMap::new(),
        virtual_changes: true,
        ignored_paths: HashSet::new(),
    };

    for change in changes {
        let Some(full_path) = root_path.resolve(change.path.as_ref()) else {
            continue;
        };
        let path = Arc::<str>::from(full_path.as_str());
        snapshot.changed_paths.push(path.clone());
        if let Some(kind) = change.kind {
            snapshot.kinds.insert(path.clone(), kind);
        }

        let status = provided_status(change.status, change.staged);
        insert_provided_decoration_path_and_parents(
            &mut snapshot.entries,
            &full_path,
            GitFileDecorations::from_status(status, true),
            &root_path,
        );

        if change.additions > 0 || change.deletions > 0 {
            insert_provided_decoration_path_and_parents(
                &mut snapshot.entries,
                &full_path,
                GitFileDecorations::from_diff_stat(
                    GitDiffStat::new(change.additions, change.deletions),
                    true,
                ),
                &root_path,
            );
        }
    }

    snapshot.changed_paths.sort();
    snapshot.changed_paths.dedup();
    snapshot
}

fn provided_status(status: FileTreeChangeStatus, staged: bool) -> GitFileStatus {
    let state = Some(GitChangeState::from(status));
    if staged {
        GitFileStatus::new(state, None)
    } else {
        GitFileStatus::new(None, state)
    }
}

pub(crate) fn discover_git_root(path: &Path) -> Option<PathBuf> {
    let start = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    for ancestor in start.ancestors() {
        let marker = ancestor.join(".git");
        if marker.exists() {
            return Some(ancestor.to_path_buf());
        }
    }

    None
}

/// A `git` invocation for reading status in `repo_root`.
///
/// Optional locks are off so that reading never rewrites the index: a refresh would otherwise
/// write a file a watching tree observes, and refresh again in a loop.
fn git_command(repo_root: &Path) -> ProcessCommand {
    let mut command = ProcessCommand::new("git");
    command
        .env("GIT_OPTIONAL_LOCKS", "0")
        .arg("-C")
        .arg(repo_root);
    command
}

/// The metadata directories whose changes can alter `repo_root`'s status, asked of Git itself.
///
/// A linked worktree's `.git` is a file pointing elsewhere, and its branch refs and `packed-refs`
/// live in the common directory shared with the main checkout, so neither can be derived from
/// `repo_root/.git`. The result holds the worktree's Git directory (`HEAD`, `index`), the common
/// directory (`packed-refs`), its `info` directory (`info/exclude` ignore rules) when it exists,
/// and the directory holding the checked-out branch's ref, when any. A global
/// `core.excludesFile` is left to the periodic repair.
pub(crate) fn git_watch_directories(repo_root: &Path) -> Vec<PathBuf> {
    let Some(output) = git_command(repo_root)
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-dir",
            "--git-common-dir",
        ])
        .output()
        .ok()
        .filter(|output| output.status.success())
    else {
        return Vec::new();
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut dirs: Vec<PathBuf> = stdout.lines().map(PathBuf::from).collect();

    // Only when present: a directory that cannot be watched would mark the whole watch incomplete.
    if let Some(info) = dirs.get(1).map(|common_dir| common_dir.join("info"))
        && info.is_dir()
    {
        dirs.push(info);
    }
    if let Some(common_dir) = dirs.get(1).cloned()
        && let Some(branch_ref) = git_command(repo_root)
            .args(["symbolic-ref", "-q", "HEAD"])
            .output()
            .ok()
            .filter(|output| output.status.success())
        && let Some(ref_dir) = common_dir
            .join(String::from_utf8_lossy(&branch_ref.stdout).trim())
            .parent()
    {
        dirs.push(ref_dir.to_path_buf());
    }

    dirs.sort();
    dirs.dedup();
    dirs
}

/// Everything a repository watch needs: the metadata directories, and the repository root plus
/// every directory whose contents can change `git status`, with their ancestors.
///
/// Watches are one level deep, so every such directory is listed rather than relying on an
/// ancestor: each directory holding a tracked file (from the index, so staged paths count), each
/// holding an untracked file that is not ignored, and each untracked directory, empty ones
/// included. Deleting the last file of an untracked directory, or adding the first file to an empty
/// one, is then seen. Ignored build output holds none of these and stays unwatched. Past
/// [`MAX_REPOSITORY_WATCH_DIRS`] only the root is kept and the result is marked incomplete, so the
/// tree polls Git status rather than adding watches without bound.
///
/// [`MAX_REPOSITORY_WATCH_DIRS`]: super::watch::MAX_REPOSITORY_WATCH_DIRS
pub(crate) fn repository_watch_directories(repo_root: &Path) -> super::watch::RepositoryWatchDirs {
    let key = |path: &Path| Arc::<str>::from(path.to_string_lossy().as_ref());
    let metadata = git_watch_directories(repo_root)
        .iter()
        .map(|dir| key(dir))
        .collect();
    let list = |args: &[&str]| {
        git_command(repo_root)
            .args(["ls-files", "-z"])
            .args(args)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| {
                output
                    .stdout
                    .split(|byte| *byte == 0)
                    .filter(|path| !path.is_empty())
                    .map(|path| String::from_utf8_lossy(path).into_owned())
                    .collect::<Vec<_>>()
            })
    };
    // Every file `git status` can report on, then untracked directories (`--directory` lists an
    // untracked tree once, as `dir/`, and lists empty ones that hold no file to find them by).
    let files = list(&["--cached", "--others", "--exclude-standard"]);
    let untracked = list(&["--others", "--directory", "--exclude-standard"]);
    let (Some(files), Some(untracked)) = (files, untracked) else {
        return super::watch::RepositoryWatchDirs {
            metadata,
            worktree: HashSet::from([key(repo_root)]),
            complete: false,
        };
    };
    let relative = status_directories(
        files,
        untracked
            .into_iter()
            .filter_map(|path| path.strip_suffix('/').map(str::to_owned)),
    );
    let complete = relative.len() <= super::watch::MAX_REPOSITORY_WATCH_DIRS;
    let mut worktree = HashSet::from([key(repo_root)]);
    if complete {
        worktree.extend(relative.iter().map(|dir| key(&repo_root.join(dir))));
    }
    super::watch::RepositoryWatchDirs {
        metadata,
        worktree,
        complete,
    }
}

/// The distinct non-root directories, relative to the repository, that contain one of `files`
/// directly or through a subdirectory, plus each of `dirs` and its ancestors.
fn status_directories(
    files: impl IntoIterator<Item = String>,
    dirs: impl IntoIterator<Item = String>,
) -> HashSet<PathBuf> {
    let mut found = HashSet::new();
    let mut insert_with_ancestors = |start: Option<&Path>| {
        let mut next = start;
        while let Some(dir) = next.filter(|dir| !dir.as_os_str().is_empty()) {
            if !found.insert(dir.to_path_buf()) {
                // Its ancestors were added along with it.
                break;
            }
            next = dir.parent();
        }
    };
    for file in files {
        insert_with_ancestors(Path::new(&file).parent());
    }
    for dir in dirs {
        insert_with_ancestors(Some(Path::new(&dir)));
    }
    found
}

pub(crate) fn load_git_snapshot(
    repo_root: &Path,
    include_diff_stats: bool,
) -> Option<GitStatusSnapshot> {
    let output = git_command(repo_root)
        .arg("status")
        .arg("--porcelain=v1")
        // NUL-separated and unquoted: a path with a space or a non-ASCII name arrives verbatim, and
        // a rename's two paths are separate fields instead of one ` -> ` string.
        .arg("-z")
        .arg("--untracked-files=normal")
        // `matching` names an ignored directory once instead of walking into it, so this costs
        // little over `no` even beside a large build directory.
        .arg("--ignored=matching")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut snapshot = snapshot_from_status(&stdout, repo_root);

    if include_diff_stats {
        load_git_diff_stats(repo_root, &mut snapshot.entries);
    }

    Some(snapshot)
}

/// Read `git status --porcelain=v1 -z --ignored=matching` output: `!!` records are ignored paths,
/// and every other record is a change.
fn snapshot_from_status(stdout: &str, repo_root: &Path) -> GitStatusSnapshot {
    let mut snapshot = GitStatusSnapshot {
        entries: HashMap::new(),
        changed_paths: Vec::new(),
        kinds: HashMap::new(),
        virtual_changes: false,
        ignored_paths: HashSet::new(),
    };

    let mut records = stdout.split('\0');
    while let Some(record) = records.next() {
        let (Some(code), Some(relative_path)) = (record.get(..2), record.get(3..)) else {
            continue;
        };
        // A rename or copy is followed by the path it came from, as a record of its own.
        if code.contains(['R', 'C']) {
            records.next();
        }
        if code == "!!" {
            let relative_path = relative_path.trim_end_matches('/');
            let full_path = super::fs::normalize_path(&repo_root.join(relative_path));
            snapshot.ignored_paths.insert(full_path);
            continue;
        }
        let Some(status) = status_from_code(code) else {
            continue;
        };
        let full_path = super::fs::normalize_path(&repo_root.join(relative_path));
        snapshot.changed_paths.push(full_path.clone());
        insert_decoration_path_and_parents(
            &mut snapshot.entries,
            Path::new(full_path.as_ref()),
            GitFileDecorations::from_status(status, true),
            repo_root,
        );
    }

    snapshot
}

/// The status a porcelain `XY` code describes, or `None` for one that is not a change (`!!`).
pub(crate) fn status_from_code(code: &str) -> Option<GitFileStatus> {
    let [x, y] = *code.as_bytes() else {
        return None;
    };
    GitFileStatus::from_porcelain_xy(char::from(x), char::from(y))
}

pub(crate) fn parse_git_numstat_line(line: &str) -> Option<(String, GitDiffStat)> {
    let mut fields = line.splitn(3, '\t');
    let added = fields.next()?;
    let removed = fields.next()?;
    let path = fields.next()?.trim();

    if added == "-" || removed == "-" || path.is_empty() {
        return None;
    }

    Some((
        normalize_numstat_path(path),
        GitDiffStat {
            added: added.parse().ok()?,
            removed: removed.parse().ok()?,
        },
    ))
}

fn normalize_numstat_path(path: &str) -> String {
    let Some((before, after)) = path.split_once(" => ") else {
        return path.to_string();
    };

    if let Some(open_brace) = before.rfind('{') {
        let prefix = &before[..open_brace];
        let (renamed, suffix) = after.split_once('}').unwrap_or((after, ""));
        return format!("{prefix}{renamed}{suffix}");
    }

    after.to_string()
}

fn load_git_diff_stats(repo_root: &Path, entries: &mut HashMap<Arc<str>, GitFileDecorations>) {
    let Some(output) = git_command(repo_root)
        .arg("diff")
        .arg("--numstat")
        .arg("HEAD")
        .arg("--")
        .output()
        .ok()
    else {
        return;
    };

    if !output.status.success() {
        return;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let Some((relative_path, diff_stat)) = parse_git_numstat_line(line) else {
            continue;
        };
        let full_path = super::fs::normalize_path(&repo_root.join(relative_path));
        insert_decoration_path_and_parents(
            entries,
            Path::new(full_path.as_ref()),
            GitFileDecorations::from_diff_stat(diff_stat, true),
            repo_root,
        );
    }
}

fn char_to_change_state(c: char) -> Option<GitChangeState> {
    match c {
        'M' | 'T' => Some(GitChangeState::Modified),
        'A' => Some(GitChangeState::Added),
        'D' => Some(GitChangeState::Deleted),
        'R' | 'C' => Some(GitChangeState::Renamed),
        'U' => Some(GitChangeState::Conflicted),
        _ => None,
    }
}

fn insert_decoration_path_and_parents(
    entries: &mut HashMap<Arc<str>, GitFileDecorations>,
    full_path: &Path,
    decoration: GitFileDecorations,
    repo_root: &Path,
) {
    insert_decoration(entries, super::fs::normalize_path(full_path), decoration);

    let mut current = full_path.parent();
    while let Some(parent) = current {
        if !parent.starts_with(repo_root) {
            break;
        }
        insert_decoration(
            entries,
            super::fs::normalize_path(parent),
            GitFileDecorations {
                direct: false,
                ..decoration
            },
        );
        if parent == repo_root {
            break;
        }
        current = parent.parent();
    }
}

pub(crate) fn insert_provided_decoration_path_and_parents(
    entries: &mut HashMap<Arc<str>, GitFileDecorations>,
    full_path: &ProvidedPath,
    decoration: GitFileDecorations,
    repo_root: &ProvidedPath,
) {
    insert_decoration(entries, Arc::from(full_path.as_str()), decoration);

    let mut current = full_path.parent();
    while let Some(parent) = current {
        if !parent.starts_with(repo_root) {
            break;
        }
        insert_decoration(
            entries,
            Arc::from(parent.as_str()),
            GitFileDecorations {
                direct: false,
                ..decoration
            },
        );
        if parent == *repo_root {
            break;
        }
        current = parent.parent();
    }
}

#[cfg(test)]
pub(crate) fn insert_status(
    statuses: &mut HashMap<Arc<str>, GitFileStatus>,
    path: Arc<str>,
    status: GitFileStatus,
) {
    match statuses.get(path.as_ref()).copied() {
        Some(existing) => {
            // Merge the statuses - keep the higher priority for each field
            let merged = GitFileStatus::new(
                // Keep staged if it has higher priority, otherwise use new
                match (existing.staged, status.staged) {
                    (Some(e), Some(s)) if e.priority() >= s.priority() => Some(e),
                    (e, None) => e,
                    (_, s) => s,
                },
                // Keep unstaged if it has higher priority, otherwise use new
                match (existing.unstaged, status.unstaged) {
                    (Some(e), Some(s)) if e.priority() >= s.priority() => Some(e),
                    (e, None) => e,
                    (_, s) => s,
                },
            );
            statuses.insert(path, merged);
        }
        None => {
            statuses.insert(path, status);
        }
    }
}

pub(crate) fn insert_decoration(
    entries: &mut HashMap<Arc<str>, GitFileDecorations>,
    path: Arc<str>,
    decoration: GitFileDecorations,
) {
    match entries.get(path.as_ref()).copied() {
        Some(existing) => {
            let merged = GitFileDecorations {
                status: merge_status(existing.status, decoration.status),
                diff_stat: merge_diff_stat(existing.diff_stat, decoration.diff_stat),
                direct: existing.direct || decoration.direct,
            };
            entries.insert(path, merged);
        }
        None => {
            entries.insert(path, decoration);
        }
    }
}

fn merge_status(existing: GitFileStatus, status: GitFileStatus) -> GitFileStatus {
    GitFileStatus::new(
        match (existing.staged, status.staged) {
            (Some(e), Some(s)) if e.priority() >= s.priority() => Some(e),
            (e, None) => e,
            (_, s) => s,
        },
        match (existing.unstaged, status.unstaged) {
            (Some(e), Some(s)) if e.priority() >= s.priority() => Some(e),
            (e, None) => e,
            (_, s) => s,
        },
    )
}

fn merge_diff_stat(
    existing: Option<GitDiffStat>,
    diff_stat: Option<GitDiffStat>,
) -> Option<GitDiffStat> {
    match (existing, diff_stat) {
        (Some(existing), Some(diff_stat)) => Some(existing.saturating_add(diff_stat)),
        (Some(existing), None) => Some(existing),
        (None, Some(diff_stat)) => Some(diff_stat),
        (None, None) => None,
    }
}

#[cfg(test)]
mod watch_directory_tests {
    use super::*;

    #[test]
    fn status_directories_include_every_ancestor_once() {
        let dirs = status_directories(
            [
                "README.md",
                "src/main.rs",
                "src/update/tree.rs",
                "docs/a/b/c.md",
            ]
            .map(String::from),
            ["scratch/empty"].map(String::from),
        );
        let mut dirs: Vec<_> = dirs
            .iter()
            .map(|dir| dir.to_string_lossy().into_owned())
            .collect();
        dirs.sort();
        assert_eq!(
            dirs,
            [
                "docs",
                "docs/a",
                "docs/a/b",
                "scratch",
                "scratch/empty",
                "src",
                "src/update"
            ]
        );
    }

    #[test]
    fn a_repository_watch_covers_status_directories_but_not_ignored_ones() {
        let dir = std::env::temp_dir().join(format!(
            "tui-lipan-file-tree-repo-watch-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("target/debug")).unwrap();
        std::fs::write(dir.join("src/main.rs"), "").unwrap();
        std::fs::write(dir.join("target/debug/out"), "").unwrap();
        std::fs::create_dir_all(dir.join("local")).unwrap();
        std::fs::write(dir.join("local/note.txt"), "").unwrap();
        std::fs::write(dir.join(".gitignore"), "target/\nlocal/\n").unwrap();
        let git = |args: &[&str]| {
            ProcessCommand::new("git")
                .arg("-C")
                .arg(&dir)
                .args(args)
                .output()
                .is_ok_and(|output| output.status.success())
        };
        if !git(&["init", "-q"]) || !git(&["add", "-A"]) {
            eprintln!("skipping: git is unavailable");
            return;
        }
        // Untracked after the add: an empty directory, and a tree with a file two levels down.
        std::fs::create_dir_all(dir.join("scratch")).unwrap();
        std::fs::create_dir_all(dir.join("notes/drafts")).unwrap();
        std::fs::write(dir.join("notes/drafts/idea.md"), "").unwrap();
        let root = dunce::canonicalize(&dir).unwrap();

        let dirs = repository_watch_directories(&root);

        let key = |path: PathBuf| Arc::<str>::from(path.to_string_lossy().as_ref());
        assert!(dirs.complete);
        assert!(dirs.worktree.contains(&key(root.clone())));
        assert!(dirs.worktree.contains(&key(root.join("src"))));
        assert!(
            dirs.worktree.contains(&key(root.join("scratch"))),
            "adding the first file to an empty untracked directory changes status"
        );
        assert!(
            dirs.worktree.contains(&key(root.join("notes/drafts"))),
            "removing the last untracked file changes status"
        );
        assert!(!dirs.worktree.contains(&key(root.join("target"))));
        assert!(!dirs.worktree.contains(&key(root.join("local"))));
        assert!(!dirs.metadata.is_empty(), "the Git directory is watched");
        if root.join(".git/info").is_dir() {
            assert!(
                dirs.metadata.contains(&key(root.join(".git/info"))),
                "info/exclude holds ignore rules"
            );
        }

        // Un-ignoring a directory makes its contents part of `git status`.
        std::fs::write(dir.join(".gitignore"), "target/\n").unwrap();
        let dirs = repository_watch_directories(&root);
        assert!(dirs.worktree.contains(&key(root.join("local"))));

        let _ = std::fs::remove_dir_all(dir);
    }
}

#[cfg(test)]
mod cache_tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn status_output_separates_ignored_paths_from_changes() {
        let root = Path::new("/no-such-repo");
        let snapshot = snapshot_from_status(
            " M src/main.rs\0R  new name.rs\0old name.rs\0?? notes.md\0!! target/\0!! foo bar/\0",
            root,
        );

        // The rename counts once, by its new path; the path it came from is not a record.
        assert_eq!(
            snapshot.changed_paths,
            [
                Arc::<str>::from("/no-such-repo/src/main.rs"),
                Arc::from("/no-such-repo/new name.rs"),
                Arc::from("/no-such-repo/notes.md"),
            ]
        );
        let mut ignored: Vec<&str> = snapshot.ignored_paths.iter().map(AsRef::as_ref).collect();
        ignored.sort_unstable();
        // A directory loses its trailing slash, and a space arrives unquoted, so both match the
        // row's path.
        assert_eq!(ignored, ["/no-such-repo/foo bar", "/no-such-repo/target"]);
    }

    fn snapshot(path: &'static str) -> GitStatusSnapshot {
        GitStatusSnapshot {
            changed_paths: vec![Arc::from(path)],
            ..GitStatusSnapshot::default()
        }
    }

    #[test]
    fn newest_shared_request_wins_and_failures_keep_last_good_snapshot() {
        let cache = FileTreeGitStatusCache::new();
        let key = GitStatusCacheKey::new(Path::new("/repo"), false);
        let older = cache.begin_request(key.clone());
        let newer = cache.begin_request(key.clone());

        assert!(!cache.finish_request(&older, Some(&snapshot("/repo/old"))));
        assert!(cache.is_empty());
        assert!(cache.finish_request(&newer, Some(&snapshot("/repo/new"))));
        assert_eq!(
            cache.get(&key).unwrap().changed_paths,
            [Arc::<str>::from("/repo/new")]
        );

        let failed = cache.begin_request(key.clone());
        assert!(cache.finish_request(&failed, None));
        assert_eq!(
            cache.get(&key).unwrap().changed_paths,
            [Arc::<str>::from("/repo/new")]
        );
    }

    #[test]
    fn capacity_is_lru_and_diff_stat_modes_do_not_alias() {
        let cache = FileTreeGitStatusCache::with_capacity(2);
        let first = GitStatusCacheKey::new(Path::new("/first"), false);
        let first_with_stats = GitStatusCacheKey::new(Path::new("/first"), true);
        let second = GitStatusCacheKey::new(Path::new("/second"), false);

        for (key, path) in [
            (first.clone(), "/first/plain"),
            (first_with_stats.clone(), "/first/stats"),
        ] {
            let request = cache.begin_request(key);
            assert!(cache.finish_request(&request, Some(&snapshot(path))));
        }
        assert!(cache.get(&first).is_some(), "touch the plain entry");
        let request = cache.begin_request(second.clone());
        assert!(cache.finish_request(&request, Some(&snapshot("/second/file"))));

        assert!(cache.get(&first).is_some());
        assert!(cache.get(&first_with_stats).is_none());
        assert!(cache.get(&second).is_some());
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn abandoned_request_ordering_state_is_bounded() {
        let cache = FileTreeGitStatusCache::with_capacity(2);
        for root in ["/one", "/two", "/three"] {
            cache.begin_request(GitStatusCacheKey::new(Path::new(root), false));
        }

        assert_eq!(cache.inner.borrow().pending.len(), 2);
    }
}

#[cfg(test)]
mod porcelain_contract_tests {
    use super::*;
    use GitChangeState::{
        Added as A, Conflicted, Deleted as D, Modified as M, Renamed as R, Untracked,
    };

    #[test]
    fn porcelain_xy_matrix_preserves_columns_and_staged_precedence() {
        // Meaningful ordinary pairs from git-status's short-format table.
        let cases = [
            (" M", None, Some(M)),
            (" A", None, Some(A)),
            (" D", None, Some(D)),
            (" T", None, Some(M)),
            (" R", None, Some(R)),
            (" C", None, Some(R)),
            ("M ", Some(M), None),
            ("MM", Some(M), Some(M)),
            ("MT", Some(M), Some(M)),
            ("MD", Some(M), Some(D)),
            ("T ", Some(M), None),
            ("TM", Some(M), Some(M)),
            ("TT", Some(M), Some(M)),
            ("TD", Some(M), Some(D)),
            ("A ", Some(A), None),
            ("AM", Some(A), Some(M)),
            ("AT", Some(A), Some(M)),
            ("AD", Some(A), Some(D)),
            ("D ", Some(D), None),
            ("R ", Some(R), None),
            ("RM", Some(R), Some(M)),
            ("RT", Some(R), Some(M)),
            ("RD", Some(R), Some(D)),
            ("C ", Some(R), None),
            ("CM", Some(R), Some(M)),
            ("CT", Some(R), Some(M)),
            ("CD", Some(R), Some(D)),
            ("??", None, Some(Untracked)),
        ];
        for (code, staged, unstaged) in cases {
            let bytes = code.as_bytes();
            let expected = GitFileStatus::new(staged, unstaged);
            assert_eq!(
                GitFileStatus::from_porcelain_xy(bytes[0] as char, bytes[1] as char),
                Some(expected),
                "{code:?}"
            );
            assert_eq!(expected.primary_state(), staged.or(unstaged), "{code:?}");
            // Porcelain v2 spells an unchanged column with a dot.
            let v2 = code.replace(' ', ".");
            let bytes = v2.as_bytes();
            assert_eq!(
                GitFileStatus::from_porcelain_xy(bytes[0] as char, bytes[1] as char),
                Some(expected),
                "{v2:?}"
            );
            assert_eq!(status_from_code(code), Some(expected), "FileTree: {code:?}");
        }
    }

    #[test]
    fn all_unmerged_pairs_are_conflicts_rather_than_independent_changes() {
        let styles = crate::FileTree::new("/repo")
            .props
            .git_styles(&crate::Theme::default());
        for code in ["UU", "AA", "DD", "AU", "UA", "DU", "UD"] {
            let bytes = code.as_bytes();
            let status =
                GitFileStatus::from_porcelain_xy(bytes[0] as char, bytes[1] as char).unwrap();
            assert_eq!(status, GitFileStatus::new(Some(Conflicted), None), "{code}");
            assert_eq!(status.primary_state(), Some(Conflicted), "{code}");
            assert_eq!(status.style(&styles), styles.conflicted, "{code}");
            assert_eq!(status_from_code(code), Some(status));
        }
    }

    #[test]
    fn ignored_clean_and_invalid_codes_are_not_changes() {
        for (x, y) in [
            ('!', '!'),
            (' ', ' '),
            ('.', '.'),
            ('?', 'M'),
            ('U', ' '),
            ('M', 'U'),
            ('X', 'M'),
            ('é', 'M'),
        ] {
            assert_eq!(GitFileStatus::from_porcelain_xy(x, y), None, "{x}{y}");
        }
        for invalid in ["", "M", "MMextra", "éM"] {
            assert_eq!(status_from_code(invalid), None, "{invalid}");
        }
    }

    #[test]
    fn filetree_parser_uses_the_decoder_and_consumes_copy_and_rename_sources() {
        let snapshot = snapshot_from_status(
            "AA conflict\0C  copy\0source\0R  rename\0old\0 M next\0",
            Path::new("/repo"),
        );
        assert_eq!(snapshot.changed_paths.len(), 4);
        let conflict = &snapshot.entries["/repo/conflict"];
        assert_eq!(conflict.status.primary_state(), Some(Conflicted));
        for name in ["copy", "rename"] {
            assert_eq!(
                snapshot.entries[&Arc::<str>::from(format!("/repo/{name}"))]
                    .status
                    .staged,
                Some(R)
            );
        }
        assert!(!snapshot.entries.contains_key("/repo/source"));
    }
}
