//! Temporarily release the interactive terminal so a subprocess (e.g. `$EDITOR`)
//! can use the real TTY without fighting raw mode, the alternate screen, or the
//! framework's stdin reader thread.

use std::cell::RefCell;
use std::io::{self, stdout};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use crossterm::event;

use crate::app::context::SurfaceMode;

use super::host_input::{HostEvent, read_host_event};
use super::native_terminal::surface_terminal_policy;
use super::terminal_transition::{
    CrosstermTransitionExecutor, execute_plan_with_rollback, resume_plan, suspend_plan,
};
#[cfg(unix)]
use super::terminal_transition::{execute_plan, pixel_mouse_plan, theme_notification_plan};

static STDIN_READER_PAUSED: AtomicBool = AtomicBool::new(false);

pub(crate) trait InputHandoffControl {
    fn pause(&self) -> io::Result<()>;
    fn resume(&self) -> io::Result<()>;
    fn is_paused(&self) -> bool;
}

pub(crate) type InputHandoffSlot = Arc<Mutex<Option<Weak<dyn InputHandoffControl + Send + Sync>>>>;

pub(crate) fn input_handoff_slot() -> InputHandoffSlot {
    Arc::new(Mutex::new(None))
}

pub(crate) fn pause_input_from_slot(slot: &InputHandoffSlot) {
    if let Some(control) = slot
        .lock()
        .ok()
        .and_then(|control| control.as_ref().and_then(Weak::upgrade))
    {
        let _ = control.pause();
    }
}

thread_local! {
    static INPUT_HANDOFF_CONTROL: RefCell<Option<Weak<dyn InputHandoffControl + Send + Sync>>> =
        RefCell::new(None);
}

pub(crate) fn register_input_handoff_control(control: Weak<dyn InputHandoffControl + Send + Sync>) {
    INPUT_HANDOFF_CONTROL.with(|slot| *slot.borrow_mut() = Some(control));
}

pub(crate) fn unregister_input_handoff_control() {
    INPUT_HANDOFF_CONTROL.with(|slot| *slot.borrow_mut() = None);
}

fn input_handoff_control() -> Option<std::sync::Arc<dyn InputHandoffControl + Send + Sync>> {
    INPUT_HANDOFF_CONTROL.with(|slot| slot.borrow().as_ref()?.upgrade())
}

pub(crate) fn pause_input_for_terminal_restore() {
    if let Some(control) = input_handoff_control() {
        let _ = control.pause();
    }
}

/// Set when [`resume_after_external_process`] succeeds so the runner can clear ratatui buffers
/// and schedule a full frame (host TTY may not match the last draw after alt-screen handoff).
static FULL_REPAINT_AFTER_HANDOFF: AtomicBool = AtomicBool::new(false);

/// Whether the Windows crossterm reader thread should leave console input alone, so an external
/// program or a terminal query gets it instead.
#[cfg(not(unix))]
pub(crate) fn stdin_reader_is_paused() -> bool {
    STDIN_READER_PAUSED.load(Ordering::SeqCst)
}

pub(crate) fn take_handoff_full_repaint_request() -> bool {
    FULL_REPAINT_AFTER_HANDOFF.swap(false, Ordering::SeqCst)
}

pub(crate) fn reset_handoff_state_for_terminal_restore() {
    STDIN_READER_PAUSED.store(false, Ordering::SeqCst);
    FULL_REPAINT_AFTER_HANDOFF.store(false, Ordering::SeqCst);
}

/// Hand each event to `on_event` until `wait` passes without one, `max_events` have been read, or
/// the terminal hangs up.
fn for_each_host_event(
    max_events: usize,
    wait: Duration,
    mut on_event: impl FnMut(event::Event),
) -> io::Result<()> {
    for _ in 0..max_events {
        match read_host_event(wait)? {
            HostEvent::Input(ev) => on_event(ev),
            #[cfg(unix)]
            HostEvent::Pointer(ev, _) => on_event(ev),
            #[cfg(unix)]
            HostEvent::ThemeRefresh => {}
            #[cfg(unix)]
            HostEvent::HungUp => break,
            HostEvent::Quiet => break,
        }
    }
    Ok(())
}

/// Drop pending stdin so CSI/OSC/DA responses and mode-switch garbage are not read as keys.
///
/// Must run while the fullscreen reader thread is still paused so only this call competes
/// with [`event::read`]. On Unix, also flushes the kernel TTY input queue (`tcflush`).
fn discard_pending_terminal_input() -> io::Result<()> {
    drain_crossterm_events(8192)?;
    #[cfg(unix)]
    {
        flush_stdin_input_queue_unix();
    }
    drain_crossterm_events_until_quiet(4096)?;
    Ok(())
}

fn drain_crossterm_events(max_events: usize) -> io::Result<()> {
    for_each_host_event(max_events, Duration::ZERO, drop)
}

fn drain_crossterm_events_until_quiet(max_events: usize) -> io::Result<()> {
    for_each_host_event(max_events, Duration::from_millis(10), drop)
}

#[cfg(unix)]
fn flush_stdin_input_queue_unix() {
    use std::os::unix::io::AsRawFd;
    let fd = std::io::stdin().as_raw_fd();
    // SAFETY: `tcflush(TCIFLUSH)` on stdin is valid for a TTY; drops unread bytes after an
    // external process used the same fd (CSI/OSC tails, DA replies, etc.).
    #[allow(unsafe_code)]
    let rc = unsafe { libc::tcflush(fd, libc::TCIFLUSH) };
    if rc != 0 {
        let err = io::Error::last_os_error();
        // Ignore when stdin is not a tty (tests, pipes).
        if err.raw_os_error() != Some(libc::EINVAL) && err.raw_os_error() != Some(libc::ENOTTY) {
            crate::debug::internal_log!(
                "[tui-lipan] terminal_handoff: tcflush stdin failed (non-fatal): {}",
                err
            );
        }
    }
}

/// Release the terminal for an external full-screen program.
///
/// `surface_mode` must match the running app surface mode.
/// Pass the same mode and `mouse_enabled` value to
/// [`resume_after_external_process`] when the subprocess exits.
pub fn suspend_for_external_process(surface_mode: SurfaceMode) -> io::Result<()> {
    let policy = surface_terminal_policy(surface_mode);
    #[cfg(unix)]
    let termina_paused = if let Some(control) = input_handoff_control() {
        control.pause()?;
        true
    } else {
        false
    };
    #[cfg(not(unix))]
    let termina_paused = false;
    STDIN_READER_PAUSED.store(true, Ordering::SeqCst);
    if !termina_paused {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    let mut out = stdout();
    let mut executor = CrosstermTransitionExecutor::new(&mut out);
    if termina_paused {
        #[cfg(unix)]
        if let Err(err) = execute_plan_with_rollback(&mut executor, &theme_notification_plan(false))
        {
            if let Some(control) = input_handoff_control() {
                let _ = control.resume();
            }
            STDIN_READER_PAUSED.store(false, Ordering::SeqCst);
            return Err(err);
        }
        #[cfg(unix)]
        {
            crate::style::flush_pending_terminal_responses_on_exit();
        }
    }

    let plan = suspend_plan(policy);
    let result = execute_plan_with_rollback(&mut executor, &plan);
    // The plan turns pixel reporting off before handing the terminal over. A rollback puts it back,
    // so only a plan that stuck changes what reports are read as.
    #[cfg(unix)]
    if result.is_ok() {
        crate::app::input::pixel_mouse::note_mode_enabled(false);
    }

    if result.is_err() {
        #[cfg(unix)]
        if termina_paused {
            let _ = execute_plan(&mut executor, &theme_notification_plan(true));
            if let Some(control) = input_handoff_control() {
                let _ = control.resume();
            }
        }
        STDIN_READER_PAUSED.store(false, Ordering::SeqCst);
    }

    result
}

/// Restore the terminal after [`suspend_for_external_process`].
pub fn resume_after_external_process(
    surface_mode: SurfaceMode,
    mouse_enabled: bool,
) -> io::Result<()> {
    let policy = surface_terminal_policy(surface_mode);
    #[cfg(unix)]
    let termina_paused = input_handoff_control().is_some_and(|control| control.is_paused());
    #[cfg(not(unix))]
    let termina_paused = false;
    let mut out = stdout();
    let plan = resume_plan(policy, mouse_enabled);
    let mut executor = CrosstermTransitionExecutor::new(&mut out);
    if let Err(err) = execute_plan_with_rollback(&mut executor, &plan) {
        if !termina_paused {
            STDIN_READER_PAUSED.store(false, Ordering::SeqCst);
        }
        return Err(err);
    }

    if termina_paused {
        #[cfg(unix)]
        {
            crate::style::flush_pending_terminal_responses_on_exit();
            if let Err(err) =
                execute_plan_with_rollback(&mut executor, &theme_notification_plan(true))
            {
                if let Some(control) = input_handoff_control() {
                    let _ = control.resume();
                }
                STDIN_READER_PAUSED.store(false, Ordering::SeqCst);
                crate::style::flush_pending_terminal_responses_on_exit();
                return Err(err);
            }
            if let Some(control) = input_handoff_control()
                && let Err(err) = control.resume()
            {
                let disable_plan = theme_notification_plan(false);
                let _ = execute_plan(&mut executor, &disable_plan);
                crate::style::flush_pending_terminal_responses_on_exit();
                STDIN_READER_PAUSED.store(false, Ordering::SeqCst);
                return Err(err);
            }
        }
    } else if let Err(err) = discard_pending_terminal_input() {
        crate::debug::internal_log!(
            "[tui-lipan] terminal_handoff: discard pending input failed (non-fatal): {}",
            err
        );
    }
    STDIN_READER_PAUSED.store(false, Ordering::SeqCst);
    FULL_REPAINT_AFTER_HANDOFF.store(true, Ordering::SeqCst);
    // The suspend turned pixel reporting off; the host is the same one that answered at startup, so
    // it goes straight back on rather than being asked again. Only for a paused Termina worker,
    // though: it is the one decoder that can read a pixel report, and handing pixels to any other
    // would put every click hundreds of columns from the pointer.
    #[cfg(unix)]
    if termina_paused
        && crate::app::input::pixel_mouse::is_active()
        && execute_plan(&mut executor, &pixel_mouse_plan(true)).is_ok()
    {
        crate::app::input::pixel_mouse::note_mode_enabled(true);
    }
    Ok(())
}
