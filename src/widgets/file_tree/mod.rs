//! Lazy file tree widget.

mod component;
mod events;
mod explorer;
mod fs;
mod git;
mod mod_private;

pub use crate::style::FileIconPalette;
pub use events::{FileTreeEvent, FileTreeToggleEvent};
pub use fs::{FileIconStyle, FileKind};
pub use git::{GitChangeState, GitFileStatus, GitIconStyle};
pub(crate) use mod_private::FileTreeProps;
use mod_private::{
    default_git_style_added, default_git_style_conflicted, default_git_style_deleted,
    default_git_style_modified, default_git_style_renamed, default_git_style_untracked,
};

use crate::callback::Callback;
use crate::core::element::Element;
use crate::style::{BorderStyle, Color, Length, Padding, ScrollbarConfig, Style, StyleSlot};
use crate::widgets::{ScrollKeymap, TreeKeymap};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub use crate::utils::file_icons::FileIconOverride;

/// Optional style decorations for an exact [`FileTree`] path.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileTreeItemStyle {
    /// Style applied to the full row.
    pub row: Option<Style>,
    /// Style patched onto the icon span.
    pub icon: Option<Style>,
    /// Style patched onto label spans before search highlighting.
    pub label: Option<Style>,
    /// Style patched onto right-aligned change metadata spans.
    pub suffix: Option<Style>,
}

impl FileTreeItemStyle {
    /// Create an empty item style decoration.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the full-row style.
    pub fn row(mut self, style: Style) -> Self {
        self.row = Some(style);
        self
    }

    /// Set the icon style patch.
    pub fn icon(mut self, style: Style) -> Self {
        self.icon = Some(style);
        self
    }

    /// Set the label style patch.
    pub fn label(mut self, style: Style) -> Self {
        self.label = Some(style);
        self
    }

    /// Set the right-side change metadata style patch.
    pub fn suffix(mut self, style: Style) -> Self {
        self.suffix = Some(style);
        self
    }
}

/// Truncation priority for right-aligned FileTree change metadata.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FileTreeSuffixPriority {
    /// Prefer keeping the file/directory label visible and truncate suffix metadata first.
    #[default]
    Label,
    /// Prefer keeping right-aligned suffix metadata visible and truncate the label first.
    Suffix,
}

/// Source-agnostic file tree change display mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FileTreeChangeView {
    /// Show all files under the configured root.
    #[default]
    AllFiles,
    /// Show only changed files and ancestor directories needed to group them.
    ChangedOnly,
}

/// Compatibility alias for the previous Git-specific display mode name.
pub type FileTreeGitView = FileTreeChangeView;

/// Source used for file change decorations and changed-only projection.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FileTreeChangeSource {
    /// Discover changes from the local Git repository containing the tree root.
    #[default]
    Git,
    /// Use an app/server-provided virtual change set without local Git discovery.
    Provided(Vec<FileTreeChange>),
}

impl FileTreeChangeSource {
    /// Create a provided virtual change source.
    pub fn provided(changes: impl IntoIterator<Item = FileTreeChange>) -> Self {
        Self::Provided(changes.into_iter().collect())
    }
}

/// Status for a source-agnostic changed file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileTreeChangeStatus {
    /// File was modified.
    Modified,
    /// File was added.
    Added,
    /// File was deleted.
    Deleted,
    /// File was renamed.
    Renamed,
    /// File is untracked/new to the source.
    Untracked,
    /// File has a conflict.
    Conflicted,
}

impl From<FileTreeChangeStatus> for GitChangeState {
    fn from(status: FileTreeChangeStatus) -> Self {
        match status {
            FileTreeChangeStatus::Modified => Self::Modified,
            FileTreeChangeStatus::Added => Self::Added,
            FileTreeChangeStatus::Deleted => Self::Deleted,
            FileTreeChangeStatus::Renamed => Self::Renamed,
            FileTreeChangeStatus::Untracked => Self::Untracked,
            FileTreeChangeStatus::Conflicted => Self::Conflicted,
        }
    }
}

/// App/server-provided file change entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTreeChange {
    /// Changed path, either relative to the tree root or absolute under it.
    pub path: Arc<str>,
    /// Change status used for markers and styling.
    pub status: FileTreeChangeStatus,
    /// Optional virtual file kind. Defaults to `FileKind::File` for leaf rows.
    pub kind: Option<FileKind>,
    /// Added-line count for diff stats.
    pub additions: usize,
    /// Deleted-line count for diff stats.
    pub deletions: usize,
    /// Whether the status should render as staged. Defaults to unstaged.
    pub staged: bool,
}

impl FileTreeChange {
    /// Create a changed file entry.
    pub fn new(path: impl Into<Arc<str>>, status: FileTreeChangeStatus) -> Self {
        Self {
            path: path.into(),
            status,
            kind: None,
            additions: 0,
            deletions: 0,
            staged: false,
        }
    }

    /// Set the virtual file kind for this changed path.
    pub fn kind(mut self, kind: FileKind) -> Self {
        self.kind = Some(kind);
        self
    }

    /// Set diff statistics for this changed path.
    pub fn diff_stat(mut self, additions: usize, deletions: usize) -> Self {
        self.additions = additions;
        self.deletions = deletions;
        self
    }

    /// Set the added-line count for this changed path.
    pub fn additions(mut self, additions: usize) -> Self {
        self.additions = additions;
        self
    }

    /// Set the deleted-line count for this changed path.
    pub fn deletions(mut self, deletions: usize) -> Self {
        self.deletions = deletions;
        self
    }

    /// Mark this change as staged or unstaged.
    pub fn staged(mut self, staged: bool) -> Self {
        self.staged = staged;
        self
    }
}

/// Lazy-loading file explorer tree.
#[derive(Clone)]
pub struct FileTree {
    props: FileTreeProps,
}

impl FileTree {
    /// Create a new file tree rooted at `root`.
    pub fn new(root: impl Into<Arc<str>>) -> Self {
        Self {
            props: FileTreeProps {
                root: root.into(),
                show_hidden: false,
                max_entries_per_dir: 2_000,
                show_icons: true,
                icon_style: FileIconStyle::default(),
                icon_palette: FileIconPalette::default(),
                icon_overrides: HashMap::new(),
                show_arrows: true,
                indent_style: crate::widgets::IndentStyle::None,
                indent_guide_style: Style::default(),
                directory_icon: "[D]".into(),
                opened_directory_icon: "[D]".into(),
                file_icon: "[F]".into(),
                symlink_icon: "[L]".into(),
                other_icon: "[?]".into(),
                directory_label_style: Style::default(),
                file_label_style: Style::default(),
                loading_label: "loading...".into(),
                error_prefix: "error:".into(),
                width: Length::Flex(1),
                height: Length::Flex(1),
                style: Style::default(),
                hover_style: StyleSlot::Inherit,
                item_hover_style: StyleSlot::Inherit,
                selection_style: StyleSlot::Inherit,
                unfocused_selection_style: StyleSlot::Inherit,
                selected: None,
                selected_path: None,
                reveal_path: None,
                select_path: None,
                force_scroll_to_selected: false,
                expanded_paths: None,
                selection_symbol: None,
                selection_symbol_style: None,
                unfocused_selection_symbol_style: None,
                scrollbar: true,
                scrollbar_config: ScrollbarConfig::default(),
                scroll_keys: ScrollKeymap::default(),
                scroll_wheel: true,
                show_scroll_indicators: false,
                scroll_indicator_style: Style::default(),
                empty_text: Some("Directory is empty".into()),
                empty_text_style: Style::default(),
                explorer: false,
                explorer_placeholder: "Find files...".into(),
                explorer_prefix: " ".into(),
                explorer_input_border: false,
                explorer_input_border_style: BorderStyle::Plain,
                explorer_input_padding: Padding {
                    left: 1,
                    right: 0,
                    top: 0,
                    bottom: 0,
                },
                explorer_input_style: Style::default(),
                explorer_input_focus_style: StyleSlot::Inherit,
                explorer_input_focus_content_style: Style::default(),
                explorer_placeholder_style: Style::default(),
                explorer_focus_placeholder_style: Style::default(),
                explorer_match_style: Style::default(),
                explorer_divider: true,
                explorer_divider_join_frame: true,
                explorer_divider_char: '─',
                explorer_divider_style: Style::default(),
                activate_on_click: true,
                focusable: true,
                tab_stop: true,
                on_focus: None,
                on_blur: None,
                keymap: TreeKeymap::default(),
                git_status: true,
                highlight_changed_labels: false,
                change_source: FileTreeChangeSource::default(),
                change_view: FileTreeChangeView::default(),
                git_diff_stats: false,
                git_icon_style: GitIconStyle::NerdFont,
                git_refresh_nonce: 0,
                git_marker_modified: "M".into(),
                git_marker_added: "A".into(),
                git_marker_deleted: "D".into(),
                git_marker_renamed: "R".into(),
                git_marker_untracked: "?".into(),
                git_marker_conflicted: "!".into(),
                // Git status colors - these are defaults that work standalone
                // ThemeProvider will override these if they match the defaults
                git_style_modified: default_git_style_modified(),
                git_style_added: default_git_style_added(),
                git_style_deleted: default_git_style_deleted(),
                git_style_renamed: default_git_style_renamed(),
                git_style_untracked: default_git_style_untracked(),
                git_style_conflicted: default_git_style_conflicted(),
                change_suffix_style: Style::default(),
                change_suffix_priority: FileTreeSuffixPriority::default(),
                path_styles: HashMap::new(),
                on_select: None,
                on_activate: None,
                on_toggle: None,
            },
        }
    }

    /// Toggle hidden entries (dotfiles).
    pub fn show_hidden(mut self, show_hidden: bool) -> Self {
        self.props.show_hidden = show_hidden;
        self
    }

    /// Set max number of children loaded per directory.
    pub fn max_entries_per_dir(mut self, max_entries: usize) -> Self {
        self.props.max_entries_per_dir = max_entries.max(1);
        self
    }

    /// Toggle icon rendering.
    pub fn show_icons(mut self, show_icons: bool) -> Self {
        self.props.show_icons = show_icons;
        self
    }

    /// Set icon style for file tree items.
    pub fn icon_style(mut self, style: FileIconStyle) -> Self {
        self.props.icon_style = style;
        self
    }

    /// Set the color palette for file icons.
    pub fn icon_palette(mut self, palette: FileIconPalette) -> Self {
        self.props.icon_palette = palette;
        self
    }

    /// Add a custom icon override for a file extension or name.
    ///
    /// The `pattern` can be a file extension (e.g., "rs", "md") or a full filename (e.g., "README.md").
    /// The icon will be used for files matching this pattern.
    pub fn icon_override(
        mut self,
        pattern: impl Into<Arc<str>>,
        icon: impl Into<Arc<str>>,
        color: Option<Color>,
    ) -> Self {
        self.props.icon_overrides.insert(
            pattern.into(),
            FileIconOverride {
                icon: icon.into(),
                color,
            },
        );
        self
    }

    /// Toggle expansion arrows before directories.
    pub fn show_arrows(mut self, show: bool) -> Self {
        self.props.show_arrows = show;
        self
    }

    /// Set style of indentation guides.
    pub fn indent_style(mut self, style: crate::widgets::IndentStyle) -> Self {
        self.props.indent_style = style;
        self
    }

    /// Set style for indent guides.
    pub fn indent_guide_style(mut self, style: Style) -> Self {
        self.props.indent_guide_style = style;
        self
    }

    /// Set directory icon.
    pub fn directory_icon(mut self, icon: impl Into<Arc<str>>) -> Self {
        self.props.directory_icon = icon.into();
        self
    }

    /// Set "opened directory" icon.
    pub fn opened_directory_icon(mut self, icon: impl Into<Arc<str>>) -> Self {
        self.props.opened_directory_icon = icon.into();
        self
    }

    /// Set regular file icon.
    pub fn file_icon(mut self, icon: impl Into<Arc<str>>) -> Self {
        self.props.file_icon = icon.into();
        self
    }

    /// Set symlink icon.
    pub fn symlink_icon(mut self, icon: impl Into<Arc<str>>) -> Self {
        self.props.symlink_icon = icon.into();
        self
    }

    /// Set "other type" icon.
    pub fn other_icon(mut self, icon: impl Into<Arc<str>>) -> Self {
        self.props.other_icon = icon.into();
        self
    }

    /// Set the label style for directory rows.
    pub fn directory_label_style(mut self, style: Style) -> Self {
        self.props.directory_label_style = style;
        self
    }

    /// Set the label style for regular file rows.
    pub fn file_label_style(mut self, style: Style) -> Self {
        self.props.file_label_style = style;
        self
    }

    /// Set item decorations for an exact path under the effective root.
    pub fn path_style(mut self, path: impl Into<Arc<str>>, style: FileTreeItemStyle) -> Self {
        self.props.path_styles.insert(path.into(), style);
        self
    }

    /// Set item decorations for exact paths under the effective root.
    pub fn path_styles(
        mut self,
        styles: impl IntoIterator<Item = (impl Into<Arc<str>>, FileTreeItemStyle)>,
    ) -> Self {
        self.props
            .path_styles
            .extend(styles.into_iter().map(|(path, style)| (path.into(), style)));
        self
    }

    /// Set loading row label.
    pub fn loading_label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.props.loading_label = label.into();
        self
    }

    /// Set load-error prefix.
    pub fn error_prefix(mut self, prefix: impl Into<Arc<str>>) -> Self {
        self.props.error_prefix = prefix.into();
        self
    }

    /// Set width.
    pub fn width(mut self, width: Length) -> Self {
        self.props.width = width;
        self
    }

    /// Set height.
    pub fn height(mut self, height: Length) -> Self {
        self.props.height = height;
        self
    }

    /// Set base style.
    pub fn style(mut self, style: Style) -> Self {
        self.props.style = style;
        self
    }

    /// Set hovered style.
    pub fn hover_style(mut self, style: Style) -> Self {
        self.props.hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hovered style.
    pub fn extend_hover_style(mut self, style: Style) -> Self {
        self.props.hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed hovered style.
    pub fn inherit_hover_style(mut self) -> Self {
        self.props.hover_style = StyleSlot::Inherit;
        self
    }

    /// Set hover style slot directly for composite forwarding.
    pub fn hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.hover_style = slot;
        self
    }

    /// Set hovered row style.
    pub fn item_hover_style(mut self, style: Style) -> Self {
        self.props.item_hover_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed hovered row style.
    pub fn extend_item_hover_style(mut self, style: Style) -> Self {
        self.props.item_hover_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed hovered row style.
    pub fn inherit_item_hover_style(mut self) -> Self {
        self.props.item_hover_style = StyleSlot::Inherit;
        self
    }

    /// Set item-hover style slot directly for composite forwarding.
    pub fn item_hover_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.item_hover_style = slot;
        self
    }

    /// Set selected row style.
    pub fn selection_style(mut self, style: Style) -> Self {
        self.props.selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed selected row style.
    pub fn extend_selection_style(mut self, style: Style) -> Self {
        self.props.selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed selected row style.
    pub fn inherit_selection_style(mut self) -> Self {
        self.props.selection_style = StyleSlot::Inherit;
        self
    }

    /// Set selected row style slot directly for composite forwarding.
    pub fn selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.selection_style = slot;
        self
    }

    /// Set selected row style while the file tree is not focused.
    pub fn unfocused_selection_style(mut self, style: Style) -> Self {
        self.props.unfocused_selection_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed selected row style while the file tree is not focused.
    pub fn extend_unfocused_selection_style(mut self, style: Style) -> Self {
        self.props.unfocused_selection_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed selected row style while the file tree is not focused.
    pub fn inherit_unfocused_selection_style(mut self) -> Self {
        self.props.unfocused_selection_style = StyleSlot::Inherit;
        self
    }

    /// Set unfocused selected row style slot directly for composite forwarding.
    pub fn unfocused_selection_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.unfocused_selection_style = slot;
        self
    }

    /// Set the selected visible row index.
    pub fn selected(mut self, selected: usize) -> Self {
        self.props.selected = Some(selected);
        self
    }

    /// Select a visible row by path.
    ///
    /// The path may be absolute under the tree root or relative to it. Selection is a no-op when
    /// the normalized path is outside the root or is not present in the current visible projection.
    pub fn selected_path(mut self, path: impl Into<Arc<str>>) -> Self {
        self.props.selected_path = Some(path.into());
        self
    }

    /// Reveal a path by expanding/loading ancestor directories when possible.
    ///
    /// The path may be absolute under the tree root or relative to it. Revealing is a no-op for
    /// paths outside the root, paths hidden by `show_hidden(false)`, unreadable directories, capped
    /// directory entries, and paths absent from the active all-files/changed-only projection.
    pub fn reveal_path(mut self, path: impl Into<Arc<str>>) -> Self {
        self.props.reveal_path = Some(path.into());
        self
    }

    /// Reveal and select a path, forcing scroll to the row when it is visible.
    ///
    /// This combines `reveal_path` and `selected_path` behavior. With controlled
    /// `expanded_paths`, the path can only be revealed when the controlled expansion set (plus this
    /// reveal request for rendering) makes the ancestors available to load.
    pub fn select_path(mut self, path: impl Into<Arc<str>>) -> Self {
        self.props.select_path = Some(path.into());
        self
    }

    /// Force scroll to make the selected item visible on next render.
    pub fn force_scroll_to_selected(mut self, force: bool) -> Self {
        self.props.force_scroll_to_selected = force;
        self
    }

    /// Control expanded directory paths.
    pub fn expanded_paths(mut self, paths: impl IntoIterator<Item = impl Into<Arc<str>>>) -> Self {
        self.props.expanded_paths = Some(paths.into_iter().map(Into::into).collect::<HashSet<_>>());
        self
    }

    /// Set selected row prefix symbol.
    pub fn selection_symbol(mut self, symbol: Option<impl Into<Arc<str>>>) -> Self {
        self.props.selection_symbol = symbol.map(Into::into);
        self
    }

    /// Set selected row prefix style.
    pub fn selection_symbol_style(mut self, style: Option<Style>) -> Self {
        self.props.selection_symbol_style = style;
        self
    }

    /// Set selected row prefix style while the file tree is not focused.
    pub fn unfocused_selection_symbol_style(mut self, style: Option<Style>) -> Self {
        self.props.unfocused_selection_symbol_style = style;
        self
    }

    /// Toggle scrollbar.
    pub fn scrollbar(mut self, scrollbar: bool) -> Self {
        self.props.scrollbar = scrollbar;
        self
    }

    /// Set scrollbar configuration.
    pub fn scrollbar_config(mut self, config: ScrollbarConfig) -> Self {
        self.props.scrollbar_config = config;
        self
    }

    /// Configure keyboard scrolling bindings.
    pub fn scroll_keys(mut self, keys: ScrollKeymap) -> Self {
        self.props.scroll_keys = keys;
        self
    }

    /// Enable mouse wheel scrolling.
    pub fn scroll_wheel(mut self, enabled: bool) -> Self {
        self.props.scroll_wheel = enabled;
        self
    }

    /// Toggle hidden-row indicators (`N more`).
    pub fn show_scroll_indicators(mut self, show: bool) -> Self {
        self.props.show_scroll_indicators = show;
        self
    }

    /// Set hidden-row indicator style.
    pub fn scroll_indicator_style(mut self, style: Style) -> Self {
        self.props.scroll_indicator_style = style;
        self
    }

    /// Set empty-state text.
    pub fn empty_text(mut self, text: impl Into<Arc<str>>) -> Self {
        self.props.empty_text = Some(text.into());
        self
    }

    /// Set empty-state style.
    pub fn empty_text_style(mut self, style: Style) -> Self {
        self.props.empty_text_style = style;
        self
    }

    /// Toggle explorer filter input above the tree.
    pub fn explorer(mut self, explorer: bool) -> Self {
        self.props.explorer = explorer;
        self
    }

    /// Set explorer placeholder text.
    pub fn explorer_placeholder(mut self, text: impl Into<Arc<str>>) -> Self {
        self.props.explorer_placeholder = text.into();
        self
    }

    /// Set explorer input prefix text.
    pub fn explorer_prefix(mut self, prefix: impl Into<Arc<str>>) -> Self {
        self.props.explorer_prefix = prefix.into();
        self
    }

    /// Toggle explorer input border.
    pub fn explorer_input_border(mut self, border: bool) -> Self {
        self.props.explorer_input_border = border;
        self
    }

    /// Set explorer input border style.
    pub fn explorer_input_border_style(mut self, style: BorderStyle) -> Self {
        self.props.explorer_input_border_style = style;
        self
    }

    /// Set explorer input padding.
    pub fn explorer_input_padding(mut self, padding: impl Into<Padding>) -> Self {
        self.props.explorer_input_padding = padding.into();
        self
    }

    /// Set explorer input style.
    pub fn explorer_input_style(mut self, style: Style) -> Self {
        self.props.explorer_input_style = style;
        self
    }

    /// Set explorer input style when focused.
    pub fn explorer_input_focus_style(mut self, style: Style) -> Self {
        self.props.explorer_input_focus_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the themed explorer input focus style.
    pub fn extend_explorer_input_focus_style(mut self, style: Style) -> Self {
        self.props.explorer_input_focus_style = StyleSlot::Extend(style);
        self
    }

    /// Inherit the themed explorer input focus style.
    pub fn inherit_explorer_input_focus_style(mut self) -> Self {
        self.props.explorer_input_focus_style = StyleSlot::Inherit;
        self
    }

    /// Set explorer input focus style slot directly for composite forwarding.
    pub fn explorer_input_focus_style_slot(mut self, slot: StyleSlot) -> Self {
        self.props.explorer_input_focus_style = slot;
        self
    }

    /// Set focused explorer input content text style.
    pub fn explorer_input_focus_content_style(mut self, style: Style) -> Self {
        self.props.explorer_input_focus_content_style = style;
        self
    }

    /// Set explorer placeholder style.
    pub fn explorer_placeholder_style(mut self, style: Style) -> Self {
        self.props.explorer_placeholder_style = style;
        self
    }

    /// Set explorer placeholder style when focused.
    pub fn explorer_focus_placeholder_style(mut self, style: Style) -> Self {
        self.props.explorer_focus_placeholder_style = style;
        self
    }

    /// Set explorer search match highlight style.
    pub fn explorer_match_style(mut self, style: Style) -> Self {
        self.props.explorer_match_style = style;
        self
    }

    /// Toggle divider below the explorer input.
    pub fn explorer_divider(mut self, show: bool) -> Self {
        self.props.explorer_divider = show;
        self
    }

    /// Toggle frame-join behavior for explorer divider.
    pub fn explorer_divider_join_frame(mut self, join: bool) -> Self {
        self.props.explorer_divider_join_frame = join;
        self
    }

    /// Set divider character for explorer divider.
    pub fn explorer_divider_char(mut self, ch: char) -> Self {
        self.props.explorer_divider_char = ch;
        self
    }

    /// Set explorer divider style.
    pub fn explorer_divider_style(mut self, style: Style) -> Self {
        self.props.explorer_divider_style = style;
        self
    }

    /// Set tree activation behavior for mouse clicks.
    pub fn activate_on_click(mut self, activate_on_click: bool) -> Self {
        self.props.activate_on_click = activate_on_click;
        self
    }

    /// Control focusability.
    pub fn focusable(mut self, focusable: bool) -> Self {
        self.props.focusable = focusable;
        self
    }

    /// Control whether the file tree participates in Tab / Shift+Tab traversal.
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.props.tab_stop = tab_stop;
        self
    }

    /// Set the callback fired when the file tree gains focus.
    pub fn on_focus(mut self, cb: Callback<()>) -> Self {
        self.props.on_focus = Some(cb);
        self
    }

    /// Set the callback fired when the file tree loses focus.
    pub fn on_blur(mut self, cb: Callback<()>) -> Self {
        self.props.on_blur = Some(cb);
        self
    }

    /// Configure expand/collapse keymap.
    pub fn keymap(mut self, keymap: TreeKeymap) -> Self {
        self.props.keymap = keymap;
        self
    }

    /// Toggle Git status decorations.
    pub fn git_status(mut self, enabled: bool) -> Self {
        self.props.git_status = enabled;
        self
    }

    /// Toggle applying change status styles to file and directory labels.
    ///
    /// Status indicators remain styled independently. The default is `false`, so
    /// file names keep their regular file-kind styling while dirty state is shown
    /// in the right-aligned metadata.
    pub fn highlight_changed_labels(mut self, enabled: bool) -> Self {
        self.props.highlight_changed_labels = enabled;
        self
    }

    /// Set the changed-file display mode using the Git-compatible alias.
    pub fn git_view(mut self, view: FileTreeGitView) -> Self {
        self.props.change_view = view;
        self
    }

    /// Set the source used for changed-file decorations and changed-only projection.
    pub fn change_source(mut self, source: FileTreeChangeSource) -> Self {
        self.props.change_source = source;
        self
    }

    /// Set the source-agnostic changed-file display mode.
    pub fn change_view(mut self, view: FileTreeChangeView) -> Self {
        self.props.change_view = view;
        self
    }

    /// Toggle changed-only mode using the Git-compatible builder name.
    pub fn git_changed_only(mut self, enabled: bool) -> Self {
        self.props.change_view = if enabled {
            FileTreeChangeView::ChangedOnly
        } else {
            FileTreeChangeView::AllFiles
        };
        self
    }

    /// Toggle diff statistics decorations for any change source.
    pub fn show_diff_stats(self, enabled: bool) -> Self {
        self.git_diff_stats(enabled)
    }

    /// Toggle diff statistics decorations using the Git-compatible builder name.
    pub fn git_diff_stats(mut self, enabled: bool) -> Self {
        self.props.git_diff_stats = enabled;
        self
    }

    /// Set icon style for Git status indicators.
    pub fn git_icon_style(mut self, style: GitIconStyle) -> Self {
        self.props.git_icon_style = style;
        self
    }

    /// Request an immediate Git status refresh.
    ///
    /// Call this when building the widget in response to a user action.
    pub fn refresh_git_status(mut self) -> Self {
        use std::sync::atomic::{AtomicU64, Ordering};
        static FILE_TREE_REFRESH_NONCE: AtomicU64 = AtomicU64::new(1);
        self.props.git_refresh_nonce = FILE_TREE_REFRESH_NONCE.fetch_add(1, Ordering::Relaxed);
        self
    }

    /// Set explicit refresh token for Git status loading.
    ///
    /// When the token changes, the component triggers a new background refresh.
    pub fn git_refresh_token(mut self, token: u64) -> Self {
        self.props.git_refresh_nonce = token;
        self
    }

    /// Set marker for `Modified` git status.
    pub fn git_marker_modified(mut self, marker: impl Into<Arc<str>>) -> Self {
        self.props.git_marker_modified = marker.into();
        self
    }

    /// Set marker for `Added` git status.
    pub fn git_marker_added(mut self, marker: impl Into<Arc<str>>) -> Self {
        self.props.git_marker_added = marker.into();
        self
    }

    /// Set marker for `Deleted` git status.
    pub fn git_marker_deleted(mut self, marker: impl Into<Arc<str>>) -> Self {
        self.props.git_marker_deleted = marker.into();
        self
    }

    /// Set marker for `Renamed` git status.
    pub fn git_marker_renamed(mut self, marker: impl Into<Arc<str>>) -> Self {
        self.props.git_marker_renamed = marker.into();
        self
    }

    /// Set marker for `Untracked` git status.
    pub fn git_marker_untracked(mut self, marker: impl Into<Arc<str>>) -> Self {
        self.props.git_marker_untracked = marker.into();
        self
    }

    /// Set marker for `Conflicted` git status.
    pub fn git_marker_conflicted(mut self, marker: impl Into<Arc<str>>) -> Self {
        self.props.git_marker_conflicted = marker.into();
        self
    }

    /// Set style for `Modified` git status marker.
    pub fn git_style_modified(mut self, style: Style) -> Self {
        self.props.git_style_modified = style;
        self
    }

    /// Set style for `Added` git status marker.
    pub fn git_style_added(mut self, style: Style) -> Self {
        self.props.git_style_added = style;
        self
    }

    /// Set style for `Deleted` git status marker.
    pub fn git_style_deleted(mut self, style: Style) -> Self {
        self.props.git_style_deleted = style;
        self
    }

    /// Set style for `Renamed` git status marker.
    pub fn git_style_renamed(mut self, style: Style) -> Self {
        self.props.git_style_renamed = style;
        self
    }

    /// Set style for `Untracked` git status marker.
    pub fn git_style_untracked(mut self, style: Style) -> Self {
        self.props.git_style_untracked = style;
        self
    }

    /// Set style for `Conflicted` git status marker.
    pub fn git_style_conflicted(mut self, style: Style) -> Self {
        self.props.git_style_conflicted = style;
        self
    }

    /// Set a source-agnostic style patch for right-aligned change metadata.
    pub fn change_suffix_style(mut self, style: Style) -> Self {
        self.props.change_suffix_style = style;
        self
    }

    /// Set a Git-compatible style patch for right-aligned change metadata.
    pub fn git_suffix_style(self, style: Style) -> Self {
        self.change_suffix_style(style)
    }

    /// Set whether labels or right-aligned change metadata win when rows are narrow.
    pub fn change_suffix_priority(mut self, priority: FileTreeSuffixPriority) -> Self {
        self.props.change_suffix_priority = priority;
        self
    }

    /// Set Git-compatible truncation priority for right-aligned change metadata.
    pub fn git_suffix_priority(self, priority: FileTreeSuffixPriority) -> Self {
        self.change_suffix_priority(priority)
    }

    /// Set selection callback.
    pub fn on_select(mut self, cb: Callback<FileTreeEvent>) -> Self {
        self.props.on_select = Some(cb);
        self
    }

    /// Fired when a row is activated (Enter, or click when `activate_on_click` is true).
    pub fn on_activate(mut self, cb: Callback<FileTreeEvent>) -> Self {
        self.props.on_activate = Some(cb);
        self
    }

    /// Set expand/collapse callback.
    pub fn on_toggle(mut self, cb: Callback<FileTreeToggleEvent>) -> Self {
        self.props.on_toggle = Some(cb);
        self
    }
}

impl From<FileTree> for Element {
    fn from(file_tree: FileTree) -> Self {
        let root_key = file_tree.props.root.clone();
        crate::child(component::FileTreeComponent::new, file_tree.props).key(root_key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{HashMap, HashSet};
    use std::sync::Arc;

    #[test]
    fn parses_untracked_line() {
        use git::GitChangeState;
        use git::parse_git_porcelain_line;
        let parsed = parse_git_porcelain_line("?? src/new.rs");
        assert_eq!(
            parsed.map(|(p, s)| (p, s.unstaged)),
            Some(("src/new.rs", Some(GitChangeState::Untracked)))
        );
    }

    #[test]
    fn parses_rename_destination() {
        use git::GitChangeState;
        use git::parse_git_porcelain_line;
        let parsed = parse_git_porcelain_line("R  old/name.rs -> new/name.rs");
        assert_eq!(
            parsed.map(|(p, s)| (p, s.staged)),
            Some(("new/name.rs", Some(GitChangeState::Renamed)))
        );
    }

    #[test]
    fn conflicting_status_wins_priority() {
        use git::insert_status;
        use git::{GitChangeState, GitFileStatus};
        let mut statuses = HashMap::new();
        let key: Arc<str> = "/repo/src/app.rs".into();

        insert_status(
            &mut statuses,
            key.clone(),
            GitFileStatus::new(None, Some(GitChangeState::Modified)),
        );
        insert_status(
            &mut statuses,
            key.clone(),
            GitFileStatus::new(None, Some(GitChangeState::Conflicted)),
        );
        insert_status(
            &mut statuses,
            key.clone(),
            GitFileStatus::new(None, Some(GitChangeState::Added)),
        );

        assert_eq!(
            statuses.get(key.as_ref()).copied().and_then(|s| s.unstaged),
            Some(GitChangeState::Conflicted)
        );
    }

    #[test]
    fn parses_numstat_line() {
        use git::parse_git_numstat_line;

        let parsed = parse_git_numstat_line("30\t2\tsrc/lib.rs");

        assert_eq!(
            parsed.map(|(path, stat)| (path, stat.added, stat.removed)),
            Some(("src/lib.rs".to_string(), 30, 2))
        );
    }

    #[test]
    fn parses_braced_numstat_rename_destination() {
        use git::parse_git_numstat_line;

        let parsed = parse_git_numstat_line("4\t1\tsrc/{old => new}/file.rs");

        assert_eq!(
            parsed.map(|(path, stat)| (path, stat.added, stat.removed)),
            Some(("src/new/file.rs".to_string(), 4, 1))
        );
    }

    #[test]
    fn ignores_binary_numstat_line() {
        use git::parse_git_numstat_line;

        assert_eq!(parse_git_numstat_line("-\t-\tassets/logo.png"), None);
    }

    #[test]
    fn git_view_builders_update_props() {
        let tree = FileTree::new(".")
            .git_changed_only(true)
            .git_diff_stats(true);

        assert_eq!(tree.props.change_view, FileTreeChangeView::ChangedOnly);
        assert!(tree.props.git_diff_stats);

        let tree = tree.git_changed_only(false);
        assert_eq!(tree.props.change_view, FileTreeChangeView::AllFiles);
    }

    #[test]
    fn highlight_changed_labels_builder_updates_props() {
        let tree = FileTree::new(".").highlight_changed_labels(true);

        assert!(tree.props.highlight_changed_labels);

        let tree = tree.highlight_changed_labels(false);
        assert!(!tree.props.highlight_changed_labels);
    }

    #[test]
    fn label_style_builders_update_props() {
        let directory_style = Style::new().fg(Color::Blue).bold();
        let file_style = Style::new().fg(Color::Green);

        let tree = FileTree::new(".")
            .directory_label_style(directory_style)
            .file_label_style(file_style);

        assert_eq!(tree.props.directory_label_style, directory_style);
        assert_eq!(tree.props.file_label_style, file_style);
    }

    #[test]
    fn item_style_builders_store_optional_styles() {
        let row = Style::new().bg(Color::Blue);
        let icon = Style::new().fg(Color::Cyan);
        let label = Style::new().fg(Color::Green).bold();
        let suffix = Style::new().dim();

        let style = FileTreeItemStyle::new()
            .row(row)
            .icon(icon)
            .label(label)
            .suffix(suffix);

        assert_eq!(style.row, Some(row));
        assert_eq!(style.icon, Some(icon));
        assert_eq!(style.label, Some(label));
        assert_eq!(style.suffix, Some(suffix));
    }

    #[test]
    fn path_and_suffix_style_builders_update_props() {
        let item_style = FileTreeItemStyle::new().label(Style::new().fg(Color::Green));
        let suffix_style = Style::new().dim();
        let tree = FileTree::new("/repo")
            .path_style("src/main.rs", item_style)
            .path_styles([("src/lib.rs", item_style.suffix(suffix_style))])
            .change_suffix_style(suffix_style)
            .change_suffix_priority(FileTreeSuffixPriority::Suffix);

        assert_eq!(tree.props.path_styles.get("src/main.rs"), Some(&item_style));
        assert_eq!(tree.props.change_suffix_style, suffix_style);
        assert_eq!(
            tree.props.change_suffix_priority,
            FileTreeSuffixPriority::Suffix
        );

        let tree = tree
            .git_suffix_style(Style::new().italic())
            .git_suffix_priority(FileTreeSuffixPriority::Label);
        assert_eq!(tree.props.change_suffix_style, Style::new().italic());
        assert_eq!(
            tree.props.change_suffix_priority,
            FileTreeSuffixPriority::Label
        );
    }

    #[test]
    fn crate_root_export_compiles() {
        let _: crate::FileTreeItemStyle = FileTreeItemStyle::new();
        let _: crate::FileTreeSuffixPriority = FileTreeSuffixPriority::Suffix;
    }

    #[test]
    fn change_view_builders_update_source_agnostic_props() {
        let changes = vec![FileTreeChange::new(
            "src/main.rs",
            FileTreeChangeStatus::Modified,
        )];
        let tree = FileTree::new("/repo")
            .change_source(FileTreeChangeSource::Provided(changes.clone()))
            .change_view(FileTreeChangeView::ChangedOnly)
            .show_diff_stats(true);

        assert_eq!(
            tree.props.change_source,
            FileTreeChangeSource::Provided(changes)
        );
        assert_eq!(tree.props.change_view, FileTreeChangeView::ChangedOnly);
        assert!(tree.props.git_diff_stats);
    }

    #[test]
    fn controlled_tree_state_builders_update_props() {
        let tree = FileTree::new("/repo")
            .selected(3)
            .selected_path("src/main.rs")
            .reveal_path("src")
            .select_path("tests/tree.rs")
            .force_scroll_to_selected(true)
            .expanded_paths(["/repo/src", "/repo/tests"]);

        assert_eq!(tree.props.selected, Some(3));
        assert_eq!(tree.props.selected_path.as_deref(), Some("src/main.rs"));
        assert_eq!(tree.props.reveal_path.as_deref(), Some("src"));
        assert_eq!(tree.props.select_path.as_deref(), Some("tests/tree.rs"));
        assert!(tree.props.force_scroll_to_selected);
        assert_eq!(
            tree.props.expanded_paths,
            Some(HashSet::from([
                Arc::<str>::from("/repo/src"),
                Arc::<str>::from("/repo/tests")
            ]))
        );
    }

    #[test]
    fn provided_snapshot_resolves_absolute_and_rejects_outside_paths() {
        let changes = vec![
            FileTreeChange::new("/repo/src/main.rs", FileTreeChangeStatus::Modified),
            FileTreeChange::new("/elsewhere/ignored.rs", FileTreeChangeStatus::Deleted),
            FileTreeChange::new("../escape.rs", FileTreeChangeStatus::Added),
        ];

        let snapshot = git::provided_change_snapshot("/repo", &changes);

        assert_eq!(snapshot.changed_paths, vec![Arc::from("/repo/src/main.rs")]);
    }

    #[test]
    fn provided_snapshot_accepts_absolute_paths_under_relative_root() {
        let root = std::path::Path::new("relative-provided-root");
        let absolute = std::env::current_dir()
            .unwrap()
            .join(root)
            .join("src/main.rs");
        let changes = vec![FileTreeChange::new(
            absolute.to_string_lossy().into_owned(),
            FileTreeChangeStatus::Modified,
        )];

        let snapshot = git::provided_change_snapshot(root.to_str().unwrap(), &changes);

        assert_eq!(
            snapshot.changed_paths,
            vec![Arc::from(absolute.to_string_lossy().as_ref())]
        );
    }

    #[test]
    fn crate_root_exports_file_tree_change_api() {
        let _tree: crate::FileTree = FileTree::new("/repo");
        let change =
            crate::FileTreeChange::new("src/main.rs", crate::FileTreeChangeStatus::Modified);
        let _source = crate::FileTreeChangeSource::provided([change]);
        let _view: crate::FileTreeChangeView = crate::FileTreeGitView::ChangedOnly;
        let _kind: crate::FileKind = FileKind::File;
    }

    #[test]
    fn merges_status_and_diff_stat_decorations() {
        use git::{
            GitChangeState, GitDiffStat, GitFileDecorations, GitFileStatus, insert_decoration,
        };

        let mut entries = HashMap::new();
        let key: Arc<str> = "/repo/src/app.rs".into();

        insert_decoration(
            &mut entries,
            key.clone(),
            GitFileDecorations {
                status: GitFileStatus::new(None, Some(GitChangeState::Modified)),
                diff_stat: None,
                direct: true,
            },
        );
        insert_decoration(
            &mut entries,
            key.clone(),
            GitFileDecorations {
                status: GitFileStatus::new(None, None),
                diff_stat: Some(GitDiffStat {
                    added: 10,
                    removed: 4,
                }),
                direct: false,
            },
        );

        let decoration = entries.get(key.as_ref()).copied().unwrap();
        assert_eq!(decoration.status.unstaged, Some(GitChangeState::Modified));
        assert_eq!(
            decoration.diff_stat,
            Some(GitDiffStat {
                added: 10,
                removed: 4
            })
        );
        assert!(decoration.direct);
    }
}
