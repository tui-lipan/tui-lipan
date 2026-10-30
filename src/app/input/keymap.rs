use crate::clipboard::{ClipboardConfig, PasteShiftInsertBehavior};
use crate::core::event::{KeyCode, KeyEvent};
use crate::input::{
    ChordMatcher, ChordResult, KeyBinding, KeyBindingParseError, KeyBindings, is_none_binding,
};
use std::collections::{HashMap as StdHashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock;

#[cfg(test)]
use crate::core::event::KeyMods;

#[cfg(not(test))]
const KEYMAP_ENV: &str = "TUI_LIPAN_KEYMAP";
const DEFAULT_KEYMAP_NAME: &str = "keymap.conf";

/// Semantic input actions for text manipulation, navigation, and app control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Action {
    /// Copy selection to clipboard.
    Copy,
    /// Cut selection to clipboard.
    Cut,
    /// Paste from clipboard.
    Paste,
    /// Paste from primary selection (if supported).
    PasteFromSelection,
    /// Copy image to clipboard.
    CopyImage,
    /// Paste image from clipboard.
    PasteImage,
    /// Undo last edit.
    Undo,
    /// Redo last undone edit.
    Redo,
    /// Select all text.
    SelectAll,
    /// Clear all text.
    Clear,

    /// Quit the application.
    Quit,

    /// Dismiss the top-most overlay (modal/popover).
    DismissOverlay,
    /// Move focus to the next focusable widget.
    FocusNext,
    /// Move focus to the previous focusable widget.
    FocusPrev,
    /// Toggle DevTools visibility.
    ToggleDevTools,

    // Navigation
    MoveLeft,
    MoveRight,
    MoveUp,
    MoveDown,
    MoveHome,
    MoveEnd,
    MoveWordLeft,
    MoveWordRight,
    MoveVisualLineHome,
    MoveVisualLineEnd,
    MoveBufferHome,
    MoveBufferEnd,

    // Selection
    SelectLeft,
    SelectRight,
    SelectUp,
    SelectDown,
    SelectHome,
    SelectEnd,
    SelectWordLeft,
    SelectWordRight,
    SelectVisualLineHome,
    SelectVisualLineEnd,
    SelectBufferHome,
    SelectBufferEnd,

    // Editing
    Backspace,
    Delete,
    DeleteWordLeft,
    DeleteWordRight,
    /// Delete from the cursor to the start of the line, or the selection if any.
    DeleteToLineStart,
    /// Delete from the cursor to the end of the line, or the selection if any.
    DeleteToLineEnd,
    /// Delete the current line, or the selection if any.
    DeleteLine,
    InsertNewline,

    /// Default action (usually insert char).
    InsertChar(char),
    /// No action mapped.
    None,
}

/// Generates [`FrameworkAction`] and the config-file names from one table, so a bindable action
/// cannot be missing from either. `Action::framework_action` matches exhaustively, so an [`Action`]
/// left out of the table fails to compile. Names are in the `-` form config names normalize to.
macro_rules! bindable_actions {
    ($($(#[$meta:meta])* $variant:ident = $name:literal $(| $alias:literal)*;)+) => {
        /// Actions that can be bound from Rust through [`FrameworkKeymap`] or `App` builders.
        ///
        /// Covers every action `keymap.conf` can bind. Binding an action replaces all of its
        /// default bindings, so list any keys it should keep.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum FrameworkAction {
            $($(#[$meta])* $variant,)+
        }

        impl FrameworkAction {
            #[cfg(test)]
            const ALL: &'static [(Self, &'static str)] = &[$((Self::$variant, $name),)+];

            fn action(self) -> Action {
                match self {
                    $(Self::$variant => Action::$variant,)+
                }
            }
        }

        impl Action {
            fn from_config_name(raw: &str) -> Option<Self> {
                let mut name = raw.trim().to_ascii_lowercase();
                if name.is_empty() {
                    return None;
                }
                name = name.replace('_', "-");
                match name.as_str() {
                    $($name $(| $alias)* => Some(Self::$variant),)+
                    "none" | "unbind" | "disabled" => Some(Self::None),
                    _ => None,
                }
            }

            /// The bindable action this runs as, if it is one.
            #[cfg_attr(not(test), allow(dead_code))]
            fn framework_action(self) -> Option<FrameworkAction> {
                match self {
                    $(Self::$variant => Some(FrameworkAction::$variant),)+
                    Self::InsertChar(_) | Self::None => None,
                }
            }
        }
    };
}

bindable_actions! {
    /// Quit the application.
    Quit = "quit" | "exit" | "app-quit" | "app-exit";
    /// Dismiss the top-most overlay.
    DismissOverlay = "dismiss-overlay" | "dismiss" | "close" | "cancel" | "escape";
    /// Move focus to the next focusable widget.
    FocusNext = "focus-next" | "next-focus" | "next-widget" | "focus-forward";
    /// Move focus to the previous focusable widget.
    FocusPrev = "focus-prev" | "focus-previous" | "prev-focus" | "previous-focus" | "prev-widget"
        | "focus-backward";
    /// Toggle DevTools visibility.
    ToggleDevTools = "toggle-devtools" | "devtools-toggle" | "devtools" | "toggle-dev-tools"
        | "toggle-debug" | "debug-toggle";
    /// Copy the selection to the clipboard.
    Copy = "copy";
    /// Cut the selection to the clipboard.
    Cut = "cut";
    /// Paste from the clipboard.
    Paste = "paste";
    /// Paste from the primary selection, where supported.
    PasteFromSelection = "paste-selection" | "paste-from-selection" | "paste-primary"
        | "paste-primary-selection";
    /// Copy an image to the clipboard.
    CopyImage = "copy-image";
    /// Paste an image from the clipboard.
    PasteImage = "paste-image";
    /// Undo the last edit.
    Undo = "undo";
    /// Redo the last undone edit.
    Redo = "redo";
    /// Select all text.
    SelectAll = "select-all";
    /// Clear all text.
    Clear = "clear" | "clear-text" | "clear-input";
    /// Move the cursor left.
    MoveLeft = "move-left";
    /// Move the cursor right.
    MoveRight = "move-right";
    /// Move the cursor up.
    MoveUp = "move-up";
    /// Move the cursor down.
    MoveDown = "move-down";
    /// Move to the start of the line.
    MoveHome = "move-home";
    /// Move to the end of the line.
    MoveEnd = "move-end";
    /// Move to the previous word.
    MoveWordLeft = "move-word-left";
    /// Move to the next word.
    MoveWordRight = "move-word-right";
    /// Move to the start of the wrapped row; the line start where text does not wrap.
    MoveVisualLineHome = "move-visual-line-home";
    /// Move to the end of the wrapped row; the line end where text does not wrap.
    MoveVisualLineEnd = "move-visual-line-end";
    /// Move to the start of the text.
    MoveBufferHome = "move-buffer-home";
    /// Move to the end of the text.
    MoveBufferEnd = "move-buffer-end";
    /// Extend the selection left.
    SelectLeft = "select-left";
    /// Extend the selection right.
    SelectRight = "select-right";
    /// Extend the selection up.
    SelectUp = "select-up";
    /// Extend the selection down.
    SelectDown = "select-down";
    /// Extend the selection to the start of the line.
    SelectHome = "select-home";
    /// Extend the selection to the end of the line.
    SelectEnd = "select-end";
    /// Extend the selection to the previous word.
    SelectWordLeft = "select-word-left";
    /// Extend the selection to the next word.
    SelectWordRight = "select-word-right";
    /// Extend the selection to the start of the wrapped row.
    SelectVisualLineHome = "select-visual-line-home";
    /// Extend the selection to the end of the wrapped row.
    SelectVisualLineEnd = "select-visual-line-end";
    /// Extend the selection to the start of the text.
    SelectBufferHome = "select-buffer-home";
    /// Extend the selection to the end of the text.
    SelectBufferEnd = "select-buffer-end";
    /// Delete the previous character or the selection.
    Backspace = "backspace";
    /// Delete the next character or the selection.
    Delete = "delete";
    /// Delete the previous word or the selection.
    DeleteWordLeft = "delete-word-left";
    /// Delete the next word or the selection.
    DeleteWordRight = "delete-word-right";
    /// Delete from the cursor to the start of the line, or the selection if any.
    DeleteToLineStart = "delete-to-line-start";
    /// Delete from the cursor to the end of the line, or the selection if any.
    DeleteToLineEnd = "delete-to-line-end";
    /// Delete the current line, or the selection if any.
    DeleteLine = "delete-line";
    /// Insert a newline.
    InsertNewline = "insert-newline" | "newline" | "enter";
}

/// Enables or disables loading user keymap files from env/default paths.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UserKeymapPolicy {
    /// Load user keymap files normally.
    #[default]
    Enabled,
    /// Ignore user keymap files while preserving built-in defaults.
    Disabled,
}

#[derive(Debug, Clone)]
enum FrameworkBindingOverride {
    Bind(KeyBindings),
    Unbind,
}

/// Rust-side framework keymap overrides applied after file and built-in bindings.
///
/// When two actions are bound to the same key, the later `bind` call wins.
#[derive(Debug, Clone, Default)]
pub struct FrameworkKeymap {
    overrides: Vec<(FrameworkAction, FrameworkBindingOverride)>,
}

impl FrameworkKeymap {
    /// Bind a framework action to one or more key bindings.
    pub fn bind(self, action: FrameworkAction, bindings: KeyBindings) -> Self {
        self.with_override(action, FrameworkBindingOverride::Bind(bindings))
    }

    /// Remove all key bindings for a framework action.
    pub fn unbind(self, action: FrameworkAction) -> Self {
        self.with_override(action, FrameworkBindingOverride::Unbind)
    }

    fn with_override(mut self, action: FrameworkAction, value: FrameworkBindingOverride) -> Self {
        self.overrides.retain(|(existing, _)| *existing != action);
        self.overrides.push((action, value));
        self
    }
}

impl Action {
    pub(crate) fn is_clipboard(self) -> bool {
        matches!(
            self,
            Self::Copy
                | Self::Cut
                | Self::Paste
                | Self::PasteFromSelection
                | Self::CopyImage
                | Self::PasteImage
        )
    }

    /// Whether a focused `Input` or `TextArea` carries this out directly, without a clipboard or
    /// other framework service, so a chord can run it.
    pub(crate) fn runs_in_text_widget(self) -> bool {
        !self.is_clipboard()
            && !matches!(
                self,
                Self::Quit
                    | Self::DismissOverlay
                    | Self::FocusNext
                    | Self::FocusPrev
                    | Self::ToggleDevTools
                    | Self::InsertChar(_)
                    | Self::None
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BindingMode {
    Performable,
    Always,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BindingMatch {
    pub action: Action,
    pub mode: BindingMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct KeymapRuntimeMatch {
    pub action: Action,
    pub mode: BindingMode,
    pub is_chord: bool,
}

impl KeymapRuntimeMatch {
    pub(crate) fn binding_match(self) -> BindingMatch {
        BindingMatch {
            action: self.action,
            mode: self.mode,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum KeymapRuntimeResult {
    None,
    Pending,
    Matched(KeymapRuntimeMatch),
}

#[derive(Debug, Clone)]
pub(crate) struct Binding {
    combination: KeyBinding,
    action: Action,
    mode: BindingMode,
}

#[derive(Debug, Clone)]
pub struct Keymap {
    config: KeymapConfig,
    user_keymap: ParsedKeymapConfig,
    bindings: Vec<Binding>,
    /// Index from combination to the index of the first matching binding.
    index: StdHashMap<KeyBinding, Vec<usize>>,
}

pub(crate) struct KeymapMatcher {
    matcher: ChordMatcher<KeymapRuntimeMatch>,
}

pub(crate) type KeymapRuntime = KeymapMatcher;

impl KeymapMatcher {
    pub(crate) fn new(keymap: &Keymap) -> Self {
        let entries = keymap
            .bindings
            .iter()
            .map(|binding| {
                (
                    binding.combination.clone(),
                    KeymapRuntimeMatch {
                        action: binding.action,
                        mode: binding.mode,
                        is_chord: binding.combination.is_chord(),
                    },
                )
            })
            .collect();
        Self {
            matcher: ChordMatcher::new(entries),
        }
    }

    pub(crate) fn feed(&mut self, key: KeyEvent) -> KeymapRuntimeResult {
        match self.matcher.feed(&key) {
            ChordResult::None => KeymapRuntimeResult::None,
            ChordResult::Pending => KeymapRuntimeResult::Pending,
            ChordResult::Matched(binding) => KeymapRuntimeResult::Matched(*binding),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.matcher.reset();
    }

    #[cfg(test)]
    pub(crate) fn is_pending(&self) -> bool {
        self.matcher.is_pending()
    }
}

#[derive(Debug, Clone)]
pub(crate) struct KeymapConfig {
    pub enable_performable_ctrl_c_copy: bool,
    pub paste_shift_insert_behavior: PasteShiftInsertBehavior,
    pub keymap_path: Option<PathBuf>,
    pub user_keymap_policy: UserKeymapPolicy,
    pub framework_keymap: FrameworkKeymap,
}

impl KeymapConfig {
    pub fn from_clipboard_config(config: &ClipboardConfig) -> Self {
        Self {
            enable_performable_ctrl_c_copy: config.enable_performable_ctrl_c_copy,
            paste_shift_insert_behavior: config.paste_shift_insert_behavior,
            keymap_path: None,
            user_keymap_policy: UserKeymapPolicy::Enabled,
            framework_keymap: FrameworkKeymap::default(),
        }
    }

    pub fn keymap_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.keymap_path = Some(path.into());
        self
    }

    pub fn user_keymap_policy(mut self, policy: UserKeymapPolicy) -> Self {
        self.user_keymap_policy = policy;
        self
    }

    pub fn framework_keymap(mut self, keymap: FrameworkKeymap) -> Self {
        self.framework_keymap = keymap;
        self
    }
}

fn parse_binding(raw: &str) -> Result<KeyBinding, KeyBindingParseError> {
    KeyBinding::from_str(raw)
}

fn binding_mode_for(action: Action, _combination: &KeyBinding) -> BindingMode {
    if action.is_clipboard() {
        BindingMode::Performable
    } else {
        BindingMode::Always
    }
}

fn default_keymap_path() -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("XDG_CONFIG_HOME") {
        return Some(
            PathBuf::from(dir)
                .join("tui-lipan")
                .join(DEFAULT_KEYMAP_NAME),
        );
    }
    if let Ok(dir) = std::env::var("APPDATA") {
        return Some(
            PathBuf::from(dir)
                .join("tui-lipan")
                .join(DEFAULT_KEYMAP_NAME),
        );
    }
    if let Ok(home) = std::env::var("HOME") {
        return Some(
            PathBuf::from(home)
                .join(".config")
                .join("tui-lipan")
                .join(DEFAULT_KEYMAP_NAME),
        );
    }
    None
}

#[cfg(test)]
static TEST_KEYMAP_ENV: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

#[cfg(test)]
fn set_test_keymap_env(path: impl Into<PathBuf>) {
    *TEST_KEYMAP_ENV.lock().expect("test env lock") = Some(path.into());
}

#[cfg(test)]
fn remove_test_keymap_env() {
    *TEST_KEYMAP_ENV.lock().expect("test env lock") = None;
}

#[cfg(test)]
fn env_keymap_path() -> Option<PathBuf> {
    TEST_KEYMAP_ENV.lock().expect("test env lock").clone()
}

#[cfg(not(test))]
fn env_keymap_path() -> Option<PathBuf> {
    if let Ok(raw) = std::env::var(KEYMAP_ENV) {
        let raw = raw.trim();
        if !raw.is_empty() {
            return Some(PathBuf::from(raw));
        }
    }
    None
}

fn resolve_keymap_path(config: &KeymapConfig) -> Option<(PathBuf, bool)> {
    if let Some(path) = config.keymap_path.clone() {
        return Some((path, true));
    }

    if let Some(path) = env_keymap_path() {
        return Some((path, true));
    }
    let path = default_keymap_path()?;
    if path.is_file() {
        Some((path, false))
    } else {
        None
    }
}

#[cfg(test)]
fn parse_keymap_config(path: &Path, contents: &str) -> Vec<Binding> {
    parse_keymap_file(path, contents).bindings
}

#[derive(Clone, Debug, Default)]
struct ParsedKeymapConfig {
    bindings: Vec<Binding>,
    overridden_actions: HashSet<Action>,
}

fn parse_keymap_file(path: &Path, contents: &str) -> ParsedKeymapConfig {
    let mut bindings = Vec::new();
    let mut overridden_actions = HashSet::new();

    for (line_idx, raw_line) in contents.lines().enumerate() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        let Some((action_raw, keys_raw)) = line.split_once(['=', ':']) else {
            crate::debug::internal_log!(
                "[tui-lipan] Invalid keymap entry at {}:{} (expected 'action = keys')",
                path.display(),
                line_idx + 1
            );
            continue;
        };

        let action_name = action_raw.trim();
        let Some(action) = Action::from_config_name(action_name) else {
            crate::debug::internal_log!(
                "[tui-lipan] Unknown keymap action '{}' at {}:{}",
                action_name,
                path.display(),
                line_idx + 1
            );
            continue;
        };

        overridden_actions.insert(action);

        for key in keys_raw.split(',') {
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            if is_none_binding(key) {
                continue;
            }
            match parse_binding(key) {
                Ok(comb) => {
                    let mode = binding_mode_for(action, &comb);
                    bindings.push(Binding {
                        combination: comb,
                        action,
                        mode,
                    });
                }
                Err(err) => {
                    crate::debug::internal_log!(
                        "[tui-lipan] Invalid key binding '{}' at {}:{}: {}",
                        key,
                        path.display(),
                        line_idx + 1,
                        err
                    );
                }
            }
        }
    }

    ParsedKeymapConfig {
        bindings,
        overridden_actions,
    }
}

fn load_user_bindings(config: &KeymapConfig) -> ParsedKeymapConfig {
    if config.user_keymap_policy == UserKeymapPolicy::Disabled {
        return ParsedKeymapConfig::default();
    }

    let Some((path, explicit)) = resolve_keymap_path(config) else {
        return ParsedKeymapConfig::default();
    };

    let contents = match std::fs::read_to_string(&path) {
        Ok(contents) => contents,
        Err(err) => {
            if explicit {
                crate::debug::internal_log!(
                    "[tui-lipan] Failed to read keymap file {}: {}",
                    path.display(),
                    err
                );
            }
            return ParsedKeymapConfig::default();
        }
    };

    parse_keymap_file(&path, &contents)
}

fn apply_framework_keymap_overrides(bindings: &mut Vec<Binding>, keymap: &FrameworkKeymap) {
    for (framework_action, override_) in &keymap.overrides {
        let action = framework_action.action();
        bindings.retain(|binding| binding.action != action);
        if let FrameworkBindingOverride::Bind(key_bindings) = override_ {
            // Keys resolve to their first binding, so an app binding goes in front to outrank a
            // default or file binding on the same key.
            bindings.splice(
                0..0,
                key_bindings.iter().cloned().map(|combination| Binding {
                    mode: binding_mode_for(action, &combination),
                    combination,
                    action,
                }),
            );
        }
    }
}

fn default_bindings(config: &KeymapConfig) -> Vec<Binding> {
    let mut bindings = Vec::new();

    // Helper to add bindings. Warning: ORDER MATTERS. Specific bindings first.
    let mut bind = |key: &str, action: Action| match parse_binding(key) {
        Ok(comb) => {
            let mode = binding_mode_for(action, &comb);
            bindings.push(Binding {
                combination: comb,
                action,
                mode,
            });
        }
        Err(e) => crate::debug::internal_log!("[tui-lipan] Invalid key binding '{}': {}", key, e),
    };

    // --- Clipboard ---
    if config.enable_performable_ctrl_c_copy {
        bind("ctrl-c", Action::Copy);
    }
    bind("ctrl-shift-c", Action::Copy);
    bind("ctrl-insert", Action::Copy); // Legacy CUA
    bind("super-c", Action::Copy);
    bind("super-insert", Action::Copy); // Common Linux/VM binding

    bind("ctrl-x", Action::Cut);
    bind("super-x", Action::Cut);
    bind("shift-delete", Action::Cut); // Legacy CUA

    bind("ctrl-v", Action::Paste);
    match config.paste_shift_insert_behavior {
        PasteShiftInsertBehavior::Clipboard => bind("shift-insert", Action::Paste),
        PasteShiftInsertBehavior::PrimarySelection => {
            bind("shift-insert", Action::PasteFromSelection)
        }
    }
    bind("ctrl-shift-v", Action::Paste);
    bind("super-v", Action::Paste);
    bind("super-shift-v", Action::Paste); // Explicit SUPER+SHIFT+V binding often sent by terminals for "Paste"

    // --- Image clipboard ---
    // Note: no default key for PasteImage - smart paste via Ctrl+V handles it automatically
    // when the focused widget has `on_image_paste` set. Users can bind `paste-image` manually.
    bind("ctrl-shift-y", Action::CopyImage);

    // --- Undo/Redo ---
    bind("ctrl-z", Action::Undo);
    bind("ctrl-shift-z", Action::Redo); // Common variant
    bind("ctrl-y", Action::Redo); // Windows variant

    // --- Selection ---
    bind("ctrl-a", Action::SelectAll);

    // Word selection
    bind("ctrl-shift-left", Action::SelectWordLeft);
    bind("alt-shift-left", Action::SelectWordLeft); // Mac-style
    bind("ctrl-shift-right", Action::SelectWordRight);
    bind("alt-shift-right", Action::SelectWordRight);

    // Char selection
    bind("shift-left", Action::SelectLeft);
    bind("shift-right", Action::SelectRight);
    bind("shift-up", Action::SelectUp);
    bind("shift-down", Action::SelectDown);
    bind("shift-home", Action::SelectHome);
    bind("shift-end", Action::SelectEnd);

    // --- Navigation ---
    // Word movement
    bind("ctrl-left", Action::MoveWordLeft);
    bind("alt-left", Action::MoveWordLeft);
    bind("ctrl-right", Action::MoveWordRight);
    bind("alt-right", Action::MoveWordRight);

    // Basic movement
    bind("left", Action::MoveLeft);
    bind("right", Action::MoveRight);
    bind("up", Action::MoveUp);
    bind("down", Action::MoveDown);
    bind("home", Action::MoveHome);
    bind("end", Action::MoveEnd);

    // --- Deletion ---
    bind("ctrl-backspace", Action::DeleteWordLeft);
    bind("alt-backspace", Action::DeleteWordLeft);
    bind("ctrl-delete", Action::DeleteWordRight);
    bind("alt-delete", Action::DeleteWordRight);
    bind("backspace", Action::Backspace);
    bind("delete", Action::Delete);

    bind("enter", Action::InsertNewline);

    // --- App ---
    bind("ctrl-q", Action::Quit);

    // --- Overlays ---
    bind("esc", Action::DismissOverlay);

    // --- Focus traversal ---
    bind("tab", Action::FocusNext);
    bind("shift-tab", Action::FocusPrev);

    // --- DevTools ---
    bind("f12", Action::ToggleDevTools);

    bindings
}

fn build_binding_index(bindings: &[Binding]) -> StdHashMap<KeyBinding, Vec<usize>> {
    let mut index: StdHashMap<KeyBinding, Vec<usize>> = StdHashMap::new();
    for (i, binding) in bindings.iter().enumerate() {
        index
            .entry(binding.combination.clone())
            .or_default()
            .push(i);
    }
    index
}

impl Keymap {
    pub(crate) fn new(config: KeymapConfig) -> Self {
        let user_keymap = load_user_bindings(&config);
        Self::from_config_and_user_keymap(config, user_keymap)
    }

    fn from_config_and_user_keymap(config: KeymapConfig, user_keymap: ParsedKeymapConfig) -> Self {
        let mut bindings = Vec::new();
        bindings.extend(user_keymap.bindings.iter().cloned());

        let defaults = default_bindings(&config);
        for binding in defaults {
            if user_keymap.overridden_actions.contains(&binding.action)
                || user_keymap
                    .bindings
                    .iter()
                    .any(|user| user.combination == binding.combination)
            {
                continue;
            }
            bindings.push(binding);
        }
        apply_framework_keymap_overrides(&mut bindings, &config.framework_keymap);
        let index = build_binding_index(&bindings);
        Self {
            config,
            user_keymap,
            bindings,
            index,
        }
    }

    pub(crate) fn reconfigure_clipboard(&mut self, config: &ClipboardConfig) {
        let mut keymap_config = self.config.clone();
        keymap_config.enable_performable_ctrl_c_copy = config.enable_performable_ctrl_c_copy;
        keymap_config.paste_shift_insert_behavior = config.paste_shift_insert_behavior;
        *self = Self::from_config_and_user_keymap(keymap_config, self.user_keymap.clone());
    }

    pub(crate) fn resolve_action(&self, key: KeyEvent) -> Action {
        let normalized_key = crate::input::normalize_ctrl_char(key);
        let event_binding = KeyBinding::from_key_event(key);

        if let Some(indices) = self.index.get(&event_binding)
            && let Some(&idx) = indices.first()
        {
            return self.bindings[idx].action;
        }

        match normalized_key.code {
            KeyCode::Char(c) => {
                if normalized_key.mods.ctrl || normalized_key.mods.super_key {
                    Action::None
                } else {
                    Action::InsertChar(c)
                }
            }
            _ => Action::None,
        }
    }

    pub(crate) fn matches(&self, key: KeyEvent) -> Vec<BindingMatch> {
        let event_comb = KeyBinding::from_key_event(key);

        match self.index.get(&event_comb) {
            Some(indices) => {
                // Most keypresses match 1-2 bindings; pre-allocate to avoid
                // incremental growth from collect()'s default empty Vec.
                let mut result = Vec::with_capacity(indices.len().min(4));
                for &idx in indices {
                    let binding = &self.bindings[idx];
                    result.push(BindingMatch {
                        action: binding.action,
                        mode: binding.mode,
                    });
                }
                result
            }
            None => Vec::new(),
        }
    }

    pub fn binding_for_action(&self, action: Action) -> Option<&KeyBinding> {
        self.bindings
            .iter()
            .find(|binding| binding.action == action)
            .map(|binding| &binding.combination)
    }
}
impl Default for Keymap {
    fn default() -> Self {
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default());
        Self::new(config)
    }
}

static DEFAULT_KEYMAP: LazyLock<Keymap> = LazyLock::new(Keymap::default);

pub(crate) fn default_keymap() -> &'static Keymap {
    &DEFAULT_KEYMAP
}

#[cfg(test)]
pub(crate) fn binding_for_test(key: &str, action: Action, mode: BindingMode) -> Binding {
    let combination = parse_binding(key).expect("test binding parses");
    Binding {
        combination,
        action,
        mode,
    }
}

#[cfg(test)]
pub(crate) fn keymap_for_test(bindings: Vec<Binding>) -> Keymap {
    let index = build_binding_index(&bindings);
    Keymap {
        config: KeymapConfig::from_clipboard_config(&ClipboardConfig::default()),
        user_keymap: ParsedKeymapConfig::default(),
        bindings,
        index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn plain_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            mods: KeyMods::default(),
        }
    }

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        }
    }

    #[test]
    fn super_alias_parses() {
        let super_comb = parse_binding("super-c").expect("super alias parses");
        let cmd_comb = parse_binding("cmd-c").expect("cmd parses");
        assert_eq!(super_comb, cmd_comb);
    }

    #[test]
    fn normalizes_raw_ctrl_char() {
        let key = KeyEvent {
            code: KeyCode::Char('\x03'),
            mods: KeyMods::default(),
        };
        let normalized = crate::input::normalize_ctrl_char(key);
        assert!(normalized.mods.ctrl);
        assert_eq!(normalized.code, KeyCode::Char('c'));
    }

    #[test]
    fn resolve_complex_combinations() {
        // Test binding a complex key and resolving it
        // We'll manually parse a config to test the resolution logic without relying on default bindings
        // or the global keymap.
        // However, we can test `parse_keymap_config` and `to_crokey_event`.

        let _config = "
            custom-action = super-alt-x
            move-up = ctrl-shift-up
        ";
        // We need a dummy path
        let path = PathBuf::from("test_config.conf");
        // Note: `custom-action` is NOT a valid Action variant, so it should be skipped/logged error.
        // Let's use valid actions.
        let config_valid = "
            copy = super-alt-c
            move-up = ctrl-shift-up
        ";

        let bindings = parse_keymap_config(&path, config_valid);

        // verify mappings
        let comb1 = bindings
            .iter()
            .find(|binding| binding.action == Action::Copy)
            .expect("Copy binding found")
            .combination
            .clone();
        let comb2 = bindings
            .iter()
            .find(|binding| binding.action == Action::MoveUp)
            .expect("MoveUp binding found")
            .combination
            .clone();

        // Verify matches using normalized binding comparison
        let expected_copy = parse_binding("cmd-alt-c").unwrap();
        let expected_move = parse_binding("ctrl-shift-up").unwrap();

        assert_eq!(comb1, expected_copy);
        assert_eq!(comb2, expected_move);
    }

    #[test]
    fn explicit_keymap_path_takes_priority() {
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
            .keymap_path("/tmp/project-keymap.conf");
        let resolved = resolve_keymap_path(&config).expect("explicit path should resolve");

        assert_eq!(resolved.0, PathBuf::from("/tmp/project-keymap.conf"));
        assert!(resolved.1);
    }

    fn write_temp_keymap(contents: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("tui-lipan-keymap-{unique}.conf"));
        fs::write(&path, contents).expect("write test keymap");
        path
    }

    #[test]
    fn clipboard_reconfiguration_preserves_the_loaded_user_keymap() {
        let path = write_temp_keymap("copy = alt-c\n");
        let config =
            KeymapConfig::from_clipboard_config(&ClipboardConfig::default()).keymap_path(&path);
        let mut keymap = Keymap::new(config);
        fs::remove_file(&path).expect("remove test keymap");

        keymap.reconfigure_clipboard(&ClipboardConfig {
            enable_performable_ctrl_c_copy: false,
            ..ClipboardConfig::default()
        });

        assert_eq!(
            keymap.resolve_action(KeyEvent {
                code: KeyCode::Char('c'),
                mods: KeyMods {
                    alt: true,
                    ..KeyMods::default()
                },
            }),
            Action::Copy
        );
        assert_eq!(keymap.resolve_action(ctrl_key('c')), Action::None);
    }

    #[test]
    fn rust_framework_unbind_beats_explicit_keymap_file() {
        let path = write_temp_keymap("quit = ctrl-q\n");
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
            .keymap_path(&path)
            .framework_keymap(FrameworkKeymap::default().unbind(FrameworkAction::Quit));
        let keymap = Keymap::new(config);
        let _ = fs::remove_file(&path);
        assert!(keymap.matches(ctrl_key('q')).is_empty());
    }

    #[test]
    fn app_keymap_path_beats_env_keymap() {
        let env_path = write_temp_keymap("quit = ctrl-x q\n");
        let app_path = write_temp_keymap("quit = ctrl-y q\n");
        set_test_keymap_env(&env_path);
        let config =
            KeymapConfig::from_clipboard_config(&ClipboardConfig::default()).keymap_path(&app_path);
        let keymap = Keymap::new(config);
        remove_test_keymap_env();
        let _ = fs::remove_file(&env_path);
        let _ = fs::remove_file(&app_path);
        assert!(
            keymap
                .binding_for_action(Action::Quit)
                .is_some_and(|b| b.canonical_lowercase() == "ctrl+y q")
        );
    }

    #[test]
    fn rust_framework_rebind_beats_file_none() {
        let path = write_temp_keymap("quit = none\n");
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
            .keymap_path(&path)
            .framework_keymap(FrameworkKeymap::default().bind(
                FrameworkAction::Quit,
                KeyBindings::from_str("ctrl-y q").unwrap(),
            ));
        let keymap = Keymap::new(config);
        let _ = fs::remove_file(&path);
        assert!(
            keymap
                .binding_for_action(Action::Quit)
                .is_some_and(|b| b.canonical_lowercase() == "ctrl+y q")
        );
    }

    #[test]
    fn user_keymap_policy_disabled_ignores_env_and_default_files() {
        let path = write_temp_keymap("quit = ctrl-x q\n");
        set_test_keymap_env(&path);
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
            .user_keymap_policy(UserKeymapPolicy::Disabled);
        let keymap = Keymap::new(config);
        remove_test_keymap_env();
        let _ = fs::remove_file(&path);
        assert!(keymap.binding_for_action(Action::Quit).is_some());
        assert!(
            keymap
                .matches(ctrl_key('q'))
                .iter()
                .any(|b| b.action == Action::Quit)
        );
        assert!(
            keymap
                .matches(ctrl_key('x'))
                .iter()
                .all(|binding| binding.action != Action::Quit)
        );
    }

    #[test]
    fn user_keymap_policy_disabled_does_not_disable_builtins() {
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
            .user_keymap_policy(UserKeymapPolicy::Disabled);
        let keymap = Keymap::new(config);
        assert!(
            keymap
                .matches(ctrl_key('q'))
                .iter()
                .any(|b| b.action == Action::Quit)
        );
        assert!(
            keymap
                .matches(ctrl_key('c'))
                .iter()
                .any(|b| b.action == Action::Copy)
        );
    }

    #[test]
    fn clipboard_bindings_are_performable() {
        let copy = parse_binding("super-c").expect("copy binding parses");
        let cut = parse_binding("shift-delete").expect("cut binding parses");
        let paste = parse_binding("super-v").expect("paste binding parses");
        let quit = parse_binding("ctrl-q").expect("quit binding parses");

        assert_eq!(
            binding_mode_for(Action::Copy, &copy),
            BindingMode::Performable
        );
        assert_eq!(
            binding_mode_for(Action::Cut, &cut),
            BindingMode::Performable
        );
        assert_eq!(
            binding_mode_for(Action::Paste, &paste),
            BindingMode::Performable
        );
        assert_eq!(binding_mode_for(Action::Quit, &quit), BindingMode::Always);
    }

    #[test]
    fn resolve_ctrl_v() {
        // Test that Ctrl+V resolves to Paste
        let keymap = default_keymap();

        // Case 1: Raw control char (legacy terminal)
        let key_raw = KeyEvent {
            code: KeyCode::Char('\x16'), // Ctrl+V is ASCII 22
            mods: KeyMods::default(),
        };
        assert_eq!(
            keymap.resolve_action(key_raw),
            Action::Paste,
            "Raw Ctrl+V should resolve to Paste"
        );

        // Case 2: Discrete event with lowercase 'v' (enhanced terminal)
        let key_lower = KeyEvent {
            code: KeyCode::Char('v'),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        };
        assert_eq!(
            keymap.resolve_action(key_lower),
            Action::Paste,
            "Ctrl+v (lowercase) should resolve to Paste"
        );

        // Case 3: Discrete event with uppercase 'V' (some terminals report this)
        let key_upper = KeyEvent {
            code: KeyCode::Char('V'),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        };
        assert_eq!(
            keymap.resolve_action(key_upper),
            Action::Paste,
            "Ctrl+V (uppercase) should resolve to Paste"
        );
    }

    #[test]
    fn default_keymap_binds_tab_focus_actions() {
        let keymap = default_keymap();

        assert_eq!(
            keymap.resolve_action(KeyEvent {
                code: KeyCode::Tab,
                mods: KeyMods::default(),
            }),
            Action::FocusNext
        );
        assert_eq!(
            keymap.resolve_action(KeyEvent {
                code: KeyCode::BackTab,
                mods: KeyMods::default(),
            }),
            Action::FocusPrev
        );
    }

    #[test]
    fn default_keymap_binds_f12_to_toggle_devtools() {
        let keymap = default_keymap();

        assert_eq!(
            keymap.resolve_action(KeyEvent {
                code: KeyCode::F(12),
                mods: KeyMods::default(),
            }),
            Action::ToggleDevTools
        );
    }

    #[test]
    fn parses_toggle_devtools_aliases() {
        assert_eq!(
            Action::from_config_name("toggle-devtools"),
            Some(Action::ToggleDevTools)
        );
        assert_eq!(
            Action::from_config_name("devtools-toggle"),
            Some(Action::ToggleDevTools)
        );
        assert_eq!(
            Action::from_config_name("toggle_debug"),
            Some(Action::ToggleDevTools)
        );
    }

    #[test]
    fn parses_clear_aliases() {
        assert_eq!(Action::from_config_name("clear"), Some(Action::Clear));
        assert_eq!(Action::from_config_name("clear-text"), Some(Action::Clear));
        assert_eq!(Action::from_config_name("clear_input"), Some(Action::Clear));
    }

    #[test]
    fn every_bindable_action_round_trips_through_its_config_name_and_is_documented() {
        let docs = include_str!("../../../docs/keybindings.md");
        for &(framework_action, name) in FrameworkAction::ALL {
            let action = framework_action.action();
            assert_eq!(Action::from_config_name(name), Some(action), "{name}");
            assert_eq!(
                Action::from_config_name(&name.replace('-', "_")),
                Some(action),
                "{name}"
            );
            assert_eq!(action.framework_action(), Some(framework_action), "{name}");
            let documented = format!("`{}`", name.replace('-', "_"));
            assert!(docs.contains(&documented), "{documented} in keybindings.md");
        }
    }

    #[test]
    fn newly_bindable_actions_have_no_default_binding() {
        let keymap = default_keymap();
        for action in [
            Action::DeleteToLineStart,
            Action::DeleteToLineEnd,
            Action::DeleteLine,
            Action::MoveVisualLineHome,
            Action::MoveVisualLineEnd,
            Action::SelectVisualLineHome,
            Action::SelectVisualLineEnd,
            Action::MoveBufferHome,
            Action::MoveBufferEnd,
            Action::SelectBufferHome,
            Action::SelectBufferEnd,
        ] {
            assert_eq!(keymap.binding_for_action(action), None, "{action:?}");
        }
    }

    #[test]
    fn rust_bind_replaces_an_actions_default_keys() {
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
            .user_keymap_policy(UserKeymapPolicy::Disabled)
            .framework_keymap(FrameworkKeymap::default().bind(
                FrameworkAction::DeleteWordLeft,
                KeyBindings::from_str("ctrl-w").unwrap(),
            ));
        let keymap = Keymap::new(config);

        assert_eq!(keymap.resolve_action(ctrl_key('w')), Action::DeleteWordLeft);
        assert_eq!(
            keymap.resolve_action(KeyEvent {
                code: KeyCode::Backspace,
                mods: KeyMods {
                    alt: true,
                    ..KeyMods::default()
                },
            }),
            Action::None
        );
    }

    #[test]
    fn later_rust_bind_wins_when_two_actions_share_a_key() {
        for (first, second, expected) in [
            (
                FrameworkAction::DeleteToLineStart,
                FrameworkAction::Clear,
                Action::Clear,
            ),
            (
                FrameworkAction::Clear,
                FrameworkAction::DeleteToLineStart,
                Action::DeleteToLineStart,
            ),
        ] {
            let bindings = KeyBindings::from_str("ctrl-u").unwrap();
            let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
                .user_keymap_policy(UserKeymapPolicy::Disabled)
                .framework_keymap(
                    FrameworkKeymap::default()
                        .bind(first, bindings.clone())
                        .bind(second, bindings),
                );

            assert_eq!(Keymap::new(config).resolve_action(ctrl_key('u')), expected);
        }
    }

    #[test]
    fn rust_bound_clipboard_action_stays_performable() {
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
            .user_keymap_policy(UserKeymapPolicy::Disabled)
            .framework_keymap(FrameworkKeymap::default().bind(
                FrameworkAction::Copy,
                KeyBindings::from_str("alt-c").unwrap(),
            ));
        let keymap = Keymap::new(config);
        let alt_c = KeyEvent {
            code: KeyCode::Char('c'),
            mods: KeyMods {
                alt: true,
                ..KeyMods::default()
            },
        };

        assert_eq!(
            keymap.matches(alt_c),
            vec![BindingMatch {
                action: Action::Copy,
                mode: BindingMode::Performable,
            }]
        );
    }

    #[test]
    fn delete_to_line_start_has_a_config_name_and_no_default_binding() {
        assert_eq!(
            Action::from_config_name("delete_to_line_start"),
            Some(Action::DeleteToLineStart)
        );
        assert_eq!(
            Action::from_config_name("delete-to-line-start"),
            Some(Action::DeleteToLineStart)
        );
        assert_eq!(default_keymap().resolve_action(ctrl_key('u')), Action::None);
    }

    #[test]
    fn keymap_file_can_bind_delete_to_line_start() {
        let path = write_temp_keymap("delete_to_line_start = ctrl-u\n");
        let keymap = Keymap::new(
            KeymapConfig::from_clipboard_config(&ClipboardConfig::default()).keymap_path(&path),
        );
        let _ = fs::remove_file(&path);

        assert_eq!(
            keymap.resolve_action(ctrl_key('u')),
            Action::DeleteToLineStart
        );
    }

    #[test]
    fn rust_text_action_bind_beats_default_binding_on_the_same_key() {
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
            .user_keymap_policy(UserKeymapPolicy::Disabled)
            .framework_keymap(FrameworkKeymap::default().bind(
                FrameworkAction::DeleteToLineStart,
                KeyBindings::from_str("ctrl-a").unwrap(),
            ));
        let keymap = Keymap::new(config);

        assert_eq!(
            keymap.resolve_action(ctrl_key('a')),
            Action::DeleteToLineStart
        );
        assert_eq!(
            KeymapRuntime::new(&keymap).feed(ctrl_key('a')),
            KeymapRuntimeResult::Matched(KeymapRuntimeMatch {
                action: Action::DeleteToLineStart,
                mode: BindingMode::Always,
                is_chord: false,
            })
        );
    }

    #[test]
    fn rust_text_action_bind_beats_keymap_file_on_the_same_key() {
        let path = write_temp_keymap("clear = ctrl-u\n");
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
            .keymap_path(&path)
            .framework_keymap(FrameworkKeymap::default().bind(
                FrameworkAction::DeleteToLineStart,
                KeyBindings::from_str("ctrl-u").unwrap(),
            ));
        let keymap = Keymap::new(config);
        let _ = fs::remove_file(&path);

        assert_eq!(
            keymap.resolve_action(ctrl_key('u')),
            Action::DeleteToLineStart
        );
    }

    #[test]
    fn rust_framework_unbind_removes_a_file_delete_to_line_start_binding() {
        let path = write_temp_keymap("delete_to_line_start = ctrl-u\n");
        let config = KeymapConfig::from_clipboard_config(&ClipboardConfig::default())
            .keymap_path(&path)
            .framework_keymap(
                FrameworkKeymap::default().unbind(FrameworkAction::DeleteToLineStart),
            );
        let keymap = Keymap::new(config);
        let _ = fs::remove_file(&path);

        assert_eq!(keymap.resolve_action(ctrl_key('u')), Action::None);
    }

    #[test]
    fn default_keymap_does_not_bind_ctrl_c_to_quit() {
        let keymap = default_keymap();
        let matches = keymap.matches(KeyEvent {
            code: KeyCode::Char('c'),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        });

        assert!(matches.iter().any(|binding| binding.action == Action::Copy));
        assert!(!matches.iter().any(|binding| binding.action == Action::Quit));
    }

    #[test]
    fn user_keymap_can_remap_and_disable_focus_actions() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("tui-lipan-keymap-{unique}.conf"));
        fs::write(&path, "focus-next = ctrl-j\nfocus-prev = none\n").expect("write test keymap");

        let keymap = Keymap::new(
            KeymapConfig::from_clipboard_config(&ClipboardConfig::default()).keymap_path(&path),
        );

        let _ = fs::remove_file(&path);

        assert_eq!(
            keymap.resolve_action(KeyEvent {
                code: KeyCode::Tab,
                mods: KeyMods::default(),
            }),
            Action::None,
        );
        assert_eq!(
            keymap.resolve_action(KeyEvent {
                code: KeyCode::BackTab,
                mods: KeyMods::default(),
            }),
            Action::None,
        );
        assert_eq!(
            keymap.resolve_action(KeyEvent {
                code: KeyCode::Char('j'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            }),
            Action::FocusNext,
        );
    }

    #[test]
    fn user_clear_binding_overrides_default_copy_combo() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should move forward")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("tui-lipan-keymap-clear-{unique}.conf"));
        fs::write(&path, "clear = ctrl-c\n").expect("write test keymap");

        let keymap = Keymap::new(
            KeymapConfig::from_clipboard_config(&ClipboardConfig::default()).keymap_path(&path),
        );

        let _ = fs::remove_file(&path);

        let ctrl_c = KeyEvent {
            code: KeyCode::Char('c'),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        };
        assert_eq!(keymap.resolve_action(ctrl_c), Action::Clear);
        let matches = keymap.matches(ctrl_c);
        assert!(
            matches
                .iter()
                .any(|binding| binding.action == Action::Clear)
        );
        assert!(!matches.iter().any(|binding| binding.action == Action::Copy));
    }

    #[test]
    fn keymap_runtime_matches_chorded_action() {
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-x q",
            Action::Quit,
            BindingMode::Always,
        )]);
        let mut runtime = KeymapRuntime::new(&keymap);

        assert_eq!(runtime.feed(ctrl_key('x')), KeymapRuntimeResult::Pending);
        assert!(runtime.is_pending());
        assert_eq!(
            runtime.feed(plain_key('q')),
            KeymapRuntimeResult::Matched(KeymapRuntimeMatch {
                action: Action::Quit,
                mode: BindingMode::Always,
                is_chord: true,
            })
        );
        assert!(!runtime.is_pending());
    }

    #[test]
    fn keymap_runtime_mismatch_resets_and_allows_fresh_match() {
        let keymap = keymap_for_test(vec![
            binding_for_test("ctrl-x q", Action::Quit, BindingMode::Always),
            binding_for_test("ctrl-g", Action::DismissOverlay, BindingMode::Always),
        ]);
        let mut runtime = KeymapRuntime::new(&keymap);

        assert_eq!(runtime.feed(ctrl_key('x')), KeymapRuntimeResult::Pending);
        assert_eq!(
            runtime.feed(ctrl_key('g')),
            KeymapRuntimeResult::Matched(KeymapRuntimeMatch {
                action: Action::DismissOverlay,
                mode: BindingMode::Always,
                is_chord: false,
            })
        );
        assert!(!runtime.is_pending());
    }

    #[test]
    fn keymap_runtime_mismatch_resets_to_none() {
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-x q",
            Action::Quit,
            BindingMode::Always,
        )]);
        let mut runtime = KeymapRuntime::new(&keymap);

        assert_eq!(runtime.feed(ctrl_key('x')), KeymapRuntimeResult::Pending);
        assert_eq!(runtime.feed(plain_key('z')), KeymapRuntimeResult::None);
        assert!(!runtime.is_pending());
    }

    #[test]
    fn keymap_runtime_preserves_single_key_matches() {
        let keymap = keymap_for_test(vec![binding_for_test(
            "ctrl-q",
            Action::Quit,
            BindingMode::Always,
        )]);
        let mut runtime = KeymapRuntime::new(&keymap);

        assert_eq!(keymap.resolve_action(ctrl_key('q')), Action::Quit);
        assert_eq!(
            runtime.feed(ctrl_key('q')),
            KeymapRuntimeResult::Matched(KeymapRuntimeMatch {
                action: Action::Quit,
                mode: BindingMode::Always,
                is_chord: false,
            })
        );
    }
}
