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
use std::io::{self, Write};
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
    /// Whether the host has the registrations, as far as can be known.
    host: HostHas,
}

/// What the host has of the app's registrations.
///
/// A failed write does not prove nothing got through: bytes can be written and the flush fail, or
/// one escape code arrive and the next not. So a failure leaves the host's state uncertain, and
/// uncertain is treated as both: a handoff withdraws, and the app registers again.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum HostHas {
    /// The host has every registration.
    #[default]
    All,
    /// A handoff withdrew them all, and the withdrawal was written.
    Nothing,
    /// A withdrawal or a restore failed part way, or a registration reached a host that had been
    /// withdrawn from: some may have reached the host, some not.
    Uncertain,
}

static REGISTRATIONS: Mutex<Registrations> = Mutex::new(Registrations {
    accepting: None,
    drop_machine_id: None,
    offering: None,
    host: HostHas::All,
});

impl Registrations {
    fn note(&mut self, command: &ProgramCommand) {
        // A registration that reaches a host the app had withdrawn from leaves it with part of
        // them: the next handoff must withdraw, and the rest must come back. Withdrawing something
        // the host no longer has costs one redundant escape code; missing one would leak it.
        if self.host == HostHas::Nothing
            && matches!(
                command,
                ProgramCommand::AcceptDrops { .. } | ProgramCommand::OfferDrags { .. }
            )
        {
            self.host = HostHas::Uncertain;
        }
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

    /// The escape codes that withdraw what the host may have, unless it has none for certain.
    fn withdrawal(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.host != HostHas::Nothing {
            if self.accepting.is_some() {
                ProgramCommand::StopAcceptingDrops.encode(None, &mut out);
            }
            if self.offering.is_some() {
                ProgramCommand::StopOfferingDrags.encode(None, &mut out);
            }
        }
        out
    }

    /// The escape codes that register again what the host may lack, unless it has all for certain.
    fn restoration(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.host == HostHas::All {
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

fn locked<R>(registrations: &Mutex<Registrations>, f: impl FnOnce(&mut Registrations) -> R) -> R {
    let mut guard = registrations
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut guard)
}

fn write_all(out: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    if bytes.is_empty() {
        return Ok(());
    }
    out.write_all(bytes)?;
    out.flush()
}

/// Run `handoff` with the app's registrations withdrawn from the host, so the program it hands the
/// terminal to never receives the app's drags.
///
/// The registrations count as withdrawn only once the withdrawal has been written. If writing it
/// fails, `handoff` does not run, and the host's state is uncertain until the runner restores the
/// registrations. If `handoff` fails, the app keeps the terminal, so the registrations are written
/// again before the error is returned.
pub(crate) fn withdrawn_for<W: Write, T>(
    out: &mut W,
    handoff: impl FnOnce(&mut W) -> io::Result<T>,
) -> io::Result<T> {
    withdraw_around(&REGISTRATIONS, out, handoff)
}

fn withdraw_around<W: Write, T>(
    registrations: &Mutex<Registrations>,
    out: &mut W,
    handoff: impl FnOnce(&mut W) -> io::Result<T>,
) -> io::Result<T> {
    let withdrawal = locked(registrations, |r| r.withdrawal());
    if let Err(err) = write_all(out, &withdrawal) {
        locked(registrations, |r| r.host = HostHas::Uncertain);
        return Err(err);
    }
    locked(registrations, |r| r.host = HostHas::Nothing);
    match handoff(out) {
        Ok(value) => Ok(value),
        Err(err) => {
            let _ = restore_into(registrations, out);
            Err(err)
        }
    }
}

/// Register again what a handoff withdrew. They count as restored only once written.
pub(crate) fn restore_registrations(out: &mut impl Write) -> io::Result<()> {
    restore_into(&REGISTRATIONS, out)
}

/// Retry a restore that failed after a handoff, before anything else reaches the host.
///
/// The runner calls this only while it owns the terminal, so registrations it finds withdrawn or
/// uncertain then are ones whose restore did not get written.
pub(crate) fn retry_restore(out: &mut impl Write) -> io::Result<()> {
    restore_into(&REGISTRATIONS, out)
}

fn restore_into(registrations: &Mutex<Registrations>, out: &mut impl Write) -> io::Result<()> {
    let restoration = locked(registrations, |r| r.restoration());
    let written = write_all(out, &restoration);
    locked(registrations, |r| {
        r.host = if written.is_ok() {
            HostHas::All
        } else {
            HostHas::Uncertain
        };
    });
    written
}

/// The escape codes that withdraw every registration when the app exits, after which they are
/// forgotten.
pub(crate) fn withdraw_for_exit() -> Vec<u8> {
    locked(&REGISTRATIONS, |r| {
        let out = r.withdrawal();
        *r = Registrations::default();
        out
    })
}

/// Note escape codes that have been written to the host.
pub(crate) fn commit_registrations(escape_codes: &[u8]) {
    if let Ok(mut registrations) = REGISTRATIONS.lock() {
        registrations.commit(escape_codes);
    }
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

    fn registered(commands: &[ProgramCommand]) -> Mutex<Registrations> {
        let mut registrations = Registrations::default();
        registrations.commit(&encoded(commands));
        Mutex::new(registrations)
    }

    fn handoff(
        registrations: &Mutex<Registrations>,
        result: io::Result<()>,
    ) -> (io::Result<()>, Vec<u8>) {
        let mut out = Vec::new();
        let result = withdraw_around(registrations, &mut out, |_| result);
        (result, out)
    }

    #[test]
    fn registrations_are_withdrawn_for_a_handoff_and_come_back() {
        let offer = ProgramCommand::OfferDrags {
            machine_id: Some("1:abcdef0123456789abcd".into()),
        };
        let registrations = registered(&[accept(&["image/png"]), offer.clone()]);
        let (result, out) = handoff(&registrations, Ok(()));
        assert!(result.is_ok());
        assert_eq!(out, b"\x1b]72;t=A\x1b\\\x1b]72;t=o:x=2\x1b\\");
        assert_eq!(handoff(&registrations, Ok(())).1, b"", "already withdrawn");
        let mut out = Vec::new();
        restore_into(&registrations, &mut out).unwrap();
        assert_eq!(out, encoded(&[accept(&["image/png"]), offer]));
        let mut out = Vec::new();
        restore_into(&registrations, &mut out).unwrap();
        assert_eq!(out, b"", "already restored");
    }

    #[test]
    fn a_failed_handoff_registers_again_and_the_next_one_withdraws() {
        let registrations = registered(&[accept(&[])]);
        let (result, out) = handoff(&registrations, Err(io::Error::other("suspend failed")));
        assert!(result.is_err());
        assert_eq!(
            out,
            [b"\x1b]72;t=A\x1b\\".to_vec(), encoded(&[accept(&[])])].concat()
        );
        assert_eq!(locked(&registrations, |r| r.host), HostHas::All);
        let (_, out) = handoff(&registrations, Ok(()));
        assert_eq!(
            out, b"\x1b]72;t=A\x1b\\",
            "the next handoff still withdraws"
        );
    }

    struct Broken;
    impl Write for Broken {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("gone"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_registration_after_a_failed_restore_is_withdrawn_by_the_next_handoff() {
        let registrations = registered(&[accept(&["a/b"])]);
        handoff(&registrations, Ok(())).0.unwrap();
        assert!(restore_into(&registrations, &mut Broken).is_err());
        assert_eq!(locked(&registrations, |r| r.host), HostHas::Uncertain);
        // A queued registration reaches the host normally.
        locked(&registrations, |r| r.commit(&encoded(&[accept(&["c/d"])])));
        let (_, out) = handoff(&registrations, Ok(()));
        assert_eq!(
            out, b"\x1b]72;t=A\x1b\\",
            "the live registration is withdrawn"
        );
    }

    #[test]
    fn a_failed_restore_is_retried_whole() {
        let offer = ProgramCommand::OfferDrags { machine_id: None };
        let registrations = registered(&[accept(&[]), offer.clone()]);
        handoff(&registrations, Ok(())).0.unwrap();
        assert!(restore_into(&registrations, &mut Broken).is_err());
        let mut out = Vec::new();
        restore_into(&registrations, &mut out).unwrap();
        assert_eq!(out, encoded(&[accept(&[]), offer]));
        assert_eq!(locked(&registrations, |r| r.host), HostHas::All);
    }

    /// Takes the first `accepted` bytes, then fails; `flush` always fails.
    struct Partial {
        accepted: usize,
        written: Vec<u8>,
    }

    impl Write for Partial {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let room = self.accepted.saturating_sub(self.written.len());
            if room == 0 {
                return Err(io::Error::other("gone"));
            }
            let taken = room.min(bytes.len());
            self.written.extend_from_slice(&bytes[..taken]);
            Ok(taken)
        }
        fn flush(&mut self) -> io::Result<()> {
            Err(io::Error::other("flush failed"))
        }
    }

    #[test]
    fn a_restore_that_may_have_reached_the_host_is_withdrawn_by_the_next_handoff() {
        let offer = ProgramCommand::OfferDrags { machine_id: None };
        let whole = encoded(&[accept(&[]), offer.clone()]).len();
        // All written but the flush failed; the first registration only; nothing at all.
        for accepted in [whole, encoded(&[accept(&[])]).len(), 0] {
            let registrations = registered(&[accept(&[]), offer.clone()]);
            handoff(&registrations, Ok(())).0.unwrap();
            let mut out = Partial {
                accepted,
                written: Vec::new(),
            };
            assert!(restore_into(&registrations, &mut out).is_err());
            let (_, withdrawal) = handoff(&registrations, Ok(()));
            assert_eq!(
                withdrawal, b"\x1b]72;t=A\x1b\\\x1b]72;t=o:x=2\x1b\\",
                "accepted {accepted} bytes: the next handoff withdraws everything"
            );
        }
    }

    #[test]
    fn a_withdrawal_that_may_have_half_reached_the_host_is_restored() {
        let offer = ProgramCommand::OfferDrags { machine_id: None };
        let registrations = registered(&[accept(&[]), offer.clone()]);
        let mut out = Partial {
            accepted: 12,
            written: Vec::new(),
        };
        assert!(withdraw_around(&registrations, &mut out, |_| Ok(())).is_err());
        assert_eq!(locked(&registrations, |r| r.host), HostHas::Uncertain);
        let mut retried = Vec::new();
        restore_into(&registrations, &mut retried).unwrap();
        assert_eq!(
            retried,
            encoded(&[accept(&[]), offer]),
            "the runner registers again"
        );
    }

    #[test]
    fn a_withdrawal_that_cannot_be_written_stops_the_handoff() {
        let registrations = registered(&[accept(&[])]);
        let mut ran = false;
        let result = withdraw_around(&registrations, &mut Broken, |_| {
            ran = true;
            Ok(())
        });
        assert!(result.is_err());
        assert!(
            !ran,
            "the terminal is not handed over while the app is still registered"
        );
        assert_eq!(locked(&registrations, |r| r.host), HostHas::Uncertain);
        assert!(
            !handoff(&registrations, Ok(())).1.is_empty(),
            "and the next try withdraws"
        );
    }

    #[test]
    fn a_withdrawal_follows_what_reached_the_host_not_what_is_queued() {
        let (channel, host) = channel();
        let registrations = Mutex::new(Registrations::default());
        host.send(&accept(&[]));
        locked(&registrations, |r| r.commit(&channel.take_output()));
        // Queued, then a handoff before the next flush: the host still has the registration.
        host.send(&ProgramCommand::StopAcceptingDrops);
        assert_eq!(handoff(&registrations, Ok(())).1, b"\x1b]72;t=A\x1b\\");
        // On return it registers again, and the queued withdrawal then reaches the host.
        let mut out = Vec::new();
        restore_into(&registrations, &mut out).unwrap();
        assert_eq!(out, encoded(&[accept(&[])]));
        locked(&registrations, |r| r.commit(&channel.take_output()));
        assert_eq!(*registrations.lock().unwrap(), Registrations::default());
    }

    #[test]
    fn a_new_registration_keeps_the_machine_id_across_a_handoff() {
        let id = ProgramCommand::DropMachineId("1:abcdef0123456789abcd".into());
        let registrations = registered(&[accept(&["a/b"]), id.clone(), accept(&["c/d"])]);
        handoff(&registrations, Ok(())).0.unwrap();
        let mut out = Vec::new();
        restore_into(&registrations, &mut out).unwrap();
        assert_eq!(out, encoded(&[accept(&["c/d"]), id]));
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
