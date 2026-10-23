use super::events::{FileTreeEvent, FileTreeToggleEvent};
use super::fs::FileIconStyle;
use super::git::{FileTreeGitStatusCache, GitIconStyle};
use super::{
    FileIconOverride, FileTreeChangeSource, FileTreeChangeView, FileTreeEntrySource,
    FileTreeItemStyle, FileTreeSuffixPriority,
};
use crate::callback::Callback;
use crate::style::{BorderStyle, FileIconPalette, Length, Padding, Style, StyleSlot};
use crate::widgets::ScrollKeymap;
use crate::widgets::TreeKeymap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Clone, PartialEq)]
pub(crate) struct FileTreeProps {
    pub(crate) root: Arc<str>,
    pub(crate) entry_source: FileTreeEntrySource,
    pub(crate) entry_refresh_token: u64,
    pub(crate) refresh_policy: super::FileTreeRefreshPolicy,
    pub(crate) show_hidden: bool,
    pub(crate) max_entries_per_dir: usize,
    pub(crate) show_icons: bool,
    pub(crate) icon_style: FileIconStyle,
    pub(crate) icon_palette: FileIconPalette,
    pub(crate) icon_overrides: HashMap<Arc<str>, FileIconOverride>,
    pub(crate) show_arrows: bool,
    pub(crate) expanded_arrow: Arc<str>,
    pub(crate) collapsed_arrow: Arc<str>,
    pub(crate) arrow_style: Style,
    pub(crate) content_gap: u16,
    pub(crate) icon_gap: u16,
    pub(crate) indent_style: crate::widgets::IndentStyle,
    pub(crate) indent_guide_style: Style,
    pub(crate) indent_guide_start_depth: usize,
    pub(crate) indent_width: u16,
    pub(crate) directory_icon: Arc<str>,
    pub(crate) opened_directory_icon: Arc<str>,
    pub(crate) file_icon: Arc<str>,
    pub(crate) symlink_icon: Arc<str>,
    pub(crate) other_icon: Arc<str>,
    pub(crate) directory_label_style: Style,
    pub(crate) file_label_style: Style,
    pub(crate) hidden_style: Style,
    pub(crate) ignored_style: Style,
    pub(crate) loading_label: Arc<str>,
    pub(crate) error_prefix: Arc<str>,
    pub(crate) width: Length,
    pub(crate) height: Length,
    pub(crate) style: Style,
    pub(crate) hover_style: StyleSlot,
    pub(crate) item_hover_style: StyleSlot,
    pub(crate) selection_style: StyleSlot,
    pub(crate) unfocused_selection_style: StyleSlot,
    pub(crate) selected: Option<usize>,
    pub(crate) clear_selection: bool,
    pub(crate) selected_path: Option<Arc<str>>,
    pub(crate) reveal_path: Option<Arc<str>>,
    pub(crate) select_path: Option<Arc<str>>,
    pub(crate) force_scroll_to_selected: bool,
    pub(crate) expanded_paths: Option<HashSet<Arc<str>>>,
    pub(crate) initial_expanded_paths: HashSet<Arc<str>>,
    pub(crate) selection_symbol: Option<Arc<str>>,
    pub(crate) selection_symbol_style: Option<Style>,
    pub(crate) unfocused_selection_symbol_style: Option<Style>,
    pub(crate) scrollbar: bool,
    pub(crate) scrollbar_config: crate::style::ScrollbarConfig,
    pub(crate) scroll_keys: ScrollKeymap,
    pub(crate) scroll_wheel: bool,
    pub(crate) show_scroll_indicators: bool,
    pub(crate) scroll_indicator_style: Style,
    pub(crate) empty_text: Option<Arc<str>>,
    pub(crate) empty_text_style: Style,
    pub(crate) empty_text_padding: Padding,
    pub(crate) explorer: bool,
    pub(crate) explorer_max_matches: usize,
    pub(crate) tree_focus_key: Arc<str>,
    pub(crate) explorer_focus_key: Arc<str>,
    pub(crate) explorer_placeholder: Arc<str>,
    pub(crate) explorer_prefix: Arc<str>,
    pub(crate) explorer_input_border: bool,
    pub(crate) explorer_input_border_style: BorderStyle,
    pub(crate) explorer_input_padding: Padding,
    pub(crate) explorer_input_style: Style,
    pub(crate) explorer_input_focus_style: StyleSlot,
    pub(crate) explorer_input_focus_content_style: Style,
    pub(crate) explorer_placeholder_style: Style,
    pub(crate) explorer_focus_placeholder_style: Style,
    pub(crate) explorer_match_style: Style,
    pub(crate) explorer_divider: bool,
    pub(crate) explorer_divider_join_frame: bool,
    pub(crate) explorer_divider_char: char,
    pub(crate) explorer_divider_style: Style,
    pub(crate) on_explorer_focus: Option<Callback<super::FileTreeExplorerFocusOrigin>>,
    pub(crate) on_explorer_blur: Option<Callback<()>>,
    pub(crate) on_explorer_escape: Option<Callback<()>>,
    pub(crate) activate_on_click: bool,
    pub(crate) focusable: bool,
    pub(crate) tab_stop: bool,
    pub(crate) on_focus: Option<Callback<()>>,
    pub(crate) on_blur: Option<Callback<()>>,
    pub(crate) keymap: TreeKeymap,
    pub(crate) git_status: bool,
    pub(crate) highlight_changed_labels: bool,
    pub(crate) change_source: FileTreeChangeSource,
    pub(crate) change_view: FileTreeChangeView,
    pub(crate) compact_directories: bool,
    pub(crate) git_diff_stats: bool,
    pub(crate) git_icon_style: GitIconStyle,
    pub(crate) git_status_padding_right: u16,
    pub(crate) git_status_cache: Option<FileTreeGitStatusCache>,
    pub(crate) git_refresh_nonce: u64,
    pub(crate) git_marker_modified: Arc<str>,
    pub(crate) git_marker_added: Arc<str>,
    pub(crate) git_marker_deleted: Arc<str>,
    pub(crate) git_marker_renamed: Arc<str>,
    pub(crate) git_marker_untracked: Arc<str>,
    pub(crate) git_marker_conflicted: Arc<str>,
    pub(crate) git_style_modified: Option<Style>,
    pub(crate) git_style_added: Option<Style>,
    pub(crate) git_style_deleted: Option<Style>,
    pub(crate) git_style_renamed: Option<Style>,
    pub(crate) git_style_untracked: Option<Style>,
    pub(crate) git_style_conflicted: Option<Style>,
    pub(crate) change_suffix_style: Style,
    pub(crate) change_suffix_priority: FileTreeSuffixPriority,
    pub(crate) symlink_targets: bool,
    pub(crate) symlink_target_arrow: Arc<str>,
    pub(crate) symlink_target_style: Style,
    pub(crate) path_styles: HashMap<Arc<str>, FileTreeItemStyle>,
    pub(crate) on_select: Option<Callback<FileTreeEvent>>,
    pub(crate) on_activate: Option<Callback<FileTreeEvent>>,
    pub(crate) on_ctrl_activate: Option<Callback<FileTreeEvent>>,
    pub(crate) on_toggle: Option<Callback<FileTreeToggleEvent>>,
    pub(crate) on_entry_request: Option<Callback<super::events::FileTreeEntryRequest>>,
}

/// Resolved Git styles for a single view, without cloning filesystem props or provided entries.
#[derive(Clone, Copy)]
pub(super) struct GitStyles {
    pub(super) modified: Style,
    pub(super) added: Style,
    pub(super) deleted: Style,
    pub(super) renamed: Style,
    pub(super) untracked: Style,
    pub(super) conflicted: Style,
}

impl FileTreeProps {
    pub(super) fn git_styles(&self, theme: &crate::style::Theme) -> GitStyles {
        GitStyles {
            modified: self
                .git_style_modified
                .unwrap_or_else(|| Style::new().fg(theme.git_status.modified)),
            added: self
                .git_style_added
                .unwrap_or_else(|| Style::new().fg(theme.git_status.added)),
            deleted: self
                .git_style_deleted
                .unwrap_or_else(|| Style::new().fg(theme.git_status.deleted)),
            renamed: self
                .git_style_renamed
                .unwrap_or_else(|| Style::new().fg(theme.git_status.renamed)),
            untracked: self
                .git_style_untracked
                .unwrap_or_else(|| Style::new().fg(theme.git_status.untracked)),
            conflicted: self
                .git_style_conflicted
                .unwrap_or_else(|| Style::new().fg(theme.git_status.conflicted)),
        }
    }
}

#[cfg(test)]
mod consumer_contract_tests {
    use super::*;
    use crate::style::{Color, Theme};
    use crate::widgets::FileTree;

    fn styles(styles: GitStyles) -> [Style; 6] {
        [
            styles.modified,
            styles.added,
            styles.deleted,
            styles.renamed,
            styles.untracked,
            styles.conflicted,
        ]
    }

    fn colors(theme: &Theme) -> [Style; 6] {
        let p = theme.git_status;
        [
            p.modified,
            p.added,
            p.deleted,
            p.renamed,
            p.untracked,
            p.conflicted,
        ]
        .map(|color| Style::new().fg(color))
    }

    #[test]
    fn git_defaults_follow_each_active_theme_without_mutating_overrides() {
        let props = FileTree::new("/repo").props;
        let first = Theme::default();
        let mut second = first.clone();
        second.git_status = crate::style::GitStatusPalette {
            modified: Color::Red,
            added: Color::Blue,
            deleted: Color::Green,
            renamed: Color::Yellow,
            untracked: Color::Cyan,
            conflicted: Color::Magenta,
        };
        assert_eq!(styles(props.git_styles(&first)), colors(&first));
        assert_eq!(styles(props.git_styles(&second)), colors(&second));
        assert_eq!(styles(props.git_styles(&first)), colors(&first));
        assert!(props.git_style_modified.is_none());
    }

    #[test]
    fn every_explicit_git_style_including_empty_replaces_the_theme() {
        let explicit = Style::new().bold();
        let props = FileTree::new("/repo")
            .git_style_modified(explicit)
            .git_style_added(explicit)
            .git_style_deleted(explicit)
            .git_style_renamed(explicit)
            .git_style_untracked(explicit)
            .git_style_conflicted(Style::default())
            .props;
        let theme = Theme::default();
        assert_eq!(
            styles(props.git_styles(&theme)),
            [
                explicit,
                explicit,
                explicit,
                explicit,
                explicit,
                Style::default()
            ]
        );
    }

    #[test]
    fn partial_git_overrides_survive_theme_switches() {
        let explicit = Style::new().fg(Color::White).italic();
        let props = FileTree::new("/repo").git_style_modified(explicit).props;
        let mut theme = Theme::default();
        for color in [Color::Red, Color::Blue] {
            theme.git_status.added = color;
            let resolved = props.git_styles(&theme);
            assert_eq!(resolved.modified, explicit);
            assert_eq!(resolved.added, Style::new().fg(color));
            assert_eq!(
                resolved.conflicted,
                Style::new().fg(theme.git_status.conflicted)
            );
        }
    }
}

#[cfg(test)]
mod theme_runtime_tests {
    use crate::core::component::{Component, Context, Update};
    use crate::core::element::Element;
    use crate::core::node::NodeKind;
    use crate::style::{Color, Style, Theme};
    use crate::test_backend::TestBackend;
    use crate::widgets::{
        FileTree, FileTreeChange, FileTreeChangeSource, FileTreeChangeStatus, FileTreeEntrySource,
        FileTreeRefreshPolicy, GitIconStyle, ThemeProvider,
    };

    struct Probe;
    impl Component for Probe {
        type Message = Color;
        type Properties = ();
        type State = Color;
        fn create_state(&self, _: &()) -> Color {
            Color::Red
        }
        fn update(&mut self, color: Color, ctx: &mut Context<Self>) -> Update {
            ctx.state = color;
            Update::full()
        }
        fn view(&self, ctx: &Context<Self>) -> Element {
            let mut theme = Theme::default();
            theme.git_status.modified = ctx.state;
            ThemeProvider::new(theme)
                .child(
                    FileTree::new("/provided")
                        .entry_source(FileTreeEntrySource::provided([
                            crate::FileTreeDirectoryListing::new(
                                "/provided",
                                [
                                    crate::FileTreeEntry::file("modified"),
                                    crate::FileTreeEntry::file("added"),
                                ],
                            ),
                        ]))
                        .refresh_policy(FileTreeRefreshPolicy::Manual)
                        .change_source(FileTreeChangeSource::provided([
                            FileTreeChange::new("modified", FileTreeChangeStatus::Modified),
                            FileTreeChange::new("added", FileTreeChangeStatus::Added),
                        ]))
                        .change_view(crate::FileTreeChangeView::ChangedOnly)
                        .git_status(true)
                        .git_icon_style(GitIconStyle::Text)
                        .git_style_added(Style::default())
                        .expanded_paths(["/provided"]),
                )
                .into()
        }
    }

    fn marker_style(backend: &TestBackend<Probe>, marker: &str) -> Style {
        backend
            .core
            .tree
            .iter()
            .find_map(|node| {
                let NodeKind::List(list) = &node.kind else {
                    return None;
                };
                list.items
                    .iter()
                    .flat_map(|item| &item.description_spans)
                    .find(|span| span.content.as_ref() == marker)
                    .map(|span| span.style)
            })
            .unwrap_or_else(|| panic!("missing marker {marker}"))
    }

    #[test]
    fn a_mounted_filetree_tracks_scoped_theme_changes_and_keeps_empty_overrides() {
        let mut backend = TestBackend::new(Probe);
        assert_eq!(marker_style(&backend, "M"), Style::new().fg(Color::Red));
        assert_eq!(marker_style(&backend, "A"), Style::default());
        backend.dispatch(Color::Blue).unwrap();
        assert_eq!(marker_style(&backend, "M"), Style::new().fg(Color::Blue));
        assert_eq!(marker_style(&backend, "A"), Style::default());
    }
}
