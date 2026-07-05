/// Mouse button.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    /// Left mouse button.
    Left,
    /// Right mouse button.
    Right,
    /// Middle mouse button.
    Middle,
}

/// High-level mouse event kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseKind {
    /// Button pressed.
    Down(MouseButton),
    /// Button released.
    Up(MouseButton),
    /// Mouse moved while a button is pressed.
    Drag(MouseButton),
    /// Mouse moved.
    Moved,
    /// Scroll up.
    ScrollUp,
    /// Scroll down.
    ScrollDown,
}

/// A mouse event in terminal coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MouseEvent {
    /// X coordinate (column).
    pub x: u16,
    /// Y coordinate (row).
    pub y: u16,
    /// Event kind.
    pub kind: MouseKind,
    /// Modifiers.
    pub mods: KeyMods,
}

/// A mouse-move event with both global and local coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MouseMoveEvent {
    /// Global X coordinate (column) in content-space.
    pub x: u16,
    /// Global Y coordinate (row) in content-space.
    pub y: u16,
    /// X coordinate relative to the target widget rect.
    pub local_x: u16,
    /// Y coordinate relative to the target widget rect.
    pub local_y: u16,
    /// Target widget width in cells.
    pub target_w: u16,
    /// Target widget height in cells.
    pub target_h: u16,
    /// Modifiers for the move event.
    pub mods: KeyMods,
}

/// A mouse drag event with global and region-local coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct MouseDragEvent {
    /// Global X coordinate where the drag started.
    pub from_x: u16,
    /// Global Y coordinate where the drag started.
    pub from_y: u16,
    /// Starting X coordinate relative to the target widget rect.
    pub from_local_x: u16,
    /// Starting Y coordinate relative to the target widget rect.
    pub from_local_y: u16,
    /// Current global X coordinate.
    pub x: u16,
    /// Current global Y coordinate.
    pub y: u16,
    /// Current X coordinate relative to the target widget rect.
    pub local_x: u16,
    /// Current Y coordinate relative to the target widget rect.
    pub local_y: u16,
    /// X delta since the previous drag tick.
    pub delta_x: i16,
    /// Y delta since the previous drag tick.
    pub delta_y: i16,
    /// Target widget width in cells.
    pub target_w: u16,
    /// Target widget height in cells.
    pub target_h: u16,
    /// Modifiers for the drag event.
    pub mods: KeyMods,
}

/// Key modifiers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct KeyMods {
    /// Control modifier.
    pub ctrl: bool,
    /// Alt modifier.
    pub alt: bool,
    /// Shift modifier.
    pub shift: bool,
    /// Super modifier (Windows/Command/Meta).
    pub super_key: bool,
}

impl KeyMods {
    /// No modifiers.
    pub const NONE: Self = Self {
        ctrl: false,
        alt: false,
        shift: false,
        super_key: false,
    };

    /// Shift modifier only.
    pub const SHIFT: Self = Self {
        ctrl: false,
        alt: false,
        shift: true,
        super_key: false,
    };

    /// Control modifier only.
    pub const CTRL: Self = Self {
        ctrl: true,
        alt: false,
        shift: false,
        super_key: false,
    };

    /// Alt modifier only.
    pub const ALT: Self = Self {
        ctrl: false,
        alt: true,
        shift: false,
        super_key: false,
    };

    /// Returns true when no modifiers are set.
    pub fn is_empty(&self) -> bool {
        !self.ctrl && !self.alt && !self.shift && !self.super_key
    }
}

/// Key code.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyCode {
    /// A unicode character.
    Char(char),
    /// Insert.
    Insert,
    /// Enter.
    Enter,
    /// Escape.
    Esc,
    /// Tab.
    Tab,
    /// Shift+Tab.
    BackTab,
    /// Backspace.
    Backspace,
    /// Delete.
    Delete,
    /// Home.
    Home,
    /// End.
    End,
    /// Page up.
    PageUp,
    /// Page down.
    PageDown,
    /// Arrow up.
    Up,
    /// Arrow down.
    Down,
    /// Arrow left.
    Left,
    /// Arrow right.
    Right,
    /// Function key.
    F(u8),
}

/// A keyboard event.
///
/// Common pitfall: matching only `key.code` ignores modifiers. Prefer
/// `key.is(...)` for plain-key checks or `key.is_with(...)` for exact
/// key+modifier combinations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyEvent {
    /// Key code.
    pub code: KeyCode,
    /// Modifiers.
    pub mods: KeyMods,
}

impl KeyEvent {
    /// Returns true when this key matches `code` with no modifiers.
    pub fn is(&self, code: KeyCode) -> bool {
        self.code == code && self.mods.is_empty()
    }

    /// Returns true when this key matches `code` with exactly `mods`.
    pub fn is_with(&self, code: KeyCode, mods: KeyMods) -> bool {
        self.code == code && self.mods == mods
    }

    /// Formats the key event into a readable string (e.g., "Ctrl+E" or "ctrl+e").
    ///
    /// If `lowercase` is true, the resulting string will be in lowercase.
    pub fn to_formatted_string(&self, lowercase: bool) -> String {
        let mut parts = Vec::new();

        if self.mods.ctrl {
            parts.push("Ctrl");
        }
        if self.mods.alt {
            parts.push("Alt");
        }
        if self.mods.super_key {
            parts.push("Super");
        }
        if self.mods.shift {
            parts.push("Shift");
        }

        let code_str = match self.code {
            KeyCode::Char(c) => {
                if c == ' ' {
                    "Space".to_string()
                } else {
                    c.to_uppercase().to_string()
                }
            }
            KeyCode::Insert => "Insert".to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::BackTab => "BackTab".to_string(),
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Delete => "Delete".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::PageUp => "PageUp".to_string(),
            KeyCode::PageDown => "PageDown".to_string(),
            KeyCode::Up => "Up".to_string(),
            KeyCode::Down => "Down".to_string(),
            KeyCode::Left => "Left".to_string(),
            KeyCode::Right => "Right".to_string(),
            KeyCode::F(n) => format!("F{n}"),
        };
        parts.push(&code_str);

        let result = parts.join("+");
        if lowercase {
            result.to_lowercase()
        } else {
            result
        }
    }
}

impl std::fmt::Display for KeyEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_formatted_string(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_event_formatting() {
        let key1 = KeyEvent {
            code: KeyCode::Char('e'),
            mods: KeyMods {
                ctrl: true,
                alt: false,
                shift: false,
                super_key: false,
            },
        };
        assert_eq!(key1.to_formatted_string(false), "Ctrl+E");
        assert_eq!(key1.to_formatted_string(true), "ctrl+e");
        assert_eq!(key1.to_string(), "Ctrl+E");

        let key2 = KeyEvent {
            code: KeyCode::Enter,
            mods: KeyMods {
                ctrl: true,
                alt: true,
                shift: true,
                super_key: false,
            },
        };
        assert_eq!(key2.to_formatted_string(false), "Ctrl+Alt+Shift+Enter");
        assert_eq!(key2.to_formatted_string(true), "ctrl+alt+shift+enter");
        assert_eq!(key2.to_string(), "Ctrl+Alt+Shift+Enter");

        let key3 = KeyEvent {
            code: KeyCode::Char(' '),
            mods: KeyMods {
                ctrl: false,
                alt: false,
                shift: false,
                super_key: false,
            },
        };
        assert_eq!(key3.to_formatted_string(false), "Space");
        assert_eq!(key3.to_string(), "Space");

        let key4 = KeyEvent {
            code: KeyCode::F(12),
            mods: KeyMods {
                ctrl: false,
                alt: false,
                shift: false,
                super_key: false,
            },
        };
        assert_eq!(key4.to_formatted_string(false), "F12");
        assert_eq!(key4.to_formatted_string(true), "f12");
        assert_eq!(key4.to_string(), "F12");
    }

    #[test]
    fn test_key_event_matching_helpers_and_mod_constants() {
        let plain_enter = KeyEvent {
            code: KeyCode::Enter,
            mods: KeyMods::NONE,
        };
        assert!(plain_enter.is(KeyCode::Enter));
        assert!(plain_enter.is_with(KeyCode::Enter, KeyMods::NONE));

        let shift_enter = KeyEvent {
            code: KeyCode::Enter,
            mods: KeyMods::SHIFT,
        };
        assert!(!shift_enter.is(KeyCode::Enter));
        assert!(shift_enter.is_with(KeyCode::Enter, KeyMods::SHIFT));
        assert!(!shift_enter.is_with(KeyCode::Enter, KeyMods::NONE));

        assert!(KeyMods::NONE.is_empty());
        assert!(!KeyMods::SHIFT.is_empty());
    }
}
