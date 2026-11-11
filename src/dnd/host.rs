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
use std::sync::{Arc, Mutex};

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

/// What the host has been told the app registered, kept process-wide so the exit and handoff
/// paths, which may run off the UI thread, can withdraw it.
///
/// It follows what reached the host, not what was queued: a command still waiting in the outbox
/// when a handoff withdraws the registrations has not changed what the host knows.
#[derive(Debug, Default, PartialEq, Eq)]
struct Registrations {
    /// Accepting drops, with these extra MIME types.
    accepting: Option<Vec<Arc<str>>>,
    /// The machine id sent for drops. It outlives a new `AcceptDrops`, as it does in the terminal.
    drop_machine_id: Option<Arc<str>>,
    /// Offering drags, with this machine id.
    offering: Option<Option<Arc<str>>>,
    /// Withdrawn for a handoff, to be restored on return.
    withdrawn: bool,
}

static REGISTRATIONS: Mutex<Registrations> = Mutex::new(Registrations {
    accepting: None,
    drop_machine_id: None,
    offering: None,
    withdrawn: false,
});

impl Registrations {
    fn note(&mut self, command: &ProgramCommand) {
        match command {
            ProgramCommand::AcceptDrops { mimes } => self.accepting = Some(mimes.clone()),
            ProgramCommand::DropMachineId(id) => {
                self.drop_machine_id = (!id.is_empty()).then(|| Arc::clone(id));
            }
            ProgramCommand::StopAcceptingDrops => {
                self.accepting = None;
                self.drop_machine_id = None;
            }
            ProgramCommand::OfferDrags { machine_id } => self.offering = Some(machine_id.clone()),
            ProgramCommand::StopOfferingDrags => self.offering = None,
            _ => {}
        }
    }

    /// Record escape codes the host has been sent.
    fn commit(&mut self, escape_codes: &[u8]) {
        let mut decoder = super::CommandDecoder::default();
        for (_, body) in super::OscScanner::default().scan(escape_codes) {
            for tagged in decoder.push_body(&body) {
                self.note(&tagged.command);
            }
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
        let mut out = Vec::new();
        if !std::mem::take(&mut self.withdrawn) {
            return out;
        }
        if let Some(mimes) = &self.accepting {
            ProgramCommand::AcceptDrops {
                mimes: mimes.clone(),
            }
            .encode(None, &mut out);
            if let Some(id) = &self.drop_machine_id {
                ProgramCommand::DropMachineId(Arc::clone(id)).encode(None, &mut out);
            }
        }
        if let Some(machine_id) = &self.offering {
            ProgramCommand::OfferDrags {
                machine_id: machine_id.clone(),
            }
            .encode(None, &mut out);
        }
        out
    }
}

/// Note escape codes that have been written to the host.
pub(crate) fn commit_registrations(escape_codes: &[u8]) {
    if let Ok(mut registrations) = REGISTRATIONS.lock() {
        registrations.commit(escape_codes);
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
        command.encode(None, &mut self.channel.outbox.borrow_mut());
    }

    /// Queue escape codes already encoded, such as a [`DataStreamWriter`](super::DataStreamWriter)
    /// produces. Anything other than OSC 72 escape codes is not written.
    pub fn send_encoded(&self, escape_codes: &[u8]) {
        if !self.supported() {
            return;
        }
        let mut outbox = self.channel.outbox.borrow_mut();
        for (_, body) in super::OscScanner::default().scan(escape_codes) {
            if let Ok(frame) = super::wire::Frame::parse(&body) {
                frame.write(&mut outbox);
            }
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

    fn encoded(commands: &[ProgramCommand]) -> Vec<u8> {
        let mut out = Vec::new();
        for command in commands {
            command.encode(None, &mut out);
        }
        out
    }

    fn accept(mimes: &[&str]) -> ProgramCommand {
        ProgramCommand::AcceptDrops {
            mimes: mimes.iter().map(|m| Arc::from(*m)).collect(),
        }
    }

    #[test]
    fn registrations_are_withdrawn_for_a_handoff_and_come_back() {
        let mut registrations = Registrations::default();
        let offer = ProgramCommand::OfferDrags {
            machine_id: Some("1:abcdef0123456789abcd".into()),
        };
        registrations.commit(&encoded(&[accept(&["image/png"]), offer.clone()]));
        assert_eq!(
            registrations.withdraw(false),
            b"\x1b]72;t=A\x1b\\\x1b]72;t=o:x=2\x1b\\"
        );
        assert_eq!(registrations.withdraw(false), b"", "already withdrawn");
        assert_eq!(
            registrations.restore(),
            encoded(&[accept(&["image/png"]), offer])
        );
        assert_eq!(registrations.restore(), b"", "already restored");
        registrations.commit(&encoded(&[ProgramCommand::StopOfferingDrags]));
        assert_eq!(registrations.withdraw(true), b"\x1b]72;t=A\x1b\\");
        assert_eq!(registrations.restore(), b"", "an exit forgets");
    }

    #[test]
    fn a_withdrawal_follows_what_reached_the_host_not_what_is_queued() {
        let (channel, host) = channel();
        let mut registrations = Registrations::default();
        host.send(&accept(&[]));
        registrations.commit(&channel.take_output());
        // Queued, then a handoff before the next flush: the host still has the registration.
        host.send(&ProgramCommand::StopAcceptingDrops);
        assert_eq!(registrations.withdraw(false), b"\x1b]72;t=A\x1b\\");
        // On return it registers again, and the queued withdrawal then reaches the host.
        assert_eq!(registrations.restore(), encoded(&[accept(&[])]));
        registrations.commit(&channel.take_output());
        assert_eq!(registrations, Registrations::default());
    }

    #[test]
    fn a_new_registration_keeps_the_machine_id_across_a_handoff() {
        let mut registrations = Registrations::default();
        let id = ProgramCommand::DropMachineId("1:abcdef0123456789abcd".into());
        registrations.commit(&encoded(&[accept(&["a/b"]), id.clone(), accept(&["c/d"])]));
        registrations.withdraw(false);
        assert_eq!(registrations.restore(), encoded(&[accept(&["c/d"]), id]));
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
