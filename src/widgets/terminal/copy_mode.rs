//! Keyboard copy-mode state for terminal grids.
//!
//! This module owns navigation state only. It does not own a terminal screen, clipboard,
//! scrollback application, runtime updates, or copy feedback. Applications compose it with a
//! [`crate::widgets::TerminalScreen`] and [`crate::widgets::TerminalRenderSnapshot`].

use super::{TerminalPos, TerminalSelection, TerminalSelectionEvent};
use crate::core::event::{KeyCode, KeyEvent, KeyMods};
use crate::text_motion::{
    byte_to_char_col, cell_big_word_backward_start, cell_big_word_end, cell_big_word_forward_start,
    cell_line_first_nonblank, cell_line_last, cell_word_backward_start, cell_word_end,
    cell_word_forward_start, char_col_to_byte,
};
use crate::utils::spans::{byte_at_display_column, display_column};

/// The terminal grid data needed to handle one copy-mode key.
///
/// Cursor and selection coordinates (`cols`, `cursor`, and `anchor`) are **display columns**.
/// `cursor_row_text` is the text row under the cursor; word/line motions bridge its character
/// columns to display columns internally. `prompt_lines` contains absolute retained-line indices
/// for semantic prompt jumps, in ascending order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyModeGrid<'a> {
    /// Number of visible rows.
    pub rows: usize,
    /// Number of visible display columns.
    pub cols: usize,
    /// Maximum scrollback offset (the number of retained history rows).
    pub total_scrollback_rows: usize,
    /// Text of the row under the cursor, used as the source for character-column motion adapters.
    pub cursor_row_text: &'a str,
    /// Absolute retained-line positions of semantic prompts, oldest first.
    pub prompt_lines: &'a [usize],
}

/// The viewport a terminal widget reported a pointer selection in.
///
/// `scrollback_offset` is the widget's offset when it reported the event, which can differ from
/// the copy-mode offset after the wheel or a drag past an edge scrolled the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CopyModeViewport {
    /// Number of visible rows.
    pub rows: usize,
    /// Number of visible display columns.
    pub cols: usize,
    /// Maximum scrollback offset (the number of retained history rows).
    pub total_scrollback_rows: usize,
    /// The widget's scrollback offset.
    pub scrollback_offset: usize,
}

/// Result of handling a copy-mode key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyModeAction {
    /// The key is not a copy-mode key; the application may route it elsewhere.
    Ignored,
    /// The cursor or scrollback offset moved without changing an active selection.
    Moved,
    /// The selection anchor or an anchored cursor position changed.
    SelectionChanged,
    /// The application should copy the current selection.
    RequestCopy,
    /// The application should leave copy mode without copying.
    Cancel,
}

/// Stateful keyboard navigation for terminal copy mode.
///
/// Cursor coordinates are viewport-relative while the anchor is an absolute retained-line position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalCopyMode {
    cursor_row: usize,
    cursor_col: usize,
    anchor: Option<TerminalPos>,
    scrollback_offset: usize,
}

impl TerminalCopyMode {
    /// Create copy mode at a viewport cursor and scrollback offset.
    ///
    /// `cursor_col` is a display-column coordinate.
    pub fn new(cursor_row: usize, cursor_col: usize, scrollback_offset: usize) -> Self {
        Self {
            cursor_row,
            cursor_col,
            anchor: None,
            scrollback_offset,
        }
    }

    /// Handle a copy-mode key against the current terminal grid.
    ///
    /// Navigation keys use vim's character-column motions. `[` and `]` jump to the previous and
    /// next absolute prompt line in `grid.prompt_lines`; all other unlisted keys return
    /// [`CopyModeAction::Ignored`].
    pub fn handle_key(&mut self, key: KeyEvent, grid: CopyModeGrid<'_>) -> CopyModeAction {
        let rows = grid.rows.max(1);
        let cols = grid.cols.max(1);
        self.cursor_row = self.cursor_row.min(rows - 1);
        self.cursor_col = self.cursor_col.min(cols - 1);
        self.scrollback_offset = self.scrollback_offset.min(grid.total_scrollback_rows);
        if let Some(anchor) = self.anchor.as_mut() {
            anchor.line = anchor
                .line
                .min(grid.total_scrollback_rows.saturating_add(rows - 1));
            anchor.col = anchor.col.min(cols - 1);
        }

        let shifted_upper =
            key.mods == KeyMods::SHIFT && matches!(key.code, KeyCode::Char('G' | 'W' | 'B' | 'E'));
        let is_ctrl_page =
            key.mods == KeyMods::CTRL && matches!(key.code, KeyCode::Char('u' | 'd'));
        if !key.mods.is_empty() && !shifted_upper && !is_ctrl_page {
            return CopyModeAction::Ignored;
        }

        if key.mods.is_empty() && (key.is(KeyCode::Esc) || key.is(KeyCode::Char('q'))) {
            return CopyModeAction::Cancel;
        }
        if key.mods.is_empty() && (key.is(KeyCode::Char('y')) || key.is(KeyCode::Enter)) {
            return CopyModeAction::RequestCopy;
        }
        if key.mods.is_empty() && (key.is(KeyCode::Char('v')) || key.is(KeyCode::Char(' '))) {
            self.toggle_anchor(grid.total_scrollback_rows);
            return CopyModeAction::SelectionChanged;
        }

        if key.mods.is_empty() && key.is(KeyCode::Char('[')) {
            return self.jump_prompt(false, grid);
        }
        if key.mods.is_empty() && key.is(KeyCode::Char(']')) {
            return self.jump_prompt(true, grid);
        }

        let before = (self.cursor_row, self.cursor_col, self.scrollback_offset);
        let half_page = (rows / 2).max(1);
        let handled = match key.code {
            KeyCode::Char('h') | KeyCode::Left => {
                self.cursor_col = self.cursor_col.saturating_sub(1);
                true
            }
            KeyCode::Char('l') | KeyCode::Right => {
                self.cursor_col = (self.cursor_col + 1).min(cols - 1);
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                move_up(self, 1, grid.total_scrollback_rows);
                true
            }
            KeyCode::Char('j') | KeyCode::Down => {
                move_down(self, 1, rows);
                true
            }
            KeyCode::Char('u') if key.mods == KeyMods::CTRL => {
                move_up(self, half_page, grid.total_scrollback_rows);
                true
            }
            KeyCode::Char('d') if key.mods == KeyMods::CTRL => {
                move_down(self, half_page, rows);
                true
            }
            KeyCode::Char('g') => {
                self.scrollback_offset = grid.total_scrollback_rows;
                self.cursor_row = 0;
                true
            }
            KeyCode::Char('G') => {
                self.scrollback_offset = 0;
                self.cursor_row = rows - 1;
                true
            }
            KeyCode::Char('w') => {
                self.cursor_col = display_motion(
                    grid.cursor_row_text,
                    self.cursor_col,
                    cell_word_forward_start,
                );
                true
            }
            KeyCode::Char('b') => {
                self.cursor_col = display_motion(
                    grid.cursor_row_text,
                    self.cursor_col,
                    cell_word_backward_start,
                );
                true
            }
            KeyCode::Char('e') => {
                self.cursor_col =
                    display_motion(grid.cursor_row_text, self.cursor_col, cell_word_end);
                true
            }
            KeyCode::Char('W') => {
                self.cursor_col = display_motion(
                    grid.cursor_row_text,
                    self.cursor_col,
                    cell_big_word_forward_start,
                );
                true
            }
            KeyCode::Char('B') => {
                self.cursor_col = display_motion(
                    grid.cursor_row_text,
                    self.cursor_col,
                    cell_big_word_backward_start,
                );
                true
            }
            KeyCode::Char('E') => {
                self.cursor_col =
                    display_motion(grid.cursor_row_text, self.cursor_col, cell_big_word_end);
                true
            }
            KeyCode::Char('0') => {
                self.cursor_col = 0;
                true
            }
            KeyCode::Char('^') => {
                self.cursor_col =
                    display_motion(grid.cursor_row_text, self.cursor_col, |row, _| {
                        cell_line_first_nonblank(row)
                    });
                true
            }
            KeyCode::Char('$') => {
                self.cursor_col =
                    display_motion(grid.cursor_row_text, self.cursor_col, |row, _| {
                        cell_line_last(row)
                    });
                true
            }
            _ => false,
        };

        if !handled {
            return CopyModeAction::Ignored;
        }
        if before == (self.cursor_row, self.cursor_col, self.scrollback_offset) {
            return CopyModeAction::Ignored;
        }

        if self.anchor.is_some() {
            CopyModeAction::SelectionChanged
        } else {
            CopyModeAction::Moved
        }
    }

    /// Return the viewport cursor position. The returned column is a display-column coordinate.
    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    /// Return the current selection endpoints, if an anchor is active.
    ///
    /// Both columns in the returned point are display-column coordinates.
    pub fn anchor(&self) -> Option<TerminalPos> {
        self.anchor
    }

    /// Return the current scrollback offset.
    pub fn scrollback_offset(&self) -> usize {
        self.scrollback_offset
    }

    /// Return the selection from the anchor to the cursor.
    pub fn selection(&self, total_scrollback_rows: usize) -> Option<TerminalSelection> {
        let anchor = self.anchor?;
        Some(TerminalSelection {
            anchor,
            cursor: TerminalPos {
                line: current_absolute_line(self, total_scrollback_rows),
                col: self.cursor_col,
            },
        })
    }

    /// Adopt a mouse press or drag reported by the terminal widget's `on_selection`.
    ///
    /// A drag becomes an anchored selection over the same cells the widget's own mouse selection
    /// covers: the widget's end is exclusive and copy mode's is inclusive, so the copy cursor lands
    /// on the last selected cell at the pointer's side. A press, or a drag back to where it began,
    /// drops the anchor and moves the cursor to the pointer. Events without a pointer, such as a
    /// key clearing the widget's selection, are ignored, because copy mode owns its selection.
    ///
    /// The cursor is kept on screen at `viewport.scrollback_offset`, scrolling only when the
    /// selection ends outside it. Returns [`CopyModeAction::Ignored`] when nothing changed.
    pub fn handle_pointer(
        &mut self,
        event: &TerminalSelectionEvent,
        viewport: CopyModeViewport,
    ) -> CopyModeAction {
        let Some(pointer) = event.pointer else {
            return CopyModeAction::Ignored;
        };
        let cols = viewport.cols.max(1);
        let clamp = |pos: TerminalPos| TerminalPos {
            line: pos.line,
            col: pos.col.min(cols - 1),
        };
        let (anchor, cursor) = match event.selection.filter(|sel| !sel.is_empty()) {
            Some(selection) => {
                let (start, end) = selection.normalized();
                // The exclusive end's cell is not selected, so step back one cell; an end at a
                // row's first column ends the selection at the end of the row above.
                let last = if end.col == 0 {
                    TerminalPos {
                        line: end.line.saturating_sub(1),
                        col: cols - 1,
                    }
                } else {
                    TerminalPos {
                        line: end.line,
                        col: end.col - 1,
                    }
                };
                let (start, last) = (clamp(start), clamp(last));
                if selection.cursor == start {
                    (Some(last), start)
                } else {
                    (Some(start), last)
                }
            }
            None => (None, clamp(pointer)),
        };

        let before = self.clone();
        self.anchor = anchor;
        self.place_cursor(cursor, viewport);
        if before == *self {
            CopyModeAction::Ignored
        } else if self.anchor.is_some() || before.anchor.is_some() {
            CopyModeAction::SelectionChanged
        } else {
            CopyModeAction::Moved
        }
    }

    /// Put the cursor on an absolute cell, scrolling `viewport` the least needed to show it.
    fn place_cursor(&mut self, pos: TerminalPos, viewport: CopyModeViewport) {
        let rows = viewport.rows.max(1);
        let total = viewport.total_scrollback_rows;
        let line = pos.line.min(total.saturating_add(rows - 1));
        let mut offset = viewport.scrollback_offset.min(total);
        let top = total - offset;
        if line < top {
            offset = total - line;
        } else if line >= top + rows {
            offset = (total + rows - 1).saturating_sub(line).min(total);
        }
        self.scrollback_offset = offset;
        self.cursor_row = line - (total - offset);
        self.cursor_col = pos.col;
    }

    /// Move the copy cursor and scrollback offset to an application-selected location.
    ///
    /// `col` is a display-column coordinate.
    pub fn goto(&mut self, row: usize, col: usize, scrollback_offset: usize) {
        self.cursor_row = row;
        self.cursor_col = col;
        self.scrollback_offset = scrollback_offset;
    }

    /// Toggle the selection anchor at the current cursor position.
    fn toggle_anchor(&mut self, total_scrollback_rows: usize) {
        self.anchor = match self.anchor {
            Some(_) => None,
            None => Some(TerminalPos {
                line: current_absolute_line(self, total_scrollback_rows),
                col: self.cursor_col,
            }),
        };
    }

    fn jump_prompt(&mut self, forward: bool, grid: CopyModeGrid<'_>) -> CopyModeAction {
        let Some(&line) = prompt_target(
            grid.prompt_lines,
            current_absolute_line(self, grid.total_scrollback_rows),
            forward,
        ) else {
            return CopyModeAction::Ignored;
        };

        if line < grid.total_scrollback_rows {
            self.scrollback_offset = grid.total_scrollback_rows - line;
            self.cursor_row = 0;
        } else {
            self.scrollback_offset = 0;
            self.cursor_row = (line - grid.total_scrollback_rows).min(grid.rows.max(1) - 1);
        }
        self.cursor_col = 0;
        if self.anchor.is_some() {
            CopyModeAction::SelectionChanged
        } else {
            CopyModeAction::Moved
        }
    }
}

/// Apply a character-column motion while keeping the copy cursor in display columns.
fn display_motion(row: &str, display_col: usize, motion: fn(&str, usize) -> usize) -> usize {
    let byte = byte_at_display_column(row, display_col);
    let char_col = byte_to_char_col(row, byte);
    let next_char_col = motion(row, char_col);
    display_column(row, char_col_to_byte(row, next_char_col))
}

fn current_absolute_line(mode: &TerminalCopyMode, total_scrollback_rows: usize) -> usize {
    total_scrollback_rows
        .saturating_sub(mode.scrollback_offset)
        .saturating_add(mode.cursor_row)
}

fn prompt_target(lines: &[usize], current: usize, forward: bool) -> Option<&usize> {
    if forward {
        lines.iter().find(|line| **line > current)
    } else {
        lines.iter().rfind(|line| **line < current)
    }
}

fn move_up(mode: &mut TerminalCopyMode, steps: usize, total_scrollback_rows: usize) {
    for _ in 0..steps {
        if mode.cursor_row > 0 {
            mode.cursor_row -= 1;
        } else if mode.scrollback_offset < total_scrollback_rows {
            mode.scrollback_offset += 1;
        } else {
            break;
        }
    }
}

fn move_down(mode: &mut TerminalCopyMode, steps: usize, rows: usize) {
    let bottom = rows.max(1) - 1;
    for _ in 0..steps {
        if mode.cursor_row < bottom {
            mode.cursor_row += 1;
        } else if mode.scrollback_offset > 0 {
            mode.scrollback_offset -= 1;
        } else {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GRID: CopyModeGrid<'static> = CopyModeGrid {
        rows: 4,
        cols: 20,
        total_scrollback_rows: 10,
        cursor_row_text: "one two  three",
        prompt_lines: &[],
    };

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: Default::default(),
        }
    }

    #[test]
    fn movement_scrolls_at_viewport_edges() {
        let mut mode = TerminalCopyMode::new(2, 0, 0);
        assert_eq!(
            mode.handle_key(key(KeyCode::Up), GRID),
            CopyModeAction::Moved
        );
        assert_eq!(mode.cursor(), (1, 0));
        assert_eq!(
            mode.handle_key(key(KeyCode::Up), GRID),
            CopyModeAction::Moved
        );
        assert_eq!(mode.cursor(), (0, 0));
        assert_eq!(
            mode.handle_key(key(KeyCode::Up), GRID),
            CopyModeAction::Moved
        );
        assert_eq!(mode.scrollback_offset(), 1);

        mode.goto(3, 0, 2);
        assert_eq!(
            mode.handle_key(key(KeyCode::Down), GRID),
            CopyModeAction::Moved
        );
        assert_eq!(mode.scrollback_offset(), 1);
        assert_eq!(mode.cursor(), (3, 0));
    }

    #[test]
    fn anchor_stays_absolute_while_scrolling_at_viewport_edges() {
        let mut mode = TerminalCopyMode::new(0, 2, 0);
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('v')), GRID),
            CopyModeAction::SelectionChanged
        );
        let anchor = mode.anchor().expect("anchor");
        assert_eq!(anchor, TerminalPos { line: 10, col: 2 });
        assert_eq!(
            mode.handle_key(key(KeyCode::Up), GRID),
            CopyModeAction::SelectionChanged
        );
        assert_eq!(mode.scrollback_offset(), 1);
        assert_eq!(mode.anchor(), Some(anchor));
        assert_eq!(
            mode.selection(GRID.total_scrollback_rows)
                .expect("selection")
                .cursor
                .line,
            9
        );
    }

    #[test]
    fn movement_at_a_boundary_is_ignored() {
        let mut mode = TerminalCopyMode::new(0, 0, GRID.total_scrollback_rows);
        assert_eq!(
            mode.handle_key(key(KeyCode::Left), GRID),
            CopyModeAction::Ignored
        );
        assert_eq!(
            mode.handle_key(key(KeyCode::Up), GRID),
            CopyModeAction::Ignored
        );

        mode.goto(GRID.rows - 1, GRID.cols - 1, 0);
        assert_eq!(
            mode.handle_key(key(KeyCode::Right), GRID),
            CopyModeAction::Ignored
        );
        assert_eq!(
            mode.handle_key(key(KeyCode::Down), GRID),
            CopyModeAction::Ignored
        );
    }

    #[test]
    fn ctrl_page_motions_and_g_commands_use_copy_mode_bounds() {
        let mut mode = TerminalCopyMode::new(2, 0, 0);
        let ctrl_u = KeyEvent {
            code: KeyCode::Char('u'),
            mods: crate::core::event::KeyMods::CTRL,
        };
        assert_eq!(mode.handle_key(ctrl_u, GRID), CopyModeAction::Moved);
        assert_eq!(mode.cursor(), (0, 0));

        assert_eq!(
            mode.handle_key(key(KeyCode::Char('g')), GRID),
            CopyModeAction::Moved
        );
        assert_eq!((mode.cursor(), mode.scrollback_offset()), ((0, 0), 10));
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('G')), GRID),
            CopyModeAction::Moved
        );
        assert_eq!((mode.cursor(), mode.scrollback_offset()), ((3, 0), 0));
    }

    #[test]
    fn vim_motions_and_anchor_report_selection_changes() {
        let mut mode = TerminalCopyMode::new(0, 0, 0);
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('w')), GRID),
            CopyModeAction::Moved
        );
        assert_eq!(mode.cursor().1, 4);
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('v')), GRID),
            CopyModeAction::SelectionChanged
        );
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('e')), GRID),
            CopyModeAction::SelectionChanged
        );
        assert_eq!(mode.anchor(), Some(TerminalPos { line: 10, col: 4 }));
        assert_eq!(
            mode.selection(GRID.total_scrollback_rows)
                .map(|s| (s.anchor.col, s.cursor.col)),
            Some((4, 6))
        );
        assert_eq!(
            mode.handle_key(key(KeyCode::Char(' ')), GRID),
            CopyModeAction::SelectionChanged
        );
        assert_eq!(mode.selection(GRID.total_scrollback_rows), None);
    }

    #[test]
    fn text_motions_keep_cursor_and_selection_in_display_columns() {
        let grid = CopyModeGrid {
            rows: 2,
            cols: 20,
            total_scrollback_rows: 0,
            cursor_row_text: "界 foo",
            prompt_lines: &[],
        };
        let mut mode = TerminalCopyMode::new(0, 0, 0);
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('w')), grid),
            CopyModeAction::Moved
        );
        // 界 occupies display columns 0..2, then the space is column 2; `w` lands on `f` at 3.
        assert_eq!(mode.cursor(), (0, 3));
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('e')), grid),
            CopyModeAction::Moved
        );
        assert_eq!(mode.cursor(), (0, 5));

        mode.goto(0, 1, 0);
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('v')), grid),
            CopyModeAction::SelectionChanged
        );
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('w')), grid),
            CopyModeAction::SelectionChanged
        );
        let selection = mode
            .selection(grid.total_scrollback_rows)
            .expect("anchor should create a selection");
        assert_eq!((selection.anchor.col, selection.cursor.col), (1, 3));
        mode.handle_key(key(KeyCode::Char(' ')), grid);

        let combining = CopyModeGrid {
            cursor_row_text: "  e\u{301} foo",
            ..grid
        };
        mode.goto(0, 0, 0);
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('^')), combining),
            CopyModeAction::Moved
        );
        assert_eq!(mode.cursor().1, 2);
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('$')), combining),
            CopyModeAction::Moved
        );
        assert_eq!(mode.cursor().1, 6);
    }

    #[test]
    fn ordinary_keys_require_no_modifiers() {
        let grid = GRID;
        let mut mode = TerminalCopyMode::new(1, 2, 0);
        for mods in [
            KeyMods::CTRL,
            KeyMods::SHIFT,
            KeyMods {
                alt: true,
                ..KeyMods::NONE
            },
        ] {
            let key = KeyEvent {
                code: KeyCode::Char('h'),
                mods,
            };
            assert_eq!(mode.handle_key(key, grid), CopyModeAction::Ignored);
            assert_eq!(mode.cursor(), (1, 2));
        }

        let ctrl_shift_u = KeyEvent {
            code: KeyCode::Char('u'),
            mods: KeyMods {
                ctrl: true,
                shift: true,
                ..KeyMods::NONE
            },
        };
        assert_eq!(mode.handle_key(ctrl_shift_u, grid), CopyModeAction::Ignored);

        let ctrl_u = KeyEvent {
            code: KeyCode::Char('u'),
            mods: KeyMods::CTRL,
        };
        assert_eq!(mode.handle_key(ctrl_u, grid), CopyModeAction::Moved);
        assert_eq!(mode.cursor(), (0, 2));

        let ctrl_d = KeyEvent {
            code: KeyCode::Char('d'),
            mods: KeyMods::CTRL,
        };
        assert_eq!(mode.handle_key(ctrl_d, grid), CopyModeAction::Moved);
        assert_eq!(mode.cursor(), (2, 2));

        let shifted_g = KeyEvent {
            code: KeyCode::Char('G'),
            mods: KeyMods::SHIFT,
        };
        assert_eq!(mode.handle_key(shifted_g, grid), CopyModeAction::Moved);
        assert_eq!(mode.cursor(), (3, 2));
    }

    #[test]
    fn copy_cancel_and_unknown_keys_are_distinct() {
        let mut mode = TerminalCopyMode::new(0, 0, 0);
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('x')), GRID),
            CopyModeAction::Ignored
        );
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('q')), GRID),
            CopyModeAction::Cancel
        );
        assert_eq!(
            mode.handle_key(key(KeyCode::Enter), GRID),
            CopyModeAction::RequestCopy
        );
    }

    #[test]
    fn prompt_jumps_use_absolute_line_math() {
        let grid = CopyModeGrid {
            prompt_lines: &[2, 7, 13],
            ..GRID
        };
        let mut mode = TerminalCopyMode::new(0, 4, 5);
        // history - offset + row = 5, so ] selects line 7 and parks at offset 3.
        assert_eq!(
            mode.handle_key(key(KeyCode::Char(']')), grid),
            CopyModeAction::Moved
        );
        assert_eq!((mode.cursor(), mode.scrollback_offset()), ((0, 0), 3));
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('[')), grid),
            CopyModeAction::Moved
        );
        assert_eq!((mode.cursor(), mode.scrollback_offset()), ((0, 0), 8));

        mode.goto(0, 0, 10);
        assert_eq!(
            mode.handle_key(key(KeyCode::Char('[')), grid),
            CopyModeAction::Ignored
        );
        mode.goto(3, 0, 0);
        assert_eq!(
            mode.handle_key(key(KeyCode::Char(']')), grid),
            CopyModeAction::Ignored
        );
    }

    fn viewport(scrollback_offset: usize) -> CopyModeViewport {
        CopyModeViewport {
            rows: GRID.rows,
            cols: GRID.cols,
            total_scrollback_rows: GRID.total_scrollback_rows,
            scrollback_offset,
        }
    }

    fn pointer_event(
        selection: Option<TerminalSelection>,
        pointer: TerminalPos,
    ) -> TerminalSelectionEvent {
        TerminalSelectionEvent {
            selection,
            text: None,
            pointer: Some(pointer),
        }
    }

    fn pos(line: usize, col: usize) -> TerminalPos {
        TerminalPos { line, col }
    }

    #[test]
    fn pointer_drags_select_the_same_cells_as_a_mouse_selection() {
        // Forward: the widget's end cell is exclusive, so the cursor stops one cell short.
        let mut mode = TerminalCopyMode::new(0, 0, 0);
        let forward = TerminalSelection {
            anchor: pos(10, 2),
            cursor: pos(11, 6),
        };
        assert_eq!(
            mode.handle_pointer(&pointer_event(Some(forward), pos(11, 6)), viewport(0)),
            CopyModeAction::SelectionChanged
        );
        assert_eq!(mode.anchor(), Some(pos(10, 2)));
        assert_eq!(mode.cursor(), (1, 5));

        // Backward: the pointer side is the start, so the anchor moves in by the excluded cell.
        let backward = TerminalSelection {
            anchor: pos(11, 6),
            cursor: pos(10, 2),
        };
        mode.handle_pointer(&pointer_event(Some(backward), pos(10, 2)), viewport(0));
        assert_eq!(mode.anchor(), Some(pos(11, 5)));
        assert_eq!(mode.cursor(), (0, 2));

        // An end at a row's first column ends the selection at the end of the row above.
        let to_row_start = TerminalSelection {
            anchor: pos(10, 2),
            cursor: pos(11, 0),
        };
        mode.handle_pointer(&pointer_event(Some(to_row_start), pos(11, 0)), viewport(0));
        assert_eq!(mode.cursor(), (0, GRID.cols - 1));

        // Below the last row the widget reports one column past the edge.
        let past_edge = TerminalSelection {
            anchor: pos(10, 2),
            cursor: pos(13, GRID.cols),
        };
        mode.handle_pointer(
            &pointer_event(Some(past_edge), pos(13, GRID.cols)),
            viewport(0),
        );
        assert_eq!(mode.cursor(), (3, GRID.cols - 1));
        assert_eq!(
            mode.selection(GRID.total_scrollback_rows),
            Some(TerminalSelection {
                anchor: pos(10, 2),
                cursor: pos(13, GRID.cols - 1)
            })
        );
    }

    #[test]
    fn a_press_or_an_empty_drag_moves_the_cursor_and_drops_the_anchor() {
        let mut mode = TerminalCopyMode::new(0, 0, 0);
        mode.handle_key(key(KeyCode::Char('v')), GRID);
        assert_eq!(
            mode.handle_pointer(&pointer_event(None, pos(12, 7)), viewport(0)),
            CopyModeAction::SelectionChanged
        );
        assert_eq!(mode.anchor(), None);
        assert_eq!(mode.cursor(), (2, 7));

        let empty = TerminalSelection::new(pos(12, 3));
        assert_eq!(
            mode.handle_pointer(&pointer_event(Some(empty), pos(12, 3)), viewport(0)),
            CopyModeAction::Moved
        );
        assert_eq!(mode.cursor(), (2, 3));
        assert_eq!(
            mode.handle_pointer(&pointer_event(Some(empty), pos(12, 3)), viewport(0)),
            CopyModeAction::Ignored
        );
    }

    #[test]
    fn selection_events_without_a_pointer_leave_copy_mode_alone() {
        let mut mode = TerminalCopyMode::new(1, 4, 0);
        mode.handle_key(key(KeyCode::Char('v')), GRID);
        let before = mode.clone();
        let cleared = TerminalSelectionEvent {
            selection: None,
            text: None,
            pointer: None,
        };
        assert_eq!(
            mode.handle_pointer(&cleared, viewport(0)),
            CopyModeAction::Ignored
        );
        assert_eq!(mode, before);
    }

    #[test]
    fn pointer_cursor_follows_the_widget_offset_and_stays_on_screen() {
        // The wheel scrolled the widget three rows up; the pointer's row is in that view.
        let mut mode = TerminalCopyMode::new(0, 0, 0);
        mode.handle_pointer(&pointer_event(None, pos(8, 1)), viewport(3));
        assert_eq!(mode.scrollback_offset(), 3);
        assert_eq!(mode.cursor(), (1, 1));

        // A selection ending above the view scrolls just far enough to show its cursor.
        let above = TerminalSelection {
            anchor: pos(9, 0),
            cursor: pos(2, 0),
        };
        mode.handle_pointer(&pointer_event(Some(above), pos(2, 0)), viewport(3));
        assert_eq!(mode.scrollback_offset(), 8);
        assert_eq!(mode.cursor(), (0, 0));
    }
}
