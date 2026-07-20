# Tutorial: Build a Complete App

This tutorial walks through building a realistic multi-panel application step by step.
By the end you'll understand components, state, messages, nested components, focus routing,
async commands, overlays, and toasts - everything needed to build production TUI apps.

---

## Step 1: Skeleton - Single Component

Start with the minimal `Component` skeleton:

```rust
use tui_lipan::prelude::*;

struct App;

#[derive(Default)]
struct State {
    items: Vec<String>,
    selected: usize,
    detail: String,
}

#[derive(Clone)]
enum Msg {
    Select(usize),
    ItemsLoaded(Vec<String>),
}

impl Component for App {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> State { State::default() }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Text::new("Hello").into()
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        Update::full()
    }
}

fn main() -> tui_lipan::Result<()> {
    tui_lipan::App::new().title("My App").mount(App).run()
}
```

**Key points:**
- `State` holds all mutable data. Use `#[derive(Default)]`.
- `Msg` enumerates every event. Use `#[derive(Clone)]`.
- `create_state` initializes state from properties.
- `view` returns the UI tree. Called on every re-render.
- `update` handles messages. Returns `(needs_redraw, optional_command)`.

---

## Step 2: Layout - Panels with Focus

Build a master-detail layout with focus-aware borders:

```rust
fn view(&self, ctx: &Context<Self>) -> Element {
    let sidebar_focused = ctx.has_focus_within_key("sidebar");
    let detail_focused = ctx.has_focus_within_key("detail");

    HStack::new()
        .gap(0)
        .child(
            Frame::new()
                .title("Items")
                .border(true)
                .border_style(if sidebar_focused { BorderStyle::Thick } else { BorderStyle::Rounded })
                .width(Length::Px(30))
                .child(
                    List::new()
                        .key("sidebar")
                        .items(ctx.state.items.iter().map(|s| ListItem::new(s.clone())))
                        .selected(ctx.state.selected)
                        .scrollbar(true)
                        .selection_full_width(true)
                        .on_select(ctx.link().callback(|e: ListEvent| Msg::Select(e.index)))
                        .on_activate(ctx.link().callback(|e: ListEvent| Msg::Activate(e.index)))
                ),
        )
        .child(
            Frame::new()
                .title("Detail")
                .border(true)
                .border_style(if detail_focused { BorderStyle::Thick } else { BorderStyle::Rounded })
                .padding(1)
                .child(
                    Text::new(ctx.state.detail.clone()).key("detail")
                ),
        )
        .into()
}
```

**Key points:**
- `.key("sidebar")` assigns a stable identity for focus routing and reconciliation.
- `ctx.has_focus_within_key("sidebar")` queries whether focus is within that subtree.
- `BorderStyle::Thick` vs `BorderStyle::Rounded` gives visual focus cues.
- `ListEvent { index }` is the payload for `on_select` and `on_activate`.

> **Note**: We use `Frame` here because we need borders and titles to visually
> distinguish panels. If you don't need visual chrome (border, title, status),
> use `VStack`/`HStack` directly - they are lighter containers with the same
> layout behavior. See [`patterns.md`](patterns.md) for container selection guidance.

---

## Step 3: Handle Messages

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::Select(idx) => {
            ctx.state.selected = idx;
            ctx.state.detail = format!("Selected: {}", ctx.state.items[idx]);
            Update::full()
        }
        Msg::Activate(idx) => {
            ctx.state.detail = format!("Activated: {}", ctx.state.items[idx]);
            ctx.request_focus("detail");  // Move focus to detail panel
            Update::full()
        }
        Msg::ItemsLoaded(items) => {
            ctx.state.items = items;
            ctx.state.selected = 0;
            ctx.toast().push(Toast::new("Items loaded!"));
            Update::full()
        }
    }
}
```

**Key points:**
- Return `Update::full()` to redraw without spawning background work.
- Return `Update::none()` when no visual change occurred.
- `ctx.request_focus("detail")` moves focus to a keyed widget on next render.
- `ctx.toast().push(Toast::new(...))` shows a toast notification.

---

## Step 4: Async Data Loading

Load data on startup via `init()` and `Command`:

```rust
fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
    Some(ctx.link().command(move |link| {
        // Runs on a background thread - safe to block here
        let items: Vec<String> = (1..=50)
            .map(|i| format!("Item {i}"))
            .collect();
        std::thread::sleep(std::time::Duration::from_millis(500));
        link.send(Msg::ItemsLoaded(items));
    }))
}
```

**Key points:**
- `init()` runs once when the component mounts.
- `ctx.link().command(...)` spawns a background task.
- `link.send(Msg::...)` sends a message back to the component's update loop.
- The component itself never needs `Send` or `Sync`.

---

## Step 5: Key Event Handling

Add global keyboard shortcuts via `on_key`:

```rust
fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
    match key.code {
        KeyCode::Char('q') if key.mods.ctrl => {
            ctx.quit();
            KeyUpdate::handled(Update::full())
        }
        KeyCode::Char('1') if key.mods.ctrl => {
            ctx.request_focus("sidebar");
            KeyUpdate::handled(Update::none())
        }
        KeyCode::Char('2') if key.mods.ctrl => {
            ctx.request_focus("detail");
            KeyUpdate::handled(Update::none())
        }
        KeyCode::Char('r') if key.mods.ctrl => {
            // Reload data
            let cmd = ctx.link().command(move |link| {
                let items: Vec<String> = (1..=50).map(|i| format!("Item {i}")).collect();
                link.send(Msg::ItemsLoaded(items));
            });
            KeyUpdate::handled(Update::with_command(cmd))
        }
        _ => KeyUpdate::unhandled(Update::none())
    }
}
```

**Key points:**
- `KeyUpdate::handled(update)` stops the key from bubbling further.
- `KeyUpdate::unhandled(update)` lets the key continue to parent components.
- `key.mods.ctrl`, `key.mods.alt`, `key.mods.shift` are boolean flags.
- You can return a `Command` from `on_key` too (via the `Update` tuple).

---

## Step 6: Conditional Overlays (Modal)

Show a confirmation dialog via state flag:

```rust
#[derive(Default)]
struct State {
    // ... existing fields ...
    show_confirm: bool,
    pending_delete: Option<usize>,
}

#[derive(Clone)]
enum Msg {
    // ... existing variants ...
    RequestDelete(usize),
    ConfirmDelete,
    CancelDelete,
}

fn view(&self, ctx: &Context<Self>) -> Element {
    rsx! {
        VStack {
            // ... main content ...

            if ctx.state.show_confirm {
                Modal {
                    title: "Confirm Delete",
                    VStack {
                        gap: 1,
                        padding: 1,
                        Text { content: "Are you sure?" }
                        HStack {
                            gap: 1,
                            Button {
                                label: "Cancel",
                                on_click: ctx.link().callback(|_| Msg::CancelDelete),
                            }
                            Button {
                                label: "Delete",
                                style: Style::new().fg(Color::Red),
                                on_click: ctx.link().callback(|_| Msg::ConfirmDelete),
                            }
                        }
                    }
                }
            }
        }
    }
}
```

---

## Step 7: Keyed Task Coalescing (Search)

For filter-as-you-type, use `command_keyed` with `TaskPolicy::LatestOnly`:

```rust
Msg::QueryChanged(q) => {
    ctx.state.query = q.clone();
    let cmd = ctx.link().command_keyed(
        "search",
        TaskPolicy::LatestOnly,   // Drop stale searches
        move |link| {
            let results = expensive_search(&q);
            link.send(Msg::SearchDone(results));
        },
    );
    Update::with_command(cmd)
}
```

| Policy | Behavior |
|--------|----------|
| `QueueAll` | Run every task sequentially |
| `DropIfRunning` | Ignore new task while one with same key is running |
| `LatestOnly` | Keep only the newest pending task; drop older ones |

---

## Step 8: Nested Components

For complex apps, split into nested components with `child()`:

```rust
use tui_lipan::child;

// Parent passes data via Properties, gets results via callbacks
#[derive(Clone, PartialEq)]   // Properties MUST be Clone + PartialEq
struct SidebarProps {
    items: Vec<String>,
    selected: usize,
    on_select: Callback<usize>,
}

struct Sidebar;

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
                ctx.props.on_select.emit(idx);  // Notify parent
                Update::none()  // Parent will redraw with new props
            }
        }
    }
}

// In parent's view():
fn view(&self, ctx: &Context<Self>) -> Element {
    HStack::new()
        .child(child(
            || Sidebar,
            SidebarProps {
                items: ctx.state.items.clone(),
                selected: ctx.state.selected,
                on_select: ctx.link().callback(Msg::Select),
            },
        ))
        .child(/* detail panel */)
        .into()
}
```

**Key points:**
- Properties must implement `Clone + PartialEq` for reconciliation.
- Communication is **parent → child** via props, **child → parent** via callback props.
- Children don't directly access parent state. Messages are scoped.
- `child(|| Sidebar, props)` takes a factory closure, not just a type.

---

## Complete App Configuration

```rust
fn main() -> tui_lipan::Result<()> {
    tui_lipan::App::new()
        .title("My App")                        // Chrome frame title
        .theme(Theme::one_dark())               // Theme preset
        .mouse(true)                            // Mouse capture (default: true)
        .toast_placement(ToastPlacement::BottomEnd)
        .contrast_policy(ContrastPolicy::Wcag)  // WCAG-fix low-contrast text
        .mount(App)
        .run()
}
```

Common themes: `default()`, `one_dark()`, `dracula()`, `nord()`,
`gruvbox_dark()`, `catppuccin_mocha()`, `tokyo_night()`, `rose_pine()`,
`kanagawa()`, `solarized_dark()`, `monokai()`, plus light variants such as
`solarized_light()` and `catppuccin_latte()`. See
[Styling](./styling.md#named-presets) for the full list.
