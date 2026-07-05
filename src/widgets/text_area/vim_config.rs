use std::sync::Arc;

use crate::core::event::{KeyCode, KeyEvent, KeyMods};
use crate::input::KeyBindings;
use crate::style::{Style, StyleSlot};

/// TextArea-only Vim-style modal motion mode.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAreaVimMode {
    /// Plain text insertion mode.
    Insert,
    /// Vim-style normal/motion mode. This is the initial mode.
    #[default]
    Normal,
    /// Vim-style visual selection mode.
    ///
    /// Motions extend the selection from the original visual anchor. Supported
    /// Vim edit commands can yank or replace the active selection.
    Visual,
    /// Vim-style linewise visual selection mode.
    ///
    /// Motions extend the selection from the original logical line and keep
    /// the selected range aligned to whole logical lines.
    VisualLine,
}

/// Widget-local Vim key remaps for [`TextArea::vim_motions`](crate::widgets::TextArea::vim_motions).
///
/// The keymap translates matching input keys into canonical Vim command
/// characters before the TextArea Vim dispatcher sees them. This keeps the
/// default Vim grammar intact while allowing applications to add aliases such
/// as mapping `ctrl+n` to `j` or `ctrl+p` to `k` for a particular TextArea.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextAreaVimKeymap {
    bindings: Vec<TextAreaVimKeyBinding>,
}

/// One TextArea Vim key remap entry.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextAreaVimKeyBinding {
    /// Triggering key bindings. Only single-key bindings are used by the Vim
    /// dispatcher; chord bindings are ignored here and remain available to the
    /// app-level keymap/interceptor layers.
    pub bindings: KeyBindings,
    /// Canonical Vim command character to dispatch when a binding matches.
    pub command: char,
}

impl TextAreaVimKeymap {
    /// Create an empty TextArea Vim keymap.
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a remap from one or more key bindings to a canonical Vim command
    /// character.
    pub fn bind(mut self, bindings: KeyBindings, command: char) -> Self {
        self.bindings
            .push(TextAreaVimKeyBinding { bindings, command });
        self
    }

    pub(crate) fn translate_key(&self, key: KeyEvent) -> KeyEvent {
        let Some(binding) = self.bindings.iter().find(|binding| {
            binding
                .bindings
                .iter()
                .any(|candidate| !candidate.is_chord() && candidate.matches_sequence(&[key]))
        }) else {
            return key;
        };

        KeyEvent {
            code: KeyCode::Char(binding.command),
            mods: KeyMods::default(),
        }
    }
}

/// Current-line highlighting mode for [`TextArea::vim_config`](crate::widgets::TextArea::vim_config).
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextAreaVimCurrentLineHighlight {
    /// Do not render a Vim current-line highlight.
    #[default]
    Off,
    /// Highlight only the text content area for every wrapped row of the cursor's
    /// logical line.
    Content,
    /// Highlight the full inner row, including line numbers or custom gutter.
    Full,
}

/// Rendering configuration for TextArea Vim affordances.
///
/// This keeps Vim-only visual options grouped separately from the plain TextArea
/// builder API. It controls the built-in search bar/match feedback and optional
/// current-line highlighting; Vim command semantics are still enabled with
/// [`TextArea::vim_motions`](crate::widgets::TextArea::vim_motions).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextAreaVimConfig {
    /// Style for the Vim search/status bar. `Inherit` resolves to the widget
    /// chrome style, `Extend` patches the chrome style, and `Replace` uses the
    /// supplied style directly.
    pub search_bar_style: StyleSlot,
    /// Style overlay for the Vim search bar prefix icons.
    /// Unset fields fall through to [`Self::search_bar_style`].
    pub search_bar_prefix_style: StyleSlot,
    /// Style overlay for Vim search count labels like `[2/5]`.
    /// Unset fields fall through to [`Self::search_bar_style`].
    pub search_bar_count_style: StyleSlot,
    /// Style patched over visible matches while Vim search feedback is shown.
    /// `Inherit` resolves against the theme text-selection role.
    pub search_match_style: StyleSlot,
    /// Style patched over the current Vim search match. While a search prompt is
    /// pending, this is the match `Enter` would jump to; after `Enter`, `n` and
    /// `N` update it. `Inherit` resolves against the theme text-selection role so
    /// the default uses a background highlight.
    pub current_search_match_style: StyleSlot,
    /// Optional Vim current-line highlighting.
    pub current_line_highlight: TextAreaVimCurrentLineHighlight,
    /// Style for current-line highlighting. `Inherit` resolves against the theme
    /// item-hover role so the default follows the active theme.
    pub current_line_style: StyleSlot,
    /// Style for the current line number/custom gutter row. Unset fields fall
    /// through to the current-line style so full-row highlights stay continuous.
    pub current_line_number_style: StyleSlot,
}

impl Default for TextAreaVimConfig {
    fn default() -> Self {
        Self {
            search_bar_style: StyleSlot::Extend(Style::new().reverse()),
            search_bar_prefix_style: StyleSlot::Inherit,
            search_bar_count_style: StyleSlot::Inherit,
            search_match_style: StyleSlot::Replace(Style::new().underline().bold()),
            current_search_match_style: StyleSlot::Inherit,
            current_line_highlight: TextAreaVimCurrentLineHighlight::Off,
            current_line_style: StyleSlot::Inherit,
            current_line_number_style: StyleSlot::Inherit,
        }
    }
}

impl TextAreaVimConfig {
    /// Create a Vim rendering config with default search feedback and no
    /// current-line highlight.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the Vim search/status bar style.
    pub fn search_bar_style(mut self, style: Style) -> Self {
        self.search_bar_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the TextArea chrome style for the Vim search/status bar.
    pub fn extend_search_bar_style(mut self, style: Style) -> Self {
        self.search_bar_style = StyleSlot::Extend(style);
        self
    }

    /// Set the Vim search/status bar style slot directly for composite forwarding.
    pub fn search_bar_style_slot(mut self, slot: StyleSlot) -> Self {
        self.search_bar_style = slot;
        self
    }

    /// Overlay style for the Vim search bar prefix icons.
    pub fn search_bar_prefix_style(mut self, style: Style) -> Self {
        self.search_bar_prefix_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the Vim search bar style for the prefix icons.
    pub fn extend_search_bar_prefix_style(mut self, style: Style) -> Self {
        self.search_bar_prefix_style = StyleSlot::Extend(style);
        self
    }

    /// Set the Vim search bar prefix style slot directly for composite forwarding.
    pub fn search_bar_prefix_style_slot(mut self, slot: StyleSlot) -> Self {
        self.search_bar_prefix_style = slot;
        self
    }

    /// Overlay style for Vim search count labels like `[2/5]`.
    pub fn search_bar_count_style(mut self, style: Style) -> Self {
        self.search_bar_count_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the Vim search bar style for count labels like `[2/5]`.
    pub fn extend_search_bar_count_style(mut self, style: Style) -> Self {
        self.search_bar_count_style = StyleSlot::Extend(style);
        self
    }

    /// Set the Vim search bar count style slot directly for composite forwarding.
    pub fn search_bar_count_style_slot(mut self, slot: StyleSlot) -> Self {
        self.search_bar_count_style = slot;
        self
    }

    /// Replace the visible Vim search-match style.
    pub fn search_match_style(mut self, style: Style) -> Self {
        self.search_match_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the theme text-selection role for visible Vim search matches.
    pub fn extend_search_match_style(mut self, style: Style) -> Self {
        self.search_match_style = StyleSlot::Extend(style);
        self
    }

    /// Set the search-match style slot directly for composite forwarding.
    pub fn search_match_style_slot(mut self, slot: StyleSlot) -> Self {
        self.search_match_style = slot;
        self
    }

    /// Replace the current Vim search-match style.
    pub fn current_search_match_style(mut self, style: Style) -> Self {
        self.current_search_match_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the theme text-selection role for the current Vim search match.
    pub fn extend_current_search_match_style(mut self, style: Style) -> Self {
        self.current_search_match_style = StyleSlot::Extend(style);
        self
    }

    /// Set the current-search-match style slot directly for composite forwarding.
    pub fn current_search_match_style_slot(mut self, slot: StyleSlot) -> Self {
        self.current_search_match_style = slot;
        self
    }

    /// Configure Vim current-line highlighting.
    pub fn current_line_highlight(mut self, mode: TextAreaVimCurrentLineHighlight) -> Self {
        self.current_line_highlight = mode;
        self
    }

    /// Convenience toggle for full-row Vim current-line highlighting.
    pub fn highlight_current_line(mut self, enabled: bool) -> Self {
        self.current_line_highlight = if enabled {
            TextAreaVimCurrentLineHighlight::Full
        } else {
            TextAreaVimCurrentLineHighlight::Off
        };
        self
    }

    /// Replace the Vim current-line style.
    pub fn current_line_style(mut self, style: Style) -> Self {
        self.current_line_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the theme item-hover role for Vim current-line highlighting.
    pub fn extend_current_line_style(mut self, style: Style) -> Self {
        self.current_line_style = StyleSlot::Extend(style);
        self
    }

    /// Set the Vim current-line style slot directly for composite forwarding.
    pub fn current_line_style_slot(mut self, slot: StyleSlot) -> Self {
        self.current_line_style = slot;
        self
    }

    /// Replace the style overlay used for the current line number/custom gutter row.
    pub fn current_line_number_style(mut self, style: Style) -> Self {
        self.current_line_number_style = StyleSlot::Replace(style);
        self
    }

    /// Extend the current-line style for the current line number/custom gutter row.
    pub fn extend_current_line_number_style(mut self, style: Style) -> Self {
        self.current_line_number_style = StyleSlot::Extend(style);
        self
    }

    /// Set the current-line-number style slot directly for composite forwarding.
    pub fn current_line_number_style_slot(mut self, slot: StyleSlot) -> Self {
        self.current_line_number_style = slot;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TextAreaVimSearchFeedback {
    pub query: Arc<str>,
    pub cursor: usize,
    pub forward: bool,
    pub pending: bool,
    pub target_range: Option<(usize, usize)>,
    pub current_match_index: Option<usize>,
    pub match_count: usize,
}
