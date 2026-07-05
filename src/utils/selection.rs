//! Grid-based selection utilities.

/// A position in a 2D grid.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GridPos {
    /// Zero-based row index in the grid.
    pub row: usize,
    /// Zero-based column index in the grid.
    pub col: usize,
}

/// A selection in a 2D grid using (row, col) coordinates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GridSelection {
    /// Starting point (where mouse was pressed).
    pub anchor: GridPos,
    /// Current point (where mouse is now).
    pub cursor: GridPos,
}

impl GridSelection {
    /// Create a new selection starting at the given position.
    pub fn new(pos: GridPos) -> Self {
        Self {
            anchor: pos,
            cursor: pos,
        }
    }

    /// Extend selection to new cursor position.
    pub fn extend_to(&mut self, pos: GridPos) {
        self.cursor = pos;
    }

    /// Get normalized (start, end) positions where start <= end.
    pub fn normalized(&self) -> (GridPos, GridPos) {
        if (self.anchor.row, self.anchor.col) <= (self.cursor.row, self.cursor.col) {
            (self.anchor, self.cursor)
        } else {
            (self.cursor, self.anchor)
        }
    }

    /// Check if selection is empty (anchor == cursor).
    pub fn is_empty(&self) -> bool {
        self.anchor == self.cursor
    }

    /// Check if a cell is within the selection.
    pub fn contains(&self, row: usize, col: usize) -> bool {
        let (start, end) = self.normalized();
        if row < start.row || row > end.row {
            return false;
        }
        if row == start.row && row == end.row {
            col >= start.col && col < end.col
        } else if row == start.row {
            col >= start.col
        } else if row == end.row {
            col < end.col
        } else {
            true
        }
    }

    /// Extract selected text from grid lines.
    /// Handles multi-line selection with newlines between rows.
    pub fn extract_text<S: AsRef<str>>(&self, lines: &[S]) -> String {
        let (start, end) = self.normalized();
        let mut result = String::new();

        for row in start.row..=end.row {
            let Some(line) = lines.get(row) else { continue };
            let line = line.as_ref();

            let col_start = if row == start.row { start.col } else { 0 };
            let col_end = if row == end.row {
                end.col
            } else {
                line.chars().count()
            };

            let extracted: String = line
                .chars()
                .skip(col_start)
                .take(col_end.saturating_sub(col_start))
                .collect();

            result.push_str(&extracted);
            if row < end.row {
                result.push('\n');
            }
        }

        result
    }

    /// Get the column range for a specific row (for rendering).
    /// Returns None if row is not in selection.
    pub fn columns_for_row(&self, row: usize, line_width: usize) -> Option<(usize, usize)> {
        let (start, end) = self.normalized();
        if row < start.row || row > end.row {
            return None;
        }

        let col_start = if row == start.row { start.col } else { 0 };
        let col_end = if row == end.row { end.col } else { line_width };

        Some((col_start, col_end))
    }
}

/// Selection-related event.
#[derive(Clone, Debug)]
pub struct GridSelectionEvent {
    /// Selection range in grid coordinates.
    pub selection: Option<GridSelection>,
    /// Extracted text (if requested by the caller).
    pub text: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(row: usize, col: usize) -> GridPos {
        GridPos { row, col }
    }

    #[test]
    fn empty_selection_is_empty() {
        let sel = GridSelection::new(pos(3, 7));
        assert!(sel.is_empty());
    }

    #[test]
    fn empty_selection_contains_nothing() {
        let sel = GridSelection::new(pos(1, 1));
        assert!(!sel.contains(0, 0));
        assert!(!sel.contains(1, 1));
        assert!(!sel.contains(1, 0));
        assert!(!sel.contains(2, 2));
    }

    #[test]
    fn single_row_selection() {
        let mut sel = GridSelection::new(pos(0, 2));
        sel.extend_to(pos(0, 5));

        assert!(sel.contains(0, 2));
        assert!(sel.contains(0, 3));
        assert!(sel.contains(0, 4));
        // end col is exclusive
        assert!(!sel.contains(0, 5));
        // outside
        assert!(!sel.contains(0, 1));
        assert!(!sel.contains(1, 3));
    }

    #[test]
    fn backward_selection_normalized() {
        let mut sel = GridSelection::new(pos(0, 5));
        sel.extend_to(pos(0, 2));

        let (start, end) = sel.normalized();
        assert_eq!(start, pos(0, 2));
        assert_eq!(end, pos(0, 5));
    }

    #[test]
    fn multi_row_selection_contains() {
        let mut sel = GridSelection::new(pos(0, 3));
        sel.extend_to(pos(2, 2));

        // first row: col 3+ included
        assert!(!sel.contains(0, 2));
        assert!(sel.contains(0, 3));
        assert!(sel.contains(0, 100));

        // middle row: fully contained
        assert!(sel.contains(1, 0));
        assert!(sel.contains(1, 999));

        // last row: up to col 2 (exclusive)
        assert!(sel.contains(2, 0));
        assert!(sel.contains(2, 1));
        assert!(!sel.contains(2, 2));

        // outside rows
        assert!(!sel.contains(3, 0));
    }

    #[test]
    fn multi_row_middle_row_fully_contained() {
        let mut sel = GridSelection::new(pos(1, 5));
        sel.extend_to(pos(4, 1));

        // rows 2 and 3 are middle rows - every column should be contained
        for col in 0..50 {
            assert!(sel.contains(2, col), "row 2, col {col} should be contained");
            assert!(sel.contains(3, col), "row 3, col {col} should be contained");
        }
    }

    #[test]
    fn extract_text_single_line() {
        let lines = ["hello world"];
        let mut sel = GridSelection::new(pos(0, 0));
        sel.extend_to(pos(0, 5));

        assert_eq!(sel.extract_text(&lines), "hello");
    }

    #[test]
    fn extract_text_multi_line() {
        let lines = ["abc", "def", "ghi"];
        let mut sel = GridSelection::new(pos(0, 1));
        sel.extend_to(pos(2, 2));

        assert_eq!(sel.extract_text(&lines), "bc\ndef\ngh");
    }

    #[test]
    fn extract_text_empty_selection() {
        let lines = ["hello", "world"];
        let sel = GridSelection::new(pos(0, 3));

        assert_eq!(sel.extract_text(&lines), "");
    }

    #[test]
    fn columns_for_row_first_row() {
        let mut sel = GridSelection::new(pos(1, 4));
        sel.extend_to(pos(3, 2));

        // first row of selection (row 1), line_width=10
        let result = sel.columns_for_row(1, 10);
        assert_eq!(result, Some((4, 10)));
    }

    #[test]
    fn columns_for_row_middle_row() {
        let mut sel = GridSelection::new(pos(1, 4));
        sel.extend_to(pos(3, 2));

        // middle row (row 2), line_width=8
        let result = sel.columns_for_row(2, 8);
        assert_eq!(result, Some((0, 8)));
    }

    #[test]
    fn columns_for_row_last_row() {
        let mut sel = GridSelection::new(pos(1, 4));
        sel.extend_to(pos(3, 2));

        // last row (row 3), line_width=10
        let result = sel.columns_for_row(3, 10);
        assert_eq!(result, Some((0, 2)));
    }

    #[test]
    fn columns_for_row_outside() {
        let mut sel = GridSelection::new(pos(1, 4));
        sel.extend_to(pos(3, 2));

        assert_eq!(sel.columns_for_row(0, 10), None);
        assert_eq!(sel.columns_for_row(4, 10), None);
    }
}
