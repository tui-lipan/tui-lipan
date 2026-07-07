#![allow(dead_code)]

use std::sync::Arc;

use crate::core::event::KeyEvent;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FocusKind {
    Widget,
    Terminal,
}

/// Ordering policy for widget key handlers versus app command shortcuts.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyDispatchPolicy {
    /// Offer keys to focused widgets before app command shortcuts.
    WidgetFirst,
    /// Resolve app command shortcuts before focused widget handlers.
    AppCommandsFirst,
}

/// Ordering policy for keys when focus is inside an embedded terminal widget.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalKeyPolicy {
    /// Framework shortcuts run before terminal passthrough.
    FrameworkFirst,
    /// App command shortcuts run before terminal passthrough.
    AppCommandsThenTerminal,
    /// Terminal preflight/passthrough runs before app/framework handling.
    TerminalFirst,
    /// Send keys only to the terminal except required preflight handling.
    TerminalOnly,
}

/// Policy for resolving app command shortcut conflicts.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum CommandConflictPolicy {
    /// Prefer the first registered command for a conflicting shortcut.
    #[default]
    FirstRegistered,
    /// Prefer the highest priority command, then first registered.
    HighestPriority,
}

/// Policy for handling a mismatched key while an app-command chord is pending.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ChordMismatchPolicy {
    /// Swallow the prefix and retry the current key as a fresh dispatch.
    #[default]
    SwallowPrefixReplayCurrent,
    /// Forward both the swallowed prefix and the mismatching key to lower sinks.
    ForwardPrefixAndCurrent,
    /// Cancel pending command state without replaying either key.
    CancelOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum CommandDispatchState {
    None,
    Pending,
    Mismatch,
    Matched(Arc<str>),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum FrameworkDispatch {
    None,
    Handled,
    Quit,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DispatchOutcome {
    Unhandled,
    Widget,
    Bubble,
    Command,
    CommandPending,
    Framework,
    FrameworkQuit,
    TerminalPreflight,
    Terminal,
    AmbientScroll,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct DispatchRequest {
    key: KeyEvent,
    focus: FocusKind,
    key_policy: KeyDispatchPolicy,
    terminal_policy: TerminalKeyPolicy,
    chord_mismatch_policy: ChordMismatchPolicy,
}

impl DispatchRequest {
    pub(crate) fn new(key: KeyEvent, focus: FocusKind) -> Self {
        Self {
            key,
            focus,
            key_policy: KeyDispatchPolicy::WidgetFirst,
            terminal_policy: TerminalKeyPolicy::FrameworkFirst,
            chord_mismatch_policy: ChordMismatchPolicy::default(),
        }
    }

    pub(crate) fn key_policy(mut self, policy: KeyDispatchPolicy) -> Self {
        self.key_policy = policy;
        self
    }

    pub(crate) fn terminal_policy(mut self, policy: TerminalKeyPolicy) -> Self {
        self.terminal_policy = policy;
        self
    }

    pub(crate) fn chord_mismatch_policy(mut self, policy: ChordMismatchPolicy) -> Self {
        self.chord_mismatch_policy = policy;
        self
    }
}

pub(crate) trait DispatchOps {
    fn continue_command_chord(&mut self, key: KeyEvent) -> CommandDispatchState;
    fn dispatch_widget(&mut self, key: KeyEvent) -> bool;
    fn dispatch_bubble(&mut self, key: KeyEvent) -> bool;
    fn dispatch_command(&mut self, key: KeyEvent) -> bool;
    fn dispatch_framework(&mut self, key: KeyEvent) -> FrameworkDispatch;
    fn dispatch_terminal_preflight(&mut self, key: KeyEvent) -> TerminalPreflightDispatch;
    fn dispatch_terminal(&mut self, key: KeyEvent) -> bool;
    fn dispatch_ambient_scroll(&mut self, key: KeyEvent) -> bool;
}

pub(crate) fn dispatch_key(
    request: DispatchRequest,
    ops: &mut impl DispatchOps,
) -> DispatchOutcome {
    match request.focus {
        FocusKind::Widget => dispatch_widget_focus(request, ops),
        FocusKind::Terminal => dispatch_terminal_focus(request, ops),
    }
}

fn dispatch_widget_focus(request: DispatchRequest, ops: &mut impl DispatchOps) -> DispatchOutcome {
    if let Some(outcome) = handle_command_chord(request, ops) {
        return outcome;
    }

    match request.key_policy {
        KeyDispatchPolicy::WidgetFirst => {
            dispatch_widget_bubble_command_framework(request.key, ops)
        }
        KeyDispatchPolicy::AppCommandsFirst => {
            if ops.dispatch_command(request.key) {
                DispatchOutcome::Command
            } else {
                dispatch_widget_bubble_framework(request.key, ops)
            }
        }
    }
}

fn handle_command_chord(
    request: DispatchRequest,
    ops: &mut impl DispatchOps,
) -> Option<DispatchOutcome> {
    match ops.continue_command_chord(request.key) {
        CommandDispatchState::Pending => Some(DispatchOutcome::CommandPending),
        CommandDispatchState::Matched(_) => Some(DispatchOutcome::Command),
        CommandDispatchState::Mismatch => match request.chord_mismatch_policy {
            ChordMismatchPolicy::SwallowPrefixReplayCurrent
            | ChordMismatchPolicy::ForwardPrefixAndCurrent => None,
            ChordMismatchPolicy::CancelOnly => Some(DispatchOutcome::Unhandled),
        },
        CommandDispatchState::None => None,
    }
}

fn dispatch_terminal_focus(
    request: DispatchRequest,
    ops: &mut impl DispatchOps,
) -> DispatchOutcome {
    match request.terminal_policy {
        TerminalKeyPolicy::FrameworkFirst => match ops.dispatch_framework(request.key) {
            FrameworkDispatch::Handled => DispatchOutcome::Framework,
            FrameworkDispatch::Quit => DispatchOutcome::FrameworkQuit,
            FrameworkDispatch::None => dispatch_terminal_forward(request.key, ops),
        },
        TerminalKeyPolicy::AppCommandsThenTerminal => {
            match ops.dispatch_terminal_preflight(request.key) {
                TerminalPreflightDispatch::Consumed => return DispatchOutcome::TerminalPreflight,
                TerminalPreflightDispatch::NotApplicable
                | TerminalPreflightDispatch::NotConsumed => {}
            }
            if let Some(outcome) = handle_command_chord(request, ops) {
                return outcome;
            }
            if ops.dispatch_command(request.key) {
                return DispatchOutcome::Command;
            }
            if let Some(outcome) = dispatch_terminal_then_bubble_framework(request.key, ops) {
                return outcome;
            }
            DispatchOutcome::Unhandled
        }
        TerminalKeyPolicy::TerminalFirst => {
            match ops.dispatch_terminal_preflight(request.key) {
                TerminalPreflightDispatch::Consumed => return DispatchOutcome::TerminalPreflight,
                TerminalPreflightDispatch::NotApplicable
                | TerminalPreflightDispatch::NotConsumed => {}
            }
            if ops.dispatch_terminal(request.key) {
                return DispatchOutcome::Terminal;
            }
            if ops.dispatch_command(request.key) {
                return DispatchOutcome::Command;
            }
            if ops.dispatch_bubble(request.key) {
                return DispatchOutcome::Bubble;
            }
            dispatch_framework_only(request.key, ops)
        }
        TerminalKeyPolicy::TerminalOnly => match ops.dispatch_terminal_preflight(request.key) {
            TerminalPreflightDispatch::Consumed => DispatchOutcome::TerminalPreflight,
            TerminalPreflightDispatch::NotApplicable | TerminalPreflightDispatch::NotConsumed => {
                dispatch_terminal_forward(request.key, ops)
            }
        },
    }
}

fn dispatch_widget_bubble_command_framework(
    key: KeyEvent,
    ops: &mut impl DispatchOps,
) -> DispatchOutcome {
    if ops.dispatch_widget(key) {
        DispatchOutcome::Widget
    } else if ops.dispatch_bubble(key) {
        DispatchOutcome::Bubble
    } else if ops.dispatch_command(key) {
        DispatchOutcome::Command
    } else {
        dispatch_framework_ambient(key, ops)
    }
}

fn dispatch_widget_bubble_framework(key: KeyEvent, ops: &mut impl DispatchOps) -> DispatchOutcome {
    if ops.dispatch_widget(key) {
        DispatchOutcome::Widget
    } else if ops.dispatch_bubble(key) {
        DispatchOutcome::Bubble
    } else {
        dispatch_framework_ambient(key, ops)
    }
}

fn dispatch_framework_ambient(key: KeyEvent, ops: &mut impl DispatchOps) -> DispatchOutcome {
    match ops.dispatch_framework(key) {
        FrameworkDispatch::Handled => DispatchOutcome::Framework,
        FrameworkDispatch::Quit => DispatchOutcome::FrameworkQuit,
        FrameworkDispatch::None if ops.dispatch_ambient_scroll(key) => {
            DispatchOutcome::AmbientScroll
        }
        FrameworkDispatch::None => DispatchOutcome::Unhandled,
    }
}

fn dispatch_terminal_forward(key: KeyEvent, ops: &mut impl DispatchOps) -> DispatchOutcome {
    if ops.dispatch_terminal(key) {
        DispatchOutcome::Terminal
    } else {
        DispatchOutcome::Unhandled
    }
}

fn dispatch_terminal_then_bubble_framework(
    key: KeyEvent,
    ops: &mut impl DispatchOps,
) -> Option<DispatchOutcome> {
    if ops.dispatch_terminal(key) {
        return Some(DispatchOutcome::Terminal);
    }
    if ops.dispatch_bubble(key) {
        return Some(DispatchOutcome::Bubble);
    }
    match ops.dispatch_framework(key) {
        FrameworkDispatch::Handled => Some(DispatchOutcome::Framework),
        FrameworkDispatch::Quit => Some(DispatchOutcome::FrameworkQuit),
        FrameworkDispatch::None => None,
    }
}

fn dispatch_framework_only(key: KeyEvent, ops: &mut impl DispatchOps) -> DispatchOutcome {
    match ops.dispatch_framework(key) {
        FrameworkDispatch::Handled => DispatchOutcome::Framework,
        FrameworkDispatch::Quit => DispatchOutcome::FrameworkQuit,
        FrameworkDispatch::None => DispatchOutcome::Unhandled,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum TerminalPreflightDispatch {
    Consumed,
    NotApplicable,
    NotConsumed,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};

    #[derive(Default)]
    struct FakeOps {
        widget_handles: bool,
        bubble_handles: bool,
        terminal_preflight_handles: bool,
        terminal_preflight_not_consumed: bool,
        terminal_handles: bool,
        command_chord_match: Option<&'static str>,
        command_match: Option<&'static str>,
        command_pending: bool,
        command_mismatch: bool,
        framework_quit: bool,
        framework_handles: bool,
        ambient_handles: bool,
        calls: Vec<&'static str>,
    }

    impl FakeOps {
        fn from_case(case: &Case) -> Self {
            Self {
                widget_handles: case.widget_handles,
                bubble_handles: case.bubble_handles,
                terminal_preflight_handles: case.terminal_preflight_handles,
                terminal_preflight_not_consumed: case.terminal_preflight_not_consumed,
                terminal_handles: case.terminal_handles,
                command_chord_match: case.command_chord_match,
                command_match: case.command_match,
                command_pending: case.command_pending,
                command_mismatch: case.command_mismatch,
                framework_quit: case.framework_quit,
                framework_handles: case.framework_handles,
                ambient_handles: case.ambient_handles,
                calls: Vec::new(),
            }
        }
    }

    impl DispatchOps for FakeOps {
        fn continue_command_chord(&mut self, _key: KeyEvent) -> CommandDispatchState {
            self.calls.push("command-chord");
            if self.command_pending {
                return CommandDispatchState::Pending;
            }
            if self.command_mismatch {
                return CommandDispatchState::Mismatch;
            }
            self.command_chord_match
                .map(|id| CommandDispatchState::Matched(id.into()))
                .unwrap_or(CommandDispatchState::None)
        }

        fn dispatch_widget(&mut self, _key: KeyEvent) -> bool {
            self.calls.push("widget");
            self.widget_handles
        }

        fn dispatch_bubble(&mut self, _key: KeyEvent) -> bool {
            self.calls.push("bubble");
            self.bubble_handles
        }

        fn dispatch_command(&mut self, _key: KeyEvent) -> bool {
            self.calls.push("command");
            self.command_match.is_some()
        }

        fn dispatch_framework(&mut self, _key: KeyEvent) -> FrameworkDispatch {
            self.calls.push("framework");
            if self.framework_quit {
                FrameworkDispatch::Quit
            } else if self.framework_handles {
                FrameworkDispatch::Handled
            } else {
                FrameworkDispatch::None
            }
        }

        fn dispatch_terminal_preflight(&mut self, _key: KeyEvent) -> TerminalPreflightDispatch {
            self.calls.push("terminal-preflight");
            if self.terminal_preflight_handles {
                TerminalPreflightDispatch::Consumed
            } else if self.terminal_preflight_not_consumed {
                TerminalPreflightDispatch::NotConsumed
            } else {
                TerminalPreflightDispatch::NotApplicable
            }
        }

        fn dispatch_terminal(&mut self, _key: KeyEvent) -> bool {
            self.calls.push("terminal");
            self.terminal_handles
        }

        fn dispatch_ambient_scroll(&mut self, _key: KeyEvent) -> bool {
            self.calls.push("ambient");
            self.ambient_handles
        }
    }

    fn ctrl(ch: char) -> KeyEvent {
        KeyEvent {
            code: KeyCode::Char(ch),
            mods: KeyMods {
                ctrl: true,
                ..KeyMods::default()
            },
        }
    }

    struct Case {
        name: &'static str,
        focus: FocusKind,
        key_policy: KeyDispatchPolicy,
        terminal_policy: TerminalKeyPolicy,
        widget_handles: bool,
        bubble_handles: bool,
        terminal_preflight_handles: bool,
        terminal_preflight_not_consumed: bool,
        terminal_handles: bool,
        command_chord_match: Option<&'static str>,
        command_match: Option<&'static str>,
        command_pending: bool,
        command_mismatch: bool,
        framework_quit: bool,
        framework_handles: bool,
        ambient_handles: bool,
        expected_calls: &'static [&'static str],
        expected: DispatchOutcome,
    }

    #[test]
    fn dispatch_matrix_covers_focus_policy_and_consumption_paths() {
        let cases = [
            Case {
                name: "widget-first widget consumes",
                focus: FocusKind::Widget,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: true,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: false,
                command_chord_match: None,
                command_match: Some("app.save"),
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["command-chord", "widget"],
                expected: DispatchOutcome::Widget,
            },
            Case {
                name: "widget-first bubble consumes before command",
                focus: FocusKind::Widget,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: false,
                bubble_handles: true,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: false,
                command_chord_match: None,
                command_match: Some("app.save"),
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["command-chord", "widget", "bubble"],
                expected: DispatchOutcome::Bubble,
            },
            Case {
                name: "widget-first command after bubble miss",
                focus: FocusKind::Widget,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: false,
                command_chord_match: None,
                command_match: Some("app.save"),
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["command-chord", "widget", "bubble", "command"],
                expected: DispatchOutcome::Command,
            },
            Case {
                name: "widget-first framework fallback",
                focus: FocusKind::Widget,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: false,
                command_chord_match: None,
                command_match: None,
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["command-chord", "widget", "bubble", "command", "framework"],
                expected: DispatchOutcome::FrameworkQuit,
            },
            Case {
                name: "widget-first ambient fallback",
                focus: FocusKind::Widget,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: false,
                command_chord_match: None,
                command_match: None,
                command_pending: false,
                command_mismatch: false,
                framework_quit: false,
                framework_handles: false,
                ambient_handles: true,
                expected_calls: &[
                    "command-chord",
                    "widget",
                    "bubble",
                    "command",
                    "framework",
                    "ambient",
                ],
                expected: DispatchOutcome::AmbientScroll,
            },
            Case {
                name: "app-commands-first command before widget",
                focus: FocusKind::Widget,
                key_policy: KeyDispatchPolicy::AppCommandsFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: true,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: false,
                command_chord_match: None,
                command_match: Some("app.save"),
                command_pending: false,
                command_mismatch: false,
                framework_quit: false,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["command-chord", "command"],
                expected: DispatchOutcome::Command,
            },
            Case {
                name: "completed widget command chord consumes before widget",
                focus: FocusKind::Widget,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: true,
                bubble_handles: true,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: false,
                command_chord_match: Some("app.save"),
                command_match: None,
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["command-chord"],
                expected: DispatchOutcome::Command,
            },
            Case {
                name: "widget framework handled fallback",
                focus: FocusKind::Widget,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: false,
                command_chord_match: None,
                command_match: None,
                command_pending: false,
                command_mismatch: false,
                framework_quit: false,
                framework_handles: true,
                ambient_handles: true,
                expected_calls: &["command-chord", "widget", "bubble", "command", "framework"],
                expected: DispatchOutcome::Framework,
            },
            Case {
                name: "terminal framework-first quit",
                focus: FocusKind::Terminal,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: true,
                command_chord_match: None,
                command_match: Some("mux.detach"),
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["framework"],
                expected: DispatchOutcome::FrameworkQuit,
            },
            Case {
                name: "terminal framework-first handled",
                focus: FocusKind::Terminal,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: true,
                command_chord_match: None,
                command_match: Some("mux.detach"),
                command_pending: false,
                command_mismatch: false,
                framework_quit: false,
                framework_handles: true,
                ambient_handles: false,
                expected_calls: &["framework"],
                expected: DispatchOutcome::Framework,
            },
            Case {
                name: "terminal app-then-terminal preflight wins",
                focus: FocusKind::Terminal,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::AppCommandsThenTerminal,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: true,
                terminal_preflight_not_consumed: false,
                terminal_handles: true,
                command_chord_match: None,
                command_match: Some("mux.copy"),
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["terminal-preflight"],
                expected: DispatchOutcome::TerminalPreflight,
            },
            Case {
                name: "terminal app-then-terminal command wins after preflight miss",
                focus: FocusKind::Terminal,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::AppCommandsThenTerminal,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: true,
                command_chord_match: None,
                command_match: Some("mux.detach"),
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["terminal-preflight", "command-chord", "command"],
                expected: DispatchOutcome::Command,
            },
            Case {
                name: "terminal app command chord consumes after preflight miss",
                focus: FocusKind::Terminal,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::AppCommandsThenTerminal,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: true,
                command_chord_match: Some("mux.detach"),
                command_match: None,
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["terminal-preflight", "command-chord"],
                expected: DispatchOutcome::Command,
            },
            Case {
                name: "terminal preflight not-consumed forwards to terminal",
                focus: FocusKind::Terminal,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::TerminalFirst,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: true,
                terminal_handles: true,
                command_chord_match: None,
                command_match: None,
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["terminal-preflight", "terminal"],
                expected: DispatchOutcome::Terminal,
            },
            Case {
                name: "terminal-first terminal consumes",
                focus: FocusKind::Terminal,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::TerminalFirst,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: true,
                command_chord_match: None,
                command_match: Some("mux.detach"),
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["terminal-preflight", "terminal"],
                expected: DispatchOutcome::Terminal,
            },
            Case {
                name: "terminal-only never command/framework",
                focus: FocusKind::Terminal,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::TerminalOnly,
                widget_handles: false,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: true,
                command_chord_match: None,
                command_match: Some("mux.detach"),
                command_pending: false,
                command_mismatch: false,
                framework_quit: true,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["terminal-preflight", "terminal"],
                expected: DispatchOutcome::Terminal,
            },
            Case {
                name: "command chord pending consumes",
                focus: FocusKind::Widget,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: true,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: false,
                command_chord_match: None,
                command_match: None,
                command_pending: true,
                command_mismatch: false,
                framework_quit: false,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["command-chord"],
                expected: DispatchOutcome::CommandPending,
            },
            Case {
                name: "command chord mismatch replays current to widget",
                focus: FocusKind::Widget,
                key_policy: KeyDispatchPolicy::WidgetFirst,
                terminal_policy: TerminalKeyPolicy::FrameworkFirst,
                widget_handles: true,
                bubble_handles: false,
                terminal_preflight_handles: false,
                terminal_preflight_not_consumed: false,
                terminal_handles: false,
                command_chord_match: None,
                command_match: None,
                command_pending: false,
                command_mismatch: true,
                framework_quit: false,
                framework_handles: false,
                ambient_handles: false,
                expected_calls: &["command-chord", "widget"],
                expected: DispatchOutcome::Widget,
            },
        ];

        for case in cases {
            let mut ops = FakeOps::from_case(&case);
            let outcome = dispatch_key(
                DispatchRequest::new(ctrl('s'), case.focus)
                    .key_policy(case.key_policy)
                    .terminal_policy(case.terminal_policy),
                &mut ops,
            );
            assert_eq!(ops.calls, case.expected_calls, "{} calls", case.name);
            assert_eq!(outcome, case.expected, "{} outcome", case.name);
        }
    }
}
