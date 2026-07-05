use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use ignore::WalkBuilder;
use nucleo::pattern::{CaseMatching, Normalization, Pattern};
use nucleo::{Config, Matcher, Utf32String};

use super::fs::normalize_path;

#[derive(Clone, Debug, Default)]
pub(crate) struct ExplorerFilter {
    pub(crate) visible_paths: HashSet<Arc<str>>,
    pub(crate) expanded_paths: HashSet<Arc<str>>,
    pub(crate) label_hits: HashMap<Arc<str>, Vec<u32>>,
    pub(crate) primary_match_directory: Option<Arc<str>>,
    pub(crate) match_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct ExplorerCandidate {
    pub(crate) path: Arc<str>,
    pub(crate) label: Arc<str>,
    pub(crate) is_dir: bool,
}

pub(crate) fn search_filesystem(
    root_path: &str,
    query: &str,
    show_hidden: bool,
    max_entries_per_dir: usize,
) -> ExplorerFilter {
    let query = query.trim();
    if query.is_empty() {
        return ExplorerFilter::default();
    }

    let root = PathBuf::from(root_path);
    let root_path = normalize_path(&root);
    let entries = collect_entries(&root, show_hidden, max_entries_per_dir);

    search_candidates(&root_path, entries, query)
}

pub(crate) fn search_candidates(
    root_path: &Arc<str>,
    candidates: impl IntoIterator<Item = ExplorerCandidate>,
    query: &str,
) -> ExplorerFilter {
    let query = query.trim();
    if query.is_empty() {
        return ExplorerFilter::default();
    }

    let mut matcher = Matcher::new(Config::DEFAULT);
    let pattern = Pattern::parse(query, CaseMatching::Smart, Normalization::Smart);
    let allow_path_fuzzy_match = should_fuzzy_match_full_path(query);

    let mut filter = ExplorerFilter::default();
    for entry in candidates {
        let mut label_hits = Vec::new();
        let label_utf32 = Utf32String::from(entry.label.as_ref());
        let label_match = pattern
            .indices(label_utf32.slice(..), &mut matcher, &mut label_hits)
            .is_some();

        let path_match = if allow_path_fuzzy_match {
            let path_utf32 = Utf32String::from(entry.path.as_ref());
            pattern
                .indices(path_utf32.slice(..), &mut matcher, &mut Vec::new())
                .is_some()
        } else {
            false
        };

        if !(label_match || path_match) {
            continue;
        }

        filter.match_count = filter.match_count.saturating_add(1);
        include_path_with_ancestors(&mut filter.visible_paths, &entry.path, root_path);

        if label_match {
            label_hits.sort_unstable();
            label_hits.dedup();
            if !label_hits.is_empty() {
                filter.label_hits.insert(entry.path.clone(), label_hits);
            }
        }

        let entry_dir = entry_directory_path(&entry, root_path);
        include_path_with_ancestors(&mut filter.expanded_paths, &entry_dir, root_path);
        if filter.primary_match_directory.is_none() {
            filter.primary_match_directory = Some(entry_dir);
        }
    }

    if !filter.visible_paths.is_empty() {
        filter.visible_paths.insert(root_path.clone());
    }

    if !filter.expanded_paths.is_empty() {
        filter.expanded_paths.insert(root_path.clone());
    }

    filter
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

#[cfg(not(target_arch = "wasm32"))]
fn collect_entries(
    root: &Path,
    show_hidden: bool,
    max_entries_per_dir: usize,
) -> Vec<ExplorerCandidate> {
    let mut entries = Vec::new();

    let mut per_dir_scanned: HashMap<PathBuf, usize> = HashMap::new();
    let mut capped_dirs: HashSet<PathBuf> = HashSet::new();

    let mut walker = WalkBuilder::new(root);
    walker.hidden(!show_hidden);
    walker.git_ignore(true);
    walker.git_global(true);
    walker.git_exclude(true);
    walker.ignore(true);
    walker.parents(true);
    walker.require_git(false);
    walker.follow_links(false);

    for child in walker.build() {
        let Ok(child) = child else {
            continue;
        };

        let child_path = child.path();
        if child_path == root {
            continue;
        }

        if is_under_capped_directory(child_path, &capped_dirs, root) {
            continue;
        }

        let Some(file_type) = child.file_type() else {
            continue;
        };
        let Some(parent) = child_path.parent() else {
            continue;
        };
        let Some(name) = child_path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };

        let scanned = per_dir_scanned.entry(parent.to_path_buf()).or_insert(0);
        if *scanned >= max_entries_per_dir {
            if file_type.is_dir() {
                capped_dirs.insert(child_path.to_path_buf());
            }
            continue;
        }

        *scanned = scanned.saturating_add(1);

        entries.push(ExplorerCandidate {
            path: normalize_path(child_path),
            label: Arc::from(name.to_string()),
            is_dir: file_type.is_dir(),
        });
    }

    entries.sort_by(|left, right| left.path.cmp(&right.path));
    entries
}

fn entry_directory_path(entry: &ExplorerCandidate, root_path: &Arc<str>) -> Arc<str> {
    if entry.is_dir {
        return entry.path.clone();
    }

    Path::new(entry.path.as_ref())
        .parent()
        .map(|parent| Arc::<str>::from(parent.to_string_lossy().as_ref()))
        .unwrap_or_else(|| root_path.clone())
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
) {
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

        let entries = collect_entries(&root, true, 10_000);
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

        let entries = collect_entries(&root, false, 10_000);
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

        let filter = search_candidates(
            &root_path,
            [ExplorerCandidate {
                path: included_path.clone(),
                label: Arc::from("target.rs"),
                is_dir: false,
            }],
            "target",
        );

        assert_eq!(filter.match_count, 1);
        assert!(filter.visible_paths.contains(included_path.as_ref()));
        assert!(!filter.visible_paths.contains(omitted_path.as_ref()));

        let _ = fs::remove_dir_all(&root);
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
