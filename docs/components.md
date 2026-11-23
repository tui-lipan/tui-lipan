# Components

## Component Trait

Every component implements the `Component` trait with three associated types:

```rust
impl Component for MyApp {
    type Message = Msg;       // Events this component handles
    type Properties = Props;  // Input from parent (often `()`)
    type State = State;       // Local mutable state
}
```

## Where state lives

Keep mutable UI state such as the current tab, a draft, or a loading flag in `ctx.state`.
Initialize it in `create_state`, read it in `view`, and change it from `update` or a lifecycle
hook. Keep injected dependencies and handles, such as a service client, on the component
instance (`self`). Use `type State = ()` when the component needs no local UI state.

## Lifecycle Methods

| Method | Required | Signature | Purpose |
|--------|----------|-----------|---------|
| `create_state` | Yes | `(&self, &Props) -> State` | Initialize state from properties |
| `memo_key` | No | `(&self, &Props, &Context<Self>) -> Option<u64>` | Opt into retained subtree reuse |
| `view` | Yes | `(&self, &Context<Self>) -> Element` | Return UI tree |
| `update` | Yes | `(&mut self, Msg, &mut Context<Self>) -> Update` | Handle messages |
| `init` | No | `(&mut self, &mut Context<Self>) -> Update` | One-time setup on mount |
| `on_paste` | No | `(&mut self, &str, PasteOrigin, &mut Context<Self>) -> KeyUpdate` | Handle pasted text declined by the focused widget |
| `on_key` | No | `(&mut self, KeyEvent, &mut Context<Self>) -> KeyUpdate` | Handle unhandled key events |
| `on_modifiers_changed` | No | `(&mut self, KeyMods, &mut Context<Self>) -> Update` | React to physically held modifier keys |
| `on_window_focus_changed` | No | `(&mut self, bool, &mut Context<Self>) -> Update` | React to host terminal/window focus transitions |
| `on_host_theme_changed` | No | `(&mut self, &mut Context<Self>) -> Update` | React to host terminal palette or light/dark scheme changes |
| `on_focus_changed` | No | `(&mut self, &FocusChanged, &mut Context<Self>) -> Update` | Observe a completed widget focus transition |
| `on_host_drag_and_drop` | No, root only | `(&mut self, HostEvent, &mut Context<Self>) -> Update` | Respond to the host's drag-and-drop protocol |
| `on_props_changed` | No | `(&mut self, &Props, &mut Context<Self>) -> Update` | React to property changes |
| `unmount` | No | `(&mut self, &mut Context<Self>)` | Teardown before removal |

The runtime calls `init` once before the component's first `view`. Return `Update::none()`
after local setup or `Update::command_only(cmd)` to start background work.

The runtime delivers widget focus, window focus, host theme, and modifier changes to all
mounted components in tree order, starting at the root. Each hook can update local state
and return an `Update` for its own scope. See [Focus events](focus.md#focus-events)
for widget focus.

`on_window_focus_changed` reports host window focus. A child terminal requesting
CSI `?1004` focus reporting receives its own host sequences.

`on_modifiers_changed` reports the effective set of physically
held Ctrl, Alt, Shift, and Super keys, including physical Alt side when available. Call
`ctx.set_modifier_key_reporting(true)` while an overlay needs physical modifier identity or captures
shortcuts, and disable it when the overlay closes. Fullscreen Unix input decodes associated text,
so shifted, composed, and AltGr text remains usable while reporting is enabled. Unix inline
surfaces ignore this request until their input reader supports associated text. Unsupported
terminals also leave physical sides unknown, while ordinary combined `KeyEvent`s still carry
modifiers. A key already held when reporting turns on is known from the latest mouse event, and
mouse events keep Ctrl and Shift current while reporting is on. Losing host focus clears the held
set. Tests can drive the callback with
`TestBackend::set_held_modifiers`.

Some hosts clear a Shift+drag text selection when a reported Shift release looks like typing. Call
`ctx.set_modifier_key_reporting_keeps_shift(false)` while users may select host text, such as in a
picker. A lone Shift press then pauses reporting until Shift is released, the next key, or a mouse
event without Shift. Modifiers pressed after Shift are not reported during the pause, so keep the
default `true` while recording shortcuts.

## State Flow

```
User Action → Event → Message → update() → State Change → Re-render
                  ↑___________________________|
```

1. User interacts (click, keypress)
2. Callback fires (`ctx.link().callback(...)`)
3. Message queued
4. `update()` called - mutate state
5. Return an `Update` with the requested refresh level and any command.
6. The runtime reruns views and reconciles the tree as needed for that level.

## The `Update` Return Type

Return `Update::render()` when your component's state changes what its subtree displays.
Use `Update::full()` when the change affects other component scopes or global composition.
Read the requested refresh with `update.level()`. `UpdateLevel::None` indicates that the
update requests no frame. `update.has_command()` reports whether it starts background work.

| Return | Use when |
|--------|----------|
| `Update::none()` | State changed only to mirror widget-owned runtime state, or nothing visual changed |
| `Update::terminal_paint()` | Repaint only the rows a live `Terminal` screen changed; nothing else on screen moved |
| `Update::paint()` | Repaint the existing realized tree without rerunning component views or layout |
| `Update::paint_with_command(cmd)` | Repaint the existing tree and start work without rebuilding view or layout |
| `Update::render()` | Rerun the emitting component scope's `view()`, then reconcile and lay out that subtree |
| `Update::render_with_command(cmd)` | Same component-scoped refresh while also starting background work |
| `Update::full()` | Rebuild from the root because state affects other scopes or global composition |
| `Update::full_with_command(cmd)` | Same root-wide refresh while also starting background work |
| `Update::command_only(cmd)` | Start background work without marking a component dirty |

`paint_with_command`, `render_with_command`, and `full_with_command` accept either a `Command`
or `Option<Command>`. The requested level stays the same when the command is absent.
`left.merge(right)` keeps the strongest refresh level and submits both commands in that
order, in the emitting component's scope. Background tasks can complete in any order.
A command combined with `TerminalPaint` or `TerminalGraphics` widens the result to `Paint`.
There is no terminal-paint-with-command constructor: arbitrary commands cannot guarantee
that terminal damage is the only visible change. Commands that change view or layout
inputs must send a follow-up message requesting the appropriate refresh.

High-frequency widget callbacks such as `ScrollView::on_viewport_change`,
`on_scroll`, drag updates, and cursor/selection sync should usually return
`Update::none()` when they only store the reported offset or selection in parent
state. Returning `Update::full()` from those paths can rebuild large trees on
every wheel tick or drag frame.

See [Performance](perf.md) for production patterns around update scope,
scrolling, memoization, and bounded work.

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::Increment => {
            ctx.state.count += 1;
            Update::render()
        }
        Msg::LoadData => {
            let id = ctx.props.user_id;
            Update::render_with_command(ctx.link().command(move |link| {
                // Runs on background thread
                let data = fetch_data(id);
                link.send(Msg::DataLoaded(data));
            }))
        }
        Msg::DataLoaded(data) => {
            ctx.state.data = data;
            Update::render()
        }
        Msg::NoOp => Update::none(),  // no redraw
    }
}
```

## Context Methods

Context methods that change runtime settings, register observers, or request actions take
`&mut self`. Call them from `init`, `update`, or another lifecycle hook. Reads such as
`ctx.theme()` and `ctx.use_context::<T>()` take `&self` and are available in `view`.
View animations are declarative. Methods such as `ctx.transition(...)` read or declare
an animation for the current view. Explicit restart and snap methods take `&mut self`.

| Method | Purpose |
|--------|---------|
| `ctx.state` | Mutable access to component state |
| `ctx.props` | Read-only access to current properties |
| `ctx.link()` | Build callbacks and commands |
| `ctx.request_focus(key)` | Move focus to a keyed widget, including before mount or inside an excluded scope |
| `ctx.blur()` | Clear current and retained focus identity (`Auto` restores its default target on render) |
| `ctx.focus_next()` / `ctx.focus_prev()` | Move through the focus ring explicitly, including under `Manual` |
| `ctx.show_devtools()` | Show the built-in DevTools panel on the next tick |
| `ctx.hide_devtools()` | Hide the built-in DevTools panel on the next tick |
| `ctx.toggle_devtools()` | Toggle the built-in DevTools panel on the next tick |
| `ctx.devtools_visible()` | Read the current runner-synchronized DevTools panel visibility |
| `ctx.set_devtools_metrics(factory)` | Lazily replace the ordered label/value rows in the DevTools App tab without scheduling a frame; an empty iterator clears them |
| `ctx.has_focus_within_key(key)` | Check if focus is within a subtree |
| `ctx.text_area_scrollbars(key)` | Read resolved vertical/horizontal scrollbar visibility for a keyed `TextArea` from the previous frame |
| `ctx.has_focus_within_scope(id)` | Check focus within a scope |
| `ctx.toast()` | Show toast notifications |
| `ctx.clipboard()` | Programmatic clipboard access (copy/read) |
| `ctx.quit()` | Exit the application |
| `ctx.set_title(title)` | Replace the host terminal window title set by `App::title`; written with the next frame, and only when it changed |
| `ctx.is_inline()` | Whether running in inline mode |
| `ctx.command_chord_pending()` | Whether an app command chord is currently pending completion (e.g., after a leader prefix key). Entering or leaving pending always dirties a frame so chrome like a PREFIX badge updates even when the completing/mismatch key is forwarded to a focused terminal with no paint of its own. |
| `ctx.command_chord_pending_since()` | When the pending chord started, or `None` when none is pending |
| `ctx.command_chord_revealed()` | Whether the pending chord has been held for at least [`App::command_chord_reveal_delay`](#deferring-chord-chrome); the signal for a which-key panel |
| `ctx.set_command_chord_reveal_delay(d)` | Retime the reveal delay at runtime (config reload) |
| `ctx.cursor_trail()` | The [`App::cursor_trail`](quick-start.md#app-configuration) in effect, or `None` |
| `ctx.set_cursor_trail(trail)` | Change or (with `None`) remove the cursor trail at runtime (config reload); a trail on screen stops with the next frame |
| `ctx.skip_cursor_trail()` | Draw no trail for the caret's next move and stop one on screen, for a transition the app animates itself, such as switching to another screen; the skip waits for that move through frames that leave the caret where it was, and later moves trail as usual |
| `ctx.scroll_wheel_multiplier()` | Current app-wide mouse wheel step multiplier |
| `ctx.set_scroll_wheel_multiplier(n)` | Change the app-wide mouse wheel step multiplier at runtime (clamped to `>= 1`) |
| `ctx.last_mouse()` | Last pointer in terminal content coordinates, or `None` until a mouse event has been seen. Updated even when motion is forwarded to a tracking terminal. |
| `ctx.effect_phase()` | Current renderer animation phase; capture it when starting one-shot phase-based effects |
| `ctx.mouse_capture_enabled()` | Current mouse capture state |
| `ctx.set_mouse_capture(bool)` | Change mouse capture at runtime |
| `ctx.toggle_mouse_capture()` | Toggle mouse capture, returns new state |
| `ctx.suspend_to_shell()` | Stop the app to the shell like `ctrl+z`, releasing and restoring the terminal around the stop (see [External programs](external-programs.md#suspending-to-the-shell-ctrlz)) |
| `ctx.theme()` | Clone the active theme for this subtree |
| `ctx.theme_ref()` | Borrow the active theme for this subtree without cloning (`Ref<'_, Theme>`) |
| `ctx.with_theme(f)` | Run `f(&Theme)` against the active theme without cloning |
| `ctx.set_theme(theme)` | Replace the app-wide theme immediately (including from `init()`) and stop following the host palette |
| `ctx.follow_system_theme()` | Resume following the host terminal palette when `App::system_theme()` or `App::live_host_terminal_colors(true)` is enabled |
| `ctx.theme_extension::<T>()` | Clone a typed app-specific theme extension |
| `ctx.host_terminal_colors()` | Read the runner-managed `HostTerminalColors` cache when live host colors are enabled |
| `ctx.set_host_default_background(color)` | Write cells whose background is `color` as the host terminal's default background, so a translucent terminal shows through them; `None` turns it off. Unlike `App::host_background_hint`, which gives blending a concrete color, this changes only the bytes sent to the host. Read `host_terminal_colors()` in `init` and `on_host_theme_changed` to follow the host palette. |
| `ctx.host_drag_and_drop()` | Send drag-and-drop commands to the host terminal; events arrive at `on_host_drag_and_drop` (see [Drag and drop](drag-and-drop.md)) |
| `ctx.host_cursor_style()` | Read the host cursor shape and blink from the startup probe, or `None` when the host did not answer |
| `ctx.host_color_scheme()` | Read the light or dark `ColorScheme` the host reported through DEC mode 2031, or `None` before it reports one |
| `ctx.host_terminal_color_generation()` | Read the cache generation; increments when refreshed colors or the reported color scheme differ |
| `ctx.request_host_terminal_color_refresh()` | Queue a safe runner-owned host-color refresh on the UI thread |
| `ctx.use_context::<T>()` | Read nearest `ContextProvider<T>` value for this subtree |
| `ctx.append_transcript_lines(lines)` | Append styled lines to transcript history (inline only) |
| `ctx.append_transcript_element(el)` | Append a rendered element to transcript history (inline only) |
| `ctx.request_full_repaint()` | Next frame does a **full** reconcile + paint (use after the host terminal was used by another process; see [External programs](external-programs.md)) |
| `ctx.request_ui_snapshot_to(path)` | Queue a UI snapshot file write after the next paint (see [Agent snapshots](#agent--design-review-snapshots)) |
| `ctx.request_ui_snapshot_to_slot(slot)` | Queue in-memory UI snapshot delivery into `UiSnapshotSlot` after the next paint |
| `ctx.request_ui_snapshot(callback)` | Call `callback` with a `UiSnapshot` of the next paint; callbacks accumulate, so every caller before that paint gets the same snapshot |
| `ctx.observe_painted_frames(callback)` | Call `callback` with a `PaintedFrame` after every paint until the returned `PaintSubscription` is dropped. Passive: never causes a paint (see [Observing every painted frame](testing.md#observing-every-painted-frame)) |

`ctx.effect_phase()` is a snapshot, not a render subscription. Use it to store a start tick in component state during `update()` / `init()`, then build phase-based effects like `VisualEffect::centered_burst_ripple(...)` from that stored value.

Live host terminal colors are opt-in. Use `App::system_theme()` for a framework-wide theme derived from the host palette, or `App::live_host_terminal_colors(true)` when app code needs extra host-derived tokens. The runner probes OSC 4/10/11 once at startup, refreshes on terminal focus gained, and services `ctx.request_host_terminal_color_refresh()` while coordinating with its input reader. On Unix fullscreen surfaces it additionally enables DEC private mode 2031 and asks for the current scheme once (`CSI ? 996 n`); compatible terminals then send exact dark/light palette-change notifications that trigger the same complete OSC 4/10/11 refresh. The reported scheme is available as `ctx.host_color_scheme()`: it is the terminal's own setting, so prefer it to guessing light or dark from the background color, and fall back to the background when it is `None`. The persistent input worker observes those replies without handing the byte stream to a second parser, so interleaved input remains intact while all 16 resolved RGB ANSI slots plus foreground and background are acquired. Palette-only changes advance the cache generation and repaint immediately. A changed refresh schedules a complete repaint without presenting a cleared intermediate frame. Inline, non-Unix, and unsupported terminals retain startup, focus-gained, and manual refresh behavior. The runner never polls continuously. Use `ctx.host_terminal_colors()` for app-specific tokens beyond the framework theme; keep those tokens app-owned.

When the `devtools` feature is enabled, the built-in panel can be controlled from app code as well as the global keymap. This is useful for wiring DevTools to a button, command palette entry, startup action, or app-specific command:

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::OpenDevtools => ctx.show_devtools(),
        Msg::CloseDevtools => ctx.hide_devtools(),
        Msg::ToggleDevtools => ctx.toggle_devtools(),
    }
    Update::none()
}
```

`ctx.devtools_visible()` reports the applied panel state, including closes from
Esc or the global toggle. A queued show/hide/toggle request is reflected after
the runner applies it on the next tick. It always returns `false` without the
`devtools` feature.

Apps can also publish a small ordered set of structured metrics. Each call
replaces the previous set, so call it from `init()` or `update()` with the
current snapshot:

```rust
use tui_lipan::DevToolsMetric;

ctx.set_devtools_metrics(|| [
    DevToolsMetric::new("Panes", pane_count.to_string()),
    DevToolsMetric::new("Queue", queue_depth.to_string()),
]);

// Clear the App tab:
ctx.set_devtools_metrics(std::iter::empty);
```

The closure runs immediately when `devtools` is enabled; the panel later reads
stored values only and never calls back into the host app while rendering.
Publishing does not schedule an app frame. An open App tab shows changed rows
on its next panel refresh. The tab sizes to its content within the viewport and
becomes vertically scrollable when rows do not fit. Without the `devtools`
feature, the closure is not invoked, so formatting and allocation are skipped
while the same source keeps compiling.

To opt out of individual subsystems (logs, metrics) at app start time, see [DevTools runtime configuration](quick-start.md#devtools-runtime-configuration) in the Quick Start.

### Deferring chord chrome

A panel that lists what a pending chord can do next — a which-key panel — should not flash on every
chord the user completes from muscle memory. `App::command_chord_reveal_delay` sets how long a chord
must be held before `ctx.command_chord_revealed()` reports it:

```rust
App::new().command_chord_reveal_delay(Duration::from_millis(350))
```

```rust
fn view(&self, ctx: &Context<Self>) -> Element {
    let mut root = ZStack::new().child(self.workspace(ctx));
    if ctx.command_chord_revealed() {
        root = root.child(which_key_panel(ctx));
    }
    root.into()
}
```

The runtime schedules the frame at which the delay elapses, so the view needs no timer: a chord
completed or cancelled first simply never reveals. Keep using `ctx.command_chord_pending()` for
chrome that must react on the first keystroke — a mode badge, or suppressing a caret that would
otherwise suggest the next key goes to the focused widget. The default delay is zero, which makes
the two identical.

## Component Mounting

```rust
fn main() -> tui_lipan::Result<()> {
    App::new()
        .mount(MyApp)        // Takes an instance, not a type
        .run()
}

// Dependency injection: pass data into the constructor
let app = MyApp::new(db_connection, config);
App::new().mount(app).run();
```

## Properties vs State

| | Properties | State |
|--|------------|-------|
| **Source** | Parent / mount | Local to component |
| **Mutability** | Immutable (read via `ctx.props`) | Mutable via `ctx.state` |
| **Lifetime** | Passed each render | Persisted across renders |
| **Common use** | Configuration, DI | User input, loaded data |

```rust
#[derive(Clone, PartialEq)]
struct Props { user_id: u64 }

#[derive(Default)]
struct State {
    user_name: String,
    is_loading: bool,
}
```

> **Note**: Properties must implement `Clone + PartialEq` for reconciliation.

## Commands (Async / Background Work)

Build background commands through `ctx.link()` and return them in an `Update`.
The component link infers the message type for the closure's `CommandLink` and
determines which component receives its messages. If another component returns
the command in an `Update`, messages still go to the component whose link built
the command. Commands built from a link run only in that link's runtime.
The message type must implement `Send`.

```rust
// Run a blocking call on a worker.
let cmd = ctx.link().command(move |link| {
    let result = blocking_call();
    link.send(Msg::Done(result));
});

// Coalesce work with the same key.
let cmd = ctx.link().command_keyed(
    "search",
    TaskPolicy::LatestOnly,
    move |link| {
        if link.is_cancelled() {
            return;
        }
        let results = do_search(&query);
        let _sent = link.send_if_not_cancelled(Msg::SearchDone(results));
    },
);
```

`command_keyed` accepts `impl Into<Arc<str>>`, including string literals, owned
`String` values, and `Arc<str>`. A key identifies work for one component within
one runtime.

### TaskPolicy options

| Policy | Behavior |
|--------|----------|
| `QueueAll` | Enqueue every task. Native workers may run same-key tasks concurrently. |
| `DropIfRunning` | Ignore new task while one with the same key is running; the active task is not cancelled. |
| `LatestOnly` | Keep only the newest pending task, cancel the active token, and cancel replaced pending tokens. |

Cancellation is cooperative: a keyed `LatestOnly` task is not preempted. Poll
`link.is_cancelled()` or clone `link.cancellation_token()` for long loops, and
use `link.send_if_not_cancelled(msg)` to suppress stale results. `link.send(msg)`
remains unconditional for cleanup/error messages that should report even after
cancellation.

```rust
use tui_lipan::TaskPolicy;

// Example: filter-as-you-type pattern
match msg {
    Msg::QueryChanged(q) => {
        let cmd = ctx.link().command_keyed("filter", TaskPolicy::LatestOnly, move |link| {
            let results = filter_items(&q);
            let _ = link.send_if_not_cancelled(Msg::FilterDone(results));
        });
        Update::command_only(cmd)
    }
}
```

### Delayed work: debounces, retries, and ticks

On native targets, background commands run on a pool of 2-8 workers. Sleeping
inside a command holds a worker for the whole delay. Use
`ctx.link().command_after(delay, f)` to wait on a shared timer and submit the
closure to the pool when it is due.

```rust
use std::time::Duration;

let command = ctx.link().command_after(Duration::from_millis(16), |link| {
    link.send(Msg::FlushResizes);
});
Update::command_only(command)
```

Re-arm the command from the message handler to schedule a recurring tick.

```rust
Msg::Tick => {
    refresh(ctx);
    Update::render_with_command(ctx.link().command_after(Duration::from_secs(1), |link| {
        link.send(Msg::Tick);
    }))
}
```

When you already hold a `CommandLink` and only need to deliver a message later,
use `link.send_after(Duration::from_millis(800), Msg::Deadline)`. If the
command is cancelled before the deadline, the timer drops the message.

Delayed commands use the owning session's clock. In a controlled
`AutomationSession` or `TestBackend`, a delayed command runs when
`advance(...)` reaches its deadline. Realtime terminal and web sessions use
their timer workers. An immediate background command can finish without
advancing a controlled session's clock.

### Async runtimes (tokio)

On native targets, tui-lipan commands are blocking closures. Keep a
`tokio::runtime::Handle` on the component and inject it when you mount the
component. Keep the runtime alive while the app runs. Clone the handle in
`update` and move the `CommandLink` into the future.

```ignore
let handle = self.tokio.clone();
Update::command_only(ctx.link().command(move |link| {
    let busy = link.busy();
    handle.spawn(async move {
        let _busy = busy;
        if link.is_cancelled() {
            return;
        }
        let result = fetch_async().await;
        let _ = link.send_if_not_cancelled(Msg::Done(result));
    });
}))
```

Tokio polls the future, so the UI thread never waits for it. The worker is
free as soon as `spawn` returns. The component itself can stay on the UI
thread. The guard from `link.busy()` keeps the session busy until the future
finishes or drops the guard. See [Automation idle detection](testing.md#persistent-automation-sessions).

`is_cancelled` checks the command's token and `send_if_not_cancelled`
suppresses delivery if the token was cancelled. A keyed command finishes
when its closure returns. A Tokio task spawned by that closure can outlive
it, so use a Tokio task handle or your own cancellation signal to cancel
the detached task when newer work replaces it.

### External interactive subprocesses

Use `Command::on_ui_thread` with `terminal_handoff` to run an editor or
pager that needs the real terminal. The event loop waits for the closure
to return. See [External programs](external-programs.md).

## Nested Components

Use `child()` to embed components within a view:

```rust
use tui_lipan::child;

fn view(&self, ctx: &Context<Self>) -> Element {
    child(
        || MyChild,             // factory closure
        MyChildProps { x: 1 }, // properties
    )
}
```

Or use the `rsx!` macro with a component type:

```rust
rsx! {
    // Widget types used directly as elements
    VStack {
        MyChildWidget { value: 42 }
    }
}
```

### Parent → Child Communication (Props)

Parents pass data and callbacks to children via Properties:

```rust
#[derive(Clone, PartialEq)]  // ← REQUIRED: Clone + PartialEq
struct SidebarProps {
    items: Vec<String>,
    selected: usize,
    on_select: Callback<usize>,   // Callback for child → parent
}
```

### Child → Parent Communication (Callback Props)

Children notify parents by emitting callback props. Messages are **scoped** - a child
cannot directly send messages to the parent's update loop:

```rust
struct Sidebar;

#[derive(Clone)]
enum SidebarMsg {
    Selected(usize),
}

impl Component for Sidebar {
    type Message = SidebarMsg;
    type Properties = SidebarProps;
    type State = ();

    fn create_state(&self, _: &SidebarProps) -> () { () }

    fn view(&self, ctx: &Context<Self>) -> Element {
        List::new()
            .items(ctx.props.items.iter().map(|s| ListItem::new(s.clone())))
            .selected(ctx.props.selected)
            .on_select(ctx.link().callback(|e: ListEvent| SidebarMsg::Selected(e.index)))
            .into()
    }

    fn update(&mut self, msg: SidebarMsg, ctx: &mut Context<Self>) -> Update {
        match msg {
            SidebarMsg::Selected(idx) => {
                // Notify parent via callback prop:
                ctx.props.on_select.emit(idx);
                Update::none()  // Parent will re-render with new props
            }
        }
    }
}

// In parent view():
fn view(&self, ctx: &Context<Self>) -> Element {
    HStack::new()
        .child(child(
            || Sidebar,
            SidebarProps {
                items: ctx.state.items.clone(),
                selected: ctx.state.selected,
                on_select: ctx.link().callback(Msg::ItemSelected),
            },
        ))
        .child(Text::new("Detail panel").into())
        .into()
}
```

### Key Rules for Nested Components

1. **Properties must implement `Clone + PartialEq`** - required for reconciliation.
2. **Messages are scoped** - each component has its own message queue.
3. **`child()` takes a factory closure** - not just a type: `child(|| MyComp, props)`.
4. **Communication is unidirectional**: parent → child via props, child → parent via callback props.
5. **State is isolated** - children don't access parent state.

## Retained Subtree Reuse

Components can opt into retained subtree reuse by returning a stable key from `memo_key()`:

```rust
impl Component for MessageRow {
    type Message = Msg;
    type Properties = RowProps;
    type State = RowState;

    fn create_state(&self, props: &Self::Properties) -> Self::State {
        RowState::from(props)
    }

    fn memo_key(&self, props: &Self::Properties, _ctx: &Context<Self>) -> Option<u64> {
        Some(props.revision)
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        render_row(ctx.props)
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        handle_row_msg(msg, ctx)
    }
}
```

When `memo_key()` returns the same value, the runtime may reuse the component's previously
expanded subtree and skip `view()`. Reuse is automatically invalidated when:

- local state or props mark the component dirty
- a nested child component under that subtree needs refresh
- a `Context` value read during `view()` changes (`theme()`, `theme_extension()`, focus/hover
  queries, `mouse_capture_enabled()`, `viewport()`, `breakpoint()`, `use_context::<T>()`)

Use `memo_key()` for expensive rows, panes, or tool outputs that are stable across unrelated
parent updates. Keep the key focused on semantic content identity (`revision`, `version`, hash of
derived props), not transient UI state that already lives in `State`.

## Component State Keys

`component_state_key` preserves a component's local state even when its ancestor container
structure changes (for example, wrapping a widget in an extra `VStack` or moving it between
branches). It is declared on the element that mounts the component:

```rust
fn view(&self, _ctx: &Context<Self>) -> Element {
    VStack::new()
        .child(
            child(|| Modal, modal_props)
                .component_state_key("modal")
        )
        .into()
}
```

### Scoping and duplicate-key policy

State keys are global within the app's component registry. Give each simultaneously mounted
component a unique state key, even across different parents. When a component moves to
another branch, the same state key and component type let the runtime reuse its instance.
When the type differs, the state key cannot carry the old instance over.

## Snapshot / Visual Testing

Snapshot capture, headless PNGs, recordings, the live control channel, design
sketches, and visual regression baselines live in [`docs/testing.md`](testing.md).

## Key Attribute (Reconciliation)

Assign stable keys to preserve state across re-renders and enable focus routing:

```rust
rsx! {
    List { key: "file-list", ... }
    Input { key: format!("input-{}", id), ... }
}
```

> Without a key, reconciliation uses position, which breaks when items are added/removed.

### Terminal presentation observers

With `terminal`, `Context::observe_terminal_presentations(callback)` registers a passive observer
for successful host frames. Keep its `TerminalPresentationSubscription` alive; dropping it removes
the callback. The payload is `Vec<PresentedTerminal>`, with the highest application token per
visible screen in that frame. Set tokens with `TerminalScreen::set_presentation_token()` after
processing output. Unlike painted-frame captures, this observer performs no software render.

`Update::terminal_graphics()` requests the narrowest visual update for retained terminal graphics.
The runtime widens it when the native presentation plan cannot reproduce the scene correctly.
Adding `UpdateLevel::TerminalGraphics` is a breaking enum change: exhaustive matches must handle
it below `TerminalPaint` and above `None`.

## Restarting and snapping keyed transitions

Ordinary `ctx.transition`, `ctx.animated_color`, and `ctx.animated_amount` calls keep their
current timing when the target is unchanged, even if a new configuration has zero duration.
Use an explicit control once at a lifecycle boundary to restart or hand off an animation:

```rust,ignore
// Restart from the current value, including when the target is already 1.0.
ctx.restart_transition("reveal", 1.0f32, config);
// Hand off from a known value, including when this key has not been seen before.
ctx.restart_transition_from("reveal", captured_progress, 1.0f32, config);
// Cancel motion and set both current value and target immediately.
ctx.snap_transition("reveal", 1.0f32);
```

When calling a control from `update`, return an update requesting the repaint or view/layout
work that the change needs. Then read the key through the ordinary method on subsequent views.
Calling a restart on every view resets its timing on every view. A new key restarted without
a seed starts at its target; use `restart_transition_from` when it must start elsewhere.

These operations address Context's keyed registry. An `Animated` wrapper owns separate
per-node transitions; a registry key does not identify those internal transitions.

Controls return no interpolated value, so an existing `animated_color` or `animated_amount`
remains paint-only. A key read through `transition` continues to require view updates.
Existing render handles remain valid, and the key retains its selected repaint cadence.
Keys remain scoped to the component, follow the usual lifetime rules, and reject a change
of value type. A restart with zero duration snaps to the requested target.

An explicit control replaces a pulse on that key. Ordinary pulse reads keep their phase,
including across changes to their amplitude; unrelated and suspended pulses keep their
own timelines. Reading a controlled key as a pulse again starts a new pulse cycle.

## Paste routing

Implement `Component::on_paste` to accept text while a non-editable child such as a
form option list has focus. Inputs, text areas and terminals receive pastes first.
Unhandled pastes then visit the focused component and its parents, stopping at
`KeyUpdate::handled`. Sibling components do not receive the paste, and no component
receives it without a focused node. Focus queries reflect the current focus.

```rust,ignore
use tui_lipan::PasteOrigin;

fn on_paste(
    &mut self,
    text: &str,
    _origin: PasteOrigin,
    ctx: &mut Context<Self>,
) -> KeyUpdate {
    ctx.state.custom_answer = text.to_owned();
    ctx.state.editing = true;
    ctx.request_focus("custom-answer");
    KeyUpdate::handled(Update::render())
}
```

`PasteOrigin` distinguishes terminal paste events from clipboard text delivered by
mouse paste. Updates and commands returned by the hook run in the receiving
component's scope, just as for `on_key`. Return `KeyUpdate::unhandled` to let a
parent handle the text. An editable child that accepts the paste prevents the
component hook from running.
