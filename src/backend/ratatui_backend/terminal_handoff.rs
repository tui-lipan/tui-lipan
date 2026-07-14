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

use super::native_terminal::surface_terminal_policy;
use super::terminal_transition::{
    CrosstermTransitionExecutor, execute_plan_with_rollback, resume_plan, suspend_plan,
};
#[cfg(unix)]
use super::terminal_transition::{execute_plan, theme_notification_plan};

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

const READER_PAUSE_SETTLE: Duration = Duration::from_millis(125);

/// Pause the fullscreen crossterm reader thread so stdin is not consumed while
/// an external program runs.
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

pub(crate) struct StdinReaderPauseGuard {
    /// When set, blanket-discard everything left in the input queue on drop.
    /// Correct after an external full-screen process (arbitrary mode-switch
    /// garbage), but destructive for a quick OSC color probe where genuine user
    /// input may have queued during the round-trip. The color path clears this
    /// and drains selectively via [`drain_terminal_query_responses_preserving_input`].
    flush_on_drop: bool,
}

impl Drop for StdinReaderPauseGuard {
    fn drop(&mut self) {
        if self.flush_on_drop
            && let Err(err) = discard_pending_terminal_input()
        {
            crate::debug::internal_log!(
                "[tui-lipan] terminal_handoff: discard pending input failed (non-fatal): {}",
                err
            );
        }
        STDIN_READER_PAUSED.store(false, Ordering::SeqCst);
    }
}

/// Pause the fullscreen crossterm reader while the UI thread probes the TTY.
///
/// When a reader thread exists, wait for its 100ms poll window to settle before
/// issuing OSC queries so palette responses are not consumed as normal input.
///
/// With `flush_on_drop = true` the input queue is blanket-discarded on drop
/// (correct for external-process handoff). With `false` the caller is responsible
/// for draining query-response garbage (see
/// [`drain_terminal_query_responses_preserving_input`]) before the guard drops,
/// which preserves genuine user input that queued during the probe.
pub(crate) fn pause_stdin_reader_for_terminal_query_with(
    wait_for_reader: bool,
    flush_on_drop: bool,
) -> StdinReaderPauseGuard {
    STDIN_READER_PAUSED.store(true, Ordering::SeqCst);
    if wait_for_reader {
        std::thread::sleep(READER_PAUSE_SETTLE);
    }
    StdinReaderPauseGuard { flush_on_drop }
}

/// Drain pending terminal input after an OSC color query, dropping query-response
/// garbage while preserving genuine user input so the caller can re-deliver it.
///
/// crossterm parses leaked OSC/DA color-query responses (`ESC ] … ST`) as bogus
/// `Key` events but can never turn them into `Mouse`, `Resize`, `Paste`, or focus
/// events. So we drop `Key` events — matching the previous blanket discard, which
/// lost any keystrokes typed during the probe anyway — and return the rest.
///
/// Without this, a wheel scroll performed right after the window regains focus is
/// silently flushed for the duration of the blocking color round-trip (the reader
/// is paused, then the whole queue is `tcflush`ed), so scrolling appears dead for
/// a beat after focus.
///
/// Must run while the reader thread is still paused so only this call competes with
/// `event::read`.
pub(crate) fn drain_terminal_query_responses_preserving_input() -> io::Result<Vec<event::Event>> {
    let mut preserved = Vec::new();
    collect_pending_terminal_events(8192, &mut preserved)?;
    #[cfg(unix)]
    {
        // Hard-reset any residual partial response fragment that has not yet
        // assembled into a full event. The genuine, fully-arrived input was
        // already recovered above, so this only competes with a microsecond-wide
        // tail rather than the whole probe window.
        flush_stdin_input_queue_unix();
    }
    collect_terminal_events_until_quiet(4096, &mut preserved)?;
    Ok(preserved)
}

/// Read currently-available parsed events, keeping genuine input and dropping
/// `Key` events (which is where leaked OSC/DA query responses surface).
fn collect_pending_terminal_events(
    max_events: usize,
    preserved: &mut Vec<event::Event>,
) -> io::Result<()> {
    for _ in 0..max_events {
        if !event::poll(Duration::ZERO)? {
            break;
        }
        let ev = event::read()?;
        if is_preservable_input(&ev) {
            preserved.push(ev);
        }
    }
    Ok(())
}

fn collect_terminal_events_until_quiet(
    max_events: usize,
    preserved: &mut Vec<event::Event>,
) -> io::Result<()> {
    for _ in 0..max_events {
        if !event::poll(Duration::from_millis(10))? {
            break;
        }
        let ev = event::read()?;
        if is_preservable_input(&ev) {
            preserved.push(ev);
        }
    }
    Ok(())
}

/// Whether an event read while draining query responses is genuine user input
/// worth re-delivering, as opposed to OSC/DA color-query garbage.
///
/// crossterm only ever surfaces leaked `ESC ] … ST` / `ESC [ … c` responses as
/// `Key` events, so dropping `Key` discards the garbage. The previous blanket
/// flush dropped any keystrokes typed during the probe anyway, so this is not a
/// regression for typing; it specifically rescues mouse/scroll, resize, paste,
/// and focus events.
fn is_preservable_input(ev: &event::Event) -> bool {
    !matches!(ev, event::Event::Key(_))
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
    for _ in 0..max_events {
        if !event::poll(Duration::ZERO)? {
            break;
        }
        let _ = event::read()?;
    }
    Ok(())
}

fn drain_crossterm_events_until_quiet(max_events: usize) -> io::Result<()> {
    for _ in 0..max_events {
        if !event::poll(Duration::from_millis(10))? {
            break;
        }
        let _ = event::read()?;
    }
    Ok(())
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
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use crossterm::event::{Event, KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};

    fn mouse(kind: MouseEventKind) -> Event {
        Event::Mouse(MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        })
    }

    #[test]
    fn preserves_genuine_input_drops_key_garbage() {
        // OSC/DA color-query responses only ever surface as Key events; those
        // are the garbage to drop. Everything else is genuine user input the
        // color probe must not eat (notably wheel scrolls after FocusGained).
        assert!(is_preservable_input(&mouse(MouseEventKind::ScrollUp)));
        assert!(is_preservable_input(&mouse(MouseEventKind::ScrollDown)));
        assert!(is_preservable_input(&mouse(MouseEventKind::Down(
            MouseButton::Left
        ))));
        assert!(is_preservable_input(&Event::Resize(80, 24)));
        assert!(is_preservable_input(&Event::FocusGained));
        assert!(is_preservable_input(&Event::FocusLost));
        assert!(is_preservable_input(&Event::Paste("x".into())));

        assert!(!is_preservable_input(&Event::Key(KeyEvent::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE
        ))));
    }
}
