use std::collections::VecDeque;
use std::sync::Arc;

const DEFAULT_HISTORY_LIMIT: usize = 1000;

#[derive(Clone)]
struct HexSnapshot {
    bytes: Arc<[u8]>,
    cursor: usize,
    anchor: Option<usize>,
}

impl HexSnapshot {
    fn new(bytes: Arc<[u8]>, cursor: usize, anchor: Option<usize>) -> Self {
        Self {
            bytes,
            cursor,
            anchor,
        }
    }
}

#[derive(Default)]
pub(crate) struct HexHistory {
    current: Option<HexSnapshot>,
    undo_stack: VecDeque<HexSnapshot>,
    redo_stack: VecDeque<HexSnapshot>,
}

impl HexHistory {
    pub(crate) fn new(bytes: Arc<[u8]>, cursor: usize, anchor: Option<usize>) -> Self {
        Self {
            current: Some(HexSnapshot::new(bytes, cursor, anchor)),
            ..Self::default()
        }
    }

    pub(crate) fn sync_from(&mut self, bytes: Arc<[u8]>, cursor: usize, anchor: Option<usize>) {
        match self.current.as_mut() {
            Some(current) => {
                if current.bytes.as_ref() != bytes.as_ref() {
                    self.current = Some(HexSnapshot::new(bytes, cursor, anchor));
                    self.undo_stack.clear();
                    self.redo_stack.clear();
                } else {
                    current.cursor = cursor;
                    current.anchor = anchor;
                }
            }
            None => {
                self.current = Some(HexSnapshot::new(bytes, cursor, anchor));
                self.undo_stack.clear();
                self.redo_stack.clear();
            }
        }
    }

    pub(crate) fn apply_change(&mut self, bytes: Arc<[u8]>, cursor: usize, anchor: Option<usize>) {
        let next = HexSnapshot::new(bytes, cursor, anchor);

        if let Some(current) = self.current.as_ref() {
            self.undo_stack.push_back(current.clone());
            if self.undo_stack.len() > DEFAULT_HISTORY_LIMIT {
                self.undo_stack.pop_front();
            }
        }

        self.current = Some(next);
        self.redo_stack.clear();
    }

    pub(crate) fn undo(&mut self) -> Option<(Arc<[u8]>, usize, Option<usize>)> {
        let previous = self.undo_stack.pop_back()?;
        if let Some(current) = self.current.take() {
            self.redo_stack.push_back(current);
        }
        self.current = Some(previous.clone());
        Some((previous.bytes, previous.cursor, previous.anchor))
    }

    pub(crate) fn redo(&mut self) -> Option<(Arc<[u8]>, usize, Option<usize>)> {
        let next = self.redo_stack.pop_back()?;
        if let Some(current) = self.current.take() {
            self.undo_stack.push_back(current);
            if self.undo_stack.len() > DEFAULT_HISTORY_LIMIT {
                self.undo_stack.pop_front();
            }
        }
        self.current = Some(next.clone());
        Some((next.bytes, next.cursor, next.anchor))
    }
}

#[cfg(test)]
mod tests {
    use super::HexHistory;
    use std::sync::Arc;

    #[test]
    fn undo_redo_roundtrip() {
        let mut history = HexHistory::new(Arc::from([0xAA_u8, 0xBB_u8]), 0, None);
        history.apply_change(Arc::from([0xCC_u8, 0xBB_u8]), 0, None);
        history.apply_change(Arc::from([0xCC_u8, 0xDD_u8]), 1, None);

        let (bytes, cursor, anchor) = history.undo().expect("undo should succeed");
        assert_eq!(bytes.as_ref(), &[0xCC, 0xBB]);
        assert_eq!(cursor, 0);
        assert_eq!(anchor, None);

        let (bytes, cursor, anchor) = history.redo().expect("redo should succeed");
        assert_eq!(bytes.as_ref(), &[0xCC, 0xDD]);
        assert_eq!(cursor, 1);
        assert_eq!(anchor, None);
    }

    #[test]
    fn sync_from_external_bytes_resets_history() {
        let mut history = HexHistory::new(Arc::from([0x01_u8]), 0, None);
        history.apply_change(Arc::from([0x02_u8]), 0, None);
        history.sync_from(Arc::from([0xFF_u8]), 0, None);

        assert!(history.undo().is_none());
    }
}
