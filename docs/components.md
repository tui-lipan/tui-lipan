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
| `on_props_changed` | No | `(&mut self, &Props, &mut Context<Self>) -> Update` | React to property changes |
| `unmount` | No | `(&mut self, &mut Context<Self>)` | Teardown before removal |

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
| `ctx.effect_phase()` | Current renderer animation phase; capture it when starting one-shot phase-based effects |
| `ctx.mouse_capture_enabled()` | Current mouse capture state |
| `ctx.set_mouse_capture(bool)` | Change mouse capture at runtime |
| `ctx.toggle_mouse_capture()` | Toggle mouse capture, returns new state |
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

`TestBackend` supports headless snapshot testing via `capture_frame()`. After a `render()` (or `dispatch()` / `send_key()` which implicitly re-render), call `capture_frame()` to get a `CapturedFrame` containing the full rendered buffer as crate-owned types - no ratatui types leak.

### Plain-text snapshot with `insta`

```rust
use tui_lipan::prelude::*;

struct MyWidget;

impl Component for MyWidget {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _: &()) -> () { () }
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update { Update::none() }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Frame::new()
            .header_left("Panel")
            .child(Text::new("hello"))
            .into()
    }
}

#[test]
fn snapshot_my_widget() {
    let mut backend = TestBackend::new(MyWidget);
    backend.set_viewport(Rect { x: 0, y: 0, w: 30, h: 5 });
    backend.render();

    let frame = backend.capture_frame();
    insta::assert_snapshot!(frame.plain_text());
}
```

`plain_text()` returns newline-joined rows with trailing spaces trimmed - the output is stable and deterministic across runs.

### Per-cell style assertions

```rust
let frame = backend.capture_frame();
let cell = frame.cell(0, 0);

assert_eq!(cell.symbol, "A");
assert_eq!(cell.fg, Color::Rgb(12, 34, 56));
assert_eq!(cell.bg, Color::Rgb(90, 80, 70));
assert!(cell.modifiers.bold);
```

### Styled runs

`styled_lines()` groups each row into `Vec<(String, Style)>` runs by identical style, useful for asserting that specific text is rendered with a certain color:

```rust
let runs = &frame.styled_lines()[0];
assert_eq!(runs[0].0, "error:");
assert_eq!(runs[0].1.fg, Some(Color::Red));
```

### Cursor capture

When a focused input widget requests cursor placement, `frame.cursor` is populated:

```rust
backend.focus_next();
backend.render();
let frame = backend.capture_frame();

let cursor = frame.cursor.expect("input should place cursor");
assert!(cursor.visible);
assert_eq!(cursor.y, 0);
```

### Animations

`render()` recomputes the tree but does not advance time, so anything time-based
renders at its starting value. `advance(dt)` ticks every animation and refreshes
layout or the rendered tree as needed, which lets a test assert that an animation
actually moves rather than that it merely exists. It covers `Animated` transitions,
smooth scrolls, and the property transitions behind `Context::transition`.

```rust
backend.render();
let start = width_of(&backend, "panel");

backend.advance(Duration::from_millis(25));
assert!(width_of(&backend, "panel") < start, "the panel should be shrinking");

backend.advance(Duration::from_millis(200));
assert_eq!(width_of(&backend, "panel"), 0);
```

Each call is clamped to one frame's worth of time (50 ms), the same clamp the
runner applies, so a single large `dt` behaves like one long frame instead of
jumping to the end of the animation. Step through with repeated calls when you
need intermediate states.

### Viewport resize

```rust
backend.set_viewport(Rect { x: 0, y: 0, w: 40, h: 10 });
backend.render();
let frame = backend.capture_frame();
assert_eq!(frame.width, 40);
assert_eq!(frame.height, 10);
```

### `CapturedFrame` API summary

| Method | Returns | Description |
|--------|---------|-------------|
| `plain_text()` | `String` | Full frame as trimmed plain text, `\n`-separated |
| `to_lines()` | `Vec<String>` | Same as `plain_text()` but per-row |
| `row(y)` | `&[CapturedCell]` | All cells for row `y` |
| `cell(x, y)` | `&CapturedCell` | Single cell at `(x, y)` |
| `styled_lines()` | `Vec<Vec<(String, Style)>>` | Rows grouped into style runs |
| `to_fixed_grid()` | `String` | Full-width rows without trailing trim (layout-faithful) |
| `to_ansi()` | `String` | ANSI styled frame (full terminal repaint prelude) |
| `to_ansi_diff(prev)` | `String` | Incremental ANSI update from a previous frame |
| `to_png(&PngOptions)` | `Result<Vec<u8>>` | PNG bytes with font-backed or bitmap rendering (`ui-snapshot-png`) |

`CapturedCell` fields: `symbol`, `fg`, `bg`, `underline_color`, `modifiers` (`CellModifiers` with bool fields `bold`, `dim`, `italic`, `underline`, `reverse`, `strikethrough`).

### Agent / design-review snapshots

`TestBackend::capture_ui_snapshot()` returns a `UiSnapshot`: rendered
`CapturedFrame` plus semantic `UiWidgetDesc` entries (widget kind, keys, rects,
focus/hover, selection, values). Use `to_markdown()` for agent-readable reports.
Enable the `ui-snapshot-json` feature for `to_json()` / `to_json_pretty()`.
Enable `ui-snapshot-png` for `to_png()` / `to_png_default()` when layout, color,
focus chrome, and visual hierarchy matter; PNG complements markdown/JSON rather
than replacing them. Both return `Result`, so an encoder failure surfaces at the
call rather than as a zero-byte file.

The PNG renderer uses antialiased real-font text by default when a system font is
available, with font8x8 bitmap rendering as the fallback. `PngOptions` is a
crate-root import (not prelude) and can select `PngTextRenderer::Auto`, `Font`,
or `Bitmap`; `font_family` / `font_path` let captures use system or Nerd Fonts.
Force `Bitmap` for deterministic coarse cell output and fallback-style reviews.

```rust
let mut backend = TestBackend::new(MyApp);
backend.set_viewport(Rect { x: 0, y: 0, w: 80, h: 24 });
backend.render();

let snapshot = backend.capture_ui_snapshot();
println!("{}", snapshot.to_markdown());

#[cfg(feature = "ui-snapshot-png")]
std::fs::write("/tmp/ui-snapshot.png", snapshot.to_png_default()?)?;
```

For design review captures, prefer fit-to-content margin helpers so flex space is visible without hand-tuning a viewport. The recommended default margin is `(20, 8)`:

```rust
let snapshot = backend.capture_ui_snapshot_with_margin(
    20,
    8,
    &UiSnapshotOptions::default(),
);
```

`capture_frame_with_margin(20, 8)` provides the same fit-to-content viewport behavior when you only need the rendered `CapturedFrame`.

**Live apps:** snapshot export is **queued until after the next paint** (not synchronous from `update()`). Each request replaces any earlier pending one. Requests schedule a repaint so idle apps still deliver. File routing follows the path extension: `.md` writes markdown, `.json` writes JSON with `ui-snapshot-json`, and `.png` writes the current viewport as PNG with `ui-snapshot-png`.

```rust
// Store the slot in component state:
struct State {
    slot: UiSnapshotSlot,
}

// In update():
ctx.request_ui_snapshot_to("ui-snapshot.md");
ctx.request_ui_snapshot_to_slot(&ctx.state.slot);

// Later (next tick / handler):
if let Some(snap) = ctx.state.slot.take() {
    // use snap
}
```

See `examples/ui_snapshot.rs`.

### Headless snapshots from the environment

Set `TUI_LIPAN_SNAPSHOT` and `AppRunner::run()` renders one frame off-screen,
writes the artifact, and returns - without entering raw mode or opening a
terminal. This captures an existing app or example without editing its source,
and works where there is no tty (CI runners, agent sessions).

```sh
TUI_LIPAN_SNAPSHOT=/tmp/app.png cargo run --example todo --features ui-snapshot-png
```

| Variable | Default | Effect |
|----------|---------|--------|
| `TUI_LIPAN_SNAPSHOT` | unset | Output path; setting it enables headless mode. Format routed by extension |
| `TUI_LIPAN_SNAPSHOT_VIEWPORT` | `100x30` | Layout viewport, `WIDTHxHEIGHT` |
| `TUI_LIPAN_SNAPSHOT_FRAMES` | `1` | Render/message passes before capture; raise when `init()` starts work |
| `TUI_LIPAN_SNAPSHOT_FOCUS` | `0` | Focus advances before capture, for visible focus chrome |
| `TUI_LIPAN_SNAPSHOT_KEYS` | unset | Comma-separated key script dispatched before capture, e.g. `tab,tab,enter` |
| `TUI_LIPAN_SNAPSHOT_SCRIPT` | unset | Full action script (see below); takes precedence over `_KEYS` |
| `TUI_LIPAN_SNAPSHOT_DIAGNOSTIC` | unset | `1` captures with `UiSnapshotOptions::diagnostic()` |

`TUI_LIPAN_SNAPSHOT_KEYS` uses ordinary keybinding syntax (`ctrl+n`, `esc`,
`f12`), the same spelling as keymaps. Each key is dispatched, its messages
drained, and the tree re-rendered before the next one - the same sequence as the
event loop - so typed text accumulates instead of collapsing to its last
character. This is how states behind a keystroke are captured without a harness:

```sh
TUI_LIPAN_SNAPSHOT=/tmp/modal.png TUI_LIPAN_SNAPSHOT_KEYS="tab,enter" cargo snap myapp
```

An unparseable script fails the run rather than being skipped, because a dropped
keystroke silently captures the wrong state.

Format follows the path extension, matching `request_ui_snapshot_to`: `.json`
with `ui-snapshot-json`, `.png` with `ui-snapshot-png`, markdown otherwise.

### Terminal recordings

A recording is text, not video: an asciinema cast v2 file is a JSON header plus
one `[time, "o", data]` line per output chunk, and `CapturedFrame::to_ansi_diff`
already produces that `data`. A few seconds of a real app is typically smaller
than a single PNG frame of it, and the result scrubs, selects as text, and plays
in a browser.

Recording needs **no feature flag and no extra dependency** - the cast JSON is
written directly, so it works in any build.

```sh
TUI_LIPAN_RECORD=/tmp/demo.cast TUI_LIPAN_RECORD_KEYS="tab,enter" cargo run --example todo
```

| Variable | Default | Effect |
|----------|---------|--------|
| `TUI_LIPAN_RECORD` | unset | Output path; setting it enables headless recording |
| `TUI_LIPAN_RECORD_VIEWPORT` | `100x30` | Recorded terminal size, `WIDTHxHEIGHT` |
| `TUI_LIPAN_RECORD_FPS` | `30` | Capture rate |
| `TUI_LIPAN_RECORD_KEYS` | unset | Key script to play, e.g. `tab,tab,enter` |
| `TUI_LIPAN_RECORD_KEY_DELAY_MS` | `400` | Pause after each key, so a viewer can follow |
| `TUI_LIPAN_RECORD_SETTLE_MS` | `1200` | Hold on the final frame |
| `TUI_LIPAN_RECORD_FRAMES` | unset | Directory for truecolor PNG frames (needs `ui-snapshot-png`) |
| `TUI_LIPAN_RECORD_SCRIPT` | unset | Full action script (see below); takes precedence over `_KEYS` |

In code, `Recording` mirrors `Sketch`:

```rust
use tui_lipan::Recording;

Recording::view("demo", login_screen)   // or Recording::component(...)
    .viewport(100, 30)
    .keys("tab,enter")
    .fps(30)
    .write("docs/demo.cast")?;
```

| Method | Effect |
|--------|--------|
| `Recording::view(title, fn)` | Record a plain `Fn() -> Element` |
| `Recording::component(title, c)` | Record a `Component` with default properties |
| `viewport(w, h)` | Recorded terminal size |
| `fps(n)` | Capture rate |
| `keys(script)` | Key script to play |
| `key_delay(duration)` | Pause after each key |
| `settle(duration)` | Hold on the final frame |
| `png_options(opts)` | Rendering options for frame export |
| `quiet(b)` | Suppress the written-path line |
| `record()` | Return a `CastRecording` without writing |
| `write(path)` | Write the cast; returns the path |
| `write_frames(dir)` | Write one truecolor PNG per frame; returns the paths |

**Timing is a synthetic fixed step.** The recorder advances a clock in `1/fps`
increments and ticks animations by the same amount, so the same view and script
always produce identical bytes - a committed recording stays diffable, and
animations are captured at full rate. The trade is that work depending on real
elapsed time (a PTY child's output, a network response) does not arrive on a
synthetic clock: recordings capture an app's own rendering, not a live session.

Identical frames are dropped, so a still stretch costs nothing. Because that
would otherwise end the file at the last visible change, the recorder writes a
final zero-length event to hold the closing frame for the intended duration
(`CastRecording::mark_time`).

### Choosing an output format

| Format | Size (7s demo) | Best for | Cost |
|--------|----------------|----------|------|
| `.cast` | **9 KB** | Docs sites, PR links, committing to the repo | Needs a player |
| `.mp4` via GIF | 26 KB | Slack, quick shares | 256-colour quantisation |
| `.gif` | 44 KB | READMEs - auto-plays inline with no player | 256 colours, largest |
| `.mp4` truecolor | 55 KB | Marketing, high-DPI, real colour fidelity | Needs frame export + ffmpeg |

Sizes are from the same recording of `examples/todo`; a busier UI widens the gap
in the cast's favour, since it transmits only changed cells while video re-encodes
whole frames.

Start with `.cast`. Reach for video only when the destination cannot play one.

#### GIF

[`agg`](https://github.com/asciinema/agg) converts a cast:

```sh
agg --theme dracula --idle-time-limit 1 --speed 1.5 demo.cast demo.gif
```

`--idle-time-limit` is the biggest size win - it caps dead air between keystrokes.
Themes: `asciinema`, `dracula`, `github-dark`, `github-light`, `kanagawa`,
`monokai`, `nord`, `solarized-dark`, `solarized-light`, `gruvbox-dark`.

#### MP4, the quick way

```sh
ffmpeg -i demo.gif -pix_fmt yuv420p -movflags +faststart \
       -vf "scale=trunc(iw/2)*2:trunc(ih/2)*2" demo.mp4
```

Both filters earn their place: `yuv420p` is what makes the file play in browsers
and chat clients, and the `scale` filter forces even dimensions, which H.264
requires - without it an odd-sized terminal fails to encode.

This route inherits GIF's 256-colour palette. For a flat theme that is invisible;
for gradients it bands.

#### MP4, truecolor

Export PNG frames instead, skipping GIF entirely:

```sh
TUI_LIPAN_RECORD=demo.cast TUI_LIPAN_RECORD_FRAMES=frames \
  cargo run --features ui-snapshot-png --example todo

ffmpeg -framerate 30 -i frames/frame_%05d.png \
       -pix_fmt yuv420p -movflags +faststart demo.mp4
```

`write_frames` and `TUI_LIPAN_RECORD_FRAMES` print that exact `ffmpeg` line with
the right paths and frame rate filled in, so it can be pasted straight back.

Frames are written at a **constant rate**, one per `1/fps` tick including
unchanged ones, because an encoder reconstructs timing from a numbered sequence.
That costs disk: a 7-second capture at 30fps is ~200 files and ~19 MB. They are
an intermediate - delete them after encoding. Unchanged frames reuse the previous
encode rather than paying for it twice.

Raise `PngOptions::scale` (via `Recording::png_options`) for a higher-resolution
video; the default 2x gives 16x32 pixels per cell.

```rust
Recording::view("demo", view)
    .viewport(90, 26)
    .keys("tab,enter")
    .write_frames("target/recordings/frames")?;
```

### Action scripts

A key script can only type. An action script can also click, hover, focus,
scroll, drag, and wait - enough to reach a modal behind a button or a row behind
a scroll.

```sh
TUI_LIPAN_SNAPSHOT=/tmp/after.png \
TUI_LIPAN_SNAPSHOT_SCRIPT="focus:#draft; type:buy milk; click:#add; wait:200" \
  cargo run --example todo --features ui-snapshot-png
```

Steps are separated by `;` or newlines.

| Step | Effect |
|------|--------|
| `key:ctrl+n` | One key event, in keybinding syntax |
| `type:hello world` | Literal text, one key event per character |
| `click:#submit` | Left click the centre of the widget keyed `submit` |
| `click:12,7` | Left click a cell |
| `rclick:` / `mclick:` | Right / middle click |
| `hover:#sidebar` | Move the pointer over a widget |
| `focus:#email` | Focus a widget directly |
| `focus:next` / `focus:prev` | Move focus one step |
| `scroll:#list,down` | Scroll over a widget (`up` / `down`) |
| `scroll:down` | Scroll at the current pointer position |
| `drag:#card>#column` | Press, move, release |
| `wait:500` | Advance the clock 500ms, ticking animations |

**Target widgets by key, not by coordinate.** `#submit` resolves through the
current tree to that widget's rect and clicks its centre, so it survives the
widget moving and **fails loudly** when the key is absent:

```
Error: no widget with key `does-not-exist` is currently rendered
```

A coordinate cannot do that - a layout change silently turns `click:42,7` into a
click on empty space while the script still reports success. Coordinates remain
available for what keys cannot express.

Give widgets stable keys (`.key("add")`) to make them scriptable; the keys a
running app exposes are listed in any markdown snapshot.

In code, `Recording::script(...)` takes the same syntax, and `Recording::keys(...)`
remains the shorthand for the typing-only case.

### Live control channel

`TUI_LIPAN_CONTROL=<path>` makes a running app listen on a Unix socket, so an
agent can inspect and drive a live TUI the way a browser tool drives a page:
snapshot, pick a widget by key, act, look again.

```sh
TUI_LIPAN_CONTROL=/tmp/app.sock cargo run --example todo
```

Requests are single `\n`-terminated lines:

| Command | Reply |
|---------|-------|
| `ping` | `pong` |
| `keys` | Newline-separated reconciliation keys currently rendered |
| `snapshot` | Markdown snapshot |
| `snapshot json` | JSON snapshot (needs `ui-snapshot-json`) |
| `snapshot png <path>` | Writes a PNG, replies with the path (needs `ui-snapshot-png`) |
| `act <script>` | Runs an action script; empty payload on success |
| `highlight <key>` | Outlines a widget; replies with the resolved rect |
| `highlight <col>,<row>` | Outlines the smallest widget covering a cell |
| `highlight clear` | Removes the outline |
| `quit` | Asks the app to exit |

Replies are a status line plus exactly that many bytes:

```text
ok <byte-length>\n<payload>
err <byte-length>\n<message>
```

Length prefixing keeps payloads newline- and binary-safe without escaping, so a
client is a few lines in any language. `keys` is the index of what `act` can
target - the equivalent of a browser tool's element refs.

`highlight` is an inspector marker, drawn over the finished frame in magenta. It
does not depend on the widget styling itself for hover or focus, so it marks
anything - including widgets with no interactive styling at all. Large rects are
outlined so the content underneath stays readable; rects one or two cells thick
are filled, having no interior to preserve. The outline reaches captures as well
as the live paint, so `snapshot png` shows what the operator sees.

Cell targeting resolves to the **smallest** widget covering that cell rather than
the topmost, which lands on the leaf a user would say they are pointing at
instead of the panel containing it. That is how unkeyed widgets stay
inspectable.

**Notes:**

- Unix only. The socket is created `0600`, because anything that can reach it can
  type into your application. Do not place it on a shared filesystem.
- `AF_UNIX` paths are limited to about 100 bytes; a long path fails to bind.
- Connections are served one at a time - the UI is a single shared surface.
- Runtime state stays single-threaded: the listener thread queues requests and
  the event loop answers them, the same pattern the terminal reader uses.

### Design sketches

`Sketch` renders a view at one or more viewports and writes every artifact in a
single call, so a design capture is small enough to keep in the repository
instead of being written and deleted:

```rust
use tui_lipan::{Result, Sketch};

Sketch::view("login", login_screen)   // any Fn() -> Element
    .viewport(80, 24)
    .fit(20, 8)                       // content minimum + margin
    .focus_next(1)                    // visible focus chrome
    .write()?;
```

| Method | Effect |
|--------|--------|
| `Sketch::view(name, fn)` | Sketch a plain `Fn() -> Element` (mounted through `Mockup`) |
| `Sketch::component(name, c)` | Sketch a `Component` with default properties |
| `viewport(w, h)` | Capture at an exact size; repeat for breakpoints |
| `fit(margin_w, margin_h)` | Capture at content minimum size plus margin |
| `focus_next(n)` | Advance focus `n` times before capturing |
| `options(opts)` | Describe options, e.g. `UiSnapshotOptions::diagnostic()` |
| `keys(script)` | Dispatch a key script before capturing, e.g. `"tab,enter"` |
| `markdown(b)` / `png(b)` / `json(b)` | Toggle formats; markdown and PNG default on |
| `dir(path)` | Output directory override |
| `baseline(dir)` | Compare each capture against a stored baseline image |
| `tolerance(ratio)` | Max fraction of differing pixels still counted as a match (default `0.0`) |
| `quiet(b)` | Suppress printing written paths |
| `write()` | Run every pass; returns `Result<Vec<PathBuf>>` |
| `check()` | Run and return `Vec<BaselineComparison>` |
| `assert_baseline()` | Run and fail if any capture regressed |

With no explicit viewport, `Sketch` captures `80x24` plus a fit-to-content pass -
the pairing that exposes flex-distribution bugs a single viewport hides. Output
defaults to `target/ui-sketches/` (override with `TUI_LIPAN_SKETCH_DIR`), so
sketches need no `.gitignore` entry.

Keep sketches in `examples/sketches/`; see that directory's `main.rs` for how new
ones register without a `Cargo.toml` change.

### Visual regression baselines

A kept sketch only protects against regressions if something notices when the
picture changes. `baseline(dir)` stores one PNG per capture, compares the next
render against it pixel by pixel, and writes a highlighted `*.diff.png` beside
any baseline that changed - unchanged pixels dimmed for context, changed pixels
in magenta.

```rust
#[test]
fn login_screen_has_not_drifted() -> Result<()> {
    Sketch::view("login", login_screen)
        .viewport(80, 24)
        .baseline("tests/ui-baselines")
        .assert_baseline()
}
```

The first run records baselines and passes. Later runs fail with every changed
capture listed at once, each naming its diff image. Accept new output with:

```sh
TUI_LIPAN_UPDATE_BASELINES=1 cargo test
```

**Baseline captures force `PngTextRenderer::Bitmap`.** The default `Auto`
renderer picks whichever system font it discovers, so the same UI produces
different pixels on CI than on a laptop and comparison becomes meaningless. The
built-in bitmap font ships with the crate, so it renders identically everywhere.
Font-rendered artifacts are still written for human review - they are simply not
what gets compared.

`BaselineOutcome` distinguishes `Created` (first run, not a failure), `Match`,
`Updated`, `Changed` (with pixel counts, ratio, and diff path), and `SizeChanged`
(dimensions differ, so pixels cannot be compared). `is_regression()` is the
single check for whether an outcome should fail a build.

Prefer removing nondeterminism over raising `tolerance`; a tolerance that hides
a real change is worse than no baseline.

---

## Key Attribute (Reconciliation)

Assign stable keys to preserve state across re-renders and enable focus routing:

```rust
rsx! {
    List { key: "file-list", ... }
    Input { key: format!("input-{}", id), ... }
}
```

> Without a key, reconciliation uses position, which breaks when items are added/removed.
