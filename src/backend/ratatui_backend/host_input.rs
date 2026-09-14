//! Reading the host terminal from the UI thread.
//!
//! On Unix every read of the host terminal goes through termina. crossterm's Unix event source
//! retries a tty read that returns end-of-file forever, without checking the timeout it was given,
//! and a pty whose far end has closed returns exactly that. No check made before handing a read to
//! crossterm can close that race, because the terminal can go away while crossterm is inside its
//! loop. termina's source returns `UnexpectedEof` instead, so a hang-up ends the read.
//!
//! The fullscreen runner reads through its own worker (`TerminaInputCoordinator`). Everything that
//! reads on the UI thread instead - inline input, the drains around a terminal handoff, and cursor
//! position queries - uses the reader registered here, so the tty keeps one reader at a time.
//! Windows console input does not go through that loop and keeps crossterm.

use std::io;
use std::time::Duration;

use crossterm::event::Event as CrosstermEvent;

/// One step of reading the host terminal.
pub(crate) enum HostEvent {
    Input(CrosstermEvent),
    /// A pointer report in pixels: the cell it landed in, and where inside that cell.
    #[cfg(unix)]
    Pointer(CrosstermEvent, (u16, u16)),
    /// The host announced a theme change.
    #[cfg(unix)]
    ThemeRefresh,
    /// Nothing arrived within the wait, or what arrived is not input.
    Quiet,
    /// The terminal has gone. Nothing will ever arrive.
    #[cfg(unix)]
    HungUp,
}

#[cfg(unix)]
pub(crate) use imp::{InlineHostReader, is_hang_up};

#[cfg(unix)]
mod imp {
    use std::io::{self, Write};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use termina::escape::csi::{Csi, Cursor};
    use termina::event::Event as TerminaEvent;
    use termina::{EventReader, PlatformTerminal, Terminal as _};

    use super::HostEvent;
    use crate::app::runner::input_coordinator::{TerminaEventAction, map_termina_event};

    /// The reader UI-thread consumers share while an inline runner owns input.
    static REGISTERED: Mutex<Option<Registered>> = Mutex::new(None);

    struct Registered {
        reader: EventReader,
        hung_up: &'static AtomicBool,
    }

    static INLINE_HUNG_UP: AtomicBool = AtomicBool::new(false);

    /// Whether `err` means the terminal is gone rather than that one read failed.
    ///
    /// termina reports end-of-file as `UnexpectedEof`. A pty slave read after its master closed,
    /// but before the kernel finishes hanging it up, fails with `EIO` instead.
    pub(crate) fn is_hang_up(err: &io::Error) -> bool {
        err.kind() == io::ErrorKind::UnexpectedEof || err.raw_os_error() == Some(libc::EIO)
    }

    /// Input for an inline runner, read synchronously on the UI thread.
    ///
    /// Registered for as long as it lives, so the drains and ratatui's cursor queries read through
    /// the same reader instead of racing it for the tty.
    pub(crate) struct InlineHostReader {
        _private: (),
    }

    impl InlineHostReader {
        pub(crate) fn start() -> io::Result<Self> {
            let reader = open_reader()?;
            INLINE_HUNG_UP.store(false, Ordering::SeqCst);
            if let Ok(mut registered) = REGISTERED.lock() {
                *registered = Some(Registered {
                    reader,
                    hung_up: &INLINE_HUNG_UP,
                });
            }
            Ok(Self { _private: () })
        }

        pub(crate) fn read(&self, wait: Duration) -> io::Result<HostEvent> {
            if INLINE_HUNG_UP.load(Ordering::SeqCst) {
                // Asking again would fail at once; the runner is only waiting out its grace period.
                std::thread::sleep(wait);
                return Ok(HostEvent::Quiet);
            }
            with_reader(|reader, hung_up| read_one(reader, wait, hung_up))?
        }
    }

    impl Drop for InlineHostReader {
        fn drop(&mut self) {
            if let Ok(mut registered) = REGISTERED.lock() {
                *registered = None;
            }
        }
    }

    fn open_reader() -> io::Result<EventReader> {
        // Dropping the terminal restores the termios it captured a moment ago, which changes nothing.
        Ok(PlatformTerminal::new()?.event_reader())
    }

    /// Run `f` with the registered reader, or with a reader opened for this call alone.
    fn with_reader<T>(f: impl FnOnce(&EventReader, &AtomicBool) -> T) -> io::Result<T> {
        let registered = REGISTERED
            .lock()
            .ok()
            .and_then(|registered| registered.as_ref().map(|r| (r.reader.clone(), r.hung_up)));
        match registered {
            Some((reader, hung_up)) => Ok(f(&reader, hung_up)),
            None => {
                let local = AtomicBool::new(false);
                Ok(f(&open_reader()?, &local))
            }
        }
    }

    fn read_one(
        reader: &EventReader,
        wait: Duration,
        hung_up: &AtomicBool,
    ) -> io::Result<HostEvent> {
        let event = match reader.poll(Some(wait), |_| true) {
            Ok(true) => reader.read(|_| true),
            Ok(false) => return Ok(HostEvent::Quiet),
            Err(err) => Err(err),
        };
        match event {
            Ok(event) => Ok(match map_termina_event(event) {
                TerminaEventAction::Input(event) => HostEvent::Input(event),
                TerminaEventAction::Pointer(event, sub_cell) => HostEvent::Pointer(event, sub_cell),
                TerminaEventAction::ThemeRefresh => HostEvent::ThemeRefresh,
                TerminaEventAction::Ignore => HostEvent::Quiet,
            }),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => Ok(HostEvent::Quiet),
            Err(err) if is_hang_up(&err) => {
                hung_up.store(true, Ordering::SeqCst);
                Ok(HostEvent::HungUp)
            }
            Err(err) => Err(err),
        }
    }

    pub(crate) fn read_host_event(wait: Duration) -> io::Result<HostEvent> {
        with_reader(|reader, hung_up| {
            if hung_up.load(Ordering::SeqCst) {
                return Ok(HostEvent::HungUp);
            }
            read_one(reader, wait, hung_up)
        })?
    }

    /// The cursor's zero-based column and row, or `None` when the terminal does not answer within
    /// `timeout` or goes away first.
    pub(crate) fn cursor_position(timeout: Duration) -> io::Result<Option<(u16, u16)>> {
        let was_raw = crossterm::terminal::is_raw_mode_enabled()?;
        if !was_raw {
            crossterm::terminal::enable_raw_mode()?;
        }
        let position = with_reader(|reader, hung_up| query_cursor(reader, timeout, hung_up));
        if !was_raw {
            let _ = crossterm::terminal::disable_raw_mode();
        }
        position?
    }

    fn query_cursor(
        reader: &EventReader,
        timeout: Duration,
        hung_up: &AtomicBool,
    ) -> io::Result<Option<(u16, u16)>> {
        if hung_up.load(Ordering::SeqCst) {
            return Ok(None);
        }
        let mut out = io::stdout().lock();
        out.write_all(b"\x1b[6n")?;
        out.flush()?;
        drop(out);

        let is_report = |event: &TerminaEvent| {
            matches!(
                event,
                TerminaEvent::Csi(Csi::Cursor(Cursor::ActivePositionReport { .. }))
            )
        };
        let deadline = Instant::now() + timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(None);
            }
            // The filter leaves every other event queued in the reader, so input typed while the
            // terminal answers is still delivered afterwards.
            let found = match reader.poll(Some(remaining), is_report) {
                Ok(found) => found,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
                Err(err) if is_hang_up(&err) => {
                    hung_up.store(true, Ordering::SeqCst);
                    return Ok(None);
                }
                Err(err) => return Err(err),
            };
            if !found {
                continue;
            }
            match reader.read(is_report) {
                Ok(TerminaEvent::Csi(Csi::Cursor(Cursor::ActivePositionReport { line, col }))) => {
                    return Ok(Some((col.get_zero_based(), line.get_zero_based())));
                }
                Ok(_) => continue,
                Err(err) if is_hang_up(&err) => {
                    hung_up.store(true, Ordering::SeqCst);
                    return Ok(None);
                }
                Err(err) => return Err(err),
            }
        }
    }
}

#[cfg(not(unix))]
mod imp {
    use std::io;
    use std::time::Duration;

    use crossterm::event;

    use super::HostEvent;

    pub(crate) fn read_host_event(wait: Duration) -> io::Result<HostEvent> {
        Ok(if event::poll(wait)? {
            HostEvent::Input(event::read()?)
        } else {
            HostEvent::Quiet
        })
    }

    /// Windows reports the cursor through the console API, not by reading input.
    pub(crate) fn cursor_position(_timeout: Duration) -> io::Result<Option<(u16, u16)>> {
        Ok(crossterm::cursor::position().ok())
    }
}

/// Read the next host event, waiting at most `wait` for one.
pub(crate) fn read_host_event(wait: Duration) -> io::Result<HostEvent> {
    imp::read_host_event(wait)
}

/// Where the cursor is, asked without risking crossterm's reader.
pub(crate) fn cursor_position(timeout: Duration) -> io::Result<Option<(u16, u16)>> {
    imp::cursor_position(timeout)
}

/// Run a test body in a child process whose controlling terminal is a pty, then hang that terminal
/// up while the child is blocked reading it.
///
/// The child is this same test binary re-running one test by name. It ignores `SIGHUP`, so what
/// ends its reads is end-of-file on the tty, not the signal. It writes `key=value` lines to a report
/// file; the parent waits for `ready`, closes the pty master, and collects the report.
#[cfg(all(test, target_os = "linux"))]
#[allow(unsafe_code)]
pub(crate) mod pty_test {
    use std::collections::HashMap;
    use std::ffi::CStr;
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::os::fd::{FromRawFd, OwnedFd};
    use std::os::unix::fs::OpenOptionsExt;
    use std::os::unix::process::CommandExt;
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    const CHILD_ENV: &str = "TUI_LIPAN_PTY_CHILD_REPORT";

    /// The report file when running as the child, `None` when running as the parent.
    pub(crate) fn child_report() -> Option<Report> {
        let path = std::env::var_os(CHILD_ENV)?;
        // SAFETY: plain signal disposition change in a single-purpose child process.
        unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
        Some(Report(PathBuf::from(path)))
    }

    pub(crate) struct Report(PathBuf);

    impl Report {
        pub(crate) fn line(&self, key: &str, value: impl std::fmt::Display) {
            if let Ok(mut file) = OpenOptions::new().append(true).create(true).open(&self.0) {
                let _ = writeln!(file, "{key}={value}");
            }
        }
    }

    /// CPU time this process has used so far, user and system together.
    pub(crate) fn cpu_time() -> Duration {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        // SAFETY: `usage` is a valid out-pointer.
        unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        let tv = |t: libc::timeval| {
            Duration::from_secs(t.tv_sec as u64) + Duration::from_micros(t.tv_usec as u64)
        };
        tv(usage.ru_utime) + tv(usage.ru_stime)
    }

    /// Run `test_name` (its path without the crate name) as a child on a fresh pty, close the
    /// master once the child reports `ready`, and return the child's report.
    pub(crate) fn run_and_hang_up(test_name: &str) -> HashMap<String, String> {
        // SAFETY: straightforward pty allocation; every return value is checked.
        let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
        assert!(master >= 0, "posix_openpt failed");
        // SAFETY: `master` was just opened and is owned here.
        let master = unsafe { OwnedFd::from_raw_fd(master) };
        let raw_master = std::os::fd::AsRawFd::as_raw_fd(&master);
        assert_eq!(unsafe { libc::grantpt(raw_master) }, 0, "grantpt failed");
        assert_eq!(unsafe { libc::unlockpt(raw_master) }, 0, "unlockpt failed");
        let mut name = [0 as libc::c_char; 128];
        assert_eq!(
            unsafe { libc::ptsname_r(raw_master, name.as_mut_ptr(), name.len()) },
            0,
            "ptsname_r failed"
        );
        let slave_path = unsafe { CStr::from_ptr(name.as_ptr()) }
            .to_string_lossy()
            .into_owned();
        let slave = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOCTTY)
            .open(&slave_path)
            .expect("open pty slave");

        static NEXT: AtomicU64 = AtomicU64::new(0);
        let report = std::env::temp_dir().join(format!(
            "tui-lipan-pty-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_file(&report);

        let mut command = Command::new(std::env::current_exe().expect("test binary path"));
        command
            .args([test_name, "--exact", "--nocapture", "--test-threads=1"])
            .env(CHILD_ENV, &report)
            .stdin(Stdio::from(slave.try_clone().expect("dup slave")))
            .stdout(Stdio::from(slave.try_clone().expect("dup slave")))
            .stderr(Stdio::from(slave));
        // SAFETY: only async-signal-safe calls between fork and exec.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() < 0 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut child = command.spawn().expect("spawn pty child");

        let wait_for = |predicate: &dyn Fn(&str) -> bool, limit: Duration| {
            let deadline = Instant::now() + limit;
            while Instant::now() < deadline {
                if fs::read_to_string(&report).is_ok_and(|text| predicate(&text)) {
                    return true;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            false
        };
        if !wait_for(&|text| text.contains("ready="), Duration::from_secs(20)) {
            let _ = child.kill();
            let _ = child.wait();
            panic!("pty child never became ready");
        }
        // Give the child time to be blocked inside its read, then take the terminal away.
        std::thread::sleep(Duration::from_millis(300));
        drop(master);

        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if child.try_wait().expect("wait for pty child").is_some() {
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                let text = fs::read_to_string(&report).unwrap_or_default();
                panic!("pty child did not finish after its terminal hung up; report:\n{text}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let text = fs::read_to_string(&report).unwrap_or_default();
        let _ = fs::remove_file(&report);
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(k, v)| (k.to_owned(), v.to_owned()))
            .collect()
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use std::time::{Duration, Instant};

    use super::pty_test::{child_report, cpu_time, run_and_hang_up};
    use super::{HostEvent, InlineHostReader, cursor_position};

    fn test_path(name: &str) -> String {
        let module = module_path!();
        let module = module.split_once("::").map_or(module, |(_, rest)| rest);
        format!("{module}::{name}")
    }

    #[test]
    fn inline_reader_reports_a_hang_up_instead_of_spinning() {
        if let Some(report) = child_report() {
            let reader = InlineHostReader::start().expect("open inline reader");
            report.line("ready", 1);
            let started = Instant::now();
            loop {
                match reader.read(Duration::from_millis(200)) {
                    Ok(HostEvent::HungUp) => break,
                    Ok(_) if started.elapsed() < Duration::from_secs(10) => {}
                    Ok(_) => {
                        report.line("error", "no hang-up within 10s");
                        return;
                    }
                    Err(err) => {
                        report.line("error", err);
                        return;
                    }
                }
            }
            report.line("hung_up", 1);
            let cpu_before = cpu_time();
            let quiet_started = Instant::now();
            while quiet_started.elapsed() < Duration::from_millis(500) {
                let _ = reader.read(Duration::from_millis(50));
            }
            report.line("cpu_ms", (cpu_time() - cpu_before).as_millis());
            report.line("done", 1);
            return;
        }

        let report = run_and_hang_up(&test_path(
            "inline_reader_reports_a_hang_up_instead_of_spinning",
        ));
        assert_eq!(report.get("error"), None, "{report:?}");
        assert_eq!(
            report.get("hung_up").map(String::as_str),
            Some("1"),
            "{report:?}"
        );
        let cpu_ms: u64 = report["cpu_ms"].parse().unwrap();
        assert!(
            cpu_ms < 100,
            "reading after the hang-up used {cpu_ms}ms of CPU in 500ms"
        );
    }

    #[test]
    fn a_cursor_query_gives_up_when_the_terminal_hangs_up_mid_wait() {
        if let Some(report) = child_report() {
            report.line("ready", 1);
            let started = Instant::now();
            let cpu_before = cpu_time();
            // Nothing answers: the parent never reads the query, and closes the terminal instead.
            let position = cursor_position(Duration::from_secs(10));
            report.line("elapsed_ms", started.elapsed().as_millis());
            report.line("cpu_ms", (cpu_time() - cpu_before).as_millis());
            report.line("answer", format!("{position:?}"));
            report.line("done", 1);
            return;
        }

        let report = run_and_hang_up(&test_path(
            "a_cursor_query_gives_up_when_the_terminal_hangs_up_mid_wait",
        ));
        assert_eq!(
            report.get("done").map(String::as_str),
            Some("1"),
            "{report:?}"
        );
        let elapsed_ms: u64 = report["elapsed_ms"].parse().unwrap();
        assert!(
            elapsed_ms < 5_000,
            "the query outlived the terminal: {report:?}"
        );
        let cpu_ms: u64 = report["cpu_ms"].parse().unwrap();
        assert!(cpu_ms < 250, "the query spun: {report:?}");
        assert!(report["answer"].starts_with("Ok(None)") || report["answer"].starts_with("Err"));
    }
}
