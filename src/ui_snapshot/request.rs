use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use super::UiSnapshot;
use super::options::UiSnapshotFileFormat;
use crate::callback::Callback;

/// Queued live-app UI snapshot delivery request.
#[derive(Clone)]
pub(crate) enum UiSnapshotRequest {
    Write {
        path: PathBuf,
        format: UiSnapshotFileFormat,
    },
    Deliver(Rc<RefCell<Option<UiSnapshot>>>),
}

/// Everything waiting for the next painted frame's snapshot.
///
/// A file or slot request replaces the one before it, while callbacks accumulate: every callback
/// registered before the paint receives that paint's snapshot.
#[derive(Default)]
pub(crate) struct PendingUiSnapshot {
    pub(crate) request: Option<UiSnapshotRequest>,
    pub(crate) callbacks: Vec<Callback<UiSnapshot>>,
}

impl PendingUiSnapshot {
    pub(crate) fn is_empty(&self) -> bool {
        self.request.is_none() && self.callbacks.is_empty()
    }

    /// Hand `snapshot` to every waiter. A write request's error is returned after the in-memory
    /// waiters have been served, so one failed file does not starve them.
    pub(crate) fn deliver(self, snapshot: UiSnapshot) -> crate::Result<()> {
        let mut result = Ok(());
        match self.request {
            Some(UiSnapshotRequest::Write { path, format }) => {
                result = super::write_snapshot(&snapshot, &path, format);
            }
            Some(UiSnapshotRequest::Deliver(slot)) => {
                *slot.borrow_mut() = Some(snapshot.clone());
            }
            None => {}
        }
        for callback in self.callbacks {
            callback.emit(snapshot.clone());
        }
        result
    }
}
