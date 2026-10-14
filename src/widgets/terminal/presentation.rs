//! Tokens observed by actual host presentation, independent of retained captures.
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use crate::callback::Callback;

/// One terminal's application token included in a successfully written host frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PresentedTerminal {
    /// Stable identity of the retained screen, independent of its mounted widgets.
    pub stable_terminal_id: u64,
    /// Application-supplied sequence associated with the rendered terminal state.
    pub token: u64,
}

/// Keeps a successful-presentation observer registered until dropped.
#[must_use = "dropping the subscription unsubscribes immediately"]
pub struct TerminalPresentationSubscription {
    registry: Weak<PresentationObservers>,
    id: u64,
}

impl Drop for TerminalPresentationSubscription {
    fn drop(&mut self) {
        if let Some(registry) = self.registry.upgrade() {
            registry
                .entries
                .borrow_mut()
                .retain(|(id, _)| *id != self.id);
        }
    }
}

#[derive(Default)]
pub(crate) struct PresentationObservers {
    next_id: Cell<u64>,
    delivered: Cell<bool>,
    entries: RefCell<Vec<(u64, Callback<Vec<PresentedTerminal>>)>>,
}

impl PresentationObservers {
    pub(crate) fn subscribe(
        self: &Rc<Self>,
        callback: Callback<Vec<PresentedTerminal>>,
    ) -> TerminalPresentationSubscription {
        let id = self.next_id.get();
        self.next_id.set(id.wrapping_add(1));
        self.entries.borrow_mut().push((id, callback));
        TerminalPresentationSubscription {
            registry: Rc::downgrade(self),
            id,
        }
    }

    pub(crate) fn deliver(&self, mut terminals: Vec<PresentedTerminal>) -> bool {
        if terminals.is_empty() || self.entries.borrow().is_empty() {
            return false;
        }
        terminals.sort_unstable_by_key(|terminal| {
            (
                terminal.stable_terminal_id,
                std::cmp::Reverse(terminal.token),
            )
        });
        terminals.dedup_by_key(|terminal| terminal.stable_terminal_id);
        let callbacks = self.entries.borrow().clone();
        for (id, callback) in callbacks {
            if self
                .entries
                .borrow()
                .iter()
                .any(|(current, _)| *current == id)
            {
                callback.emit(terminals.clone());
            }
        }
        self.delivered.set(true);
        true
    }

    pub(crate) fn take_delivered(&self) -> bool {
        self.delivered.replace(false)
    }
}

thread_local! {
    static COLLECTOR: RefCell<Option<Rc<RefCell<Vec<PresentedTerminal>>>>> = const { RefCell::new(None) };
}

/// Stack-scoped collector: offscreen captures suspend it, and failed frames drop their tokens.
pub(crate) struct PresentationScope {
    previous: Option<Rc<RefCell<Vec<PresentedTerminal>>>>,
    current: Option<Rc<RefCell<Vec<PresentedTerminal>>>>,
}

impl PresentationScope {
    pub(crate) fn begin() -> Self {
        Self::replace(Some(Rc::default()))
    }
    pub(crate) fn suspend() -> Self {
        Self::replace(None)
    }
    fn replace(current: Option<Rc<RefCell<Vec<PresentedTerminal>>>>) -> Self {
        let previous = COLLECTOR.with(|slot| slot.replace(current.clone()));
        Self { previous, current }
    }
    pub(crate) fn take(&self) -> Vec<PresentedTerminal> {
        self.current
            .as_ref()
            .map_or_else(Vec::new, |tokens| std::mem::take(&mut *tokens.borrow_mut()))
    }
}

impl Drop for PresentationScope {
    fn drop(&mut self) {
        COLLECTOR.with(|slot| {
            slot.replace(self.previous.take());
        });
    }
}

pub(crate) fn stage(terminal: Option<PresentedTerminal>) {
    if let Some(terminal) = terminal {
        COLLECTOR.with(|slot| {
            if let Some(tokens) = slot.borrow().as_ref() {
                tokens.borrow_mut().push(terminal);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_batch_highest_token_and_unsubscribe_without_requesting_frames() {
        let observers = Rc::new(PresentationObservers::default());
        let reports = Rc::new(RefCell::new(Vec::new()));
        let received = reports.clone();
        let subscription = observers.subscribe(Callback::new(move |batch| {
            received.borrow_mut().push(batch)
        }));
        let terminal = |id, token| PresentedTerminal {
            stable_terminal_id: id,
            token,
        };
        observers.deliver(vec![terminal(2, 5), terminal(1, 3), terminal(1, 7)]);
        assert_eq!(
            *reports.borrow(),
            vec![vec![terminal(1, 7), terminal(2, 5)]]
        );
        assert!(observers.take_delivered());
        assert!(!observers.take_delivered());
        drop(subscription);
        assert!(!observers.deliver(vec![terminal(1, 8)]));
        assert_eq!(reports.borrow().len(), 1);
    }

    #[test]
    fn offscreen_captures_do_not_collect_presentation_tokens() {
        let frame = PresentationScope::begin();
        let terminal = PresentedTerminal {
            stable_terminal_id: 1,
            token: 4,
        };
        stage(Some(terminal));
        {
            let _capture = PresentationScope::suspend();
            stage(Some(PresentedTerminal {
                token: 100,
                ..terminal
            }));
        }
        stage(Some(terminal));
        assert_eq!(frame.take(), vec![terminal, terminal]);
    }
}
