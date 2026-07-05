use std::cell::RefCell;

use crate::core::element::Key;

thread_local! {
    static DRAG_SOURCE_SNAPSHOT_COLLAPSE_KEY: RefCell<Option<Key>> = const { RefCell::new(None) };
}

pub(crate) fn set_drag_source_snapshot_collapse_key(key: Option<Key>) {
    DRAG_SOURCE_SNAPSHOT_COLLAPSE_KEY.with(|c| *c.borrow_mut() = key);
}

pub(crate) fn clear_drag_source_snapshot_collapse_key() {
    DRAG_SOURCE_SNAPSHOT_COLLAPSE_KEY.with(|c| *c.borrow_mut() = None);
}

pub(crate) fn drag_source_snapshot_collapse_key() -> Option<Key> {
    DRAG_SOURCE_SNAPSHOT_COLLAPSE_KEY.with(|c| c.borrow().clone())
}
