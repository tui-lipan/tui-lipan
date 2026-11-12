//! [`Terminal`]: one libghostty-vt terminal and the closures it calls.

use std::ffi::c_void;
use std::ptr::{self, NonNull};

use super::callbacks::{
    self, BytesFn, ClipboardWriteFn, Handlers, NotifyFn, ProgramStatusFn, RenderHoldFn,
    SemanticPromptFn, SizeFn,
};
use super::{GhosttyError, Rgb, Style, check, no_value, sized, string_bytes, sys};
use crate::style::PointerShape;

/// A terminal mode: a mode number and whether it is ANSI or DEC private.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct Mode(sys::GhosttyMode);

impl Mode {
    pub const INSERT: Self = Self(sys::GHOSTTY_MODE_INSERT);
    pub const LINEFEED: Self = Self(sys::GHOSTTY_MODE_LINEFEED);
    pub const CURSOR_KEYS: Self = Self(sys::GHOSTTY_MODE_CURSOR_KEYS);
    pub const REVERSE_COLORS: Self = Self(sys::GHOSTTY_MODE_REVERSE_COLORS);
    pub const ORIGIN: Self = Self(sys::GHOSTTY_MODE_ORIGIN);
    pub const WRAPAROUND: Self = Self(sys::GHOSTTY_MODE_WRAPAROUND);
    pub const MOUSE_EVENT_X10: Self = Self(sys::GHOSTTY_MODE_MOUSE_EVENT_X10);
    pub const CURSOR_BLINKING: Self = Self(sys::GHOSTTY_MODE_CURSOR_BLINKING);
    pub const CURSOR_VISIBLE: Self = Self(sys::GHOSTTY_MODE_CURSOR_VISIBLE);
    pub const KEYPAD_KEYS: Self = Self(sys::GHOSTTY_MODE_KEYPAD_KEYS);
    pub const MOUSE_EVENT_NORMAL: Self = Self(sys::GHOSTTY_MODE_MOUSE_EVENT_NORMAL);
    pub const MOUSE_EVENT_BUTTON: Self = Self(sys::GHOSTTY_MODE_MOUSE_EVENT_BUTTON);
    pub const MOUSE_EVENT_ANY: Self = Self(sys::GHOSTTY_MODE_MOUSE_EVENT_ANY);
    pub const FOCUS_EVENT: Self = Self(sys::GHOSTTY_MODE_FOCUS_EVENT);
    pub const MOUSE_FORMAT_UTF8: Self = Self(sys::GHOSTTY_MODE_MOUSE_FORMAT_UTF8);
    pub const MOUSE_FORMAT_SGR: Self = Self(sys::GHOSTTY_MODE_MOUSE_FORMAT_SGR);
    pub const MOUSE_ALTERNATE_SCROLL: Self = Self(sys::GHOSTTY_MODE_MOUSE_ALTERNATE_SCROLL);
    pub const MOUSE_FORMAT_URXVT: Self = Self(sys::GHOSTTY_MODE_MOUSE_FORMAT_URXVT);
    pub const MOUSE_FORMAT_SGR_PIXELS: Self = Self(sys::GHOSTTY_MODE_MOUSE_FORMAT_SGR_PIXELS);
    pub const ALT_SCREEN_SAVE_CURSOR_CLEAR_ENTER: Self =
        Self(sys::GHOSTTY_MODE_ALT_SCREEN_SAVE_CURSOR_CLEAR_ENTER);
    pub const BRACKETED_PASTE: Self = Self(sys::GHOSTTY_MODE_BRACKETED_PASTE);
    pub const SYNCHRONIZED_OUTPUT: Self = Self(sys::GHOSTTY_MODE_SYNCHRONIZED_OUTPUT);
    pub const GRAPHEME_CLUSTER: Self = Self(sys::GHOSTTY_MODE_GRAPHEME_CLUSTER);
    pub const REPORT_COLOR_SCHEME: Self = Self(sys::GHOSTTY_MODE_REPORT_COLOR_SCHEME);
    pub const IN_BAND_SIZE_REPORTS: Self = Self(sys::GHOSTTY_MODE_IN_BAND_SIZE_REPORTS);

    /// A DEC private mode (`CSI ? n h`).
    pub const fn dec(number: u16) -> Self {
        Self(sys::ghostty_mode_new(number, false))
    }

    /// An ANSI mode (`CSI n h`).
    pub const fn ansi(number: u16) -> Self {
        Self(sys::ghostty_mode_new(number, true))
    }
}

/// Which screen is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Screen {
    Primary,
    Alternate,
}

/// A grid position in one of the four coordinate systems.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Point {
    /// Relative to the top of the active area, where the cursor moves.
    Active { x: u16, y: u32 },
    /// Relative to the top of the visible viewport.
    Viewport { x: u16, y: u32 },
    /// Relative to the oldest scrollback row, through the last written row.
    Screen { x: u16, y: u32 },
    /// Relative to the oldest scrollback row, scrollback only.
    History { x: u16, y: u32 },
}

impl Point {
    fn tag(self) -> sys::GhosttyPointTag {
        match self {
            Self::Active { .. } => sys::GHOSTTY_POINT_TAG_ACTIVE,
            Self::Viewport { .. } => sys::GHOSTTY_POINT_TAG_VIEWPORT,
            Self::Screen { .. } => sys::GHOSTTY_POINT_TAG_SCREEN,
            Self::History { .. } => sys::GHOSTTY_POINT_TAG_HISTORY,
        }
    }

    fn coordinate(self) -> (u16, u32) {
        match self {
            Self::Active { x, y }
            | Self::Viewport { x, y }
            | Self::Screen { x, y }
            | Self::History { x, y } => (x, y),
        }
    }

    fn to_raw(self) -> sys::GhosttyPoint {
        let (x, y) = self.coordinate();
        // SAFETY: the value union is plain data; zero it so the padding arm is initialized.
        let mut value: sys::GhosttyPointValue = unsafe { std::mem::zeroed() };
        value.coordinate = sys::GhosttyPointCoordinate { x, y };
        sys::GhosttyPoint {
            tag: self.tag(),
            value,
        }
    }

    fn with_coordinate(self, c: sys::GhosttyPointCoordinate) -> Self {
        let (x, y) = (c.x, c.y);
        match self {
            Self::Active { .. } => Self::Active { x, y },
            Self::Viewport { .. } => Self::Viewport { x, y },
            Self::Screen { .. } => Self::Screen { x, y },
            Self::History { .. } => Self::History { x, y },
        }
    }
}

/// How to move the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollViewport {
    /// The oldest scrollback row.
    Top,
    /// The active area; the viewport follows new output again.
    Bottom,
    /// Rows to move, negative for up.
    Delta(isize),
    /// The row (in [`Scrollbar::offset`] space) to show at the top.
    Row(usize),
    /// Shell prompts to move, negative for back. Needs OSC 133 markers.
    DeltaPrompt(isize),
}

/// The scrollable area and where the viewport sits in it, in rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Scrollbar {
    pub total: u64,
    pub offset: u64,
    pub len: u64,
}

/// Scrollback limits. `None` is unlimited. When both are set, the first one reached prunes.
///
/// Both are enforced in whole pages (about 400 KB of grid each), so the terminal keeps somewhat
/// more than either limit asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbackLimits {
    pub max_lines: Option<usize>,
    pub max_bytes: Option<usize>,
}

impl Default for ScrollbackLimits {
    /// 10,000 lines and no byte limit. libghostty-vt's own default is a 10,000-*byte* limit,
    /// one page, which [`Terminal::new`] always replaces.
    fn default() -> Self {
        Self {
            max_lines: Some(10_000),
            max_bytes: None,
        }
    }
}

/// How much compression work [`Terminal::compress`] does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompressionMode {
    /// One bounded step, for an idle callback.
    Incremental,
    /// A synchronous pass over all eligible pages; can stall on large scrollback.
    Full,
}

/// What [`Terminal::compress`] says about further work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CompressionResult {
    /// This platform cannot reclaim compressed memory.
    Unsupported,
    /// Call again while the terminal stays idle.
    Pending,
    /// Nothing to do until [`Terminal::compression_activity`] changes.
    Complete,
}

/// Memory held by a terminal's two screens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct MemoryUsage {
    pub compression_supported: bool,
    pub primary_pages: u64,
    pub primary_resident_bytes: u64,
    pub primary_virtual_bytes: u64,
    pub primary_compressed_pages: u64,
    pub primary_compressed_bytes: u64,
    pub primary_image_bytes: u64,
    pub alternate_pages: u64,
    pub alternate_resident_bytes: u64,
    pub alternate_virtual_bytes: u64,
    pub alternate_compressed_pages: u64,
    pub alternate_compressed_bytes: u64,
    pub alternate_image_bytes: u64,
}

impl MemoryUsage {
    /// Physical bytes held by both screens, the figure to budget against.
    pub fn resident_bytes(&self) -> u64 {
        self.primary_resident_bytes + self.alternate_resident_bytes
    }
}

/// A libghostty-vt terminal: parser, screens, scrollback and modes.
///
/// Not `Send`: the library is single-threaded, and the registered closures need not be `Send`.
pub(crate) struct Terminal {
    raw: NonNull<sys::GhosttyTerminalImpl>,
    /// Owned; boxed so the library's userdata pointer stays put. Freed after the terminal.
    handlers: NonNull<Handlers>,
}

impl Drop for Terminal {
    fn drop(&mut self) {
        // SAFETY: `raw` is a live terminal owned by self; after this nothing calls back into
        // `handlers`, which was allocated by `Box::new` in `new`.
        unsafe {
            sys::ghostty_terminal_free(self.raw.as_ptr());
            drop(Box::from_raw(self.handlers.as_ptr()));
        }
    }
}

impl std::fmt::Debug for Terminal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Terminal")
            .field("cols", &self.cols())
            .field("rows", &self.rows())
            .finish_non_exhaustive()
    }
}

impl Terminal {
    /// Creates a `cols` × `rows` terminal with [`ScrollbackLimits::default`].
    pub fn new(cols: u16, rows: u16) -> Result<Self, GhosttyError> {
        let mut raw: sys::GhosttyTerminal = ptr::null_mut();
        // SAFETY: NULL selects the default allocator; `raw` receives the handle.
        check(unsafe { sys::ghostty_terminal_new(ptr::null(), &raw mut raw, cols, rows) })?;
        let raw = NonNull::new(raw).ok_or(GhosttyError::OutOfMemory)?;
        let handlers = NonNull::from(Box::leak(Box::<Handlers>::default()));
        let mut terminal = Self { raw, handlers };
        // SAFETY: USERDATA takes the pointer itself. Handlers outlive the terminal (Drop).
        check(unsafe {
            sys::ghostty_terminal_set(
                terminal.raw(),
                sys::GHOSTTY_TERMINAL_OPT_USERDATA,
                handlers.as_ptr().cast_const().cast(),
            )
        })?;
        terminal.set_scrollback_limits(ScrollbackLimits::default())?;
        Ok(terminal)
    }

    fn raw(&self) -> sys::GhosttyTerminal {
        self.raw.as_ptr()
    }

    fn handlers(&self) -> &Handlers {
        // SAFETY: owned by self and only ever shared.
        unsafe { self.handlers.as_ref() }
    }

    /// Runs a library call that may invoke callbacks, then resumes any panic they raised.
    fn with_callbacks<R>(&mut self, f: impl FnOnce(sys::GhosttyTerminal) -> R) -> R {
        let result = f(self.raw());
        self.handlers().resume_panic();
        result
    }

    /// Feeds program output through the parser. Callbacks run before this returns.
    pub fn write(&mut self, bytes: &[u8]) {
        if bytes.is_empty() {
            return;
        }
        self.with_callbacks(|raw| {
            // SAFETY: `bytes` is readable for its length. `&mut self` keeps any callback from
            // reaching this terminal, so the write is not reentered.
            unsafe { sys::ghostty_terminal_vt_write(raw, bytes.as_ptr(), bytes.len()) }
        });
    }

    /// Resizes the grid, reflowing the primary screen, and records the cell size in pixels
    /// for image protocols and size reports. Ends any synchronized-output hold.
    pub fn resize(
        &mut self,
        cols: u16,
        rows: u16,
        cell_width_px: u32,
        cell_height_px: u32,
    ) -> Result<(), GhosttyError> {
        let result = self.with_callbacks(|raw| {
            // SAFETY: live terminal; may call render_hold and write_pty.
            unsafe { sys::ghostty_terminal_resize(raw, cols, rows, cell_width_px, cell_height_px) }
        });
        check(result)
    }

    /// Full reset (RIS): modes, screens, scrollback. Keeps the size and the options set here.
    pub fn reset(&mut self) {
        // SAFETY: live terminal; may call render_hold.
        self.with_callbacks(|raw| unsafe { sys::ghostty_terminal_reset(raw) });
    }

    // ---- options ---------------------------------------------------------------------------

    /// Sets an option whose value is passed by pointer.
    ///
    /// # Safety
    ///
    /// `value` must have the input type the library documents for `option`.
    unsafe fn set_value<T>(
        &mut self,
        option: sys::GhosttyTerminalOption,
        value: Option<&T>,
    ) -> Result<(), GhosttyError> {
        let ptr = value.map_or(ptr::null(), |v| ptr::from_ref(v).cast::<c_void>());
        // SAFETY: guaranteed by the caller; the library copies the value.
        check(unsafe { sys::ghostty_terminal_set(self.raw(), option, ptr) })
    }

    /// Sets both scrollback limits. Lowering one prunes eligible history immediately.
    pub fn set_scrollback_limits(&mut self, limits: ScrollbackLimits) -> Result<(), GhosttyError> {
        let lines = limits.max_lines.unwrap_or(usize::MAX);
        let bytes = limits.max_bytes.unwrap_or(usize::MAX);
        // SAFETY: both options take `size_t*`; SIZE_MAX means unlimited.
        unsafe {
            self.set_value(sys::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_BYTES, Some(&bytes))?;
            self.set_value(sys::GHOSTTY_TERMINAL_OPT_SCROLLBACK_MAX_LINES, Some(&lines))
        }
    }

    /// The configured limits, read back from the library.
    pub fn scrollback_limits(&self) -> ScrollbackLimits {
        // SAFETY: both keys write `size_t`; NO_VALUE means unlimited.
        unsafe {
            ScrollbackLimits {
                max_lines: self
                    .get_optional::<usize>(sys::GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_LINES),
                max_bytes: self
                    .get_optional::<usize>(sys::GHOSTTY_TERMINAL_DATA_SCROLLBACK_MAX_BYTES),
            }
        }
    }

    /// Sets the current value and the value restored by RIS or an API reset.
    pub fn set_default_mode(&mut self, mode: Mode, value: bool) -> Result<(), GhosttyError> {
        let config = sys::GhosttyTerminalModeConfig {
            mode: mode.0,
            value,
        };
        // SAFETY: OPT_MODE_DEFAULT takes GhosttyTerminalModeConfig*.
        unsafe { self.set_value(sys::GHOSTTY_TERMINAL_OPT_MODE_DEFAULT, Some(&config)) }
    }

    /// Sets the current value of a mode, without changing what a reset restores.
    pub fn set_mode(&mut self, mode: Mode, value: bool) -> Result<(), GhosttyError> {
        let config = sys::GhosttyTerminalModeConfig {
            mode: mode.0,
            value,
        };
        // SAFETY: OPT_MODE takes GhosttyTerminalModeConfig*.
        unsafe { self.set_value(sys::GHOSTTY_TERMINAL_OPT_MODE, Some(&config)) }
    }

    /// Sets (or with `None`, clears) the default foreground, background and cursor colors.
    pub fn set_default_colors(
        &mut self,
        foreground: Option<Rgb>,
        background: Option<Rgb>,
        cursor: Option<Rgb>,
    ) -> Result<(), GhosttyError> {
        let [fg, bg, cur] =
            [foreground, background, cursor].map(|c| c.map(sys::GhosttyColorRgb::from));
        // SAFETY: the color options take GhosttyColorRgb*; NULL clears.
        unsafe {
            self.set_value(sys::GHOSTTY_TERMINAL_OPT_COLOR_FOREGROUND, fg.as_ref())?;
            self.set_value(sys::GHOSTTY_TERMINAL_OPT_COLOR_BACKGROUND, bg.as_ref())?;
            self.set_value(sys::GHOSTTY_TERMINAL_OPT_COLOR_CURSOR, cur.as_ref())
        }
    }

    /// Sets the default 256-color palette, or with `None` restores the built-in one.
    pub fn set_default_palette(
        &mut self,
        palette: Option<&[Rgb; 256]>,
    ) -> Result<(), GhosttyError> {
        let raw = palette.map(|p| p.map(sys::GhosttyColorRgb::from));
        // SAFETY: OPT_COLOR_PALETTE takes GhosttyColorRgb[256]*; NULL restores the default.
        unsafe { self.set_value(sys::GHOSTTY_TERMINAL_OPT_COLOR_PALETTE, raw.as_ref()) }
    }

    /// Whether the program may switch to the alternate screen etc. by resize pulling rows back
    /// from scrollback. Turn off when the PTY keeps its own scrollback-less buffer (ConPTY).
    pub fn set_resize_pull_scrollback(&mut self, pull: bool) -> Result<(), GhosttyError> {
        // SAFETY: takes bool*.
        unsafe {
            self.set_value(
                sys::GHOSTTY_TERMINAL_OPT_RESIZE_PULL_SCROLLBACK,
                Some(&pull),
            )
        }
    }

    /// The terminfo name reported for an XTGETTCAP `TN` query.
    pub fn set_terminfo_name(&mut self, name: &str) -> Result<(), GhosttyError> {
        let value = sys::GhosttyString {
            ptr: name.as_ptr(),
            len: name.len(),
        };
        // SAFETY: takes GhosttyString*; the string is copied.
        unsafe { self.set_value(sys::GHOSTTY_TERMINAL_OPT_TERMINFO_NAME, Some(&value)) }
    }

    /// The reply to XTVERSION (`CSI > q`), or `None` for the library's default.
    pub fn set_xtversion(&mut self, reply: Option<&str>) -> Result<(), GhosttyError> {
        let callback: sys::GhosttyTerminalXtversionFn =
            reply.map(|_| callbacks::xtversion as unsafe extern "C" fn(_, _) -> _);
        *self.handlers().xtversion.borrow_mut() = reply.map(|r| r.as_bytes().into());
        self.set_callback(sys::GHOSTTY_TERMINAL_OPT_XTVERSION, callback)
    }

    // ---- callbacks -------------------------------------------------------------------------

    fn set_callback<F: Copy>(
        &mut self,
        option: sys::GhosttyTerminalOption,
        callback: Option<F>,
    ) -> Result<(), GhosttyError> {
        const {
            assert!(std::mem::size_of::<F>() == std::mem::size_of::<*const c_void>());
        }
        let ptr = match callback {
            // SAFETY: F is an `unsafe extern "C" fn` pointer type (size checked above), which
            // the library takes as the pointer itself.
            Some(f) => unsafe { std::mem::transmute_copy::<F, *const c_void>(&f) },
            None => ptr::null(),
        };
        // SAFETY: callback options take the function pointer itself, NULL to unset.
        check(unsafe { sys::ghostty_terminal_set(self.raw(), option, ptr) })
    }

    /// Replies the terminal sends to the program: query answers, mode reports, clipboard and
    /// OSC 7501 support replies. Write them to the PTY.
    pub fn on_write_pty(&mut self, f: Option<BytesFn>) -> Result<(), GhosttyError> {
        let set = f.is_some();
        *self.handlers().write_pty.borrow_mut() = f;
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_WRITE_PTY,
            set.then_some(callbacks::write_pty as unsafe extern "C" fn(_, _, _, _)),
        )
    }

    /// BEL.
    pub fn on_bell(&mut self, f: Option<NotifyFn>) -> Result<(), GhosttyError> {
        let set = f.is_some();
        *self.handlers().bell.borrow_mut() = f;
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_BELL,
            set.then_some(callbacks::bell as unsafe extern "C" fn(_, _)),
        )
    }

    /// The title changed (OSC 0/2). Receives the new title's bytes.
    pub fn on_title_changed(&mut self, f: Option<BytesFn>) -> Result<(), GhosttyError> {
        let set = f.is_some();
        *self.handlers().title_changed.borrow_mut() = f;
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_TITLE_CHANGED,
            set.then_some(callbacks::title_changed as unsafe extern "C" fn(_, _)),
        )
    }

    /// The working directory changed (OSC 7, OSC 9, OSC 1337 CurrentDir). Receives the raw
    /// value, a `file://` URI for OSC 7; empty when the shell clears it.
    pub fn on_pwd_changed(&mut self, f: Option<BytesFn>) -> Result<(), GhosttyError> {
        let set = f.is_some();
        *self.handlers().pwd_changed.borrow_mut() = f;
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_PWD_CHANGED,
            set.then_some(callbacks::pwd_changed as unsafe extern "C" fn(_, _)),
        )
    }

    /// A program asks to write the clipboard. Unset, writes are ignored.
    pub fn on_clipboard_write(&mut self, f: Option<ClipboardWriteFn>) -> Result<(), GhosttyError> {
        let set = f.is_some();
        *self.handlers().clipboard_write.borrow_mut() = f;
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_CLIPBOARD_WRITE,
            set.then_some(callbacks::clipboard_write as unsafe extern "C" fn(_, _, _)),
        )
    }

    /// Shell integration steps (OSC 133).
    pub fn on_semantic_prompt(&mut self, f: Option<SemanticPromptFn>) -> Result<(), GhosttyError> {
        let set = f.is_some();
        *self.handlers().semantic_prompt.borrow_mut() = f;
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_SEMANTIC_PROMPT,
            set.then_some(callbacks::semantic_prompt as unsafe extern "C" fn(_, _, _)),
        )
    }

    /// Program status reports (OSC 7501). While set, `OSC 7501 ; ?` is answered through
    /// [`on_write_pty`](Self::on_write_pty). The library keeps no records; the caller does.
    pub fn on_program_status(&mut self, f: Option<ProgramStatusFn>) -> Result<(), GhosttyError> {
        let set = f.is_some();
        *self.handlers().program_status.borrow_mut() = f;
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_PROGRAM_STATUS,
            set.then_some(callbacks::program_status as unsafe extern "C" fn(_, _, _)),
        )
    }

    /// Synchronized output (mode 2026) began (`true`) or ended (`false`).
    pub fn on_render_hold(&mut self, f: Option<RenderHoldFn>) -> Result<(), GhosttyError> {
        let set = f.is_some();
        *self.handlers().render_hold.borrow_mut() = f;
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_RENDER_HOLD,
            set.then_some(callbacks::render_hold as unsafe extern "C" fn(_, _, _)),
        )
    }

    /// The program performed a full reset (RIS).
    pub fn on_reset(&mut self, f: Option<NotifyFn>) -> Result<(), GhosttyError> {
        let set = f.is_some();
        *self.handlers().reset.borrow_mut() = f;
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_RESET,
            set.then_some(callbacks::reset as unsafe extern "C" fn(_, _)),
        )
    }

    /// Geometry for XTWINOPS size queries and mode 2048 reports; `None` suppresses the reply.
    pub fn on_size(&mut self, f: Option<SizeFn>) -> Result<(), GhosttyError> {
        let set = f.is_some();
        *self.handlers().size.borrow_mut() = f;
        self.set_callback(
            sys::GHOSTTY_TERMINAL_OPT_SIZE,
            set.then_some(callbacks::size as unsafe extern "C" fn(_, _, _) -> _),
        )
    }

    // ---- state -----------------------------------------------------------------------------

    /// Reads a datum.
    ///
    /// # Safety
    ///
    /// `T` must be the output type the library documents for `key`, valid when zeroed.
    unsafe fn get<T>(&self, key: sys::GhosttyTerminalData) -> Result<T, GhosttyError> {
        // SAFETY: zero is valid for T (caller contract).
        let mut out: T = unsafe { std::mem::zeroed() };
        // SAFETY: `out` has the documented type for `key`.
        check(unsafe { sys::ghostty_terminal_get(self.raw(), key, (&raw mut out).cast()) })?;
        Ok(out)
    }

    /// Reads a datum that may have no value. Errors read as no value.
    ///
    /// # Safety
    ///
    /// As [`get`](Self::get).
    unsafe fn get_optional<T>(&self, key: sys::GhosttyTerminalData) -> Option<T> {
        // SAFETY: zero is valid for T (caller contract).
        let mut out: T = unsafe { std::mem::zeroed() };
        // SAFETY: `out` has the documented type for `key`.
        let result = unsafe { sys::ghostty_terminal_get(self.raw(), key, (&raw mut out).cast()) };
        no_value(result, out).ok().flatten()
    }

    /// Reads a datum every terminal has. Failure would mean a stale header/library pairing.
    ///
    /// # Safety
    ///
    /// As [`get`](Self::get).
    unsafe fn get_or_default<T: Default>(&self, key: sys::GhosttyTerminalData) -> T {
        // SAFETY: forwarded.
        unsafe { self.get(key) }.unwrap_or_default()
    }

    pub fn cols(&self) -> u16 {
        // SAFETY: uint16_t.
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_COLS) }
    }

    pub fn rows(&self) -> u16 {
        // SAFETY: uint16_t.
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_ROWS) }
    }

    /// Cursor column and row within the active area.
    pub fn cursor_position(&self) -> (u16, u16) {
        // SAFETY: both uint16_t.
        unsafe {
            (
                self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_CURSOR_X),
                self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_CURSOR_Y),
            )
        }
    }

    /// Whether the next printed character soft-wraps first.
    pub fn cursor_pending_wrap(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_CURSOR_PENDING_WRAP) }
    }

    /// DECTCEM (mode 25).
    pub fn cursor_visible(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_CURSOR_VISIBLE) }
    }

    /// The SGR style new text is printed with.
    pub fn cursor_style(&self) -> Style {
        // SAFETY: GhosttyStyle, plain data.
        let raw: sys::GhosttyStyle =
            unsafe { self.get_or_default_zeroed(sys::GHOSTTY_TERMINAL_DATA_CURSOR_SGR_STYLE) };
        Style::from_raw(&raw)
    }

    /// # Safety
    ///
    /// As [`get`](Self::get), for types without `Default`.
    unsafe fn get_or_default_zeroed<T>(&self, key: sys::GhosttyTerminalData) -> T {
        // SAFETY: forwarded; zero is valid for T.
        unsafe { self.get(key).unwrap_or_else(|_| std::mem::zeroed()) }
    }

    pub fn active_screen(&self) -> Screen {
        // SAFETY: GhosttyTerminalScreen (int).
        let screen: sys::GhosttyTerminalScreen =
            unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_ACTIVE_SCREEN) };
        if screen == sys::GHOSTTY_TERMINAL_SCREEN_ALTERNATE {
            Screen::Alternate
        } else {
            Screen::Primary
        }
    }

    /// The current value of a mode, `None` for a mode the library does not know.
    pub fn mode(&self, mode: Mode) -> Option<bool> {
        let mut config = sys::GhosttyTerminalModeConfig {
            mode: mode.0,
            value: false,
        };
        // SAFETY: DATA_MODE reads and writes GhosttyTerminalModeConfig.
        let result = unsafe {
            sys::ghostty_terminal_get(
                self.raw(),
                sys::GHOSTTY_TERMINAL_DATA_MODE,
                (&raw mut config).cast(),
            )
        };
        check(result).ok().map(|()| config.value)
    }

    /// The Kitty keyboard protocol flags in effect.
    pub fn kitty_keyboard_flags(&self) -> u8 {
        // SAFETY: GhosttyKittyKeyFlags (uint8_t).
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_KITTY_KEYBOARD_FLAGS) }
    }

    /// Whether any mouse tracking mode (X10, normal, button, any) is on.
    pub fn mouse_tracking(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_MOUSE_TRACKING) }
    }

    /// The pointer shape the program asked for with OSC 22.
    pub fn pointer_shape(&self) -> PointerShape {
        // SAFETY: GhosttyMouseShape (int).
        let shape: sys::GhosttyMouseShape =
            unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_MOUSE_SHAPE) };
        pointer_shape(shape)
    }

    /// The title set by OSC 0/2, empty if none. Borrowed until the next mutation.
    pub fn title(&self) -> &[u8] {
        self.string(sys::GHOSTTY_TERMINAL_DATA_TITLE)
    }

    /// The working directory set by OSC 7 and friends, raw. Borrowed until the next mutation.
    pub fn pwd(&self) -> &[u8] {
        self.string(sys::GHOSTTY_TERMINAL_DATA_PWD)
    }

    fn string(&self, key: sys::GhosttyTerminalData) -> &[u8] {
        // SAFETY: GhosttyString; zeroed is an empty string.
        let s: sys::GhosttyString = unsafe { self.get_or_default_zeroed(key) };
        // SAFETY: valid until the next mutating call, which needs `&mut self`.
        unsafe { string_bytes(s) }
    }

    /// The viewport's place in the scrollable area. Cheap; poll per frame.
    pub fn scrollbar(&self) -> Scrollbar {
        // SAFETY: GhosttyTerminalScrollbar, plain data.
        let raw: sys::GhosttyTerminalScrollbar =
            unsafe { self.get_or_default_zeroed(sys::GHOSTTY_TERMINAL_DATA_SCROLLBAR) };
        Scrollbar {
            total: raw.total,
            offset: raw.offset,
            len: raw.len,
        }
    }

    /// Whether the viewport follows the active area (not scrolled into history).
    pub fn viewport_active(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_VIEWPORT_ACTIVE) }
    }

    /// Rows in the active screen including scrollback.
    pub fn total_rows(&self) -> usize {
        // SAFETY: size_t.
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_TOTAL_ROWS) }
    }

    /// Rows of scrollback in the active screen.
    pub fn scrollback_rows(&self) -> usize {
        // SAFETY: size_t.
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_SCROLLBACK_ROWS) }
    }

    /// The cursor color set by OSC 12, if any.
    pub fn cursor_color(&self) -> Option<Rgb> {
        // SAFETY: GhosttyColorRgb, plain data; NO_VALUE when unset.
        unsafe {
            self.get_optional::<sys::GhosttyColorRgb>(sys::GHOSTTY_TERMINAL_DATA_COLOR_CURSOR)
        }
        .map(Rgb::from)
    }

    /// Whether the cursor is in a shell prompt or its input (OSC 133).
    pub fn cursor_at_prompt(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_CURSOR_AT_PROMPT) }
    }

    /// Whether the parser is between sequences, where injected bytes cannot split one.
    pub fn at_ground(&self) -> bool {
        // SAFETY: bool.
        unsafe { self.get_or_default(sys::GHOSTTY_TERMINAL_DATA_VT_GROUND) }
    }

    /// Memory held by both screens. Visits every page; don't call per write.
    pub fn memory_usage(&self) -> MemoryUsage {
        // SAFETY: a sized struct, plain data.
        let mut raw: sys::GhosttyTerminalMemoryUsage = unsafe { sized() };
        // SAFETY: DATA_MEMORY_USAGE writes the sized struct whose size we set.
        let result = unsafe {
            sys::ghostty_terminal_get(
                self.raw(),
                sys::GHOSTTY_TERMINAL_DATA_MEMORY_USAGE,
                (&raw mut raw).cast(),
            )
        };
        if check(result).is_err() {
            return MemoryUsage::default();
        }
        MemoryUsage {
            compression_supported: raw.compression_supported,
            primary_pages: raw.primary_pages,
            primary_resident_bytes: raw.primary_resident_bytes,
            primary_virtual_bytes: raw.primary_virtual_bytes,
            primary_compressed_pages: raw.primary_compressed_pages,
            primary_compressed_bytes: raw.primary_compressed_bytes,
            primary_image_bytes: raw.primary_image_bytes,
            alternate_pages: raw.alternate_pages,
            alternate_resident_bytes: raw.alternate_resident_bytes,
            alternate_virtual_bytes: raw.alternate_virtual_bytes,
            alternate_compressed_pages: raw.alternate_compressed_pages,
            alternate_compressed_bytes: raw.alternate_compressed_bytes,
            alternate_image_bytes: raw.alternate_image_bytes,
        }
    }

    // ---- scrolling and compression ---------------------------------------------------------

    pub fn scroll_viewport(&mut self, scroll: ScrollViewport) -> Result<(), GhosttyError> {
        // SAFETY: the value union is plain data; zero it so unused bytes are initialized.
        let mut value: sys::GhosttyTerminalScrollViewportValue = unsafe { std::mem::zeroed() };
        let tag = match scroll {
            ScrollViewport::Top => sys::GHOSTTY_TERMINAL_SCROLL_VIEWPORT_TOP,
            ScrollViewport::Bottom => sys::GHOSTTY_TERMINAL_SCROLL_VIEWPORT_BOTTOM,
            ScrollViewport::Delta(delta) => {
                value.delta = delta;
                sys::GHOSTTY_TERMINAL_SCROLL_VIEWPORT_DELTA
            }
            ScrollViewport::Row(row) => {
                value.row = row;
                sys::GHOSTTY_TERMINAL_SCROLL_VIEWPORT_ROW
            }
            ScrollViewport::DeltaPrompt(delta) => {
                value.delta_prompt = delta;
                sys::GHOSTTY_TERMINAL_SCROLL_VIEWPORT_DELTA_PROMPT
            }
        };
        let behavior = sys::GhosttyTerminalScrollViewport { tag, value };
        // SAFETY: live terminal, valid tagged union.
        check(unsafe { sys::ghostty_terminal_scroll_viewport(self.raw(), behavior) })
    }

    /// An opaque token that changes whenever compression should wait for a new idle period.
    pub fn compression_activity(&self) -> u64 {
        let mut token = 0u64;
        // SAFETY: writes a uint64_t.
        let result =
            unsafe { sys::ghostty_terminal_compression_activity(self.raw(), &raw mut token) };
        if check(result).is_err() { 0 } else { token }
    }

    /// Compresses eligible scrollback pages. Logical contents never change.
    pub fn compress(&mut self, mode: CompressionMode) -> Result<CompressionResult, GhosttyError> {
        let mode = match mode {
            CompressionMode::Incremental => sys::GHOSTTY_TERMINAL_COMPRESSION_MODE_INCREMENTAL,
            CompressionMode::Full => sys::GHOSTTY_TERMINAL_COMPRESSION_MODE_FULL,
        };
        let mut out: sys::GhosttyTerminalCompressionResult = 0;
        // SAFETY: live terminal; `out` receives the result.
        check(unsafe { sys::ghostty_terminal_compress(self.raw(), mode, &raw mut out) })?;
        Ok(match out {
            sys::GHOSTTY_TERMINAL_COMPRESSION_RESULT_PENDING => CompressionResult::Pending,
            sys::GHOSTTY_TERMINAL_COMPRESSION_RESULT_COMPLETE => CompressionResult::Complete,
            _ => CompressionResult::Unsupported,
        })
    }

    // ---- grid access -----------------------------------------------------------------------

    /// Resolves a point to an untracked reference, read immediately by `f`. Not for rendering
    /// whole frames; use [`RenderState`](super::RenderState).
    fn with_grid_ref<R>(
        &self,
        point: Point,
        f: impl FnOnce(&sys::GhosttyGridRef) -> R,
    ) -> Option<R> {
        // SAFETY: GhosttyGridRef is plain data; zeroed refers to nothing.
        let mut grid_ref: sys::GhosttyGridRef = unsafe { std::mem::zeroed() };
        // SAFETY: live terminal; out pointer valid.
        let result = unsafe {
            sys::ghostty_terminal_grid_ref(self.raw(), point.to_raw(), &raw mut grid_ref)
        };
        check(result).ok()?;
        // The reference is valid until the terminal mutates, which `&self` rules out.
        Some(f(&grid_ref))
    }

    /// The text of the cell at `point` (its full grapheme cluster), `None` if out of range.
    pub fn cell_text(&self, point: Point) -> Option<String> {
        self.with_grid_ref(point, grid_ref_text).flatten()
    }

    /// The style of the cell at `point`.
    pub fn cell_style(&self, point: Point) -> Option<Style> {
        self.with_grid_ref(point, |r| {
            // SAFETY: GhosttyStyle is plain data.
            let mut style: sys::GhosttyStyle = unsafe { std::mem::zeroed() };
            // SAFETY: fresh, valid reference.
            check(unsafe { sys::ghostty_grid_ref_style(r, &raw mut style) }).ok()?;
            Some(Style::from_raw(&style))
        })
        .flatten()
    }

    /// Writes the OSC 8 URI of the cell at `point` into `out`, reusing its allocation; false when
    /// it has none.
    pub fn hyperlink_uri_into(&self, point: Point, out: &mut Vec<u8>) -> bool {
        self.with_grid_ref(point, |r| hyperlink_into(r, out))
            .unwrap_or_else(|| {
                out.clear();
                false
            })
    }

    /// The OSC 8 hyperlink URI of the cell at `point`, if it has one.
    pub fn hyperlink_uri(&self, point: Point) -> Option<Vec<u8>> {
        self.with_grid_ref(point, |r| {
            let mut len = 0usize;
            // SAFETY: size query with a NULL buffer.
            let result =
                unsafe { sys::ghostty_grid_ref_hyperlink_uri(r, ptr::null_mut(), 0, &raw mut len) };
            if result == sys::GHOSTTY_SUCCESS || len == 0 {
                return None;
            }
            let mut uri = vec![0u8; len];
            // SAFETY: `uri` has `len` writable bytes; the reference is still fresh.
            let result = unsafe {
                sys::ghostty_grid_ref_hyperlink_uri(r, uri.as_mut_ptr(), uri.len(), &raw mut len)
            };
            check(result).ok()?;
            uri.truncate(len);
            Some(uri)
        })
        .flatten()
    }

    /// Whether the cell at `point` is narrow, wide, or the spacer half of a wide character.
    pub fn cell_wide(&self, point: Point) -> Option<super::CellWide> {
        self.with_grid_ref(point, |r| {
            let mut cell: sys::GhosttyCell = 0;
            // SAFETY: fresh reference.
            check(unsafe { sys::ghostty_grid_ref_cell(r, &raw mut cell) }).ok()?;
            let mut wide: sys::GhosttyCellWide = 0;
            // SAFETY: CELL_DATA_WIDE writes a GhosttyCellWide.
            let result = unsafe {
                sys::ghostty_cell_get(cell, sys::GHOSTTY_CELL_DATA_WIDE, (&raw mut wide).cast())
            };
            check(result).ok()?;
            Some(match wide {
                sys::GHOSTTY_CELL_WIDE_WIDE => super::CellWide::Wide,
                sys::GHOSTTY_CELL_WIDE_SPACER_TAIL => super::CellWide::SpacerTail,
                sys::GHOSTTY_CELL_WIDE_SPACER_HEAD => super::CellWide::SpacerHead,
                _ => super::CellWide::Narrow,
            })
        })
        .flatten()
    }

    /// Whether the row at `point` soft-wraps into the next one.
    pub fn row_wraps(&self, point: Point) -> Option<bool> {
        self.with_grid_ref(point, |r| {
            let mut row: sys::GhosttyRow = 0;
            // SAFETY: fresh reference.
            check(unsafe { sys::ghostty_grid_ref_row(r, &raw mut row) }).ok()?;
            let mut wrap = false;
            // SAFETY: ROW_DATA_WRAP writes a bool.
            let result = unsafe {
                sys::ghostty_row_get(row, sys::GHOSTTY_ROW_DATA_WRAP, (&raw mut wrap).cast())
            };
            check(result).ok().map(|()| wrap)
        })
        .flatten()
    }

    /// Reads the row at `point` (its x is ignored) cell by cell, for rows a render state does not
    /// hold, such as history. Resolves the row once; `None` when out of range.
    pub fn grid_row(&self, point: Point) -> Option<GridRow<'_>> {
        let point = match point {
            Point::Active { y, .. } => Point::Active { x: 0, y },
            Point::Viewport { y, .. } => Point::Viewport { x: 0, y },
            Point::Screen { y, .. } => Point::Screen { x: 0, y },
            Point::History { y, .. } => Point::History { x: 0, y },
        };
        let base = self.with_grid_ref(point, |r| *r)?;
        Some(GridRow {
            base,
            cols: self.cols(),
            _terminal: std::marker::PhantomData,
        })
    }

    /// Anchors a reference to the cell at `point`; it follows scrolling, reflow and pruning.
    pub fn track(&mut self, point: Point) -> Result<TrackedRef, GhosttyError> {
        let mut raw: sys::GhosttyTrackedGridRef = ptr::null_mut();
        // SAFETY: live terminal; `raw` receives an owned handle.
        check(unsafe {
            sys::ghostty_terminal_grid_ref_track(self.raw(), point.to_raw(), &raw mut raw)
        })?;
        NonNull::new(raw)
            .map(|raw| TrackedRef { raw })
            .ok_or(GhosttyError::OutOfMemory)
    }

    /// The raw handle, for [`RenderState::update`](super::RenderState::update).
    pub(super) fn as_raw(&mut self) -> sys::GhosttyTerminal {
        self.raw()
    }
}

/// Reads the grapheme cluster behind a fresh grid reference.
fn grid_ref_text(r: &sys::GhosttyGridRef) -> Option<String> {
    let mut stack = [0u32; 8];
    let mut len = 0usize;
    // SAFETY: `stack` has 8 writable codepoints; the reference is fresh.
    let result = unsafe {
        sys::ghostty_grid_ref_graphemes(r, stack.as_mut_ptr(), stack.len(), &raw mut len)
    };
    let heap;
    let codepoints: &[u32] = if result == sys::GHOSTTY_OUT_OF_SPACE {
        let mut buf = vec![0u32; len];
        // SAFETY: `buf` holds the reported count.
        let result = unsafe {
            sys::ghostty_grid_ref_graphemes(r, buf.as_mut_ptr(), buf.len(), &raw mut len)
        };
        check(result).ok()?;
        heap = buf;
        &heap[..len.min(heap.len())]
    } else {
        check(result).ok()?;
        &stack[..len.min(stack.len())]
    };
    Some(
        codepoints
            .iter()
            .filter_map(|&c| char::from_u32(c))
            .collect(),
    )
}

fn pointer_shape(shape: sys::GhosttyMouseShape) -> PointerShape {
    match shape {
        sys::GHOSTTY_MOUSE_SHAPE_HELP => PointerShape::Help,
        sys::GHOSTTY_MOUSE_SHAPE_POINTER => PointerShape::Pointer,
        sys::GHOSTTY_MOUSE_SHAPE_PROGRESS => PointerShape::Progress,
        sys::GHOSTTY_MOUSE_SHAPE_WAIT => PointerShape::Wait,
        sys::GHOSTTY_MOUSE_SHAPE_CELL => PointerShape::Cell,
        sys::GHOSTTY_MOUSE_SHAPE_CROSSHAIR => PointerShape::Crosshair,
        sys::GHOSTTY_MOUSE_SHAPE_TEXT => PointerShape::Text,
        sys::GHOSTTY_MOUSE_SHAPE_VERTICAL_TEXT => PointerShape::VerticalText,
        sys::GHOSTTY_MOUSE_SHAPE_ALIAS => PointerShape::Alias,
        sys::GHOSTTY_MOUSE_SHAPE_COPY => PointerShape::Copy,
        sys::GHOSTTY_MOUSE_SHAPE_MOVE => PointerShape::Move,
        sys::GHOSTTY_MOUSE_SHAPE_NO_DROP => PointerShape::NoDrop,
        sys::GHOSTTY_MOUSE_SHAPE_NOT_ALLOWED => PointerShape::NotAllowed,
        sys::GHOSTTY_MOUSE_SHAPE_GRAB => PointerShape::Grab,
        sys::GHOSTTY_MOUSE_SHAPE_GRABBING => PointerShape::Grabbing,
        sys::GHOSTTY_MOUSE_SHAPE_ALL_SCROLL => PointerShape::AllScroll,
        sys::GHOSTTY_MOUSE_SHAPE_COL_RESIZE | sys::GHOSTTY_MOUSE_SHAPE_EW_RESIZE => {
            PointerShape::EwResize
        }
        sys::GHOSTTY_MOUSE_SHAPE_ROW_RESIZE | sys::GHOSTTY_MOUSE_SHAPE_NS_RESIZE => {
            PointerShape::NsResize
        }
        sys::GHOSTTY_MOUSE_SHAPE_N_RESIZE => PointerShape::NResize,
        sys::GHOSTTY_MOUSE_SHAPE_E_RESIZE => PointerShape::EResize,
        sys::GHOSTTY_MOUSE_SHAPE_S_RESIZE => PointerShape::SResize,
        sys::GHOSTTY_MOUSE_SHAPE_W_RESIZE => PointerShape::WResize,
        sys::GHOSTTY_MOUSE_SHAPE_NE_RESIZE => PointerShape::NeResize,
        sys::GHOSTTY_MOUSE_SHAPE_NW_RESIZE => PointerShape::NwResize,
        sys::GHOSTTY_MOUSE_SHAPE_SE_RESIZE => PointerShape::SeResize,
        sys::GHOSTTY_MOUSE_SHAPE_SW_RESIZE => PointerShape::SwResize,
        sys::GHOSTTY_MOUSE_SHAPE_NESW_RESIZE => PointerShape::NeswResize,
        sys::GHOSTTY_MOUSE_SHAPE_NWSE_RESIZE => PointerShape::NwseResize,
        sys::GHOSTTY_MOUSE_SHAPE_ZOOM_IN => PointerShape::ZoomIn,
        sys::GHOSTTY_MOUSE_SHAPE_ZOOM_OUT => PointerShape::ZoomOut,
        // DEFAULT, CONTEXT_MENU (no host equivalent) and shapes added by later releases.
        _ => PointerShape::Default,
    }
}

/// A cell anchor that follows its cell through scrolling, reflow and scrollback pruning, and
/// reports no value once the cell is gone. Owned; may outlive its terminal.
///
/// Every tracked reference adds bookkeeping to each terminal change, so keep few.
pub(crate) struct TrackedRef {
    raw: NonNull<sys::GhosttyTrackedGridRefImpl>,
}

impl Drop for TrackedRef {
    fn drop(&mut self) {
        // SAFETY: owned handle; freeing after the terminal is allowed.
        unsafe { sys::ghostty_tracked_grid_ref_free(self.raw.as_ptr()) };
    }
}

impl std::fmt::Debug for TrackedRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TrackedRef")
            .field("has_value", &self.has_value())
            .finish()
    }
}

impl TrackedRef {
    /// Whether the cell still exists. False after pruning, or once the terminal is freed.
    pub fn has_value(&self) -> bool {
        // SAFETY: owned handle.
        unsafe { sys::ghostty_tracked_grid_ref_has_value(self.raw.as_ptr()) }
    }

    /// The cell's position in the coordinate system of `like` (its x/y are ignored), or `None`
    /// if the cell is gone or not representable there (e.g. scrolled out of the viewport).
    pub fn point(&self, like: Point) -> Option<Point> {
        let mut out = sys::GhosttyPointCoordinate { x: 0, y: 0 };
        // SAFETY: owned handle; `out` receives the coordinate.
        let result = unsafe {
            sys::ghostty_tracked_grid_ref_point(self.raw.as_ptr(), like.tag(), &raw mut out)
        };
        no_value(result, out)
            .ok()
            .flatten()
            .map(|c| like.with_coordinate(c))
    }

    /// The text of the tracked cell.
    pub fn text(&self) -> Option<String> {
        // SAFETY: GhosttyGridRef is plain data.
        let mut snapshot: sys::GhosttyGridRef = unsafe { std::mem::zeroed() };
        // SAFETY: owned handle; the snapshot is read before anything can mutate a terminal on
        // this thread (wrapper types are !Send).
        let result =
            unsafe { sys::ghostty_tracked_grid_ref_snapshot(self.raw.as_ptr(), &raw mut snapshot) };
        no_value(result, ()).ok().flatten()?;
        grid_ref_text(&snapshot)
    }
}

/// One grid row, read cell by cell through a grid reference. Borrows the terminal, so the row
/// cannot change while it is read.
///
/// The row is resolved once. Each cell is then addressed by setting the reference's column:
/// `GhosttyGridRef` has a frozen layout whose `x` is the column within the page row, and a page
/// row spans every column, so column `x` of the row is the reference with `x` replaced. The
/// engine's cell-level comparison with Alacritty (`screen/engine/ghostty/read/tests.rs`) reads
/// every column of thousands of rows this way.
pub(crate) struct GridRow<'t> {
    base: sys::GhosttyGridRef,
    cols: u16,
    _terminal: std::marker::PhantomData<&'t Terminal>,
}

impl GridRow<'_> {
    pub fn cols(&self) -> u16 {
        self.cols
    }

    fn at(&self, x: u16) -> sys::GhosttyGridRef {
        sys::GhosttyGridRef { x, ..self.base }
    }

    /// The packed cell at column `x` (0 past the end).
    pub fn raw(&self, x: u16) -> sys::GhosttyCell {
        if x >= self.cols {
            return 0;
        }
        let r = self.at(x);
        let mut cell: sys::GhosttyCell = 0;
        // SAFETY: a fresh reference into the borrowed terminal, column in range.
        unsafe { sys::ghostty_grid_ref_cell(&raw const r, &raw mut cell) };
        cell
    }

    /// The style of the cell at column `x`.
    pub fn style(&self, x: u16) -> Style {
        if x >= self.cols {
            return Style::default();
        }
        let r = self.at(x);
        // SAFETY: GhosttyStyle is plain data.
        let mut style: sys::GhosttyStyle = unsafe { std::mem::zeroed() };
        // SAFETY: as `raw`.
        let ok = unsafe { sys::ghostty_grid_ref_style(&raw const r, &raw mut style) };
        if ok == sys::GHOSTTY_SUCCESS {
            Style::from_raw(&style)
        } else {
            Style::default()
        }
    }

    /// Appends the codepoints of the cell at column `x`, base first.
    pub fn graphemes(&self, x: u16, out: &mut Vec<u32>) {
        if x >= self.cols {
            return;
        }
        let r = self.at(x);
        let mut stack = [0u32; 8];
        let mut len = 0usize;
        // SAFETY: as `raw`; `stack` holds 8 codepoints.
        let result = unsafe {
            sys::ghostty_grid_ref_graphemes(&raw const r, stack.as_mut_ptr(), 8, &raw mut len)
        };
        if result == sys::GHOSTTY_SUCCESS {
            out.extend_from_slice(&stack[..len.min(8)]);
        } else if result == sys::GHOSTTY_OUT_OF_SPACE {
            let start = out.len();
            out.resize(start + len, 0);
            // SAFETY: `out` has room for the reported count.
            let result = unsafe {
                sys::ghostty_grid_ref_graphemes(
                    &raw const r,
                    out[start..].as_mut_ptr(),
                    len,
                    &raw mut len,
                )
            };
            if result != sys::GHOSTTY_SUCCESS {
                out.truncate(start);
            }
        }
    }

    /// Writes the OSC 8 URI of the cell at column `x` into `out`; false when it has none.
    pub fn hyperlink_uri(&self, x: u16, out: &mut Vec<u8>) -> bool {
        out.clear();
        if x >= self.cols {
            return false;
        }
        hyperlink_into(&self.at(x), out)
    }

    /// Whether the row soft-wraps into the next.
    pub fn wraps(&self) -> bool {
        let mut row: sys::GhosttyRow = 0;
        // SAFETY: as `raw`.
        if unsafe { sys::ghostty_grid_ref_row(&raw const self.base, &raw mut row) }
            != sys::GHOSTTY_SUCCESS
        {
            return false;
        }
        let mut wrap = false;
        // SAFETY: ROW_DATA_WRAP writes a bool.
        unsafe { sys::ghostty_row_get(row, sys::GHOSTTY_ROW_DATA_WRAP, (&raw mut wrap).cast()) };
        wrap
    }
}

/// Writes the URI behind a fresh grid reference into `out`, reusing its allocation.
fn hyperlink_into(r: &sys::GhosttyGridRef, out: &mut Vec<u8>) -> bool {
    out.clear();
    let mut len = 0usize;
    // SAFETY: size query with the buffer's spare capacity; the reference is fresh.
    let result = unsafe {
        sys::ghostty_grid_ref_hyperlink_uri(r, out.as_mut_ptr(), out.capacity(), &raw mut len)
    };
    if result == sys::GHOSTTY_SUCCESS {
        // SAFETY: the library wrote `len` bytes into the capacity.
        unsafe { out.set_len(len.min(out.capacity())) };
        return len > 0;
    }
    if result != sys::GHOSTTY_OUT_OF_SPACE || len == 0 {
        return false;
    }
    out.reserve(len);
    // SAFETY: `out` now has `len` bytes of capacity.
    let result = unsafe {
        sys::ghostty_grid_ref_hyperlink_uri(r, out.as_mut_ptr(), out.capacity(), &raw mut len)
    };
    if result != sys::GHOSTTY_SUCCESS {
        return false;
    }
    // SAFETY: as above.
    unsafe { out.set_len(len.min(out.capacity())) };
    true
}
