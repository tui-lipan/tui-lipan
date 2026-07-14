use std::io::{self, BufWriter, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::context::SurfaceMode;
use crate::style::{
    drain_pending_terminal_responses, flush_pending_terminal_responses_on_exit,
    query_keyboard_enhancement_support,
};
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::style::Print;
use ratatui::backend::CrosstermBackend;
use ratatui::{TerminalOptions, Viewport};

use super::terminal_handoff::reset_handoff_state_for_terminal_restore;
#[cfg(unix)]
use super::terminal_transition::theme_notification_plan;
use super::terminal_transition::{
    CrosstermTransitionExecutor, enter_plan, execute_plan, execute_plan_with_rollback, exit_plan,
};

#[cfg(feature = "image")]
use super::image_support;

type TerminalWriter = BufWriter<Stdout>;
const TERMINAL_BUFFER_CAPACITY: usize = 64 * 1024;

pub(crate) type Terminal = ratatui::Terminal<CrosstermBackend<TerminalWriter>>;

fn buffered_stdout() -> TerminalWriter {
    BufWriter::with_capacity(TERMINAL_BUFFER_CAPACITY, io::stdout())
}

pub(crate) fn create_inline_terminal(height: u16) -> io::Result<Terminal> {
    let backend = CrosstermBackend::new(buffered_stdout());
    let options = TerminalOptions {
        viewport: Viewport::Inline(height.max(1)),
    };
    ratatui::Terminal::with_options(backend, options)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SurfaceTerminalPolicy {
    pub(crate) uses_alternate_screen: bool,
    pub(crate) disable_auto_wrap: bool,
    pub(crate) clear_on_start: bool,
}

pub(crate) fn surface_terminal_policy(surface_mode: SurfaceMode) -> SurfaceTerminalPolicy {
    let inline_disable_auto_wrap = matches!(surface_mode, SurfaceMode::InlineEphemeral { .. });
    SurfaceTerminalPolicy {
        uses_alternate_screen: !surface_mode.is_inline(),
        disable_auto_wrap: inline_disable_auto_wrap,
        clear_on_start: surface_mode.clear_on_start(),
    }
}

pub(crate) struct DisableMouseAllMotion;

impl crossterm::Command for DisableMouseAllMotion {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1003l\x1b[?1006l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

pub(crate) struct DisableAutoWrap;

impl crossterm::Command for DisableAutoWrap {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?7l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

pub(crate) struct EnableAutoWrap;

impl crossterm::Command for EnableAutoWrap {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?7h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

struct EnableMouseMotionTracking;

impl crossterm::Command for EnableMouseMotionTracking {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1003h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

struct DisableMouseMotionTracking;

impl crossterm::Command for DisableMouseMotionTracking {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1b[?1003l")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> io::Result<()> {
        Ok(())
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        true
    }
}

pub(crate) fn set_mouse_all_motion_enabled(
    writer: &mut impl std::io::Write,
    enabled: bool,
) -> io::Result<()> {
    if enabled {
        execute!(writer, EnableMouseMotionTracking)?;
    } else {
        execute!(writer, DisableMouseMotionTracking)?;
        execute!(
            writer,
            Print("\x1b[?1000h\x1b[?1002h\x1b[?1015h\x1b[?1006h")
        )?;
    }
    Ok(())
}

pub(crate) fn set_mouse_capture_enabled(
    writer: &mut impl std::io::Write,
    enabled: bool,
) -> io::Result<()> {
    if enabled {
        execute!(writer, EnableMouseCapture)?;
    } else {
        execute!(writer, DisableMouseAllMotion, DisableMouseCapture)?;
    }
    Ok(())
}

pub(crate) struct TerminalGuard {
    stdout: Stdout,
    policy: SurfaceTerminalPolicy,
    keyboard_enhancement: bool,
    theme_notifications: bool,
}

impl TerminalGuard {
    pub(crate) fn enter(
        surface_mode: SurfaceMode,
        mouse_enabled: bool,
        panic_keyboard_enhancement: &AtomicBool,
    ) -> io::Result<(Terminal, Self)> {
        let policy = surface_terminal_policy(surface_mode);
        let mut stdout = io::stdout();
        let keyboard_enhancement = query_keyboard_enhancement_support().unwrap_or(false);
        let plan = enter_plan(policy, mouse_enabled, keyboard_enhancement);
        let mut executor = CrosstermTransitionExecutor::new(stdout);
        execute_plan_with_rollback(&mut executor, &plan)?;
        stdout = executor.into_inner();
        panic_keyboard_enhancement.store(keyboard_enhancement, Ordering::SeqCst);

        #[cfg(feature = "image")]
        image_support::init_image_picker();

        // Both the keyboard-enhancement probe above and the image graphics query
        // send a `CSI c` sentinel. The keyboard probe consumes its reply unless it
        // arrives after timeout; the image probe may leave one unread. Drain either
        // case before the event loop starts.
        drain_pending_terminal_responses();

        let terminal = if policy.uses_alternate_screen {
            let backend =
                CrosstermBackend::new(BufWriter::with_capacity(TERMINAL_BUFFER_CAPACITY, stdout));
            match ratatui::Terminal::new(backend) {
                Ok(terminal) => terminal,
                Err(err) => {
                    rollback_entered_terminal(policy, keyboard_enhancement);
                    panic_keyboard_enhancement.store(false, Ordering::SeqCst);
                    return Err(err);
                }
            }
        } else {
            let height = match surface_mode {
                SurfaceMode::InlineEphemeral { height }
                | SurfaceMode::InlineTranscript { height, .. } => height.initial_rows(),
                SurfaceMode::Fullscreen => 1,
            };
            match create_inline_terminal(height) {
                Ok(terminal) => terminal,
                Err(err) => {
                    rollback_entered_terminal(policy, keyboard_enhancement);
                    panic_keyboard_enhancement.store(false, Ordering::SeqCst);
                    return Err(err);
                }
            }
        };

        let guard = Self {
            stdout: io::stdout(),
            policy,
            keyboard_enhancement,
            theme_notifications: false,
        };
        Ok((terminal, guard))
    }

    pub(crate) fn enable_theme_notifications(&mut self) -> io::Result<bool> {
        #[cfg(not(unix))]
        return Ok(false);
        #[cfg(unix)]
        {
            if self.theme_notifications {
                return Ok(true);
            }
            let plan = theme_notification_plan(true);
            let mut executor = CrosstermTransitionExecutor::new(&mut self.stdout);
            execute_plan_with_rollback(&mut executor, &plan)?;
            self.theme_notifications = true;
            Ok(true)
        }
    }
}

fn rollback_entered_terminal(policy: SurfaceTerminalPolicy, keyboard_enhancement: bool) {
    // Drop any probe DA1 reply still queued before `exit_plan` disables raw mode,
    // so it is not echoed to the shell as `^[[?…c`.
    flush_pending_terminal_responses_on_exit();
    let mut stdout = io::stdout();
    let plan = exit_plan(policy, keyboard_enhancement);
    let mut executor = CrosstermTransitionExecutor::new(&mut stdout);
    let _ = execute_plan(&mut executor, &plan);
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        #[cfg(unix)]
        super::terminal_handoff::pause_input_for_terminal_restore();
        let mut executor = CrosstermTransitionExecutor::new(&mut self.stdout);
        #[cfg(unix)]
        if self.theme_notifications {
            // Stop notifications and flush the command while raw mode is still
            // active, then discard any report that raced the Termina shutdown.
            let _ = execute_plan(&mut executor, &theme_notification_plan(false));
            self.theme_notifications = false;
        }
        flush_pending_terminal_responses_on_exit();
        let plan = exit_plan(self.policy, self.keyboard_enhancement);
        let _ = execute_plan(&mut executor, &plan);
    }
}

pub(crate) fn restore_terminal_on_panic(
    surface_mode: SurfaceMode,
    keyboard_enhancement: bool,
    theme_notifications: bool,
) {
    #[cfg(unix)]
    super::terminal_handoff::pause_input_for_terminal_restore();
    let policy = surface_terminal_policy(surface_mode);
    let mut stdout = io::stdout();
    let mut executor = CrosstermTransitionExecutor::new(&mut stdout);
    #[cfg(unix)]
    if theme_notifications {
        let _ = execute_plan(&mut executor, &theme_notification_plan(false));
    }
    #[cfg(not(unix))]
    let _ = theme_notifications;
    flush_pending_terminal_responses_on_exit();
    let plan = exit_plan(policy, keyboard_enhancement);
    let _ = execute_plan(&mut executor, &plan);
    reset_handoff_state_for_terminal_restore();
}

#[cfg(test)]
pub(crate) fn assert_inline_surface_internal_wrap_policy_is_opaque() {
    use crate::app::context::{InlineHeight, InlineStartupPolicy, SurfaceMode};

    let fullscreen = surface_terminal_policy(SurfaceMode::Fullscreen);
    assert!(fullscreen.uses_alternate_screen);
    assert!(!fullscreen.disable_auto_wrap);

    let ephemeral = surface_terminal_policy(SurfaceMode::InlineEphemeral {
        height: InlineHeight::Fixed(3),
    });
    assert!(!ephemeral.uses_alternate_screen);
    assert!(ephemeral.disable_auto_wrap);

    let transcript = surface_terminal_policy(SurfaceMode::InlineTranscript {
        height: InlineHeight::Fixed(3),
        startup: InlineStartupPolicy::PreserveHost,
    });
    assert!(!transcript.uses_alternate_screen);
    assert!(!transcript.disable_auto_wrap);
}
