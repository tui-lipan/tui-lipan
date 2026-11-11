//! The vocabulary both sides of the protocol share.

use std::fmt;
use std::sync::Arc;

/// What a drop does with its data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[cfg_attr(feature = "terminal-serde", serde(rename_all = "snake_case"))]
pub enum DropOperation {
    /// Refused, or canceled.
    #[default]
    None,
    /// The source keeps its data.
    Copy,
    /// The source may delete its data once the drop completes.
    Move,
}

impl DropOperation {
    /// The protocol's number for this operation.
    pub const fn code(self) -> u32 {
        match self {
            Self::None => 0,
            Self::Copy => 1,
            Self::Move => 2,
        }
    }

    /// The operation a number names. Anything unknown reads as a refusal.
    pub const fn from_code(code: u32) -> Self {
        match code {
            1 => Self::Copy,
            2 => Self::Move,
            _ => Self::None,
        }
    }
}

/// The operations a drag allows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct DropOperations {
    /// Copying is allowed.
    pub copy: bool,
    /// Moving is allowed.
    pub move_: bool,
}

impl DropOperations {
    /// Copy only.
    pub const COPY: Self = Self {
        copy: true,
        move_: false,
    };
    /// Move only.
    pub const MOVE: Self = Self {
        copy: false,
        move_: true,
    };
    /// Either, at the receiver's choice.
    pub const EITHER: Self = Self {
        copy: true,
        move_: true,
    };

    /// The protocol's bit set: 1 copy, 2 move, 3 either.
    pub const fn code(self) -> u32 {
        (self.copy as u32) | ((self.move_ as u32) << 1)
    }

    /// Read the protocol's bit set.
    pub const fn from_code(code: u32) -> Self {
        Self {
            copy: code & 1 != 0,
            move_: code & 2 != 0,
        }
    }

    /// Whether this set allows `operation`.
    pub const fn allows(self, operation: DropOperation) -> bool {
        match operation {
            DropOperation::None => true,
            DropOperation::Copy => self.copy,
            DropOperation::Move => self.move_,
        }
    }

    /// The operation to perform when the receiver has no preference: copy when allowed.
    pub const fn preferred(self) -> DropOperation {
        if self.copy {
            DropOperation::Copy
        } else if self.move_ {
            DropOperation::Move
        } else {
            DropOperation::None
        }
    }
}

/// Where the pointer is, in cells and in pixels from the top-left of the window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct DragPosition {
    /// Cell column.
    pub cell_x: u32,
    /// Cell row.
    pub cell_y: u32,
    /// Pixel column.
    pub pixel_x: i32,
    /// Pixel row.
    pub pixel_y: i32,
}

/// A POSIX error name, as the protocol reports failures.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub enum ErrorCode {
    /// No such item, or an index out of range.
    NotFound,
    /// Not allowed.
    PermissionDenied,
    /// A malformed or out-of-order request.
    Invalid,
    /// Reading failed.
    Io,
    /// Too many requests or open resources.
    TooManyOpen,
    /// Out of memory, or over a resource limit.
    OutOfMemory,
    /// Too much data.
    TooBig,
    /// An unsupported item, or a failure without a better name.
    Unknown,
    /// A name this build does not know.
    Other(Arc<str>),
}

impl ErrorCode {
    /// The protocol's spelling.
    pub fn as_str(&self) -> &str {
        match self {
            Self::NotFound => "ENOENT",
            Self::PermissionDenied => "EPERM",
            Self::Invalid => "EINVAL",
            Self::Io => "EIO",
            Self::TooManyOpen => "EMFILE",
            Self::OutOfMemory => "ENOMEM",
            Self::TooBig => "EFBIG",
            Self::Unknown => "EUNKNOWN",
            Self::Other(name) => name,
        }
    }

    /// Read a POSIX error name.
    pub fn parse(name: &str) -> Self {
        match name {
            "ENOENT" => Self::NotFound,
            "EPERM" | "EACCES" => Self::PermissionDenied,
            "EINVAL" => Self::Invalid,
            "EIO" => Self::Io,
            "EMFILE" => Self::TooManyOpen,
            "ENOMEM" => Self::OutOfMemory,
            "EFBIG" => Self::TooBig,
            "EUNKNOWN" => Self::Unknown,
            other => Self::Other(Arc::from(other)),
        }
    }

    /// The name for an I/O failure.
    pub fn from_io(error: &std::io::Error) -> Self {
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound,
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied,
            std::io::ErrorKind::OutOfMemory => Self::OutOfMemory,
            std::io::ErrorKind::InvalidInput => Self::Invalid,
            _ => Self::Io,
        }
    }
}

/// A failure the protocol reports: a POSIX name and an optional description.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct DndError {
    /// What went wrong.
    pub code: ErrorCode,
    /// A human-readable detail, already stripped of control characters.
    pub description: Option<Arc<str>>,
}

/// The longest description kept from or sent in an error.
const MAX_DESCRIPTION: usize = 1000;

impl DndError {
    /// An error without a description.
    pub const fn new(code: ErrorCode) -> Self {
        Self {
            code,
            description: None,
        }
    }

    /// An error with a description. Control characters are dropped, as the protocol requires.
    pub fn with_description(code: ErrorCode, description: impl AsRef<str>) -> Self {
        let clean = safe_text(description.as_ref());
        Self {
            code,
            description: (!clean.is_empty()).then(|| Arc::from(clean)),
        }
    }

    /// Read `NAME` or `NAME:description`.
    pub fn parse(payload: &[u8]) -> Self {
        let text = String::from_utf8_lossy(payload);
        let (name, description) = match text.split_once(':') {
            Some((name, description)) => (name, Some(description)),
            None => (text.as_ref(), None),
        };
        let code = ErrorCode::parse(name.trim());
        match description {
            Some(description) => Self::with_description(code, description),
            None => Self::new(code),
        }
    }

    /// The wire form, `NAME` or `NAME:description`.
    pub fn to_payload(&self) -> Vec<u8> {
        let mut out = self.code.as_str().as_bytes().to_vec();
        if let Some(description) = &self.description {
            out.push(b':');
            out.extend_from_slice(description.as_bytes());
        }
        out
    }
}

impl fmt::Display for DndError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code.as_str())?;
        if let Some(description) = &self.description {
            write!(f, ": {description}")?;
        }
        Ok(())
    }
}

impl std::error::Error for DndError {}

/// Text with every control character removed and a bounded length.
pub(crate) fn safe_text(text: &str) -> String {
    let mut out: String = text.chars().filter(|c| !c.is_control()).collect();
    if out.len() > MAX_DESCRIPTION {
        let mut end = MAX_DESCRIPTION;
        while !out.is_char_boundary(end) {
            end -= 1;
        }
        out.truncate(end);
    }
    out
}

/// What a transferred file-system entry is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[cfg_attr(feature = "terminal-serde", serde(rename_all = "snake_case"))]
pub enum EntryKind {
    /// A regular file; the data is its contents.
    File,
    /// A symbolic link; the data is its target.
    Symlink,
    /// A directory; the data is its NUL-separated entry names, and the handle names it.
    Directory(u32),
}

impl EntryKind {
    /// The protocol's `X` value.
    pub const fn code(self) -> i32 {
        match self {
            Self::File => 0,
            Self::Symlink => 1,
            Self::Directory(handle) => handle as i32,
        }
    }

    /// Read an `X` value.
    pub const fn from_code(code: i32) -> Self {
        match code {
            0 => Self::File,
            1 => Self::Symlink,
            handle => Self::Directory(handle as u32),
        }
    }
}

/// The pixel format of a drag image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[cfg_attr(feature = "terminal-serde", serde(rename_all = "snake_case"))]
pub enum DragImageFormat {
    /// UTF-8 text the terminal renders; the size scales the base font size by `width/height`.
    Text,
    /// 24-bit sRGB pixels.
    Rgb,
    /// 32-bit sRGB pixels with alpha.
    Rgba,
    /// A PNG file.
    Png,
}

impl DragImageFormat {
    /// The protocol's `y` value.
    pub const fn code(self) -> i32 {
        match self {
            Self::Text => 0,
            Self::Rgb => 24,
            Self::Rgba => 32,
            Self::Png => 100,
        }
    }

    /// Read a `y` value.
    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(Self::Text),
            24 => Some(Self::Rgb),
            32 => Some(Self::Rgba),
            100 => Some(Self::Png),
            _ => None,
        }
    }
}

/// Which data a request or a response concerns.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "terminal-serde",
    derive(serde::Serialize, serde::Deserialize)
)]
#[cfg_attr(feature = "terminal-serde", serde(rename_all = "snake_case"))]
pub enum DataTarget {
    /// The data of one MIME type of the drop, by its one-based index in the drop's list.
    Mime {
        /// One-based index into the drop's MIME types.
        index: u32,
    },
    /// One entry of the drop's `text/uri-list`, read from the dropping machine.
    UriEntry {
        /// One-based index of `text/uri-list` in the drop's MIME types.
        mime_index: u32,
        /// One-based index into the URI list.
        entry: u32,
    },
    /// One entry of a directory sent earlier.
    DirEntry {
        /// The directory's handle.
        handle: u32,
        /// One-based index into the directory's entries.
        entry: u32,
    },
}

/// A space-separated MIME list as the protocol sends it.
pub fn parse_mime_list(payload: &[u8]) -> Vec<Arc<str>> {
    String::from_utf8_lossy(payload)
        .split_ascii_whitespace()
        .map(Arc::from)
        .collect()
}

/// The protocol's spelling of a MIME list.
pub fn format_mime_list<S: AsRef<str>>(mimes: &[S]) -> Vec<u8> {
    let mut out = Vec::new();
    for (index, mime) in mimes.iter().enumerate() {
        if index > 0 {
            out.push(b' ');
        }
        out.extend_from_slice(mime.as_ref().as_bytes());
    }
    out
}

/// Whether `mime` can travel in a space-separated list: printable ASCII without spaces.
pub fn is_valid_mime(mime: &str) -> bool {
    !mime.is_empty() && mime.bytes().all(|b| b.is_ascii_graphic())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn errors_round_trip_and_lose_control_characters() {
        let error = DndError::parse(b"EPERM:not\x1b[31m yours");
        assert_eq!(error.code, ErrorCode::PermissionDenied);
        assert_eq!(error.description.as_deref(), Some("not[31m yours"));
        assert_eq!(error.to_payload(), b"EPERM:not[31m yours");
        assert_eq!(DndError::parse(b"EXDEV").code.as_str(), "EXDEV");
    }

    #[test]
    fn operations_read_and_write_their_bits() {
        assert_eq!(DropOperations::from_code(3), DropOperations::EITHER);
        assert_eq!(DropOperations::MOVE.code(), 2);
        assert_eq!(DropOperations::MOVE.preferred(), DropOperation::Move);
        assert!(!DropOperations::COPY.allows(DropOperation::Move));
        assert_eq!(DropOperation::from_code(7), DropOperation::None);
    }

    #[test]
    fn mime_lists_split_on_any_whitespace() {
        let list = parse_mime_list(b"text/plain  text/uri-list ");
        assert_eq!(list.len(), 2);
        assert_eq!(format_mime_list(&list), b"text/plain text/uri-list");
        assert!(!is_valid_mime("text/plain charset"));
    }
}
