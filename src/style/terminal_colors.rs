#![allow(unsafe_code)]

#[cfg(unix)]
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(unix)]
use std::time::Duration;
#[cfg(unix)]
use web_time::Instant;

use crate::style::Color;

/// Colors reported by the host terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostTerminalColors {
    /// ANSI slots 0..15 resolved to their reported RGB values.
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

/// What the host terminal said when asked, at startup, what it implements.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HostCapabilities {
    /// The Kitty keyboard protocol, which distinguishes key presses a legacy encoding cannot.
    pub keyboard_enhancement: bool,
    /// SGR-pixels mouse reporting (DEC private mode 1016), which puts the pointer's position on the
    /// wire in pixels rather than cells.
    pub pixel_mouse: bool,
    /// A graphics-protocol query answered `OK`. Only meaningful when one was asked.
    pub graphics_query_ok: bool,
}

/// What the startup probe learned about the host, in two pieces with different lifetimes.
///
/// **How long the host takes to answer `CSI c`** is durable: it describes the terminal, and stays
/// true until a different probe replaces it. **Whether the probe's own reply is still in flight**
/// is not - it holds only until something drains the input queue, and the first flush is that
/// something. Keeping the two apart is what stops a transient condition from being read as a
/// standing fact for the rest of the process.
///
/// Both are atomics rather than a lock: the readers run in `Drop` and on the panic path, where
/// taking a lock is not worth the risk. `Relaxed` is enough because each value stands alone and
/// publishes no accompanying memory.
///
/// The round trip in microseconds, or [`ROUND_TRIP_UNKNOWN`] when nothing answered.
#[cfg(unix)]
static PROBE_ROUND_TRIP: AtomicU64 = AtomicU64::new(ROUND_TRIP_UNKNOWN);

/// Whether the startup probe's `CSI c` reply may still be on its way. Cleared by whoever acts on
/// it - see [`take_startup_reply_outstanding`].
#[cfg(unix)]
static STARTUP_REPLY_OUTSTANDING: AtomicBool = AtomicBool::new(false);

/// Every other value is a real measurement, so the marker takes the top of the range where one
/// cannot reach.
#[cfg(unix)]
const ROUND_TRIP_UNKNOWN: u64 = u64::MAX;

#[cfg(unix)]
fn record_round_trip(round_trip: Option<Duration>) {
    let bits = round_trip.map_or(ROUND_TRIP_UNKNOWN, |round_trip| {
        u64::try_from(round_trip.as_micros())
            .unwrap_or(u64::MAX)
            .min(ROUND_TRIP_UNKNOWN - 1)
    });
    PROBE_ROUND_TRIP.store(bits, Ordering::Relaxed);
}

#[cfg(unix)]
fn probe_round_trip() -> Option<Duration> {
    match PROBE_ROUND_TRIP.load(Ordering::Relaxed) {
        ROUND_TRIP_UNKNOWN => None,
        micros => Some(Duration::from_micros(micros)),
    }
}

/// Read the "startup reply may still be coming" flag and clear it in one step.
///
/// Taken rather than read, because the caller is about to drain and flush the queue the reply would
/// arrive in. Leaving it set would make every later call wait the reply out again - and there are
/// many later calls: this runs on terminal drop, on panic restore, and on both sides of every
/// external-program handoff, so a flag that never cleared would put a wait on each of them for a
/// reply the first one already dealt with.
#[cfg(unix)]
fn take_startup_reply_outstanding() -> bool {
    STARTUP_REPLY_OUTSTANDING.swap(false, Ordering::Relaxed)
}

/// Ask the host what it implements, in one round trip with a short, bounded timeout.
///
/// The keyboard half mirrors `crossterm::terminal::supports_keyboard_enhancement` (write `CSI ? u`,
/// wait for a Kitty flags reply `CSI ? … u`) but caps the wait at ~250ms instead of crossterm's
/// hard-coded 2s. Terminals answer in well under a millisecond; the long timeout only ever bites
/// when nothing is on the other end of the TTY (a non-interactive PTY, a harness, a pipe), where it
/// stalls startup for two seconds before defaulting to `false`.
///
/// Both questions ride one `CSI c` sentinel - Primary Device Attributes, which every terminal
/// answers - so the wait ends as soon as that reply arrives and the sentinel is consumed rather than
/// left queued to leak into the shell later. Asking them separately would cost a second timeout.
///
/// The mouse half cannot be asked through the event reader instead: `DECRPM` replies for modes the
/// input parser does not model are a parse error there, not an event. Neither can a graphics query,
/// whose reply is an `APC` sequence the parser does not surface at all - hence `graphics_probe`,
/// written verbatim before the sentinel, whose `OK` reply is reported as `graphics_query_ok`.
///
/// Returns `None` when `/dev/tty` cannot be opened or raw mode cannot be set; callers should treat
/// that as "nothing supported".
#[cfg(unix)]
pub fn query_host_capabilities(graphics_probe: &[u8]) -> Option<HostCapabilities> {
    // Cleared before the first thing that can fail. Every step below - opening the TTY, raw mode,
    // the write, the poll, the read - can return early, and a guard may be entering a different
    // terminal than the one an earlier probe measured. Without this, a probe that never establishes
    // that the current host answers at all would leave the previous host's round trip standing, and
    // teardown would write a sentinel on the strength of it.
    record_round_trip(None);
    STARTUP_REPLY_OUTSTANDING.store(false, Ordering::Relaxed);

    let fd = tty_open()?;
    let _fd_guard = FdGuard(fd);
    let _raw_guard = RawModeGuard::new(fd)?;

    let mut probe = Vec::with_capacity(graphics_probe.len() + 24);
    probe.extend_from_slice(b"\x1b[?u\x1b[?1016$p");
    probe.extend_from_slice(graphics_probe);
    probe.extend_from_slice(b"\x1b[c");
    // Timed from before the write: what teardown needs to budget for is the whole trip, including
    // however long the host sat on the request before answering it.
    let asked = Instant::now();
    tty_write_all(fd, &probe)?;

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
        if let Some(capabilities) = scan_host_capabilities(&buffer) {
            record_round_trip(Some(asked.elapsed()));
            return Some(capabilities);
        }
    }
    // No decisive reply within the budget: treat as unsupported, and leave the round trip unknown -
    // nothing here measured one. Whether a reply is nevertheless still on its way is the separate
    // question teardown asks, and an `ESC` is the evidence for it: every reply starts with one, and
    // a key pressed during startup - the other thing that can put bytes here - usually does not.
    STARTUP_REPLY_OUTSTANDING.store(buffer.contains(&0x1b), Ordering::Relaxed);
    Some(HostCapabilities::default())
}

/// Query stub for non-Unix hosts (crossterm handles Windows keyboard enhancement natively).
#[cfg(not(unix))]
pub fn query_host_capabilities(_graphics_probe: &[u8]) -> Option<HostCapabilities> {
    None
}

/// Discard any capability-probe responses still sitting in the TTY input queue.
///
/// The startup probes each send a Primary Device Attributes request (`CSI c`)
/// as a sentinel: the keyboard-enhancement probe above, and (with the `image`
/// feature) `ratatui-image`'s graphics query. Terminals answer with a DA1 reply
/// such as `CSI ? 62 ; 22 ; 52 c`. The keyboard probe normally consumes its
/// sentinel, but a reply delayed past its timeout or left by the image probe can
/// remain unread. Those bytes stay invisible while the app holds raw mode, then
/// get echoed to the shell prompt as a stray `^[[?…c` when the terminal is
/// restored to cooked mode on exit.
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

/// What the teardown flush does about the reply it may be owed.
#[cfg(unix)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ExitFlush {
    /// Write a fresh `CSI c` as an ordering sentinel before draining.
    sentinel: bool,
    /// Upper bound on the wait. It is reached only when no reply comes; a reply ends the wait
    /// immediately, so the usual cost is one round trip rather than the whole budget.
    budget: Duration,
}

/// Enough for any terminal on the same machine, and the floor a measured round trip is raised to.
#[cfg(unix)]
const EXIT_FLUSH_FLOOR: Duration = Duration::from_millis(50);
/// Ceiling on the wait, so a host that stops answering cannot hold a quit open indefinitely.
#[cfg(unix)]
const EXIT_FLUSH_CEILING: Duration = Duration::from_millis(500);
/// Headroom over the measured round trip, covering the jitter a single sample cannot.
#[cfg(unix)]
const EXIT_FLUSH_HEADROOM: u32 = 4;

/// Decide the flush from what is known about the host and what may still be owed by it.
///
/// The rule behind every arm is that a sentinel is a promise to wait for its reply. Writing one and
/// then giving up early does not merely fail to help - it queues a reply that lands after cooked
/// mode returns, which is the leak this whole path exists to prevent.
#[cfg(unix)]
fn exit_flush_plan(round_trip: Option<Duration>, startup_reply_outstanding: bool) -> ExitFlush {
    // A reply is already owed from startup, so asking again would queue a second one behind it.
    // This waits out the first instead of adding to the problem, and the wait is the ceiling
    // because the round trip that would have sized it is exactly what did not come back.
    //
    // Only the first flush sees this: the flag is taken, not read. The wait can run the full
    // ceiling, because the fragment already read at startup was consumed there - the tail arriving
    // here cannot be recognised as the end of a `CSI c` reply on its own. Erring long is the safe
    // direction: too long costs a one-time pause, too short is the leak.
    if startup_reply_outstanding {
        return ExitFlush {
            sentinel: false,
            budget: EXIT_FLUSH_CEILING,
        };
    }
    match round_trip {
        // The host answers and how fast is known, so a sentinel can be both asked for and waited
        // out.
        Some(round_trip) => ExitFlush {
            sentinel: true,
            budget: round_trip
                .saturating_mul(EXIT_FLUSH_HEADROOM)
                .clamp(EXIT_FLUSH_FLOOR, EXIT_FLUSH_CEILING),
        },
        // Nothing is known to answer, and nothing is owed. Waiting would spend the ceiling to learn
        // what is already known, on every call.
        None => ExitFlush {
            sentinel: false,
            budget: Duration::ZERO,
        },
    }
}

/// Read until the `CSI c` reply arrives or `budget` runs out.
#[cfg(unix)]
fn drain_through_da_reply(fd: i32, budget: Duration) {
    if budget.is_zero() {
        return;
    }
    let deadline = Instant::now() + budget;
    let mut pending = Vec::with_capacity(256);
    let mut chunk = [0u8; 256];
    while Instant::now() < deadline {
        let remaining = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as i32;
        match poll_readable(fd, remaining.min(10)) {
            // Sliced so the deadline is still checked while the host is quiet.
            Some(false) => continue,
            // A poll error will not fix itself, and retrying it inside the deadline is a spin.
            None => return,
            Some(true) => {}
        }
        match tty_read(fd, &mut chunk) {
            Some(0) | None => return,
            Some(n) => pending.extend_from_slice(&chunk[..n]),
        }
        if scan_host_capabilities(&pending).is_some() {
            return;
        }
    }
}

/// Discard terminal protocol-response bytes still queued on the controlling TTY.
///
/// A capability probe's DA1 reply (`CSI ? … c`) can arrive after the startup
/// [`drain_pending_terminal_responses`] window closed and then sit unread in the
/// input queue. The fullscreen reader thread normally consumes it mid-session
/// (crossterm parses it as an internal, non-public event and drops it), which is
/// why the leak is intermittent — but on slower terminals or multiplexers the
/// reply can still be pending at teardown. Mode-2031 reports can likewise race
/// notification disablement. Callers use this only while the input worker is
/// paused or joined and before restoring cooked mode or handing the TTY to an
/// external child, so a kernel flush cannot compete with a runtime decoder.
///
/// At those boundaries there is no application input to preserve, so a blanket
/// flush is correct.
///
/// A DA1 request is an ordering sentinel: its reply can only arrive after the terminal has
/// processed the preceding mode changes and their reports, so draining through it drains those
/// too. But the wait for it has to be a wait the round trip actually fits inside. Over SSH, or
/// through a multiplexer on a remote host, the trip runs from the application to `sshd`, across
/// the network to the user's real terminal, and all the way back; a budget picked for a terminal
/// on the same machine expires first, and the reply then lands at the shell prompt as a stray
/// `61;4;…c`. That is the leak, arriving by way of the sentinel written to prevent it. So the
/// budget comes from [`exit_flush_plan`], which sizes it against the round trip the startup probe
/// measured on this very link, and declines to write a sentinel it cannot promise to wait for.
///
/// Despite the name this is not a once-per-process call. It runs on terminal drop, on panic
/// restore, and on both sides of every external-program handoff, so anything it costs is paid again
/// on each trip out to an editor or a shell - which is why the one condition here that is genuinely
/// transient, the startup reply still being in flight, is taken and cleared rather than read.
#[cfg(unix)]
pub(crate) fn flush_pending_terminal_responses_on_exit() {
    let plan = exit_flush_plan(probe_round_trip(), take_startup_reply_outstanding());
    let Some(fd) = tty_open() else {
        return;
    };
    let _fd_guard = FdGuard(fd);
    if plan.sentinel {
        let _ = tty_write_all(fd, b"\x1b[c");
    }
    drain_through_da_reply(fd, plan.budget);
    // SAFETY: `tcflush(TCIFLUSH)` on the controlling TTY drops unread response
    // fragments after the ordering sentinel; callers have paused the input worker.
    unsafe {
        libc::tcflush(fd, libc::TCIFLUSH);
    }
}

/// Flush stub for non-Unix hosts. See [`drain_pending_terminal_responses`].
#[cfg(not(unix))]
pub(crate) fn flush_pending_terminal_responses_on_exit() {}

/// Scan probe output through the terminating DA reply.
///
/// `Some(_)` — the DA terminator arrived; whatever replies preceded it are the answer.
/// `None`    — it has not arrived yet; keep reading. Waiting for it consumes the sentinel instead of
///             leaving it queued to leak into the shell when raw mode is disabled.
#[cfg(unix)]
fn scan_host_capabilities(buf: &[u8]) -> Option<HostCapabilities> {
    let mut capabilities = HostCapabilities::default();
    let mut i = 0usize;
    while i + 2 < buf.len() {
        // APC: the graphics protocol's own reply, `ESC _ G i=… ; OK ESC \`.
        if buf[i] == 0x1b && buf[i + 1] == b'_' {
            let end = find_string_terminator(&buf[i..])?;
            capabilities.graphics_query_ok |= buf[i..i + end].ends_with(b";OK");
            i += end;
            continue;
        }
        if buf[i] == 0x1b && buf[i + 1] == b'[' && buf[i + 2] == b'?' {
            let params = i + 3;
            let mut j = params;
            while j < buf.len() {
                match buf[j] {
                    b'u' => {
                        capabilities.keyboard_enhancement = true;
                        i = j;
                        break;
                    }
                    b'y' => {
                        capabilities.pixel_mouse |= implements_mode(&buf[params..j], 1016);
                        i = j;
                        break;
                    }
                    b'c' => return Some(capabilities),
                    // CSI parameter (0x30..=0x3f) or intermediate (0x20..=0x2f) bytes
                    0x20..=0x3f => j += 1,
                    // any other final byte: not a reply we sent, stop this scan
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

/// Where the `ESC \` that ends a string sequence begins, counted from its introducer.
#[cfg(unix)]
fn find_string_terminator(buf: &[u8]) -> Option<usize> {
    buf.windows(2)
        .position(|pair| pair == [0x1b, b'\\'])
        .filter(|position| *position > 0)
}

/// Whether a `DECRPM` reply's parameters say `mode` is implemented.
///
/// The parameters run from the `?` to the `$` of a `CSI ? Pa ; Ps $ y` reply. Setting 0 is "not
/// recognized"; every other value describes a mode the terminal has, set or reset, so the question
/// of whether it is on right now is a different one from whether asking for it will work.
#[cfg(unix)]
fn implements_mode(params: &[u8], mode: u16) -> bool {
    let Ok(text) = std::str::from_utf8(params) else {
        return false;
    };
    let Some(text) = text.strip_suffix('$') else {
        return false;
    };
    let mut fields = text.split(';');
    if fields.next().and_then(|field| field.parse::<u16>().ok()) != Some(mode) {
        return false;
    }
    matches!(
        fields.next().and_then(|field| field.parse::<u8>().ok()),
        Some(1..=4)
    )
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
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    use super::{
        EXIT_FLUSH_CEILING, EXIT_FLUSH_FLOOR, ExitFlush, HostCapabilities,
        STARTUP_REPLY_OUTSTANDING, exit_flush_plan, probe_round_trip, record_round_trip,
        scan_host_capabilities, take_startup_reply_outstanding,
    };

    fn keyboard(enhancement: bool) -> Option<HostCapabilities> {
        Some(HostCapabilities {
            keyboard_enhancement: enhancement,
            ..HostCapabilities::default()
        })
    }

    #[test]
    fn kitty_flags_reply_waits_for_da_terminator() {
        // The flags reply proves support, but the DA sentinel must still be consumed.
        assert_eq!(scan_host_capabilities(b"\x1b[?1u"), None);
    }

    #[test]
    fn kitty_reply_before_da_reports_supported() {
        // Kitty terminals answer the flags query first, then the DA terminator.
        assert_eq!(
            scan_host_capabilities(b"\x1b[?5u\x1b[?62;1;6c"),
            keyboard(true)
        );
    }

    #[test]
    fn primary_da_only_reports_unsupported() {
        // No Kitty support: only the Primary Device Attributes reply arrives.
        assert_eq!(scan_host_capabilities(b"\x1b[?62;1;6c"), keyboard(false));
    }

    #[test]
    fn partial_sequence_is_inconclusive() {
        // A `CSI ? …` prefix with no terminator yet must not decide early.
        assert_eq!(scan_host_capabilities(b"\x1b[?62;1;6"), None);
        assert_eq!(scan_host_capabilities(b"\x1b[?"), None);
        assert_eq!(scan_host_capabilities(b""), None);
    }

    #[test]
    fn unrelated_bytes_are_ignored() {
        // Stray output that is not a `CSI ? …` reply is not misread as a decision.
        assert_eq!(scan_host_capabilities(b"hello\x1b[2J world"), None);
    }

    /// The graphics reply is an APC sequence rather than a CSI one, and its `OK` is what says a
    /// terminal can read pixels out of shared memory.
    #[test]
    fn a_graphics_reply_decides_shared_memory_support() {
        assert_eq!(
            scan_host_capabilities(b"\x1b_Gi=4294967295;OK\x1b\\\x1b[?62;1;6c"),
            Some(HostCapabilities {
                graphics_query_ok: true,
                ..HostCapabilities::default()
            })
        );
        assert_eq!(
            scan_host_capabilities(b"\x1b_Gi=4294967295;ENOTSUPP:shared memory\x1b\\\x1b[?62;1;6c"),
            keyboard(false)
        );
        assert_eq!(
            scan_host_capabilities(b"\x1b_Gi=4294967295;OK"),
            None,
            "an unterminated APC reply is not yet an answer"
        );
        assert_eq!(
            scan_host_capabilities(b"\x1b_Gi=1;OK\x1b\\\x1b[?5u\x1b[?1016;2$y\x1b[?62;1;6c"),
            Some(HostCapabilities {
                keyboard_enhancement: true,
                pixel_mouse: true,
                graphics_query_ok: true,
            }),
            "three answers and a terminator in one round trip"
        );
    }

    /// The mode report is why this probe exists at all: a terminal that implements SGR-pixels says
    /// so here, and a terminal that does not answers 0 or does not answer.
    #[test]
    fn a_mode_report_decides_pixel_mouse_support() {
        assert_eq!(
            scan_host_capabilities(b"\x1b[?1016;2$y\x1b[?62;1;6c"),
            Some(HostCapabilities {
                pixel_mouse: true,
                ..HostCapabilities::default()
            })
        );
        assert_eq!(
            scan_host_capabilities(b"\x1b[?5u\x1b[?1016;1$y\x1b[?62;1;6c"),
            Some(HostCapabilities {
                keyboard_enhancement: true,
                pixel_mouse: true,
                graphics_query_ok: false,
            })
        );
        assert_eq!(
            scan_host_capabilities(b"\x1b[?1016;0$y\x1b[?62;1;6c"),
            keyboard(false),
            "not recognized"
        );
        assert_eq!(
            scan_host_capabilities(b"\x1b[?2026;2$y\x1b[?62;1;6c"),
            keyboard(false),
            "a report about some other mode decides nothing"
        );
    }
    #[test]
    fn a_terminal_on_this_machine_keeps_the_floor() {
        // Four times nothing is still nothing, and the floor is what a local terminal ran on
        // before any of this was measured.
        assert_eq!(
            exit_flush_plan(Some(Duration::from_micros(300)), false),
            ExitFlush {
                sentinel: true,
                budget: EXIT_FLUSH_FLOOR,
            }
        );
    }

    #[test]
    fn a_round_trip_over_ssh_widens_the_budget_past_the_floor() {
        // The reported case: a 60ms link answers long after a 50ms budget has given up, and the
        // reply lands at the shell prompt.
        assert_eq!(
            exit_flush_plan(Some(Duration::from_millis(60)), false),
            ExitFlush {
                sentinel: true,
                budget: Duration::from_millis(240),
            }
        );
    }

    #[test]
    fn a_slow_link_stops_widening_at_the_ceiling() {
        assert_eq!(
            exit_flush_plan(Some(Duration::from_millis(200)), false),
            ExitFlush {
                sentinel: true,
                budget: EXIT_FLUSH_CEILING,
            },
            "four times 200ms would be 800ms; a quit does not wait that long"
        );
    }

    #[test]
    fn a_reply_already_owed_is_waited_out_rather_than_asked_for_again() {
        assert_eq!(
            exit_flush_plan(None, true),
            ExitFlush {
                sentinel: false,
                budget: EXIT_FLUSH_CEILING,
            },
            "a second sentinel would only queue a second reply to leak"
        );
    }

    #[test]
    fn an_owed_reply_outranks_a_known_round_trip() {
        // Both can be true at once - a probe answers, a later one times out with a fragment - and
        // the owed reply has to win: a sentinel sized by the old round trip would be queued behind
        // a reply that has not arrived.
        assert_eq!(
            exit_flush_plan(Some(Duration::from_millis(1)), true),
            ExitFlush {
                sentinel: false,
                budget: EXIT_FLUSH_CEILING,
            }
        );
    }

    #[test]
    fn a_host_that_never_answered_is_not_waited_for() {
        assert_eq!(
            exit_flush_plan(None, false),
            ExitFlush {
                sentinel: false,
                budget: Duration::ZERO,
            },
            "nothing is known to answer, and nothing is owed"
        );
    }

    /// Owns `STARTUP_REPLY_OUTSTANDING` for the whole suite. Nothing else writes it: the only
    /// other writer is the startup probe, which no test may run - CI has no tty, and library code
    /// must never query the host terminal from a test.
    #[test]
    fn the_owed_reply_is_taken_once_and_not_again() {
        STARTUP_REPLY_OUTSTANDING.store(true, Ordering::Relaxed);

        assert!(take_startup_reply_outstanding(), "the first flush sees it");
        assert!(
            !take_startup_reply_outstanding(),
            "and no later one does - the first drained the queue it would have arrived in. A flag \
             that stayed set would put the ceiling on terminal drop, on panic restore, and on both \
             sides of every external-program handoff, forever."
        );
    }

    /// Owns `PROBE_ROUND_TRIP` for the whole suite, for the same reason as the test above.
    #[test]
    fn the_round_trip_survives_the_atomic_encoding() {
        for round_trip in [Duration::from_micros(1), Duration::from_millis(60)] {
            record_round_trip(Some(round_trip));
            assert_eq!(probe_round_trip(), Some(round_trip));
        }

        // The marker lives at the top of the range, so a saturating measurement has to land below
        // it: read back as `None` it would say the host never answered, which is the opposite.
        record_round_trip(Some(Duration::MAX));
        assert!(probe_round_trip().is_some(), "saturated, not unknown");

        record_round_trip(None);
        assert_eq!(
            probe_round_trip(),
            None,
            "and unknown round-trips as unknown"
        );
    }
}
