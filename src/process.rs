//! Native-only helpers for streaming child-process output into components.
//!
//! This module is available only on non-wasm targets. It is intended for
//! non-interactive subprocesses whose stdout/stderr should be consumed by the
//! TUI while it keeps running. Interactive programs that need the real terminal
//! should use [`crate::terminal_handoff`] instead.

use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, ExitStatus, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crate::{Command, TaskPolicy};

/// Description of a native child process to run and stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessSpec {
    program: Arc<str>,
    args: Vec<Arc<str>>,
    cwd: Option<PathBuf>,
    env: Vec<(Arc<str>, Arc<str>)>,
    stdin: Option<Vec<u8>>,
}

impl ProcessSpec {
    /// Create a process spec for `program`.
    pub fn new(program: impl Into<Arc<str>>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            stdin: None,
        }
    }

    /// Append a single argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<Arc<str>>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Append multiple arguments.
    #[must_use]
    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<Arc<str>>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Set the child process working directory.
    #[must_use]
    pub fn cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    /// Set an environment variable for the child process.
    #[must_use]
    pub fn env(mut self, key: impl Into<Arc<str>>, value: impl Into<Arc<str>>) -> Self {
        self.env.push((key.into(), value.into()));
        self
    }

    /// Provide bytes to write to the child process stdin, then close stdin.
    #[must_use]
    pub fn stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(stdin.into());
        self
    }

    /// Program executable name or path.
    pub fn program(&self) -> &str {
        &self.program
    }

    /// Process arguments.
    pub fn args_slice(&self) -> &[Arc<str>] {
        &self.args
    }

    /// Working directory, when configured.
    pub fn cwd_path(&self) -> Option<&Path> {
        self.cwd.as_deref()
    }

    /// Environment overrides.
    pub fn env_slice(&self) -> &[(Arc<str>, Arc<str>)] {
        &self.env
    }

    /// Bytes that will be written to stdin, when configured.
    pub fn stdin_bytes(&self) -> Option<&[u8]> {
        self.stdin.as_deref()
    }

    /// Run the process on the current thread and emit streaming events.
    ///
    /// Stdout and stderr are drained on separate helper threads so a child that
    /// writes heavily to both streams cannot deadlock on a full pipe.
    pub fn stream(self, emit: impl FnMut(ProcessEvent)) -> io::Result<()> {
        stream_process(self, emit)
    }

    /// Create an unkeyed background [`Command`] that streams process events as messages.
    ///
    /// Unkeyed commands do not have a normal runtime coalescing/cancellation
    /// owner. Use [`Self::command_keyed`] when newer work should cancel an
    /// active process.
    pub fn command<Msg, F>(self, map: F) -> Command
    where
        Msg: Send + 'static,
        F: Fn(ProcessEvent) -> Msg + Send + 'static,
    {
        process_command(self, map)
    }

    /// Create a keyed background [`Command`] that streams process events as messages.
    ///
    /// With [`TaskPolicy::LatestOnly`], submitting newer work for the same key
    /// cancels the active token. This helper observes that token, kills the
    /// child process, drains stdout/stderr, and suppresses stale messages.
    pub fn command_keyed<Msg, F>(
        self,
        key: impl Into<Arc<str>>,
        policy: TaskPolicy,
        map: F,
    ) -> Command
    where
        Msg: Send + 'static,
        F: Fn(ProcessEvent) -> Msg + Send + 'static,
    {
        process_command_keyed(key, policy, self, map)
    }

    fn to_std_command(&self) -> StdCommand {
        let mut command = StdCommand::new(self.program.as_ref());
        command.args(self.args.iter().map(AsRef::<str>::as_ref));
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        for (key, value) in &self.env {
            command.env(key.as_ref(), value.as_ref());
        }
        command.stdin(if self.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        command.stdout(Stdio::piped());
        command.stderr(Stdio::piped());
        command
    }
}

/// Framework-owned child-process exit status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProcessExitStatus {
    code: Option<i32>,
    success: bool,
}

impl ProcessExitStatus {
    /// Platform exit code, when the platform reports one.
    pub fn code(self) -> Option<i32> {
        self.code
    }

    /// Whether the process exited successfully.
    pub fn success(self) -> bool {
        self.success
    }
}

impl From<ExitStatus> for ProcessExitStatus {
    fn from(status: ExitStatus) -> Self {
        Self {
            code: status.code(),
            success: status.success(),
        }
    }
}

/// Streaming child-process event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessEvent {
    /// Bytes read from stdout.
    Stdout(Vec<u8>),
    /// Bytes read from stderr.
    Stderr(Vec<u8>),
    /// Process exited and all captured output has been drained.
    Exited(ProcessExitStatus),
    /// Process-level or pipe-level error.
    Error(Arc<str>),
}

/// Run a process on the current thread and emit stdout/stderr/exit events.
pub fn stream_process(spec: ProcessSpec, mut emit: impl FnMut(ProcessEvent)) -> io::Result<()> {
    stream_process_until(spec, || false, &mut emit)
}

/// Run a process and stop it when `should_cancel` returns `true`.
///
/// On cancellation the child is killed, stdout/stderr are drained, and an
/// `Exited` event is emitted with the platform status reported by `wait`.
pub fn stream_process_until(
    mut spec: ProcessSpec,
    should_cancel: impl Fn() -> bool,
    mut emit: impl FnMut(ProcessEvent),
) -> io::Result<()> {
    let mut command = spec.to_std_command();
    let stdin_bytes = spec.stdin.take();
    let mut child = command.spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child_stdin = child.stdin.take();
    let (tx, rx) = mpsc::channel();

    let stdout_thread = stdout.map(|stdout| spawn_reader(stdout, tx.clone(), ProcessEvent::Stdout));
    let stderr_thread = stderr.map(|stderr| spawn_reader(stderr, tx.clone(), ProcessEvent::Stderr));
    let stdin_thread = stdin_bytes.and_then(|bytes| {
        child_stdin.map(|mut stdin| {
            let tx = tx.clone();
            thread::spawn(move || {
                if let Err(err) = stdin.write_all(&bytes) {
                    send_error(&tx, err);
                }
            })
        })
    });

    let status = loop {
        while let Ok(event) = rx.try_recv() {
            emit(event);
        }

        if should_cancel() {
            let _ = child.kill();
            break child.wait();
        }

        if let Some(status) = child.try_wait()? {
            break Ok(status);
        }

        match rx.recv_timeout(Duration::from_millis(20)) {
            Ok(event) => emit(event),
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                if let Some(status) = child.try_wait()? {
                    break Ok(status);
                }
            }
        }
    };

    join_optional(stdin_thread);
    join_optional(stdout_thread);
    join_optional(stderr_thread);

    for event in rx.try_iter() {
        emit(event);
    }

    emit(ProcessEvent::Exited(status?.into()));
    Ok(())
}

/// Create a background command that maps process events into component messages.
pub fn process_command<Msg, F>(spec: ProcessSpec, map: F) -> Command
where
    Msg: Send + 'static,
    F: Fn(ProcessEvent) -> Msg + Send + 'static,
{
    Command::spawn::<Msg, _>(move |link| {
        if let Err(err) = stream_process_until(
            spec,
            || link.is_cancelled(),
            |event| {
                let _ = link.send_if_not_cancelled(map(event));
            },
        ) {
            let _ =
                link.send_if_not_cancelled(map(ProcessEvent::Error(Arc::from(err.to_string()))));
        }
    })
}

/// Create a keyed background command that maps process events into component messages.
pub fn process_command_keyed<Msg, F>(
    key: impl Into<Arc<str>>,
    policy: TaskPolicy,
    spec: ProcessSpec,
    map: F,
) -> Command
where
    Msg: Send + 'static,
    F: Fn(ProcessEvent) -> Msg + Send + 'static,
{
    Command::spawn_keyed::<Msg, _>(key, policy, move |link| {
        if let Err(err) = stream_process_until(
            spec,
            || link.is_cancelled(),
            |event| {
                let _ = link.send_if_not_cancelled(map(event));
            },
        ) {
            let _ =
                link.send_if_not_cancelled(map(ProcessEvent::Error(Arc::from(err.to_string()))));
        }
    })
}

fn spawn_reader<R>(
    mut reader: R,
    tx: mpsc::Sender<ProcessEvent>,
    wrap: fn(Vec<u8>) -> ProcessEvent,
) -> thread::JoinHandle<()>
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let mut buf = [0_u8; 8192];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if tx.send(wrap(buf[..n].to_vec())).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    send_error(&tx, err);
                    break;
                }
            }
        }
    })
}

fn join_optional(handle: Option<thread::JoinHandle<()>>) {
    if let Some(handle) = handle {
        let _ = handle.join();
    }
}

fn send_error(tx: &mpsc::Sender<ProcessEvent>, err: io::Error) {
    let _ = tx.send(ProcessEvent::Error(Arc::from(err.to_string())));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_spec_builder_sets_fields() {
        let spec = ProcessSpec::new("sh")
            .arg("-c")
            .args(["printf", "ok"])
            .cwd("/")
            .env("KEY", "VALUE")
            .stdin(b"input".to_vec());

        assert_eq!(spec.program(), "sh");
        assert_eq!(spec.args_slice().len(), 3);
        assert_eq!(spec.args_slice()[0].as_ref(), "-c");
        assert_eq!(spec.cwd_path(), Some(Path::new("/")));
        assert_eq!(spec.env_slice()[0].0.as_ref(), "KEY");
        assert_eq!(spec.env_slice()[0].1.as_ref(), "VALUE");
        assert_eq!(spec.stdin_bytes(), Some(b"input".as_slice()));
    }

    #[cfg(unix)]
    #[test]
    fn streams_stdout_stderr_and_exit() {
        let spec = ProcessSpec::new("sh").args(["-c", "printf out; printf err >&2"]);
        let mut events = Vec::new();

        spec.stream(|event| events.push(event)).unwrap();

        let stdout: Vec<u8> = events
            .iter()
            .filter_map(|event| match event {
                ProcessEvent::Stdout(bytes) => Some(bytes.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect();
        let stderr: Vec<u8> = events
            .iter()
            .filter_map(|event| match event {
                ProcessEvent::Stderr(bytes) => Some(bytes.as_slice()),
                _ => None,
            })
            .flatten()
            .copied()
            .collect();

        assert_eq!(stdout, b"out");
        assert_eq!(stderr, b"err");
        assert!(matches!(events.last(), Some(ProcessEvent::Exited(status)) if status.success()));
    }

    #[cfg(unix)]
    #[test]
    fn stream_process_until_kills_child_when_cancelled() {
        let spec = ProcessSpec::new("sh").args(["-c", "sleep 5"]);
        let mut events = Vec::new();

        stream_process_until(spec, || true, |event| events.push(event)).unwrap();

        assert!(matches!(events.last(), Some(ProcessEvent::Exited(status)) if !status.success()));
    }
}
