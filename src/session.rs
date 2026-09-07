use std::ops::{Deref, DerefMut};

use crate::core::component::Component;
use crate::core::element::Key;
use crate::core::node::NodeId;
use crate::runtime::RuntimeCore;
use crate::style::Rect;

pub(crate) trait OperationHost {
    fn duplicate_automation_id(&self) -> Option<crate::automation::AutomationId> {
        None
    }
    fn cancelled(&self) -> bool {
        false
    }
    fn wall_deadline(&self) -> Option<std::time::Instant> {
        None
    }
    fn semantics(&self) -> crate::automation::SemanticTree;
    fn current_pointer(&self) -> Option<(u16, u16)>;
    fn clock_mode(&self) -> crate::automation::ClockMode;
    fn send_key(&mut self, key: crate::core::event::KeyEvent) -> crate::Result<()>;
    fn send_mouse(&mut self, event: crate::core::event::MouseEvent) -> crate::Result<()>;
    fn focus_node(&mut self, node: NodeId) -> crate::Result<bool>;
    fn focus_step(&mut self, direction: crate::automation::FocusDirection) -> crate::Result<()>;
    fn resize(&mut self, width: u16, height: u16)
    -> Result<(), crate::automation::AutomationError>;
    fn advance(
        &mut self,
        duration: std::time::Duration,
    ) -> Result<(), crate::automation::AutomationError>;
    fn sleep(
        &mut self,
        duration: std::time::Duration,
    ) -> Result<(), crate::automation::AutomationError>;
    fn drain_round(&mut self) -> Result<bool, crate::automation::AutomationError>;
    fn idle_report(&self, dirty: bool) -> crate::automation::IdleReport;
    fn logical_elapsed(&self) -> std::time::Duration;
    fn wait_for_activity(&mut self, timeout: std::time::Duration);
    fn commit_after_drain(&mut self) -> Result<(), crate::automation::AutomationError>;
    fn checkpoint(
        &mut self,
        name: &str,
    ) -> Result<crate::automation::Checkpoint, crate::automation::AutomationError>;
}

pub(crate) fn execute_operation(
    host: &mut impl OperationHost,
    kind: &crate::automation::AutomationStepKind,
) -> Result<Option<crate::automation::Checkpoint>, crate::automation::AutomationError> {
    use crate::automation::AutomationStepKind;

    ensure_operation_active(host)?;
    match kind {
        AutomationStepKind::Key(_) | AutomationStepKind::Type(_) => execute_keyboard(host, kind)?,
        AutomationStepKind::Click { .. }
        | AutomationStepKind::Hover(_)
        | AutomationStepKind::Scroll { .. }
        | AutomationStepKind::Drag { .. } => execute_pointer(host, kind)?,
        AutomationStepKind::Focus(_) | AutomationStepKind::FocusStep(_) => {
            execute_focus(host, kind)?;
        }
        AutomationStepKind::Resize(_, _)
        | AutomationStepKind::Advance(_)
        | AutomationStepKind::Sleep(_)
        | AutomationStepKind::DrainReady
        | AutomationStepKind::WaitFor(_, _)
        | AutomationStepKind::Checkpoint(_) => return execute_session_operation(host, kind),
    };
    if !matches!(
        kind,
        AutomationStepKind::WaitFor(_, _) | AutomationStepKind::Checkpoint(_)
    ) {
        drain_to_fixed_point(host)?;
    }
    Ok(None)
}

fn execute_keyboard(
    host: &mut impl OperationHost,
    kind: &crate::automation::AutomationStepKind,
) -> Result<(), crate::automation::AutomationError> {
    use crate::automation::AutomationStepKind;
    use crate::core::event::{KeyCode, KeyEvent, KeyMods};

    match kind {
        AutomationStepKind::Key(key) => send_key_and_settle(host, *key)?,
        AutomationStepKind::Type(text) => {
            for ch in text.chars() {
                ensure_operation_active(host)?;
                send_key_and_settle(
                    host,
                    KeyEvent {
                        code: KeyCode::Char(ch),
                        mods: KeyMods::NONE,
                    },
                )?;
            }
        }
        _ => unreachable!("keyboard dispatcher received a non-keyboard operation"),
    }
    Ok(())
}

fn send_key_and_settle(
    host: &mut impl OperationHost,
    key: crate::core::event::KeyEvent,
) -> Result<(), crate::automation::AutomationError> {
    host.send_key(key)?;
    drain_to_fixed_point(host)?;
    Ok(())
}

fn execute_focus(
    host: &mut impl OperationHost,
    kind: &crate::automation::AutomationStepKind,
) -> Result<(), crate::automation::AutomationError> {
    use crate::automation::{AutomationError, AutomationStepKind, SemanticAction};

    match kind {
        AutomationStepKind::Focus(selector) => {
            let semantics = host.semantics();
            let semantic = one_match(&semantics, selector)?;
            let Some(node) = semantic.runtime_node else {
                return Err(AutomationError::NotActionable);
            };
            if !semantic.enabled
                || !semantic.actionable
                || !semantic.actions.contains(&SemanticAction::Focus)
                || !host.focus_node(node)?
            {
                return Err(AutomationError::NotActionable);
            }
        }
        AutomationStepKind::FocusStep(direction) => host.focus_step(*direction)?,
        _ => unreachable!("focus dispatcher received a non-focus operation"),
    }
    Ok(())
}

fn execute_pointer(
    host: &mut impl OperationHost,
    kind: &crate::automation::AutomationStepKind,
) -> Result<(), crate::automation::AutomationError> {
    use crate::automation::AutomationStepKind;

    match kind {
        AutomationStepKind::Click { selector, button } => click(host, selector, *button),
        AutomationStepKind::Hover(selector) => hover(host, selector),
        AutomationStepKind::Scroll {
            selector,
            direction,
        } => scroll(host, selector.as_ref(), *direction),
        AutomationStepKind::Drag { from, to } => drag(host, from, to),
        _ => unreachable!("pointer dispatcher received a non-pointer operation"),
    }
}

fn click(
    host: &mut impl OperationHost,
    selector: &crate::automation::Selector,
    button: crate::core::event::MouseButton,
) -> Result<(), crate::automation::AutomationError> {
    use crate::automation::SemanticAction;
    use crate::core::event::MouseKind;

    let point = point_for(
        host,
        selector,
        &[
            SemanticAction::Click,
            SemanticAction::Toggle,
            SemanticAction::SetValue,
            SemanticAction::Expand,
            SemanticAction::Collapse,
        ],
    )?;
    send_pointer_sequence(
        host,
        &[
            mouse_at(point, MouseKind::Moved),
            mouse_at(point, MouseKind::Down(button)),
            mouse_at(point, MouseKind::Up(button)),
        ],
    )
}

fn hover(
    host: &mut impl OperationHost,
    selector: &crate::automation::Selector,
) -> Result<(), crate::automation::AutomationError> {
    let point = point_for(host, selector, &[])?;
    send_pointer_sequence(
        host,
        &[mouse_at(point, crate::core::event::MouseKind::Moved)],
    )
}

fn scroll(
    host: &mut impl OperationHost,
    selector: Option<&crate::automation::Selector>,
    direction: crate::automation::AutomationScrollDirection,
) -> Result<(), crate::automation::AutomationError> {
    use crate::automation::{AutomationScrollDirection, SemanticAction};
    use crate::core::event::MouseKind;

    let point = match selector {
        Some(selector) => point_for(host, selector, &[SemanticAction::Scroll])?,
        None => host.current_pointer().unwrap_or((0, 0)),
    };
    let mut events = Vec::with_capacity(2);
    if selector.is_some() {
        events.push(mouse_at(point, MouseKind::Moved));
    }
    let kind = match direction {
        AutomationScrollDirection::Up => MouseKind::ScrollUp,
        AutomationScrollDirection::Down => MouseKind::ScrollDown,
    };
    events.push(mouse_at(point, kind));
    send_pointer_sequence(host, &events)
}

fn drag(
    host: &mut impl OperationHost,
    from: &crate::automation::Selector,
    to: &crate::automation::Selector,
) -> Result<(), crate::automation::AutomationError> {
    use crate::automation::SemanticAction;
    use crate::core::event::{MouseButton, MouseKind};

    let from = point_for(host, from, &[SemanticAction::Drag])?;
    let to = point_for(host, to, &[SemanticAction::Drag])?;
    send_pointer_sequence(
        host,
        &[
            mouse_at(from, MouseKind::Moved),
            mouse_at(from, MouseKind::Down(MouseButton::Left)),
            mouse_at(
                (from.0.midpoint(to.0), from.1.midpoint(to.1)),
                MouseKind::Drag(MouseButton::Left),
            ),
            mouse_at(to, MouseKind::Drag(MouseButton::Left)),
            mouse_at(to, MouseKind::Up(MouseButton::Left)),
        ],
    )
}

/// Dispatch one complete pointer gesture without observing cancellation between its events.
///
/// A request may be cancelled before a gesture starts or after it finishes, but never while a
/// button is held. Runtime failures still attempt a best-effort release before returning.
fn send_pointer_sequence(
    host: &mut impl OperationHost,
    events: &[crate::core::event::MouseEvent],
) -> Result<(), crate::automation::AutomationError> {
    use crate::core::event::MouseKind;

    ensure_operation_active(host)?;
    let mut pressed = None;
    for event in events {
        let next_pressed = match event.kind {
            MouseKind::Down(button) => Some(button),
            MouseKind::Up(_) => None,
            _ => pressed,
        };
        if let Err(error) = host
            .send_mouse(*event)
            .map_err(crate::automation::AutomationError::from)
            .and_then(|()| drain_to_fixed_point_uninterruptible(host).map(|_| ()))
        {
            if let Some(button) = pressed.or(next_pressed) {
                let point = (event.x, event.y);
                let _ = host.send_mouse(mouse_at(point, MouseKind::Up(button)));
                let _ = drain_to_fixed_point_uninterruptible(host);
            }
            return Err(error);
        }
        pressed = next_pressed;
    }
    Ok(())
}

fn execute_session_operation(
    host: &mut impl OperationHost,
    kind: &crate::automation::AutomationStepKind,
) -> Result<Option<crate::automation::Checkpoint>, crate::automation::AutomationError> {
    use crate::automation::{AutomationError, AutomationStepKind, ClockMode};

    let checkpoint = match kind {
        AutomationStepKind::Resize(width, height) => {
            host.resize(*width, *height)?;
            None
        }
        AutomationStepKind::Advance(duration) => {
            if host.clock_mode() != ClockMode::Controlled {
                return Err(AutomationError::RealtimeAdvance);
            }
            host.advance(*duration)?;
            None
        }
        AutomationStepKind::Sleep(duration) => {
            host.sleep(*duration)?;
            None
        }
        AutomationStepKind::DrainReady => None,
        AutomationStepKind::WaitFor(condition, timeout) => {
            wait_for(host, condition, *timeout)?;
            return Ok(None);
        }
        AutomationStepKind::Checkpoint(name) => {
            drain_to_fixed_point(host)?;
            return host.checkpoint(name).map(Some);
        }
        _ => unreachable!("session dispatcher received an input operation"),
    };
    drain_to_fixed_point(host)?;
    Ok(checkpoint)
}

pub(crate) fn drain_to_fixed_point(
    host: &mut impl OperationHost,
) -> Result<crate::automation::IdleReport, crate::automation::AutomationError> {
    drain_to_fixed_point_impl(host, true)
}

fn drain_to_fixed_point_uninterruptible(
    host: &mut impl OperationHost,
) -> Result<crate::automation::IdleReport, crate::automation::AutomationError> {
    drain_to_fixed_point_impl(host, false)
}

fn drain_to_fixed_point_impl(
    host: &mut impl OperationHost,
    interruptible: bool,
) -> Result<crate::automation::IdleReport, crate::automation::AutomationError> {
    const MAX_ROUNDS: usize = 1024;
    for _ in 0..MAX_ROUNDS {
        ensure_drain_allowed(host, interruptible)?;
        let dirty = host.drain_round()?;
        ensure_drain_allowed(host, interruptible)?;
        let report = host.idle_report(dirty);
        if report.due_timers == 0 && report.queued_messages == 0 && !dirty {
            host.commit_after_drain()?;
            return Ok(host.idle_report(false));
        }
    }
    host.commit_after_drain()?;
    Err(crate::automation::AutomationError::DrainDidNotConverge {
        rounds: MAX_ROUNDS,
        idle: host.idle_report(true),
    })
}

fn ensure_drain_allowed(
    host: &impl OperationHost,
    interruptible: bool,
) -> Result<(), crate::automation::AutomationError> {
    if let Some(id) = host.duplicate_automation_id() {
        return Err(crate::automation::AutomationError::DuplicateAutomationId { id });
    }
    if interruptible {
        ensure_operation_active(host)?;
    }
    Ok(())
}

fn ensure_operation_active(
    host: &impl OperationHost,
) -> Result<(), crate::automation::AutomationError> {
    if host.cancelled() {
        return Err(crate::automation::AutomationError::Cancelled);
    }
    if let Some(id) = host.duplicate_automation_id() {
        return Err(crate::automation::AutomationError::DuplicateAutomationId { id });
    }
    if host
        .wall_deadline()
        .is_some_and(|deadline| std::time::Instant::now() >= deadline)
    {
        return Err(crate::automation::AutomationError::DeadlineExceeded);
    }
    Ok(())
}

pub(crate) fn wait_for(
    host: &mut impl OperationHost,
    condition: &crate::automation::WaitCondition,
    timeout: std::time::Duration,
) -> Result<(), crate::automation::AutomationError> {
    let wall_start = std::time::Instant::now();
    let logical_start = host.logical_elapsed();
    let timeout_deadline = wall_start.checked_add(timeout).unwrap_or(wall_start);
    loop {
        drain_to_fixed_point(host)?;
        let semantics = host.semantics();
        let (met, matches) = crate::automation::evaluate_wait_condition(condition, &semantics);
        if met {
            return Ok(());
        }
        ensure_operation_active(host)?;
        let now = std::time::Instant::now();
        let deadline = host
            .wall_deadline()
            .map_or(timeout_deadline, |deadline| deadline.min(timeout_deadline));
        if now >= deadline {
            return Err(crate::automation::AutomationError::WaitTimeout {
                condition: Box::new(condition.clone()),
                wall_elapsed: now.saturating_duration_since(wall_start),
                logical_elapsed: host.logical_elapsed().saturating_sub(logical_start),
                last_matches: matches.into_boxed_slice(),
                idle: host.idle_report(false),
                semantic_tree: Box::new(semantics),
                diagnostic_checkpoint: None,
            });
        }
        host.wait_for_activity(
            deadline
                .saturating_duration_since(now)
                .min(std::time::Duration::from_millis(5)),
        );
    }
}

fn one_match<'a>(
    tree: &'a crate::automation::SemanticTree,
    selector: &crate::automation::Selector,
) -> Result<&'a crate::automation::SemanticNode, crate::automation::AutomationError> {
    let matches = crate::automation::resolve_selector(tree, selector);
    match matches.as_slice() {
        [] => Err(crate::automation::AutomationError::NoMatch {
            selector: selector.clone(),
        }),
        [node] => Ok(*node),
        many => Err(crate::automation::AutomationError::ambiguous(
            selector.clone(),
            many.iter()
                .map(|node| crate::automation::SelectorMatch::from(*node))
                .collect(),
        )),
    }
}

fn point_for(
    host: &impl OperationHost,
    selector: &crate::automation::Selector,
    accepted_actions: &[crate::automation::SemanticAction],
) -> Result<(u16, u16), crate::automation::AutomationError> {
    let semantics = host.semantics();
    let node = one_match(&semantics, selector)?;
    if !node.in_view {
        return Err(crate::automation::AutomationError::NotInView);
    }
    if !accepted_actions.is_empty() && (!node.enabled || !node.actionable) {
        return Err(crate::automation::AutomationError::NotActionable);
    }
    if !accepted_actions.is_empty()
        && !node
            .actions
            .iter()
            .any(|action| accepted_actions.contains(action))
    {
        return Err(crate::automation::AutomationError::NotActionable);
    }
    let x = i32::from(node.clipped_bounds.x) + i32::from(node.clipped_bounds.w / 2);
    let y = i32::from(node.clipped_bounds.y) + i32::from(node.clipped_bounds.h / 2);
    match (u16::try_from(x), u16::try_from(y)) {
        (Ok(x), Ok(y)) => Ok((x, y)),
        _ => Err(crate::automation::AutomationError::NotInView),
    }
}

fn mouse_at(
    point: (u16, u16),
    kind: crate::core::event::MouseKind,
) -> crate::core::event::MouseEvent {
    crate::core::event::MouseEvent {
        x: point.0,
        y: point.1,
        kind,
        mods: crate::core::event::KeyMods::NONE,
    }
}

/// Shared mounted-session owner beneath terminal, test, web, and automation frontends.
pub(crate) struct SessionEngine<C: Component> {
    core: RuntimeCore<C>,
    viewport: Rect,
    generation: u64,
    duplicate_automation_id: Option<crate::automation::AutomationId>,
}

impl<C: Component> SessionEngine<C> {
    pub(crate) fn new(core: RuntimeCore<C>) -> Self {
        let viewport = core.ctx.viewport();
        Self {
            core,
            viewport,
            generation: 0,
            duplicate_automation_id: None,
        }
    }

    pub(crate) fn viewport(&self) -> Rect {
        self.viewport
    }

    pub(crate) fn set_viewport(&mut self, viewport: Rect) {
        self.viewport = viewport;
        self.core.ctx.set_viewport(viewport);
    }

    pub(crate) fn commit(&mut self, viewport: Rect) -> u64 {
        self.viewport = viewport;
        self.duplicate_automation_id = self.core.tree.duplicate_automation_id();
        if self.duplicate_automation_id.is_none() {
            self.advance_generation();
        }
        self.generation
    }

    pub(crate) fn commit_checked(&mut self, viewport: Rect) -> crate::Result<u64> {
        self.viewport = viewport;
        self.duplicate_automation_id = self.core.tree.duplicate_automation_id();
        self.ensure_valid_commit()?;
        self.advance_generation();
        Ok(self.generation)
    }

    pub(crate) fn ensure_valid_commit(&self) -> crate::Result<()> {
        if let Some(id) = &self.duplicate_automation_id {
            return Err(crate::Error::DuplicateAutomationId { id: id.clone() });
        }
        Ok(())
    }

    fn advance_generation(&mut self) {
        self.generation = self.generation.wrapping_add(1).max(1);
    }

    pub(crate) fn duplicate_automation_id(&self) -> Option<&crate::automation::AutomationId> {
        self.duplicate_automation_id.as_ref()
    }

    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn semantic_tree(&self, focused: Option<NodeId>) -> crate::automation::SemanticTree {
        crate::automation::project_semantic_tree(
            &self.core.tree,
            self.viewport,
            focused,
            self.generation,
        )
    }

    pub(crate) fn render_element(
        &mut self,
        bounds: Rect,
        focused: Option<NodeId>,
        focused_key: Option<&Key>,
        hovered: Option<NodeId>,
    ) {
        self.core
            .render_element(bounds, focused, focused_key, hovered);
        self.commit(bounds);
    }
}

impl<C: Component> From<RuntimeCore<C>> for SessionEngine<C> {
    fn from(core: RuntimeCore<C>) -> Self {
        Self::new(core)
    }
}

impl<C: Component> Deref for SessionEngine<C> {
    type Target = RuntimeCore<C>;

    fn deref(&self) -> &Self::Target {
        &self.core
    }
}

impl<C: Component> DerefMut for SessionEngine<C> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.core
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::time::Duration;

    use crate::automation::{AutomationError, ClockMode, FocusDirection, IdleReport};
    use crate::core::event::{MouseButton, MouseEvent, MouseKind};

    use super::{NodeId, OperationHost};

    struct CancellingPointerHost {
        cancelled: Cell<bool>,
        events: RefCell<Vec<MouseKind>>,
    }

    impl OperationHost for CancellingPointerHost {
        fn cancelled(&self) -> bool {
            self.cancelled.get()
        }

        fn semantics(&self) -> crate::automation::SemanticTree {
            unreachable!("this gesture test does not resolve selectors")
        }

        fn current_pointer(&self) -> Option<(u16, u16)> {
            None
        }

        fn clock_mode(&self) -> ClockMode {
            ClockMode::Controlled
        }

        fn send_key(&mut self, _key: crate::core::event::KeyEvent) -> crate::Result<()> {
            unreachable!("this gesture test does not send keys")
        }

        fn send_mouse(&mut self, event: MouseEvent) -> crate::Result<()> {
            self.events.borrow_mut().push(event.kind);
            if matches!(event.kind, MouseKind::Down(_)) {
                self.cancelled.set(true);
            }
            Ok(())
        }

        fn focus_node(&mut self, _node: NodeId) -> crate::Result<bool> {
            unreachable!("this gesture test does not focus nodes")
        }

        fn focus_step(&mut self, _direction: FocusDirection) -> crate::Result<()> {
            unreachable!("this gesture test does not move focus")
        }

        fn resize(&mut self, _width: u16, _height: u16) -> Result<(), AutomationError> {
            unreachable!("this gesture test does not resize")
        }

        fn advance(&mut self, _duration: Duration) -> Result<(), AutomationError> {
            unreachable!("this gesture test does not advance time")
        }

        fn sleep(&mut self, _duration: Duration) -> Result<(), AutomationError> {
            unreachable!("this gesture test does not sleep")
        }

        fn drain_round(&mut self) -> Result<bool, AutomationError> {
            Ok(false)
        }

        fn idle_report(&self, dirty: bool) -> IdleReport {
            IdleReport {
                dirty,
                ..IdleReport::default()
            }
        }

        fn logical_elapsed(&self) -> Duration {
            Duration::ZERO
        }

        fn wait_for_activity(&mut self, _timeout: Duration) {}

        fn commit_after_drain(&mut self) -> Result<(), AutomationError> {
            Ok(())
        }

        fn checkpoint(
            &mut self,
            _name: &str,
        ) -> Result<crate::automation::Checkpoint, AutomationError> {
            unreachable!("this gesture test does not create checkpoints")
        }
    }

    #[test]
    fn cancellation_after_pointer_down_does_not_skip_release() {
        let mut host = CancellingPointerHost {
            cancelled: Cell::new(false),
            events: RefCell::new(Vec::new()),
        };
        let point = (2, 3);

        super::send_pointer_sequence(
            &mut host,
            &[
                super::mouse_at(point, MouseKind::Moved),
                super::mouse_at(point, MouseKind::Down(MouseButton::Left)),
                super::mouse_at(point, MouseKind::Up(MouseButton::Left)),
            ],
        )
        .unwrap();

        assert_eq!(
            *host.events.borrow(),
            vec![
                MouseKind::Moved,
                MouseKind::Down(MouseButton::Left),
                MouseKind::Up(MouseButton::Left),
            ]
        );
    }
}
