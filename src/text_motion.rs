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
