use crate::utils::{GridPos, GridSelection};

/// Absolute scrollback position: `line` counts from the oldest retained line (`0`); `col` is a
/// display column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TerminalPos {
    /// Absolute line index from the oldest retained scrollback line.
    pub line: usize,
    /// Display column on that line.
    pub col: usize,
}

/// A terminal selection anchored to absolute scrollback lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalSelection {
    /// Selection start in absolute scrollback coordinates.
    pub anchor: TerminalPos,
    /// Selection end in absolute scrollback coordinates.
    pub cursor: TerminalPos,
}

/// Terminal selection change payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalSelectionEvent {
    /// The new selection, if any.
    pub selection: Option<TerminalSelection>,
    /// Selected text extracted for convenience.
    pub text: Option<String>,
}

/// Scrollback lineage counters used to rebase or invalidate absolute selections.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollbackLineage {
    /// Cumulative lines evicted from scrollback since creation.
    pub evicted_lines: u64,
    /// Bumped when retained text no longer matches prior absolute indices.
    pub history_epoch: u64,
}

impl TerminalSelection {
    /// Create a new selection starting at `pos`.
    pub fn new(pos: TerminalPos) -> Self {
        Self {
            anchor: pos,
            cursor: pos,
        }
    }

    /// Extend the selection to `pos`.
    pub fn extend_to(&mut self, pos: TerminalPos) {
        self.cursor = pos;
    }

    /// Return normalized `(start, end)` where start <= end.
    pub fn normalized(&self) -> (TerminalPos, TerminalPos) {
        if (self.anchor.line, self.anchor.col) <= (self.cursor.line, self.cursor.col) {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    /// Whether anchor and cursor coincide.
    pub fn is_empty(&self) -> bool {
        self.anchor == self.cursor
    }

    /// Whether `(row, col)` lies inside the normalized selection.
    pub fn contains(&self, line: usize, col: usize) -> bool {
        let (start, end) = self.normalized();
        if line < start.line || line > end.line {
            return false;
        }
        if line == start.line && line == end.line {
            col >= start.col && col < end.col
        } else if line == start.line {
            col >= start.col
        } else if line == end.line {
            col < end.col
        } else {
            true
        }
    }
}

/// Map a viewport row to an absolute line index.
pub fn absolute_line(
    total_scrollback_rows: usize,
    scrollback_offset: usize,
    viewport_row: usize,
) -> usize {
    total_scrollback_rows
        .saturating_sub(scrollback_offset)
        .saturating_add(viewport_row)
}

/// Map an absolute line index to a viewport row at the current offset.
pub fn viewport_row(
    total_scrollback_rows: usize,
    scrollback_offset: usize,
    absolute_line: usize,
) -> isize {
    absolute_line as isize - total_scrollback_rows as isize + scrollback_offset as isize
}

/// Build an absolute selection from viewport coordinates.
pub fn from_viewport(
    anchor: GridPos,
    cursor: GridPos,
    scrollback_offset: usize,
    total_scrollback_rows: usize,
) -> TerminalSelection {
    TerminalSelection {
        anchor: TerminalPos {
            line: absolute_line(total_scrollback_rows, scrollback_offset, anchor.row),
            col: anchor.col,
        },
        cursor: TerminalPos {
            line: absolute_line(total_scrollback_rows, scrollback_offset, cursor.row),
            col: cursor.col,
        },
    }
}

/// Project an absolute selection into viewport coordinates for rendering.
///
/// Returns `None` when no part of the selection intersects the visible viewport.
pub fn to_viewport(
    sel: &TerminalSelection,
    scrollback_offset: usize,
    total_scrollback_rows: usize,
    viewport_rows: usize,
) -> Option<GridSelection> {
    let (start, end) = sel.normalized();
    let top = total_scrollback_rows.saturating_sub(scrollback_offset);
    let bottom = top.saturating_add(viewport_rows.saturating_sub(1));

    if end.line < top || start.line > bottom {
        return None;
    }

    let start_row = viewport_row(total_scrollback_rows, scrollback_offset, start.line);
    let end_row = viewport_row(total_scrollback_rows, scrollback_offset, end.line);

    let (anchor_row, anchor_col) = if start_row < 0 {
        (0, 0)
    } else {
        (start_row as usize, start.col)
    };

    let (cursor_row, cursor_col) = if end_row >= viewport_rows as isize {
        (viewport_rows, 0)
    } else {
        (end_row as usize, end.col)
    };

    Some(GridSelection {
        anchor: GridPos {
            row: anchor_row,
            col: anchor_col,
        },
        cursor: GridPos {
            row: cursor_row,
            col: cursor_col,
        },
    })
}

impl From<TerminalSelection> for GridSelection {
    fn from(sel: TerminalSelection) -> Self {
        Self {
            anchor: GridPos {
                row: sel.anchor.line,
                col: sel.anchor.col,
            },
            cursor: GridPos {
                row: sel.cursor.line,
                col: sel.cursor.col,
            },
        }
    }
}

/// Rebase or drop a selection across scrollback lineage changes.
pub(crate) fn rebase_selection(
    sel: Option<TerminalSelection>,
    from: ScrollbackLineage,
    to: ScrollbackLineage,
) -> Option<TerminalSelection> {
    let sel = sel?;
    if from.history_epoch != to.history_epoch {
        return None;
    }
    let delta = to.evicted_lines.saturating_sub(from.evicted_lines);
    if delta == 0 {
        return Some(sel);
    }
    let shift = usize::try_from(delta).unwrap_or(usize::MAX);
    let rebase_pos = |pos: TerminalPos| -> Option<TerminalPos> {
        pos.line
            .checked_sub(shift)
            .map(|line| TerminalPos { line, col: pos.col })
    };
    Some(TerminalSelection {
        anchor: rebase_pos(sel.anchor)?,
        cursor: rebase_pos(sel.cursor)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_viewport_round_trips_at_live_view() {
        let sel = from_viewport(
            GridPos { row: 1, col: 2 },
            GridPos { row: 3, col: 5 },
            0,
            10,
        );
        let projected = to_viewport(&sel, 0, 10, 24).expect("visible");
        assert_eq!(projected.anchor, GridPos { row: 1, col: 2 });
        assert_eq!(projected.cursor, GridPos { row: 3, col: 5 });
    }

    #[test]
    fn to_viewport_clamps_start_above_and_end_below() {
        let sel = TerminalSelection {
            anchor: TerminalPos { line: 2, col: 4 },
            cursor: TerminalPos { line: 20, col: 7 },
        };
        let projected = to_viewport(&sel, 5, 10, 8).expect("partial overlap");
        assert_eq!(projected.anchor, GridPos { row: 0, col: 0 });
        assert_eq!(projected.cursor, GridPos { row: 8, col: 0 });
    }

    #[test]
    fn to_viewport_none_when_entirely_off_screen() {
        let above = TerminalSelection {
            anchor: TerminalPos { line: 0, col: 0 },
            cursor: TerminalPos { line: 1, col: 3 },
        };
        assert!(to_viewport(&above, 0, 10, 8).is_none());

        let below = TerminalSelection {
            anchor: TerminalPos { line: 30, col: 0 },
            cursor: TerminalPos { line: 31, col: 3 },
        };
        assert!(to_viewport(&below, 0, 10, 8).is_none());
    }

    #[test]
    fn rebase_selection_shifts_and_drops_on_eviction() {
        let sel = TerminalSelection {
            anchor: TerminalPos { line: 5, col: 1 },
            cursor: TerminalPos { line: 8, col: 2 },
        };
        let from = ScrollbackLineage {
            evicted_lines: 2,
            history_epoch: 1,
        };
        let to = ScrollbackLineage {
            evicted_lines: 4,
            history_epoch: 1,
        };
        let rebased = rebase_selection(Some(sel), from, to).expect("shift");
        assert_eq!(rebased.anchor.line, 3);
        assert_eq!(rebased.cursor.line, 6);

        let lost_anchor = TerminalSelection {
            anchor: TerminalPos { line: 1, col: 0 },
            cursor: TerminalPos { line: 8, col: 0 },
        };
        assert!(rebase_selection(Some(lost_anchor), from, to).is_none());

        assert!(
            rebase_selection(
                Some(sel),
                from,
                ScrollbackLineage {
                    evicted_lines: 4,
                    history_epoch: 2,
                }
            )
            .is_none()
        );
    }
}
