use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use super::events::key_event_to_bytes;
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
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string());

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
#[derive(Clone)]
pub struct TerminalPty {
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    killer: Arc<Mutex<Box<dyn portable_pty::ChildKiller + Send + Sync>>>,
    /// OS process id of the spawned child, captured at spawn time (`None` if unavailable).
    pid: Option<u32>,
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

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| TerminalPtyError::Reader(err.to_string()))?;
        let writer = pair
            .master
            .take_writer()
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

        let master = Arc::new(Mutex::new(pair.master));
        let writer = Arc::new(Mutex::new(writer));
        let pid = child.process_id();
        let killer = Arc::new(Mutex::new(child.clone_killer()));

        let on_event = Arc::new(on_event);

        {
            let on_event = on_event.clone();
            std::thread::spawn(move || {
                let mut reader = reader;
                let mut buffer = [0u8; 8192];
                loop {
                    match reader.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(read) => {
                            on_event(TerminalPtyEvent::Output(Arc::<[u8]>::from(
                                buffer[..read].to_vec(),
                            )));
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(err) => {
                            on_event(TerminalPtyEvent::Error(err.to_string().into()));
                            break;
                        }
                    }
                }
            });
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

        Ok(Self {
            master,
            writer,
            killer,
            pid,
        })
    }

    /// OS process id of the spawned child process, if the platform reports one.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Send raw bytes to child stdin.
    pub fn write(&self, bytes: &[u8]) -> std::io::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| std::io::Error::other("pty writer lock poisoned"))?;
        writer.write_all(bytes)?;
        writer.flush()
    }

    /// Encode key and send it to child stdin.
    pub fn send_key(&self, key: KeyEvent) -> std::io::Result<bool> {
        let Some(bytes) = key_event_to_bytes(key) else {
            return Ok(false);
        };
        self.write(&bytes)?;
        Ok(true)
    }

    /// Resize PTY dimensions.
    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        let master = self
            .master
            .lock()
            .map_err(|_| std::io::Error::other("pty master lock poisoned"))?;
        master
            .resize(PtySize {
                rows: rows.max(1),
                cols: cols.max(1),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|err| std::io::Error::other(err.to_string()))
    }

    /// Request graceful process termination.
    pub fn kill(&self) -> std::io::Result<()> {
        let mut killer = self
            .killer
            .lock()
            .map_err(|_| std::io::Error::other("pty killer lock poisoned"))?;
        killer
            .kill()
            .map_err(|err| std::io::Error::other(err.to_string()))
    }
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
        // Kill the child process to avoid leaking OS resources.
        // Ignore errors - the child may have already exited.
        let _ = self.kill();
    }
}
