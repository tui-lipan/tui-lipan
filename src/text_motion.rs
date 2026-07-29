//! Public, byte-offset text motion helpers shared with the vim-mode `TextArea` widget.
//!
//! Every function takes a `&str` and a byte offset and returns a new byte offset; offsets are
//! always clamped to a UTF-8 char boundary. These are the exact algorithms `TextArea`'s vim mode
//! uses internally for `w`/`b`/`e`/`0`/`^`/`$`, promoted to a stable public surface so apps that
//! render their own text grids (for example a terminal emulator's scrollback copy mode) can reuse
//! vim-style word/line motions instead of reimplementing them.
//!
//! # Cursor convention
//!
//! Offsets follow the same "insertion point" convention as [`crate::TextEditor`]: a cursor value
//! `N` sits *between* the bytes at `N - 1` and `N`, not "on" the character at `N`. This matters
//! most for [`word_end`] and [`big_word_end`], which land the cursor one byte **past** the last
//! character of the word (so the returned offset can equal `text.len()` and is not always a valid
//! index to read a character *from*).
//!
//! If your own cursor model instead tracks a selected *cell* (as in a terminal grid, where the
//! cursor always occupies a visible character), convert to an insertion point by adding the byte
//! width of the character under the cursor before calling [`word_end`] / [`big_word_end`], then
//! map the result back down to the cell at `offset - 1`. Feeding a cell's own start byte directly
//! into [`word_end`] breaks the case where the cursor already sits on a word's last character:
//! since that byte still belongs to the current word, the motion re-finds the same word's end
//! instead of advancing to the next word.
//!
//! The cell-cursor adapters below take **character columns**, because they operate on text rows.
//! Convert those columns to rendered display columns with the span helpers in
//! [`crate::utils::spans`] before using them against a styled grid.
//!
//! ```
//! use tui_lipan::text_motion::word_end;
//!
//! let text = "cat dog";
//! // Insertion point 3 sits right after 'cat' (between the 't' and the space).
//! assert_eq!(word_end(text, 3), 7); // -> end of "dog"
//!
//! // Feeding the *cell* start byte of 't' (2) instead re-finds the same word's end (3),
//! // which looks like "no progress" to a cell-based caller expecting to land on 'g' (6).
//! assert_eq!(word_end(text, 2), 3);
//! ```

/// Move forward to the start of the next vim "word" (`w`): skip the remainder of the current
/// word/punctuation run, then any following whitespace.
///
/// ```
/// use tui_lipan::text_motion::word_forward_start;
///
/// assert_eq!(word_forward_start("cat dog", 0), 4);
/// ```
pub use crate::app::input::text_area_vim::vim_word_forward_start as word_forward_start;

/// Move backward to the start of the previous vim "word" (`b`).
///
/// ```
/// use tui_lipan::text_motion::word_backward_start;
///
/// assert_eq!(word_backward_start("cat dog", 7), 4);
/// ```
pub use crate::app::input::text_area_vim::vim_word_backward_start as word_backward_start;

/// Move to the end of the current or next vim "word" (`e`).
///
/// Returns an insertion-point offset one byte past the word's last character — see the
/// "Cursor convention" section of the [`crate::text_motion`] module docs before feeding in a
/// cell-based cursor.
///
/// ```
/// use tui_lipan::text_motion::word_end;
///
/// assert_eq!(word_end("cat dog", 0), 3);
/// ```
pub use crate::app::input::text_area_vim::vim_word_end as word_end;

/// Move forward to the start of the next vim WORD (`W`): a whitespace-delimited run that
/// includes punctuation, unlike a "word".
///
/// ```
/// use tui_lipan::text_motion::big_word_forward_start;
///
/// assert_eq!(big_word_forward_start("foo.bar baz", 0), 8);
/// ```
pub use crate::app::input::text_area_vim::vim_big_word_forward_start as big_word_forward_start;

/// Move backward to the start of the previous vim WORD (`B`).
///
/// ```
/// use tui_lipan::text_motion::big_word_backward_start;
///
/// assert_eq!(big_word_backward_start("foo.bar baz", 11), 8);
/// ```
pub use crate::app::input::text_area_vim::vim_big_word_backward_start as big_word_backward_start;

/// Move to the end of the current or next vim WORD (`E`).
///
/// Returns an insertion-point offset one byte past the WORD's last character — see the
/// "Cursor convention" section of the [`crate::text_motion`] module docs.
///
/// ```
/// use tui_lipan::text_motion::big_word_end;
///
/// assert_eq!(big_word_end("foo.bar baz", 0), 7);
/// ```
pub use crate::app::input::text_area_vim::vim_big_word_end as big_word_end;

/// Byte offset of the start of the line containing `cursor` (`0` motion target).
///
/// ```
/// use tui_lipan::text_motion::line_start_at;
///
/// assert_eq!(line_start_at("foo\nbar", 5), 4);
/// ```
pub use crate::app::input::text_area_vim::line_start_at;

/// Byte offset one past the end of the line containing `cursor` (`$` motion target, exclusive).
///
/// ```
/// use tui_lipan::text_motion::line_end_at;
///
/// assert_eq!(line_end_at("foo\nbar", 5), 7);
/// ```
pub use crate::app::input::text_area_vim::line_end_at;

/// Byte offset of the first non-blank character in `text[line_start..line_end]` (`^` motion
/// target), or `line_end` if the line is entirely blank.
///
/// ```
/// use tui_lipan::text_motion::first_nonblank_in_line;
///
/// assert_eq!(first_nonblank_in_line("  bar", 0, 5), 2);
/// ```
pub use crate::app::input::text_area_vim::first_nonblank_in_line;

// ─────────────────────────────────────────────────────────────────────────────
// Cell-cursor adapters
// ─────────────────────────────────────────────────────────────────────────────

/// Convert a character-column cursor into a UTF-8 byte offset.
///
/// These adapters use **character columns**, not rendered display columns. They are for text
/// rows (`&str`); convert the result with a span helper before using it to index a rendered grid.
/// Columns past the end of the row map to `row.len()`.
///
/// ```
/// use tui_lipan::text_motion::char_col_to_byte;
///
/// assert_eq!(char_col_to_byte("hé", 1), 1);
/// assert_eq!(char_col_to_byte("hé", 2), 3);
/// ```
pub fn char_col_to_byte(row: &str, col: usize) -> usize {
    row.char_indices()
        .nth(col)
        .map(|(byte, _)| byte)
        .unwrap_or(row.len())
}

/// Convert a UTF-8 byte offset into a character-column cursor.
///
/// The offset is clamped to the preceding UTF-8 character boundary, so callers that receive an
/// arbitrary byte offset do not panic while converting it.
///
/// ```
/// use tui_lipan::text_motion::byte_to_char_col;
///
/// assert_eq!(byte_to_char_col("hél", 3), 2);
/// ```
pub fn byte_to_char_col(row: &str, byte: usize) -> usize {
    let mut byte = byte.min(row.len());
    while byte > 0 && !row.is_char_boundary(byte) {
        byte -= 1;
    }
    row[..byte].chars().count()
}

/// Apply a byte-offset motion to a character-column cursor.
fn cell_motion(row: &str, col: usize, motion: fn(&str, usize) -> usize) -> usize {
    byte_to_char_col(row, motion(row, char_col_to_byte(row, col)))
}

/// Apply a word-end byte motion to a character-column cursor.
///
/// Word-end motions use an insertion point just after the current character, then map the result
/// back to the character before it. Without this `e`/`E` would re-find the current word's end
/// when the cursor is already on its last character.
fn cell_motion_end(row: &str, col: usize, motion: fn(&str, usize) -> usize) -> usize {
    let after_col_byte = row
        .char_indices()
        .nth(col)
        .map(|(byte, ch)| byte + ch.len_utf8())
        .unwrap_or(row.len());
    byte_to_char_col(row, motion(row, after_col_byte)).saturating_sub(1)
}

/// Move a character-column cursor to the next vim word start (`w`).
///
/// This operates on character columns, not display columns; it is intended for `&str` rows.
///
/// ```
/// use tui_lipan::text_motion::cell_word_forward_start;
/// assert_eq!(cell_word_forward_start("héllo world", 0), 6);
/// ```
pub fn cell_word_forward_start(row: &str, col: usize) -> usize {
    cell_motion(row, col, word_forward_start)
}

/// Move a character-column cursor to the previous vim word start (`b`).
///
/// This operates on character columns, not display columns; it is intended for `&str` rows.
///
/// ```
/// use tui_lipan::text_motion::cell_word_backward_start;
/// assert_eq!(cell_word_backward_start("héllo world", 11), 6);
/// ```
pub fn cell_word_backward_start(row: &str, col: usize) -> usize {
    cell_motion(row, col, word_backward_start)
}

/// Move a character-column cursor to the current or next vim word end (`e`).
///
/// This operates on character columns, not display columns. The insertion-point conversion keeps
/// repeated `e` presses moving forward when the cursor is already on a word's last character.
///
/// ```
/// use tui_lipan::text_motion::cell_word_end;
/// assert_eq!(cell_word_end("héllo world", 0), 4);
/// ```
pub fn cell_word_end(row: &str, col: usize) -> usize {
    cell_motion_end(row, col, word_end)
}

/// Move a character-column cursor to the next vim WORD start (`W`).
///
/// This operates on character columns, not display columns; it is intended for `&str` rows.
///
/// ```
/// use tui_lipan::text_motion::cell_big_word_forward_start;
/// assert_eq!(cell_big_word_forward_start("foo.bar baz", 0), 8);
/// ```
pub fn cell_big_word_forward_start(row: &str, col: usize) -> usize {
    cell_motion(row, col, big_word_forward_start)
}

/// Move a character-column cursor to the previous vim WORD start (`B`).
///
/// This operates on character columns, not display columns; it is intended for `&str` rows.
///
/// ```
/// use tui_lipan::text_motion::cell_big_word_backward_start;
/// assert_eq!(cell_big_word_backward_start("foo.bar baz", 11), 8);
/// ```
pub fn cell_big_word_backward_start(row: &str, col: usize) -> usize {
    cell_motion(row, col, big_word_backward_start)
}

/// Move a character-column cursor to the current or next vim WORD end (`E`).
///
/// This operates on character columns, not display columns. The insertion-point conversion keeps
/// repeated `E` presses moving forward when the cursor is already on a WORD's last character.
///
/// ```
/// use tui_lipan::text_motion::cell_big_word_end;
/// assert_eq!(cell_big_word_end("foo.bar baz", 0), 6);
/// ```
pub fn cell_big_word_end(row: &str, col: usize) -> usize {
    cell_motion_end(row, col, big_word_end)
}

/// Move a character-column cursor to the first non-blank character (`^`).
///
/// This operates on character columns, not display columns; it is intended for `&str` rows.
/// An all-blank row returns its character length.
///
/// ```
/// use tui_lipan::text_motion::cell_line_first_nonblank;
/// assert_eq!(cell_line_first_nonblank("  hé"), 2);
/// ```
pub fn cell_line_first_nonblank(row: &str) -> usize {
    byte_to_char_col(row, first_nonblank_in_line(row, 0, row.len()))
}

/// Move a character-column cursor to the last character on the row (`$`).
///
/// This operates on character columns, not display columns; it returns `0` for an empty row.
///
/// ```
/// use tui_lipan::text_motion::cell_line_last;
/// assert_eq!(cell_line_last("hé"), 1);
/// ```
pub fn cell_line_last(row: &str) -> usize {
    row.chars().count().saturating_sub(1)
}

#[cfg(test)]
mod cell_cursor_tests {
    use super::*;

    #[test]
    fn char_column_byte_conversion_round_trips_multibyte_text() {
        let row = "héllo wörld";
        for col in 0..=row.chars().count() {
            assert_eq!(byte_to_char_col(row, char_col_to_byte(row, col)), col);
        }
    }

    #[test]
    fn byte_to_char_col_clamps_inside_a_character() {
        assert_eq!(byte_to_char_col("é", 1), 0);
        assert_eq!(byte_to_char_col("é", 2), 1);
    }

    #[test]
    fn word_motions_use_character_columns() {
        let row = "one two  three";
        assert_eq!(cell_word_forward_start(row, 0), 4);
        assert_eq!(cell_word_forward_start(row, 4), 9);
        assert_eq!(cell_word_backward_start(row, 9), 4);
        assert_eq!(cell_word_end(row, 0), 2);
        assert_eq!(cell_word_end(row, 2), 6);
    }

    #[test]
    fn word_end_advances_when_cursor_is_on_a_word_end() {
        let row = "one two  three";
        assert_eq!(cell_word_end(row, 2), 6);
        assert_eq!(cell_word_end(row, 6), 13);
    }

    #[test]
    fn big_word_motions_use_character_columns() {
        let row = "foo.bar  baz";
        assert_eq!(cell_big_word_forward_start(row, 0), 9);
        assert_eq!(cell_big_word_end(row, 0), 6);
        assert_eq!(cell_big_word_backward_start(row, 9), 0);
    }

    #[test]
    fn line_motions_use_character_columns() {
        assert_eq!(cell_line_first_nonblank("  hé"), 2);
        assert_eq!(cell_line_first_nonblank("   "), 3);
        assert_eq!(cell_line_last("hé"), 1);
        assert_eq!(cell_line_last(""), 0);
    }
}
