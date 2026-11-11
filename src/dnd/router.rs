//! Drag and drop for an app that shows other programs: a multiplexer, an embedded terminal.
//!
//! The host terminal sees one program, the app; the app shows several children, each of which may
//! speak the protocol itself. [`Router`] sits between them. It sends a drag to the child under the
//! pointer, in that child's own coordinates, and the child's answers back to the host. Data the
//! child asks for comes from the host, and only the child that took the drop may ask. A drag the
//! child starts goes out through the host, and the host's progress reports come back to it.
//!
//! A child that never registered for drops still takes text and files: the router fetches the
//! dropped paths or text itself and hands them to the app as an [`Output::Paste`], which is what
//! a terminal does for a program that does not speak the protocol.
//!
//! The router does no I/O. Feed it host events and child commands; write what it returns.

use std::collections::HashMap;
use std::fmt::Debug;
use std::hash::Hash;
use std::sync::Arc;

use super::{
    ChildRegistration, DataStreamWriter, DataTarget, DndError, DragPosition, DropOperation,
    DropOperations, ErrorCode, MachineId, ProgramCommand, TEXT_PLAIN, TEXT_PLAIN_UTF8,
    TaggedCommand, TaggedEvent, TerminalEvent, URI_LIST, data_meta,
};

/// The most data a drop on a child that does not speak the protocol may carry.
pub const MAX_PASTE: usize = 4 << 20;

/// The types a child that does not speak the protocol takes, most preferred first.
const PASTE_TYPES: [&str; 3] = [URI_LIST, TEXT_PLAIN_UTF8, TEXT_PLAIN];

/// The child under a point of the window, as the app lays its children out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit<K> {
    /// Which child.
    pub key: K,
    /// The point in the child's own cells and pixels.
    pub position: DragPosition,
    /// What the child registered.
    pub registration: ChildRegistration,
}

/// Something the router needs written or done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Output<K> {
    /// Escape codes for the host; see [`HostDragAndDrop::send_encoded`](super::HostDragAndDrop::send_encoded).
    Host(Vec<u8>),
    /// Escape codes for a child's input.
    Child(K, Vec<u8>),
    /// A drop on a child that does not speak the protocol: paste `data`, whose type is `mime`.
    Paste {
        /// The child dropped on.
        key: K,
        /// [`URI_LIST`], [`TEXT_PLAIN_UTF8`] or [`TEXT_PLAIN`].
        mime: Arc<str>,
        /// What was dropped.
        data: Vec<u8>,
    },
    /// The child a drag is over changed; `None` when it is over none.
    Hover(Option<K>),
}

#[derive(Clone, Debug)]
struct Hover<K> {
    key: K,
    client_id: Option<u32>,
    /// The child registered for drops; otherwise the router answers for it.
    speaks: bool,
}

#[derive(Debug)]
struct Dropped<K> {
    key: K,
    client_id: Option<u32>,
    /// The child runs on another machine than the app.
    remote: bool,
    mimes: Vec<Arc<str>>,
    streams: HashMap<DataTarget, DataStreamWriter>,
}

#[derive(Debug)]
struct Pasting<K> {
    key: K,
    mime: Arc<str>,
    data: Vec<u8>,
}

#[derive(Debug)]
struct Source<K> {
    key: K,
    client_id: Option<u32>,
    streams: HashMap<SourceStream, DataStreamWriter>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SourceStream {
    Present(u32),
    Image(u32),
    Drag(u32),
    Remote(u32, Option<(u32, u32)>),
}

/// Routes drags and drops between the host terminal and the children an app shows.
#[derive(Debug)]
pub struct Router<K> {
    local_machine: Option<MachineId>,
    host_mimes: Vec<Arc<str>>,
    host_operations: DropOperations,
    host_status: Option<(DropOperation, Option<Vec<Arc<str>>>)>,
    hover: Option<Hover<K>>,
    dropped: Option<Dropped<K>>,
    pasting: Option<Pasting<K>>,
    gesture: Option<(K, Option<u32>)>,
    source: Option<Source<K>>,
}

impl<K: Clone + Eq + Hash + Debug> Router<K> {
    /// A router for an app on the machine `local_machine` names.
    pub fn new(local_machine: Option<MachineId>) -> Self {
        Self {
            local_machine,
            host_mimes: Vec::new(),
            host_operations: DropOperations::default(),
            host_status: None,
            hover: None,
            dropped: None,
            pasting: None,
            gesture: None,
            source: None,
        }
    }

    /// The child a drag is over.
    pub fn hovered(&self) -> Option<&K> {
        self.hover.as_ref().map(|hover| &hover.key)
    }

    /// Every child the router is in the middle of something with: the one a drag is over, the
    /// one reading a drop, the one offering a drag. An app checks these still exist and calls
    /// [`Self::forget_child`] for any that has gone.
    pub fn children(&self) -> Vec<K> {
        let mut children: Vec<K> = Vec::new();
        let mut add = |key: &K| {
            if !children.contains(key) {
                children.push(key.clone());
            }
        };
        if let Some(hover) = &self.hover {
            add(&hover.key);
        }
        if let Some(dropped) = &self.dropped {
            add(&dropped.key);
        }
        if let Some(pasting) = &self.pasting {
            add(&pasting.key);
        }
        if let Some((gesture, _)) = &self.gesture {
            add(gesture);
        }
        if let Some(source) = &self.source {
            add(&source.key);
        }
        children
    }

    /// Whether a drop is still being read, or a drag a child started is still going.
    pub fn busy(&self) -> bool {
        self.dropped.is_some() || self.pasting.is_some() || self.source.is_some()
    }

    /// Handle an event from the host. `locate` names the child at a point of the window.
    pub fn host_event(
        &mut self,
        event: TaggedEvent,
        locate: impl Fn(DragPosition) -> Option<Hit<K>>,
    ) -> Vec<Output<K>> {
        let mut out = Vec::new();
        match event.event {
            TerminalEvent::Supported { .. } => {}
            TerminalEvent::DragOver {
                position,
                operations,
                mimes,
            } => {
                if self.hover.is_none() {
                    // A new drag. Whatever an earlier drop left unfinished, the host has dropped.
                    self.end_dropped();
                }
                if let Some(mimes) = mimes {
                    self.host_mimes = mimes;
                }
                self.host_operations = operations;
                self.drag_over(locate(position), &mut out);
            }
            TerminalEvent::DragLeave => {
                self.leave_hovered(&mut out);
                self.host_mimes.clear();
                self.host_status = None;
            }
            TerminalEvent::Drop {
                position,
                operations,
                mimes,
            } => {
                self.host_mimes = mimes;
                self.host_operations = operations;
                self.drop(locate(position), &mut out);
            }
            TerminalEvent::Data {
                target,
                kind,
                remote,
                bytes,
                end,
            } => self.data(target, kind, remote, bytes, end, &mut out),
            TerminalEvent::DataError { target, error } => {
                if self.pasting.take().is_some() {
                    finish_host(DropOperation::None, &mut out);
                } else if let Some(dropped) = &mut self.dropped {
                    if let Some(target) = target {
                        dropped.streams.remove(&target);
                    }
                    let event = TerminalEvent::DataError { target, error };
                    out.push(child(&dropped.key, dropped.client_id, &event));
                }
            }
            TerminalEvent::DragGesture { position } => {
                self.source = None;
                self.gesture = None;
                if let Some(hit) = locate(position)
                    && let Some(drags) = &hit.registration.drags
                {
                    let event = TerminalEvent::DragGesture {
                        position: hit.position,
                    };
                    out.push(child(&hit.key, drags.client_id, &event));
                    self.gesture = Some((hit.key, drags.client_id));
                }
            }
            event @ (TerminalEvent::DragStarted
            | TerminalEvent::DragFailed(_)
            | TerminalEvent::DragAccepted { .. }
            | TerminalEvent::DragActionChanged { .. }
            | TerminalEvent::DragDropped
            | TerminalEvent::DragFinished { .. }
            | TerminalEvent::DragDataRequested { .. }
            | TerminalEvent::RemoteEntryRequested { .. }) => {
                let ends = matches!(
                    event,
                    TerminalEvent::DragFailed(_) | TerminalEvent::DragFinished { .. }
                );
                if let Some(source) = &self.source {
                    out.push(child(&source.key, source.client_id, &event));
                }
                if ends {
                    self.source = None;
                }
            }
        }
        out
    }

    /// Handle a command from the child `key`, which has registered `registration`.
    pub fn child_command(
        &mut self,
        key: &K,
        registration: &ChildRegistration,
        command: TaggedCommand,
    ) -> Vec<Output<K>> {
        let mut out = Vec::new();
        let client_id = command
            .client_id
            .or_else(|| registration.drops.as_ref().and_then(|d| d.client_id))
            .or_else(|| registration.drags.as_ref().and_then(|d| d.client_id));
        match command.command {
            ProgramCommand::DropStatus { operation, mimes } => {
                if self
                    .hover
                    .as_ref()
                    .is_some_and(|hover| hover.speaks && &hover.key == key)
                {
                    self.set_host_status(operation, mimes, &mut out);
                }
            }
            ProgramCommand::RequestData(target) => match &self.dropped {
                Some(dropped) if &dropped.key == key => {
                    out.push(host(&ProgramCommand::RequestData(target)));
                }
                _ => out.push(child(
                    key,
                    client_id,
                    &TerminalEvent::DataError {
                        target: Some(target),
                        error: DndError::with_description(
                            ErrorCode::PermissionDenied,
                            "drop data can only be requested after a drop",
                        ),
                    },
                )),
            },
            command @ ProgramCommand::ReleaseDirectory { .. } => {
                if self.dropped.as_ref().is_some_and(|d| &d.key == key) {
                    out.push(host(&command));
                }
            }
            ProgramCommand::FinishDrop { operation } => {
                if self.dropped.as_ref().is_some_and(|d| &d.key == key) {
                    self.dropped = None;
                    finish_host(operation, &mut out);
                }
            }
            command @ ProgramCommand::Offer { .. } => match self.gesture.take() {
                Some((gesture, client)) if &gesture == key => {
                    self.source = Some(Source {
                        key: gesture,
                        client_id: client.or(client_id),
                        streams: HashMap::new(),
                    });
                    out.push(host(&command));
                }
                other => {
                    self.gesture = other;
                    out.push(child(key, client_id, &refused("no drag gesture to answer")));
                }
            },
            command => {
                if self.source.as_ref().is_some_and(|s| &s.key == key) {
                    self.source_command(command, &mut out);
                } else if matches!(command, ProgramCommand::StartDrag) {
                    out.push(child(key, client_id, &refused("no drag was offered")));
                }
            }
        }
        out
    }

    /// The child `key` has gone. Anything in progress with it ends.
    pub fn forget_child(&mut self, key: &K) -> Vec<Output<K>> {
        let mut out = Vec::new();
        if self.hover.as_ref().is_some_and(|hover| &hover.key == key) {
            self.hover = None;
            out.push(Output::Hover(None));
            self.set_host_status(DropOperation::None, None, &mut out);
        }
        if self.dropped.as_ref().is_some_and(|d| &d.key == key) {
            self.dropped = None;
            finish_host(DropOperation::None, &mut out);
        }
        if self.pasting.as_ref().is_some_and(|p| &p.key == key) {
            self.pasting = None;
            finish_host(DropOperation::None, &mut out);
        }
        if self.source.as_ref().is_some_and(|s| &s.key == key) {
            self.source = None;
            out.push(host(&ProgramCommand::CancelDrag));
        }
        if self.gesture.as_ref().is_some_and(|(g, _)| g == key) {
            self.gesture = None;
        }
        out
    }

    fn drag_over(&mut self, hit: Option<Hit<K>>, out: &mut Vec<Output<K>>) {
        let same = match (&self.hover, &hit) {
            (Some(hover), Some(hit)) => {
                hover.key == hit.key && hover.speaks == hit.registration.drops.is_some()
            }
            (None, None) => true,
            _ => false,
        };
        if !same {
            self.leave_hovered(out);
        }
        let Some(hit) = hit else {
            self.set_host_status(DropOperation::None, None, out);
            return;
        };
        let speaks = hit.registration.drops.is_some();
        let client_id = hit.registration.drops.as_ref().and_then(|d| d.client_id);
        if !same {
            self.hover = Some(Hover {
                key: hit.key.clone(),
                client_id,
                speaks,
            });
            out.push(Output::Hover(Some(hit.key.clone())));
        }
        if speaks {
            if !same {
                // Until the child answers, the host shows the drop as refused.
                self.set_host_status(DropOperation::None, None, out);
            }
            let event = TerminalEvent::DragOver {
                position: hit.position,
                operations: self.host_operations,
                mimes: Some(self.host_mimes.clone()),
            };
            out.push(child(&hit.key, client_id, &event));
        } else {
            let accepted = self.paste_types();
            let copy = self.host_operations.copy && !accepted.is_empty();
            let operation = if copy {
                DropOperation::Copy
            } else {
                DropOperation::None
            };
            self.set_host_status(operation, copy.then_some(accepted), out);
        }
    }

    fn leave_hovered(&mut self, out: &mut Vec<Output<K>>) {
        if let Some(hover) = self.hover.take() {
            if hover.speaks {
                out.push(child(
                    &hover.key,
                    hover.client_id,
                    &TerminalEvent::DragLeave,
                ));
            }
            out.push(Output::Hover(None));
        }
    }

    fn drop(&mut self, hit: Option<Hit<K>>, out: &mut Vec<Output<K>>) {
        if let Some(hover) = self.hover.take() {
            if hit.as_ref().is_none_or(|hit| hit.key != hover.key) && hover.speaks {
                out.push(child(
                    &hover.key,
                    hover.client_id,
                    &TerminalEvent::DragLeave,
                ));
            }
            out.push(Output::Hover(None));
        }
        self.host_status = None;
        self.end_dropped();
        let Some(hit) = hit else {
            finish_host(DropOperation::None, out);
            return;
        };
        if let Some(drops) = &hit.registration.drops {
            let remote = drops.machine().is_some_and(|child| {
                self.local_machine
                    .as_ref()
                    .is_none_or(|local| !local.same_machine(&child))
            });
            let event = TerminalEvent::Drop {
                position: hit.position,
                operations: self.host_operations,
                mimes: self.host_mimes.clone(),
            };
            out.push(child(&hit.key, drops.client_id, &event));
            self.dropped = Some(Dropped {
                key: hit.key,
                client_id: drops.client_id,
                remote,
                mimes: self.host_mimes.clone(),
                streams: HashMap::new(),
            });
            return;
        }
        let accepted = self.paste_types();
        match accepted.first() {
            Some(mime) if self.host_operations.copy => {
                let index = self
                    .host_mimes
                    .iter()
                    .position(|offered| offered == mime)
                    .map_or(1, |at| at as u32 + 1);
                out.push(host(&ProgramCommand::RequestData(DataTarget::Mime {
                    index,
                })));
                self.pasting = Some(Pasting {
                    key: hit.key,
                    mime: Arc::clone(mime),
                    data: Vec::new(),
                });
            }
            _ => finish_host(DropOperation::None, out),
        }
    }

    fn data(
        &mut self,
        target: DataTarget,
        kind: Option<super::EntryKind>,
        remote: bool,
        bytes: Vec<u8>,
        end: bool,
        out: &mut Vec<Output<K>>,
    ) {
        if let Some(pasting) = &mut self.pasting {
            if pasting.data.len() + bytes.len() > MAX_PASTE {
                self.pasting = None;
                finish_host(DropOperation::None, out);
                return;
            }
            pasting.data.extend_from_slice(&bytes);
            if end && let Some(pasting) = self.pasting.take() {
                out.push(Output::Paste {
                    key: pasting.key,
                    mime: pasting.mime,
                    data: pasting.data,
                });
                finish_host(DropOperation::Copy, out);
            }
            return;
        }
        let Some(dropped) = &mut self.dropped else {
            return;
        };
        let is_uri_list = matches!(target, DataTarget::Mime { index }
            if dropped.mimes.get(index as usize - 1).is_some_and(|m| m.as_ref() == URI_LIST));
        let remote = is_uri_list && (remote || dropped.remote);
        let client_id = dropped.client_id;
        let writer = dropped.streams.entry(target).or_insert_with(|| {
            DataStreamWriter::new(data_meta(target, kind, remote).with_i(client_id))
        });
        let mut encoded = Vec::new();
        writer.write(&bytes, &mut encoded);
        if end && let Some(writer) = dropped.streams.remove(&target) {
            writer.finish(&mut encoded);
        }
        if !encoded.is_empty() {
            out.push(Output::Child(dropped.key.clone(), encoded));
        }
    }

    fn source_command(&mut self, command: ProgramCommand, out: &mut Vec<Output<K>>) {
        let Some(source) = &mut self.source else {
            return;
        };
        let (stream, bytes, end) = match &command {
            ProgramCommand::PresentData {
                mime_index,
                bytes,
                end,
            } => (SourceStream::Present(*mime_index), bytes, *end),
            ProgramCommand::PresentImage {
                image, bytes, end, ..
            } => (SourceStream::Image(*image), bytes, *end),
            ProgramCommand::DragData {
                mime_index,
                bytes,
                end,
            } => (SourceStream::Drag(*mime_index), bytes, *end),
            ProgramCommand::RemoteEntry {
                entry,
                child,
                bytes,
                end,
                ..
            } => (SourceStream::Remote(*entry, *child), bytes, *end),
            ProgramCommand::CancelDrag | ProgramCommand::DragError(_) => {
                out.push(host(&command));
                self.source = None;
                return;
            }
            _ => {
                out.push(host(&command));
                return;
            }
        };
        let Some(meta) = command.data_meta() else {
            return;
        };
        let writer = source
            .streams
            .entry(stream)
            .or_insert_with(|| DataStreamWriter::new(meta));
        let mut encoded = Vec::new();
        writer.write(bytes, &mut encoded);
        if end && let Some(writer) = source.streams.remove(&stream) {
            writer.finish(&mut encoded);
        }
        if !encoded.is_empty() {
            out.push(Output::Host(encoded));
        }
    }

    /// A drop the child never finished: the host has moved on to another drag regardless.
    fn end_dropped(&mut self) {
        self.dropped = None;
        self.pasting = None;
    }

    fn paste_types(&self) -> Vec<Arc<str>> {
        PASTE_TYPES
            .iter()
            .filter_map(|wanted| {
                self.host_mimes
                    .iter()
                    .find(|offered| offered.eq_ignore_ascii_case(wanted))
                    .cloned()
            })
            .collect()
    }

    fn set_host_status(
        &mut self,
        operation: DropOperation,
        mimes: Option<Vec<Arc<str>>>,
        out: &mut Vec<Output<K>>,
    ) {
        let status = (operation, mimes);
        if self.host_status.as_ref() == Some(&status) {
            return;
        }
        out.push(host(&ProgramCommand::DropStatus {
            operation: status.0,
            mimes: status.1.clone(),
        }));
        self.host_status = Some(status);
    }
}

fn host<K>(command: &ProgramCommand) -> Output<K> {
    let mut bytes = Vec::new();
    command.encode(None, &mut bytes);
    Output::Host(bytes)
}

fn child<K: Clone>(key: &K, client_id: Option<u32>, event: &TerminalEvent) -> Output<K> {
    let mut bytes = Vec::new();
    event.encode(client_id, &mut bytes);
    Output::Child(key.clone(), bytes)
}

fn finish_host<K>(operation: DropOperation, out: &mut Vec<Output<K>>) {
    out.push(host(&ProgramCommand::FinishDrop { operation }));
}

fn refused(why: &str) -> TerminalEvent {
    TerminalEvent::DragFailed(DndError::with_description(ErrorCode::PermissionDenied, why))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dnd::{CommandDecoder, DragRegistration, DropRegistration, EntryKind, EventDecoder};

    const LOCAL: &[u8] = b"local-machine";

    fn local() -> MachineId {
        MachineId::from_platform_id(LOCAL)
    }

    fn drops(client_id: u32, machine: Option<&[u8]>) -> ChildRegistration {
        ChildRegistration {
            drops: Some(DropRegistration {
                client_id: Some(client_id),
                mimes: vec![],
                machine_id: machine.map(|m| Arc::from(MachineId::from_platform_id(m).as_str())),
            }),
            drags: Some(DragRegistration {
                client_id: Some(client_id),
                machine_id: None,
            }),
        }
    }

    /// Two children side by side, 10 cells wide each: `a` speaks the protocol, `b` does not.
    fn layout(a: ChildRegistration) -> impl Fn(DragPosition) -> Option<Hit<&'static str>> {
        move |position: DragPosition| {
            let (key, left, registration) = match position.cell_x {
                0..10 => ("a", 0, a.clone()),
                10..20 => ("b", 10, ChildRegistration::default()),
                _ => return None,
            };
            Some(Hit {
                key,
                position: DragPosition {
                    cell_x: position.cell_x - left,
                    pixel_x: position.pixel_x - left as i32 * 8,
                    ..position
                },
                registration,
            })
        }
    }

    fn at(x: u32) -> DragPosition {
        DragPosition {
            cell_x: x,
            cell_y: 1,
            pixel_x: x as i32 * 8 + 3,
            pixel_y: 20,
        }
    }

    fn mimes() -> Vec<Arc<str>> {
        vec!["image/png".into(), URI_LIST.into(), TEXT_PLAIN.into()]
    }

    fn event(event: TerminalEvent) -> TaggedEvent {
        TaggedEvent {
            client_id: None,
            event,
        }
    }

    fn command(command: ProgramCommand) -> TaggedCommand {
        TaggedCommand {
            client_id: Some(7),
            command,
        }
    }

    /// What each output says, decoded.
    #[derive(Debug, PartialEq)]
    enum Said {
        Host(ProgramCommand),
        Child(&'static str, Option<u32>, TerminalEvent),
        Paste(&'static str, Vec<u8>),
        Hover(Option<&'static str>),
    }

    fn said(outputs: Vec<Output<&'static str>>) -> Vec<Said> {
        let mut out = Vec::new();
        for output in outputs {
            match output {
                Output::Host(bytes) => {
                    let mut decoder = CommandDecoder::default();
                    for (_, body) in crate::dnd::OscScanner::default().scan(&bytes) {
                        out.extend(
                            decoder
                                .push_body(&body)
                                .into_iter()
                                .map(|t| Said::Host(t.command)),
                        );
                    }
                }
                Output::Child(key, bytes) => {
                    let mut decoder = EventDecoder::default();
                    for (_, body) in crate::dnd::OscScanner::default().scan(&bytes) {
                        out.extend(
                            decoder
                                .push_body(&body)
                                .into_iter()
                                .map(|t| Said::Child(key, t.client_id, t.event)),
                        );
                    }
                }
                Output::Paste { key, data, .. } => out.push(Said::Paste(key, data)),
                Output::Hover(key) => out.push(Said::Hover(key)),
            }
        }
        out
    }

    fn over(
        router: &mut Router<&'static str>,
        x: u32,
        a: &ChildRegistration,
        first: bool,
    ) -> Vec<Said> {
        said(router.host_event(
            event(TerminalEvent::DragOver {
                position: at(x),
                operations: DropOperations::EITHER,
                mimes: first.then(mimes),
            }),
            layout(a.clone()),
        ))
    }

    #[test]
    fn a_drag_follows_the_pointer_from_child_to_child() {
        let a = drops(7, None);
        let mut router = Router::new(Some(local()));
        let first = over(&mut router, 3, &a, true);
        assert_eq!(
            first,
            [
                Said::Hover(Some("a")),
                Said::Host(ProgramCommand::DropStatus {
                    operation: DropOperation::None,
                    mimes: None
                }),
                Said::Child(
                    "a",
                    Some(7),
                    TerminalEvent::DragOver {
                        position: at(3),
                        operations: DropOperations::EITHER,
                        mimes: Some(mimes()),
                    }
                ),
            ]
        );
        let accepted = said(router.child_command(
            &"a",
            &a,
            command(ProgramCommand::DropStatus {
                operation: DropOperation::Copy,
                mimes: Some(vec![URI_LIST.into()]),
            }),
        ));
        assert_eq!(
            accepted,
            [Said::Host(ProgramCommand::DropStatus {
                operation: DropOperation::Copy,
                mimes: Some(vec![URI_LIST.into()]),
            })]
        );
        // `b` does not speak the protocol: the router answers for it, and `a` hears the drag left.
        let moved = over(&mut router, 12, &a, false);
        assert_eq!(
            moved,
            [
                Said::Child("a", Some(7), TerminalEvent::DragLeave),
                Said::Hover(None),
                Said::Hover(Some("b")),
                Said::Host(ProgramCommand::DropStatus {
                    operation: DropOperation::Copy,
                    mimes: Some(vec![URI_LIST.into(), TEXT_PLAIN.into()]),
                }),
            ]
        );
        assert!(
            over(&mut router, 13, &a, false).is_empty(),
            "nothing changed"
        );
        let late = router.child_command(
            &"a",
            &a,
            command(ProgramCommand::DropStatus {
                operation: DropOperation::Move,
                mimes: None,
            }),
        );
        assert!(
            late.is_empty(),
            "a child the drag left no longer answers for it"
        );
        let gone = said(router.host_event(event(TerminalEvent::DragLeave), layout(a)));
        assert_eq!(gone, [Said::Hover(None)]);
    }

    #[test]
    fn only_the_child_dropped_on_reads_the_drop() {
        let a = drops(7, None);
        let mut router = Router::new(Some(local()));
        over(&mut router, 3, &a, true);
        let dropped = said(router.host_event(
            event(TerminalEvent::Drop {
                position: at(4),
                operations: DropOperations::COPY,
                mimes: mimes(),
            }),
            layout(a.clone()),
        ));
        assert_eq!(dropped[0], Said::Hover(None));
        assert!(matches!(
            &dropped[1],
            Said::Child("a", Some(7), TerminalEvent::Drop { position, .. }) if *position == at(4)
        ));
        assert!(router.busy());

        let refused = said(router.child_command(
            &"b",
            &ChildRegistration::default(),
            command(ProgramCommand::RequestData(DataTarget::Mime { index: 2 })),
        ));
        assert!(matches!(
            &refused[..],
            [Said::Child("b", Some(7), TerminalEvent::DataError { error, .. })]
                if error.code == ErrorCode::PermissionDenied
        ));

        let request = said(router.child_command(
            &"a",
            &a,
            command(ProgramCommand::RequestData(DataTarget::Mime { index: 2 })),
        ));
        assert_eq!(
            request,
            [Said::Host(ProgramCommand::RequestData(DataTarget::Mime {
                index: 2
            }))]
        );
        // Delivered in pieces the router realigns, ending with the empty message.
        let mut received = Vec::new();
        for (piece, end) in [
            (&b"file:///tm"[..], false),
            (b"p/x\r\n", false),
            (b"", true),
        ] {
            received.extend(said(router.host_event(
                event(TerminalEvent::Data {
                    target: DataTarget::Mime { index: 2 },
                    kind: None,
                    remote: false,
                    bytes: piece.to_vec(),
                    end,
                }),
                layout(a.clone()),
            )));
        }
        let mut bytes = Vec::new();
        let mut ended = false;
        for said in received {
            match said {
                Said::Child(
                    "a",
                    Some(7),
                    TerminalEvent::Data {
                        remote,
                        bytes: piece,
                        end,
                        ..
                    },
                ) => {
                    assert!(!remote, "the child is on this machine");
                    bytes.extend(piece);
                    ended |= end;
                }
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(bytes, b"file:///tmp/x\r\n");
        assert!(ended);

        let finished = said(router.child_command(
            &"a",
            &a,
            command(ProgramCommand::FinishDrop {
                operation: DropOperation::Copy,
            }),
        ));
        assert_eq!(
            finished,
            [Said::Host(ProgramCommand::FinishDrop {
                operation: DropOperation::Copy
            })]
        );
        assert!(!router.busy());
    }

    #[test]
    fn a_child_on_another_machine_is_told_the_files_are_elsewhere() {
        let a = drops(7, Some(b"server"));
        let mut router = Router::new(Some(local()));
        over(&mut router, 3, &a, true);
        router.host_event(
            event(TerminalEvent::Drop {
                position: at(3),
                operations: DropOperations::COPY,
                mimes: mimes(),
            }),
            layout(a.clone()),
        );
        for (index, remote) in [(2, true), (1, false)] {
            let out = said(router.host_event(
                event(TerminalEvent::Data {
                    target: DataTarget::Mime { index },
                    kind: None,
                    remote: false,
                    bytes: b"x".to_vec(),
                    end: true,
                }),
                layout(a.clone()),
            ));
            assert!(
                out.iter().all(|s| matches!(s, Said::Child(_, _, TerminalEvent::Data { remote: r, .. }) if *r == remote)),
                "index {index}: {out:?}"
            );
        }
        let entry = said(router.host_event(
            event(TerminalEvent::Data {
                target: DataTarget::UriEntry {
                    mime_index: 2,
                    entry: 1,
                },
                kind: Some(EntryKind::Directory(5)),
                remote: false,
                bytes: b"a\0b".to_vec(),
                end: true,
            }),
            layout(a),
        ));
        assert!(matches!(
            &entry[0],
            Said::Child(
                "a",
                _,
                TerminalEvent::Data {
                    kind: Some(EntryKind::Directory(5)),
                    ..
                }
            )
        ));
    }

    #[test]
    fn a_child_without_the_protocol_gets_the_drop_pasted() {
        let a = drops(7, None);
        let mut router = Router::new(Some(local()));
        over(&mut router, 15, &a, true);
        let dropped = said(router.host_event(
            event(TerminalEvent::Drop {
                position: at(15),
                operations: DropOperations::EITHER,
                mimes: mimes(),
            }),
            layout(a.clone()),
        ));
        assert_eq!(
            dropped,
            [
                Said::Hover(None),
                Said::Host(ProgramCommand::RequestData(DataTarget::Mime { index: 2 })),
            ]
        );
        let pasted = said(router.host_event(
            event(TerminalEvent::Data {
                target: DataTarget::Mime { index: 2 },
                kind: None,
                remote: false,
                bytes: b"file:///a\r\n".to_vec(),
                end: true,
            }),
            layout(a),
        ));
        assert_eq!(
            pasted,
            [
                Said::Paste("b", b"file:///a\r\n".to_vec()),
                Said::Host(ProgramCommand::FinishDrop {
                    operation: DropOperation::Copy
                }),
            ]
        );
    }

    #[test]
    fn a_move_only_drag_or_exotic_data_is_refused_for_pasting() {
        let a = drops(7, None);
        let mut router = Router::new(Some(local()));
        let out = said(router.host_event(
            event(TerminalEvent::DragOver {
                position: at(12),
                operations: DropOperations::MOVE,
                mimes: Some(mimes()),
            }),
            layout(a.clone()),
        ));
        assert!(out.contains(&Said::Host(ProgramCommand::DropStatus {
            operation: DropOperation::None,
            mimes: None
        })));
        let dropped = said(router.host_event(
            event(TerminalEvent::Drop {
                position: at(12),
                operations: DropOperations::EITHER,
                mimes: vec!["image/png".into()],
            }),
            layout(a),
        ));
        assert_eq!(
            dropped.last(),
            Some(&Said::Host(ProgramCommand::FinishDrop {
                operation: DropOperation::None
            }))
        );
    }

    #[test]
    fn a_child_offers_a_drag_through_the_host() {
        let a = drops(7, None);
        let mut router = Router::new(Some(local()));
        let refused = said(router.child_command(
            &"a",
            &a,
            command(ProgramCommand::Offer {
                operations: DropOperations::COPY,
                mimes: vec![TEXT_PLAIN.into()],
            }),
        ));
        assert!(matches!(
            &refused[..],
            [Said::Child("a", _, TerminalEvent::DragFailed(_))]
        ));

        let gesture = said(router.host_event(
            event(TerminalEvent::DragGesture { position: at(5) }),
            layout(a.clone()),
        ));
        assert_eq!(
            gesture,
            [Said::Child(
                "a",
                Some(7),
                TerminalEvent::DragGesture { position: at(5) }
            )]
        );
        let offered = said(router.child_command(
            &"a",
            &a,
            command(ProgramCommand::Offer {
                operations: DropOperations::COPY,
                mimes: vec![TEXT_PLAIN.into()],
            }),
        ));
        assert!(matches!(
            &offered[..],
            [Said::Host(ProgramCommand::Offer { .. })]
        ));
        // Pre-sent data in pieces of any size reaches the host whole.
        let mut sent = Vec::new();
        for (piece, end) in [(&b"he"[..], false), (b"llo", false), (b"", true)] {
            sent.extend(said(router.child_command(
                &"a",
                &a,
                command(ProgramCommand::PresentData {
                    mime_index: 0,
                    bytes: piece.to_vec(),
                    end,
                }),
            )));
        }
        let data: Vec<u8> = sent
            .iter()
            .flat_map(|s| match s {
                Said::Host(ProgramCommand::PresentData { bytes, .. }) => bytes.clone(),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(data, b"hello");
        let started = said(router.child_command(&"a", &a, command(ProgramCommand::StartDrag)));
        assert_eq!(started, [Said::Host(ProgramCommand::StartDrag)]);
        let accepted = said(router.host_event(
            event(TerminalEvent::DragAccepted { mime_index: 0 }),
            layout(a.clone()),
        ));
        assert_eq!(
            accepted,
            [Said::Child(
                "a",
                Some(7),
                TerminalEvent::DragAccepted { mime_index: 0 }
            )]
        );
        router.host_event(
            event(TerminalEvent::DragFinished { canceled: false }),
            layout(a.clone()),
        );
        assert!(!router.busy());
        let late = router.child_command(&"a", &a, command(ProgramCommand::StartDrag));
        assert!(matches!(
            &said(late)[..],
            [Said::Child("a", _, TerminalEvent::DragFailed(_))]
        ));
    }

    #[test]
    fn a_child_that_goes_away_ends_what_it_was_part_of() {
        let a = drops(7, None);
        let mut router = Router::new(Some(local()));
        over(&mut router, 3, &a, true);
        let gone = said(router.forget_child(&"a"));
        assert!(gone.contains(&Said::Hover(None)));
        router.host_event(
            event(TerminalEvent::Drop {
                position: at(3),
                operations: DropOperations::COPY,
                mimes: mimes(),
            }),
            layout(a.clone()),
        );
        let gone = said(router.forget_child(&"a"));
        assert_eq!(
            gone,
            [Said::Host(ProgramCommand::FinishDrop {
                operation: DropOperation::None
            })]
        );
        router.host_event(
            event(TerminalEvent::DragGesture { position: at(1) }),
            layout(a.clone()),
        );
        router.child_command(
            &"a",
            &a,
            command(ProgramCommand::Offer {
                operations: DropOperations::COPY,
                mimes: vec![],
            }),
        );
        assert_eq!(
            said(router.forget_child(&"a")),
            [Said::Host(ProgramCommand::CancelDrag)]
        );
    }
}
