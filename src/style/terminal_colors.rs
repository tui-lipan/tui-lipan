#![allow(unsafe_code)]

#[cfg(unix)]
use std::time::Duration;
#[cfg(unix)]
use web_time::Instant;

use crate::style::Color;

/// Colors reported by the host terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostTerminalColors {
    /// ANSI slots 0..15 from OSC 4.
    pub ansi: [Color; 16],
    /// Default foreground from OSC 10.
    pub fg: Color,
    /// Default background from OSC 11.
    pub bg: Color,
}

/// Query the host terminal for its color palette via OSC 4/10/11.
///
/// Returns `None` if `/dev/tty` cannot be opened or the terminal does not
/// respond within ~200ms.
#[cfg(unix)]
pub fn query_host_colors() -> Option<HostTerminalColors> {
    let fd = tty_open()?;
    let _fd_guard = FdGuard(fd);
    let _raw_guard = RawModeGuard::new(fd)?;

    tty_write_all(fd, &build_query_batch())?;

    let mut buffer = Vec::with_capacity(4096);
    let mut parsed = Parsed::default();

    let deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < deadline {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as i32;
        if timeout <= 0 || !poll_readable(fd, timeout)? {
            break;
        }

        let mut chunk = [0u8; 1024];
        let n = tty_read(fd, &mut chunk)?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
        parse_frames(&mut buffer, &mut parsed);
        if parsed.complete() {
            break;
        }
    }

    let fg = parsed.fg?;
    let bg = parsed.bg?;
    let mut ansi = [Color::Reset; 16];
    for (i, slot) in ansi.iter_mut().enumerate() {
        *slot = parsed.ansi[i].unwrap_or_else(|| default_ansi(i as u8));
    }

    Some(HostTerminalColors { ansi, fg, bg })
}

/// Query stub for non-Unix hosts.
#[cfg(not(unix))]
pub fn query_host_colors() -> Option<HostTerminalColors> {
    None
}

/// Detect Kitty keyboard-protocol support with a short, bounded timeout.
///
/// This mirrors `crossterm::terminal::supports_keyboard_enhancement` (write
/// `CSI ? u` followed by `CSI c`, then wait for a Kitty flags reply `CSI ? … u`
/// or the Primary Device Attributes terminator `CSI ? … c`) but caps the wait at
/// ~250ms instead of crossterm's hard-coded 2s. Terminals answer this probe in
/// well under a millisecond; the long crossterm timeout only ever bites when
/// nothing is on the other end of the TTY (a non-interactive PTY, a harness, a
/// pipe), where it stalls startup for two seconds before defaulting to `false`.
///
/// Returns `None` when `/dev/tty` cannot be opened or raw mode cannot be set;
/// callers should treat that as "unsupported" (`.unwrap_or(false)`), matching the
/// previous crossterm behavior.
#[cfg(unix)]
pub fn query_keyboard_enhancement_support() -> Option<bool> {
    let fd = tty_open()?;
    let _fd_guard = FdGuard(fd);
    let _raw_guard = RawModeGuard::new(fd)?;

    // CSI ? u  → request current Kitty keyboard flags.
    // CSI c    → Primary Device Attributes, a universally-supported terminator
    //            so we can stop waiting as soon as the DA reply arrives.
    tty_write_all(fd, b"\x1b[?u\x1b[c")?;

    let mut buffer = Vec::with_capacity(64);
    let deadline = Instant::now() + Duration::from_millis(250);
    while Instant::now() < deadline {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as i32;
        if timeout <= 0 || !poll_readable(fd, timeout)? {
            break;
        }
        let mut chunk = [0u8; 256];
        let n = tty_read(fd, &mut chunk)?;
        if n == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(supported) = scan_keyboard_enhancement(&buffer) {
            return Some(supported);
        }
    }
    // No decisive reply within the budget: treat as unsupported.
    Some(false)
}

/// Detection stub for non-Unix hosts (crossterm handles Windows natively, but the
/// call sites default to `false`, so returning `None` preserves that).
#[cfg(not(unix))]
pub fn query_keyboard_enhancement_support() -> Option<bool> {
    None
}

/// Discard any capability-probe responses still sitting in the TTY input queue.
///
/// The startup probes each send a Primary Device Attributes request (`CSI c`)
/// as a sentinel: the keyboard-enhancement probe above, and (with the `image`
/// feature) `ratatui-image`'s graphics query. Terminals answer with a DA1 reply
/// such as `CSI ? 62 ; 22 ; 52 c`. A probe stops reading the moment it has the
/// answer it needs, so the trailing DA reply can be left unread — e.g. a Kitty
/// terminal answers `query_keyboard_enhancement_support` with `CSI ? … u` and we
/// return before consuming the following `CSI c` reply. Those bytes stay
/// invisible while the app holds raw mode, then get echoed to the shell prompt
/// as a stray `^[[?…c` when the terminal is restored to cooked mode on exit.
///
/// Draining them here, right after the probes and before the event loop starts,
/// prevents that. The window is short and bounded; a fast, already-drained TTY
/// returns at the first empty poll.
#[cfg(unix)]
pub(crate) fn drain_pending_terminal_responses() {
    let Some(fd) = tty_open() else {
        return;
    };
    let _fd_guard = FdGuard(fd);
    let Some(_raw_guard) = RawModeGuard::new(fd) else {
        return;
    };

    // Probe replies arrive in well under a millisecond; a small grace window
    // catches a straggler without a noticeable startup stall. Stop at the first
    // empty poll so nothing is discarded once the queue is clear.
    let deadline = Instant::now() + Duration::from_millis(50);
    let mut chunk = [0u8; 256];
    while Instant::now() < deadline {
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as i32;
        match poll_readable(fd, remaining.min(10)) {
            Some(true) => {}
            _ => break,
        }
        match tty_read(fd, &mut chunk) {
            Some(n) if n > 0 => {}
            _ => break,
        }
    }
}

/// Drain stub for non-Unix hosts. Windows terminals do not exhibit the leaked
/// DA-reply behavior this guards against under crossterm's native handling.
#[cfg(not(unix))]
pub(crate) fn drain_pending_terminal_responses() {}

/// Discard any terminal query-response bytes still queued on the TTY, called
/// immediately before the terminal is restored to cooked mode on exit.
///
/// A capability probe's DA1 reply (`CSI ? … c`) can arrive after the startup
/// [`drain_pending_terminal_responses`] window closed and then sit unread in the
/// input queue. The fullscreen reader thread normally consumes it mid-session
/// (crossterm parses it as an internal, non-public event and drops it), which is
/// why the leak is intermittent — but on slower terminals or multiplexers the
/// reply can still be pending at teardown. `exit_plan` disables raw mode as its
/// first op, so anything left in the queue is then echoed to the shell prompt as
/// a stray `^[[?…c`. Flushing the kernel input queue while raw mode is still
/// active drops it deterministically.
///
/// This mirrors the `tcflush` in `terminal_handoff::discard_pending_terminal_input`
/// used on external-process resume; on final exit there is nothing to preserve
/// input for, so a blanket flush is correct.
#[cfg(unix)]
pub(crate) fn flush_pending_terminal_responses_on_exit() {
    let Some(fd) = tty_open() else {
        return;
    };
    let _fd_guard = FdGuard(fd);
    // SAFETY: `tcflush(TCIFLUSH)` on the controlling TTY drops input received but
    // not yet read (leaked DA/OSC query responses); it is a no-op on an empty
    // queue and harmless if `/dev/tty` is not a terminal.
    unsafe {
        libc::tcflush(fd, libc::TCIFLUSH);
    }
}

/// Flush stub for non-Unix hosts. See [`drain_pending_terminal_responses`].
#[cfg(not(unix))]
pub(crate) fn flush_pending_terminal_responses_on_exit() {}

/// Scan probe output for a decisive Kitty/DA reply.
///
/// `Some(true)`  — a `CSI ? … u` Kitty flags reply appeared (protocol supported).
/// `Some(false)` — a `CSI ? … c` Primary Device Attributes reply appeared first
///                 (terminator seen, no Kitty support).
/// `None`        — no complete `CSI ? …` response yet; keep reading.
#[cfg(unix)]
fn scan_keyboard_enhancement(buf: &[u8]) -> Option<bool> {
    let mut i = 0usize;
    while i + 2 < buf.len() {
        if buf[i] == 0x1b && buf[i + 1] == b'[' && buf[i + 2] == b'?' {
            let mut j = i + 3;
            while j < buf.len() {
                let b = buf[j];
                match b {
                    b'u' => return Some(true),
                    b'c' => return Some(false),
                    // CSI parameter (0x30..=0x3f) or intermediate (0x20..=0x2f) bytes
                    0x20..=0x3f => j += 1,
                    // any other final byte: not the reply we sent, stop this scan
                    _ => break,
                }
            }
            if j >= buf.len() {
                // Sequence not yet terminated; wait for more bytes.
                return None;
            }
        }
        i += 1;
    }
    None
}

#[cfg(unix)]
#[derive(Default)]
struct Parsed {
    ansi: [Option<Color>; 16],
    fg: Option<Color>,
    bg: Option<Color>,
}

#[cfg(unix)]
impl Parsed {
    fn complete(&self) -> bool {
        self.fg.is_some() && self.bg.is_some()
    }
}

#[cfg(unix)]
fn build_query_batch() -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    for i in 0..16 {
        out.extend_from_slice(format!("\x1b]4;{i};?\x1b\\").as_bytes());
    }
    out.extend_from_slice(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\");
    out
}

#[cfg(unix)]
fn parse_frames(buffer: &mut Vec<u8>, parsed: &mut Parsed) {
    let mut scan = 0usize;
    while scan + 2 <= buffer.len() {
        let Some(start_rel) = buffer[scan..].windows(2).position(|w| w == b"\x1b]") else {
            break;
        };
        let start = scan + start_rel;
        let body_start = start + 2;
        let Some((body_end, frame_end)) = find_terminator(buffer, body_start) else {
            if start > 0 {
                buffer.drain(..start);
            }
            return;
        };
        parse_body(&buffer[body_start..body_end], parsed);
        buffer.drain(..frame_end);
        scan = 0;
    }
    let keep = usize::from(buffer.last() == Some(&0x1b));
    if keep == 0 {
        buffer.clear();
    } else {
        let last = buffer[buffer.len() - 1];
        buffer.clear();
        buffer.push(last);
    }
}

#[cfg(unix)]
fn find_terminator(buf: &[u8], start: usize) -> Option<(usize, usize)> {
    let mut i = start;
    while i < buf.len() {
        if buf[i] == 0x07 {
            return Some((i, i + 1));
        }
        if buf[i] == 0x1b && i + 1 < buf.len() && buf[i + 1] == b'\\' {
            return Some((i, i + 2));
        }
        i += 1;
    }
    None
}

#[cfg(unix)]
fn parse_body(body: &[u8], parsed: &mut Parsed) {
    let Ok(text) = std::str::from_utf8(body) else {
        return;
    };
    if let Some(rest) = text.strip_prefix("4;") {
        let mut parts = rest.splitn(3, ';');
        let Some(i) = parts.next().and_then(|s| s.parse::<usize>().ok()) else {
            return;
        };
        if i >= 16 {
            return;
        }
        let Some(color_text) = parts.next() else {
            return;
        };
        if let Some(color) = parse_rgb(color_text) {
            parsed.ansi[i] = Some(color);
        }
        return;
    }
    if let Some(rest) = text.strip_prefix("10;") {
        if let Some(color) = parse_rgb(rest) {
            parsed.fg = Some(color);
        }
        return;
    }
    if let Some(rest) = text.strip_prefix("11;")
        && let Some(color) = parse_rgb(rest)
    {
        parsed.bg = Some(color);
    }
}

#[cfg(unix)]
fn parse_rgb(s: &str) -> Option<Color> {
    let raw = s.strip_prefix("rgb:")?;
    let mut parts = raw.split('/');
    let r = parse_channel(parts.next()?)?;
    let g = parse_channel(parts.next()?)?;
    let b = parse_channel(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(Color::Rgb(r, g, b))
}

#[cfg(unix)]
fn parse_channel(hex: &str) -> Option<u8> {
    match hex.len() {
        2 => u8::from_str_radix(hex, 16).ok(),
        4 => Some((u16::from_str_radix(hex, 16).ok()? >> 8) as u8),
        _ => None,
    }
}

#[cfg(unix)]
fn default_ansi(index: u8) -> Color {
    let (r, g, b) = Color::indexed(index).to_rgb().unwrap_or((0, 0, 0));
    Color::Rgb(r, g, b)
}

#[cfg(unix)]
fn tty_open() -> Option<i32> {
    let path = b"/dev/tty\0";
    // SAFETY: The path is a valid NUL-terminated C string.
    let fd = unsafe { libc::open(path.as_ptr().cast(), libc::O_RDWR | libc::O_CLOEXEC) };
    (fd >= 0).then_some(fd)
}

#[cfg(unix)]
fn tty_write_all(fd: i32, mut bytes: &[u8]) -> Option<()> {
    while !bytes.is_empty() {
        // SAFETY: Pointer and len are derived from a valid byte slice.
        let n = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        if n > 0 {
            bytes = &bytes[n as usize..];
            continue;
        }
        if n < 0 {
            let err = std::io::Error::last_os_error().raw_os_error();
            if err == Some(libc::EINTR) {
                continue;
            }
        }
        return None;
    }
    Some(())
}

#[cfg(unix)]
fn poll_readable(fd: i32, timeout_ms: i32) -> Option<bool> {
    loop {
        let mut pfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pfd` points to initialized storage for one pollfd.
        let rc = unsafe { libc::poll(&mut pfd as *mut libc::pollfd, 1, timeout_ms) };
        if rc < 0 {
            let err = std::io::Error::last_os_error().raw_os_error();
            if err == Some(libc::EINTR) {
                continue;
            }
            return None;
        }
        if rc == 0 {
            return Some(false);
        }
        return Some((pfd.revents & libc::POLLIN) != 0);
    }
}

#[cfg(unix)]
fn tty_read(fd: i32, buf: &mut [u8]) -> Option<usize> {
    loop {
        // SAFETY: `buf` is a valid writable byte buffer.
        let n = unsafe { libc::read(fd, buf.as_mut_ptr().cast(), buf.len()) };
        if n >= 0 {
            return Some(n as usize);
        }
        let err = std::io::Error::last_os_error().raw_os_error();
        if err == Some(libc::EINTR) {
            continue;
        }
        return None;
    }
}

#[cfg(unix)]
struct FdGuard(i32);

#[cfg(unix)]
impl Drop for FdGuard {
    fn drop(&mut self) {
        // SAFETY: File descriptor is owned by this guard.
        let _ = unsafe { libc::close(self.0) };
    }
}

#[cfg(unix)]
struct RawModeGuard {
    fd: i32,
    original: libc::termios,
}

#[cfg(unix)]
impl RawModeGuard {
    fn new(fd: i32) -> Option<Self> {
        // SAFETY: Zero-initialized termios is valid for immediate tcgetattr fill.
        let mut term = unsafe { std::mem::zeroed::<libc::termios>() };
        // SAFETY: `term` points to valid writable termios storage.
        if unsafe { libc::tcgetattr(fd, &mut term as *mut libc::termios) } != 0 {
            return None;
        }
        let original = term;

        term.c_iflag &= !(libc::BRKINT | libc::ICRNL | libc::INPCK | libc::ISTRIP | libc::IXON);
        term.c_oflag &= !libc::OPOST;
        term.c_cflag |= libc::CS8;
        term.c_lflag &= !(libc::ECHO | libc::ICANON | libc::IEXTEN | libc::ISIG);
        term.c_cc[libc::VMIN] = 0;
        term.c_cc[libc::VTIME] = 0;
        // SAFETY: termios pointer is valid for this fd.
        if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &term as *const libc::termios) } != 0 {
            return None;
        }
        Some(Self { fd, original })
    }
}

#[cfg(unix)]
impl Drop for RawModeGuard {
    fn drop(&mut self) {
        // SAFETY: Restoring a previously captured termios value.
        let _ = unsafe {
            libc::tcsetattr(
                self.fd,
                libc::TCSANOW,
                &self.original as *const libc::termios,
            )
        };
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::scan_keyboard_enhancement;

    #[test]
    fn kitty_flags_reply_reports_supported() {
        // CSI ? 1 u  → Kitty keyboard flags present.
        assert_eq!(scan_keyboard_enhancement(b"\x1b[?1u"), Some(true));
    }

    #[test]
    fn kitty_reply_before_da_reports_supported() {
        // Kitty terminals answer the flags query first, then the DA terminator.
        assert_eq!(
            scan_keyboard_enhancement(b"\x1b[?5u\x1b[?62;1;6c"),
            Some(true)
        );
    }

    #[test]
    fn primary_da_only_reports_unsupported() {
        // No Kitty support: only the Primary Device Attributes reply arrives.
        assert_eq!(scan_keyboard_enhancement(b"\x1b[?62;1;6c"), Some(false));
    }

    #[test]
    fn partial_sequence_is_inconclusive() {
        // A `CSI ? …` prefix with no terminator yet must not decide early.
        assert_eq!(scan_keyboard_enhancement(b"\x1b[?62;1;6"), None);
        assert_eq!(scan_keyboard_enhancement(b"\x1b[?"), None);
        assert_eq!(scan_keyboard_enhancement(b""), None);
    }

    #[test]
    fn unrelated_bytes_are_ignored() {
        // Stray output that is not a `CSI ? …` reply is not misread as a decision.
        assert_eq!(scan_keyboard_enhancement(b"hello\x1b[2J world"), None);
    }
}
