use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::Instant;

use super::CapturedFrame;
use crate::callback::Callback;

/// A frame the runtime just painted, handed to
/// [`Context::observe_painted_frames`](crate::Context::observe_painted_frames) subscribers.
///
/// Cloning is cheap: every subscriber of one paint shares the same [`CapturedFrame`], and the
/// frame is `Send`, so it can move to a writer thread without a copy.
#[derive(Clone, Debug)]
pub struct PaintedFrame {
    /// The painted frame, rendered headlessly exactly as
    /// [`Context::request_ui_snapshot`](crate::Context::request_ui_snapshot) renders it.
    pub frame: Arc<CapturedFrame>,
    /// Runtime clock time of the paint. Follows the controlled clock under automation and
    /// [`TestBackend::advance`](crate::TestBackend::advance).
    pub painted_at: Instant,
    /// Paints delivered to observers so far in this runtime, starting at 1. Counts only paints
    /// that had at least one subscriber.
    pub sequence: u64,
}

/// Keeps a [`Context::observe_painted_frames`](crate::Context::observe_painted_frames) callback
/// subscribed. Dropping it unsubscribes.
#[must_use = "dropping the subscription unsubscribes immediately"]
pub struct PaintSubscription {
    registry: Weak<PaintObservers>,
    id: u64,
}

impl PaintSubscription {
    /// Unsubscribe now. Equivalent to dropping the subscription.
    pub fn unsubscribe(self) {}
}

impl Drop for PaintSubscription {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry.remove(self.id);
        }
    }
}

impl std::fmt::Debug for PaintSubscription {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PaintSubscription")
            .field("id", &self.id)
            .finish()
    }
}

/// Callbacks waiting for every normally painted frame.
#[derive(Default)]
pub(crate) struct PaintObservers {
    next_id: Cell<u64>,
    sequence: Cell<u64>,
    entries: RefCell<Vec<(u64, Callback<PaintedFrame>)>>,
}

impl PaintObservers {
    pub(crate) fn subscribe(
        self: &Rc<Self>,
        callback: Callback<PaintedFrame>,
    ) -> PaintSubscription {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1));
        self.entries.borrow_mut().push((id, callback));
        PaintSubscription {
            registry: Rc::downgrade(self),
            id,
        }
    }

    fn remove(&self, id: u64) {
        self.entries.borrow_mut().retain(|(entry, _)| *entry != id);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.entries.borrow().is_empty()
    }

    /// Hand the frame `capture` builds to every subscriber. Captures nothing and allocates
    /// nothing when nobody is subscribed. Returns whether any callback ran.
    pub(crate) fn deliver(
        &self,
        painted_at: Instant,
        capture: impl FnOnce() -> CapturedFrame,
    ) -> bool {
        if self.is_empty() {
            return false;
        }
        // A callback may subscribe or unsubscribe, so the list is not borrowed while they run.
        let callbacks: Vec<_> = self
            .entries
            .borrow()
            .iter()
            .map(|(_, callback)| callback.clone())
            .collect();
        let sequence = self.sequence.get() + 1;
        self.sequence.set(sequence);
        let painted = PaintedFrame {
            frame: Arc::new(capture()),
            painted_at,
            sequence,
        };
        for callback in callbacks {
            callback.emit(painted.clone());
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::Rect;

    fn frame() -> CapturedFrame {
        CapturedFrame {
            viewport: Rect::default(),
            width: 0,
            height: 0,
            cells: Vec::new(),
            cursor: None,
            images: Vec::new(),
        }
    }

    #[test]
    fn no_subscriber_captures_nothing() {
        let observers = Rc::new(PaintObservers::default());
        let delivered = observers.deliver(Instant::now(), || panic!("must not capture"));
        assert!(!delivered);
    }

    #[test]
    fn subscribers_share_one_capture_until_dropped() {
        let observers = Rc::new(PaintObservers::default());
        let seen = Rc::new(RefCell::new(Vec::new()));
        let make = |tag: &'static str| {
            let seen = Rc::clone(&seen);
            Callback::new(move |painted: PaintedFrame| {
                seen.borrow_mut()
                    .push((tag, painted.sequence, painted.frame));
            })
        };
        let first = observers.subscribe(make("a"));
        let second = observers.subscribe(make("b"));

        let captures = Cell::new(0);
        let capture = || {
            captures.set(captures.get() + 1);
            frame()
        };
        assert!(observers.deliver(Instant::now(), capture));
        assert_eq!(captures.get(), 1);
        {
            let seen = seen.borrow();
            assert_eq!(seen.len(), 2);
            assert!(Arc::ptr_eq(&seen[0].2, &seen[1].2));
        }

        drop(first);
        assert!(observers.deliver(Instant::now(), frame));
        assert_eq!(
            seen.borrow()
                .iter()
                .map(|(t, s, _)| (*t, *s))
                .collect::<Vec<_>>(),
            [("a", 1), ("b", 1), ("b", 2)],
        );

        second.unsubscribe();
        assert!(observers.is_empty());
        assert!(!observers.deliver(Instant::now(), || panic!("must not capture")));
    }

    #[test]
    fn callback_may_drop_its_own_subscription() {
        let observers = Rc::new(PaintObservers::default());
        let slot: Rc<RefCell<Option<PaintSubscription>>> = Rc::default();
        let inner = Rc::clone(&slot);
        *slot.borrow_mut() = Some(observers.subscribe(Callback::new(move |_| {
            inner.borrow_mut().take();
        })));
        assert!(observers.deliver(Instant::now(), frame));
        assert!(observers.is_empty());
    }
}
