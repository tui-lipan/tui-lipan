use std::collections::VecDeque;
use std::sync::Arc;

/// Bounded line-oriented buffer for terminal output.
#[derive(Clone, Debug)]
pub struct TerminalBuffer {
    max_lines: usize,
    lines: VecDeque<String>,
    current_line: String,
    cached: Arc<str>,
    dirty: bool,
}

impl TerminalBuffer {
    /// Create a new terminal buffer keeping at most `max_lines` lines.
    pub fn new(max_lines: usize) -> Self {
        Self {
            max_lines: max_lines.max(1),
            lines: VecDeque::new(),
            current_line: String::new(),
            cached: Arc::from(""),
            dirty: false,
        }
    }

    /// Return configured line limit.
    pub fn max_lines(&self) -> usize {
        self.max_lines
    }

    /// Set line limit.
    pub fn set_max_lines(&mut self, max_lines: usize) {
        self.max_lines = max_lines.max(1);
        self.trim();
        self.dirty = true;
    }

    /// Remove all buffered output.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.current_line.clear();
        self.cached = Arc::from("");
        self.dirty = false;
    }

    /// Append UTF-8 text.
    pub fn push_text(&mut self, chunk: &str) {
        for ch in chunk.chars() {
            match ch {
                '\n' => {
                    self.commit_current_line();
                }
                '\r' => {
                    self.current_line.clear();
                    self.dirty = true;
                }
                '\u{8}' => {
                    self.current_line.pop();
                    self.dirty = true;
                }
                _ => {
                    self.current_line.push(ch);
                    self.dirty = true;
                }
            }
        }
    }

    /// Append bytes (lossy UTF-8 decoding).
    pub fn push_bytes(&mut self, bytes: &[u8]) {
        let text = String::from_utf8_lossy(bytes);
        self.push_text(&text);
    }

    /// Return current text snapshot.
    pub fn snapshot(&mut self) -> Arc<str> {
        if !self.dirty {
            return self.cached.clone();
        }

        let mut text = String::new();
        for (i, line) in self.lines.iter().enumerate() {
            if i > 0 {
                text.push('\n');
            }
            text.push_str(line);
        }

        if !self.current_line.is_empty() {
            if !text.is_empty() {
                text.push('\n');
            }
            text.push_str(&self.current_line);
        }

        self.cached = Arc::from(text);
        self.dirty = false;
        self.cached.clone()
    }

    fn commit_current_line(&mut self) {
        self.lines.push_back(std::mem::take(&mut self.current_line));
        self.trim();
        self.dirty = true;
    }

    fn trim(&mut self) {
        while self.lines.len() > self.max_lines {
            self.lines.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TerminalBuffer;
    use std::sync::Arc;

    #[test]
    fn newline_commits_current_line_and_starts_new_one() {
        let mut buffer = TerminalBuffer::new(10);

        buffer.push_text("hello\nworld");

        assert_eq!(buffer.lines.len(), 1);
        assert_eq!(buffer.lines[0], "hello");
        assert_eq!(buffer.current_line, "world");
        assert_eq!(buffer.snapshot().as_ref(), "hello\nworld");
    }

    #[test]
    fn carriage_return_clears_current_line_for_rewrite() {
        let mut buffer = TerminalBuffer::new(10);

        buffer.push_text("prefix\nprogress 10%\rprogress 90%");

        assert_eq!(buffer.lines.len(), 1);
        assert_eq!(buffer.lines[0], "prefix");
        assert_eq!(buffer.current_line, "progress 90%");
        assert_eq!(buffer.snapshot().as_ref(), "prefix\nprogress 90%");
    }

    #[test]
    fn backspace_removes_last_character_and_ignores_underflow() {
        let mut buffer = TerminalBuffer::new(10);

        buffer.push_text("abcd\u{8}\u{8}e");
        assert_eq!(buffer.current_line, "abe");
        assert_eq!(buffer.snapshot().as_ref(), "abe");

        buffer.clear();
        buffer.push_text("\u{8}\u{8}x");
        assert_eq!(buffer.current_line, "x");
        assert_eq!(buffer.snapshot().as_ref(), "x");
    }

    #[test]
    fn max_lines_trimming_drops_oldest_committed_lines() {
        let mut buffer = TerminalBuffer::new(2);

        buffer.push_text("line1\nline2\nline3\n");

        assert_eq!(buffer.lines.len(), 2);
        assert_eq!(buffer.lines[0], "line2");
        assert_eq!(buffer.lines[1], "line3");
        assert_eq!(buffer.current_line, "");
        assert_eq!(buffer.snapshot().as_ref(), "line2\nline3");
    }

    #[test]
    fn snapshot_uses_cache_until_buffer_becomes_dirty() {
        let mut buffer = TerminalBuffer::new(10);
        buffer.push_text("alpha");

        let first = buffer.snapshot();
        let second = buffer.snapshot();
        assert!(Arc::ptr_eq(&first, &second));

        buffer.push_text(" beta");
        let third = buffer.snapshot();
        assert!(!Arc::ptr_eq(&second, &third));
        assert_eq!(third.as_ref(), "alpha beta");
    }

    #[test]
    fn clear_resets_buffer_and_snapshot_to_empty() {
        let mut buffer = TerminalBuffer::new(10);
        buffer.push_text("a\nb\nc");
        let _ = buffer.snapshot();

        buffer.clear();

        assert!(buffer.lines.is_empty());
        assert!(buffer.current_line.is_empty());
        assert!(!buffer.dirty);
        assert_eq!(buffer.snapshot().as_ref(), "");
    }
}
