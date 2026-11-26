//! Shared hover-reveal timer state for delayed tooltips and context-menu submenus.

use std::sync::Arc;
use std::time::Duration;

use web_time::Instant;

use crate::core::runtime_env::RuntimeEnv;

/// Timing configuration for a [`HoverReveal`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct HoverDelay {
    /// How long the pointer must rest before revealing.
    pub delay: Duration,
    /// Skip [`delay`](Self::delay) when a sibling closed less than this long ago.
    pub skip_delay: Duration,
    /// Warm-up group recorded in `env.tooltip_closed_at` when a delayed reveal closes (`None`
    /// disables cross-sibling warm-up).
    pub warmup_group: Option<Option<Arc<str>>>,
}

impl HoverDelay {
    /// Fixed delay with no cross-sibling warm window.
    pub(crate) const fn fixed(delay: Duration) -> Self {
        Self {
            delay,
            skip_delay: Duration::ZERO,
            warmup_group: None,
        }
    }
}

/// What a hover change asks of the component that owns a [`HoverReveal`].
#[must_use]
pub(crate) enum RevealStep {
    /// Nothing visible changed.
    Nothing,
    /// Visibility changed: render again.
    Render,
    /// Send a reveal for `generation` after `wait`.
    Arm { wait: Duration, generation: u64 },
}

/// Hover timing behind delayed popups (tooltips and cascading submenus): wait `delay` after the
/// pointer arrives, optionally skip the wait while a sibling closed moments ago, and guard timers
/// with a monotonically bumped generation so stale timers are ignored.
#[derive(Clone, Debug, Default)]
pub(crate) struct HoverReveal {
    /// Whether the pointer is over the trigger.
    hovered: bool,
    /// Whether the popup has finished waiting and is shown.
    revealed: bool,
    /// Bumped on every hover change and timing change so a timer armed earlier is ignored.
    generation: u64,
    /// When the current hover began, so a changed delay counts from the original entry.
    hovered_since: Option<Instant>,
    /// Whether the current reveal skipped the delay, taking over from another popup: such a
    /// tooltip appears at once instead of fading in.
    instant: bool,
}

impl HoverReveal {
    pub(crate) fn revealed(&self) -> bool {
        self.revealed
    }

    /// Whether the current reveal took over from another popup without waiting.
    pub(crate) fn instant(&self) -> bool {
        self.revealed && self.instant
    }

    /// The popup, already revealed, takes over again from another one without waiting.
    pub(crate) fn mark_instant(&mut self) {
        self.instant = true;
    }

    /// The pointer entered (`true`) or left (`false`) the trigger.
    pub(crate) fn hover(
        &mut self,
        hovered: bool,
        timing: &HoverDelay,
        env: &RuntimeEnv,
    ) -> RevealStep {
        self.hovered = hovered;
        self.generation = self.generation.wrapping_add(1);
        if !hovered {
            self.hovered_since = None;
            if !std::mem::take(&mut self.revealed) {
                return RevealStep::Nothing;
            }
            // Only a delayed popup the user actually saw warms its neighbours: one that opens at
            // once never made the user wait.
            if !timing.delay.is_zero()
                && let Some(group) = &timing.warmup_group
            {
                env.tooltip_closed_at
                    .borrow_mut()
                    .insert(group.clone(), env.now());
            }
            return RevealStep::Render;
        }
        self.hovered_since = Some(env.now());
        let warm = timing.warmup_group.as_ref().is_some_and(|group| {
            env.tooltip_closed_at
                .borrow()
                .get(group)
                .is_some_and(|closed| env.elapsed(*closed) < timing.skip_delay)
        });
        if warm {
            self.revealed = true;
            self.instant = true;
            return RevealStep::Render;
        }
        self.arm(timing.delay)
    }

    /// The pointer moved onto something inside the trigger that suspends the wait, such as a
    /// nested tooltip's trigger: cancel a pending reveal. A popup already revealed stays so.
    pub(crate) fn pause(&mut self) {
        if self.revealed {
            return;
        }
        self.generation = self.generation.wrapping_add(1);
        self.hovered = false;
        self.hovered_since = None;
    }

    /// The pointer entered the trigger to take over from a popup already showing: open at once.
    pub(crate) fn hover_revealed(&mut self, env: &RuntimeEnv) -> RevealStep {
        self.hovered = true;
        self.generation = self.generation.wrapping_add(1);
        self.hovered_since = Some(env.now());
        self.revealed = true;
        self.instant = true;
        RevealStep::Render
    }

    /// A timer armed for `generation` elapsed; returns whether the popup opened.
    pub(crate) fn reveal(&mut self, generation: u64) -> bool {
        if generation != self.generation || !self.hovered {
            return false;
        }
        self.revealed = true;
        self.instant = false;
        true
    }

    /// Hover stopped being timed here: forget it without warming siblings.
    pub(crate) fn reset(&mut self) -> RevealStep {
        self.generation = self.generation.wrapping_add(1);
        self.hovered = false;
        self.hovered_since = None;
        if std::mem::take(&mut self.revealed) {
            RevealStep::Render
        } else {
            RevealStep::Nothing
        }
    }

    /// The delay changed: re-arm the current hover, counting from when it began.
    pub(crate) fn retime(&mut self, delay: Duration, env: &RuntimeEnv) -> RevealStep {
        // Whatever timer is pending was armed for the old delay.
        self.generation = self.generation.wrapping_add(1);
        let Some(since) = self.hovered_since.filter(|_| self.hovered) else {
            return RevealStep::Nothing;
        };
        if self.revealed {
            return RevealStep::Nothing;
        }
        let waited = env.elapsed(since);
        self.arm(delay.saturating_sub(waited))
    }

    /// Schedule a reveal for the current hover after `wait`, or reveal now when nothing is left.
    fn arm(&mut self, wait: Duration) -> RevealStep {
        self.instant = false;
        if wait.is_zero() {
            self.revealed = true;
            return RevealStep::Render;
        }
        RevealStep::Arm {
            wait,
            generation: self.generation,
        }
    }
}
