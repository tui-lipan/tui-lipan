//! Pins the public `tui_lipan::text_motion` re-export path: every function must stay reachable
//! both from the crate root path and from the prelude, and behave per the documented "insertion
//! point" cursor convention. This is the contract external apps (e.g. a terminal emulator's copy
//! mode) build on, so accidental renames/signature drift here should fail CI.

use tui_lipan::text_motion::{
    big_word_backward_start, big_word_end, big_word_forward_start, first_nonblank_in_line,
    line_end_at, line_start_at, word_backward_start, word_end, word_forward_start,
};

#[test]
fn reachable_from_prelude_too() {
    use tui_lipan::prelude::*;

    assert_eq!(word_forward_start("cat dog", 0), 4);
}

#[test]
fn word_motions_over_ascii() {
    let text = "cat dog";
    assert_eq!(word_forward_start(text, 0), 4);
    assert_eq!(word_backward_start(text, 7), 4);
    assert_eq!(word_end(text, 0), 3);
}

#[test]
fn word_end_insertion_point_convention() {
    let text = "cat dog";

    // The insertion point right after "cat" (byte 3, between 't' and the space) correctly
    // advances to the end of "dog".
    assert_eq!(word_end(text, 3), 7);

    // Feeding the *cell* start byte of the word's own last character (byte 2, the 't') instead
    // re-finds the same word's end rather than advancing — this is the exact pitfall a
    // cell-based caller (e.g. a terminal grid cursor) must avoid by converting to an insertion
    // point before calling `word_end`/`big_word_end`.
    assert_eq!(word_end(text, 2), 3);
}

#[test]
fn big_word_motions_treat_punctuation_as_part_of_the_run() {
    let text = "foo.bar baz";
    assert_eq!(big_word_forward_start(text, 0), 8);
    assert_eq!(big_word_backward_start(text, 11), 8);
    assert_eq!(big_word_end(text, 0), 7);
}

#[test]
fn line_motions() {
    let text = "foo\n  bar";
    let cursor = text.len();

    let start = line_start_at(text, cursor);
    let end = line_end_at(text, cursor);
    assert_eq!(start, 4);
    assert_eq!(end, text.len());
    assert_eq!(first_nonblank_in_line(text, start, end), 6);
}

#[test]
fn line_motions_on_blank_line_land_on_line_end() {
    let text = "foo\n   \nbar";
    let blank_start = 4;
    let blank_end = 7;
    assert_eq!(
        first_nonblank_in_line(text, blank_start, blank_end),
        blank_end
    );
}
