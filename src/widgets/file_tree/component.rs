use std::borrow::Cow;
use std::collections::{HashMap, HashSet, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use std::path::{Component as PathComponent, Path, PathBuf};
use std::sync::Arc;

use crate::core::component::{Command, Component, Context, TaskPolicy, Update};
use crate::core::element::{Element, IntoElement};
use crate::style::{Style, StyleSlot};
use crate::text::input::TextInput;
use crate::widgets::{Divider, Input, InputEvent, ListItem, MouseRegion, Tree, TreeNode, VStack};

use super::events::{
    FileTreeEntryRequest, FileTreeEvent, FileTreeExplorerFocusOrigin, FileTreeToggleEvent,
};
use super::explorer::{
    ExplorerCandidate, ExplorerFilter, ExplorerIndex, ExplorerIndexCache, ExplorerIndexKey,
};
use super::fs::{DirectoryLoadResult, FileKind, FsNode, read_directory, root_node};
use super::git::{
    GitFileDecorations, GitStatusCacheKey, GitStatusCacheRequest, GitStatusSnapshot,
    discover_git_root, insert_provided_decoration_path_and_parents, load_git_snapshot,
    provided_change_snapshot,
};
use super::mod_private::FileTreeProps;
use super::provided_path::ProvidedPath;
use super::watch::{
    self, RepositoryWatch, WatchSession, WatchState, plan_flush, visible_directories,
};
use super::{
    FileTreeChangeSource, FileTreeChangeView, FileTreeDirectoryListing, FileTreeEntrySource,
    FileTreeItemStyle, FileTreeRefreshPolicy, FileTreeSuffixPriority,
};
use crate::widgets::{TreeEvent, TreeToggleEvent};

#[derive(Clone, Debug)]
pub(crate) struct FileTreeState {
    pub(crate) root: FsNode,
    pub(crate) root_virtual: bool,
    pub(crate) expanded: HashSet<Arc<str>>,
    pub(crate) git_snapshot: GitStatusSnapshot,
    /// Whether the first `git status` for the current root is still running. Only the changed-only
    /// empty state reads it: without it, every tree would answer "nothing changed" for the frames
    /// between mounting and the snapshot landing, and then fill in.
    pub(crate) git_snapshot_pending: bool,
    pub(crate) git_load_generation: u64,
    pub(crate) last_git_refresh_nonce: u64,
    pub(crate) entry_refresh_requested_token: u64,
    pub(crate) entry_refresh_completed_token: u64,
    pub(crate) entry_refresh_requested_options: (bool, usize),
    pub(crate) entry_refresh_pending: bool,
    pub(crate) entry_refresh_generation: u64,
    pub(crate) changed_only_auto_expand_signature: u64,
    pub(crate) explorer_input: TextInput,
    pub(crate) explorer_query_id: u64,
    pub(crate) explorer_filter: ExplorerFilter,
    /// The explorer's last walk, which later keystrokes filter instead of walking again.
    pub(crate) explorer_index: Arc<ExplorerIndexCache>,
    /// Names this tree's search task, so a keystroke replaces the queued search instead of
    /// queueing another one behind it.
    pub(crate) explorer_task_key: Arc<str>,
    pub(crate) explorer_entered_from_tree: bool,
    pub(crate) explorer_focused: bool,
    pub(crate) search_expanded_snapshot: Option<HashSet<Arc<str>>>,
    pub(crate) search_found_dir: Option<Arc<str>>,
    pub(crate) requested_entry_paths: HashSet<Arc<str>>,
    pub(crate) uncontrolled_selected_path: Option<Arc<str>>,
    /// What each symlink in the tree points at, keyed by the link's own path.
    ///
    /// Kept beside the tree rather than on `FsNode` for the same reason the git decorations are:
    /// it describes a rare kind of node, and every other node would carry an empty slot.
    pub(crate) symlink_targets: HashMap<Arc<str>, SymlinkInfo>,
    /// Whether the tree itself has keyboard focus, which decides whether the cursor row shows.
    pub(crate) tree_focused: bool,
    /// Filesystem watching and periodic rereads, driven by [`FileTreeRefreshPolicy`].
    pub(crate) watch: WatchState,
}

/// Where a symlink points, and whether that is a directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct SymlinkInfo {
    /// The target as the link itself spells it, when it is known.
    pub(crate) target: Option<Arc<str>>,
    /// Whether the link resolves to a directory. Only the icon reads it: the row stays a leaf
    /// either way, because following the link would open a second copy of what it names.
    pub(crate) is_dir: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct LocalEntryRefreshResult {
    generation: u64,
    token: u64,
    root: Arc<str>,
    snapshot: FsNode,
    expanded: HashSet<Arc<str>>,
    options: (bool, usize),
}

#[derive(Clone, Debug)]
pub(crate) enum FileTreeMsg {
    TreeFocusChanged(bool),
    TreeSelected {
        entry: Option<VisibleFileTreeEntry>,
    },
    TreeActivated {
        entry: Option<VisibleFileTreeEntry>,
    },
    TreeCtrlActivated {
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
        /// Listings of the directories the matches open, read on the search's worker.
        listings: Vec<(Arc<str>, DirectoryLoadResult)>,
    },
    /// A file holding ignore rules changed, so the explorer's walk no longer matches them.
    ExplorerIgnoreRulesChanged,
    RequestEntryRefresh(u64),
    EntryRefreshLoaded(LocalEntryRefreshResult),
    RequestGitRefresh(u64),
    GitSnapshotLoaded {
        generation: u64,
        snapshot: Option<GitStatusSnapshot>,
        cache_request: Option<GitStatusCacheRequest>,
    },
    /// Start the refresh policy over: drop any watcher, and arm what the policy asks for.
    RefreshPolicyRestart,
    /// Schedule the next periodic reread of a chain.
    ArmRefreshTick {
        poll_generation: u64,
    },
    /// Periodic reread: the polling policy's interval, or a watching tree's repair pass.
    RefreshTick {
        poll_generation: u64,
    },
    WatchStarted {
        generation: u64,
        session: Arc<WatchSession>,
    },
    WatchUnavailable {
        generation: u64,
    },
    /// Resolve the tracked directories and bring the repository watch up to date.
    RepositoryWatchResolve {
        generation: u64,
    },
    RepositoryWatchUpdated {
        generation: u64,
        repository: Option<Arc<RepositoryWatch>>,
        complete: bool,
    },
    /// The watcher recorded the first change of a new batch.
    WatchDirty {
        generation: u64,
    },
    /// The batch window closed: sort what changed into rereads and a Git refresh.
    WatchFlush {
        generation: u64,
    },
    /// Reread dirty directories, unless a reread is already running.
    WatchReload {
        generation: u64,
    },
    WatchDirectoriesLoaded {
        generation: u64,
        entry_generation: u64,
        root: Arc<str>,
        options: (bool, usize),
        results: Vec<(Arc<str>, super::fs::DirectoryLoadResult)>,
    },
    WatchGitDirty {
        generation: u64,
    },
    WatchGitFlush {
        generation: u64,
    },
    SyncRootMode,
    EnsureChangedOnlyExpanded,
    EnsureRevealPaths,
    FocusExplorer {
        from_tree: bool,
    },
    FocusTree,
    TreePointerDown,
    ExplorerBlurred,
    EscapeExplorer,
}

pub(crate) struct FileTreeComponent;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct VisibleFileTreeEntry {
    path: Arc<str>,
    kind: FileKind,
}

struct ExplorerSearchCandidates {
    entries: Vec<ExplorerCandidate>,
    provided_paths: bool,
}

struct FileTreeProjection {
    root: TreeNode,
    /// Whether the tree this projection was built from had nothing to show, independent of what the
    /// projection actually rendered. Collapsing a directory removes its rows without emptying it.
    source_is_empty: bool,
    lookup: Arc<HashMap<crate::widgets::TreePath, VisibleFileTreeEntry>>,
    path_to_visible_index: HashMap<Arc<str>, usize>,
    /// Rows drawn with their hidden or ignored style, so the cursor row knows whether to lift one.
    receded_paths: HashSet<Arc<str>>,
}

struct ProjectionBuildContext<'a> {
    props: &'a FileTreeProps,
    expanded: &'a HashSet<Arc<str>>,
    explorer_filter: Option<&'a ExplorerFilter>,
    git_decorations: &'a HashMap<Arc<str>, GitFileDecorations>,
    symlink_targets: &'a HashMap<Arc<str>, SymlinkInfo>,
    ignored_paths: &'a HashSet<Arc<str>>,
    root_path: &'a str,
    highlighted_path: Option<&'a str>,
    receded_paths: &'a mut HashSet<Arc<str>>,
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
        let uncontrolled_selected_path = initial_uncontrolled_selected_path(props, &root);
        let mut state = Self::State {
            root,
            root_virtual,
            expanded,
            git_snapshot,
            git_snapshot_pending: false,
            git_load_generation: 0,
            last_git_refresh_nonce: props.git_refresh_nonce,
            entry_refresh_requested_token: props.entry_refresh_token,
            entry_refresh_completed_token: props.entry_refresh_token,
            entry_refresh_requested_options: entry_refresh_options(props),
            entry_refresh_pending: false,
            entry_refresh_generation: 0,
            changed_only_auto_expand_signature: 0,
            explorer_input: TextInput::new(""),
            explorer_query_id: 0,
            explorer_filter: ExplorerFilter::default(),
            explorer_index: Arc::default(),
            explorer_task_key: next_explorer_task_key(),
            explorer_entered_from_tree: false,
            explorer_focused: false,
            search_expanded_snapshot: None,
            search_found_dir: None,
            requested_entry_paths: HashSet::new(),
            uncontrolled_selected_path,
            symlink_targets: HashMap::new(),
            tree_focused: false,
            watch: WatchState::default(),
        };

        apply_initial_expanded_paths(&mut state, props);
        apply_reveal_paths_to_state(&mut state, props);
        sync_symlink_targets(&mut state, props);
        state
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        request_missing_provided_directories(&mut ctx.state, &ctx.props);
        if refresh_policy_active(&ctx.props) {
            ctx.link().send(FileTreeMsg::RefreshPolicyRestart);
        }

        if !needs_git_snapshot(&ctx.props) {
            return None;
        }

        let repo_root = discover_git_root(Path::new(ctx.props.root.as_ref()))
            .map(|path| Arc::<str>::from(path.to_string_lossy().as_ref()))?;

        let (cached, cache_request) = prepare_git_cache(&ctx.props, Path::new(repo_root.as_ref()));
        if let Some(snapshot) = cached.as_ref() {
            ctx.state.git_snapshot = snapshot.clone();
        }

        // A root outside any repository returns above: there is nothing to wait for, and the
        // changed-only view can say so immediately.
        ctx.state.git_snapshot_pending = cached.is_none();
        Some(git_snapshot_command(
            ctx.link().clone(),
            repo_root,
            ctx.props.git_diff_stats,
            ctx.state.git_load_generation,
            cache_request,
        ))
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        if ctx.state.root_virtual != uses_virtual_root(&ctx.props) {
            ctx.link().send(FileTreeMsg::SyncRootMode);
        }

        let refresh_options = entry_refresh_options(&ctx.props);
        if uses_local_entries(&ctx.props)
            && (ctx.props.entry_refresh_token != ctx.state.entry_refresh_requested_token
                || refresh_options != ctx.state.entry_refresh_requested_options
                || (!ctx.state.entry_refresh_pending
                    && ctx.props.entry_refresh_token != ctx.state.entry_refresh_completed_token))
        {
            ctx.link().send(FileTreeMsg::RequestEntryRefresh(
                ctx.props.entry_refresh_token,
            ));
        }

        if needs_git_snapshot(&ctx.props)
            && ctx.props.git_refresh_nonce != ctx.state.last_git_refresh_nonce
        {
            ctx.link()
                .send(FileTreeMsg::RequestGitRefresh(ctx.props.git_refresh_nonce));
        }

        let change_snapshot = effective_change_snapshot(&ctx.state);
        if should_auto_expand_changed_only(&ctx.props, &ctx.state, change_snapshot) {
            ctx.link().send(FileTreeMsg::EnsureChangedOnlyExpanded);
        }

        if has_reveal_path(&ctx.props) {
            ctx.link().send(FileTreeMsg::EnsureRevealPaths);
        }

        let active_filter = active_explorer_filter(&ctx.props, &ctx.state);

        let projection = build_projection(&ctx.props, &ctx.state, active_filter);
        let empty_changed_only =
            changed_only_projection_is_empty(&ctx.props, &ctx.state, &projection);
        let selected_by_path = selected_visible_index_by_path(&ctx.props, &ctx.state, &projection);
        let effective_selected = effective_selected_index(&ctx.props, &ctx.state, &projection);
        let select_lookup = projection.lookup.clone();
        let activate_lookup = projection.lookup.clone();
        let toggle_lookup = projection.lookup.clone();

        let has_explorer = ctx.props.explorer;

        let mut tree = Tree::new(projection.root)
            .indent(ctx.props.indent_width)
            .show_arrows(ctx.props.show_arrows)
            .expanded_arrow(ctx.props.expanded_arrow.clone())
            .collapsed_arrow(ctx.props.collapsed_arrow.clone())
            .arrow_style(ctx.props.arrow_style)
            .content_gap(ctx.props.content_gap)
            .indent_style(ctx.props.indent_style)
            .indent_guide_style(ctx.props.indent_guide_style)
            .indent_guide_start_depth(ctx.props.indent_guide_start_depth)
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
            .tab_stop(ctx.props.tab_stop)
            .focus_key(ctx.props.tree_focus_key.clone())
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
        if ctx.props.on_ctrl_activate.is_some() {
            let lookup = projection.lookup.clone();
            tree = tree.on_ctrl_activate(ctx.link().callback(move |event: TreeEvent| {
                FileTreeMsg::TreeCtrlActivated {
                    entry: lookup.get(&event.path).cloned(),
                }
            }));
        }
        if let Some(selected) = effective_selected {
            tree = tree.selected(selected);
        }
        if ctx.props.clear_selection {
            tree = tree.clear_selection(true);
        }
        if selected_by_path.is_some() && ctx.props.select_path.is_some() {
            tree = tree.force_scroll_to_selected(true);
        }

        // When explorer is enabled, "/" on the tree focuses the search input
        if has_explorer {
            tree = tree.key_interceptor(ctx.link().key_handler(|key| match key.code {
                crate::core::event::KeyCode::Char('/') => {
                    Some(FileTreeMsg::FocusExplorer { from_tree: true })
                }
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
        tree = tree
            .on_focus(
                ctx.link()
                    .callback(|()| FileTreeMsg::TreeFocusChanged(true)),
            )
            .on_blur(
                ctx.link()
                    .callback(|()| FileTreeMsg::TreeFocusChanged(false)),
            );
        if let Some(text) = ctx.props.empty_text.clone() {
            tree = tree
                .empty_text(text)
                .empty_text_style(ctx.props.empty_text_style);
        }

        // The explorer stays even with nothing to show: a query that currently matches nothing must
        // not tear its own input away mid-word.
        let body: Element = if empty_changed_only {
            changed_only_empty_state(&ctx.props)
        } else {
            tree.into()
        };

        if !has_explorer {
            return body;
        }

        let query = ctx.state.explorer_input.text().to_owned();
        let input = Input::new(query)
            .cursor(ctx.state.explorer_input.cursor())
            .anchor(ctx.state.explorer_input.anchor())
            .placeholder(ctx.props.explorer_placeholder.as_ref())
            .prefix(ctx.props.explorer_prefix.clone())
            .suffix(explorer_match_count_label(&ctx.state.explorer_filter))
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
            .on_click(
                ctx.link()
                    .callback(|_| FileTreeMsg::FocusExplorer { from_tree: false }),
            )
            .on_blur(ctx.link().callback(|_| FileTreeMsg::ExplorerBlurred))
            .tab_stop(false)
            .key_interceptor(ctx.link().key_handler(|key| match key.code {
                crate::core::event::KeyCode::Enter => Some(FileTreeMsg::FocusTree),
                crate::core::event::KeyCode::Esc => Some(FileTreeMsg::EscapeExplorer),
                _ => None,
            }));

        let mut layout = VStack::new()
            .width(ctx.props.width)
            .height(ctx.props.height)
            .child(input.key(ctx.props.explorer_focus_key.clone()));

        if ctx.props.explorer_divider {
            let divider = Divider::horizontal()
                .join_frame(ctx.props.explorer_divider_join_frame)
                .ch(ctx.props.explorer_divider_char)
                .style(ctx.props.explorer_divider_style);
            layout = layout.child(
                MouseRegion::new()
                    .on_mouse_down(ctx.link().callback(|_| FileTreeMsg::TreePointerDown))
                    .child(divider),
            );
        }

        layout
            .child(
                MouseRegion::new()
                    .on_mouse_down(ctx.link().callback(|_| FileTreeMsg::TreePointerDown))
                    .bubble_mouse_down(true)
                    .child(body),
            )
            .into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        let was_degraded = ctx.state.watch.degraded();
        let update = update_file_tree(msg, ctx);
        sync_watches(ctx, was_degraded);
        update
    }

    fn unmount(&mut self, ctx: &mut Context<Self>) {
        ctx.state.watch.reset();
    }

    fn on_props_changed(
        &mut self,
        old_props: &Self::Properties,
        ctx: &mut Context<Self>,
    ) -> Update {
        let source_changed = old_props.entry_source != ctx.props.entry_source;
        let change_source_changed = old_props.change_source != ctx.props.change_source;
        let git_cache_changed = old_props.git_status_cache != ctx.props.git_status_cache;
        let entry_mode_changed =
            uses_provided_entries(old_props) != uses_provided_entries(&ctx.props);
        let provided_options_changed = uses_provided_entries(&ctx.props)
            && (old_props.show_hidden != ctx.props.show_hidden
                || old_props.max_entries_per_dir != ctx.props.max_entries_per_dir);
        let local_options_changed = uses_local_entries(&ctx.props)
            && (old_props.show_hidden != ctx.props.show_hidden
                || old_props.max_entries_per_dir != ctx.props.max_entries_per_dir);
        let local_entry_token_changed = uses_local_entries(&ctx.props)
            && old_props.entry_refresh_token != ctx.props.entry_refresh_token;
        let root_mode_changed = old_props.root != ctx.props.root
            || entry_mode_changed
            || uses_virtual_root(old_props) != uses_virtual_root(&ctx.props);
        let should_load_git = should_load_git_after_props_change(old_props, &ctx.props)
            || (git_cache_changed && needs_git_snapshot(&ctx.props));

        if old_props.root != ctx.props.root
            || source_changed
            || change_source_changed
            || git_cache_changed
            || old_props.git_diff_stats != ctx.props.git_diff_stats
            || needs_git_snapshot(old_props) != needs_git_snapshot(&ctx.props)
        {
            ctx.state.git_load_generation = ctx.state.git_load_generation.wrapping_add(1);
        }

        if root_mode_changed {
            rebuild_root_for_props(&mut ctx.state, &ctx.props);
        } else if provided_options_changed {
            let (root, root_virtual) = initial_root(&ctx.props);
            ctx.state.root = root;
            ctx.state.root_virtual = root_virtual;
            sync_symlink_targets(&mut ctx.state, &ctx.props);
        } else if source_changed && uses_provided_entries(&ctx.props) {
            apply_changed_provided_listings(&mut ctx.state.root, old_props, &ctx.props);
            sync_symlink_targets(&mut ctx.state, &ctx.props);
        }

        if local_entry_token_changed || (source_changed && !root_mode_changed) {
            ctx.state.entry_refresh_generation = ctx.state.entry_refresh_generation.wrapping_add(1);
        }

        if !root_mode_changed
            && (source_changed || change_source_changed || provided_options_changed)
        {
            ctx.state.git_snapshot = effective_initial_snapshot(&ctx.props);
        }

        if source_changed {
            clear_completed_entry_requests(&mut ctx.state, &ctx.props);
        }

        if uses_provided_entries(&ctx.props) {
            apply_reveal_paths_to_state(&mut ctx.state, &ctx.props);
            request_missing_provided_directories(&mut ctx.state, &ctx.props);
        }

        let dirty = root_mode_changed
            || source_changed
            || change_source_changed
            || provided_options_changed
            || local_options_changed
            || local_entry_token_changed
            || old_props.expanded_paths != ctx.props.expanded_paths
            || should_load_git;
        let command = if should_load_git {
            discover_git_root(Path::new(ctx.props.root.as_ref())).map(|repo_root| {
                let (cached, cache_request) = prepare_git_cache(&ctx.props, &repo_root);
                ctx.state.git_snapshot = cached.clone().unwrap_or_default();
                ctx.state.git_snapshot_pending = cached.is_none();
                git_snapshot_command(
                    ctx.link().clone(),
                    Arc::from(repo_root.to_string_lossy().as_ref()),
                    ctx.props.git_diff_stats,
                    ctx.state.git_load_generation,
                    cache_request,
                )
            })
        } else {
            None
        };
        if should_load_git && command.is_none() {
            // A root that moved out of every repository issues no command, and is settled at once.
            ctx.state.git_snapshot_pending = false;
        }

        // A new root or entry mode needs a new watch set and new Git metadata directories.
        let was_degraded = ctx.state.watch.degraded();
        if old_props.refresh_policy != ctx.props.refresh_policy
            || root_mode_changed
            || needs_git_snapshot(old_props) != needs_git_snapshot(&ctx.props)
        {
            if refresh_policy_active(&ctx.props) {
                ctx.link().send(FileTreeMsg::RefreshPolicyRestart);
            } else {
                ctx.state.watch.reset();
            }
        }
        // Controlled `expanded_paths` can reopen a directory here, not only through a toggle.
        sync_watches(ctx, was_degraded);

        if dirty {
            Update::with_command(command)
        } else {
            Update::none()
        }
    }
}

fn update_file_tree(msg: FileTreeMsg, ctx: &mut Context<FileTreeComponent>) -> Update {
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
                    insert_path_with_ancestors_for(
                        &mut ctx.state.expanded,
                        &path,
                        &ctx.state.root.path,
                        uses_provided_paths(&ctx.props),
                    );
                }

                ctx.state.search_found_dir = None;
                forget_explorer_index(&mut ctx.state);

                let expanded = ctx.state.expanded.clone();
                load_expanded_directories(&mut ctx.state, &ctx.props, expanded);
                return Update::full();
            }

            if ctx.state.search_expanded_snapshot.is_none() {
                ctx.state.search_expanded_snapshot = Some(ctx.state.expanded.clone());
                ctx.state.search_found_dir = None;
            }

            let link = ctx.link().clone();
            Update::with_command(start_explorer_search(&ctx.props, &mut ctx.state, link))
        }
        FileTreeMsg::ExplorerResultsReady {
            query_id,
            filter,
            listings,
        } => {
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

            // Open what this query matched on top of the tree the search started from. Adding to
            // the previous query's expansion instead would keep every directory any keystroke
            // matched open, and loaded, for the rest of the search.
            if let Some(snapshot) = ctx.state.search_expanded_snapshot.as_ref() {
                ctx.state.expanded = snapshot.clone();
            }
            for path in &ctx.state.explorer_filter.expanded_paths {
                ctx.state.expanded.insert(path.clone());
            }

            let expanded = ctx.state.expanded.clone();
            apply_search_listings(
                &mut ctx.state.root,
                &mut listings.into_iter().collect(),
                &expanded,
            );
            load_expanded_directories(&mut ctx.state, &ctx.props, expanded);
            normalize_uncontrolled_selection(&ctx.props, &mut ctx.state);
            Update::full()
        }
        FileTreeMsg::ExplorerIgnoreRulesChanged => {
            forget_explorer_index(&mut ctx.state);
            let link = ctx.link().clone();
            Update::with_command(restart_explorer_search(&ctx.props, &mut ctx.state, link))
        }
        FileTreeMsg::RequestEntryRefresh(token) => {
            let options = entry_refresh_options(&ctx.props);
            if !uses_local_entries(&ctx.props) || token != ctx.props.entry_refresh_token {
                return Update::none();
            }
            if token == ctx.state.entry_refresh_requested_token
                && options == ctx.state.entry_refresh_requested_options
                && (ctx.state.entry_refresh_pending
                    || token == ctx.state.entry_refresh_completed_token)
            {
                return Update::none();
            }

            let expanded = effective_expanded_paths(&ctx.props, &ctx.state).into_owned();
            begin_local_entry_refresh(ctx, token, expanded, options)
        }
        FileTreeMsg::EntryRefreshLoaded(result) => {
            if !uses_local_entries(&ctx.props)
                || result.generation != ctx.state.entry_refresh_generation
                || result.token != ctx.state.entry_refresh_requested_token
                || result.token != ctx.props.entry_refresh_token
                || result.root != ctx.state.root.path
                || result.options != entry_refresh_options(&ctx.props)
                || !ctx.state.entry_refresh_pending
            {
                return Update::none();
            }

            let expanded = effective_expanded_paths(&ctx.props, &ctx.state).into_owned();
            if result.expanded != expanded {
                return begin_local_entry_refresh(ctx, result.token, expanded, result.options);
            }

            let link = ctx.link().clone();
            ctx.state.root = result.snapshot;
            ctx.state.entry_refresh_pending = false;
            ctx.state.entry_refresh_completed_token = result.token;
            sync_symlink_targets(&mut ctx.state, &ctx.props);
            if !ctx.props.explorer || ctx.state.explorer_input.text().trim().is_empty() {
                normalize_uncontrolled_selection(&ctx.props, &mut ctx.state);
            }
            // Changes that arrived while this reread ran may predate what it read.
            if !ctx.state.watch.dirty.is_empty() {
                ctx.link().send(FileTreeMsg::WatchReload {
                    generation: ctx.state.watch.generation,
                });
            }
            let command = restart_explorer_search(&ctx.props, &mut ctx.state, link);
            Update::with_command(command)
        }
        FileTreeMsg::TreeFocusChanged(focused) => {
            ctx.state.tree_focused = focused;
            let callback = if focused {
                ctx.props.on_focus.as_ref()
            } else {
                ctx.props.on_blur.as_ref()
            };
            if let Some(callback) = callback {
                callback.emit(());
            }
            // The cursor row shows or hides with focus, and it lifts a receded row's style.
            if recedes_rows(&ctx.props) {
                Update::full()
            } else {
                Update::none()
            }
        }
        FileTreeMsg::TreeSelected { entry } => {
            let restyle = recedes_rows(&ctx.props);
            if let Some(entry) = entry {
                if tracks_uncontrolled_selection(&ctx.props) {
                    ctx.state.uncontrolled_selected_path = Some(entry.path.clone());
                }
                if !ctx.state.explorer_input.text().trim().is_empty() {
                    ctx.state.search_found_dir = Some(selected_directory_for_restore(
                        &entry.path,
                        entry.kind,
                        &ctx.state.root.path,
                        uses_provided_paths(&ctx.props),
                    ));
                }

                if let Some(cb) = ctx.props.on_select.as_ref() {
                    cb.emit(FileTreeEvent {
                        path: entry.path,
                        kind: entry.kind,
                    });
                }
            }
            // The row the cursor left recedes again, and the one it reached lifts.
            if restyle {
                Update::full()
            } else {
                Update::none()
            }
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
        FileTreeMsg::TreeCtrlActivated { entry } => {
            if let Some(entry) = entry
                && let Some(cb) = &ctx.props.on_ctrl_activate
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

            if expanded
                && node_path.is_some()
                && (ctx.props.change_view != FileTreeChangeView::ChangedOnly
                    || uses_provided_entries(&ctx.props))
            {
                let expanded_paths = effective_expanded_paths(&ctx.props, &ctx.state).into_owned();
                load_expanded_directories(&mut ctx.state, &ctx.props, expanded_paths);
            }

            Update::full()
        }
        FileTreeMsg::RequestGitRefresh(nonce) => {
            if !needs_git_snapshot(&ctx.props) || nonce == ctx.state.last_git_refresh_nonce {
                return Update::none();
            }

            ctx.state.last_git_refresh_nonce = nonce;
            start_git_refresh(ctx)
        }
        FileTreeMsg::RefreshPolicyRestart => restart_refresh_policy(ctx),
        FileTreeMsg::ArmRefreshTick { poll_generation } => {
            let interval = refresh_interval(&ctx.props, &ctx.state.watch);
            match interval {
                Some(interval) if poll_generation == ctx.state.watch.poll_generation => {
                    Update::command_only(Command::after(
                        interval,
                        move |link: crate::callback::CommandLink<FileTreeMsg>| {
                            link.send(FileTreeMsg::RefreshTick { poll_generation });
                        },
                    ))
                }
                _ => Update::none(),
            }
        }
        FileTreeMsg::RefreshTick { poll_generation } => refresh_tick(ctx, poll_generation),
        FileTreeMsg::WatchStarted {
            generation,
            session,
        } => {
            if generation == ctx.state.watch.generation {
                ctx.state.watch.session = Some(session);
            }
            Update::none()
        }
        FileTreeMsg::WatchUnavailable { generation } => {
            if generation == ctx.state.watch.generation {
                ctx.state.watch.unavailable = true;
            }
            Update::none()
        }
        FileTreeMsg::RepositoryWatchResolve { generation } => {
            resolve_repository_watch(ctx, generation)
        }
        FileTreeMsg::RepositoryWatchUpdated {
            generation,
            repository,
            complete,
        } => {
            if generation != ctx.state.watch.generation {
                return Update::none();
            }
            let watch = &mut ctx.state.watch;
            watch.repository_resolving = false;
            watch.repository_partial = !complete;
            watch.repository = repository;
            if std::mem::take(&mut watch.repository_resolve_again) {
                ctx.link()
                    .send(FileTreeMsg::RepositoryWatchResolve { generation });
            }
            Update::none()
        }
        FileTreeMsg::WatchDirty { generation } => {
            if generation != ctx.state.watch.generation || ctx.state.watch.flush_scheduled {
                return Update::none();
            }
            ctx.state.watch.flush_scheduled = true;
            Update::command_only(Command::after(
                watch::WATCH_BATCH,
                move |link: crate::callback::CommandLink<FileTreeMsg>| {
                    link.send(FileTreeMsg::WatchFlush { generation });
                },
            ))
        }
        FileTreeMsg::WatchFlush { generation } => watch_flush(ctx, generation),
        FileTreeMsg::WatchReload { generation } => {
            if generation != ctx.state.watch.generation {
                return Update::none();
            }
            start_watch_reload(ctx).map_or_else(Update::none, Update::command_only)
        }
        FileTreeMsg::WatchDirectoriesLoaded {
            generation,
            entry_generation,
            root,
            options,
            results,
        } => apply_watch_reload(ctx, generation, entry_generation, root, options, results),
        FileTreeMsg::WatchGitDirty { generation } => {
            if generation != ctx.state.watch.generation
                || ctx.state.watch.git_scheduled
                || !needs_git_snapshot(&ctx.props)
            {
                return Update::none();
            }
            ctx.state.watch.git_scheduled = true;
            Update::command_only(Command::after(
                watch::GIT_BATCH,
                move |link: crate::callback::CommandLink<FileTreeMsg>| {
                    link.send(FileTreeMsg::WatchGitFlush { generation });
                },
            ))
        }
        FileTreeMsg::WatchGitFlush { generation } => {
            if generation != ctx.state.watch.generation {
                return Update::none();
            }
            ctx.state.watch.git_scheduled = false;
            if !needs_git_snapshot(&ctx.props) {
                return Update::none();
            }
            start_git_refresh(ctx)
        }
        FileTreeMsg::GitSnapshotLoaded {
            generation,
            snapshot,
            cache_request,
        } => {
            if generation != ctx.state.git_load_generation || !needs_git_snapshot(&ctx.props) {
                return Update::none();
            }
            ctx.state.git_snapshot_pending = false;
            if let (Some(cache), Some(request)) =
                (ctx.props.git_status_cache.as_ref(), cache_request.as_ref())
            {
                cache.finish_request(request, snapshot.as_ref());
            }
            let Some(snapshot) = snapshot else {
                return Update::none();
            };
            ctx.state.git_snapshot = snapshot;
            let snapshot = effective_change_snapshot(&ctx.state);
            if should_auto_expand_changed_only(&ctx.props, &ctx.state, snapshot) {
                let changed_paths = snapshot.changed_paths.clone();
                let provided = snapshot.virtual_changes;
                let signature = change_snapshot_signature(snapshot);
                expand_changed_only_directories(&mut ctx.state, &changed_paths, provided);
                ctx.state.changed_only_auto_expand_signature = signature;
            }
            Update::full()
        }
        FileTreeMsg::SyncRootMode => {
            if ctx.state.root_virtual == uses_virtual_root(&ctx.props) {
                return Update::none();
            }

            rebuild_root_for_props(&mut ctx.state, &ctx.props);
            Update::full()
        }
        FileTreeMsg::EnsureChangedOnlyExpanded => {
            let snapshot = effective_change_snapshot(&ctx.state);
            if should_auto_expand_changed_only(&ctx.props, &ctx.state, snapshot) {
                let changed_paths = snapshot.changed_paths.clone();
                let provided = snapshot.virtual_changes;
                let signature = change_snapshot_signature(snapshot);
                expand_changed_only_directories(&mut ctx.state, &changed_paths, provided);
                ctx.state.changed_only_auto_expand_signature = signature;
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
        FileTreeMsg::FocusExplorer { from_tree } => {
            ctx.state.explorer_entered_from_tree = from_tree;
            ctx.state.explorer_focused = true;
            ctx.request_focus(ctx.props.explorer_focus_key.clone());
            if let Some(callback) = ctx.props.on_explorer_focus.as_ref() {
                callback.emit(if from_tree {
                    FileTreeExplorerFocusOrigin::Tree
                } else {
                    FileTreeExplorerFocusOrigin::Pointer
                });
            }
            Update::none()
        }
        FileTreeMsg::FocusTree => {
            ctx.state.explorer_entered_from_tree = false;
            ctx.state.explorer_focused = false;
            ctx.request_focus(ctx.props.tree_focus_key.clone());
            Update::none()
        }
        FileTreeMsg::TreePointerDown => {
            if !ctx.state.explorer_focused {
                return Update::none();
            }

            ctx.state.explorer_entered_from_tree = false;
            ctx.state.explorer_focused = false;
            if let Some(callback) = ctx.props.on_explorer_escape.as_ref() {
                callback.emit(());
            } else {
                ctx.request_focus(ctx.props.tree_focus_key.clone());
            }
            Update::none()
        }
        FileTreeMsg::ExplorerBlurred => {
            ctx.state.explorer_focused = false;
            if let Some(callback) = ctx.props.on_explorer_blur.as_ref() {
                callback.emit(());
            }
            Update::none()
        }
        FileTreeMsg::EscapeExplorer => {
            ctx.state.explorer_focused = false;
            if ctx.state.explorer_entered_from_tree || ctx.props.on_explorer_escape.is_none() {
                ctx.state.explorer_entered_from_tree = false;
                ctx.request_focus(ctx.props.tree_focus_key.clone());
            } else if let Some(callback) = ctx.props.on_explorer_escape.as_ref() {
                callback.emit(());
            }
            Update::none()
        }
    }
}

/// Whether the changed-only view has nothing to show.
///
/// The root row is the one row a tree always has, so in a browsing tree `empty_text` would be
/// unreachable — but changed-only is not browsing. Its root is a heading over a projection rather
/// than a directory you opened, and a heading over nothing is worse than saying nothing changed.
///
/// A `git status` still running is not the same as a clean tree, so a pending snapshot holds the
/// heading rather than claiming an answer it does not have yet. Provided change data has no such
/// moment: whatever the application passed is the answer, and its own loading state is its to show.
///
/// Emptiness comes from the projection's source, not its rows: a collapsed root draws no children
/// while still having them, and treating that as clean would swap the whole tree for the
/// placeholder as soon as the user folded the heading shut.
fn changed_only_projection_is_empty(
    props: &FileTreeProps,
    state: &FileTreeState,
    projection: &FileTreeProjection,
) -> bool {
    props.change_view == FileTreeChangeView::ChangedOnly
        && props.empty_text.is_some()
        && !state.git_snapshot_pending
        && projection.source_is_empty
}

/// The empty changed-only view: the same `List` the tree renders into, with no rows, so the
/// placeholder inherits the tree's own sizing, styling, and focus target instead of collapsing the
/// surface to a bare line of text.
fn changed_only_empty_state(props: &FileTreeProps) -> Element {
    let mut list = crate::widgets::List::new()
        .width(props.width)
        .height(props.height)
        .style(props.style)
        .scrollbar(props.scrollbar)
        .scrollbar_config(props.scrollbar_config.clone())
        .focusable(props.focusable)
        .tab_stop(props.tab_stop)
        .padding(props.empty_text_padding)
        .empty_text_style(props.empty_text_style);
    if let Some(text) = props.empty_text.clone() {
        list = list.empty_text(text);
    }
    // Focus still lands here and still reports, so an application mirroring the tree's focus does
    // not go stale for exactly the states where it has nothing to draw a cursor on.
    if let Some(cb) = props.on_focus.clone() {
        list = list.on_focus(cb);
    }
    if let Some(cb) = props.on_blur.clone() {
        list = list.on_blur(cb);
    }
    list.key(props.tree_focus_key.clone())
}

fn needs_git_snapshot(props: &FileTreeProps) -> bool {
    !uses_provided_entries(props)
        && matches!(props.change_source, FileTreeChangeSource::Git)
        && (props.git_status
            || props.git_diff_stats
            || props.change_view == FileTreeChangeView::ChangedOnly
            // Ignored paths come from the same status query, markers or not.
            || props.ignored_style != Style::default())
}

fn should_load_git_after_props_change(old_props: &FileTreeProps, props: &FileTreeProps) -> bool {
    needs_git_snapshot(props)
        && (!needs_git_snapshot(old_props)
            || old_props.root != props.root
            || old_props.git_diff_stats != props.git_diff_stats)
}

fn is_provided_changed_only(props: &FileTreeProps) -> bool {
    matches!(props.change_source, FileTreeChangeSource::Provided(_))
        && props.change_view == FileTreeChangeView::ChangedOnly
}

fn uses_provided_entries(props: &FileTreeProps) -> bool {
    matches!(props.entry_source, FileTreeEntrySource::Provided(_))
}

fn uses_provided_paths(props: &FileTreeProps) -> bool {
    uses_provided_entries(props) || is_provided_changed_only(props)
}

fn uses_local_entries(props: &FileTreeProps) -> bool {
    matches!(props.entry_source, FileTreeEntrySource::Local)
}

fn entry_refresh_options(props: &FileTreeProps) -> (bool, usize) {
    (props.show_hidden, props.max_entries_per_dir)
}

fn tracks_uncontrolled_selection(props: &FileTreeProps) -> bool {
    props.selected.is_none() && props.selected_path.is_none() && props.select_path.is_none()
}

fn initial_uncontrolled_selected_path(props: &FileTreeProps, root: &FsNode) -> Option<Arc<str>> {
    tracks_uncontrolled_selection(props).then(|| root.path.clone())
}

fn uses_virtual_root(props: &FileTreeProps) -> bool {
    uses_provided_entries(props) || is_provided_changed_only(props)
}

fn initial_root(props: &FileTreeProps) -> (FsNode, bool) {
    let root_virtual = uses_virtual_root(props);
    let mut root = if uses_provided_entries(props) {
        provided_entry_root_node(&props.root)
    } else if root_virtual {
        virtual_root_node(&props.root)
    } else {
        root_node(&props.root)
    };
    if uses_provided_entries(props) {
        root.loaded = false;
        apply_provided_listings(&mut root, props);
        root.loading = !root.loaded;
    } else if root.is_dir() && !root_virtual {
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
    state.git_load_generation = state.git_load_generation.wrapping_add(1);
    state.last_git_refresh_nonce = props.git_refresh_nonce;
    state.entry_refresh_requested_token = props.entry_refresh_token;
    state.entry_refresh_completed_token = props.entry_refresh_token;
    state.entry_refresh_requested_options = entry_refresh_options(props);
    state.entry_refresh_pending = false;
    state.entry_refresh_generation = state.entry_refresh_generation.wrapping_add(1);
    state.changed_only_auto_expand_signature = 0;
    state.explorer_query_id = state.explorer_query_id.saturating_add(1);
    state.explorer_filter = ExplorerFilter::default();
    forget_explorer_index(state);
    state.search_expanded_snapshot = None;
    state.search_found_dir = None;
    state.requested_entry_paths.clear();
    state.uncontrolled_selected_path = initial_uncontrolled_selected_path(props, &state.root);
    apply_initial_expanded_paths(state, props);
    sync_symlink_targets(state, props);
}

/// Seed the tree's own expansion set from `initial_expanded_paths`, at the two moments it has none
/// of its own: mounting, and re-rooting.
///
/// Only the given paths are expanded, never their ancestors — a collapsed directory keeps its
/// contents' expansion, so seeding a descendant must not reopen the parent that hides it. Loading
/// therefore still walks down from the root and stops at the first collapsed directory, leaving the
/// deeper seeds to apply the moment their parent opens again.
fn apply_initial_expanded_paths(state: &mut FileTreeState, props: &FileTreeProps) {
    if props.expanded_paths.is_some() || props.initial_expanded_paths.is_empty() {
        return;
    }

    for path in &props.initial_expanded_paths {
        let resolved = if uses_provided_paths(props) {
            ProvidedPath::from_path(state.root.path.as_ref())
                .resolve(path.as_ref())
                .map(|path| Arc::<str>::from(path.as_str()))
        } else {
            resolve_path_under_root(&state.root.path, path.as_ref())
        };
        if let Some(resolved) = resolved {
            state.expanded.insert(resolved);
        }
    }

    let expanded = state.expanded.clone();
    load_expanded_directories(state, props, expanded);
}

fn effective_initial_snapshot(props: &FileTreeProps) -> GitStatusSnapshot {
    match &props.change_source {
        FileTreeChangeSource::Git if uses_provided_entries(props) => provided_entry_snapshot(props),
        FileTreeChangeSource::Git => GitStatusSnapshot::default(),
        FileTreeChangeSource::Provided(changes) => {
            provided_change_snapshot(props.root.as_ref(), changes)
        }
    }
}

fn effective_change_snapshot(state: &FileTreeState) -> &GitStatusSnapshot {
    &state.git_snapshot
}

fn apply_provided_listings(root: &mut FsNode, props: &FileTreeProps) {
    let FileTreeEntrySource::Provided(listings) = &props.entry_source else {
        return;
    };
    let listing_map = provided_listing_map(root.path.as_ref(), listings);
    apply_provided_listing_recursive(root, props, &listing_map);
}

fn apply_changed_provided_listings(
    root: &mut FsNode,
    old_props: &FileTreeProps,
    props: &FileTreeProps,
) {
    let (FileTreeEntrySource::Provided(old_listings), FileTreeEntrySource::Provided(listings)) =
        (&old_props.entry_source, &props.entry_source)
    else {
        return;
    };
    let old_map = provided_listing_map(root.path.as_ref(), old_listings);
    let listing_map = provided_listing_map(root.path.as_ref(), listings);
    let changed_paths: HashSet<Arc<str>> = old_map
        .keys()
        .chain(listing_map.keys())
        .filter(|path| old_map.get(path.as_ref()) != listing_map.get(path.as_ref()))
        .cloned()
        .collect();
    apply_changed_provided_listings_recursive(root, props, &listing_map, &changed_paths);
}

fn apply_changed_provided_listings_recursive(
    node: &mut FsNode,
    props: &FileTreeProps,
    listings: &HashMap<Arc<str>, &FileTreeDirectoryListing>,
    changed_paths: &HashSet<Arc<str>>,
) {
    if changed_paths.contains(&node.path) {
        if listings.contains_key(node.path.as_ref()) {
            apply_provided_listing_recursive(node, props, listings);
        } else {
            node.loaded = false;
            node.loading = true;
            node.error = None;
            node.children.clear();
        }
        return;
    }

    for child in &mut node.children {
        if child.is_dir() {
            apply_changed_provided_listings_recursive(child, props, listings, changed_paths);
        }
    }
}

fn provided_listing_map<'a>(
    root: &str,
    listings: &'a [FileTreeDirectoryListing],
) -> HashMap<Arc<str>, &'a FileTreeDirectoryListing> {
    let root = ProvidedPath::root(root);
    listings
        .iter()
        .filter_map(|listing| {
            root.resolve(listing.path.as_ref())
                .map(|path| (Arc::<str>::from(path.as_str()), listing))
        })
        .collect()
}

fn apply_provided_listing_recursive(
    node: &mut FsNode,
    props: &FileTreeProps,
    listings: &HashMap<Arc<str>, &FileTreeDirectoryListing>,
) {
    let Some(listing) = listings.get(node.path.as_ref()) else {
        return;
    };

    let directory = node.path.clone();
    apply_directory_load(
        node,
        provided_directory_load(directory.as_ref(), listing, props),
    );
    for child in &mut node.children {
        if child.is_dir() {
            apply_provided_listing_recursive(child, props, listings);
        }
    }
}

fn provided_directory_load(
    directory: &str,
    listing: &FileTreeDirectoryListing,
    props: &FileTreeProps,
) -> super::fs::DirectoryLoadResult {
    let entries = match &listing.entries {
        Ok(entries) => entries,
        Err(error) => {
            return super::fs::DirectoryLoadResult {
                entries: Vec::new(),
                omitted: 0,
                error: Some(error.clone()),
            };
        }
    };

    let mut loaded = Vec::new();
    let mut omitted = 0usize;
    let directory = ProvidedPath::from_path(directory);
    for entry in entries.iter() {
        if (!props.show_hidden && is_hidden_entry(entry.name.as_ref()))
            || !directory.is_valid_name(entry.name.as_ref())
        {
            continue;
        }
        if loaded.len() >= props.max_entries_per_dir {
            omitted = omitted.saturating_add(1);
            continue;
        }

        let kind = provided_entry_kind(entry);
        let path = directory.join_name(entry.name.as_ref());
        loaded.push(super::fs::LoadedEntry {
            name: entry.name.clone(),
            path: Arc::from(path.as_str()),
            kind,
        });
    }
    loaded.sort_by(|left, right| {
        let left_dir = left.kind == FileKind::Directory;
        let right_dir = right.kind == FileKind::Directory;
        right_dir
            .cmp(&left_dir)
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
            .then_with(|| left.name.cmp(&right.name))
    });

    super::fs::DirectoryLoadResult {
        entries: loaded,
        omitted,
        error: None,
    }
}

fn provided_entry_snapshot(props: &FileTreeProps) -> GitStatusSnapshot {
    let FileTreeEntrySource::Provided(listings) = &props.entry_source else {
        return GitStatusSnapshot::default();
    };
    let root = ProvidedPath::root(props.root.as_ref());
    let mut snapshot = GitStatusSnapshot {
        virtual_changes: true,
        ..GitStatusSnapshot::default()
    };

    for listing in listings {
        let Some(directory) = root.resolve(listing.path.as_ref()) else {
            continue;
        };
        let Ok(entries) = &listing.entries else {
            continue;
        };
        for entry in entries.iter() {
            if entry.ignored
                || (!props.show_hidden && is_hidden_entry(entry.name.as_ref()))
                || !root.is_valid_name(entry.name.as_ref())
            {
                continue;
            }
            let Some(status) = entry.git_status else {
                continue;
            };
            if status.staged.is_none() && status.unstaged.is_none() {
                continue;
            }

            let full_path = directory.join_name(entry.name.as_ref());
            let path = Arc::<str>::from(full_path.as_str());
            snapshot.changed_paths.push(path.clone());
            snapshot.kinds.insert(path, provided_entry_kind(entry));
            insert_provided_decoration_path_and_parents(
                &mut snapshot.entries,
                &full_path,
                GitFileDecorations::from_status(status, true),
                &root,
            );
        }
    }
    snapshot.changed_paths.sort();
    snapshot.changed_paths.dedup();
    snapshot
}

fn is_hidden_entry(name: &str) -> bool {
    name.starts_with('.') && name != "." && name != ".."
}

fn provided_entry_kind(entry: &super::FileTreeEntry) -> FileKind {
    if entry.is_symlink {
        FileKind::Symlink
    } else if entry.is_dir {
        FileKind::Directory
    } else {
        FileKind::File
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
            insert_path_with_ancestors_for(
                &mut expanded_paths,
                &path,
                &state.root.path,
                uses_provided_paths(props),
            );
        }
        Cow::Owned(expanded_paths)
    } else if !reveal_paths.is_empty() {
        let mut expanded_paths = state.expanded.clone();
        for path in reveal_paths {
            insert_path_with_ancestors_for(
                &mut expanded_paths,
                &path,
                &state.root.path,
                uses_provided_paths(props),
            );
        }
        Cow::Owned(expanded_paths)
    } else {
        Cow::Borrowed(&state.expanded)
    }
}

fn has_reveal_path(props: &FileTreeProps) -> bool {
    props.reveal_path.is_some() || props.select_path.is_some()
}

fn active_explorer_filter<'a>(
    props: &FileTreeProps,
    state: &'a FileTreeState,
) -> Option<&'a ExplorerFilter> {
    if !props.explorer || state.explorer_input.text().trim().is_empty() {
        None
    } else {
        Some(&state.explorer_filter)
    }
}

fn normalized_reveal_paths(props: &FileTreeProps, root_path: &Arc<str>) -> Vec<Arc<str>> {
    if uses_provided_paths(props) {
        let root = ProvidedPath::from_path(root_path.as_ref());
        return [props.reveal_path.as_ref(), props.select_path.as_ref()]
            .into_iter()
            .flatten()
            .filter_map(|path| root.resolve(path.as_ref()))
            .map(|path| Arc::<str>::from(path.as_str()))
            .collect();
    }

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
    let path = controlled_selected_path(props, state)?;
    projection.path_to_visible_index.get(path.as_ref()).copied()
}

/// The app-chosen selection, resolved to the path its row is keyed by.
fn controlled_selected_path(props: &FileTreeProps, state: &FileTreeState) -> Option<Arc<str>> {
    props
        .select_path
        .as_ref()
        .or(props.selected_path.as_ref())
        .and_then(|path| {
            if uses_provided_paths(props) {
                ProvidedPath::from_path(state.root.path.as_ref())
                    .resolve(path.as_ref())
                    .map(|path| Arc::<str>::from(path.as_str()))
            } else {
                resolve_path_under_root(&state.root.path, path.as_ref())
            }
        })
}

/// Whether hidden or ignored rows are drawn receded at all.
fn recedes_rows(props: &FileTreeProps) -> bool {
    props.hidden_style != Style::default() || props.ignored_style != Style::default()
}

/// The row the `Tree` selects, in the order it resolves one: an app-chosen path, the tracked
/// uncontrolled selection, then an app-chosen index. `clear_selection` draws no cursor at all.
fn effective_selected_index(
    props: &FileTreeProps,
    state: &FileTreeState,
    projection: &FileTreeProjection,
) -> Option<usize> {
    if props.clear_selection {
        return None;
    }
    selected_visible_index_by_path(props, state, projection)
        .or_else(|| uncontrolled_selected_visible_index(props, state, projection))
        .or(props.selected)
}

/// Whether a selected row would be highlighted now. That row drops its hidden or ignored style: the
/// highlight replaces the background but not the label's own colors, and a receded label on it all
/// but disappears.
fn cursor_highlight_shown(props: &FileTreeProps, state: &FileTreeState) -> bool {
    recedes_rows(props)
        && (state.tree_focused
            || props.unfocused_selection_style != StyleSlot::Replace(Style::default()))
}

fn uncontrolled_selected_visible_index(
    props: &FileTreeProps,
    state: &FileTreeState,
    projection: &FileTreeProjection,
) -> Option<usize> {
    if !tracks_uncontrolled_selection(props) {
        return None;
    }

    state
        .uncontrolled_selected_path
        .as_ref()
        .and_then(|path| projection.path_to_visible_index.get(path.as_ref()).copied())
}

fn normalize_uncontrolled_selection(props: &FileTreeProps, state: &mut FileTreeState) {
    if !tracks_uncontrolled_selection(props) {
        return;
    }

    let selected_path = {
        let projection = build_projection(props, state, active_explorer_filter(props, state));
        let selected = state
            .uncontrolled_selected_path
            .as_ref()
            .filter(|path| projection.path_to_visible_index.contains_key(path.as_ref()))
            .cloned();
        selected.or_else(|| {
            projection
                .path_to_visible_index
                .iter()
                .min_by_key(|(_, index)| *index)
                .map(|(path, _)| path.clone())
        })
    };

    state.uncontrolled_selected_path = selected_path;
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
            insert_path_with_ancestors_for(
                &mut state.expanded,
                path,
                &state.root.path,
                uses_provided_paths(props),
            );
            changed |= state.expanded.len() != before;
        }
        expanded_for_load = state.expanded.clone();
    }

    let before = loaded_signature(&state.root);
    load_expanded_directories(state, props, expanded_for_load);
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
    generation: u64,
    cache_request: Option<GitStatusCacheRequest>,
) -> Command {
    link.command(move |link| {
        let snapshot = load_git_snapshot(Path::new(repo_root.as_ref()), include_diff_stats);
        link.send(FileTreeMsg::GitSnapshotLoaded {
            generation,
            snapshot,
            cache_request,
        });
    })
}

fn local_entry_refresh_command(
    link: crate::callback::Link<FileTreeMsg>,
    generation: u64,
    token: u64,
    root: Arc<str>,
    expanded: HashSet<Arc<str>>,
    options: (bool, usize),
) -> Command {
    link.command(move |link| {
        let snapshot = load_local_snapshot(&root, &expanded, options);
        link.send(FileTreeMsg::EntryRefreshLoaded(LocalEntryRefreshResult {
            generation,
            token,
            root,
            snapshot,
            expanded,
            options,
        }));
    })
}

fn begin_local_entry_refresh(
    ctx: &mut Context<FileTreeComponent>,
    token: u64,
    expanded: HashSet<Arc<str>>,
    options: (bool, usize),
) -> Update {
    ctx.state.entry_refresh_requested_token = token;
    ctx.state.entry_refresh_requested_options = options;
    ctx.state.entry_refresh_pending = true;
    ctx.state.entry_refresh_generation = ctx.state.entry_refresh_generation.wrapping_add(1);
    let generation = ctx.state.entry_refresh_generation;
    let root = ctx.state.root.path.clone();
    invalidate_explorer_query_for_entry_refresh(&ctx.props, &mut ctx.state);
    Update::command_only(local_entry_refresh_command(
        ctx.link().clone(),
        generation,
        token,
        root,
        expanded,
        options,
    ))
}

/// Run `git status` for the current root, superseding any snapshot still loading.
fn start_git_refresh(ctx: &mut Context<FileTreeComponent>) -> Update {
    ctx.state.git_load_generation = ctx.state.git_load_generation.wrapping_add(1);
    let Some(repo_root) = discover_git_root(Path::new(ctx.props.root.as_ref())) else {
        ctx.state.git_snapshot_pending = false;
        return Update::none();
    };
    let repo_root = Arc::<str>::from(repo_root.to_string_lossy().as_ref());
    let include_diff_stats = ctx.props.git_diff_stats;
    let generation = ctx.state.git_load_generation;
    let (_, cache_request) = prepare_git_cache(&ctx.props, Path::new(repo_root.as_ref()));
    Update::with_command(git_snapshot_command(
        ctx.link().clone(),
        repo_root,
        include_diff_stats,
        generation,
        cache_request,
    ))
}

fn refresh_policy_active(props: &FileTreeProps) -> bool {
    uses_local_entries(props) && props.refresh_policy != FileTreeRefreshPolicy::Manual
}

fn watches_filesystem(props: &FileTreeProps) -> bool {
    uses_local_entries(props) && props.refresh_policy == FileTreeRefreshPolicy::Watch
}

/// The interval of the periodic full reread, or `None` when the policy has none.
fn refresh_interval(props: &FileTreeProps, watch: &WatchState) -> Option<std::time::Duration> {
    if !uses_local_entries(props) {
        return None;
    }
    match props.refresh_policy {
        FileTreeRefreshPolicy::Manual => None,
        FileTreeRefreshPolicy::Polling(interval) => Some(interval.max(watch::MIN_POLL_INTERVAL)),
        FileTreeRefreshPolicy::Watch if watch.degraded() => Some(watch::DEGRADED_INTERVAL),
        FileTreeRefreshPolicy::Watch => Some(watch::REPAIR_INTERVAL),
    }
}

/// Replace the running periodic chain with one at the current interval.
fn rearm_refresh_tick(ctx: &mut Context<FileTreeComponent>) {
    let poll_generation = ctx.state.watch.poll_generation.wrapping_add(1);
    ctx.state.watch.poll_generation = poll_generation;
    ctx.link()
        .send(FileTreeMsg::ArmRefreshTick { poll_generation });
}

/// Drop any watcher and start what the policy asks for from a clean slate.
fn restart_refresh_policy(ctx: &mut Context<FileTreeComponent>) -> Update {
    ctx.state.watch.reset();
    if !refresh_policy_active(&ctx.props) {
        return Update::none();
    }
    ctx.link().send(FileTreeMsg::ArmRefreshTick {
        poll_generation: ctx.state.watch.poll_generation,
    });
    if !watches_filesystem(&ctx.props) {
        return Update::none();
    }

    let generation = ctx.state.watch.generation;
    if needs_git_snapshot(&ctx.props) {
        ctx.link()
            .send(FileTreeMsg::RepositoryWatchResolve { generation });
    }
    let pending = ctx.state.watch.pending.clone();
    Update::command_only(ctx.link().command(move |link| {
        let wake = link.clone();
        match WatchSession::start(pending, move || {
            wake.send(FileTreeMsg::WatchDirty { generation });
        }) {
            Ok(session) => link.send(FileTreeMsg::WatchStarted {
                generation,
                session: Arc::new(session),
            }),
            Err(_) => link.send(FileTreeMsg::WatchUnavailable { generation }),
        }
    }))
}

/// Bring the repository watch up to date in the background: resolve the metadata directories and
/// every directory holding tracked files, then add and remove watches to match. One resolve runs at
/// a time; a change meanwhile queues exactly one more.
fn resolve_repository_watch(ctx: &mut Context<FileTreeComponent>, generation: u64) -> Update {
    let watch = &mut ctx.state.watch;
    if generation != watch.generation
        || !watches_filesystem(&ctx.props)
        || !needs_git_snapshot(&ctx.props)
    {
        return Update::none();
    }
    if watch.repository_resolving {
        watch.repository_resolve_again = true;
        return Update::none();
    }
    let Some(repo_root) = discover_git_root(Path::new(ctx.props.root.as_ref())) else {
        watch.repository = None;
        watch.repository_partial = false;
        return Update::none();
    };
    watch.repository_resolving = true;
    let existing = watch.repository.clone();
    let pending = watch.pending.clone();
    Update::command_only(ctx.link().command(move |link| {
        let repository = match existing {
            Some(repository) => repository,
            None => {
                let wake = link.clone();
                match RepositoryWatch::start(pending, move || {
                    wake.send(FileTreeMsg::WatchDirty { generation });
                }) {
                    Ok(repository) => Arc::new(repository),
                    Err(_) => {
                        link.send(FileTreeMsg::RepositoryWatchUpdated {
                            generation,
                            repository: None,
                            complete: false,
                        });
                        return;
                    }
                }
            }
        };
        let dirs = super::git::repository_watch_directories(&repo_root);
        let complete = repository.update(dirs);
        link.send(FileTreeMsg::RepositoryWatchUpdated {
            generation,
            repository: Some(repository),
            complete,
        });
    }))
}

/// One periodic pass: reread the visible tree and Git status, retry refused watches, and rearm.
fn refresh_tick(ctx: &mut Context<FileTreeComponent>, poll_generation: u64) -> Update {
    if poll_generation != ctx.state.watch.poll_generation || !refresh_policy_active(&ctx.props) {
        return Update::none();
    }
    ctx.link()
        .send(FileTreeMsg::ArmRefreshTick { poll_generation });
    // Cleared here and retried by the reconcile that follows this update.
    ctx.state.watch.failed.clear();
    if needs_git_snapshot(&ctx.props) {
        let generation = ctx.state.watch.generation;
        ctx.link().send(FileTreeMsg::WatchGitFlush { generation });
        // Also picks up tracked directories whose metadata event was missed.
        ctx.link()
            .send(FileTreeMsg::RepositoryWatchResolve { generation });
    }
    if ctx.state.entry_refresh_pending {
        return Update::none();
    }
    let expanded = effective_expanded_paths(&ctx.props, &ctx.state).into_owned();
    let token = ctx.props.entry_refresh_token;
    let options = entry_refresh_options(&ctx.props);
    begin_local_entry_refresh(ctx, token, expanded, options)
}

/// Watch exactly the visible directories, and return those whose listing must be reread.
fn reconcile_watches(state: &mut FileTreeState, props: &FileTreeProps) -> Vec<Arc<str>> {
    if !watches_filesystem(props) {
        return Vec::new();
    }
    let desired = visible_directories(&state.root, &effective_expanded_paths(props, state));
    state.watch.reconcile(desired)
}

/// Reconcile the watch set after anything that can change what is visible, an update or new props
/// alike: reread directories whose listing can no longer be trusted, and poll faster if coverage
/// was just lost. `was_degraded` is the coverage before the change.
fn sync_watches(ctx: &mut Context<FileTreeComponent>, was_degraded: bool) {
    let stale = reconcile_watches(&mut ctx.state, &ctx.props);
    if !stale.is_empty() {
        ctx.state.watch.dirty.extend(stale);
        ctx.link().send(FileTreeMsg::WatchReload {
            generation: ctx.state.watch.generation,
        });
    }
    if !was_degraded && ctx.state.watch.degraded() {
        // Something just lost watch coverage: poll at the degraded interval from now on, rather
        // than after the repair interval already armed.
        rearm_refresh_tick(ctx);
    }
}

/// Close a batch: queue the directories whose listing changed, and a Git refresh if one is due.
fn watch_flush(ctx: &mut Context<FileTreeComponent>, generation: u64) -> Update {
    if generation != ctx.state.watch.generation {
        return Update::none();
    }
    ctx.state.watch.flush_scheduled = false;
    let batch = ctx
        .state
        .watch
        .pending
        .lock()
        .map(|mut pending| pending.take())
        .unwrap_or_default();
    let entry_dirs = visible_directories(
        &ctx.state.root,
        &effective_expanded_paths(&ctx.props, &ctx.state),
    );
    let plan = plan_flush(batch, &entry_dirs);
    if plan.git && needs_git_snapshot(&ctx.props) {
        ctx.link().send(FileTreeMsg::WatchGitDirty { generation });
    }
    if plan.repository_changed {
        ctx.link()
            .send(FileTreeMsg::RepositoryWatchResolve { generation });
    }
    if plan.ignore_rules_changed {
        ctx.link().send(FileTreeMsg::ExplorerIgnoreRulesChanged);
    }
    if plan.rescan {
        ctx.state.watch.dirty.extend(entry_dirs);
    } else {
        ctx.state.watch.dirty.extend(plan.dirs);
    }
    start_watch_reload(ctx).map_or_else(Update::none, Update::command_only)
}

/// Reread the dirty directories in the background. One reread runs at a time, and none while a
/// full reread is pending; changes meanwhile wait in the dirty set and join the next one.
fn start_watch_reload(ctx: &mut Context<FileTreeComponent>) -> Option<Command> {
    let watch = &mut ctx.state.watch;
    if watch.reload_in_flight || ctx.state.entry_refresh_pending || watch.dirty.is_empty() {
        return None;
    }
    let mut dirs: Vec<Arc<str>> = watch.dirty.drain().collect();
    dirs.sort();
    watch.reload_in_flight = true;
    let generation = watch.generation;
    let entry_generation = ctx.state.entry_refresh_generation;
    let root = ctx.state.root.path.clone();
    let options = entry_refresh_options(&ctx.props);
    Some(ctx.link().command(move |link| {
        let (show_hidden, max_entries_per_dir) = options;
        let results = dirs
            .into_iter()
            .map(|dir| {
                let result = read_directory(dir.as_ref(), show_hidden, max_entries_per_dir);
                (dir, result)
            })
            .collect();
        link.send(FileTreeMsg::WatchDirectoriesLoaded {
            generation,
            entry_generation,
            root,
            options,
            results,
        });
    }))
}

/// Merge rereads into the tree, keeping every subtree that is still there.
fn apply_watch_reload(
    ctx: &mut Context<FileTreeComponent>,
    generation: u64,
    entry_generation: u64,
    root: Arc<str>,
    options: (bool, usize),
    results: Vec<(Arc<str>, super::fs::DirectoryLoadResult)>,
) -> Update {
    if generation != ctx.state.watch.generation {
        return Update::none();
    }
    ctx.state.watch.reload_in_flight = false;
    if entry_generation != ctx.state.entry_refresh_generation
        || root != ctx.state.root.path
        || options != entry_refresh_options(&ctx.props)
    {
        // The tree was replaced while these were read. Read them again against the new one.
        ctx.state
            .watch
            .dirty
            .extend(results.into_iter().map(|(dir, _)| dir));
        return start_watch_reload(ctx).map_or_else(Update::none, Update::command_only);
    }

    let mut listings_changed = false;
    for (dir, result) in results {
        if let Some(node) = node_by_path_mut(&mut ctx.state.root, &dir)
            && node.is_dir()
        {
            listings_changed |= listing_differs(node, &result);
            merge_directory_load(node, result);
        }
    }
    // A reread that found nothing new, such as the one that follows a search opening directories,
    // leaves the explorer's walk valid.
    if listings_changed {
        forget_explorer_index(&mut ctx.state);
    }

    // A directory that reappeared expanded, such as one renamed back, has no listing yet.
    let expanded = effective_expanded_paths(&ctx.props, &ctx.state).into_owned();
    let mut unloaded = Vec::new();
    collect_pending_expanded_paths(&ctx.state.root, &expanded, &mut unloaded);
    for path in &unloaded {
        if let Some(node) = node_by_path_mut(&mut ctx.state.root, path) {
            node.loading = true;
        }
    }
    ctx.state.watch.dirty.extend(unloaded);
    // Changes that arrived while this reread ran.
    if !ctx.state.watch.dirty.is_empty() {
        ctx.link().send(FileTreeMsg::WatchReload { generation });
    }

    sync_symlink_targets(&mut ctx.state, &ctx.props);
    if !ctx.props.explorer || ctx.state.explorer_input.text().trim().is_empty() {
        normalize_uncontrolled_selection(&ctx.props, &mut ctx.state);
    }
    Update::full()
}

/// Whether rereading a directory found different entries than the tree holds for it.
fn listing_differs(node: &FsNode, result: &DirectoryLoadResult) -> bool {
    let truncated = usize::from(result.omitted > 0);
    !node.loaded
        || node.error != result.error
        || node.children.len() != result.entries.len() + truncated
        || node
            .children
            .iter()
            .zip(&result.entries)
            .any(|(child, entry)| child.path != entry.path || child.kind != entry.kind)
}

/// Replace a directory's listing, keeping the loaded subtree of every child directory that is
/// still present. Unlike [`apply_directory_load`], rereading a parent does not collapse what was
/// open beneath it.
fn merge_directory_load(node: &mut FsNode, result: super::fs::DirectoryLoadResult) {
    let mut previous: HashMap<Arc<str>, FsNode> = std::mem::take(&mut node.children)
        .into_iter()
        .filter(FsNode::is_dir)
        .map(|child| (child.path.clone(), child))
        .collect();
    apply_directory_load(node, result);
    for child in &mut node.children {
        if child.is_dir()
            && let Some(old) = previous.remove(&child.path)
        {
            *child = FsNode {
                name: child.name.clone(),
                ..old
            };
        }
    }
}

fn prepare_git_cache(
    props: &FileTreeProps,
    repo_root: &Path,
) -> (Option<GitStatusSnapshot>, Option<GitStatusCacheRequest>) {
    let Some(cache) = props.git_status_cache.as_ref() else {
        return (None, None);
    };
    let key = GitStatusCacheKey::new(repo_root, props.git_diff_stats);
    let cached = cache.get(&key);
    let request = cache.begin_request(key);
    (cached, Some(request))
}

/// Search for the current query on a worker, superseding any search still waiting to run.
///
/// Each tree runs one search task at a time. A keystroke replaces the queued search rather than
/// queueing another walk behind it, and a walk already running finishes and fills the index cache,
/// which the replacing search then only filters.
fn start_explorer_search(
    props: &FileTreeProps,
    state: &mut FileTreeState,
    link: crate::callback::Link<FileTreeMsg>,
) -> Command {
    let query_id = state.explorer_query_id.saturating_add(1);
    state.explorer_query_id = query_id;
    let query: Arc<str> = Arc::from(state.explorer_input.text().to_owned());
    let search_root = if props.change_view == FileTreeChangeView::ChangedOnly {
        state.root.path.clone()
    } else {
        props.root.clone()
    };
    let source = match explorer_search_candidates(props, state) {
        Some(candidates) => ExplorerSearchSource::Candidates {
            root: search_root,
            candidates,
        },
        None => ExplorerSearchSource::Filesystem {
            cache: Arc::clone(&state.explorer_index),
            key: ExplorerIndexKey {
                root: search_root,
                show_hidden: props.show_hidden,
                max_entries_per_dir: props.max_entries_per_dir,
            },
        },
    };
    // The same directories `load_expanded_directories` would otherwise read on the UI thread.
    let listing_options =
        (!uses_provided_entries(props)).then_some((props.show_hidden, props.max_entries_per_dir));
    let max_matches = props.explorer_max_matches;
    link.command_keyed(
        Arc::clone(&state.explorer_task_key),
        TaskPolicy::LatestOnly,
        move |link| {
            let index = match source {
                ExplorerSearchSource::Filesystem { cache, key } => cache.get_or_walk(key),
                ExplorerSearchSource::Candidates { root, candidates } => {
                    Arc::new(ExplorerIndex::from_candidates(
                        &root,
                        candidates.entries,
                        candidates.provided_paths,
                    ))
                }
            };
            if link.is_cancelled() {
                return;
            }
            let filter = index.search(query.as_ref(), max_matches);
            let listings = listing_options
                .map(|(show_hidden, max_entries)| {
                    read_search_listings(&filter, show_hidden, max_entries)
                })
                .unwrap_or_default();
            link.send(FileTreeMsg::ExplorerResultsReady {
                query_id,
                filter,
                listings,
            });
        },
    )
}

/// Where a search's entries come from.
enum ExplorerSearchSource {
    /// Walk the filesystem, or reuse the cached walk.
    Filesystem {
        cache: Arc<ExplorerIndexCache>,
        key: ExplorerIndexKey,
    },
    /// Entries the tree already holds.
    Candidates {
        root: Arc<str>,
        candidates: ExplorerSearchCandidates,
    },
}

fn next_explorer_task_key() -> Arc<str> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    Arc::from(format!("tui-lipan.file-tree.explorer.{id}"))
}

/// Read the directories a search's matches open, in path order so each parent precedes its
/// children.
fn read_search_listings(
    filter: &ExplorerFilter,
    show_hidden: bool,
    max_entries_per_dir: usize,
) -> Vec<(Arc<str>, DirectoryLoadResult)> {
    let mut directories: Vec<&Arc<str>> = filter.expanded_paths.iter().collect();
    directories.sort();
    directories
        .into_iter()
        .map(|directory| {
            let listing = read_directory(directory.as_ref(), show_hidden, max_entries_per_dir);
            (directory.clone(), listing)
        })
        .collect()
}

/// Install the listings a search read into the expanded directories the tree has not loaded, in
/// one pass down the tree. A directory already loaded keeps its own listing, which the watcher
/// keeps current.
fn apply_search_listings(
    node: &mut FsNode,
    listings: &mut HashMap<Arc<str>, DirectoryLoadResult>,
    expanded: &HashSet<Arc<str>>,
) {
    if listings.is_empty() || !node.is_dir() || !expanded.contains(&node.path) {
        return;
    }
    if !node.loaded
        && let Some(listing) = listings.remove(&node.path)
    {
        apply_directory_load(node, listing);
    }
    for child in &mut node.children {
        apply_search_listings(child, listings, expanded);
    }
}

/// Drop the explorer's last walk, because what it walked may have changed.
fn forget_explorer_index(state: &mut FileTreeState) {
    state.explorer_index = Arc::default();
}

fn explorer_match_count_label(filter: &ExplorerFilter) -> String {
    if filter.shown_count < filter.match_count {
        format!("{}/{}", filter.shown_count, filter.match_count)
    } else {
        filter.match_count.to_string()
    }
}

fn invalidate_explorer_query_for_entry_refresh(props: &FileTreeProps, state: &mut FileTreeState) {
    forget_explorer_index(state);
    if props.explorer && !state.explorer_input.text().trim().is_empty() {
        state.explorer_query_id = state.explorer_query_id.saturating_add(1);
        state.explorer_filter = ExplorerFilter::default();
    }
}

fn restart_explorer_search(
    props: &FileTreeProps,
    state: &mut FileTreeState,
    link: crate::callback::Link<FileTreeMsg>,
) -> Option<Command> {
    if !props.explorer || state.explorer_input.text().trim().is_empty() {
        return None;
    }

    Some(start_explorer_search(props, state, link))
}

/// Learn where the tree's symlinks point, for the `link → target` suffix, and whether they resolve
/// to a directory, for the icon and label style.
///
/// Rebuilt from the tree rather than accumulated, so a link that was repointed between listings
/// reports where it points now, and a directory that has since collapsed out of the tree stops
/// being remembered. That costs a `read_link` and a `stat` per link on the tree, which is why this
/// runs where the tree changes shape rather than per frame — and why a tree with no links costs a
/// walk.
///
/// Under a provided entry source the targets come from the listings: the widget cannot follow a
/// link that lives on another host.
fn sync_symlink_targets(state: &mut FileTreeState, props: &FileTreeProps) {
    state.symlink_targets.clear();
    if uses_provided_paths(props) {
        collect_provided_symlink_targets(&state.root, props, &mut state.symlink_targets);
    } else {
        collect_local_symlink_targets(&state.root, &mut state.symlink_targets);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn collect_local_symlink_targets(node: &FsNode, targets: &mut HashMap<Arc<str>, SymlinkInfo>) {
    if matches!(node.kind, FileKind::Symlink) {
        let path = Path::new(node.path.as_ref());
        targets.insert(
            node.path.clone(),
            SymlinkInfo {
                target: std::fs::read_link(path)
                    .ok()
                    .map(|target| Arc::from(target.to_string_lossy().as_ref())),
                // `metadata` follows the link; a dangling one is an error and draws as a file.
                is_dir: std::fs::metadata(path).is_ok_and(|meta| meta.is_dir()),
            },
        );
    }
    for child in &node.children {
        collect_local_symlink_targets(child, targets);
    }
}

#[cfg(target_arch = "wasm32")]
fn collect_local_symlink_targets(_node: &FsNode, _targets: &mut HashMap<Arc<str>, SymlinkInfo>) {}

fn collect_provided_symlink_targets(
    root: &FsNode,
    props: &FileTreeProps,
    targets: &mut HashMap<Arc<str>, SymlinkInfo>,
) {
    let FileTreeEntrySource::Provided(listings) = &props.entry_source else {
        return;
    };
    for (directory, listing) in provided_listing_map(root.path.as_ref(), listings) {
        let directory = ProvidedPath::from_path(directory.as_ref());
        let Ok(entries) = &listing.entries else {
            continue;
        };
        for entry in entries.iter() {
            if !entry.is_symlink || !directory.is_valid_name(entry.name.as_ref()) {
                continue;
            }
            let path = directory.join_name(entry.name.as_ref());
            targets.insert(
                Arc::from(path.as_str()),
                SymlinkInfo {
                    target: entry.symlink_target.clone(),
                    is_dir: entry.is_dir,
                },
            );
        }
    }
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

fn load_expanded_directories(
    state: &mut FileTreeState,
    props: &FileTreeProps,
    expanded: HashSet<Arc<str>>,
) {
    if uses_provided_entries(props) {
        request_missing_provided_directories_for(state, props, &expanded);
    } else {
        load_expanded_directories_sync(
            &mut state.root,
            &expanded,
            props.show_hidden,
            props.max_entries_per_dir,
        );
    }
    sync_symlink_targets(state, props);
}

fn load_local_snapshot(
    root_path: &str,
    expanded: &HashSet<Arc<str>>,
    (show_hidden, max_entries_per_dir): (bool, usize),
) -> FsNode {
    let mut root = root_node(&Arc::from(root_path));
    load_local_snapshot_node(&mut root, expanded, show_hidden, max_entries_per_dir);
    root
}

fn load_local_snapshot_node(
    node: &mut FsNode,
    expanded: &HashSet<Arc<str>>,
    show_hidden: bool,
    max_entries_per_dir: usize,
) {
    if !node.is_dir() || !expanded.contains(&node.path) {
        return;
    }

    apply_directory_load(
        node,
        read_directory(node.path.as_ref(), show_hidden, max_entries_per_dir),
    );
    for child in &mut node.children {
        load_local_snapshot_node(child, expanded, show_hidden, max_entries_per_dir);
    }
}

fn request_missing_provided_directories(state: &mut FileTreeState, props: &FileTreeProps) {
    let expanded = effective_expanded_paths(props, state).into_owned();
    request_missing_provided_directories_for(state, props, &expanded);
}

fn request_missing_provided_directories_for(
    state: &mut FileTreeState,
    props: &FileTreeProps,
    expanded: &HashSet<Arc<str>>,
) {
    if !uses_provided_entries(props) {
        return;
    }

    let mut paths = Vec::new();
    collect_pending_expanded_paths(&state.root, expanded, &mut paths);
    for path in paths {
        request_provided_directory(state, props, path);
    }
}

fn collect_pending_expanded_paths(
    node: &FsNode,
    expanded: &HashSet<Arc<str>>,
    paths: &mut Vec<Arc<str>>,
) {
    if !node.is_dir() || !expanded.contains(&node.path) {
        return;
    }

    if !node.loaded {
        paths.push(node.path.clone());
        return;
    }

    for child in &node.children {
        collect_pending_expanded_paths(child, expanded, paths);
    }
}

fn request_provided_directory(state: &mut FileTreeState, props: &FileTreeProps, path: Arc<str>) {
    let Some(node) = node_by_path_mut(&mut state.root, &path) else {
        return;
    };
    if !node.is_dir() || node.loaded {
        return;
    }
    node.loading = true;
    let Some(callback) = props.on_entry_request.as_ref() else {
        return;
    };
    if !state.requested_entry_paths.insert(path.clone()) {
        return;
    }
    callback.emit(FileTreeEntryRequest { path });
}

fn clear_completed_entry_requests(state: &mut FileTreeState, props: &FileTreeProps) {
    let FileTreeEntrySource::Provided(listings) = &props.entry_source else {
        state.requested_entry_paths.clear();
        return;
    };
    let root = ProvidedPath::from_path(state.root.path.as_ref());
    for listing in listings {
        if let Some(path) = root.resolve(listing.path.as_ref()) {
            state.requested_entry_paths.remove(path.as_str());
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
    provided: bool,
) -> Arc<str> {
    if kind == FileKind::Directory {
        return path.clone();
    }

    let parent: Option<Arc<str>> = if provided {
        ProvidedPath::from_path(path.as_ref())
            .parent()
            .map(|value| Arc::<str>::from(value.as_str()))
    } else {
        Path::new(path.as_ref())
            .parent()
            .map(|value| Arc::<str>::from(value.to_string_lossy().as_ref()))
    };

    match parent {
        Some(path) if provided => {
            let path = ProvidedPath::from_path(path.as_ref());
            let root = ProvidedPath::from_path(root_path.as_ref());
            if path.starts_with(&root) {
                Arc::from(path.as_str())
            } else {
                root_path.clone()
            }
        }
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

fn insert_path_with_ancestors_for(
    expanded: &mut HashSet<Arc<str>>,
    path: &Arc<str>,
    root_path: &Arc<str>,
    provided: bool,
) {
    if provided {
        insert_provided_path_with_ancestors(expanded, path, root_path);
    } else {
        insert_path_with_ancestors(expanded, path, root_path);
    }
}

fn insert_provided_path_with_ancestors(
    expanded: &mut HashSet<Arc<str>>,
    path: &Arc<str>,
    root_path: &Arc<str>,
) {
    let current = ProvidedPath::from_path(path.as_ref());
    let root = ProvidedPath::from_path(root_path.as_ref());
    if !current.starts_with(&root) {
        return;
    }

    let mut current = Some(current);
    while let Some(path) = current {
        expanded.insert(Arc::from(path.as_str()));
        if path == root {
            break;
        }
        current = path.parent();
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
    let projection = build_projection_lifting(props, state, explorer_filter, None);
    // The cursor row is only known once the rows are: a selection by index names a row, not a path.
    // Rebuilding is rare, since it takes a cursor resting on a hidden or ignored row.
    if cursor_highlight_shown(props, state)
        && let Some(index) = effective_selected_index(props, state, &projection)
        && let Some(path) = projection
            .path_to_visible_index
            .iter()
            .find_map(|(path, &row)| (row == index).then(|| path.clone()))
        && projection.receded_paths.contains(&path)
    {
        return build_projection_lifting(props, state, explorer_filter, Some(&path));
    }
    projection
}

/// Build the rows, drawing `lifted` without its hidden or ignored style.
fn build_projection_lifting(
    props: &FileTreeProps,
    state: &FileTreeState,
    explorer_filter: Option<&ExplorerFilter>,
    lifted: Option<&str>,
) -> FileTreeProjection {
    let snapshot = effective_change_snapshot(state);
    let root = if props.change_view == FileTreeChangeView::ChangedOnly {
        let mut root = build_changed_only_root(&state.root, snapshot, props.show_hidden);
        if uses_provided_entries(props) {
            insert_unknown_provided_directories(&mut root, &state.root);
            sort_virtual_tree(&mut root);
            sync_projected_load_states(&mut root, &state.root);
        }
        // The explorer's matches are keyed by each directory's own path and name, which a folded
        // row no longer has, so a query shows the tree unfolded.
        if props.compact_directories && explorer_filter.is_none() {
            compact_directory_chains(&mut root);
        }
        root
    } else {
        state.root.clone()
    };
    let expanded = effective_expanded_paths(props, state);
    // Whether there is anything to project, read off the source tree rather than the rows that come
    // out of it. A collapsed root contributes no rows at all, and answering "nothing changed"
    // because the heading is closed would replace the whole tab with its empty state the moment the
    // user folds it shut. A filter is different: it is the user narrowing the same set, so a query
    // that matches nothing really does leave nothing to show.
    let source_is_empty = root.children.is_empty()
        || explorer_filter.is_some_and(|filter| {
            !root
                .children
                .iter()
                .any(|child| filter.visible_paths.contains(&child.path))
        });

    let mut lookup = HashMap::new();
    let path_styles = resolved_path_styles(
        &props.path_styles,
        root.path.as_ref(),
        uses_provided_paths(props),
    );
    let ignored_paths = ignored_paths(props, snapshot);
    let mut receded_paths = HashSet::new();
    let mut build_ctx = ProjectionBuildContext {
        props,
        expanded: expanded.as_ref(),
        explorer_filter,
        git_decorations: &snapshot.entries,
        symlink_targets: &state.symlink_targets,
        ignored_paths: ignored_paths.as_ref(),
        root_path: root.path.as_ref(),
        highlighted_path: lifted,
        receded_paths: &mut receded_paths,
        path_styles: &path_styles,
        lookup: &mut lookup,
    };
    let root = build_projected_tree_node(&root, true, Vec::new(), &mut build_ctx);
    let path_to_visible_index = visible_index_by_path(&root, &lookup);

    FileTreeProjection {
        root,
        source_is_empty,
        lookup: Arc::new(lookup),
        path_to_visible_index,
        receded_paths,
    }
}

/// Fold every run of directories holding nothing but one subdirectory into one node named
/// `outer/inner`. The node takes the innermost directory's path, state, and children, since those
/// are what its row opens onto. The root itself is never folded: it is the tree's heading.
///
/// The separator is always `/`, whatever the platform: it is a label, never parsed back as a path.
fn compact_directory_chains(node: &mut FsNode) {
    for child in &mut node.children {
        while is_single_directory_chain(child) {
            let Some(only) = child.children.pop() else {
                break;
            };
            child.name = Arc::from(format!("{}/{}", child.name, only.name));
            child.path = only.path;
            child.loaded = only.loaded;
            child.loading = only.loading;
            child.error = only.error;
            child.children = only.children;
        }
        compact_directory_chains(child);
    }
}

fn is_single_directory_chain(node: &FsNode) -> bool {
    node.is_dir()
        && node.loaded
        && !node.loading
        && node.error.is_none()
        && matches!(node.children.as_slice(), [only] if only.is_dir())
}

/// The paths ignore rules exclude, for `ignored_style`. A provided tree carries them on its
/// listings, whatever its change source; a local one learns them from Git status.
fn ignored_paths<'a>(
    props: &FileTreeProps,
    snapshot: &'a GitStatusSnapshot,
) -> Cow<'a, HashSet<Arc<str>>> {
    if props.ignored_style == Style::default() {
        return Cow::Owned(HashSet::new());
    }
    let FileTreeEntrySource::Provided(listings) = &props.entry_source else {
        return Cow::Borrowed(&snapshot.ignored_paths);
    };
    let root = ProvidedPath::root(props.root.as_ref());
    let mut ignored = HashSet::new();
    for listing in listings {
        let (Some(directory), Ok(entries)) =
            (root.resolve(listing.path.as_ref()), &listing.entries)
        else {
            continue;
        };
        for entry in entries.iter() {
            if entry.ignored && root.is_valid_name(entry.name.as_ref()) {
                let path = directory.join_name(entry.name.as_ref());
                ignored.insert(Arc::from(path.as_str()));
            }
        }
    }
    Cow::Owned(ignored)
}

/// Whether `path`, or a directory above it short of `root`, is ignored. Everything inside an
/// ignored directory is ignored too, and git names only the directory.
fn under_ignored_path(path: &str, root: &str, ignored: &HashSet<Arc<str>>) -> bool {
    if ignored.is_empty() {
        return false;
    }
    let mut current = path;
    while current.len() > root.len() {
        if ignored.contains(current) {
            return true;
        }
        let Some(separator) = current.rfind(['/', '\\']) else {
            break;
        };
        current = &current[..separator];
    }
    false
}

fn sync_projected_load_states(projected: &mut FsNode, source: &FsNode) {
    projected.loaded = source.loaded;
    projected.loading = source.loading;
    projected.error = source.error.clone();
    let source_by_path: HashMap<&str, &FsNode> = source
        .children
        .iter()
        .map(|child| (child.path.as_ref(), child))
        .collect();
    for child in &mut projected.children {
        if let Some(source_child) = source_by_path.get(child.path.as_ref()) {
            sync_projected_load_states(child, source_child);
        }
    }
}

fn insert_unknown_provided_directories(projected: &mut FsNode, source: &FsNode) {
    let unknown_children = source
        .children
        .iter()
        .filter_map(build_unknown_provided_directory)
        .collect();
    merge_unknown_provided_directories(projected, unknown_children);
}

fn build_unknown_provided_directory(source: &FsNode) -> Option<FsNode> {
    if !source.is_dir() {
        return None;
    }
    let children: Vec<FsNode> = source
        .children
        .iter()
        .filter_map(build_unknown_provided_directory)
        .collect();
    if source.loaded && children.is_empty() {
        return None;
    }

    Some(FsNode {
        name: source.name.clone(),
        path: source.path.clone(),
        kind: FileKind::Directory,
        loaded: source.loaded,
        loading: source.loading,
        error: source.error.clone(),
        children,
    })
}

fn merge_unknown_provided_directories(projected: &mut FsNode, unknown: Vec<FsNode>) {
    let mut projected_by_path: HashMap<Arc<str>, usize> = projected
        .children
        .iter()
        .enumerate()
        .map(|(index, child)| (child.path.clone(), index))
        .collect();
    for mut child in unknown {
        if let Some(index) = projected_by_path.get(child.path.as_ref()).copied() {
            merge_unknown_provided_directories(
                &mut projected.children[index],
                std::mem::take(&mut child.children),
            );
        } else {
            let path = child.path.clone();
            projected.children.push(child);
            projected_by_path.insert(path, projected.children.len() - 1);
        }
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
    // A folded `src/.config` row is as hidden as a `.config` one, so every segment counts.
    // The cursor row keeps full color; see `highlighted_path`.
    let recedes = !is_root && ctx.highlighted_path != Some(node.path.as_ref());
    let is_hidden = recedes && node.name.split('/').any(is_hidden_entry);
    let is_ignored =
        recedes && under_ignored_path(node.path.as_ref(), ctx.root_path, ctx.ignored_paths);
    if is_hidden || is_ignored {
        ctx.receded_paths.insert(node.path.clone());
    }
    let links_to_dir = ctx
        .symlink_targets
        .get(node.path.as_ref())
        .is_some_and(|link| link.is_dir);

    if ctx.props.show_icons {
        let mut icon_span = node
            .kind
            .icon(&node.name, is_expanded || links_to_dir, ctx.props);
        if is_hidden {
            icon_span.style = icon_span.style.patch(ctx.props.hidden_style);
        }
        if is_ignored {
            icon_span.style = icon_span.style.patch(ctx.props.ignored_style);
        }
        if let Some(style) = path_style.and_then(|style| style.icon) {
            icon_span.style = icon_span.style.patch(style);
        }
        spans.push(icon_span);
        spans.push(Span::new(" ".repeat(ctx.props.icon_gap as usize)));
    }

    // Build the label. Change status styles are opt-in so file names stay readable
    // while indicators carry dirty-state color by default.
    let label = if is_root {
        if uses_provided_paths(ctx.props) {
            node.path.as_ref().to_owned()
        } else {
            super::fs::path_to_display(node.path.as_ref())
        }
    } else {
        node.name.as_ref().to_string()
    };

    // A link takes the label of what it stands for; the icon and the target suffix already say it
    // is a link.
    let mut kind_label_style = match node.kind {
        FileKind::Directory => ctx.props.directory_label_style,
        FileKind::Symlink if links_to_dir => ctx.props.directory_label_style,
        FileKind::File | FileKind::Symlink => ctx.props.file_label_style,
        FileKind::Other => Style::default(),
    };
    if is_hidden {
        kind_label_style = kind_label_style.patch(ctx.props.hidden_style);
    }
    if is_ignored {
        kind_label_style = kind_label_style.patch(ctx.props.ignored_style);
    }

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
    // Part of the name, not of the row's metadata: `link → target` reads as one thing and belongs
    // against the label. The description slot is right-aligned, which would park the target at the
    // far edge of the row with a field of blanks between it and the link it belongs to.
    if let Some(target) = symlink_target_span(node, ctx) {
        spans.push(target);
    }

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

    let mut tree = TreeNode::new(item)
        // A directory is expandable because it is a directory, not because of what is inside it.
        // Deriving this from the loaded children instead would strand an empty one: opening it
        // loads nothing, so closing it again would leave a row with no arrow and no way back open.
        .expanded(is_expanded)
        .expansion_mode(if node.is_dir() {
            crate::widgets::TreeExpansionMode::Collapsible
        } else {
            crate::widgets::TreeExpansionMode::Auto
        });

    if node.is_dir() {
        if node.loading {
            tree = tree.child(TreeNode::new(ctx.props.loading_label.clone()));
        } else if !node.loaded {
            tree = tree.child(TreeNode::new(" "));
        } else if let Some(error) = node.error.as_ref() {
            tree = tree.child(TreeNode::new(format!("{} {error}", ctx.props.error_prefix)));
        } else if !is_expanded && ctx.explorer_filter.is_none() {
            // A collapsed directory contributes no visible rows — `Tree` never flattens past one —
            // so materializing its subtree is wasted work on every frame. Its real children are
            // built on the next render, once `expanded` contains this path.
            //
            // Explorer search keeps the full walk: its filter decides visibility per child, so a
            // collapsed directory's toggleability depends on how many of its children survive the
            // filter.
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

/// The ` → target` that follows a symlink's name, when the tree knows where it points.
///
/// One span rather than an arrow plus a target: they are never styled apart, and keeping them
/// together means truncation drops the whole tail instead of leaving a dangling arrow.
fn symlink_target_span(
    node: &FsNode,
    ctx: &ProjectionBuildContext<'_>,
) -> Option<crate::style::Span> {
    if !ctx.props.symlink_targets || !matches!(node.kind, FileKind::Symlink) {
        return None;
    }
    let target = ctx
        .symlink_targets
        .get(node.path.as_ref())?
        .target
        .as_ref()?;
    Some(
        crate::style::Span::new(format!(" {} {target}", ctx.props.symlink_target_arrow))
            .style(ctx.props.symlink_target_style),
    )
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
    lexical_only: bool,
) -> HashMap<Arc<str>, FileTreeItemStyle> {
    let provided_root = lexical_only.then(|| ProvidedPath::root(effective_root));
    let root = Path::new(effective_root);
    styles
        .iter()
        .filter_map(|(path, style)| {
            let resolved = if lexical_only {
                provided_root
                    .as_ref()
                    .and_then(|root| root.resolve(path.as_ref()))
                    .map(|path| path.as_str().to_owned())
            } else {
                resolve_item_style_path(root, path.as_ref())
                    .map(|path| path.to_string_lossy().into_owned())
            };
            resolved.map(|resolved| (Arc::<str>::from(resolved), *style))
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
    super::fs::canonicalize_plain(path).unwrap_or_else(|_| lexical_normalize_path(path))
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

    if props.git_status {
        push_git_status_spans(&mut spans, decoration.status, props);
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
    if props.git_status_padding_right > 0 {
        spans.push(Span::new(
            " ".repeat(props.git_status_padding_right as usize),
        ));
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
    if snapshot.virtual_changes {
        return build_provided_changed_only_root(source_root, snapshot, show_hidden);
    }

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

fn build_provided_changed_only_root(
    source_root: &FsNode,
    snapshot: &GitStatusSnapshot,
    show_hidden: bool,
) -> FsNode {
    let root_path = ProvidedPath::from_path(source_root.path.as_ref());
    let mut root = FsNode {
        name: source_root.name.clone(),
        path: source_root.path.clone(),
        kind: FileKind::Directory,
        loaded: true,
        loading: false,
        error: None,
        children: Vec::new(),
    };

    for changed_path in &snapshot.changed_paths {
        let path = ProvidedPath::from_path(changed_path.as_ref());
        if !path.starts_with(&root_path)
            || provided_hidden_under_root(&path, &root_path, show_hidden)
        {
            continue;
        }
        let Some(relative) = path.strip_prefix(&root_path) else {
            continue;
        };
        if relative.as_str().is_empty() {
            continue;
        }

        insert_provided_changed_path(
            &mut root,
            &root_path,
            &relative,
            snapshot.kinds.get(changed_path.as_ref()).copied(),
        );
    }

    sort_virtual_tree(&mut root);
    root
}

fn insert_provided_changed_path(
    root: &mut FsNode,
    root_path: &ProvidedPath,
    relative: &ProvidedPath,
    kind: Option<FileKind>,
) {
    let mut node = root;
    let mut full_path = root_path.clone();
    let components: Vec<_> = relative.components().collect();

    for (index, component) in components.iter().enumerate() {
        if !component.is_normal() {
            continue;
        }
        let name = component.as_normal_str().unwrap_or_default();
        full_path = full_path.join_name(name);
        let is_leaf = index + 1 == components.len();
        let path = Arc::<str>::from(full_path.as_str());
        let existing_index = node.children.iter().position(|child| child.path == path);
        let child_index = if let Some(existing_index) = existing_index {
            existing_index
        } else {
            node.children.push(FsNode {
                name: Arc::from(name),
                path: path.clone(),
                kind: if is_leaf {
                    kind.unwrap_or(FileKind::File)
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

fn provided_hidden_under_root(
    path: &ProvidedPath,
    root_path: &ProvidedPath,
    show_hidden: bool,
) -> bool {
    if show_hidden {
        return false;
    }
    path.strip_prefix(root_path).is_some_and(|relative| {
        relative.components().any(|component| {
            component
                .as_normal_str()
                .is_some_and(|name| name.starts_with('.'))
        })
    })
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
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
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

fn explorer_search_candidates(
    props: &FileTreeProps,
    state: &FileTreeState,
) -> Option<ExplorerSearchCandidates> {
    if props.change_view == FileTreeChangeView::ChangedOnly {
        return changed_only_search_candidates(props, state);
    }
    if uses_provided_entries(props) {
        return Some(ExplorerSearchCandidates {
            entries: provided_entry_search_candidates(props, &state.root),
            provided_paths: true,
        });
    }
    None
}

fn changed_only_search_candidates(
    props: &FileTreeProps,
    state: &FileTreeState,
) -> Option<ExplorerSearchCandidates> {
    if props.change_view != FileTreeChangeView::ChangedOnly {
        return None;
    }

    let snapshot = effective_change_snapshot(state);
    let root = build_changed_only_root(&state.root, snapshot, props.show_hidden);
    let mut candidates = Vec::new();
    collect_virtual_candidates(&root, true, &mut candidates);
    Some(ExplorerSearchCandidates {
        entries: candidates,
        provided_paths: snapshot.virtual_changes,
    })
}

fn provided_entry_search_candidates(
    props: &FileTreeProps,
    root: &FsNode,
) -> Vec<ExplorerCandidate> {
    let FileTreeEntrySource::Provided(listings) = &props.entry_source else {
        return Vec::new();
    };
    let root_path = ProvidedPath::from_path(root.path.as_ref());
    let mut ignored = HashSet::new();
    for listing in listings {
        let Some(directory) = root_path.resolve(listing.path.as_ref()) else {
            continue;
        };
        let Ok(entries) = &listing.entries else {
            continue;
        };
        for entry in entries.iter() {
            if entry.ignored && directory.is_valid_name(entry.name.as_ref()) {
                let path = directory.join_name(entry.name.as_ref());
                ignored.insert(Arc::<str>::from(path.as_str()));
            }
        }
    }
    let mut candidates = Vec::new();
    collect_provided_search_candidates(root, true, &ignored, &mut candidates);
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates
}

fn collect_provided_search_candidates(
    node: &FsNode,
    is_root: bool,
    ignored: &HashSet<Arc<str>>,
    candidates: &mut Vec<ExplorerCandidate>,
) {
    if !is_root {
        if ignored.contains(&node.path) || node.kind == FileKind::Other {
            return;
        }
        candidates.push(ExplorerCandidate {
            path: node.path.clone(),
            label: node.name.clone(),
            is_dir: node.is_dir(),
        });
    }
    for child in &node.children {
        collect_provided_search_candidates(child, false, ignored, candidates);
    }
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

fn expand_changed_only_directories(
    state: &mut FileTreeState,
    changed_paths: &[Arc<str>],
    provided: bool,
) {
    let root_path = state.root.path.clone();
    for path in changed_paths {
        if provided {
            if let Some(parent) = ProvidedPath::from_path(path.as_ref()).parent() {
                let parent = Arc::<str>::from(parent.as_str());
                insert_path_with_ancestors_for(&mut state.expanded, &parent, &root_path, true);
            }
        } else if let Some(parent) = Path::new(path.as_ref()).parent() {
            let parent = Arc::<str>::from(parent.to_string_lossy().as_ref());
            insert_path_with_ancestors_for(&mut state.expanded, &parent, &root_path, false);
        }
    }
}

fn virtual_root_node(root: &Arc<str>) -> FsNode {
    let path = ProvidedPath::root(root.as_ref());
    virtual_root_node_at(root, path)
}

fn provided_entry_root_node(root: &Arc<str>) -> FsNode {
    virtual_root_node_at(root, ProvidedPath::root(root.as_ref()))
}

fn virtual_root_node_at(root: &Arc<str>, path: ProvidedPath) -> FsNode {
    let name = path
        .file_name()
        .filter(|name| !name.is_empty())
        .map(Arc::from)
        .unwrap_or_else(|| root.clone());
    let path = Arc::<str>::from(path.as_str());

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
    use super::super::fs::FileIconStyle;
    use super::*;
    use crate::app::context::SurfaceMode;
    use crate::runtime::RuntimeCore;
    use crate::style::{Color, Rect, Theme};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::LazyLock;

    /// For the projection tests that build their own context: a tree with no symlinks in it needs
    /// no targets, and a borrow of an empty map has to outlive the context that holds it.
    static NO_SYMLINK_TARGETS: LazyLock<HashMap<Arc<str>, SymlinkInfo>> =
        LazyLock::new(HashMap::new);
    static NO_IGNORED_PATHS: LazyLock<HashSet<Arc<str>>> = LazyLock::new(HashSet::new);

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
            git_snapshot_pending: false,
            git_load_generation: 0,
            last_git_refresh_nonce: 0,
            entry_refresh_requested_token: 0,
            entry_refresh_completed_token: 0,
            entry_refresh_requested_options: (false, 2_000),
            entry_refresh_pending: false,
            entry_refresh_generation: 0,
            changed_only_auto_expand_signature: 0,
            explorer_input: TextInput::new(""),
            explorer_query_id: 0,
            explorer_filter: ExplorerFilter::default(),
            explorer_index: Arc::default(),
            explorer_task_key: next_explorer_task_key(),
            explorer_entered_from_tree: false,
            explorer_focused: false,
            search_expanded_snapshot: None,
            search_found_dir: None,
            requested_entry_paths: HashSet::new(),
            uncontrolled_selected_path: Some(Arc::from("/repo")),
            symlink_targets: HashMap::new(),
            tree_focused: false,
            watch: WatchState::default(),
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

    fn file_tree_runtime(props: FileTreeProps) -> RuntimeCore<FileTreeComponent> {
        RuntimeCore::new_test(
            FileTreeComponent::new(),
            props,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(std::cell::Cell::new(false)),
        )
    }

    fn replace_file_tree_props(
        runtime: &mut RuntimeCore<FileTreeComponent>,
        props: FileTreeProps,
    ) -> Update {
        let old_props = std::mem::replace(&mut runtime.ctx.props, props);
        runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx)
    }

    fn local_refresh_result(
        state: &FileTreeState,
        props: &FileTreeProps,
        generation: u64,
        token: u64,
    ) -> LocalEntryRefreshResult {
        let options = entry_refresh_options(props);
        let root = state.root.path.clone();
        let expanded = effective_expanded_paths(props, state).into_owned();
        LocalEntryRefreshResult {
            generation,
            token,
            root: root.clone(),
            snapshot: load_local_snapshot(root.as_ref(), &expanded, options),
            expanded,
            options,
        }
    }

    fn run_local_refresh_command(
        runtime: &mut RuntimeCore<FileTreeComponent>,
        generation: u64,
        token: u64,
        expanded: HashSet<Arc<str>>,
    ) -> LocalEntryRefreshResult {
        let root = runtime.ctx.state.root.path.clone();
        let options = entry_refresh_options(&runtime.ctx.props);
        local_entry_refresh_command(
            runtime.ctx.link().clone(),
            generation,
            token,
            root,
            expanded,
            options,
        )
        .run(crate::core::component::CommandRuntime {
            scope: crate::callback::ScopeId(1),
            tx: runtime.command_tx.clone(),
            runtime_id: runtime.ctx.env().runtime_id,
            now: runtime.ctx.env().now(),
            clock_mode: runtime.ctx.env().clock.mode(),
            clock: runtime.ctx.env().clock.clone(),
            activity: std::sync::Arc::clone(&runtime.ctx.env().activity),
        });

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            runtime.drain_commands();
            if let Some(message) = runtime.queue.borrow_mut().pop_front().map(|(_, message)| {
                *message
                    .downcast::<FileTreeMsg>()
                    .expect("expected a FileTree message")
            }) {
                let FileTreeMsg::EntryRefreshLoaded(result) = message else {
                    panic!("expected an entry refresh result");
                };
                return result;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "entry refresh command did not send a result"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
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

    fn collapsed_projection_props() -> FileTreeProps {
        crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(
                    ".",
                    [
                        super::super::FileTreeEntry::directory("src"),
                        super::super::FileTreeEntry::directory("empty"),
                    ],
                ),
                FileTreeDirectoryListing::new(
                    "src",
                    [super::super::FileTreeEntry::directory("widgets")],
                ),
                FileTreeDirectoryListing::new(
                    "src/widgets",
                    [super::super::FileTreeEntry::file("tree.rs")],
                ),
                FileTreeDirectoryListing::new("empty", Vec::new()),
            ]))
            .props
    }

    fn projected_child<'a>(node: &'a TreeNode, name: &str) -> &'a TreeNode {
        node.children
            .iter()
            .find(|child| {
                child
                    .item
                    .spans
                    .iter()
                    .any(|span| span.content.as_ref() == name)
            })
            .unwrap_or_else(|| panic!("no projected child named {name:?}"))
    }

    /// A symlink says where it points, the way `ls -l` does, and says it in the row's suffix so a
    /// change marker can still follow.
    #[cfg(unix)]
    #[test]
    fn a_symlink_row_shows_where_it_points() {
        let dir = unique_component_test_dir("symlink-target");
        std::fs::create_dir_all(&dir).expect("temp dir");
        std::fs::write(dir.join("AGENTS.md"), "guide").expect("target file");
        std::os::unix::fs::symlink("AGENTS.md", dir.join("CLAUDE.md")).expect("symlink");
        let root = super::super::fs::canonicalize_plain(&dir).expect("canonical temp dir");
        let root = root.to_string_lossy().to_string();

        // The target belongs to the label, beside the name. The description slot is right-aligned,
        // so a target there would sit at the far edge of the row instead.
        let label_of = |props: &FileTreeProps| {
            let state = FileTreeComponent::new().create_state(props);
            let projection = build_projection(props, &state, None);
            let row = projected_child(&projection.root, "CLAUDE.md");
            assert!(
                row.item.description_spans.is_empty(),
                "the target is not right-aligned metadata"
            );
            row.item
                .spans
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>()
        };

        let props = crate::widgets::file_tree::FileTree::new(root.clone()).props;
        assert!(
            label_of(&props).ends_with("CLAUDE.md → AGENTS.md"),
            "got {:?}",
            label_of(&props)
        );

        let arrow = crate::widgets::file_tree::FileTree::new(root.clone())
            .symlink_target_arrow("->")
            .props;
        assert!(label_of(&arrow).ends_with("CLAUDE.md -> AGENTS.md"));

        let off = crate::widgets::file_tree::FileTree::new(root.clone())
            .symlink_targets(false)
            .props;
        assert!(
            label_of(&off).ends_with("CLAUDE.md"),
            "an application can turn the target off"
        );

        // The file the link points at is an ordinary row and says nothing extra.
        let state = FileTreeComponent::new().create_state(&props);
        let projection = build_projection(&props, &state, None);
        let target_row = projected_child(&projection.root, "AGENTS.md");
        assert!(
            target_row
                .item
                .spans
                .iter()
                .all(|span| !span.content.contains('→'))
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A tree served by the application cannot have its links followed here, so the target rides
    /// along on the entry.
    #[test]
    fn a_provided_symlink_shows_the_target_the_application_supplied() {
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(
                    "/remote/repo",
                    [
                        super::super::FileTreeEntry::file("AGENTS.md"),
                        super::super::FileTreeEntry::file("CLAUDE.md").symlink_target("AGENTS.md"),
                    ],
                ),
            ]))
            .props;
        let state = FileTreeComponent::new().create_state(&props);
        let projection = build_projection(&props, &state, None);

        let label = projected_child(&projection.root, "CLAUDE.md")
            .item
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect::<String>();
        assert!(label.ends_with("CLAUDE.md → AGENTS.md"), "got {label:?}");
    }

    #[test]
    fn collapsed_directories_project_no_subtree_and_stay_expandable() {
        let props = collapsed_projection_props();
        let state = FileTreeComponent::new().create_state(&props);

        let projection = build_projection(&props, &state, None);
        let src = projected_child(&projection.root, "src");

        // Neither `src/widgets` nor `src/widgets/tree.rs` is materialized, and the row is still
        // expandable on its own rather than through a stand-in child.
        assert!(src.children.is_empty());
        assert!(src.is_collapsible());
        assert!(!src.expanded);

        // Nothing below a collapsed directory reaches the event lookup, because
        // none of it can be clicked while the parent stays closed.
        let deep = projection
            .lookup
            .values()
            .any(|entry| entry.path.as_ref() == "/remote/repo/src/widgets");
        assert!(!deep, "collapsed subtree must not populate the lookup");
    }

    /// An empty directory must be expandable whether or not it has been opened yet. A lazy tree
    /// cannot know a directory is empty until it is loaded, so deriving expandability from the
    /// loaded children would give the row an arrow, take it away the moment the user closed it
    /// again, and leave no way to reopen it.
    #[test]
    fn a_loaded_empty_directory_stays_expandable_in_both_states() {
        let props = collapsed_projection_props();
        let mut state = FileTreeComponent::new().create_state(&props);

        let projection = build_projection(&props, &state, None);
        let empty = projected_child(&projection.root, "empty");
        assert!(empty.children.is_empty());
        assert!(empty.is_collapsible());
        assert!(!empty.expanded);

        state.expanded.insert(Arc::from("/remote/repo/empty"));
        let projection = build_projection(&props, &state, None);
        let empty = projected_child(&projection.root, "empty");
        assert!(empty.children.is_empty());
        assert!(empty.is_collapsible());
        assert!(empty.expanded);
    }

    #[test]
    fn expanding_a_directory_materializes_its_real_children() {
        let props = collapsed_projection_props();
        let mut state = FileTreeComponent::new().create_state(&props);
        state.expanded.insert(Arc::from("/remote/repo/src"));

        let projection = build_projection(&props, &state, None);
        let src = projected_child(&projection.root, "src");
        let widgets = projected_child(src, "widgets");

        assert!(src.expanded);
        assert_eq!(src.children.len(), 1);
        // `widgets` is itself still collapsed, so its own subtree is not built.
        assert!(widgets.children.is_empty());
        assert!(widgets.is_collapsible());
        assert!(
            projection
                .lookup
                .values()
                .any(|entry| entry.path.as_ref() == "/remote/repo/src/widgets")
        );
    }

    /// The reported cycle, against the real filesystem: an empty directory opens, loads nothing,
    /// closes, and must still open. Loading is what used to strand it — before the first expand the
    /// directory is unread and therefore expandable, and afterwards it was judged by its (empty)
    /// children.
    #[test]
    fn an_empty_directory_survives_an_expand_collapse_cycle() {
        let dir = unique_component_test_dir("empty-cycle");
        std::fs::create_dir_all(dir.join("empty")).expect("create dirs");
        let root = dir.to_string_lossy().to_string();
        let empty_path: Arc<str> = Arc::from(format!("{root}/empty").as_str());

        let props = crate::widgets::file_tree::FileTree::new(root).props;
        let mut state = FileTreeComponent::new().create_state(&props);

        let expandable = |state: &FileTreeState| {
            projected_child(&build_projection(&props, state, None).root, "empty").is_collapsible()
        };
        assert!(expandable(&state), "unopened directory offers a toggle");

        // Expand: the directory is read and turns out to have nothing in it.
        state.expanded.insert(empty_path.clone());
        let expanded = state.expanded.clone();
        load_expanded_directories(&mut state, &props, expanded);
        assert!(
            node_by_path_mut(&mut state.root, empty_path.as_ref())
                .expect("empty node")
                .loaded
        );
        assert!(expandable(&state), "an open empty directory can be closed");

        // Collapse: knowing it is empty must not turn the row into a leaf.
        state.expanded.remove(&empty_path);
        assert!(expandable(&state), "a closed empty directory reopens");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A changed-only view with nothing changed is empty, not a heading over nothing — but only
    /// once it knows, and only in that view.
    #[test]
    fn changed_only_falls_back_to_empty_text_once_the_snapshot_settles() {
        let clean = crate::widgets::file_tree::FileTree::new("/repo")
            .change_view(FileTreeChangeView::ChangedOnly)
            .empty_text("No changes")
            .props;
        let mut state = test_state_with_root(test_root());
        let empty = |props: &FileTreeProps, state: &FileTreeState| {
            let projection = build_projection(props, state, None);
            changed_only_projection_is_empty(props, state, &projection)
        };

        assert!(empty(&clean, &state), "a clean tree says so");

        // Mid-`git status` the tree has no answer yet, so the heading stays rather than flashing a
        // claim it is about to contradict.
        state.git_snapshot_pending = true;
        assert!(!empty(&clean, &state));
        state.git_snapshot_pending = false;

        // With something changed there is a real projection to show.
        state.git_snapshot.changed_paths = vec![Arc::from("/repo/src/main.rs")];
        assert!(!empty(&clean, &state));
        state.git_snapshot.changed_paths.clear();

        // Browsing keeps its root row: that row is the directory you opened, not a heading.
        let browsing = crate::widgets::file_tree::FileTree::new("/repo")
            .empty_text("Directory is empty")
            .props;
        assert!(!empty(&browsing, &state));

        // An application that removed the placeholder text gets the old shape back.
        let mut silent = clean.clone();
        silent.empty_text = None;
        assert!(!empty(&silent, &state));
    }

    /// Folding the root shut hides the changed files; it does not make the tree clean. Reading
    /// emptiness off the rendered rows used to swap the whole view for "No changes" here, leaving
    /// no heading to click back open.
    #[test]
    fn changed_only_keeps_its_tree_when_the_root_is_collapsed() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .change_view(FileTreeChangeView::ChangedOnly)
            .empty_text("No changes")
            .props;
        let mut state = test_state_with_root(test_root());
        state.git_snapshot.changed_paths = vec![Arc::from("/repo/src/main.rs")];

        state.expanded.remove::<str>("/repo");
        let projection = build_projection(&props, &state, None);
        assert!(
            projection.root.children.is_empty(),
            "a collapsed root draws no rows"
        );
        assert!(
            !changed_only_projection_is_empty(&props, &state, &projection),
            "a collapsed root is not a clean tree"
        );

        // A filter that matches nothing is the user narrowing the same set, and still empty.
        let filter = ExplorerFilter::default();
        let filtered = build_projection(&props, &state, Some(&filter));
        assert!(changed_only_projection_is_empty(&props, &state, &filtered));
    }

    #[test]
    fn explorer_search_still_walks_collapsed_directories() {
        let props = collapsed_projection_props();
        let state = FileTreeComponent::new().create_state(&props);
        // Search visibility is decided per child, so a filtered walk cannot be
        // replaced by the placeholder shortcut.
        let filter_with = |paths: &[&str]| ExplorerFilter {
            visible_paths: paths.iter().copied().map(Arc::<str>::from).collect(),
            ..ExplorerFilter::default()
        };

        // A match below the closed `src` still has to be projected and resolvable,
        // otherwise search could not reveal it.
        let deep = filter_with(&["/remote/repo/src", "/remote/repo/src/widgets"]);
        let projection = build_projection(&props, &state, Some(&deep));
        let src = projected_child(&projection.root, "src");
        assert_eq!(src.children.len(), 1);
        assert!(
            projection
                .lookup
                .values()
                .any(|entry| entry.path.as_ref() == "/remote/repo/src/widgets"),
            "filtered projection must keep resolving matches below a closed directory"
        );

        // When the filter excludes every child, `src` must stay a leaf. The
        // placeholder shortcut would wrongly give it one child here, which is
        // exactly why the filtered path keeps the full walk.
        let shallow = filter_with(&["/remote/repo/src"]);
        let projection = build_projection(&props, &state, Some(&shallow));
        assert!(projected_child(&projection.root, "src").children.is_empty());
    }

    #[test]
    fn provided_path_resolution_rejects_escapes_and_prefix_siblings() {
        let root = ProvidedPath::root("/remote/repo");

        assert!(root.resolve("../etc").is_none());
        assert!(root.resolve("src/../../etc").is_none());
        assert!(root.resolve("/etc/passwd").is_none());
        assert!(root.resolve("/remote/repo2/file").is_none());
        assert_eq!(
            root.resolve("src/../README.md")
                .map(|path| path.as_str().to_owned()),
            Some("/remote/repo/README.md".to_owned())
        );

        for invalid in ["../x", "a/b", "/abs", "..", ".", ""] {
            assert!(!root.is_valid_name(invalid), "accepted {invalid:?}");
        }
        assert!(root.is_valid_name("main.rs"));
    }

    #[test]
    fn explicit_provided_changes_override_entry_embedded_status() {
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(
                    ".",
                    [super::super::FileTreeEntry::file("main.rs").git_status(
                        super::super::GitFileStatus::new(
                            None,
                            Some(super::super::GitChangeState::Modified),
                        ),
                    )],
                ),
            ]))
            .change_source(FileTreeChangeSource::provided([
                super::super::FileTreeChange::new(
                    "main.rs",
                    super::super::FileTreeChangeStatus::Added,
                ),
            ]))
            .props;

        let snapshot = effective_initial_snapshot(&props);
        let decoration = snapshot.entries.get("/remote/repo/main.rs").unwrap();
        assert_eq!(
            decoration.status.unstaged,
            Some(super::super::GitChangeState::Added)
        );
        assert_eq!(snapshot.changed_paths, [Arc::from("/remote/repo/main.rs")]);
    }

    #[test]
    fn props_hook_rebuilds_a_changed_local_root() {
        let old_root = unique_component_test_dir("props-hook-old-root");
        let new_root = unique_component_test_dir("props-hook-new-root");
        std::fs::create_dir_all(&old_root).unwrap();
        std::fs::create_dir_all(&new_root).unwrap();
        std::fs::write(old_root.join("old.rs"), "").unwrap();
        std::fs::write(new_root.join("new.rs"), "").unwrap();
        let old_props =
            crate::widgets::file_tree::FileTree::new(old_root.to_string_lossy().into_owned()).props;
        let new_props =
            crate::widgets::file_tree::FileTree::new(new_root.to_string_lossy().into_owned()).props;
        let mut runtime = RuntimeCore::new_test(
            FileTreeComponent::new(),
            old_props,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(std::cell::Cell::new(false)),
        );

        let old_props = std::mem::replace(&mut runtime.ctx.props, new_props);
        let update = runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx);

        assert!(update.dirty);
        assert!(
            runtime
                .ctx
                .state
                .root
                .children
                .iter()
                .any(|child| child.name.as_ref() == "new.rs")
        );
        assert!(
            runtime
                .ctx
                .state
                .root
                .children
                .iter()
                .all(|child| child.name.as_ref() != "old.rs")
        );

        let _ = std::fs::remove_dir_all(old_root);
        let _ = std::fs::remove_dir_all(new_root);
    }

    #[test]
    fn entry_refresh_command_builds_a_fresh_snapshot_for_any_token_change() {
        let dir = unique_component_test_dir("entry-refresh-root");
        std::fs::create_dir_all(dir.join("expanded")).unwrap();
        std::fs::write(dir.join("before.rs"), "").unwrap();
        std::fs::write(dir.join("expanded/before.rs"), "").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let mut runtime = file_tree_runtime(
            crate::widgets::file_tree::FileTree::new(root)
                .initial_expanded_paths(["expanded"])
                .props,
        );
        let expanded = runtime.ctx.state.expanded.clone();
        std::fs::write(dir.join("after.rs"), "").unwrap();

        let result = run_local_refresh_command(&mut runtime, 7, 2, expanded);

        assert_eq!(result.generation, 7);
        assert_eq!(result.token, 2);
        assert_eq!(result.options, (false, 2_000));
        assert!(result.snapshot.loaded);
        assert!(
            result
                .snapshot
                .children
                .iter()
                .any(|child| { child.name.as_ref() == "after.rs" })
        );
        assert!(
            result
                .snapshot
                .children
                .iter()
                .all(|child| { child.name.as_ref() != "before.rs" || child.loaded })
        );
        assert!(result.snapshot.children.iter().any(|child| {
            child.name.as_ref() == "expanded"
                && child.loaded
                && child
                    .children
                    .iter()
                    .any(|entry| entry.name.as_ref() == "before.rs")
        }));

        let _ = std::fs::remove_dir_all(dir);
    }

    /// Run `command` and collect what it and everything already queued send, up to and including
    /// the first message `done` accepts.
    fn run_command_until(
        runtime: &mut RuntimeCore<FileTreeComponent>,
        command: Command,
        done: impl FnMut(&FileTreeMsg) -> bool,
    ) -> Vec<FileTreeMsg> {
        command.run(crate::core::component::CommandRuntime {
            scope: crate::callback::ScopeId(1),
            tx: runtime.command_tx.clone(),
            runtime_id: runtime.ctx.env().runtime_id,
            now: runtime.ctx.env().now(),
            clock_mode: runtime.ctx.env().clock.mode(),
            clock: runtime.ctx.env().clock.clone(),
            activity: std::sync::Arc::clone(&runtime.ctx.env().activity),
        });
        collect_messages_until(runtime, done)
    }

    fn collect_messages_until(
        runtime: &mut RuntimeCore<FileTreeComponent>,
        mut done: impl FnMut(&FileTreeMsg) -> bool,
    ) -> Vec<FileTreeMsg> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let mut messages = Vec::new();
        loop {
            runtime.drain_commands();
            while let Some(message) = runtime.queue.borrow_mut().pop_front().map(|(_, message)| {
                *message
                    .downcast::<FileTreeMsg>()
                    .expect("expected a FileTree message")
            }) {
                let finished = done(&message);
                messages.push(message);
                if finished {
                    return messages;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the expected message never arrived; got {messages:?}"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn merging_a_reread_keeps_open_subdirectories_open() {
        let mut root = test_root();
        let result = super::super::fs::DirectoryLoadResult {
            entries: vec![
                super::super::fs::LoadedEntry {
                    name: Arc::from("new.rs"),
                    path: Arc::from("/repo/new.rs"),
                    kind: FileKind::File,
                },
                super::super::fs::LoadedEntry {
                    name: Arc::from("src"),
                    path: Arc::from("/repo/src"),
                    kind: FileKind::Directory,
                },
            ],
            omitted: 0,
            error: None,
        };

        merge_directory_load(&mut root, result);

        let names: Vec<&str> = root
            .children
            .iter()
            .map(|child| child.name.as_ref())
            .collect();
        assert_eq!(names, ["new.rs", "src"]);
        let src = node_by_path_mut(&mut root, "/repo/src").unwrap();
        assert!(
            src.loaded,
            "rereading the parent must not unload an open child"
        );
        assert_eq!(src.children[0].name.as_ref(), "main.rs");
    }

    #[test]
    fn only_a_polling_or_watching_tree_arms_a_periodic_reread() {
        let manual = file_tree_runtime(crate::widgets::file_tree::FileTree::new("/repo").props);
        assert_eq!(
            refresh_interval(&manual.ctx.props, &manual.ctx.state.watch),
            None
        );

        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .refresh_policy(FileTreeRefreshPolicy::Polling(std::time::Duration::ZERO))
            .props;
        assert_eq!(
            refresh_interval(&props, &WatchState::default()),
            Some(watch::MIN_POLL_INTERVAL),
            "a zero interval must not spin"
        );

        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .refresh_policy(FileTreeRefreshPolicy::Watch)
            .props;
        let mut watch_state = WatchState::default();
        assert_eq!(
            refresh_interval(&props, &watch_state),
            Some(watch::REPAIR_INTERVAL)
        );
        watch_state.failed.insert(Arc::from("/mnt/nfs"));
        assert_eq!(
            refresh_interval(&props, &watch_state),
            Some(watch::DEGRADED_INTERVAL),
            "an unwatchable directory falls back to polling"
        );
    }

    /// Changes made while a directory was collapsed went unwatched, so reopening it must not trust
    /// the listing it had before. Needs no watcher: the reread comes from it becoming visible.
    #[test]
    fn reopening_a_collapsed_directory_rereads_it() {
        let dir = unique_component_test_dir("watch-reopen");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/before.rs"), "").unwrap();
        let props = crate::widgets::file_tree::FileTree::new(dir.to_string_lossy().into_owned())
            .initial_expanded_paths(["src"])
            .refresh_policy(FileTreeRefreshPolicy::Watch)
            .props;
        let mut runtime = file_tree_runtime(props);
        let src: Arc<str> = Arc::from(
            Path::new(runtime.ctx.state.root.path.as_ref())
                .join("src")
                .to_string_lossy()
                .as_ref(),
        );
        let toggle = |expanded| FileTreeMsg::TreeToggled {
            entry: Some(VisibleFileTreeEntry {
                path: src.clone(),
                kind: FileKind::Directory,
            }),
            expanded,
        };

        runtime.component.update(toggle(false), &mut runtime.ctx);
        std::fs::remove_file(dir.join("src/before.rs")).unwrap();
        std::fs::write(dir.join("src/after.rs"), "").unwrap();
        runtime.component.update(toggle(true), &mut runtime.ctx);

        let reload = collect_messages_until(&mut runtime, |message| {
            matches!(message, FileTreeMsg::WatchReload { .. })
        })
        .pop()
        .unwrap();
        let reload = runtime.component.update(reload, &mut runtime.ctx);
        let loaded = run_command_until(
            &mut runtime,
            reload.command.expect("the reopened directory is reread"),
            |message| matches!(message, FileTreeMsg::WatchDirectoriesLoaded { .. }),
        )
        .pop()
        .unwrap();
        runtime.component.update(loaded, &mut runtime.ctx);

        let names: Vec<String> = node_by_path_mut(&mut runtime.ctx.state.root, &src)
            .unwrap()
            .children
            .iter()
            .map(|child| child.name.to_string())
            .collect();
        assert_eq!(names, ["after.rs"]);

        let _ = std::fs::remove_dir_all(dir);
    }

    /// The same reopen, driven by an application that controls expansion through props.
    #[test]
    fn reopening_a_directory_through_controlled_expansion_rereads_it() {
        let dir = unique_component_test_dir("watch-reopen-controlled");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/before.rs"), "").unwrap();
        let root = dunce::canonicalize(&dir).unwrap();
        let root_path = root.to_string_lossy().into_owned();
        let src: Arc<str> = Arc::from(root.join("src").to_string_lossy().as_ref());
        let props = |expanded: &[&Arc<str>]| {
            crate::widgets::file_tree::FileTree::new(root_path.clone())
                .expanded_paths(expanded.iter().map(|path| Arc::clone(path)))
                .refresh_policy(FileTreeRefreshPolicy::Watch)
                .props
        };
        let mut runtime = file_tree_runtime(props(&[&src]));

        replace_file_tree_props(&mut runtime, props(&[]));
        std::fs::remove_file(dir.join("src/before.rs")).unwrap();
        std::fs::write(dir.join("src/after.rs"), "").unwrap();
        replace_file_tree_props(&mut runtime, props(&[&src]));

        let reload = collect_messages_until(&mut runtime, |message| {
            matches!(message, FileTreeMsg::WatchReload { .. })
        })
        .pop()
        .unwrap();
        let reload = runtime.component.update(reload, &mut runtime.ctx);
        let loaded = run_command_until(
            &mut runtime,
            reload.command.expect("the reopened directory is reread"),
            |message| matches!(message, FileTreeMsg::WatchDirectoriesLoaded { .. }),
        )
        .pop()
        .unwrap();
        runtime.component.update(loaded, &mut runtime.ctx);

        let names: Vec<String> = node_by_path_mut(&mut runtime.ctx.state.root, &src)
            .unwrap()
            .children
            .iter()
            .map(|child| child.name.to_string())
            .collect();
        assert_eq!(names, ["after.rs"]);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[cfg(all(feature = "file-tree-watch", not(target_arch = "wasm32")))]
    #[test]
    fn a_watching_tree_shows_a_new_file_and_drops_collapsed_watches() {
        let dir = unique_component_test_dir("watch-new-file");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("before.rs"), "").unwrap();
        let props = crate::widgets::file_tree::FileTree::new(dir.to_string_lossy().into_owned())
            .initial_expanded_paths(["sub"])
            .refresh_policy(FileTreeRefreshPolicy::Watch)
            .props;
        let mut runtime = file_tree_runtime(props);

        let restart = runtime
            .component
            .update(FileTreeMsg::RefreshPolicyRestart, &mut runtime.ctx);
        let started = run_command_until(
            &mut runtime,
            restart.command.expect("watching starts a watcher"),
            |message| matches!(message, FileTreeMsg::WatchStarted { .. }),
        )
        .pop()
        .unwrap();
        runtime.component.update(started, &mut runtime.ctx);
        let root = runtime.ctx.state.root.path.clone();
        let sub: Arc<str> = Arc::from(
            Path::new(root.as_ref())
                .join("sub")
                .to_string_lossy()
                .as_ref(),
        );
        assert!(runtime.ctx.state.watch.watched.contains(&root));
        assert!(runtime.ctx.state.watch.watched.contains(&sub));

        std::fs::write(dir.join("after.rs"), "").unwrap();
        collect_messages_until(&mut runtime, |message| {
            matches!(message, FileTreeMsg::WatchDirty { .. })
        });
        // The batch window is a timer; close it directly instead of waiting it out.
        let generation = runtime.ctx.state.watch.generation;
        let flush = runtime
            .component
            .update(FileTreeMsg::WatchFlush { generation }, &mut runtime.ctx);
        let loaded = run_command_until(
            &mut runtime,
            flush.command.expect("a new file rereads its directory"),
            |message| matches!(message, FileTreeMsg::WatchDirectoriesLoaded { .. }),
        )
        .pop()
        .unwrap();
        runtime.component.update(loaded, &mut runtime.ctx);
        assert!(
            runtime
                .ctx
                .state
                .root
                .children
                .iter()
                .any(|child| child.name.as_ref() == "after.rs")
        );

        runtime.ctx.state.expanded.remove(&sub);
        reconcile_watches(&mut runtime.ctx.state, &runtime.ctx.props);
        assert!(!runtime.ctx.state.watch.watched.contains(&sub));
        assert!(runtime.ctx.state.watch.watched.contains(&root));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn entry_refresh_rebuilds_a_deleted_and_recreated_expanded_directory() {
        let dir = unique_component_test_dir("entry-refresh-recreated-dir");
        let expanded_dir = dir.join("expanded");
        std::fs::create_dir_all(&expanded_dir).unwrap();
        std::fs::write(expanded_dir.join("before.rs"), "").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let props = crate::widgets::file_tree::FileTree::new(root.clone())
            .initial_expanded_paths(["expanded"])
            .props;
        let mut runtime = file_tree_runtime(props);
        let expanded_before = runtime.ctx.state.expanded.clone();
        let expanded_path = Arc::<str>::from(
            Path::new(runtime.ctx.state.root.path.as_ref())
                .join("expanded")
                .to_string_lossy()
                .as_ref(),
        );

        std::fs::remove_dir_all(&expanded_dir).unwrap();
        std::fs::create_dir_all(&expanded_dir).unwrap();
        std::fs::write(expanded_dir.join("after.rs"), "").unwrap();

        replace_file_tree_props(
            &mut runtime,
            crate::widgets::file_tree::FileTree::new(root)
                .entry_refresh_token(1)
                .props,
        );
        let request = runtime
            .component
            .update(FileTreeMsg::RequestEntryRefresh(1), &mut runtime.ctx);
        assert!(request.command.is_some());
        let result = local_refresh_result(
            &runtime.ctx.state,
            &runtime.ctx.props,
            runtime.ctx.state.entry_refresh_generation,
            1,
        );
        runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(result), &mut runtime.ctx);

        assert_eq!(runtime.ctx.state.expanded, expanded_before);
        assert!(
            node_by_path_mut(&mut runtime.ctx.state.root, &expanded_path).is_some_and(|node| {
                node.loaded
                    && node.children.len() == 1
                    && node.children[0].name.as_ref() == "after.rs"
            })
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn entry_refresh_restarts_when_options_change_in_flight() {
        let dir = unique_component_test_dir("entry-refresh-options");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("visible.rs"), "").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let mut runtime =
            file_tree_runtime(crate::widgets::file_tree::FileTree::new(root.clone()).props);

        replace_file_tree_props(
            &mut runtime,
            crate::widgets::file_tree::FileTree::new(root.clone())
                .entry_refresh_token(1)
                .props,
        );
        let first = runtime
            .component
            .update(FileTreeMsg::RequestEntryRefresh(1), &mut runtime.ctx);
        assert!(first.command.is_some());
        let stale_generation = runtime.ctx.state.entry_refresh_generation;
        let stale =
            local_refresh_result(&runtime.ctx.state, &runtime.ctx.props, stale_generation, 1);

        std::fs::write(dir.join(".hidden.rs"), "").unwrap();
        let options_update = replace_file_tree_props(
            &mut runtime,
            crate::widgets::file_tree::FileTree::new(root)
                .show_hidden(true)
                .max_entries_per_dir(3)
                .entry_refresh_token(1)
                .props,
        );
        assert!(options_update.dirty);
        let _ = runtime.component.view(&runtime.ctx);
        let (_, message) = runtime
            .queue
            .borrow_mut()
            .pop_front()
            .expect("options change should queue a refresh");
        let request = *message
            .downcast::<FileTreeMsg>()
            .expect("expected a FileTree message");
        std::assert_matches!(request, FileTreeMsg::RequestEntryRefresh(1));
        let second = runtime.component.update(request, &mut runtime.ctx);
        assert!(second.command.is_some());
        assert_ne!(runtime.ctx.state.entry_refresh_generation, stale_generation);

        let ignored = runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(stale), &mut runtime.ctx);
        assert!(!ignored.dirty);
        assert!(runtime.ctx.state.entry_refresh_pending);

        let fresh = local_refresh_result(
            &runtime.ctx.state,
            &runtime.ctx.props,
            runtime.ctx.state.entry_refresh_generation,
            1,
        );
        runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(fresh), &mut runtime.ctx);
        assert!(!runtime.ctx.state.entry_refresh_pending);
        assert_eq!(runtime.ctx.state.entry_refresh_completed_token, 1);
        assert!(
            runtime
                .ctx
                .state
                .root
                .children
                .iter()
                .any(|child| { child.name.as_ref() == ".hidden.rs" })
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn entry_refresh_restarts_when_expansion_changes_in_flight() {
        let dir = unique_component_test_dir("entry-refresh-expansion-race");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::write(dir.join("src/new.rs"), "").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let mut runtime =
            file_tree_runtime(crate::widgets::file_tree::FileTree::new(root.clone()).props);

        replace_file_tree_props(
            &mut runtime,
            crate::widgets::file_tree::FileTree::new(root)
                .entry_refresh_token(1)
                .props,
        );
        runtime
            .component
            .update(FileTreeMsg::RequestEntryRefresh(1), &mut runtime.ctx);
        let stale_generation = runtime.ctx.state.entry_refresh_generation;
        let stale =
            local_refresh_result(&runtime.ctx.state, &runtime.ctx.props, stale_generation, 1);
        let src = Arc::<str>::from(
            Path::new(runtime.ctx.state.root.path.as_ref())
                .join("src")
                .to_string_lossy()
                .as_ref(),
        );
        runtime.component.update(
            FileTreeMsg::TreeToggled {
                entry: Some(VisibleFileTreeEntry {
                    path: src.clone(),
                    kind: FileKind::Directory,
                }),
                expanded: true,
            },
            &mut runtime.ctx,
        );

        let restart = runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(stale), &mut runtime.ctx);
        assert!(restart.command.is_some());
        assert!(runtime.ctx.state.entry_refresh_pending);
        assert_ne!(runtime.ctx.state.entry_refresh_generation, stale_generation);

        let fresh = local_refresh_result(
            &runtime.ctx.state,
            &runtime.ctx.props,
            runtime.ctx.state.entry_refresh_generation,
            1,
        );
        runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(fresh), &mut runtime.ctx);
        assert!(
            node_by_path_mut(&mut runtime.ctx.state.root, &src).is_some_and(|node| {
                node.loaded
                    && node
                        .children
                        .iter()
                        .any(|child| child.name.as_ref() == "new.rs")
            })
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_entry_refresh_completion_is_rejected_after_a_newer_token() {
        let dir = unique_component_test_dir("entry-refresh-stale");
        std::fs::create_dir_all(&dir).unwrap();
        let root = dir.to_string_lossy().into_owned();
        let props = crate::widgets::file_tree::FileTree::new(root.clone()).props;
        let mut runtime = file_tree_runtime(props);

        std::fs::write(dir.join("first.rs"), "").unwrap();
        let first_props = crate::widgets::file_tree::FileTree::new(root.clone())
            .entry_refresh_token(1)
            .props;
        let old_props = std::mem::replace(&mut runtime.ctx.props, first_props);
        runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx);
        let first = runtime
            .component
            .update(FileTreeMsg::RequestEntryRefresh(1), &mut runtime.ctx);
        assert!(first.command.is_some());
        let stale = local_refresh_result(
            &runtime.ctx.state,
            &runtime.ctx.props,
            runtime.ctx.state.entry_refresh_generation,
            1,
        );

        std::fs::write(dir.join("second.rs"), "").unwrap();
        let second_props = crate::widgets::file_tree::FileTree::new(root)
            .entry_refresh_token(2)
            .props;
        let old_props = std::mem::replace(&mut runtime.ctx.props, second_props);
        runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx);
        let second = runtime
            .component
            .update(FileTreeMsg::RequestEntryRefresh(2), &mut runtime.ctx);
        assert!(second.command.is_some());

        let ignored = runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(stale), &mut runtime.ctx);
        assert!(!ignored.dirty);
        assert!(runtime.ctx.state.root.children.iter().all(|child| {
            child.name.as_ref() != "first.rs" && child.name.as_ref() != "second.rs"
        }));

        let fresh = local_refresh_result(
            &runtime.ctx.state,
            &runtime.ctx.props,
            runtime.ctx.state.entry_refresh_generation,
            2,
        );
        runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(fresh), &mut runtime.ctx);
        assert!(
            runtime
                .ctx
                .state
                .root
                .children
                .iter()
                .any(|child| { child.name.as_ref() == "second.rs" })
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn stale_entry_refresh_completion_is_rejected_after_a_root_change() {
        let dir = unique_component_test_dir("entry-refresh-root-stale");
        let first_root = dir.join("first");
        let second_root = dir.join("second");
        std::fs::create_dir_all(&first_root).unwrap();
        std::fs::create_dir_all(&second_root).unwrap();
        std::fs::write(first_root.join("first.rs"), "").unwrap();
        std::fs::write(second_root.join("second.rs"), "").unwrap();
        let first = first_root.to_string_lossy().into_owned();
        let second = second_root.to_string_lossy().into_owned();
        let props = crate::widgets::file_tree::FileTree::new(first.clone()).props;
        let mut runtime = file_tree_runtime(props);

        let next_props = crate::widgets::file_tree::FileTree::new(first.clone())
            .entry_refresh_token(1)
            .props;
        let old_props = std::mem::replace(&mut runtime.ctx.props, next_props);
        runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx);
        runtime
            .component
            .update(FileTreeMsg::RequestEntryRefresh(1), &mut runtime.ctx);
        let stale = local_refresh_result(
            &runtime.ctx.state,
            &runtime.ctx.props,
            runtime.ctx.state.entry_refresh_generation,
            1,
        );

        let next_props = crate::widgets::file_tree::FileTree::new(second.clone())
            .entry_refresh_token(1)
            .props;
        let old_props = std::mem::replace(&mut runtime.ctx.props, next_props);
        runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx);
        let ignored = runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(stale), &mut runtime.ctx);

        assert!(!ignored.dirty);
        assert_eq!(runtime.ctx.state.root.path.as_ref(), second.as_str());
        assert!(
            runtime
                .ctx
                .state
                .root
                .children
                .iter()
                .any(|child| { child.name.as_ref() == "second.rs" })
        );
        assert!(
            runtime
                .ctx
                .state
                .root
                .children
                .iter()
                .all(|child| { child.name.as_ref() != "first.rs" })
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn explorer_results_install_worker_listings_and_replace_the_previous_expansion() {
        let dir = unique_component_test_dir("explorer-results-listings");
        std::fs::create_dir_all(dir.join("src")).unwrap();
        std::fs::create_dir_all(dir.join("other")).unwrap();
        std::fs::write(dir.join("src").join("on_disk.rs"), "").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let mut runtime = file_tree_runtime(
            crate::widgets::file_tree::FileTree::new(root.clone())
                .explorer(true)
                .props,
        );
        let root_path = runtime.ctx.state.root.path.clone();
        let src: Arc<str> = Arc::from(format!("{root_path}/src"));
        let other: Arc<str> = Arc::from(format!("{root_path}/other"));
        let listed: Arc<str> = Arc::from(format!("{src}/only_in_listing.rs"));
        runtime.ctx.state.explorer_input.set_text("listing");
        runtime.ctx.state.search_expanded_snapshot = Some(runtime.ctx.state.expanded.clone());
        runtime.ctx.state.explorer_query_id = 1;

        // A listing the disk does not have shows the tree took the worker's copy instead of
        // reading the directory itself.
        let root_listing = read_directory(root_path.as_ref(), false, 2_000);
        let src_listing = DirectoryLoadResult {
            entries: vec![super::super::fs::LoadedEntry {
                name: Arc::from("only_in_listing.rs"),
                path: listed.clone(),
                kind: FileKind::File,
            }],
            omitted: 0,
            error: None,
        };
        runtime.component.update(
            FileTreeMsg::ExplorerResultsReady {
                query_id: 1,
                filter: ExplorerFilter {
                    visible_paths: [root_path.clone(), src.clone(), listed.clone()]
                        .into_iter()
                        .collect(),
                    expanded_paths: [root_path.clone(), src.clone()].into_iter().collect(),
                    match_count: 1,
                    shown_count: 1,
                    ..ExplorerFilter::default()
                },
                listings: vec![
                    (root_path.clone(), root_listing),
                    (src.clone(), src_listing),
                ],
            },
            &mut runtime.ctx,
        );
        let src_node = node_by_path_mut(&mut runtime.ctx.state.root, &src).unwrap();
        let names: Vec<&str> = src_node.children.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, ["only_in_listing.rs"]);
        assert!(runtime.ctx.state.expanded.contains(&src));

        runtime.ctx.state.explorer_query_id = 2;
        runtime.component.update(
            FileTreeMsg::ExplorerResultsReady {
                query_id: 2,
                filter: ExplorerFilter {
                    visible_paths: [root_path.clone(), other.clone()].into_iter().collect(),
                    expanded_paths: [root_path.clone(), other.clone()].into_iter().collect(),
                    match_count: 1,
                    shown_count: 1,
                    ..ExplorerFilter::default()
                },
                listings: Vec::new(),
            },
            &mut runtime.ctx,
        );
        assert!(!runtime.ctx.state.expanded.contains(&src));
        assert!(runtime.ctx.state.expanded.contains(&other));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn changed_ignore_rules_rerun_the_search_against_a_new_walk() {
        let dir = unique_component_test_dir("explorer-ignore-rules");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("build.log"), "").unwrap();
        let mut runtime = file_tree_runtime(
            crate::widgets::file_tree::FileTree::new(dir.to_string_lossy().into_owned())
                .explorer(true)
                .props,
        );
        let search = |runtime: &mut RuntimeCore<FileTreeComponent>, command: Command| {
            let ready = run_command_until(runtime, command, |message| {
                matches!(message, FileTreeMsg::ExplorerResultsReady { .. })
            })
            .pop()
            .unwrap();
            runtime.component.update(ready, &mut runtime.ctx);
            runtime.ctx.state.explorer_filter.match_count
        };
        let type_query = |runtime: &mut RuntimeCore<FileTreeComponent>, query: &str| {
            runtime
                .component
                .update(
                    FileTreeMsg::ExplorerQueryChanged(InputEvent {
                        value: Arc::from(query),
                        cursor: query.len(),
                        anchor: None,
                    }),
                    &mut runtime.ctx,
                )
                .command
                .expect("a query starts a search")
        };

        let command = type_query(&mut runtime, "build");
        assert_eq!(search(&mut runtime, command), 1);

        // Until the watcher reports the edit, later keystrokes filter the walk already taken.
        std::fs::write(dir.join(".gitignore"), "*.log\n").unwrap();
        let command = type_query(&mut runtime, "build.");
        assert_eq!(search(&mut runtime, command), 1);

        let walked = Arc::clone(&runtime.ctx.state.explorer_index);
        let restart = runtime
            .component
            .update(FileTreeMsg::ExplorerIgnoreRulesChanged, &mut runtime.ctx);
        assert!(!Arc::ptr_eq(&walked, &runtime.ctx.state.explorer_index));
        let command = restart.command.expect("an active query searches again");
        assert_eq!(search(&mut runtime, command), 0);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_reread_that_finds_nothing_new_keeps_the_explorer_walk() {
        let dir = unique_component_test_dir("explorer-index-watch-reread");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("before.rs"), "").unwrap();
        let mut runtime = file_tree_runtime(
            crate::widgets::file_tree::FileTree::new(dir.to_string_lossy().into_owned())
                .explorer(true)
                .props,
        );
        let expanded = runtime.ctx.state.expanded.clone();
        load_expanded_directories(&mut runtime.ctx.state, &runtime.ctx.props, expanded);
        let reread = |runtime: &mut RuntimeCore<FileTreeComponent>| {
            let root = runtime.ctx.state.root.path.clone();
            let options = entry_refresh_options(&runtime.ctx.props);
            let listing = read_directory(root.as_ref(), options.0, options.1);
            let message = FileTreeMsg::WatchDirectoriesLoaded {
                generation: runtime.ctx.state.watch.generation,
                entry_generation: runtime.ctx.state.entry_refresh_generation,
                root: root.clone(),
                options,
                results: vec![(root, listing)],
            };
            runtime.component.update(message, &mut runtime.ctx);
        };

        let walked = Arc::clone(&runtime.ctx.state.explorer_index);
        reread(&mut runtime);
        assert!(Arc::ptr_eq(&walked, &runtime.ctx.state.explorer_index));

        std::fs::write(dir.join("after.rs"), "").unwrap();
        reread(&mut runtime);
        assert!(!Arc::ptr_eq(&walked, &runtime.ctx.state.explorer_index));

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn typing_a_query_searches_on_a_worker_and_shows_the_match() {
        let dir = unique_component_test_dir("explorer-query-worker");
        std::fs::create_dir_all(dir.join("src").join("nested")).unwrap();
        std::fs::write(dir.join("src").join("nested").join("needle.rs"), "").unwrap();
        std::fs::write(dir.join("other.rs"), "").unwrap();
        let mut runtime = file_tree_runtime(
            crate::widgets::file_tree::FileTree::new(dir.to_string_lossy().into_owned())
                .explorer(true)
                .props,
        );
        let root_path = runtime.ctx.state.root.path.clone();
        let nested: Arc<str> = Arc::from(format!("{root_path}/src/nested"));

        let typed = runtime.component.update(
            FileTreeMsg::ExplorerQueryChanged(InputEvent {
                value: Arc::from("needle"),
                cursor: 6,
                anchor: None,
            }),
            &mut runtime.ctx,
        );
        let ready = run_command_until(
            &mut runtime,
            typed.command.expect("a query starts a search"),
            |message| matches!(message, FileTreeMsg::ExplorerResultsReady { .. }),
        )
        .pop()
        .unwrap();
        runtime.component.update(ready, &mut runtime.ctx);

        assert_eq!(runtime.ctx.state.explorer_filter.match_count, 1);
        assert!(runtime.ctx.state.expanded.contains(&nested));
        let nested_node = node_by_path_mut(&mut runtime.ctx.state.root, &nested).unwrap();
        assert!(nested_node.loaded);

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn explorer_match_counter_shows_the_cap_only_when_it_applies() {
        let filter = |match_count, shown_count| ExplorerFilter {
            match_count,
            shown_count,
            ..ExplorerFilter::default()
        };
        assert_eq!(explorer_match_count_label(&filter(12, 12)), "12");
        assert_eq!(
            explorer_match_count_label(&filter(5_000, 1_000)),
            "1000/5000"
        );
    }

    #[test]
    fn active_explorer_refresh_preserves_then_remaps_selection() {
        let dir = unique_component_test_dir("entry-refresh-explorer-selection");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.rs"), "").unwrap();
        std::fs::write(dir.join("b.rs"), "").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let selected = Arc::<str>::from(format!("{root}/b.rs"));
        let mut runtime = file_tree_runtime(
            crate::widgets::file_tree::FileTree::new(root.clone())
                .explorer(true)
                .props,
        );
        runtime.component.update(
            FileTreeMsg::TreeSelected {
                entry: Some(VisibleFileTreeEntry {
                    path: selected.clone(),
                    kind: FileKind::File,
                }),
            },
            &mut runtime.ctx,
        );
        runtime.ctx.state.explorer_input.set_text(".rs");
        runtime.ctx.state.explorer_query_id = 10;
        std::fs::write(dir.join("aa.rs"), "").unwrap();

        replace_file_tree_props(
            &mut runtime,
            crate::widgets::file_tree::FileTree::new(root.clone())
                .explorer(true)
                .entry_refresh_token(1)
                .props,
        );
        let request = runtime
            .component
            .update(FileTreeMsg::RequestEntryRefresh(1), &mut runtime.ctx);
        assert!(request.command.is_some());
        assert_eq!(runtime.ctx.state.explorer_query_id, 11);
        let stale = runtime.component.update(
            FileTreeMsg::ExplorerResultsReady {
                query_id: 10,
                filter: ExplorerFilter::default(),
                listings: Vec::new(),
            },
            &mut runtime.ctx,
        );
        assert!(!stale.dirty);

        let result = local_refresh_result(
            &runtime.ctx.state,
            &runtime.ctx.props,
            runtime.ctx.state.entry_refresh_generation,
            1,
        );
        let restart = runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(result), &mut runtime.ctx);
        assert!(restart.command.is_some());
        assert_eq!(
            runtime.ctx.state.uncontrolled_selected_path,
            Some(selected.clone())
        );
        assert_eq!(runtime.ctx.state.explorer_query_id, 12);

        let current = runtime.component.update(
            FileTreeMsg::ExplorerResultsReady {
                query_id: 12,
                filter: ExplorerFilter {
                    visible_paths: [
                        runtime.ctx.state.root.path.clone(),
                        Arc::from(format!("{root}/aa.rs")),
                        selected.clone(),
                    ]
                    .into_iter()
                    .collect(),
                    ..ExplorerFilter::default()
                },
                listings: Vec::new(),
            },
            &mut runtime.ctx,
        );
        assert!(current.dirty);
        assert_eq!(
            runtime.ctx.state.uncontrolled_selected_path,
            Some(selected.clone())
        );

        std::fs::remove_file(dir.join("b.rs")).unwrap();
        replace_file_tree_props(
            &mut runtime,
            crate::widgets::file_tree::FileTree::new(root)
                .explorer(true)
                .entry_refresh_token(2)
                .props,
        );
        runtime
            .component
            .update(FileTreeMsg::RequestEntryRefresh(2), &mut runtime.ctx);
        let result = local_refresh_result(
            &runtime.ctx.state,
            &runtime.ctx.props,
            runtime.ctx.state.entry_refresh_generation,
            2,
        );
        runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(result), &mut runtime.ctx);
        assert_eq!(runtime.ctx.state.uncontrolled_selected_path, Some(selected));
        assert_eq!(runtime.ctx.state.explorer_query_id, 14);

        runtime.component.update(
            FileTreeMsg::ExplorerResultsReady {
                query_id: 14,
                filter: ExplorerFilter {
                    visible_paths: [runtime.ctx.state.root.path.clone()].into_iter().collect(),
                    ..ExplorerFilter::default()
                },
                listings: Vec::new(),
            },
            &mut runtime.ctx,
        );
        assert_eq!(
            runtime.ctx.state.uncontrolled_selected_path,
            Some(runtime.ctx.state.root.path.clone())
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn provided_entries_ignore_entry_refresh_token() {
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(".", [super::super::FileTreeEntry::file("old.rs")]),
            ]))
            .entry_refresh_token(4)
            .props;
        let mut runtime = file_tree_runtime(props);

        let update = runtime
            .component
            .update(FileTreeMsg::RequestEntryRefresh(5), &mut runtime.ctx);

        assert!(!update.dirty);
        assert_eq!(runtime.ctx.state.entry_refresh_completed_token, 4);
        assert_eq!(runtime.ctx.state.root.children.len(), 1);
        assert_eq!(runtime.ctx.state.root.children[0].name.as_ref(), "old.rs");

        let stale = LocalEntryRefreshResult {
            generation: runtime.ctx.state.entry_refresh_generation,
            token: 4,
            root: runtime.ctx.state.root.path.clone(),
            snapshot: runtime.ctx.state.root.clone(),
            expanded: HashSet::new(),
            options: (false, 2_000),
        };
        let completion = runtime
            .component
            .update(FileTreeMsg::EntryRefreshLoaded(stale), &mut runtime.ctx);
        assert!(!completion.dirty);
    }

    #[test]
    fn props_hook_delivers_listing_and_refreshes_cached_snapshot() {
        let modified =
            super::super::GitFileStatus::new(None, Some(super::super::GitChangeState::Modified));
        let old_props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(".", [super::super::FileTreeEntry::directory("src")]),
            ]))
            .props;
        let new_props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(".", [super::super::FileTreeEntry::directory("src")]),
                FileTreeDirectoryListing::new(
                    "src",
                    [super::super::FileTreeEntry::file("main.rs").git_status(modified)],
                ),
            ]))
            .props;
        let mut runtime = RuntimeCore::new_test(
            FileTreeComponent::new(),
            old_props,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(std::cell::Cell::new(false)),
        );
        runtime
            .ctx
            .state
            .expanded
            .insert(Arc::from("/remote/repo/src"));

        let old_props = std::mem::replace(&mut runtime.ctx.props, new_props);
        let update = runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx);

        assert!(update.dirty);
        assert!(runtime.ctx.state.expanded.contains("/remote/repo/src"));
        assert!(
            node_by_path_mut(&mut runtime.ctx.state.root, "/remote/repo/src")
                .is_some_and(|node| node.loaded && node.children.len() == 1)
        );
        assert!(
            effective_change_snapshot(&runtime.ctx.state)
                .entries
                .contains_key("/remote/repo/src/main.rs")
        );
        assert!(std::ptr::eq(
            effective_change_snapshot(&runtime.ctx.state),
            effective_change_snapshot(&runtime.ctx.state)
        ));

        let changed_props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(".", [super::super::FileTreeEntry::directory("src")]),
                FileTreeDirectoryListing::new("src", [super::super::FileTreeEntry::file("lib.rs")]),
            ]))
            .props;
        let old_props = std::mem::replace(&mut runtime.ctx.props, changed_props);
        runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx);

        let src = node_by_path_mut(&mut runtime.ctx.state.root, "/remote/repo/src").unwrap();
        assert_eq!(src.children.len(), 1);
        assert_eq!(src.children[0].name.as_ref(), "lib.rs");
        assert!(
            !effective_change_snapshot(&runtime.ctx.state)
                .entries
                .contains_key("/remote/repo/src/main.rs")
        );

        let removed_props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(".", [super::super::FileTreeEntry::directory("src")]),
            ]))
            .props;
        let old_props = std::mem::replace(&mut runtime.ctx.props, removed_props);
        runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx);

        assert!(
            node_by_path_mut(&mut runtime.ctx.state.root, "/remote/repo/src")
                .is_some_and(|node| !node.loaded && node.loading && node.children.is_empty())
        );
    }

    #[test]
    fn stale_git_result_cannot_replace_a_provided_snapshot() {
        let modified =
            super::super::GitFileStatus::new(None, Some(super::super::GitChangeState::Modified));
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(
                    ".",
                    [super::super::FileTreeEntry::file("README.md").git_status(modified)],
                ),
            ]))
            .props;
        let mut runtime = RuntimeCore::new_test(
            FileTreeComponent::new(),
            props,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(std::cell::Cell::new(false)),
        );
        runtime.ctx.state.git_load_generation = 1;

        let update = runtime.component.update(
            FileTreeMsg::GitSnapshotLoaded {
                generation: 0,
                snapshot: Some(GitStatusSnapshot::default()),
                cache_request: None,
            },
            &mut runtime.ctx,
        );

        assert!(!update.dirty);
        assert!(
            runtime
                .ctx
                .state
                .git_snapshot
                .entries
                .contains_key("/remote/repo/README.md")
        );
    }

    #[test]
    fn shared_git_cache_seeds_a_remounted_tree_before_revalidation_finishes() {
        let dir = unique_component_test_dir("git-cache-remount");
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("changed.rs"), "changed").unwrap();

        let cache = super::super::FileTreeGitStatusCache::new();
        let key = GitStatusCacheKey::new(&dir, false);
        let request = cache.begin_request(key);
        let changed_path = super::super::fs::normalize_path(&dir.join("changed.rs"));
        let modified =
            super::super::GitFileStatus::new(None, Some(super::super::GitChangeState::Modified));
        let mut snapshot = GitStatusSnapshot::default();
        snapshot.entries.insert(
            changed_path.clone(),
            GitFileDecorations::from_status(modified, true),
        );
        snapshot.changed_paths.push(changed_path.clone());
        assert!(cache.finish_request(&request, Some(&snapshot)));

        let props = super::super::FileTree::new(dir.to_string_lossy().as_ref())
            .git_status_cache(cache.clone())
            .props;
        let mut runtime = RuntimeCore::new_test(
            FileTreeComponent::new(),
            props,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(std::cell::Cell::new(false)),
        );

        let command = runtime.component.init(&mut runtime.ctx);

        assert!(
            command.is_some(),
            "the cached snapshot is still revalidated"
        );
        assert!(!runtime.ctx.state.git_snapshot_pending);
        assert!(
            runtime
                .ctx
                .state
                .git_snapshot
                .entries
                .contains_key(&changed_path)
        );
        assert_eq!(cache.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn disabling_and_reenabling_git_invalidates_older_loads() {
        let enabled = crate::widgets::file_tree::FileTree::new("/definitely/missing/repo").props;
        let disabled = crate::widgets::file_tree::FileTree::new("/definitely/missing/repo")
            .git_status(false)
            .props;
        let mut runtime = RuntimeCore::new_test(
            FileTreeComponent::new(),
            enabled.clone(),
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(std::cell::Cell::new(false)),
        );

        let old_props = std::mem::replace(&mut runtime.ctx.props, disabled);
        runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx);
        assert_eq!(runtime.ctx.state.git_load_generation, 1);

        let old_props = std::mem::replace(&mut runtime.ctx.props, enabled);
        runtime
            .component
            .on_props_changed(&old_props, &mut runtime.ctx);
        assert_eq!(runtime.ctx.state.git_load_generation, 2);
    }

    #[test]
    fn provided_source_requests_missing_root_once_without_local_io() {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let captured = requests.clone();
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::Provided(Vec::new()))
            .on_entry_request(crate::Callback::new(
                move |request: FileTreeEntryRequest| {
                    captured.borrow_mut().push(request.path);
                },
            ))
            .props;
        let mut state = FileTreeComponent::new().create_state(&props);

        assert_eq!(state.root.path.as_ref(), "/remote/repo");
        assert!(!state.root.loaded);
        assert!(state.root.error.is_none());

        request_missing_provided_directories(&mut state, &props);
        request_missing_provided_directories(&mut state, &props);

        assert!(state.root.loading);
        assert_eq!(requests.borrow().as_slice(), &[Arc::from("/remote/repo")]);
    }

    #[test]
    fn provided_source_requests_windows_root_without_host_path_rewriting() {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let captured = requests.clone();
        let props = crate::widgets::file_tree::FileTree::new(r"C:\remote\repo")
            .entry_source(FileTreeEntrySource::Provided(Vec::new()))
            .on_entry_request(crate::Callback::new(
                move |request: FileTreeEntryRequest| {
                    captured.borrow_mut().push(request.path);
                },
            ))
            .props;
        let mut state = FileTreeComponent::new().create_state(&props);

        request_missing_provided_directories(&mut state, &props);

        assert_eq!(
            requests.borrow().as_slice(),
            &[Arc::from(r"C:\remote\repo")]
        );
    }

    #[test]
    fn provided_windows_listing_paths_remain_windows_on_a_posix_host() {
        let props = crate::widgets::file_tree::FileTree::new(r"C:\remote\repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(".", [super::super::FileTreeEntry::file("main.rs")]),
            ]))
            .props;
        let state = FileTreeComponent::new().create_state(&props);

        assert_eq!(state.root.path.as_ref(), r"C:\remote\repo");
        assert_eq!(
            state.root.children[0].path.as_ref(),
            r"C:\remote\repo\main.rs"
        );
    }

    #[test]
    fn provided_windows_changes_group_with_windows_paths_on_a_posix_host() {
        let props = crate::widgets::file_tree::FileTree::new(r"C:\remote\repo")
            .change_source(FileTreeChangeSource::provided([
                super::super::FileTreeChange::new(
                    "src/main.rs",
                    super::super::FileTreeChangeStatus::Modified,
                ),
            ]))
            .change_view(FileTreeChangeView::ChangedOnly)
            .props;
        let mut state = FileTreeComponent::new().create_state(&props);
        state.expanded.insert(Arc::from(r"C:\remote\repo\src"));
        let projection = build_projection(&props, &state, None);

        assert_eq!(
            projection
                .lookup
                .values()
                .find(|entry| entry.kind == FileKind::File)
                .map(|entry| entry.path.as_ref()),
            Some(r"C:\remote\repo\src\main.rs")
        );
        assert!(projection.lookup.values().any(|entry| {
            entry.kind == FileKind::Directory && entry.path.as_ref() == r"C:\remote\repo\src"
        }));
    }

    #[test]
    fn provided_source_does_not_request_a_loaded_directory() {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let captured = requests.clone();
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(".", [super::super::FileTreeEntry::directory("src")]),
                FileTreeDirectoryListing::new("src", Vec::new()),
            ]))
            .on_entry_request(crate::Callback::new(
                move |request: FileTreeEntryRequest| {
                    captured.borrow_mut().push(request.path);
                },
            ))
            .props;
        let mut state = FileTreeComponent::new().create_state(&props);

        request_provided_directory(&mut state, &props, Arc::from("/remote/repo/src"));

        assert!(requests.borrow().is_empty());
        assert!(
            node_by_path_mut(&mut state.root, "/remote/repo/src")
                .is_some_and(|node| { node.loaded && !node.loading })
        );
    }

    #[test]
    fn provided_listing_options_are_applied_when_root_is_rebuilt() {
        let source = FileTreeEntrySource::provided([FileTreeDirectoryListing::new(
            ".",
            [
                super::super::FileTreeEntry::file(".hidden"),
                super::super::FileTreeEntry::file("one.rs"),
                super::super::FileTreeEntry::file("two.rs"),
            ],
        )]);
        let hidden_props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(source.clone())
            .max_entries_per_dir(1)
            .props;
        let visible_props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(source)
            .show_hidden(true)
            .max_entries_per_dir(3)
            .props;

        let (hidden_root, _) = initial_root(&hidden_props);
        let (visible_root, _) = initial_root(&visible_props);

        assert_eq!(hidden_root.children.len(), 2); // one entry plus the truncation row
        assert_eq!(visible_root.children.len(), 3);
        assert!(
            visible_root
                .children
                .iter()
                .any(|child| child.name.as_ref() == ".hidden")
        );

        let hidden_state = FileTreeComponent::new().create_state(&hidden_props);
        let candidates = explorer_search_candidates(&hidden_props, &hidden_state).unwrap();
        assert_eq!(candidates.entries.len(), 1);
        assert_eq!(candidates.entries[0].label.as_ref(), "one.rs");
    }

    /// The seed is expansion the tree adopts as its own, so it survives into the projection, loads
    /// the directories it names, and stays out of the way of a path the user has since collapsed.
    #[test]
    fn initial_expanded_paths_seed_state_without_reopening_collapsed_parents() {
        let dir = unique_component_test_dir("initial-expanded");
        std::fs::create_dir_all(dir.join("src/widgets")).expect("create dirs");
        std::fs::write(dir.join("src/widgets/tree.rs"), "").expect("write file");
        let root = dir.to_string_lossy().to_string();

        let props = crate::widgets::file_tree::FileTree::new(root.clone())
            // Relative and absolute spellings both resolve; a path outside the root is dropped.
            .initial_expanded_paths(["src", &format!("{root}/src/widgets"), "/elsewhere/src"])
            .props;
        let state = FileTreeComponent::new().create_state(&props);

        assert!(state.expanded.contains(format!("{root}/src").as_str()));
        assert!(
            state
                .expanded
                .contains(format!("{root}/src/widgets").as_str())
        );
        assert!(!state.expanded.contains("/elsewhere/src"));
        // Seeded directories are loaded, not left as lazy placeholders.
        let mut loaded_state = state;
        let widgets = node_by_path_mut(&mut loaded_state.root, &format!("{root}/src/widgets"))
            .expect("widgets node");
        assert!(widgets.loaded);
        assert_eq!(widgets.children.len(), 1);

        // A collapsed ancestor keeps its descendants' seeds without being reopened by them.
        let collapsed = crate::widgets::file_tree::FileTree::new(root.clone())
            .initial_expanded_paths([format!("{root}/src/widgets")])
            .props;
        let collapsed_state = FileTreeComponent::new().create_state(&collapsed);
        assert!(
            !collapsed_state
                .expanded
                .contains(format!("{root}/src").as_str())
        );
        assert!(
            collapsed_state
                .expanded
                .contains(format!("{root}/src/widgets").as_str())
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_parent_loads_seeded_local_descendants() {
        let dir = unique_component_test_dir("initial-expanded-open-parent");
        std::fs::create_dir_all(dir.join("src/widgets")).expect("create dirs");
        std::fs::write(dir.join("src/widgets/tree.rs"), "").expect("write file");
        let root = dir.to_string_lossy().to_string();
        let src = Arc::<str>::from(format!("{root}/src"));
        let widgets = format!("{root}/src/widgets");
        let props = crate::widgets::file_tree::FileTree::new(root)
            .initial_expanded_paths([widgets.clone()])
            .props;
        let mut runtime = RuntimeCore::new_test(
            FileTreeComponent::new(),
            props,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(std::cell::Cell::new(false)),
        );

        runtime.component.update(
            FileTreeMsg::TreeToggled {
                entry: Some(VisibleFileTreeEntry {
                    path: src,
                    kind: FileKind::Directory,
                }),
                expanded: true,
            },
            &mut runtime.ctx,
        );

        let node = node_by_path_mut(&mut runtime.ctx.state.root, &widgets).expect("widgets node");
        assert!(node.loaded);
        assert_eq!(node.children[0].name.as_ref(), "tree.rs");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn opening_parent_requests_seeded_provided_descendants() {
        let requests = Rc::new(RefCell::new(Vec::new()));
        let captured = requests.clone();
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(".", [super::super::FileTreeEntry::directory("src")]),
                FileTreeDirectoryListing::new(
                    "src",
                    [super::super::FileTreeEntry::directory("widgets")],
                ),
            ]))
            .initial_expanded_paths(["src/widgets"])
            .on_entry_request(crate::Callback::new(
                move |request: FileTreeEntryRequest| {
                    captured.borrow_mut().push(request.path);
                },
            ))
            .props;
        let mut runtime = RuntimeCore::new_test(
            FileTreeComponent::new(),
            props,
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            Theme::default(),
            SurfaceMode::Fullscreen,
            Rc::new(std::cell::Cell::new(false)),
        );

        runtime.component.update(
            FileTreeMsg::TreeToggled {
                entry: Some(VisibleFileTreeEntry {
                    path: Arc::from("/remote/repo/src"),
                    kind: FileKind::Directory,
                }),
                expanded: true,
            },
            &mut runtime.ctx,
        );

        assert_eq!(
            requests.borrow().as_slice(),
            &[Arc::from("/remote/repo/src/widgets")]
        );
    }

    /// Re-rooting is the case the seed exists for: the tree drops everything it knew, so the same
    /// seed has to apply again against the new root.
    #[test]
    fn initial_expanded_paths_reapply_when_the_root_changes() {
        let dir = unique_component_test_dir("initial-expanded-reroot");
        std::fs::create_dir_all(dir.join("one/src")).expect("create one");
        std::fs::create_dir_all(dir.join("two/src")).expect("create two");
        let one = dir.join("one").to_string_lossy().to_string();
        let two = dir.join("two").to_string_lossy().to_string();

        let props = crate::widgets::file_tree::FileTree::new(one.clone())
            .initial_expanded_paths([format!("{one}/src"), format!("{two}/src")])
            .props;
        let mut state = FileTreeComponent::new().create_state(&props);
        assert!(state.expanded.contains(format!("{one}/src").as_str()));

        let rerooted = crate::widgets::file_tree::FileTree::new(two.clone())
            .initial_expanded_paths([format!("{one}/src"), format!("{two}/src")])
            .props;
        rebuild_root_for_props(&mut state, &rerooted);

        assert!(state.expanded.contains(format!("{two}/src").as_str()));
        // The previous root's entry is not carried into a tree that cannot show it.
        assert!(!state.expanded.contains(format!("{one}/src").as_str()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Controlled expansion is authoritative, so a seed alongside it would be a second answer to
    /// the same question.
    #[test]
    fn controlled_expansion_ignores_the_seed() {
        let dir = unique_component_test_dir("initial-expanded-controlled");
        std::fs::create_dir_all(dir.join("src")).expect("create dirs");
        let root = dir.to_string_lossy().to_string();

        let props = crate::widgets::file_tree::FileTree::new(root.clone())
            .initial_expanded_paths([format!("{root}/src")])
            .expanded_paths(Vec::<String>::new())
            .props;
        let state = FileTreeComponent::new().create_state(&props);

        assert!(!state.expanded.contains(format!("{root}/src").as_str()));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn provided_changed_only_keeps_unknown_directories_expandable() {
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(".", [super::super::FileTreeEntry::directory("src")]),
            ]))
            .change_view(FileTreeChangeView::ChangedOnly)
            .props;
        let state = FileTreeComponent::new().create_state(&props);

        let projection = build_projection(&props, &state, None);

        assert_eq!(projection.root.children.len(), 1);
        assert_eq!(
            projection.root.children[0]
                .item
                .spans
                .last()
                .unwrap()
                .content
                .as_ref(),
            "src"
        );
        assert_eq!(
            projection
                .lookup
                .get(&crate::widgets::TreePath::from(vec![0]))
                .map(|entry| entry.path.as_ref()),
            Some("/remote/repo/src")
        );
    }

    #[test]
    fn leaving_provided_entries_restarts_local_git_loading() {
        let provided = crate::widgets::file_tree::FileTree::new("/repo")
            .entry_source(FileTreeEntrySource::Provided(Vec::new()))
            .props;
        let local = crate::widgets::file_tree::FileTree::new("/repo").props;

        assert!(should_load_git_after_props_change(&provided, &local));
    }

    #[test]
    fn provided_listings_hydrate_lazily_and_project_git_status() {
        let modified =
            super::super::GitFileStatus::new(None, Some(super::super::GitChangeState::Modified));
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(
                    ".",
                    [
                        super::super::FileTreeEntry::directory("src"),
                        super::super::FileTreeEntry::file("README.md").git_status(modified),
                    ],
                ),
                FileTreeDirectoryListing::new(
                    "src",
                    [super::super::FileTreeEntry::file("main.rs").git_status(modified)],
                ),
            ]))
            .props;

        let state = FileTreeComponent::new().create_state(&props);
        let src = state
            .root
            .children
            .iter()
            .find(|child| child.name.as_ref() == "src")
            .unwrap();
        assert!(state.root.loaded);
        assert!(src.loaded);
        assert_eq!(src.children[0].path.as_ref(), "/remote/repo/src/main.rs");

        let snapshot = effective_change_snapshot(&state);
        assert!(snapshot.entries.contains_key("/remote/repo"));
        assert!(snapshot.entries.contains_key("/remote/repo/src"));
        assert!(snapshot.entries.contains_key("/remote/repo/src/main.rs"));
        assert!(snapshot.entries.contains_key("/remote/repo/README.md"));
    }

    #[test]
    fn provided_ignore_state_excludes_explorer_candidates_only() {
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::new(
                    ".",
                    [
                        super::super::FileTreeEntry::file("visible.rs"),
                        super::super::FileTreeEntry::file("generated.rs").ignored(true),
                    ],
                ),
            ]))
            .props;
        let state = FileTreeComponent::new().create_state(&props);

        assert_eq!(state.root.children.len(), 2);
        let candidates = explorer_search_candidates(&props, &state).unwrap();
        assert_eq!(candidates.entries.len(), 1);
        assert_eq!(candidates.entries[0].label.as_ref(), "visible.rs");
    }

    #[test]
    fn provided_listing_error_is_renderable_without_filesystem_access() {
        let props = crate::widgets::file_tree::FileTree::new("/remote/repo")
            .entry_source(FileTreeEntrySource::provided([
                FileTreeDirectoryListing::error(".", "permission denied"),
            ]))
            .props;
        let state = FileTreeComponent::new().create_state(&props);

        assert!(state.root.loaded);
        assert_eq!(state.root.error.as_deref(), Some("permission denied"));
        let projection = build_projection(&props, &state, None);
        assert_eq!(
            projection.root.children[0].item.spans[0].content.as_ref(),
            "error: permission denied"
        );
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
    fn git_status_padding_follows_markers_without_padding_stats_only_rows() {
        use crate::widgets::file_tree::git::{
            GitChangeState::{Added, Modified},
            GitDiffStat, GitFileDecorations, GitFileStatus, GitIconStyle,
        };

        for (staged, unstaged, diff_stat, show_stats, expected) in [
            (None, Some(Modified), None, false, "  "),
            (Some(Added), None, None, false, "  "),
            (Some(Added), Some(Modified), None, false, "   "),
            (
                None,
                Some(Modified),
                Some(GitDiffStat::new(2, 1)),
                true,
                " +2 -1  ",
            ),
            (
                None,
                Some(Modified),
                Some(GitDiffStat::new(2, 1)),
                false,
                "  ",
            ),
            (
                None,
                Some(Modified),
                Some(GitDiffStat::new(0, 0)),
                true,
                "  ",
            ),
            (None, None, Some(GitDiffStat::new(2, 1)), true, " +2 -1"),
            (None, None, None, true, ""),
        ] {
            let props = crate::widgets::file_tree::FileTree::new("/repo")
                .git_icon_style(GitIconStyle::NerdFont)
                .git_status_padding_right(1)
                .show_diff_stats(show_stats)
                .props;
            let decoration = GitFileDecorations {
                status: GitFileStatus::new(staged, unstaged),
                diff_stat,
                direct: true,
            };
            let actual = git_description_spans(Some(decoration), &props, false, false)
                .iter()
                .map(|span| span.content.as_ref())
                .collect::<String>();
            assert_eq!(actual, expected, "{decoration:?}, show_stats={show_stats}");
        }
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

        assert_eq!(content, vec![" ", "+2", " ", "-1", " ", "M"]);
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
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
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
            .indent_style(crate::widgets::IndentStyle::Branch)
            .indent_width(1)
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
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, vec![0], &mut ctx);

        // A file is a leaf: the tree fills its arrow column, so it lines up with directories.
        assert!(!tree.is_collapsible());
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
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
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

        assert!(!symlink_tree.is_collapsible());
        assert!(!other_tree.is_collapsible());
        assert!(!symlink_tree.item.spans[0].content.starts_with(' '));
        assert!(!other_tree.item.spans[0].content.starts_with(' '));
    }

    #[test]
    fn directory_icon_override_keeps_its_expansion_arrow() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .icon_style(FileIconStyle::NerdFont)
            .indent_style(crate::widgets::IndentStyle::Branch)
            .icon_override("src", "★", None)
            .props;
        let node = FsNode {
            name: Arc::from("src"),
            path: Arc::from(r"C:\repo\src"),
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
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
            path_styles: &path_styles,
            lookup: &mut lookup,
        };

        let tree = build_projected_tree_node(&node, false, vec![0], &mut ctx);

        // The tree draws the directory's collapsed arrow in front of its icon.
        assert!(tree.is_collapsible() && !tree.expanded);
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
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
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
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
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
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
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
        let path_styles = resolved_path_styles(&props.path_styles, "/repo", false);
        let expanded = HashSet::new();
        let mut lookup = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
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
        let path_styles = resolved_path_styles(&props.path_styles, "/repo", false);
        let expanded = HashSet::new();
        let mut lookup = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
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
            tree.item.description_spans[5].style.fg,
            props.git_style_modified.fg
        );
        assert_eq!(
            tree.item.description_spans[1].style.fg,
            props.git_style_added.fg
        );
        assert_eq!(
            tree.item.description_spans[3].style.fg,
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
            vec![" ", "+30", " ", "-21", " ", "M"]
        );
        assert!(spans.iter().all(|span| span.style.dim == Some(true)));
        assert_eq!(spans[5].style.fg, props.git_style_modified.fg);
        assert_eq!(spans[1].style.fg, props.git_style_added.fg);
        assert_eq!(spans[3].style.fg, props.git_style_deleted.fg);
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
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
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
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
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

        let resolved = resolved_path_styles(&styles, "/repo", false);

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
            ignored_paths: HashSet::new(),
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
            ignored_paths: HashSet::new(),
        };

        let root = build_changed_only_root(&source_root, &snapshot, false);

        assert_eq!(root.children.len(), 1);
        assert_eq!(root.children[0].name.as_ref(), "src");
    }

    fn changed_only_tree(changed: &[&str]) -> FsNode {
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
            changed_paths: changed.iter().map(|path| Arc::from(*path)).collect(),
            kinds: HashMap::new(),
            virtual_changes: false,
            ignored_paths: HashSet::new(),
        };
        build_changed_only_root(&source_root, &snapshot, true)
    }

    #[test]
    fn compacting_folds_single_directory_chains_into_the_innermost_path() {
        let mut root = changed_only_tree(&[
            "/repo/src/view/sidebar/tree.rs",
            "/repo/src/view/sidebar/row.rs",
            "/repo/docs/index.md",
            "/repo/docs/guide/intro.md",
        ]);

        compact_directory_chains(&mut root);

        // The root is the heading and never folds, even with a single child of its own.
        assert_eq!(root.path.as_ref(), "/repo");
        let names: Vec<&str> = root.children.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, ["docs", "src/view/sidebar"]);
        let folded = &root.children[1];
        assert_eq!(folded.path.as_ref(), "/repo/src/view/sidebar");
        assert_eq!(folded.kind, FileKind::Directory);
        assert_eq!(folded.children.len(), 2);
        // `docs` holds a file beside its directory, so it stays; `guide` holds only a file.
        let docs = &root.children[0];
        let names: Vec<&str> = docs.children.iter().map(|c| c.name.as_ref()).collect();
        assert_eq!(names, ["guide", "index.md"]);
    }

    #[test]
    fn a_query_unfolds_compacted_directories() {
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .change_view(crate::widgets::file_tree::FileTreeChangeView::ChangedOnly)
            .compact_directories(true)
            .show_icons(false)
            .props;
        let mut state = test_state_with_root(FsNode {
            name: Arc::from("repo"),
            path: Arc::from("/repo"),
            kind: FileKind::Directory,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        });
        state.git_snapshot.changed_paths = vec![Arc::from("/repo/src/widgets/tree.rs")];
        state.expanded.insert(Arc::from("/repo"));
        let label = |projection: &FileTreeProjection| {
            projection.root.children[0].item.spans[0]
                .content
                .as_ref()
                .to_owned()
        };

        let folded = build_projection(&props, &state, None);
        let mut filter = ExplorerFilter::default();
        filter.visible_paths.insert(Arc::from("/repo/src"));
        let searching = build_projection(&props, &state, Some(&filter));

        assert_eq!(label(&folded), "src/widgets");
        assert_eq!(label(&searching), "src");
    }

    #[test]
    fn hidden_style_patches_the_icon_and_label_of_dot_entries() {
        let hidden = Style::new().dim();
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .icon_style(FileIconStyle::NerdFont)
            .hidden_style(hidden)
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
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
            path_styles: &path_styles,
            lookup: &mut lookup,
        };
        let directory = |name: &str| FsNode {
            name: Arc::from(name),
            path: Arc::from(format!("/repo/{name}")),
            kind: FileKind::Directory,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };

        let dotted = build_projected_tree_node(&directory(".github"), false, vec![0], &mut ctx);
        let folded = build_projected_tree_node(&directory("src/.cfg"), false, vec![1], &mut ctx);
        let plain = build_projected_tree_node(&directory("src"), false, vec![2], &mut ctx);

        for tree in [&dotted, &folded] {
            assert_eq!(tree.item.spans[0].style, hidden, "icon");
            assert_eq!(tree.item.spans[2].style, hidden, "label");
        }
        assert_eq!(plain.item.spans[2].style, Style::default());
    }

    #[test]
    fn the_cursor_row_drops_its_receded_style_while_the_highlight_shows() {
        let hidden = Style::new().dim();
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .hidden_style(hidden)
            .unfocused_selection_style(Style::default())
            .props;
        let file = |name: &str| FsNode {
            name: Arc::from(name),
            path: Arc::from(format!("/repo/{name}")),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let mut root = file("repo");
        root.path = Arc::from("/repo");
        root.kind = FileKind::Directory;
        root.children = vec![file(".env"), file(".envrc")];
        let mut state = test_state_with_root(root);
        state.expanded.insert(Arc::from("/repo"));
        state.uncontrolled_selected_path = Some(Arc::from("/repo/.env"));
        let label_styles = |state: &FileTreeState| {
            let projection = build_projection(&props, state, None);
            projection
                .root
                .children
                .iter()
                .map(|row| row.item.spans[0].style)
                .collect::<Vec<_>>()
        };

        // Unfocused, this tree draws no cursor, so the selected row recedes like any other.
        assert_eq!(label_styles(&state), [hidden, hidden]);
        state.tree_focused = true;
        assert_eq!(label_styles(&state), [Style::default(), hidden]);
    }

    fn dotfile_state() -> FileTreeState {
        let file = |name: &str| FsNode {
            name: Arc::from(name),
            path: Arc::from(format!("/repo/{name}")),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let mut root = file("repo");
        root.path = Arc::from("/repo");
        root.kind = FileKind::Directory;
        root.children = vec![file(".env"), file(".envrc")];
        let mut state = test_state_with_root(root);
        state.expanded.insert(Arc::from("/repo"));
        state.tree_focused = true;
        state
    }

    fn row_label_styles(props: &FileTreeProps, state: &FileTreeState) -> Vec<Style> {
        build_projection(props, state, None)
            .root
            .children
            .iter()
            .map(|row| row.item.spans[0].style)
            .collect()
    }

    #[test]
    fn a_selection_by_index_lifts_the_row_it_selects() {
        let hidden = Style::new().dim();
        // Row 0 is the root heading, so index 2 is `.envrc`.
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .hidden_style(hidden)
            .selected(2)
            .props;

        assert_eq!(
            row_label_styles(&props, &dotfile_state()),
            [hidden, Style::default()]
        );
    }

    #[test]
    fn a_cleared_selection_lifts_nothing() {
        let hidden = Style::new().dim();
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .show_icons(false)
            .hidden_style(hidden)
            .selected_path(".env")
            .clear_selection(true)
            .props;

        assert_eq!(row_label_styles(&props, &dotfile_state()), [hidden, hidden]);
    }

    #[test]
    fn ignored_style_loads_git_without_status_markers() {
        let markers_off = crate::widgets::file_tree::FileTree::new("/repo")
            .git_status(false)
            .props;
        let dim_ignored = crate::widgets::file_tree::FileTree::new("/repo")
            .git_status(false)
            .ignored_style(Style::new().dim())
            .props;

        assert!(!needs_git_snapshot(&markers_off));
        assert!(needs_git_snapshot(&dim_ignored));
        assert!(should_load_git_after_props_change(
            &markers_off,
            &dim_ignored
        ));
    }

    #[cfg(unix)]
    #[test]
    fn link_kind_is_learned_with_icons_and_targets_off() {
        let dir = std::env::temp_dir().join(format!(
            "tui-lipan-file-tree-link-label-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real")).expect("create dir");
        std::os::unix::fs::symlink("real", dir.join("to-dir")).expect("dir link");
        let link_path: Arc<str> = Arc::from(dir.join("to-dir").to_string_lossy().as_ref());
        let props = crate::widgets::file_tree::FileTree::new(dir.to_string_lossy().as_ref())
            .show_icons(false)
            .symlink_targets(false)
            .props;
        let mut root = FsNode {
            name: Arc::from("root"),
            path: Arc::from(dir.to_string_lossy().as_ref()),
            kind: FileKind::Directory,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        root.children.push(FsNode {
            name: Arc::from("to-dir"),
            path: link_path.clone(),
            kind: FileKind::Symlink,
            ..root.clone()
        });
        let mut state = test_state_with_root(root);

        sync_symlink_targets(&mut state, &props);

        // The label style reads this, so it is known even with no icon or target suffix drawn.
        assert!(state.symlink_targets[&link_path].is_dir);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_ignored_directory_covers_everything_beneath_it() {
        let ignored: HashSet<Arc<str>> = [Arc::from("/repo/target")].into_iter().collect();

        assert!(under_ignored_path("/repo/target", "/repo", &ignored));
        assert!(under_ignored_path(
            "/repo/target/debug/rozi",
            "/repo",
            &ignored
        ));
        assert!(!under_ignored_path(
            "/repo/target-notes.md",
            "/repo",
            &ignored
        ));
        assert!(!under_ignored_path("/repo/src/main.rs", "/repo", &ignored));
        // Nothing at or above the root is looked up, even when the root itself is listed.
        let root_listed: HashSet<Arc<str>> = [Arc::from("/repo")].into_iter().collect();
        assert!(!under_ignored_path("/repo/src", "/repo", &root_listed));
    }

    #[test]
    fn ignored_style_patches_rows_inside_an_ignored_directory() {
        let ignored_style = Style::new().fg(Color::DarkGray);
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .icon_style(FileIconStyle::NerdFont)
            .ignored_style(ignored_style)
            .props;
        let ignored: HashSet<Arc<str>> = [Arc::from("/repo/target")].into_iter().collect();
        let expanded = HashSet::new();
        let decorations = HashMap::new();
        let mut lookup = HashMap::new();
        let path_styles = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            symlink_targets: &NO_SYMLINK_TARGETS,
            ignored_paths: &ignored,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
            path_styles: &path_styles,
            lookup: &mut lookup,
        };
        let file = |path: &str| FsNode {
            name: Arc::from(path.rsplit('/').next().unwrap_or(path)),
            path: Arc::from(path),
            kind: FileKind::File,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };

        let built = build_projected_tree_node(&file("/repo/target/rozi"), false, vec![0], &mut ctx);
        let source =
            build_projected_tree_node(&file("/repo/src/main.rs"), false, vec![1], &mut ctx);

        assert_eq!(built.item.spans[0].style, ignored_style, "icon");
        assert_eq!(built.item.spans[2].style, ignored_style, "label");
        assert_eq!(source.item.spans[2].style, Style::default());
    }

    #[test]
    fn a_link_to_a_directory_draws_as_a_linked_directory() {
        let directory_style = Style::new().fg(Color::Blue);
        let file_style = Style::new().fg(Color::Green);
        let props = crate::widgets::file_tree::FileTree::new("/repo")
            .icon_style(FileIconStyle::NerdFont)
            .directory_label_style(directory_style)
            .file_label_style(file_style)
            .props;
        let mut links = HashMap::new();
        links.insert(
            Arc::from("/repo/docs"),
            SymlinkInfo {
                target: None,
                is_dir: true,
            },
        );
        links.insert(Arc::from("/repo/CLAUDE.md"), SymlinkInfo::default());
        let expanded = HashSet::new();
        let decorations = HashMap::new();
        let mut lookup = HashMap::new();
        let path_styles = HashMap::new();
        let mut ctx = ProjectionBuildContext {
            props: &props,
            expanded: &expanded,
            explorer_filter: None,
            git_decorations: &decorations,
            symlink_targets: &links,
            ignored_paths: &NO_IGNORED_PATHS,
            root_path: "/repo",
            highlighted_path: None,
            receded_paths: &mut HashSet::new(),
            path_styles: &path_styles,
            lookup: &mut lookup,
        };
        let link = |name: &str| FsNode {
            name: Arc::from(name),
            path: Arc::from(format!("/repo/{name}")),
            kind: FileKind::Symlink,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };

        let to_dir = build_projected_tree_node(&link("docs"), false, vec![0], &mut ctx);
        let to_file = build_projected_tree_node(&link("CLAUDE.md"), false, vec![1], &mut ctx);

        assert_eq!(to_dir.item.spans[0].content.as_ref(), "\u{f482}");
        assert_eq!(to_dir.item.spans[2].style, directory_style);
        assert!(!to_dir.is_collapsible(), "a link stays a leaf");
        assert_eq!(to_file.item.spans[0].content.as_ref(), "\u{f481}");
        assert_eq!(to_file.item.spans[2].style, file_style);
    }

    #[cfg(unix)]
    #[test]
    fn local_links_learn_whether_they_resolve_to_a_directory() {
        let dir = std::env::temp_dir().join(format!(
            "tui-lipan-file-tree-link-kind-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("real")).expect("create dir");
        std::fs::write(dir.join("file.txt"), "x").expect("write file");
        std::os::unix::fs::symlink("real", dir.join("to-dir")).expect("dir link");
        std::os::unix::fs::symlink("file.txt", dir.join("to-file")).expect("file link");
        std::os::unix::fs::symlink("missing", dir.join("dangling")).expect("dangling link");
        let node = |name: &str| FsNode {
            name: Arc::from(name),
            path: Arc::from(dir.join(name).to_string_lossy().as_ref()),
            kind: FileKind::Symlink,
            loaded: true,
            loading: false,
            error: None,
            children: Vec::new(),
        };
        let mut root = node("root");
        root.kind = FileKind::Directory;
        root.children = vec![node("to-dir"), node("to-file"), node("dangling")];

        let mut links = HashMap::new();
        collect_local_symlink_targets(&root, &mut links);
        let is_dir = |name: &str| links[dir.join(name).to_string_lossy().as_ref()].is_dir;

        assert!(is_dir("to-dir"));
        assert!(!is_dir("to-file"));
        assert!(!is_dir("dangling"));
        let _ = std::fs::remove_dir_all(&dir);
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
                ignored_paths: HashSet::new(),
            },
            git_snapshot_pending: false,
            git_load_generation: 0,
            last_git_refresh_nonce: 0,
            entry_refresh_requested_token: 0,
            entry_refresh_completed_token: 0,
            entry_refresh_requested_options: (false, 2_000),
            entry_refresh_pending: false,
            entry_refresh_generation: 0,
            changed_only_auto_expand_signature: 0,
            explorer_input: TextInput::new(""),
            explorer_query_id: 0,
            explorer_filter: ExplorerFilter::default(),
            explorer_index: Arc::default(),
            explorer_task_key: next_explorer_task_key(),
            explorer_entered_from_tree: false,
            explorer_focused: false,
            search_expanded_snapshot: None,
            search_found_dir: None,
            requested_entry_paths: HashSet::new(),
            uncontrolled_selected_path: Some(Arc::from("/repo")),
            symlink_targets: HashMap::new(),
            tree_focused: false,
            watch: WatchState::default(),
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
