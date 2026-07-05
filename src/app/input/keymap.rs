use crate::clipboard::{ClipboardConfig, PasteShiftInsertBehavior};
use crate::core::event::{KeyCode, KeyEvent};
use crate::input::{ChordMatcher, ChordResult, KeyBinding, KeyBindingParseError, is_none_binding};
use std::collections::{HashMap as StdHashMap, HashSet};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::LazyLock;

#[cfg(test)]
use crate::core::event::KeyMods;

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

    // Selection
    SelectLeft,
    SelectRight,
    SelectUp,
    SelectDown,
    SelectHome,
    SelectEnd,
    SelectWordLeft,
    SelectWordRight,

    // Editing
    Backspace,
    Delete,
    DeleteWordLeft,
    DeleteWordRight,
    InsertNewline,

    /// Default action (usually insert char).
    InsertChar(char),
    /// No action mapped.
    None,
}

impl Action {
    fn from_config_name(raw: &str) -> Option<Self> {
        let mut name = raw.trim().to_ascii_lowercase();
        if name.is_empty() {
            return None;
        }
        name = name.replace('_', "-");
        match name.as_str() {
            "quit" | "exit" | "app-quit" | "app-exit" => Some(Self::Quit),
            "dismiss" | "dismiss-overlay" | "close" | "cancel" | "escape" => {
                Some(Self::DismissOverlay)
            }
            "focus-next" | "next-focus" | "next-widget" | "focus-forward" => Some(Self::FocusNext),
            "focus-prev" | "focus-previous" | "prev-focus" | "previous-focus" | "prev-widget"
            | "focus-backward" => Some(Self::FocusPrev),
            "toggle-devtools" | "devtools-toggle" | "devtools" | "toggle-dev-tools"
            | "toggle-debug" | "debug-toggle" => Some(Self::ToggleDevTools),
            "copy" => Some(Self::Copy),
            "cut" => Some(Self::Cut),
            "paste" => Some(Self::Paste),
            "paste-selection"
            | "paste-from-selection"
            | "paste-primary"
            | "paste-primary-selection" => Some(Self::PasteFromSelection),
            "copy-image" => Some(Self::CopyImage),
            "paste-image" => Some(Self::PasteImage),
            "undo" => Some(Self::Undo),
            "redo" => Some(Self::Redo),
            "select-all" => Some(Self::SelectAll),
            "clear" | "clear-text" | "clear-input" => Some(Self::Clear),
            "move-left" => Some(Self::MoveLeft),
            "move-right" => Some(Self::MoveRight),
            "move-up" => Some(Self::MoveUp),
            "move-down" => Some(Self::MoveDown),
            "move-home" => Some(Self::MoveHome),
            "move-end" => Some(Self::MoveEnd),
            "move-word-left" => Some(Self::MoveWordLeft),
            "move-word-right" => Some(Self::MoveWordRight),
            "select-left" => Some(Self::SelectLeft),
            "select-right" => Some(Self::SelectRight),
            "select-up" => Some(Self::SelectUp),
            "select-down" => Some(Self::SelectDown),
            "select-home" => Some(Self::SelectHome),
            "select-end" => Some(Self::SelectEnd),
            "select-word-left" => Some(Self::SelectWordLeft),
            "select-word-right" => Some(Self::SelectWordRight),
            "backspace" => Some(Self::Backspace),
            "delete" => Some(Self::Delete),
            "delete-word-left" => Some(Self::DeleteWordLeft),
            "delete-word-right" => Some(Self::DeleteWordRight),
            "insert-newline" | "newline" | "enter" => Some(Self::InsertNewline),
            "none" | "unbind" | "disabled" => Some(Self::None),
            _ => None,
        }
    }

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
}

impl KeymapConfig {
    pub fn from_clipboard_config(config: &ClipboardConfig) -> Self {
        Self {
            enable_performable_ctrl_c_copy: config.enable_performable_ctrl_c_copy,
            paste_shift_insert_behavior: config.paste_shift_insert_behavior,
            keymap_path: None,
        }
    }

    pub fn keymap_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.keymap_path = Some(path.into());
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

fn resolve_keymap_path(config: &KeymapConfig) -> Option<(PathBuf, bool)> {
    if let Some(path) = config.keymap_path.clone() {
        return Some((path, true));
    }

    if let Ok(raw) = std::env::var(KEYMAP_ENV) {
        let raw = raw.trim();
        if !raw.is_empty() {
            return Some((PathBuf::from(raw), true));
        }
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

#[derive(Debug, Default)]
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
        let index = build_binding_index(&bindings);
        Self { bindings, index }
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
    Keymap { bindings, index }
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
