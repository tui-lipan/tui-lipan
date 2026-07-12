#![allow(unsafe_code)]

use std::io::{Read, Write};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::thread::JoinHandle;

#[cfg(unix)]
use std::fs::File;
#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use super::events::{TerminalKeyModes, key_event_to_bytes};
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
        }
    }
}

impl TerminalPtyConfig {
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

struct TerminalPtyInner {
    backend: Mutex<TerminalPtyBackend>,
    writer: Mutex<Option<Box<dyn Write + Send>>>,
    killer: Mutex<Option<Box<dyn portable_pty::ChildKiller + Send + Sync>>>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
    active: AtomicBool,
    kill_on_drop: AtomicBool,
    /// Number of live `TerminalPty` handles sharing this child (see [`Clone`] above).
    handle_count: AtomicUsize,
    /// OS process id of the spawned child, captured at spawn time (`None` if unavailable).
    pid: Option<u32>,
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
            .openpty(PtySize {
                rows: config.rows,
                cols: config.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
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
        builder.env("TERM", config.term.as_ref());
        if let Some(cwd) = config.cwd {
            builder.cwd(cwd.as_ref());
        }
        for (key, value) in config.env {
            builder.env(key.as_ref(), value.as_ref());
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
            active: AtomicBool::new(true),
            kill_on_drop: AtomicBool::new(true),
            handle_count: AtomicUsize::new(1),
            pid,
        });

        let on_event = Arc::new(on_event);

        {
            let on_event = on_event.clone();
            let inner = inner.clone();
            let thread_inner = inner.clone();
            let reader_thread = std::thread::spawn(move || {
                let mut reader = reader;
                let mut buffer = [0u8; 8192];
                loop {
                    if !thread_inner.active.load(Ordering::Acquire) {
                        break;
                    }
                    #[cfg(unix)]
                    if !unix_wait_readable(reader.as_raw_fd(), &thread_inner.active) {
                        break;
                    }
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
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
                            // On Linux a PTY master read returns EIO once the slave side has been
                            // fully closed (the child exited); that is the normal end-of-stream
                            // signal for a master, not a fault. Treat it like EOF and let the wait
                            // thread deliver the real exit code instead of a spurious error.
                            #[cfg(unix)]
                            if err.raw_os_error() == Some(libc::EIO) {
                                break;
                            }
                            on_event(TerminalPtyEvent::Error(err.to_string().into()));
                            break;
                        }
                    }
                }
            });
            if let Ok(mut slot) = inner.reader_thread.lock() {
                *slot = Some(reader_thread);
            }
        }

        {
            let on_event = on_event.clone();
            std::thread::spawn(move || {
                let exit_code = child
                    .wait()
                    .ok()
                    .map(|status| status.exit_code() as i32)
                    .unwrap_or(-1);
                on_event(TerminalPtyEvent::Exited(exit_code));
            });
        }

        Ok(Self { inner })
    }

    #[cfg(unix)]
    /// Prepare this PTY for transfer to another process.
    pub fn handoff(&self) -> std::io::Result<TerminalPtyHandoff> {
        self.inner.active.store(false, Ordering::Release);
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

    /// Resize PTY dimensions.
    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        if !self.inner.active.load(Ordering::Acquire) {
            return Err(std::io::Error::other("pty has been handed off"));
        }
        let backend = self
            .inner
            .backend
            .lock()
            .map_err(|_| std::io::Error::other("pty master lock poisoned"))?;
        match &*backend {
            TerminalPtyBackend::Portable(master) => master
                .resize(PtySize {
                    rows: rows.max(1),
                    cols: cols.max(1),
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .map_err(|err| std::io::Error::other(err.to_string())),
        }
    }

    /// Request graceful process termination.
    pub fn kill(&self) -> std::io::Result<()> {
        if !self.inner.kill_on_drop.load(Ordering::Acquire) {
            return Ok(());
        }
        self.inner.active.store(false, Ordering::Release);
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

#[cfg(unix)]
fn unix_wait_readable(fd: RawFd, active: &AtomicBool) -> bool {
    while active.load(Ordering::Acquire) {
        let mut pollfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let rc = unsafe { libc::poll(&mut pollfd, 1, 100) };
        if rc > 0 {
            return pollfd.revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR) != 0;
        }
        if rc == 0 {
            continue;
        }
        if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return false;
        }
    }
    false
}

/// PTY runtime event.
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
