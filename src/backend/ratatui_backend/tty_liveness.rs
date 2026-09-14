//! Reading the host terminal without spinning once it has gone away.
//!
//! crossterm's Unix event source reads the tty in a loop that only stops on `WouldBlock` or a
//! parsed event. A read that returns end-of-file, or fails with anything but `Interrupted`, goes
//! straight back round, and no timeout the caller passed is checked inside that loop. A pty whose
//! far end has closed returns exactly that. So once the terminal is gone, every `event::poll`,
//! `event::read`, and `cursor::position` spins a core forever instead of returning.
//!
//! That is an ordinary way for a program to end: an emulator window closes, or an `ssh` connection
//! drops, while the app is still tidying up after the `SIGHUP`. The app then never exits. So the
//! framework never blocks inside crossterm's reader: it waits on the descriptor itself, asks
//! crossterm only for what is already readable, and checks for a hang-up first.

use std::io;
use std::time::Duration;

use crossterm::event;

/// One step of reading the host terminal.
pub(crate) enum HostEvent {
    Event(event::Event),
    /// Nothing arrived within the wait.
    Quiet,
    /// The terminal has gone. Nothing will ever arrive, and asking crossterm would spin.
    #[cfg_attr(not(unix), allow(dead_code))]
    HungUp,
}

#[cfg(unix)]
mod imp {
    use std::io;
    use std::os::fd::RawFd;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use crossterm::event;

    use super::HostEvent;

    /// The descriptor crossterm reads events from: stdin when it is a terminal, otherwise its own
    /// `/dev/tty`. Decided while the terminal is still attached, because a hung-up stdin no longer
    /// answers `isatty` and so could not be told apart from a pipe afterwards.
    static INPUT_FD: OnceLock<Option<RawFd>> = OnceLock::new();

    fn input_fd() -> Option<RawFd> {
        *INPUT_FD.get_or_init(|| {
            #[allow(unsafe_code)]
            // SAFETY: `isatty` only inspects the descriptor.
            if unsafe { libc::isatty(libc::STDIN_FILENO) } == 1 {
                return Some(libc::STDIN_FILENO);
            }
            #[allow(unsafe_code)]
            // SAFETY: a static NUL-terminated path. The descriptor is kept for the life of the
            // process, so a later hang-up is still visible on it.
            let fd = unsafe {
                libc::open(
                    c"/dev/tty".as_ptr(),
                    libc::O_RDONLY | libc::O_NOCTTY | libc::O_CLOEXEC | libc::O_NONBLOCK,
                )
            };
            (fd >= 0).then_some(fd)
        })
    }

    pub(crate) fn note_host_tty() {
        let _ = input_fd();
    }

    #[derive(Debug, PartialEq, Eq)]
    pub(super) enum Readiness {
        Readable,
        Quiet,
        HungUp,
    }

    pub(super) fn wait_on(fd: RawFd, timeout: Duration) -> Readiness {
        let timeout_ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            #[allow(unsafe_code)]
            // SAFETY: `pfd` is one initialized pollfd.
            let rc = unsafe { libc::poll(&mut pfd, 1, timeout_ms) };
            if rc < 0 {
                if io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
                    continue;
                }
                return Readiness::HungUp;
            }
            if rc == 0 {
                return Readiness::Quiet;
            }
            if pfd.revents & (libc::POLLHUP | libc::POLLERR | libc::POLLNVAL) != 0 {
                return Readiness::HungUp;
            }
            return Readiness::Readable;
        }
    }

    pub(crate) fn host_tty_hung_up() -> bool {
        input_fd().is_some_and(|fd| wait_on(fd, Duration::ZERO) == Readiness::HungUp)
    }

    pub(crate) fn read_host_event(wait: Duration) -> io::Result<HostEvent> {
        let Some(fd) = input_fd() else {
            // No terminal to watch: crossterm fails to open one rather than spinning.
            return Ok(if event::poll(wait)? {
                HostEvent::Event(event::read()?)
            } else {
                HostEvent::Quiet
            });
        };
        let deadline = Instant::now() + wait;
        loop {
            if wait_on(fd, Duration::ZERO) == Readiness::HungUp {
                return Ok(HostEvent::HungUp);
            }
            // Events crossterm already parsed, or bytes already queued, come back without blocking.
            if event::poll(Duration::ZERO)? {
                return Ok(HostEvent::Event(event::read()?));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Ok(HostEvent::Quiet);
            }
            // A partial sequence leaves the descriptor drained, so this sleeps until the rest.
            match wait_on(fd, remaining) {
                Readiness::Readable => {}
                Readiness::Quiet => return Ok(HostEvent::Quiet),
                Readiness::HungUp => return Ok(HostEvent::HungUp),
            }
        }
    }

    /// The cursor's row, asked of the terminal directly rather than through crossterm's reader,
    /// whose own timeout cannot fire once the terminal hangs up mid-wait. `None` when no answer
    /// comes within `timeout`, or the terminal goes away first.
    pub(crate) fn cursor_row(timeout: Duration) -> Option<u16> {
        let fd = input_fd()?;
        let was_raw = crossterm::terminal::is_raw_mode_enabled().ok()?;
        if !was_raw {
            crossterm::terminal::enable_raw_mode().ok()?;
        }
        let row = query_cursor_row(fd, timeout);
        if !was_raw {
            let _ = crossterm::terminal::disable_raw_mode();
        }
        row
    }

    fn query_cursor_row(fd: RawFd, timeout: Duration) -> Option<u16> {
        use std::io::Write;

        let mut out = io::stdout().lock();
        out.write_all(b"\x1b[6n").ok()?;
        out.flush().ok()?;
        drop(out);

        let deadline = Instant::now() + timeout;
        let mut reply = Vec::with_capacity(32);
        let mut chunk = [0u8; 64];
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || wait_on(fd, remaining) != Readiness::Readable {
                return None;
            }
            #[allow(unsafe_code)]
            // SAFETY: `chunk` is a valid writable buffer of the length passed.
            let n = unsafe { libc::read(fd, chunk.as_mut_ptr().cast(), chunk.len()) };
            if n == 0 {
                return None;
            }
            if n < 0 {
                match io::Error::last_os_error().kind() {
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock => continue,
                    _ => return None,
                }
            }
            reply.extend_from_slice(&chunk[..n.unsigned_abs()]);
            if let Some(row) = parse_cursor_row(&reply) {
                return Some(row);
            }
        }
    }

    /// Zero-based row from the last complete cursor position report (`ESC [ row ; col R`) in
    /// `reply`, skipping whatever else the terminal sent first.
    pub(super) fn parse_cursor_row(reply: &[u8]) -> Option<u16> {
        let end = reply.iter().rposition(|&byte| byte == b'R')?;
        let start = reply[..end].windows(2).rposition(|pair| pair == b"\x1b[")? + 2;
        let params = std::str::from_utf8(&reply[start..end]).ok()?;
        let (row, col) = params.split_once(';')?;
        col.parse::<u16>().ok()?;
        row.parse::<u16>().ok()?.checked_sub(1)
    }
}

#[cfg(not(unix))]
mod imp {
    use std::io;
    use std::time::Duration;

    use crossterm::event;

    use super::HostEvent;

    pub(crate) fn note_host_tty() {}

    /// Windows console input does not go through the looping Unix source.
    pub(crate) fn host_tty_hung_up() -> bool {
        false
    }

    pub(crate) fn read_host_event(wait: Duration) -> io::Result<HostEvent> {
        Ok(if event::poll(wait)? {
            HostEvent::Event(event::read()?)
        } else {
            HostEvent::Quiet
        })
    }

    pub(crate) fn cursor_row(_timeout: Duration) -> Option<u16> {
        crossterm::cursor::position().ok().map(|(_, row)| row)
    }
}

pub(crate) use imp::{cursor_row, host_tty_hung_up, note_host_tty};

/// Read the next host event, waiting at most `wait` for one.
pub(crate) fn read_host_event(wait: Duration) -> io::Result<HostEvent> {
    imp::read_host_event(wait)
}

#[cfg(all(test, unix))]
#[allow(unsafe_code)]
mod tests {
    use super::imp::{Readiness, parse_cursor_row, wait_on};
    use std::time::{Duration, Instant};

    #[test]
    fn a_cursor_report_is_read_past_whatever_the_terminal_sent_before_it() {
        assert_eq!(parse_cursor_row(b"\x1b[12;40R"), Some(11));
        assert_eq!(parse_cursor_row(b"\x1b[?62;22c\x1b[1;1R"), Some(0));
        assert_eq!(parse_cursor_row(b"\x1b[12;4"), None);
        assert_eq!(parse_cursor_row(b"\x1b[0;1R"), None);
    }

    #[test]
    fn a_closed_far_end_reads_as_hung_up_at_once_instead_of_waiting_out_the_timeout() {
        let mut fds = [0; 2];
        // SAFETY: `fds` has room for the two descriptors `pipe` writes.
        assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0);
        let [read_end, write_end] = fds;

        assert_eq!(wait_on(read_end, Duration::ZERO), Readiness::Quiet);
        // SAFETY: one byte from a valid buffer into our own pipe.
        assert_eq!(
            unsafe { libc::write(write_end, b"x".as_ptr().cast(), 1) },
            1
        );
        assert_eq!(wait_on(read_end, Duration::ZERO), Readiness::Readable);

        let mut byte = [0u8; 1];
        // SAFETY: one byte into a valid buffer, then closing our own descriptor.
        unsafe {
            libc::read(read_end, byte.as_mut_ptr().cast(), 1);
            libc::close(write_end);
        }
        let started = Instant::now();
        assert_eq!(wait_on(read_end, Duration::from_secs(5)), Readiness::HungUp);
        assert!(started.elapsed() < Duration::from_secs(1));
        // SAFETY: closing our own descriptor.
        unsafe { libc::close(read_end) };
    }
}
