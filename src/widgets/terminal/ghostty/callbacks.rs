//! Effect callbacks: Rust closures behind the C function pointers libghostty-vt calls during
//! `vt_write`, `resize` and `reset`.
//!
//! The terminal's userdata pointer is the [`Handlers`] of its [`Terminal`](super::Terminal),
//! allocated once and freed after the terminal. Each closure sits in its own `RefCell`, so a
//! callback the library invokes from inside another (a clipboard reply answering through
//! `write_pty`) borrows a different cell, and a callback can never alias itself. Closures are
//! only replaced through `&mut Terminal`, which no callback can reach.

use std::any::Any;
use std::cell::RefCell;
use std::ffi::c_void;
use std::panic::{AssertUnwindSafe, catch_unwind};

use super::{string_bytes, sys};

/// Receives bytes borrowed for the duration of the call.
pub(crate) type BytesFn = Box<dyn FnMut(&[u8])>;
/// Receives nothing.
pub(crate) type NotifyFn = Box<dyn FnMut()>;
/// Answers a clipboard write.
pub(crate) type ClipboardWriteFn = Box<dyn FnMut(&ClipboardWrite<'_>) -> ClipboardWriteReply>;
/// Receives a shell integration event.
pub(crate) type SemanticPromptFn = Box<dyn FnMut(&SemanticPrompt<'_>)>;
/// Receives a program status report.
pub(crate) type ProgramStatusFn = Box<dyn FnMut(&ProgramStatus<'_>)>;
/// Receives whether a synchronized-output hold began.
pub(crate) type RenderHoldFn = Box<dyn FnMut(bool)>;
/// Supplies the geometry for a size report.
pub(crate) type SizeFn = Box<dyn FnMut() -> Option<SizeReport>>;

type Slot<T> = RefCell<Option<T>>;

/// The closures registered on one terminal.
#[derive(Default)]
pub(super) struct Handlers {
    pub write_pty: Slot<BytesFn>,
    pub bell: Slot<NotifyFn>,
    pub title_changed: Slot<BytesFn>,
    pub pwd_changed: Slot<BytesFn>,
    pub clipboard_write: Slot<ClipboardWriteFn>,
    pub semantic_prompt: Slot<SemanticPromptFn>,
    pub program_status: Slot<ProgramStatusFn>,
    pub render_hold: Slot<RenderHoldFn>,
    pub reset: Slot<NotifyFn>,
    pub size: Slot<SizeFn>,
    /// The XTVERSION reply. The library reads it before the callback returns.
    pub xtversion: RefCell<Option<Box<[u8]>>>,
    /// A panic caught in a callback, resumed by the terminal once the library call returns.
    pub panic: RefCell<Option<Box<dyn Any + Send>>>,
}

impl Handlers {
    /// Resumes a panic that a callback raised during the last library call.
    pub fn resume_panic(&self) {
        if let Some(payload) = self.panic.borrow_mut().take() {
            std::panic::resume_unwind(payload);
        }
    }
}

/// Runs `f` with the handlers behind `userdata`, catching any panic.
///
/// # Safety
///
/// `userdata` must be NULL or the `Handlers` pointer the terminal was configured with, alive for
/// the duration of the call.
unsafe fn dispatch<R>(userdata: *mut c_void, fallback: R, f: impl FnOnce(&Handlers) -> R) -> R {
    if userdata.is_null() {
        return fallback;
    }
    // SAFETY: guaranteed by the caller. Only shared references to Handlers ever exist.
    let handlers = unsafe { &*userdata.cast::<Handlers>() };
    if handlers.panic.borrow().is_some() {
        // Something already panicked during this call; stay quiet until it is resumed.
        return fallback;
    }
    match catch_unwind(AssertUnwindSafe(|| f(handlers))) {
        Ok(value) => value,
        Err(payload) => {
            *handlers.panic.borrow_mut() = Some(payload);
            fallback
        }
    }
}

/// Calls the closure in `slot`, unless it is unset or already running.
fn call<T, R>(slot: &Slot<T>, f: impl FnOnce(&mut T) -> R) -> Option<R> {
    let mut guard = slot.try_borrow_mut().ok()?;
    guard.as_mut().map(f)
}

pub(super) unsafe extern "C" fn write_pty(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    data: *const u8,
    len: usize,
) {
    if data.is_null() {
        return;
    }
    // SAFETY: the library passes `len` readable bytes, valid for this call.
    let bytes = unsafe { std::slice::from_raw_parts(data, len) };
    // SAFETY: userdata is the terminal's Handlers.
    unsafe {
        dispatch(userdata, (), |h| {
            call(&h.write_pty, |f| f(bytes));
        })
    };
}

pub(super) unsafe extern "C" fn bell(_terminal: sys::GhosttyTerminal, userdata: *mut c_void) {
    // SAFETY: userdata is the terminal's Handlers.
    unsafe {
        dispatch(userdata, (), |h| {
            call(&h.bell, |f| f());
        })
    };
}

/// Reads a string-valued terminal datum from inside a callback.
///
/// # Safety
///
/// `terminal` must be the live terminal invoking the callback; the bytes are valid until it
/// next mutates, which is after the callback returns.
unsafe fn terminal_string<'a>(
    terminal: sys::GhosttyTerminal,
    key: sys::GhosttyTerminalData,
) -> &'a [u8] {
    let mut out = sys::GhosttyString {
        ptr: std::ptr::null(),
        len: 0,
    };
    // SAFETY: reading terminal data from a callback is allowed (the effects guide reads the
    // title this way); `out` has the documented output type.
    let result = unsafe { sys::ghostty_terminal_get(terminal, key, (&raw mut out).cast()) };
    if result != sys::GHOSTTY_SUCCESS {
        return &[];
    }
    // SAFETY: borrowed until the next mutating call, per GHOSTTY_TERMINAL_DATA_TITLE/PWD.
    unsafe { string_bytes(out) }
}

pub(super) unsafe extern "C" fn title_changed(
    terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
) {
    // SAFETY: called by the live terminal; userdata is its Handlers.
    unsafe {
        let title = terminal_string(terminal, sys::GHOSTTY_TERMINAL_DATA_TITLE);
        dispatch(userdata, (), |h| {
            call(&h.title_changed, |f| f(title));
        });
    }
}

pub(super) unsafe extern "C" fn pwd_changed(terminal: sys::GhosttyTerminal, userdata: *mut c_void) {
    // SAFETY: called by the live terminal; userdata is its Handlers.
    unsafe {
        let pwd = terminal_string(terminal, sys::GHOSTTY_TERMINAL_DATA_PWD);
        dispatch(userdata, (), |h| {
            call(&h.pwd_changed, |f| f(pwd));
        });
    }
}

pub(super) unsafe extern "C" fn render_hold(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    held: bool,
) {
    // SAFETY: userdata is the terminal's Handlers.
    unsafe {
        dispatch(userdata, (), |h| {
            call(&h.render_hold, |f| f(held));
        })
    };
}

pub(super) unsafe extern "C" fn reset(_terminal: sys::GhosttyTerminal, userdata: *mut c_void) {
    // SAFETY: userdata is the terminal's Handlers.
    unsafe {
        dispatch(userdata, (), |h| {
            call(&h.reset, |f| f());
        })
    };
}

/// The geometry reported for XTWINOPS size queries and in-band size reports (mode 2048).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SizeReport {
    pub rows: u16,
    pub columns: u16,
    pub cell_width: u32,
    pub cell_height: u32,
}

pub(super) unsafe extern "C" fn size(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    out: *mut sys::GhosttySizeReportSize,
) -> bool {
    // SAFETY: userdata is the terminal's Handlers.
    let report = unsafe { dispatch(userdata, None, |h| call(&h.size, |f| f()).flatten()) };
    let Some(report) = report else {
        return false;
    };
    if out.is_null() {
        return false;
    }
    // SAFETY: the library passes a valid, zeroed output struct.
    unsafe {
        *out = sys::GhosttySizeReportSize {
            rows: report.rows,
            columns: report.columns,
            cell_width: report.cell_width,
            cell_height: report.cell_height,
        };
    }
    true
}

pub(super) unsafe extern "C" fn xtversion(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
) -> sys::GhosttyString {
    let empty = sys::GhosttyString {
        ptr: std::ptr::null(),
        len: 0,
    };
    // SAFETY: userdata is the terminal's Handlers. The returned pointer is into the boxed reply,
    // which only `&mut Terminal` can replace, so it outlives this callback as required.
    unsafe {
        dispatch(userdata, empty, |h| match h.xtversion.borrow().as_deref() {
            Some(reply) => sys::GhosttyString {
                ptr: reply.as_ptr(),
                len: reply.len(),
            },
            None => empty,
        })
    }
}

/// Where a clipboard write goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardLocation {
    Standard,
    Selection,
    Primary,
}

/// One MIME representation of a clipboard write.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ClipboardContent<'a> {
    pub mime: &'a [u8],
    pub data: &'a [u8],
}

/// A program's request to write the clipboard (OSC 52, OSC 1337 Copy, OSC 5522). Every entry in
/// `contents` represents the same value; no contents means clear the clipboard.
#[derive(Debug)]
pub(crate) struct ClipboardWrite<'a> {
    pub location: ClipboardLocation,
    pub contents: Vec<ClipboardContent<'a>>,
    /// The writing program's name, if the protocol carries one.
    pub name: &'a [u8],
    /// Whether the terminal already holds a session grant for this program.
    pub granted: bool,
    /// Whether [`ClipboardWriteReply::remember`] may record a grant.
    pub can_remember: bool,
}

/// The outcome of a clipboard write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClipboardWriteResult {
    Success,
    Denied,
    Unsupported,
    Busy,
    InvalidData,
    IoError,
}

/// The answer to a [`ClipboardWrite`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClipboardWriteReply {
    pub result: ClipboardWriteResult,
    /// Record a session grant so this program's later writes arrive with `granted` set.
    pub remember: bool,
}

pub(super) unsafe extern "C" fn clipboard_write(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    write: *const sys::GhosttyClipboardWrite,
) {
    if write.is_null() {
        return;
    }
    // SAFETY: the library passes a valid request whose arrays and strings live for this call.
    let raw = unsafe { &*write };
    let location = match raw.location {
        sys::GHOSTTY_CLIPBOARD_LOCATION_SELECTION => ClipboardLocation::Selection,
        sys::GHOSTTY_CLIPBOARD_LOCATION_PRIMARY => ClipboardLocation::Primary,
        _ => ClipboardLocation::Standard,
    };
    let entries = if raw.contents.is_null() || raw.contents_len == 0 {
        &[][..]
    } else {
        // SAFETY: `contents` holds `contents_len` entries for this call.
        unsafe { std::slice::from_raw_parts(raw.contents, raw.contents_len) }
    };
    let request = ClipboardWrite {
        location,
        contents: entries
            .iter()
            // SAFETY: borrowed strings valid for this call.
            .map(|c| unsafe {
                ClipboardContent {
                    mime: string_bytes(c.mime),
                    data: string_bytes(c.data),
                }
            })
            .collect(),
        // SAFETY: borrowed string valid for this call.
        name: unsafe { string_bytes(raw.name) },
        granted: raw.granted,
        can_remember: raw.can_remember,
    };
    // SAFETY: userdata is the terminal's Handlers.
    let reply = unsafe {
        dispatch(userdata, None, |h| {
            call(&h.clipboard_write, |f| f(&request))
        })
    };
    // No reply means denied, which is what the library assumes when we return without one.
    let (Some(reply), Some(reply_fn)) = (reply, raw.reply) else {
        return;
    };
    let result = match reply.result {
        ClipboardWriteResult::Success => sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_SUCCESS,
        ClipboardWriteResult::Denied => sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_DENIED,
        ClipboardWriteResult::Unsupported => sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_UNSUPPORTED,
        ClipboardWriteResult::Busy => sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_BUSY,
        ClipboardWriteResult::InvalidData => sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_INVALID_DATA,
        ClipboardWriteResult::IoError => sys::GHOSTTY_CLIPBOARD_WRITE_RESULT_IO_ERROR,
    };
    // SAFETY: GhosttyClipboardWriteReply is a sized struct.
    let mut answer: sys::GhosttyClipboardWriteReply = unsafe { super::sized() };
    answer.result = result;
    answer.remember = reply.remember && raw.can_remember;
    // SAFETY: `reply` is the library's function for this request, called before returning.
    // It may run `write_pty`, whose slot is not borrowed here.
    unsafe { reply_fn(write, &raw const answer) };
}

/// A step of a command, reported by shell integration (OSC 133).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SemanticPromptKind {
    /// The shell started drawing a prompt.
    PromptStart(PromptKind),
    /// The prompt is drawn and the user can type.
    InputStart,
    /// The command started; what follows is its output.
    OutputStart,
    /// The command finished.
    CommandEnd,
}

/// Which prompt a [`SemanticPromptKind::PromptStart`] starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PromptKind {
    Primary,
    Right,
    Continuation,
    Secondary,
}

/// A shell integration event. Strings are borrowed for the callback.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SemanticPrompt<'a> {
    pub kind: SemanticPromptKind,
    /// Set for [`SemanticPromptKind::CommandEnd`] when the shell reported it.
    pub exit_code: Option<i32>,
    /// The command line, for [`SemanticPromptKind::OutputStart`], decoded; empty if not sent.
    pub command: &'a [u8],
    /// What went wrong, for [`SemanticPromptKind::CommandEnd`]; usually empty.
    pub error: &'a [u8],
}

pub(super) unsafe extern "C" fn semantic_prompt(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    event: *const sys::GhosttyTerminalSemanticPrompt,
) {
    if event.is_null() {
        return;
    }
    // SAFETY: the library passes a valid event whose strings live for this call.
    let raw = unsafe { &*event };
    let kind = match raw.kind {
        sys::GHOSTTY_SEMANTIC_PROMPT_PROMPT_START => {
            SemanticPromptKind::PromptStart(match raw.prompt_kind {
                sys::GHOSTTY_SEMANTIC_PROMPT_PROMPT_RIGHT => PromptKind::Right,
                sys::GHOSTTY_SEMANTIC_PROMPT_PROMPT_CONTINUATION => PromptKind::Continuation,
                sys::GHOSTTY_SEMANTIC_PROMPT_PROMPT_SECONDARY => PromptKind::Secondary,
                _ => PromptKind::Primary,
            })
        }
        sys::GHOSTTY_SEMANTIC_PROMPT_INPUT_START => SemanticPromptKind::InputStart,
        sys::GHOSTTY_SEMANTIC_PROMPT_OUTPUT_START => SemanticPromptKind::OutputStart,
        sys::GHOSTTY_SEMANTIC_PROMPT_COMMAND_END => SemanticPromptKind::CommandEnd,
        // Kinds added by later releases.
        _ => return,
    };
    let prompt = SemanticPrompt {
        kind,
        exit_code: raw.has_exit_code.then_some(raw.exit_code),
        // SAFETY: borrowed strings valid for this call.
        command: unsafe { string_bytes(raw.command) },
        // SAFETY: as above.
        error: unsafe { string_bytes(raw.error) },
    };
    // SAFETY: userdata is the terminal's Handlers.
    unsafe {
        dispatch(userdata, (), |h| {
            call(&h.semantic_prompt, |f| f(&prompt));
        })
    };
}

/// What a program reports it is doing (OSC 7501).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgramStatusState {
    Idle,
    Working,
    Done,
    Blocked,
    Error,
    /// Remove the record with this id and every record beneath it; an empty id removes all.
    Clear,
}

/// What a blocked program needs (OSC 7501).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProgramStatusKind {
    None,
    Permission,
    Question,
    Auth,
}

/// A validated program status report (OSC 7501). Strings are borrowed for the callback and are
/// untrusted program text.
#[derive(Debug, Clone, Copy)]
pub(crate) struct ProgramStatus<'a> {
    pub state: ProgramStatusState,
    pub kind: ProgramStatusKind,
    /// 0 through 100, or `None` when indeterminate or not applicable.
    pub progress: Option<u8>,
    /// The record id; empty for the root record.
    pub id: &'a [u8],
    pub app: &'a [u8],
    pub title: &'a [u8],
    pub message: &'a [u8],
}

pub(super) unsafe extern "C" fn program_status(
    _terminal: sys::GhosttyTerminal,
    userdata: *mut c_void,
    report: *const sys::GhosttyTerminalProgramStatus,
) {
    if report.is_null() {
        return;
    }
    // SAFETY: the library passes a valid report whose strings live for this call.
    let raw = unsafe { &*report };
    let state = match raw.state {
        sys::GHOSTTY_PROGRAM_STATUS_STATE_IDLE => ProgramStatusState::Idle,
        sys::GHOSTTY_PROGRAM_STATUS_STATE_WORKING => ProgramStatusState::Working,
        sys::GHOSTTY_PROGRAM_STATUS_STATE_DONE => ProgramStatusState::Done,
        sys::GHOSTTY_PROGRAM_STATUS_STATE_BLOCKED => ProgramStatusState::Blocked,
        sys::GHOSTTY_PROGRAM_STATUS_STATE_ERROR => ProgramStatusState::Error,
        sys::GHOSTTY_PROGRAM_STATUS_STATE_CLEAR => ProgramStatusState::Clear,
        _ => return,
    };
    let kind = match raw.kind {
        sys::GHOSTTY_PROGRAM_STATUS_KIND_PERMISSION => ProgramStatusKind::Permission,
        sys::GHOSTTY_PROGRAM_STATUS_KIND_QUESTION => ProgramStatusKind::Question,
        sys::GHOSTTY_PROGRAM_STATUS_KIND_AUTH => ProgramStatusKind::Auth,
        _ => ProgramStatusKind::None,
    };
    // SAFETY: borrowed strings valid for this call.
    let status = unsafe {
        ProgramStatus {
            state,
            kind,
            progress: u8::try_from(raw.progress).ok().filter(|p| *p <= 100),
            id: string_bytes(raw.id),
            app: string_bytes(raw.app),
            title: string_bytes(raw.title),
            message: string_bytes(raw.message),
        }
    };
    // SAFETY: userdata is the terminal's Handlers.
    unsafe {
        dispatch(userdata, (), |h| {
            call(&h.program_status, |f| f(&status));
        })
    };
}
