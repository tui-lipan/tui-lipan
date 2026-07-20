use rustc_hash::FxHashMap;
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use crate::app::context::SurfaceMode;
use crate::app::input::command_registry::CommandRegistry;
use crate::callback::{CancellationToken, CommandLink, Dispatcher, ScopeId};
use crate::clipboard::ClipboardConfig;
use crate::core::component::{
    Command, CommandRuntime, Component, Context, FocusContext, HoverContext, ScrollContext,
    TaskPolicy, Update,
};
use crate::core::element::{Element, ElementKind};
use crate::core::nested::{ComponentRegistry, ComponentRegistryConfig, HostState};
use crate::core::runtime_env::{
    RuntimeEnv, ScrollDependency, ScrollDependencyKind, ScrollIdentity,
};
use crate::overlay::OverlayManager;
use crate::style::{Rect, Theme};
use crate::widgets::{Frame, Splitter, Text, VStack};

#[cfg(feature = "ui-snapshot-png")]
use crate::test_backend::TestBackend;

fn new_registry() -> ComponentRegistry {
    let dispatcher = Dispatcher::new(|_, _| {});
    let (command_tx, _command_rx) = mpsc::channel();
    let quit = Rc::new(Cell::new(false));
    let overlay_manager = Rc::new(RefCell::new(OverlayManager::new()));

    ComponentRegistry::new(ComponentRegistryConfig {
        dispatcher,
        command_tx,
        env: RuntimeEnv {
            command_registry: CommandRegistry::default(),
            quit,
            focus: Rc::new(FocusContext::default()),
            hover: Rc::new(HoverContext::default()),
            scroll: Rc::new(ScrollContext::default()),
            animations: Rc::new(crate::animation::AnimationRegistry::default()),
            overlay_manager,
            focus_request: Rc::new(RefCell::new(None)),
            mouse_capture: Rc::new(Cell::new(true)),
            surface_mode: SurfaceMode::Fullscreen,
            transcript_history: Rc::new(RefCell::new(Vec::new())),
            pending_transcript_entries: Rc::new(RefCell::new(VecDeque::new())),
            clipboard: crate::clipboard::test_clipboard(),
            clipboard_config: ClipboardConfig::default(),
            active_theme: Rc::new(RefCell::new(Theme::default())),
            active_theme_generation: Rc::new(Cell::new(1)),
            effect_phase: Rc::new(Cell::new(0)),
            contexts: Rc::new(RefCell::new(FxHashMap::default())),
            context_generations: Rc::new(RefCell::new(FxHashMap::default())),
            host_terminal_colors: Rc::new(Cell::new(None)),
            host_terminal_color_generation: Rc::new(Cell::new(0)),
            host_terminal_color_refresh_requested: Rc::new(Cell::new(false)),
            host_terminal_color_refresh_enabled: false,
            mouse_capture_generation: Rc::new(Cell::new(1)),
            memo_dependency_recorder: Rc::new(RefCell::new(None)),
            full_repaint: Rc::new(Cell::new(false)),
            devtools_request: Rc::new(RefCell::new(None)),
            ui_snapshot_request: Rc::new(RefCell::new(None)),
            command_chord_pending: Rc::new(Cell::new(false)),
        },
    })
}

fn find_first_component_scope(element: &Element) -> Option<ScopeId> {
    match &element.kind {
        ElementKind::Group(group) => Some(group.scope),
        _ => {
            for child in element.kind.children() {
                if let Some(scope) = find_first_component_scope(child) {
                    return Some(scope);
                }
            }
            None
        }
    }
}

struct Counter;

enum CounterMsg {
    Inc,
}

#[cfg(feature = "ui-snapshot-png")]
struct SnapshotRequester;

#[cfg(feature = "ui-snapshot-png")]
enum SnapshotRequestMsg {
    Write(&'static str),
}

#[cfg(feature = "ui-snapshot-png")]
impl Component for SnapshotRequester {
    type Message = SnapshotRequestMsg;
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Text::new("snapshot requester").into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            SnapshotRequestMsg::Write(path) => ctx.request_ui_snapshot_to(path),
        }
        Update::none()
    }
}

impl Component for Counter {
    type Message = CounterMsg;
    type Properties = u32;
    type State = u32;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        0
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Text::new(format!("{}:{}", ctx.props, ctx.state)).into()
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            CounterMsg::Inc => {
                ctx.state += 1;
                Update::full()
            }
        }
    }
}

#[test]
fn update_constructors_set_expected_levels() {
    let none = Update::none();
    assert!(!none.dirty);
    assert_eq!(none.level(), super::UpdateLevel::None);

    let paint = Update::paint();
    assert!(paint.dirty);
    assert_eq!(paint.level(), super::UpdateLevel::Paint);

    let layout = Update::layout();
    assert!(layout.dirty);
    assert_eq!(layout.level(), super::UpdateLevel::Layout);

    let full = Update::full();
    assert!(full.dirty);
    assert_eq!(full.level(), super::UpdateLevel::Full);

    let full2 = Update::full();
    assert!(full2.dirty);
    assert_eq!(full2.level(), super::UpdateLevel::Full);
}

#[test]
fn scroll_view_dependency_ignores_another_scope_with_the_same_key() {
    let scroll = ScrollContext::default();
    let observed = ScrollDependency {
        identity: ScrollIdentity {
            scope: ScopeId(7),
            key: "editor".into(),
        },
        kind: ScrollDependencyKind::Metrics,
    };
    let unrelated = ScrollIdentity {
        scope: ScopeId(8),
        key: "editor".into(),
    };
    scroll.mark_view_dependency(&observed);
    scroll
        .metrics_generations
        .borrow_mut()
        .insert(observed.identity.clone(), 1);
    scroll
        .metrics_generations
        .borrow_mut()
        .insert(unrelated.clone(), 1);
    let snapshot = scroll.view_generations();

    scroll.metrics_generations.borrow_mut().insert(unrelated, 2);
    assert!(!scroll.view_dependencies_stale(&snapshot));

    scroll
        .metrics_generations
        .borrow_mut()
        .insert(observed.identity, 2);
    assert!(scroll.view_dependencies_stale(&snapshot));
}

#[cfg(feature = "ui-snapshot-png")]
#[test]
fn request_ui_snapshot_to_routes_png_extension_when_feature_enabled() {
    let mut backend = TestBackend::new(SnapshotRequester);

    backend
        .dispatch(SnapshotRequestMsg::Write("/tmp/ui-snapshot.PNG"))
        .expect("dispatch should succeed");

    let request = backend
        .core
        .ctx
        .take_ui_snapshot_request()
        .expect("snapshot request");
    match request {
        crate::ui_snapshot::UiSnapshotRequest::Write { path, format } => {
            assert_eq!(path.as_path(), std::path::Path::new("/tmp/ui-snapshot.PNG"));
            assert_eq!(format, crate::ui_snapshot::UiSnapshotFileFormat::Png);
        }
        crate::ui_snapshot::UiSnapshotRequest::Deliver(_) => panic!("expected write request"),
    }
}

#[test]
fn with_command_defaults_to_full_level() {
    let update = Update::with_command(Command::new(|| {}));
    assert!(update.dirty);
    assert_eq!(update.level(), super::UpdateLevel::Full);
    assert!(update.command.is_some());
}

#[test]
fn layout_with_command_preserves_layout_level() {
    let update = Update::layout_with_command(Command::new(|| {}));
    assert!(update.dirty);
    assert_eq!(update.level(), super::UpdateLevel::Layout);
    assert!(update.command.is_some());

    let update = Update::layout_with_command(Option::<Command>::None);
    assert!(update.dirty);
    assert_eq!(update.level(), super::UpdateLevel::Layout);
    assert!(update.command.is_none());
}

#[test]
fn with_command_optional_and_command_only() {
    assert_eq!(
        Update::with_command(Option::<Command>::None).level(),
        super::UpdateLevel::Full
    );
    let cmd = Command::new(|| {});
    let u = Update::with_command(cmd);
    assert_eq!(u.level(), super::UpdateLevel::Full);
    assert!(u.dirty);
    assert!(u.command.is_some());

    let cmd2 = Command::new(|| {});
    let u2 = Update::command_only(cmd2);
    assert_eq!(u2.level(), super::UpdateLevel::None);
    assert!(!u2.dirty);
    assert!(u2.command.is_some());
}

#[test]
fn command_message_to_unmounted_component_is_dropped() {
    let mut registry = new_registry();
    let mut host = HostState::default();

    let epoch1 = registry.begin_epoch();
    let root1 = VStack::new()
        .child(crate::child::<Counter, _>(|| Counter, 1).key("c"))
        .into();
    let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
    registry.sweep(epoch1);

    let scope_c =
        find_first_component_scope(&expanded1).expect("expected a mounted component with key 'c'");

    // Unmount the component by rendering an empty tree.
    let epoch2 = registry.begin_epoch();
    let root2 = VStack::new().into();
    registry.expand_in_host(&mut host, None, root2, epoch2, Rect::default());
    registry.sweep(epoch2);

    // After unmount, messages to the old scope should be dropped without panic.
    let update = registry
        .update_by_scope(scope_c, Box::new(CounterMsg::Inc))
        .expect("update should not panic");
    assert_eq!(update.level(), super::UpdateLevel::None);
    assert!(!update.dirty);
}

#[test]
fn splitter_expands_components_in_panes_before_layout() {
    let mut registry = new_registry();
    let mut host = HostState::default();

    let epoch = registry.begin_epoch();
    let root = Splitter::vertical()
        .child(Frame::new().child(crate::child::<Counter, _>(|| Counter, 7).key("c")))
        .child(Text::new("patches"))
        .into();
    let expanded = registry.expand_in_host(&mut host, None, root, epoch, Rect::default());
    registry.sweep(epoch);

    assert!(!expanded.contains_unexpanded_component());
    assert!(find_first_component_scope(&expanded).is_some());
}

struct ReleaseGuard {
    tx: Option<mpsc::Sender<()>>,
}

impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        if let Some(tx) = self.tx.take() {
            let _ = tx.send(());
        }
    }
}

struct KeyedCmdComponent {
    key: Arc<str>,
    started_tx: mpsc::Sender<()>,
    release_rx: Rc<RefCell<Option<mpsc::Receiver<()>>>>,
    ran_tx: mpsc::Sender<&'static str>,
    call_count: u32,
}

enum KeyedCmdMsg {
    Trigger,
}

impl Component for KeyedCmdComponent {
    type Message = KeyedCmdMsg;
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Text::new("keyed").into()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        self.call_count += 1;
        let key = Arc::clone(&self.key);
        let ran_tx = self.ran_tx.clone();

        if self.call_count == 1 {
            let started_tx = self.started_tx.clone();
            let release_rx = self.release_rx.borrow_mut().take().unwrap();
            Update::with_command(Command::spawn_keyed(
                key,
                TaskPolicy::LatestOnly,
                move |_link: CommandLink<()>| {
                    let _ = started_tx.send(());
                    let _ = release_rx.recv();
                    let _ = ran_tx.send("A");
                },
            ))
        } else if self.call_count == 2 {
            Update::with_command(Command::spawn_keyed(
                key,
                TaskPolicy::LatestOnly,
                move |_link: CommandLink<()>| {
                    let _ = ran_tx.send("B");
                },
            ))
        } else {
            Update::with_command(Command::spawn_keyed(
                key,
                TaskPolicy::LatestOnly,
                move |_link: CommandLink<()>| {
                    let _ = ran_tx.send("C");
                },
            ))
        }
    }
}

#[test]
fn keyed_command_latest_only_coalesces_pending_tasks() {
    // Use a unique key to avoid collision with other tests using the global executor.
    let key: Arc<str> = Arc::from(format!(
        "test-coalesce-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));

    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (ran_tx, ran_rx) = mpsc::channel();

    let _guard = ReleaseGuard {
        tx: Some(release_tx),
    };

    let release_rx = Rc::new(RefCell::new(Some(release_rx)));

    let mut registry = new_registry();
    let mut host = HostState::default();

    let epoch1 = registry.begin_epoch();
    let root1 = VStack::new()
        .child(crate::child(
            move || KeyedCmdComponent {
                key: Arc::clone(&key),
                started_tx: started_tx.clone(),
                release_rx: Rc::clone(&release_rx),
                ran_tx: ran_tx.clone(),
                call_count: 0,
            },
            (),
        ))
        .into();
    let expanded1 = registry.expand_in_host(&mut host, None, root1, epoch1, Rect::default());
    registry.sweep(epoch1);

    let scope = find_first_component_scope(&expanded1).expect("expected a mounted keyed component");

    // First update: blocking task starts.
    let update1 = registry
        .update_by_scope(scope, Box::new(KeyedCmdMsg::Trigger))
        .expect("first update should succeed");
    let cmd1 = update1.command.expect("first update should have command");

    let (dummy_cmd_tx, _dummy_cmd_rx) = mpsc::channel();
    cmd1.run(CommandRuntime {
        scope,
        tx: dummy_cmd_tx.clone(),
    });

    started_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("first task should start");

    // Second update: task B submitted while A is active.
    let update2 = registry
        .update_by_scope(scope, Box::new(KeyedCmdMsg::Trigger))
        .expect("second update should succeed");
    let cmd2 = update2.command.expect("second update should have command");
    cmd2.run(CommandRuntime {
        scope,
        tx: dummy_cmd_tx.clone(),
    });

    // Third update: task C replaces pending B.
    let update3 = registry
        .update_by_scope(scope, Box::new(KeyedCmdMsg::Trigger))
        .expect("third update should succeed");
    let cmd3 = update3.command.expect("third update should have command");
    cmd3.run(CommandRuntime {
        scope,
        tx: dummy_cmd_tx,
    });

    // Release the active task.
    drop(_guard);

    // A ran, then C ran; B was replaced.
    assert_eq!(
        ran_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("A should run"),
        "A"
    );
    assert_eq!(
        ran_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("C should run"),
        "C"
    );
    assert!(
        matches!(
            ran_rx.recv_timeout(Duration::from_millis(200)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "intermediate task B should be replaced"
    );
}

#[test]
fn command_link_send_if_not_cancelled_suppresses_messages() {
    let (tx, rx) = mpsc::channel();
    let token = CancellationToken::default();
    let link = CommandLink::new(ScopeId(7), tx, token.clone());

    token.cancel();

    assert!(!link.send_if_not_cancelled(()));
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(50)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
}

/// `Command::after` must not occupy an executor worker while it waits. The executor runs 2-8
/// workers, so if delays were served by sleeping tasks, a couple of recurring timers would park
/// the pool and stall everything behind them. Saturating the pool with long timers and then
/// requiring prompt work to run proves the waiting happens elsewhere.
#[test]
fn after_does_not_occupy_executor_workers_while_waiting() {
    let (cmd_tx, _cmd_rx) = mpsc::channel();
    let runtime = || CommandRuntime {
        scope: ScopeId(1),
        tx: cmd_tx.clone(),
    };

    // Far more long timers than the pool has workers (worker count is capped at 8).
    for _ in 0..64 {
        Command::after(Duration::from_secs(30), |_link: CommandLink<()>| {}).run(runtime());
    }

    // Ordinary background work must still run promptly.
    let (ran_tx, ran_rx) = mpsc::channel();
    Command::spawn(move |_link: CommandLink<()>| {
        let _ = ran_tx.send(());
    })
    .run(runtime());

    ran_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("pending timers must not block the executor pool");
}

#[test]
fn after_runs_the_task_once_the_delay_elapses() {
    let (cmd_tx, _cmd_rx) = mpsc::channel();
    let (ran_tx, ran_rx) = mpsc::channel();

    let start = std::time::Instant::now();
    Command::after(Duration::from_millis(60), move |_link: CommandLink<()>| {
        let _ = ran_tx.send(());
    })
    .run(CommandRuntime {
        scope: ScopeId(1),
        tx: cmd_tx,
    });

    ran_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("delayed task must run");
    assert!(
        start.elapsed() >= Duration::from_millis(50),
        "task ran before its delay elapsed"
    );
}

/// Timers must fire in due order even when queued out of order, or a short debounce submitted
/// after a long tick would be held behind it.
#[test]
fn after_fires_in_due_order_regardless_of_submission_order() {
    let (cmd_tx, _cmd_rx) = mpsc::channel();
    let runtime = || CommandRuntime {
        scope: ScopeId(1),
        tx: cmd_tx.clone(),
    };
    let (order_tx, order_rx) = mpsc::channel();

    let late = order_tx.clone();
    Command::after(Duration::from_millis(220), move |_link: CommandLink<()>| {
        let _ = late.send("late");
    })
    .run(runtime());

    let early = order_tx.clone();
    Command::after(Duration::from_millis(40), move |_link: CommandLink<()>| {
        let _ = early.send("early");
    })
    .run(runtime());

    assert_eq!(
        order_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "early",
        "a shorter delay submitted second must still fire first"
    );
    assert_eq!(
        order_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        "late"
    );
}
