use std::sync::Arc;
use std::time::Duration;

use crate::core::event::{KeyEvent, MouseButton};

use super::{Selector, SelectorMatch, SemanticTree};

/// Logical clock behavior fixed for the lifetime of a session.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ClockMode {
    /// Logical time follows elapsed wall time.
    Realtime,
    /// Logical time moves only through explicit advancement.
    #[default]
    Controlled,
}

/// Stable predicate used by automation waits.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WaitCondition {
    /// At least one node matches.
    Exists(Selector),
    /// No node matches.
    Missing(Selector),
    /// Exactly one matching node intersects the viewport.
    InView(Selector),
    /// Exactly one matching node has focus.
    Focused(Selector),
    /// Exactly one matching node is enabled or disabled.
    Enabled {
        /// Target selector.
        selector: Selector,
        /// Required enabled state.
        enabled: bool,
    },
    /// Exactly one matching node has the requested selection state.
    Selected {
        /// Target selector.
        selector: Selector,
        /// Required selection state.
        selected: bool,
    },
    /// Exactly one matching node has this safe value.
    ValueEquals {
        /// Target selector.
        selector: Selector,
        /// Required value.
        value: Arc<str>,
    },
    /// At least one matching node's safe text contains this string.
    TextContains {
        /// Target selector.
        selector: Selector,
        /// Required text.
        text: Arc<str>,
    },
    /// The selector resolves to exactly `count` nodes.
    Count {
        /// Target selector.
        selector: Selector,
        /// Required count.
        count: usize,
    },
}

impl WaitCondition {
    /// Wait until a selector has at least one match.
    pub fn exists(selector: Selector) -> Self {
        Self::Exists(selector)
    }

    /// Wait until a selector has no matches.
    pub fn missing(selector: Selector) -> Self {
        Self::Missing(selector)
    }

    /// Wait until one match intersects the viewport.
    pub fn in_view(selector: Selector) -> Self {
        Self::InView(selector)
    }

    /// Wait until one match has focus.
    pub fn focused(selector: Selector) -> Self {
        Self::Focused(selector)
    }

    /// Wait until one match is enabled.
    pub fn enabled(selector: Selector) -> Self {
        Self::Enabled {
            selector,
            enabled: true,
        }
    }

    /// Wait until one match is disabled.
    pub fn disabled(selector: Selector) -> Self {
        Self::Enabled {
            selector,
            enabled: false,
        }
    }

    /// Wait until one match has the requested selection state.
    pub fn selected(selector: Selector, selected: bool) -> Self {
        Self::Selected { selector, selected }
    }

    /// Wait until one match has the requested safe value.
    pub fn value_equals(selector: Selector, value: impl Into<Arc<str>>) -> Self {
        Self::ValueEquals {
            selector,
            value: value.into(),
        }
    }

    /// Wait until matching safe semantic text contains `text`.
    pub fn text_contains(selector: Selector, text: impl Into<Arc<str>>) -> Self {
        Self::TextContains {
            selector,
            text: text.into(),
        }
    }

    /// Wait until a selector has exactly `count` matches.
    pub fn count(selector: Selector, count: usize) -> Self {
        Self::Count { selector, count }
    }
}

/// Focus traversal direction used by typed operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FocusDirection {
    /// Move forward.
    Next,
    /// Move backward.
    Previous,
}

/// Scroll direction used by typed operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AutomationScrollDirection {
    /// Scroll upward.
    Up,
    /// Scroll downward.
    Down,
}

/// One typed automation operation.
///
/// Constructors keep the underlying operation private so adding new operations
/// does not make downstream exhaustive matches a breaking change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutomationStep {
    pub(crate) kind: AutomationStepKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AutomationStepKind {
    Key(KeyEvent),
    Type(String),
    Click {
        selector: Selector,
        button: MouseButton,
    },
    Hover(Selector),
    Focus(Selector),
    FocusStep(FocusDirection),
    Scroll {
        selector: Option<Selector>,
        direction: AutomationScrollDirection,
    },
    Drag {
        from: Selector,
        to: Selector,
    },
    Resize(u16, u16),
    Advance(Duration),
    Sleep(Duration),
    DrainReady,
    WaitFor(WaitCondition, Duration),
    Checkpoint(Arc<str>),
}

impl AutomationStep {
    /// Send one key event.
    pub fn key(key: KeyEvent) -> Self {
        Self {
            kind: AutomationStepKind::Key(key),
        }
    }

    /// Type literal text, one key event per character.
    pub fn type_text(text: impl Into<String>) -> Self {
        Self {
            kind: AutomationStepKind::Type(text.into()),
        }
    }

    /// Click the exactly matching node.
    pub fn click(selector: Selector) -> Self {
        Self::click_button(selector, MouseButton::Left)
    }

    /// Click with a specific mouse button.
    pub fn click_button(selector: Selector, button: MouseButton) -> Self {
        Self {
            kind: AutomationStepKind::Click { selector, button },
        }
    }

    /// Move the pointer over the exactly matching node.
    pub fn hover(selector: Selector) -> Self {
        Self {
            kind: AutomationStepKind::Hover(selector),
        }
    }

    /// Focus the exactly matching node.
    pub fn focus(selector: Selector) -> Self {
        Self {
            kind: AutomationStepKind::Focus(selector),
        }
    }

    /// Move focus through the current traversal ring.
    pub fn focus_step(direction: FocusDirection) -> Self {
        Self {
            kind: AutomationStepKind::FocusStep(direction),
        }
    }

    /// Scroll over an optional target.
    pub fn scroll(selector: Option<Selector>, direction: AutomationScrollDirection) -> Self {
        Self {
            kind: AutomationStepKind::Scroll {
                selector,
                direction,
            },
        }
    }

    /// Drag from one exactly matching node to another.
    pub fn drag(from: Selector, to: Selector) -> Self {
        Self {
            kind: AutomationStepKind::Drag { from, to },
        }
    }

    /// Resize the current session without resetting application state.
    pub fn resize(width: u16, height: u16) -> Self {
        Self {
            kind: AutomationStepKind::Resize(width.max(1), height.max(1)),
        }
    }

    /// Advance a controlled session's logical clock.
    pub fn advance(duration: Duration) -> Self {
        Self {
            kind: AutomationStepKind::Advance(duration),
        }
    }

    pub(crate) fn sleep(duration: Duration) -> Self {
        Self {
            kind: AutomationStepKind::Sleep(duration),
        }
    }

    pub(crate) fn advance_duration(&self) -> Option<Duration> {
        match self.kind {
            AutomationStepKind::Advance(duration) => Some(duration),
            _ => None,
        }
    }

    /// Drain work runnable at the current logical time.
    pub fn drain_ready() -> Self {
        Self {
            kind: AutomationStepKind::DrainReady,
        }
    }

    /// Wait for a condition using a wall-clock timeout.
    pub fn wait_for(condition: WaitCondition, timeout: Duration) -> Self {
        Self {
            kind: AutomationStepKind::WaitFor(condition, timeout),
        }
    }

    /// Commit configured evidence under `name`.
    pub fn checkpoint(name: impl Into<Arc<str>>) -> Self {
        Self {
            kind: AutomationStepKind::Checkpoint(name.into()),
        }
    }
}

/// Activity visible to the session at one instant.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct IdleReport {
    /// Component messages waiting in the UI queue.
    pub queued_messages: usize,
    /// Timers due at the current logical time.
    pub due_timers: usize,
    /// Timers scheduled after the current logical time.
    pub future_timers: usize,
    /// Whether UI state still requires a commit.
    pub dirty: bool,
    /// Runtime-owned commands still tracked.
    pub tracked_commands: usize,
    /// Whether arbitrary external `Link` holders make permanent idleness unknowable.
    pub external_links_untracked: bool,
}

impl IdleReport {
    /// Whether all work tracked by the session is currently quiet.
    pub fn is_idle(&self) -> bool {
        self.queued_messages == 0
            && self.due_timers == 0
            && !self.dirty
            && self.tracked_commands == 0
    }
}

/// Result of one successful operation.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AutomationStepResult {
    /// Zero-based index in the submitted operation sequence.
    pub step_index: usize,
    /// Coherent generation committed after the operation.
    pub generation: u64,
    /// Evidence returned by a checkpoint operation.
    pub checkpoint: Option<super::Checkpoint>,
}

/// Automation failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum AutomationError {
    /// More than one realized node carries the same app-authored identity.
    #[error("duplicate automation ID `{id}`")]
    DuplicateAutomationId {
        /// Repeated identity.
        id: super::AutomationId,
    },
    /// No semantic node matched.
    #[error("selector matched no semantic node: {selector:?}")]
    NoMatch {
        /// Selector that failed.
        selector: Selector,
    },
    /// More than one semantic node matched.
    #[error("selector matched {matches_len} semantic nodes: {selector:?}")]
    AmbiguousMatch {
        /// Selector that was ambiguous.
        selector: Selector,
        /// Safe descriptions of the matches.
        matches: Vec<SelectorMatch>,
        #[doc(hidden)]
        matches_len: usize,
    },
    /// The matching node cannot perform the requested operation.
    #[error("matching semantic node is not actionable for this operation")]
    NotActionable,
    /// The matching node is outside the current viewport.
    #[error("matching semantic node is outside the current viewport")]
    NotInView,
    /// A controlled-only operation was requested from a realtime session.
    #[error("logical time can only be advanced in a controlled session")]
    RealtimeAdvance,
    /// A wait reached its wall-clock deadline.
    #[error("wait timed out after {wall_elapsed:?}: {condition:?}")]
    WaitTimeout {
        /// Requested condition.
        condition: Box<WaitCondition>,
        /// Wall time spent waiting.
        wall_elapsed: Duration,
        /// Logical time elapsed during the wait.
        logical_elapsed: Duration,
        /// Last selector match set.
        last_matches: Box<[SelectorMatch]>,
        /// Last activity report.
        idle: IdleReport,
        /// Last committed semantic tree.
        semantic_tree: Box<SemanticTree>,
        /// Optional diagnostic checkpoint name.
        diagnostic_checkpoint: Option<Box<str>>,
    },
    /// Tracked work did not remain quiet before the wall deadline.
    #[error("session did not become idle within {wall_elapsed:?}")]
    IdleTimeout {
        /// Wall time spent waiting.
        wall_elapsed: Duration,
        /// Last observed activity.
        idle: IdleReport,
    },
    /// Runnable work did not reach a fixed point within the safety limit.
    #[error("session did not converge after {rounds} drain rounds")]
    DrainDidNotConverge {
        /// Number of drain rounds attempted.
        rounds: usize,
        /// Last observed activity.
        idle: IdleReport,
    },
    /// A requested artifact is absent from this build.
    #[error("unsupported checkpoint format: {0}")]
    UnsupportedFormat(&'static str),
    /// A visual checkpoint differs from its committed baseline.
    #[error("visual baseline regression: {summary}")]
    BaselineMismatch {
        /// Human-readable comparison details, including the diff path when available.
        summary: String,
    },
    /// A checkpoint name is invalid.
    #[error("invalid checkpoint name `{0}`")]
    InvalidCheckpointName(String),
    /// The session has shut down.
    #[error("automation session is closed")]
    SessionClosed,
    /// The caller cancelled an in-flight operation.
    #[error("automation operation cancelled")]
    Cancelled,
    /// The caller's wall-clock deadline elapsed during an operation.
    #[error("automation operation deadline exceeded")]
    DeadlineExceeded,
    /// A runtime operation failed.
    #[error(transparent)]
    Runtime(#[from] crate::Error),
    /// Artifact or transport I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
    /// Text script parsing failed.
    #[error("invalid automation script: {0}")]
    Script(String),
    /// One step in a sequence failed.
    #[error("automation step {step_index} failed: {source}")]
    Step {
        /// Zero-based failed step index.
        step_index: usize,
        /// Underlying failure.
        #[source]
        source: Box<AutomationError>,
    },
}

impl AutomationError {
    pub(crate) fn ambiguous(selector: Selector, matches: Vec<SelectorMatch>) -> Self {
        let matches_len = matches.len();
        Self::AmbiguousMatch {
            selector,
            matches,
            matches_len,
        }
    }
}
