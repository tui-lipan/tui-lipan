//! Snapshot line/column indexing for UTF-8 text.

use std::ops::Range;

use crate::utils::text::clamp_cursor;

/// Zero-based text position in a logical line.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextPosition {
    /// Zero-based logical line.
    pub line: usize,
    /// Zero-based Unicode-scalar column by default.
    pub column: usize,
}

/// Half-open text range expressed as line/column positions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TextRange {
    /// Start position of the range.
    pub start: TextPosition,
    /// End position of the range.
    pub end: TextPosition,
}

/// Column encoding used when projecting byte offsets to text positions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextEncoding {
    /// Column is a UTF-8 byte offset from the start of the line.
    Utf8,
    /// Column is a UTF-16 code-unit offset from the start of the line.
    Utf16,
    /// Column is a Unicode scalar count from the start of the line.
    #[default]
    UnicodeScalar,
}

/// Snapshot index of logical line starts for a UTF-8 text buffer.
///
/// `LineIndex` stores only line-start byte offsets and the source text length.
/// Methods that need to inspect content take `&str`; callers should rebuild the
/// index when that text changes. Only `\n` creates a new logical line, so `\r`
/// remains ordinary content. Empty text has one logical line, and a trailing
/// `\n` creates a final empty line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineIndex {
    line_starts: Vec<usize>,
    text_len: usize,
}

impl LineIndex {
    /// Build a line index for a text snapshot.
    pub fn new(text: &str) -> Self {
        let mut line_starts = vec![0];
        for (byte, ch) in text.char_indices() {
            if ch == '\n' {
                line_starts.push(byte + ch.len_utf8());
            }
        }

        Self {
            line_starts,
            text_len: text.len(),
        }
    }

    /// Return the byte length of the text snapshot used to build this index.
    pub fn text_len(&self) -> usize {
        self.text_len
    }

    /// Return the number of logical lines in this snapshot.
    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// Return the byte offset at the start of `line`, clamped to an existing line.
    pub fn line_start(&self, line: usize) -> usize {
        self.line_starts[self.clamp_line(line)]
    }

    /// Return the byte offset at the end of `line`, excluding a trailing newline.
    pub fn line_end(&self, text: &str, line: usize) -> usize {
        let line = self.clamp_line(line);
        let end = if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1].saturating_sub(1)
        } else {
            text.len()
        };

        clamp_cursor(text, end.min(text.len()))
    }

    /// Return the byte offset at the end of `line`, including its newline if present.
    pub fn line_end_including_newline(&self, text: &str, line: usize) -> usize {
        let line = self.clamp_line(line);
        let end = if line + 1 < self.line_starts.len() {
            self.line_starts[line + 1]
        } else {
            text.len()
        };

        clamp_cursor(text, end.min(text.len()))
    }

    /// Return the byte range for `line`, excluding a trailing newline.
    pub fn line_range(&self, text: &str, line: usize) -> Range<usize> {
        self.line_start(line).min(text.len())..self.line_end(text, line)
    }

    /// Convert a byte offset to a Unicode-scalar line/column position.
    pub fn byte_to_position(&self, text: &str, byte: usize) -> TextPosition {
        self.byte_to_position_with_encoding(text, byte, TextEncoding::UnicodeScalar)
    }

    /// Convert a Unicode-scalar line/column position to a byte offset.
    pub fn position_to_byte(&self, text: &str, position: TextPosition) -> usize {
        self.position_to_byte_with_encoding(text, position, TextEncoding::UnicodeScalar)
    }

    /// Convert a byte offset to a line/column position using `encoding` for columns.
    pub fn byte_to_position_with_encoding(
        &self,
        text: &str,
        byte: usize,
        encoding: TextEncoding,
    ) -> TextPosition {
        let byte = clamp_cursor(text, byte);
        let line = self.line_for_byte(byte);
        let start = self.line_start(line).min(byte);
        let column_text = &text[start..byte];
        let column = match encoding {
            TextEncoding::Utf8 => byte - start,
            TextEncoding::Utf16 => column_text.chars().map(char::len_utf16).sum(),
            TextEncoding::UnicodeScalar => column_text.chars().count(),
        };

        TextPosition { line, column }
    }

    /// Convert a line/column position using `encoding` for columns to a byte offset.
    pub fn position_to_byte_with_encoding(
        &self,
        text: &str,
        position: TextPosition,
        encoding: TextEncoding,
    ) -> usize {
        let line = self.clamp_line(position.line);
        let start = self.line_start(line).min(text.len());
        let end = self.line_end(text, line).max(start);
        let line_text = &text[start..end];
        let offset = match encoding {
            TextEncoding::Utf8 => clamp_cursor(line_text, position.column),
            TextEncoding::Utf16 => byte_offset_for_utf16_column(line_text, position.column),
            TextEncoding::UnicodeScalar => {
                byte_offset_for_scalar_column(line_text, position.column)
            }
        };

        start + offset
    }

    /// Convert a byte range to a text range using Unicode-scalar columns.
    pub fn byte_range_to_range(&self, text: &str, range: Range<usize>) -> TextRange {
        TextRange {
            start: self.byte_to_position(text, range.start),
            end: self.byte_to_position(text, range.end),
        }
    }

    /// Convert a text range using Unicode-scalar columns to a byte range.
    pub fn range_to_byte_range(&self, text: &str, range: TextRange) -> Range<usize> {
        self.position_to_byte(text, range.start)..self.position_to_byte(text, range.end)
    }

    fn clamp_line(&self, line: usize) -> usize {
        line.min(self.line_starts.len().saturating_sub(1))
    }

    fn line_for_byte(&self, byte: usize) -> usize {
        self.line_starts
            .partition_point(|&start| start <= byte)
            .saturating_sub(1)
            .min(self.line_starts.len().saturating_sub(1))
    }
}

fn byte_offset_for_scalar_column(text: &str, column: usize) -> usize {
    text.char_indices()
        .map(|(byte, _)| byte)
        .nth(column)
        .unwrap_or(text.len())
}

fn byte_offset_for_utf16_column(text: &str, column: usize) -> usize {
    let mut units = 0;
    for (byte, ch) in text.char_indices() {
        let next_units = units + ch.len_utf16();
        if next_units > column {
            return byte;
        }
        units = next_units;
        if units == column {
            return byte + ch.len_utf8();
        }
    }
    text.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexes_empty_text_as_one_line() {
        let text = "";
        let index = LineIndex::new(text);

        assert_eq!(index.text_len(), 0);
        assert_eq!(index.line_count(), 1);
        assert_eq!(index.line_start(0), 0);
        assert_eq!(index.line_end(text, 0), 0);
        assert_eq!(index.line_end_including_newline(text, 0), 0);
        assert_eq!(index.line_range(text, 0), 0..0);
        assert_eq!(
            index.byte_to_position(text, 0),
            TextPosition { line: 0, column: 0 }
        );
    }

    #[test]
    fn indexes_single_line() {
        let text = "hello";
        let index = LineIndex::new(text);

        assert_eq!(index.line_count(), 1);
        assert_eq!(index.line_start(0), 0);
        assert_eq!(index.line_end(text, 0), 5);
        assert_eq!(
            index.byte_to_position(text, 3),
            TextPosition { line: 0, column: 3 }
        );
        assert_eq!(
            index.position_to_byte(text, TextPosition { line: 0, column: 4 }),
            4
        );
    }

    #[test]
    fn indexes_multiple_lines() {
        let text = "alpha\nbeta\ngamma";
        let index = LineIndex::new(text);

        assert_eq!(index.line_count(), 3);
        assert_eq!(index.line_start(1), 6);
        assert_eq!(index.line_end(text, 1), 10);
        assert_eq!(index.line_end_including_newline(text, 1), 11);
        assert_eq!(
            index.byte_to_position(text, 8),
            TextPosition { line: 1, column: 2 }
        );
    }

    #[test]
    fn trailing_newline_creates_final_empty_line() {
        let text = "alpha\n";
        let index = LineIndex::new(text);

        assert_eq!(index.line_count(), 2);
        assert_eq!(index.line_start(1), text.len());
        assert_eq!(index.line_end(text, 1), text.len());
        assert_eq!(
            index.byte_to_position(text, text.len()),
            TextPosition { line: 1, column: 0 }
        );
    }

    #[test]
    fn indexes_consecutive_empty_lines() {
        let text = "a\n\nb";
        let index = LineIndex::new(text);

        assert_eq!(index.line_count(), 3);
        assert_eq!(index.line_range(text, 1), 2..2);
        assert_eq!(
            index.byte_to_position(text, 2),
            TextPosition { line: 1, column: 0 }
        );
        assert_eq!(
            index.byte_to_position(text, 3),
            TextPosition { line: 2, column: 0 }
        );
    }

    #[test]
    fn counts_non_ascii_columns_as_scalars_by_default() {
        let text = "aé你";
        let index = LineIndex::new(text);

        assert_eq!(
            index.byte_to_position(text, 3),
            TextPosition { line: 0, column: 2 }
        );
        assert_eq!(
            index.position_to_byte(text, TextPosition { line: 0, column: 2 }),
            3
        );
        assert_eq!(
            index.position_to_byte(text, TextPosition { line: 0, column: 3 }),
            text.len()
        );
    }

    #[test]
    fn clamps_bytes_inside_codepoint_to_previous_boundary() {
        let text = "aéb";
        let index = LineIndex::new(text);

        assert_eq!(
            index.byte_to_position(text, 2),
            TextPosition { line: 0, column: 1 }
        );
        assert_eq!(
            index.byte_to_position_with_encoding(text, 2, TextEncoding::Utf8),
            TextPosition { line: 0, column: 1 }
        );
    }

    #[test]
    fn supports_utf16_columns_for_non_bmp_characters() {
        let text = "a💩b";
        let index = LineIndex::new(text);

        assert_eq!(
            index.byte_to_position_with_encoding(text, 5, TextEncoding::Utf16),
            TextPosition { line: 0, column: 3 }
        );
        assert_eq!(
            index.position_to_byte_with_encoding(
                text,
                TextPosition { line: 0, column: 1 },
                TextEncoding::Utf16
            ),
            1
        );
        assert_eq!(
            index.position_to_byte_with_encoding(
                text,
                TextPosition { line: 0, column: 2 },
                TextEncoding::Utf16
            ),
            1
        );
        assert_eq!(
            index.position_to_byte_with_encoding(
                text,
                TextPosition { line: 0, column: 3 },
                TextEncoding::Utf16
            ),
            5
        );
        assert_eq!(
            index.position_to_byte_with_encoding(
                text,
                TextPosition { line: 0, column: 4 },
                TextEncoding::Utf16
            ),
            6
        );
    }

    #[test]
    fn converts_ranges_round_trip() {
        let text = "ab\nc💩d";
        let index = LineIndex::new(text);
        let bytes = 1..8;
        let range = index.byte_range_to_range(text, bytes.clone());

        assert_eq!(range.start, TextPosition { line: 0, column: 1 });
        assert_eq!(range.end, TextPosition { line: 1, column: 2 });
        assert_eq!(index.range_to_byte_range(text, range), bytes);
    }

    #[test]
    fn clamps_out_of_range_line_and_column() {
        let text = "abc\nde";
        let index = LineIndex::new(text);

        assert_eq!(index.line_start(99), 4);
        assert_eq!(index.line_end(text, 99), text.len());
        assert_eq!(
            index.position_to_byte(
                text,
                TextPosition {
                    line: 99,
                    column: 99
                }
            ),
            text.len()
        );
        assert_eq!(
            index.byte_to_position(text, 999),
            TextPosition { line: 1, column: 2 }
        );
    }

    #[test]
    fn treats_carriage_return_as_content() {
        let text = "a\rb\nc";
        let index = LineIndex::new(text);

        assert_eq!(index.line_count(), 2);
        assert_eq!(
            index.byte_to_position(text, 3),
            TextPosition { line: 0, column: 3 }
        );
    }
}
