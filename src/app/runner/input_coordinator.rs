#![allow(unsafe_code)]

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossterm::event::{
    Event as CrosstermEvent, KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent,
    KeyEventKind as CrosstermKeyEventKind, KeyEventState as CrosstermKeyEventState,
    KeyModifiers as CrosstermKeyModifiers, MediaKeyCode as CrosstermMediaKeyCode,
    ModifierKeyCode as CrosstermModifierKeyCode, MouseButton as CrosstermMouseButton,
    MouseEvent as CrosstermMouseEvent, MouseEventKind as CrosstermMouseEventKind,
};
use termina::Parser;
use termina::escape::csi::{Csi, Mode};
use termina::event::{
    Event as TerminaEvent, KeyCode as TerminaKeyCode, KeyEvent as TerminaKeyEvent,
    KeyEventKind as TerminaKeyEventKind, KeyEventState as TerminaKeyEventState,
    MediaKeyCode as TerminaMediaKeyCode, ModifierKeyCode as TerminaModifierKeyCode,
    Modifiers as TerminaModifiers, MouseButton as TerminaMouseButton,
    MouseEvent as TerminaMouseEvent, MouseEventKind as TerminaMouseEventKind,
};

use crate::app::input::pixel_mouse::{self, PointerReport};
use crate::backend::ratatui_backend::host_input::is_hang_up;
use crate::backend::ratatui_backend::terminal_handoff::{
    InputHandoffControl, InputHandoffSlot, register_input_handoff_control,
    unregister_input_handoff_control,
};
use crate::style::HostTerminalColors;
use crate::style::terminal_colors::{HostColorResponseParser, build_live_color_query_batch};

use super::RunnerEvent;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const ESCAPE_DISAMBIGUATION: Duration = Duration::from_millis(10);
const HOST_COLOR_QUERY_TIMEOUT: Duration = Duration::from_millis(200);
/// How often the worker asks the host for its size. The answer only backs up resize reports that
/// never arrived, and asking after every read would cost a system call per pointer report.
const SIZE_CHECK_INTERVAL: Duration = Duration::from_millis(100);
/// How long a switch of the pointer encoding waits for the host to confirm it before the decoder
/// switches anyway. Only a host that ignores the mode query waits this long.
const PIXEL_SWITCH_TIMEOUT: Duration = Duration::from_millis(500);

pub(super) struct TerminaInputCoordinator {
    events: mpsc::Receiver<RunnerEvent>,
    /// Spare sender so other producers (the control channel) can wake this loop.
    sender: mpsc::Sender<RunnerEvent>,
    control: Arc<WorkerControl>,
    worker: Option<JoinHandle<()>>,
    panic_control: InputHandoffSlot,
}

impl TerminaInputCoordinator {
    pub(super) fn start(panic_control: InputHandoffSlot) -> io::Result<Self> {
        let terminal = open_terminal_input()?;
        let (wake_reader, wake_writer) = worker_wake_pipe()?;
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let spare_sender = event_tx.clone();
        let control = Arc::new(WorkerControl {
            commands: command_tx,
            wake: Mutex::new(wake_writer),
            paused: AtomicBool::new(false),
            worker_thread: Mutex::new(None),
        });
        let worker_control = Arc::clone(&control);
        let panic_events = event_tx.clone();
        let worker = std::thread::Builder::new()
            .name("termina-reader".into())
            .spawn(move || {
                contain_worker_panic(&panic_events, || {
                    run_worker(terminal, wake_reader, command_rx, event_tx, worker_control);
                });
            })?;

        let handoff_control: Arc<dyn InputHandoffControl + Send + Sync> = control.clone();
        let weak_control = Arc::downgrade(&handoff_control);
        register_input_handoff_control(weak_control.clone());
        if let Ok(mut slot) = panic_control.lock() {
            *slot = Some(weak_control);
        }
        Ok(Self {
            events,
            sender: spare_sender,
            control,
            worker: Some(worker),
            panic_control,
        })
    }

    pub(super) fn receiver(&self) -> &mpsc::Receiver<RunnerEvent> {
        &self.events
    }

    /// A sender into the channel this coordinator's receiver drains.
    pub(super) fn sender(&self) -> mpsc::Sender<RunnerEvent> {
        self.sender.clone()
    }

    pub(super) fn request_host_colors(
        &self,
        previous: Option<HostTerminalColors>,
    ) -> io::Result<()> {
        self.control
            .commands
            .send(WorkerCommand::RefreshHostColors(previous))
            .map_err(|_| worker_stopped())?;
        self.control.wake();
        Ok(())
    }

    /// Have the worker bring the host's pixel-reporting mode in line with whether anything wants
    /// it. The worker writes the escape because it is the one reading the reports it changes.
    #[cfg(feature = "terminal")]
    pub(super) fn sync_pixel_mouse(&self) -> io::Result<()> {
        self.control
            .commands
            .send(WorkerCommand::SyncPixelMouse)
            .map_err(|_| worker_stopped())?;
        self.control.wake();
        Ok(())
    }
}

impl Drop for TerminaInputCoordinator {
    fn drop(&mut self) {
        unregister_input_handoff_control();
        if let Ok(mut slot) = self.panic_control.lock() {
            *slot = None;
        }
        self.control.shutdown();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

enum WorkerCommand {
    Pause(mpsc::SyncSender<()>),
    Resume(mpsc::SyncSender<io::Result<()>>),
    RefreshHostColors(Option<HostTerminalColors>),
    #[cfg_attr(not(feature = "terminal"), allow(dead_code))]
    SyncPixelMouse,
    Shutdown,
}

struct WorkerControl {
    commands: mpsc::Sender<WorkerCommand>,
    wake: Mutex<UnixStream>,
    paused: AtomicBool,
    worker_thread: Mutex<Option<std::thread::ThreadId>>,
}

impl WorkerControl {
    fn wake(&self) {
        if let Ok(mut wake) = self.wake.lock() {
            let _ = wake.write(&[1]);
        }
    }

    fn pause(&self) -> io::Result<()> {
        if self
            .worker_thread
            .lock()
            .is_ok_and(|thread| thread.as_ref() == Some(&std::thread::current().id()))
        {
            return Ok(());
        }
        if self.paused.load(Ordering::SeqCst) {
            return Ok(());
        }
        let (ack_tx, ack_rx) = mpsc::sync_channel(0);
        self.commands
            .send(WorkerCommand::Pause(ack_tx))
            .map_err(|_| worker_stopped())?;
        self.wake();
        ack_rx.recv().map_err(|_| worker_stopped())
    }

    fn resume(&self) -> io::Result<()> {
        if !self.paused.load(Ordering::SeqCst) {
            return Ok(());
        }
        let (ack_tx, ack_rx) = mpsc::sync_channel(0);
        self.commands
            .send(WorkerCommand::Resume(ack_tx))
            .map_err(|_| worker_stopped())?;
        ack_rx.recv().map_err(|_| worker_stopped())?
    }

    fn shutdown(&self) {
        let _ = self.commands.send(WorkerCommand::Shutdown);
        self.wake();
    }
}

impl InputHandoffControl for WorkerControl {
    fn pause(&self) -> io::Result<()> {
        WorkerControl::pause(self)
    }

    fn resume(&self) -> io::Result<()> {
        WorkerControl::resume(self)
    }

    fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
}

fn worker_stopped() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "Termina input worker stopped")
}

fn contain_worker_panic(events: &mpsc::Sender<RunnerEvent>, worker: impl FnOnce()) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(worker)) {
        let message = if let Some(message) = payload.downcast_ref::<&str>() {
            *message
        } else if let Some(message) = payload.downcast_ref::<String>() {
            message.as_str()
        } else {
            "unknown panic payload"
        };
        let _ = events.send(RunnerEvent::InputError(format!(
            "Termina input worker panicked: {message}"
        )));
    }
}

fn worker_wake_pipe() -> io::Result<(UnixStream, UnixStream)> {
    let (reader, writer) = UnixStream::pair()?;
    reader.set_nonblocking(true)?;
    writer.set_nonblocking(true)?;
    Ok((reader, writer))
}

fn open_terminal_input() -> io::Result<File> {
    let terminal = OpenOptions::new().read(true).write(true).open("/dev/tty")?;
    if let Ok(size) = window_size(&terminal) {
        pixel_mouse::note_window_size(size.columns, size.rows, Some(size.width), Some(size.height));
    }
    Ok(terminal)
}

fn run_worker(
    mut terminal: File,
    mut wake_reader: UnixStream,
    commands: mpsc::Receiver<WorkerCommand>,
    events: mpsc::Sender<RunnerEvent>,
    control: Arc<WorkerControl>,
) {
    if let Ok(mut worker_thread) = control.worker_thread.lock() {
        *worker_thread = Some(std::thread::current().id());
    }
    // Every way out of the loop that is not a shutdown or a closed channel is one of these. A terminal
    // that has gone away ends reading without failing the run: the app may still be tidying up after
    // the `SIGHUP` that came with it, and the runner decides how long to wait for that.
    let stop = |err: io::Error| {
        let event = if is_hang_up(&err) {
            RunnerEvent::HostHungUp
        } else {
            RunnerEvent::InputError(err.to_string())
        };
        let _ = events.send(event);
    };
    let mut input = WorkerInput::default();
    // Asked for here rather than in the enter plan: the host's own answer arrived before this, but
    // the cell size a pixel report is divided by is only known once the terminal was opened.
    input.sync_pixel_mouse(&mut io::stdout());
    let mut query = None;
    let mut settle_at = None;
    let mut size = WindowSizeWatch::new(&terminal);
    loop {
        input.pixel.expire(Instant::now());
        match process_worker_commands(
            &mut terminal,
            &commands,
            &control,
            &events,
            &mut input,
            &mut query,
        ) {
            Ok(true) => {}
            Ok(false) => break,
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => {
                stop(err);
                break;
            }
        }

        match poll_terminal_input(&terminal, &wake_reader, Duration::ZERO) {
            Ok(ready) if ready.wake => {
                if let Err(err) = drain_worker_wake(&mut wake_reader) {
                    stop(err);
                    break;
                }
                continue;
            }
            Ok(ready) if ready.terminal => {
                match read_terminal_bytes(&mut terminal, &mut input, &events) {
                    Ok(true) => {
                        settle_at = Some(Instant::now() + ESCAPE_DISAMBIGUATION);
                    }
                    Ok(false) => break,
                    Err(err) => {
                        stop(err);
                        break;
                    }
                }
                if !size.refresh(&terminal, &events, &mut input) {
                    break;
                }
                continue;
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(err) => {
                stop(err);
                break;
            }
        }

        if let Err(err) =
            start_color_query_if_ready(&mut terminal, &mut input, &mut query, settle_at.is_none())
        {
            stop(err);
            break;
        }
        if finish_color_query_if_ready(&events, &mut input, &mut query) {
            continue;
        }
        match poll_terminal_input(
            &terminal,
            &wake_reader,
            next_poll_timeout(
                settle_at,
                query.as_ref(),
                [input.pixel.deadline, Some(size.next_check)],
            ),
        ) {
            Ok(ready) if ready.wake => {
                if let Err(err) = drain_worker_wake(&mut wake_reader) {
                    stop(err);
                    break;
                }
                continue;
            }
            Ok(ready) if ready.terminal => {
                match read_terminal_bytes(&mut terminal, &mut input, &events) {
                    Ok(true) => {
                        settle_at = Some(Instant::now() + ESCAPE_DISAMBIGUATION);
                    }
                    Ok(false) => break,
                    Err(err) => {
                        stop(err);
                        break;
                    }
                }
            }
            Ok(_) => {
                if settle_at.is_some_and(|deadline| Instant::now() >= deadline) {
                    if !input.settle(&events) {
                        break;
                    }
                    settle_at = None;
                }
                finish_color_query_if_ready(&events, &mut input, &mut query);
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => {
                stop(err);
                break;
            }
        }
        if !size.refresh(&terminal, &events, &mut input) {
            break;
        }
    }
}

fn process_worker_commands(
    terminal: &mut File,
    commands: &mpsc::Receiver<WorkerCommand>,
    control: &WorkerControl,
    events: &mpsc::Sender<RunnerEvent>,
    input: &mut WorkerInput,
    query: &mut Option<ActiveColorQuery>,
) -> io::Result<bool> {
    while let Ok(command) = commands.try_recv() {
        match command {
            WorkerCommand::Pause(ack) => {
                control.paused.store(true, Ordering::SeqCst);
                let _ = ack.send(());
                loop {
                    match commands.recv().map_err(|_| worker_stopped())? {
                        WorkerCommand::Resume(ack) => match open_terminal_input() {
                            Ok(new_terminal) => {
                                *terminal = new_terminal;
                                *input = WorkerInput::default();
                                // The suspend turned pixel reporting off. The host is the same one
                                // that answered at startup, so it is asked again straight away.
                                input.sync_pixel_mouse(&mut io::stdout());
                                *query = None;
                                control.paused.store(false, Ordering::SeqCst);
                                let _ = events.send(RunnerEvent::HostTerminalColorRefreshRequested);
                                let _ = ack.send(Ok(()));
                                break;
                            }
                            Err(err) => {
                                let message = err.to_string();
                                let kind = err.kind();
                                let _ = ack.send(Err(err));
                                return Err(io::Error::new(kind, message));
                            }
                        },
                        // Resuming syncs the mode itself.
                        WorkerCommand::RefreshHostColors(_) | WorkerCommand::SyncPixelMouse => {}
                        WorkerCommand::Shutdown => return Ok(false),
                        WorkerCommand::Pause(ack) => {
                            let _ = ack.send(());
                        }
                    }
                }
            }
            WorkerCommand::Resume(ack) => {
                let _ = ack.send(Ok(()));
            }
            WorkerCommand::RefreshHostColors(previous) => {
                queue_color_refresh(query, previous);
            }
            WorkerCommand::SyncPixelMouse => input.sync_pixel_mouse(&mut io::stdout()),
            WorkerCommand::Shutdown => return Ok(false),
        }
    }
    Ok(true)
}

#[derive(Default)]
struct WorkerInput {
    // Both decoders observe every byte read from the TTY for the worker's full lifetime. The color
    // scanner never takes ownership of a suffix that Termina's parser expects to finish later.
    parser: Parser,
    text: crate::app::input::kitty_text::KittyText,
    colors: HostColorResponseParser,
    /// The newest plain motion report of the read being dispatched. See [`WorkerInput::send`].
    pending_motion: Option<RunnerEvent>,
    pixel: PixelSwitch,
}

fn is_plain_motion(event: &RunnerEvent) -> bool {
    let (RunnerEvent::Terminal(CrosstermEvent::Mouse(mouse))
    | RunnerEvent::Pointer {
        event: CrosstermEvent::Mouse(mouse),
        ..
    }) = event
    else {
        return false;
    };
    mouse.kind == CrosstermMouseEventKind::Moved
}

impl WorkerInput {
    fn push(&mut self, bytes: &[u8], events: &mpsc::Sender<RunnerEvent>) -> bool {
        // A switch of the pointer encoding takes effect exactly at the host's answer to the query
        // behind it, so the bytes before the answer are decoded, and their reports sent, first.
        let mut rest = bytes;
        while let Some((end, setting)) = self.pixel.reply.find(rest) {
            if !self.decode(&rest[..end], events) {
                return false;
            }
            self.pixel.confirm(setting);
            rest = &rest[end..];
        }
        self.decode(rest, events)
    }

    fn decode(&mut self, bytes: &[u8], events: &mpsc::Sender<RunnerEvent>) -> bool {
        self.colors.push(bytes);
        self.parser.parse(&self.text.push(bytes), true);
        self.dispatch(events)
    }

    fn settle(&mut self, events: &mpsc::Sender<RunnerEvent>) -> bool {
        self.parser.parse(&self.text.settle(), false);
        self.colors.settle_input();
        self.dispatch(events)
    }

    fn dispatch(&mut self, events: &mpsc::Sender<RunnerEvent>) -> bool {
        let mut resized = false;
        while let Some(event) = self.parser.pop() {
            let Some(event) = termina_runner_event(event) else {
                continue;
            };
            resized |= matches!(event, RunnerEvent::Terminal(CrosstermEvent::Resize(..)));
            if !self.send(event, events) {
                return false;
            }
        }
        // A resize is where a padded window can first divide evenly, and so where the mode can
        // first be worth asking for.
        if resized {
            self.sync_pixel_mouse(&mut io::stdout());
        }
        self.flush_motion(events)
    }

    /// Bring the host's SGR-pixels mode in line with what this run can make sense of and has a
    /// use for.
    ///
    /// Both halves of `pixel_mouse::is_active` can turn true after startup, because the cell size
    /// is re-derived from every resize, and a program can ask for pixels at any time. So the mode
    /// is synced whenever either may have changed, not only at startup.
    ///
    /// Only the Termina worker calls this: it is the one decoder that can read a pixel report, and
    /// the switch only takes effect once its input reaches the host's answer. See [`PixelSwitch`].
    fn sync_pixel_mouse(&mut self, out: &mut impl Write) {
        let wanted = pixel_mouse::should_request();
        if wanted == self.pixel.requested() {
            return;
        }
        let switch: &[u8] = if wanted {
            b"\x1b[?1016h\x1b[?1016$p"
        } else {
            b"\x1b[?1016l\x1b[?1016$p"
        };
        // A host that never heard the escape keeps sending what it sent, and is read that way.
        if out.write_all(switch).and_then(|()| out.flush()).is_ok() {
            self.pixel.request(wanted);
        }
    }

    /// Send one event to the runner, keeping only the newest plain motion of a read.
    ///
    /// A host can hand over a whole frame's worth of pointer motion in one read - every pixel the
    /// pointer crossed. Each report sent wakes the UI thread for a pass of its loop, and the runner
    /// only ever wants where the pointer is now, so the reports behind the newest one are dropped
    /// here instead. Nothing is held across reads: the newest position goes out with the read that
    /// brought it, so coalescing never makes a hover arrive later.
    ///
    /// Only motion with no button held is merged. A press, a release, a drag, a wheel notch or a
    /// key each mean something on their own, so they go in order - behind the motion that came
    /// before them, so the runner still learns where the pointer was first.
    fn send(&mut self, event: RunnerEvent, events: &mpsc::Sender<RunnerEvent>) -> bool {
        if is_plain_motion(&event) {
            self.pending_motion = Some(event);
            return true;
        }
        self.flush_motion(events) && events.send(event).is_ok()
    }

    fn flush_motion(&mut self, events: &mpsc::Sender<RunnerEvent>) -> bool {
        self.pending_motion
            .take()
            .is_none_or(|motion| events.send(motion).is_ok())
    }
}

/// Switches of the host's pointer encoding (DEC 1016) that the host has not confirmed yet.
///
/// The mode changes what the coordinates in a report mean without changing its shape, and the host
/// goes on reporting the old way until it reads the switch. Reports already queued on the tty, or
/// sent while the escape was on its way, still carry the old coordinates when they arrive, and
/// reading them the new way would put a click on the wrong cell. So the decoder does not change when
/// the escape is written. A query of the mode follows every switch, and the host answers only after
/// acting on it: reports before the answer are read the old way, reports after it the new way.
#[derive(Default)]
struct PixelSwitch {
    /// Encodings asked for and not yet confirmed, oldest first. `true` is pixels.
    pending: VecDeque<bool>,
    /// When to stop waiting for the oldest confirmation and take the newest switch as done.
    deadline: Option<Instant>,
    reply: ModeReplyScanner,
}

impl PixelSwitch {
    /// What the host will be sending once every switch written so far has taken effect.
    fn requested(&self) -> bool {
        self.pending
            .back()
            .copied()
            .unwrap_or_else(pixel_mouse::reports_pixels)
    }

    fn request(&mut self, pixels: bool) {
        self.pending.push_back(pixels);
        self.deadline
            .get_or_insert_with(|| Instant::now() + PIXEL_SWITCH_TIMEOUT);
    }

    /// The host answered the oldest query: from here on, reports carry what it says.
    fn confirm(&mut self, setting: u16) {
        // An answer nobody here asked for, such as a late reply to a startup probe, changes nothing.
        if self.pending.pop_front().is_none() {
            return;
        }
        // The host's word decides rather than the request, so one that refused the mode stays read
        // as cells. 1 and 3 are set and permanently set.
        pixel_mouse::note_mode_enabled(matches!(setting, 1 | 3));
        self.deadline = (!self.pending.is_empty()).then(|| Instant::now() + PIXEL_SWITCH_TIMEOUT);
    }

    /// Give up on a host that does not answer the query, and read reports as last asked for.
    fn expire(&mut self, now: Instant) {
        if self.deadline.is_none_or(|deadline| now < deadline) {
            return;
        }
        if let Some(&pixels) = self.pending.back() {
            pixel_mouse::note_mode_enabled(pixels);
        }
        self.pending.clear();
        self.deadline = None;
    }
}

/// Finds the host's answer to `CSI ? 1016 $ p` (`CSI ? 1016 ; Ps $ y`) in raw input, across reads.
///
/// Termina parses mode reports only for the modes it knows, and drops this one, so it is found in
/// the bytes before they are parsed. That also gives its exact place among the reports around it.
#[derive(Default)]
struct ModeReplyScanner {
    /// How much of the answer has matched so far.
    matched: usize,
    setting: u16,
}

impl ModeReplyScanner {
    const PREFIX: &[u8] = b"\x1b[?1016;";

    /// The offset just past the first complete answer in `bytes`, and the setting it reports.
    fn find(&mut self, bytes: &[u8]) -> Option<(usize, u16)> {
        bytes
            .iter()
            .enumerate()
            .find_map(|(index, &byte)| self.step(byte).map(|setting| (index + 1, setting)))
    }

    fn step(&mut self, byte: u8) -> Option<u16> {
        let prefix = Self::PREFIX.len();
        if self.matched < prefix {
            if byte == Self::PREFIX[self.matched] {
                self.matched += 1;
                self.setting = 0;
            } else {
                self.restart(byte);
            }
            return None;
        }
        match (self.matched - prefix, byte) {
            (0 | 1, b'0'..=b'9') => {
                self.setting = self
                    .setting
                    .saturating_mul(10)
                    .saturating_add(u16::from(byte - b'0'));
                self.matched = prefix + 1;
            }
            (1, b'$') => self.matched += 1,
            (2, b'y') => {
                self.matched = 0;
                return Some(self.setting);
            }
            _ => self.restart(byte),
        }
        None
    }

    /// Anything unexpected ends the match, though it may itself begin the next one.
    fn restart(&mut self, byte: u8) {
        self.matched = usize::from(byte == Self::PREFIX[0]);
    }
}

/// Read what `poll` reported ready, without ever blocking.
///
/// A read is the usual way the app finds out it has lost the terminal: it
/// raises `SIGTTIN`, and the process stops until the job is foregrounded. The
/// read is then restarted, but the shell has typically consumed the bytes that
/// were waiting in the meantime, and a blocking read would sit there until the
/// next keypress, with the runner waiting on this worker to take the terminal
/// back. Reading non-blocking turns that into an empty read the loop moves past.
fn read_terminal_bytes(
    terminal: &mut (impl Read + AsRawFd),
    input: &mut WorkerInput,
    events: &mpsc::Sender<RunnerEvent>,
) -> io::Result<bool> {
    let mut bytes = [0; 1024];
    loop {
        match read_nonblocking(terminal, &mut bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "terminal input reached end-of-file",
                ));
            }
            Ok(read) => return Ok(input.push(&bytes[..read], events)),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(true),
            Err(err) => return Err(err),
        }
    }
}

/// One `read` with `O_NONBLOCK` set for its duration only. The worker opened
/// this description itself, so the flag reaches nobody else, and the worker's
/// own writes stay blocking.
fn read_nonblocking(terminal: &mut (impl Read + AsRawFd), bytes: &mut [u8]) -> io::Result<usize> {
    let fd = terminal.as_raw_fd();
    // SAFETY: `fcntl` with `F_GETFL`/`F_SETFL` takes no pointers.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || flags & libc::O_NONBLOCK != 0 {
        return terminal.read(bytes);
    }
    // SAFETY: as above.
    if unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return terminal.read(bytes);
    }
    let read = terminal.read(bytes);
    // SAFETY: as above.
    unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
    read
}

struct ActiveColorQuery {
    deadline: Option<Instant>,
    previous: Option<HostTerminalColors>,
    refresh_pending: bool,
}

fn queue_color_refresh(query: &mut Option<ActiveColorQuery>, previous: Option<HostTerminalColors>) {
    match query {
        Some(active) if active.deadline.is_some() => active.refresh_pending = true,
        Some(active) => active.previous = previous,
        None => {
            *query = Some(ActiveColorQuery {
                deadline: None,
                previous,
                refresh_pending: false,
            });
        }
    }
}

fn start_color_query_if_ready(
    terminal: &mut impl Write,
    input: &mut WorkerInput,
    query: &mut Option<ActiveColorQuery>,
    input_is_quiet: bool,
) -> io::Result<()> {
    let Some(active) = query.as_mut() else {
        return Ok(());
    };
    if active.deadline.is_some() || !input_is_quiet || !input.colors.at_input_boundary() {
        return Ok(());
    }
    terminal.write_all(&build_live_color_query_batch())?;
    terminal.flush()?;
    input.colors.start_query();
    active.deadline = Some(Instant::now() + HOST_COLOR_QUERY_TIMEOUT);
    Ok(())
}

fn finish_color_query_if_ready(
    events: &mpsc::Sender<RunnerEvent>,
    input: &mut WorkerInput,
    query: &mut Option<ActiveColorQuery>,
) -> bool {
    let Some(active) = query.as_ref() else {
        return false;
    };
    let Some(deadline) = active.deadline else {
        return false;
    };
    if !input.colors.query_complete() && Instant::now() < deadline {
        return false;
    }
    let Some(active) = query.take() else {
        return false;
    };
    if let Some(colors) = input.colors.finish_query(active.previous.as_ref()) {
        let _ = events.send(RunnerEvent::HostTerminalColors(colors));
    }
    if active.refresh_pending {
        let _ = events.send(RunnerEvent::HostTerminalColorRefreshRequested);
    }
    true
}

fn next_poll_timeout(
    settle_at: Option<Instant>,
    query: Option<&ActiveColorQuery>,
    other: [Option<Instant>; 2],
) -> Duration {
    let now = Instant::now();
    [settle_at, query.and_then(|query| query.deadline)]
        .into_iter()
        .chain(other)
        .flatten()
        .map(|deadline| deadline.saturating_duration_since(now))
        .fold(INPUT_POLL_INTERVAL, Duration::min)
}

/// The host's size, asked of the terminal this worker already holds.
///
/// `crossterm::terminal::window_size` answers the same question by opening `/dev/tty`, asking, and
/// closing it again, which cost more than everything else a read of pointer motion did.
fn window_size(terminal: &impl AsRawFd) -> io::Result<crossterm::terminal::WindowSize> {
    // SAFETY: `winsize` is plain integers, for which all-zero is a valid value.
    let mut size: libc::winsize = unsafe { std::mem::zeroed() };
    // The request's integer type differs between libcs, hence `.into()`.
    #[allow(clippy::useless_conversion)]
    // SAFETY: `TIOCGWINSZ` writes one `winsize` through the pointer, which outlives the call.
    if unsafe { libc::ioctl(terminal.as_raw_fd(), libc::TIOCGWINSZ.into(), &mut size) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(crossterm::terminal::WindowSize {
        rows: size.ws_row,
        columns: size.ws_col,
        width: size.ws_xpixel,
        height: size.ws_ypixel,
    })
}

/// The host size as last seen, checked on a cadence of its own.
///
/// Nothing tells the worker the host was resized - termina reports a resize only when the host
/// sends one in-band - so it asks. Asking after every read would cost a system call per pointer
/// report, and an idle worker wakes this often anyway, so a busy one keeps the same pace.
struct WindowSizeWatch {
    last: Option<crossterm::terminal::WindowSize>,
    next_check: Instant,
}

impl WindowSizeWatch {
    fn new(terminal: &impl AsRawFd) -> Self {
        Self {
            last: window_size(terminal).ok(),
            next_check: Instant::now() + SIZE_CHECK_INTERVAL,
        }
    }

    /// Send a resize if the host size changed since the last check. False when the runner is gone.
    fn refresh(
        &mut self,
        terminal: &impl AsRawFd,
        events: &mpsc::Sender<RunnerEvent>,
        input: &mut WorkerInput,
    ) -> bool {
        let now = Instant::now();
        if now < self.next_check {
            return true;
        }
        self.next_check = now + SIZE_CHECK_INTERVAL;
        let Ok(size) = window_size(terminal) else {
            return true;
        };
        let changed = self.last.as_ref().is_none_or(|old| {
            (old.columns, old.rows, old.width, old.height)
                != (size.columns, size.rows, size.width, size.height)
        });
        if !changed {
            return true;
        }
        let (columns, rows, width, height) = (size.columns, size.rows, size.width, size.height);
        self.last = Some(size);
        pixel_mouse::note_window_size(columns, rows, Some(width), Some(height));
        input.sync_pixel_mouse(&mut io::stdout());
        events
            .send(RunnerEvent::Terminal(CrosstermEvent::Resize(columns, rows)))
            .is_ok()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct WorkerReady {
    terminal: bool,
    wake: bool,
}

fn drain_worker_wake(wake_reader: &mut UnixStream) -> io::Result<()> {
    let mut buffer = [0; 64];
    loop {
        match wake_reader.read(&mut buffer) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "input worker wake channel closed",
                ));
            }
            Ok(_) => {}
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => return Ok(()),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn poll_terminal_input(
    terminal: &impl AsRawFd,
    wake_reader: &impl AsRawFd,
    timeout: Duration,
) -> io::Result<WorkerReady> {
    let mut descriptors = [
        libc::pollfd {
            fd: terminal.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wake_reader.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    // Rounded up: `poll` counts whole milliseconds, and truncating a wait for a deadline less than
    // one away turns it into a busy loop until the deadline passes.
    let timeout = timeout.as_nanos().div_ceil(1_000_000).min(i32::MAX as u128) as i32;
    let result = unsafe { libc::poll(descriptors.as_mut_ptr(), descriptors.len() as _, timeout) };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    if descriptors
        .iter()
        .any(|fd| fd.revents & libc::POLLNVAL != 0)
    {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "input worker polling failed",
        ));
    }
    let ready = |descriptor: &libc::pollfd| {
        result > 0 && descriptor.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0
    };
    Ok(WorkerReady {
        terminal: ready(&descriptors[0]),
        wake: ready(&descriptors[1]),
    })
}

#[cfg(target_os = "macos")]
fn poll_terminal_input(
    terminal: &impl AsRawFd,
    wake_reader: &impl AsRawFd,
    timeout: Duration,
) -> io::Result<WorkerReady> {
    let terminal_fd = terminal.as_raw_fd();
    let wake_fd = wake_reader.as_raw_fd();
    let mut read_fds = unsafe { std::mem::zeroed::<libc::fd_set>() };
    unsafe {
        libc::FD_ZERO(&mut read_fds);
        libc::FD_SET(terminal_fd, &mut read_fds);
        libc::FD_SET(wake_fd, &mut read_fds);
    }
    let mut timeout = libc::timeval {
        tv_sec: timeout.as_secs().min(libc::time_t::MAX as u64) as libc::time_t,
        tv_usec: timeout.subsec_micros() as libc::suseconds_t,
    };
    let result = unsafe {
        libc::select(
            terminal_fd.max(wake_fd) + 1,
            &mut read_fds,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            &mut timeout,
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(WorkerReady {
        terminal: result > 0 && unsafe { libc::FD_ISSET(terminal_fd, &read_fds) },
        wake: result > 0 && unsafe { libc::FD_ISSET(wake_fd, &read_fds) },
    })
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum TerminaEventAction {
    Input(CrosstermEvent),
    /// A pointer position the host reported in pixels, split into cell and sub-cell parts.
    Pointer(CrosstermEvent, (u16, u16)),
    ThemeRefresh,
    Ignore,
}

/// What the runner should hear about a parsed host event, if anything.
fn termina_runner_event(event: TerminaEvent) -> Option<RunnerEvent> {
    match map_termina_event(event) {
        TerminaEventAction::Input(event) => Some(RunnerEvent::Terminal(event)),
        TerminaEventAction::Pointer(event, sub_cell) => {
            Some(RunnerEvent::Pointer { event, sub_cell })
        }
        TerminaEventAction::ThemeRefresh => Some(RunnerEvent::HostTerminalColorRefreshRequested),
        TerminaEventAction::Ignore => None,
    }
}

pub(crate) fn map_termina_event(event: TerminaEvent) -> TerminaEventAction {
    match event {
        TerminaEvent::Key(key) => {
            TerminaEventAction::Input(CrosstermEvent::Key(map_key_event(key)))
        }
        TerminaEvent::Mouse(mouse) => map_pointer_event(mouse),
        TerminaEvent::WindowResized(size) => {
            // The pixel dimensions ride along with every resize, which is what keeps the cell size
            // current through a font zoom rather than only at startup.
            pixel_mouse::note_window_size(
                size.cols,
                size.rows,
                size.pixel_width,
                size.pixel_height,
            );
            TerminaEventAction::Input(CrosstermEvent::Resize(size.cols, size.rows))
        }
        TerminaEvent::FocusIn => TerminaEventAction::Input(CrosstermEvent::FocusGained),
        TerminaEvent::FocusOut => TerminaEventAction::Input(CrosstermEvent::FocusLost),
        TerminaEvent::Paste(text) => TerminaEventAction::Input(CrosstermEvent::Paste(text)),
        TerminaEvent::Csi(Csi::Mode(Mode::ReportTheme(_))) => TerminaEventAction::ThemeRefresh,
        TerminaEvent::Csi(_) | TerminaEvent::Osc(_) | TerminaEvent::Dcs(_) => {
            TerminaEventAction::Ignore
        }
    }
}

/// A pointer report, in cells or in pixels depending on what the host was asked for.
///
/// Mode 1016 puts pixels on the wire in the same shape 1006 puts cells, and termina's input parser
/// does not distinguish the two - it subtracts the protocol's one-based origin and hands back
/// `column`/`row` either way. So when the mode is on, those fields hold zero-based *pixels*, and the
/// split has to happen here, before anything downstream can mistake one for the other.
/// (`MouseReport::Sgr1016` models the sequence for emitting, and never arrives as input.)
fn map_pointer_event(mouse: TerminaMouseEvent) -> TerminaEventAction {
    let mut event = map_mouse_event(mouse);
    match pixel_mouse::read_report(mouse.column, mouse.row) {
        PointerReport::Cells => TerminaEventAction::Input(CrosstermEvent::Mouse(event)),
        PointerReport::Pixels(position) => {
            event.column = position.cell.0;
            event.row = position.cell.1;
            TerminaEventAction::Pointer(CrosstermEvent::Mouse(event), position.sub_cell)
        }
    }
}

fn map_key_event(key: TerminaKeyEvent) -> CrosstermKeyEvent {
    let mut state = CrosstermKeyEventState::NONE;
    if key.state.contains(TerminaKeyEventState::KEYPAD) {
        state.insert(CrosstermKeyEventState::KEYPAD);
    }
    if key.state.contains(TerminaKeyEventState::CAPS_LOCK)
        || key.modifiers.contains(TerminaModifiers::CAPS_LOCK)
    {
        state.insert(CrosstermKeyEventState::CAPS_LOCK);
    }
    if key.state.contains(TerminaKeyEventState::NUM_LOCK)
        || key.modifiers.contains(TerminaModifiers::NUM_LOCK)
    {
        state.insert(CrosstermKeyEventState::NUM_LOCK);
    }

    CrosstermKeyEvent::new_with_kind_and_state(
        map_reported_key_code(key.code, key.kind, key.modifiers),
        map_modifiers(key.modifiers),
        match key.kind {
            TerminaKeyEventKind::Press => CrosstermKeyEventKind::Press,
            TerminaKeyEventKind::Release => CrosstermKeyEventKind::Release,
            TerminaKeyEventKind::Repeat => CrosstermKeyEventKind::Repeat,
        },
        state,
    )
}

fn map_reported_key_code(
    code: TerminaKeyCode,
    kind: TerminaKeyEventKind,
    modifiers: TerminaModifiers,
) -> CrosstermKeyCode {
    // Ghostty/Wayland can pair a LeftShift press (57441) with a CapsLock-coded release (57358)
    // while the release's modifier mask still contains Shift. Treat that exact release shape as
    // Shift; a real CapsLock release without Shift remains CapsLock.
    if matches!(kind, TerminaKeyEventKind::Release)
        && matches!(code, TerminaKeyCode::CapsLock)
        && modifiers.contains(TerminaModifiers::SHIFT)
    {
        return CrosstermKeyCode::Modifier(CrosstermModifierKeyCode::LeftShift);
    }
    // Some Kitty-protocol terminals append `:0` as the shifted alternate for a Shift release.
    // Termina 0.4 treats that zero as U+0000 and replaces the already-decoded Shift key with it,
    // so the release would never reach modifier-state tracking. REPORT_ALL_KEYS makes ordinary
    // modified text report its physical non-NUL key, which keeps this release-only repair narrow.
    if matches!(kind, TerminaKeyEventKind::Release) && matches!(code, TerminaKeyCode::Char('\0')) {
        return CrosstermKeyCode::Modifier(CrosstermModifierKeyCode::LeftShift);
    }
    map_key_code(code)
}

fn map_key_code(code: TerminaKeyCode) -> CrosstermKeyCode {
    match code {
        TerminaKeyCode::Char(ch) => CrosstermKeyCode::Char(ch),
        TerminaKeyCode::Enter => CrosstermKeyCode::Enter,
        TerminaKeyCode::Backspace => CrosstermKeyCode::Backspace,
        TerminaKeyCode::Tab => CrosstermKeyCode::Tab,
        TerminaKeyCode::Escape => CrosstermKeyCode::Esc,
        TerminaKeyCode::Left => CrosstermKeyCode::Left,
        TerminaKeyCode::Right => CrosstermKeyCode::Right,
        TerminaKeyCode::Up => CrosstermKeyCode::Up,
        TerminaKeyCode::Down => CrosstermKeyCode::Down,
        TerminaKeyCode::Home => CrosstermKeyCode::Home,
        TerminaKeyCode::End => CrosstermKeyCode::End,
        TerminaKeyCode::BackTab => CrosstermKeyCode::BackTab,
        TerminaKeyCode::PageUp => CrosstermKeyCode::PageUp,
        TerminaKeyCode::PageDown => CrosstermKeyCode::PageDown,
        TerminaKeyCode::Insert => CrosstermKeyCode::Insert,
        TerminaKeyCode::Delete => CrosstermKeyCode::Delete,
        TerminaKeyCode::KeypadBegin => CrosstermKeyCode::KeypadBegin,
        TerminaKeyCode::CapsLock => CrosstermKeyCode::CapsLock,
        TerminaKeyCode::ScrollLock => CrosstermKeyCode::ScrollLock,
        TerminaKeyCode::NumLock => CrosstermKeyCode::NumLock,
        TerminaKeyCode::PrintScreen => CrosstermKeyCode::PrintScreen,
        TerminaKeyCode::Pause => CrosstermKeyCode::Pause,
        TerminaKeyCode::Menu => CrosstermKeyCode::Menu,
        TerminaKeyCode::Null => CrosstermKeyCode::Null,
        TerminaKeyCode::Function(number) => CrosstermKeyCode::F(number),
        TerminaKeyCode::Modifier(modifier) => {
            CrosstermKeyCode::Modifier(map_modifier_key(modifier))
        }
        TerminaKeyCode::Media(media) => CrosstermKeyCode::Media(map_media_key(media)),
    }
}

fn map_modifiers(modifiers: TerminaModifiers) -> CrosstermKeyModifiers {
    let mut mapped = CrosstermKeyModifiers::NONE;
    if modifiers.contains(TerminaModifiers::SHIFT) {
        mapped.insert(CrosstermKeyModifiers::SHIFT);
    }
    if modifiers.contains(TerminaModifiers::CONTROL) {
        mapped.insert(CrosstermKeyModifiers::CONTROL);
    }
    if modifiers.contains(TerminaModifiers::ALT) {
        mapped.insert(CrosstermKeyModifiers::ALT);
    }
    if modifiers.contains(TerminaModifiers::SUPER) {
        mapped.insert(CrosstermKeyModifiers::SUPER);
    }
    if modifiers.contains(TerminaModifiers::HYPER) {
        mapped.insert(CrosstermKeyModifiers::HYPER);
    }
    if modifiers.contains(TerminaModifiers::META) {
        mapped.insert(CrosstermKeyModifiers::META);
    }
    mapped
}

fn map_modifier_key(modifier: TerminaModifierKeyCode) -> CrosstermModifierKeyCode {
    match modifier {
        TerminaModifierKeyCode::LeftShift => CrosstermModifierKeyCode::LeftShift,
        TerminaModifierKeyCode::LeftControl => CrosstermModifierKeyCode::LeftControl,
        TerminaModifierKeyCode::LeftAlt => CrosstermModifierKeyCode::LeftAlt,
        TerminaModifierKeyCode::LeftSuper => CrosstermModifierKeyCode::LeftSuper,
        TerminaModifierKeyCode::LeftHyper => CrosstermModifierKeyCode::LeftHyper,
        TerminaModifierKeyCode::LeftMeta => CrosstermModifierKeyCode::LeftMeta,
        TerminaModifierKeyCode::RightShift => CrosstermModifierKeyCode::RightShift,
        TerminaModifierKeyCode::RightControl => CrosstermModifierKeyCode::RightControl,
        TerminaModifierKeyCode::RightAlt => CrosstermModifierKeyCode::RightAlt,
        TerminaModifierKeyCode::RightSuper => CrosstermModifierKeyCode::RightSuper,
        TerminaModifierKeyCode::RightHyper => CrosstermModifierKeyCode::RightHyper,
        TerminaModifierKeyCode::RightMeta => CrosstermModifierKeyCode::RightMeta,
        TerminaModifierKeyCode::IsoLevel3Shift => CrosstermModifierKeyCode::IsoLevel3Shift,
        TerminaModifierKeyCode::IsoLevel5Shift => CrosstermModifierKeyCode::IsoLevel5Shift,
    }
}

fn map_media_key(media: TerminaMediaKeyCode) -> CrosstermMediaKeyCode {
    match media {
        TerminaMediaKeyCode::Play => CrosstermMediaKeyCode::Play,
        TerminaMediaKeyCode::Pause => CrosstermMediaKeyCode::Pause,
        TerminaMediaKeyCode::PlayPause => CrosstermMediaKeyCode::PlayPause,
        TerminaMediaKeyCode::Reverse => CrosstermMediaKeyCode::Reverse,
        TerminaMediaKeyCode::Stop => CrosstermMediaKeyCode::Stop,
        TerminaMediaKeyCode::FastForward => CrosstermMediaKeyCode::FastForward,
        TerminaMediaKeyCode::Rewind => CrosstermMediaKeyCode::Rewind,
        TerminaMediaKeyCode::TrackNext => CrosstermMediaKeyCode::TrackNext,
        TerminaMediaKeyCode::TrackPrevious => CrosstermMediaKeyCode::TrackPrevious,
        TerminaMediaKeyCode::Record => CrosstermMediaKeyCode::Record,
        TerminaMediaKeyCode::LowerVolume => CrosstermMediaKeyCode::LowerVolume,
        TerminaMediaKeyCode::RaiseVolume => CrosstermMediaKeyCode::RaiseVolume,
        TerminaMediaKeyCode::MuteVolume => CrosstermMediaKeyCode::MuteVolume,
    }
}

fn map_mouse_event(mouse: TerminaMouseEvent) -> CrosstermMouseEvent {
    CrosstermMouseEvent {
        kind: match mouse.kind {
            TerminaMouseEventKind::Down(button) => {
                CrosstermMouseEventKind::Down(map_mouse_button(button))
            }
            TerminaMouseEventKind::Up(button) => {
                CrosstermMouseEventKind::Up(map_mouse_button(button))
            }
            TerminaMouseEventKind::Drag(button) => {
                CrosstermMouseEventKind::Drag(map_mouse_button(button))
            }
            TerminaMouseEventKind::Moved => CrosstermMouseEventKind::Moved,
            TerminaMouseEventKind::ScrollDown => CrosstermMouseEventKind::ScrollDown,
            TerminaMouseEventKind::ScrollUp => CrosstermMouseEventKind::ScrollUp,
            TerminaMouseEventKind::ScrollLeft => CrosstermMouseEventKind::ScrollLeft,
            TerminaMouseEventKind::ScrollRight => CrosstermMouseEventKind::ScrollRight,
        },
        column: mouse.column,
        row: mouse.row,
        modifiers: map_modifiers(mouse.modifiers),
    }
}

fn map_mouse_button(button: TerminaMouseButton) -> CrosstermMouseButton {
    match button {
        TerminaMouseButton::Left => CrosstermMouseButton::Left,
        TerminaMouseButton::Right => CrosstermMouseButton::Right,
        TerminaMouseButton::Middle => CrosstermMouseButton::Middle,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use termina::escape::csi::ThemeMode;
    use termina::escape::osc::Osc;
    use termina::event::KeyEventState;
    use web_time::Instant;

    fn parsed_event(bytes: &[u8]) -> TerminaEvent {
        let mut parser = Parser::default();
        parser.parse(bytes, false);
        parser.pop().expect("sequence should parse")
    }

    #[test]
    fn exact_dark_and_light_theme_reports_request_refresh() {
        for (bytes, expected) in [
            (b"\x1b[?997;1n".as_slice(), ThemeMode::Dark),
            (b"\x1b[?997;2n".as_slice(), ThemeMode::Light),
        ] {
            let event = parsed_event(bytes);
            assert_eq!(
                event,
                TerminaEvent::Csi(Csi::Mode(Mode::ReportTheme(expected)))
            );
            assert_eq!(map_termina_event(event), TerminaEventAction::ThemeRefresh);
        }
    }

    #[test]
    fn theme_report_wakes_runner_for_full_palette_refresh() {
        let (events, receiver) = mpsc::channel();

        let mut input = WorkerInput::default();
        assert!(input.push(b"\x1b[?997;1n", &events));
        assert_eq!(
            receiver.recv().unwrap(),
            RunnerEvent::HostTerminalColorRefreshRequested
        );
    }

    #[test]
    fn palette_query_keeps_the_persistent_parser_across_split_input() {
        let (events, receiver) = mpsc::channel();
        let mut input = WorkerInput::default();

        assert!(input.push(b"\x1b[200~abc \x1b]4;1;rgb:1111/", &events));
        input.colors.start_query();
        assert!(input.push(b"1111/1111\x1b\\ def \xf0\x9f\x99\x82\x1b[201~", &events));

        let RunnerEvent::Terminal(CrosstermEvent::Paste(text)) = receiver.recv().unwrap() else {
            panic!("the split bracketed paste should remain one input event");
        };
        assert_eq!(text, "abc \x1b]4;1;rgb:1111/1111/1111\x1b\\ def 🙂");
        let previous = HostTerminalColors {
            ansi: std::array::from_fn(|index| crate::style::Color::Rgb(index as u8, 1, 2)),
            fg: crate::style::Color::Rgb(230, 230, 230),
            bg: crate::style::Color::Rgb(20, 20, 20),
            ansi_reported: HostTerminalColors::ALL_ANSI_REPORTED,
        };
        let colors = input.colors.finish_query(Some(&previous)).unwrap();
        assert_eq!(
            colors.ansi[1], previous.ansi[1],
            "an OSC 4-looking string inside paste content is not a palette response"
        );
    }

    fn mouse_kind(event: RunnerEvent) -> (CrosstermMouseEventKind, u16) {
        match event {
            RunnerEvent::Terminal(CrosstermEvent::Mouse(mouse))
            | RunnerEvent::Pointer {
                event: CrosstermEvent::Mouse(mouse),
                ..
            } => (mouse.kind, mouse.column),
            other => panic!("expected a mouse event, got {other:?}"),
        }
    }

    #[test]
    fn a_read_of_motion_sends_only_its_newest_position() {
        let (events, receiver) = mpsc::channel();
        let mut input = WorkerInput::default();

        assert!(input.push(b"\x1b[<35;10;5M\x1b[<35;11;5M\x1b[<35;12;5M", &events));

        assert_eq!(
            mouse_kind(receiver.try_recv().unwrap()),
            (CrosstermMouseEventKind::Moved, 11),
            "the newest position, not the first one of the read"
        );
        assert!(receiver.try_recv().is_err());
        assert!(
            input.pending_motion.is_none(),
            "nothing is held past its read"
        );
    }

    #[test]
    fn every_read_sends_its_motion_at_once() {
        let (events, receiver) = mpsc::channel();
        let mut input = WorkerInput::default();

        for column in [10, 11, 12] {
            assert!(input.push(format!("\x1b[<35;{column};5M").as_bytes(), &events));
            assert_eq!(
                mouse_kind(receiver.try_recv().unwrap()),
                (CrosstermMouseEventKind::Moved, column - 1),
                "coalescing never makes a position wait for a later read"
            );
        }
    }

    #[test]
    fn a_press_sends_the_motion_before_it_first() {
        let (events, receiver) = mpsc::channel();
        let mut input = WorkerInput::default();

        assert!(input.push(
            b"\x1b[<35;10;5M\x1b[<35;20;5M\x1b[<0;20;5M\x1b[<35;30;5M",
            &events
        ));

        assert_eq!(
            mouse_kind(receiver.try_recv().unwrap()),
            (CrosstermMouseEventKind::Moved, 19),
            "the runner learns where the pointer went before it learns of the press"
        );
        assert_eq!(
            mouse_kind(receiver.try_recv().unwrap()).0,
            CrosstermMouseEventKind::Down(CrosstermMouseButton::Left)
        );
        assert_eq!(
            mouse_kind(receiver.try_recv().unwrap()),
            (CrosstermMouseEventKind::Moved, 29)
        );
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn drags_are_never_merged() {
        let (events, receiver) = mpsc::channel();
        let mut input = WorkerInput::default();

        assert!(input.push(b"\x1b[<32;10;5M\x1b[<32;11;5M\x1b[<32;12;5M", &events));

        assert_eq!(receiver.try_iter().count(), 3);
    }

    #[test]
    fn persistent_parser_keeps_a_split_utf8_character() {
        let (events, receiver) = mpsc::channel();
        let mut input = WorkerInput::default();

        assert!(input.push(&[0xc3], &events));
        assert!(receiver.try_recv().is_err());
        input.colors.start_query();
        assert!(input.push(&[0xa9], &events));

        std::assert_matches!(
            receiver.recv().unwrap(),
            RunnerEvent::Terminal(CrosstermEvent::Key(event))
                if event.code == CrosstermKeyCode::Char('é'));
    }

    #[test]
    fn pending_palette_query_waits_for_a_split_ss3_key() {
        let (events, receiver) = mpsc::channel();
        let mut input = WorkerInput::default();
        let mut query = Some(ActiveColorQuery {
            deadline: None,
            previous: None,
            refresh_pending: false,
        });
        let mut output = Vec::new();

        assert!(input.push(b"\x1bO", &events));
        start_color_query_if_ready(&mut output, &mut input, &mut query, true).unwrap();
        assert!(output.is_empty());
        assert!(query.as_ref().unwrap().deadline.is_none());

        assert!(input.push(b"P", &events));
        std::assert_matches!(
            receiver.recv().unwrap(),
            RunnerEvent::Terminal(CrosstermEvent::Key(event))
                if event.code == CrosstermKeyCode::F(1));
        start_color_query_if_ready(&mut output, &mut input, &mut query, true).unwrap();
        assert_eq!(output, build_live_color_query_batch());
        assert!(query.as_ref().unwrap().deadline.is_some());
    }

    #[test]
    fn refresh_during_active_query_rearms_after_applying_completed_colors() {
        let previous = HostTerminalColors {
            ansi: std::array::from_fn(|index| crate::style::Color::Rgb(index as u8, 1, 2)),
            fg: crate::style::Color::Rgb(230, 230, 230),
            bg: crate::style::Color::Rgb(20, 20, 20),
            ansi_reported: HostTerminalColors::ALL_ANSI_REPORTED,
        };
        let newer_baseline = HostTerminalColors {
            ansi: std::array::from_fn(|index| crate::style::Color::Rgb(index as u8, 3, 4)),
            fg: crate::style::Color::Rgb(240, 240, 240),
            bg: crate::style::Color::Rgb(10, 10, 10),
            ansi_reported: HostTerminalColors::ALL_ANSI_REPORTED,
        };
        let mut input = WorkerInput::default();
        input.colors.start_query();
        let mut query = Some(ActiveColorQuery {
            deadline: Some(std::time::Instant::now() - Duration::from_millis(1)),
            previous: Some(previous),
            refresh_pending: false,
        });

        queue_color_refresh(&mut query, Some(newer_baseline));
        assert!(query.as_ref().unwrap().refresh_pending);

        let (events, receiver) = mpsc::channel();
        assert!(finish_color_query_if_ready(&events, &mut input, &mut query));
        assert_eq!(
            receiver.recv().unwrap(),
            RunnerEvent::HostTerminalColors(previous)
        );
        assert_eq!(
            receiver.recv().unwrap(),
            RunnerEvent::HostTerminalColorRefreshRequested
        );
        assert!(query.is_none());
    }

    #[test]
    fn maps_ordinary_events_without_casting_flags() {
        let key = TerminaKeyEvent {
            code: TerminaKeyCode::Function(12),
            kind: TerminaKeyEventKind::Repeat,
            modifiers: TerminaModifiers::SHIFT
                | TerminaModifiers::CONTROL
                | TerminaModifiers::ALT
                | TerminaModifiers::SUPER
                | TerminaModifiers::HYPER
                | TerminaModifiers::META
                | TerminaModifiers::CAPS_LOCK,
            state: KeyEventState::KEYPAD | KeyEventState::NUM_LOCK,
        };
        let TerminaEventAction::Input(CrosstermEvent::Key(mapped)) =
            map_termina_event(TerminaEvent::Key(key))
        else {
            panic!("key should map to Crossterm input");
        };
        assert_eq!(mapped.code, CrosstermKeyCode::F(12));
        assert_eq!(mapped.kind, CrosstermKeyEventKind::Repeat);
        assert_eq!(
            mapped.modifiers,
            CrosstermKeyModifiers::SHIFT
                | CrosstermKeyModifiers::CONTROL
                | CrosstermKeyModifiers::ALT
                | CrosstermKeyModifiers::SUPER
                | CrosstermKeyModifiers::HYPER
                | CrosstermKeyModifiers::META
        );
        assert_eq!(
            mapped.state,
            CrosstermKeyEventState::KEYPAD
                | CrosstermKeyEventState::CAPS_LOCK
                | CrosstermKeyEventState::NUM_LOCK
        );

        let mouse = TerminaMouseEvent {
            kind: TerminaMouseEventKind::Drag(TerminaMouseButton::Right),
            column: 7,
            row: 9,
            modifiers: TerminaModifiers::SHIFT,
        };
        std::assert_matches!(
            map_termina_event(TerminaEvent::Mouse(mouse)),
            TerminaEventAction::Input(CrosstermEvent::Mouse(CrosstermMouseEvent {
                kind: CrosstermMouseEventKind::Drag(CrosstermMouseButton::Right),
                column: 7,
                row: 9,
                modifiers: CrosstermKeyModifiers::SHIFT,
            }))
        );
        assert_eq!(
            map_termina_event(TerminaEvent::FocusIn),
            TerminaEventAction::Input(CrosstermEvent::FocusGained)
        );
        assert_eq!(
            map_termina_event(TerminaEvent::FocusOut),
            TerminaEventAction::Input(CrosstermEvent::FocusLost)
        );
        assert_eq!(
            map_termina_event(TerminaEvent::WindowResized(termina::WindowSize {
                cols: 80,
                rows: 24,
                pixel_width: None,
                pixel_height: None,
            })),
            TerminaEventAction::Input(CrosstermEvent::Resize(80, 24))
        );
        assert_eq!(
            map_termina_event(TerminaEvent::Paste("hello".into())),
            TerminaEventAction::Input(CrosstermEvent::Paste("hello".into()))
        );
    }

    #[test]
    fn malformed_shift_releases_map_back_to_shift() {
        let TerminaEventAction::Input(CrosstermEvent::Key(capslock_coded)) =
            map_termina_event(parsed_event(b"\x1b[57358;130:3u"))
        else {
            panic!("CapsLock-coded Shift release should map to Crossterm input");
        };
        assert_eq!(
            capslock_coded.code,
            CrosstermKeyCode::Modifier(CrosstermModifierKeyCode::LeftShift)
        );
        assert_eq!(capslock_coded.kind, CrosstermKeyEventKind::Release);

        let TerminaEventAction::Input(CrosstermEvent::Key(mapped)) =
            map_termina_event(parsed_event(b"\x1b[57441:0;2:3u"))
        else {
            panic!("Shift release should map to Crossterm input");
        };

        assert_eq!(
            mapped.code,
            CrosstermKeyCode::Modifier(CrosstermModifierKeyCode::LeftShift)
        );
        assert_eq!(mapped.kind, CrosstermKeyEventKind::Release);
    }

    #[test]
    fn unrelated_protocol_responses_are_not_exposed_as_input() {
        assert_eq!(
            map_termina_event(TerminaEvent::Csi(Csi::Mode(Mode::QueryTheme))),
            TerminaEventAction::Ignore
        );
        assert_eq!(
            map_termina_event(TerminaEvent::Osc(Osc::SetWindowTitle("ignored"))),
            TerminaEventAction::Ignore
        );
    }

    #[test]
    fn worker_control_wake_makes_poll_immediately_ready() {
        let (terminal_reader, _terminal_writer) = UnixStream::pair().unwrap();
        let (mut wake_reader, wake_writer) = worker_wake_pipe().unwrap();
        let (commands, _command_receiver) = mpsc::channel();
        let control = WorkerControl {
            commands,
            wake: Mutex::new(wake_writer),
            paused: AtomicBool::new(false),
            worker_thread: Mutex::new(None),
        };

        control.wake();
        assert_eq!(
            poll_terminal_input(&terminal_reader, &wake_reader, INPUT_POLL_INTERVAL).unwrap(),
            WorkerReady {
                terminal: false,
                wake: true,
            }
        );
        drain_worker_wake(&mut wake_reader).unwrap();
        assert_eq!(
            poll_terminal_input(&terminal_reader, &wake_reader, Duration::ZERO).unwrap(),
            WorkerReady::default()
        );
    }

    #[test]
    fn simultaneous_wake_and_tty_readiness_defers_probe_until_input_completes() {
        let (mut terminal_reader, mut terminal_writer) = UnixStream::pair().unwrap();
        let (mut wake_reader, wake_writer) = worker_wake_pipe().unwrap();
        let (commands, command_receiver) = mpsc::channel();
        let control = WorkerControl {
            commands,
            wake: Mutex::new(wake_writer),
            paused: AtomicBool::new(false),
            worker_thread: Mutex::new(None),
        };

        terminal_writer.write_all(b"\x1bO").unwrap();
        control
            .commands
            .send(WorkerCommand::RefreshHostColors(None))
            .unwrap();
        control.wake();
        assert_eq!(
            poll_terminal_input(&terminal_reader, &wake_reader, Duration::ZERO).unwrap(),
            WorkerReady {
                terminal: true,
                wake: true,
            }
        );
        drain_worker_wake(&mut wake_reader).unwrap();

        let mut query = None;
        let WorkerCommand::RefreshHostColors(previous) = command_receiver.recv().unwrap() else {
            panic!("refresh command should remain queued after the wake");
        };
        queue_color_refresh(&mut query, previous);

        let (events, receiver) = mpsc::channel();
        let mut input = WorkerInput::default();
        assert_eq!(
            poll_terminal_input(&terminal_reader, &wake_reader, Duration::ZERO).unwrap(),
            WorkerReady {
                terminal: true,
                wake: false,
            }
        );
        assert!(read_terminal_bytes(&mut terminal_reader, &mut input, &events).unwrap());
        assert!(receiver.try_recv().is_err());

        let mut output = Vec::new();
        start_color_query_if_ready(&mut output, &mut input, &mut query, false).unwrap();
        assert!(input.settle(&events));
        start_color_query_if_ready(&mut output, &mut input, &mut query, true).unwrap();
        assert!(output.is_empty());

        terminal_writer.write_all(b"P").unwrap();
        assert!(
            poll_terminal_input(&terminal_reader, &wake_reader, Duration::ZERO)
                .unwrap()
                .terminal
        );
        assert!(read_terminal_bytes(&mut terminal_reader, &mut input, &events).unwrap());
        std::assert_matches!(
            receiver.recv().unwrap(),
            RunnerEvent::Terminal(CrosstermEvent::Key(event))
                if event.code == CrosstermKeyCode::F(1));
        assert!(input.settle(&events));
        start_color_query_if_ready(&mut output, &mut input, &mut query, true).unwrap();
        assert_eq!(output, build_live_color_query_batch());
    }

    #[test]
    #[cfg(panic = "unwind")]
    fn worker_panic_is_reported_as_input_error() {
        let (events, receiver) = mpsc::channel();

        contain_worker_panic(&events, || panic!("contained worker panic"));

        std::assert_matches!(
            receiver.recv().unwrap(),
            RunnerEvent::InputError(message) if message.contains("contained worker panic"));
    }

    #[test]
    fn the_hosts_answer_about_pixel_reports_is_found_across_reads() {
        let mut scanner = ModeReplyScanner::default();
        assert_eq!(
            scanner.find(b"\x1b[<35;4;2M\x1b[?1016;1$y\x1b[<35;40;20M"),
            Some((21, 1))
        );

        // Split anywhere, the answer still ends where its last byte arrives.
        assert_eq!(scanner.find(b"x\x1b[?10"), None);
        assert_eq!(scanner.find(b"16;"), None);
        assert_eq!(scanner.find(b"2$y\x1b[A"), Some((3, 2)));

        // Another mode's answer, or a broken one, is not it - but can be followed straight by it.
        assert_eq!(scanner.find(b"\x1b[?2026;2$y"), None);
        assert_eq!(scanner.find(b"\x1b[?1016;$y"), None);
        assert_eq!(scanner.find(b"\x1b[?1016;\x1b[?1016;4$y"), Some((19, 4)));
    }

    /// The host keeps sending its old coordinates until it reads a switch of mode 1016, and a
    /// report already on its way still carries them. Played here by a real pty: the "host" sends
    /// one report the old way after the switch was written, then its answer, then one the new way.
    #[test]
    #[cfg(all(target_os = "linux", feature = "terminal"))]
    fn a_pixel_switch_takes_effect_where_the_host_confirms_it() {
        use crate::backend::ratatui_backend::host_input::pty_test::{child_report, run_on_pty};
        use crate::backend::ratatui_backend::terminal_handoff::input_handoff_slot;

        let name = "a_pixel_switch_takes_effect_where_the_host_confirms_it";
        if let Some(report) = child_report() {
            crossterm::terminal::enable_raw_mode().expect("raw mode");
            pixel_mouse::note_host_support(true);
            // Cells of 10 by 20 pixels.
            pixel_mouse::note_window_size(80, 24, Some(800), Some(480));
            pixel_mouse::note_wanted(true);
            let coordinator = TerminaInputCoordinator::start(input_handoff_slot()).expect("start");
            report.line("ready", 1);
            let started = Instant::now();
            let mut seen = 0;
            while seen < 4 && started.elapsed() < Duration::from_secs(10) {
                let read = match coordinator
                    .receiver()
                    .recv_timeout(Duration::from_millis(100))
                {
                    Ok(RunnerEvent::Terminal(CrosstermEvent::Mouse(mouse))) => {
                        format!("cells {},{}", mouse.column, mouse.row)
                    }
                    Ok(RunnerEvent::Pointer {
                        event: CrosstermEvent::Mouse(mouse),
                        sub_cell,
                    }) => format!(
                        "pixels {},{} +{},{}",
                        mouse.column, mouse.row, sub_cell.0, sub_cell.1
                    ),
                    _ => continue,
                };
                report.line(&format!("report{seen}"), read);
                seen += 1;
                if seen == 2 {
                    // The program that wanted pixels is gone.
                    pixel_mouse::note_wanted(false);
                    coordinator.sync_pixel_mouse().expect("sync");
                }
            }
            let _ = crossterm::terminal::disable_raw_mode();
            report.line("done", 1);
            return;
        }

        let module = module_path!();
        let module = module.split_once("::").map_or(module, |(_, rest)| rest);
        let report = run_on_pty(&format!("{module}::{name}"), |master| {
            let mut terminal = File::from(master);
            let mut output = Vec::new();
            let mut wait_for = |terminal: &mut File, wanted: &[u8]| {
                let deadline = Instant::now() + Duration::from_secs(10);
                while !output.windows(wanted.len()).any(|bytes| bytes == wanted) {
                    assert!(
                        Instant::now() < deadline,
                        "the child never wrote {wanted:?}"
                    );
                    let mut poll = libc::pollfd {
                        fd: terminal.as_raw_fd(),
                        events: libc::POLLIN,
                        revents: 0,
                    };
                    // SAFETY: one valid descriptor, bounded wait.
                    if unsafe { libc::poll(&mut poll, 1, 50) } > 0 {
                        let mut buffer = [0; 4096];
                        let count = terminal.read(&mut buffer).expect("read the child");
                        output.extend_from_slice(&buffer[..count]);
                    }
                }
            };
            // Cell (10, 5) is `11;6` on the wire; pixel (105, 45) is `106;46`, cell (10, 2).
            wait_for(&mut terminal, b"\x1b[?1016h\x1b[?1016$p");
            terminal.write_all(b"\x1b[<0;11;6M").unwrap();
            std::thread::sleep(Duration::from_millis(50));
            terminal
                .write_all(b"\x1b[?1016;1$y\x1b[<0;106;46M")
                .unwrap();
            wait_for(&mut terminal, b"\x1b[?1016l\x1b[?1016$p");
            terminal
                .write_all(b"\x1b[<0;106;46M\x1b[?1016;2$y\x1b[<0;11;6M")
                .unwrap();
            terminal
        });
        assert_eq!(
            report.get("done").map(String::as_str),
            Some("1"),
            "{report:?}"
        );
        let reports: Vec<_> = (0..4)
            .map(|index| report[&format!("report{index}")].as_str())
            .collect();
        assert_eq!(
            reports,
            [
                "cells 10,5",
                "pixels 10,2 +5,5",
                "pixels 10,2 +5,5",
                "cells 10,5"
            ],
            "each report is read as the host sent it"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn worker_stops_cleanly_when_the_terminal_hangs_up_during_a_read() {
        use crate::backend::ratatui_backend::host_input::pty_test::{
            child_report, cpu_time, run_and_hang_up,
        };
        use crate::backend::ratatui_backend::terminal_handoff::input_handoff_slot;

        let name = "worker_stops_cleanly_when_the_terminal_hangs_up_during_a_read";
        if let Some(report) = child_report() {
            let coordinator = TerminaInputCoordinator::start(input_handoff_slot()).expect("start");
            report.line("ready", 1);
            let started = Instant::now();
            loop {
                match coordinator
                    .receiver()
                    .recv_timeout(Duration::from_millis(100))
                {
                    Ok(RunnerEvent::HostHungUp) => break,
                    Ok(RunnerEvent::InputError(message)) => {
                        report.line("input_error", message);
                        return;
                    }
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        report.line("error", "channel disconnected");
                        return;
                    }
                }
                if started.elapsed() > Duration::from_secs(10) {
                    report.line("error", "no hang-up within 10s");
                    return;
                }
            }
            report.line("hung_up", 1);
            let cpu_before = cpu_time();
            std::thread::sleep(Duration::from_millis(500));
            report.line("cpu_ms", (cpu_time() - cpu_before).as_millis());
            let join_started = Instant::now();
            drop(coordinator);
            report.line("join_ms", join_started.elapsed().as_millis());
            report.line("done", 1);
            return;
        }

        let module = module_path!();
        let module = module.split_once("::").map_or(module, |(_, rest)| rest);
        let report = run_and_hang_up(&format!("{module}::{name}"));
        assert_eq!(report.get("input_error"), None, "{report:?}");
        assert_eq!(report.get("error"), None, "{report:?}");
        assert_eq!(
            report.get("done").map(String::as_str),
            Some("1"),
            "{report:?}"
        );
        let cpu_ms: u64 = report["cpu_ms"].parse().unwrap();
        assert!(
            cpu_ms < 100,
            "the stopped worker used {cpu_ms}ms of CPU in 500ms"
        );
        let join_ms: u64 = report["join_ms"].parse().unwrap();
        assert!(join_ms < 1_000, "joining the worker took {join_ms}ms");
    }
}
