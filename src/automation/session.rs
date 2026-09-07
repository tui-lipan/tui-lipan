use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::app::App;
use crate::core::component::Component;
use crate::core::event::{KeyEvent, MouseEvent};
use crate::style::Rect;
use crate::test_backend::TestBackend;

#[cfg(feature = "ui-snapshot-png")]
use super::CheckpointBaseline;
use super::checkpoint::{
    semantic_json, semantic_markdown, validate_checkpoint_name, validate_checkpoint_support,
};
use super::{
    AutomationError, AutomationStep, AutomationStepResult, Checkpoint, CheckpointArtifact,
    CheckpointFormat, CheckpointSink, ClockMode, FocusDirection, IdleReport, Selector,
    SelectorMatch, SemanticTree, WaitCondition, resolve_selector,
};

/// Configuration fixed when an automation session starts.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct AutomationOptions {
    /// Synthetic viewport used by the headless session.
    pub viewport: Rect,
    /// Session clock behavior.
    pub clock_mode: ClockMode,
    /// Evidence sinks run by checkpoint operations.
    pub checkpoint_sinks: Vec<CheckpointSink>,
}

impl Default for AutomationOptions {
    fn default() -> Self {
        Self {
            viewport: Rect {
                x: 0,
                y: 0,
                w: 80,
                h: 24,
            },
            clock_mode: ClockMode::Controlled,
            checkpoint_sinks: Vec::new(),
        }
    }
}

impl AutomationOptions {
    /// Set the synthetic viewport.
    #[must_use]
    pub fn viewport(mut self, width: u16, height: u16) -> Self {
        self.viewport = Rect {
            x: 0,
            y: 0,
            w: width.max(1),
            h: height.max(1),
        };
        self
    }

    /// Select realtime or controlled logical time.
    #[must_use]
    pub fn clock_mode(mut self, mode: ClockMode) -> Self {
        self.clock_mode = mode;
        self
    }

    /// Add a checkpoint sink.
    #[must_use]
    pub fn checkpoint_sink(mut self, sink: CheckpointSink) -> Self {
        self.checkpoint_sinks.push(sink);
        self
    }
}

/// Viewport, semantics, interaction result, and pixels from one coherent commit.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct AutomationSnapshot {
    /// Monotonic session generation.
    pub generation: u64,
    /// Viewport used by this commit.
    pub viewport: Rect,
    /// Hierarchical semantic projection.
    pub semantics: SemanticTree,
    /// Rendered frame.
    pub frame: crate::CapturedFrame,
}

/// Shared UI-thread session implementation used by headless automation.
struct AutomationEngine<C: Component> {
    backend: TestBackend<C>,
    clock_mode: ClockMode,
    generation: u64,
    committed: AutomationSnapshot,
    checkpoint_sinks: Vec<CheckpointSink>,
    closed: bool,
}

impl<C: Component> AutomationEngine<C> {
    fn new(
        app: App,
        component: C,
        props: C::Properties,
        options: AutomationOptions,
    ) -> Result<Self, AutomationError> {
        validate_checkpoint_support(&options.checkpoint_sinks)?;
        let backend = TestBackend::new_with_app_clock(
            app,
            component,
            props,
            options.clock_mode,
            options.viewport,
        );
        if let Some(id) = backend.core.duplicate_automation_id() {
            return Err(AutomationError::DuplicateAutomationId { id: id.clone() });
        }
        let generation = backend.core.generation();
        let semantics = backend.core.semantic_tree(backend.focused);
        let committed = AutomationSnapshot {
            generation,
            viewport: options.viewport,
            semantics,
            frame: backend.capture_frame(),
        };
        Ok(Self {
            backend,
            clock_mode: options.clock_mode,
            generation,
            committed,
            checkpoint_sinks: options.checkpoint_sinks,
            closed: false,
        })
    }

    fn ensure_open(&self) -> Result<(), AutomationError> {
        if self.closed || self.backend.core.is_shutdown() {
            Err(AutomationError::SessionClosed)
        } else {
            Ok(())
        }
    }

    fn commit(&mut self) -> Result<(), AutomationError> {
        self.backend.render();
        if let Some(id) = self.backend.core.duplicate_automation_id() {
            return Err(AutomationError::DuplicateAutomationId { id: id.clone() });
        }
        self.generation = self.backend.core.generation();
        let viewport = self.backend.viewport();
        let semantics = self.backend.core.semantic_tree(self.backend.focused);
        self.committed = AutomationSnapshot {
            generation: self.generation,
            viewport,
            semantics,
            frame: self.backend.capture_frame(),
        };
        Ok(())
    }

    fn drain_ready(&mut self) -> Result<IdleReport, AutomationError> {
        self.ensure_open()?;
        crate::session::drain_to_fixed_point(self)
    }

    fn idle_report(&self, dirty: bool) -> IdleReport {
        let (due_timers, future_timers) = self.backend.core.deferred_timer_counts();
        IdleReport {
            queued_messages: self.backend.core.queue.borrow().len(),
            due_timers,
            future_timers,
            dirty,
            tracked_commands: self.backend.core.ctx.env().activity.tracked_commands(),
            external_links_untracked: true,
        }
    }

    fn checkpoint(&mut self, name: &str) -> Result<Checkpoint, AutomationError> {
        self.ensure_open()?;
        validate_checkpoint_name(name)?;
        validate_checkpoint_support(&self.checkpoint_sinks)?;
        #[cfg(feature = "ui-snapshot-png")]
        let snapshot = self.backend.capture_ui_snapshot();
        let artifacts = self
            .checkpoint_sinks
            .iter()
            .map(|sink| {
                self.checkpoint_artifact(
                    name,
                    sink,
                    #[cfg(feature = "ui-snapshot-png")]
                    &snapshot,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Checkpoint {
            name: Arc::from(name),
            generation: self.generation,
            artifacts,
        })
    }

    fn checkpoint_artifact(
        &self,
        name: &str,
        sink: &CheckpointSink,
        #[cfg(feature = "ui-snapshot-png")] snapshot: &crate::ui_snapshot::UiSnapshot,
    ) -> Result<CheckpointArtifact, AutomationError> {
        match sink {
            CheckpointSink::Markdown { directory } => self.file_artifact(
                directory,
                name,
                "md",
                CheckpointFormat::Markdown,
                semantic_markdown(&self.committed.semantics),
            ),
            CheckpointSink::Json { directory } => self.file_artifact(
                directory,
                name,
                "json",
                CheckpointFormat::Json,
                semantic_json(&self.committed.semantics),
            ),
            CheckpointSink::Png { directory } => {
                #[cfg(feature = "ui-snapshot-png")]
                {
                    let bytes = snapshot.to_png_default()?;
                    self.file_artifact(directory, name, "png", CheckpointFormat::Png, bytes)
                }
                #[cfg(not(feature = "ui-snapshot-png"))]
                {
                    let _ = directory;
                    Err(AutomationError::UnsupportedFormat("PNG"))
                }
            }
            CheckpointSink::RecordingMarker => Ok(CheckpointArtifact {
                format: CheckpointFormat::RecordingMarker,
                bytes: Some(Arc::from(
                    format!("{name}@{}", self.generation).into_bytes(),
                )),
                path: None,
                baseline: None,
            }),
            CheckpointSink::Baseline {
                directory,
                tolerance,
            } => {
                #[cfg(feature = "ui-snapshot-png")]
                {
                    self.baseline_artifact(name, directory, *tolerance, snapshot)
                }
                #[cfg(not(feature = "ui-snapshot-png"))]
                {
                    let _ = (directory, tolerance);
                    Err(AutomationError::UnsupportedFormat("PNG baseline"))
                }
            }
        }
    }

    fn file_artifact(
        &self,
        directory: &std::path::Path,
        name: &str,
        extension: &str,
        format: CheckpointFormat,
        bytes: impl AsRef<[u8]>,
    ) -> Result<CheckpointArtifact, AutomationError> {
        let path = directory.join(format!("{name}.{extension}"));
        crate::utils::atomic_file::write(&path, bytes.as_ref())?;
        Ok(CheckpointArtifact {
            format,
            bytes: None,
            path: Some(path),
            baseline: None,
        })
    }

    #[cfg(feature = "ui-snapshot-png")]
    fn baseline_artifact(
        &self,
        name: &str,
        directory: &std::path::Path,
        tolerance: f64,
        snapshot: &crate::ui_snapshot::UiSnapshot,
    ) -> Result<CheckpointArtifact, AutomationError> {
        let comparison = snapshot
            .clone()
            .baseline(directory)
            .name(name)
            .tolerance(tolerance)
            .check()?;
        let baseline = match comparison.outcome {
            crate::ui_snapshot::BaselineOutcome::Created => CheckpointBaseline::Created,
            crate::ui_snapshot::BaselineOutcome::Match { ratio } => {
                CheckpointBaseline::Matched { ratio }
            }
            crate::ui_snapshot::BaselineOutcome::Updated => CheckpointBaseline::Updated,
            outcome => {
                return Err(AutomationError::BaselineMismatch {
                    summary: outcome.summary(name),
                });
            }
        };
        Ok(CheckpointArtifact {
            format: CheckpointFormat::Baseline,
            bytes: None,
            path: Some(comparison.baseline_path),
            baseline: Some(baseline),
        })
    }

    fn shutdown(&mut self) {
        if !self.closed {
            self.backend.core.shutdown();
            self.closed = true;
        }
    }
}

impl<C: Component> crate::session::OperationHost for AutomationEngine<C> {
    fn duplicate_automation_id(&self) -> Option<super::AutomationId> {
        self.backend.core.duplicate_automation_id().cloned()
    }

    fn semantics(&self) -> SemanticTree {
        self.committed.semantics.clone()
    }

    fn current_pointer(&self) -> Option<(u16, u16)> {
        self.backend.mouse.last_mouse.get()
    }

    fn clock_mode(&self) -> ClockMode {
        self.clock_mode
    }

    fn send_key(&mut self, key: KeyEvent) -> crate::Result<()> {
        self.backend.send_key(key).map(|_| ())
    }

    fn send_mouse(&mut self, event: MouseEvent) -> crate::Result<()> {
        self.backend.send_mouse(event).map(|_| ())
    }

    fn focus_node(&mut self, node: crate::core::node::NodeId) -> crate::Result<bool> {
        if !self.backend.core.tree.node(node).is_focusable() {
            return Ok(false);
        }
        self.backend.set_focused(node);
        Ok(true)
    }

    fn focus_step(&mut self, direction: FocusDirection) -> crate::Result<()> {
        match direction {
            FocusDirection::Next => self.backend.focus_next(),
            FocusDirection::Previous => self.backend.focus_prev(),
        }
        Ok(())
    }

    fn resize(&mut self, width: u16, height: u16) -> Result<(), AutomationError> {
        self.backend.set_viewport(Rect {
            x: 0,
            y: 0,
            w: width,
            h: height,
        });
        Ok(())
    }

    fn advance(&mut self, duration: Duration) -> Result<(), AutomationError> {
        self.backend.advance(duration);
        Ok(())
    }

    fn sleep(&mut self, duration: Duration) -> Result<(), AutomationError> {
        self.backend.settle(duration).map_err(AutomationError::from)
    }

    fn drain_round(&mut self) -> Result<bool, AutomationError> {
        self.backend.core.drain_due_timers();
        self.backend.pump().map_err(AutomationError::from)
    }

    fn idle_report(&self, dirty: bool) -> IdleReport {
        AutomationEngine::idle_report(self, dirty)
    }

    fn logical_elapsed(&self) -> Duration {
        self.backend.core.ctx.env().clock.elapsed()
    }

    fn wait_for_activity(&mut self, timeout: Duration) {
        let _ = self.backend.core.wait_for_command(timeout);
    }

    fn commit_after_drain(&mut self) -> Result<(), AutomationError> {
        self.commit()
    }

    fn checkpoint(&mut self, name: &str) -> Result<Checkpoint, AutomationError> {
        AutomationEngine::checkpoint(self, name)
    }
}

impl<C: Component> Drop for AutomationEngine<C> {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// In-process typed control of one persistent mounted application.
pub struct AutomationSession<C: Component> {
    engine: AutomationEngine<C>,
}

impl<C: Component> AutomationSession<C> {
    fn execute_operation(
        &mut self,
        step: &AutomationStep,
    ) -> Result<Option<Checkpoint>, AutomationError> {
        crate::session::execute_operation(&mut self.engine, &step.kind)
    }

    /// Mount with explicit app configuration and component properties.
    pub fn with_app(
        app: App,
        component: C,
        props: C::Properties,
        options: AutomationOptions,
    ) -> Result<Self, AutomationError> {
        Ok(Self {
            engine: AutomationEngine::new(app, component, props, options)?,
        })
    }

    /// Execute one typed operation.
    pub fn execute(
        &mut self,
        step: AutomationStep,
    ) -> Result<AutomationStepResult, AutomationError> {
        let checkpoint = self.execute_operation(&step)?;
        Ok(AutomationStepResult {
            step_index: 0,
            generation: self.engine.generation,
            checkpoint,
        })
    }

    /// Execute operations in order and stop on the first error.
    pub fn execute_all(
        &mut self,
        steps: impl IntoIterator<Item = AutomationStep>,
    ) -> Result<Vec<AutomationStepResult>, AutomationError> {
        let mut results = Vec::new();
        for (step_index, step) in steps.into_iter().enumerate() {
            let checkpoint =
                self.execute_operation(&step)
                    .map_err(|source| AutomationError::Step {
                        step_index,
                        source: Box::new(source),
                    })?;
            results.push(AutomationStepResult {
                step_index,
                generation: self.engine.generation,
                checkpoint,
            });
        }
        Ok(results)
    }

    /// Compile and execute a textual action script.
    ///
    /// `#name` selects an app-authored [`super::AutomationId`], never a reconciliation key.
    pub fn execute_script(
        &mut self,
        script: &str,
    ) -> Result<Vec<AutomationStepResult>, AutomationError> {
        self.execute_all(crate::ui_snapshot::compile_script(script)?)
    }

    /// Drain work runnable at the current logical time.
    pub fn drain_ready(&mut self) -> Result<IdleReport, AutomationError> {
        self.engine.drain_ready()
    }

    /// Wait for bounded realtime quiescence of tracked session work.
    pub fn wait_for_idle(
        &mut self,
        timeout: Duration,
        quiet_window: Duration,
    ) -> Result<IdleReport, AutomationError> {
        let start = Instant::now();
        let deadline = start.checked_add(timeout).unwrap_or(start);
        let mut quiet_since = None;
        loop {
            let report = self.engine.drain_ready()?;
            if report.is_idle() {
                let since = quiet_since.get_or_insert_with(Instant::now);
                if since.elapsed() >= quiet_window {
                    return Ok(report);
                }
            } else {
                quiet_since = None;
            }
            let now = Instant::now();
            if now >= deadline {
                return Err(AutomationError::IdleTimeout {
                    wall_elapsed: now.saturating_duration_since(start),
                    idle: report,
                });
            }
            let wait = quiet_since
                .map(|since| quiet_window.saturating_sub(since.elapsed()))
                .unwrap_or_else(|| deadline.saturating_duration_since(now))
                .min(deadline.saturating_duration_since(now));
            let _ = self.engine.backend.core.wait_for_command(wait);
        }
    }

    /// Wait for a semantic condition. Controlled time stays frozen.
    pub fn wait_for(
        &mut self,
        condition: WaitCondition,
        timeout: Duration,
    ) -> Result<AutomationStepResult, AutomationError> {
        crate::session::wait_for(&mut self.engine, &condition, timeout)?;
        Ok(AutomationStepResult {
            step_index: 0,
            generation: self.engine.generation,
            checkpoint: None,
        })
    }

    /// Return all matches without requiring uniqueness.
    pub fn discover(&self, selector: &Selector) -> Vec<SelectorMatch> {
        resolve_selector(&self.engine.committed.semantics, selector)
            .into_iter()
            .map(SelectorMatch::from)
            .collect()
    }

    /// Return the latest coherent committed generation.
    pub fn snapshot(&self) -> &AutomationSnapshot {
        &self.engine.committed
    }

    /// Clock behavior selected when this session was mounted.
    pub fn clock_mode(&self) -> ClockMode {
        self.engine.clock_mode
    }

    /// Elapsed logical application time.
    pub fn logical_elapsed(&self) -> Duration {
        self.engine.backend.core.ctx.env().clock.elapsed()
    }

    /// Run configured checkpoint sinks for the current generation.
    pub fn checkpoint(&mut self, name: &str) -> Result<Checkpoint, AutomationError> {
        match self.execute_operation(&AutomationStep::checkpoint(name))? {
            Some(checkpoint) => Ok(checkpoint),
            None => unreachable!("checkpoint operations always return checkpoint evidence"),
        }
    }

    /// Shut down and unmount exactly once.
    pub fn shutdown(&mut self) {
        self.engine.shutdown();
    }
}

impl<C> AutomationSession<C>
where
    C: Component,
    C::Properties: Default,
{
    /// Mount a component with default app and properties.
    pub fn new(component: C, options: AutomationOptions) -> Result<Self, AutomationError> {
        Self::with_app(App::new(), component, C::Properties::default(), options)
    }
}

pub(crate) fn evaluate_wait_condition(
    condition: &WaitCondition,
    tree: &SemanticTree,
) -> (bool, Vec<SelectorMatch>) {
    let selector = match condition {
        WaitCondition::Exists(selector)
        | WaitCondition::Missing(selector)
        | WaitCondition::InView(selector)
        | WaitCondition::Focused(selector) => selector,
        WaitCondition::Enabled { selector, .. }
        | WaitCondition::Selected { selector, .. }
        | WaitCondition::ValueEquals { selector, .. }
        | WaitCondition::TextContains { selector, .. }
        | WaitCondition::Count { selector, .. } => selector,
    };
    let nodes = resolve_selector(tree, selector);
    let met = match condition {
        WaitCondition::Exists(_) => !nodes.is_empty(),
        WaitCondition::Missing(_) => nodes.is_empty(),
        WaitCondition::InView(_) => nodes.len() == 1 && nodes[0].in_view,
        WaitCondition::Focused(_) => nodes.len() == 1 && nodes[0].focused,
        WaitCondition::Enabled { enabled, .. } => nodes.len() == 1 && nodes[0].enabled == *enabled,
        WaitCondition::Selected { selected, .. } => {
            nodes.len() == 1 && nodes[0].selected == Some(*selected)
        }
        WaitCondition::ValueEquals { value, .. } => {
            nodes.len() == 1
                && nodes[0]
                    .value
                    .as_ref()
                    .and_then(|value| value.text.as_deref())
                    == Some(value.as_ref())
        }
        WaitCondition::TextContains { text, .. } => nodes
            .iter()
            .any(|node| node.searchable_text().contains(text.as_ref())),
        WaitCondition::Count { count, .. } => nodes.len() == *count,
    };
    (met, nodes.into_iter().map(SelectorMatch::from).collect())
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use crate::automation::SemanticRole;
    use crate::core::component::{Command, Context, Update};
    use crate::core::element::{Element, IntoElement};
    use crate::widgets::{Button, Input, List, ListItem, Tab, Table, TableRow, Tabs, Text, VStack};

    use super::*;

    struct Counter {
        unmounts: Rc<Cell<usize>>,
    }

    #[derive(Clone, Copy)]
    enum CounterMsg {
        Increment,
    }

    impl Component for Counter {
        type Message = CounterMsg;
        type Properties = ();
        type State = usize;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                CounterMsg::Increment => ctx.state += 1,
            }
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(
                    Button::new("Increment")
                        .on_click(ctx.link().callback(|_| CounterMsg::Increment))
                        .automation_id("increment"),
                )
                .child(Button::new("Other").automation_id("other"))
                .child(Text::new(format!("count={}", ctx.state)).automation_id("count"))
                .into()
        }

        fn unmount(&mut self, _ctx: &mut Context<Self>) {
            self.unmounts.set(self.unmounts.get() + 1);
        }
    }

    #[test]
    fn click_commits_state_and_shutdown_unmounts_once() {
        let unmounts = Rc::new(Cell::new(0));
        let mut session = AutomationSession::new(
            Counter {
                unmounts: Rc::clone(&unmounts),
            },
            AutomationOptions::default(),
        )
        .unwrap();

        let result = session
            .execute(AutomationStep::click(Selector::id("increment")))
            .unwrap();
        assert!(result.generation > 1);
        assert_eq!(
            session.discover(&Selector::text_contains("count=1")).len(),
            1
        );

        session.shutdown();
        session.shutdown();
        drop(session);
        assert_eq!(unmounts.get(), 1);
    }

    #[test]
    fn focus_only_operation_commits_a_new_generation() {
        let mut session = AutomationSession::new(
            Counter {
                unmounts: Rc::new(Cell::new(0)),
            },
            AutomationOptions::default(),
        )
        .unwrap();
        let before = session.snapshot().generation;

        let result = session
            .execute(AutomationStep::focus(Selector::id("increment")))
            .unwrap();

        assert!(result.generation > before);
        assert!(
            session
                .snapshot()
                .semantics
                .by_id(&crate::automation::AutomationId::try_new("increment").unwrap())
                .first()
                .is_some_and(|node| node.focused)
        );
    }

    struct DisabledButton;

    impl Component for DisabledButton {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Button::new("Disabled")
                .disabled(true)
                .automation_id("disabled")
        }
    }

    #[test]
    fn disabled_semantic_target_is_not_actionable() {
        let mut session =
            AutomationSession::new(DisabledButton, AutomationOptions::default()).unwrap();

        let error = session
            .execute(AutomationStep::click(Selector::id("disabled")))
            .expect_err("disabled buttons must reject click operations");

        assert!(matches!(error, AutomationError::NotActionable));
    }

    struct Timer;

    #[derive(Clone, Copy)]
    enum TimerMsg {
        Fired,
    }

    impl Component for Timer {
        type Message = TimerMsg;
        type Properties = ();
        type State = bool;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            false
        }

        fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
            let _ = ctx;
            Some(Command::after(Duration::from_millis(50), |link| {
                link.send(TimerMsg::Fired);
            }))
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state = true;
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(if ctx.state { "fired" } else { "waiting" }).into()
        }
    }

    #[test]
    fn controlled_timer_only_fires_after_explicit_advance() {
        let mut session = AutomationSession::new(Timer, AutomationOptions::default()).unwrap();
        std::thread::sleep(Duration::from_millis(60));
        session.drain_ready().unwrap();
        assert!(
            session
                .discover(&Selector::text_contains("fired"))
                .is_empty()
        );
        assert_eq!(session.drain_ready().unwrap().future_timers, 1);

        session
            .execute(AutomationStep::advance(Duration::from_millis(50)))
            .unwrap();
        assert_eq!(session.discover(&Selector::text_contains("fired")).len(), 1);
    }

    struct CommandLinkTimer;

    impl Component for CommandLinkTimer {
        type Message = TimerMsg;
        type Properties = ();
        type State = bool;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            false
        }

        fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
            Some(ctx.link().command(|link| {
                link.send_after(Duration::from_millis(50), TimerMsg::Fired);
            }))
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state = true;
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(if ctx.state { "fired" } else { "waiting" }).into()
        }
    }

    #[test]
    fn command_link_delays_use_the_controlled_clock() {
        let mut session =
            AutomationSession::new(CommandLinkTimer, AutomationOptions::default()).unwrap();
        session
            .wait_for_idle(Duration::from_secs(1), Duration::from_millis(5))
            .unwrap();
        std::thread::sleep(Duration::from_millis(60));
        session.drain_ready().unwrap();
        assert!(
            session
                .discover(&Selector::text_contains("fired"))
                .is_empty()
        );

        session
            .execute(AutomationStep::advance(Duration::from_millis(50)))
            .unwrap();
        assert_eq!(session.discover(&Selector::text_contains("fired")).len(), 1);
    }

    #[derive(Clone, Copy)]
    enum TimerChainMsg {
        First,
        Second,
    }

    struct TimerChain;

    impl Component for TimerChain {
        type Message = TimerChainMsg;
        type Properties = ();
        type State = u8;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            0
        }

        fn init(&mut self, _ctx: &mut Context<Self>) -> Option<Command> {
            Some(Command::after(Duration::ZERO, |link| {
                link.send(TimerChainMsg::First);
            }))
        }

        fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            match msg {
                TimerChainMsg::First => {
                    ctx.state = 1;
                    Update::with_command(Command::after(Duration::ZERO, |link| {
                        link.send(TimerChainMsg::Second);
                    }))
                }
                TimerChainMsg::Second => {
                    ctx.state = 2;
                    Update::full()
                }
            }
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(if ctx.state == 2 { "done" } else { "pending" }).into()
        }
    }

    #[test]
    fn one_drain_reaches_timers_scheduled_by_messages() {
        let mut session = AutomationSession::new(TimerChain, AutomationOptions::default()).unwrap();
        session.drain_ready().unwrap();
        assert!(
            session
                .snapshot()
                .semantics
                .nodes()
                .any(|node| node.searchable_text().contains("done"))
        );
    }

    struct NonConvergingTimer;

    impl Component for NonConvergingTimer {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn init(&mut self, _ctx: &mut Context<Self>) -> Option<Command> {
            Some(Command::after(Duration::ZERO, |link| link.send(())))
        }

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::with_command(Command::after(Duration::ZERO, |link| link.send(())))
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Text::new("loop").into()
        }
    }

    #[test]
    fn non_converging_ready_work_reports_the_bound() {
        let mut session =
            AutomationSession::new(NonConvergingTimer, AutomationOptions::default()).unwrap();
        let error = session.drain_ready().unwrap_err();
        assert!(matches!(
            error,
            AutomationError::DrainDidNotConverge { rounds: 1024, .. }
        ));
    }

    struct External;

    #[derive(Clone, Copy)]
    enum ExternalMsg {
        Done,
    }

    impl Component for External {
        type Message = ExternalMsg;
        type Properties = ();
        type State = bool;

        fn create_state(&self, _props: &Self::Properties) -> Self::State {
            false
        }

        fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
            Some(ctx.link().command(|link| {
                std::thread::sleep(Duration::from_millis(10));
                link.send(ExternalMsg::Done);
            }))
        }

        fn update(&mut self, _msg: Self::Message, ctx: &mut Context<Self>) -> Update {
            ctx.state = true;
            Update::full()
        }

        fn view(&self, ctx: &Context<Self>) -> Element {
            Text::new(if ctx.state { "done" } else { "pending" }).into()
        }
    }

    #[test]
    fn controlled_wait_wakes_for_external_command_without_advancing_time() {
        let mut session = AutomationSession::new(External, AutomationOptions::default()).unwrap();
        let before = session.engine.backend.core.ctx.env().clock.elapsed();
        session
            .wait_for(
                WaitCondition::exists(Selector::text_contains("done")),
                Duration::from_secs(1),
            )
            .unwrap();
        let after = session.engine.backend.core.ctx.env().clock.elapsed();
        assert_eq!(before, after);
    }

    struct DuplicateIds;

    impl Component for DuplicateIds {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(Button::new("A").automation_id("same"))
                .child(Button::new("B").automation_id("same"))
                .into()
        }
    }

    #[test]
    fn duplicate_ids_fail_in_all_builds() {
        let error = match AutomationSession::new(DuplicateIds, AutomationOptions::default()) {
            Ok(_) => panic!("duplicate IDs must reject the session"),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            AutomationError::DuplicateAutomationId { .. }
        ));
    }

    #[test]
    fn ambiguous_selectors_do_not_choose_the_first_match() {
        let unmounts = Rc::new(Cell::new(0));
        let mut session =
            AutomationSession::new(Counter { unmounts }, AutomationOptions::default()).unwrap();
        let error = session
            .execute(AutomationStep::click(Selector::role(
                super::super::SemanticRole::Button,
            )))
            .unwrap_err();
        assert!(matches!(error, AutomationError::AmbiguousMatch { .. }));
    }

    #[test]
    fn resize_preserves_session_and_changes_capture_viewport() {
        let unmounts = Rc::new(Cell::new(0));
        let mut session =
            AutomationSession::new(Counter { unmounts }, AutomationOptions::default()).unwrap();
        session.execute(AutomationStep::resize(120, 40)).unwrap();
        assert_eq!(session.snapshot().viewport.w, 120);
        assert_eq!(session.snapshot().viewport.h, 40);
        assert_eq!(session.snapshot().frame.width, 120);
        assert_eq!(session.snapshot().frame.height, 40);
    }

    struct Sensitive;

    impl Component for Sensitive {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            Input::new("super-secret")
                .automation_id("password")
                .sensitive_value(true)
        }
    }

    #[test]
    fn sensitive_values_are_redacted_from_semantics_and_markdown() {
        let directory = std::env::temp_dir().join(format!(
            "tui-lipan-sensitive-checkpoint-{}",
            std::process::id()
        ));
        let mut session = AutomationSession::new(
            Sensitive,
            AutomationOptions::default().checkpoint_sink(CheckpointSink::Markdown {
                directory: directory.clone(),
            }),
        )
        .unwrap();

        let value = session
            .snapshot()
            .semantics
            .by_id(&super::super::AutomationId::from("password"))[0]
            .value
            .as_ref()
            .unwrap();
        assert_eq!(value.text, None);
        assert_eq!(value.sensitivity, super::super::ValueSensitivity::Sensitive);

        let checkpoint = session.checkpoint("redacted").unwrap();
        let markdown =
            std::fs::read_to_string(checkpoint.artifacts[0].path.as_ref().unwrap()).unwrap();
        assert!(!markdown.contains("super-secret"));
        assert!(markdown.contains("redacted:Sensitive"));
        let _ = std::fs::remove_dir_all(directory);
    }

    struct CollectionRoles;

    impl Component for CollectionRoles {
        type Message = ();
        type Properties = ();
        type State = ();

        fn create_state(&self, _props: &Self::Properties) -> Self::State {}

        fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
            Update::none()
        }

        fn view(&self, _ctx: &Context<Self>) -> Element {
            VStack::new()
                .child(List::new().items([ListItem::new("Alpha")]))
                .child(Table::new().rows([TableRow::new(["A", "B"])]))
                .child(Tabs::new().tabs([Tab::new("Overview")]))
                .into()
        }
    }

    #[test]
    fn collections_project_item_row_cell_and_tab_roles() {
        let mut session =
            AutomationSession::new(CollectionRoles, AutomationOptions::default()).unwrap();
        for role in [
            SemanticRole::ListItem,
            SemanticRole::Row,
            SemanticRole::Cell,
            SemanticRole::Tab,
        ] {
            assert!(
                !session.discover(&Selector::role(role)).is_empty(),
                "missing {role:?}"
            );
        }
        let error = session
            .execute(AutomationStep::click(
                Selector::role(SemanticRole::Cell).name("A"),
            ))
            .unwrap_err();
        assert!(matches!(error, AutomationError::NotActionable));
    }

    #[cfg(not(feature = "ui-snapshot-png"))]
    #[test]
    fn unsupported_png_sink_creates_no_directory() {
        let directory = std::env::temp_dir().join(format!(
            "tui-lipan-unsupported-checkpoint-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&directory);
        let result = AutomationSession::new(
            Sensitive,
            AutomationOptions::default().checkpoint_sink(CheckpointSink::Png {
                directory: directory.clone(),
            }),
        );
        assert!(matches!(
            result,
            Err(AutomationError::UnsupportedFormat("PNG"))
        ));
        assert!(!directory.exists());
    }
}
