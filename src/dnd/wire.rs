//! OSC 72 framing: one escape code is metadata plus an optional payload.
//!
//! `ESC ] 72 ; key=value:key=value ; payload ESC \`. A message whose payload exceeds
//! [`MAX_PAYLOAD`] is split across escape codes; every one but the last carries `m=1`, and the
//! receiver takes the metadata of the first. [`FrameReader`] undoes that split without holding a
//! whole message in memory, so a transfer of any size streams through it.

use std::fmt::Write as _;

/// The OSC number of the drag-and-drop protocol.
pub const OSC_CODE: u32 = 72;

/// The largest payload one escape code may carry, measured after base64 encoding.
pub const MAX_PAYLOAD: usize = 4096;

/// Raw bytes per escape code when the payload is base64: encodes to exactly [`MAX_PAYLOAD`].
pub(crate) const RAW_CHUNK: usize = MAX_PAYLOAD / 4 * 3;

const PREFIX: &[u8] = b"\x1b]72;";
const ST: &[u8] = b"\x1b\\";

/// The metadata of one escape code.
///
/// Every key is optional on the wire. The protocol defines a default for each (`t=a`, zero for
/// the numbers), but several messages differ only in whether a key is present at all, so presence
/// is kept and [`Metadata::kind`] and friends apply the defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Metadata {
    /// `t`: the message type.
    pub t: Option<u8>,
    /// `m`: more escape codes follow for this message.
    pub more: bool,
    /// `i`: the multiplexer client id.
    pub i: Option<u32>,
    /// `o`: an operation or a set of operations.
    pub o: Option<u32>,
    /// `x`: a cell column or an index.
    pub x: Option<i64>,
    /// `y`: a cell row or an index.
    pub y: Option<i64>,
    /// `X`: a pixel column, a remote flag, or an entry kind.
    pub big_x: Option<i64>,
    /// `Y`: a pixel row or a directory handle.
    pub big_y: Option<i64>,
}

impl Metadata {
    /// Metadata of the given type and nothing else.
    pub const fn of(t: u8) -> Self {
        Self {
            t: Some(t),
            more: false,
            i: None,
            o: None,
            x: None,
            y: None,
            big_x: None,
            big_y: None,
        }
    }

    /// The message type, `a` when absent.
    pub fn kind(&self) -> u8 {
        self.t.unwrap_or(b'a')
    }

    /// `x`, zero when absent.
    pub fn x_or_zero(&self) -> i64 {
        self.x.unwrap_or(0)
    }

    /// `y`, zero when absent.
    pub fn y_or_zero(&self) -> i64 {
        self.y.unwrap_or(0)
    }

    /// `o`, zero when absent.
    pub fn o_or_zero(&self) -> u32 {
        self.o.unwrap_or(0)
    }

    /// A value as the unsigned 32-bit number it stands for; out of range is clamped.
    pub(crate) fn unsigned(value: i64) -> u32 {
        value.clamp(0, i64::from(u32::MAX)) as u32
    }

    pub(crate) fn with_i(mut self, i: Option<u32>) -> Self {
        self.i = i;
        self
    }

    fn write(&self, out: &mut String) {
        out.push_str("t=");
        out.push(char::from(self.kind()));
        if self.more {
            out.push_str(":m=1");
        }
        let numbers = [
            ('i', self.i.map(i64::from)),
            ('o', self.o.map(i64::from)),
            ('x', self.x),
            ('y', self.y),
            ('X', self.big_x),
            ('Y', self.big_y),
        ];
        for (key, value) in numbers {
            if let Some(value) = value {
                let _ = write!(out, ":{key}={value}");
            }
        }
    }
}

/// One escape code: metadata and the payload exactly as it was on the wire.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Frame {
    /// The escape code's metadata.
    pub meta: Metadata,
    /// The payload, still base64 where the message type says so.
    pub payload: Vec<u8>,
}

/// Why an escape code could not be read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum FrameError {
    /// A key had no `=`, or a number did not fit in 32 bits.
    #[error("malformed drag-and-drop metadata")]
    Metadata,
    /// The payload was longer than the protocol allows.
    #[error("drag-and-drop payload longer than {MAX_PAYLOAD} bytes")]
    Oversized,
}

impl Frame {
    /// Read the body of an OSC 72 escape code: everything after `72;` and before the terminator.
    ///
    /// Unknown keys are skipped, so a newer peer's additions do not make the message unreadable.
    pub fn parse(body: &[u8]) -> Result<Self, FrameError> {
        let (meta, payload) = match body.iter().position(|&b| b == b';') {
            Some(at) => (&body[..at], &body[at + 1..]),
            None => (body, &[][..]),
        };
        if payload.len() > MAX_PAYLOAD {
            return Err(FrameError::Oversized);
        }
        let mut parsed = Metadata::default();
        for field in meta.split(|&b| b == b':').filter(|f| !f.is_empty()) {
            let [key, b'=', value @ ..] = field else {
                return Err(FrameError::Metadata);
            };
            match key {
                b't' => {
                    let [t] = value else {
                        return Err(FrameError::Metadata);
                    };
                    parsed.t = Some(*t);
                }
                b'm' => parsed.more = parse_u32(value)? != 0,
                b'i' => parsed.i = Some(parse_u32(value)?),
                b'o' => parsed.o = Some(parse_u32(value)?),
                b'x' => parsed.x = Some(parse_i32(value)?),
                b'y' => parsed.y = Some(parse_i32(value)?),
                b'X' => parsed.big_x = Some(parse_i32(value)?),
                b'Y' => parsed.big_y = Some(parse_i32(value)?),
                _ => {}
            }
        }
        Ok(Self {
            meta: parsed,
            payload: payload.to_vec(),
        })
    }

    /// Append this escape code, terminator included.
    pub fn write(&self, out: &mut Vec<u8>) {
        write_frame(out, &self.meta, &self.payload);
    }
}

fn parse_u32(value: &[u8]) -> Result<u32, FrameError> {
    std::str::from_utf8(value)
        .ok()
        .and_then(|v| v.parse().ok())
        .ok_or(FrameError::Metadata)
}

fn parse_i32(value: &[u8]) -> Result<i64, FrameError> {
    let text = std::str::from_utf8(value).map_err(|_| FrameError::Metadata)?;
    // Both signs of 32-bit values appear: indices and handles are unsigned, coordinates may be -1.
    let number: i64 = text.parse().map_err(|_| FrameError::Metadata)?;
    if (i64::from(i32::MIN)..=i64::from(u32::MAX)).contains(&number) {
        Ok(number)
    } else {
        Err(FrameError::Metadata)
    }
}

fn write_frame(out: &mut Vec<u8>, meta: &Metadata, payload: &[u8]) {
    let mut header = String::with_capacity(48);
    meta.write(&mut header);
    out.extend_from_slice(PREFIX);
    out.extend_from_slice(header.as_bytes());
    if !payload.is_empty() {
        out.push(b';');
        // A control byte would end the escape code early and let the rest of the payload act as
        // another one. Base64 and well-formed text never hold one; anything else loses it.
        out.extend(payload.iter().copied().filter(|&b| b >= 0x20 && b != 0x7f));
    }
    out.extend_from_slice(ST);
}

/// Text without control characters, C1 included, so it reads as text in any terminal.
fn clean_text(payload: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    match std::str::from_utf8(payload) {
        Ok(text) if !text.chars().any(char::is_control) => payload.into(),
        Ok(text) => text
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .into_bytes()
            .into(),
        Err(_) => payload.into(),
    }
}

/// Append one message, split into as many escape codes as its payload needs.
///
/// A text payload is split at [`MAX_PAYLOAD`] bytes. A binary payload is base64-encoded without
/// padding, 3072 raw bytes per escape code, so every escape code but the last decodes on
/// its own. Every escape code repeats the full metadata, as the protocol's reference terminal
/// does; a receiver reads the first.
pub fn write_message(out: &mut Vec<u8>, meta: Metadata, payload: &[u8], binary: bool) {
    if payload.is_empty() {
        write_frame(
            out,
            &Metadata {
                more: false,
                ..meta
            },
            &[],
        );
        return;
    }
    let step = if binary { RAW_CHUNK } else { MAX_PAYLOAD };
    let payload = if binary {
        payload.into()
    } else {
        clean_text(payload)
    };
    let mut chunks = payload.chunks(step).peekable();
    let mut encoded = Vec::new();
    while let Some(chunk) = chunks.next() {
        let meta = Metadata {
            more: chunks.peek().is_some(),
            ..meta
        };
        if binary {
            encoded.clear();
            super::base64::encode_into(chunk, &mut encoded);
            write_frame(out, &meta, &encoded);
        } else {
            write_frame(out, &meta, chunk);
        }
    }
}

/// One escape code of a message, with the metadata of the message's first escape code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fragment {
    /// The metadata that applies: the first escape code's, whatever later ones carried.
    pub meta: Metadata,
    /// This escape code's payload.
    pub payload: Vec<u8>,
    /// Whether this is the message's first escape code.
    pub first: bool,
    /// Whether this is the message's last escape code.
    pub last: bool,
}

/// Groups escape codes into messages as they arrive.
///
/// While a message is split, its sender may still send a query (`t=q`). A query is answered as
/// its own message and does not disturb the one in progress.
#[derive(Clone, Debug, Default)]
pub struct FrameReader {
    open: Option<Metadata>,
}

impl FrameReader {
    /// Place one escape code within its message.
    pub fn push(&mut self, frame: Frame) -> Fragment {
        if frame.meta.t == Some(b'q') {
            return Fragment {
                meta: frame.meta,
                payload: frame.payload,
                first: true,
                last: true,
            };
        }
        let more = frame.meta.more;
        let (meta, first) = match self.open.take() {
            Some(open) => (Metadata { more, ..open }, false),
            None => (frame.meta, true),
        };
        if more {
            self.open = Some(meta);
        }
        Fragment {
            meta,
            payload: frame.payload,
            first,
            last: !more,
        }
    }

    /// Whether a message is half-read.
    pub fn in_message(&self) -> bool {
        self.open.is_some()
    }

    /// Forget a half-read message.
    pub fn reset(&mut self) {
        self.open = None;
    }
}

/// Finds OSC 72 escape codes in a byte stream, wherever the reads that carry it split it.
///
/// Bytes that are not part of an OSC 72 escape code pass by unclaimed. Both string terminators,
/// `ESC \` and `BEL`, end an escape code.
#[derive(Clone, Debug, Default)]
pub struct OscScanner {
    state: ScanState,
    body: Vec<u8>,
    /// Bytes of the escape code seen so far, beyond what `body` keeps once it overflowed.
    overflow: bool,
    /// Skip bracketed pastes: text the user pasted is never an escape code from the terminal.
    paste_aware: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ScanState {
    #[default]
    Ground,
    Escape,
    /// Inside `ESC ]`, matching the `72;` that makes it ours.
    Prefix(u8),
    Body,
    BodyEscape,
    /// Inside some other string, waiting for its end.
    Other,
    OtherEscape,
    /// After `ESC [`, matching the `200~` that starts a bracketed paste.
    PasteStart(u8),
    /// Inside a bracketed paste, matching the `ESC [ 201 ~` that ends it.
    Paste(u8),
}

/// The longest escape code body the scanner keeps: the payload limit plus generous metadata.
const MAX_BODY: usize = MAX_PAYLOAD + 512;

impl OscScanner {
    /// A scanner for a terminal's input, which skips the contents of bracketed pastes.
    pub fn for_terminal_input() -> Self {
        Self {
            paste_aware: true,
            ..Self::default()
        }
    }

    /// Feed bytes; returns the end offset (exclusive) and body of every escape code completed.
    pub fn scan(&mut self, bytes: &[u8]) -> Vec<(usize, Vec<u8>)> {
        let mut found = Vec::new();
        for (index, &byte) in bytes.iter().enumerate() {
            if let Some(body) = self.step(byte) {
                found.push((index + 1, body));
            }
        }
        found
    }

    /// Feed one byte; returns a body when it completes an OSC 72 escape code.
    pub fn step(&mut self, byte: u8) -> Option<Vec<u8>> {
        use ScanState::*;
        match self.state {
            Ground => {
                if byte == 0x1b {
                    self.state = Escape;
                }
            }
            Escape => {
                self.state = match byte {
                    b']' => Prefix(0),
                    b'P' | b'_' | b'^' | b'X' => Other,
                    b'[' if self.paste_aware => PasteStart(0),
                    0x1b => Escape,
                    _ => Ground,
                }
            }
            PasteStart(matched) => {
                const START: &[u8] = b"200~";
                self.state = if byte == START[usize::from(matched)] {
                    if usize::from(matched) + 1 == START.len() {
                        Paste(0)
                    } else {
                        PasteStart(matched + 1)
                    }
                } else if byte == 0x1b {
                    Escape
                } else {
                    Ground
                };
            }
            Paste(matched) => {
                const END: &[u8] = b"\x1b[201~";
                self.state = if byte == END[usize::from(matched)] {
                    if usize::from(matched) + 1 == END.len() {
                        Ground
                    } else {
                        Paste(matched + 1)
                    }
                } else {
                    Paste(u8::from(byte == END[0]))
                };
            }
            Prefix(matched) => {
                const WANT: &[u8] = b"72;";
                if byte == WANT[usize::from(matched)] {
                    if usize::from(matched) + 1 == WANT.len() {
                        self.body.clear();
                        self.overflow = false;
                        self.state = Body;
                    } else {
                        self.state = Prefix(matched + 1);
                    }
                } else {
                    self.state = self.other_string(byte);
                }
            }
            Body => match byte {
                0x07 => {
                    self.state = Ground;
                    return self.finish();
                }
                0x1b => self.state = BodyEscape,
                0x18 | 0x1a => self.state = Ground,
                _ => self.keep(byte),
            },
            BodyEscape => {
                if byte == b'\\' {
                    self.state = Ground;
                    return self.finish();
                }
                // An escape that is not a terminator abandons the string, as VT parsers do.
                self.state = Escape;
                return self.step(byte);
            }
            Other => self.state = self.other_string(byte),
            OtherEscape => {
                self.state = if byte == b'\\' { Ground } else { Escape };
                if self.state == Escape {
                    return self.step(byte);
                }
            }
        }
        None
    }

    fn other_string(&self, byte: u8) -> ScanState {
        match byte {
            0x07 | 0x18 | 0x1a => ScanState::Ground,
            0x1b => ScanState::OtherEscape,
            _ => ScanState::Other,
        }
    }

    fn keep(&mut self, byte: u8) {
        if self.body.len() < MAX_BODY {
            self.body.push(byte);
        } else {
            self.overflow = true;
        }
    }

    fn finish(&mut self) -> Option<Vec<u8>> {
        let body = std::mem::take(&mut self.body);
        (!std::mem::take(&mut self.overflow)).then_some(body)
    }

    /// Whether the scanner is inside an OSC 72 escape code.
    pub fn in_escape_code(&self) -> bool {
        matches!(self.state, ScanState::Body | ScanState::BodyEscape)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(bytes: &[u8]) -> Vec<Frame> {
        OscScanner::default()
            .scan(bytes)
            .into_iter()
            .map(|(_, body)| Frame::parse(&body).unwrap())
            .collect()
    }

    #[test]
    fn metadata_keeps_presence_and_reads_both_signs() {
        let frame = Frame::parse(b"t=m:x=-1:y=-1:X=12:Y=4294967295:o=3;text/plain").unwrap();
        assert_eq!(frame.meta.kind(), b'm');
        assert_eq!((frame.meta.x, frame.meta.y), (Some(-1), Some(-1)));
        assert_eq!(frame.meta.big_y, Some(4294967295));
        let mut out = Vec::new();
        frame.write(&mut out);
        assert!(
            String::from_utf8_lossy(&out).contains(":Y=4294967295"),
            "unsigned values round-trip"
        );
        assert_eq!(Frame::parse(b"t=r:x=4294967296"), Err(FrameError::Metadata));
        assert_eq!(frame.meta.o, Some(3));
        assert_eq!(frame.meta.i, None);
        assert_eq!(frame.payload, b"text/plain");
    }

    #[test]
    fn empty_metadata_defaults_to_accepting_drops() {
        let frame = Frame::parse(b";text/plain").unwrap();
        assert_eq!(frame.meta.kind(), b'a');
        assert_eq!(frame.payload, b"text/plain");
    }

    #[test]
    fn unknown_keys_are_skipped_and_malformed_ones_refused() {
        assert_eq!(Frame::parse(b"t=r:z=9:x=2").unwrap().meta.x, Some(2));
        assert_eq!(Frame::parse(b"t=r:x"), Err(FrameError::Metadata));
        assert_eq!(Frame::parse(b"t=r:x=abc"), Err(FrameError::Metadata));
        let oversized = [b"t=r;".as_slice(), &[b'A'; MAX_PAYLOAD + 1]].concat();
        assert_eq!(Frame::parse(&oversized), Err(FrameError::Oversized));
    }

    #[test]
    fn a_split_message_round_trips_with_the_first_metadata() {
        let payload: Vec<u8> = (0..10_000u32).map(|n| (n % 251) as u8).collect();
        let mut out = Vec::new();
        let meta = Metadata {
            x: Some(3),
            i: Some(7),
            ..Metadata::of(b'r')
        };
        write_message(&mut out, meta, &payload, true);
        let found = frames(&out);
        assert_eq!(found.len(), payload.len().div_ceil(RAW_CHUNK));
        assert!(found.iter().all(|f| f.payload.len() <= MAX_PAYLOAD));
        let mut reader = FrameReader::default();
        let mut decoded = Vec::new();
        let mut stream = super::super::base64::Decoder::default();
        for frame in found {
            let fragment = reader.push(frame);
            assert_eq!(fragment.meta.x, Some(3));
            assert_eq!(fragment.meta.i, Some(7));
            stream.push(&fragment.payload, &mut decoded).unwrap();
            if fragment.last {
                stream.finish(&mut decoded).unwrap();
            }
        }
        assert_eq!(decoded, payload);
        assert!(!reader.in_message());
    }

    #[test]
    fn continuation_codes_may_drop_their_metadata() {
        let mut reader = FrameReader::default();
        let first = reader.push(Frame::parse(b"t=o:o=3:m=1;text/plain ").unwrap());
        let query = reader.push(Frame::parse(b"t=q").unwrap());
        let last = reader.push(Frame::parse(b"m=0;text/uri-list").unwrap());
        assert!(first.first && !first.last);
        assert!(query.first && query.last && query.meta.kind() == b'q');
        assert_eq!(last.meta.kind(), b'o');
        assert_eq!(last.meta.o, Some(3));
        assert!(!last.first && last.last);
    }

    #[test]
    fn the_scanner_finds_codes_split_anywhere_and_skips_other_strings() {
        let stream = b"ab\x1b]7501;x\x07\x1b]72;t=q\x1b\\\x1bP+q\x1b\\c\x1b]72;t=A\x07";
        for split in 0..stream.len() {
            let mut scanner = OscScanner::default();
            let mut found = scanner.scan(&stream[..split]);
            found.extend(
                scanner
                    .scan(&stream[split..])
                    .into_iter()
                    .map(|(end, body)| (end + split, body)),
            );
            let bodies: Vec<_> = found.iter().map(|(_, b)| b.as_slice()).collect();
            assert_eq!(bodies, [b"t=q".as_slice(), b"t=A"], "split at {split}");
            assert_eq!(found[0].0, 21);
        }
    }

    #[test]
    fn terminal_input_skips_bracketed_pastes() {
        let stream = b"\x1b[200~\x1b]72;t=M;text/plain\x1b\\\x1b[201~\x1b]72;t=q\x1b\\";
        for split in 0..stream.len() {
            let mut scanner = OscScanner::for_terminal_input();
            let mut bodies: Vec<_> = scanner.scan(&stream[..split]);
            bodies.extend(scanner.scan(&stream[split..]));
            let bodies: Vec<_> = bodies.into_iter().map(|(_, body)| body).collect();
            assert_eq!(bodies, [b"t=q".to_vec()], "split at {split}");
        }
        let unaware: Vec<_> = OscScanner::default().scan(stream);
        assert_eq!(unaware.len(), 2, "program output has no pastes to skip");
    }

    #[test]
    fn a_payload_cannot_end_its_escape_code_or_start_another() {
        let mut out = Vec::new();
        write_message(
            &mut out,
            Metadata::of(b'a'),
            "text/plain\x1b\\\x1b]2;owned\x07\u{9c}x".as_bytes(),
            false,
        );
        assert_eq!(out, b"\x1b]72;t=a;text/plain\\]2;ownedx\x1b\\");
        let frames = frames(&out);
        assert_eq!(frames.len(), 1);
    }

    #[test]
    fn an_overlong_code_is_dropped_whole() {
        let mut bytes = b"\x1b]72;t=r;".to_vec();
        bytes.extend(std::iter::repeat_n(b'A', MAX_BODY + 10));
        bytes.extend_from_slice(b"\x1b\\\x1b]72;t=A\x1b\\");
        let found = frames(&bytes);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].meta.kind(), b'A');
    }
}
