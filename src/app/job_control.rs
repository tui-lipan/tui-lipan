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
//! async-signal-safe calls, and stops on the spot; the runner then takes the
//! terminal back at its next frame boundary.
//!
//! [`Context::suspend_to_shell`]: crate::core::component::Context::suspend_to_shell

use std::sync::atomic::{AtomicBool, Ordering};

/// Targets with the POSIX job control this module needs.
const SUPPORTED: bool = cfg!(all(unix, not(target_arch = "wasm32")));

static SUSPEND_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Set by the `SIGTTIN`/`SIGTTOU` handler once it has released the terminal
/// and the process has been continued.
static STOPPED_IN_BACKGROUND: AtomicBool = AtomicBool::new(false);

/// Whether the running surface is on the alternate screen, which decides
/// whether the background release leaves it.
static USES_ALTERNATE_SCREEN: AtomicBool = AtomicBool::new(false);

/// What the background release writes: every terminal mode the runner turns on
/// that the shell would otherwise inherit, mouse reporting first. It leads with
/// CAN, which abandons any escape sequence a frame was halfway through writing,
/// so the resets that follow are read as resets.
///
/// It leaves the tty's termios settings alone. By the time a background signal
/// arrives another process group owns the terminal, and those settings are now
/// its own.
#[cfg_attr(not(all(unix, not(target_arch = "wasm32"))), allow(dead_code))]
const BACKGROUND_RELEASE: &[u8] = b"\x18\
    \x1b[?1003l\x1b[?1002l\x1b[?1000l\x1b[?1016l\x1b[?1006l\x1b[?1015l\
    \x1b[?2004l\x1b[?1004l\x1b[?2031l\x1b[?7h\x1b[?25h";

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
        SUSPEND_REQUESTED.store(false, Ordering::SeqCst);
        STOPPED_IN_BACKGROUND.store(false, Ordering::SeqCst);
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
/// still ours. Turn them off, stop, and leave the rest to the runner.
///
/// `write` and `raise` are async-signal-safe, and the handler runs with both
/// background signals blocked, which is what lets its own `write` through even
/// on a terminal with `tostop` set. Whatever call raised the signal is
/// restarted once the process is continued: in the foreground it succeeds, and
/// still in the background (`bg`) it lands here again and stops again.
#[cfg(all(unix, not(target_arch = "wasm32")))]
extern "C" fn release_and_stop(_signal: libc::c_int) {
    write_background_release(
        libc::STDOUT_FILENO,
        USES_ALTERNATE_SCREEN.load(Ordering::SeqCst),
    );
    // SAFETY: `raise` takes no pointers. `SIGSTOP` cannot be caught, so this
    // returns only once the process has been continued.
    #[allow(unsafe_code)]
    unsafe {
        libc::raise(libc::SIGSTOP)
    };
    STOPPED_IN_BACKGROUND.store(true, Ordering::SeqCst);
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn write_background_release(fd: libc::c_int, uses_alternate_screen: bool) {
    write_all_signal_safe(fd, BACKGROUND_RELEASE);
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
    let stop_set = set_disposition(libc::SIGTSTP, stop);
    let input_set = set_disposition(libc::SIGTTIN, background);
    let output_set = set_disposition(libc::SIGTTOU, background);
    stop_set && input_set && output_set
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn set_disposition(signal: libc::c_int, handler: libc::sighandler_t) -> bool {
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
        // Restart interrupted syscalls so a stop never surfaces as an EINTR
        // read error in the input path.
        action.sa_flags = libc::SA_RESTART;
        libc::sigaction(signal, &action, std::ptr::null_mut()) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_taken_once_and_cleared_by_the_guard() {
        assert!(!take_suspend_request(), "no request is pending initially");

        let guard = install_stop_handler(true);
        assert_eq!(
            guard.installed, SUPPORTED,
            "the handler installs exactly where job control exists"
        );

        request_suspend();
        assert_eq!(take_suspend_request(), SUPPORTED);
        assert!(!take_suspend_request(), "a request is delivered once");

        // A request that outlives the runner must not stop a later one.
        request_suspend();
        STOPPED_IN_BACKGROUND.store(true, Ordering::SeqCst);
        drop(guard);
        assert!(!take_suspend_request());
        assert!(!take_background_stop());
    }

    #[cfg(all(unix, not(target_arch = "wasm32")))]
    #[test]
    fn background_release_turns_off_every_mode_the_shell_would_inherit() {
        let release = |uses_alternate_screen: bool| {
            let mut fds = [0; 2];
            // SAFETY: `fds` has room for the two descriptors `pipe` writes.
            #[allow(unsafe_code)]
            let created = unsafe { libc::pipe(fds.as_mut_ptr()) };
            assert_eq!(created, 0, "pipe");
            write_background_release(fds[1], uses_alternate_screen);
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

        let fullscreen = release(true);
        assert_eq!(
            fullscreen.first(),
            Some(&0x18),
            "CAN ends a half-written frame"
        );
        for mode in [
            "1000", "1002", "1003", "1006", "1015", "1016", "1004", "2004", "2031", "1049",
        ] {
            let reset = format!("\x1b[?{mode}l");
            assert!(
                fullscreen
                    .windows(reset.len())
                    .any(|window| window == reset.as_bytes()),
                "fullscreen release resets mode {mode}"
            );
        }
        assert!(fullscreen.ends_with(LEAVE_ALTERNATE_SCREEN));

        let inline = release(false);
        assert_eq!(
            inline, BACKGROUND_RELEASE,
            "an inline surface was never on the alternate screen"
        );
    }
}
