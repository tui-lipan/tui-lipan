use std::io::{self, Write};
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
use termina::escape::osc::{ColorOrQuery, DynamicColorNumber, Osc};
use termina::event::{
    Event as TerminaEvent, KeyCode as TerminaKeyCode, KeyEvent as TerminaKeyEvent,
    KeyEventKind as TerminaKeyEventKind, KeyEventState as TerminaKeyEventState,
    MediaKeyCode as TerminaMediaKeyCode, ModifierKeyCode as TerminaModifierKeyCode,
    Modifiers as TerminaModifiers, MouseButton as TerminaMouseButton,
    MouseEvent as TerminaMouseEvent, MouseEventKind as TerminaMouseEventKind,
};
use termina::{EventReader, PlatformTerminal, Terminal as _};
use web_time::Instant;

use crate::backend::ratatui_backend::terminal_handoff::{
    InputHandoffControl, InputHandoffSlot, register_input_handoff_control,
    unregister_input_handoff_control,
};
use crate::style::{Color, HostTerminalColors};

use super::RunnerEvent;

const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(100);
const COLOR_QUERY_TIMEOUT: Duration = Duration::from_millis(200);
const MAX_COALESCED_COLOR_QUERIES: usize = 4;

pub(super) struct TerminaInputCoordinator {
    events: mpsc::Receiver<RunnerEvent>,
    control: Arc<WorkerControl>,
    worker: Option<JoinHandle<()>>,
    panic_control: InputHandoffSlot,
}

impl TerminaInputCoordinator {
    pub(super) fn start(
        initial_colors: Option<HostTerminalColors>,
        panic_control: InputHandoffSlot,
    ) -> io::Result<Self> {
        let reader = open_event_reader()?;
        let waker = reader.waker();
        let (command_tx, command_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let control = Arc::new(WorkerControl {
            commands: command_tx,
            wake: Mutex::new(Box::new(move || waker.wake())),
            refresh: RefreshRequests::default(),
            paused: AtomicBool::new(false),
            worker_thread: Mutex::new(None),
        });
        let worker_control = Arc::clone(&control);
        let panic_events = event_tx.clone();
        let worker = std::thread::Builder::new()
            .name("termina-reader".into())
            .spawn(move || {
                contain_worker_panic(&panic_events, || {
                    run_worker(reader, command_rx, event_tx, worker_control, initial_colors);
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
            control,
            worker: Some(worker),
            panic_control,
        })
    }

    pub(super) fn receiver(&self) -> &mpsc::Receiver<RunnerEvent> {
        &self.events
    }

    pub(super) fn request_host_color_refresh(&self) {
        self.control.request_refresh();
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
    Shutdown,
}

struct WorkerControl {
    commands: mpsc::Sender<WorkerCommand>,
    wake: Mutex<Box<dyn Fn() -> io::Result<()> + Send + Sync>>,
    refresh: RefreshRequests,
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

    fn request_refresh(&self) {
        if self.refresh.request() {
            self.wake();
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

#[derive(Default)]
struct RefreshRequests(AtomicBool);

impl RefreshRequests {
    fn request(&self) -> bool {
        !self.0.swap(true, Ordering::SeqCst)
    }

    fn take(&self) -> bool {
        self.0.swap(false, Ordering::SeqCst)
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
    Ok(terminal.event_reader())
}

fn run_worker(
    mut reader: EventReader,
    commands: mpsc::Receiver<WorkerCommand>,
    events: mpsc::Sender<RunnerEvent>,
    control: Arc<WorkerControl>,
    mut last_colors: Option<HostTerminalColors>,
) {
    if let Ok(mut worker_thread) = control.worker_thread.lock() {
        *worker_thread = Some(std::thread::current().id());
    }
    loop {
        match process_worker_commands(&mut reader, &commands, &control) {
            Ok(true) => {}
            Ok(false) => break,
            Err(err) => {
                let _ = events.send(RunnerEvent::InputError(err.to_string()));
                break;
            }
        }

        if control.refresh.take() {
            match coalesced_host_color_refresh(&control.refresh, || {
                let colors =
                    query_host_colors(&reader, &events, &control.refresh, last_colors.as_ref())?;
                if let Some(colors) = colors {
                    last_colors = Some(colors);
                }
                Ok(colors)
            }) {
                Ok(Some(colors)) => {
                    if events
                        .send(RunnerEvent::HostTerminalColors(colors))
                        .is_err()
                    {
                        break;
                    }
                }
                Ok(None) => {}
                Err(err) => {
                    let _ = events.send(RunnerEvent::InputError(err.to_string()));
                    break;
                }
            }
            continue;
        }

        match reader.poll(Some(INPUT_POLL_INTERVAL), |_| true) {
            Ok(true) => match reader.read(|_| true) {
                Ok(event) => {
                    if !dispatch_termina_event(event, &events, &control.refresh) {
                        break;
                    }
                }
                Err(err) => {
                    let _ = events.send(RunnerEvent::InputError(err.to_string()));
                    break;
                }
            },
            Ok(false) => {}
            Err(err) => {
                let _ = events.send(RunnerEvent::InputError(err.to_string()));
                break;
            }
        }
    }
}

fn coalesced_host_color_refresh(
    refresh: &RefreshRequests,
    mut query: impl FnMut() -> io::Result<Option<HostTerminalColors>>,
) -> io::Result<Option<HostTerminalColors>> {
    let mut latest = None;
    for attempt in 0..MAX_COALESCED_COLOR_QUERIES {
        let result = query()?;
        if let Some(colors) = result {
            latest = Some(colors);
        }
        let retry_requested = refresh.take();
        if result.is_none() {
            if retry_requested {
                refresh.request();
            }
            break;
        }
        if !retry_requested {
            break;
        }
        if attempt + 1 == MAX_COALESCED_COLOR_QUERIES {
            refresh.request();
            break;
        }
    }
    Ok(latest)
}

fn process_worker_commands(
    reader: &mut EventReader,
    commands: &mpsc::Receiver<WorkerCommand>,
    control: &WorkerControl,
) -> io::Result<bool> {
    while let Ok(command) = commands.try_recv() {
        match command {
            WorkerCommand::Pause(ack) => {
                control.paused.store(true, Ordering::SeqCst);
                let _ = ack.send(());
                loop {
                    match commands.recv().map_err(|_| worker_stopped())? {
                        WorkerCommand::Resume(ack) => match open_event_reader() {
                            Ok(new_reader) => {
                                let waker = new_reader.waker();
                                *reader = new_reader;
                                control.replace_wake(move || waker.wake());
                                control.paused.store(false, Ordering::SeqCst);
                                control.refresh.request();
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
            WorkerCommand::Shutdown => return Ok(false),
        }
    }
    Ok(true)
}

fn query_host_colors(
    reader: &EventReader,
    events: &mpsc::Sender<RunnerEvent>,
    refresh: &RefreshRequests,
    previous: Option<&HostTerminalColors>,
) -> io::Result<Option<HostTerminalColors>> {
    let foreground_query = Osc::ChangeDynamicColors(
        DynamicColorNumber::TextForegroundColor,
        vec![ColorOrQuery::Query],
    );
    let background_query = Osc::ChangeDynamicColors(
        DynamicColorNumber::TextBackgroundColor,
        vec![ColorOrQuery::Query],
    );
    let mut output = io::stdout().lock();
    write!(output, "{foreground_query}{background_query}")?;
    output.flush()?;
    drop(output);

    let mut foreground = None;
    let mut background = None;
    let deadline = Instant::now() + COLOR_QUERY_TIMEOUT;
    while Instant::now() < deadline && (foreground.is_none() || background.is_none()) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if !reader.poll(Some(remaining), |_| true)? {
            break;
        }
        let event = reader.read(|_| true)?;
        if let Some((slot, color)) = dynamic_color_response(&event) {
            match slot {
                DynamicColorNumber::TextForegroundColor => foreground = Some(color),
                DynamicColorNumber::TextBackgroundColor => background = Some(color),
                _ => {}
            }
            continue;
        }
        if !dispatch_termina_event(event, events, refresh) {
            return Ok(None);
        }
    }

    let (Some(foreground), Some(background)) = (foreground, background) else {
        return Ok(None);
    };
    Ok(Some(host_terminal_colors(previous, foreground, background)))
}

fn host_terminal_colors(
    previous: Option<&HostTerminalColors>,
    foreground: termina::style::RgbColor,
    background: termina::style::RgbColor,
) -> HostTerminalColors {
    HostTerminalColors {
        // Termina 0.3 parses OSC 10/11 but not OSC 4. Keep the last resolved
        // ANSI palette rather than degrading app-owned truecolor tokens to
        // unresolved indices after the first runtime refresh.
        ansi: previous.map_or_else(resolved_default_ansi, |colors| colors.ansi),
        fg: Color::Rgb(foreground.red, foreground.green, foreground.blue),
        bg: Color::Rgb(background.red, background.green, background.blue),
    }
}

fn resolved_default_ansi() -> [Color; 16] {
    std::array::from_fn(|index| {
        let (red, green, blue) = Color::Indexed(index as u8).to_rgb().unwrap_or((0, 0, 0));
        Color::Rgb(red, green, blue)
    })
}

fn dynamic_color_response(
    event: &TerminaEvent,
) -> Option<(DynamicColorNumber, termina::style::RgbColor)> {
    let TerminaEvent::Osc(Osc::ChangeDynamicColors(slot, colors)) = event else {
        return None;
    };
    let color = colors.iter().find_map(|color| match color {
        ColorOrQuery::Color(color) => Some(*color),
        ColorOrQuery::Query => None,
    })?;
    Some((*slot, color))
}

#[derive(Debug, PartialEq, Eq)]
enum TerminaEventAction {
    Input(CrosstermEvent),
    ThemeRefresh,
    Ignore,
}

fn dispatch_termina_event(
    event: TerminaEvent,
    events: &mpsc::Sender<RunnerEvent>,
    refresh: &RefreshRequests,
) -> bool {
    match map_termina_event(event) {
        TerminaEventAction::Input(event) => events.send(RunnerEvent::Terminal(event)).is_ok(),
        TerminaEventAction::ThemeRefresh => {
            refresh.request();
            true
        }
        TerminaEventAction::Ignore => true,
    }
}

fn map_termina_event(event: TerminaEvent) -> TerminaEventAction {
    match event {
        TerminaEvent::Key(key) => {
            TerminaEventAction::Input(CrosstermEvent::Key(map_key_event(key)))
        }
        TerminaEvent::Mouse(mouse) => {
            TerminaEventAction::Input(CrosstermEvent::Mouse(map_mouse_event(mouse)))
        }
        TerminaEvent::WindowResized(size) => {
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
        map_key_code(key.code),
        map_modifiers(key.modifiers),
        match key.kind {
            TerminaKeyEventKind::Press => CrosstermKeyEventKind::Press,
            TerminaKeyEventKind::Release => CrosstermKeyEventKind::Release,
            TerminaKeyEventKind::Repeat => CrosstermKeyEventKind::Repeat,
        },
        state,
    )
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
    use termina::Parser;
    use termina::escape::csi::ThemeMode;
    use termina::event::KeyEventState;

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
    fn typed_dynamic_color_responses_are_recognized() {
        let foreground = parsed_event(b"\x1b]10;rgb:ffff/0000/8080\x1b\\");
        let (slot, color) = dynamic_color_response(&foreground).expect("typed foreground response");
        assert_eq!(slot, DynamicColorNumber::TextForegroundColor);
        assert_eq!((color.red, color.green, color.blue), (255, 0, 128));

        let mut previous = HostTerminalColors {
            ansi: resolved_default_ansi(),
            fg: Color::Rgb(9, 9, 9),
            bg: Color::Rgb(8, 8, 8),
        };
        previous.ansi[5] = Color::Rgb(17, 34, 51);
        let colors = host_terminal_colors(
            Some(&previous),
            color,
            termina::style::RgbColor::new(1, 2, 3),
        );
        assert_eq!(colors.fg, Color::Rgb(255, 0, 128));
        assert_eq!(colors.bg, Color::Rgb(1, 2, 3));
        assert_eq!(colors.ansi, previous.ansi);
        assert_eq!(colors.ansi[5], Color::Rgb(17, 34, 51));
        assert!(
            colors
                .ansi
                .iter()
                .all(|color| matches!(color, Color::Rgb(..)))
        );
    }

    #[test]
    fn refresh_requests_coalesce_until_taken() {
        let requests = RefreshRequests::default();
        assert!(requests.request());
        assert!(!requests.request());
        assert!(requests.take());
        assert!(!requests.take());
        assert!(requests.request());
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
            refresh: RefreshRequests::default(),
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
    fn coalesced_refresh_delivers_only_latest_successful_query() {
        let requests = RefreshRequests::default();
        let first = host_terminal_colors(
            None,
            termina::style::RgbColor::new(10, 20, 30),
            termina::style::RgbColor::new(1, 2, 3),
        );
        let second = host_terminal_colors(
            Some(&first),
            termina::style::RgbColor::new(40, 50, 60),
            termina::style::RgbColor::new(4, 5, 6),
        );
        let mut calls = 0;

        let colors = coalesced_host_color_refresh(&requests, || {
            calls += 1;
            if calls == 1 {
                requests.request();
                Ok(Some(first))
            } else {
                Ok(Some(second))
            }
        })
        .unwrap();

        assert_eq!(calls, 2);
        assert_eq!(colors, Some(second));
        assert!(!requests.take());
    }

    #[test]
    fn coalesced_refresh_retries_are_bounded() {
        let requests = RefreshRequests::default();
        let mut calls = 0;

        let colors = coalesced_host_color_refresh(&requests, || {
            calls += 1;
            requests.request();
            Ok(Some(host_terminal_colors(
                None,
                termina::style::RgbColor::new(calls as u8, 0, 0),
                termina::style::RgbColor::new(0, calls as u8, 0),
            )))
        })
        .unwrap()
        .expect("the final bounded query should be retained");

        assert_eq!(calls, MAX_COALESCED_COLOR_QUERIES);
        assert_eq!(
            colors.fg,
            Color::Rgb(MAX_COALESCED_COLOR_QUERIES as u8, 0, 0)
        );
        assert!(requests.take());
    }

    #[test]
    fn failed_coalesced_refresh_keeps_racing_request_pending() {
        let requests = RefreshRequests::default();

        let colors = coalesced_host_color_refresh(&requests, || {
            requests.request();
            Ok(None)
        })
        .unwrap();

        assert_eq!(colors, None);
        assert!(requests.take());
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
}
