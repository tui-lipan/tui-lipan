//! The terminal drag-and-drop protocol (OSC 72), as kitty defines it.
//!
//! A program running in a terminal can accept drops from, and start drags to, the desktop around
//! it. Both directions travel as one escape code, `OSC 72 ; metadata ; payload ST`, specified at
//! <https://sw.kovidgoyal.net/kitty/dnd-protocol/>.
//!
//! This module is the protocol itself, for either side of the conversation:
//!
//! - a program reads [`TerminalEvent`]s with an [`EventDecoder`] and writes [`ProgramCommand`]s;
//! - a terminal reads [`ProgramCommand`]s with a [`CommandDecoder`] and writes
//!   [`TerminalEvent`]s.
//!
//! A multiplexer sits on both sides at once. [`OscScanner`] finds the escape codes in a byte
//! stream, and the `client_id` of [`TaggedEvent`] and [`TaggedCommand`] carries the protocol's
//! `i` key, which tells a multiplexer which of its clients a message belongs to.
//!
//! Data (dropped contents, files, drag images) streams: every data message carries decoded bytes,
//! and the stream ends with `end` set. Nothing here holds a whole transfer in memory.

mod base64;
mod command;
pub(crate) mod drag_out;
mod event;
pub(crate) mod host;
mod machine_id;
mod registration;
pub mod router;
mod stream;
mod types;
mod uri_list;
pub mod wire;

pub use self::base64::InvalidBase64;
pub use self::command::{CommandDecoder, ProgramCommand, TaggedCommand};
pub use self::drag_out::{
    HostDrag, HostDragFinished, HostDragItems, HostDragSource, HostDragStart,
};
pub use self::event::{EventDecoder, TaggedEvent, TerminalEvent, data_meta};
pub use self::host::{HostDragAndDrop, HostEvent, HostTarget};
pub use self::machine_id::MachineId;
pub use self::registration::{ChildRegistration, DragRegistration, DropRegistration};
pub use self::router::{Hit, Output, Router};
pub use self::stream::{DataStreamWriter, MAX_TEXT};
pub use self::types::{
    DataTarget, DndError, DragImageFormat, DragPosition, DropOperation, DropOperations, EntryKind,
    ErrorCode, format_mime_list, is_valid_mime, parse_mime_list,
};
pub use self::uri_list::{file_uri, file_uri_path, format_uri_list, parse_uri_list};
pub use self::wire::OscScanner;

/// The MIME type of a list of URIs, one per line.
pub const URI_LIST: &str = "text/uri-list";
/// The MIME type of plain text.
pub const TEXT_PLAIN: &str = "text/plain";
/// The MIME type of plain text that names its encoding.
pub const TEXT_PLAIN_UTF8: &str = "text/plain;charset=utf-8";
