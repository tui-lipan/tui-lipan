use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

use super::UiSnapshot;
use super::options::UiSnapshotFileFormat;

/// Queued live-app UI snapshot delivery request.
#[derive(Clone)]
pub(crate) enum UiSnapshotRequest {
    Write {
        path: PathBuf,
        format: UiSnapshotFileFormat,
    },
    Deliver(Rc<RefCell<Option<UiSnapshot>>>),
}
