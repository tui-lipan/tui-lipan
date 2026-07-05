use std::borrow::Cow;
use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Component as PathComponent, Path, PathBuf};
use std::sync::Arc;

use crate::core::component::{Command, Component, Context, Update};
use crate::core::element::{Element, IntoElement};
use crate::style::Style;
use crate::text::input::TextInput;
use crate::widgets::{Divider, Input, InputEvent, ListItem, Tree, TreeNode, VStack};

use super::events::{FileTreeEvent, FileTreeToggleEvent};
use super::explorer::{ExplorerCandidate, ExplorerFilter, search_candidates, search_filesystem};
use super::fs::{FileIconStyle, FileKind, FsNode, read_directory, root_node};
use super::git::{
    GitFileDecorations, GitStatusSnapshot, discover_git_root, load_git_snapshot,
    provided_change_snapshot, provided_root_path,
};
use super::mod_private::FileTreeProps;
use super::{FileTreeChangeSource, FileTreeChangeView, FileTreeItemStyle, FileTreeSuffixPriority};
use crate::widgets::{TreeEvent, TreeToggleEvent};

#[derive(Clone, Debug)]
pub(crate) struct FileTreeState {
    pub(crate) root: FsNode,
    pub(crate) root_virtual: bool,
    pub(crate) expanded: HashSet<Arc<str>>,
    pub(crate) git_snapshot: GitStatusSnapshot,
    pub(crate) last_git_refresh_nonce: u64,
    pub(crate) changed_only_auto_expand_signature: u64,
    pub(crate) explorer_input: TextInput,
    pub(crate) explorer_query_id: u64,
    pub(crate) explorer_filter: ExplorerFilter,
    pub(crate) search_expanded_snapshot: Option<HashSet<Arc<str>>>,
    pub(crate) search_found_dir: Option<Arc<str>>,
}

#[derive(Clone, Debug)]
pub(crate) enum FileTreeMsg {
    TreeSelected {
        entry: Option<VisibleFileTreeEntry>,
    },
    TreeActivated {
        entry: Option<VisibleFileTreeEntry>,
    },
    TreeToggled {
        entry: Option<VisibleFileTreeEntry>,
        expanded: bool,
    },
    ExplorerQueryChanged(InputEvent),
    ExplorerResultsReady {
        query_id: u64,
        filter: ExplorerFilter,
    },
    RequestGitRefresh(u64),
    GitSnapshotLoaded {
        snapshot: GitStatusSnapshot,
    },
    SyncRootMode,
    EnsureChangedOnlyExpanded,
    EnsureRevealPaths,
    FocusExplorer,
    FocusTree,
}

pub(crate) struct FileTreeComponent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VisibleFileTreeEntry {
    path: Arc<str>,
    kind: FileKind,
}

struct FileTreeProjection {
    root: TreeNode,
    lookup: Arc<HashMap<crate::widgets::TreePath, VisibleFileTreeEntry>>,
    path_to_visible_index: HashMap<Arc<str>, usize>,
}

struct ProjectionBuildContext<'a> {
    props: &'a FileTreeProps,
    expanded: &'a HashSet<Arc<str>>,
    explorer_filter: Option<&'a ExplorerFilter>,
    git_decorations: &'a HashMap<Arc<str>, GitFileDecorations>,
    path_styles: &'a HashMap<Arc<str>, FileTreeItemStyle>,
    lookup: &'a mut HashMap<crate::widgets::TreePath, VisibleFileTreeEntry>,
}

impl FileTreeComponent {
    pub(crate) fn new() -> Self {
        Self
    }
}

impl Component for FileTreeComponent {
    type Message = FileTreeMsg;
    type Properties = FileTreeProps;
    type State = FileTreeState;

    fn create_state(&self, props: &Self::Properties) -> Self::State {
        let (root, root_virtual) = initial_root(props);
        let mut expanded = HashSet::new();
        expanded.insert(root.path.clone());
        let git_snapshot = effective_initial_snapshot(props);
        let mut state = Self::State {
            root,
            root_virtual,
            expanded,
            git_snapshot,
            last_git_refresh_nonce: props.git_refresh_nonce,
            changed_only_auto_expand_signature: 0,
            explorer_input: TextInput::new(""),
            explorer_query_id: 0,
            explorer_filter: ExplorerFilter::default(),
            search_expanded_snapshot: None,
            search_found_dir: None,
        };

        apply_reveal_paths_to_state(&mut state, props);
        state
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        if !needs_git_snapshot(&ctx.props) {
            return None;
        }

        let repo_root = discover_git_root(Path::new(ctx.props.root.as_ref()))
            .map(|path| Arc::<str>::from(path.to_string_lossy().as_ref()))?;

        Some(git_snapshot_command(
            ctx.link().clone(),
            repo_root,
            ctx.props.git_diff_stats,
        ))
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        if ctx.state.root_virtual != is_provided_changed_only(&ctx.props) {
            ctx.link().send(FileTreeMsg::SyncRootMode);
        }

        if needs_git_snapshot(&ctx.props)
            && ctx.props.git_refresh_nonce != ctx.state.last_git_refresh_nonce
        {
            ctx.link()
                .send(FileTreeMsg::RequestGitRefresh(ctx.props.git_refresh_nonce));
        }

        let change_snapshot = effective_change_snapshot(&ctx.props, &ctx.state);
        if should_auto_expand_changed_only(&ctx.props, &ctx.state, &change_snapshot) {
            ctx.link().send(FileTreeMsg::EnsureChangedOnlyExpanded);
        }

        if has_reveal_path(&ctx.props) {
            ctx.link().send(FileTreeMsg::EnsureRevealPaths);
        }

        let query = ctx.state.explorer_input.text().to_owned();
        let active_filter = if !ctx.props.explorer || query.trim().is_empty() {
            None
        } else {
            Some(&ctx.state.explorer_filter)
        };

        let projection = build_projection(&ctx.props, &ctx.state, active_filter);
        let selected_by_path = selected_visible_index_by_path(&ctx.props, &ctx.state, &projection);
        let select_lookup = projection.lookup.clone();
        let activate_lookup = projection.lookup.clone();
        let toggle_lookup = projection.lookup.clone();

        let has_explorer = ctx.props.explorer;

        let mut tree = Tree::new(projection.root)
            .show_icons(false)
            .solid_indent_connector_gap(file_tree_uses_guide_aware_nerd_arrows(&ctx.props))
            .indent_style(ctx.props.indent_style)
            .indent_guide_style(ctx.props.indent_guide_style)
            .style(ctx.props.style)
            .hover_style_slot(ctx.props.hover_style)
            .item_hover_style_slot(ctx.props.item_hover_style)
            .selection_style_slot(ctx.props.selection_style)
            .unfocused_selection_style_slot(ctx.props.unfocused_selection_style)
            .selection_full_width(true)
            .unselected_symbol(Some(""))
            .scrollbar(ctx.props.scrollbar)
            .scrollbar_config(ctx.props.scrollbar_config.clone())
            .scroll_keys(ctx.props.scroll_keys)
            .scroll_wheel(ctx.props.scroll_wheel)
            .show_scroll_indicators(ctx.props.show_scroll_indicators)
            .scroll_indicator_style(ctx.props.scroll_indicator_style)
            .width(ctx.props.width)
            .height(ctx.props.height)
            .force_scroll_to_selected(ctx.props.force_scroll_to_selected)
            .activate_on_click(ctx.props.activate_on_click)
            .focusable(ctx.props.focusable)
            .keymap(ctx.props.keymap)
            .on_select(
                ctx.link()
                    .callback(move |event: TreeEvent| FileTreeMsg::TreeSelected {
                        entry: select_lookup.get(&event.path).cloned(),
                    }),
            )
            .on_activate(
                ctx.link()
                    .callback(move |event: TreeEvent| FileTreeMsg::TreeActivated {
                        entry: activate_lookup.get(&event.path).cloned(),
                    }),
            )
            .on_toggle(ctx.link().callback(move |event: TreeToggleEvent| {
                FileTreeMsg::TreeToggled {
                    entry: toggle_lookup.get(&event.path).cloned(),
                    expanded: event.expanded,
                }
            }));
        if let Some(selected) = selected_by_path.or(ctx.props.selected) {
            tree = tree.selected(selected);
        }
        if selected_by_path.is_some() && ctx.props.select_path.is_some() {
            tree = tree.force_scroll_to_selected(true);
        }

        // When explorer is enabled, "/" on the tree focuses the search input
        if has_explorer {
            tree = tree.key_interceptor(ctx.link().key_handler(|key| match key.code {
                crate::core::event::KeyCode::Char('/') => Some(FileTreeMsg::FocusExplorer),
                _ => None,
            }));
        }

        if let Some(symbol) = ctx.props.selection_symbol.clone() {
            tree = tree.selection_symbol(Some(symbol));
        }
        if let Some(style) = ctx.props.selection_symbol_style {
            tree = tree.selection_symbol_style(Some(style));
        }
        if let Some(style) = ctx.props.unfocused_selection_symbol_style {
            tree = tree.unfocused_selection_symbol_style(Some(style));
        }
        if let Some(text) = ctx.props.empty_text.clone() {
            tree = tree
                .empty_text(text)
                .empty_text_style(ctx.props.empty_text_style);
        }

        if !has_explorer {
            return tree.into();
        }

        let query = ctx.state.explorer_input.text().to_owned();
        let input = Input::new(query)
            .cursor(ctx.state.explorer_input.cursor())
            .anchor(ctx.state.explorer_input.anchor())
            .placeholder(ctx.props.explorer_placeholder.as_ref())
            .prefix(ctx.props.explorer_prefix.clone())
            .suffix(format!("{}", ctx.state.explorer_filter.match_count))
            .suffix_style(Style::default())
            .focus_suffix_style(Style::default())
            .border(ctx.props.explorer_input_border)
            .border_style(ctx.props.explorer_input_border_style)
            .padding(ctx.props.explorer_input_padding)
            .style(ctx.props.explorer_input_style)
            .focus_style_slot(ctx.props.explorer_input_focus_style)
            .focus_content_style(ctx.props.explorer_input_focus_content_style)
            .placeholder_style(ctx.props.explorer_placeholder_style)
            .focus_placeholder_style(ctx.props.explorer_focus_placeholder_style)
            .on_change(ctx.link().callback(FileTreeMsg::ExplorerQueryChanged))
            .tab_order(false)
            .key_interceptor(ctx.link().key_handler(|key| match key.code {
                crate::core::event::KeyCode::Esc => Some(FileTreeMsg::FocusTree),
                _ => None,
            }));

        let mut layout = VStack::new()
            .width(ctx.props.width)
            .height(ctx.props.height)
            .child(input.key("__ft_input"));

        if ctx.props.explorer_divider {
            let divider = Divider::horizontal()
                .join_frame(ctx.props.explorer_divider_join_frame)
                .ch(ctx.props.explorer_divider_char)
                .style(ctx.props.explorer_divider_style);
            layout = layout.child(divider);
        }

        layout.child(tree.key("__ft_tree")).into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            FileTreeMsg::ExplorerQueryChanged(event) => {
                ctx.state
                    .explorer_input
                    .set_text(event.value.as_ref().to_string());
                ctx.state.explorer_input.set_cursor(event.cursor);
                ctx.state.explorer_input.set_anchor(event.anchor);

                let query: Arc<str> = Arc::from(ctx.state.explorer_input.text().to_owned());
                if query.trim().is_empty() {
                    let restore_dir = ctx
                        .state
                        .search_found_dir
                        .clone()
                        .or_else(|| ctx.state.explorer_filter.primary_match_directory.clone());

                    ctx.state.explorer_filter = ExplorerFilter::default();

                    if let Some(snapshot) = ctx.state.search_expanded_snapshot.take() {
                        ctx.state.expanded = snapshot;
                    }

                    if let Some(path) = restore_dir {
                        insert_path_with_ancestors(
                            &mut ctx.state.expanded,
                            &path,
                            &ctx.state.root.path,
                        );
                    }

                    ctx.state.search_found_dir = None;

                    load_expanded_directories_sync(
                        &mut ctx.state.root,
                        &ctx.state.expanded,
                        ctx.props.show_hidden,
                        ctx.props.max_entries_per_dir,
                    );
                    return Update::full();
                }

                if ctx.state.search_expanded_snapshot.is_none() {
                    ctx.state.search_expanded_snapshot = Some(ctx.state.expanded.clone());
                    ctx.state.search_found_dir = None;
                }

                let query_id = ctx.state.explorer_query_id.saturating_add(1);
                ctx.state.explorer_query_id = query_id;
                let search_root = if ctx.props.change_view == FileTreeChangeView::ChangedOnly {
                    ctx.state.root.path.clone()
                } else {
                    ctx.props.root.clone()
                };
                Update::with_command(spawn_explorer_search(
                    ctx.link().clone(),
                    query_id,
                    query,
                    search_root,
                    ctx.props.show_hidden,
                    ctx.props.max_entries_per_dir,
                    changed_only_search_candidates(&ctx.props, &ctx.state),
                ))
            }
            FileTreeMsg::ExplorerResultsReady { query_id, filter } => {
                if query_id != ctx.state.explorer_query_id {
                    return Update::none();
                }

                if ctx.state.explorer_input.text().trim().is_empty() {
                    return Update::none();
                }

                ctx.state.explorer_filter = filter;

                if ctx.state.search_found_dir.is_none() {
                    ctx.state.search_found_dir =
                        ctx.state.explorer_filter.primary_match_directory.clone();
                }

                for path in &ctx.state.explorer_filter.expanded_paths {
                    ctx.state.expanded.insert(path.clone());
                }

                load_expanded_directories_sync(
                    &mut ctx.state.root,
                    &ctx.state.expanded,
                    ctx.props.show_hidden,
                    ctx.props.max_entries_per_dir,
                );
                Update::full()
            }
            FileTreeMsg::TreeSelected { entry } => {
                if let Some(entry) = entry {
                    if !ctx.state.explorer_input.text().trim().is_empty() {
                        ctx.state.search_found_dir = Some(selected_directory_for_restore(
                            &entry.path,
                            entry.kind,
                            &ctx.state.root.path,
                        ));
                    }

                    if let Some(cb) = ctx.props.on_select.as_ref() {
                        cb.emit(FileTreeEvent {
                            path: entry.path,
                            kind: entry.kind,
                        });
                    }
                }
                Update::none()
            }
            FileTreeMsg::TreeActivated { entry } => {
                if let Some(entry) = entry
                    && let Some(cb) = ctx.props.on_activate.as_ref()
                {
                    cb.emit(FileTreeEvent {
                        path: entry.path,
                        kind: entry.kind,
                    });
                }
                Update::none()
            }
            FileTreeMsg::TreeToggled { entry, expanded } => {
                let node_path = if let Some(entry) = entry {
                    if ctx.props.expanded_paths.is_none() {
                        if expanded {
                            ctx.state.expanded.insert(entry.path.clone());
                        } else {
                            ctx.state.expanded.remove(&entry.path);
                        }
                    }

                    if let Some(cb) = ctx.props.on_toggle.as_ref() {
                        cb.emit(FileTreeToggleEvent {
                            path: entry.path.clone(),
                            kind: entry.kind,
                            expanded,
                        });
                    }

                    Some(entry.path)
                } else {
                    None
                };

                if ctx.props.change_view != FileTreeChangeView::ChangedOnly
                    && expanded
                    && let Some(path) = node_path
                    && let Some(node) = node_by_path_mut(&mut ctx.state.root, &path)
                    && node.is_dir()
                    && !node.loaded
                {
                    let result =
                        read_directory(&path, ctx.props.show_hidden, ctx.props.max_entries_per_dir);
                    apply_directory_load(node, result);
                }

                Update::full()
            }
            FileTreeMsg::RequestGitRefresh(nonce) => {
                if !needs_git_snapshot(&ctx.props) || nonce <= ctx.state.last_git_refresh_nonce {
                    return Update::none();
                }

                ctx.state.last_git_refresh_nonce = nonce;
                let root = ctx.props.root.clone();
                let include_diff_stats = ctx.props.git_diff_stats;
                Update {
                    dirty: false,
                    level: crate::core::component::UpdateLevel::None,
                    command: Some(ctx.link().command(move |link| {
                        let snapshot = discover_git_root(Path::new(root.as_ref()))
                            .and_then(|repo_root| load_git_snapshot(&repo_root, include_diff_stats))
                            .unwrap_or_default();
                        link.send(FileTreeMsg::GitSnapshotLoaded { snapshot });
                    })),
                }
            }
            FileTreeMsg::GitSnapshotLoaded { snapshot } => {
                ctx.state.git_snapshot = snapshot;
                let snapshot = effective_change_snapshot(&ctx.props, &ctx.state);
                if should_auto_expand_changed_only(&ctx.props, &ctx.state, &snapshot) {
                    expand_changed_only_directories(&mut ctx.state, &snapshot);
                    ctx.state.changed_only_auto_expand_signature =
                        change_snapshot_signature(&snapshot);
                }
                Update::full()
            }
            FileTreeMsg::SyncRootMode => {
                if ctx.state.root_virtual == is_provided_changed_only(&ctx.props) {
                    return Update::none();
                }

                rebuild_root_for_props(&mut ctx.state, &ctx.props);
                Update::full()
            }
            FileTreeMsg::EnsureChangedOnlyExpanded => {
                let snapshot = effective_change_snapshot(&ctx.props, &ctx.state);
                if should_auto_expand_changed_only(&ctx.props, &ctx.state, &snapshot) {
                    expand_changed_only_directories(&mut ctx.state, &snapshot);
                    ctx.state.changed_only_auto_expand_signature =
                        change_snapshot_signature(&snapshot);
                    return Update::full();
                }
                Update::none()
            }
            FileTreeMsg::EnsureRevealPaths => {
                if apply_reveal_paths_to_state(&mut ctx.state, &ctx.props) {
                    return Update::full();
                }
                Update::none()
            }
            FileTreeMsg::FocusExplorer => {
                ctx.request_focus("__ft_input");
                Update::none()
            }
            FileTreeMsg::FocusTree => {
                ctx.request_focus("__ft_tree");
                Update::none()
            }
        }
    }
}

fn needs_git_snapshot(props: &FileTreeProps) -> bool {
    matches!(props.change_source, FileTreeChangeSource::Git)
        && (props.git_status
            || props.git_diff_stats
            || props.change_view == FileTreeChangeView::ChangedOnly)
}

fn is_provided_changed_only(props: &FileTreeProps) -> bool {
    matches!(props.change_source, FileTreeChangeSource::Provided(_))
        && props.change_view == FileTreeChangeView::ChangedOnly
}

fn initial_root(props: &FileTreeProps) -> (FsNode, bool) {
    let root_virtual = is_provided_changed_only(props);
    let mut root = if root_virtual {
        virtual_root_node(&props.root)
    } else {
        root_node(&props.root)
    };
    if root.is_dir() && !root_virtual {
        let result = read_directory(&root.path, props.show_hidden, props.max_entries_per_dir);
        apply_directory_load(&mut root, result);
    }
    (root, root_virtual)
}

fn rebuild_root_for_props(state: &mut FileTreeState, props: &FileTreeProps) {
    let (root, root_virtual) = initial_root(props);
    state.root = root;
    state.root_virtual = root_virtual;
    state.expanded.clear();
    state.expanded.insert(state.root.path.clone());
    state.git_snapshot = effective_initial_snapshot(props);
    state.last_git_refresh_nonce = props.git_refresh_nonce;
    state.changed_only_auto_expand_signature = 0;
    state.explorer_filter = ExplorerFilter::default();
    state.search_expanded_snapshot = None;
    state.search_found_dir = None;
}

fn effective_initial_snapshot(props: &FileTreeProps) -> GitStatusSnapshot {
    match &props.change_source {
        FileTreeChangeSource::Git => GitStatusSnapshot::default(),
        FileTreeChangeSource::Provided(changes) => {
            provided_change_snapshot(props.root.as_ref(), changes)
        }
    }
}

fn effective_change_snapshot(props: &FileTreeProps, state: &FileTreeState) -> GitStatusSnapshot {
    match &props.change_source {
        FileTreeChangeSource::Git => state.git_snapshot.clone(),
        FileTreeChangeSource::Provided(changes) => {
            provided_change_snapshot(state.root.path.as_ref(), changes)
        }
    }
}

fn should_auto_expand_changed_only(
    props: &FileTreeProps,
    state: &FileTreeState,
    snapshot: &GitStatusSnapshot,
) -> bool {
    props.change_view == FileTreeChangeView::ChangedOnly
        && props.expanded_paths.is_none()
        && !snapshot.changed_paths.is_empty()
        && state.changed_only_auto_expand_signature != change_snapshot_signature(snapshot)
}

fn effective_expanded_paths<'a>(
    props: &FileTreeProps,
    state: &'a FileTreeState,
) -> Cow<'a, HashSet<Arc<str>>> {
    let reveal_paths = normalized_reveal_paths(props, &state.root.path);
    if let Some(expanded_paths) = props.expanded_paths.as_ref() {
        let mut expanded_paths = expanded_paths.clone();
        expanded_paths.insert(state.root.path.clone());
        for path in reveal_paths {
            insert_path_with_ancestors(&mut expanded_paths, &path, &state.root.path);
        }
        Cow::Owned(expanded_paths)
    } else if !reveal_paths.is_empty() {
        let mut expanded_paths = state.expanded.clone();
        for path in reveal_paths {
            insert_path_with_ancestors(&mut expanded_paths, &path, &state.root.path);
        }
        Cow::Owned(expanded_paths)
    } else {
        Cow::Borrowed(&state.expanded)
    }
}

fn has_reveal_path(props: &FileTreeProps) -> bool {
    props.reveal_path.is_some() || props.select_path.is_some()
}

fn normalized_reveal_paths(props: &FileTreeProps, root_path: &Arc<str>) -> Vec<Arc<str>> {
    [props.reveal_path.as_ref(), props.select_path.as_ref()]
        .into_iter()
        .flatten()
        .filter_map(|path| resolve_path_under_root(root_path, path.as_ref()))
        .collect()
}

fn selected_visible_index_by_path(
    props: &FileTreeProps,
    state: &FileTreeState,
    projection: &FileTreeProjection,
) -> Option<usize> {
    let path = props
        .select_path
        .as_ref()
        .or(props.selected_path.as_ref())
        .and_then(|path| resolve_path_under_root(&state.root.path, path.as_ref()))?;
    projection.path_to_visible_index.get(path.as_ref()).copied()
}

fn apply_reveal_paths_to_state(state: &mut FileTreeState, props: &FileTreeProps) -> bool {
    let reveal_paths = normalized_reveal_paths(props, &state.root.path);
    if reveal_paths.is_empty() {
        return false;
    }

    let mut changed = false;
    let mut expanded_for_load = effective_expanded_paths(props, state).into_owned();
    if props.expanded_paths.is_none() {
        for path in &reveal_paths {
            let before = state.expanded.len();
            insert_path_with_ancestors(&mut state.expanded, path, &state.root.path);
            changed |= state.expanded.len() != before;
        }
        expanded_for_load = state.expanded.clone();
    }

    let before = loaded_signature(&state.root);
    load_expanded_directories_sync(
        &mut state.root,
        &expanded_for_load,
        props.show_hidden,
        props.max_entries_per_dir,
    );
    changed || loaded_signature(&state.root) != before
}

fn loaded_signature(node: &FsNode) -> u64 {
    let mut hasher = DefaultHasher::new();
    hash_loaded_signature(node, &mut hasher);
    hasher.finish()
}

fn hash_loaded_signature(node: &FsNode, hasher: &mut DefaultHasher) {
    node.path.hash(hasher);
    node.loaded.hash(hasher);
    node.error.hash(hasher);
    node.children.len().hash(hasher);
    for child in &node.children {
        hash_loaded_signature(child, hasher);
    }
}

fn change_snapshot_signature(snapshot: &GitStatusSnapshot) -> u64 {
    if snapshot.changed_paths.is_empty() {
        return 0;
    }

    let mut hasher = DefaultHasher::new();
    snapshot.changed_paths.hash(&mut hasher);
    hasher.finish()
}

fn git_snapshot_command(
    link: crate::callback::Link<FileTreeMsg>,
    repo_root: Arc<str>,
    include_diff_stats: bool,
) -> Command {
    link.command(move |link| {
        let snapshot = load_git_snapshot(Path::new(repo_root.as_ref()), include_diff_stats)
            .unwrap_or_default();
        link.send(FileTreeMsg::GitSnapshotLoaded { snapshot });
    })
}

fn spawn_explorer_search(
    link: crate::callback::Link<FileTreeMsg>,
    query_id: u64,
    query: Arc<str>,
    root: Arc<str>,
    show_hidden: bool,
    max_entries_per_dir: usize,
    candidates: Option<Vec<ExplorerCandidate>>,
) -> Command {
    link.command(move |link| {
        let filter = if let Some(candidates) = candidates {
            search_candidates(&root, candidates, query.as_ref())
        } else {
            search_filesystem(
                root.as_ref(),
                query.as_ref(),
                show_hidden,
                max_entries_per_dir,
            )
        };
        link.send(FileTreeMsg::ExplorerResultsReady { query_id, filter });
    })
}

fn apply_directory_load(node: &mut FsNode, result: super::fs::DirectoryLoadResult) {
    node.loading = false;
    node.loaded = true;
    node.error = result.error.clone();
    node.children.clear();

    if node.error.is_none() {
        node.children = result
            .entries
            .into_iter()
            .map(|entry| FsNode {
                name: entry.name,
                path: entry.path,
                kind: entry.kind,
                loaded: !matches!(entry.kind, FileKind::Directory),
                loading: false,
                error: None,
                children: Vec::new(),
            })
            .collect();

        if result.omitted > 0 {
            node.children.push(FsNode {
                name: format!("... {} more entries", result.omitted).into(),
                path: format!("{}/__truncated__", node.path).into(),
                kind: FileKind::Other,
                loaded: true,
                loading: false,
                error: None,
                children: Vec::new(),
            });
        }
    }
}

fn load_expanded_directories_sync(
    root: &mut FsNode,
    expanded: &HashSet<Arc<str>>,
    show_hidden: bool,
    max_entries: usize,
) {
    if !root.is_dir() || !expanded.contains(&root.path) {
        return;
    }

    if !root.loaded {
        let result = read_directory(&root.path, show_hidden, max_entries);
        apply_directory_load(root, result);
    }

    for i in 0..root.children.len() {
        load_expanded_directories_sync(&mut root.children[i], expanded, show_hidden, max_entries);
    }
}

fn selected_directory_for_restore(
    path: &Arc<str>,
    kind: FileKind,
    root_path: &Arc<str>,
) -> Arc<str> {
    if kind == FileKind::Directory {
        return path.clone();
    }

    let parent: Option<Arc<str>> = Path::new(path.as_ref())
        .parent()
        .map(|value| Arc::<str>::from(value.to_string_lossy().as_ref()));

    match parent {
        Some(path) if Path::new(path.as_ref()).starts_with(root_path.as_ref()) => path,
        _ => root_path.clone(),
    }
}

fn insert_path_with_ancestors(
    expanded: &mut HashSet<Arc<str>>,
    path: &Arc<str>,
    root_path: &Arc<str>,
) {
    let mut current = PathBuf::from(path.as_ref());
    let root = Path::new(root_path.as_ref());

    loop {
        if !current.starts_with(root) {
            break;
        }

        expanded.insert(Arc::from(current.to_string_lossy().as_ref()));

        if current.as_path() == root {
            break;
        }

        if !current.pop() {
            break;
        }
    }
}

fn resolve_path_under_root(root_path: &Arc<str>, path: &str) -> Option<Arc<str>> {
    let root = Path::new(root_path.as_ref());
    let input = Path::new(path);
    let candidate = if input.is_absolute() {
        canonical_or_lexical_path(input)
    } else {
        canonical_or_lexical_path(&root.join(input))
    };

    if candidate.starts_with(root) {
        Some(Arc::<str>::from(candidate.to_string_lossy().as_ref()))
    } else {
        None
    }
}

fn build_projection(
    props: &FileTreeProps,
    state: &FileTreeState,
    explorer_filter: Option<&ExplorerFilter>,
) -> FileTreeProjection {
    let snapshot = effective_change_snapshot(props, state);
    let root = if props.change_view == FileTreeChangeView::ChangedOnly {
        build_changed_only_root(&state.root, &snapshot, props.show_hidden)
    } else {
        state.root.clone()
    };
    let expanded = effective_expanded_paths(props, state);

    let mut lookup = HashMap::new();
    let path_styles = resolved_path_styles(&props.path_styles, root.path.as_ref());
    let mut build_ctx = ProjectionBuildContext {
        props,
        expanded: expanded.as_ref(),
        explorer_filter,
        git_decorations: &snapshot.entries,
        path_styles: &path_styles,
        lookup: &mut lookup,
    };
    let root = build_projected_tree_node(&root, true, Vec::new(), &mut build_ctx);
    let path_to_visible_index = visible_index_by_path(&root, &lookup);

    FileTreeProjection {
        root,
        lookup: Arc::new(lookup),
        path_to_visible_index,
    }
}

fn visible_index_by_path(
    root: &TreeNode,
    lookup: &HashMap<crate::widgets::TreePath, VisibleFileTreeEntry>,
) -> HashMap<Arc<str>, usize> {
    let mut out = HashMap::new();
    let mut next_index = 0usize;
    collect_visible_index_by_path(root, Vec::new(), lookup, &mut next_index, &mut out);
    out
}

fn collect_visible_index_by_path(
    node: &TreeNode,
    tree_path: Vec<usize>,
    lookup: &HashMap<crate::widgets::TreePath, VisibleFileTreeEntry>,
    next_index: &mut usize,
    out: &mut HashMap<Arc<str>, usize>,
) {
    let path_key = crate::widgets::TreePath::from(tree_path.clone());
    if let Some(entry) = lookup.get(&path_key) {
        out.insert(entry.path.clone(), *next_index);
    }
    *next_index = (*next_index).saturating_add(1);

    if !node.expanded {
        return;
    }

    for (index, child) in node.children.iter().enumerate() {
        let mut child_path = tree_path.clone();
        child_path.push(index);
        collect_visible_index_by_path(child, child_path, lookup, next_index, out);
    }
}

fn build_projected_tree_node(
    node: &FsNode,
    is_root: bool,
    tree_path: Vec<usize>,
    ctx: &mut ProjectionBuildContext<'_>,
) -> TreeNode {
    use crate::style::Span;

    ctx.lookup.insert(
        crate::widgets::TreePath::from(tree_path.clone()),
        VisibleFileTreeEntry {
            path: node.path.clone(),
            kind: node.kind,
        },
    );

    let mut spans = Vec::new();

    let is_expanded = ctx.expanded.contains(&node.path);

    // Get git decorations for this node
    let git_decoration = ctx.git_decorations.get(node.path.as_ref()).copied();
    let git_status = git_decoration.map(|decoration| decoration.status);

    let path_style = ctx.path_styles.get(node.path.as_ref()).copied();

    if ctx.props.show_icons {
        let mut icon_span = node.kind.icon(&node.path, is_expanded, is_root, ctx.props);
        if let Some(style) = path_style.and_then(|style| style.icon) {
            icon_span.style = icon_span.style.patch(style);
        }
        spans.push(icon_span);
        spans.push(Span::new(" "));
    }

    // Build the label. Change status styles are opt-in so file names stay readable
    // while indicators carry dirty-state color by default.
    let label = if is_root {
        super::fs::path_to_display(node.path.as_ref())
    } else {
        node.name.as_ref().to_string()
    };

    let kind_label_style = match node.kind {
        FileKind::Directory => ctx.props.directory_label_style,
        FileKind::File => ctx.props.file_label_style,
        FileKind::Symlink | FileKind::Other => Style::default(),
    };

    let mut label_base_style = if ctx.props.highlight_changed_labels
        && ctx.props.git_status
        && let Some(status) = git_status
        && !is_root
    {
        kind_label_style.patch(status.style(ctx.props))
    } else {
        kind_label_style
    };
    if let Some(style) = path_style.and_then(|style| style.label) {
        label_base_style = label_base_style.patch(style);
    }

    let label_hits = ctx
        .explorer_filter
        .and_then(|filter| filter.label_hits.get(node.path.as_ref()))
        .map(Vec::as_slice);
    spans.extend(highlight_label_spans(
        &label,
        label_hits,
        label_base_style,
        ctx.props.explorer_match_style,
    ));

    let mut description_spans =
        git_description_spans(git_decoration, ctx.props, node.is_dir(), is_expanded);
    apply_suffix_style(
        &mut description_spans,
        ctx.props.change_suffix_style,
        path_style.and_then(|style| style.suffix),
    );

    let mut item = ListItem::from_spans(spans).primary_truncate_description_first(matches!(
        ctx.props.change_suffix_priority,
        FileTreeSuffixPriority::Label
    ));
    if let Some(style) = path_style.and_then(|style| style.row) {
        item = item.style(style);
    }
    if !description_spans.is_empty() {
        item = item.description_spans(description_spans);
    }

    let mut tree = TreeNode::new(item).expanded(is_expanded);
    if file_tree_needs_nerd_arrow_placeholder(node, is_root, ctx.props) {
        tree = tree.leading_guide_fill_cells(2);
    }

    if node.is_dir() {
        if node.loading {
            tree = tree.child(TreeNode::new(ctx.props.loading_label.clone()));
        } else if !node.loaded {
            tree = tree.child(TreeNode::new(" "));
        } else if let Some(error) = node.error.as_ref() {
            tree = tree.child(TreeNode::new(format!("{} {error}", ctx.props.error_prefix)));
        } else {
            let mut display_index = 0usize;
            for child in &node.children {
                let is_visible = ctx
                    .explorer_filter
                    .map(|filter| filter.visible_paths.contains(child.path.as_ref()))
                    .unwrap_or(true);
                if !is_visible {
                    continue;
                }

                let mut child_path = tree_path.clone();
                child_path.push(display_index);
                display_index = display_index.saturating_add(1);
                tree = tree.child(build_projected_tree_node(child, false, child_path, ctx));
            }
        }
    }

    tree
}

fn apply_suffix_style(
    spans: &mut [crate::style::Span],
    global_style: Style,
    path_style: Option<Style>,
) {
    if spans.is_empty() {
        return;
    }

    for span in spans {
        span.style = span.style.patch(global_style);
        if let Some(style) = path_style {
            span.style = span.style.patch(style);
        }
    }
}

fn resolved_path_styles(
    styles: &HashMap<Arc<str>, FileTreeItemStyle>,
    effective_root: &str,
) -> HashMap<Arc<str>, FileTreeItemStyle> {
    let root = Path::new(effective_root);
    styles
        .iter()
        .filter_map(|(path, style)| {
            resolve_item_style_path(root, path.as_ref()).map(|resolved| {
                (
                    Arc::<str>::from(resolved.to_string_lossy().as_ref()),
                    *style,
                )
            })
        })
        .collect()
}

fn resolve_item_style_path(root: &Path, path: &str) -> Option<PathBuf> {
    let input = Path::new(path);
    let candidate = if input.is_absolute() {
        canonical_or_lexical_path(input)
    } else {
        canonical_or_lexical_path(&root.join(input))
    };

    if candidate.starts_with(root) {
        Some(candidate)
    } else {
        None
    }
}

fn canonical_or_lexical_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| lexical_normalize_path(path))
}

fn lexical_normalize_path(path: &Path) -> PathBuf {
    let mut output = PathBuf::new();
    for component in path.components() {
        match component {
            PathComponent::Prefix(prefix) => output.push(prefix.as_os_str()),
            PathComponent::RootDir => output.push(component.as_os_str()),
            PathComponent::CurDir => {}
            PathComponent::ParentDir => {
                output.pop();
            }
            PathComponent::Normal(part) => output.push(part),
        }
    }
    output
}

fn git_description_spans(
    decoration: Option<GitFileDecorations>,
    props: &FileTreeProps,
    is_dir: bool,
    is_expanded: bool,
) -> Vec<crate::style::Span> {
    use crate::style::Span;

    let Some(decoration) = decoration else {
        return Vec::new();
    };

    let show_dir_metadata = !is_dir || !is_expanded;
    if !show_dir_metadata {
        return Vec::new();
    }

    let mut spans = Vec::new();
    if props.git_status {
        push_git_status_spans(&mut spans, decoration.status, props);
    }

    if props.git_diff_stats
        && let Some(diff_stat) = decoration.diff_stat
        && !diff_stat.is_empty()
    {
        spans.push(Span::new(" "));
        if diff_stat.added > 0 {
            spans.push(Span::new(format!("+{}", diff_stat.added)).style(props.git_style_added));
        }
        if diff_stat.removed > 0 {
            if diff_stat.added > 0 {
                spans.push(Span::new(" "));
            }
            spans.push(Span::new(format!("-{}", diff_stat.removed)).style(props.git_style_deleted));
        }
    }

    spans
}

fn push_git_status_spans(
    spans: &mut Vec<crate::style::Span>,
    status: super::git::GitFileStatus,
    props: &FileTreeProps,
) {
    use crate::style::Span;

    if status.staged.is_none() && status.unstaged.is_none() {
        return;
    }

    spans.push(Span::new(" "));
    if let Some(staged) = status.staged {
        spans.push(Span::new(git_marker(staged, props, true)).style(props.git_style_added));
    }

    if let Some(unstaged) = status.unstaged {
        if status.staged.is_some() {
            spans.push(Span::new(" "));
        }
        spans.push(Span::new(git_marker(unstaged, props, false)).style(unstaged.style(props)));
    }
}

fn git_marker(
    state: super::git::GitChangeState,
    props: &FileTreeProps,
    is_staged: bool,
) -> Arc<str> {
    if props.git_icon_style == super::git::GitIconStyle::NerdFont {
        return Arc::from(state.marker(props.git_icon_style, is_staged));
    }

    match state {
        super::git::GitChangeState::Modified => props.git_marker_modified.clone(),
        super::git::GitChangeState::Added => props.git_marker_added.clone(),
        super::git::GitChangeState::Deleted => props.git_marker_deleted.clone(),
        super::git::GitChangeState::Renamed => props.git_marker_renamed.clone(),
        super::git::GitChangeState::Untracked => props.git_marker_untracked.clone(),
        super::git::GitChangeState::Conflicted => props.git_marker_conflicted.clone(),
    }
}

fn build_changed_only_root(
    source_root: &FsNode,
    snapshot: &GitStatusSnapshot,
    show_hidden: bool,
) -> FsNode {
    let mut root = FsNode {
        name: source_root.name.clone(),
        path: source_root.path.clone(),
        kind: FileKind::Directory,
        loaded: true,
        loading: false,
        error: None,
        children: Vec::new(),
    };

    let root_path = Path::new(source_root.path.as_ref());
    for changed_path in &snapshot.changed_paths {
        let path = Path::new(changed_path.as_ref());
        if !path.starts_with(root_path) || hidden_under_root(path, root_path, show_hidden) {
            continue;
        }

        let Ok(relative) = path.strip_prefix(root_path) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }

        let kind = snapshot.kinds.get(changed_path.as_ref()).copied();
        insert_changed_path(
            &mut root,
            root_path,
            relative,
            kind,
            snapshot.virtual_changes,
        );
    }

    sort_virtual_tree(&mut root);
    root
}

fn insert_changed_path(
    root: &mut FsNode,
    root_path: &Path,
    relative: &Path,
    kind: Option<FileKind>,
    virtual_changes: bool,
) {
    let mut node = root;
    let mut full_path = PathBuf::from(root_path);
    let component_count = relative.components().count();

    for (index, component) in relative.components().enumerate() {
        let name = component.as_os_str().to_string_lossy().to_string();
        full_path.push(component.as_os_str());
        let is_leaf = index + 1 == component_count;
        let path = Arc::<str>::from(full_path.to_string_lossy().as_ref());

        let existing_index = node.children.iter().position(|child| child.path == path);
        let child_index = if let Some(existing_index) = existing_index {
            existing_index
        } else {
            node.children.push(FsNode {
                name: Arc::from(name),
                path: path.clone(),
                kind: if is_leaf {
                    kind.unwrap_or_else(|| {
                        if virtual_changes {
                            FileKind::File
                        } else {
                            file_kind_for_changed_leaf(&full_path)
                        }
                    })
                } else {
                    FileKind::Directory
                },
                loaded: true,
                loading: false,
                error: None,
                children: Vec::new(),
            });
            node.children.len() - 1
        };

        node = &mut node.children[child_index];
    }
}

fn sort_virtual_tree(node: &mut FsNode) {
    node.children.sort_by(|left, right| {
        let left_dir = left.kind == FileKind::Directory;
        let right_dir = right.kind == FileKind::Directory;
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.name.cmp(&right.name))
    });
    for child in &mut node.children {
        sort_virtual_tree(child);
    }
}

fn hidden_under_root(path: &Path, root_path: &Path, show_hidden: bool) -> bool {
    if show_hidden {
        return false;
    }

    path.strip_prefix(root_path)
        .ok()
        .into_iter()
        .flat_map(Path::components)
        .any(|component| {
            component
                .as_os_str()
                .to_str()
                .is_some_and(|name| name.starts_with('.'))
        })
}

#[cfg(not(target_arch = "wasm32"))]
fn file_kind_for_changed_leaf(path: &Path) -> FileKind {
    std::fs::symlink_metadata(path)
        .map(|metadata| FileKind::from_file_type(&metadata.file_type()))
        .unwrap_or(FileKind::File)
}

#[cfg(target_arch = "wasm32")]
fn file_kind_for_changed_leaf(_path: &Path) -> FileKind {
    FileKind::File
}

fn changed_only_search_candidates(
    props: &FileTreeProps,
    state: &FileTreeState,
) -> Option<Vec<ExplorerCandidate>> {
    if props.change_view != FileTreeChangeView::ChangedOnly {
        return None;
    }

    let snapshot = effective_change_snapshot(props, state);
    let root = build_changed_only_root(&state.root, &snapshot, props.show_hidden);
    let mut candidates = Vec::new();
    collect_virtual_candidates(&root, true, &mut candidates);
    Some(candidates)
}

fn collect_virtual_candidates(
    node: &FsNode,
    is_root: bool,
    candidates: &mut Vec<ExplorerCandidate>,
) {
    if !is_root {
        candidates.push(ExplorerCandidate {
            path: node.path.clone(),
            label: node.name.clone(),
            is_dir: node.kind == FileKind::Directory,
        });
    }

    for child in &node.children {
        collect_virtual_candidates(child, false, candidates);
    }
}

fn expand_changed_only_directories(state: &mut FileTreeState, snapshot: &GitStatusSnapshot) {
    let root_path = state.root.path.clone();
    for path in &snapshot.changed_paths {
        if let Some(parent) = Path::new(path.as_ref()).parent() {
            let parent = Arc::<str>::from(parent.to_string_lossy().as_ref());
            insert_path_with_ancestors(&mut state.expanded, &parent, &root_path);
        }
    }
}

fn virtual_root_node(root: &Arc<str>) -> FsNode {
    let path = provided_root_path(root.as_ref());
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(Arc::from)
        .unwrap_or_else(|| root.clone());
    let path = Arc::<str>::from(path.to_string_lossy().as_ref());

    FsNode {
        name,
        path,
        kind: FileKind::Directory,
        loaded: true,
        loading: false,
        error: None,
        children: Vec::new(),
    }
}

fn file_tree_uses_guide_aware_nerd_arrows(props: &FileTreeProps) -> bool {
    props.show_icons
        && props.show_arrows
        && matches!(
            props.icon_style,
            FileIconStyle::NerdFont | FileIconStyle::NerdFontColored
        )
}

fn file_tree_needs_nerd_arrow_placeholder(
    node: &FsNode,
    is_root: bool,
    props: &FileTreeProps,
) -> bool {
    file_tree_uses_guide_aware_nerd_arrows(props)
        && !is_root
        && (node.kind != FileKind::Directory || directory_icon_is_overridden(&node.path, props))
}

fn directory_icon_is_overridden(path: &str, props: &FileTreeProps) -> bool {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| props.icon_overrides.contains_key(name))
}

fn highlight_label_spans(
    text: &str,
    hits: Option<&[u32]>,
    base_style: Style,
    selection_style: Style,
) -> Vec<crate::style::Span> {
    use crate::style::Span;

    if text.is_empty() {
        return Vec::new();
    }

    let Some(hits) = hits else {
        return vec![Span::new(text).style(base_style)];
    };

    if hits.is_empty() {
        return vec![Span::new(text).style(base_style)];
    }

    let mut sorted_hits = hits.to_vec();
    sorted_hits.sort_unstable();
    sorted_hits.dedup();

    let mut spans = Vec::new();
    let mut current = String::new();
    let mut current_highlight = false;
    let mut hit_idx = 0usize;

    for (idx, ch) in text.chars().enumerate() {
        while hit_idx < sorted_hits.len() && (sorted_hits[hit_idx] as usize) < idx {
            hit_idx += 1;
        }
        let hit = hit_idx < sorted_hits.len() && (sorted_hits[hit_idx] as usize) == idx;

        if hit != current_highlight && !current.is_empty() {
            let style = if current_highlight {
                base_style.patch(selection_style)
            } else {
                base_style
            };
            spans.push(Span::new(std::mem::take(&mut current)).style(style));
        }

        current_highlight = hit;
        current.push(ch);
    }

    if !current.is_empty() {
        let style = if current_highlight {
            base_style.patch(selection_style)
        } else {
            base_style
        };
        spans.push(Span::new(current).style(style));
    }

    spans
}

fn node_by_path_mut<'a>(root: &'a mut FsNode, path: &str) -> Option<&'a mut FsNode> {
    if root.path.as_ref() == path {
        return Some(root);
    }

    for child in &mut root.children {
        if let Some(found) = node_by_path_mut(child, path) {
            return Some(found);
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Color;

    fn test_root() -> FsNode {
        FsNode {
            name: Arc::from("repo"),
            path: Arc::from("/repo"),
            kind: FileKind::Directory,
            loaded: true,
            loading: false,
            error: None,
            children: vec![FsNode {
                name: Arc::from("src"),
                path: Arc::from("/repo/src"),
                kind: FileKind::Directory,
                loaded: true,
                loading: false,
                error: None,
                children: vec![FsNode {
                    name: Arc::from("main.rs"),
                    path: Arc::from("/repo/src/main.rs"),
                    kind: FileKind::File,
                    loaded: true,
                    loading: false,
                    error: None,
                    children: Vec::new(),
                }],
            }],
        }
    }

    fn test_state_with_root(root: FsNode) -> FileTreeState {
        let mut expanded = HashSet::new();
        expanded.insert(Arc::from("/repo"));
        expanded.insert(Arc::from("/repo/src"));
        FileTreeState {
            root,
            root_virtual: false,
            expanded,
            git_snapshot: GitStatusSnapshot::default(),
            last_git_refresh_nonce: 0,
            changed_only_auto_expand_signature: 0,
            explorer_input: TextInput::new(""),
            explorer_query_id: 0,
            explorer_filter: ExplorerFilter::default(),
            search_expanded_snapshot: None,
            search_found_dir: None,
        }
    }

    fn unique_component_test_dir(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "tui-lipan-file-tree-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn insert_path_with_ancestors_adds_chain_up_to_root() {
        let root_path: Arc<str> = "/repo".into();
        let nested: Arc<str> = "/repo/src/widgets".into();
        let mut expanded = HashSet::new();

        insert_path_with_ancestors(&mut expanded, &nested, &root_path);

        assert!(expanded.contains("/repo"));
        assert!(expanded.contains("/repo/src"));
        assert!(expanded.contains("/repo/src/widgets"));
    }

    #[test]
    fn highlight_label_spans_marks_requested_indices() {
        let base = Style::new().fg(Color::Red);
        let highlight = Style::new().fg(Color::Cyan).underline();

        let spans = highlight_label_spans("hello", Some(&[1, 2]), base, highlight);

        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0].content.as_ref(), "h");
        assert_eq!(spans[1].content.as_ref(), "el");
        assert_eq!(spans[2].content.as_ref(), "lo");

        assert_eq!(spans[0].style, base);
        assert_eq!(spans[1].style, base.patch(highlight));
        assert_eq!(spans[2].style, base);
    }

    #[test]
    fn git_status_description_has_no_trailing_padding() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .git_icon_style(crate::widgets::file_tree::git::GitIconStyle::Text)
            .props;
        let decoration = crate::widgets::file_tree::git::GitFileDecorations::from_status(
            crate::widgets::file_tree::git::GitFileStatus::new(
                None,
                Some(crate::widgets::file_tree::git::GitChangeState::Modified),
            ),
            true,
        );

        let spans = git_description_spans(Some(decoration), &props, false, false);
        let content: Vec<&str> = spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec![" ", "M"]);
    }

    #[test]
    fn git_diff_description_has_no_trailing_padding() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .git_icon_style(crate::widgets::file_tree::git::GitIconStyle::Text)
            .show_diff_stats(true)
            .props;
        let decoration = crate::widgets::file_tree::git::GitFileDecorations::from_diff_stat(
            crate::widgets::file_tree::git::GitDiffStat::new(2, 1),
            true,
        );

        let spans = git_description_spans(Some(decoration), &props, false, false);
        let content: Vec<&str> = spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec![" ", "+2", " ", "-1"]);
    }

    #[test]
    fn git_status_and_diff_description_keep_internal_separator_only() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .git_icon_style(crate::widgets::file_tree::git::GitIconStyle::Text)
            .show_diff_stats(true)
            .props;
        let decoration = crate::widgets::file_tree::git::GitFileDecorations {
            status: crate::widgets::file_tree::git::GitFileStatus::new(
                None,
                Some(crate::widgets::file_tree::git::GitChangeState::Modified),
            ),
            diff_stat: Some(crate::widgets::file_tree::git::GitDiffStat::new(2, 1)),
            direct: true,
        };

        let spans = git_description_spans(Some(decoration), &props, false, false);
        let content: Vec<&str> = spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec![" ", "M", " ", "+2", " ", "-1"]);
    }

    #[test]
    fn collapsed_dirty_directory_shows_git_marker() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .git_icon_style(crate::widgets::file_tree::git::GitIconStyle::Text)
            .props;
        let decoration = crate::widgets::file_tree::git::GitFileDecorations::from_status(
            crate::widgets::file_tree::git::GitFileStatus::new(
                None,
                Some(crate::widgets::file_tree::git::GitChangeState::Modified),
            ),
            false,
        );

        let spans = git_description_spans(Some(decoration), &props, true, false);
        let content: Vec<&str> = spans.iter().map(|span| span.content.as_ref()).collect();

        assert_eq!(content, vec![" ", "M"]);
    }

    #[test]
    fn expanded_dirty_directory_hides_git_marker() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .git_icon_style(crate::widgets::file_tree::git::GitIconStyle::Text)
            .props;
        let decoration = crate::widgets::file_tree::git::GitFileDecorations::from_status(
            crate::widgets::file_tree::git::GitFileStatus::new(
                None,
                Some(crate::widgets::file_tree::git::GitChangeState::Modified),
            ),
            false,
        );

        let spans = git_description_spans(Some(decoration), &props, true, true);

        assert!(spans.is_empty());
    }

    #[test]
    fn changed_label_styling_is_disabled_by_default() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .props;
        let node = FsNode {
            name: Arc::from("main.rs"),
            path: Arc::from("/repo/src/main.rs"),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let mut decorations = HashMap::new();
        decorations.insert(
            Arc::from("/repo/src/main.rs"),
            crate::widgets::file_tree::git::GitFileDecorations::from_status(
                crate::widgets::file_tree::git::GitFileStatus::new(
                    None,
                    Some(crate::widgets::file_tree::git::GitChangeState::Modified),
                ),
                true,
            ),
        );
        let expanded = HashSet::new();
        let mut lookup = HashMap::new();
        let path_styles = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, Vec::new(), &mut ctx);

        assert_eq!(tree.item.spans[0].content.as_ref(), "main.rs");
        assert_eq!(tree.item.spans[0].style, Style::default());
        assert_eq!(
            tree.item.description_spans[1].style,
            props.git_style_modified
        );
    }

    #[test]
    fn nerd_font_file_icon_uses_tree_guide_placeholder_instead_of_raw_spaces() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .icon_style(FileIconStyle::NerdFont)
            .indent_style(crate::widgets::IndentStyle::Long)
            .props;
        let node = FsNode {
            name: Arc::from("main.rs"),
            path: Arc::from("/repo/src/main.rs"),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let expanded = HashSet::new();
        let decorations = HashMap::new();
        let mut lookup = HashMap::new();
        let path_styles = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, vec![0], &mut ctx);

        assert_eq!(tree.leading_guide_fill_cells, 2);
        assert!(!tree.item.spans[0].content.starts_with(' '));
        assert_eq!(tree.item.spans[1].content.as_ref(), " ");
        assert_eq!(tree.item.spans[2].content.as_ref(), "main.rs");
    }

    #[test]
    fn nerd_font_symlink_and_other_icons_reserve_arrow_gutter() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .icon_style(FileIconStyle::NerdFontColored)
            .props;
        let expanded = HashSet::new();
        let decorations = HashMap::new();
        let mut lookup = HashMap::new();
        let path_styles = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };
        let symlink = FsNode {
            name: Arc::from("link"),
            path: Arc::from("/repo/link"),
            kind: FileKind::Symlink,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let other = FsNode {
            name: Arc::from("socket"),
            path: Arc::from("/repo/socket"),
            kind: FileKind::Other,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };

        let symlink_tree = build_projected_tree_node(&symlink, false, vec![0], &mut ctx);
        let other_tree = build_projected_tree_node(&other, false, vec![1], &mut ctx);

        assert_eq!(symlink_tree.leading_guide_fill_cells, 2);
        assert_eq!(other_tree.leading_guide_fill_cells, 2);
        assert!(!symlink_tree.item.spans[0].content.starts_with(' '));
        assert!(!other_tree.item.spans[0].content.starts_with(' '));
    }

    #[test]
    fn nerd_font_directory_icon_override_reserves_arrow_gutter() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .icon_style(FileIconStyle::NerdFont)
            .indent_style(crate::widgets::IndentStyle::Long)
            .icon_override("src", "★", None)
            .props;
        let node = FsNode {
            name: Arc::from("src"),
            path: Arc::from("/repo/src"),
            kind: FileKind::Directory,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let expanded = HashSet::new();
        let decorations = HashMap::new();
        let mut lookup = HashMap::new();
        let path_styles = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, vec![0], &mut ctx);

        assert_eq!(tree.leading_guide_fill_cells, 2);
        assert_eq!(tree.item.spans[0].content.as_ref(), "★");
        assert_eq!(tree.item.spans[1].content.as_ref(), " ");
        assert_eq!(tree.item.spans[2].content.as_ref(), "src");
    }

    #[test]
    fn changed_label_styling_can_be_enabled() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .highlight_changed_labels(true)
            .props;
        let node = FsNode {
            name: Arc::from("main.rs"),
            path: Arc::from("/repo/src/main.rs"),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let mut decorations = HashMap::new();
        decorations.insert(
            Arc::from("/repo/src/main.rs"),
            crate::widgets::file_tree::git::GitFileDecorations::from_status(
                crate::widgets::file_tree::git::GitFileStatus::new(
                    None,
                    Some(crate::widgets::file_tree::git::GitChangeState::Modified),
                ),
                true,
            ),
        );
        let expanded = HashSet::new();
        let mut lookup = HashMap::new();
        let path_styles = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, Vec::new(), &mut ctx);

        assert_eq!(tree.item.spans[0].content.as_ref(), "main.rs");
        assert_eq!(tree.item.spans[0].style, props.git_style_modified);
    }

    #[test]
    fn directory_and_file_label_styles_are_applied_separately() {
        let directory_style = Style::new().fg(Color::Blue).bold();
        let file_style = Style::new().fg(Color::Green);
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .directory_label_style(directory_style)
            .file_label_style(file_style)
            .props;
        let expanded = HashSet::new();
        let decorations = HashMap::new();
        let mut lookup = HashMap::new();
        let path_styles = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };
        let directory = FsNode {
            name: Arc::from("src"),
            path: Arc::from("/repo/src"),
            kind: FileKind::Directory,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let file = FsNode {
            name: Arc::from("main.rs"),
            path: Arc::from("/repo/src/main.rs"),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };

        let directory_tree = build_projected_tree_node(&directory, false, vec![0], &mut ctx);
        let file_tree = build_projected_tree_node(&file, false, vec![1], &mut ctx);

        assert_eq!(directory_tree.item.spans[0].content.as_ref(), "src");
        assert_eq!(directory_tree.item.spans[0].style, directory_style);
        assert_eq!(file_tree.item.spans[0].content.as_ref(), "main.rs");
        assert_eq!(file_tree.item.spans[0].style, file_style);
    }

    #[test]
    fn changed_label_style_patches_over_file_label_style_when_enabled() {
        let file_style = Style::new().fg(Color::Green).italic();
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .file_label_style(file_style)
            .highlight_changed_labels(true)
            .props;
        let node = FsNode {
            name: Arc::from("main.rs"),
            path: Arc::from("/repo/src/main.rs"),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let mut decorations = HashMap::new();
        decorations.insert(
            Arc::from("/repo/src/main.rs"),
            crate::widgets::file_tree::git::GitFileDecorations::from_status(
                crate::widgets::file_tree::git::GitFileStatus::new(
                    None,
                    Some(crate::widgets::file_tree::git::GitChangeState::Modified),
                ),
                true,
            ),
        );
        let expanded = HashSet::new();
        let mut lookup = HashMap::new();
        let path_styles = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, Vec::new(), &mut ctx);

        assert_eq!(
            tree.item.spans[0].style,
            file_style.patch(props.git_style_modified)
        );
    }

    #[test]
    fn path_label_style_affects_label_not_icon_or_suffix() {
        let label_style = Style::new().fg(Color::Green).bold();
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .path_style("src/main.rs", FileTreeItemStyle::new().label(label_style))
            .git_icon_style(crate::widgets::file_tree::git::GitIconStyle::Text)
            .props;
        let node = FsNode {
            name: Arc::from("main.rs"),
            path: Arc::from("/repo/src/main.rs"),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let mut decorations = HashMap::new();
        decorations.insert(
            Arc::from("/repo/src/main.rs"),
            crate::widgets::file_tree::git::GitFileDecorations::from_status(
                crate::widgets::file_tree::git::GitFileStatus::new(
                    None,
                    Some(crate::widgets::file_tree::git::GitChangeState::Modified),
                ),
                true,
            ),
        );
        let path_styles = resolved_path_styles(&props.path_styles, "/repo");
        let expanded = HashSet::new();
        let mut lookup = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, Vec::new(), &mut ctx);

        assert_eq!(tree.item.spans[0].content.as_ref(), "[F]");
        assert_eq!(tree.item.spans[0].style, Style::default());
        assert_eq!(tree.item.spans[2].content.as_ref(), "main.rs");
        assert_eq!(tree.item.spans[2].style, label_style);
        assert_eq!(
            tree.item.description_spans[1].style,
            props.git_style_modified
        );
    }

    #[test]
    fn suffix_only_path_style_patches_only_description_spans() {
        let suffix_style = Style::new().dim();
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .path_style("src/main.rs", FileTreeItemStyle::new().suffix(suffix_style))
            .git_icon_style(crate::widgets::file_tree::git::GitIconStyle::Text)
            .show_diff_stats(true)
            .props;
        let node = FsNode {
            name: Arc::from("main.rs"),
            path: Arc::from("/repo/src/main.rs"),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let mut decorations = HashMap::new();
        decorations.insert(
            Arc::from("/repo/src/main.rs"),
            crate::widgets::file_tree::git::GitFileDecorations {
                status: crate::widgets::file_tree::git::GitFileStatus::new(
                    None,
                    Some(crate::widgets::file_tree::git::GitChangeState::Modified),
                ),
                diff_stat: Some(crate::widgets::file_tree::git::GitDiffStat::new(30, 21)),
                direct: true,
            },
        );
        let path_styles = resolved_path_styles(&props.path_styles, "/repo");
        let expanded = HashSet::new();
        let mut lookup = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, Vec::new(), &mut ctx);

        assert_eq!(tree.item.spans[0].style, Style::default());
        assert_eq!(tree.item.spans[2].style, Style::default());
        assert!(
            tree.item
                .description_spans
                .iter()
                .all(|span| span.style.dim == Some(true))
        );
        assert_eq!(
            tree.item.description_spans[1].style.fg,
            props.git_style_modified.fg
        );
        assert_eq!(
            tree.item.description_spans[3].style.fg,
            props.git_style_added.fg
        );
        assert_eq!(
            tree.item.description_spans[5].style.fg,
            props.git_style_deleted.fg
        );
    }

    #[test]
    fn global_suffix_style_patches_git_markers_and_diff_stats() {
        let suffix_style = Style::new().dim();
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .git_icon_style(crate::widgets::file_tree::git::GitIconStyle::Text)
            .show_diff_stats(true)
            .change_suffix_style(suffix_style)
            .props;
        let decoration = crate::widgets::file_tree::git::GitFileDecorations {
            status: crate::widgets::file_tree::git::GitFileStatus::new(
                None,
                Some(crate::widgets::file_tree::git::GitChangeState::Modified),
            ),
            diff_stat: Some(crate::widgets::file_tree::git::GitDiffStat::new(30, 21)),
            direct: true,
        };
        let mut spans = git_description_spans(Some(decoration), &props, false, false);

        apply_suffix_style(&mut spans, props.change_suffix_style, None);

        assert_eq!(
            spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<Vec<_>>(),
            vec![" ", "M", " ", "+30", " ", "-21"]
        );
        assert!(spans.iter().all(|span| span.style.dim == Some(true)));
        assert_eq!(spans[1].style.fg, props.git_style_modified.fg);
        assert_eq!(spans[3].style.fg, props.git_style_added.fg);
        assert_eq!(spans[5].style.fg, props.git_style_deleted.fg);
    }

    #[test]
    fn suffix_priority_defaults_to_label_priority() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .props;
        let node = FsNode {
            name: Arc::from("main.rs"),
            path: Arc::from("/repo/src/main.rs"),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let expanded = HashSet::new();
        let decorations = HashMap::new();
        let path_styles = HashMap::new();
        let mut lookup = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, Vec::new(), &mut ctx);

        assert!(tree.item.primary_truncate_description_first);
    }

    #[test]
    fn suffix_priority_can_preserve_suffix_over_label() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .change_suffix_priority(FileTreeSuffixPriority::Suffix)
            .props;
        let node = FsNode {
            name: Arc::from("main.rs"),
            path: Arc::from("/repo/src/main.rs"),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let expanded = HashSet::new();
        let decorations = HashMap::new();
        let path_styles = HashMap::new();
        let mut lookup = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, Vec::new(), &mut ctx);

        assert!(!tree.item.primary_truncate_description_first);
    }

    #[test]
    fn path_styles_resolve_relative_and_absolute_under_effective_root() {
        let item_style = FileTreeItemStyle::new().row(Style::new().bg(Color::Blue));
        let styles = HashMap::from([
            (Arc::<str>::from("src/main.rs"), item_style),
            (
                Arc::<str>::from("/repo/src/lib.rs"),
                item_style.label(Style::new().fg(Color::Green)),
            ),
            (
                Arc::<str>::from("../escape.rs"),
                item_style.icon(Style::new().fg(Color::Red)),
            ),
        ]);

        let resolved = resolved_path_styles(&styles, "/repo");

        assert!(resolved.contains_key("/repo/src/main.rs"));
        assert!(resolved.contains_key("/repo/src/lib.rs"));
        assert!(!resolved.contains_key("/escape.rs"));
    }

    #[test]
    fn changed_only_root_groups_changed_paths_by_directory() {
        let source_root = FsNode {
            name: Arc::from("repo"),
            path: Arc::from("/repo"),
            kind: FileKind::Directory,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let snapshot = GitStatusSnapshot {
            entries: HashMap::new(),
            changed_paths: vec![
                Arc::from("/repo/src/lib.rs"),
                Arc::from("/repo/src/widgets/file_tree.rs"),
                Arc::from("/elsewhere/ignored.rs"),
            ],
            kinds: HashMap::new(),
            virtual_changes: false,
        };

        let root = build_changed_only_root(&source_root, &snapshot, true);

        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].name.as_ref(), "src");
        assert_eq!(root.children[0].kind, FileKind::Directory);
        assert_eq!(root.children[0].children[0].name.as_ref(), "widgets");
        assert_eq!(root.children[0].children[1].name.as_ref(), "lib.rs");
        assert_eq!(root.children[0].children[1].kind, FileKind::File);
        assert_eq!(
            root.children[0].children[0].children[0].path.as_ref(),
            "/repo/src/widgets/file_tree.rs"
        );
    }

    #[test]
    fn changed_only_root_respects_hidden_components() {
        let source_root = FsNode {
            name: Arc::from("repo"),
            path: Arc::from("/repo"),
            kind: FileKind::Directory,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let snapshot = GitStatusSnapshot {
            entries: HashMap::new(),
            changed_paths: vec![Arc::from("/repo/.env"), Arc::from("/repo/src/main.rs")],
            kinds: HashMap::new(),
            virtual_changes: false,
        };

        let root = build_changed_only_root(&source_root, &snapshot, false);

        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].name.as_ref(), "src");
    }

    #[test]
    fn changed_only_projection_lookup_uses_displayed_paths() {
        let mut props = crate::widgets::file_tree::FileTree::new("/repo")
            .git_changed_only(true)
            .show_icons(false)
            .props;
        props.git_status = false;

        let mut expanded = HashSet::new();
        expanded.insert(Arc::from("/repo"));
        expanded.insert(Arc::from("/repo/src"));
        let state = FileTreeState {
            root: FsNode {
                name: Arc::from("repo"),
                path: Arc::from("/repo"),
                kind: FileKind::Directory,
                loaded: true,
                loading: false,
                error: None,
                children: Vec::new(),
            },
            root_virtual: false,
            expanded,
            git_snapshot: GitStatusSnapshot {
                entries: HashMap::new(),
                changed_paths: vec![Arc::from("/repo/src/main.rs")],
                kinds: HashMap::new(),
                virtual_changes: false,
            },
            last_git_refresh_nonce: 0,
            changed_only_auto_expand_signature: 0,
            explorer_input: TextInput::new(""),
            explorer_query_id: 0,
            explorer_filter: ExplorerFilter::default(),
            search_expanded_snapshot: None,
            search_found_dir: None,
        };

        let projection = build_projection(&props, &state, None);

        assert_eq!(projection.root.children.len(), 1);
        assert_eq!(projection.root.children[0].children.len(), 1);
        assert_eq!(
            projection
                .lookup
                .get(&crate::widgets::TreePath::from(vec![0, 0]))
                .map(|entry| entry.path.as_ref()),
            Some("/repo/src/main.rs")
        );
    }

    #[test]
    fn selected_path_maps_to_visible_row_index() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .selected_path("src/main.rs")
            .props;
        let state = test_state_with_root(test_root());

        let projection = build_projection(&props, &state, None);

        assert_eq!(
            selected_visible_index_by_path(&props, &state, &projection),
            Some(2)
        );
    }

    #[test]
    fn selected_path_hidden_by_projection_is_noop() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .selected_path("src/main.rs")
            .props;
        let mut state = test_state_with_root(test_root());
        state.expanded.remove("/repo/src");

        let projection = build_projection(&props, &state, None);

        assert_eq!(
            selected_visible_index_by_path(&props, &state, &projection),
            None
        );
    }

    #[test]
    fn reveal_path_expands_ancestors_and_loads_when_possible() {
        let root_path = unique_component_test_dir("reveal_path_expands_ancestors");
        std::fs::create_dir_all(root_path.join("src").join("nested")).unwrap();
        std::fs::write(root_path.join("src").join("nested").join("main.rs"), "").unwrap();
        let target = root_path.join("src").join("nested").join("main.rs");
        let props = crate::widgets::file_tree::FileTree::new(root_path.to_string_lossy().as_ref())
            .reveal_path(target.to_string_lossy().into_owned())
            .props;
        let mut state = FileTreeComponent::new().create_state(&props);

        let changed = apply_reveal_paths_to_state(&mut state, &props);

        let root = state.root.path.clone();
        let src = resolve_path_under_root(&root, "src").unwrap();
        let nested = resolve_path_under_root(&root, "src/nested").unwrap();
        assert!(changed || state.expanded.contains(src.as_ref()));
        assert!(state.expanded.contains(src.as_ref()));
        assert!(state.expanded.contains(nested.as_ref()));
        assert!(node_by_path_mut(&mut state.root, nested.as_ref()).is_some_and(|node| node.loaded));

        let _ = std::fs::remove_dir_all(root_path);
    }

    #[test]
    fn select_path_reveals_and_maps_changed_only_provided_rows() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .change_source(crate::widgets::file_tree::FileTreeChangeSource::Provided(
                vec![crate::widgets::file_tree::FileTreeChange::new(
                    "src/main.rs",
                    crate::widgets::file_tree::FileTreeChangeStatus::Modified,
                )],
            ))
            .change_view(crate::widgets::file_tree::FileTreeChangeView::ChangedOnly)
            .show_icons(false)
            .select_path("src/main.rs")
            .props;
        let state = FileTreeComponent::new().create_state(&props);
        let projection = build_projection(&props, &state, None);

        assert_eq!(
            selected_visible_index_by_path(&props, &state, &projection),
            Some(2)
        );
    }

    #[test]
    fn provided_changed_only_uses_virtual_root_for_nonexistent_path() {
        let props = crate::widgets::file_tree::FileTree::new("/definitely/missing/repo")
            .change_source(crate::widgets::file_tree::FileTreeChangeSource::Provided(
                vec![crate::widgets::file_tree::FileTreeChange::new(
                    "src/main.rs",
                    crate::widgets::file_tree::FileTreeChangeStatus::Modified,
                )],
            ))
            .change_view(crate::widgets::file_tree::FileTreeChangeView::ChangedOnly)
            .props;

        let state = FileTreeComponent::new().create_state(&props);
        let root = build_changed_only_root(&state.root, &state.git_snapshot, true);

        assert_eq!(state.root.kind, FileKind::Directory);
        assert!(state.root.error.is_none());
        assert_eq!(
            root.children[0].path.as_ref(),
            "/definitely/missing/repo/src"
        );
        assert_eq!(
            root.children[0].children[0].path.as_ref(),
            "/definitely/missing/repo/src/main.rs"
        );
        assert_eq!(root.children[0].children[0].kind, FileKind::File);
    }

    #[test]
    fn root_mode_rebuild_loads_filesystem_after_leaving_provided_changed_only() {
        let root_path = std::env::temp_dir().join(format!(
            "tui-lipan-file-tree-rebuild-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root_path).unwrap();
        std::fs::write(root_path.join("real.rs"), "").unwrap();
        let root = root_path.to_string_lossy().into_owned();

        let provided_props = crate::widgets::file_tree::FileTree::new(root.clone())
            .change_source(crate::widgets::file_tree::FileTreeChangeSource::Provided(
                vec![crate::widgets::file_tree::FileTreeChange::new(
                    "virtual.rs",
                    crate::widgets::file_tree::FileTreeChangeStatus::Modified,
                )],
            ))
            .change_view(crate::widgets::file_tree::FileTreeChangeView::ChangedOnly)
            .props;
        let mut state = FileTreeComponent::new().create_state(&provided_props);

        assert!(state.root_virtual);
        assert!(state.root.children.is_empty());

        let all_files_props = crate::widgets::file_tree::FileTree::new(root)
            .change_source(crate::widgets::file_tree::FileTreeChangeSource::Provided(
                Vec::new(),
            ))
            .change_view(crate::widgets::file_tree::FileTreeChangeView::AllFiles)
            .props;
        rebuild_root_for_props(&mut state, &all_files_props);

        assert!(!state.root_virtual);
        assert!(
            state
                .root
                .children
                .iter()
                .any(|child| child.name.as_ref() == "real.rs")
        );

        let _ = std::fs::remove_dir_all(root_path);
    }

    #[test]
    fn provided_snapshot_maps_kind_status_and_diff_stats() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .change_source(crate::widgets::file_tree::FileTreeChangeSource::Provided(
                vec![
                    crate::widgets::file_tree::FileTreeChange::new(
                        "src/generated",
                        crate::widgets::file_tree::FileTreeChangeStatus::Added,
                    )
                    .kind(FileKind::Directory)
                    .diff_stat(3, 1)
                    .staged(true),
                ],
            ))
            .change_view(crate::widgets::file_tree::FileTreeChangeView::ChangedOnly)
            .show_diff_stats(true)
            .props;

        let state = FileTreeComponent::new().create_state(&props);
        let decoration = state
            .git_snapshot
            .entries
            .get("/repo/src/generated")
            .copied();

        assert!(state.git_snapshot.virtual_changes);
        assert_eq!(
            state.git_snapshot.kinds.get("/repo/src/generated"),
            Some(&FileKind::Directory)
        );
        assert_eq!(
            decoration.and_then(|decoration| decoration.status.staged),
            Some(crate::widgets::file_tree::git::GitChangeState::Added)
        );
        assert_eq!(
            decoration.and_then(|decoration| decoration.diff_stat),
            Some(crate::widgets::file_tree::git::GitDiffStat::new(3, 1))
        );
    }

    #[test]
    fn provided_source_does_not_request_git_snapshot() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .change_source(crate::widgets::file_tree::FileTreeChangeSource::Provided(
                vec![crate::widgets::file_tree::FileTreeChange::new(
                    "src/main.rs",
                    crate::widgets::file_tree::FileTreeChangeStatus::Modified,
                )],
            ))
            .change_view(crate::widgets::file_tree::FileTreeChangeView::ChangedOnly)
            .show_diff_stats(true)
            .props;

        assert!(!needs_git_snapshot(&props));
    }
}
