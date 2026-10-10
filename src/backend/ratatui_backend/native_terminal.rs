use std::io::{self, BufWriter, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::app::context::SurfaceMode;
use crate::style::{
    drain_pending_terminal_responses, flush_pending_terminal_responses_on_exit, host_renders_utf8,
    query_host_capabilities_with,
};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::style::Print;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::{TerminalOptions, Viewport};

use super::terminal_handoff::reset_handoff_state_for_terminal_restore;
#[cfg(unix)]
use super::terminal_transition::theme_notification_plan;
use super::terminal_transition::{
    CrosstermTransitionExecutor, enter_plan, execute_plan, execute_plan_with_rollback, exit_plan,
    modifier_key_reporting_plan,
};

#[cfg(feature = "image")]
use super::image_support;

type TerminalWriter = BufWriter<FrameOutput>;
const TERMINAL_BUFFER_CAPACITY: usize = 64 * 1024;

/// Whether the keyboard enhancement a surface pushed is live on the main screen,
/// which the shell shares with an inline surface. A fullscreen surface pushes
/// onto the alternate screen's own stack and never sets this.
///
/// Swapped rather than read wherever the push is popped, so the stack stays
/// balanced however many of the exit, suspend and background-stop paths run.
pub(crate) static MAIN_SCREEN_KEYBOARD_PUSHED: AtomicBool = AtomicBool::new(false);

/// Whether the running inline surface uses keyboard enhancement at all, so a
/// resume knows to push it again after a suspend popped it.
pub(crate) static MAIN_SCREEN_KEYBOARD_WANTED: AtomicBool = AtomicBool::new(false);

/// Whether an exit plan should pop the keyboard enhancement: always on the
/// alternate screen, and on the main screen only while the push is still live.
fn pop_keyboard_on_exit(policy: SurfaceTerminalPolicy, keyboard_enhancement: bool) -> bool {
    MAIN_SCREEN_KEYBOARD_WANTED.store(false, Ordering::SeqCst);
    let main_screen_pushed = MAIN_SCREEN_KEYBOARD_PUSHED.swap(false, Ordering::SeqCst);
    keyboard_enhancement && (policy.uses_alternate_screen || main_screen_pushed)
}

/// Where frames go: the terminal on standard output, unbuffered here because
/// the [`BufWriter`] above it is the only buffer frames may sit in.
///
/// A background stop releases the terminal from a signal handler, wherever the
/// renderer happens to be, and the renderer carries on from there once the
/// process is continued. Every write therefore goes through
/// [`frame_write`](crate::app::job_control::frame_write), which drops the frame
/// once the terminal is released and makes the release wait for a write already
/// under way; the runner repaints in full once it has taken the terminal back.
///
/// The release waits for that write, so it should be short. Frames go through a
/// descriptor of their own opened non-blocking: a full terminal is waited out
/// in `poll`, outside `frame_write`, and checked again before the next write.
/// Where it cannot be reopened, frames fall back to standard output, blocking:
/// still never after the release, but the release may then wait for the
/// terminal to drain. Writing to a descriptor directly, rather than through
/// [`Stdout`], also keeps a dropped frame's tail from waiting in `Stdout`'s own
/// buffer for the next writer to flush it.
pub(crate) struct FrameOutput {
    // Successful writes discarded by job control still invalidate the frame's handoffs.
    dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(test)]
    test_buffer: Option<std::sync::Arc<std::sync::Mutex<Vec<u8>>>>,
    #[cfg(test)]
    test_job_control: bool,
    #[cfg(unix)]
    terminal: Option<std::os::fd::OwnedFd>,
    #[cfg(not(unix))]
    stdout: Stdout,
}

impl FrameOutput {
    pub(crate) fn new() -> Self {
        Self {
            dropped: Default::default(),
            #[cfg(test)]
            test_buffer: None,
            #[cfg(test)]
            test_job_control: false,
            #[cfg(unix)]
            terminal: open_stdout_terminal_nonblocking(),
            #[cfg(not(unix))]
            stdout: io::stdout(),
        }
    }
}

/// A new, non-blocking open of the terminal standard output refers to, or
/// `None` when it is not a terminal or cannot be opened again; frames then go
/// to standard output itself. Its own open file description, so the flag never
/// reaches the shell sharing standard output's.
#[cfg(unix)]
fn open_stdout_terminal_nonblocking() -> Option<std::os::fd::OwnedFd> {
    use std::os::fd::FromRawFd;

    // SAFETY: `isatty` takes no pointers.
    #[allow(unsafe_code)]
    if unsafe { libc::isatty(libc::STDOUT_FILENO) } != 1 {
        return None;
    }
    let mut name = [0 as libc::c_char; 256];
    // SAFETY: `name` is a writable buffer of the length passed, and `ttyname_r`
    // fails rather than overrun it.
    #[allow(unsafe_code)]
    let named = unsafe { libc::ttyname_r(libc::STDOUT_FILENO, name.as_mut_ptr(), name.len()) };
    if named != 0 {
        crate::debug::internal_log!(
            "[tui-lipan] frame output: no path for the terminal on stdout ({}), writing \
             frames to stdout blocking",
            io::Error::from_raw_os_error(named)
        );
        return None;
    }
    // SAFETY: `ttyname_r` succeeded, so `name` holds a nul-terminated path.
    #[allow(unsafe_code)]
    let fd = unsafe {
        libc::open(
            name.as_ptr(),
            libc::O_WRONLY | libc::O_NOCTTY | libc::O_NONBLOCK | libc::O_CLOEXEC,
        )
    };
    if fd < 0 {
        crate::debug::internal_log!(
            "[tui-lipan] frame output: reopening the terminal failed ({}), writing frames to \
             stdout blocking",
            io::Error::last_os_error()
        );
        return None;
    }
    // SAFETY: a non-negative `fd` was just opened here and nothing else owns it.
    #[allow(unsafe_code)]
    Some(unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) })
}

impl Write for FrameOutput {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        #[cfg(test)]
        if let Some(buffer) = &mut self.test_buffer {
            let mut buffer = buffer
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !self.test_job_control {
                buffer.extend_from_slice(buf);
                return Ok(buf.len());
            }
            if !crate::app::job_control::frame_write(false, || {
                buffer.extend_from_slice(buf);
                true
            }) {
                self.dropped.fetch_add(1, Ordering::Relaxed);
            }
            return Ok(buf.len());
        }
        cfg_select! {
            unix => {
                use std::os::fd::AsRawFd;

                let fd = self
                    .terminal
                    .as_ref()
                    .map_or(libc::STDOUT_FILENO, AsRawFd::as_raw_fd);
                loop {
                    let written = crate::app::job_control::frame_write(None, || {
                        // SAFETY: the pointer and length describe the live `buf` slice.
                        #[allow(unsafe_code)]
                        let written = unsafe { libc::write(fd, buf.as_ptr().cast(), buf.len()) };
                        Some(usize::try_from(written).map_err(|_| io::Error::last_os_error()))
                    });
                    let err = match written {
                        None => {
                            self.dropped.fetch_add(1, Ordering::Relaxed);
                            return Ok(buf.len());
                        },
                        Some(Ok(written)) => return Ok(written),
                        Some(Err(err)) => err,
                    };
                    match err.kind() {
                        io::ErrorKind::Interrupted => {}
                        io::ErrorKind::WouldBlock => wait_until_writable(fd),
                        _ => return Err(err),
                    }
                }
            }
            _ => {
                let stdout = &mut self.stdout;
                match crate::app::job_control::frame_write(None, || Some(stdout.write(buf))) {
                    Some(result) => result,
                    None => {
                        self.dropped.fetch_add(1, Ordering::Relaxed);
                        Ok(buf.len())
                    }
                }
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        #[cfg(test)]
        if self.test_buffer.is_some() {
            return Ok(());
        }
        cfg_select! {
            unix => Ok(()),
            _ => self.stdout.flush(),
        }
    }
}

/// Wait for room in the terminal's output queue. A signal ends the wait early,
/// which is the point: the caller checks the frame output again first.
#[cfg(unix)]
fn wait_until_writable(fd: libc::c_int) {
    let mut descriptor = libc::pollfd {
        fd,
        events: libc::POLLOUT,
        revents: 0,
    };
    // SAFETY: `descriptor` is one valid `pollfd`, as the count says.
    #[allow(unsafe_code)]
    unsafe {
        libc::poll(&mut descriptor, 1, -1)
    };
}

/// Image id for the startup graphics probe, high enough not to collide with a real placement's.
#[cfg(feature = "terminal-images")]
const GRAPHICS_PROBE_ID: u32 = u32::MAX;

pub(crate) type Terminal = ratatui::Terminal<HostBackend<TerminalWriter>>;

/// How long a cursor position query waits for the terminal, as crossterm's own query does.
const CURSOR_REPLY_TIMEOUT: Duration = Duration::from_secs(2);

/// The crossterm backend with guarded graphics handoffs and cursor queries through [`host_input`].
///
/// ratatui asks for the cursor whenever it places an inline viewport: on creation, on autoresize,
/// and around `insert_before`. crossterm answers that by reading its own Unix event source, which
/// spins forever if the terminal hangs up during the wait, and which would compete with the
/// runner's reader for the reply. Graphics handoffs become final only after a successful flush
/// without discarded frame output; ordinary placements are deleted on the following frame.
///
/// [`host_input`]: super::host_input
pub(crate) struct HostBackend<W: io::Write> {
    backend: CrosstermBackend<W>,
    #[cfg(all(test, feature = "terminal-images"))]
    test_output: Option<std::sync::Arc<std::sync::Mutex<Vec<u8>>>>,
    dropped: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(feature = "terminal-images")]
    placement_deletes: Vec<String>,
    #[cfg(feature = "terminal-images")]
    placement_retirement: Option<(usize, usize)>,
    #[cfg(feature = "terminal-images")]
    uploads: Vec<(std::sync::Arc<super::renderers::image::KittyUpload>, usize)>,
}

impl<W: io::Write> HostBackend<W> {
    pub(crate) fn new(writer: W) -> Self {
        Self {
            backend: CrosstermBackend::new(writer),
            #[cfg(all(test, feature = "terminal-images"))]
            test_output: None,
            dropped: Default::default(),
            #[cfg(feature = "terminal-images")]
            uploads: Vec::new(),
            #[cfg(feature = "terminal-images")]
            placement_deletes: Vec::new(),
            #[cfg(feature = "terminal-images")]
            placement_retirement: None,
        }
    }

    fn flush_frame(&mut self) -> io::Result<()> {
        let result = io::Write::flush(&mut self.backend);
        #[cfg(feature = "terminal-images")]
        if result.is_ok()
            && let Some((retired, epoch)) = self.placement_retirement.take()
            && epoch == self.dropped.load(Ordering::Relaxed)
        {
            self.placement_deletes.drain(..retired);
        }
        #[cfg(feature = "terminal-images")]
        for (upload, epoch) in self.uploads.drain(..) {
            if result.is_ok() && epoch == self.dropped.load(Ordering::Relaxed) {
                upload.handed_over();
            }
        }
        result
    }
}

impl HostBackend<TerminalWriter> {
    #[cfg(all(test, feature = "terminal-images"))]
    pub(crate) fn test_output(&self) -> Vec<u8> {
        self.test_output
            .as_ref()
            .expect("test output")
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn buffered(writer: TerminalWriter) -> Self {
        let dropped = std::sync::Arc::clone(&writer.get_ref().dropped);
        #[cfg(all(test, feature = "terminal-images"))]
        let test_output = writer.get_ref().test_buffer.clone();
        let mut backend = Self::new(writer);
        #[cfg(all(test, feature = "terminal-images"))]
        {
            backend.test_output = test_output;
        }
        backend.dropped = dropped;
        backend
    }
}

impl<W: io::Write> io::Write for HostBackend<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.backend.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_frame()
    }
}

impl<W: io::Write> Backend for HostBackend<W> {
    type Error = io::Error;

    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a ratatui::buffer::Cell)>,
    {
        #[cfg(feature = "terminal-images")]
        {
            use super::renderers::image::{kitty_draw_pending, prepare_kitty_cells};
            let epoch = self.dropped.load(Ordering::Relaxed);
            self.placement_retirement = None;
            for delete in &self.placement_deletes {
                self.backend.write_all(delete.as_bytes())?;
            }
            self.placement_retirement = Some((self.placement_deletes.len(), epoch));
            if !kitty_draw_pending() {
                return self.backend.draw(content);
            }
            let prepared = prepare_kitty_cells(content);
            self.placement_deletes.extend(prepared.placement_deletes);
            self.backend.draw(
                prepared
                    .cells
                    .iter()
                    .map(|(x, y, cell)| (*x, *y, cell.as_ref())),
            )?;
            // Draw may only fill BufWriter. Acknowledge after flush, provided FrameOutput
            // has not discarded any part of this frame during terminal release.
            self.uploads
                .extend(prepared.uploads.into_iter().map(|upload| (upload, epoch)));
            Ok(())
        }
        #[cfg(not(feature = "terminal-images"))]
        self.backend.draw(content)
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.backend.append_lines(n)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.backend.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.backend.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<ratatui::layout::Position> {
        // Anything still buffered has to reach the terminal before it can say where the cursor is.
        self.flush_frame()?;
        match super::host_input::cursor_position(CURSOR_REPLY_TIMEOUT)? {
            Some((x, y)) => Ok(ratatui::layout::Position { x, y }),
            None => Err(io::Error::other("the cursor position could not be read")),
        }
    }

    fn set_cursor_position<P: Into<ratatui::layout::Position>>(
        &mut self,
        position: P,
    ) -> io::Result<()> {
        self.backend.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.backend.clear()
    }

    fn clear_region(&mut self, clear_type: ratatui::backend::ClearType) -> io::Result<()> {
        self.backend.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<ratatui::layout::Size> {
        self.backend.size()
    }

    fn window_size(&mut self) -> io::Result<ratatui::backend::WindowSize> {
        self.backend.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_frame()
    }

    fn scroll_region_up(
        &mut self,
        region: std::ops::Range<u16>,
        line_count: u16,
    ) -> io::Result<()> {
        self.backend.scroll_region_up(region, line_count)
    }

    fn scroll_region_down(
        &mut self,
        region: std::ops::Range<u16>,
        line_count: u16,
    ) -> io::Result<()> {
        self.backend.scroll_region_down(region, line_count)
    }
}

fn buffered_stdout() -> TerminalWriter {
    BufWriter::with_capacity(TERMINAL_BUFFER_CAPACITY, FrameOutput::new())
}

#[cfg(all(test, feature = "terminal-images"))]
pub(crate) fn buffered_test_output() -> (
    HostBackend<TerminalWriter>,
    std::sync::MutexGuard<'static, ()>,
) {
    let state = crate::app::job_control::lock_test_terminal_state();
    let backend = HostBackend::buffered(BufWriter::with_capacity(
        TERMINAL_BUFFER_CAPACITY,
        FrameOutput {
            dropped: Default::default(),
            test_buffer: Some(Default::default()),
            test_job_control: true,
            #[cfg(unix)]
            terminal: None,
            #[cfg(not(unix))]
            stdout: io::stdout(),
        },
    ));
    (backend, state)
}

/// Exercise the production renderer without reading or writing the host terminal.
#[cfg(all(test, feature = "terminal"))]
pub(crate) fn create_test_terminal(width: u16, height: u16) -> io::Result<Terminal> {
    let output = FrameOutput {
        dropped: Default::default(),
        test_buffer: Some(Default::default()),
        test_job_control: false,
        #[cfg(unix)]
        terminal: None,
        #[cfg(not(unix))]
        stdout: io::stdout(),
    };
    let backend = HostBackend::buffered(BufWriter::new(output));
    ratatui::Terminal::with_options(
        backend,
        TerminalOptions {
            viewport: Viewport::Fixed(ratatui::layout::Rect::new(0, 0, width, height)),
        },
    )
}

pub(crate) fn create_inline_terminal(height: u16) -> io::Result<Terminal> {
    let backend = HostBackend::buffered(buffered_stdout());
    let options = TerminalOptions {
        viewport: Viewport::Inline(height.max(1)),
    };
    ratatui::Terminal::with_options(backend, options)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SurfaceTerminalPolicy {
    pub(crate) uses_alternate_screen: bool,
    pub(crate) disable_auto_wrap: bool,
    pub(crate) clear_on_start: bool,
}

pub(crate) fn surface_terminal_policy(surface_mode: SurfaceMode) -> SurfaceTerminalPolicy {
    let inline_disable_auto_wrap = matches!(surface_mode, SurfaceMode::InlineEphemeral { .. });
    SurfaceTerminalPolicy {
        uses_alternate_screen: !surface_mode.is_inline(),
        disable_auto_wrap: inline_disable_auto_wrap,
        clear_on_start: surface_mode.clear_on_start(),
    }
}

pub(crate) struct DisableMouseAllMotion;

impl crossterm::Command for DisableMouseAllMotion {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1003l\x1b[?1006l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

pub(crate) struct DisableAutoWrap;

impl crossterm::Command for DisableAutoWrap {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?7l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

pub(crate) struct EnableAutoWrap;

impl crossterm::Command for EnableAutoWrap {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?7h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

struct EnableMouseMotionTracking;

impl crossterm::Command for EnableMouseMotionTracking {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1003h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

struct DisableMouseMotionTracking;

impl crossterm::Command for DisableMouseMotionTracking {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1003l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

pub(crate) fn set_mouse_all_motion_enabled(
    writer: &mut impl std::io::Write,
    enabled: bool,
) -> io::Result<()> {
    if enabled {
        execute!(writer, EnableMouseMotionTracking)?;
    } else {
        execute!(writer, DisableMouseMotionTracking)?;
        // Keep button and drag reports enabled without changing their coordinate encoding.
        // Re-enabling 1006 here can replace an active SGR-pixels (1016) mode while the input
        // decoder still expects pixel coordinates.
        execute!(writer, Print("\x1b[?1000h\x1b[?1002h"))?;
    }
    Ok(())
}

pub(crate) fn set_mouse_capture_enabled(
    writer: &mut impl std::io::Write,
    enabled: bool,
) -> io::Result<()> {
    if enabled {
        execute!(writer, EnableMouseCapture)?;
    } else {
        execute!(writer, DisableMouseAllMotion, DisableMouseCapture)?;
    }
    Ok(())
}

/// A ratatui terminal dropped so that a host which has gone away cannot abort the exit.
///
/// ratatui's own `Drop` shows a cursor it hid and reports a failure with `eprintln!`. Once the host
/// terminal has hung up, the write and the report both fail, and a failed `eprintln!` panics: under
/// `panic = "abort"` that is a core dump on the way out of an otherwise clean exit. So the cursor is
/// shown here first, and a terminal that cannot take even that is not dropped at all. Nothing can
/// use it by then, and its buffers go when the process does.
pub(crate) struct OwnedTerminal<B: Backend = HostBackend<TerminalWriter>>(
    Option<ratatui::Terminal<B>>,
);

impl<B: Backend> OwnedTerminal<B> {
    pub(crate) fn new(terminal: ratatui::Terminal<B>) -> Self {
        Self(Some(terminal))
    }
}

impl<B: Backend> std::ops::Deref for OwnedTerminal<B> {
    type Target = ratatui::Terminal<B>;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("the terminal is only taken when dropped")
    }
}

impl<B: Backend> std::ops::DerefMut for OwnedTerminal<B> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
            .as_mut()
            .expect("the terminal is only taken when dropped")
    }
}

impl<B: Backend> Drop for OwnedTerminal<B> {
    fn drop(&mut self) {
        if let Some(mut terminal) = self.0.take()
            && terminal.show_cursor().is_err()
        {
            std::mem::forget(terminal);
        }
    }
}

pub(crate) struct TerminalGuard {
    stdout: Stdout,
    policy: SurfaceTerminalPolicy,
    keyboard_enhancement: bool,
    modifier_key_reporting: bool,
    theme_notifications: bool,
}

impl TerminalGuard {
    pub(crate) fn enter(
        surface_mode: SurfaceMode,
        mouse_enabled: bool,
        require_utf8: bool,
        panic_keyboard_enhancement: &AtomicBool,
    ) -> io::Result<(OwnedTerminal, Self)> {
        let policy = surface_terminal_policy(surface_mode);
        let stdout = io::stdout();
        // The object outlives the query so a terminal that declines it still finds it there, and is
        // unlinked when this scope ends whether or not the terminal read it.
        //
        // Nothing is asked of a host that cannot answer. This runs before the alternate screen is
        // entered, so a terminal that prints an `APC` string instead of consuming one leaves the
        // question in the user's scrollback for good.
        #[cfg(feature = "terminal-images")]
        let graphics_probe = super::shared_frame::worth_asking_about_shared_memory()
            .then(|| super::shared_frame::kitty_shared_memory_probe(GRAPHICS_PROBE_ID))
            .flatten();
        #[cfg(feature = "terminal-images")]
        let probe_bytes = graphics_probe
            .as_ref()
            .map(|(_, query)| query.as_bytes())
            .unwrap_or_default();
        #[cfg(not(feature = "terminal-images"))]
        let probe_bytes: &[u8] = &[];
        let capabilities =
            query_host_capabilities_with(probe_bytes, require_utf8).unwrap_or_default();
        // Refused here, before raw mode or the alternate screen, so the error lands on a terminal
        // that is exactly as the user left it. A probe reply still in flight is dropped first, or
        // it would surface at the shell prompt. The probe has already restored cooked mode, so raw
        // mode is held for the drain: in cooked mode the tty would echo the replies it reads.
        if require_utf8 && host_renders_utf8() == Some(false) {
            let raw = crossterm::terminal::enable_raw_mode().is_ok();
            flush_pending_terminal_responses_on_exit();
            if raw {
                let _ = crossterm::terminal::disable_raw_mode();
            }
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "the terminal does not display UTF-8 text",
            ));
        }
        let keyboard_enhancement = capabilities.keyboard_enhancement;
        // Recorded rather than acted on: the mode is only worth asking for once the cell size is
        // known too. Both halves have to be settled here, before the runner chooses an input
        // decoder, because only one of them can read a pixel report - asking a decoder that cannot
        // for one would put every click in the wrong cell.
        #[cfg(unix)]
        {
            crate::app::input::pixel_mouse::note_host_support(capabilities.pixel_mouse);
            if let Ok(size) = crossterm::terminal::window_size() {
                crate::app::input::pixel_mouse::note_window_size(
                    size.columns,
                    size.rows,
                    Some(size.width),
                    Some(size.height),
                );
            }
        }
        #[cfg(feature = "terminal-images")]
        super::shared_frame::note_host_support(
            graphics_probe.is_some() && capabilities.graphics_query_ok,
        );
        let plan = enter_plan(policy, mouse_enabled, keyboard_enhancement);
        let mut executor = CrosstermTransitionExecutor::new(stdout);
        execute_plan_with_rollback(&mut executor, &plan)?;
        panic_keyboard_enhancement.store(keyboard_enhancement, Ordering::SeqCst);
        let main_screen_keyboard = keyboard_enhancement && !policy.uses_alternate_screen;
        MAIN_SCREEN_KEYBOARD_WANTED.store(main_screen_keyboard, Ordering::SeqCst);
        MAIN_SCREEN_KEYBOARD_PUSHED.store(main_screen_keyboard, Ordering::SeqCst);

        #[cfg(feature = "image")]
        image_support::init_image_picker();

        // Both the keyboard-enhancement probe above and the image graphics query
        // send a `CSI c` sentinel. The keyboard probe consumes its reply unless it
        // arrives after timeout; the image probe may leave one unread. Drain either
        // case before the event loop starts.
        drain_pending_terminal_responses();

        let terminal = if policy.uses_alternate_screen {
            let backend = HostBackend::buffered(buffered_stdout());
            match ratatui::Terminal::new(backend) {
                Ok(terminal) => terminal,
                Err(err) => {
                    rollback_entered_terminal(policy, keyboard_enhancement);
                    panic_keyboard_enhancement.store(false, Ordering::SeqCst);
                    return Err(err);
                }
            }
        } else {
            let height = match surface_mode {
                SurfaceMode::InlineEphemeral { height }
                | SurfaceMode::InlineTranscript { height, .. } => height.initial_rows(),
                SurfaceMode::Fullscreen => 1,
            };
            match create_inline_terminal(height) {
                Ok(terminal) => terminal,
                Err(err) => {
                    rollback_entered_terminal(policy, keyboard_enhancement);
                    panic_keyboard_enhancement.store(false, Ordering::SeqCst);
                    return Err(err);
                }
            }
        };

        let guard = Self {
            stdout: io::stdout(),
            policy,
            keyboard_enhancement,
            modifier_key_reporting: false,
            theme_notifications: false,
        };
        Ok((OwnedTerminal::new(terminal), guard))
    }

    pub(crate) fn enable_theme_notifications(&mut self) -> io::Result<bool> {
        cfg_select! {
            unix => {
                if self.theme_notifications {
                    return Ok(true);
                }
                let plan = theme_notification_plan(true);
                let mut executor = CrosstermTransitionExecutor::new(&mut self.stdout);
                execute_plan_with_rollback(&mut executor, &plan)?;
                self.theme_notifications = true;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    pub(crate) fn set_modifier_key_reporting(&mut self, enabled: bool) -> io::Result<bool> {
        // Unix inline input goes through Termina's EventReader, which drops CSI-u associated
        // text. Keep its ordinary text mode until that reader can preprocess raw bytes too.
        #[cfg(unix)]
        if !self.policy.uses_alternate_screen {
            return Ok(false);
        }
        if !self.keyboard_enhancement || self.modifier_key_reporting == enabled {
            return Ok(false);
        }
        let plan = modifier_key_reporting_plan(enabled);
        let mut executor = CrosstermTransitionExecutor::new(&mut self.stdout);
        execute_plan_with_rollback(&mut executor, &plan)?;
        self.modifier_key_reporting = enabled;
        #[cfg(unix)]
        crate::app::input::kitty_text::set_enabled(enabled);
        Ok(true)
    }
}

fn rollback_entered_terminal(policy: SurfaceTerminalPolicy, keyboard_enhancement: bool) {
    // Drop any probe DA1 reply still queued before `exit_plan` disables raw mode,
    // so it is not echoed to the shell as `^[[?…c`.
    flush_pending_terminal_responses_on_exit();
    let mut stdout = io::stdout();
    let plan = exit_plan(policy, pop_keyboard_on_exit(policy, keyboard_enhancement));
    let mut executor = CrosstermTransitionExecutor::new(&mut stdout);
    let _ = execute_plan(&mut executor, &plan);
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        crate::app::input::kitty_text::set_enabled(false);
        #[cfg(unix)]
        super::terminal_handoff::pause_input_for_terminal_restore();
        let mut executor = CrosstermTransitionExecutor::new(&mut self.stdout);
        #[cfg(unix)]
        if self.theme_notifications {
            // Stop notifications and flush the command while raw mode is still
            // active, then discard any report that raced the Termina shutdown.
            let _ = execute_plan(&mut executor, &theme_notification_plan(false));
            self.theme_notifications = false;
        }
        flush_pending_terminal_responses_on_exit();
        let plan = exit_plan(
            self.policy,
            pop_keyboard_on_exit(self.policy, self.keyboard_enhancement),
        );
        let _ = execute_plan(&mut executor, &plan);
    }
}

pub(crate) fn restore_terminal_on_panic(
    surface_mode: SurfaceMode,
    keyboard_enhancement: bool,
    theme_notifications: bool,
) {
    #[cfg(unix)]
    super::terminal_handoff::pause_input_for_terminal_restore();
    let policy = surface_terminal_policy(surface_mode);
    let mut stdout = io::stdout();
    let mut executor = CrosstermTransitionExecutor::new(&mut stdout);
    #[cfg(unix)]
    if theme_notifications {
        let _ = execute_plan(&mut executor, &theme_notification_plan(false));
    }
    #[cfg(not(unix))]
    let _ = theme_notifications;
    flush_pending_terminal_responses_on_exit();
    let plan = exit_plan(policy, pop_keyboard_on_exit(policy, keyboard_enhancement));
    let _ = execute_plan(&mut executor, &plan);
    reset_handoff_state_for_terminal_restore();
}

#[cfg(test)]
pub(crate) fn assert_inline_surface_internal_wrap_policy_is_opaque() {
    use crate::app::context::{InlineHeight, InlineStartupPolicy, SurfaceMode};

    let fullscreen = surface_terminal_policy(SurfaceMode::Fullscreen);
    assert!(fullscreen.uses_alternate_screen);
    assert!(!fullscreen.disable_auto_wrap);

    let ephemeral = surface_terminal_policy(SurfaceMode::InlineEphemeral {
        height: InlineHeight::Fixed(3),
    });
    assert!(!ephemeral.uses_alternate_screen);
    assert!(ephemeral.disable_auto_wrap);

    let transcript = surface_terminal_policy(SurfaceMode::InlineTranscript {
        height: InlineHeight::Fixed(3),
        startup: InlineStartupPolicy::PreserveHost,
    });
    assert!(!transcript.uses_alternate_screen);
    assert!(!transcript.disable_auto_wrap);
}

#[cfg(test)]
mod mouse_motion_tests {
    use super::set_mouse_all_motion_enabled;

    #[test]
    fn changing_motion_tracking_preserves_mouse_coordinate_encoding() {
        let mut transcript = Vec::new();
        transcript.extend_from_slice(b"\x1b[?1016h");

        set_mouse_all_motion_enabled(&mut transcript, true).unwrap();
        set_mouse_all_motion_enabled(&mut transcript, false).unwrap();
        set_mouse_all_motion_enabled(&mut transcript, true).unwrap();

        assert_eq!(
            transcript,
            b"\x1b[?1016h\x1b[?1003h\x1b[?1003l\x1b[?1000h\x1b[?1002h\x1b[?1003h"
        );
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::backend::ratatui_backend::host_input::{
        HostEvent, InlineHostReader,
        pty_test::{child_report, run_with_text},
    };
    use crossterm::event::{Event, KeyCode};

    #[test]
    fn inline_modifier_request_preserves_shifted_and_composed_text() {
        if let Some(report) = child_report() {
            crossterm::terminal::enable_raw_mode().expect("raw PTY");
            let mut guard = TerminalGuard {
                stdout: io::stdout(),
                policy: surface_terminal_policy(SurfaceMode::InlineEphemeral { height: 3.into() }),
                keyboard_enhancement: true,
                modifier_key_reporting: false,
                theme_notifications: false,
            };
            assert!(
                !guard
                    .set_modifier_key_reporting(true)
                    .expect("request modifiers")
            );
            assert!(!guard.modifier_key_reporting);
            // The transcript surface shares the same input restriction.
            guard.policy = surface_terminal_policy(SurfaceMode::InlineTranscript {
                height: 3.into(),
                startup: crate::app::context::InlineStartupPolicy::PreserveHost,
            });
            assert!(
                !guard
                    .set_modifier_key_reporting(true)
                    .expect("request modifiers")
            );
            let reader = InlineHostReader::start().expect("inline reader");
            report.line("ready", 1);
            let mut text = String::new();
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while text.chars().count() < 2 && std::time::Instant::now() < deadline {
                if let HostEvent::Input(Event::Key(key)) =
                    reader.read(Duration::from_millis(50)).expect("inline key")
                    && let KeyCode::Char(ch) = key.code
                {
                    text.push(ch);
                }
            }
            report.line("text", text);
            return;
        }
        let module = module_path!().split_once("::").unwrap().1;
        let name = format!("{module}::inline_modifier_request_preserves_shifted_and_composed_text");
        let report = run_with_text(&name, "Ał");
        assert_eq!(report.get("text").map(String::as_str), Some("Ał"));
    }
}
