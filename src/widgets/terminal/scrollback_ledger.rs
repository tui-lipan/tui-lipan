//! Exact accounting of scrollback lines evicted while parsing terminal output.
//!
//! Eviction is not something the grid can be asked about after the fact: once
//! `history_size()` reaches the scrollback limit it stays pinned while content
//! keeps shifting underneath, so `topmost_line()`, `history_size()` and
//! `display_offset()` are all identical before and after a line falls off the
//! top. Anything anchored to an absolute line index therefore drifts silently.
//!
//! Eviction *is* an event, and it happens synchronously inside our own
//! `process_bytes` call. [`LedgerTerm`] wraps the [`Term`] that the VTE parser
//! drives and observes it at that point:
//!
//! - the grid is built with `scrollback_len + rows` of capacity, and
//! - after every handler call, anything above `scrollback_len` is trimmed and
//!   counted.
//!
//! Only the methods `Term` actually implements are delegated; the rest are
//! trait defaults that no-op identically for `Term` and for this wrapper. The
//! `handler_delegation_matches_term` test guards that equivalence, since a
//! future vte release adding a method would otherwise be silently dropped here.
//!
//! A single handler call can push at most `rows` lines into history
//! (`Term::scroll_up_relative` clamps to the scroll region, which is at most
//! the screen), so history can never reach the capacity mid-call and no scroll
//! is ever hidden. Every delegation is the same shape, which is why none of
//! Alacritty's scrolling semantics are reimplemented here - the count comes
//! from the grid itself, not from a model of when it scrolls.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::Term;
use alacritty_terminal::vte::ansi::{
    Attr, CharsetIndex, ClearMode, CursorShape, CursorStyle, Handler, Hyperlink, KeyboardModes,
    KeyboardModesApplyBehavior, LineClearMode, Mode, PrivateMode, Rgb, StandardCharset,
    TabulationClearMode,
};

/// Grid capacity to build a [`Term`] with, given the scrollback we expose.
///
/// The `rows` of headroom are what keep the grid from saturating inside a single
/// handler call; see the module docs.
pub(super) fn ledger_capacity(scrollback_len: usize, rows: u16) -> usize {
    scrollback_len.saturating_add(usize::from(rows))
}

/// A [`Term`] that counts the scrollback lines evicted while it is driven.
pub(super) struct LedgerTerm<'a, T: EventListener> {
    inner: &'a mut Term<T>,
    /// Scrollback depth actually exposed to callers.
    limit: usize,
    /// Grid capacity, always `limit + rows`.
    capacity: usize,
    evicted: usize,
}

impl<'a, T: EventListener> LedgerTerm<'a, T> {
    pub(super) fn new(inner: &'a mut Term<T>, limit: usize, capacity: usize) -> Self {
        Self {
            inner,
            limit,
            capacity,
            evicted: 0,
        }
    }

    /// Lines that fell out of scrollback while this wrapper was driving the term.
    pub(super) fn evicted(&self) -> usize {
        self.evicted
    }

    /// Trim history back to the exposed limit, counting whatever was dropped.
    #[inline]
    fn settle(&mut self) {
        self.evicted += settle_history(self.inner, self.limit, self.capacity);
    }
}

/// Trim `term`'s history back to `limit`, returning how many lines were dropped.
///
/// Also used after `Term::resize`, which can push lines into history without going
/// through a `Handler` call and so is invisible to [`LedgerTerm`].
pub(super) fn settle_history<T: EventListener>(
    term: &mut Term<T>,
    limit: usize,
    capacity: usize,
) -> usize {
    let history = term.history_size();
    if history <= limit {
        return 0;
    }
    let grid = term.grid_mut();
    // Shrinking drops the oldest lines; raising the cap again only makes room.
    grid.update_history(limit);
    grid.update_history(capacity);
    history - limit
}

impl<T: EventListener> Handler for LedgerTerm<'_, T> {
    fn set_title(&mut self, arg0: Option<String>) {
        self.inner.set_title(arg0);
        self.settle();
    }
    fn set_cursor_style(&mut self, arg0: Option<CursorStyle>) {
        self.inner.set_cursor_style(arg0);
        self.settle();
    }
    fn set_cursor_shape(&mut self, shape: CursorShape) {
        self.inner.set_cursor_shape(shape);
        self.settle();
    }
    fn input(&mut self, c: char) {
        self.inner.input(c);
        self.settle();
    }
    fn goto(&mut self, line: i32, col: usize) {
        self.inner.goto(line, col);
        self.settle();
    }
    fn goto_line(&mut self, line: i32) {
        self.inner.goto_line(line);
        self.settle();
    }
    fn goto_col(&mut self, col: usize) {
        self.inner.goto_col(col);
        self.settle();
    }
    fn insert_blank(&mut self, arg0: usize) {
        self.inner.insert_blank(arg0);
        self.settle();
    }
    fn move_up(&mut self, arg0: usize) {
        self.inner.move_up(arg0);
        self.settle();
    }
    fn move_down(&mut self, arg0: usize) {
        self.inner.move_down(arg0);
        self.settle();
    }
    fn identify_terminal(&mut self, intermediate: Option<char>) {
        self.inner.identify_terminal(intermediate);
        self.settle();
    }
    fn device_status(&mut self, arg0: usize) {
        self.inner.device_status(arg0);
        self.settle();
    }
    fn move_forward(&mut self, col: usize) {
        self.inner.move_forward(col);
        self.settle();
    }
    fn move_backward(&mut self, col: usize) {
        self.inner.move_backward(col);
        self.settle();
    }
    fn move_down_and_cr(&mut self, row: usize) {
        self.inner.move_down_and_cr(row);
        self.settle();
    }
    fn move_up_and_cr(&mut self, row: usize) {
        self.inner.move_up_and_cr(row);
        self.settle();
    }
    fn put_tab(&mut self, count: u16) {
        self.inner.put_tab(count);
        self.settle();
    }
    fn backspace(&mut self) {
        self.inner.backspace();
        self.settle();
    }
    fn carriage_return(&mut self) {
        self.inner.carriage_return();
        self.settle();
    }
    fn linefeed(&mut self) {
        self.inner.linefeed();
        self.settle();
    }
    fn bell(&mut self) {
        self.inner.bell();
        self.settle();
    }
    fn substitute(&mut self) {
        self.inner.substitute();
        self.settle();
    }
    fn newline(&mut self) {
        self.inner.newline();
        self.settle();
    }
    fn set_horizontal_tabstop(&mut self) {
        self.inner.set_horizontal_tabstop();
        self.settle();
    }
    fn scroll_up(&mut self, arg0: usize) {
        self.inner.scroll_up(arg0);
        self.settle();
    }
    fn scroll_down(&mut self, arg0: usize) {
        self.inner.scroll_down(arg0);
        self.settle();
    }
    fn insert_blank_lines(&mut self, arg0: usize) {
        self.inner.insert_blank_lines(arg0);
        self.settle();
    }
    fn delete_lines(&mut self, arg0: usize) {
        self.inner.delete_lines(arg0);
        self.settle();
    }
    fn erase_chars(&mut self, arg0: usize) {
        self.inner.erase_chars(arg0);
        self.settle();
    }
    fn delete_chars(&mut self, arg0: usize) {
        self.inner.delete_chars(arg0);
        self.settle();
    }
    fn move_backward_tabs(&mut self, count: u16) {
        self.inner.move_backward_tabs(count);
        self.settle();
    }
    fn move_forward_tabs(&mut self, count: u16) {
        self.inner.move_forward_tabs(count);
        self.settle();
    }
    fn save_cursor_position(&mut self) {
        self.inner.save_cursor_position();
        self.settle();
    }
    fn restore_cursor_position(&mut self) {
        self.inner.restore_cursor_position();
        self.settle();
    }
    fn clear_line(&mut self, mode: LineClearMode) {
        self.inner.clear_line(mode);
        self.settle();
    }
    fn clear_screen(&mut self, mode: ClearMode) {
        self.inner.clear_screen(mode);
        self.settle();
    }
    fn clear_tabs(&mut self, mode: TabulationClearMode) {
        self.inner.clear_tabs(mode);
        self.settle();
    }
    fn reset_state(&mut self) {
        self.inner.reset_state();
        self.settle();
    }
    fn reverse_index(&mut self) {
        self.inner.reverse_index();
        self.settle();
    }
    fn terminal_attribute(&mut self, attr: Attr) {
        self.inner.terminal_attribute(attr);
        self.settle();
    }
    fn set_mode(&mut self, mode: Mode) {
        self.inner.set_mode(mode);
        self.settle();
    }
    fn unset_mode(&mut self, mode: Mode) {
        self.inner.unset_mode(mode);
        self.settle();
    }
    fn report_mode(&mut self, mode: Mode) {
        self.inner.report_mode(mode);
        self.settle();
    }
    fn set_private_mode(&mut self, mode: PrivateMode) {
        self.inner.set_private_mode(mode);
        self.settle();
    }
    fn unset_private_mode(&mut self, mode: PrivateMode) {
        self.inner.unset_private_mode(mode);
        self.settle();
    }
    fn report_private_mode(&mut self, mode: PrivateMode) {
        self.inner.report_private_mode(mode);
        self.settle();
    }
    fn set_scrolling_region(&mut self, top: usize, bottom: Option<usize>) {
        self.inner.set_scrolling_region(top, bottom);
        self.settle();
    }
    fn set_keypad_application_mode(&mut self) {
        self.inner.set_keypad_application_mode();
        self.settle();
    }
    fn unset_keypad_application_mode(&mut self) {
        self.inner.unset_keypad_application_mode();
        self.settle();
    }
    fn set_active_charset(&mut self, arg0: CharsetIndex) {
        self.inner.set_active_charset(arg0);
        self.settle();
    }
    fn configure_charset(&mut self, arg0: CharsetIndex, arg1: StandardCharset) {
        self.inner.configure_charset(arg0, arg1);
        self.settle();
    }
    fn set_color(&mut self, arg0: usize, arg1: Rgb) {
        self.inner.set_color(arg0, arg1);
        self.settle();
    }
    fn dynamic_color_sequence(&mut self, arg0: String, arg1: usize, arg2: &str) {
        self.inner.dynamic_color_sequence(arg0, arg1, arg2);
        self.settle();
    }
    fn reset_color(&mut self, arg0: usize) {
        self.inner.reset_color(arg0);
        self.settle();
    }
    fn clipboard_store(&mut self, arg0: u8, arg1: &[u8]) {
        self.inner.clipboard_store(arg0, arg1);
        self.settle();
    }
    fn clipboard_load(&mut self, arg0: u8, arg1: &str) {
        self.inner.clipboard_load(arg0, arg1);
        self.settle();
    }
    fn decaln(&mut self) {
        self.inner.decaln();
        self.settle();
    }
    fn push_title(&mut self) {
        self.inner.push_title();
        self.settle();
    }
    fn pop_title(&mut self) {
        self.inner.pop_title();
        self.settle();
    }
    fn text_area_size_pixels(&mut self) {
        self.inner.text_area_size_pixels();
        self.settle();
    }
    fn text_area_size_chars(&mut self) {
        self.inner.text_area_size_chars();
        self.settle();
    }
    fn set_hyperlink(&mut self, arg0: Option<Hyperlink>) {
        self.inner.set_hyperlink(arg0);
        self.settle();
    }
    fn report_keyboard_mode(&mut self) {
        self.inner.report_keyboard_mode();
        self.settle();
    }
    fn push_keyboard_mode(&mut self, mode: KeyboardModes) {
        self.inner.push_keyboard_mode(mode);
        self.settle();
    }
    fn pop_keyboard_modes(&mut self, to_pop: u16) {
        self.inner.pop_keyboard_modes(to_pop);
        self.settle();
    }
    fn set_keyboard_mode(&mut self, mode: KeyboardModes, behavior: KeyboardModesApplyBehavior) {
        self.inner.set_keyboard_mode(mode, behavior);
        self.settle();
    }
}

#[cfg(test)]
mod tests {
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::index::{Column, Line};
    use alacritty_terminal::term::Config as TermConfig;
    use alacritty_terminal::term::cell::Cell;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi::{Processor, StdSyncHandler};

    use super::*;

    /// Visible screen as plain text, one string per row.
    fn render_plain<T: EventListener>(term: &Term<T>) -> Vec<String> {
        let grid = term.grid();
        (0..grid.screen_lines())
            .map(|row| {
                (0..grid.columns())
                    .map(|col| {
                        let cell: &Cell = &grid[Line(row as i32)][Column(col)];
                        cell.c
                    })
                    .collect()
            })
            .collect()
    }

    /// Driving `Term` through `LedgerTerm` must produce the same screen as driving it
    /// directly.
    ///
    /// This is what keeps the hand-written delegation honest: a vte release that adds a
    /// `Handler` method `Term` implements would otherwise be silently dropped here,
    /// since every trait method has a default no-op body and omitting one is not a
    /// compile error.
    #[test]
    fn handler_delegation_matches_term() {
        // Exercise cursor motion, scroll regions, wrapping, attributes, charsets,
        // erase/insert/delete, alt screen, tabs and OSC - i.e. the delegated surface.
        let corpus: &[&[u8]] = &[
            b"plain text\r\n",
            b"\x1b[1;32mbold green\x1b[0m\r\n",
            b"wrap-me-across-the-right-edge-of-the-screen-and-keep-going\r\n",
            b"\x1b[2;5r\x1b[3;1Hscroll region\r\n\r\n\r\n",
            b"\x1b[Hhome\x1b[10Cright\x1b[2Dback\r\n",
            b"\x1b[2J\x1b[Hcleared\r\n",
            b"\x1b[4hinsert\x1b[4l\r\n",
            b"tab\there\tand\there\r\n",
            b"\x1b[3L\x1b[2M\x1b[5X\x1b[2P\r\n",
            b"\x1b7saved\x1b8restored\r\n",
            b"\x1b[?1049halt screen\r\n\x1b[?1049l",
            b"\x1b]0;title\x07\x1b]133;A\x1b\\",
            b"\x1b#8\r\n",
            b"\x1bMreverse index\r\n",
            b"\x1b[5S\x1b[3T\r\n",
            b"\x1b(0line drawing\x1b(B\r\n",
        ];

        let size = TermSize::new(10, 6);
        let config = TermConfig::default();
        let mut direct = Term::new(config.clone(), &size, VoidListener);
        let mut wrapped = Term::new(config, &size, VoidListener);
        let mut direct_parser = Processor::<StdSyncHandler>::new();
        let mut wrapped_parser = Processor::<StdSyncHandler>::new();

        for chunk in corpus {
            direct_parser.advance(&mut direct, chunk);
            // A capacity equal to the limit disables trimming, so any difference is
            // delegation, not the ledger's own eviction.
            let mut ledger = LedgerTerm::new(&mut wrapped, 10_000, 10_000);
            wrapped_parser.advance(&mut ledger, chunk);
            assert_eq!(ledger.evicted(), 0);

            assert_eq!(
                render_plain(&wrapped),
                render_plain(&direct),
                "screens diverged after {:?} - a Handler method is not delegated",
                String::from_utf8_lossy(chunk)
            );
            assert_eq!(
                wrapped.grid().cursor.point,
                direct.grid().cursor.point,
                "cursor diverged after {:?}",
                String::from_utf8_lossy(chunk)
            );
            assert_eq!(*wrapped.mode(), *direct.mode());
        }
    }

    #[test]
    fn counts_every_evicted_line_once_saturated() {
        let size = TermSize::new(10, 2);
        let mut term = Term::new(
            TermConfig {
                scrolling_history: ledger_capacity(3, 2),
                ..TermConfig::default()
            },
            &size,
            VoidListener,
        );
        let mut parser = Processor::<StdSyncHandler>::new();
        let mut total = 0usize;
        for i in 0..20 {
            let mut ledger = LedgerTerm::new(&mut term, 3, ledger_capacity(3, 2));
            parser.advance(&mut ledger, format!("l{i}\r\n").as_bytes());
            total += ledger.evicted();
        }
        // On a 2-row screen the first newline only moves the cursor down, so 20
        // newlines scroll 19 lines into history; 3 are retained.
        assert_eq!(total, 19 - 3);
        assert_eq!(term.history_size(), 3);
    }

    /// A single escape sequence can scroll a whole screen at once; the ledger must
    /// still account for it exactly.
    #[test]
    fn counts_bulk_scroll_from_one_sequence() {
        let size = TermSize::new(10, 4);
        let capacity = ledger_capacity(2, 4);
        let mut term = Term::new(
            TermConfig {
                scrolling_history: capacity,
                ..TermConfig::default()
            },
            &size,
            VoidListener,
        );
        let mut parser = Processor::<StdSyncHandler>::new();
        let mut ledger = LedgerTerm::new(&mut term, 2, capacity);
        parser.advance(&mut ledger, b"a\r\nb\r\nc\r\nd");
        parser.advance(&mut ledger, b"\x1b[999S");
        let evicted = ledger.evicted();
        assert_eq!(term.history_size(), 2);
        // 4 screen lines pushed into a 2-line scrollback => 2 evicted.
        assert_eq!(evicted, 2);
    }
}
