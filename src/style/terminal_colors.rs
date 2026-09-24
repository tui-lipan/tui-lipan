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
    /// ANSI slots 0..15 as RGB values.
    ///
    /// A slot the terminal did not report (it ignores OSC 4, or answered only some slots) holds
    /// the value from an earlier query, else the standard ANSI color, so every slot is usable as
    /// an RGB value. [`Self::ansi_reported`] tells the two apart.
    pub ansi: [Color; 16],
    /// Which [`Self::ansi`] slots the terminal reported: bit `i` is set when slot `i` came from an
    /// OSC 4 reply, in this query or an earlier one.
    ///
    /// Only a reported slot says what the terminal shows for that color. Anything that must match
    /// the terminal - rather than merely needing some RGB value - should use
    /// [`Self::reported_ansi`].
    pub ansi_reported: u16,
    /// Default foreground from OSC 10.
    pub fg: Color,
    /// Default background from OSC 11.
    pub bg: Color,
}

impl HostTerminalColors {
    /// Every ANSI slot marked as reported by the terminal.
    pub const ALL_ANSI_REPORTED: u16 = u16::MAX;

    /// ANSI slot `slot` (0..15) as the terminal reported it, or `None` when it did not report
    /// that slot and [`Self::ansi`] holds a fallback.
    pub fn reported_ansi(&self, slot: usize) -> Option<Color> {
        (slot < 16 && self.ansi_reported & (1 << slot) != 0).then(|| self.ansi[slot])
    }
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

    let mut ordered_response = Vec::with_capacity(4096);
    let mut parser = HostColorResponseParser::default();
    parser.start_query();

    let deadline = Instant::now() + Duration::from_millis(200);
    while Instant::now() < deadline {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .as_millis() as i32;
        if timeout <= 0 || !poll_readable(fd, timeout).unwrap_or(false) {
            break;
        }

        let mut chunk = [0u8; 1024];
        let Some(n) = tty_read(fd, &mut chunk) else {
            break;
        };
        if n == 0 {
            break;
        }
        ordered_response.extend_from_slice(&chunk[..n]);
        parser.push(&chunk[..n]);
        if host_color_query_settled(&ordered_response) {
            break;
        }
    }

    parser.finish_query(None)
}

/// Whether the DA1 ordering sentinel at the end of a host-color query has come back.
///
/// Foreground and background replies are not enough. A terminal may schedule its OSC 4 palette
/// reports separately and return OSC 10/11 first. Restoring cooked mode at that point lets the
/// pending palette reports echo onto the primary screen before the application enters its
/// alternate screen, where they stay hidden until exit.
#[cfg(unix)]
fn host_color_query_settled(response: &[u8]) -> bool {
    scan_host_capabilities(response).is_some()
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

/// Whether the startup probe's `CSI c` reply may still be on its way. Set the moment the sentinel
/// is written and cleared by whoever drains the queue for it - see [`settle_startup_reply`].
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

#[cfg(unix)]
fn set_startup_reply_outstanding(outstanding: bool) {
    STARTUP_REPLY_OUTSTANDING.store(outstanding, Ordering::Relaxed);
}

#[cfg(unix)]
fn startup_reply_outstanding() -> bool {
    STARTUP_REPLY_OUTSTANDING.load(Ordering::Relaxed)
}

/// Clear the flag, but only for a caller that actually observed it set and has since drained and
/// flushed the queue the reply would have arrived in.
///
/// Read-then-settle rather than a single `swap`, because taking the flag up front would let a
/// caller that never got that far - one whose `/dev/tty` open failed, say - count as having dealt
/// with the reply, leaving every later flush to skip a wait nothing had performed.
///
/// Clearing it at all is what keeps this from becoming a standing cost: the flush runs on terminal
/// drop, on panic restore, and on both sides of every external-program handoff, so a flag that
/// never cleared would put the ceiling on each trip out to an editor or a shell.
#[cfg(unix)]
fn settle_startup_reply(was_outstanding: bool) {
    if was_outstanding {
        STARTUP_REPLY_OUTSTANDING.store(false, Ordering::Relaxed);
    }
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
/// A `CSI 6 n` on each side of the batch brackets it, so that a host which prints one of these
/// sequences rather than consuming it can have the mess erased again. That erase is the only thing
/// here that ever writes to the screen, and it happens only on a host whose two reports differ.
///
/// Returns `None` when `/dev/tty` cannot be opened or raw mode cannot be set; callers should treat
/// that as "nothing supported".
#[cfg(unix)]
pub fn query_host_capabilities(graphics_probe: &[u8]) -> Option<HostCapabilities> {
    // Cleared before the first thing that can fail. Opening the TTY and raw mode can both return
    // early, and a guard may be entering a different terminal than the one an earlier probe
    // measured. Without this, a probe that never establishes that the current host answers at all
    // would leave the previous host's round trip standing, and teardown would write a sentinel on
    // the strength of it. Nothing is owed yet either: nothing has been asked.
    record_round_trip(None);
    set_startup_reply_outstanding(false);

    let fd = tty_open()?;
    let _fd_guard = FdGuard(fd);
    let _raw_guard = RawModeGuard::new(fd)?;

    let mut probe = Vec::with_capacity(graphics_probe.len() + 32);
    // The cursor, before and after everything that could be echoed rather than consumed. Two
    // reports in the same batch cost no extra round trip and are what `erase_echoed_probe` reads.
    probe.extend_from_slice(b"\x1b[6n");
    probe.extend_from_slice(b"\x1b[?u\x1b[?1016$p");
    probe.extend_from_slice(graphics_probe);
    probe.extend_from_slice(b"\x1b[6n");
    probe.extend_from_slice(b"\x1b[c");
    // Timed from before the write: what teardown needs to budget for is the whole trip, including
    // however long the host sat on the request before answering it.
    let asked = Instant::now();
    tty_write_all(fd, &probe)?;
    // The sentinel is on the wire, so a reply is owed from here until one is seen. Recorded now
    // rather than after the read loop because everything between is fallible: a poll or read error
    // returns without ever looking at the queue, and the reply nobody waited for still arrives.
    // "The probe failed" is not "the host owes nothing" once the question has been asked.
    set_startup_reply_outstanding(true);

    let mut buffer = Vec::with_capacity(64);
    let deadline = Instant::now() + Duration::from_millis(250);
    let mut answered = None;
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
            // The sentinel came back and was consumed here, so nothing is owed downstream.
            set_startup_reply_outstanding(false);
            answered = Some(capabilities);
            break;
        }
    }

    erase_echoed_probe(fd, &buffer);

    // No decisive reply within the budget: treat as unsupported, and leave the round trip unknown -
    // nothing here measured one. The owed-reply flag stays set. An empty buffer does not prove the
    // host is silent, only that nothing arrived inside 250ms, which is also what a link slower than
    // that looks like with a reply still in flight. Erring towards "owed" costs one bounded wait at
    // the next flush; erring the other way is the leak.
    Some(answered.unwrap_or_default())
}

/// Wipe the probe off the screen when the host printed it instead of consuming it.
///
/// The probe is written to the primary screen, before the alternate screen is entered, because what
/// it learns decides how the terminal is entered. A host that prints a sequence it does not
/// implement therefore leaves the mess in the user's scrollback, where it outlives the process.
/// `DECRQM` is the one that provokes this in practice - a standard `CSI ? 1016 $ p` whose `$`
/// intermediate some emulators drop, printing the final `p` - and it cannot be dropped from the
/// probe, because pixel-mouse reporting is worth having on every host that does implement it.
///
/// Cleaning up is only safe where the damage can be located exactly, so this asks rather than
/// assumes. The cursor was reported on both sides of the probe: a host that consumed everything
/// reports the same position twice and nothing is written, which is the case that must stay free of
/// side effects. A host that echoed reports a column further along, and the difference is what gets
/// erased - the line is already spoiled, so erasing to its end cannot destroy anything the echo had
/// not destroyed first.
///
/// A move to another row is left alone. It means the echo wrapped, and possibly scrolled, and the
/// saved position no longer names the same cell; erasing from a stale origin would take the user's
/// own output with it. Leaving a mess is better than that.
#[cfg(unix)]
fn erase_echoed_probe(fd: i32, response: &[u8]) {
    let Some(column) = echoed_from_column(response) else {
        return;
    };
    // CHA back to where the probe began, then erase what follows on that line.
    let _ = tty_write_all(fd, format!("\x1b[{column}G\x1b[K").as_bytes());
}

/// The column the probe began at, if the host echoed it and the mess is confined to that one line.
///
/// `None` covers every case that is not demonstrably safe to erase: a host that consumed the probe,
/// one that never answered `CSI 6 n`, and one whose echo left the row it started on.
#[cfg(unix)]
fn echoed_from_column(response: &[u8]) -> Option<u16> {
    let reports = cursor_reports(response);
    let [(before_row, before_column), (after_row, after_column)] = reports[..] else {
        return None;
    };
    (after_row == before_row && after_column > before_column).then_some(before_column)
}

/// The first two `CSI row ; column R` reports in a response, in the order the host sent them.
///
/// Scanned rather than parsed in sequence because the replies to the rest of the probe are
/// interleaved with these and arrive in whatever order the host schedules them.
#[cfg(unix)]
fn cursor_reports(response: &[u8]) -> Vec<(u16, u16)> {
    let mut reports = Vec::with_capacity(2);
    let mut rest = response;
    while let Some(introducer) = rest.windows(2).position(|pair| pair == [0x1b, b'[']) {
        rest = &rest[introducer + 2..];
        let Some(final_byte) = rest
            .iter()
            .position(|byte| !matches!(byte, b'0'..=b'9' | b';'))
        else {
            break;
        };
        if rest[final_byte] == b'R'
            && let Some(report) = parse_cursor_report(&rest[..final_byte])
        {
            reports.push(report);
            if reports.len() == 2 {
                break;
            }
        }
        rest = &rest[final_byte..];
    }
    reports
}

/// The `row ; column` of a cursor report, both 1-based as the terminal counts them.
#[cfg(unix)]
fn parse_cursor_report(params: &[u8]) -> Option<(u16, u16)> {
    let (row, column) = std::str::from_utf8(params).ok()?.split_once(';')?;
    Some((row.parse().ok()?, column.parse().ok()?))
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
/// transient, the startup reply still being in flight, is cleared once it has been dealt with.
///
/// **Restores are assumed not to overlap.** Nothing serializes them - the panic hook can run this
/// on a panicking thread while `Drop` runs it on another - and this function does not make that
/// safe: a second caller can still finish and restore cooked mode while the first is mid-drain.
/// That is unchanged from before the round-trip budget existed. What the read-then-settle pattern
/// avoids is making it worse: a caller that arrives while a reply is still owed sees the flag set
/// and waits for it too, rather than being handed a zero-budget plan because someone else claimed
/// the flag first. Genuine overlap wants a restore lock, which belongs above this function.
#[cfg(unix)]
pub(crate) fn flush_pending_terminal_responses_on_exit() {
    let Some(fd) = tty_open() else {
        // Nothing was drained, so nothing is settled: whatever was owed is still owed, and the next
        // caller inherits it.
        return;
    };
    let _fd_guard = FdGuard(fd);

    let outstanding = startup_reply_outstanding();
    let plan = exit_flush_plan(probe_round_trip(), outstanding);
    if plan.sentinel {
        let _ = tty_write_all(fd, b"\x1b[c");
    }
    drain_through_da_reply(fd, plan.budget);
    // SAFETY: `tcflush(TCIFLUSH)` on the controlling TTY drops unread response
    // fragments after the ordering sentinel; callers have paused the input worker.
    unsafe {
        libc::tcflush(fd, libc::TCIFLUSH);
    }
    settle_startup_reply(outstanding);
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
fn resolve_host_colors(
    parsed: &Parsed,
    previous: Option<&HostTerminalColors>,
) -> Option<HostTerminalColors> {
    let fg = parsed.fg.or_else(|| previous.map(|colors| colors.fg))?;
    let bg = parsed.bg.or_else(|| previous.map(|colors| colors.bg))?;
    let mut ansi_reported = 0u16;
    let ansi = std::array::from_fn(|index| {
        if let Some(color) = parsed.ansi[index] {
            ansi_reported |= 1 << index;
            return color;
        }
        if let Some(previous) = previous {
            ansi_reported |= previous.ansi_reported & (1 << index);
            return previous.ansi[index];
        }
        default_ansi(index as u8)
    });
    Some(HostTerminalColors {
        ansi,
        ansi_reported,
        fg,
        bg,
    })
}

#[cfg(unix)]
pub(crate) fn build_live_color_query_batch() -> Vec<u8> {
    let mut out = Vec::with_capacity(256);
    for i in 0..16 {
        out.extend_from_slice(format!("\x1b]4;{i};?\x1b\\").as_bytes());
    }
    out.extend_from_slice(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\");
    out
}

#[cfg(unix)]
fn build_query_batch() -> Vec<u8> {
    let mut out = build_live_color_query_batch();
    // Primary DA is an ordering sentinel. Its reply can only arrive after the terminal has
    // processed the preceding palette queries, so the temporary raw-mode guard stays active until
    // no palette report can be echoed by cooked mode.
    out.extend_from_slice(b"\x1b[c");
    out
}

#[cfg(unix)]
#[derive(Default)]
pub(crate) struct HostColorResponseParser {
    // This mirrors every incomplete input family in Termina's parser. Protocol state survives
    // between queries, so `at_input_boundary` cannot start a probe in the middle of a key, mouse,
    // string, UTF-8 character, or bracketed paste event.
    state: ResponseScanState,
    parsed: Parsed,
    collecting: bool,
}

#[cfg(unix)]
#[derive(Default)]
enum ResponseScanState {
    #[default]
    Ground,
    Escape,
    Ss3,
    Utf8(u8),
    Osc(Vec<u8>),
    OscEscape(Vec<u8>),
    Dcs,
    DcsEscape,
    CsiStart,
    CsiDoubleBracket,
    CsiNormalMouse(u8),
    CsiSgrMouse,
    CsiQuestion(u16),
    CsiGreater(u8),
    CsiNumbered(Vec<u8>),
    Paste(usize),
}

#[cfg(unix)]
impl HostColorResponseParser {
    pub(crate) fn start_query(&mut self) {
        self.parsed = Parsed::default();
        self.collecting = true;
    }

    pub(crate) fn push(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.push_byte(byte);
        }
    }

    pub(crate) fn finish_query(
        &mut self,
        previous: Option<&HostTerminalColors>,
    ) -> Option<HostTerminalColors> {
        self.collecting = false;
        resolve_host_colors(&self.parsed, previous)
    }

    pub(crate) fn query_complete(&self) -> bool {
        self.parsed.fg.is_some()
            && self.parsed.bg.is_some()
            && self.parsed.ansi.iter().all(Option::is_some)
    }

    pub(crate) fn at_input_boundary(&self) -> bool {
        matches!(self.state, ResponseScanState::Ground)
    }

    pub(crate) fn settle_input(&mut self) {
        // This is the only incomplete shape Termina resolves when `maybe_more` becomes false.
        // All other states remain buffered until their protocol terminator arrives.
        if matches!(self.state, ResponseScanState::Escape) {
            self.state = ResponseScanState::Ground;
        }
    }

    fn push_byte(&mut self, byte: u8) {
        const PASTE_END: &[u8] = b"\x1b[201~";
        let state = std::mem::take(&mut self.state);
        self.state = match state {
            ResponseScanState::Ground if byte == 0x1b => ResponseScanState::Escape,
            ResponseScanState::Ground => utf8_tail(byte)
                .map(ResponseScanState::Utf8)
                .unwrap_or(ResponseScanState::Ground),
            ResponseScanState::Escape if byte == b']' => ResponseScanState::Osc(Vec::new()),
            ResponseScanState::Escape if byte == b'[' => ResponseScanState::CsiStart,
            ResponseScanState::Escape if byte == b'P' => ResponseScanState::Dcs,
            ResponseScanState::Escape if byte == b'O' => ResponseScanState::Ss3,
            ResponseScanState::Escape if byte == 0x1b => ResponseScanState::Ground,
            ResponseScanState::Escape => utf8_tail(byte)
                .map(ResponseScanState::Utf8)
                .unwrap_or(ResponseScanState::Ground),
            ResponseScanState::Ss3 => ResponseScanState::Ground,
            ResponseScanState::Utf8(remaining) if byte & 0b1100_0000 == 0b1000_0000 => {
                if remaining == 1 {
                    ResponseScanState::Ground
                } else {
                    ResponseScanState::Utf8(remaining - 1)
                }
            }
            ResponseScanState::Utf8(_) => ResponseScanState::Ground,
            ResponseScanState::Osc(body) if byte == 0x07 => {
                self.capture_body(&body);
                ResponseScanState::Ground
            }
            ResponseScanState::Osc(body) if byte == 0x1b => ResponseScanState::OscEscape(body),
            ResponseScanState::Osc(mut body) => {
                body.push(byte);
                ResponseScanState::Osc(body)
            }
            ResponseScanState::OscEscape(body) if byte == b'\\' => {
                self.capture_body(&body);
                ResponseScanState::Ground
            }
            ResponseScanState::OscEscape(mut body) if byte == 0x1b => {
                body.push(0x1b);
                ResponseScanState::OscEscape(body)
            }
            ResponseScanState::OscEscape(mut body) => {
                body.extend_from_slice(&[0x1b, byte]);
                ResponseScanState::Osc(body)
            }
            ResponseScanState::Dcs if byte == 0x1b => ResponseScanState::DcsEscape,
            ResponseScanState::Dcs => ResponseScanState::Dcs,
            ResponseScanState::DcsEscape if byte == b'\\' => ResponseScanState::Ground,
            ResponseScanState::DcsEscape if byte == 0x1b => ResponseScanState::DcsEscape,
            ResponseScanState::DcsEscape => ResponseScanState::Dcs,
            ResponseScanState::CsiStart => match byte {
                b'[' => ResponseScanState::CsiDoubleBracket,
                b'M' => ResponseScanState::CsiNormalMouse(3),
                b'<' => ResponseScanState::CsiSgrMouse,
                b'?' => ResponseScanState::CsiQuestion(0),
                b'>' => ResponseScanState::CsiGreater(b'>'),
                b'0'..=b'9' => ResponseScanState::CsiNumbered(vec![byte]),
                _ => ResponseScanState::Ground,
            },
            ResponseScanState::CsiDoubleBracket => ResponseScanState::Ground,
            ResponseScanState::CsiNormalMouse(remaining) => {
                if remaining == 1 {
                    ResponseScanState::Ground
                } else {
                    ResponseScanState::CsiNormalMouse(remaining - 1)
                }
            }
            ResponseScanState::CsiSgrMouse if matches!(byte, b'M' | b'm') => {
                ResponseScanState::Ground
            }
            ResponseScanState::CsiSgrMouse => ResponseScanState::CsiSgrMouse,
            ResponseScanState::CsiQuestion(_) if matches!(byte, b'c' | b'n' | b'y') => {
                ResponseScanState::Ground
            }
            ResponseScanState::CsiQuestion(seen) if byte == b'u' && seen >= 1 => {
                ResponseScanState::Ground
            }
            ResponseScanState::CsiQuestion(seen) => {
                ResponseScanState::CsiQuestion(seen.saturating_add(1))
            }
            ResponseScanState::CsiGreater(previous) if previous == b' ' && byte == b'q' => {
                ResponseScanState::Ground
            }
            ResponseScanState::CsiGreater(_) => ResponseScanState::CsiGreater(byte),
            ResponseScanState::CsiNumbered(mut body) => {
                body.push(byte);
                if (0x40..=0x7e).contains(&byte) {
                    if body == b"200~" {
                        ResponseScanState::Paste(0)
                    } else {
                        ResponseScanState::Ground
                    }
                } else {
                    ResponseScanState::CsiNumbered(body)
                }
            }
            ResponseScanState::Paste(mut matched) => {
                if byte == PASTE_END[matched] {
                    matched += 1;
                    if matched == PASTE_END.len() {
                        ResponseScanState::Ground
                    } else {
                        ResponseScanState::Paste(matched)
                    }
                } else {
                    ResponseScanState::Paste(usize::from(byte == PASTE_END[0]))
                }
            }
        };
    }

    fn capture_body(&mut self, body: &[u8]) {
        if self.collecting {
            parse_body(body, &mut self.parsed);
        }
    }
}

#[cfg(unix)]
fn utf8_tail(byte: u8) -> Option<u8> {
    match byte {
        0xc0..=0xdf => Some(1),
        0xe0..=0xef => Some(2),
        0xf0..=0xf7 => Some(3),
        _ => None,
    }
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

/// Wait for `/dev/tty` to become readable.
///
/// macOS `poll(2)` does not support devices: on `/dev/tty` it returns at once with `POLLNVAL`, so
/// every probe would stop reading before the host answered and restore cooked mode with the replies
/// still in flight. They then echo onto the primary screen, hidden by the alternate screen until
/// exit. `select(2)` works on terminal devices there.
#[cfg(target_os = "macos")]
fn poll_readable(fd: i32, timeout_ms: i32) -> Option<bool> {
    if !(0..libc::FD_SETSIZE as i32).contains(&fd) {
        return None;
    }
    let deadline = Instant::now() + Duration::from_millis(timeout_ms.max(0) as u64);
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let mut timeout = libc::timeval {
            tv_sec: remaining.as_secs() as libc::time_t,
            tv_usec: remaining.subsec_micros() as libc::suseconds_t,
        };
        // SAFETY: `fd_set` is plain data; FD_ZERO initializes it before use, and `fd` was checked
        // against FD_SETSIZE above.
        let mut readable: libc::fd_set = unsafe { std::mem::zeroed() };
        unsafe {
            libc::FD_ZERO(&mut readable);
            libc::FD_SET(fd, &mut readable);
        }
        // SAFETY: Every pointer refers to initialized local storage that outlives the call.
        let rc = unsafe {
            libc::select(
                fd + 1,
                &mut readable,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut timeout,
            )
        };
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
        // SAFETY: `readable` was initialized above and `fd` is in range.
        return Some(unsafe { libc::FD_ISSET(fd, &readable) });
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
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
    use std::time::Duration;

    use super::{
        EXIT_FLUSH_CEILING, EXIT_FLUSH_FLOOR, ExitFlush, HostCapabilities, HostColorResponseParser,
        Parsed, build_query_batch, cursor_reports, echoed_from_column, exit_flush_plan,
        host_color_query_settled, probe_round_trip, record_round_trip, resolve_host_colors,
        scan_host_capabilities, set_startup_reply_outstanding, settle_startup_reply,
        startup_reply_outstanding,
    };
    use crate::style::{Color, HostTerminalColors};

    fn keyboard(enhancement: bool) -> Option<HostCapabilities> {
        Some(HostCapabilities {
            keyboard_enhancement: enhancement,
            ..HostCapabilities::default()
        })
    }

    #[test]
    fn host_color_query_waits_for_its_ordering_sentinel() {
        let colors = b"\x1b]10;rgb:eeee/eeee/eeee\x1b\\\
                       \x1b]11;rgb:1111/1111/1111\x1b\\";
        assert!(
            !host_color_query_settled(colors),
            "dynamic colors may arrive before the separately scheduled palette reports"
        );

        let mut ordered = colors.to_vec();
        ordered.extend_from_slice(b"\x1b]4;0;rgb:0000/0000/0000\x1b\\\x1b[?62;1;6c");
        assert!(host_color_query_settled(&ordered));
        assert!(
            build_query_batch().ends_with(b"\x1b[c"),
            "the query must put DA after every OSC color request"
        );
    }

    #[test]
    fn partial_live_palette_keeps_previous_slots_and_dynamic_colors() {
        let previous = HostTerminalColors {
            ansi: std::array::from_fn(|index| Color::Rgb(index as u8, 10, 20)),
            fg: Color::Rgb(230, 230, 230),
            bg: Color::Rgb(20, 20, 20),
            ansi_reported: HostTerminalColors::ALL_ANSI_REPORTED,
        };
        let mut parsed = Parsed::default();
        parsed.ansi[4] = Some(Color::Rgb(80, 120, 240));

        let colors = resolve_host_colors(&parsed, Some(&previous)).unwrap();

        assert_eq!(colors.ansi[4], Color::Rgb(80, 120, 240));
        assert_eq!(colors.ansi[3], previous.ansi[3]);
        assert_eq!(colors.fg, previous.fg);
        assert_eq!(colors.bg, previous.bg);
    }

    /// A terminal that answers OSC 10/11 but only some OSC 4 slots still yields colors, and the
    /// slots it skipped are marked as stand-ins rather than passed off as its palette.
    #[test]
    fn slots_missing_from_the_reply_are_not_marked_reported() {
        let mut parser = HostColorResponseParser::default();
        parser.start_query();
        parser.push(
            b"\x1b]4;6;rgb:7b7b/a6a6/a3a3\x1b\\\
              \x1b]10;rgb:ffff/ffff/ffff\x1b\\\
              \x1b]11;rgb:2222/2222/2222\x1b\\",
        );
        let colors = parser.finish_query(None).unwrap();

        assert_eq!(colors.ansi_reported, 1 << 6);
        assert_eq!(colors.reported_ansi(6), Some(Color::Rgb(0x7b, 0xa6, 0xa3)));
        assert_eq!(colors.reported_ansi(1), None);
        assert_eq!(
            colors.ansi[1],
            super::default_ansi(1),
            "the stand-in is still there for anything that only needs an RGB value"
        );
        assert_eq!(colors.reported_ansi(16), None);
    }

    /// A slot reported by an earlier query stays reported when a later reply skips it; a slot
    /// never reported stays a stand-in however often it is carried over.
    #[test]
    fn carried_slots_keep_whether_they_were_reported() {
        let previous = HostTerminalColors {
            ansi: std::array::from_fn(|index| Color::Rgb(index as u8, 10, 20)),
            ansi_reported: 1 << 3,
            fg: Color::Rgb(230, 230, 230),
            bg: Color::Rgb(20, 20, 20),
        };
        let mut parsed = Parsed::default();
        parsed.ansi[4] = Some(Color::Rgb(80, 120, 240));

        let colors = resolve_host_colors(&parsed, Some(&previous)).unwrap();

        assert_eq!(colors.ansi_reported, (1 << 3) | (1 << 4));
        assert_eq!(colors.reported_ansi(3), Some(previous.ansi[3]));
        assert_eq!(colors.reported_ansi(5), None);
    }

    #[test]
    fn live_palette_parser_ignores_osc_inside_split_bracketed_paste() {
        let mut parser = HostColorResponseParser::default();
        parser.push(b"\x1b[20");
        parser.push(b"0~literal \x1b]4;1;rgb:1111/1111/1111\x1b\\");
        assert!(
            !parser.at_input_boundary(),
            "a live query must wait for the paste terminator"
        );
        parser.start_query();
        parser.push(b" and \x1b[12c\x1b[20");
        parser.push(
            b"1~\x1b]4;0;rgb:0000/0000/0000\x1b\\\
              \x1b]10;rgb:ffff/ffff/ffff\x1b\\\
              \x1b]11;rgb:2222/2222/2222\x1b\\",
        );
        assert!(parser.at_input_boundary());

        let colors = parser.finish_query(None).unwrap();
        assert_eq!(colors.ansi[0], Color::Rgb(0, 0, 0));
        assert_eq!(
            colors.ansi[1],
            super::default_ansi(1),
            "an OSC 4 frame inside paste content must not become a palette reply"
        );
        assert_eq!(colors.fg, Color::Rgb(255, 255, 255));
        assert_eq!(colors.bg, Color::Rgb(34, 34, 34));
    }

    #[test]
    fn input_boundary_tracks_every_incomplete_termina_escape_family() {
        let mut parser = HostColorResponseParser::default();

        for (prefix, suffix) in [
            (b"\x1bO".as_slice(), b"P".as_slice()),
            (b"\x1bP1$r0;4m\x1b".as_slice(), b"\\".as_slice()),
            (b"\x1b[M !".as_slice(), b"!".as_slice()),
            (b"\x1b[[", b"A"),
            (b"\x1b[<0;1;1", b"M"),
            (b"\x1b[?1".as_slice(), b"u".as_slice()),
            (b"\x1b[>1;2 ".as_slice(), b"q".as_slice()),
            (b"\x1b[1;2".as_slice(), b"A".as_slice()),
            (
                b"\x1b]10;rgb:ffff/ffff/ffff\x1b".as_slice(),
                b"\\".as_slice(),
            ),
            (b"\xc3".as_slice(), b"\xa9".as_slice()),
            (b"\x1b\xc3".as_slice(), b"\xa9".as_slice()),
        ] {
            let mut termina = termina::Parser::default();
            parser.push(prefix);
            termina.parse(prefix, true);
            assert!(
                !parser.at_input_boundary(),
                "{prefix:?} is still incomplete for Termina"
            );
            assert!(
                termina.pop().is_none(),
                "{prefix:?} unexpectedly completed a Termina event"
            );
            parser.push(suffix);
            termina.parse(suffix, true);
            assert!(
                parser.at_input_boundary(),
                "{prefix:?}{suffix:?} completes the input event"
            );
            assert!(
                termina.pop().is_some(),
                "{prefix:?}{suffix:?} should complete a Termina event"
            );
        }

        parser.push(b"\x1b");
        assert!(!parser.at_input_boundary());
        parser.settle_input();
        assert!(parser.at_input_boundary());
    }

    #[test]
    fn live_palette_parser_ignores_osc_inside_dcs() {
        let previous = HostTerminalColors {
            ansi: std::array::from_fn(|index| super::default_ansi(index as u8)),
            fg: Color::Rgb(230, 230, 230),
            bg: Color::Rgb(20, 20, 20),
            ansi_reported: HostTerminalColors::ALL_ANSI_REPORTED,
        };
        let mut parser = HostColorResponseParser::default();
        parser.start_query();
        parser.push(b"\x1bPignored \x1b]4;1;rgb:1111/1111/1111\x07 text\x1b\\");

        let colors = parser.finish_query(Some(&previous)).unwrap();
        assert_eq!(colors.ansi[1], previous.ansi[1]);
        assert!(parser.at_input_boundary());
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
    fn an_owed_reply_is_settled_only_by_a_caller_that_drained_for_it() {
        set_startup_reply_outstanding(true);

        let observed = startup_reply_outstanding();
        assert!(observed, "the first flush sees it");

        // What a caller that never opened the TTY passes: it drained nothing, so the reply is
        // still owed and the next flush has to inherit it. Taking the flag up front would have
        // spent it here.
        settle_startup_reply(false);
        assert!(
            startup_reply_outstanding(),
            "a caller that did no work must not settle it"
        );

        settle_startup_reply(observed);
        assert!(
            !startup_reply_outstanding(),
            "settled once the queue was drained and flushed - a flag that stayed set would put the \
             ceiling on terminal drop, on panic restore, and on both sides of every \
             external-program handoff, forever"
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

    /// The batch has to bracket everything that could be echoed, or the second report describes a
    /// position the mess has already moved past and the erase starts in the wrong place.
    #[test]
    fn the_probe_asks_for_the_cursor_on_both_sides_of_itself() {
        let probe = b"\x1b[6n\x1b[?u\x1b[?1016$p\x1b[6n\x1b[c";
        assert!(probe.starts_with(b"\x1b[6n"), "before anything is written");
        assert!(
            probe.ends_with(b"\x1b[6n\x1b[c"),
            "and after everything but the sentinel"
        );
    }

    /// The case this exists for, as a terminal that drops the `$` of `CSI ? 1016 $ p` produces it:
    /// the final `p` is printed, so the cursor comes back one column further along the same row.
    #[test]
    fn a_host_that_echoed_one_line_is_erased_from_where_it_started() {
        let response = b"\x1b[12;1R\x1b[?1016;2$y\x1b[12;2R\x1b[?62;1;6c";
        assert_eq!(echoed_from_column(response), Some(1));
    }

    /// A host that consumed the probe reports the same cell twice, and must be left completely
    /// alone: erasing to the end of that line would take a right-hand prompt with it.
    #[test]
    fn a_host_that_consumed_the_probe_is_not_written_to() {
        let response = b"\x1b[12;40R\x1b[?5u\x1b[12;40R\x1b[?62;1;6c";
        assert_eq!(echoed_from_column(response), None);
    }

    /// An echo that left its row may have scrolled the screen, which makes the first report a stale
    /// name for a cell that has moved. Erasing from it would delete the user's own output.
    #[test]
    fn an_echo_that_wrapped_to_another_row_is_left_alone() {
        assert_eq!(
            echoed_from_column(b"\x1b[12;70R\x1b[13;9R\x1b[?62;1;6c"),
            None
        );
    }

    /// Nothing is written on the strength of a report that never came. A host silent about the
    /// cursor is the ordinary case for a pipe or a harness, not an error.
    #[test]
    fn a_host_that_does_not_report_its_cursor_is_not_written_to() {
        assert_eq!(echoed_from_column(b"\x1b[?62;1;6c"), None);
        assert_eq!(echoed_from_column(b"\x1b[12;1R\x1b[?62;1;6c"), None);
        assert_eq!(echoed_from_column(b""), None);
    }

    /// The reports are picked out of whatever else the host said, in the order it said them, and
    /// the scan stops at two so a later `R` cannot be read as the pair's second half.
    #[test]
    fn cursor_reports_are_found_among_the_other_replies() {
        let response = b"\x1b[?5u\x1b[3;7R\x1b_Gi=4294967295;OK\x1b\\\x1b[3;9R\x1b[3;11R";
        assert_eq!(cursor_reports(response), vec![(3, 7), (3, 9)]);
    }

    /// A malformed or truncated report is not a position. Reading one as `(0, 0)` would send the
    /// erase to the top-left corner of the screen.
    #[test]
    fn a_report_that_is_not_a_position_is_not_one() {
        assert_eq!(cursor_reports(b"\x1b[R\x1b[;R\x1b[4R\x1b[9;"), Vec::new());
    }
}
