//! The app's side of the protocol: talking to the host terminal the app runs in.
//!
//! Events from the host reach the root component through
//! [`Component::on_host_drag_and_drop`](crate::Component::on_host_drag_and_drop). Commands go out
//! through [`HostDragAndDrop`], from [`Context::host_drag_and_drop`](crate::Context::host_drag_and_drop),
//! and are written between frames.
//!
//! The runtime keeps what the app registered (accepting drops, offering drags). It withdraws both
//! when the app exits or hands the terminal to another program, and registers again on return, so
//! a program run in the meantime never receives the app's drags.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Mutex;

use super::{DragPosition, MachineId, ProgramCommand, TaggedEvent, TerminalEvent};
use crate::core::element::Key;
use crate::style::Rect;

/// An event from the host terminal, with the widgets it happened over.
#[derive(Clone, Debug, PartialEq)]
pub struct HostEvent {
    /// The `i` key the host tagged the event with.
    pub client_id: Option<u32>,
    /// The event.
    pub event: TerminalEvent,
    /// For an event with a position (a drag over the window, a drop, a drag gesture): the keyed
    /// widgets under it, innermost first. Empty otherwise.
    pub targets: Vec<HostTarget>,
}

impl HostEvent {
    /// The innermost target whose key satisfies `matches`.
    pub fn target(&self, matches: impl Fn(&Key) -> bool) -> Option<&HostTarget> {
        self.targets.iter().find(|target| matches(&target.key))
    }
}

/// A keyed widget under a host drag, and where the drag is within it.
#[derive(Clone, Debug, PartialEq)]
pub struct HostTarget {
    /// The widget's key.
    pub key: Key,
    /// Where the widget is, in window cells.
    pub rect: Rect,
    /// The pointer, in the widget's own cells and pixels.
    pub position: DragPosition,
}

/// The pointer position an event carries, in window cells and pixels.
fn event_position(event: &TerminalEvent) -> Option<DragPosition> {
    match event {
        TerminalEvent::DragOver { position, .. }
        | TerminalEvent::Drop { position, .. }
        | TerminalEvent::DragGesture { position } => Some(*position),
        _ => None,
    }
}

/// Attach the keyed widgets under the event's position, as `keyed_at` finds them.
///
/// `cell` is the host's cell size in pixels when known; otherwise it is estimated from the event.
pub(crate) fn host_event(
    tagged: TaggedEvent,
    cell: Option<(u16, u16)>,
    keyed_at: impl Fn(u16, u16) -> Vec<(Key, Rect)>,
) -> HostEvent {
    let targets = event_position(&tagged.event)
        .map(|position| {
            let (width, height) = cell.map_or_else(
                || {
                    (
                        estimate(position.pixel_x, position.cell_x),
                        estimate(position.pixel_y, position.cell_y),
                    )
                },
                |(width, height)| (i32::from(width), i32::from(height)),
            );
            let x = u16::try_from(position.cell_x).unwrap_or(u16::MAX);
            let y = u16::try_from(position.cell_y).unwrap_or(u16::MAX);
            keyed_at(x, y)
                .into_iter()
                .map(|(key, rect)| {
                    let left = rect.x.max(0) as u32;
                    let top = rect.y.max(0) as u32;
                    HostTarget {
                        key,
                        rect,
                        position: DragPosition {
                            cell_x: position.cell_x.saturating_sub(left),
                            cell_y: position.cell_y.saturating_sub(top),
                            pixel_x: (position.pixel_x - left as i32 * width).max(0),
                            pixel_y: (position.pixel_y - top as i32 * height).max(0),
                        },
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    HostEvent {
        client_id: tagged.client_id,
        event: tagged.event,
        targets,
    }
}

fn estimate(pixels: i32, cells: u32) -> i32 {
    if cells == 0 { 0 } else { pixels / cells as i32 }
}

/// The runtime's end of the channel, shared through the runtime environment.
#[derive(Debug, Default)]
pub(crate) struct HostChannel {
    supported: Cell<bool>,
    outbox: RefCell<Vec<u8>>,
}

impl HostChannel {
    pub(crate) fn set_supported(&self, supported: bool) {
        self.supported.set(supported);
    }

    /// Everything queued since the last call.
    pub(crate) fn take_output(&self) -> Vec<u8> {
        std::mem::take(&mut self.outbox.borrow_mut())
    }

    pub(crate) fn has_output(&self) -> bool {
        !self.outbox.borrow().is_empty()
    }
}

/// What the app has registered with the host, kept process-wide so the exit and handoff paths,
/// which may run off the UI thread, can withdraw it.
#[derive(Debug, Default)]
struct Registrations {
    /// Escape codes that registered for drops, replayed to register again.
    accepting: Option<Vec<u8>>,
    /// Escape codes that registered to offer drags.
    offering: Option<Vec<u8>>,
    /// Withdrawn for a handoff, to be restored on return.
    withdrawn: bool,
}

static REGISTRATIONS: Mutex<Registrations> = Mutex::new(Registrations {
    accepting: None,
    offering: None,
    withdrawn: false,
});

impl Registrations {
    fn note(&mut self, command: &ProgramCommand, encoded: &[u8]) {
        match command {
            ProgramCommand::AcceptDrops { .. } => self.accepting = Some(encoded.to_vec()),
            ProgramCommand::DropMachineId(_) => {
                if let Some(accepting) = &mut self.accepting {
                    accepting.extend_from_slice(encoded);
                }
            }
            ProgramCommand::StopAcceptingDrops => self.accepting = None,
            ProgramCommand::OfferDrags { .. } => self.offering = Some(encoded.to_vec()),
            ProgramCommand::StopOfferingDrags => self.offering = None,
            _ => {}
        }
    }

    fn withdraw(&mut self, exiting: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if !self.withdrawn {
            if self.accepting.is_some() {
                ProgramCommand::StopAcceptingDrops.encode(None, &mut out);
            }
            if self.offering.is_some() {
                ProgramCommand::StopOfferingDrags.encode(None, &mut out);
            }
        }
        if exiting {
            *self = Self::default();
        } else {
            self.withdrawn = true;
        }
        out
    }

    fn restore(&mut self) -> Vec<u8> {
        if !std::mem::take(&mut self.withdrawn) {
            return Vec::new();
        }
        let mut out = self.accepting.clone().unwrap_or_default();
        out.extend(self.offering.iter().flatten());
        out
    }
}

fn note_sent(command: &ProgramCommand, encoded: &[u8]) {
    if let Ok(mut registrations) = REGISTRATIONS.lock() {
        registrations.note(command, encoded);
    }
}

/// The escape codes that withdraw every registration, for a handoff or an exit.
///
/// After a handoff the registrations come back with [`restore_registrations`]; after an exit they
/// are forgotten.
pub(crate) fn withdraw_registrations(exiting: bool) -> Vec<u8> {
    REGISTRATIONS
        .lock()
        .map(|mut registrations| registrations.withdraw(exiting))
        .unwrap_or_default()
}

/// The escape codes that register again after a handoff.
pub(crate) fn restore_registrations() -> Vec<u8> {
    REGISTRATIONS
        .lock()
        .map(|mut registrations| registrations.restore())
        .unwrap_or_default()
}

/// Talks to the host terminal on the app's behalf.
///
/// Commands are queued and written between frames, in order. While the host does not speak the
/// protocol they are dropped, so an app can register unconditionally.
#[derive(Clone, Debug)]
pub struct HostDragAndDrop {
    channel: Rc<HostChannel>,
}

impl HostDragAndDrop {
    pub(crate) fn new(channel: Rc<HostChannel>) -> Self {
        Self { channel }
    }

    /// Whether the host answered the startup query: drags and drops can reach the app.
    pub fn supported(&self) -> bool {
        self.channel.supported.get()
    }

    /// Queue a command for the host.
    pub fn send(&self, command: &ProgramCommand) {
        if !self.supported() {
            return;
        }
        let mut encoded = Vec::new();
        command.encode(None, &mut encoded);
        note_sent(command, &encoded);
        self.channel.outbox.borrow_mut().extend_from_slice(&encoded);
    }

    /// Queue escape codes already encoded, such as a [`DataStreamWriter`](super::DataStreamWriter)
    /// produces. Anything other than OSC 72 escape codes is not written.
    pub fn send_encoded(&self, escape_codes: &[u8]) {
        if !self.supported() {
            return;
        }
        let mut decoder = super::CommandDecoder::default();
        let mut outbox = self.channel.outbox.borrow_mut();
        for (_, body) in super::OscScanner::default().scan(escape_codes) {
            let mut code = Vec::with_capacity(body.len() + 7);
            code.extend_from_slice(b"\x1b]72;");
            code.extend_from_slice(&body);
            code.extend_from_slice(b"\x1b\\");
            for tagged in decoder.push_body(&body) {
                note_sent(&tagged.command, &code);
            }
            outbox.extend_from_slice(&code);
        }
    }

    /// This machine's id, to register with so the host can tell a drop from another machine.
    pub fn machine_id(&self) -> Option<MachineId> {
        MachineId::local()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel() -> (Rc<HostChannel>, HostDragAndDrop) {
        let channel = Rc::new(HostChannel::default());
        channel.set_supported(true);
        (Rc::clone(&channel), HostDragAndDrop::new(channel))
    }

    #[test]
    fn nothing_is_queued_for_a_host_without_the_protocol() {
        let channel = Rc::new(HostChannel::default());
        HostDragAndDrop::new(Rc::clone(&channel)).send(&ProgramCommand::Query);
        assert!(!channel.has_output());
    }

    #[test]
    fn registrations_are_withdrawn_for_a_handoff_and_come_back() {
        let mut registrations = Registrations::default();
        let mut accept = Vec::new();
        ProgramCommand::AcceptDrops { mimes: vec![] }.encode(None, &mut accept);
        registrations.note(&ProgramCommand::AcceptDrops { mimes: vec![] }, &accept);
        let mut offer = Vec::new();
        ProgramCommand::OfferDrags { machine_id: None }.encode(None, &mut offer);
        registrations.note(&ProgramCommand::OfferDrags { machine_id: None }, &offer);
        assert_eq!(
            registrations.withdraw(false),
            b"\x1b]72;t=A\x1b\\\x1b]72;t=o:x=2\x1b\\"
        );
        assert_eq!(registrations.withdraw(false), b"", "already withdrawn");
        assert_eq!(registrations.restore(), [accept.clone(), offer].concat());
        assert_eq!(registrations.restore(), b"", "already restored");
        registrations.note(&ProgramCommand::StopOfferingDrags, b"");
        assert_eq!(registrations.withdraw(true), b"\x1b]72;t=A\x1b\\");
        assert_eq!(registrations.restore(), b"", "an exit forgets");
    }

    #[test]
    fn encoded_input_keeps_only_whole_drag_and_drop_codes() {
        let (channel, host) = channel();
        host.send_encoded(b"junk\x1b]72;t=r:x=1\x1b\\\x1b]2;title\x07\x1b]72;t=A\x1b\\tail");
        assert_eq!(
            channel.take_output(),
            b"\x1b]72;t=r:x=1\x1b\\\x1b]72;t=A\x1b\\"
        );
    }
}
