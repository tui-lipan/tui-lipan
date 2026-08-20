use std::io::{self, Write};

use crossterm::cursor::{Hide, Show};
use crossterm::event::{
    DisableBracketedPaste, DisableFocusChange, DisableMouseCapture, EnableBracketedPaste,
    EnableFocusChange, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::style::Print;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};

use super::native_terminal::{
    DisableAutoWrap, DisableMouseAllMotion, EnableAutoWrap, SurfaceTerminalPolicy,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalOp {
    EnableRawMode,
    DisableRawMode,
    EnterAlternateScreen,
    LeaveAlternateScreen,
    EnableMouseCapture,
    DisableMouseCapture,
    EnableBracketedPaste,
    DisableBracketedPaste,
    EnableFocusChange,
    DisableFocusChange,
    DisableAutoWrap,
    EnableAutoWrap,
    ShowCursor,
    HideCursor,
    ClearScreen,
    PushKeyboardEnhancement,
    PopKeyboardEnhancement,
    EnableThemeNotifications,
    DisableThemeNotifications,
    EnablePixelMouse,
    DisablePixelMouse,
    Flush,
}

impl TerminalOp {
    fn rollback(self) -> Option<Self> {
        match self {
            Self::EnableRawMode => Some(Self::DisableRawMode),
            Self::DisableRawMode => Some(Self::EnableRawMode),
            Self::EnterAlternateScreen => Some(Self::LeaveAlternateScreen),
            Self::LeaveAlternateScreen => Some(Self::EnterAlternateScreen),
            Self::EnableMouseCapture => Some(Self::DisableMouseCapture),
            Self::DisableMouseCapture => Some(Self::EnableMouseCapture),
            Self::EnableBracketedPaste => Some(Self::DisableBracketedPaste),
            Self::DisableBracketedPaste => Some(Self::EnableBracketedPaste),
            Self::EnableFocusChange => Some(Self::DisableFocusChange),
            Self::DisableFocusChange => Some(Self::EnableFocusChange),
            Self::DisableAutoWrap => Some(Self::EnableAutoWrap),
            Self::EnableAutoWrap => Some(Self::DisableAutoWrap),
            Self::ShowCursor => Some(Self::HideCursor),
            Self::HideCursor => Some(Self::ShowCursor),
            Self::PushKeyboardEnhancement => Some(Self::PopKeyboardEnhancement),
            Self::PopKeyboardEnhancement => Some(Self::PushKeyboardEnhancement),
            Self::EnableThemeNotifications => Some(Self::DisableThemeNotifications),
            Self::DisableThemeNotifications => Some(Self::EnableThemeNotifications),
            Self::EnablePixelMouse => Some(Self::DisablePixelMouse),
            Self::DisablePixelMouse => Some(Self::EnablePixelMouse),
            Self::ClearScreen | Self::Flush => None,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TerminalTransitionPlan {
    ops: Vec<TerminalOp>,
}

impl TerminalTransitionPlan {
    fn new(ops: Vec<TerminalOp>) -> Self {
        Self { ops }
    }

    pub(crate) fn ops(&self) -> &[TerminalOp] {
        &self.ops
    }
}

pub(crate) fn enter_plan(
    policy: SurfaceTerminalPolicy,
    mouse_enabled: bool,
    keyboard_enhancement: bool,
) -> TerminalTransitionPlan {
    let mut ops = vec![TerminalOp::EnableRawMode];
    if policy.uses_alternate_screen {
        ops.push(TerminalOp::EnterAlternateScreen);
    }
    if mouse_enabled {
        ops.push(TerminalOp::EnableMouseCapture);
    }
    ops.push(TerminalOp::EnableBracketedPaste);
    if policy.disable_auto_wrap {
        ops.push(TerminalOp::DisableAutoWrap);
    }
    ops.push(TerminalOp::EnableFocusChange);
    if policy.clear_on_start {
        ops.push(TerminalOp::ClearScreen);
        ops.push(TerminalOp::Flush);
    }
    if keyboard_enhancement {
        ops.push(TerminalOp::PushKeyboardEnhancement);
    }
    TerminalTransitionPlan::new(ops)
}

pub(crate) fn exit_plan(
    policy: SurfaceTerminalPolicy,
    keyboard_enhancement: bool,
) -> TerminalTransitionPlan {
    let mut ops = vec![TerminalOp::DisableRawMode];
    if keyboard_enhancement {
        ops.push(TerminalOp::PopKeyboardEnhancement);
    }
    if policy.disable_auto_wrap {
        ops.push(TerminalOp::EnableAutoWrap);
    }
    if policy.uses_alternate_screen {
        ops.push(TerminalOp::LeaveAlternateScreen);
    }
    ops.push(TerminalOp::DisableBracketedPaste);
    ops.push(TerminalOp::DisablePixelMouse);
    ops.push(TerminalOp::DisableMouseCapture);
    ops.push(TerminalOp::DisableFocusChange);
    TerminalTransitionPlan::new(ops)
}

pub(crate) fn suspend_plan(policy: SurfaceTerminalPolicy) -> TerminalTransitionPlan {
    let mut ops = vec![TerminalOp::DisableRawMode];
    if policy.disable_auto_wrap {
        ops.push(TerminalOp::EnableAutoWrap);
    }
    if policy.uses_alternate_screen {
        ops.push(TerminalOp::LeaveAlternateScreen);
    }
    ops.push(TerminalOp::DisableBracketedPaste);
    ops.push(TerminalOp::DisablePixelMouse);
    ops.push(TerminalOp::DisableMouseCapture);
    ops.push(TerminalOp::DisableFocusChange);
    // Ratatui hides the cursor on every frame that draws without one, and
    // `Terminal` only shows it again when it is dropped - which does not happen
    // here, because the app is coming back. Whoever gets the terminal next (a
    // shell prompt, `read`, a pager) would otherwise inherit `?25l` and show no
    // cursor at all.
    ops.push(TerminalOp::ShowCursor);
    ops.push(TerminalOp::Flush);
    TerminalTransitionPlan::new(ops)
}

pub(crate) fn resume_plan(
    policy: SurfaceTerminalPolicy,
    mouse_enabled: bool,
) -> TerminalTransitionPlan {
    let mut ops = vec![TerminalOp::EnableRawMode];
    if policy.uses_alternate_screen {
        ops.push(TerminalOp::EnterAlternateScreen);
    }
    if mouse_enabled {
        ops.push(TerminalOp::EnableMouseCapture);
    }
    ops.push(TerminalOp::EnableBracketedPaste);
    if policy.disable_auto_wrap {
        ops.push(TerminalOp::DisableAutoWrap);
    }
    ops.push(TerminalOp::EnableFocusChange);
    ops.push(TerminalOp::Flush);
    TerminalTransitionPlan::new(ops)
}

/// Turn SGR-pixels reporting on or off once the host has said whether it implements it.
#[cfg(unix)]
pub(crate) fn pixel_mouse_plan(enabled: bool) -> TerminalTransitionPlan {
    TerminalTransitionPlan::new(vec![
        if enabled {
            TerminalOp::EnablePixelMouse
        } else {
            TerminalOp::DisablePixelMouse
        },
        TerminalOp::Flush,
    ])
}

#[cfg(unix)]
pub(crate) fn theme_notification_plan(enabled: bool) -> TerminalTransitionPlan {
    TerminalTransitionPlan::new(vec![
        if enabled {
            TerminalOp::EnableThemeNotifications
        } else {
            TerminalOp::DisableThemeNotifications
        },
        TerminalOp::Flush,
    ])
}

pub(crate) trait TerminalTransitionExecutor {
    fn execute_op(&mut self, op: TerminalOp) -> io::Result<()>;
}

pub(crate) fn execute_plan<E: TerminalTransitionExecutor>(
    executor: &mut E,
    plan: &TerminalTransitionPlan,
) -> io::Result<()> {
    for &op in plan.ops() {
        executor.execute_op(op)?;
    }
    Ok(())
}

pub(crate) fn execute_plan_with_rollback<E: TerminalTransitionExecutor>(
    executor: &mut E,
    plan: &TerminalTransitionPlan,
) -> io::Result<()> {
    let mut applied = Vec::new();
    for &op in plan.ops() {
        if let Err(err) = executor.execute_op(op) {
            rollback_applied(executor, &applied);
            return Err(err);
        }
        applied.push(op);
    }
    Ok(())
}

fn rollback_applied<E: TerminalTransitionExecutor>(executor: &mut E, applied: &[TerminalOp]) {
    for op in applied.iter().rev().filter_map(|op| op.rollback()) {
        let _ = executor.execute_op(op);
    }
}

pub(crate) struct CrosstermTransitionExecutor<W> {
    writer: W,
}

impl<W> CrosstermTransitionExecutor<W> {
    pub(crate) fn new(writer: W) -> Self {
        Self { writer }
    }

    pub(crate) fn into_inner(self) -> W {
        self.writer
    }
}

fn keyboard_enhancement_flags() -> KeyboardEnhancementFlags {
    KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
        | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
        | KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES
}

impl<W: Write> TerminalTransitionExecutor for CrosstermTransitionExecutor<W> {
    fn execute_op(&mut self, op: TerminalOp) -> io::Result<()> {
        let flags = keyboard_enhancement_flags();
        match op {
            TerminalOp::EnableRawMode => enable_raw_mode(),
            TerminalOp::DisableRawMode => disable_raw_mode(),
            TerminalOp::EnterAlternateScreen => execute!(self.writer, EnterAlternateScreen),
            TerminalOp::LeaveAlternateScreen => execute!(self.writer, LeaveAlternateScreen),
            TerminalOp::EnableMouseCapture => execute!(self.writer, EnableMouseCapture),
            TerminalOp::DisableMouseCapture => {
                execute!(self.writer, DisableMouseAllMotion, DisableMouseCapture)
            }
            TerminalOp::EnableBracketedPaste => execute!(self.writer, EnableBracketedPaste),
            TerminalOp::DisableBracketedPaste => execute!(self.writer, DisableBracketedPaste),
            TerminalOp::EnableFocusChange => execute!(self.writer, EnableFocusChange),
            TerminalOp::DisableFocusChange => execute!(self.writer, DisableFocusChange),
            TerminalOp::DisableAutoWrap => execute!(self.writer, DisableAutoWrap),
            TerminalOp::EnableAutoWrap => execute!(self.writer, EnableAutoWrap),
            TerminalOp::ShowCursor => execute!(self.writer, Show),
            TerminalOp::HideCursor => execute!(self.writer, Hide),
            TerminalOp::ClearScreen => execute!(self.writer, Print("\x1b[2J\x1b[H")),
            TerminalOp::PushKeyboardEnhancement => {
                execute!(self.writer, PushKeyboardEnhancementFlags(flags))
            }
            TerminalOp::PopKeyboardEnhancement => {
                execute!(self.writer, PopKeyboardEnhancementFlags)
            }
            TerminalOp::EnableThemeNotifications => {
                execute!(self.writer, Print("\x1b[?2031h"))
            }
            TerminalOp::DisableThemeNotifications => {
                execute!(self.writer, Print("\x1b[?2031l"))
            }
            TerminalOp::EnablePixelMouse => execute!(self.writer, Print("\x1b[?1016h")),
            TerminalOp::DisablePixelMouse => execute!(self.writer, Print("\x1b[?1016l")),
            TerminalOp::Flush => self.writer.flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockExecutor {
        ops: Vec<TerminalOp>,
        fail_on: Option<TerminalOp>,
    }

    impl TerminalTransitionExecutor for MockExecutor {
        fn execute_op(&mut self, op: TerminalOp) -> io::Result<()> {
            if self.fail_on == Some(op) {
                return Err(io::Error::other("planned failure"));
            }
            self.ops.push(op);
            Ok(())
        }
    }

    fn fullscreen_policy() -> SurfaceTerminalPolicy {
        SurfaceTerminalPolicy {
            uses_alternate_screen: true,
            disable_auto_wrap: false,
            clear_on_start: true,
        }
    }

    #[test]
    fn keyboard_enhancement_requests_standalone_modifier_events() {
        assert!(
            keyboard_enhancement_flags()
                .contains(KeyboardEnhancementFlags::REPORT_ALL_KEYS_AS_ESCAPE_CODES)
        );
    }

    #[test]
    fn handoff_resume_plan_does_not_push_keyboard_enhancement() {
        let plan = resume_plan(fullscreen_policy(), true);
        assert_eq!(
            plan.ops(),
            &[
                TerminalOp::EnableRawMode,
                TerminalOp::EnterAlternateScreen,
                TerminalOp::EnableMouseCapture,
                TerminalOp::EnableBracketedPaste,
                TerminalOp::EnableFocusChange,
                TerminalOp::Flush,
            ]
        );
    }

    #[test]
    fn handoff_suspend_plan_hands_back_a_visible_cursor() {
        // Ratatui leaves `?25l` set while the app draws frames without a cursor
        // and only undoes it when `Terminal` drops, which a handoff never does.
        // Whatever runs next - a shell prompt, a pager - gets the terminal with
        // no visible cursor unless the plan shows it here.
        let plan = suspend_plan(fullscreen_policy());
        assert_eq!(
            plan.ops(),
            &[
                TerminalOp::DisableRawMode,
                TerminalOp::LeaveAlternateScreen,
                TerminalOp::DisableBracketedPaste,
                TerminalOp::DisablePixelMouse,
                TerminalOp::DisableMouseCapture,
                TerminalOp::DisableFocusChange,
                TerminalOp::ShowCursor,
                TerminalOp::Flush,
            ]
        );
    }

    #[test]
    fn enter_plan_includes_keyboard_enhancement_when_requested() {
        let plan = enter_plan(fullscreen_policy(), true, true);
        assert_eq!(
            plan.ops().last(),
            Some(&TerminalOp::PushKeyboardEnhancement)
        );
    }

    #[test]
    fn enter_failure_rolls_back_applied_terminal_modes() {
        let plan = enter_plan(fullscreen_policy(), true, true);
        let mut executor = MockExecutor {
            fail_on: Some(TerminalOp::PushKeyboardEnhancement),
            ..MockExecutor::default()
        };

        let err = execute_plan_with_rollback(&mut executor, &plan).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert_eq!(
            executor.ops,
            vec![
                TerminalOp::EnableRawMode,
                TerminalOp::EnterAlternateScreen,
                TerminalOp::EnableMouseCapture,
                TerminalOp::EnableBracketedPaste,
                TerminalOp::EnableFocusChange,
                TerminalOp::ClearScreen,
                TerminalOp::Flush,
                TerminalOp::DisableFocusChange,
                TerminalOp::DisableBracketedPaste,
                TerminalOp::DisableMouseCapture,
                TerminalOp::LeaveAlternateScreen,
                TerminalOp::DisableRawMode,
            ]
        );
    }

    #[test]
    fn resume_failure_rolls_back_without_keyboard_enhancement() {
        let plan = resume_plan(fullscreen_policy(), true);
        let mut executor = MockExecutor {
            fail_on: Some(TerminalOp::EnableFocusChange),
            ..MockExecutor::default()
        };

        let err = execute_plan_with_rollback(&mut executor, &plan).unwrap_err();

        assert_eq!(err.kind(), io::ErrorKind::Other);
        assert_eq!(
            executor.ops,
            vec![
                TerminalOp::EnableRawMode,
                TerminalOp::EnterAlternateScreen,
                TerminalOp::EnableMouseCapture,
                TerminalOp::EnableBracketedPaste,
                TerminalOp::DisableBracketedPaste,
                TerminalOp::DisableMouseCapture,
                TerminalOp::LeaveAlternateScreen,
                TerminalOp::DisableRawMode,
            ]
        );
    }

    #[test]
    #[cfg(unix)]
    fn theme_notification_operations_emit_exact_dec_mode_2031_sequences() {
        let mut executor = CrosstermTransitionExecutor::new(Vec::new());

        assert_eq!(
            theme_notification_plan(false).ops(),
            &[TerminalOp::DisableThemeNotifications, TerminalOp::Flush]
        );
        execute_plan(&mut executor, &theme_notification_plan(true)).unwrap();
        execute_plan(&mut executor, &theme_notification_plan(false)).unwrap();

        assert_eq!(executor.into_inner(), b"\x1b[?2031h\x1b[?2031l");
    }

    #[test]
    #[cfg(unix)]
    fn theme_notification_enable_rolls_back_on_later_enter_failure() {
        let mut plan = theme_notification_plan(true);
        plan.ops.push(TerminalOp::Flush);
        let mut executor = MockExecutor {
            fail_on: Some(TerminalOp::Flush),
            ..MockExecutor::default()
        };

        execute_plan_with_rollback(&mut executor, &plan).unwrap_err();

        assert!(executor.ops.windows(2).any(|ops| {
            ops == [
                TerminalOp::EnableThemeNotifications,
                TerminalOp::DisableThemeNotifications,
            ]
        }));
    }
}
