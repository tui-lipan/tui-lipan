//! Public keybinding parsing, matching, and formatting utilities.

use crate::core::event::{KeyCode, KeyEvent, KeyMods};
#[cfg(not(target_arch = "wasm32"))]
use crokey::{KeyCombination, crossterm};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
type KeyStep = KeyCombination;
#[cfg(target_arch = "wasm32")]
type KeyStep = Arc<str>;

/// One parsed keyboard shortcut, possibly a multi-key chord (e.g. "ctrl+x b").
#[derive(Clone, Debug)]
pub struct KeyBinding {
    steps: Vec<KeyStep>,
    canonical: Arc<str>,
}

impl PartialEq for KeyBinding {
    fn eq(&self, other: &Self) -> bool {
        self.steps == other.steps
    }
}

impl Eq for KeyBinding {}

impl Hash for KeyBinding {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.steps.hash(state);
    }
}

/// A set of alternative keybindings.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct KeyBindings {
    bindings: Vec<KeyBinding>,
}

/// Parse error for keybinding strings.
#[derive(Clone, Debug, thiserror::Error)]
pub enum KeyBindingParseError {
    /// Invalid keybinding expression.
    #[error("invalid key binding: {0}")]
    Invalid(String),
}

impl KeyBinding {
    /// Returns true when this binding matches the given sequence of key events.
    pub fn matches_sequence(&self, events: &[KeyEvent]) -> bool {
        self.steps.len() == events.len()
            && self.steps.iter().zip(events).all(|(step, event)| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    *step == key_combination_from_event(*event)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    step.as_ref() == key_combination_from_event(event).as_ref()
                }
            })
    }

    /// Returns true if this is a chord (multi-step) binding.
    pub fn is_chord(&self) -> bool {
        self.steps.len() > 1
    }

    /// Returns the number of key steps in this binding.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Expand this binding into one [`KeyEvent`] per chord step.
    ///
    /// Combinations that press multiple key codes at once are rejected — send-keys and similar
    /// callers need a single discrete event per step.
    pub fn key_events(&self) -> Result<Vec<KeyEvent>, KeyBindingParseError> {
        self.steps
            .iter()
            .map(|step| {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    key_event_from_combination(*step)
                }
                #[cfg(target_arch = "wasm32")]
                {
                    key_event_from_canonical(step.as_ref())
                }
            })
            .collect()
    }

    /// Returns the canonical display string for this binding.
    pub fn canonical(&self) -> &str {
        &self.canonical
    }

    /// Returns the canonical display string in lowercase.
    pub fn canonical_lowercase(&self) -> String {
        self.canonical.to_ascii_lowercase()
    }

    pub(crate) fn from_key_event(key: KeyEvent) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self::from_combination(key_combination_from_event(key))
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::from_combination(key_combination_from_event(&key))
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn from_combination(combination: KeyCombination) -> Self {
        Self::from_steps(vec![combination])
    }

    #[cfg(target_arch = "wasm32")]
    pub(crate) fn from_combination(combination: KeyStep) -> Self {
        Self::from_steps(vec![combination])
    }

    fn from_steps(steps: Vec<KeyStep>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        let canonical = steps
            .iter()
            .map(|s| canonicalize_combination(&s.to_string()))
            .collect::<Vec<_>>()
            .join(" ");
        #[cfg(target_arch = "wasm32")]
        let canonical = steps
            .iter()
            .map(|s| s.as_ref())
            .collect::<Vec<_>>()
            .join(" ");
        Self {
            steps,
            canonical: Arc::from(canonical),
        }
    }

    fn matches_step(&self, step_index: usize, key: &KeyEvent) -> bool {
        self.steps.get(step_index).is_some_and(|step| {
            #[cfg(not(target_arch = "wasm32"))]
            {
                *step == key_combination_from_event(*key)
            }
            #[cfg(target_arch = "wasm32")]
            {
                step.as_ref() == key_combination_from_event(key).as_ref()
            }
        })
    }
}

impl FromStr for KeyBinding {
    type Err = KeyBindingParseError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = raw.split_whitespace().collect();
        if parts.is_empty() {
            return Err(KeyBindingParseError::Invalid(raw.trim().to_string()));
        }

        let mut steps = Vec::with_capacity(parts.len());
        for part in &parts {
            steps.push(parse_key_combination(part)?);
        }

        Ok(Self::from_steps(steps))
    }
}

impl fmt::Display for KeyBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.canonical)
    }
}

impl KeyBindings {
    /// Create a binding set from individual bindings.
    pub fn from_bindings(bindings: impl IntoIterator<Item = KeyBinding>) -> Self {
        Self {
            bindings: bindings.into_iter().collect(),
        }
    }

    /// Returns true if no bindings are configured.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    /// Returns the number of bindings.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Returns the first binding, if present.
    pub fn primary(&self) -> Option<&KeyBinding> {
        self.bindings.first()
    }

    /// Iterates over all bindings.
    pub fn iter(&self) -> impl Iterator<Item = &KeyBinding> {
        self.bindings.iter()
    }

    /// Returns canonical display text in lowercase.
    pub fn canonical_lowercase(&self) -> String {
        let mut bindings = self.bindings.iter();
        let Some(first) = bindings.next() else {
            return String::new();
        };

        let mut out = first.canonical_lowercase();
        for binding in bindings {
            out.push_str(" / ");
            out.push_str(&binding.canonical_lowercase());
        }
        out
    }
}

impl FromStr for KeyBindings {
    type Err = KeyBindingParseError;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let mut bindings = Vec::new();
        for candidate in raw.split(',') {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                continue;
            }
            bindings.push(KeyBinding::from_str(candidate)?);
        }

        if bindings.is_empty() {
            return Err(KeyBindingParseError::Invalid(raw.trim().to_string()));
        }

        Ok(Self { bindings })
    }
}

impl fmt::Display for KeyBindings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut bindings = self.bindings.iter();
        let Some(first) = bindings.next() else {
            return Ok(());
        };

        write!(f, "{first}")?;
        for binding in bindings {
            write!(f, " / {binding}")?;
        }
        Ok(())
    }
}

/// Parse and canonicalize one binding string.
pub fn format_binding(raw: &str) -> Result<String, KeyBindingParseError> {
    Ok(KeyBinding::from_str(raw)?.to_string())
}

/// Parse and canonicalize one binding string, then lowercase it.
pub fn format_binding_lowercase(raw: &str) -> Result<String, KeyBindingParseError> {
    Ok(KeyBinding::from_str(raw)?.canonical_lowercase())
}

/// Parse and canonicalize comma-separated binding alternatives.
pub fn format_bindings(raw: &str) -> Result<String, KeyBindingParseError> {
    Ok(KeyBindings::from_str(raw)?.to_string())
}

/// Parse and canonicalize comma-separated binding alternatives, then lowercase them.
pub fn format_bindings_lowercase(raw: &str) -> Result<String, KeyBindingParseError> {
    Ok(KeyBindings::from_str(raw)?.canonical_lowercase())
}

pub(crate) fn normalize_binding(raw: &str) -> String {
    let mut normalized = raw.trim().replace('+', "-").to_ascii_lowercase();
    normalized = normalized.replace("super-", "cmd-");
    normalized = normalized.replace("command-", "cmd-");
    normalized = normalized.replace("meta-", "cmd-");
    normalized = normalized.replace("win-", "cmd-");
    normalized = normalized.replace("windows-", "cmd-");
    normalized = normalized.replace("control-", "ctrl-");
    normalized = normalized.replace("option-", "alt-");
    normalized = normalized.replace("page-up", "pageup");
    normalized = normalized.replace("page-down", "pagedown");
    normalized
}

pub(crate) fn is_none_binding(raw: &str) -> bool {
    matches!(
        normalize_binding(raw).as_str(),
        "none" | "unbind" | "disabled"
    )
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn key_combination_from_event(key: KeyEvent) -> KeyStep {
    let normalized = normalize_ctrl_char(key);
    let ct_event = to_crokey_event(normalized);
    KeyCombination::from(ct_event)
}

#[cfg(target_arch = "wasm32")]
pub(crate) fn key_combination_from_event(key: &KeyEvent) -> KeyStep {
    let key = normalize_ctrl_char(*key);
    Arc::from(wasm_event_canonical(&key))
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn to_crokey_event(key: KeyEvent) -> crossterm::event::KeyEvent {
    use crossterm::event::{KeyCode as CTKeyCode, KeyEventKind, KeyEventState, KeyModifiers};

    let mut mods = KeyModifiers::empty();
    if key.mods.ctrl {
        mods |= KeyModifiers::CONTROL;
    }
    if key.mods.alt {
        mods |= KeyModifiers::ALT;
    }
    if key.mods.shift {
        mods |= KeyModifiers::SHIFT;
    }
    if key.mods.super_key {
        mods |= KeyModifiers::SUPER;
    }
    if matches!(key.code, KeyCode::BackTab) {
        mods |= KeyModifiers::SHIFT;
    }

    let code = match key.code {
        KeyCode::Char(c) => CTKeyCode::Char(c.to_ascii_lowercase()),
        KeyCode::Enter => CTKeyCode::Enter,
        KeyCode::Esc => CTKeyCode::Esc,
        KeyCode::Tab => CTKeyCode::Tab,
        KeyCode::BackTab => CTKeyCode::Tab,
        KeyCode::Backspace => CTKeyCode::Backspace,
        KeyCode::Delete => CTKeyCode::Delete,
        KeyCode::Home => CTKeyCode::Home,
        KeyCode::End => CTKeyCode::End,
        KeyCode::PageUp => CTKeyCode::PageUp,
        KeyCode::PageDown => CTKeyCode::PageDown,
        KeyCode::Up => CTKeyCode::Up,
        KeyCode::Down => CTKeyCode::Down,
        KeyCode::Left => CTKeyCode::Left,
        KeyCode::Right => CTKeyCode::Right,
        KeyCode::Insert => CTKeyCode::Insert,
        KeyCode::F(n) => CTKeyCode::F(n),
    };

    crossterm::event::KeyEvent {
        code,
        modifiers: mods,
        kind: KeyEventKind::Press,
        state: KeyEventState::empty(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn key_event_from_combination(
    combination: KeyCombination,
) -> Result<KeyEvent, KeyBindingParseError> {
    use crokey::OneToThree;
    use crossterm::event::KeyCode as CtKeyCode;

    let combination = combination.normalized();
    let ct_code = match combination.codes {
        OneToThree::One(code) => code,
        _ => {
            return Err(KeyBindingParseError::Invalid(
                "multi-key combinations are not supported for key event expansion".into(),
            ));
        }
    };

    let mut mods = KeyMods::NONE;
    if combination
        .modifiers
        .contains(crossterm::event::KeyModifiers::CONTROL)
    {
        mods.ctrl = true;
    }
    if combination
        .modifiers
        .contains(crossterm::event::KeyModifiers::ALT)
    {
        mods.alt = true;
    }
    if combination
        .modifiers
        .contains(crossterm::event::KeyModifiers::SHIFT)
    {
        mods.shift = true;
    }
    if combination
        .modifiers
        .contains(crossterm::event::KeyModifiers::SUPER)
    {
        mods.super_key = true;
    }

    let code = match ct_code {
        CtKeyCode::Char(c) => KeyCode::Char(c),
        CtKeyCode::Enter => KeyCode::Enter,
        CtKeyCode::Esc => KeyCode::Esc,
        CtKeyCode::Tab if mods.shift => {
            mods.shift = false;
            KeyCode::BackTab
        }
        CtKeyCode::Tab => KeyCode::Tab,
        CtKeyCode::Backspace => KeyCode::Backspace,
        CtKeyCode::Delete => KeyCode::Delete,
        CtKeyCode::Home => KeyCode::Home,
        CtKeyCode::End => KeyCode::End,
        CtKeyCode::PageUp => KeyCode::PageUp,
        CtKeyCode::PageDown => KeyCode::PageDown,
        CtKeyCode::Up => KeyCode::Up,
        CtKeyCode::Down => KeyCode::Down,
        CtKeyCode::Left => KeyCode::Left,
        CtKeyCode::Right => KeyCode::Right,
        CtKeyCode::Insert => KeyCode::Insert,
        CtKeyCode::F(n) => KeyCode::F(n),
        other => {
            return Err(KeyBindingParseError::Invalid(format!(
                "unsupported key code for event expansion: {other:?}"
            )));
        }
    };

    Ok(KeyEvent { code, mods })
}

#[cfg(target_arch = "wasm32")]
fn key_event_from_canonical(canonical: &str) -> Result<KeyEvent, KeyBindingParseError> {
    let mut mods = KeyMods::NONE;
    let mut key_token = None;
    for part in canonical.split('+').filter(|part| !part.is_empty()) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" => mods.ctrl = true,
            "alt" => mods.alt = true,
            "cmd" | "super" => mods.super_key = true,
            "shift" => mods.shift = true,
            token if key_token.is_none() => key_token = Some(token.to_string()),
            _ => {
                return Err(KeyBindingParseError::Invalid(canonical.to_string()));
            }
        }
    }
    let Some(token) = key_token else {
        return Err(KeyBindingParseError::Invalid(canonical.to_string()));
    };
    let code = match token.as_str() {
        "esc" | "escape" => KeyCode::Esc,
        "enter" | "return" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "space" => KeyCode::Char(' '),
        token if token.len() >= 2 && token.starts_with('f') && token[1..].parse::<u8>().is_ok() => {
            KeyCode::F(token[1..].parse().unwrap_or(1))
        }
        token if token.chars().count() == 1 => KeyCode::Char(token.chars().next().unwrap()),
        _ => return Err(KeyBindingParseError::Invalid(canonical.to_string())),
    };
    Ok(KeyEvent { code, mods })
}

#[cfg(target_arch = "wasm32")]
fn wasm_event_canonical(key: &KeyEvent) -> String {
    if matches!(key.code, KeyCode::BackTab) {
        return canonicalize_combination(&normalize_binding("shift-tab"));
    }
    let mut raw = String::new();
    if key.mods.ctrl {
        raw.push_str("ctrl-");
    }
    if key.mods.alt {
        raw.push_str("alt-");
    }
    if key.mods.super_key {
        raw.push_str("cmd-");
    }
    if key.mods.shift {
        raw.push_str("shift-");
    }
    match key.code {
        KeyCode::Char(' ') => raw.push_str("space"),
        KeyCode::Char(c) if c.is_ascii_alphabetic() => raw.push(c.to_ascii_lowercase()),
        KeyCode::Char(c) => raw.push(c),
        KeyCode::Enter => raw.push_str("enter"),
        KeyCode::Esc => raw.push_str("esc"),
        KeyCode::Tab => raw.push_str("tab"),
        KeyCode::Backspace => raw.push_str("backspace"),
        KeyCode::Delete => raw.push_str("delete"),
        KeyCode::Insert => raw.push_str("insert"),
        KeyCode::Home => raw.push_str("home"),
        KeyCode::End => raw.push_str("end"),
        KeyCode::PageUp => raw.push_str("pageup"),
        KeyCode::PageDown => raw.push_str("pagedown"),
        KeyCode::Up => raw.push_str("up"),
        KeyCode::Down => raw.push_str("down"),
        KeyCode::Left => raw.push_str("left"),
        KeyCode::Right => raw.push_str("right"),
        KeyCode::F(n) => raw.push_str(&format!("f{n}")),
        KeyCode::BackTab => unreachable!("handled above"),
    }
    canonicalize_combination(&normalize_binding(&raw))
}

pub(crate) fn normalize_ctrl_char(key: KeyEvent) -> KeyEvent {
    if key.mods.ctrl || key.mods.alt || key.mods.shift || key.mods.super_key {
        return key;
    }
    let KeyCode::Char(c) = key.code else {
        return key;
    };
    let Some(letter) = ctrl_char_to_letter(c) else {
        return key;
    };
    KeyEvent {
        code: KeyCode::Char(letter),
        mods: KeyMods {
            ctrl: true,
            ..KeyMods::default()
        },
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn parse_key_combination(raw: &str) -> Result<KeyStep, KeyBindingParseError> {
    let normalized = normalize_binding(raw);
    if normalized.is_empty() {
        return Err(KeyBindingParseError::Invalid(raw.trim().to_string()));
    }
    KeyCombination::from_str(&normalized)
        .map_err(|_err| KeyBindingParseError::Invalid(raw.trim().to_string()))
}

#[cfg(target_arch = "wasm32")]
fn parse_key_combination(raw: &str) -> Result<KeyStep, KeyBindingParseError> {
    let normalized = normalize_binding(raw);
    if normalized.is_empty() {
        return Err(KeyBindingParseError::Invalid(raw.trim().to_string()));
    }
    if !wasm_normalized_has_key(&normalized) {
        return Err(KeyBindingParseError::Invalid(raw.trim().to_string()));
    }
    Ok(Arc::from(canonicalize_combination(&normalized)))
}

#[cfg(target_arch = "wasm32")]
fn wasm_normalized_has_key(normalized: &str) -> bool {
    const MODS: &[&str] = &["ctrl", "alt", "cmd", "shift"];
    let mut saw_non_mod = false;
    for token in normalized
        .split('-')
        .filter(|p| !p.is_empty())
        .map(|p| p.to_ascii_lowercase())
    {
        if MODS.contains(&token.as_str()) {
            continue;
        }
        saw_non_mod = true;
        break;
    }
    saw_non_mod
}

fn ctrl_char_to_letter(c: char) -> Option<char> {
    let code = c as u32;
    if (1..=26).contains(&code) {
        Some(((code as u8).saturating_sub(1) + b'a') as char)
    } else {
        None
    }
}

fn canonicalize_combination(raw: &str) -> String {
    let mut has_ctrl = false;
    let mut has_alt = false;
    let mut has_cmd = false;
    let mut has_shift = false;
    let mut key_tokens: Vec<String> = Vec::new();

    for token in raw
        .split('-')
        .filter(|part| !part.is_empty())
        .map(|part| part.to_ascii_lowercase())
    {
        match token.as_str() {
            "ctrl" => has_ctrl = true,
            "alt" => has_alt = true,
            "cmd" => has_cmd = true,
            "shift" => has_shift = true,
            _ => key_tokens.push(token),
        }
    }

    let mut parts = Vec::with_capacity(5);
    if has_ctrl {
        parts.push("Ctrl".to_string());
    }
    if has_alt {
        parts.push("Alt".to_string());
    }
    if has_cmd {
        parts.push("Cmd".to_string());
    }
    if has_shift {
        parts.push("Shift".to_string());
    }

    let key = display_key_name(&key_tokens.join("-"));
    if !key.is_empty() {
        parts.push(key);
    }

    parts.join("+")
}

fn display_key_name(raw: &str) -> String {
    if raw.len() >= 2
        && raw.starts_with('f')
        && raw[1..].chars().all(|ch| ch.is_ascii_digit())
        && raw[1..].parse::<u8>().is_ok()
    {
        return raw.to_ascii_uppercase();
    }

    match raw {
        "" => String::new(),
        "esc" | "escape" => "Esc".to_string(),
        "enter" | "return" => "Enter".to_string(),
        "tab" => "Tab".to_string(),
        "backtab" | "back-tab" => "BackTab".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Delete".to_string(),
        "insert" => "Insert".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        "pageup" | "page-up" => "PageUp".to_string(),
        "pagedown" | "page-down" => "PageDown".to_string(),
        "up" => "Up".to_string(),
        "down" => "Down".to_string(),
        "left" => "Left".to_string(),
        "right" => "Right".to_string(),
        "space" => "Space".to_string(),
        _ if raw.chars().count() == 1 => {
            let ch = raw.chars().next().unwrap_or_default();
            if ch.is_ascii_alphabetic() {
                ch.to_ascii_uppercase().to_string()
            } else {
                ch.to_string()
            }
        }
        _ => raw
            .split('-')
            .map(title_case_token)
            .collect::<Vec<_>>()
            .join("-"),
    }
}

fn title_case_token(token: &str) -> String {
    let mut chars = token.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out = String::new();
    if first.is_ascii_alphabetic() {
        out.push(first.to_ascii_uppercase());
    } else {
        out.push(first);
    }
    out.push_str(chars.as_str());
    out
}

/// Result of feeding a key event into a [`ChordMatcher`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChordResult<T> {
    /// No binding matched and no chord is pending.
    None,
    /// A binding was fully matched. Contains the associated value.
    Matched(T),
    /// The key is a valid prefix of one or more chord bindings. Waiting for more keys.
    Pending,
}

/// Stateful matcher for key chord sequences.
///
/// Tracks partial matches across multiple key events, allowing multi-key
/// chord bindings like "ctrl+x b" to be matched incrementally.
///
/// # Example
///
/// ```
/// use tui_lipan::prelude::{KeyBinding, KeyCode, KeyEvent, KeyMods};
/// use tui_lipan::{ChordMatcher, ChordResult};
/// use std::str::FromStr;
///
/// let mut matcher = ChordMatcher::new(vec![
///     (KeyBinding::from_str("ctrl+x b").unwrap(), "sidebar"),
///     (KeyBinding::from_str("ctrl+x l").unwrap(), "list"),
///     (KeyBinding::from_str("ctrl+q").unwrap(), "quit"),
/// ]);
///
/// let ctrl_x = KeyEvent {
///     code: KeyCode::Char('x'),
///     mods: KeyMods { ctrl: true, ..KeyMods::default() },
/// };
/// let b = KeyEvent {
///     code: KeyCode::Char('b'),
///     mods: KeyMods::default(),
/// };
///
/// assert_eq!(matcher.feed(&ctrl_x), ChordResult::Pending);
/// assert_eq!(matcher.feed(&b), ChordResult::Matched(&"sidebar"));
/// ```
pub struct ChordMatcher<T> {
    entries: Vec<(KeyBinding, T)>,
    /// Indices of entries with partial matches and how many steps have been matched.
    pending: Vec<(usize, usize)>,
}

impl<T> ChordMatcher<T> {
    /// Creates a new chord matcher from a list of (binding, value) pairs.
    pub fn new(entries: Vec<(KeyBinding, T)>) -> Self {
        Self {
            entries,
            pending: Vec::new(),
        }
    }

    /// Feeds a key event into the matcher, advancing chord state.
    ///
    /// Returns [`ChordResult::Matched`] when a binding is fully matched,
    /// [`ChordResult::Pending`] when waiting for more keys, or
    /// [`ChordResult::None`] when nothing matches.
    pub fn feed(&mut self, key: &KeyEvent) -> ChordResult<&T> {
        if self.pending.is_empty() {
            return self.try_fresh(key);
        }

        // Try to continue pending chords.
        let mut new_pending = Vec::new();
        for &(entry_idx, steps_matched) in &self.pending {
            let (binding, _) = &self.entries[entry_idx];
            if binding.matches_step(steps_matched, key) {
                if steps_matched + 1 == binding.step_count() {
                    self.pending.clear();
                    return ChordResult::Matched(&self.entries[entry_idx].1);
                }
                new_pending.push((entry_idx, steps_matched + 1));
            }
        }

        if !new_pending.is_empty() {
            self.pending = new_pending;
            return ChordResult::Pending;
        }

        // Nothing continued - reset and try this key as a fresh start.
        self.pending.clear();
        self.try_fresh(key)
    }

    /// Returns true if the matcher is waiting for more keys to complete a chord.
    pub fn is_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Resets the matcher, discarding any partial chord state.
    pub fn reset(&mut self) {
        self.pending.clear();
    }

    fn try_fresh(&mut self, key: &KeyEvent) -> ChordResult<&T> {
        let mut first_single_match = None;

        for (idx, (binding, _)) in self.entries.iter().enumerate() {
            if binding.matches_step(0, key) {
                if binding.step_count() == 1 {
                    if first_single_match.is_none() {
                        first_single_match = Some(idx);
                    }
                } else {
                    self.pending.push((idx, 1));
                }
            }
        }

        // If there are pending chords, defer single-step matches.
        if !self.pending.is_empty() {
            return ChordResult::Pending;
        }

        if let Some(idx) = first_single_match {
            return ChordResult::Matched(&self.entries[idx].1);
        }

        ChordResult::None
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    #[test]
    fn normalizes_super_aliases() {
        let cmd = KeyBinding::from_str("cmd-p").expect("cmd binding parses");
        let super_key = KeyBinding::from_str("super-p").expect("super alias parses");
        let command = KeyBinding::from_str("command-p").expect("command alias parses");
        let meta = KeyBinding::from_str("meta-p").expect("meta alias parses");
        let win = KeyBinding::from_str("win-p").expect("win alias parses");

        assert_eq!(super_key, cmd);
        assert_eq!(command, cmd);
        assert_eq!(meta, cmd);
        assert_eq!(win, cmd);
    }

    #[test]
    fn normalizes_control_and_option_aliases() {
        let ctrl = KeyBinding::from_str("ctrl-p").expect("ctrl parses");
        let control = KeyBinding::from_str("control-p").expect("control alias parses");
        let alt = KeyBinding::from_str("alt-p").expect("alt parses");
        let option = KeyBinding::from_str("option-p").expect("option alias parses");

        assert_eq!(control, ctrl);
        assert_eq!(option, alt);
    }

    #[test]
    fn formats_bindings_canonically() {
        assert_eq!(format_binding("ctrl+shift+up").unwrap(), "Ctrl+Shift+Up");
        assert_eq!(format_binding("esc").unwrap(), "Esc");
        assert_eq!(format_binding("page-up").unwrap(), "PageUp");
        assert_eq!(format_binding("f12").unwrap(), "F12");
        assert_eq!(
            format_bindings("ctrl+d, ctrl+q").unwrap(),
            "Ctrl+D / Ctrl+Q"
        );
    }

    #[test]
    fn key_binding_matches_function_key() {
        let binding = KeyBinding::from_str("f12").expect("binding parses");
        let key = KeyEvent {
            code: KeyCode::F(12),
            mods: KeyMods::default(),
        };
        assert!(binding.matches_sequence(&[key]));
    }

    #[test]
    fn key_binding_matches_single_event() {
        let binding = KeyBinding::from_str("ctrl-c").expect("binding parses");
        let key = KeyEvent {
            code: KeyCode::Char('c'),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        };
        assert!(binding.matches_sequence(&[key]));
    }

    #[test]
    fn key_binding_expands_to_key_events() {
        let events = KeyBinding::from_str("ctrl-c")
            .expect("parses")
            .key_events()
            .expect("expands");
        assert_eq!(
            events,
            vec![KeyEvent {
                code: KeyCode::Char('c'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            }]
        );

        let chord = KeyBinding::from_str("ctrl-x b")
            .expect("parses")
            .key_events()
            .expect("expands");
        assert_eq!(chord.len(), 2);
        assert_eq!(chord[0].code, KeyCode::Char('x'));
        assert!(chord[0].mods.ctrl);
        assert_eq!(chord[1].code, KeyCode::Char('b'));
    }

    #[test]
    fn normalizes_raw_ctrl_chars_for_matching() {
        let binding = KeyBinding::from_str("ctrl-v").expect("binding parses");
        let key = KeyEvent {
            code: KeyCode::Char('\x16'),
            mods: KeyMods::default(),
        };
        assert!(binding.matches_sequence(&[key]));
    }

    #[test]
    fn backtab_is_distinct_from_tab() {
        let tab = KeyBinding::from_str("tab").expect("tab parses");
        let backtab = KeyBinding::from_str("shift-tab").expect("shift-tab parses");

        assert_ne!(tab, backtab);
        assert!(tab.matches_sequence(&[KeyEvent {
            code: KeyCode::Tab,
            mods: KeyMods::default(),
        }]));
        assert!(backtab.matches_sequence(&[KeyEvent {
            code: KeyCode::BackTab,
            mods: KeyMods::default(),
        }]));
        assert!(!backtab.matches_sequence(&[KeyEvent {
            code: KeyCode::Tab,
            mods: KeyMods::default(),
        }]));
    }

    #[test]
    fn rejects_invalid_bindings() {
        assert!(KeyBinding::from_str("").is_err());
        assert!(KeyBinding::from_str("ctrl-").is_err());
        assert!(KeyBindings::from_str(" , ").is_err());
    }

    #[test]
    fn formats_lowercase_variants() {
        assert_eq!(
            format_binding_lowercase("ctrl+shift+up").unwrap(),
            "ctrl+shift+up"
        );
        assert_eq!(format_binding_lowercase("Esc").unwrap(), "esc");
        assert_eq!(
            format_bindings_lowercase("ctrl+d, super+q").unwrap(),
            "ctrl+d / cmd+q"
        );
    }

    // --- Chord support ---

    #[test]
    fn parses_chord_binding() {
        let chord = KeyBinding::from_str("ctrl+x b").expect("chord parses");
        assert!(chord.is_chord());
        assert_eq!(chord.step_count(), 2);
        assert_eq!(chord.canonical(), "Ctrl+X B");
    }

    #[test]
    fn chord_matches_sequence() {
        let chord = KeyBinding::from_str("ctrl+x b").expect("chord parses");
        let events = [
            KeyEvent {
                code: KeyCode::Char('x'),
                mods: KeyMods {
                    ctrl: true,
                    ..KeyMods::default()
                },
            },
            KeyEvent {
                code: KeyCode::Char('b'),
                mods: KeyMods::default(),
            },
        ];
        assert!(chord.matches_sequence(&events));
    }

    #[test]
    fn formats_chord_binding() {
        assert_eq!(format_binding("ctrl+x b").unwrap(), "Ctrl+X B");
        assert_eq!(format_binding_lowercase("ctrl+x b").unwrap(), "ctrl+x b");
    }

    #[test]
    fn formats_chord_with_alternatives() {
        assert_eq!(
            format_bindings("ctrl+x b, ctrl+q").unwrap(),
            "Ctrl+X B / Ctrl+Q"
        );
    }

    #[test]
    fn chord_display_three_steps() {
        let chord = KeyBinding::from_str("ctrl+x a b").expect("three-step chord parses");
        assert_eq!(chord.step_count(), 3);
        assert_eq!(chord.canonical(), "Ctrl+X A B");
    }

    #[test]
    fn single_binding_is_not_chord() {
        let single = KeyBinding::from_str("ctrl+c").expect("single parses");
        assert!(!single.is_chord());
        assert_eq!(single.step_count(), 1);
    }

    #[test]
    fn chord_equality() {
        let a = KeyBinding::from_str("ctrl+x b").expect("a parses");
        let b = KeyBinding::from_str("ctrl-x b").expect("b parses");
        assert_eq!(a, b);
    }

    // --- ChordMatcher ---

    fn ctrl_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        }
    }

    fn plain_key(c: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(c),
            mods: KeyMods::default(),
        }
    }

    #[test]
    fn chord_matcher_single_step() {
        let mut matcher =
            ChordMatcher::new(vec![(KeyBinding::from_str("ctrl+q").unwrap(), "quit")]);

        assert_eq!(matcher.feed(&ctrl_key('q')), ChordResult::Matched(&"quit"));
        assert_eq!(matcher.feed(&plain_key('x')), ChordResult::None);
    }

    #[test]
    fn chord_matcher_two_step_chord() {
        let mut matcher = ChordMatcher::new(vec![
            (KeyBinding::from_str("ctrl+x b").unwrap(), "sidebar"),
            (KeyBinding::from_str("ctrl+x l").unwrap(), "list"),
        ]);

        assert_eq!(matcher.feed(&ctrl_key('x')), ChordResult::Pending);
        assert_eq!(
            matcher.feed(&plain_key('b')),
            ChordResult::Matched(&"sidebar")
        );

        // Second chord
        assert_eq!(matcher.feed(&ctrl_key('x')), ChordResult::Pending);
        assert_eq!(matcher.feed(&plain_key('l')), ChordResult::Matched(&"list"));
    }

    #[test]
    fn chord_matcher_resets_on_wrong_second_key() {
        let mut matcher =
            ChordMatcher::new(vec![(KeyBinding::from_str("ctrl+x b").unwrap(), "sidebar")]);

        assert_eq!(matcher.feed(&ctrl_key('x')), ChordResult::Pending);
        // Wrong second key - should reset
        assert_eq!(matcher.feed(&plain_key('z')), ChordResult::None);
        assert!(!matcher.is_pending());
    }

    #[test]
    fn chord_matcher_prefix_defers_single_match() {
        let mut matcher = ChordMatcher::new(vec![
            (KeyBinding::from_str("ctrl+x").unwrap(), "cut"),
            (KeyBinding::from_str("ctrl+x b").unwrap(), "sidebar"),
        ]);

        // ctrl+x matches "cut" but also starts "ctrl+x b" - should be Pending
        assert_eq!(matcher.feed(&ctrl_key('x')), ChordResult::Pending);
        // b completes the chord
        assert_eq!(
            matcher.feed(&plain_key('b')),
            ChordResult::Matched(&"sidebar")
        );
    }

    #[test]
    fn chord_matcher_wrong_continuation_tries_fresh() {
        let mut matcher = ChordMatcher::new(vec![
            (KeyBinding::from_str("ctrl+x b").unwrap(), "sidebar"),
            (KeyBinding::from_str("ctrl+q").unwrap(), "quit"),
        ]);

        assert_eq!(matcher.feed(&ctrl_key('x')), ChordResult::Pending);
        // ctrl+q doesn't continue the chord, but it IS a fresh match
        assert_eq!(matcher.feed(&ctrl_key('q')), ChordResult::Matched(&"quit"));
    }

    #[test]
    fn chord_matcher_manual_reset() {
        let mut matcher =
            ChordMatcher::new(vec![(KeyBinding::from_str("ctrl+x b").unwrap(), "sidebar")]);

        assert_eq!(matcher.feed(&ctrl_key('x')), ChordResult::Pending);
        assert!(matcher.is_pending());
        matcher.reset();
        assert!(!matcher.is_pending());
    }
}
