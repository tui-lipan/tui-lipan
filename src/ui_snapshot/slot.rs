use std::cell::RefCell;
use std::rc::Rc;

use super::UiSnapshot;

/// Slot that receives a UI snapshot after the next render in a live app.
#[derive(Clone, Default)]
pub struct UiSnapshotSlot {
    inner: Rc<RefCell<Option<UiSnapshot>>>,
}

impl UiSnapshotSlot {
    /// Create an empty snapshot slot.
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when a snapshot has been delivered and not yet taken.
    pub fn is_ready(&self) -> bool {
        self.inner.borrow().is_some()
    }

    /// Returns and clears the delivered snapshot, if any.
    pub fn take(&self) -> Option<UiSnapshot> {
        self.inner.borrow_mut().take()
    }

    pub(crate) fn shared(&self) -> Rc<RefCell<Option<UiSnapshot>>> {
        Rc::clone(&self.inner)
    }
}
