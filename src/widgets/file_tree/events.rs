use super::fs::FileKind;
use std::sync::Arc;

/// File selection event emitted by `FileTree`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTreeEvent {
    /// Full path of selected entry.
    pub path: Arc<str>,
    /// Kind of selected entry.
    pub kind: FileKind,
}

/// File expand/collapse event emitted by `FileTree`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileTreeToggleEvent {
    /// Full path of toggled entry.
    pub path: Arc<str>,
    /// Kind of toggled entry.
    pub kind: FileKind,
    /// New expand state.
    pub expanded: bool,
}
