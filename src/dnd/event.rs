//! What a terminal tells the program running in it.

use std::sync::Arc;

use super::stream::{MessageReader, Read};
use super::types::{
    DataTarget, DndError, DragPosition, DropOperation, DropOperations, EntryKind, ErrorCode,
    format_mime_list, parse_mime_list,
};
use super::wire::{Frame, Metadata, write_message};

/// A message from the terminal to the program running in it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TerminalEvent {
    /// The answer to a query: the terminal speaks the protocol. `options` lists optional parts it
    /// supports, empty in the protocol's current revision.
    Supported {
        /// `key=value` pairs naming optional features.
        options: Vec<(Arc<str>, Arc<str>)>,
    },
    /// A drag is over the window.
    DragOver {
        /// Where the pointer is.
        position: DragPosition,
        /// What the drag's source allows.
        operations: DropOperations,
        /// The MIME types on offer, sent on the first event of a drag and whenever they change.
        mimes: Option<Vec<Arc<str>>>,
    },
    /// The drag left the window without dropping.
    DragLeave,
    /// The user dropped. Data may be requested from here on.
    Drop {
        /// Where the pointer is.
        position: DragPosition,
        /// What the drag's source allows.
        operations: DropOperations,
        /// Every MIME type on offer; data requests index into this list.
        mimes: Vec<Arc<str>>,
    },
    /// Requested data. A stream of these ends with `end` set and an empty `bytes`.
    Data {
        /// What was requested.
        target: DataTarget,
        /// For a URI or directory entry, what it is.
        kind: Option<EntryKind>,
        /// For `text/uri-list`, whether the drop came from another machine.
        remote: bool,
        /// Decoded bytes.
        bytes: Vec<u8>,
        /// The data is complete.
        end: bool,
    },
    /// A request failed. Unless the protocol says otherwise, the drop is over.
    DataError {
        /// What was requested, when the terminal said.
        target: Option<DataTarget>,
        /// What went wrong.
        error: DndError,
    },
    /// The user began a drag gesture over the window; the program may offer a drag.
    DragGesture {
        /// Where the gesture began.
        position: DragPosition,
    },
    /// The drag the program offered has started.
    DragStarted,
    /// The terminal refused or aborted the program's drag.
    DragFailed(DndError),
    /// A drop target accepted the drag and will likely want the MIME type at this index.
    DragAccepted {
        /// Zero-based index into the offered MIME types.
        mime_index: u32,
    },
    /// What a drop target will likely do changed.
    DragActionChanged {
        /// The new operation.
        operation: DropOperation,
    },
    /// The drag was dropped on a target; data requests are likely.
    DragDropped,
    /// The drag is over.
    DragFinished {
        /// The user canceled it.
        canceled: bool,
    },
    /// A drop target wants the data of the MIME type at this index.
    DragDataRequested {
        /// Zero-based index into the offered MIME types.
        mime_index: u32,
    },
    /// The terminal wants an entry of the offered `text/uri-list` for another machine.
    RemoteEntryRequested {
        /// One-based index into the URI list.
        entry: u32,
    },
}

/// A terminal event and the multiplexer client id it was tagged with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaggedEvent {
    /// The `i` key: whom a multiplexer should route the event to.
    pub client_id: Option<u32>,
    /// The event.
    pub event: TerminalEvent,
}

fn position(meta: &Metadata) -> DragPosition {
    DragPosition {
        cell_x: Metadata::unsigned(meta.x_or_zero()),
        cell_y: Metadata::unsigned(meta.y_or_zero()),
        pixel_x: pixel(meta.big_x),
        pixel_y: pixel(meta.big_y),
    }
}

fn pixel(value: Option<i64>) -> i32 {
    value
        .unwrap_or(0)
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

fn response_target(meta: &Metadata) -> Option<DataTarget> {
    let positive = |value: Option<i64>| value.filter(|v| *v > 0).map(Metadata::unsigned);
    match (positive(meta.big_y), positive(meta.x), positive(meta.y)) {
        (Some(handle), Some(entry), _) => Some(DataTarget::DirEntry { handle, entry }),
        (None, Some(mime_index), Some(entry)) => Some(DataTarget::UriEntry { mime_index, entry }),
        (None, Some(index), None) => Some(DataTarget::Mime { index }),
        _ => None,
    }
}

fn is_data(meta: &Metadata) -> bool {
    meta.kind() == b'r'
}

/// Reads terminal events from OSC 72 escape codes, as a program receives them.
#[derive(Clone, Debug)]
pub struct EventDecoder {
    reader: MessageReader,
}

impl Default for EventDecoder {
    fn default() -> Self {
        Self {
            reader: MessageReader::new(is_data),
        }
    }
}

impl EventDecoder {
    /// Read one escape code's body (everything after `72;`).
    pub fn push_body(&mut self, body: &[u8]) -> Vec<TaggedEvent> {
        match Frame::parse(body) {
            Ok(frame) => self.push(frame),
            Err(_) => Vec::new(),
        }
    }

    /// Read one escape code.
    pub fn push(&mut self, frame: Frame) -> Vec<TaggedEvent> {
        self.reader
            .push(frame)
            .into_iter()
            .filter_map(|read| {
                let meta = match &read {
                    Read::Text { meta, .. }
                    | Read::Data { meta, .. }
                    | Read::DataEnd { meta, .. }
                    | Read::Invalid { meta } => *meta,
                };
                event(read).map(|event| TaggedEvent {
                    client_id: meta.i,
                    event,
                })
            })
            .collect()
    }

    /// Forget a half-read message.
    pub fn reset(&mut self) {
        self.reader.reset();
    }
}

fn data_event(meta: &Metadata, bytes: Vec<u8>, end: bool) -> Option<TerminalEvent> {
    let target = response_target(meta)?;
    let (kind, remote) = match target {
        DataTarget::Mime { .. } => (None, meta.big_x == Some(1)),
        _ => (Some(EntryKind::from_code(meta.big_x.unwrap_or(0))), false),
    };
    Some(TerminalEvent::Data {
        target,
        kind,
        remote,
        bytes,
        end,
    })
}

fn event(read: Read) -> Option<TerminalEvent> {
    match read {
        Read::Data { meta, bytes } => data_event(&meta, bytes, false),
        Read::DataEnd { meta, empty } => {
            if empty {
                data_event(&meta, Vec::new(), true)
            } else {
                None
            }
        }
        Read::Invalid { meta } if is_data(&meta) => Some(TerminalEvent::DataError {
            target: response_target(&meta),
            error: DndError::with_description(ErrorCode::Invalid, "undecodable data"),
        }),
        Read::Invalid { .. } => None,
        Read::Text { meta, text } => text_event(&meta, text),
    }
}

fn text_event(meta: &Metadata, text: Vec<u8>) -> Option<TerminalEvent> {
    Some(match meta.kind() {
        b'q' => TerminalEvent::Supported {
            options: parse_options(&text),
        },
        b'm' if meta.x == Some(-1) && meta.y == Some(-1) => TerminalEvent::DragLeave,
        b'm' => TerminalEvent::DragOver {
            position: position(meta),
            operations: DropOperations::from_code(meta.o_or_zero()),
            mimes: (!text.is_empty()).then(|| parse_mime_list(&text)),
        },
        b'M' => TerminalEvent::Drop {
            position: position(meta),
            operations: DropOperations::from_code(meta.o_or_zero()),
            mimes: parse_mime_list(&text),
        },
        b'R' => TerminalEvent::DataError {
            target: response_target(meta),
            error: DndError::parse(&text),
        },
        b'o' => TerminalEvent::DragGesture {
            position: position(meta),
        },
        b'E' if text == b"OK" => TerminalEvent::DragStarted,
        b'E' => TerminalEvent::DragFailed(DndError::parse(&text)),
        b'e' => {
            let index = Metadata::unsigned(meta.y_or_zero());
            match meta.x_or_zero() {
                1 => TerminalEvent::DragAccepted { mime_index: index },
                2 => TerminalEvent::DragActionChanged {
                    operation: DropOperation::from_code(meta.o_or_zero()),
                },
                3 => TerminalEvent::DragDropped,
                4 => TerminalEvent::DragFinished {
                    canceled: meta.y_or_zero() == 1,
                },
                5 => TerminalEvent::DragDataRequested { mime_index: index },
                _ => return None,
            }
        }
        b'k' => TerminalEvent::RemoteEntryRequested {
            entry: Metadata::unsigned(meta.x_or_zero()),
        },
        _ => return None,
    })
}

fn parse_options(text: &[u8]) -> Vec<(Arc<str>, Arc<str>)> {
    String::from_utf8_lossy(text)
        .split(':')
        .filter(|field| !field.is_empty())
        .map(|field| match field.split_once('=') {
            Some((key, value)) => (Arc::from(key), Arc::from(value)),
            None => (Arc::from(field), Arc::from("")),
        })
        .collect()
}

fn position_meta(t: u8, position: DragPosition) -> Metadata {
    Metadata {
        x: Some(i64::from(position.cell_x)),
        y: Some(i64::from(position.cell_y)),
        big_x: Some(i64::from(position.pixel_x)),
        big_y: Some(i64::from(position.pixel_y)),
        ..Metadata::of(t)
    }
}

/// The metadata a response about `target` echoes.
pub(crate) fn target_meta(t: u8, target: DataTarget) -> Metadata {
    match target {
        DataTarget::Mime { index } => Metadata {
            x: Some(i64::from(index)),
            ..Metadata::of(t)
        },
        DataTarget::UriEntry { mime_index, entry } => Metadata {
            x: Some(i64::from(mime_index)),
            y: Some(i64::from(entry)),
            ..Metadata::of(t)
        },
        DataTarget::DirEntry { handle, entry } => Metadata {
            x: Some(i64::from(entry)),
            big_y: Some(i64::from(handle)),
            ..Metadata::of(t)
        },
    }
}

/// The metadata of a data response, the key the reader uses to tell them apart included.
pub fn data_meta(target: DataTarget, kind: Option<EntryKind>, remote: bool) -> Metadata {
    let mut meta = target_meta(b'r', target);
    meta.big_x = match (target, kind) {
        (DataTarget::Mime { .. }, _) => remote.then_some(1),
        (_, Some(EntryKind::File) | None) => None,
        (_, Some(kind)) => Some(kind.code()),
    };
    meta
}

impl TerminalEvent {
    /// Append this event's escape codes, tagged with `client_id` when given.
    ///
    /// A [`TerminalEvent::Data`] is written as its own message, then an empty one when `end` is
    /// set. To write a stream in pieces whose sizes are not multiples of three, use
    /// [`DataStreamWriter`](super::DataStreamWriter) with [`data_meta`] instead.
    pub fn encode(&self, client_id: Option<u32>, out: &mut Vec<u8>) {
        let text = |out: &mut Vec<u8>, meta: Metadata, payload: &[u8]| {
            write_message(out, meta.with_i(client_id), payload, false);
        };
        match self {
            Self::Supported { options } => {
                let mut payload = String::new();
                // A key holding `:` or `=`, or a value holding `:`, would read back as other pairs.
                let pairs = options.iter().filter(|(key, value)| {
                    !key.is_empty() && !key.contains([':', '=']) && !value.contains(':')
                });
                for (index, (key, value)) in pairs.enumerate() {
                    if index > 0 {
                        payload.push(':');
                    }
                    payload.push_str(key);
                    payload.push('=');
                    payload.push_str(value);
                }
                text(out, Metadata::of(b'q'), payload.as_bytes());
            }
            Self::DragOver {
                position,
                operations,
                mimes,
            } => {
                let meta = Metadata {
                    o: Some(operations.code()),
                    ..position_meta(b'm', *position)
                };
                let list = mimes.as_deref().map(format_mime_list).unwrap_or_default();
                text(out, meta, &list);
            }
            Self::DragLeave => text(
                out,
                Metadata {
                    x: Some(-1),
                    y: Some(-1),
                    ..Metadata::of(b'm')
                },
                &[],
            ),
            Self::Drop {
                position,
                operations,
                mimes,
            } => {
                let meta = Metadata {
                    o: Some(operations.code()),
                    ..position_meta(b'M', *position)
                };
                text(out, meta, &format_mime_list(mimes));
            }
            Self::Data {
                target,
                kind,
                remote,
                bytes,
                end,
            } => {
                let meta = data_meta(*target, *kind, *remote).with_i(client_id);
                if !bytes.is_empty() {
                    write_message(out, meta, bytes, true);
                }
                if *end {
                    write_message(out, meta, &[], true);
                }
            }
            Self::DataError { target, error } => {
                let meta = target.map_or(Metadata::of(b'R'), |t| target_meta(b'R', t));
                text(out, meta, &error.to_payload());
            }
            Self::DragGesture { position } => text(out, position_meta(b'o', *position), &[]),
            Self::DragStarted => text(out, Metadata::of(b'E'), b"OK"),
            Self::DragFailed(error) => text(out, Metadata::of(b'E'), &error.to_payload()),
            Self::DragAccepted { mime_index } => text(out, drag_event(1, *mime_index), &[]),
            Self::DragActionChanged { operation } => text(
                out,
                Metadata {
                    o: Some(operation.code()),
                    ..drag_event_meta(2)
                },
                &[],
            ),
            Self::DragDropped => text(out, drag_event_meta(3), &[]),
            Self::DragFinished { canceled } => {
                text(out, drag_event(4, u32::from(*canceled)), &[]);
            }
            Self::DragDataRequested { mime_index } => text(out, drag_event(5, *mime_index), &[]),
            Self::RemoteEntryRequested { entry } => text(
                out,
                Metadata {
                    x: Some(i64::from(*entry)),
                    ..Metadata::of(b'k')
                },
                &[],
            ),
        }
    }
}

fn drag_event_meta(x: i32) -> Metadata {
    Metadata {
        x: Some(i64::from(x)),
        ..Metadata::of(b'e')
    }
}

fn drag_event(x: i32, y: u32) -> Metadata {
    Metadata {
        y: Some(i64::from(y)),
        ..drag_event_meta(x)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dnd::wire::OscScanner;

    fn decode(bytes: &[u8]) -> Vec<TaggedEvent> {
        let mut decoder = EventDecoder::default();
        OscScanner::default()
            .scan(bytes)
            .into_iter()
            .flat_map(|(_, body)| decoder.push_body(&body))
            .collect()
    }

    fn round_trip(event: TerminalEvent) {
        let mut out = Vec::new();
        event.encode(Some(9), &mut out);
        let back = decode(&out);
        assert_eq!(
            back,
            [TaggedEvent {
                client_id: Some(9),
                event: event.clone()
            }],
            "{}",
            String::from_utf8_lossy(&out)
        );
    }

    #[test]
    fn every_event_round_trips() {
        let position = DragPosition {
            cell_x: 4,
            cell_y: 2,
            pixel_x: 40,
            pixel_y: 37,
        };
        let mimes: Vec<Arc<str>> = vec!["text/uri-list".into(), "text/plain".into()];
        for event in [
            TerminalEvent::Supported { options: vec![] },
            TerminalEvent::Supported {
                options: vec![("v".into(), "2".into())],
            },
            TerminalEvent::DragOver {
                position,
                operations: DropOperations::EITHER,
                mimes: Some(mimes.clone()),
            },
            TerminalEvent::DragOver {
                position,
                operations: DropOperations::COPY,
                mimes: None,
            },
            TerminalEvent::DragLeave,
            TerminalEvent::Drop {
                position,
                operations: DropOperations::MOVE,
                mimes,
            },
            TerminalEvent::DataError {
                target: Some(DataTarget::UriEntry {
                    mime_index: 1,
                    entry: 3,
                }),
                error: DndError::with_description(ErrorCode::NotFound, "gone"),
            },
            TerminalEvent::DataError {
                target: None,
                error: DndError::new(ErrorCode::PermissionDenied),
            },
            TerminalEvent::DragGesture { position },
            TerminalEvent::DragStarted,
            TerminalEvent::DragFailed(DndError::new(ErrorCode::TooBig)),
            TerminalEvent::DragAccepted { mime_index: 0 },
            TerminalEvent::DragActionChanged {
                operation: DropOperation::Move,
            },
            TerminalEvent::DragDropped,
            TerminalEvent::DragFinished { canceled: true },
            TerminalEvent::DragFinished { canceled: false },
            TerminalEvent::DragDataRequested { mime_index: 2 },
            TerminalEvent::RemoteEntryRequested { entry: 5 },
        ] {
            round_trip(event);
        }
    }

    #[test]
    fn data_streams_carry_their_target_and_kind() {
        for (target, kind, remote) in [
            (DataTarget::Mime { index: 2 }, None, true),
            (DataTarget::Mime { index: 1 }, None, false),
            (
                DataTarget::UriEntry {
                    mime_index: 1,
                    entry: 2,
                },
                Some(EntryKind::Directory(7)),
                false,
            ),
            (
                DataTarget::DirEntry {
                    handle: 7,
                    entry: 1,
                },
                Some(EntryKind::Symlink),
                false,
            ),
            (
                DataTarget::DirEntry {
                    handle: 7,
                    entry: 2,
                },
                Some(EntryKind::File),
                false,
            ),
        ] {
            let mut out = Vec::new();
            let event = TerminalEvent::Data {
                target,
                kind,
                remote,
                bytes: b"payload".to_vec(),
                end: true,
            };
            event.encode(None, &mut out);
            let back: Vec<_> = decode(&out).into_iter().map(|t| t.event).collect();
            assert_eq!(
                back,
                [
                    TerminalEvent::Data {
                        target,
                        kind,
                        remote,
                        bytes: b"payload".to_vec(),
                        end: false,
                    },
                    TerminalEvent::Data {
                        target,
                        kind,
                        remote,
                        bytes: Vec::new(),
                        end: true,
                    },
                ]
            );
        }
    }

    #[test]
    fn reads_what_the_reference_terminal_writes() {
        // Escape codes as kitty 0.47 formats them, client id last.
        let events = decode(
            b"\x1b]72;t=m:x=3:y=1:X=30:Y=17:o=3:i=4:m=0;text/plain text/uri-list \x1b\\\
              \x1b]72;t=M:x=3:y=1:X=30:Y=17:o=1:i=4:m=0;text/plain text/uri-list \x1b\\\
              \x1b]72;t=r:X=1:x=2:i=4:m=0;ZmlsZTovLy90bXAvYQ\x1b\\\
              \x1b]72;t=r:X=1:x=2:i=4\x1b\\\
              \x1b]72;t=R:x=2:y=1:i=4:m=0;ENOENT:drop data file does not exist\x1b\\",
        );
        let events: Vec<_> = events.into_iter().map(|t| (t.client_id, t.event)).collect();
        assert!(matches!(
            &events[0],
            (Some(4), TerminalEvent::DragOver { mimes: Some(m), operations, .. })
                if m.len() == 2 && *operations == DropOperations::EITHER
        ));
        assert!(
            matches!(&events[1], (Some(4), TerminalEvent::Drop { mimes, .. }) if mimes.len() == 2)
        );
        assert_eq!(
            events[2].1,
            TerminalEvent::Data {
                target: DataTarget::Mime { index: 2 },
                kind: None,
                remote: true,
                bytes: b"file:///tmp/a".to_vec(),
                end: false,
            }
        );
        assert!(matches!(events[3].1, TerminalEvent::Data { end: true, .. }));
        assert!(matches!(
            &events[4].1,
            TerminalEvent::DataError {
                target: Some(DataTarget::UriEntry { mime_index: 2, entry: 1 }),
                error,
            } if error.code == ErrorCode::NotFound
        ));
    }
}
