use super::events::{FileTreeEvent, FileTreeToggleEvent};
use super::fs::FileIconStyle;
use super::git::GitIconStyle;
use super::{
    FileIconOverride, FileTreeChangeSource, FileTreeChangeView, FileTreeItemStyle,
    FileTreeSuffixPriority,
};
use crate::callback::Callback;
use crate::style::{BorderStyle, FileIconPalette, Length, Padding, Style, StyleSlot};
use crate::widgets::ScrollKeymap;
use crate::widgets::TreeKeymap;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

// Default git status styles - used both for initialization and comparison in apply_theme.
// Actual colors come from the theme; these are intentionally Style::default() so the
// theme system detects them as "not user-customized" and applies themed colors.
pub(super) fn default_git_style_modified() -> Style {
    Style::default()
}

pub(super) fn default_git_style_added() -> Style {
    Style::default()
}

pub(super) fn default_git_style_deleted() -> Style {
    Style::default()
}

pub(super) fn default_git_style_renamed() -> Style {
    Style::default()
}

pub(super) fn default_git_style_untracked() -> Style {
    Style::default()
}

pub(super) fn default_git_style_conflicted() -> Style {
    Style::default()
}

#[derive(Clone, PartialEq)]
pub(crate) struct FileTreeProps {
    pub(crate) root: Arc<str>,
    pub(crate) show_hidden: bool,
    pub(crate) max_entries_per_dir: usize,
    pub(crate) show_icons: bool,
    pub(crate) icon_style: FileIconStyle,
    pub(crate) icon_palette: FileIconPalette,
    pub(crate) icon_overrides: HashMap<Arc<str>, FileIconOverride>,
    pub(crate) show_arrows: bool,
    pub(crate) indent_style: crate::widgets::IndentStyle,
    pub(crate) indent_guide_style: Style,
    pub(crate) directory_icon: Arc<str>,
    pub(crate) opened_directory_icon: Arc<str>,
    pub(crate) file_icon: Arc<str>,
    pub(crate) symlink_icon: Arc<str>,
    pub(crate) other_icon: Arc<str>,
    pub(crate) directory_label_style: Style,
    pub(crate) file_label_style: Style,
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
    pub(crate) selected_path: Option<Arc<str>>,
    pub(crate) reveal_path: Option<Arc<str>>,
    pub(crate) select_path: Option<Arc<str>>,
    pub(crate) force_scroll_to_selected: bool,
    pub(crate) expanded_paths: Option<HashSet<Arc<str>>>,
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
    pub(crate) explorer: bool,
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
    pub(crate) git_diff_stats: bool,
    pub(crate) git_icon_style: GitIconStyle,
    pub(crate) git_refresh_nonce: u64,
    pub(crate) git_marker_modified: Arc<str>,
    pub(crate) git_marker_added: Arc<str>,
    pub(crate) git_marker_deleted: Arc<str>,
    pub(crate) git_marker_renamed: Arc<str>,
    pub(crate) git_marker_untracked: Arc<str>,
    pub(crate) git_marker_conflicted: Arc<str>,
    pub(crate) git_style_modified: Style,
    pub(crate) git_style_added: Style,
    pub(crate) git_style_deleted: Style,
    pub(crate) git_style_renamed: Style,
    pub(crate) git_style_untracked: Style,
    pub(crate) git_style_conflicted: Style,
    pub(crate) change_suffix_style: Style,
    pub(crate) change_suffix_priority: FileTreeSuffixPriority,
    pub(crate) path_styles: HashMap<Arc<str>, FileTreeItemStyle>,
    pub(crate) on_select: Option<Callback<FileTreeEvent>>,
    pub(crate) on_activate: Option<Callback<FileTreeEvent>>,
    pub(crate) on_toggle: Option<Callback<FileTreeToggleEvent>>,
}
