use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

#[cfg(not(target_arch = "wasm32"))]
use ignore::{DirEntry, ParallelVisitor, ParallelVisitorBuilder, WalkBuilder, WalkState};
use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32String};

use super::fs::normalize_path;
#[cfg(not(target_arch = "wasm32"))]
use super::fs::read_directory;
use super::provided_path::ProvidedPath;

/// How many matches an explorer query shows unless the application sets
/// `FileTree::explorer_max_matches`.
pub(crate) const DEFAULT_MAX_MATCHES: usize = 1_000;

#[derive(Clone, Debug, Default)]
pub(crate) struct ExplorerFilter {
    pub(crate) visible_paths: HashSet<Arc<str>>,
    pub(crate) expanded_paths: HashSet<Arc<str>>,
    pub(crate) label_hits: HashMap<Arc<str>, Vec<u32>>,
    pub(crate) primary_match_directory: Option<Arc<str>>,
    /// Every entry the query matched.
    pub(crate) match_count: usize,
    /// The best-scoring matches the tree shows, at most the match limit.
    pub(crate) shown_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ExplorerCandidate {
    pub(crate) path: Arc<str>,
    pub(crate) label: Arc<str>,
    pub(crate) is_dir: bool,
}

/// What a walk depends on. A cached index answers a query only while all of it still holds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExplorerIndexKey {
    pub(crate) root: Arc<str>,
    pub(crate) show_hidden: bool,
    pub(crate) max_entries_per_dir: usize,
}

/// One searchable entry, with the strings the matcher reads prepared once rather than per query.
#[derive(Debug)]
struct IndexedCandidate {
    path: Arc<str>,
    is_dir: bool,
    label: Utf32String,
    /// The path below the root. Matching the absolute path would let the root's own spelling
    /// match: under `/home/user/project`, a query of `proj` would match every entry.
    relative: Utf32String,
}

/// Every entry an explorer query can match, in path order.
#[derive(Debug)]
pub(crate) struct ExplorerIndex {
    key: Option<ExplorerIndexKey>,
    root_path: Arc<str>,
    provided_paths: bool,
    candidates: Vec<IndexedCandidate>,
}

impl ExplorerIndex {
    /// Index entries the tree already holds, such as provided listings or a changed-only view.
    pub(crate) fn from_candidates(
        root_path: &Arc<str>,
        candidates: impl IntoIterator<Item = ExplorerCandidate>,
        provided_paths: bool,
    ) -> Self {
        let candidates = candidates
            .into_iter()
            .map(|candidate| index_candidate(candidate, root_path, provided_paths))
            .collect();
        Self {
            key: None,
            root_path: root_path.clone(),
            provided_paths,
            candidates,
        }
    }

    /// Walk the filesystem under `key.root`, honoring ignore files.
    pub(crate) fn walk(key: ExplorerIndexKey) -> Self {
        let root = PathBuf::from(key.root.as_ref());
        let root_path = normalize_path(&root);
        let entries = collect_entries(
            Path::new(root_path.as_ref()),
            key.show_hidden,
            key.max_entries_per_dir,
        );
        let mut index = Self::from_candidates(&root_path, entries, false);
        index.key = Some(key);
        index
    }

    /// Match `query` and keep the `max_matches` best-scoring entries.
    ///
    /// A filename match outranks a match that needs the directories above it, so `comp` lists
    /// `component.rs` before `src/compat/mod.rs`. Ties keep path order.
    pub(crate) fn search(&self, query: &str, max_matches: usize) -> ExplorerFilter {
        let query = query.trim();
        if query.is_empty() {
            return ExplorerFilter::default();
        }

        let mut label_matcher = Matcher::new(Config::DEFAULT);
        let mut path_matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
        let allow_path_fuzzy_match = should_fuzzy_match_full_path(query);

        let mut hits: Vec<SearchHit> = Vec::new();
        for (index, candidate) in self.candidates.iter().enumerate() {
            let label_score = pattern.score(candidate.label.slice(..), &mut label_matcher);
            let rank = match label_score {
                Some(score) => (true, score),
                None if allow_path_fuzzy_match => {
                    match pattern.score(candidate.relative.slice(..), &mut path_matcher) {
                        Some(score) => (false, score),
                        None => continue,
                    }
                }
                None => continue,
            };
            hits.push(SearchHit { rank, index });
        }

        let match_count = hits.len();
        let max_matches = max_matches.max(1);
        if hits.len() > max_matches {
            hits.select_nth_unstable_by(max_matches, SearchHit::best_first);
            hits.truncate(max_matches);
        }
        hits.sort_unstable_by(SearchHit::best_first);

        let mut filter = ExplorerFilter {
            match_count,
            shown_count: hits.len(),
            ..ExplorerFilter::default()
        };
        for hit in &hits {
            let candidate = &self.candidates[hit.index];
            include_path_with_ancestors(
                &mut filter.visible_paths,
                &candidate.path,
                &self.root_path,
                self.provided_paths,
            );

            let (label_matched, _) = hit.rank;
            if label_matched {
                let mut label_hits = Vec::new();
                pattern.indices(
                    candidate.label.slice(..),
                    &mut label_matcher,
                    &mut label_hits,
                );
                label_hits.sort_unstable();
                label_hits.dedup();
                if !label_hits.is_empty() {
                    filter.label_hits.insert(candidate.path.clone(), label_hits);
                }
            }

            let entry_dir = entry_directory_path(
                &candidate.path,
                candidate.is_dir,
                &self.root_path,
                self.provided_paths,
            );
            include_path_with_ancestors(
                &mut filter.expanded_paths,
                &entry_dir,
                &self.root_path,
                self.provided_paths,
            );
            if filter.primary_match_directory.is_none() {
                filter.primary_match_directory = Some(entry_dir);
            }
        }

        if !filter.visible_paths.is_empty() {
            filter.visible_paths.insert(self.root_path.clone());
        }

        if !filter.expanded_paths.is_empty() {
            filter.expanded_paths.insert(self.root_path.clone());
        }

        filter
    }
}

#[derive(Clone, Copy)]
struct SearchHit {
    /// Whether the filename matched, then the matcher's score.
    rank: (bool, u32),
    index: usize,
}

impl SearchHit {
    fn best_first(left: &Self, right: &Self) -> Ordering {
        right
            .rank
            .cmp(&left.rank)
            .then_with(|| left.index.cmp(&right.index))
    }
}

/// The last walk a tree's explorer made, so the next keystroke filters it instead of walking again.
///
/// A search fills the slot from its worker. The tree replaces the whole cache, rather than
/// emptying it, when what it walked may have changed: a walk still running then fills a cache
/// nothing reads anymore.
#[derive(Debug, Default)]
pub(crate) struct ExplorerIndexCache {
    index: Mutex<Option<Arc<ExplorerIndex>>>,
}

impl ExplorerIndexCache {
    /// The cached index for `key`, walking first when there is none or it walked something else.
    pub(crate) fn get_or_walk(&self, key: ExplorerIndexKey) -> Arc<ExplorerIndex> {
        if let Some(index) = self.cached(&key) {
            return index;
        }
        let index = Arc::new(ExplorerIndex::walk(key));
        if let Ok(mut slot) = self.index.lock() {
            *slot = Some(Arc::clone(&index));
        }
        index
    }

    fn cached(&self, key: &ExplorerIndexKey) -> Option<Arc<ExplorerIndex>> {
        let slot = self.index.lock().ok()?;
        slot.as_ref()
            .filter(|index| index.key.as_ref() == Some(key))
            .cloned()
    }
}

fn index_candidate(
    candidate: ExplorerCandidate,
    root_path: &Arc<str>,
    provided_paths: bool,
) -> IndexedCandidate {
    let relative = relative_path(&candidate.path, root_path, provided_paths);
    IndexedCandidate {
        label: Utf32String::from(candidate.label.as_ref()),
        relative: Utf32String::from(relative.as_str()),
        path: candidate.path,
        is_dir: candidate.is_dir,
    }
}

fn relative_path(path: &str, root_path: &str, provided_paths: bool) -> String {
    if provided_paths {
        let root = ProvidedPath::from_path(root_path);
        return ProvidedPath::from_path(path)
            .strip_prefix(&root)
            .map_or_else(
                || path.to_string(),
                |relative| relative.as_str().to_string(),
            );
    }
    Path::new(path).strip_prefix(root_path).map_or_else(
        |_| path.to_string(),
        |relative| relative.to_string_lossy().into_owned(),
    )
}

fn should_fuzzy_match_full_path(query: &str) -> bool {
    query.contains('/') || query.contains('\\') || !query.contains('.')
}

#[cfg(target_arch = "wasm32")]
fn collect_entries(
    _root: &Path,
    _show_hidden: bool,
    _max_entries_per_dir: usize,
) -> Vec<ExplorerCandidate> {
    Vec::new()
}

/// Walk `root`, which must already be canonical, in parallel.
///
/// Entries are spelled from the root down, never canonicalized one by one: that would cost a
/// syscall per entry, and it would spell a symlink as its target, which is not the row the tree
/// shows for it.
#[cfg(not(target_arch = "wasm32"))]
fn collect_entries(
    root: &Path,
    show_hidden: bool,
    max_entries_per_dir: usize,
) -> Vec<ExplorerCandidate> {
    let mut walker = WalkBuilder::new(root);
    walker.hidden(!show_hidden);
    walker.git_ignore(true);
    walker.git_global(true);
    walker.git_exclude(true);
    walker.ignore(true);
    walker.parents(true);
    walker.require_git(false);
    walker.follow_links(false);
    walker.threads(walk_threads());

    let walked = Mutex::new(Walked::default());
    walker.build_parallel().visit(&mut CollectorBuilder {
        root,
        max_entries_per_dir,
        walked: &walked,
    });
    let walked = walked.into_inner().unwrap_or_default();
    let mut entries = walked.entries;
    entries.sort_by(|left: &ExplorerCandidate, right| left.path.cmp(&right.path));
    keep_loadable_entries(
        entries,
        walked.oversized,
        root,
        show_hidden,
        max_entries_per_dir,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn walk_threads() -> usize {
    std::thread::available_parallelism()
        .map(|threads| threads.get().min(8))
        .unwrap_or(2)
}

/// What the walker threads found.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Default)]
struct Walked {
    entries: Vec<ExplorerCandidate>,
    /// Directories holding more entries than the cap, ignored ones included.
    oversized: Vec<PathBuf>,
}

#[cfg(not(target_arch = "wasm32"))]
struct CollectorBuilder<'a> {
    root: &'a Path,
    max_entries_per_dir: usize,
    walked: &'a Mutex<Walked>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<'a> ParallelVisitorBuilder<'a> for CollectorBuilder<'a> {
    fn build(&mut self) -> Box<dyn ParallelVisitor + 'a> {
        Box::new(Collector {
            root: self.root,
            max_entries_per_dir: self.max_entries_per_dir,
            walked: self.walked,
            local: Walked::default(),
        })
    }
}

/// One walker thread's findings, handed over once when the thread finishes.
#[cfg(not(target_arch = "wasm32"))]
struct Collector<'a> {
    root: &'a Path,
    max_entries_per_dir: usize,
    walked: &'a Mutex<Walked>,
    local: Walked,
}

#[cfg(not(target_arch = "wasm32"))]
impl ParallelVisitor for Collector<'_> {
    fn visit(&mut self, entry: Result<DirEntry, ignore::Error>) -> WalkState {
        let Ok(entry) = entry else {
            return WalkState::Continue;
        };
        let path = entry.path();
        let Some(file_type) = entry.file_type() else {
            return WalkState::Continue;
        };
        // The walker never yields ignored entries, but the tree lists them and they count toward
        // its cap, so a directory is measured on disk. Only a count past the cap matters.
        if file_type.is_dir()
            && std::fs::read_dir(path).is_ok_and(|children| {
                children
                    .take(self.max_entries_per_dir.saturating_add(1))
                    .count()
                    > self.max_entries_per_dir
            })
        {
            self.local.oversized.push(path.to_path_buf());
        }
        if path == self.root {
            return WalkState::Continue;
        }
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            return WalkState::Continue;
        };
        self.local.entries.push(ExplorerCandidate {
            path: Arc::from(path.to_string_lossy().as_ref()),
            label: Arc::from(name),
            is_dir: file_type.is_dir(),
        });
        WalkState::Continue
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for Collector<'_> {
    fn drop(&mut self) {
        if let Ok(mut walked) = self.walked.lock() {
            walked.entries.append(&mut self.local.entries);
            walked.oversized.append(&mut self.local.oversized);
        }
    }
}

/// Keep only entries the tree can show. In a directory over the cap, that is what the tree's own
/// listing keeps, and nothing beneath a directory it leaves out. `entries` must be in path order,
/// which puts each directory before its descendants.
#[cfg(not(target_arch = "wasm32"))]
fn keep_loadable_entries(
    entries: Vec<ExplorerCandidate>,
    oversized: Vec<PathBuf>,
    root: &Path,
    show_hidden: bool,
    max_entries_per_dir: usize,
) -> Vec<ExplorerCandidate> {
    let listed: HashMap<PathBuf, HashSet<Arc<str>>> = oversized
        .into_iter()
        .filter_map(|dir| {
            let listing = read_directory(&dir.to_string_lossy(), show_hidden, max_entries_per_dir);
            (listing.omitted > 0).then(|| {
                let kept = listing
                    .entries
                    .into_iter()
                    .map(|entry| entry.path)
                    .collect();
                (dir, kept)
            })
        })
        .collect();
    if listed.is_empty() {
        return entries;
    }

    let mut left_out: HashSet<PathBuf> = HashSet::new();
    entries
        .into_iter()
        .filter(|entry| {
            let path = Path::new(entry.path.as_ref());
            if !left_out.is_empty() && is_under_capped_directory(path, &left_out, root) {
                return false;
            }
            let kept = path
                .parent()
                .and_then(|parent| listed.get(parent))
                .is_none_or(|kept| kept.contains(&entry.path));
            if !kept && entry.is_dir {
                left_out.insert(path.to_path_buf());
            }
            kept
        })
        .collect()
}

fn entry_directory_path(
    path: &Arc<str>,
    is_dir: bool,
    root_path: &Arc<str>,
    provided_paths: bool,
) -> Arc<str> {
    if is_dir {
        return path.clone();
    }

    if provided_paths {
        ProvidedPath::from_path(path.as_ref())
            .parent()
            .map(|parent| Arc::<str>::from(parent.as_str()))
            .unwrap_or_else(|| root_path.clone())
    } else {
        Path::new(path.as_ref())
            .parent()
            .map(|parent| Arc::<str>::from(parent.to_string_lossy().as_ref()))
            .unwrap_or_else(|| root_path.clone())
    }
}

fn is_under_capped_directory(path: &Path, capped_dirs: &HashSet<PathBuf>, root: &Path) -> bool {
    let mut current = path.parent();
    while let Some(parent) = current {
        if capped_dirs.contains(parent) {
            return true;
        }

        if parent == root {
            break;
        }

        current = parent.parent();
    }

    false
}

fn include_path_with_ancestors(
    visible: &mut HashSet<Arc<str>>,
    path: &Arc<str>,
    root_path: &Arc<str>,
    provided_paths: bool,
) {
    if provided_paths {
        include_provided_path_with_ancestors(visible, path, root_path);
        return;
    }

    let mut current = PathBuf::from(path.as_ref());
    loop {
        visible.insert(Arc::from(current.to_string_lossy().as_ref()));
        if current.as_os_str().is_empty() || current.as_path() == Path::new(root_path.as_ref()) {
            break;
        }
        if !current.pop() {
            break;
        }
    }
}

fn include_provided_path_with_ancestors(
    visible: &mut HashSet<Arc<str>>,
    path: &Arc<str>,
    root_path: &Arc<str>,
) {
    let root = ProvidedPath::from_path(root_path.as_ref());
    let Some(current) = ProvidedPath::from_path(path.as_ref()).strip_prefix(&root) else {
        return;
    };
    let mut full_path = root.clone();
    let components: Vec<_> = current.components().collect();
    for component in components {
        if !component.is_normal() {
            continue;
        }
        full_path = full_path.join_name(component.as_normal_str().unwrap_or_default());
        visible.insert(Arc::from(full_path.as_str()));
    }
    visible.insert(Arc::from(root.as_str()));
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn search_respects_gitignore_patterns() {
        let root = unique_test_dir("search_respects_gitignore_patterns");
        fs::create_dir_all(root.join("kept")).expect("create kept");
        fs::create_dir_all(root.join("ignored")).expect("create ignored");
        fs::write(root.join(".gitignore"), "ignored/\n*.tmp\n").expect("write gitignore");
        fs::write(root.join("kept").join("visible.txt"), "ok").expect("write kept file");
        fs::write(root.join("ignored").join("secret.txt"), "nope").expect("write ignored file");
        fs::write(root.join("note.tmp"), "nope").expect("write ignored tmp file");

        let entries = collect_entries(Path::new(normalize_path(&root).as_ref()), true, 10_000);
        let visible_path = normalize_path(&root.join("kept").join("visible.txt"));
        let ignored_path = normalize_path(&root.join("ignored").join("secret.txt"));
        let ignored_tmp_path = normalize_path(&root.join("note.tmp"));

        assert!(entries.iter().any(|entry| entry.path == visible_path));
        assert!(entries.iter().all(|entry| entry.path != ignored_path));
        assert!(entries.iter().all(|entry| entry.path != ignored_tmp_path));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn search_respects_gitignore_patterns_with_show_hidden_disabled() {
        let root = unique_test_dir("search_respects_gitignore_patterns_with_show_hidden_disabled");
        fs::create_dir_all(root.join("kept")).expect("create kept");
        fs::create_dir_all(root.join("ignored")).expect("create ignored");
        fs::write(root.join(".gitignore"), "ignored/\n*.tmp\n").expect("write gitignore");
        fs::write(root.join("kept").join("visible.txt"), "ok").expect("write kept file");
        fs::write(root.join("ignored").join("secret.txt"), "nope").expect("write ignored file");
        fs::write(root.join("note.tmp"), "nope").expect("write ignored tmp file");

        let entries = collect_entries(Path::new(normalize_path(&root).as_ref()), false, 10_000);
        let visible_path = normalize_path(&root.join("kept").join("visible.txt"));
        let ignored_path = normalize_path(&root.join("ignored").join("secret.txt"));
        let ignored_tmp_path = normalize_path(&root.join("note.tmp"));
        let gitignore_path = normalize_path(&root.join(".gitignore"));

        assert!(entries.iter().any(|entry| entry.path == visible_path));
        assert!(entries.iter().all(|entry| entry.path != ignored_path));
        assert!(entries.iter().all(|entry| entry.path != ignored_tmp_path));
        assert!(entries.iter().all(|entry| entry.path != gitignore_path));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn search_collects_visible_and_expanded_paths_for_matches() {
        let root = unique_test_dir("search_collects_visible_and_expanded_paths_for_matches");
        fs::create_dir_all(root.join("src").join("nested")).expect("create tree");
        fs::write(
            root.join("src").join("nested").join("target.rs"),
            "fn main() {}",
        )
        .expect("write file");

        let root_str = root.to_string_lossy().to_string();
        let filter = search_filesystem(&root_str, "target", true, 10_000);

        let root_path = normalize_path(&root);
        let src_path = normalize_path(&root.join("src"));
        let nested_path = normalize_path(&root.join("src").join("nested"));
        let file_path = normalize_path(&root.join("src").join("nested").join("target.rs"));

        assert_eq!(filter.match_count, 1);
        assert!(filter.visible_paths.contains(root_path.as_ref()));
        assert!(filter.visible_paths.contains(src_path.as_ref()));
        assert!(filter.visible_paths.contains(nested_path.as_ref()));
        assert!(filter.visible_paths.contains(file_path.as_ref()));

        assert!(filter.expanded_paths.contains(root_path.as_ref()));
        assert!(filter.expanded_paths.contains(src_path.as_ref()));
        assert!(filter.expanded_paths.contains(nested_path.as_ref()));
        assert!(!filter.expanded_paths.contains(file_path.as_ref()));

        let file_hits = filter
            .label_hits
            .get(file_path.as_ref())
            .expect("expected label hits for matched file");
        assert!(!file_hits.is_empty());

        assert_eq!(
            filter.primary_match_directory.as_deref(),
            Some(nested_path.as_ref())
        );

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn query_with_extension_filters_by_filename_not_path_fuzzy_noise() {
        let root = unique_test_dir("query_with_extension_filters_by_filename_not_path_fuzzy_noise");
        fs::create_dir_all(root.join("src").join("layout")).expect("create src/layout");
        fs::create_dir_all(root.join("tests")).expect("create tests");

        fs::write(root.join("src").join("layout.rs"), "mod layout;").expect("write layout.rs");
        fs::write(
            root.join("tests").join("layout_integration.rs"),
            "#[test] fn it_works() {}",
        )
        .expect("write layout_integration.rs");
        fs::write(
            root.join("src").join("layout").join("reconcile.rs"),
            "pub fn x() {}",
        )
        .expect("write reconcile.rs");

        let root_str = root.to_string_lossy().to_string();
        let filter = search_filesystem(&root_str, "layout.rs", true, 10_000);

        let layout_file = normalize_path(&root.join("src").join("layout.rs"));
        let layout_integration_file =
            normalize_path(&root.join("tests").join("layout_integration.rs"));
        let reconcile_file = normalize_path(&root.join("src").join("layout").join("reconcile.rs"));

        assert!(filter.visible_paths.contains(layout_file.as_ref()));
        assert!(
            filter
                .visible_paths
                .contains(layout_integration_file.as_ref())
        );
        assert!(!filter.visible_paths.contains(reconcile_file.as_ref()));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn search_candidates_only_uses_provided_candidates() {
        let root = unique_test_dir("search_candidates_only_uses_provided_candidates");
        let root_path = normalize_path(&root);
        let included_path = normalize_path(&root.join("src").join("target.rs"));
        let omitted_path = normalize_path(&root.join("other").join("target.rs"));

        let filter = ExplorerIndex::from_candidates(
            &root_path,
            [ExplorerCandidate {
                path: included_path.clone(),
                label: Arc::from("target.rs"),
                is_dir: false,
            }],
            false,
        )
        .search("target", usize::MAX);

        assert_eq!(filter.match_count, 1);
        assert!(filter.visible_paths.contains(included_path.as_ref()));
        assert!(!filter.visible_paths.contains(omitted_path.as_ref()));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn provided_candidate_search_uses_windows_parents_on_a_posix_host() {
        let root: Arc<str> = Arc::from(r"C:\remote\repo");
        let candidate = ExplorerCandidate {
            path: Arc::from(r"C:\remote\repo\src\target.rs"),
            label: Arc::from("target.rs"),
            is_dir: false,
        };

        let filter =
            ExplorerIndex::from_candidates(&root, [candidate], true).search("target", usize::MAX);

        assert!(filter.visible_paths.contains(r"C:\remote\repo"));
        assert!(filter.visible_paths.contains(r"C:\remote\repo\src"));
        assert!(
            filter
                .visible_paths
                .contains(r"C:\remote\repo\src\target.rs")
        );
        assert_eq!(
            filter.primary_match_directory.as_deref(),
            Some(r"C:\remote\repo\src")
        );
    }

    #[test]
    fn matching_ignores_the_root_path() {
        let root = unique_test_dir("matching_ignores_the_root_path_projxyz");
        fs::create_dir_all(root.join("src")).expect("create src");
        fs::write(root.join("src").join("main.rs"), "").expect("write main.rs");
        fs::write(root.join("README.md"), "").expect("write readme");

        let root_str = root.to_string_lossy().to_string();
        let filter = search_filesystem(&root_str, "projxyz", true, 10_000);

        assert_eq!(filter.match_count, 0);
        assert!(filter.visible_paths.is_empty());

        let filter = search_filesystem(&root_str, "src/main", true, 10_000);
        assert_eq!(filter.match_count, 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn search_keeps_the_best_matches_and_counts_them_all() {
        let root: Arc<str> = Arc::from("/repo");
        let candidate = |path: &str, is_dir: bool| ExplorerCandidate {
            path: Arc::from(format!("/repo/{path}")),
            label: Arc::from(path.rsplit('/').next().unwrap_or(path)),
            is_dir,
        };
        let index = ExplorerIndex::from_candidates(
            &root,
            [
                candidate("compat", true),
                candidate("compat/mod.rs", false),
                candidate("src", true),
                candidate("src/component.rs", false),
            ],
            false,
        );

        let all = index.search("comp", usize::MAX);
        assert_eq!(all.match_count, 3);
        assert_eq!(all.shown_count, 3);

        let best = index.search("component", 1);
        assert_eq!(best.match_count, 1);
        assert!(best.visible_paths.contains("/repo/src/component.rs"));

        let capped = index.search("comp", 1);
        assert_eq!(capped.match_count, 3);
        assert_eq!(capped.shown_count, 1);
        // `compat/mod.rs` matches only through its directory, so the two filename matches
        // outrank it, and the first of those in path order wins the tie for the one slot.
        assert!(!capped.visible_paths.contains("/repo/compat/mod.rs"));
        assert_eq!(capped.visible_paths.len(), 2);
    }

    #[test]
    fn walk_keeps_what_the_tree_lists_in_a_capped_directory() {
        let root = unique_test_dir("walk_keeps_what_the_tree_lists");
        fs::create_dir_all(root.join("b_dir")).expect("create b_dir");
        fs::write(root.join("b_dir").join("inner.rs"), "").expect("write inner");
        fs::write(root.join("a.rs"), "").expect("write a");
        fs::write(root.join("c.rs"), "").expect("write c");

        let canonical = normalize_path(&root);
        let entries = collect_entries(Path::new(canonical.as_ref()), true, 1);
        let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_ref()).collect();

        // The tree lists directories first, so with room for one entry it shows `b_dir`.
        assert_eq!(
            paths,
            [
                format!("{canonical}/b_dir").as_str(),
                format!("{canonical}/b_dir/inner.rs").as_str()
            ]
        );
        assert_searchable_entries_are_loadable(&entries, true, 1);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ignored_entries_take_the_tree_slots_they_fill() {
        let root = unique_test_dir("ignored_entries_take_tree_slots");
        fs::create_dir_all(&root).expect("create root");
        fs::write(root.join(".gitignore"), "*.log\n").expect("write gitignore");
        fs::write(root.join("a.log"), "").expect("write a.log");
        fs::write(root.join("b.log"), "").expect("write b.log");
        fs::write(root.join("z.rs"), "").expect("write z.rs");

        let canonical = normalize_path(&root);
        // The tree shows the two ignored logs and leaves `z.rs` out, so search cannot offer it.
        let entries = collect_entries(Path::new(canonical.as_ref()), false, 2);
        assert!(entries.is_empty(), "{entries:?}");

        let entries = collect_entries(Path::new(canonical.as_ref()), false, 3);
        assert_eq!(entries.len(), 1);
        assert_searchable_entries_are_loadable(&entries, false, 3);

        let _ = fs::remove_dir_all(&root);
    }

    fn assert_searchable_entries_are_loadable(
        entries: &[ExplorerCandidate],
        show_hidden: bool,
        max_entries_per_dir: usize,
    ) {
        for entry in entries {
            let parent = Path::new(entry.path.as_ref()).parent().expect("parent");
            let listing =
                read_directory(&parent.to_string_lossy(), show_hidden, max_entries_per_dir);
            assert!(
                listing
                    .entries
                    .iter()
                    .any(|listed| listed.path == entry.path),
                "{} is searchable but the tree cannot list it",
                entry.path
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn walk_spells_a_symlink_by_its_own_name() {
        let root = unique_test_dir("walk_spells_a_symlink_by_its_own_name");
        fs::create_dir_all(&root).expect("create root");
        fs::write(root.join("AGENTS.md"), "").expect("write target");
        std::os::unix::fs::symlink("AGENTS.md", root.join("CLAUDE.md")).expect("symlink");

        let root_str = root.to_string_lossy().to_string();
        let filter = search_filesystem(&root_str, "claude", true, 10_000);
        let link = normalize_path(&root).to_string() + "/CLAUDE.md";

        assert_eq!(filter.match_count, 1);
        assert!(filter.visible_paths.contains(link.as_str()));

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn index_cache_reuses_its_walk_until_the_key_changes() {
        let root = unique_test_dir("index_cache_reuses_its_walk");
        fs::create_dir_all(&root).expect("create root");
        fs::write(root.join("first.rs"), "").expect("write first");
        let key = ExplorerIndexKey {
            root: Arc::from(root.to_string_lossy().as_ref()),
            show_hidden: true,
            max_entries_per_dir: 10_000,
        };
        let cache = ExplorerIndexCache::default();

        let walked = cache.get_or_walk(key.clone());
        fs::write(root.join("second.rs"), "").expect("write second");
        let reused = cache.get_or_walk(key.clone());
        assert!(Arc::ptr_eq(&walked, &reused));
        assert_eq!(reused.search("second", usize::MAX).match_count, 0);

        let rewalked = cache.get_or_walk(ExplorerIndexKey {
            show_hidden: false,
            ..key
        });
        assert_eq!(rewalked.search("second", usize::MAX).match_count, 1);

        let _ = fs::remove_dir_all(&root);
    }

    fn search_filesystem(
        root: &str,
        query: &str,
        show_hidden: bool,
        max_entries_per_dir: usize,
    ) -> ExplorerFilter {
        ExplorerIndex::walk(ExplorerIndexKey {
            root: Arc::from(root),
            show_hidden,
            max_entries_per_dir,
        })
        .search(query, usize::MAX)
    }

    fn unique_test_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "tui_lipan_file_tree_{name}_{}_{}",
            std::process::id(),
            nanos
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }
}
