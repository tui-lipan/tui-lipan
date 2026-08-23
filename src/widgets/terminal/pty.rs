#![allow(unsafe_code)]

use std::io::{Read, Write};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};
use std::thread::JoinHandle;
use std::time::Duration;

#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use super::events::{TerminalKeyModes, key_event_to_bytes};
use super::screen::TerminalCellSize;
use crate::core::event::KeyEvent;

/// PTY launch error.
#[derive(thiserror::Error, Debug)]
pub enum TerminalPtyError {
    /// Could not initialize PTY pair.
    #[error("pty initialization failed: {0}")]
    Setup(String),
    /// Could not clone master reader.
    #[error("failed to clone pty reader: {0}")]
    Reader(String),
    /// Could not acquire writer.
    #[error("failed to acquire pty writer: {0}")]
    Writer(String),
    /// Child spawn failed.
    #[error("failed to spawn pty command: {0}")]
    Spawn(String),
}

/// Resolve the generic fallback shell for [`TerminalPtyConfig::default`].
///
/// Unix: `$SHELL`, falling back to `/bin/sh` when unset/empty. Windows has no `$SHELL`
/// equivalent and `/bin/sh` does not exist, so the fallback there is `%COMSPEC%` (normally
/// `cmd.exe`), falling back to a bare `cmd.exe` lookup via `PATH` when even that is unset.
///
/// This is a last-resort generic default for library consumers that never configure a command;
/// app-level shell resolution (respecting user config, `pwsh.exe`/`powershell.exe` preference,
/// etc.) belongs to the host application, not this widget.
fn default_shell_command() -> String {
    #[cfg(windows)]
    {
        std::env::var("COMSPEC")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "cmd.exe".to_string())
    }
    #[cfg(not(windows))]
    {
        std::env::var("SHELL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string())
    }
}

#[cfg(windows)]
fn prime_conpty_cursor(writer: &mut dyn Write) -> std::io::Result<()> {
    // portable-pty 0.9 enables PSEUDOCONSOLE_INHERIT_CURSOR. Satisfy its initial DSR before
    // CreateProcessW, because another ConPTY request can otherwise wait for the cursor reply.
    writer.write_all(b"\x1b[1;1R")?;
    writer.flush()
}

#[cfg(not(windows))]
fn prime_conpty_cursor(_writer: &mut dyn Write) -> std::io::Result<()> {
    Ok(())
}

/// Environment markers that advertise an enclosing terminal multiplexer.
///
/// A child spawned into a tui-lipan terminal pane is hosted by *this* process,
/// not by whatever multiplexer launched it, so these are removed by default.
/// See [`TerminalPtyConfig::inherit_multiplexer_env`].
pub(crate) const MULTIPLEXER_ENV: [&str; 4] = ["TMUX", "TMUX_PANE", "STY", "WINDOW"];

/// Apply the configured environment to a spawn builder.
///
/// `CommandBuilder::new` seeds the child from this process's environment, so
/// anything the host inherited leaks into the pane unless removed here.
/// Removals run first so an explicitly configured value still wins.
fn configure_env(
    builder: &mut CommandBuilder,
    term: &str,
    env_remove: &[Arc<str>],
    env: &[(Arc<str>, Arc<str>)],
) {
    for key in env_remove {
        builder.env_remove(key.as_ref());
    }
    builder.env("TERM", term);
    for (key, value) in env {
        builder.env(key.as_ref(), value.as_ref());
    }
}

/// PTY spawn options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerminalPtyConfig {
    pub(crate) command: Arc<str>,
    pub(crate) args: Vec<Arc<str>>,
    pub(crate) cols: u16,
    pub(crate) rows: u16,
    pub(crate) cwd: Option<Arc<str>>,
    pub(crate) term: Arc<str>,
    pub(crate) env: Vec<(Arc<str>, Arc<str>)>,
    pub(crate) env_remove: Vec<Arc<str>>,
    pub(crate) cell: TerminalCellSize,
}

impl Default for TerminalPtyConfig {
    fn default() -> Self {
        let shell = default_shell_command();

        Self {
            command: shell.into(),
            args: Vec::new(),
            cols: 120,
            rows: 32,
            cwd: None,
            term: Arc::from("xterm-256color"),
            env: vec![(Arc::from("COLORTERM"), Arc::from("truecolor"))],
            env_remove: MULTIPLEXER_ENV.iter().copied().map(Arc::from).collect(),
            cell: TerminalCellSize::default(),
        }
    }
}

impl TerminalPtyConfig {
    /// Report `cell` as the host's cell size in the PTY's `TIOCGWINSZ` pixel fields.
    ///
    /// A program that draws pictures reads those fields to learn how many pixels a cell is worth.
    /// Pass [`host_cell_size`](crate::host_cell_size), and give the same value to
    /// [`TerminalScreen::set_cell_size`](super::TerminalScreen::set_cell_size) so both ends agree.
    pub fn cell_size(mut self, cell: TerminalCellSize) -> Self {
        self.cell = cell;
        self
    }

    /// Create config with an explicit executable.
    pub fn new(command: impl Into<Arc<str>>) -> Self {
        Self {
            command: command.into(),
            ..Self::default()
        }
    }

    /// Add one CLI argument.
    pub fn arg(mut self, arg: impl Into<Arc<str>>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Set CLI arguments.
    pub fn args<I>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = Arc<str>>,
    {
        self.args = args.into_iter().collect();
        self
    }

    /// Set initial PTY size (columns x rows).
    pub fn size(mut self, cols: u16, rows: u16) -> Self {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
        self
    }

    /// Set child process working directory.
    pub fn cwd(mut self, cwd: impl Into<Arc<str>>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set `TERM` passed to the child process.
    pub fn term(mut self, term: impl Into<Arc<str>>) -> Self {
        self.term = term.into();
        self
    }

    /// Add one environment variable.
    pub fn env(mut self, key: impl Into<Arc<str>>, value: impl Into<Arc<str>>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Remove one variable from the environment the child inherits.
    ///
    /// The child otherwise inherits this process's environment. Removals are
    /// applied before [`env`](Self::env), so setting a variable you also remove
    /// still wins.
    pub fn env_remove(mut self, key: impl Into<Arc<str>>) -> Self {
        self.env_remove.push(key.into());
        self
    }

    /// Keep the host's multiplexer markers in the child environment.
    ///
    /// By default `TMUX`, `TMUX_PANE`, `STY`, and `WINDOW` are removed: a child
    /// in this pane is hosted by this process, and a child that believes it is
    /// inside tmux behaves accordingly - wrapping its `OSC 52` clipboard writes
    /// in tmux's DCS passthrough (which this widget's parser does not unwrap)
    /// and suppressing inline-image protocols. Pass `true` only if the host
    /// genuinely wants the enclosing multiplexer to handle those.
    pub fn inherit_multiplexer_env(mut self, inherit: bool) -> Self {
        if inherit {
            self.env_remove
                .retain(|key| !MULTIPLEXER_ENV.contains(&key.as_ref()));
        } else if !MULTIPLEXER_ENV
            .iter()
            .all(|marker| self.env_remove.iter().any(|key| key.as_ref() == *marker))
        {
            for marker in MULTIPLEXER_ENV {
                if !self.env_remove.iter().any(|key| key.as_ref() == marker) {
                    self.env_remove.push(Arc::from(marker));
                }
            }
        }
        self
    }
}

/// Handle to a running PTY process.
///
/// Cloning shares the same underlying child process; see [`Drop`](#impl-Drop-for-TerminalPty) for
/// why dropping one clone must not affect the others.
pub struct TerminalPty {
    inner: Arc<TerminalPtyInner>,
}

impl Clone for TerminalPty {
    fn clone(&self) -> Self {
        // Track *logical* handles separately from `Arc::strong_count`: the reader thread and the
        // exit-wait thread each hold their own internal clone of `inner` for as long as they run,
        // so `Arc::strong_count` alone can never reach 1 while the PTY is still connected and
        // would make `Drop` unable to ever kill a live child. `handle_count` counts only
        // `TerminalPty` values a caller can see (this one, `TerminalPtyHandoff`'s keepalive,
        // etc.), independent of that internal bookkeeping.
        self.inner.handle_count.fetch_add(1, Ordering::AcqRel);
        Self {
            inner: self.inner.clone(),
        }
    }
}

/// How long the exit-wait thread lets the reader thread finish draining before it delivers
/// [`TerminalPtyEvent::Exited`] itself.
///
/// Only a backstop: on every platform the reader delivers the exit itself as soon as the stream is
/// drained, so this elapses only when the reader can neither reach end-of-stream nor observe an
/// idle master - a child that exited while a grandchild still holds the PTY open on a platform
/// without readiness polling. Generous on purpose, because expiring early is what truncates output.
const EXIT_DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Handshake that keeps [`TerminalPtyEvent::Exited`] behind the output the child already wrote.
///
/// `child.wait()` returns the moment the child dies, which is typically *before* the reader thread
/// has been scheduled to pick up the bytes still sitting in the master's buffer. Consumers
/// reasonably treat `Exited` as "this PTY is finished" and drop the handle, and dropping kills the
/// reader - so an unordered exit event silently truncates the output of any command that writes and
/// exits immediately. Whichever thread can prove the stream is drained emits the event; this pairs
/// them so it is emitted exactly once.
#[derive(Default)]
struct ExitSync {
    state: Mutex<ExitState>,
    signal: Condvar,
}

#[derive(Default)]
struct ExitState {
    /// Set once `child.wait()` has returned.
    code: Option<i32>,
    /// Set once `Exited` has been handed to the callback, so only one thread ever emits it.
    emitted: bool,
    /// Set when the PTY is being torn down (`kill`/`handoff`). Both waits below give up on it,
    /// because a deactivated reader will never reach the end of the stream to report a drain.
    stopped: bool,
}

impl ExitSync {
    /// Publish the child's status and wake whoever is waiting on it.
    fn publish(&self, code: i32) {
        if let Ok(mut state) = self.state.lock() {
            state.code = Some(code);
        }
        self.signal.notify_all();
    }

    /// Release both waits: the PTY is being torn down, so no drain is coming.
    fn stop(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.stopped = true;
        }
        self.signal.notify_all();
    }

    /// Take ownership of emitting `Exited`, if the status is known and nobody has emitted it yet.
    ///
    /// Returns the code to emit; the caller must emit it *outside* the lock, because the event
    /// callback can block on a full consumer queue.
    fn claim(&self) -> Option<i32> {
        let mut state = self.state.lock().ok()?;
        let code = state.code?;
        if state.emitted {
            return None;
        }
        state.emitted = true;
        Some(code)
    }

    /// Reader side: the stream is drained, so wait (briefly) for the status and claim it.
    fn claim_when_known(&self, timeout: Duration) -> Option<i32> {
        self.claim_when(timeout, |state| state.code.is_none())
    }

    /// Wait side: give the reader a chance to claim the exit once it has drained, then step in.
    fn claim_after_drain(&self, timeout: Duration) -> Option<i32> {
        self.claim_when(timeout, |state| !state.emitted)
    }

    fn claim_when(
        &self,
        timeout: Duration,
        mut keep_waiting: impl FnMut(&mut ExitState) -> bool,
    ) -> Option<i32> {
        let state = self.state.lock().ok()?;
        let (state, _) = self
            .signal
            .wait_timeout_while(state, timeout, |state| {
                !state.stopped && keep_waiting(state)
            })
            .ok()?;
        drop(state);
        self.claim()
    }
}

struct TerminalPtyInner {
    backend: Mutex<TerminalPtyBackend>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    killer: Mutex<Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
    /// Orders `Exited` behind the child's final output; see [`ExitSync`].
    exit: ExitSync,
    active: AtomicBool,
    kill_on_drop: AtomicBool,
    /// Number of live `TerminalPty` handles sharing this child (see [`Clone`] above).
    handle_count: AtomicUsize,
    /// OS process id of the spawned child, captured at spawn time (`None` if unavailable).
    pid: Option<u32>,
    /// Cell size last reported to the child, so a plain `resize` keeps it.
    cell: AtomicCellSize,
}

/// The cell size a PTY last reported, kept lock-free because a resize can come from any thread.
///
/// Packed as `width << 16 | height`; both axes are `u16` and neither is ever zero.
struct AtomicCellSize(AtomicU32);

impl AtomicCellSize {
    fn new(cell: TerminalCellSize) -> Self {
        let slot = Self(AtomicU32::new(0));
        slot.store(cell);
        slot
    }

    fn load(&self) -> TerminalCellSize {
        let packed = self.0.load(Ordering::Acquire);
        TerminalCellSize::new((packed >> 16) as u16, packed as u16)
    }

    fn store(&self, cell: TerminalCellSize) {
        let packed = (u32::from(cell.width) << 16) | u32::from(cell.height);
        self.0.store(packed, Ordering::Release);
    }
}

/// A PTY window size that carries the pixel dimensions a graphics-drawing child needs.
fn pty_size(cols: u16, rows: u16, cell: TerminalCellSize) -> PtySize {
    let cols = cols.max(1);
    let rows = rows.max(1);
    PtySize {
        rows,
        cols,
        pixel_width: cols.saturating_mul(cell.width),
        pixel_height: rows.saturating_mul(cell.height),
    }
}

enum TerminalPtyBackend {
    Portable(Box<dyn portable_pty::MasterPty + Send>),
}

#[cfg(unix)]
/// A live PTY master fd prepared for transfer to another process.
pub struct TerminalPtyHandoff {
    /// Raw master PTY fd kept open by this token until it is dropped.
    pub master_fd: RawFd,
    /// Child process id, if the platform reported one at spawn time.
    pub pid: Option<u32>,
    _keepalive: TerminalPty,
}

impl TerminalPty {
    /// Spawn a PTY process and stream events through `on_event`.
    pub fn spawn(
        config: TerminalPtyConfig,
        on_event: impl Fn(TerminalPtyEvent) + Send + Sync + 'static,
    ) -> Result<Self, TerminalPtyError> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(pty_size(config.cols, config.rows, config.cell))
            .map_err(|err| TerminalPtyError::Setup(err.to_string()))?;

        #[cfg(unix)]
        let reader = File::from(
            unix_dup_master_fd(&*pair.master)
                .map_err(|err| TerminalPtyError::Reader(err.to_string()))?,
        );
        #[cfg(not(unix))]
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| TerminalPtyError::Reader(err.to_string()))?;

        #[cfg(unix)]
        let mut writer = Box::new(File::from(
            unix_dup_master_fd(&*pair.master)
                .map_err(|err| TerminalPtyError::Writer(err.to_string()))?,
        )) as Box<dyn Write + Send>;
        #[cfg(not(unix))]
        let mut writer = pair
            .master
            .take_writer()
            .map_err(|err| TerminalPtyError::Writer(err.to_string()))?;

        prime_conpty_cursor(&mut *writer)
            .map_err(|err| TerminalPtyError::Writer(err.to_string()))?;

        let mut builder = CommandBuilder::new(config.command.as_ref());
        for arg in config.args {
            builder.arg(arg.as_ref());
        }
        configure_env(
            &mut builder,
            config.term.as_ref(),
            &config.env_remove,
            &config.env,
        );
        if let Some(cwd) = config.cwd {
            builder.cwd(cwd.as_ref());
        }

        let mut child = pair
            .slave
            .spawn_command(builder)
            .map_err(|err| TerminalPtyError::Spawn(err.to_string()))?;

        let pid = child.process_id();
        let inner = Arc::new(TerminalPtyInner {
            backend: Mutex::new(TerminalPtyBackend::Portable(pair.master)),
            writer: Mutex::new(Some(writer)),
            killer: Mutex::new(Some(child.clone_killer())),
            reader_thread: Mutex::new(None),
            exit: ExitSync::default(),
            active: AtomicBool::new(true),
            kill_on_drop: AtomicBool::new(true),
            handle_count: AtomicUsize::new(1),
            pid,
            cell: AtomicCellSize::new(config.cell),
        });

        let on_event = Arc::new(on_event);

        {
            let on_event = on_event.clone();
            let inner = inner.clone();
            let thread_inner = inner.clone();
            let reader_thread = std::thread::spawn(move || {
                let mut reader = reader;
                let mut buffer = [0u8; 8192];
                // Whether the loop ended because the stream itself ended, as opposed to the PTY
                // being deactivated under it. Only the former proves there is nothing left to read.
                let mut stream_ended = false;
                loop {
                    if !thread_inner.active.load(Ordering::Acquire) {
                        break;
                    }
                    #[cfg(unix)]
                    match unix_wait_readable(reader.as_raw_fd(), &thread_inner.active) {
                        PtyReadiness::Readable => {}
                        PtyReadiness::Idle => {
                            // The master has nothing pending. If the child is already gone, every
                            // byte it wrote has been delivered, so its status can be released now.
                            // Keep reading afterwards: a grandchild may still hold the PTY open.
                            if let Some(code) = thread_inner.exit.claim() {
                                on_event(TerminalPtyEvent::Exited(code));
                            }
                            continue;
                        }
                        PtyReadiness::Stop => break,
                    }
                    match reader.read(&mut buffer) {
                        Ok(0) => {
                            stream_ended = true;
                            break;
                        }
                        Ok(read) => {
                            if !thread_inner.active.load(Ordering::Acquire) {
                                break;
                            }
                            on_event(TerminalPtyEvent::Output(Arc::<[u8]>::from(
                                buffer[..read].to_vec(),
                            )));
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(err) => {
                            stream_ended = true;
                            // On Linux a PTY master read returns EIO once the slave side has been
                            // fully closed (the child exited); that is the normal end-of-stream
                            // signal for a master, not a fault. Treat it like EOF - the exit code
                            // is delivered below, not as an error.
                            #[cfg(unix)]
                            if err.raw_os_error() == Some(libc::EIO) {
                                break;
                            }
                            on_event(TerminalPtyEvent::Error(err.to_string().into()));
                            break;
                        }
                    }
                }
                // End of stream: no further output can arrive on this PTY, so deliver the exit as
                // soon as the status is known rather than making the consumer wait out the exit
                // thread's grace. This is the ordering guarantee - `Exited` is emitted from the
                // same thread as, and after, the child's last `Output`.
                if stream_ended
                    && let Some(code) = thread_inner.exit.claim_when_known(EXIT_DRAIN_GRACE)
                {
                    on_event(TerminalPtyEvent::Exited(code));
                }
            });
            if let Ok(mut slot) = inner.reader_thread.lock() {
                *slot = Some(reader_thread);
            }
        }

        {
            let on_event = on_event.clone();
            let thread_inner = inner.clone();
            std::thread::spawn(move || {
                let exit_code = child
                    .wait()
                    .ok()
                    .map(|status| status.exit_code() as i32)
                    .unwrap_or(-1);
                // Publish rather than emit: `wait` returns the instant the child dies, which is
                // usually before the reader has picked up the bytes it left behind. The reader
                // emits the exit once it has drained them; this thread only steps in when the
                // reader cannot get there (see `EXIT_DRAIN_GRACE`) or when the PTY was killed.
                thread_inner.exit.publish(exit_code);
                if let Some(code) = thread_inner.exit.claim_after_drain(EXIT_DRAIN_GRACE) {
                    on_event(TerminalPtyEvent::Exited(code));
                }
            });
        }

        Ok(Self { inner })
    }

    #[cfg(unix)]
    /// Prepare this PTY for transfer to another process.
    pub fn handoff(&self) -> std::io::Result<TerminalPtyHandoff> {
        self.inner.active.store(false, Ordering::Release);
        // The reader is about to stop without reaching end of stream, so nothing will ever report
        // a drain; release the exit waits so joining it below cannot stall on the drain grace.
        self.inner.exit.stop();
        self.inner.kill_on_drop.store(false, Ordering::Release);
        if let Some(handle) = self
            .inner
            .reader_thread
            .lock()
            .map_err(|_| std::io::Error::other("pty reader thread lock poisoned"))?
            .take()
        {
            let _ = handle.join();
        }
        let mut writer = self
            .inner
            .writer
            .lock()
            .map_err(|_| std::io::Error::other("pty writer lock poisoned"))?;
        writer.take();
        drop(writer);

        let backend = self
            .inner
            .backend
            .lock()
            .map_err(|_| std::io::Error::other("pty master lock poisoned"))?;
        let fd = match &*backend {
            TerminalPtyBackend::Portable(master) => master
                .as_raw_fd()
                .ok_or_else(|| std::io::Error::other("pty master fd unavailable"))?,
        };
        Ok(TerminalPtyHandoff {
            master_fd: fd,
            pid: self.inner.pid,
            _keepalive: self.clone(),
        })
    }

    /// OS process id of the spawned child process, if the platform reports one.
    pub fn pid(&self) -> Option<u32> {
        self.inner.pid
    }

    #[cfg(unix)]
    /// Foreground process-group id currently attached to this PTY (`tcgetpgrp(3)`).
    ///
    /// This is the building block a Linux/macOS foreground-executable fallback needs (e.g. to
    /// resolve which process a shell handed the terminal to) without exposing the underlying
    /// master file descriptor to callers. Returns `None` once the PTY has been killed or handed
    /// off, or if the ioctl fails (e.g. no foreground group is currently set).
    pub fn foreground_process_group_id(&self) -> Option<i32> {
        if !self.inner.active.load(Ordering::Acquire) {
            return None;
        }
        let backend = self.inner.backend.lock().ok()?;
        let fd = match &*backend {
            TerminalPtyBackend::Portable(master) => master.as_raw_fd()?,
        };
        let pgid = unsafe { libc::tcgetpgrp(fd) };
        (pgid >= 0).then_some(pgid)
    }

    /// Send raw bytes to child stdin.
    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        if !self.inner.active.load(Ordering::Acquire) {
            return Err(std::io::Error::other("pty has been handed off"));
        }
        let mut writer = self
            .inner
            .writer
            .lock()
            .map_err(|_| std::io::Error::other("pty writer lock poisoned"))?;
        let writer = writer
            .as_mut()
            .ok_or_else(|| std::io::Error::other("pty writer unavailable"))?;
        writer.write_all(bytes)?;
        writer.flush()
    }

    /// Encode key and send it to child stdin.
    ///
    /// Pass the modes the child has enabled, from `TerminalScreen::key_modes()`. Returns `false`
    /// when the key has no terminal encoding and nothing was written.
    pub fn send_key(&self, key: KeyEvent, modes: TerminalKeyModes) -> std::io::Result<bool> {
        let Some(bytes) = key_event_to_bytes(key, modes) else {
            return Ok(false);
        };
        self.write(&bytes)?;
        Ok(true)
    }

    /// Resize PTY dimensions, keeping the cell size the child was last told.
    ///
    /// Equivalent to [`resize_with_cell_size`](Self::resize_with_cell_size) with the size from
    /// [`TerminalPtyConfig::cell_size`].
    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        self.resize_with_cell_size(cols, rows, self.inner.cell.load())
    }

    /// Resize PTY dimensions and report the host's cell size in pixels.
    ///
    /// The pixel fields of `TIOCGWINSZ` are how a program that draws pictures learns how big a
    /// cell is; a terminal that leaves them zero forces it to guess or to fall back to `CSI 14 t`.
    /// Pass the same size the screen was given through
    /// [`TerminalScreen::set_cell_size`](super::TerminalScreen::set_cell_size).
    pub fn resize_with_cell_size(
        &self,
        cols: u16,
        rows: u16,
        cell: TerminalCellSize,
    ) -> std::io::Result<()> {
        if !self.inner.active.load(Ordering::Acquire) {
            return Err(std::io::Error::other("pty has been handed off"));
        }
        let backend = self
            .inner
            .backend
            .lock()
            .map_err(|_| std::io::Error::other("pty master lock poisoned"))?;
        match &*backend {
            TerminalPtyBackend::Portable(master) => {
                self.inner.cell.store(cell);
                master
                    .resize(pty_size(cols, rows, cell))
                    .map_err(|err| std::io::Error::other(err.to_string()))
            }
        }
    }

    /// Request graceful process termination.
    pub fn kill(&self) -> std::io::Result<()> {
        if !self.inner.kill_on_drop.load(Ordering::Acquire) {
            return Ok(());
        }
        self.inner.active.store(false, Ordering::Release);
        // An explicit kill stops the reader mid-stream, so the exit must not wait for a drain that
        // will never be reported - it is delivered as soon as the child is reaped.
        self.inner.exit.stop();
        let mut killer = self
            .inner
            .killer
            .lock()
            .map_err(|_| std::io::Error::other("pty killer lock poisoned"))?;
        if let Some(killer) = killer.as_mut() {
            return killer
                .kill()
                .map_err(|err| std::io::Error::other(err.to_string()));
        }
        Ok(())
    }
}

#[cfg(unix)]
fn unix_dup_master_fd(master: &dyn portable_pty::MasterPty) -> std::io::Result<OwnedFd> {
    let fd = master
        .as_raw_fd()
        .ok_or_else(|| std::io::Error::other("pty master fd unavailable"))?;
    unix_dup_raw_fd(fd)
}

#[cfg(unix)]
fn unix_dup_raw_fd(fd: RawFd) -> std::io::Result<OwnedFd> {
    let dup = unsafe { libc::fcntl(fd, libc::F_DUPFD_CLOEXEC, 0) };
    if dup < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(dup) })
}

/// What [`unix_wait_readable`] observed about the master.
#[cfg(unix)]
enum PtyReadiness {
    /// Data (or a hangup) is pending; read it.
    Readable,
    /// Nothing is pending right now - which, once the child has exited, means fully drained.
    Idle,
    /// The PTY was deactivated, or the master can no longer be polled.
    Stop,
}

#[cfg(unix)]
fn unix_wait_readable(fd: RawFd, active: &AtomicBool) -> PtyReadiness {
    if !active.load(Ordering::Acquire) {
        return PtyReadiness::Stop;
    }
    let mut pollfd = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let rc = unsafe { libc::poll(&mut pollfd, 1, 100) };
    if rc > 0 {
        if pollfd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0 {
            return PtyReadiness::Readable;
        }
        // `POLLNVAL` means the master is no longer a valid descriptor: it would be signalled again
        // immediately, so reporting it as idle would spin.
        if pollfd.revents & libc::POLLNVAL != 0 {
            return PtyReadiness::Stop;
        }
        return PtyReadiness::Idle;
    }
    if rc == 0 {
        return PtyReadiness::Idle;
    }
    if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
        return PtyReadiness::Idle;
    }
    PtyReadiness::Stop
}

/// PTY runtime event.
///
/// `Exited` is delivered after every `Output` the child produced, so a consumer can treat it as
/// "this PTY is finished" and drop the handle without losing bytes still in flight. An explicit
/// [`TerminalPty::kill`] is the exception: it stops the reader deliberately, so anything buffered
/// at that moment is discarded and the exit is reported as soon as the child is reaped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalPtyEvent {
    /// Raw bytes emitted by PTY stdout/stderr stream.
    Output(Arc<[u8]>),
    /// Child process exited with status code (or -1 when unavailable).
    Exited(i32),
    /// Runtime error message.
    Error(Arc<str>),
}

impl Drop for TerminalPty {
    fn drop(&mut self) {
        // Only kill the child when *this* drop removes the last outstanding logical handle
        // (`handle_count`, not `Arc::strong_count` - see the `Clone` impl above for why).
        if self.inner.handle_count.fetch_sub(1, Ordering::AcqRel) == 1 {
            // Ignore errors - the child may have already exited.
            let _ = self.kill();
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn removes(config: &TerminalPtyConfig, key: &str) -> bool {
        config.env_remove.iter().any(|k| k.as_ref() == key)
    }

    #[test]
    fn multiplexer_markers_are_scrubbed_by_default() {
        // `CommandBuilder::new` seeds the child from this process's environment,
        // so a host launched under tmux would otherwise tell every pane child it
        // is inside tmux.
        let config = TerminalPtyConfig::new("/bin/sh");
        for marker in MULTIPLEXER_ENV {
            assert!(removes(&config, marker), "{marker} should be removed");
        }
    }

    #[test]
    fn inherit_multiplexer_env_keeps_the_markers() {
        let config = TerminalPtyConfig::new("/bin/sh").inherit_multiplexer_env(true);
        for marker in MULTIPLEXER_ENV {
            assert!(!removes(&config, marker), "{marker} should be kept");
        }
    }

    #[test]
    fn inherit_multiplexer_env_round_trips() {
        let config = TerminalPtyConfig::new("/bin/sh")
            .inherit_multiplexer_env(true)
            .inherit_multiplexer_env(false);
        for marker in MULTIPLEXER_ENV {
            assert!(removes(&config, marker), "{marker} should be removed again");
        }
        // Re-enabling must not duplicate entries.
        assert_eq!(config.env_remove.len(), MULTIPLEXER_ENV.len());
    }

    #[test]
    fn inherit_multiplexer_env_preserves_custom_removals() {
        let config = TerminalPtyConfig::new("/bin/sh")
            .env_remove("SECRET_TOKEN")
            .inherit_multiplexer_env(true);
        assert!(removes(&config, "SECRET_TOKEN"));
        assert!(!removes(&config, "TMUX"));
    }

    #[test]
    fn env_remove_is_additive() {
        let config = TerminalPtyConfig::new("/bin/sh").env_remove("SECRET_TOKEN");
        assert!(removes(&config, "SECRET_TOKEN"));
        assert!(removes(&config, "TMUX"), "defaults are kept");
    }

    #[test]
    fn configure_env_strips_inherited_markers_from_the_builder() {
        // Seed the marker on the builder rather than on this process: mutating
        // the real environment would race the other tests in this binary.
        let mut builder = CommandBuilder::new("/bin/sh");
        builder.env("TMUX", "/tmp/fake,1,0");
        builder.env("STY", "1234.pts-0.host");

        let config = TerminalPtyConfig::new("/bin/sh");
        configure_env(
            &mut builder,
            "xterm-256color",
            &config.env_remove,
            &config.env,
        );

        assert_eq!(builder.get_env("TMUX"), None);
        assert_eq!(builder.get_env("STY"), None);
        assert_eq!(
            builder.get_env("TERM"),
            Some(std::ffi::OsStr::new("xterm-256color"))
        );
    }

    #[test]
    fn configure_env_lets_an_explicit_value_win_over_a_removal() {
        let mut builder = CommandBuilder::new("/bin/sh");
        let config = TerminalPtyConfig::new("/bin/sh").env("TMUX", "kept");

        configure_env(
            &mut builder,
            "xterm-256color",
            &config.env_remove,
            &config.env,
        );

        assert_eq!(builder.get_env("TMUX"), Some(std::ffi::OsStr::new("kept")));
    }

    #[test]
    fn dropping_a_clone_does_not_kill_the_shared_pty() {
        let pty = TerminalPty::spawn(
            TerminalPtyConfig::new("/bin/sh").arg("-c").arg("sleep 5"),
            |_event| {},
        )
        .expect("spawn");

        let clone = pty.clone();
        assert_eq!(pty.inner.handle_count.load(Ordering::Acquire), 2);
        drop(clone);
        assert_eq!(pty.inner.handle_count.load(Ordering::Acquire), 1);

        // Before the fix, dropping any clone unconditionally killed the child; this must not
        // happen while another handle (`pty`) is still alive.
        assert!(
            pty.write(b"").is_ok(),
            "pty should still be alive after dropping a clone"
        );

        drop(pty);
    }

    /// A consumer that treats `Exited` as "this PTY is done" and drops the handle must still have
    /// been given everything the child wrote. Dropping kills the reader, so an exit event emitted
    /// ahead of the reader used to discard whatever was still sitting in the master's buffer: a
    /// command that wrote and exited immediately could lose its output entirely.
    #[test]
    fn a_fast_command_s_output_arrives_before_its_exit() {
        // The race needs the child to write and exit in one breath, so retry: a single run that
        // happens to schedule the reader first would pass either way.
        for attempt in 0..40 {
            let events = Arc::new(Mutex::new(Vec::new()));
            let sink = events.clone();
            let mut pty = Some(
                TerminalPty::spawn(
                    TerminalPtyConfig::new("/bin/sh")
                        .arg("-c")
                        .arg("printf 'fast output\\n'; exit 3"),
                    move |event| sink.lock().expect("events").push(event),
                )
                .expect("spawn"),
            );

            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            while std::time::Instant::now() < deadline {
                let exited = events
                    .lock()
                    .expect("events")
                    .iter()
                    .any(|event| matches!(event, TerminalPtyEvent::Exited(_)));
                if exited {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            // Exactly what a consumer does on exit, and exactly what used to truncate the output.
            drop(pty.take());

            let events = events.lock().expect("events");
            let text: String = events
                .iter()
                .filter_map(|event| match event {
                    TerminalPtyEvent::Output(bytes) => Some(String::from_utf8_lossy(bytes)),
                    _ => None,
                })
                .collect();
            assert!(
                text.contains("fast output"),
                "attempt {attempt}: the command's output was lost; events: {events:?}"
            );
            assert!(
                matches!(events.last(), Some(TerminalPtyEvent::Exited(3))),
                "attempt {attempt}: the exit must come last and carry the real status; \
                 events: {events:?}"
            );
        }
    }

    /// The exit event must still be delivered promptly when the reader cannot observe the end of
    /// the stream, rather than waiting out the drain grace or being lost.
    #[test]
    fn killing_a_pty_reports_the_exit_without_waiting_for_a_drain() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let pty = TerminalPty::spawn(
            TerminalPtyConfig::new("/bin/sh").arg("-c").arg("sleep 30"),
            move |event| sink.lock().expect("events").push(event),
        )
        .expect("spawn");

        let started = std::time::Instant::now();
        pty.kill().expect("kill");
        let deadline = started + EXIT_DRAIN_GRACE;
        while std::time::Instant::now() < deadline {
            if events
                .lock()
                .expect("events")
                .iter()
                .any(|event| matches!(event, TerminalPtyEvent::Exited(_)))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("a killed pty must report its exit without waiting out the drain grace");
    }

    #[test]
    fn foreground_process_group_id_reports_a_value_while_alive() {
        let pty = TerminalPty::spawn(
            TerminalPtyConfig::new("/bin/sh").arg("-c").arg("sleep 5"),
            |_event| {},
        )
        .expect("spawn");

        // The freshly spawned shell is its own foreground process group.
        assert!(pty.foreground_process_group_id().is_some());

        drop(pty);
    }
}
