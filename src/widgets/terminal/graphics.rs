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
//! - Transmit (`a=t`), transmit-and-display (`a=T`), display a stored image (`a=p`), delete
//!   (`a=d`), and query (`a=q`).
//! - Source cropping (`x`/`y`/`w`/`h`), explicit cell sizing (`c`/`r`), z-index (`z`), suppressed
//!   cursor movement (`C=1`), image numbers (`I=`), and response quieting (`q=`).
//!
//! ## Not implemented
//!
//! - Transmission through a file or shared memory (`t=f`, `t=t`, `t=s`). A multiplexer client can
//!   be on a different machine from the program that wrote the file, so the path is meaningless
//!   often enough that answering `ENOTSUPP` is more honest than reading it sometimes.
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

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use image::DynamicImage;

use super::screen::TerminalCellSize;

/// Largest single `APC` sequence buffered before it is abandoned.
///
/// The protocol caps one escape's payload at 4096 base64 bytes, so this only trips on a child
/// that is not speaking the protocol at all.
const MAX_APC_BYTES: usize = 64 * 1024;

/// Largest payload accumulated across `m=1` chunks, before decoding.
const MAX_TRANSMIT_BYTES: usize = 32 * 1024 * 1024;

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

// ─── Public types ────────────────────────────────────────────────────────────

/// Decoded pixels the child transmitted.
///
/// Opaque on purpose: the pixels live behind this crate's `image` dependency, and pinning that
/// crate's types into the public API would make each of its releases a breaking change here.
#[derive(Clone)]
pub struct TerminalImage {
    pixels: Arc<DynamicImage>,
    source_hash: u64,
}

impl TerminalImage {
    /// Width in pixels.
    pub fn width(&self) -> u32 {
        self.pixels.width()
    }

    /// Height in pixels.
    pub fn height(&self) -> u32 {
        self.pixels.height()
    }

    /// Stable hash of the transmitted payload.
    ///
    /// Two images with the same hash decoded from the same bytes, which is what lets the renderer
    /// cache one encoded protocol across frames and across panes showing the same picture.
    pub fn source_hash(&self) -> u64 {
        self.source_hash
    }

    pub(crate) fn pixels(&self) -> &Arc<DynamicImage> {
        &self.pixels
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
#[derive(Debug, Default)]
pub(super) struct GraphicsScanner {
    state: ScanState,
    /// Body of the `APC` being accumulated, without the `ESC _` introducer.
    apc: Vec<u8>,
    /// Set once the body passed [`MAX_APC_BYTES`] and is being discarded.
    overflowed: bool,
}

impl GraphicsScanner {
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
        if let Some(command) = GraphicsCommand::parse(rest) {
            out.push(GraphicsSegment::Command(Box::new(command)));
        }
    }

    /// Drop any sequence in flight, for a hard reset of the screen.
    pub(super) fn reset(&mut self) {
        self.state = ScanState::Ground;
        self.apc.clear();
        self.overflowed = false;
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

/// Where the pixels come from (`t=`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum GraphicsMedium {
    /// `t=d` - inline in the escape sequence.
    #[default]
    Direct,
    /// `t=f`, `t=t`, `t=s` - a path or shared-memory object on the sender's machine.
    OutOfBand,
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
    /// `d=` - what a delete applies to.
    delete: u8,
    /// `q=` - 1 suppresses success reports, 2 suppresses failures too.
    quiet: u32,
    /// Payload with the base64 already undone.
    payload: Vec<u8>,
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
            delete: b'a',
            quiet: 0,
            payload: Vec::new(),
        }
    }
}

impl GraphicsCommand {
    fn parse(body: &[u8]) -> Option<Self> {
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

        // A payload that does not decode makes the whole command malformed rather than empty:
        // acting on half an image would draw garbage.
        command.payload = BASE64.decode(payload).ok()?;
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
            b't' => {
                self.medium = match first {
                    b'f' | b't' | b's' => GraphicsMedium::OutOfBand,
                    _ => GraphicsMedium::Direct,
                }
            }
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
    images: HashMap<u32, StoredImage>,
    /// `I=` image numbers mapped to the ids they were transmitted under.
    numbers: HashMap<u32, u32>,
    placements: Vec<Placement>,
    pending: Option<PendingTransmit>,
    next_auto_id: u32,
    budget: usize,
    used_bytes: usize,
    clock: u64,
}

impl Default for TerminalGraphics {
    fn default() -> Self {
        Self {
            images: HashMap::new(),
            numbers: HashMap::new(),
            placements: Vec::new(),
            pending: None,
            next_auto_id: FIRST_AUTO_ID,
            budget: DEFAULT_IMAGE_BUDGET_BYTES,
            used_bytes: 0,
            clock: 0,
        }
    }
}

impl TerminalGraphics {
    /// Replace the decoded-pixel budget, evicting immediately if it shrank.
    pub(super) fn set_budget(&mut self, bytes: usize) {
        self.budget = bytes;
        self.enforce_budget();
    }

    /// Drop everything, for `RIS` or a screen reset.
    pub(super) fn reset(&mut self) {
        self.images.clear();
        self.numbers.clear();
        self.placements.clear();
        self.pending = None;
        self.used_bytes = 0;
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
        let result = match command.medium {
            GraphicsMedium::OutOfBand => Err("ENOTSUPP:file transmission"),
            GraphicsMedium::Direct => decode_payload(command, &command.payload).map(|_| ()),
        };
        GraphicsOutcome {
            response: report(command, command.id, result),
            advance: None,
        }
    }

    fn transmit(&mut self, command: GraphicsCommand, ctx: GraphicsContext) -> GraphicsOutcome {
        if command.medium == GraphicsMedium::OutOfBand {
            self.pending = None;
            return GraphicsOutcome {
                response: report(&command, command.id, Err("ENOTSUPP:file transmission")),
                advance: None,
            };
        }
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
            header: command.clone(),
            data: Vec::new(),
        });
        if pending.id == 0 {
            pending.id = self.resolve_id(pending.header.id, pending.header.number);
        }

        if pending.data.len().saturating_add(command.payload.len()) > MAX_TRANSMIT_BYTES {
            return GraphicsOutcome {
                response: report(&command, pending.id, Err("EFBIG:payload too large")),
                advance: None,
            };
        }
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
        let decoded = match decode_payload(command, &payload) {
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
            pixels: Arc::new(decoded),
            source_hash: hash_payload(command.format, &payload),
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
        let clock = self.clock;
        let (image_w, image_h) = {
            let stored = self.images.get_mut(&id)?;
            stored.used = clock;
            (stored.image.width(), stored.image.height())
        };
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
                    let ids: Vec<u32> = self.images.keys().copied().collect();
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
        self.remove_image(id);
        let clock = self.clock;
        self.images.insert(
            id,
            StoredImage {
                image,
                bytes,
                used: clock,
            },
        );
        self.used_bytes = self.used_bytes.saturating_add(bytes);
        self.enforce_budget();
    }

    fn remove_image(&mut self, id: u32) {
        if let Some(stored) = self.images.remove(&id) {
            self.used_bytes = self.used_bytes.saturating_sub(stored.bytes);
        }
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
            return self.images.contains_key(&id).then_some(id);
        }
        let mapped = *self.numbers.get(&number)?;
        self.images.contains_key(&mapped).then_some(mapped)
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
fn decode_payload(command: &GraphicsCommand, payload: &[u8]) -> Result<DynamicImage, &'static str> {
    let mut data = if command.compressed {
        decompress(payload).ok_or("EINVAL:bad zlib payload")?
    } else {
        payload.to_vec()
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

fn hash_payload(format: u32, payload: &[u8]) -> u64 {
    use std::hash::{Hash as _, Hasher as _};

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format.hash(&mut hasher);
    payload.hash(&mut hasher);
    hasher.finish()
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

    #[test]
    fn out_of_band_transmission_is_refused_in_the_protocol_s_own_terms() {
        let mut graphics = TerminalGraphics::default();
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
