use crate::core::event::KeyEvent;
use crate::text::buffer::TextBufferCore;
use crate::text::edit::TextEditKind;
use crate::utils::text::clamp_cursor;
use crate::widgets::InputEvent;
use std::ops::{Deref, DerefMut};

/// A minimal, single-line text input model with selection support.
///
/// The cursor is stored as a byte index into the UTF-8 string and is always kept on a
/// character boundary. Selection is represented by an optional anchor position.
///
/// All shared text-buffer operations (cursor movement, selection, word
/// boundaries, undo/redo, etc.) are provided via [`Deref`]/[`DerefMut`] to the
/// inner `TextBufferCore`.
#[derive(Clone, Debug)]
pub struct TextInput {
    pub(crate) core: TextBufferCore,
}

impl Default for TextInput {
    fn default() -> Self {
        Self::new("")
    }
}

impl PartialEq for TextInput {
    fn eq(&self, other: &Self) -> bool {
        self.core == other.core
    }
}

impl Eq for TextInput {}

impl Deref for TextInput {
    type Target = TextBufferCore;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl DerefMut for TextInput {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

impl TextInput {
    /// Create a new input with the cursor at the end.
    pub fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let cursor = text.len();
        Self {
            core: TextBufferCore {
                text,
                cursor,
                anchor: None,
                history: TextBufferCore::fresh_history(),
                last_edit: None,
            },
        }
    }

    pub(crate) fn insert_text(&mut self, text: &str) -> bool {
        if text.is_empty() {
            return false;
        }

        let text = text.replace('\n', " ").replace('\r', "");
        let (start, end) = self
            .core
            .selection()
            .unwrap_or((self.core.cursor, self.core.cursor));
        self.core
            .replace_range(start, end, &text, TextEditKind::Replace)
    }

    /// Clear the input.
    pub fn clear(&mut self) {
        self.clear_changed();
    }

    pub(crate) fn clear_changed(&mut self) -> bool {
        self.core.clear_preserving_history()
    }

    /// Apply an [`InputEvent`] to this state bundle.
    pub fn apply(&mut self, ev: &InputEvent) {
        self.core.text = ev.value.to_string();
        self.core.cursor = clamp_cursor(&self.core.text, ev.cursor);
        self.core.anchor = ev
            .anchor
            .map(|anchor| clamp_cursor(&self.core.text, anchor));
    }

    /// Handle a key event.
    ///
    /// Returns `true` if the input state changed (text, cursor, or selection).
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.handle_key_with(key, crate::app::input::keymap::default_keymap())
    }

    pub(crate) fn handle_key_with(
        &mut self,
        key: KeyEvent,
        keymap: &crate::app::input::keymap::Keymap,
    ) -> bool {
        self.handle_key_with_masked(key, keymap, false)
    }

    pub(crate) fn handle_key_with_masked(
        &mut self,
        key: KeyEvent,
        keymap: &crate::app::input::keymap::Keymap,
        masked: bool,
    ) -> bool {
        use crate::app::input::keymap::Action;

        let action = keymap.resolve_action(key);

        if masked {
            match action {
                Action::MoveWordLeft => return self.move_home(),
                Action::MoveWordRight => return self.move_end(),
                Action::SelectWordLeft => return self.select_home(),
                Action::SelectWordRight => return self.select_end(),
                Action::DeleteWordLeft => return self.delete_to_start(),
                Action::DeleteWordRight => return self.delete_to_end(),
                _ => {}
            }
        }

        self.handle_action(action)
    }

    fn handle_action(&mut self, action: crate::app::input::keymap::Action) -> bool {
        use crate::app::input::keymap::Action;

        match action {
            Action::MoveLeft => self.core.move_left(),
            Action::MoveRight => self.core.move_right(),
            Action::MoveWordLeft => self.core.move_word_left(),
            Action::MoveWordRight => self.core.move_word_right(),
            Action::MoveHome => self.move_home(),
            Action::MoveEnd => self.move_end(),

            Action::SelectLeft => self.core.select_left(),
            Action::SelectRight => self.core.select_right(),
            Action::SelectWordLeft => self.core.select_word_left(),
            Action::SelectWordRight => self.core.select_word_right(),
            Action::SelectHome => self.select_home(),
            Action::SelectEnd => self.select_end(),
            Action::SelectAll => self.core.select_all(),

            Action::Backspace => self.core.backspace(),
            Action::Clear => self.clear_changed(),
            Action::Delete => self.core.delete(),
            Action::DeleteWordLeft => self.core.delete_word_left(),
            Action::DeleteWordRight => self.core.delete_word_right(),

            Action::Copy
            | Action::Cut
            | Action::Paste
            | Action::PasteFromSelection
            | Action::CopyImage
            | Action::PasteImage
            | Action::ToggleDevTools
            | Action::Quit => false,
            Action::Undo => self.core.undo(),
            Action::Redo => self.core.redo(),

            Action::InsertChar(c) => self.core.insert_char(c),

            Action::MoveUp => self.move_home(),
            Action::MoveDown => self.move_end(),
            Action::SelectUp => self.select_home(),
            Action::SelectDown => self.select_end(),

            // Single-line input ignores these
            Action::InsertNewline
            | Action::DismissOverlay
            | Action::FocusNext
            | Action::FocusPrev
            | Action::None => false,
        }
    }

    /// Delete from the start to the cursor or selection.
    pub fn delete_to_start(&mut self) -> bool {
        if self.core.delete_selection() {
            return true;
        }
        if self.core.cursor == 0 {
            return false;
        }
        let cursor = self.core.cursor;
        self.core
            .replace_range(0, cursor, "", TextEditKind::DeleteBackspace)
    }

    /// Delete from the cursor to the end or selection.
    pub fn delete_to_end(&mut self) -> bool {
        if self.core.delete_selection() {
            return true;
        }
        if self.core.cursor >= self.core.text.len() {
            return false;
        }
        let cursor = self.core.cursor;
        let len = self.core.text.len();
        self.core
            .replace_range(cursor, len, "", TextEditKind::DeleteForward)
    }

    /// Move to the start, clearing selection (string boundary for single-line).
    pub fn move_home(&mut self) -> bool {
        self.core.anchor = None;
        if self.core.cursor == 0 {
            return false;
        }
        self.core.cursor = 0;
        true
    }

    /// Move to the end, clearing selection (string boundary for single-line).
    pub fn move_end(&mut self) -> bool {
        self.core.anchor = None;
        let end = self.core.text.len();
        if self.core.cursor == end {
            return false;
        }
        self.core.cursor = end;
        true
    }

    /// Extend selection to the start (string boundary for single-line).
    pub fn select_home(&mut self) -> bool {
        if self.core.cursor == 0 {
            return false;
        }
        if self.core.anchor.is_none() {
            self.core.anchor = Some(self.core.cursor);
        }
        self.core.cursor = 0;
        self.core.collapse_empty_selection();
        true
    }

    /// Extend selection to the end (string boundary for single-line).
    pub fn select_end(&mut self) -> bool {
        let end = self.core.text.len();
        if self.core.cursor == end {
            return false;
        }
        if self.core.anchor.is_none() {
            self.core.anchor = Some(self.core.cursor);
        }
        self.core.cursor = end;
        self.core.collapse_empty_selection();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::input::keymap::default_keymap;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};
    use crate::widgets::InputEvent;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods::default(),
        }
    }

    fn shift(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods {
                shift: true,
                ..KeyMods::default()
            },
        }
    }

    fn alt(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods {
                alt: true,
                ..KeyMods::default()
            },
        }
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        }
    }

    fn shift_alt(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods {
                shift: true,
                alt: true,
                ..KeyMods::default()
            },
        }
    }

    #[test]
    fn insert_and_backspace() {
        let mut input = TextInput::new("");
        assert!(input.handle_key(key(KeyCode::Char('a'))));
        assert_eq!(input.text(), "a");
        assert_eq!(input.cursor(), 1);

        assert!(input.handle_key(key(KeyCode::Backspace)));
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn alt_word_movement() {
        let mut input = TextInput::new("hello world");
        assert_eq!(input.cursor(), 11);

        assert!(input.handle_key(alt(KeyCode::Left)));
        assert_eq!(input.cursor(), 6);

        assert!(input.handle_key(alt(KeyCode::Left)));
        assert_eq!(input.cursor(), 0);

        assert!(input.handle_key(alt(KeyCode::Right)));
        assert_eq!(input.cursor(), 5);

        assert!(input.handle_key(alt(KeyCode::Right)));
        assert_eq!(input.cursor(), 11);
    }

    #[test]
    fn masked_word_actions_treat_as_single_word() {
        let keymap = default_keymap();
        let mut input = TextInput::new("hello world");
        input.set_cursor(6);

        assert!(input.handle_key_with_masked(alt(KeyCode::Left), keymap, true));
        assert_eq!(input.cursor(), 0);

        assert!(input.handle_key_with_masked(alt(KeyCode::Right), keymap, true));
        assert_eq!(input.cursor(), input.text().len());

        input.set_cursor(5);
        assert!(input.handle_key_with_masked(shift_alt(KeyCode::Left), keymap, true));
        assert_eq!(input.selection(), Some((0, 5)));

        let len = input.text().len();
        input.set_cursor(len);
        assert!(input.handle_key_with_masked(alt(KeyCode::Backspace), keymap, true));
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn ctrl_word_movement() {
        let mut input = TextInput::new("hello world");
        assert_eq!(input.cursor(), 11);

        assert!(input.handle_key(ctrl(KeyCode::Left)));
        assert_eq!(input.cursor(), 6);

        assert!(input.handle_key(ctrl(KeyCode::Left)));
        assert_eq!(input.cursor(), 0);

        assert!(input.handle_key(ctrl(KeyCode::Right)));
        assert_eq!(input.cursor(), 5);

        assert!(input.handle_key(ctrl(KeyCode::Right)));
        assert_eq!(input.cursor(), 11);
    }

    #[test]
    fn shift_arrow_selection() {
        let mut input = TextInput::new("hello");
        input.set_cursor(2); // "he|llo"

        // Select right
        assert!(input.handle_key(shift(KeyCode::Right)));
        assert_eq!(input.cursor(), 3);
        assert_eq!(input.selection(), Some((2, 3)));
        assert_eq!(input.selected_text(), Some("l"));

        // Extend selection
        assert!(input.handle_key(shift(KeyCode::Right)));
        assert_eq!(input.cursor(), 4);
        assert_eq!(input.selection(), Some((2, 4)));
        assert_eq!(input.selected_text(), Some("ll"));

        // Clear selection by moving without shift
        assert!(input.handle_key(key(KeyCode::Right)));
        assert_eq!(input.cursor(), 4); // Moves to end of selection
        assert_eq!(input.selection(), None);
    }

    #[test]
    fn up_down_move_to_single_line_boundaries() {
        let mut input = TextInput::new("hello");
        input.set_cursor(2);

        assert!(input.handle_key(key(KeyCode::Up)));
        assert_eq!(input.cursor(), 0);
        assert!(!input.handle_key(key(KeyCode::Up)));

        input.set_cursor(2);
        assert!(input.handle_key(key(KeyCode::Down)));
        assert_eq!(input.cursor(), 5);
        assert!(!input.handle_key(key(KeyCode::Down)));
    }

    #[test]
    fn shift_up_down_select_single_line_boundaries() {
        let mut input = TextInput::new("hello");
        input.set_cursor(2);

        assert!(input.handle_key(shift(KeyCode::Up)));
        assert_eq!(input.cursor(), 0);
        assert_eq!(input.selection(), Some((0, 2)));

        input.set_cursor(2);
        assert!(input.handle_key(shift(KeyCode::Down)));
        assert_eq!(input.cursor(), 5);
        assert_eq!(input.selection(), Some((2, 5)));
    }

    #[test]
    fn select_and_delete() {
        let mut input = TextInput::new("hello");
        input.set_cursor(1);

        // Select "ell"
        input.handle_key(shift(KeyCode::Right));
        input.handle_key(shift(KeyCode::Right));
        input.handle_key(shift(KeyCode::Right));
        assert_eq!(input.selected_text(), Some("ell"));

        // Delete selection
        input.handle_key(key(KeyCode::Backspace));
        assert_eq!(input.text(), "ho");
        assert_eq!(input.cursor(), 1);
        assert_eq!(input.selection(), None);
    }

    #[test]
    fn select_and_type_replaces() {
        let mut input = TextInput::new("hello");
        input.set_cursor(1);

        // Select "ell"
        input.handle_key(shift(KeyCode::Right));
        input.handle_key(shift(KeyCode::Right));
        input.handle_key(shift(KeyCode::Right));

        // Type to replace
        input.handle_key(key(KeyCode::Char('X')));
        assert_eq!(input.text(), "hXo");
        assert_eq!(input.cursor(), 2);
    }

    #[test]
    fn alt_backspace_deletes_word() {
        let mut input = TextInput::new("hello world");
        assert_eq!(input.cursor(), 11);

        assert!(input.handle_key(alt(KeyCode::Backspace)));
        assert_eq!(input.text(), "hello ");
        assert_eq!(input.cursor(), 6);

        assert!(input.handle_key(alt(KeyCode::Backspace)));
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor(), 0);
    }

    #[test]
    fn shift_alt_word_selection() {
        let mut input = TextInput::new("hello world");
        input.set_cursor(6); // "hello |world"

        assert!(input.handle_key(shift_alt(KeyCode::Right)));
        assert_eq!(input.cursor(), 11);
        assert_eq!(input.selected_text(), Some("world"));

        assert!(input.handle_key(shift_alt(KeyCode::Left)));
        assert_eq!(input.cursor(), 6);
        assert_eq!(input.selection(), None); // Collapsed back
    }

    #[test]
    fn select_all() {
        let mut input = TextInput::new("hello");
        input.set_cursor(2);

        input.handle_key(KeyEvent {
            code: KeyCode::Char('a'),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        });
        assert_eq!(input.selection(), Some((0, 5)));
        assert_eq!(input.selected_text(), Some("hello"));
    }

    #[test]
    fn undo_redo_inserts() {
        let mut input = TextInput::new("");
        assert!(input.handle_key(key(KeyCode::Char('a'))));
        assert!(input.handle_key(key(KeyCode::Char('b'))));
        assert!(input.handle_key(key(KeyCode::Char('c'))));
        assert_eq!(input.text(), "abc");

        assert!(input.handle_key(ctrl(KeyCode::Char('z'))));
        assert_eq!(input.text(), "");

        assert!(input.handle_key(ctrl(KeyCode::Char('y'))));
        assert_eq!(input.text(), "abc");
    }

    #[test]
    fn clear_action_is_undoable_with_cursor_and_anchor() {
        let mut input = TextInput::new("hello");
        input.set_cursor(4);
        input.set_anchor(Some(1));

        assert!(input.handle_action(crate::app::input::keymap::Action::Clear));
        assert_eq!(input.text(), "");
        assert_eq!(input.cursor(), 0);
        assert_eq!(input.anchor(), None);

        assert!(input.undo());
        assert_eq!(input.text(), "hello");
        assert_eq!(input.cursor(), 4);
        assert_eq!(input.anchor(), Some(1));
    }

    #[test]
    fn undo_merges_trailing_space() {
        let mut input = TextInput::new("");
        assert!(input.handle_key(key(KeyCode::Char('a'))));
        assert!(input.handle_key(key(KeyCode::Char('b'))));
        assert!(input.handle_key(key(KeyCode::Char(' '))));
        assert_eq!(input.text(), "ab ");

        assert!(input.handle_key(ctrl(KeyCode::Char('z'))));
        assert_eq!(input.text(), "");

        assert!(input.handle_key(ctrl(KeyCode::Char('y'))));
        assert_eq!(input.text(), "ab ");
    }

    #[test]
    fn toggle_devtools_action_is_non_editing() {
        let mut input = TextInput::new("hello");
        let changed = input.handle_key(KeyEvent {
            code: KeyCode::F(12),
            mods: KeyMods::default(),
        });

        assert!(!changed);
        assert_eq!(input.text(), "hello");
        assert_eq!(input.cursor(), 5);
    }

    #[test]
    fn apply_updates_text_cursor_and_anchor_after_insert() {
        let mut input = TextInput::new("hello");
        InputEvent {
            value: "hello!".into(),
            cursor: 6,
            anchor: None,
        }
        .apply_to(&mut input);

        assert_eq!(input.text(), "hello!");
        assert_eq!(input.cursor(), 6);
        assert_eq!(input.anchor(), None);
    }

    #[test]
    fn apply_updates_state_after_delete() {
        let mut input = TextInput::new("hello");
        InputEvent {
            value: "helo".into(),
            cursor: 3,
            anchor: None,
        }
        .apply_to(&mut input);

        assert_eq!(input.text(), "helo");
        assert_eq!(input.cursor(), 3);
        assert_eq!(input.anchor(), None);
    }

    #[test]
    fn apply_updates_cursor_navigation_without_text_change() {
        let mut input = TextInput::new("hello");
        InputEvent {
            value: "hello".into(),
            cursor: 1,
            anchor: None,
        }
        .apply_to(&mut input);

        assert_eq!(input.text(), "hello");
        assert_eq!(input.cursor(), 1);
        assert_eq!(input.anchor(), None);
    }

    #[test]
    fn apply_updates_selection_anchor() {
        let mut input = TextInput::new("hello");
        InputEvent {
            value: "hello".into(),
            cursor: 4,
            anchor: Some(1),
        }
        .apply_to(&mut input);

        assert_eq!(input.selection(), Some((1, 4)));
        assert_eq!(input.selected_text(), Some("ell"));
    }

    #[test]
    fn apply_clamps_cursor_and_anchor_inside_unicode_characters() {
        let value = "części";
        let cursor_inside_s = 5;
        assert!(!value.is_char_boundary(cursor_inside_s));

        let mut input = TextInput::new("");
        InputEvent {
            value: value.into(),
            cursor: cursor_inside_s,
            anchor: Some(cursor_inside_s),
        }
        .apply_to(&mut input);

        assert_eq!(input.cursor(), 4);
        assert_eq!(input.anchor(), Some(4));
        assert!(input.text().is_char_boundary(input.cursor()));
    }

    #[test]
    fn buffer_selection_and_replace_tolerate_unicode_boundary_drift() {
        let mut input = TextInput::new("części");
        input.core.cursor = 5;
        input.core.anchor = Some(input.text().len());

        assert_eq!(input.selected_text(), Some("ści"));
        assert!(input.delete_to_end());
        assert_eq!(input.text(), "czę");

        input.core.cursor = 3;
        assert!(!input.text().is_char_boundary(input.core.cursor));
        assert!(input.insert_char('X'));
        assert_eq!(input.text(), "czXę");

        let mut input = TextInput::new("części");
        input.core.cursor = 5;
        assert!(input.move_right());
        assert_eq!(input.cursor(), 6);

        input.core.cursor = 5;
        assert!(input.delete());
        assert_eq!(input.text(), "częci");
    }
}
