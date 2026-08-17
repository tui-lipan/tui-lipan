//! Kitty graphics protocol (`APC _G`) support for [`TerminalScreen`](super::TerminalScreen).
//!
//! The child program speaks the [Kitty graphics protocol]; the host terminal may speak something
//! else entirely (sixel, iTerm2, or nothing at all). So this module does not forward the child's
//! escape sequences to the host: it decodes them into pixels, anchors each *placement* to an
//! absolute scrollback line, and hands the renderer a plain list of "these pixels, at this cell
//! rect". The renderer re-encodes through the same [`ratatui_image`] path the
//! [`Image`](crate::widgets::Image) widget uses, which is what makes images survive a host that
//! cannot speak Kitty, a pane that is only half on screen, and two panes that both picked image
//! id `1`.
//!
//! Decoding rather than forwarding also keeps the cell grid honest. Nothing here touches the
//! Alacritty grid: [`TerminalScreen`](super::TerminalScreen) splits the byte stream around each
//! command, so the grid sees a stream with the graphics escapes removed, plus the cursor movement
//! the protocol specifies for a placement. Images are a parallel layer keyed to the same
//! absolute-line space as [`SemanticMark`](super::SemanticMark)s, and they scroll, evict, and
//! reset with it.
//!
//! ## Implemented
//!
//! - Direct transmission (`t=d`), chunked with `m=1`, in RGB (`f=24`), RGBA (`f=32`) and PNG
//!   (`f=100`), with optional zlib compression (`o=z`).
//! - Out-of-band transmission through a file (`t=f`), a temporary file (`t=t`) or a shared-memory
//!   object (`t=s`), with the `O=`/`S=` window into it, subject to a
//!   [`GraphicsMediaPolicy`]. This is the difference between a sender pasting every pixel into the
//!   stream and naming where they already are, so it is also the difference between a frame that
//!   must be compressed to be affordable and one that costs a hundred bytes.
//! - Transmit (`a=t`), transmit-and-display (`a=T`), display a stored image (`a=p`), delete
//!   (`a=d`), and query (`a=q`).
//! - Source cropping (`x`/`y`/`w`/`h`), explicit cell sizing (`c`/`r`), z-index (`z`), suppressed
//!   cursor movement (`C=1`), image numbers (`I=`), and response quieting (`q=`).
//!
//! ## Not implemented
//!
//! - Unicode placeholders (`U=1`), animation (`a=a`, `a=f`, `a=c`), and relative placements.
//!
//! Unsupported requests are answered with the protocol's own error report, so a child that probes
//! before drawing gets a clean "no" instead of silence.
//!
//! [Kitty graphics protocol]: https://sw.kovidgoyal.net/kitty/graphics-protocol/

use std::collections::HashMap;
use std::fmt::Write as _;
use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::DynamicImage;

use super::graphics_media::{self, GraphicsMediaPolicy, GraphicsMedium};
use super::screen::TerminalCellSize;

/// Largest payload accumulated across `m=1` chunks, before decoding.
const MAX_TRANSMIT_BYTES: usize = 32 * 1024 * 1024;

/// Largest single `APC` sequence buffered before it is abandoned.
///
/// The protocol tells senders to chunk at 4096 base64 bytes, but this is deliberately not held to
/// that: plenty of senders emit raw pixels in one escape, which clears 64 KiB with a picture only
/// a few hundred cells wide, and a dropped transmission is indistinguishable - from the sender's
/// side - from a terminal with no graphics support at all. What this bound is actually for is
/// stopping an *unterminated* `APC` from growing without end, and the accumulated-payload cap
/// already decides how much a transmission may total, so matching it costs nothing.
const MAX_APC_BYTES: usize = MAX_TRANSMIT_BYTES;

/// Default decoded-pixel budget kept per screen, across every stored image.
///
/// Decoded pixels are 4 bytes each, so this is roughly sixteen 1080p frames. The budget is what
/// stops a session that keeps plotting from growing without bound; images beyond it are evicted
/// least-recently-used.
const DEFAULT_IMAGE_BUDGET_BYTES: usize = 96 * 1024 * 1024;

/// Largest number of live placements retained.
const MAX_PLACEMENTS: usize = 256;

/// Guard on decoded dimensions, applied before pixels are allocated.
const MAX_IMAGE_DIMENSION: u32 = 16384;

/// First auto-assigned image id.
///
/// Clients that transmit without `i=` get an id from here up, above the range a client numbering
/// its own images from `1` would ever reach, so the two never collide.
const FIRST_AUTO_ID: u32 = 1 << 24;

/// Namespace terminal image streams across screens.
///
/// Child-assigned image ids restart at small integers in every pane. The renderer needs a stable
/// owner id as well, both to keep the previous frame visible during an encode and to avoid sharing
/// one host-protocol placement between unrelated panes that both chose image id `1`.
static NEXT_STREAM_NAMESPACE: AtomicU64 = AtomicU64::new(1);

fn next_stream_namespace() -> u64 {
    NEXT_STREAM_NAMESPACE.fetch_add(1, Ordering::Relaxed)
}

// ─── Public types ────────────────────────────────────────────────────────────

/// Decoded pixels the child transmitted.
///
/// Opaque on purpose: the pixels live behind this crate's `image` dependency, and pinning that
/// crate's types into the public API would make each of its releases a breaking change here.
#[derive(Clone)]
pub struct TerminalImage {
    source: ImageSource,
    width: u32,
    height: u32,
    source_hash: u64,
    stream_namespace: u64,
}

/// Everything [`decode_payload`] needs, kept so the decode can happen later.
#[derive(Debug)]
struct DeferredDecode {
    payload: Vec<u8>,
    compressed: bool,
    format: u32,
    width: u32,
    height: u32,
}

/// Where an image's pixels are: already decoded, or still a payload waiting to be asked for.
///
/// A sender streaming frames replaces one id's image far faster than anything draws it - at 300
/// frames a second against a client painting 120, roughly two thirds of the frames are superseded
/// before they are ever looked at. Decoding those costs a full inflate and a full frame of memory
/// traffic for pixels nothing will see. Deferring means the superseded ones are simply dropped.
///
/// Deferring is only safe when nothing is waiting to be told whether the payload was good, which is
/// why [`GraphicsStore::finish_transmit`] applies it to `q=2` transmissions alone. See there.
#[derive(Clone)]
enum ImageSource {
    Decoded(Arc<DynamicImage>),
    Deferred {
        input: Arc<DeferredDecode>,
        /// `None` once decoding has been attempted and failed, so a broken payload is not retried
        /// on every frame.
        decoded: std::sync::OnceLock<Option<Arc<DynamicImage>>>,
    },
}

impl TerminalImage {
    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.height
    }

    /// Stable hash of the transmitted payload.
    ///
    /// Two images with the same hash decoded from the same bytes. The renderer combines this with
    /// the screen and placement identity so unchanged frames hit the cache without letting two
    /// panes accidentally share one host-protocol placement.
    pub fn source_hash(&self) -> u64 {
        self.source_hash
    }

    /// The pixels, decoding them now if that was put off. `None` for a payload that turns out not
    /// to decode - which the renderer treats as nothing to draw, the same as it would an image the
    /// budget evicted.
    pub(crate) fn pixels(&self) -> Option<&Arc<DynamicImage>> {
        match &self.source {
            ImageSource::Decoded(pixels) => Some(pixels),
            ImageSource::Deferred { input, decoded } => decoded
                .get_or_init(|| decode_deferred(input).map(Arc::new))
                .as_ref(),
        }
    }

    pub(crate) fn stream_namespace(&self) -> u64 {
        self.stream_namespace
    }
}

impl PartialEq for TerminalImage {
    /// Same payload, same image: contents are immutable once decoded.
    fn eq(&self, other: &Self) -> bool {
        self.source_hash == other.source_hash
    }
}

impl std::fmt::Debug for TerminalImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TerminalImage")
            .field("width", &self.width())
            .field("height", &self.height())
            .field("source_hash", &self.source_hash)
            .finish()
    }
}

/// An image the child asked to have shown, positioned against the current viewport.
///
/// A placement scrolled entirely out of view is absent from a snapshot; one that is partly visible
/// still reports its whole rect (with `row`/`col` allowed to go negative), so the renderer can
/// crop the pixels instead of squashing them into what is left.
#[derive(Clone, Debug, PartialEq)]
pub struct TerminalImagePlacement {
    /// The image id the child assigned, which is what distinguishes two placements that happen to
    /// hold identical pixels.
    ///
    /// The renderer must key its encoding on this and not on the pixels alone. A host that draws
    /// through Kitty identifies a placement by image id, so two placements sharing one id are one
    /// placement to it - and two copies of the same picture would collapse into a single one on
    /// screen, the other simply not drawn.
    pub image_id: u32,
    /// The pixels to draw.
    pub image: TerminalImage,
    /// Top row, relative to the top of the viewport.
    pub row: i32,
    /// Left column, relative to the left of the viewport.
    pub col: i32,
    /// Height in cells.
    pub rows: u16,
    /// Width in cells.
    pub cols: u16,
    /// Kitty z-index; negative sits behind text. Placements are ordered back to front.
    pub z: i32,
    /// Source sub-rectangle to draw, when the child asked for one (`x`/`y`/`w`/`h`).
    pub source_crop: Option<TerminalImageCrop>,
}

/// A source-pixel sub-rectangle of a transmitted image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalImageCrop {
    /// Left edge in source pixels.
    pub x: u32,
    /// Top edge in source pixels.
    pub y: u32,
    /// Width in source pixels.
    pub width: u32,
    /// Height in source pixels.
    pub height: u32,
}

// ─── Wire scanning ───────────────────────────────────────────────────────────

/// A piece of a byte stream, split around the graphics commands in it.
#[derive(Debug)]
pub(super) enum GraphicsSegment {
    /// `bytes[range]` of the scanned chunk, to be handed to the VT parser unchanged.
    Text(Range<usize>),
    /// A lone `ESC` held back from the previous chunk that did not start a graphics command.
    ///
    /// Carried separately because it belongs to a slice the caller no longer has.
    HeldEscape,
    /// A complete, well-formed graphics command. Boxed to keep the enum small.
    Command(Box<GraphicsCommand>),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ScanState {
    /// Outside any escape sequence.
    #[default]
    Ground,
    /// Saw `ESC`, waiting to find out whether `_` follows.
    Escape,
    /// Inside `APC …`, accumulating.
    Apc,
    /// Inside `APC …`, saw `ESC`, waiting for the `\` that ends it.
    ApcEscape,
}

/// Splits a PTY byte stream into VT bytes and Kitty graphics commands.
///
/// Every `APC` sequence is swallowed, not just the graphics ones: the VT parser discards `APC`
/// bodies wholesale, so removing them from its input cannot change the grid, and it saves this
/// scanner from having to reproduce `vte`'s string-state rules byte for byte.
#[derive(Debug)]
pub(super) struct GraphicsScanner {
    state: ScanState,
    /// Body of the `APC` being accumulated, without the `ESC _` introducer.
    apc: Vec<u8>,
    /// Set once the body passed [`MAX_APC_BYTES`] and is being discarded.
    overflowed: bool,
    decode_payload: bool,
    decode_continuation_payload: bool,
}

impl Default for GraphicsScanner {
    fn default() -> Self {
        Self {
            state: ScanState::default(),
            apc: Vec::new(),
            overflowed: false,
            decode_payload: true,
            decode_continuation_payload: false,
        }
    }
}

impl GraphicsScanner {
    pub(super) fn set_decode_payload(&mut self, enabled: bool) {
        self.decode_payload = enabled;
        self.reset();
    }

    /// Whether `bytes` can go to the VT parser whole, with no splitting and no allocation.
    ///
    /// The overwhelmingly common case for a terminal is a chunk with no graphics in it at all, so
    /// it is worth one linear scan to keep [`scan`](Self::scan)'s bookkeeping off that path.
    ///
    /// A chunk ending on a bare `ESC` is never plain: the `_` that would make it a graphics
    /// introducer is in the next chunk, and taking this path would leave the scanner unaware.
    pub(super) fn is_plain(&self, bytes: &[u8]) -> bool {
        self.state == ScanState::Ground
            && bytes.last() != Some(&0x1b)
            && !bytes.windows(2).any(|pair| pair == b"\x1b_")
    }

    /// Split `bytes`, in order.
    ///
    /// Ranges in [`GraphicsSegment::Text`] index `bytes`; whatever is needed to finish a command
    /// straddling the chunk boundary stays in `self`.
    pub(super) fn scan(&mut self, bytes: &[u8]) -> Vec<GraphicsSegment> {
        let mut out = Vec::new();
        // Start of the run of plain VT bytes not yet emitted.
        let mut text_start = 0usize;
        let mut idx = 0usize;

        while idx < bytes.len() {
            let byte = bytes[idx];
            match self.state {
                ScanState::Ground => {
                    if byte == 0x1b {
                        // Hold the ESC back: it rejoins the text if this is not an APC, and is
                        // dropped with the rest of the sequence if it is.
                        if text_start < idx {
                            out.push(GraphicsSegment::Text(text_start..idx));
                        }
                        text_start = idx;
                        self.state = ScanState::Escape;
                    }
                    idx += 1;
                }
                ScanState::Escape => {
                    if byte == b'_' {
                        self.state = ScanState::Apc;
                        self.apc.clear();
                        self.overflowed = false;
                        idx += 1;
                        text_start = idx;
                    } else {
                        self.state = ScanState::Ground;
                        if text_start == idx {
                            // The ESC was the last byte of an earlier chunk.
                            out.push(GraphicsSegment::HeldEscape);
                        }
                        // Re-read this byte in `Ground`, so `ESC ESC` starts a fresh sequence.
                    }
                }
                ScanState::Apc => {
                    match byte {
                        0x1b => self.state = ScanState::ApcEscape,
                        // Some emitters end a string with BEL. Protocol payloads are base64 and
                        // never contain one, so accepting it costs nothing.
                        0x07 => {
                            self.finish_apc(&mut out);
                            self.state = ScanState::Ground;
                        }
                        // CAN / SUB abort a string in flight.
                        0x18 | 0x1a => {
                            self.apc.clear();
                            self.overflowed = false;
                            self.state = ScanState::Ground;
                        }
                        _ => self.push_apc(byte),
                    }
                    idx += 1;
                    text_start = idx;
                }
                ScanState::ApcEscape => {
                    if byte == b'\\' {
                        self.finish_apc(&mut out);
                        self.state = ScanState::Ground;
                        idx += 1;
                        text_start = idx;
                    } else {
                        // Unterminated: the VT parser would discard this body too, so drop it and
                        // re-read the byte as the start of whatever follows.
                        self.apc.clear();
                        self.overflowed = false;
                        self.state = ScanState::Ground;
                        text_start = idx;
                    }
                }
            }
        }

        if self.state == ScanState::Ground && text_start < bytes.len() {
            out.push(GraphicsSegment::Text(text_start..bytes.len()));
        }

        out
    }

    fn push_apc(&mut self, byte: u8) {
        if self.overflowed {
            return;
        }
        if self.apc.len() >= MAX_APC_BYTES {
            self.apc.clear();
            self.overflowed = true;
            return;
        }
        self.apc.push(byte);
    }

    fn finish_apc(&mut self, out: &mut Vec<GraphicsSegment>) {
        let body = std::mem::take(&mut self.apc);
        let overflowed = std::mem::take(&mut self.overflowed);
        if overflowed {
            return;
        }
        // Any other `APC` application command; the VT parser ignores those as well.
        let Some(rest) = body.strip_prefix(b"G") else {
            return;
        };
        if let Some(command) = GraphicsCommand::parse(
            rest,
            self.decode_payload || self.decode_continuation_payload,
        ) {
            self.decode_continuation_payload = command.more
                && (self.decode_payload
                    || self.decode_continuation_payload
                    || command.format == 100
                    || matches!(command.action, GraphicsAction::Query));
            out.push(GraphicsSegment::Command(Box::new(command)));
        }
    }

    /// Drop any sequence in flight, for a hard reset of the screen.
    pub(super) fn reset(&mut self) {
        self.state = ScanState::Ground;
        self.apc.clear();
        self.overflowed = false;
        self.decode_continuation_payload = false;
    }
}

// ─── Command model ───────────────────────────────────────────────────────────

/// What a command asks for (`a=`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum GraphicsAction {
    /// `a=t` - store without showing.
    #[default]
    Transmit,
    /// `a=T` - store and place at the cursor.
    TransmitAndDisplay,
    /// `a=p` - place an already-stored image.
    Display,
    /// `a=d` - delete images and/or placements.
    Delete,
    /// `a=q` - capability probe.
    Query,
    /// `a=a`, `a=f`, `a=c` - animation, which this implementation does not do.
    Animate,
}

/// A parsed `APC _G` command.
#[derive(Clone, Debug)]
pub(super) struct GraphicsCommand {
    action: GraphicsAction,
    medium: GraphicsMedium,
    /// `f=` - 24 (RGB), 32 (RGBA), or 100 (PNG).
    format: u32,
    /// `s=` / `v=` - pixel dimensions, required by the raw formats.
    width: u32,
    height: u32,
    /// `i=` - client-assigned image id.
    id: u32,
    /// `I=` - client-assigned image number, mapped to an id on transmit.
    number: u32,
    /// `p=` - placement id.
    placement: u32,
    /// `m=1` - more chunks follow.
    more: bool,
    /// `o=z` - payload is zlib-compressed.
    compressed: bool,
    /// `x` / `y` / `w` / `h` - source rectangle to display.
    src_x: u32,
    src_y: u32,
    src_w: u32,
    src_h: u32,
    /// `c` / `r` - explicit cell size of the placement.
    cols: u32,
    rows: u32,
    /// `z=` - stacking order against the text layer.
    z: i32,
    /// `C=1` - leave the cursor where it is.
    no_cursor_move: bool,
    /// `U=1` - a virtual placement, shown wherever placeholder cells name it rather than here.
    virtual_placement: bool,
    /// `d=` - what a delete applies to.
    delete: u8,
    /// `q=` - 1 suppresses success reports, 2 suppresses failures too.
    quiet: u32,
    /// `O=` - where in an out-of-band source the pixels start.
    source_offset: u64,
    /// `S=` - how much of an out-of-band source to read; 0 means to the end.
    source_size: usize,
    /// Payload with the base64 already undone. For an out-of-band medium this names the source
    /// rather than holding the pixels, until [`TerminalGraphics::resolve_medium`] swaps it for
    /// what the source held.
    payload: Vec<u8>,
    payload_len: usize,
}

impl Default for GraphicsCommand {
    fn default() -> Self {
        Self {
            action: GraphicsAction::default(),
            medium: GraphicsMedium::default(),
            format: 32,
            width: 0,
            height: 0,
            id: 0,
            number: 0,
            placement: 0,
            more: false,
            compressed: false,
            src_x: 0,
            src_y: 0,
            src_w: 0,
            src_h: 0,
            cols: 0,
            rows: 0,
            z: 0,
            no_cursor_move: false,
            virtual_placement: false,
            delete: b'a',
            quiet: 0,
            source_offset: 0,
            source_size: 0,
            payload: Vec::new(),
            payload_len: 0,
        }
    }
}

impl GraphicsCommand {
    fn parse(body: &[u8], decode_payload: bool) -> Option<Self> {
        let (control, payload) = match body.iter().position(|byte| *byte == b';') {
            Some(at) => (&body[..at], &body[at + 1..]),
            None => (body, &body[body.len()..]),
        };

        let mut command = Self::default();
        for pair in control.split(|byte| *byte == b',') {
            let mut halves = pair.splitn(2, |byte| *byte == b'=');
            let ([key], Some(value)) = (halves.next()?, halves.next()) else {
                continue;
            };
            command.apply_key(*key, value);
        }

        // An out-of-band payload is a name, not pixels, and it is short: decode it even on a
        // screen that skips payloads, because the name is the only part that has to be understood
        // before the pixels are fetched.
        let must_decode = decode_payload
            || matches!(command.action, GraphicsAction::Query)
            || command.medium.is_out_of_band()
            || command.format == 100;
        if must_decode {
            // A payload that does not decode makes the whole command malformed rather than empty:
            // acting on half an image would draw garbage.
            command.payload = BASE64.decode(payload).ok()?;
            command.payload_len = command.payload.len();
        } else {
            command.payload_len = base64_decoded_len(payload)?;
        }
        Some(command)
    }

    fn apply_key(&mut self, key: u8, value: &[u8]) {
        let text = std::str::from_utf8(value).unwrap_or("");
        let first = value.first().copied().unwrap_or(0);
        match key {
            b'a' => {
                self.action = match first {
                    b'T' => GraphicsAction::TransmitAndDisplay,
                    b'p' => GraphicsAction::Display,
                    b'd' => GraphicsAction::Delete,
                    b'q' => GraphicsAction::Query,
                    b'a' | b'f' | b'c' => GraphicsAction::Animate,
                    _ => GraphicsAction::Transmit,
                }
            }
            b't' => self.medium = GraphicsMedium::from_key(first),
            b'O' => self.source_offset = text.parse().unwrap_or(0),
            b'S' => self.source_size = text.parse().unwrap_or(0),
            b'f' => self.format = text.parse().unwrap_or(32),
            b's' => self.width = text.parse().unwrap_or(0),
            b'v' => self.height = text.parse().unwrap_or(0),
            b'i' => self.id = text.parse().unwrap_or(0),
            b'I' => self.number = text.parse().unwrap_or(0),
            b'p' => self.placement = text.parse().unwrap_or(0),
            b'm' => self.more = text.parse().unwrap_or(0) == 1,
            b'o' => self.compressed = first == b'z',
            b'x' => self.src_x = text.parse().unwrap_or(0),
            b'y' => self.src_y = text.parse().unwrap_or(0),
            b'w' => self.src_w = text.parse().unwrap_or(0),
            b'h' => self.src_h = text.parse().unwrap_or(0),
            b'c' => self.cols = text.parse().unwrap_or(0),
            b'r' => self.rows = text.parse().unwrap_or(0),
            b'z' => self.z = text.parse().unwrap_or(0),
            b'C' => self.no_cursor_move = text.parse().unwrap_or(0) == 1,
            b'U' => self.virtual_placement = text.parse().unwrap_or(0) == 1,
            b'd' => self.delete = first,
            b'q' => self.quiet = text.parse().unwrap_or(0),
            _ => {}
        }
    }

    /// Whether a report about this command goes back to the child.
    fn reports(&self, ok: bool) -> bool {
        match self.quiet {
            0 => true,
            1 => !ok,
            _ => false,
        }
    }
}

fn base64_decoded_len(payload: &[u8]) -> Option<usize> {
    if !payload.len().is_multiple_of(4) {
        return None;
    }
    let padding = payload
        .iter()
        .rev()
        .take_while(|byte| **byte == b'=')
        .count();
    if padding > 2
        || payload[..payload.len().saturating_sub(padding)]
            .iter()
            .any(|byte| !byte.is_ascii_alphanumeric() && !matches!(*byte, b'+' | b'/'))
        || payload[payload.len().saturating_sub(padding)..]
            .iter()
            .any(|byte| *byte != b'=')
    {
        return None;
    }
    payload
        .len()
        .checked_div(4)?
        .checked_mul(3)?
        .checked_sub(padding)
}

// ─── Unicode placeholders ────────────────────────────────────────────────────

/// The character a virtual placement is drawn with.
///
/// A program that wants an image to sit in the text flow - which is what every terminal UI
/// toolkit wants - transmits it with `U=1` and then writes this character into the cells the image
/// should cover, tagging each with the image id (in the cell's foreground colour) and its position
/// inside the image (in combining marks). Nothing is drawn where the *transmission* happened, so a
/// virtual placement is stored and then found again by reading the grid.
pub(crate) const PLACEHOLDER: char = '\u{10EEEE}';

/// The Kitty protocol's row/column diacritics, in the order the protocol assigns them.
///
/// A placeholder cell names its position inside the image with up to three of these: the first is
/// the image row, the second the column, and the third the most significant byte of the image id.
/// The list is the one in the protocol specification, and is sorted by code point so a lookup can
/// binary-search it.
static ROWCOLUMN_DIACRITICS: [char; 297] = [
    '\u{305}',
    '\u{30d}',
    '\u{30e}',
    '\u{310}',
    '\u{312}',
    '\u{33d}',
    '\u{33e}',
    '\u{33f}',
    '\u{346}',
    '\u{34a}',
    '\u{34b}',
    '\u{34c}',
    '\u{350}',
    '\u{351}',
    '\u{352}',
    '\u{357}',
    '\u{35b}',
    '\u{363}',
    '\u{364}',
    '\u{365}',
    '\u{366}',
    '\u{367}',
    '\u{368}',
    '\u{369}',
    '\u{36a}',
    '\u{36b}',
    '\u{36c}',
    '\u{36d}',
    '\u{36e}',
    '\u{36f}',
    '\u{483}',
    '\u{484}',
    '\u{485}',
    '\u{486}',
    '\u{487}',
    '\u{592}',
    '\u{593}',
    '\u{594}',
    '\u{595}',
    '\u{597}',
    '\u{598}',
    '\u{599}',
    '\u{59c}',
    '\u{59d}',
    '\u{59e}',
    '\u{59f}',
    '\u{5a0}',
    '\u{5a1}',
    '\u{5a8}',
    '\u{5a9}',
    '\u{5ab}',
    '\u{5ac}',
    '\u{5af}',
    '\u{5c4}',
    '\u{610}',
    '\u{611}',
    '\u{612}',
    '\u{613}',
    '\u{614}',
    '\u{615}',
    '\u{616}',
    '\u{617}',
    '\u{657}',
    '\u{658}',
    '\u{659}',
    '\u{65a}',
    '\u{65b}',
    '\u{65d}',
    '\u{65e}',
    '\u{6d6}',
    '\u{6d7}',
    '\u{6d8}',
    '\u{6d9}',
    '\u{6da}',
    '\u{6db}',
    '\u{6dc}',
    '\u{6df}',
    '\u{6e0}',
    '\u{6e1}',
    '\u{6e2}',
    '\u{6e4}',
    '\u{6e7}',
    '\u{6e8}',
    '\u{6eb}',
    '\u{6ec}',
    '\u{730}',
    '\u{732}',
    '\u{733}',
    '\u{735}',
    '\u{736}',
    '\u{73a}',
    '\u{73d}',
    '\u{73f}',
    '\u{740}',
    '\u{741}',
    '\u{743}',
    '\u{745}',
    '\u{747}',
    '\u{749}',
    '\u{74a}',
    '\u{7eb}',
    '\u{7ec}',
    '\u{7ed}',
    '\u{7ee}',
    '\u{7ef}',
    '\u{7f0}',
    '\u{7f1}',
    '\u{7f3}',
    '\u{816}',
    '\u{817}',
    '\u{818}',
    '\u{819}',
    '\u{81b}',
    '\u{81c}',
    '\u{81d}',
    '\u{81e}',
    '\u{81f}',
    '\u{820}',
    '\u{821}',
    '\u{822}',
    '\u{823}',
    '\u{825}',
    '\u{826}',
    '\u{827}',
    '\u{829}',
    '\u{82a}',
    '\u{82b}',
    '\u{82c}',
    '\u{82d}',
    '\u{951}',
    '\u{953}',
    '\u{954}',
    '\u{f82}',
    '\u{f83}',
    '\u{f86}',
    '\u{f87}',
    '\u{135d}',
    '\u{135e}',
    '\u{135f}',
    '\u{17dd}',
    '\u{193a}',
    '\u{1a17}',
    '\u{1a75}',
    '\u{1a76}',
    '\u{1a77}',
    '\u{1a78}',
    '\u{1a79}',
    '\u{1a7a}',
    '\u{1a7b}',
    '\u{1a7c}',
    '\u{1b6b}',
    '\u{1b6d}',
    '\u{1b6e}',
    '\u{1b6f}',
    '\u{1b70}',
    '\u{1b71}',
    '\u{1b72}',
    '\u{1b73}',
    '\u{1cd0}',
    '\u{1cd1}',
    '\u{1cd2}',
    '\u{1cda}',
    '\u{1cdb}',
    '\u{1ce0}',
    '\u{1dc0}',
    '\u{1dc1}',
    '\u{1dc3}',
    '\u{1dc4}',
    '\u{1dc5}',
    '\u{1dc6}',
    '\u{1dc7}',
    '\u{1dc8}',
    '\u{1dc9}',
    '\u{1dcb}',
    '\u{1dcc}',
    '\u{1dd1}',
    '\u{1dd2}',
    '\u{1dd3}',
    '\u{1dd4}',
    '\u{1dd5}',
    '\u{1dd6}',
    '\u{1dd7}',
    '\u{1dd8}',
    '\u{1dd9}',
    '\u{1dda}',
    '\u{1ddb}',
    '\u{1ddc}',
    '\u{1ddd}',
    '\u{1dde}',
    '\u{1ddf}',
    '\u{1de0}',
    '\u{1de1}',
    '\u{1de2}',
    '\u{1de3}',
    '\u{1de4}',
    '\u{1de5}',
    '\u{1de6}',
    '\u{1dfe}',
    '\u{20d0}',
    '\u{20d1}',
    '\u{20d4}',
    '\u{20d5}',
    '\u{20d6}',
    '\u{20d7}',
    '\u{20db}',
    '\u{20dc}',
    '\u{20e1}',
    '\u{20e7}',
    '\u{20e9}',
    '\u{20f0}',
    '\u{2cef}',
    '\u{2cf0}',
    '\u{2cf1}',
    '\u{2de0}',
    '\u{2de1}',
    '\u{2de2}',
    '\u{2de3}',
    '\u{2de4}',
    '\u{2de5}',
    '\u{2de6}',
    '\u{2de7}',
    '\u{2de8}',
    '\u{2de9}',
    '\u{2dea}',
    '\u{2deb}',
    '\u{2dec}',
    '\u{2ded}',
    '\u{2dee}',
    '\u{2def}',
    '\u{2df0}',
    '\u{2df1}',
    '\u{2df2}',
    '\u{2df3}',
    '\u{2df4}',
    '\u{2df5}',
    '\u{2df6}',
    '\u{2df7}',
    '\u{2df8}',
    '\u{2df9}',
    '\u{2dfa}',
    '\u{2dfb}',
    '\u{2dfc}',
    '\u{2dfd}',
    '\u{2dfe}',
    '\u{2dff}',
    '\u{a66f}',
    '\u{a67c}',
    '\u{a67d}',
    '\u{a6f0}',
    '\u{a6f1}',
    '\u{a8e0}',
    '\u{a8e1}',
    '\u{a8e2}',
    '\u{a8e3}',
    '\u{a8e4}',
    '\u{a8e5}',
    '\u{a8e6}',
    '\u{a8e7}',
    '\u{a8e8}',
    '\u{a8e9}',
    '\u{a8ea}',
    '\u{a8eb}',
    '\u{a8ec}',
    '\u{a8ed}',
    '\u{a8ee}',
    '\u{a8ef}',
    '\u{a8f0}',
    '\u{a8f1}',
    '\u{aab0}',
    '\u{aab2}',
    '\u{aab3}',
    '\u{aab7}',
    '\u{aab8}',
    '\u{aabe}',
    '\u{aabf}',
    '\u{aac1}',
    '\u{fe20}',
    '\u{fe21}',
    '\u{fe22}',
    '\u{fe23}',
    '\u{fe24}',
    '\u{fe25}',
    '\u{fe26}',
    '\u{10a0f}',
    '\u{10a38}',
    '\u{1d185}',
    '\u{1d186}',
    '\u{1d187}',
    '\u{1d188}',
    '\u{1d189}',
    '\u{1d1aa}',
    '\u{1d1ab}',
    '\u{1d1ac}',
    '\u{1d1ad}',
    '\u{1d242}',
    '\u{1d243}',
    '\u{1d244}',
];

/// The diacritic that encodes `index`, saturating at the last one the protocol defines.
///
/// The decoder never needs this - it only reads marks - but building the sequences a real sender
/// emits is how the placeholder path is tested, so it lives next to the table it indexes.
pub(crate) fn diacritic(index: u16) -> char {
    ROWCOLUMN_DIACRITICS[usize::from(index).min(ROWCOLUMN_DIACRITICS.len() - 1)]
}

/// The position a row/column diacritic encodes, or `None` if the character is not one.
fn diacritic_value(mark: char) -> Option<u16> {
    ROWCOLUMN_DIACRITICS
        .binary_search(&mark)
        .ok()
        .map(|index| index as u16)
}

/// One placeholder cell, as read off the grid.
#[derive(Clone, Copy, Debug)]
pub(super) struct PlaceholderCell {
    /// Viewport row.
    pub(super) row: u16,
    /// Viewport column.
    pub(super) col: u16,
    /// The low 24 bits of the image id, carried by the cell's foreground colour.
    pub(super) id_low: u32,
    /// Image row, when the cell spelled one out.
    pub(super) image_row: Option<u16>,
    /// Image column, when the cell spelled one out.
    pub(super) image_col: Option<u16>,
    /// High byte of the image id, for ids that do not fit in a colour.
    pub(super) id_high: Option<u16>,
}

impl PlaceholderCell {
    /// Read the position marks off a cell's combining characters.
    ///
    /// The protocol lets a cell omit any of them, in which case it continues the cell to its left:
    /// that is what keeps a row of placeholders down to one escape sequence instead of one per
    /// cell, and it is why these are resolved in a left-to-right pass rather than per cell.
    pub(super) fn new(row: u16, col: u16, id_low: u32, marks: &[char]) -> Self {
        let mut values = marks.iter().filter_map(|mark| diacritic_value(*mark));
        Self {
            row,
            col,
            id_low,
            image_row: values.next(),
            image_col: values.next(),
            id_high: values.next(),
        }
    }
}

/// A resolved run of placeholder cells: one screen row of one image.
#[derive(Clone, Copy, Debug)]
struct PlaceholderRun {
    image_id: u32,
    /// High byte of the image id, kept so the cells continuing this run can inherit it.
    id_high: u16,
    row: u16,
    col: u16,
    width: u16,
    image_row: u16,
    image_col: u16,
}

/// Resolve placeholder cells into runs, applying the protocol's inheritance rules.
///
/// Everything a cell can leave out is inherited from the cell to its left: its row, its column
/// (which advances by one), and the high byte of the image id. Inheriting the id byte matters as
/// much as the position - ids above 24 bits split across the foreground colour and a third
/// combining mark, and a sender writes that mark on the first cell of a row only. Defaulting it
/// to zero instead of inheriting makes every cell after the first name a *different* image, which
/// looks exactly like an image one column wide.
fn placeholder_runs(cells: &[PlaceholderCell]) -> Vec<PlaceholderRun> {
    let mut runs: Vec<PlaceholderRun> = Vec::new();
    let mut open: Option<PlaceholderRun> = None;

    for cell in cells {
        // Adjacency has to be settled before the id, since the id is what may be inherited.
        let adjacent =
            open.is_some_and(|run| run.row == cell.row && run.col + run.width == cell.col);
        let inherited_high = match (adjacent, open) {
            (true, Some(run)) => run.id_high,
            _ => 0,
        };
        let id_high = cell.id_high.unwrap_or(inherited_high);
        let image_id = (u32::from(id_high) << 24) | (cell.id_low & 0x00ff_ffff);

        let continues = adjacent
            && open.is_some_and(|run| {
                run.image_id == image_id
                    && cell.image_row.is_none_or(|value| value == run.image_row)
                    && cell
                        .image_col
                        .is_none_or(|value| value == run.image_col + run.width)
            });

        if continues {
            if let Some(run) = open.as_mut() {
                run.width += 1;
            }
            continue;
        }

        if let Some(run) = open.take() {
            runs.push(run);
        }
        open = Some(PlaceholderRun {
            image_id,
            id_high,
            row: cell.row,
            col: cell.col,
            width: 1,
            image_row: cell.image_row.unwrap_or(0),
            image_col: cell.image_col.unwrap_or(0),
        });
    }

    runs.extend(open);
    runs
}

/// A rectangle of one image, assembled from the rows of placeholders covering it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PlaceholderRect {
    image_id: u32,
    row: u16,
    col: u16,
    width: u16,
    height: u16,
    image_row: u16,
    image_col: u16,
}

/// Stack runs into rectangles, so a whole image is one placement instead of one per row.
///
/// It matters: each placement is separately cropped and encoded, so leaving a 40-row picture as 40
/// strips would mean 40 encodes and 40 sequences on the wire for what the sender meant as one
/// image. Rows that do not line up stay separate rather than being forced together.
fn merge_placeholder_runs(runs: &[PlaceholderRun]) -> Vec<PlaceholderRect> {
    let mut rects: Vec<PlaceholderRect> = Vec::new();

    for run in runs {
        let stackable = rects.iter_mut().find(|rect| {
            rect.image_id == run.image_id
                && rect.col == run.col
                && rect.width == run.width
                && rect.image_col == run.image_col
                && rect.row + rect.height == run.row
                && rect.image_row + rect.height == run.image_row
        });
        if let Some(rect) = stackable {
            rect.height += 1;
            continue;
        }
        rects.push(PlaceholderRect {
            image_id: run.image_id,
            row: run.row,
            col: run.col,
            width: run.width,
            height: 1,
            image_row: run.image_row,
            image_col: run.image_col,
        });
    }

    rects
}

// ─── Store ───────────────────────────────────────────────────────────────────

/// What the screen tells the store so a placement lands in the right place.
#[derive(Clone, Copy, Debug)]
pub(super) struct GraphicsContext {
    /// Absolute line the cursor sits on, in the same space as
    /// [`TerminalScreen::total_text_lines`](super::TerminalScreen::total_text_lines).
    pub(super) cursor_line: usize,
    /// Cursor column.
    pub(super) cursor_col: u16,
    /// Absolute line at the top of the live viewport, for screen-addressed deletes.
    pub(super) viewport_top_line: usize,
    /// Whether the alternate screen is active.
    pub(super) alt_screen: bool,
    /// Host cell size, for converting pixels to cells.
    pub(super) cell: TerminalCellSize,
    /// Viewport width in cells.
    pub(super) cols: u16,
}

/// What the screen does after the store handled a command.
#[derive(Debug, Default)]
pub(super) struct GraphicsOutcome {
    /// A protocol report to write back to the child.
    pub(super) response: Option<Vec<u8>>,
    /// Cursor movement the placement implies, as `(rows down, columns right)`.
    pub(super) advance: Option<(u16, u16)>,
}

/// A stored image plus the accounting the budget needs.
struct StoredImage {
    image: TerminalImage,
    bytes: usize,
    /// Monotonic tick of the last transmit or placement, for LRU eviction.
    used: u64,
    /// `c`/`r` from the image's virtual placement: the cell box the pixels are to be fitted to.
    ///
    /// The protocol has a sender declare this once and then write placeholder cells naming
    /// positions *within it*, so without it a cell's position says nothing about which pixels it
    /// covers. `None` for an image that never got a virtual placement, whose placeholders are read
    /// against the cell grid its own pixels imply.
    virtual_cells: Option<(u32, u32)>,
}

/// A live placement of a stored image.
#[derive(Clone, Debug)]
struct Placement {
    image_id: u32,
    placement_id: u32,
    /// Absolute line of the placement's top row.
    line: usize,
    col: u16,
    rows: u16,
    cols: u16,
    z: i32,
    crop: Option<TerminalImageCrop>,
    /// Placements made on the alternate screen die with it.
    alt_screen: bool,
}

impl Placement {
    fn covers_cell(&self, line: usize, col: u16) -> bool {
        self.covers_line(line) && self.covers_column(col)
    }

    fn covers_line(&self, line: usize) -> bool {
        line >= self.line && line < self.line.saturating_add(usize::from(self.rows))
    }

    fn covers_column(&self, col: u16) -> bool {
        col >= self.col && col < self.col.saturating_add(self.cols)
    }
}

/// A transmission still accumulating `m=1` chunks.
struct PendingTransmit {
    id: u32,
    /// The first chunk's keys, which carry the format and any display request.
    header: GraphicsCommand,
    data: Vec<u8>,
}

/// Decoded images and their placements for one [`TerminalScreen`](super::TerminalScreen).
pub(super) struct TerminalGraphics {
    stream_namespace: u64,
    images: HashMap<u32, StoredImage>,
    image_dimensions: HashMap<u32, (u32, u32)>,
    /// `I=` image numbers mapped to the ids they were transmitted under.
    numbers: HashMap<u32, u32>,
    placements: Vec<Placement>,
    pending: Option<PendingTransmit>,
    next_auto_id: u32,
    budget: usize,
    used_bytes: usize,
    clock: u64,
    storage_enabled: bool,
    media: GraphicsMediaPolicy,
    /// Counts transmissions that named their pixels rather than carrying them. See
    /// [`TerminalGraphics::named_source_identity`].
    source_serial: u64,
}

impl Default for TerminalGraphics {
    fn default() -> Self {
        Self {
            stream_namespace: next_stream_namespace(),
            images: HashMap::new(),
            image_dimensions: HashMap::new(),
            numbers: HashMap::new(),
            placements: Vec::new(),
            pending: None,
            next_auto_id: FIRST_AUTO_ID,
            budget: DEFAULT_IMAGE_BUDGET_BYTES,
            used_bytes: 0,
            clock: 0,
            storage_enabled: true,
            media: GraphicsMediaPolicy::default(),
            source_serial: 0,
        }
    }
}

impl TerminalGraphics {
    /// Whether any image has been transmitted, so a session with none can skip the grid walk
    /// that looks for placeholder cells.
    pub(super) fn has_images(&self) -> bool {
        !self.images.is_empty()
    }

    /// Replace the decoded-pixel budget, evicting immediately if it shrank.
    pub(super) fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
        self.enforce_budget();
    }

    pub(super) fn set_storage_enabled(&mut self, enabled: bool) {
        if self.storage_enabled == enabled {
            return;
        }
        self.storage_enabled = enabled;
        self.reset();
    }

    pub(super) fn set_media_policy(&mut self, media: GraphicsMediaPolicy) {
        self.media = media;
    }

    /// Drop everything, for `RIS` or a screen reset.
    pub(super) fn reset(&mut self) {
        self.stream_namespace = next_stream_namespace();
        self.images.clear();
        self.image_dimensions.clear();
        self.numbers.clear();
        self.placements.clear();
        self.pending = None;
        self.used_bytes = 0;
        self.source_serial = 0;
    }

    /// Drop every placement while keeping the images themselves.
    ///
    /// For a reflow: a column change rewraps history, so the absolute line a placement was
    /// anchored to no longer names the text it was drawn against, and no shift can correct it.
    pub(super) fn clear_placements(&mut self) {
        self.placements.clear();
    }

    /// Drop placements made on the alternate screen, on the way back to the primary one.
    pub(super) fn clear_alt_screen(&mut self) -> bool {
        let before = self.placements.len();
        self.placements.retain(|placement| !placement.alt_screen);
        before != self.placements.len()
    }

    /// Shift placements up by the lines that just fell out of scrollback.
    ///
    /// Mirrors the semantic-mark bookkeeping: eviction is only observable as it happens, so a
    /// placement not corrected here silently slides onto unrelated text.
    pub(super) fn drop_evicted(&mut self, evicted: usize) -> bool {
        if evicted == 0 || self.placements.is_empty() {
            return false;
        }
        // An image scrolling off the top keeps its remaining rows, so it fades out row by row
        // instead of vanishing whole.
        self.placements
            .retain(|placement| placement.line + usize::from(placement.rows) > evicted);
        for placement in &mut self.placements {
            placement.line = placement.line.saturating_sub(evicted);
        }
        true
    }

    /// Placements overlapping a viewport of `rows` rows, back to front.
    ///
    /// `history_lines` is the number of lines above the live viewport (the grid's history size),
    /// and `display_offset` is how far the view is scrolled into it. Only placements belonging to
    /// the grid currently on screen are returned: the alternate screen has an absolute-line space
    /// of its own, so primary-screen placements would land on unrelated rows there.
    pub(super) fn visible(
        &self,
        history_lines: usize,
        display_offset: usize,
        rows: u16,
        alt_screen: bool,
    ) -> Vec<TerminalImagePlacement> {
        let mut visible: Vec<_> = self
            .placements
            .iter()
            .filter(|placement| placement.alt_screen == alt_screen)
            .filter_map(|placement| {
                let row = placement.line as i64 - history_lines as i64 + display_offset as i64;
                if row + i64::from(placement.rows) <= 0 || row >= i64::from(rows) {
                    return None;
                }
                Some(TerminalImagePlacement {
                    image_id: placement.image_id,
                    image: self.images.get(&placement.image_id)?.image.clone(),
                    row: row.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                    col: i32::from(placement.col),
                    rows: placement.rows,
                    cols: placement.cols,
                    z: placement.z,
                    source_crop: placement.crop,
                })
            })
            .collect();
        visible.sort_by_key(|placement| placement.z);
        visible
    }

    /// Turn the placeholder cells on screen into placements.
    ///
    /// Unlike a direct placement, a virtual one is not anchored to a scrollback line: it *is* the
    /// text, so it scrolls, clears, and reflows for free, and it disappears the moment the cells
    /// holding it do. That is why these are derived per snapshot rather than stored.
    pub(super) fn placeholder_placements(
        &self,
        cells: &[PlaceholderCell],
        cell: TerminalCellSize,
    ) -> Vec<TerminalImagePlacement> {
        merge_placeholder_runs(&placeholder_runs(cells))
            .into_iter()
            .filter_map(|rect| {
                let stored = self.images.get(&rect.image_id)?;
                let (width, height) = (stored.image.width(), stored.image.height());
                // The source region a rect covers. A virtual placement said how many cells the
                // whole image spans, so a cell's share of the pixels follows from that; without one
                // the only thing left to read positions against is the grid the pixels themselves
                // imply. Clamped either way: a rect running past the pixels it names is a sender
                // that rounded up, not an image that should stretch.
                let (x, y, crop_width, crop_height) = match stored.virtual_cells {
                    Some((cols, rows)) if cols != 0 && rows != 0 => (
                        width * u32::from(rect.image_col) / cols,
                        height * u32::from(rect.image_row) / rows,
                        width * u32::from(rect.width) / cols,
                        height * u32::from(rect.height) / rows,
                    ),
                    _ => (
                        u32::from(rect.image_col) * u32::from(cell.width),
                        u32::from(rect.image_row) * u32::from(cell.height),
                        u32::from(rect.width) * u32::from(cell.width),
                        u32::from(rect.height) * u32::from(cell.height),
                    ),
                };
                if x >= width || y >= height {
                    return None;
                }
                let crop = TerminalImageCrop {
                    x,
                    y,
                    width: crop_width.min(width - x).max(1),
                    height: crop_height.min(height - y).max(1),
                };
                Some(TerminalImagePlacement {
                    image_id: rect.image_id,
                    image: stored.image.clone(),
                    row: i32::from(rect.row),
                    col: i32::from(rect.col),
                    rows: rect.height,
                    cols: rect.width,
                    z: 0,
                    source_crop: Some(crop),
                })
            })
            .collect()
    }

    /// Handle one command.
    pub(super) fn apply(
        &mut self,
        command: GraphicsCommand,
        ctx: GraphicsContext,
    ) -> GraphicsOutcome {
        self.clock = self.clock.wrapping_add(1);
        match command.action {
            GraphicsAction::Query => self.query(&command),
            GraphicsAction::Delete => {
                self.delete(&command, ctx);
                GraphicsOutcome::default()
            }
            GraphicsAction::Display => self.display_stored(&command, ctx),
            GraphicsAction::Transmit | GraphicsAction::TransmitAndDisplay => {
                self.transmit(command, ctx)
            }
            GraphicsAction::Animate => GraphicsOutcome {
                response: report(&command, command.id, Err("ENOTSUPP:animation")),
                advance: None,
            },
        }
    }

    /// Answer a capability probe without storing anything.
    ///
    /// A probe carries a real (tiny) payload, so it is validated exactly like a transmission: the
    /// client learns from the answer whether this terminal understands the format it wants to use.
    fn query(&mut self, command: &GraphicsCommand) -> GraphicsOutcome {
        let result = self
            .resolve_medium(command, command.payload.clone())
            .and_then(|payload| decode_payload(command, payload).map(|_| ()));
        GraphicsOutcome {
            response: report(command, command.id, result),
            advance: None,
        }
    }

    /// Turn a command's payload into pixels, fetching them when the command only named where they
    /// are. A direct payload is already the pixels and passes straight through.
    fn resolve_medium(
        &self,
        command: &GraphicsCommand,
        payload: Vec<u8>,
    ) -> Result<Vec<u8>, &'static str> {
        if !command.medium.is_out_of_band() {
            return Ok(payload);
        }
        if !self.media.allows(command.medium) {
            return Err("ENOTSUPP:file transmission");
        }
        graphics_media::load(
            command.medium,
            &payload,
            command.source_offset,
            command.source_size,
            MAX_TRANSMIT_BYTES,
        )
    }

    fn transmit(&mut self, command: GraphicsCommand, ctx: GraphicsContext) -> GraphicsOutcome {
        if command.more || self.pending.is_some() {
            return self.transmit_chunked(command, ctx);
        }
        let id = self.resolve_id(command.id, command.number);
        let payload = command.payload.clone();
        self.finish_transmit(id, &command, payload, ctx)
    }

    /// Accumulate a `m=1` run.
    ///
    /// Only the first chunk carries the format and display keys; every later chunk is payload with
    /// an `m` flag, so the first chunk's command is what the completed transmission is judged by.
    fn transmit_chunked(
        &mut self,
        command: GraphicsCommand,
        ctx: GraphicsContext,
    ) -> GraphicsOutcome {
        let mut pending = self.pending.take().unwrap_or_else(|| PendingTransmit {
            id: 0,
            header: GraphicsCommand {
                payload: Vec::new(),
                payload_len: 0,
                ..command.clone()
            },
            data: Vec::new(),
        });
        if pending.id == 0 {
            pending.id = self.resolve_id(pending.header.id, pending.header.number);
        }

        if pending
            .header
            .payload_len
            .saturating_add(command.payload_len)
            > MAX_TRANSMIT_BYTES
        {
            return GraphicsOutcome {
                response: report(&command, pending.id, Err("EFBIG:payload too large")),
                advance: None,
            };
        }
        pending.header.payload_len = pending
            .header
            .payload_len
            .saturating_add(command.payload_len);
        pending.data.extend_from_slice(&command.payload);

        if command.more {
            self.pending = Some(pending);
            return GraphicsOutcome::default();
        }
        self.finish_transmit(pending.id, &pending.header, pending.data, ctx)
    }

    fn finish_transmit(
        &mut self,
        id: u32,
        command: &GraphicsCommand,
        payload: Vec<u8>,
        ctx: GraphicsContext,
    ) -> GraphicsOutcome {
        if !self.storage_enabled {
            let declared = if command.medium.is_out_of_band() && !self.media.allows(command.medium)
            {
                Err("ENOTSUPP:file transmission")
            } else {
                validate_transmit_dimensions(command, &payload)
            };
            let dimensions = match declared {
                Ok(dimensions) => dimensions,
                Err(error) => {
                    return GraphicsOutcome {
                        response: report(command, id, Err(error)),
                        advance: None,
                    };
                }
            };
            self.image_dimensions.insert(id, dimensions);
            if command.number != 0 {
                self.numbers.insert(command.number, id);
            }
            return GraphicsOutcome {
                response: report(command, id, Ok(())),
                advance: (command.action == GraphicsAction::TransmitAndDisplay)
                    .then(|| self.place(id, command, ctx))
                    .flatten(),
            };
        }

        // Identity is taken from what the child *named*, before the pixels are loaded. An out-of-band
        // transmission's payload is a path, tens of bytes; hashing the 2 MB it points at would be
        // most of a scrolling pane's remaining CPU, and a child that reuses a path (a ring of files
        // rewritten in place) would then hash as identical frames. The serial is what keeps those
        // distinct. Inline pixels are still hashed in full: they are the identity, and there is no
        // cheaper one.
        let source_hash = if command.medium.is_out_of_band() {
            self.named_source_identity(command.format, &payload)
        } else {
            hash_payload(command.format, &payload)
        };
        let payload = match self.resolve_medium(command, payload) {
            Ok(payload) => payload,
            Err(error) => {
                return GraphicsOutcome {
                    response: report(command, id, Err(error)),
                    advance: None,
                };
            }
        };
        let (image, bytes) = match deferrable(command) {
            // Nothing is waiting to hear whether this decodes and the size is already known, so the
            // work waits until something asks to draw it. A frame superseded before then - the
            // common case for a sender streaming under one id - is dropped without ever being
            // inflated. See [`ImageSource`].
            Some((width, height, channels)) => {
                let bytes = (width as usize)
                    .saturating_mul(height as usize)
                    .saturating_mul(channels);
                let image = TerminalImage {
                    source: ImageSource::Deferred {
                        input: Arc::new(DeferredDecode {
                            payload,
                            compressed: command.compressed,
                            format: command.format,
                            width,
                            height,
                        }),
                        decoded: std::sync::OnceLock::new(),
                    },
                    width,
                    height,
                    source_hash,
                    stream_namespace: self.stream_namespace,
                };
                (image, bytes)
            }
            None => {
                let decoded = match decode_payload(command, payload) {
                    Ok(image) => image,
                    Err(error) => {
                        return GraphicsOutcome {
                            response: report(command, id, Err(error)),
                            advance: None,
                        };
                    }
                };
                let bytes = decoded_bytes(&decoded);
                let image = TerminalImage {
                    width: decoded.width(),
                    height: decoded.height(),
                    source: ImageSource::Decoded(Arc::new(decoded)),
                    source_hash,
                    stream_namespace: self.stream_namespace,
                };
                (image, bytes)
            }
        };
        self.insert_image(id, image, bytes);
        if command.number != 0 {
            self.numbers.insert(command.number, id);
        }

        GraphicsOutcome {
            response: report(command, id, Ok(())),
            advance: (command.action == GraphicsAction::TransmitAndDisplay)
                .then(|| self.place(id, command, ctx))
                .flatten(),
        }
    }

    /// Identity of a transmission that named its pixels rather than carrying them.
    ///
    /// The payload here is the path or shm name, not the pixels. Hashing those pixels would spend
    /// most of a scrolling pane's remaining CPU on a walk whose answer we already know: this is a
    /// new transmission. The serial is mixed in because a child that rewrites a file in place
    /// (a ring of paths) would otherwise hash as the same frame forever.
    ///
    /// The serial is spread across the word before it is combined: XORed in raw it only ever
    /// touches the low bits, so two paths would collide whenever their name hashes differed by the
    /// small number that separates two serials, rather than by a full word.
    fn named_source_identity(&mut self, format: u32, name: &[u8]) -> u64 {
        const PRIME: u64 = 0x9E37_79B9_7F4A_7C15;

        self.source_serial = self.source_serial.wrapping_add(1);
        hash_payload(format, name) ^ self.source_serial.wrapping_mul(PRIME).rotate_left(31)
    }

    fn display_stored(
        &mut self,
        command: &GraphicsCommand,
        ctx: GraphicsContext,
    ) -> GraphicsOutcome {
        let id = match self.lookup(command.id, command.number) {
            Some(id) => id,
            None => {
                return GraphicsOutcome {
                    response: report(command, command.id, Err("ENOENT:no such image")),
                    advance: None,
                };
            }
        };
        let advance = self.place(id, command, ctx);
        GraphicsOutcome {
            response: report(command, id, Ok(())),
            advance,
        }
    }

    /// Add a placement, returning the cursor movement it implies.
    fn place(
        &mut self,
        id: u32,
        command: &GraphicsCommand,
        ctx: GraphicsContext,
    ) -> Option<(u16, u16)> {
        let (image_w, image_h) = *self.image_dimensions.get(&id)?;
        if let Some(stored) = self.images.get_mut(&id) {
            stored.used = self.clock;
        }
        // A virtual placement draws nothing here and moves nothing: the sender goes on to write
        // placeholder cells naming this image, and those are what put it on screen. What it does
        // carry is the box those cells are positions in, which is the only statement of how big the
        // image is meant to be - a frame drawn at twice the cell resolution is read as covering half
        // as many cells without it.
        if command.virtual_placement {
            if command.cols != 0
                && command.rows != 0
                && let Some(stored) = self.images.get_mut(&id)
            {
                stored.virtual_cells = Some((command.cols, command.rows));
            }
            return None;
        }
        if image_w == 0 || image_h == 0 {
            return None;
        }

        let crop = source_crop(command, image_w, image_h);
        let (src_w, src_h) = crop
            .map(|crop| (crop.width, crop.height))
            .unwrap_or((image_w, image_h));

        // Cells the image occupies: what the client asked for, else what its pixels need.
        let cols = match command.cols {
            0 => src_w.div_ceil(u32::from(ctx.cell.width)),
            cols => cols,
        };
        let rows = match command.rows {
            0 => src_h.div_ceil(u32::from(ctx.cell.height)),
            rows => rows,
        };
        let cols = cols.clamp(1, u32::from(ctx.cols.max(1))) as u16;
        let rows = rows.clamp(1, u32::from(u16::MAX)) as u16;

        if self.storage_enabled {
            // A second placement with the same ids replaces the first, as the protocol specifies.
            self.placements.retain(|placement| {
                placement.image_id != id || placement.placement_id != command.placement
            });
            self.placements.push(Placement {
                image_id: id,
                placement_id: command.placement,
                line: ctx.cursor_line,
                col: ctx.cursor_col,
                rows,
                cols,
                z: command.z,
                crop,
                alt_screen: ctx.alt_screen,
            });
            while self.placements.len() > MAX_PLACEMENTS {
                self.placements.remove(0);
            }
        }

        (!command.no_cursor_move).then_some((rows, cols))
    }

    fn delete(&mut self, command: &GraphicsCommand, ctx: GraphicsContext) {
        // An uppercase selector also frees the image data; lowercase only drops placements.
        let free_data = command.delete.is_ascii_uppercase();
        let selector = command.delete.to_ascii_lowercase();
        // Screen-addressed deletes use 1-based viewport coordinates.
        let target_col = command.src_x.saturating_sub(1).min(u32::from(u16::MAX)) as u16;
        let target_line = ctx
            .viewport_top_line
            .saturating_add(command.src_y.saturating_sub(1) as usize);

        let hit: Box<dyn Fn(&Placement) -> bool> = match selector {
            b'a' => Box::new(|_| true),
            b'i' => {
                let (id, placement) = (command.id, command.placement);
                Box::new(move |item| {
                    item.image_id == id && (placement == 0 || item.placement_id == placement)
                })
            }
            b'n' => {
                let id = self.numbers.get(&command.number).copied().unwrap_or(0);
                Box::new(move |item| id != 0 && item.image_id == id)
            }
            b'c' => {
                let (line, col) = (ctx.cursor_line, ctx.cursor_col);
                Box::new(move |item| item.covers_cell(line, col))
            }
            b'z' => {
                let z = command.z;
                Box::new(move |item| item.z == z)
            }
            b'p' => Box::new(move |item| item.covers_cell(target_line, target_col)),
            b'x' => Box::new(move |item| item.covers_column(target_col)),
            b'y' => Box::new(move |item| item.covers_line(target_line)),
            _ => return,
        };

        let mut freed: Vec<u32> = Vec::new();
        self.placements.retain(|item| {
            if !hit(item) {
                return true;
            }
            if free_data {
                freed.push(item.image_id);
            }
            false
        });

        if free_data {
            match selector {
                // "Delete all" frees every stored image, placed or not.
                b'a' => {
                    let ids: Vec<u32> = self.image_dimensions.keys().copied().collect();
                    for id in ids {
                        self.remove_image(id);
                    }
                }
                b'i' if command.placement == 0 => self.remove_image(command.id),
                b'n' => {
                    if let Some(id) = self.numbers.get(&command.number).copied() {
                        self.remove_image(id);
                    }
                }
                _ => {
                    for id in freed {
                        self.remove_image(id);
                    }
                }
            }
        }
    }

    fn insert_image(&mut self, id: u32, image: TerminalImage, bytes: usize) {
        // The cell box belongs to the id's virtual placement, not to one frame's pixels: the
        // protocol has a sender declare it once and then send frame after frame under the same id.
        let virtual_cells = self.images.get(&id).and_then(|stored| stored.virtual_cells);
        self.remove_image(id);
        self.image_dimensions
            .insert(id, (image.width(), image.height()));
        let clock = self.clock;
        self.images.insert(
            id,
            StoredImage {
                image,
                bytes,
                used: clock,
                virtual_cells,
            },
        );
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        self.enforce_budget();
    }

    fn remove_image(&mut self, id: u32) {
        if let Some(stored) = self.images.remove(&id) {
            self.used_bytes = self.used_bytes.saturating_sub(stored.bytes);
        }
        self.image_dimensions.remove(&id);
        self.numbers.retain(|_, mapped| *mapped != id);
        self.placements.retain(|placement| placement.image_id != id);
    }

    /// Drop least-recently-used images until the decoded-pixel budget is met.
    ///
    /// Placed images are not exempt: a session that keeps drawing must not be able to pin memory
    /// just by leaving old plots on screen. The placement goes with the pixels, so an evicted
    /// image disappears rather than rendering as a hole.
    ///
    /// The last image standing is never evicted. One picture larger than the whole budget is a
    /// budget that is too small, not a picture that should silently fail to appear.
    fn enforce_budget(&mut self) {
        while self.used_bytes > self.budget && self.images.len() > 1 {
            let victim = self
                .images
                .iter()
                .min_by_key(|(_, stored)| (stored.used, stored.bytes))
                .map(|(id, _)| *id);
            let Some(victim) = victim else { break };
            self.remove_image(victim);
        }
    }

    /// The id a command addresses, for commands that do not create one.
    fn lookup(&self, id: u32, number: u32) -> Option<u32> {
        if id != 0 {
            return self.image_dimensions.contains_key(&id).then_some(id);
        }
        let mapped = *self.numbers.get(&number)?;
        self.image_dimensions
            .contains_key(&mapped)
            .then_some(mapped)
    }

    /// The id a transmission stores under, assigning one when the client did not.
    fn resolve_id(&mut self, id: u32, number: u32) -> u32 {
        if id != 0 {
            return id;
        }
        if number != 0
            && let Some(existing) = self.numbers.get(&number).copied()
        {
            return existing;
        }
        let assigned = self.next_auto_id;
        self.next_auto_id = self.next_auto_id.checked_add(1).unwrap_or(FIRST_AUTO_ID);
        assigned
    }
}

fn source_crop(command: &GraphicsCommand, width: u32, height: u32) -> Option<TerminalImageCrop> {
    if command.src_x == 0 && command.src_y == 0 && command.src_w == 0 && command.src_h == 0 {
        return None;
    }
    let x = command.src_x.min(width.saturating_sub(1));
    let y = command.src_y.min(height.saturating_sub(1));
    let w = match command.src_w {
        0 => width - x,
        requested => requested.min(width - x),
    };
    let h = match command.src_h {
        0 => height - y,
        requested => requested.min(height - y),
    };
    (w > 0 && h > 0).then_some(TerminalImageCrop {
        x,
        y,
        width: w,
        height: h,
    })
}

/// Decode a transmitted payload into pixels, or say why it could not be.
///
/// Errors are the protocol's own codes, so they can be reported straight back to the child.
fn validate_transmit_dimensions(
    command: &GraphicsCommand,
    payload: &[u8],
) -> Result<(u32, u32), &'static str> {
    // An out-of-band command names its pixels instead of carrying them, and reading the source is
    // how `t=t` and `t=s` are claimed. A screen that only tracks metadata must not claim one out
    // from under the reader that actually needs it, so it goes on what the command declared.
    if command.medium.is_out_of_band() {
        if command.width > MAX_IMAGE_DIMENSION || command.height > MAX_IMAGE_DIMENSION {
            return Err("EFBIG:image too large");
        }
        if command.format != 100 && (command.width == 0 || command.height == 0) {
            return Err("EINVAL:missing s/v for raw pixels");
        }
        return Ok((command.width, command.height));
    }

    match command.format {
        format @ (24 | 32) => {
            let (width, height) = (command.width, command.height);
            if width == 0 || height == 0 {
                return Err("EINVAL:missing s/v for raw pixels");
            }
            if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
                return Err("EFBIG:image too large");
            }
            if !command.compressed {
                let channels = if format == 24 { 3usize } else { 4usize };
                let expected = (width as usize)
                    .checked_mul(height as usize)
                    .and_then(|pixels| pixels.checked_mul(channels))
                    .ok_or("EFBIG:image too large")?;
                if command.payload_len < expected {
                    return Err("EINVAL:truncated pixel payload");
                }
            }
            Ok((width, height))
        }
        // Only the compressed size is known up front for a PNG, so its dimensions cost a decode.
        // This runs on a metadata-only screen, which never sees a raw frame's pixels at all.
        100 => {
            decode_payload(command, payload.to_vec()).map(|image| (image.width(), image.height()))
        }
        _ => Err("ENOTSUPP:unsupported format"),
    }
}

/// Turn a transmitted payload into pixels, consuming it.
///
/// By value rather than by reference because raw formats hand the buffer straight to the image: a
/// full window of pixels arriving sixty times a second cannot afford a copy that exists only to
/// satisfy a signature.
/// Whether a transmission's decode can be put off, and the size it will decode to.
///
/// Two conditions, both strict:
///
/// - **`q=2`.** Deferring moves the moment a bad payload is discovered from the transmission to the
///   first draw, and by then there is nobody to tell. Under `q=2` the child has asked for no
///   reports at all - neither success nor failure - so nothing is lost. `q=0` and `q=1` both still
///   hear about errors, and are decoded immediately so that they do.
/// - **Raw pixels with a declared size.** The dimensions have to be known without decoding, because
///   layout asks for them long before anything draws. `f=24`/`f=32` carry them in `s`/`v`; a PNG
///   only reveals its size to the decoder.
///
/// Between them these describe a sender streaming frames, which is the only case where the saving
/// exists in the first place.
fn deferrable(command: &GraphicsCommand) -> Option<(u32, u32, usize)> {
    if command.quiet < 2 {
        return None;
    }
    let channels = match command.format {
        24 => 3usize,
        32 => 4usize,
        _ => return None,
    };
    let (width, height) = (command.width, command.height);
    if width == 0 || height == 0 || width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return None;
    }
    Some((width, height, channels))
}

/// Run a decode that [`ImageSource::Deferred`] put off.
fn decode_deferred(input: &DeferredDecode) -> Option<DynamicImage> {
    decode_payload(
        &GraphicsCommand {
            compressed: input.compressed,
            format: input.format,
            width: input.width,
            height: input.height,
            ..GraphicsCommand::default()
        },
        input.payload.clone(),
    )
    .ok()
}

fn decode_payload(
    command: &GraphicsCommand,
    payload: Vec<u8>,
) -> Result<DynamicImage, &'static str> {
    let mut data = if command.compressed {
        decompress(&payload).ok_or("EINVAL:bad zlib payload")?
    } else {
        payload
    };

    match command.format {
        100 => decode_png(&data),
        format @ (24 | 32) => {
            let channels = if format == 24 { 3usize } else { 4usize };
            let (width, height) = (command.width, command.height);
            if width == 0 || height == 0 {
                return Err("EINVAL:missing s/v for raw pixels");
            }
            if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
                return Err("EFBIG:image too large");
            }
            let expected = (width as usize)
                .checked_mul(height as usize)
                .and_then(|pixels| pixels.checked_mul(channels))
                .ok_or("EFBIG:image too large")?;
            if data.len() < expected {
                return Err("EINVAL:truncated pixel payload");
            }
            data.truncate(expected);
            if channels == 3 {
                image::RgbImage::from_raw(width, height, data).map(DynamicImage::ImageRgb8)
            } else {
                image::RgbaImage::from_raw(width, height, data).map(DynamicImage::ImageRgba8)
            }
            .ok_or("EINVAL:bad pixel payload")
        }
        _ => Err("ENOTSUPP:unsupported format"),
    }
}

fn decode_png(data: &[u8]) -> Result<DynamicImage, &'static str> {
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(data), image::ImageFormat::Png);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_TRANSMIT_BYTES as u64);
    reader.limits(limits);
    reader.decode().map_err(|_| "EINVAL:bad PNG payload")
}

fn decompress(payload: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read as _;

    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(payload)
        .take(MAX_TRANSMIT_BYTES as u64)
        .read_to_end(&mut out)
        .ok()?;
    Some(out)
}

fn decoded_bytes(image: &DynamicImage) -> usize {
    (image.width() as usize)
        .saturating_mul(image.height() as usize)
        .saturating_mul(4)
}

/// Identity of a transmitted payload, for the render cache.
///
/// Not a general-purpose hash: it exists so the renderer can see that pixels changed. A collision
/// costs one stale frame on screen. Inline transmissions still hash every pixel, so this is on the
/// hot path in a way its size suggests it is not: a 1.5 MB frame at 60 fps is 90 MB/s through here,
/// and one dependent multiply per eight bytes held that to about 5.6 GB/s. Out-of-band
/// transmissions skip this walk: they take identity from the name and a serial.
///
/// Four lanes fix that without changing the mixing: the multiplies within a 32-byte block do not
/// depend on each other, so the processor overlaps them instead of waiting out each latency in
/// turn, and the loop becomes limited by memory rather than by arithmetic. The values differ from
/// the single-lane version, which costs nothing - the hash keys an in-process cache and is never
/// persisted, sent to the server, or compared across builds.
fn hash_payload(format: u32, payload: &[u8]) -> u64 {
    const PRIME: u64 = 0x9E37_79B9_7F4A_7C15;
    const LANES: usize = 4;
    const BLOCK: usize = LANES * 8;

    let seed = u64::from(format)
        .wrapping_mul(PRIME)
        .wrapping_add(payload.len() as u64);
    // Distinct starting points, so a block of identical words does not leave the lanes equal.
    let mut lanes = [
        seed,
        seed.wrapping_add(PRIME),
        seed.wrapping_add(PRIME.wrapping_mul(2)),
        seed.wrapping_add(PRIME.wrapping_mul(3)),
    ];

    let mut blocks = payload.chunks_exact(BLOCK);
    for block in &mut blocks {
        for (lane, word) in lanes.iter_mut().zip(block.chunks_exact(8)) {
            let word = u64::from_le_bytes(word.try_into().expect("eight bytes"));
            *lane = (*lane ^ word).wrapping_mul(PRIME).rotate_left(31);
        }
    }

    // Whatever did not fill a block folds into one lane, position included so that the same bytes
    // arriving at a different offset do not hash alike.
    let mut state = lanes[0];
    let mut words = blocks.remainder().chunks_exact(8);
    for word in &mut words {
        let word = u64::from_le_bytes(word.try_into().expect("eight bytes"));
        state = (state ^ word).wrapping_mul(PRIME).rotate_left(31);
    }
    let mut tail = 0u64;
    for (index, byte) in words.remainder().iter().enumerate() {
        tail |= u64::from(*byte) << (index * 8);
    }
    state = (state ^ tail).wrapping_mul(PRIME);

    for lane in &lanes[1..] {
        state = (state ^ lane).wrapping_mul(PRIME).rotate_left(27);
    }
    state ^= state >> 29;
    state = state.wrapping_mul(PRIME);
    state ^ (state >> 32)
}

/// Build the protocol's report for a command, when it asked for one.
fn report(command: &GraphicsCommand, id: u32, result: Result<(), &str>) -> Option<Vec<u8>> {
    if !command.reports(result.is_ok()) {
        return None;
    }
    let mut response = format!("\x1b_Gi={id}");
    if command.number != 0 {
        let _ = write!(response, ",I={}", command.number);
    }
    if command.placement != 0 {
        let _ = write!(response, ",p={}", command.placement);
    }
    let body = result.err().unwrap_or("OK");
    let _ = write!(response, ";{body}\x1b\\");
    Some(response.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A base64 direct-transmission command for a `width` x `height` solid RGB image.
    fn rgb_command(keys: &str, width: u32, height: u32) -> Vec<u8> {
        let pixels = vec![0xa0u8; (width * height * 3) as usize];
        let payload = BASE64.encode(pixels);
        format!("\x1b_Gf=24,s={width},v={height},t=d,{keys};{payload}\x1b\\").into_bytes()
    }

    /// A cache key that missed a changed byte would leave a stale frame on screen, and one that
    /// ignored position would do the same for a frame whose content merely moved.
    #[test]
    fn the_payload_hash_separates_frames_that_differ_anywhere() {
        // Longer than one 32-byte block, and deliberately not a whole number of them, so the
        // block loop, the word remainder and the byte tail are all exercised.
        let base: Vec<u8> = (0..205u32).map(|byte| byte as u8).collect();
        let hash = hash_payload(24, &base);

        for index in [0usize, 7, 8, 31, 32, 100, 199, 200, 204] {
            let mut changed = base.clone();
            changed[index] ^= 0x01;
            assert_ne!(
                hash_payload(24, &changed),
                hash,
                "a byte at {index} changed without changing the hash"
            );
        }

        assert_ne!(
            hash_payload(32, &base),
            hash,
            "the format is part of the identity"
        );
        assert_ne!(
            hash_payload(24, &base[..204]),
            hash,
            "the length is part of the identity"
        );

        let mut swapped = base.clone();
        swapped.swap(0, 8);
        assert_ne!(
            hash_payload(24, &swapped),
            hash,
            "two words exchanged is a different frame, not the same one reordered"
        );
        assert_eq!(hash_payload(24, &base), hash, "and it is stable");
    }

    /// Deferring a decode moves the moment a bad payload is noticed to the first draw, so it is
    /// allowed only where nobody is listening for the verdict and the size is already known.
    #[test]
    fn only_a_quiet_raw_transmission_may_have_its_decode_deferred() {
        let raw_quiet = GraphicsCommand {
            quiet: 2,
            format: 24,
            width: 4,
            height: 4,
            ..GraphicsCommand::default()
        };
        assert_eq!(deferrable(&raw_quiet), Some((4, 4, 3)));
        assert_eq!(
            deferrable(&GraphicsCommand {
                format: 32,
                ..raw_quiet.clone()
            }),
            Some((4, 4, 4)),
            "an alpha channel is still raw"
        );

        for quiet in [0, 1] {
            assert_eq!(
                deferrable(&GraphicsCommand {
                    quiet,
                    ..raw_quiet.clone()
                }),
                None,
                "q={quiet} still hears about a payload that does not decode"
            );
        }
        assert_eq!(
            deferrable(&GraphicsCommand {
                format: 100,
                ..raw_quiet.clone()
            }),
            None,
            "a PNG only tells the decoder how big it is"
        );
        assert_eq!(
            deferrable(&GraphicsCommand {
                width: 0,
                ..raw_quiet.clone()
            }),
            None,
            "no declared size to lay out against"
        );
    }

    /// The pixels a deferred decode produces are the pixels an immediate one would have.
    #[test]
    fn a_deferred_decode_yields_the_same_image() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &rgb_command("q=2,i=7,a=T", 3, 2));
        for command in commands {
            graphics.apply(command, context());
        }
        let stored = &graphics.images.get(&7).expect("stored image").image;
        assert!(
            matches!(stored.source, ImageSource::Deferred { .. }),
            "a quiet raw transmission is kept undecoded"
        );
        // Declared before anything is decoded, because layout asks long before the first draw.
        assert_eq!((stored.width(), stored.height()), (3, 2));
        let pixels = stored.pixels().expect("decodes on demand");
        assert_eq!(pixels.width(), 3);
        assert_eq!(pixels.height(), 2);
        assert_eq!(pixels.to_rgb8().as_raw(), &vec![0xa0u8; 3 * 2 * 3]);
    }

    fn context() -> GraphicsContext {
        GraphicsContext {
            cursor_line: 0,
            cursor_col: 0,
            viewport_top_line: 0,
            alt_screen: false,
            cell: TerminalCellSize::new(10, 20),
            cols: 80,
        }
    }

    fn scan_all(scanner: &mut GraphicsScanner, bytes: &[u8]) -> (Vec<u8>, Vec<GraphicsCommand>) {
        let mut text = Vec::new();
        let mut commands = Vec::new();
        for segment in scanner.scan(bytes) {
            match segment {
                GraphicsSegment::Text(range) => text.extend_from_slice(&bytes[range]),
                GraphicsSegment::HeldEscape => text.push(0x1b),
                GraphicsSegment::Command(command) => commands.push(*command),
            }
        }
        (text, commands)
    }

    #[test]
    fn image_stream_namespaces_are_unique_per_screen_and_reset() {
        let mut first = TerminalGraphics::default();
        let second = TerminalGraphics::default();
        assert_ne!(first.stream_namespace, second.stream_namespace);

        let before_reset = first.stream_namespace;
        first.reset();
        assert_ne!(first.stream_namespace, before_reset);
        assert_ne!(first.stream_namespace, second.stream_namespace);
    }

    #[test]
    fn scanner_lifts_commands_out_of_surrounding_text() {
        let mut scanner = GraphicsScanner::default();
        let mut stream = b"before".to_vec();
        stream.extend_from_slice(&rgb_command("a=T", 2, 2));
        stream.extend_from_slice(b"after");

        let (text, commands) = scan_all(&mut scanner, &stream);
        assert_eq!(text, b"beforeafter");
        assert_eq!(commands.len(), 1);
        assert_eq!(commands[0].action, GraphicsAction::TransmitAndDisplay);
        assert_eq!(commands[0].payload.len(), 2 * 2 * 3);
    }

    #[test]
    fn scanner_survives_a_command_split_across_chunks() {
        let command = rgb_command("a=T", 2, 2);
        // Every split point must produce the same command: the PTY chooses where chunks land.
        for split in 1..command.len() {
            let mut scanner = GraphicsScanner::default();
            let (head_text, head) = scan_all(&mut scanner, &command[..split]);
            let (tail_text, tail) = scan_all(&mut scanner, &command[split..]);
            assert!(
                head_text.is_empty() && tail_text.is_empty(),
                "split at {split} leaked graphics bytes into the grid stream"
            );
            assert_eq!(
                head.len() + tail.len(),
                1,
                "split at {split} lost or duplicated the command"
            );
        }
    }

    #[test]
    fn escape_that_is_not_a_command_reaches_the_grid() {
        let mut scanner = GraphicsScanner::default();
        // Split so the chunk ends on the bare ESC, the case `is_plain` must refuse.
        assert!(!scanner.is_plain(b"red\x1b"));
        let (first, _) = scan_all(&mut scanner, b"red\x1b");
        let (second, commands) = scan_all(&mut scanner, b"[0m");
        let mut text = first;
        text.extend_from_slice(&second);
        assert_eq!(text, b"red\x1b[0m");
        assert!(commands.is_empty());
    }

    #[test]
    fn non_graphics_apc_is_swallowed_like_the_vt_parser_would() {
        let mut scanner = GraphicsScanner::default();
        let (text, commands) = scan_all(&mut scanner, b"a\x1b_Xsomething\x1b\\b");
        assert_eq!(text, b"ab");
        assert!(commands.is_empty());
    }

    #[test]
    fn transmit_and_display_places_the_image_and_moves_the_cursor() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        // 30x40 pixels in 10x20 cells is 3 columns by 2 rows.
        let (_, commands) = scan_all(&mut scanner, &rgb_command("a=T,i=7", 30, 40));

        let outcome = graphics.apply(commands[0].clone(), context());
        assert_eq!(outcome.advance, Some((2, 3)));

        let visible = graphics.visible(0, 0, 24, false);
        assert_eq!(visible.len(), 1);
        assert_eq!((visible[0].row, visible[0].col), (0, 0));
        assert_eq!((visible[0].rows, visible[0].cols), (2, 3));
    }

    #[test]
    fn explicit_cell_size_overrides_the_pixel_size() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &rgb_command("a=T,c=8,r=4", 30, 40));

        let outcome = graphics.apply(commands[0].clone(), context());
        assert_eq!(outcome.advance, Some((4, 8)));
    }

    #[test]
    fn suppressed_cursor_movement_still_places() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &rgb_command("a=T,C=1", 30, 40));

        let outcome = graphics.apply(commands[0].clone(), context());
        assert_eq!(outcome.advance, None);
        assert_eq!(graphics.visible(0, 0, 24, false).len(), 1);
    }

    #[test]
    fn a_probe_is_answered_without_storing_anything() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &rgb_command("a=q,i=31", 1, 1));

        let outcome = graphics.apply(commands[0].clone(), context());
        assert_eq!(
            outcome.response.as_deref(),
            Some(b"\x1b_Gi=31;OK\x1b\\".as_ref())
        );
        assert!(graphics.visible(0, 0, 24, false).is_empty());
    }

    /// Write a frame where a child would leave one, and return the `t=f` command naming it.
    fn out_of_band_frame(name: &str, width: u32, height: u32) -> (std::path::PathBuf, Vec<u8>) {
        let path = std::env::temp_dir().join(format!("tui-lipan-oob-{name}"));
        std::fs::write(&path, vec![0x7fu8; (width * height * 4) as usize]).expect("write frame");
        let named = BASE64.encode(path.to_string_lossy().as_bytes());
        let escape =
            format!("\x1b_Ga=T,t=f,f=32,s={width},v={height},i=7;{named}\x1b\\").into_bytes();
        (path, escape)
    }

    #[test]
    fn a_file_the_child_named_is_read_instead_of_the_escape_payload() {
        let (path, escape) = out_of_band_frame("read", 4, 3);
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &escape);

        let outcome = graphics.apply(commands[0].clone(), context());
        let response = String::from_utf8(outcome.response.expect("a report")).unwrap();
        assert!(response.contains("OK"), "unexpected report: {response}");

        let placed = graphics.visible(0, 0, 24, false);
        assert_eq!(placed.len(), 1);
        assert_eq!((placed[0].image.width(), placed[0].image.height()), (4, 3));
        assert!(path.exists(), "t=f leaves the frame for the next reader");
        std::fs::remove_file(&path).ok();
    }

    /// A child that rewrites a file in place (a ring of paths) would hash as the same frame if
    /// identity were the path, and hashing the pixels would spend a scrolling pane's remaining
    /// budget on a walk whose answer we already know: this is a new transmission.
    #[test]
    fn a_named_file_rewritten_in_place_is_a_new_frame() {
        let (path, escape) = out_of_band_frame("rewrite", 4, 3);
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();

        let (_, first) = scan_all(&mut scanner, &escape);
        graphics.apply(first[0].clone(), context());
        let first_hash = graphics.images.get(&7).expect("stored").image.source_hash();

        std::fs::write(&path, vec![0x11u8; 4 * 3 * 4]).expect("rewrite");
        let (_, second) = scan_all(&mut scanner, &escape);
        graphics.apply(second[0].clone(), context());
        let second_hash = graphics
            .images
            .get(&7)
            .expect("replaced")
            .image
            .source_hash();

        assert_ne!(
            first_hash, second_hash,
            "the same path with new pixels must miss the render cache"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_screen_that_only_tracks_metadata_answers_without_claiming_the_source() {
        let (path, escape) = out_of_band_frame("metadata", 4, 3);
        let mut graphics = TerminalGraphics::default();
        graphics.set_storage_enabled(false);
        let mut scanner = GraphicsScanner::default();
        scanner.set_decode_payload(false);
        let (_, commands) = scan_all(&mut scanner, &escape);

        let outcome = graphics.apply(commands[0].clone(), context());
        let response = String::from_utf8(outcome.response.expect("a report")).unwrap();
        assert!(response.contains("OK"), "unexpected report: {response}");
        assert_eq!(graphics.image_dimensions.get(&7), Some(&(4, 3)));
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_policy_without_consuming_media_keeps_the_re_readable_one() {
        let (path, escape) = out_of_band_frame("policy", 2, 2);
        let mut graphics = TerminalGraphics::default();
        graphics.set_media_policy(GraphicsMediaPolicy::SHARED);
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &escape);
        let outcome = graphics.apply(commands[0].clone(), context());
        let response = String::from_utf8(outcome.response.expect("a report")).unwrap();
        assert!(response.contains("OK"), "unexpected report: {response}");

        let claiming = escape
            .windows(4)
            .position(|window| window == b"t=f,")
            .map(|at| {
                let mut bytes = escape.clone();
                bytes[at + 2] = b't';
                bytes
            })
            .expect("the escape names its medium");
        let (_, commands) = scan_all(&mut scanner, &claiming);
        let outcome = graphics.apply(commands[0].clone(), context());
        let response = String::from_utf8(outcome.response.expect("a report")).unwrap();
        assert!(
            response.contains("ENOTSUPP"),
            "unexpected report: {response}"
        );
        assert!(
            path.exists(),
            "a refused medium must not consume the source"
        );
        std::fs::remove_file(&path).ok();
    }

    /// The exchange a child uses to decide how it will send every later frame. It asks with one
    /// pixel in each medium it can produce, and the cheapest medium that answers `OK` is the one it
    /// commits to. Answering the file probe with an error is what makes a sender fall back to
    /// pasting compressed pixels into the stream, so this answer is worth a frame rate.
    #[test]
    fn a_capability_probe_for_a_file_is_answered_from_the_file() {
        let (path, _) = out_of_band_frame("probe", 1, 1);
        let named = BASE64.encode(path.to_string_lossy().as_bytes());
        let probe = format!("\x1b_Gi=300,a=q,t=f,f=32,s=1,v=1;{named}\x1b\\").into_bytes();
        let mut graphics = TerminalGraphics::default();
        graphics.set_media_policy(GraphicsMediaPolicy::SHARED);
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &probe);

        let outcome = graphics.apply(commands[0].clone(), context());
        assert_eq!(
            outcome.response.as_deref(),
            Some(b"\x1b_Gi=300;OK\x1b\\".as_ref())
        );
        assert!(path.exists(), "a probe must not consume what it asks about");
        std::fs::remove_file(&path).ok();
    }

    /// Both ends of the shared-memory medium, checked against each other: the sequences this
    /// framework writes to a host are parsed by the same framework's terminal. A malformed query
    /// would be answered `EINVAL` rather than `OK`, which reads as "the host does not support it" and
    /// would silently keep every frame on the inline path forever.
    #[cfg(all(unix, feature = "terminal-images", not(target_arch = "wasm32")))]
    #[test]
    fn the_shared_memory_sequences_this_writes_to_a_host_are_the_ones_it_reads() {
        use crate::backend::ratatui_backend::renderers::image::kitty_transmit_shared_memory;
        use crate::backend::ratatui_backend::shared_frame::{
            SharedFrame, kitty_shared_memory_probe,
        };

        let (mut probe_frame, query) = kitty_shared_memory_probe(300).expect("probe frame");
        probe_frame.handed_over();
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, query.as_bytes());
        let outcome = graphics.apply(commands[0].clone(), context());
        assert_eq!(
            outcome.response.as_deref(),
            Some(b"\x1b_Gi=300;OK\x1b\\".as_ref()),
            "the probe a host is asked with has to be a transmission it can answer"
        );

        let mut frame = SharedFrame::write(&[0x40u8; 4 * 3 * 3]).expect("frame");
        let transmit = kitty_transmit_shared_memory(
            frame.name(),
            4,
            3,
            24,
            7,
            ratatui::layout::Size::new(6, 2),
        );
        frame.handed_over();
        let (_, commands) = scan_all(&mut scanner, transmit.as_bytes());
        graphics.apply(commands[0].clone(), context());

        // A `U=1` transmission draws nothing where it happens, so what proves it arrived is the
        // stored image the placeholder cells will later name.
        let stored = graphics.images.get(&7).expect("stored under its own id");
        assert_eq!((stored.image.width(), stored.image.height()), (4, 3));

        // And the cell box travels with it. The renderer transmits a frame at whatever resolution it
        // arrived in and leaves the fitting to the terminal, so the box has to be what placeholder
        // positions are read against: against the pixels' own cell grid instead, these six columns
        // of a four-pixel-wide image would each claim a whole cell's worth and show the first
        // fraction of it magnified.
        let placement = whole_box_placement(&graphics, 7, 6, 2);
        assert_eq!((placement.cols, placement.rows), (6, 2));
        let crop = placement.source_crop.expect("a source region");
        assert_eq!((crop.x, crop.y, crop.width, crop.height), (0, 0, 4, 3));
    }

    /// The placement a full box of placeholder cells resolves to, which is what a host showing the
    /// image actually draws from.
    #[cfg(feature = "terminal-images")]
    fn whole_box_placement(
        graphics: &TerminalGraphics,
        image_id: u32,
        cols: u16,
        rows: u16,
    ) -> TerminalImagePlacement {
        let cells: Vec<PlaceholderCell> = (0..rows)
            .flat_map(|row| {
                (0..cols).map(move |col| {
                    PlaceholderCell::new(row, col, image_id, &[diacritic(row), diacritic(col)])
                })
            })
            .collect();
        let mut placements = graphics.placeholder_placements(&cells, TerminalCellSize::new(10, 20));
        assert_eq!(placements.len(), 1, "one image, one rectangle of cells");
        placements.remove(0)
    }

    #[test]
    fn out_of_band_transmission_is_refused_when_the_policy_allows_nothing() {
        let mut graphics = TerminalGraphics::default();
        graphics.set_media_policy(GraphicsMediaPolicy::NONE);
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, b"\x1b_Ga=T,t=f,i=3;L3RtcC9pbWcucG5n\x1b\\");

        let outcome = graphics.apply(commands[0].clone(), context());
        let response = String::from_utf8(outcome.response.expect("a refusal is reported")).unwrap();
        assert!(
            response.contains("ENOTSUPP"),
            "unexpected report: {response}"
        );
    }

    #[test]
    fn quiet_two_suppresses_even_failures() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, b"\x1b_Ga=T,t=f,q=2;Lw==\x1b\\");

        assert!(
            graphics
                .apply(commands[0].clone(), context())
                .response
                .is_none()
        );
    }

    #[test]
    fn chunked_transmission_reassembles_before_decoding() {
        let mut graphics = TerminalGraphics::default();
        let pixels = vec![0x40u8; 30 * 40 * 3];
        let encoded = BASE64.encode(&pixels);
        let (head, tail) = encoded.split_at(encoded.len() / 2);

        let mut scanner = GraphicsScanner::default();
        let mut stream = format!("\x1b_Ga=T,f=24,s=30,v=40,t=d,i=9,m=1;{head}\x1b\\").into_bytes();
        stream.extend_from_slice(format!("\x1b_Gm=0;{tail}\x1b\\").as_bytes());
        let (_, commands) = scan_all(&mut scanner, &stream);
        assert_eq!(commands.len(), 2);

        assert!(
            graphics
                .apply(commands[0].clone(), context())
                .advance
                .is_none()
        );
        let outcome = graphics.apply(commands[1].clone(), context());
        assert_eq!(outcome.advance, Some((2, 3)));
        assert_eq!(graphics.visible(0, 0, 24, false).len(), 1);
    }

    #[test]
    fn renderer_compressed_kitty_output_round_trips() {
        use std::io::Write as _;

        let pixels = vec![0x40u8; 30 * 40 * 4];
        let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::fast());
        encoder.write_all(&pixels).unwrap();
        let compressed = encoder.finish().unwrap();
        let wire = crate::backend::ratatui_backend::renderers::image::kitty_transmit_compressed_for(
            &compressed,
            30,
            40,
            33,
            ratatui::layout::Size::new(5, 4),
            false,
        );

        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, wire.as_bytes());
        for command in commands {
            graphics.apply(command, context());
        }

        let decoded = graphics
            .images
            .get(&33)
            .unwrap()
            .image
            .pixels()
            .expect("pixels decode")
            .to_rgba8();
        assert_eq!(decoded.as_raw(), &pixels);
        let placement = whole_box_placement(&graphics, 33, 5, 4);
        let crop = placement.source_crop.expect("a source region");
        assert_eq!((crop.x, crop.y, crop.width, crop.height), (0, 0, 30, 40));
    }

    #[test]
    fn deleting_by_id_drops_the_placement() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &rgb_command("a=T,i=4", 30, 40));
        graphics.apply(commands[0].clone(), context());

        let (_, deletes) = scan_all(&mut scanner, b"\x1b_Ga=d,d=i,i=4;\x1b\\");
        graphics.apply(deletes[0].clone(), context());
        assert!(graphics.visible(0, 0, 24, false).is_empty());
    }

    #[test]
    fn evicted_scrollback_pulls_placements_up_and_then_off() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &rgb_command("a=T", 30, 40));
        let mut ctx = context();
        ctx.cursor_line = 5;
        graphics.apply(commands[0].clone(), ctx);

        graphics.drop_evicted(3);
        assert_eq!(graphics.visible(0, 0, 24, false)[0].row, 2);

        // Two rows tall: evicting the top row leaves the bottom one, still on screen.
        graphics.drop_evicted(3);
        assert_eq!(graphics.visible(0, 0, 24, false)[0].row, 0);

        // The placement only disappears once its last row is gone too.
        graphics.drop_evicted(4);
        assert!(graphics.visible(0, 0, 24, false).is_empty());
    }

    #[test]
    fn alt_screen_placements_are_kept_apart_from_the_primary_ones() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &rgb_command("a=T,i=1", 30, 40));
        graphics.apply(commands[0].clone(), context());

        let (_, alt) = scan_all(&mut scanner, &rgb_command("a=T,i=2", 30, 40));
        let mut alt_ctx = context();
        alt_ctx.alt_screen = true;
        graphics.apply(alt[0].clone(), alt_ctx);

        assert_eq!(graphics.visible(0, 0, 24, true).len(), 1);
        assert_eq!(graphics.visible(0, 0, 24, false).len(), 1);

        graphics.clear_alt_screen();
        assert!(graphics.visible(0, 0, 24, true).is_empty());
        assert_eq!(graphics.visible(0, 0, 24, false).len(), 1);
    }

    #[test]
    fn the_budget_evicts_least_recently_used_images() {
        let mut graphics = TerminalGraphics::default();
        // Room for one 30x40 image's decoded pixels, and no more.
        graphics.set_budget(30 * 40 * 4);

        let mut scanner = GraphicsScanner::default();
        let (_, first) = scan_all(&mut scanner, &rgb_command("a=T,i=1", 30, 40));
        graphics.apply(first[0].clone(), context());
        let (_, second) = scan_all(&mut scanner, &rgb_command("a=T,i=2", 31, 40));
        graphics.apply(second[0].clone(), context());

        let visible = graphics.visible(0, 0, 24, false);
        assert_eq!(visible.len(), 1, "the older image must have been evicted");
        assert_eq!(visible[0].image.width(), 31);
    }

    #[test]
    fn metadata_only_storage_preserves_image_cursor_movement() {
        let mut graphics = TerminalGraphics::default();
        graphics.set_storage_enabled(false);
        let mut scanner = GraphicsScanner::default();
        scanner.set_decode_payload(false);
        let (_, commands) = scan_all(&mut scanner, &rgb_command("a=T,i=7", 30, 40));
        assert!(commands[0].payload.is_empty());
        assert_eq!(commands[0].payload_len, 30 * 40 * 3);

        let outcome = graphics.apply(commands[0].clone(), context());
        assert_eq!(outcome.advance, Some((2, 3)));
        assert!(!graphics.has_images());
        assert_eq!(graphics.image_dimensions.get(&7), Some(&(30, 40)));

        let (_, display) = scan_all(&mut scanner, b"\x1b_Ga=p,i=7;\x1b\\");
        assert_eq!(
            graphics.apply(display[0].clone(), context()).advance,
            Some((2, 3))
        );
    }

    #[test]
    fn a_source_rectangle_is_carried_to_the_renderer() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (_, commands) = scan_all(&mut scanner, &rgb_command("a=T,x=5,y=6,w=10,h=12", 30, 40));
        graphics.apply(commands[0].clone(), context());

        let visible = graphics.visible(0, 0, 24, false);
        assert_eq!(
            visible[0].source_crop,
            Some(TerminalImageCrop {
                x: 5,
                y: 6,
                width: 10,
                height: 12,
            })
        );
        // The placement is sized from the crop, not from the whole image.
        assert_eq!((visible[0].rows, visible[0].cols), (1, 1));
    }

    #[test]
    fn a_large_unchunked_transmission_is_not_dropped() {
        // The protocol tells senders to chunk at 4096 base64 bytes, but plenty do not - anything
        // emitting raw pixels in one escape clears 64 KiB with a picture barely 300 cells wide.
        // Dropping those on the floor is indistinguishable, from the sender's side, from the
        // terminal not supporting graphics at all.
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        let (text, commands) = scan_all(&mut scanner, &rgb_command("a=T,i=1", 280, 160));

        assert!(
            text.is_empty(),
            "the escape must not leak into the grid stream"
        );
        assert_eq!(
            commands.len(),
            1,
            "a large single-escape transmit must survive scanning"
        );
        assert_eq!(
            graphics.apply(commands[0].clone(), context()).advance,
            Some((8, 28))
        );
    }

    #[test]
    fn a_truncated_raw_payload_is_reported_rather_than_drawn() {
        let mut graphics = TerminalGraphics::default();
        let mut scanner = GraphicsScanner::default();
        // Claims 30x40 RGB but sends one pixel.
        let payload = BASE64.encode([1u8, 2, 3]);
        let (_, commands) = scan_all(
            &mut scanner,
            format!("\x1b_Ga=T,f=24,s=30,v=40,t=d;{payload}\x1b\\").as_bytes(),
        );

        let outcome = graphics.apply(commands[0].clone(), context());
        let response = String::from_utf8(outcome.response.expect("a refusal is reported")).unwrap();
        assert!(response.contains("EINVAL"), "unexpected report: {response}");
        assert!(graphics.visible(0, 0, 24, false).is_empty());
    }
}
