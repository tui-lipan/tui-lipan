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

High-frequency widget callbacks such as `ScrollView::on_viewport_change`,
`on_scroll`, drag updates, and cursor/selection sync should usually return
`Update::none()` when they only store the reported offset or selection in parent
state. Returning `Update::full()` from those paths can rebuild large trees on
every wheel tick or drag frame.

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
| `ctx.has_focus_within_key(key)` | Check if focus is within a subtree |
| `ctx.text_area_scrollbars(key)` | Read resolved vertical/horizontal scrollbar visibility for a keyed `TextArea` from the previous frame |
| `ctx.has_focus_within_scope(id)` | Check focus within a scope |
| `ctx.toast()` | Show toast notifications |
| `ctx.clipboard()` | Programmatic clipboard access (copy/read) |
| `ctx.quit()` | Exit the application |
| `ctx.is_inline()` | Whether running in inline mode |
| `ctx.command_chord_pending()` | Whether an app command chord is currently pending completion (e.g., after a leader prefix key) |
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

Built-in DevTools panel layout is fixed; `Context` methods control visibility only.

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
        Update { dirty: false, command: Some(cmd) }
    }
}
```

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
            .title("Panel")
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
| `to_png(&PngOptions)` | `Vec<u8>` | PNG bytes with font-backed or bitmap rendering (`ui-snapshot-png`) |
| `try_to_png(&PngOptions)` | `Result<Vec<u8>>` | PNG bytes with encoder errors surfaced (`ui-snapshot-png`) |

`CapturedCell` fields: `symbol`, `fg`, `bg`, `underline_color`, `modifiers` (`CellModifiers` with bool fields `bold`, `dim`, `italic`, `underline`, `reverse`, `strikethrough`).

### Agent / design-review snapshots

`TestBackend::capture_ui_snapshot()` returns a `UiSnapshot`: rendered
`CapturedFrame` plus semantic `UiWidgetDesc` entries (widget kind, keys, rects,
focus/hover, selection, values). Use `to_markdown()` for agent-readable reports.
Enable the `ui-snapshot-json` feature for `to_json()` / `to_json_pretty()`.
Enable `ui-snapshot-png` for `to_png()` / `to_png_default()` or `try_to_png()` /
`try_to_png_default()` when layout, color, focus chrome, and visual hierarchy
matter; PNG complements markdown/JSON rather than replacing them.

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
std::fs::write("/tmp/ui-snapshot.png", snapshot.to_png_default())?;
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
