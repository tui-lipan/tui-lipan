//! What a program tells the terminal it runs in.

use std::sync::Arc;

use super::stream::{MessageReader, Read};
use super::types::{
    DataTarget, DndError, DragImageFormat, DropOperation, DropOperations, EntryKind, ErrorCode,
    format_mime_list, parse_mime_list,
};
use super::wire::{Frame, Metadata, write_message};

/// A message from a program to the terminal it runs in.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProgramCommand {
    /// Ask whether the terminal speaks the protocol.
    Query,
    /// Start accepting drops. `mimes` registers types a platform would otherwise not deliver.
    AcceptDrops {
        /// Extra MIME types to register; may be empty.
        mimes: Vec<Arc<str>>,
    },
    /// The machine the program runs on, so the terminal can serve files from another one.
    DropMachineId(Arc<str>),
    /// Stop accepting drops.
    StopAcceptingDrops,
    /// Whether the program would accept a drop now, and which types it wants.
    DropStatus {
        /// What it would do; [`DropOperation::None`] refuses.
        operation: DropOperation,
        /// The types it wants, most preferred first; `None` keeps the previous choice.
        mimes: Option<Vec<Arc<str>>>,
    },
    /// Ask for dropped data.
    RequestData(DataTarget),
    /// Free a directory handle.
    ReleaseDirectory {
        /// The handle to free.
        handle: u32,
    },
    /// The program has read what it needs; the drop is complete.
    FinishDrop {
        /// What it did; [`DropOperation::None`] cancels the drop.
        operation: DropOperation,
    },
    /// Start offering drags, from the given machine when it is known.
    OfferDrags {
        /// The program's machine id.
        machine_id: Option<Arc<str>>,
    },
    /// Stop offering drags.
    StopOfferingDrags,
    /// Offer a drag in answer to a gesture.
    Offer {
        /// What the program allows the target to do.
        operations: DropOperations,
        /// The types on offer.
        mimes: Vec<Arc<str>>,
    },
    /// Data sent ahead for an offered type. A stream ends with `end` set.
    PresentData {
        /// Zero-based index into the offered MIME types.
        mime_index: u32,
        /// Bytes.
        bytes: Vec<u8>,
        /// The data is complete.
        end: bool,
    },
    /// An image to show while dragging. A stream ends with `end` set.
    PresentImage {
        /// One-based number of the image, up to 2³¹; images are sent in order. The number travels
        /// negated in a signed 32-bit key, so a larger one is sent as 2³¹.
        image: u32,
        /// How `bytes` encode it.
        format: DragImageFormat,
        /// Width in pixels, or the text scale's numerator.
        width: u32,
        /// Height in pixels, or the text scale's denominator.
        height: u32,
        /// Background opacity of a text image, out of 1024.
        opacity: u32,
        /// Bytes.
        bytes: Vec<u8>,
        /// The image is complete.
        end: bool,
    },
    /// Show another image during the drag; out of range removes the image.
    ChangeImage {
        /// Zero-based image index.
        index: u32,
    },
    /// Start the offered drag.
    StartDrag,
    /// Data a drop target asked for. A stream ends with `end` set.
    DragData {
        /// Zero-based index into the offered MIME types.
        mime_index: u32,
        /// Bytes.
        bytes: Vec<u8>,
        /// The data is complete.
        end: bool,
    },
    /// The data a drop target asked for could not be produced.
    DragDataError {
        /// Zero-based index into the offered MIME types.
        mime_index: u32,
        /// What went wrong.
        error: DndError,
    },
    /// Cancel the drag.
    CancelDrag,
    /// The drag cannot continue; the terminal aborts it.
    DragError(DndError),
    /// An entry of the offered `text/uri-list`, for a drop on another machine. A stream ends with
    /// `end` set.
    RemoteEntry {
        /// One-based index into the URI list.
        entry: u32,
        /// For a directory's child: the directory's handle and the child's one-based index.
        child: Option<(u32, u32)>,
        /// What the entry is.
        kind: EntryKind,
        /// Bytes: file contents, a link target, or NUL-separated child names.
        bytes: Vec<u8>,
        /// The entry is complete.
        end: bool,
    },
}

/// A program command and the multiplexer client id it was tagged with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaggedCommand {
    /// The `i` key the program set.
    pub client_id: Option<u32>,
    /// The command.
    pub command: ProgramCommand,
}

fn is_data(meta: &Metadata) -> bool {
    matches!(meta.kind(), b'p' | b'k' | b'e')
}

/// Reads program commands from OSC 72 escape codes, as a terminal receives them.
#[derive(Clone, Debug)]
pub struct CommandDecoder {
    reader: MessageReader,
}

impl Default for CommandDecoder {
    fn default() -> Self {
        Self {
            reader: MessageReader::new(is_data),
        }
    }
}

impl CommandDecoder {
    /// Read one escape code's body (everything after `72;`).
    pub fn push_body(&mut self, body: &[u8]) -> Vec<TaggedCommand> {
        match Frame::parse(body) {
            Ok(frame) => self.push(frame),
            Err(_) => Vec::new(),
        }
    }

    /// Read one escape code.
    pub fn push(&mut self, frame: Frame) -> Vec<TaggedCommand> {
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
                command(read).map(|command| TaggedCommand {
                    client_id: meta.i,
                    command,
                })
            })
            .collect()
    }

    /// Forget a half-read message.
    pub fn reset(&mut self) {
        self.reader.reset();
    }
}

fn positive(value: Option<i64>) -> Option<u32> {
    value.filter(|v| *v > 0).map(Metadata::unsigned)
}

fn data_command(meta: &Metadata, bytes: Vec<u8>, end: bool) -> Option<ProgramCommand> {
    Some(match meta.kind() {
        b'p' => {
            let x = meta.x_or_zero();
            if x >= 0 {
                ProgramCommand::PresentData {
                    mime_index: Metadata::unsigned(x),
                    bytes,
                    end,
                }
            } else {
                ProgramCommand::PresentImage {
                    image: Metadata::unsigned(-x),
                    format: DragImageFormat::from_code(meta.y_or_zero())?,
                    width: Metadata::unsigned(meta.big_x.unwrap_or(0)),
                    height: Metadata::unsigned(meta.big_y.unwrap_or(0)),
                    opacity: meta.o_or_zero(),
                    bytes,
                    end,
                }
            }
        }
        b'e' => ProgramCommand::DragData {
            mime_index: Metadata::unsigned(meta.y_or_zero()),
            bytes,
            end,
        },
        b'k' => ProgramCommand::RemoteEntry {
            entry: positive(meta.x)?,
            child: positive(meta.big_y).zip(positive(meta.y)),
            kind: EntryKind::from_code(meta.big_x.unwrap_or(0)),
            bytes,
            end,
        },
        _ => return None,
    })
}

fn command(read: Read) -> Option<ProgramCommand> {
    match read {
        Read::Data { meta, bytes } => data_command(&meta, bytes, false),
        Read::DataEnd { meta, empty: true } => data_command(&meta, Vec::new(), true),
        Read::DataEnd { .. } => None,
        Read::Invalid { meta } => match meta.kind() {
            b'e' => Some(ProgramCommand::DragDataError {
                mime_index: Metadata::unsigned(meta.y_or_zero()),
                error: DndError::with_description(ErrorCode::Invalid, "undecodable data"),
            }),
            b'p' | b'k' => Some(ProgramCommand::DragError(DndError::with_description(
                ErrorCode::Invalid,
                "undecodable data",
            ))),
            _ => None,
        },
        Read::Text { meta, text } => text_command(&meta, text),
    }
}

fn text_command(meta: &Metadata, text: Vec<u8>) -> Option<ProgramCommand> {
    let text_value = || Arc::<str>::from(String::from_utf8_lossy(&text).trim());
    Some(match meta.kind() {
        b'q' => ProgramCommand::Query,
        b'a' if meta.x == Some(1) => ProgramCommand::DropMachineId(text_value()),
        b'a' => ProgramCommand::AcceptDrops {
            mimes: parse_mime_list(&text),
        },
        b'A' => ProgramCommand::StopAcceptingDrops,
        b'm' => ProgramCommand::DropStatus {
            operation: DropOperation::from_code(meta.o_or_zero()),
            mimes: (!text.is_empty()).then(|| parse_mime_list(&text)),
        },
        b'r' => match (positive(meta.big_y), positive(meta.x), positive(meta.y)) {
            (Some(handle), Some(entry), _) => {
                ProgramCommand::RequestData(DataTarget::DirEntry { handle, entry })
            }
            (Some(handle), None, _) => ProgramCommand::ReleaseDirectory { handle },
            (None, Some(mime_index), Some(entry)) => {
                ProgramCommand::RequestData(DataTarget::UriEntry { mime_index, entry })
            }
            (None, Some(index), None) => ProgramCommand::RequestData(DataTarget::Mime { index }),
            (None, None, _) => ProgramCommand::FinishDrop {
                operation: DropOperation::from_code(meta.o_or_zero()),
            },
        },
        b'o' => match meta.x_or_zero() {
            1 => ProgramCommand::OfferDrags {
                machine_id: (!text.is_empty()).then(text_value),
            },
            2 => ProgramCommand::StopOfferingDrags,
            _ => ProgramCommand::Offer {
                operations: DropOperations::from_code(meta.o_or_zero()),
                mimes: parse_mime_list(&text),
            },
        },
        b'P' => match meta.x_or_zero() {
            -1 => ProgramCommand::StartDrag,
            index => ProgramCommand::ChangeImage {
                index: Metadata::unsigned(index),
            },
        },
        b'E' => match meta.y {
            Some(-1) => ProgramCommand::CancelDrag,
            Some(index) => ProgramCommand::DragDataError {
                mime_index: Metadata::unsigned(index),
                error: DndError::parse(&text),
            },
            None => ProgramCommand::DragError(DndError::parse(&text)),
        },
        _ => return None,
    })
}

impl ProgramCommand {
    /// Append this command's escape codes, tagged with `client_id` when given.
    ///
    /// A data command is written as its own message, then an empty one when `end` is set. To
    /// write a stream in pieces whose sizes are not multiples of three, use
    /// [`DataStreamWriter`](super::DataStreamWriter) with [`ProgramCommand::data_meta`].
    pub fn encode(&self, client_id: Option<u32>, out: &mut Vec<u8>) {
        let text = |out: &mut Vec<u8>, meta: Metadata, payload: &[u8]| {
            write_message(out, meta.with_i(client_id), payload, false);
        };
        let data = |out: &mut Vec<u8>, meta: Metadata, bytes: &[u8], end: bool| {
            let meta = meta.with_i(client_id);
            if !bytes.is_empty() {
                write_message(out, meta, bytes, true);
            }
            if end {
                write_message(out, meta, &[], true);
            }
        };
        if let Some(meta) = self.data_meta() {
            let (bytes, end) = match self {
                Self::PresentData { bytes, end, .. }
                | Self::PresentImage { bytes, end, .. }
                | Self::DragData { bytes, end, .. }
                | Self::RemoteEntry { bytes, end, .. } => (bytes.as_slice(), *end),
                _ => unreachable!("only data commands have data metadata"),
            };
            data(out, meta, bytes, end);
            return;
        }
        match self {
            Self::Query => text(out, Metadata::of(b'q'), &[]),
            Self::AcceptDrops { mimes } => text(out, Metadata::of(b'a'), &format_mime_list(mimes)),
            Self::DropMachineId(id) => text(
                out,
                Metadata {
                    x: Some(1),
                    ..Metadata::of(b'a')
                },
                id.as_bytes(),
            ),
            Self::StopAcceptingDrops => text(out, Metadata::of(b'A'), &[]),
            Self::DropStatus { operation, mimes } => text(
                out,
                Metadata {
                    o: Some(operation.code()),
                    ..Metadata::of(b'm')
                },
                &mimes.as_deref().map(format_mime_list).unwrap_or_default(),
            ),
            Self::RequestData(target) => {
                text(out, super::event::target_meta(b'r', *target), &[]);
            }
            Self::ReleaseDirectory { handle } => text(
                out,
                Metadata {
                    big_y: Some(i64::from(*handle)),
                    ..Metadata::of(b'r')
                },
                &[],
            ),
            Self::FinishDrop { operation } => text(
                out,
                Metadata {
                    o: Some(operation.code()),
                    ..Metadata::of(b'r')
                },
                &[],
            ),
            Self::OfferDrags { machine_id } => text(
                out,
                Metadata {
                    x: Some(1),
                    ..Metadata::of(b'o')
                },
                machine_id.as_deref().unwrap_or("").as_bytes(),
            ),
            Self::StopOfferingDrags => text(
                out,
                Metadata {
                    x: Some(2),
                    ..Metadata::of(b'o')
                },
                &[],
            ),
            Self::Offer { operations, mimes } => text(
                out,
                Metadata {
                    o: Some(operations.code()),
                    ..Metadata::of(b'o')
                },
                &format_mime_list(mimes),
            ),
            Self::ChangeImage { index } => text(
                out,
                Metadata {
                    x: Some(i64::from(*index)),
                    ..Metadata::of(b'P')
                },
                &[],
            ),
            Self::StartDrag => text(
                out,
                Metadata {
                    x: Some(-1),
                    ..Metadata::of(b'P')
                },
                &[],
            ),
            Self::DragDataError { mime_index, error } => text(
                out,
                Metadata {
                    y: Some(i64::from(*mime_index)),
                    ..Metadata::of(b'E')
                },
                &error.to_payload(),
            ),
            Self::CancelDrag => text(
                out,
                Metadata {
                    y: Some(-1),
                    ..Metadata::of(b'E')
                },
                &[],
            ),
            Self::DragError(error) => text(out, Metadata::of(b'E'), &error.to_payload()),
            Self::PresentData { .. }
            | Self::PresentImage { .. }
            | Self::DragData { .. }
            | Self::RemoteEntry { .. } => unreachable!("handled above"),
        }
    }

    /// For a data command, the metadata every message of its stream carries.
    pub fn data_meta(&self) -> Option<Metadata> {
        Some(match self {
            Self::PresentData { mime_index, .. } => Metadata {
                x: Some(i64::from(*mime_index)),
                ..Metadata::of(b'p')
            },
            Self::PresentImage {
                image,
                format,
                width,
                height,
                opacity,
                ..
            } => Metadata {
                x: Some(-i64::from(*image)),
                y: Some(format.code()),
                big_x: Some(i64::from(*width)),
                big_y: Some(i64::from(*height)),
                o: Some(*opacity),
                ..Metadata::of(b'p')
            },
            Self::DragData { mime_index, .. } => Metadata {
                y: Some(i64::from(*mime_index)),
                ..Metadata::of(b'e')
            },
            Self::RemoteEntry {
                entry, child, kind, ..
            } => Metadata {
                x: Some(i64::from(*entry)),
                big_x: match kind {
                    EntryKind::File => None,
                    kind => Some(kind.code()),
                },
                big_y: child.map(|(handle, _)| i64::from(handle)),
                y: child.map(|(_, index)| i64::from(index)),
                ..Metadata::of(b'k')
            },
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dnd::wire::OscScanner;

    fn decode(bytes: &[u8]) -> Vec<TaggedCommand> {
        let mut decoder = CommandDecoder::default();
        OscScanner::default()
            .scan(bytes)
            .into_iter()
            .flat_map(|(_, body)| decoder.push_body(&body))
            .collect()
    }

    #[test]
    fn every_text_command_round_trips() {
        let mimes: Vec<Arc<str>> = vec!["text/uri-list".into(), "image/png".into()];
        for command in [
            ProgramCommand::Query,
            ProgramCommand::AcceptDrops { mimes: vec![] },
            ProgramCommand::AcceptDrops {
                mimes: mimes.clone(),
            },
            ProgramCommand::DropMachineId("1:abcdef0123456789abcdef".into()),
            ProgramCommand::StopAcceptingDrops,
            ProgramCommand::DropStatus {
                operation: DropOperation::Copy,
                mimes: Some(mimes.clone()),
            },
            ProgramCommand::DropStatus {
                operation: DropOperation::None,
                mimes: None,
            },
            ProgramCommand::RequestData(DataTarget::Mime { index: 1 }),
            ProgramCommand::RequestData(DataTarget::UriEntry {
                mime_index: 2,
                entry: 3,
            }),
            ProgramCommand::RequestData(DataTarget::DirEntry {
                handle: 9,
                entry: 1,
            }),
            ProgramCommand::ReleaseDirectory { handle: 9 },
            ProgramCommand::RequestData(DataTarget::DirEntry {
                handle: u32::MAX,
                entry: 3_000_000_000,
            }),
            ProgramCommand::ReleaseDirectory {
                handle: 4_000_000_000,
            },
            ProgramCommand::FinishDrop {
                operation: DropOperation::Move,
            },
            ProgramCommand::FinishDrop {
                operation: DropOperation::None,
            },
            ProgramCommand::OfferDrags { machine_id: None },
            ProgramCommand::OfferDrags {
                machine_id: Some("1:00".into()),
            },
            ProgramCommand::StopOfferingDrags,
            ProgramCommand::Offer {
                operations: DropOperations::EITHER,
                mimes,
            },
            ProgramCommand::ChangeImage { index: 0 },
            ProgramCommand::StartDrag,
            ProgramCommand::DragDataError {
                mime_index: 0,
                error: DndError::new(ErrorCode::NotFound),
            },
            ProgramCommand::CancelDrag,
            ProgramCommand::DragError(DndError::new(ErrorCode::Io)),
        ] {
            let mut out = Vec::new();
            command.encode(Some(3), &mut out);
            assert_eq!(
                decode(&out),
                [TaggedCommand {
                    client_id: Some(3),
                    command: command.clone(),
                }],
                "{}",
                String::from_utf8_lossy(&out)
            );
        }
    }

    #[test]
    fn every_data_command_round_trips_as_a_stream() {
        for command in [
            ProgramCommand::PresentData {
                mime_index: 0,
                bytes: b"file:///tmp/x\r\n".to_vec(),
                end: true,
            },
            ProgramCommand::PresentImage {
                image: 2,
                format: DragImageFormat::Rgba,
                width: 2,
                height: 1,
                opacity: 0,
                bytes: vec![1, 2, 3, 4, 5, 6, 7, 8],
                end: true,
            },
            ProgramCommand::DragData {
                mime_index: 1,
                bytes: b"hello".to_vec(),
                end: true,
            },
            ProgramCommand::RemoteEntry {
                entry: 1,
                child: Some((5, 2)),
                kind: EntryKind::Symlink,
                bytes: b"../target".to_vec(),
                end: true,
            },
            ProgramCommand::RemoteEntry {
                entry: 2,
                child: None,
                kind: EntryKind::Directory(5),
                bytes: b"a\0b".to_vec(),
                end: true,
            },
        ] {
            let mut out = Vec::new();
            command.encode(None, &mut out);
            let back: Vec<_> = decode(&out).into_iter().map(|t| t.command).collect();
            let mut data_part = command.clone();
            let mut end_part = command.clone();
            match (&mut data_part, &mut end_part) {
                (
                    ProgramCommand::PresentData { end, .. }
                    | ProgramCommand::PresentImage { end, .. }
                    | ProgramCommand::DragData { end, .. }
                    | ProgramCommand::RemoteEntry { end, .. },
                    ProgramCommand::PresentData { bytes, .. }
                    | ProgramCommand::PresentImage { bytes, .. }
                    | ProgramCommand::DragData { bytes, .. }
                    | ProgramCommand::RemoteEntry { bytes, .. },
                ) => {
                    *end = false;
                    bytes.clear();
                }
                _ => unreachable!(),
            }
            assert_eq!(back, [data_part, end_part]);
        }
    }

    #[test]
    fn reads_what_the_reference_client_writes() {
        // As kitty's own client library formats them.
        let commands = decode(
            b"\x1b]72;t=a;text/uri-list text/plain\x1b\\\
              \x1b]72;t=a:x=1;1:0123456789abcdef0123\x1b\\\
              \x1b]72;t=m:o=1;text/uri-list\x1b\\\
              \x1b]72;t=r:x=1\x1b\\\
              \x1b]72;t=r:o=1\x1b\\",
        );
        let commands: Vec<_> = commands.into_iter().map(|t| t.command).collect();
        assert!(matches!(&commands[0], ProgramCommand::AcceptDrops { mimes } if mimes.len() == 2));
        assert_eq!(
            commands[1],
            ProgramCommand::DropMachineId("1:0123456789abcdef0123".into())
        );
        assert!(matches!(
            commands[2],
            ProgramCommand::DropStatus {
                operation: DropOperation::Copy,
                ..
            }
        ));
        assert_eq!(
            commands[3],
            ProgramCommand::RequestData(DataTarget::Mime { index: 1 })
        );
        assert_eq!(
            commands[4],
            ProgramCommand::FinishDrop {
                operation: DropOperation::Copy
            }
        );
    }
}
