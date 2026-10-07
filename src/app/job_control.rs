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
//! landed would otherwise finish on the shell's screen. Before it releases
//! anything, the handler closes the frame output ([`terminal_released`]):
//! frame bytes are discarded from then on, until the runner has taken the
//! terminal back at its next frame boundary and repainted in full. The handler
//! itself returns only once the job is in the foreground again, so a job
//! continued with `bg` stops again before any of its threads can matter.
//!
//! [`Context::suspend_to_shell`]: crate::core::component::Context::suspend_to_shell

use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};

/// Targets with the POSIX job control this module needs.
const SUPPORTED: bool = cfg!(all(unix, not(target_arch = "wasm32")));

static SUSPEND_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Set by the `SIGTTIN`/`SIGTTOU` handler once it has released the terminal
/// and the job is back in the foreground.
static STOPPED_IN_BACKGROUND: AtomicBool = AtomicBool::new(false);

/// Closed by the background release, reopened by the runner once it owns the
/// terminal again. While closed, frame output is discarded.
static TERMINAL_RELEASED: AtomicBool = AtomicBool::new(false);

/// Held by the one `SIGTTIN`/`SIGTTOU` handler doing the release and stop. A
/// job continued in the background can raise the signal again on another
/// thread before the first handler has stopped it; that second handler leaves
/// the stopping to the first, rather than waking after `fg` and stopping a job
/// that is back in the foreground.
static RELEASE_IN_PROGRESS: AtomicBool = AtomicBool::new(false);

/// Whether the running surface is on the alternate screen, which decides
/// whether the background release leaves it.
static USES_ALTERNATE_SCREEN: AtomicBool = AtomicBool::new(false);

/// The parent process when the handler was installed: the job-control shell,
/// as long as it is still there to continue us.
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
/// it owns the terminal, and hands them back to the OS default on the way out.
pub(crate) struct StopSignalGuard {
    installed: bool,
}

impl Drop for StopSignalGuard {
    fn drop(&mut self) {
        #[cfg(all(unix, not(target_arch = "wasm32")))]
        if self.installed {
            set_stop_dispositions(StopDispositions::Default);
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
            installed: set_stop_dispositions(StopDispositions::Runner),
        }
    }
    #[cfg(not(all(unix, not(target_arch = "wasm32"))))]
    {
        StopSignalGuard { installed: false }
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
/// Everything here is async-signal-safe: atomics, `write`, `raise`,
/// `tcgetpgrp`, `tcsetattr`, `getpgrp` and `getppid`. Both background signals are blocked
/// while it runs, which is what lets its own `write` through even on a
/// terminal with `tostop` set.
#[cfg(all(unix, not(target_arch = "wasm32")))]
extern "C" fn release_and_stop(_signal: libc::c_int) {
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
    TERMINAL_RELEASED.store(true, Ordering::SeqCst);
    let pop_keyboard = !USES_ALTERNATE_SCREEN.load(Ordering::SeqCst)
        && crate::backend::ratatui_backend::MAIN_SCREEN_KEYBOARD_PUSHED
            .swap(false, Ordering::SeqCst);
    write_background_release(
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

#[cfg(all(unix, not(target_arch = "wasm32")))]
#[derive(Clone, Copy)]
enum StopDispositions {
    /// The handlers above.
    Runner,
    /// The OS default: stop the process.
    Default,
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn set_stop_dispositions(dispositions: StopDispositions) -> bool {
    let (stop, background) = match dispositions {
        StopDispositions::Runner => (
            note_stop_request as *const () as libc::sighandler_t,
            release_and_stop as *const () as libc::sighandler_t,
        ),
        StopDispositions::Default => (libc::SIG_DFL, libc::SIG_DFL),
    };
    // Interrupted calls restart after `SIGTSTP` and `SIGTTIN`, so neither
    // surfaces as an EINTR error in the input path. Not after `SIGTTOU`: a
    // frame `write` that `tostop` held back would restart below the frame
    // output's check and land on the screen the release just handed over.
    // Failing with EINTR instead sends it back through the check, which
    // discards it.
    let stop_set = set_disposition(libc::SIGTSTP, stop, true);
    let input_set = set_disposition(libc::SIGTTIN, background, true);
    let output_set = set_disposition(libc::SIGTTOU, background, false);
    stop_set && input_set && output_set
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn set_disposition(signal: libc::c_int, handler: libc::sighandler_t, restart: bool) -> bool {
    // SAFETY: `action` is a zeroed `sigaction` filled in through libc's own
    // accessors before use, and `sigaction` copies it; the null third argument
    // means "do not report the previous disposition".
    #[allow(unsafe_code)]
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler;
        libc::sigemptyset(&mut action.sa_mask);
        // Block both background signals while either handler runs, so the
        // release's own `write` is never stopped by `tostop`.
        libc::sigaddset(&mut action.sa_mask, libc::SIGTTIN);
        libc::sigaddset(&mut action.sa_mask, libc::SIGTTOU);
        action.sa_flags = if restart { libc::SA_RESTART } else { 0 };
        libc::sigaction(signal, &action, std::ptr::null_mut()) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_taken_once_and_cleared_by_the_guard() {
        assert!(!take_suspend_request(), "no request is pending initially");

        let guard = install_stop_handler(true);
        assert_eq!(
            guard.installed, SUPPORTED,
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

    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn background_release_hands_back_every_mode_the_runner_turned_on() {
        let release = |uses_alternate_screen: bool, pop_keyboard: bool| {
            let mut fds = [0; 2];
            // SAFETY: `fds` has room for the two descriptors `pipe` writes.
            #[allow(unsafe_code)]
            let created = unsafe { libc::pipe(fds.as_mut_ptr()) };
            assert_eq!(created, 0, "pipe");
            write_background_release(fds[1], uses_alternate_screen, pop_keyboard);
            // SAFETY: both descriptors came from `pipe` above and are closed once.
            #[allow(unsafe_code)]
            unsafe {
                libc::close(fds[1]);
            }
            let mut written = Vec::new();
            // SAFETY: `fds[0]` is the open read end, owned by the `File` from here on.
            #[allow(unsafe_code)]
            let mut reader =
                unsafe { <std::fs::File as std::os::fd::FromRawFd>::from_raw_fd(fds[0]) };
            std::io::Read::read_to_end(&mut reader, &mut written).expect("read release");
            written
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
