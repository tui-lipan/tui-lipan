//! POSIX job control: stopping the app to the shell and coming back.
//!
//! Raw mode clears the tty's `ISIG` flag, so the terminal driver never turns
//! `ctrl+z` into `SIGTSTP` while an app is running — an app that wants that
//! keybinding has to ask for it ([`Context::suspend_to_shell`]). A `SIGTSTP`
//! that arrives from anywhere else (`kill -TSTP`, a parent shell) would
//! otherwise stop the process with the terminal still in raw mode, on the
//! alternate screen and with mouse tracking on: the shell prompt then draws
//! over the frozen UI and mouse motion prints escape sequences into it.
//!
//! Both paths set one request flag. The runner drains it at a frame boundary,
//! where it can hand the terminal back, stop for real, and restore it once the
//! job is foregrounded again.
//!
//! `SIGTTIN` and `SIGTTOU` cannot wait for a frame boundary. The kernel sends
//! them when a process outside the terminal's foreground process group reads
//! the tty or changes its settings, and it sends them again every time the
//! call is retried, so the input reader would never get far enough to be
//! paused. Their handler releases the terminal itself, using only
//! async-signal-safe calls, and stops on the spot.
//!
//! Stopping freezes every thread wherever it happens to be, and continuing
//! resumes them all at once, so a frame that was half written when the signal
//! landed would otherwise finish on the shell's screen. Frames are therefore
//! written through [`frame_write`], and the handler closes that path before it
//! releases anything: it marks the terminal released, waits for any frame write
//! already past that check to finish, and only then writes the release. Frame
//! bytes are discarded from then on, until the runner has taken the terminal
//! back at its next frame boundary and repainted in full. The handler itself
//! returns only once the job is in the foreground again, so a job continued
//! with `bg` stops again before any of its threads can matter.
//!
//! [`Context::suspend_to_shell`]: crate::core::component::Context::suspend_to_shell

#[cfg(all(unix, not(target_arch = "wasm32")))]
use std::sync::atomic::AtomicI32;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Targets with the POSIX job control this module needs.
const SUPPORTED: bool = cfg!(all(unix, not(target_arch = "wasm32")));

static SUSPEND_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Set by the `SIGTTIN`/`SIGTTOU` handler once it has released the terminal
/// and the job is back in the foreground.
static STOPPED_IN_BACKGROUND: AtomicBool = AtomicBool::new(false);

/// Closed by the background release, reopened by the runner once it owns the
/// terminal again. While closed, frame output is discarded.
static TERMINAL_RELEASED: AtomicBool = AtomicBool::new(false);

/// Frame writes that have found the terminal ours and not finished yet. The
/// release waits for this to reach zero, so a write it did not stop lands
/// before the release rather than after it.
static FRAME_WRITES_IN_FLIGHT: AtomicUsize = AtomicUsize::new(0);

/// Held by the one `SIGTTIN`/`SIGTTOU` handler doing the release and stop. A
/// job continued in the background can raise the signal again on another
/// thread before the first handler has stopped it; that second handler leaves
/// the stopping to the first, rather than waking after `fg` and stopping a job
/// that is back in the foreground.
#[cfg(all(unix, not(target_arch = "wasm32")))]
static RELEASE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Whether the running surface is on the alternate screen, which decides
/// whether the background release leaves it.
static USES_ALTERNATE_SCREEN: AtomicBool = AtomicBool::new(false);

/// The parent process when the handler was installed: the job-control shell,
/// as long as it is still there to continue us.
#[cfg(all(unix, not(target_arch = "wasm32")))]
static JOB_CONTROL_PARENT: AtomicI32 = AtomicI32::new(0);

/// The terminal settings the runner put in place (raw mode), captured when the
/// handler is installed so the handler can put them back. Written only while no
/// handler is installed, and read only by the handler.
#[cfg(all(unix, not(target_arch = "wasm32")))]
struct SavedTermios(std::cell::UnsafeCell<std::mem::MaybeUninit<libc::termios>>);

// SAFETY: see `RUNNER_TERMIOS`: writes and the handler's reads never overlap.
#[cfg(all(unix, not(target_arch = "wasm32")))]
#[allow(unsafe_code)]
unsafe impl Sync for SavedTermios {}

#[cfg(all(unix, not(target_arch = "wasm32")))]
static RUNNER_TERMIOS: SavedTermios =
    SavedTermios(std::cell::UnsafeCell::new(std::mem::MaybeUninit::uninit()));

/// Whether [`RUNNER_TERMIOS`] holds settings to restore.
static RUNNER_TERMIOS_SAVED: AtomicBool = AtomicBool::new(false);

/// What the background release writes: every terminal mode the runner turns on
/// that the shell would otherwise inherit, mouse reporting first. It leads with
/// CAN, which abandons any escape sequence a frame was halfway through writing,
/// so the resets that follow are read as resets, and then ends synchronized
/// output: a frame stopped between its `?2026h` and `?2026l` would otherwise
/// keep the terminal from presenting anything the shell draws.
///
/// It leaves the tty's termios settings alone. By the time a background signal
/// arrives another process group owns the terminal, and those settings are now
/// its own.
#[cfg_attr(not(all(unix, not(target_arch = "wasm32"))), allow(dead_code))]
const BACKGROUND_RELEASE: &[u8] = b"\x18\x1b[?2026l\
    \x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?1016l\x1b[?1006l\x1b[?1015l\
    \x1b[?2004l\x1b[?1004l\x1b[?2031l\x1b[?7h\x1b[?25h";

/// Pops the keyboard enhancement an inline surface pushed. The kitty protocol
/// keeps one stack per screen, so leaving the alternate screen covers a
/// fullscreen surface, but an inline one shares the main screen with the shell.
#[cfg_attr(not(all(unix, not(target_arch = "wasm32"))), allow(dead_code))]
const POP_KEYBOARD_ENHANCEMENT: &[u8] = b"\x1b[<1u";

/// Leaves the alternate screen, after [`BACKGROUND_RELEASE`] on a fullscreen surface.
#[cfg_attr(not(all(unix, not(target_arch = "wasm32"))), allow(dead_code))]
const LEAVE_ALTERNATE_SCREEN: &[u8] = b"\x1b[?1049l";

/// Ask the runner to suspend at the next frame boundary.
///
/// Does nothing where job control is unavailable, so a `ctrl+z` keybinding can
/// be wired unconditionally.
pub(crate) fn request_suspend() {
    if SUPPORTED {
        SUSPEND_REQUESTED.store(true, Ordering::SeqCst);
    }
}

/// Take a pending suspend request, from either the keybinding or `SIGTSTP`.
pub(crate) fn take_suspend_request() -> bool {
    SUSPEND_REQUESTED.swap(false, Ordering::SeqCst)
}

/// Whether the process was stopped in the background since the last call, with
/// the terminal released, so the runner has to take it back.
pub(crate) fn take_background_stop() -> bool {
    STOPPED_IN_BACKGROUND.swap(false, Ordering::SeqCst)
}

/// Whether a background stop has released the terminal and the runner has not
/// taken it back yet. Frame output must not reach the terminal meanwhile.
pub(crate) fn terminal_released() -> bool {
    TERMINAL_RELEASED.load(Ordering::SeqCst)
}

/// Run `write`, one write of frame output, unless a background stop has
/// released the terminal; then return `discarded` without running it.
///
/// The check and the write form one step the release cannot come between. The
/// write is counted in flight before the check, and the release, which marks
/// the terminal released first, waits for the count to drain before it writes
/// anything: so either this sees the terminal released and drops the frame, or
/// the release sees this write and lets it land first. The background signals
/// are blocked on this thread meanwhile, so the handler never runs on top of
/// the very write it would be waiting for; it is delivered to another thread,
/// or here once the write is done.
///
/// `write` must not block for long: the release waits for it.
pub(crate) fn frame_write<T>(discarded: T, write: impl FnOnce() -> T) -> T {
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    let unblock = block_background_signals();
    FRAME_WRITES_IN_FLIGHT.fetch_add(1, Ordering::SeqCst);
    let result = if TERMINAL_RELEASED.load(Ordering::SeqCst) {
        discarded
    } else {
        write()
    };
    FRAME_WRITES_IN_FLIGHT.fetch_sub(1, Ordering::SeqCst);
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    restore_signal_mask(&unblock);
    result
}

/// Block `SIGTTIN` and `SIGTTOU` on this thread, returning the mask to restore.
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn block_background_signals() -> libc::sigset_t {
    // SAFETY: both sets are initialised by `sigemptyset` before use, and
    // `pthread_sigmask` only reads `blocked` and writes `previous`.
    #[allow(unsafe_code)]
    unsafe {
        let mut blocked: libc::sigset_t = std::mem::zeroed();
        let mut previous: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut blocked);
        libc::sigemptyset(&mut previous);
        libc::sigaddset(&mut blocked, libc::SIGTTIN);
        libc::sigaddset(&mut blocked, libc::SIGTTOU);
        libc::pthread_sigmask(libc::SIG_BLOCK, &blocked, &mut previous);
        previous
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn restore_signal_mask(previous: &libc::sigset_t) {
    // SAFETY: `previous` came from `pthread_sigmask` and is only read.
    #[allow(unsafe_code)]
    unsafe {
        libc::pthread_sigmask(libc::SIG_SETMASK, previous, std::ptr::null_mut())
    };
}

/// Record that the runner owns the terminal again after a background stop.
pub(crate) fn reclaim_terminal() {
    TERMINAL_RELEASED.store(false, Ordering::SeqCst);
}

/// Keep a background stop pending after taking the terminal back failed, so the
/// next frame boundary tries again. Frame output stays closed until one works.
pub(crate) fn retry_background_stop() {
    if terminal_released() {
        STOPPED_IN_BACKGROUND.store(true, Ordering::SeqCst);
    }
}

/// Keeps the job-control stop signals routed through the runner for as long as
/// it owns the terminal, and puts back whatever handled them before on the way
/// out.
pub(crate) struct StopSignalGuard {
    /// The dispositions the runner's replaced, all three or none: installing is
    /// all or nothing.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    previous: Option<[libc::sigaction; 3]>,
}

impl StopSignalGuard {
    #[cfg(test)]
    fn installed(&self) -> bool {
        cfg_select! {
            all(unix, not(target_arch = "wasm32")) => self.previous.is_some(),
            _ => false,
        }
    }
}

impl Drop for StopSignalGuard {
    fn drop(&mut self) {
        #[cfg(all(unix, not(target_arch = "wasm32")))]
        if let Some(previous) = &self.previous {
            restore_dispositions(STOP_SIGNALS, previous, STOP_SIGNALS.len());
        }
        RUNNER_TERMIOS_SAVED.store(false, Ordering::SeqCst);
        SUSPEND_REQUESTED.store(false, Ordering::SeqCst);
        STOPPED_IN_BACKGROUND.store(false, Ordering::SeqCst);
        TERMINAL_RELEASED.store(false, Ordering::SeqCst);
    }
}

/// Route the stop signals to the runner until the returned guard drops.
///
/// `uses_alternate_screen` must match the running surface: a background stop
/// leaves the alternate screen only when the app entered it.
pub(crate) fn install_stop_handler(uses_alternate_screen: bool) -> StopSignalGuard {
    USES_ALTERNATE_SCREEN.store(uses_alternate_screen, Ordering::SeqCst);
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        // SAFETY: `getppid` takes no arguments and cannot fail.
        #[allow(unsafe_code)]
        JOB_CONTROL_PARENT.store(unsafe { libc::getppid() }, Ordering::SeqCst);
        // SAFETY: no handler is installed yet, so nothing reads the cell while
        // `tcgetattr` fills it; `isatty` and `tcgetattr` take no other pointers.
        #[allow(unsafe_code)]
        let saved = unsafe {
            libc::isatty(libc::STDIN_FILENO) == 1
                && libc::tcgetattr(libc::STDIN_FILENO, (*RUNNER_TERMIOS.0.get()).as_mut_ptr()) == 0
        };
        RUNNER_TERMIOS_SAVED.store(saved, Ordering::SeqCst);
        StopSignalGuard {
            previous: install_runner_dispositions(STOP_SIGNALS),
        }
    }
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    {
        StopSignalGuard {}
    }
}

/// Stop this process the way the terminal driver would, and return once the job
/// has been foregrounded again.
///
/// The signal goes to our whole process group with the OS default disposition
/// back in place — group-wide because that is what a `ctrl+z` at the tty does,
/// and what a shell tracking the job expects to see stop. Children that must
/// keep running while the TUI sleeps belong in their own process group.
pub(crate) fn stop_until_continued() {
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        set_stop_dispositions(StopDispositions::Default);
        // SAFETY: `killpg(0, …)` targets the caller's own process group and
        // takes no pointers. It returns once we are continued (`SIGCONT`).
        #[allow(unsafe_code)]
        unsafe {
            libc::killpg(0, libc::SIGTSTP)
        };
        set_stop_dispositions(StopDispositions::Runner);
    }
}

/// `SIGTSTP` handler: an atomic store is all it does, because that is one of
/// the few things a signal handler may safely do. The terminal work happens
/// later, on the runner's own thread.
#[cfg(all(unix, not(target_arch = "wasm32")))]
extern "C" fn note_stop_request(_signal: libc::c_int) {
    SUSPEND_REQUESTED.store(true, Ordering::SeqCst);
}

/// `SIGTTIN`/`SIGTTOU` handler: we have lost the terminal while its modes are
/// still ours. Close the frame output, turn the modes off, and stay stopped
/// until the job is in the foreground again; the runner does the rest.
///
/// Everything here is on POSIX's async-signal-safe list: atomics, `write`,
/// `poll`, `raise`, `tcgetpgrp`, `tcsetattr`, `getpgrp` and `getppid`. Both
/// background signals are blocked while it runs, which is what lets its own
/// `write` through even on a terminal with `tostop` set. Those calls can change
/// `errno` under whatever code the signal interrupted, so the handler puts it
/// back on the way out, whichever way it leaves.
#[cfg(all(unix, not(target_arch = "wasm32")))]
extern "C" fn release_and_stop(_signal: libc::c_int) {
    preserving_errno(release_and_stop_body);
}

/// Run `body`, then put `errno` back as it was before.
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn preserving_errno(body: impl FnOnce()) {
    let saved = errno::get();
    body();
    errno::set(saved);
}

/// This thread's `errno`, through the platform `errno` APIs `libc` exposes,
/// based on the standard library's platform cases. (std reads RTEMS's and
/// DragonFly's thread-local `errno` directly; here they go through `libc`'s
/// `__errno` and `__errno_location`.) A target outside it has no `errno` the handler could put back, so
/// the background signals are left to their default there ([`errno::KNOWN`]).
#[cfg(all(unix, not(target_arch = "wasm32")))]
mod errno {
    cfg_select! {
        target_os = "vxworks" => {
            pub(super) const KNOWN: bool = true;

            pub(super) fn get() -> libc::c_int {
                // SAFETY: takes no arguments; reads the calling task's `errno`.
                #[allow(unsafe_code)]
                unsafe {
                    libc::errnoGet()
                }
            }

            pub(super) fn set(value: libc::c_int) {
                // SAFETY: takes a value; sets the calling task's `errno`.
                #[allow(unsafe_code)]
                unsafe {
                    libc::errnoSet(value)
                };
            }
        }
        any(
            target_os = "linux",
            target_os = "emscripten",
            target_os = "fuchsia",
            target_os = "l4re",
            target_os = "hurd",
            target_os = "redox",
            target_os = "dragonfly",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "cygwin",
            target_os = "android",
            target_os = "nuttx",
            target_env = "newlib",
            target_os = "solaris",
            target_os = "illumos",
            target_os = "nto",
            target_os = "freebsd",
            target_vendor = "apple",
            target_os = "haiku",
            target_os = "aix",
        ) => {
            pub(super) const KNOWN: bool = true;

            pub(super) fn get() -> libc::c_int {
                // SAFETY: `location` points at the calling thread's `errno`,
                // which lives as long as the thread.
                #[allow(unsafe_code)]
                unsafe {
                    *location()
                }
            }

            pub(super) fn set(value: libc::c_int) {
                // SAFETY: as in `get`.
                #[allow(unsafe_code)]
                unsafe {
                    *location() = value
                };
            }

            #[allow(unsafe_code)]
            fn location() -> *mut libc::c_int {
                // SAFETY: every accessor here takes no arguments and returns
                // the calling thread's `errno`.
                unsafe {
                    cfg_select! {
                        any(
                            target_os = "netbsd",
                            target_os = "openbsd",
                            target_os = "cygwin",
                            target_os = "android",
                            target_os = "nuttx",
                            target_env = "newlib",
                        ) => libc::__errno(),
                        any(target_os = "solaris", target_os = "illumos") => libc::___errno(),
                        target_os = "nto" => libc::__get_errno_ptr(),
                        any(target_os = "freebsd", target_vendor = "apple") => libc::__error(),
                        target_os = "haiku" => libc::_errnop(),
                        target_os = "aix" => libc::_Errno(),
                        // Only the targets listed above reach this: Linux,
                        // Emscripten, Fuchsia, L4Re, Hurd, Redox and DragonFly.
                        _ => libc::__errno_location(),
                    }
                }
            }
        }
        _ => {
            pub(super) const KNOWN: bool = false;

            /// Never called: without a known `errno` the handler that would
            /// use it is not installed.
            pub(super) fn get() -> libc::c_int {
                0
            }

            pub(super) fn set(_value: libc::c_int) {}
        }
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn release_and_stop_body() {
    // The kernel only raises these for a background job, but one can still
    // reach us after `fg` has continued the job and given it the terminal
    // back: raised on one thread while another was stopping the job. Whatever
    // raised it succeeds when it restarts, so there is nothing to do. (A stop
    // asked for with `kill` belongs to `SIGTSTP`.)
    if in_foreground() {
        return;
    }
    if RELEASE_IN_PROGRESS.swap(true, Ordering::SeqCst) {
        return;
    }
    let pop_keyboard = !USES_ALTERNATE_SCREEN.load(Ordering::SeqCst)
        && crate::backend::ratatui_backend::MAIN_SCREEN_KEYBOARD_PUSHED
            .swap(false, Ordering::SeqCst);
    release_terminal(
        libc::STDOUT_FILENO,
        USES_ALTERNATE_SCREEN.load(Ordering::SeqCst),
        pop_keyboard,
    );
    // Checked before every stop, not only after one: by the time the release is
    // written the job may already be back in the foreground.
    while !in_foreground() && job_control_parent_present() {
        // SAFETY: `raise` takes no pointers. `SIGSTOP` cannot be caught, so this
        // returns only once the process has been continued.
        #[allow(unsafe_code)]
        unsafe {
            libc::raise(libc::SIGSTOP)
        };
    }
    // The shell has had the terminal in its own mode, usually line-buffered.
    // Whatever read raised the signal resumes when this returns, possibly with
    // the bytes it was after already taken by the shell; in raw mode the next
    // keypress completes it, where line mode would hold it until Enter and keep
    // the runner from ever reaching its next frame. Only in the foreground,
    // where the settings are ours to change.
    if in_foreground() && RUNNER_TERMIOS_SAVED.load(Ordering::SeqCst) {
        // SAFETY: the cell was filled before this handler was installed and is
        // not written while it is; `tcsetattr` only reads through the pointer.
        #[allow(unsafe_code)]
        unsafe {
            libc::tcsetattr(
                libc::STDIN_FILENO,
                libc::TCSANOW,
                (*RUNNER_TERMIOS.0.get()).as_ptr(),
            )
        };
    }
    STOPPED_IN_BACKGROUND.store(true, Ordering::SeqCst);
    RELEASE_IN_PROGRESS.store(false, Ordering::SeqCst);
}

/// Whether our process group owns the terminal. A stdout that is not a
/// terminal has no foreground to lose, so it counts as owned.
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn in_foreground() -> bool {
    // SAFETY: both calls take no pointers.
    #[allow(unsafe_code)]
    let (foreground, own) = unsafe { (libc::tcgetpgrp(libc::STDOUT_FILENO), libc::getpgrp()) };
    foreground < 0 || foreground == own
}

/// Whether the shell that started us is still our parent. Once it is gone the
/// job is orphaned and nobody will foreground it, so stopping again would be
/// for good.
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn job_control_parent_present() -> bool {
    // SAFETY: `getppid` takes no arguments and cannot fail.
    #[allow(unsafe_code)]
    let parent = unsafe { libc::getppid() };
    parent == JOB_CONTROL_PARENT.load(Ordering::SeqCst)
}

/// Close frame output, wait out the frame writes already past it, then write
/// the release, so nothing a frame writes can land after it.
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn release_terminal(fd: libc::c_int, uses_alternate_screen: bool, pop_keyboard: bool) {
    TERMINAL_RELEASED.store(true, Ordering::SeqCst);
    while FRAME_WRITES_IN_FLIGHT.load(Ordering::SeqCst) != 0 {
        // A timed wait that is async-signal-safe, unlike `nanosleep`: `poll`
        // on no descriptors for a millisecond.
        // SAFETY: zero descriptors, so the null array is never read.
        #[allow(unsafe_code)]
        unsafe {
            libc::poll(std::ptr::null_mut(), 0, 1)
        };
    }
    write_background_release(fd, uses_alternate_screen, pop_keyboard);
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn write_background_release(fd: libc::c_int, uses_alternate_screen: bool, pop_keyboard: bool) {
    write_all_signal_safe(fd, BACKGROUND_RELEASE);
    if pop_keyboard {
        write_all_signal_safe(fd, POP_KEYBOARD_ENHANCEMENT);
    }
    if uses_alternate_screen {
        write_all_signal_safe(fd, LEAVE_ALTERNATE_SCREEN);
    }
}

/// `write` until done, retrying interruptions and giving up on any other error:
/// there is nobody to report it to from a signal handler.
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn write_all_signal_safe(fd: libc::c_int, mut bytes: &[u8]) {
    while !bytes.is_empty() {
        // SAFETY: the pointer and length describe the live `bytes` slice.
        #[allow(unsafe_code)]
        let written = unsafe { libc::write(fd, bytes.as_ptr().cast(), bytes.len()) };
        match usize::try_from(written) {
            Ok(0) => return,
            Ok(written) => bytes = &bytes[written.min(bytes.len())..],
            Err(_) if std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted => {
            }
            Err(_) => return,
        }
    }
}

/// The signals the runner routes through its own handlers, in install order.
#[cfg(all(unix, not(target_arch = "wasm32")))]
const STOP_SIGNALS: [libc::c_int; 3] = [libc::SIGTSTP, libc::SIGTTIN, libc::SIGTTOU];

#[cfg(all(unix, not(target_arch = "wasm32")))]
#[derive(Clone, Copy)]
enum StopDispositions {
    /// The handlers above.
    Runner,
    /// The OS default: stop the process.
    Default,
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
impl StopDispositions {
    fn handler(self, signal: libc::c_int) -> libc::sighandler_t {
        match (self, signal) {
            (Self::Default, _) => libc::SIG_DFL,
            (Self::Runner, libc::SIGTSTP) => note_stop_request as *const () as libc::sighandler_t,
            // Without an `errno` to put back, the handler could not run safely;
            // the background signals keep their default stop there.
            (Self::Runner, _) if !errno::KNOWN => libc::SIG_DFL,
            (Self::Runner, _) => release_and_stop as *const () as libc::sighandler_t,
        }
    }
}

/// Install the runner's handlers for all of `signals` ([`STOP_SIGNALS`] outside
/// tests), or for none: a failure part way puts back the ones already replaced.
/// Returns what they replaced.
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn install_runner_dispositions(signals: [libc::c_int; 3]) -> Option<[libc::sigaction; 3]> {
    // SAFETY: an all-zero `sigaction` is a valid value to be overwritten.
    #[allow(unsafe_code)]
    let mut previous: [libc::sigaction; 3] = unsafe { std::mem::zeroed() };
    for (installed, signal) in signals.into_iter().enumerate() {
        let handler = StopDispositions::Runner.handler(signal);
        if !set_disposition(signal, handler, Some(&mut previous[installed])) {
            restore_dispositions(signals, &previous, installed);
            return None;
        }
    }
    Some(previous)
}

/// Put back the first `count` of `previous`, in `signals` order.
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn restore_dispositions(signals: [libc::c_int; 3], previous: &[libc::sigaction; 3], count: usize) {
    for (signal, action) in signals.into_iter().zip(previous).take(count) {
        // SAFETY: `action` came from `sigaction` itself and is only read.
        #[allow(unsafe_code)]
        unsafe {
            libc::sigaction(signal, action, std::ptr::null_mut())
        };
    }
}

/// Switch between the runner's handlers and the default around a stop the
/// runner makes itself. Best effort: the guard owns the dispositions.
#[cfg(all(unix, not(target_arch = "wasm32")))]
fn set_stop_dispositions(dispositions: StopDispositions) {
    for signal in STOP_SIGNALS {
        set_disposition(signal, dispositions.handler(signal), None);
    }
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn set_disposition(
    signal: libc::c_int,
    handler: libc::sighandler_t,
    previous: Option<&mut libc::sigaction>,
) -> bool {
    // SAFETY: `action` is a zeroed `sigaction` filled in through libc's own
    // accessors before use, and `sigaction` copies it; `previous`, when given,
    // is a valid place for the old disposition, and null means "do not report
    // it".
    #[allow(unsafe_code)]
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler;
        libc::sigemptyset(&mut action.sa_mask);
        // Block both background signals while either handler runs, so the
        // release's own `write` is never stopped by `tostop`.
        libc::sigaddset(&mut action.sa_mask, libc::SIGTTIN);
        libc::sigaddset(&mut action.sa_mask, libc::SIGTTOU);
        // Restart interrupted calls, so a stop never surfaces as an EINTR
        // error. Frame writes are never among them: `frame_write` blocks
        // the background signals around each one.
        action.sa_flags = libc::SA_RESTART;
        let previous = previous.map_or(std::ptr::null_mut(), |previous| previous as *mut _);
        libc::sigaction(signal, &action, previous) == 0
    }
}

#[cfg(all(test, feature = "terminal-images"))]
pub(crate) fn lock_test_terminal_state() -> std::sync::MutexGuard<'static, ()> {
    tests::STATE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

// The buffered test output holds lock_test_terminal_state for its whole test.
#[cfg(all(test, feature = "terminal-images"))]
pub(crate) fn with_released_terminal(
    _state: &std::sync::MutexGuard<'static, ()>,
    test: impl FnOnce(),
) {
    struct Restore;
    impl Drop for Restore {
        fn drop(&mut self) {
            TERMINAL_RELEASED.store(false, Ordering::SeqCst);
        }
    }
    let _restore = Restore;
    TERMINAL_RELEASED.store(true, Ordering::SeqCst);
    test();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Held by tests that change the module's process-wide state.
    pub(super) static STATE: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn requests_are_taken_once_and_cleared_by_the_guard() {
        let _state = STATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert!(!take_suspend_request(), "no request is pending initially");

        let guard = install_stop_handler(true);
        assert_eq!(
            guard.installed(),
            SUPPORTED,
            "the handler installs exactly where job control exists"
        );

        request_suspend();
        assert_eq!(take_suspend_request(), SUPPORTED);
        assert!(!take_suspend_request(), "a request is delivered once");

        // A failed takeover keeps a background stop pending, and only while the
        // frame output is still closed.
        retry_background_stop();
        assert!(
            !take_background_stop(),
            "nothing to retry while output is open"
        );
        TERMINAL_RELEASED.store(true, Ordering::SeqCst);
        retry_background_stop();
        assert!(take_background_stop());
        reclaim_terminal();
        assert!(!terminal_released());

        // State that outlives the runner must not affect a later one.
        request_suspend();
        STOPPED_IN_BACKGROUND.store(true, Ordering::SeqCst);
        TERMINAL_RELEASED.store(true, Ordering::SeqCst);
        drop(guard);
        assert!(!take_suspend_request());
        assert!(!take_background_stop());
        assert!(!terminal_released());
    }

    /// The handler installed for `signal` right now.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    fn disposition(signal: libc::c_int) -> libc::sighandler_t {
        // SAFETY: a null new action only reads the current one into `current`.
        #[allow(unsafe_code)]
        unsafe {
            let mut current: libc::sigaction = std::mem::zeroed();
            libc::sigaction(signal, std::ptr::null(), &mut current);
            current.sa_sigaction
        }
    }

    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn handlers_install_all_together_or_not_at_all() {
        let _state = STATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let runner = release_and_stop as *const () as libc::sighandler_t;
        let before = [libc::SIGTSTP, libc::SIGTTIN].map(disposition);

        // `SIGKILL` cannot be caught, so the third install fails, and the two
        // before it must be put back rather than left behind.
        let failed = install_runner_dispositions([libc::SIGTSTP, libc::SIGTTIN, libc::SIGKILL]);
        assert!(
            failed.is_none(),
            "an install that fails part way reports it"
        );
        assert_eq!([libc::SIGTSTP, libc::SIGTTIN].map(disposition), before);

        let guard = install_stop_handler(true);
        assert!(guard.installed());
        assert_eq!(disposition(libc::SIGTTIN), runner);
        assert_eq!(disposition(libc::SIGTTOU), runner);
        drop(guard);
        assert_eq!(
            [libc::SIGTSTP, libc::SIGTTIN].map(disposition),
            before,
            "the guard puts back what was there before"
        );
    }

    /// This thread's `errno`.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    fn errno() -> libc::c_int {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
    }

    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn the_background_handler_leaves_errno_as_it_found_it() {
        // The handler's own calls fail and set `errno`, as `tcgetpgrp` does on
        // anything that is not a terminal. The code it interrupted must still
        // see the error it was about to read.
        let (read_end, write_end) = pipe();
        errno::set(libc::EAGAIN);
        preserving_errno(|| {
            // SAFETY: `tcgetpgrp` takes no pointers; a pipe is not a terminal.
            #[allow(unsafe_code)]
            let group = unsafe { libc::tcgetpgrp(read_end) };
            assert_eq!(group, -1);
            assert_eq!(errno(), libc::ENOTTY, "the body really changed errno");
        });
        assert_eq!(errno(), libc::EAGAIN);
        drain_pipe(read_end, write_end);
    }

    /// A pipe, as two raw descriptors: read end, write end.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    fn pipe() -> (libc::c_int, libc::c_int) {
        let mut fds = [0; 2];
        // SAFETY: `fds` has room for the two descriptors `pipe` writes.
        #[allow(unsafe_code)]
        let created = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(created, 0, "pipe");
        (fds[0], fds[1])
    }

    /// Close `write_end`, then read everything written to the pipe.
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    fn drain_pipe(read_end: libc::c_int, write_end: libc::c_int) -> Vec<u8> {
        // SAFETY: both descriptors came from `pipe` and are closed once: the
        // write end here, the read end by the `File` that takes it over.
        #[allow(unsafe_code)]
        let mut reader = unsafe {
            libc::close(write_end);
            <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(read_end)
        };
        let mut written = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut written).expect("read pipe");
        written
    }

    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn the_release_waits_for_a_frame_write_already_past_the_check() {
        let _state = STATE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        reclaim_terminal();
        let (read_end, write_end) = pipe();
        let (checked, writer_checked) = std::sync::mpsc::channel();
        let (resume, writer_resumes) = std::sync::mpsc::channel::<()>();

        // A frame writer that finds the terminal still ours, then loses the CPU
        // before its `write`.
        let writer = std::thread::spawn(move || {
            frame_write(false, || {
                checked.send(()).unwrap();
                writer_resumes.recv().unwrap();
                write_all_signal_safe(write_end, b"FRAME");
                true
            })
        });
        writer_checked.recv().unwrap();

        let release = std::thread::spawn(move || release_terminal(write_end, true, false));
        std::thread::sleep(std::time::Duration::from_millis(100));
        assert!(
            !release.is_finished(),
            "the release waits for the frame write it did not stop"
        );
        assert!(
            terminal_released(),
            "and closes frame output before waiting"
        );

        resume.send(()).unwrap();
        assert!(writer.join().unwrap(), "the frame write ran");
        release.join().unwrap();
        assert!(
            !frame_write(false, || true),
            "a frame write after the release is dropped without running"
        );
        reclaim_terminal();

        let written = drain_pipe(read_end, write_end);
        assert!(
            written.starts_with(b"FRAME\x18\x1b[?2026l"),
            "the frame lands whole, before the release: {written:?}"
        );
        assert!(written.ends_with(LEAVE_ALTERNATE_SCREEN));
    }

    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn background_release_hands_back_every_mode_the_runner_turned_on() {
        let release = |uses_alternate_screen: bool, pop_keyboard: bool| {
            let (read_end, write_end) = pipe();
            write_background_release(write_end, uses_alternate_screen, pop_keyboard);
            drain_pipe(read_end, write_end)
        };
        let contains = |haystack: &[u8], needle: &[u8]| {
            haystack
                .windows(needle.len())
                .any(|window| window == needle)
        };

        let fullscreen = release(true, false);
        assert_eq!(
            fullscreen.first(),
            Some(&0x18),
            "CAN ends a half-written frame"
        );
        for mode in [
            "2026", "1000", "1002", "1003", "1006", "1015", "1016", "1004", "2004", "2031", "1049",
        ] {
            assert!(
                contains(&fullscreen, format!("\x1b[?{mode}l").as_bytes()),
                "fullscreen release resets mode {mode}"
            );
        }
        assert!(
            fullscreen.starts_with(b"\x18\x1b[?2026l"),
            "synchronized output ends before anything else, so every reset is presented"
        );
        assert!(fullscreen.ends_with(LEAVE_ALTERNATE_SCREEN));
        assert!(
            !contains(&fullscreen, POP_KEYBOARD_ENHANCEMENT),
            "the alternate screen keeps its own keyboard stack"
        );

        let inline = release(false, true);
        assert!(
            !contains(&inline, LEAVE_ALTERNATE_SCREEN),
            "an inline surface was never on the alternate screen"
        );
        assert!(
            inline.ends_with(POP_KEYBOARD_ENHANCEMENT),
            "an inline surface pops the keyboard enhancement it pushed onto the shell's screen"
        );
        assert_eq!(release(false, false), BACKGROUND_RELEASE);
    }
}
