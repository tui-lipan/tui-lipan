use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::Duration;

use crossterm::event::{
    Event as CrosstermEvent, KeyCode as CrosstermKeyCode, KeyEvent as CrosstermKeyEvent,
    KeyEventKind as CrosstermKeyEventKind, KeyEventState as CrosstermKeyEventState,
    KeyModifiers as CrosstermKeyModifiers, MediaKeyCode as CrosstermMediaKeyCode,
    ModifierKeyCode as CrosstermModifierKeyCode, MouseButton as CrosstermMouseButton,
    MouseEvent as CrosstermMouseEvent, MouseEventKind as CrosstermMouseEventKind,
};
use termina::escape::csi::{Csi, Mode};
use termina::event::{
    Event as TerminaEvent, KeyCode as TerminaKeyCode, KeyEvent as TerminaKeyEvent,
    KeyEventKind as TerminaKeyEventKind, KeyEventState as TerminaKeyEventState,
    MediaKeyCode as TerminaMediaKeyCode, ModifierKeyCode as TerminaModifierKeyCode,
    Modifiers as TerminaModifiers, MouseButton as TerminaMouseButton,
    MouseEvent as TerminaMouseEvent, MouseEventKind as TerminaMouseEventKind,
};
use termina::{EventReader, Parser, PlatformTerminal, Terminal as _};

use crate::app::input::pixel_mouse::{self, PointerReport};
use crate::backend::ratatui_backend::host_input::is_hang_up;
use crate::backend::ratatui_backend::terminal_handoff::{
    InputHandoffControl, InputHandoffSlot, InputResumeKind, register_input_handoff_control,
    unregister_input_handoff_control,
};
use crate::backend::ratatui_backend::terminal_transition::{
    CrosstermTransitionExecutor, execute_plan, pixel_mouse_plan,
};

use super::RunnerEvent;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(100);

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
        let reader = open_event_reader()?;
        let waker = reader.waker();
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let spare_sender = event_tx.clone();
        let control = Arc::new(WorkerControl {
            commands: command_tx,
            wake: Mutex::new(Box::new(move || waker.wake())),
            paused: AtomicBool::new(false),
            worker_thread: Mutex::new(None),
        });
        let worker_control = Arc::clone(&control);
        let panic_events = event_tx.clone();
        let worker = std::thread::Builder::new()
            .name("termina-reader".into())
            .spawn(move || {
                contain_worker_panic(&panic_events, || {
                    run_worker(reader, command_rx, event_tx, worker_control);
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
    Resume {
        ack: mpsc::SyncSender<io::Result<()>>,
        kind: InputResumeKind,
    },
    Shutdown,
}

struct WorkerControl {
    commands: mpsc::Sender<WorkerCommand>,
    wake: Mutex<Box<dyn Fn() -> io::Result<()> + Send + Sync>>,
    paused: AtomicBool,
    worker_thread: Mutex<Option<std::thread::ThreadId>>,
}

impl WorkerControl {
    fn wake(&self) {
        if let Ok(wake) = self.wake.lock() {
            let _ = wake();
        }
    }

    fn replace_wake(&self, wake: impl Fn() -> io::Result<()> + Send + Sync + 'static) {
        if let Ok(mut current) = self.wake.lock() {
            *current = Box::new(wake);
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

    fn resume(&self, kind: InputResumeKind) -> io::Result<()> {
        if !self.paused.load(Ordering::SeqCst) {
            return Ok(());
        }
        let (ack_tx, ack_rx) = mpsc::sync_channel(0);
        self.commands
            .send(WorkerCommand::Resume { ack: ack_tx, kind })
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

    fn resume(&self, kind: InputResumeKind) -> io::Result<()> {
        WorkerControl::resume(self, kind)
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

fn open_event_reader() -> io::Result<EventReader> {
    let terminal = PlatformTerminal::new()?;
    if let Ok(size) = terminal.get_dimensions() {
        pixel_mouse::note_window_size(size.cols, size.rows, size.pixel_width, size.pixel_height);
    }
    let reader = terminal.event_reader();
    // Asked for here rather than in the enter plan: the host's own answer arrived before this, but
    // the cell size a pixel report is divided by is only known now.
    sync_pixel_mouse_mode();
    Ok(reader)
}

fn run_worker(
    mut reader: EventReader,
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
    loop {
        match process_worker_commands(&mut reader, &commands, &control, &events) {
            Ok(true) => {}
            Ok(false) => break,
            Err(err) => {
                stop(err);
                break;
            }
        }

        match reader.poll(Some(INPUT_POLL_INTERVAL), |_| true) {
            Ok(true) => match reader.read(|_| true) {
                Ok(event) => {
                    if !dispatch_termina_event(event, &events) {
                        break;
                    }
                }
                // The waker unblocks this read to hand the worker a command, so an interrupt is
                // the signal working as intended rather than a failed read. Loop around and let
                // `process_worker_commands` collect what the wake was for.
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => {
                    stop(err);
                    break;
                }
            },
            Ok(false) => {}
            Err(err) => {
                stop(err);
                break;
            }
        }
    }
}

fn process_worker_commands(
    reader: &mut EventReader,
    commands: &mpsc::Receiver<WorkerCommand>,
    control: &WorkerControl,
    events: &mpsc::Sender<RunnerEvent>,
) -> io::Result<bool> {
    while let Ok(command) = commands.try_recv() {
        match command {
            WorkerCommand::Pause(ack) => {
                if !drain_ready_reader_events(reader, events)? {
                    let _ = ack.send(());
                    return Ok(false);
                }
                control.paused.store(true, Ordering::SeqCst);
                let _ = ack.send(());
                loop {
                    match commands.recv().map_err(|_| worker_stopped())? {
                        WorkerCommand::Resume { ack, kind } => match kind {
                            InputResumeKind::TerminalQuery => {
                                control.paused.store(false, Ordering::SeqCst);
                                let _ = ack.send(Ok(()));
                                break;
                            }
                            InputResumeKind::ExternalProcess => match open_event_reader() {
                                Ok(new_reader) => {
                                    let waker = new_reader.waker();
                                    *reader = new_reader;
                                    control.replace_wake(move || waker.wake());
                                    control.paused.store(false, Ordering::SeqCst);
                                    let _ =
                                        events.send(RunnerEvent::HostTerminalColorRefreshRequested);
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
                        },
                        WorkerCommand::Shutdown => return Ok(false),
                        WorkerCommand::Pause(ack) => {
                            let _ = ack.send(());
                        }
                    }
                }
            }
            WorkerCommand::Resume { ack, .. } => {
                let _ = ack.send(Ok(()));
            }
            WorkerCommand::Shutdown => return Ok(false),
        }
    }
    Ok(true)
}

fn drain_ready_reader_events(
    reader: &EventReader,
    events: &mpsc::Sender<RunnerEvent>,
) -> io::Result<bool> {
    for _ in 0..8192 {
        if !reader.poll(Some(Duration::ZERO), |_| true)? {
            break;
        }
        match reader.read(|_| true) {
            Ok(event) => {
                if !dispatch_termina_event(event, events) {
                    return Ok(false);
                }
            }
            Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
    }
    Ok(true)
}

pub(super) fn decode_preserved_input(bytes: &[u8]) -> Vec<RunnerEvent> {
    let mut parser = Parser::default();
    parser.parse(bytes, false);
    let mut events = Vec::new();
    while let Some(event) = parser.pop() {
        match map_termina_event(event) {
            TerminaEventAction::Input(event) => events.push(RunnerEvent::Terminal(event)),
            TerminaEventAction::Pointer(event, sub_cell) => {
                events.push(RunnerEvent::Pointer { event, sub_cell });
            }
            TerminaEventAction::ThemeRefresh => {
                events.push(RunnerEvent::HostTerminalColorRefreshRequested);
            }
            TerminaEventAction::Ignore => {}
        }
    }
    events
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
    use std::sync::atomic::AtomicUsize;
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
    fn input_interleaved_with_palette_responses_is_reinjected() {
        let events = decode_preserved_input(b"a\x1b[?997;2n\x1b[<64;2;3M");

        assert!(matches!(
            events.first(),
            Some(RunnerEvent::Terminal(CrosstermEvent::Key(event)))
                if event.code == CrosstermKeyCode::Char('a')
        ));
        assert!(matches!(
            events.get(1),
            Some(RunnerEvent::HostTerminalColorRefreshRequested)
        ));
        assert!(matches!(
            events.get(2),
            Some(RunnerEvent::Terminal(CrosstermEvent::Mouse(_)))
                | Some(RunnerEvent::Pointer {
                    event: CrosstermEvent::Mouse(_),
                    ..
                })
        ));
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
    fn replacing_reader_replaces_control_waker() {
        let old_wakes = Arc::new(AtomicUsize::new(0));
        let new_wakes = Arc::new(AtomicUsize::new(0));
        let (commands, _command_rx) = mpsc::channel();
        let old_wakes_for_callback = Arc::clone(&old_wakes);
        let control = WorkerControl {
            commands,
            wake: Mutex::new(Box::new(move || {
                old_wakes_for_callback.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })),
            paused: AtomicBool::new(false),
            worker_thread: Mutex::new(None),
        };

        control.wake();
        let new_wakes_for_callback = Arc::clone(&new_wakes);
        control.replace_wake(move || {
            new_wakes_for_callback.fetch_add(1, Ordering::SeqCst);
            Ok(())
        });
        control.wake();

        assert_eq!(old_wakes.load(Ordering::SeqCst), 1);
        assert_eq!(new_wakes.load(Ordering::SeqCst), 1);
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
