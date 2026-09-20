#![allow(unsafe_code)]

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
use crate::backend::ratatui_backend::terminal_transition::{
    CrosstermTransitionExecutor, execute_plan, pixel_mouse_plan,
};
use crate::style::HostTerminalColors;
use crate::style::terminal_colors::{HostColorResponseParser, build_live_color_query_batch};

use super::RunnerEvent;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const ESCAPE_DISAMBIGUATION: Duration = Duration::from_millis(10);
const HOST_COLOR_QUERY_TIMEOUT: Duration = Duration::from_millis(200);

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
    if let Ok(size) = crossterm::terminal::window_size() {
        pixel_mouse::note_window_size(size.columns, size.rows, Some(size.width), Some(size.height));
    }
    // Asked for here rather than in the enter plan: the host's own answer arrived before this, but
    // the cell size a pixel report is divided by is only known now.
    sync_pixel_mouse_mode();
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
    let mut query = None;
    let mut settle_at = None;
    let mut last_window_size = crossterm::terminal::window_size().ok();
    loop {
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
            next_poll_timeout(settle_at, query.as_ref()),
        ) {
            Ok(ready) if ready.wake => {
                if let Err(err) = drain_worker_wake(&mut wake_reader) {
                    stop(err);
                    break;
                }
                continue;
            }
            Ok(ready) if ready.terminal => {
                let mut bytes = [0; 1024];
                match terminal.read(&mut bytes) {
                    Ok(0) => {
                        stop(io::Error::new(
                            io::ErrorKind::UnexpectedEof,
                            "terminal input reached end-of-file",
                        ));
                        break;
                    }
                    Ok(read) => {
                        if !input.push(&bytes[..read], &events) {
                            break;
                        }
                        settle_at = Some(Instant::now() + ESCAPE_DISAMBIGUATION);
                    }
                    Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
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
        if !refresh_window_size(&events, &mut last_window_size) {
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
                        WorkerCommand::RefreshHostColors(_) => {}
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
    colors: HostColorResponseParser,
}

impl WorkerInput {
    fn push(&mut self, bytes: &[u8], events: &mpsc::Sender<RunnerEvent>) -> bool {
        self.colors.push(bytes);
        self.parser.parse(bytes, true);
        self.dispatch(events)
    }

    fn settle(&mut self, events: &mpsc::Sender<RunnerEvent>) -> bool {
        self.parser.parse(&[], false);
        self.colors.settle_input();
        self.dispatch(events)
    }

    fn dispatch(&mut self, events: &mpsc::Sender<RunnerEvent>) -> bool {
        while let Some(event) = self.parser.pop() {
            if !dispatch_termina_event(event, events) {
                return false;
            }
        }
        true
    }
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

fn next_poll_timeout(settle_at: Option<Instant>, query: Option<&ActiveColorQuery>) -> Duration {
    let now = Instant::now();
    [settle_at, query.and_then(|query| query.deadline)]
        .into_iter()
        .flatten()
        .map(|deadline| deadline.saturating_duration_since(now))
        .fold(INPUT_POLL_INTERVAL, Duration::min)
}

fn refresh_window_size(
    events: &mpsc::Sender<RunnerEvent>,
    previous: &mut Option<crossterm::terminal::WindowSize>,
) -> bool {
    let Ok(size) = crossterm::terminal::window_size() else {
        return true;
    };
    let changed = previous.as_ref().is_none_or(|old| {
        (old.columns, old.rows, old.width, old.height)
            != (size.columns, size.rows, size.width, size.height)
    });
    if !changed {
        return true;
    }
    let (columns, rows, width, height) = (size.columns, size.rows, size.width, size.height);
    *previous = Some(size);
    pixel_mouse::note_window_size(columns, rows, Some(width), Some(height));
    sync_pixel_mouse_mode();
    events
        .send(RunnerEvent::Terminal(CrosstermEvent::Resize(columns, rows)))
        .is_ok()
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
    let timeout = timeout.as_millis().min(i32::MAX as u128) as i32;
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

fn dispatch_termina_event(event: TerminaEvent, events: &mpsc::Sender<RunnerEvent>) -> bool {
    let action = map_termina_event(event);
    // A resize is where a padded window can first divide evenly, and so where the mode can first be
    // worth asking for. The ask lives here rather than in the mapping because it writes to the host,
    // and this is the worker that owns input.
    if matches!(
        action,
        TerminaEventAction::Input(CrosstermEvent::Resize(..))
    ) {
        sync_pixel_mouse_mode();
    }
    match action {
        TerminaEventAction::Input(event) => events.send(RunnerEvent::Terminal(event)).is_ok(),
        TerminaEventAction::Pointer(event, sub_cell) => events
            .send(RunnerEvent::Pointer { event, sub_cell })
            .is_ok(),
        TerminaEventAction::ThemeRefresh => events
            .send(RunnerEvent::HostTerminalColorRefreshRequested)
            .is_ok(),
        TerminaEventAction::Ignore => true,
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

/// Bring the host's SGR-pixels mode in line with what this run can make sense of.
///
/// Both halves of `pixel_mouse::is_active` can turn true after startup, because the cell size is
/// re-derived from every resize. The mode has to be asked for at that moment rather than only at
/// startup: `map_pointer_event` reads reports as pixels only once the host has actually been asked,
/// so a cell size learned late would otherwise leave the precision on the table forever.
///
/// Only ever called from the Termina worker, which is the one decoder that can read a pixel report -
/// which is also why it stays out of `map_termina_event`, whose job is a pure mapping. The write is
/// noted only if it lands, so a host that never heard the escape keeps sending cells and keeps being
/// read that way.
fn sync_pixel_mouse_mode() {
    let wanted = pixel_mouse::is_active();
    if wanted == pixel_mouse::reports_pixels() {
        return;
    }
    let mut stdout = io::stdout();
    let mut executor = CrosstermTransitionExecutor::new(&mut stdout);
    if execute_plan(&mut executor, &pixel_mouse_plan(wanted)).is_ok() {
        pixel_mouse::note_mode_enabled(wanted);
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

        assert!(dispatch_termina_event(
            parsed_event(b"\x1b[?997;1n"),
            &events
        ));
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
        };
        let colors = input.colors.finish_query(Some(&previous)).unwrap();
        assert_eq!(
            colors.ansi[1], previous.ansi[1],
            "an OSC 4-looking string inside paste content is not a palette response"
        );
    }

    #[test]
    fn persistent_parser_keeps_a_split_utf8_character() {
        let (events, receiver) = mpsc::channel();
        let mut input = WorkerInput::default();

        assert!(input.push(&[0xc3], &events));
        assert!(receiver.try_recv().is_err());
        input.colors.start_query();
        assert!(input.push(&[0xa9], &events));

        assert!(matches!(
            receiver.recv().unwrap(),
            RunnerEvent::Terminal(CrosstermEvent::Key(event))
                if event.code == CrosstermKeyCode::Char('é')
        ));
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
        assert!(matches!(
            receiver.recv().unwrap(),
            RunnerEvent::Terminal(CrosstermEvent::Key(event))
                if event.code == CrosstermKeyCode::F(1)
        ));
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
        };
        let newer_baseline = HostTerminalColors {
            ansi: std::array::from_fn(|index| crate::style::Color::Rgb(index as u8, 3, 4)),
            fg: crate::style::Color::Rgb(240, 240, 240),
            bg: crate::style::Color::Rgb(10, 10, 10),
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
        assert!(matches!(
            map_termina_event(TerminaEvent::Mouse(mouse)),
            TerminaEventAction::Input(CrosstermEvent::Mouse(CrosstermMouseEvent {
                kind: CrosstermMouseEventKind::Drag(CrosstermMouseButton::Right),
                column: 7,
                row: 9,
                modifiers: CrosstermKeyModifiers::SHIFT,
            }))
        ));
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
    #[cfg(panic = "unwind")]
    fn worker_panic_is_reported_as_input_error() {
        let (events, receiver) = mpsc::channel();

        contain_worker_panic(&events, || panic!("contained worker panic"));

        assert!(matches!(
            receiver.recv().unwrap(),
            RunnerEvent::InputError(message) if message.contains("contained worker panic")
        ));
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
