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
//! [`Context::suspend_to_shell`]: crate::core::component::Context::suspend_to_shell

use std::sync::atomic::{AtomicBool, Ordering};

/// Targets with the POSIX job control this module needs.
const SUPPORTED: bool = cfg!(all(unix, not(target_arch = "wasm32")));

static SUSPEND_REQUESTED: AtomicBool = AtomicBool::new(false);

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

/// Keeps `SIGTSTP` routed to the runner for as long as it owns the terminal,
/// and hands the signal back to the OS default on the way out.
pub(crate) struct StopSignalGuard {
    installed: bool,
}

impl Drop for StopSignalGuard {
    fn drop(&mut self) {
        #[cfg(all(unix, not(target_arch = "wasm32")))]
        if self.installed {
            set_stop_disposition(libc::SIG_DFL);
        }
        SUSPEND_REQUESTED.store(false, Ordering::SeqCst);
    }
}

/// Route `SIGTSTP` to the runner until the returned guard drops.
pub(crate) fn install_stop_handler() -> StopSignalGuard {
    #[cfg(all(unix, not(target_arch = "wasm32")))]
    {
        StopSignalGuard {
            installed: set_stop_disposition(stop_request_handler()),
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
        set_stop_disposition(libc::SIG_DFL);
        // SAFETY: `killpg(0, …)` targets the caller's own process group and
        // takes no pointers. It returns once we are continued (`SIGCONT`).
        #[allow(unsafe_code)]
        unsafe {
            libc::killpg(0, libc::SIGTSTP)
        };
        set_stop_disposition(stop_request_handler());
    }
}

/// Signal handler: an atomic store is all it does, because that is one of the
/// few things a signal handler may safely do. The terminal work happens later,
/// on the runner's own thread.
#[cfg(all(unix, not(target_arch = "wasm32")))]
extern "C" fn note_stop_request(_signal: libc::c_int) {
    SUSPEND_REQUESTED.store(true, Ordering::SeqCst);
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn stop_request_handler() -> libc::sighandler_t {
    note_stop_request as *const () as libc::sighandler_t
}

#[cfg(all(unix, not(target_arch = "wasm32")))]
fn set_stop_disposition(handler: libc::sighandler_t) -> bool {
    // SAFETY: `action` is a zeroed `sigaction` filled in through libc's own
    // accessors before use, and `sigaction` copies it; the null third argument
    // means "do not report the previous disposition".
    #[allow(unsafe_code)]
    unsafe {
        let mut action: libc::sigaction = std::mem::zeroed();
        action.sa_sigaction = handler;
        libc::sigemptyset(&mut action.sa_mask);
        // Restart interrupted syscalls so a stop request never surfaces as an
        // EINTR read error in the input path.
        action.sa_flags = libc::SA_RESTART;
        libc::sigaction(libc::SIGTSTP, &action, std::ptr::null_mut()) == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_taken_once_and_cleared_by_the_guard() {
        assert!(!take_suspend_request(), "no request is pending initially");

        let guard = install_stop_handler();
        assert_eq!(
            guard.installed, SUPPORTED,
            "the handler installs exactly where job control exists"
        );

        request_suspend();
        assert_eq!(take_suspend_request(), SUPPORTED);
        assert!(!take_suspend_request(), "a request is delivered once");

        // A request that outlives the runner must not stop a later one.
        request_suspend();
        drop(guard);
        assert!(!take_suspend_request());
    }
}
