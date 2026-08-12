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

## Lifecycle Methods

| Method | Required | Signature | Purpose |
|--------|----------|-----------|---------|
| `create_state` | Yes | `(&self, &Props) -> State` | Initialize state from properties |
| `memo_key` | No | `(&self, &Props, &Context<Self>) -> Option<u64>` | Opt into retained subtree reuse |
| `view` | Yes | `(&self, &Context<Self>) -> Element` | Return UI tree |
| `update` | Yes | `(&mut self, Msg, &mut Context<Self>) -> Update` | Handle messages |
| `init` | No | `(&mut self, &mut Context<Self>) -> Option<Command>` | One-time setup on mount |
| `on_key` | No | `(&mut self, KeyEvent, &mut Context<Self>) -> KeyUpdate` | Handle unhandled key events |
| `on_window_focus_changed` | No, root only | `(&mut self, bool, &mut Context<Self>) -> Update` | React to host terminal/window focus transitions |
| `on_props_changed` | No | `(&mut self, &Props, &mut Context<Self>) -> Update` | React to property changes |
| `unmount` | No | `(&mut self, &mut Context<Self>)` | Teardown before removal |

`on_window_focus_changed` runs only on the mounted root when the host reports an actual
focus transition. It is not widget focus: use widget `.on_focus` / `.on_blur` callbacks and the
focus APIs for keyboard routing. It is also separate from a child `Terminal` requesting CSI
`?1004` focus reporting; the runner continues to send those sequences only to that terminal.

## State Flow

```
User Action → Event → Message → update() → State Change → Re-render
                  ↑___________________________|
```

1. User interacts (click, keypress)
2. Callback fires (`ctx.link().callback(...)`)
3. Message queued
4. `update()` called - mutate state
5. Return `(needs_redraw: bool, command: Option<Command>)`
6. `view()` re-executed if dirty or memoization cannot retain the subtree
7. Tree reconciled and rendered

## The `Update` Return Type

`Update` is a named struct with a dirty flag, a refresh level, and an optional
`Command`. Pick the smallest refresh that matches the state change:

| Return | Use when |
|--------|----------|
| `Update::none()` | State changed only to mirror widget-owned runtime state, or nothing visual changed |
| `Update::paint()` | Repaint the existing realized tree without rerunning component views or layout |
| `Update::layout()` | Rerun the emitting component scope's `view()`, then reconcile and lay out that subtree |
| `Update::layout_with_command(cmd)` | Same component-scoped refresh while also starting background work |
| `Update::full()` | Rebuild from the root because state affects other scopes or global composition |
| `Update::with_command(cmd)` | Same root-wide refresh while also starting background work |
| `Update::command_only(cmd)` | Start background work without marking a component dirty |

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
            Update::full()   // redraw, no background work
        }
        Msg::LoadData => {
            let id = ctx.props.user_id;
            Update::with_command(ctx.link().command(move |link| {
                // Runs on background thread
                let data = fetch_data(id);
                link.send(Msg::DataLoaded(data));
            }))
        }
        Msg::DataLoaded(data) => {
            ctx.state.data = data;
            Update::full()
        }
        Msg::NoOp => Update::none(),  // no redraw
    }
}
```

## Context Methods

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
| `ctx.is_inline()` | Whether running in inline mode |
| `ctx.command_chord_pending()` | Whether an app command chord is currently pending completion (e.g., after a leader prefix key). Entering or leaving pending always dirties a frame so chrome like a PREFIX badge updates even when the completing/mismatch key is forwarded to a focused terminal with no paint of its own. |
| `ctx.command_chord_pending_since()` | When the pending chord started, or `None` when none is pending |
| `ctx.command_chord_revealed()` | Whether the pending chord has been held for at least [`App::command_chord_reveal_delay`](#deferring-chord-chrome); the signal for a which-key panel |
| `ctx.effect_phase()` | Current renderer animation phase; capture it when starting one-shot phase-based effects |
| `ctx.mouse_capture_enabled()` | Current mouse capture state |
| `ctx.set_mouse_capture(bool)` | Change mouse capture at runtime |
| `ctx.toggle_mouse_capture()` | Toggle mouse capture, returns new state |
| `ctx.suspend_to_shell()` | Stop the app to the shell like `ctrl+z`, releasing and restoring the terminal around the stop (see [External programs](external-programs.md#suspending-to-the-shell-ctrlz)) |
| `ctx.theme()` | Clone the active theme for this subtree |
| `ctx.theme_extension::<T>()` | Clone a typed app-specific theme extension |
| `ctx.host_terminal_colors()` | Read the runner-managed `HostTerminalColors` cache when live host colors are enabled |
| `ctx.host_terminal_color_generation()` | Read the cache generation; increments when refreshed colors differ |
| `ctx.request_host_terminal_color_refresh()` | Queue a safe runner-owned host-color refresh on the UI thread |
| `ctx.use_context::<T>()` | Read nearest `ContextProvider<T>` value for this subtree |
| `ctx.append_transcript_lines(lines)` | Append styled lines to transcript history (inline only) |
| `ctx.append_transcript_element(el)` | Append a rendered element to transcript history (inline only) |
| `ctx.request_full_repaint()` | Next frame does a **full** reconcile + paint (use after the host terminal was used by another process; see [External programs](external-programs.md)) |
| `ctx.request_ui_snapshot_to(path)` | Queue a UI snapshot file write after the next paint (see [Agent snapshots](#agent--design-review-snapshots)) |
| `ctx.request_ui_snapshot_to_slot(slot)` | Queue in-memory UI snapshot delivery into `UiSnapshotSlot` after the next paint |

`ctx.effect_phase()` is a snapshot, not a render subscription. Use it to store a start tick in component state during `update()` / `init()`, then build phase-based effects like `VisualEffect::centered_burst_ripple(...)` from that stored value.

Live host terminal colors are opt-in. Use `App::system_theme()` for a framework-wide theme derived from the host palette, or `App::live_host_terminal_colors(true)` when app code needs extra host-derived tokens. The runner probes OSC 4/10/11 once at startup, refreshes on terminal focus gained, and services `ctx.request_host_terminal_color_refresh()` while coordinating with its input reader. On Unix fullscreen surfaces it additionally enables DEC private mode 2031; compatible terminals then send exact dark/light palette-change notifications, which trigger an immediate typed OSC 10/11 refresh. The runtime cache retains the startup probe's resolved RGB ANSI slots because Termina does not yet expose OSC 4 responses; it never substitutes unresolved indexed colors into app-owned theme tokens. A changed refresh schedules a complete repaint without presenting a cleared intermediate frame. Inline, non-Unix, non-live, and unsupported terminals retain startup, focus-gained, and manual OSC 4/10/11 refresh behavior. The runner never polls continuously. Use `ctx.host_terminal_colors()` for app-specific tokens beyond the framework theme; keep those tokens app-owned.

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
replaces the previous set, so call it from `view()` or `update()` with the
current snapshot:

```rust
ctx.set_devtools_metrics(|| [
    DevToolsMetric::new("Panes", pane_count.to_string()),
    DevToolsMetric::new("Queue", queue_depth.to_string()),
]);

// Clear the App tab:
ctx.set_devtools_metrics(std::iter::empty);
```

The closure runs immediately when `devtools` is enabled; the panel later reads
stored values only and never calls back into the host app while rendering.
Publishing does not schedule a frame: values published from `view()` are read
by the DevTools extra root later in that same frame. Calls made elsewhere are
stored until a later frame rebuilds the panel; the host update is responsible
for requesting that frame. The tab sizes to its content within the viewport and
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

Components are single-threaded. Use `Command` for background work:

```rust
// Generic command: any closure
let cmd = ctx.link().command(move |link| {
    let result = blocking_call();
    link.send(Msg::Done(result));
});

// Keyed command: prevent stale work from piling up
let cmd = ctx.link().command_keyed(
    "search",                  // key (any &'static str)
    TaskPolicy::LatestOnly,    // coalescing policy
    move |link| {
        if link.is_cancelled() {
            return;
        }
        let results = do_search(&query);
        let _sent = link.send_if_not_cancelled(Msg::SearchDone(results));
    },
);
```

### TaskPolicy Options

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

Never `thread::sleep` inside a command. Tasks run on a fixed pool of 2-8 workers, so a sleeping
task occupies one for the whole delay — two recurring timers are enough to park the pool on a
low-core machine and stall every other background task behind them.

Use `Command::after`, which waits on a shared timer thread and reaches the pool only once due:

```rust
use std::time::Duration;

// Debounce: coalesce a resize storm into one flush.
Command::after(Duration::from_millis(16), |link: CommandLink<Msg>| {
    link.send(Msg::FlushResizes);
})
```

Re-arming from the handler gives a recurring tick that costs no thread between firings:

```rust
Msg::Tick => {
    refresh(ctx);
    Update::with_command(Command::after(Duration::from_secs(1), |link: CommandLink<Msg>| {
        link.send(Msg::Tick);
    }))
}
```

When you already hold a `CommandLink` and only need to deliver a message later, `send_after` is
the direct form. It is dropped if the command is cancelled first:

```rust
link.send_after(Duration::from_millis(800), Msg::Deadline);
```

Delay first, then work: the closure body still runs on the pool, so a delayed fetch or filesystem
sweep is fine inside `Command::after` — only the *waiting* moves off the pool.

### Thread Safety

Commands use channels internally. The component itself never needs to be `Send` or `Sync`.

### External interactive subprocesses

Spawning an editor or pager that needs the real terminal must **not** use `Command::spawn` / `ctx.link().command(...)` alone: use `Command::new` on the UI thread together with [`terminal_handoff`](external-programs.md), then [`request_full_repaint()`](external-programs.md#force-a-full-redraw-after-handoff) if needed. See **[External programs](external-programs.md)**.

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

State keys are scoped per **parent component**. Two components with the same
`component_state_key` that are children of the same parent are considered duplicates.
In that case the runtime uses **last-writer-wins**: the second component reuses (and
overwrites props on) the same instance.

Debug builds log a warning when duplicate sibling keys are detected:

```
Duplicate component_state_key "modal" detected; last-writer-wins
```

Duplicates across **different parent scopes** (or unrelated branches) are fine. Because
the key is global within the registry, a component in one branch can reuse the state of
a previously-mounted component with the same key in another branch. This is useful for
preserving form state when switching between tabs or conditional views.

### Type mismatches

If a state key is reused but the component type does not match, the runtime falls back
to creating a fresh instance rather than coercing the wrong type.

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
