use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::Arc;

use super::fs::FileKind;
use super::mod_private::FileTreeProps;
use super::{FileTreeChange, FileTreeChangeStatus};
use crate::style::Style;

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

    pub(crate) fn style(self, props: &FileTreeProps) -> Style {
        match self {
            Self::Modified => props.git_style_modified,
            Self::Added => props.git_style_added,
            Self::Deleted => props.git_style_deleted,
            Self::Renamed => props.git_style_renamed,
            Self::Untracked => props.git_style_untracked,
            Self::Conflicted => props.git_style_conflicted,
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
    /// Create a new status entry from staged and unstaged states.
    pub fn new(staged: Option<GitChangeState>, unstaged: Option<GitChangeState>) -> Self {
        Self { staged, unstaged }
    }

    /// Get the primary status for styling (prioritizes staged over unstaged).
    pub fn primary_state(self) -> Option<GitChangeState> {
        self.staged.or(self.unstaged)
    }

    /// Get style for the primary state.
    /// Prioritizes staged changes (rendered as green).
    pub(crate) fn style(self, props: &FileTreeProps) -> Style {
        if self.staged.is_some() {
            return props.git_style_added;
        }
        match self.unstaged {
            Some(state) => state.style(props),
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
    let root_path_buf = provided_root_path(root);
    let root_path = root_path_buf.as_path();
    let mut snapshot = GitStatusSnapshot {
        entries: HashMap::new(),
        changed_paths: Vec::new(),
        kinds: HashMap::new(),
        virtual_changes: true,
    };

    for change in changes {
        let Some(full_path) = lexical_change_path(root_path, change.path.as_ref()) else {
            continue;
        };
        let path = Arc::<str>::from(full_path.to_string_lossy().as_ref());
        snapshot.changed_paths.push(path.clone());
        if let Some(kind) = change.kind {
            snapshot.kinds.insert(path.clone(), kind);
        }

        let status = provided_status(change.status, change.staged);
        insert_provided_decoration_path_and_parents(
            &mut snapshot.entries,
            &full_path,
            GitFileDecorations::from_status(status, true),
            root_path,
        );

        if change.additions > 0 || change.deletions > 0 {
            insert_provided_decoration_path_and_parents(
                &mut snapshot.entries,
                &full_path,
                GitFileDecorations::from_diff_stat(
                    GitDiffStat::new(change.additions, change.deletions),
                    true,
                ),
                root_path,
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

pub(crate) fn provided_root_path(root: &str) -> PathBuf {
    let root = Path::new(root);
    if let Ok(canonical) = std::fs::canonicalize(root) {
        return canonical;
    }

    let absolute = if root.is_absolute() {
        root.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(root)
    };
    lexical_normalize_path(&absolute)
}

fn lexical_change_path(root: &Path, path: &str) -> Option<PathBuf> {
    let input = Path::new(path);
    let output = if input.is_absolute() {
        lexical_normalize_path(input)
    } else {
        lexical_normalize_path(&root.join(input))
    };

    if output.starts_with(root) {
        Some(output)
    } else {
        None
    }
}

fn lexical_normalize_path(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::Prefix(prefix) => output.push(prefix.as_os_str()),
            std::path::Component::RootDir => output.push(component.as_os_str()),
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                output.pop();
            }
            std::path::Component::Normal(part) => output.push(part),
        }
    }
    output
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

pub(crate) fn load_git_snapshot(
    repo_root: &Path,
    include_diff_stats: bool,
) -> Option<GitStatusSnapshot> {
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(repo_root)
        .arg("status")
        .arg("--porcelain=v1")
        .arg("--untracked-files=normal")
        .arg("--ignored=no")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut snapshot = GitStatusSnapshot {
        entries: HashMap::new(),
        changed_paths: Vec::new(),
        kinds: HashMap::new(),
        virtual_changes: false,
    };

    for line in stdout.lines() {
        let Some((relative_path, status)) = parse_git_porcelain_line(line) else {
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

    if include_diff_stats {
        load_git_diff_stats(repo_root, &mut snapshot.entries);
    }

    Some(snapshot)
}

pub(crate) fn parse_git_porcelain_line(line: &str) -> Option<(&str, GitFileStatus)> {
    if line.len() < 3 {
        return None;
    }

    let status_code = &line[..2];
    let path_field = line.get(3..)?.trim();

    let status = if status_code == "??" {
        GitFileStatus::new(None, Some(GitChangeState::Untracked))
    } else {
        let mut chars = status_code.chars();
        let x = chars.next()?;
        let y = chars.next()?;
        git_status_from_xy(x, y)?
    };

    let path = if status_code.contains('R') {
        path_field
            .rsplit_once(" -> ")
            .map(|(_, to)| to)
            .unwrap_or(path_field)
    } else {
        path_field
    };

    Some((path, status))
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
    let Some(output) = ProcessCommand::new("git")
        .arg("-C")
        .arg(repo_root)
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
        'M' | 'C' => Some(GitChangeState::Modified),
        'A' => Some(GitChangeState::Added),
        'D' => Some(GitChangeState::Deleted),
        'R' => Some(GitChangeState::Renamed),
        'U' => Some(GitChangeState::Conflicted),
        _ => None,
    }
}

fn git_status_from_xy(x: char, y: char) -> Option<GitFileStatus> {
    let staged = char_to_change_state(x);
    let unstaged = char_to_change_state(y);

    // If both are None, there's no status
    if staged.is_none() && unstaged.is_none() {
        return None;
    }

    Some(GitFileStatus::new(staged, unstaged))
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
    full_path: &Path,
    decoration: GitFileDecorations,
    repo_root: &Path,
) {
    insert_decoration(entries, lossy_path_arc(full_path), decoration);

    let mut current = full_path.parent();
    while let Some(parent) = current {
        insert_decoration(
            entries,
            lossy_path_arc(parent),
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

fn lossy_path_arc(path: &Path) -> Arc<str> {
    Arc::<str>::from(path.to_string_lossy().as_ref())
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
