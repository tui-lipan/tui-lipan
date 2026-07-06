//! Public, byte-offset text motion helpers shared with the vim-mode `TextArea` widget.
//!
//! Every function takes a `&str` and a byte offset and returns a new byte offset; offsets are
//! always clamped to a UTF-8 char boundary. These are the exact algorithms `TextArea`'s vim mode
//! uses internally for `w`/`b`/`e`/`0`/`^`/`$`, promoted to a stable public surface so apps that
//! render their own text grids (for example a terminal emulator's scrollback copy mode) can reuse
//! vim-style word/line motions instead of reimplementing them.

/// Move forward to the start of the next vim "word" (`w`): skip the remainder of the current
/// word/punctuation run, then any following whitespace.
pub use crate::app::input::text_area_vim::vim_word_forward_start as word_forward_start;

/// Move backward to the start of the previous vim "word" (`b`).
pub use crate::app::input::text_area_vim::vim_word_backward_start as word_backward_start;

/// Move to the end of the current or next vim "word" (`e`).
pub use crate::app::input::text_area_vim::vim_word_end as word_end;

/// Move forward to the start of the next vim WORD (`W`): a whitespace-delimited run that
/// includes punctuation, unlike a "word".
pub use crate::app::input::text_area_vim::vim_big_word_forward_start as big_word_forward_start;

/// Move backward to the start of the previous vim WORD (`B`).
pub use crate::app::input::text_area_vim::vim_big_word_backward_start as big_word_backward_start;

/// Move to the end of the current or next vim WORD (`E`).
pub use crate::app::input::text_area_vim::vim_big_word_end as big_word_end;

/// Byte offset of the start of the line containing `cursor` (`0` motion target).
pub use crate::app::input::text_area_vim::line_start_at;

/// Byte offset one past the end of the line containing `cursor` (`$` motion target, exclusive).
pub use crate::app::input::text_area_vim::line_end_at;

/// Byte offset of the first non-blank character in `text[line_start..line_end]` (`^` motion
/// target), or `line_end` if the line is entirely blank.
pub use crate::app::input::text_area_vim::first_nonblank_in_line;
