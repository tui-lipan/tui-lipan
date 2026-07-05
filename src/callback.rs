use std::any::Any;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};

use crate::core::event::KeyEvent;

/// A cheap-to-clone event handler.
#[derive(Clone)]
pub struct Callback<E>(Rc<dyn Fn(E)>);

impl<E> Callback<E> {
    /// Create a new callback.
    pub fn new(f: impl Fn(E) + 'static) -> Self {
        Self(Rc::new(f))
    }

    /// Invoke the callback.
    pub fn emit(&self, event: E) {
        (self.0)(event)
    }
}

impl<E> PartialEq for Callback<E> {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl<E> Eq for Callback<E> {}

/// A cheap-to-clone key handler that reports handled status.
#[derive(Clone)]
pub struct KeyHandler(Rc<dyn Fn(KeyEvent) -> bool>);

impl KeyHandler {
    /// Create a new key handler.
    pub fn new(f: impl Fn(KeyEvent) -> bool + 'static) -> Self {
        Self(Rc::new(f))
    }

    /// Invoke the handler and return whether it handled the key.
    pub fn handle(&self, event: KeyEvent) -> bool {
        (self.0)(event)
    }
}

impl PartialEq for KeyHandler {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

/// Identifies a mounted component instance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ScopeId(pub u32);

/// Message dispatcher used by `Link`.
#[derive(Clone)]
pub struct Dispatcher(Rc<dyn Fn(ScopeId, Box<dyn Any>)>);

impl Dispatcher {
    /// Create a new dispatcher.
    pub fn new(f: impl Fn(ScopeId, Box<dyn Any>) + 'static) -> Self {
        Self(Rc::new(f))
    }

    /// Dispatch a boxed message to a component scope.
    pub fn dispatch(&self, scope: ScopeId, msg: Box<dyn Any>) {
        (self.0)(scope, msg)
    }
}

pub(crate) type CommandTx = mpsc::Sender<(ScopeId, Box<dyn Any + Send>)>;
pub(crate) type CommandRx = mpsc::Receiver<(ScopeId, Box<dyn Any + Send>)>;

/// Cooperative cancellation state for a background command.
#[derive(Clone, Debug, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    /// Returns `true` when the owning command has been cancelled by the runtime.
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub(crate) fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }
}

/// Type-safe handle used by background tasks to send messages back to the UI thread.
#[derive(Clone)]
pub struct CommandLink<Msg: Send + 'static> {
    scope: ScopeId,
    tx: CommandTx,
    cancellation_token: CancellationToken,
    _marker: PhantomData<fn(Msg)>,
}

impl<Msg: Send + 'static> CommandLink<Msg> {
    pub(crate) fn new(
        scope: ScopeId,
        tx: CommandTx,
        cancellation_token: CancellationToken,
    ) -> Self {
        Self {
            scope,
            tx,
            cancellation_token,
            _marker: PhantomData,
        }
    }

    /// Return the cooperative cancellation token for this command.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.cancellation_token.clone()
    }

    /// Returns `true` when this command has been cancelled by the runtime.
    pub fn is_cancelled(&self) -> bool {
        self.cancellation_token.is_cancelled()
    }

    /// Send a message back to this component instance.
    pub fn send(&self, msg: Msg) {
        let _ = self.tx.send((self.scope, Box::new(msg)));
    }

    /// Send a message unless this command has already been cancelled.
    pub fn send_if_not_cancelled(&self, msg: Msg) -> bool {
        if self.is_cancelled() {
            return false;
        }
        self.tx.send((self.scope, Box::new(msg))).is_ok()
    }
}

/// Type-safe handle used to send messages to a specific component instance.
pub struct Link<Msg: 'static> {
    scope: ScopeId,
    dispatcher: Dispatcher,
    _marker: PhantomData<fn(Msg)>,
}

impl<Msg: 'static> Clone for Link<Msg> {
    fn clone(&self) -> Self {
        Self {
            scope: self.scope,
            dispatcher: self.dispatcher.clone(),
            _marker: PhantomData,
        }
    }
}

impl<Msg: 'static> Link<Msg> {
    pub(crate) fn new(scope: ScopeId, dispatcher: Dispatcher) -> Self {
        Self {
            scope,
            dispatcher,
            _marker: PhantomData,
        }
    }

    /// Send a message to the component.
    pub fn send(&self, msg: Msg) {
        self.dispatcher.dispatch(self.scope, Box::new(msg));
    }

    /// Convert an event into a message.
    pub fn callback<E: 'static>(&self, f: impl Fn(E) -> Msg + 'static) -> Callback<E> {
        let link = (*self).clone();
        Callback::new(move |e| link.send(f(e)))
    }

    /// Convert an event into an optional message.
    ///
    /// If the closure returns `None`, no message is sent.
    pub fn callback_opt<E: 'static>(&self, f: impl Fn(E) -> Option<Msg> + 'static) -> Callback<E> {
        let link = (*self).clone();
        Callback::new(move |e| {
            if let Some(msg) = f(e) {
                link.send(msg);
            }
        })
    }

    /// Convert a key event into an optional message.
    ///
    /// Returns `true` when a message is produced.
    pub fn key_handler(&self, f: impl Fn(KeyEvent) -> Option<Msg> + 'static) -> KeyHandler {
        let link = (*self).clone();
        KeyHandler::new(move |e| {
            if let Some(msg) = f(e) {
                link.send(msg);
                true
            } else {
                false
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::cell::RefCell;
    use std::rc::Rc;

    use super::{Dispatcher, KeyHandler, Link, ScopeId};
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};

    type TestQueue = Rc<RefCell<Vec<(ScopeId, Box<dyn Any>)>>>;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Msg {
        Ping,
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            mods: KeyMods::default(),
        }
    }

    #[test]
    fn key_handler_respects_explicit_handled_flag() {
        let queue: TestQueue = Rc::new(RefCell::new(Vec::new()));
        let dispatcher = {
            let queue = queue.clone();
            Dispatcher::new(move |scope, msg| queue.borrow_mut().push((scope, msg)))
        };
        let link: Link<Msg> = Link::new(ScopeId(1), dispatcher);
        let handler = KeyHandler::new({
            let link = link.clone();
            move |_key| {
                link.send(Msg::Ping);
                false
            }
        });

        let handled = handler.handle(key(KeyCode::Enter));

        assert!(!handled);
        assert_eq!(queue.borrow().len(), 1);
    }

    #[test]
    fn key_handler_returns_true_when_message_emitted() {
        let queue: TestQueue = Rc::new(RefCell::new(Vec::new()));
        let dispatcher = {
            let queue = queue.clone();
            Dispatcher::new(move |scope, msg| queue.borrow_mut().push((scope, msg)))
        };
        let link: Link<Msg> = Link::new(ScopeId(1), dispatcher);
        let handler = link.key_handler(|key| match key.code {
            KeyCode::Enter => Some(Msg::Ping),
            _ => None,
        });

        assert!(handler.handle(key(KeyCode::Enter)));
        assert_eq!(queue.borrow().len(), 1);
        assert!(!handler.handle(key(KeyCode::Tab)));
        assert_eq!(queue.borrow().len(), 1);
    }
}
