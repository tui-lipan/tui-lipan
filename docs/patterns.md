# Patterns & Anti-Patterns

## Common Patterns

### 1. View Function Extraction (Mockup → App Workflow)

Extract reusable view functions so the same code works in both mockup previews and real components:

```rust
// View functions take plain data, return Element
fn sidebar(items: &[&str], selected: usize) -> Element {
    Frame::new()
        .title("Nav").border(true).width(Length::Px(28))
        .child(List::new().items(items.iter().map(|s| ListItem::new(*s))).selected(selected))
        .into()
}

// Step 1: preview with zero boilerplate
fn main() -> tui_lipan::Result<()> {
    let items = vec!["Home", "Settings"];
    mockup!("Preview", { sidebar(&items, 0) })
}

// Step 2: reuse in real component - no changes needed
fn view(&self, ctx: &Context<Self>) -> Element {
    sidebar(&ctx.state.items, ctx.state.selected)
}
```

---

### 2. Reusable Panel Shells (Parameterized UI)

When multiple panels share the same chrome but differ in title, status, or body,
extract the shared shell into plain Rust helpers. The parameters come from Rust,
while `rsx!` stays focused on composition.

```rust
fn app_panel(title: impl Into<Arc<str>>, child: impl Into<Element>) -> Element {
    Frame::new()
        .title(title)
        .border(true)
        .padding(1)
        .child(child)
        .into()
}

fn stats_panel(title: &str, value: &str) -> Element {
    app_panel(
        title,
        rsx! {
            Text {
                content: value,
            }
        },
    )
}

fn view(&self, _ctx: &Context<Self>) -> Element {
    rsx! {
        HStack {
            stats_panel("CPU", "42%"),
            stats_panel("Memory", "1.2 GB"),
        }
    }
}
```

This is the preferred pattern when:
- the reusable piece is mostly view/chrome
- there is no local state or message handling
- you want to share the same UI between mockups, examples, and real components

If you want a more structured API, wrap the same idea in a small composite widget:

Composite widget recipe:
- define a `#[derive(Clone)]` props/builder struct;
- store child `Element`s where the composite accepts arbitrary content;
- implement `From<MyComposite> for Element`;
- build the returned tree from existing primitives such as `Frame`, `VStack`,
  `HStack`, `Text`, and `Button`.

```rust
#[derive(Clone)]
struct AppPanel {
    title: Arc<str>,
    child: Element,
}

impl AppPanel {
    fn new(title: impl Into<Arc<str>>, child: impl Into<Element>) -> Self {
        Self {
            title: title.into(),
            child: child.into(),
        }
    }
}

impl From<AppPanel> for Element {
    fn from(panel: AppPanel) -> Element {
        Frame::new()
            .title(panel.title)
            .border(true)
            .padding(1)
            .child(panel.child)
            .into()
    }
}
```

Use this when the same shell needs a named, reusable API (`AppPanel::new(...)`,
`.status(...)`, `.footer(...)`, etc.).

Choose the abstraction level by behavior:
- **Helper function returning `Element`** - best for simple reusable shells
- **Composite widget struct** - best for reusable builder-style UI with several options
- **`Component` with `Properties`** - best when the reusable panel has its own state or events

---

### 3. Numbered List Rows

Prefer `ListItem::numbered(...)` or `ListItem::bulleted(...)` instead of embedding
the prefix in the label string yourself. Extra lines automatically indent under the
label text.

```rust
List::new().items([
    ListItem::new("Alpha")
        .numbered(1)
        .line(ListItemLine::new("first option")),
    ListItem::new("Beta")
        .numbered(2)
        .line(ListItemLine::new("second option")),
])
```

---

### 4. Background Command Pattern

Offload blocking I/O to a background thread:

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::LoadUser(id) => {
            ctx.state.loading = true;
            let cmd = ctx.link().command(move |link| {
                let user = db::get_user(id);   // blocking
                link.send(Msg::UserLoaded(user));
            });
            Update::with_command(cmd)
        }
        Msg::UserLoaded(user) => {
            ctx.state.loading = false;
            ctx.state.user = Some(user);
            Update::full()
        }
    }
}
```

If the blocking work is an **interactive subprocess** that needs the real TTY (`$EDITOR`, a pager), do **not** use `link.command` / `Command::spawn` for that phase: use `Command::new` on the UI thread with [`terminal_handoff`](external-programs.md), then [`request_full_repaint()`](external-programs.md#force-a-full-redraw-after-handoff) as needed. See **[External programs](external-programs.md)**.

---

### 4. Keyed Coalescing (Filter-as-You-Type)

Avoid stale search results by using `TaskPolicy::LatestOnly`:

```rust
Msg::QueryChanged(q) => {
    ctx.state.query = q.clone();
    let cmd = ctx.link().command_keyed(
        "search",
        TaskPolicy::LatestOnly,
        move |link| {
            let results = search_items(&q);
            link.send(Msg::SearchDone(results));
        },
    );
    Update::with_command(cmd)  // Redraw to show query immediately
}
```

---

### 5. Controlled Scroll with Synced State

For high-frequency scroll callbacks, separate widget-owned runtime state from
visual state. Mirroring the latest offset usually should not rebuild the parent
view.

```rust
// Controlled list with parent-managed offset
List::new()
    .items(self.items.clone())
    .selected(self.selected)
    .on_scroll_to(ctx.link().callback(Msg::Scrolled))

// In update:
Msg::Scrolled(offset) => {
    ctx.state.scroll_offset = offset;
    Update::none()
}
```

For `ScrollView::on_viewport_change`, return `Update::none()` when only the
stored offset changed. Return `Update::layout()` only when visible metadata
changes something already in the mounted subtree, such as a sticky header label.
Reserve `Update::full()` for cases where the component's `view()` output really
changes.

---

### 6. Memoized Heavy Child Components

When a parent rerenders often but some child panels/rows are expensive to rebuild, move those
children into their own `Component` and give them a stable `memo_key()`:

```rust
impl Component for MessageRow {
    type Message = RowMsg;
    type Properties = MessageRowProps;
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn memo_key(&self, props: &Self::Properties, _ctx: &Context<Self>) -> Option<u64> {
        Some(props.revision)
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        render_message_row(ctx.props)
    }

    fn update(&mut self, _msg: RowMsg, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
}
```

Use this when:
- the subtree is expensive to build
- parent updates are frequent but mostly unrelated to the child
- the child's visual output is driven by a clear semantic revision key

Avoid using `memo_key()` as a manual diff cache for everything. If the subtree is cheap, a plain
view helper is simpler.

---

### 7. Focus Routing

Navigate between panels with `ctx.request_focus`:

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::FocusSidebar => { ctx.request_focus("sidebar"); Update::none() }
        Msg::FocusEditor  => { ctx.request_focus("editor");  Update::none() }
    }
}

fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
    match key.code {
        KeyCode::Char('1') if key.mods.ctrl => {
            ctx.request_focus("sidebar");
            KeyUpdate::handled(Update::none())
        }
        KeyCode::Char('2') if key.mods.ctrl => {
            ctx.request_focus("editor");
            KeyUpdate::handled(Update::none())
        }
        _ => KeyUpdate::unhandled(Update::none())
    }
}
```

---

### 8. Conditional Overlay Pattern

Show modals/overlays via state flags (not routing):

```rust
// State
struct State {
    show_delete_confirm: bool,
    selected_item: usize,
}

// View
fn view(&self, ctx: &Context<Self>) -> Element {
    rsx! {
        VStack {
            List { items: ..., selected: ctx.state.selected_item }

            if ctx.state.show_delete_confirm {
                Modal {
                    title: "Confirm Delete",
                    VStack {
                        gap: 1,
                        Text { content: "This cannot be undone." }
                        HStack {
                            gap: 1,
                            Button {
                                label: "Cancel",
                                on_click: ctx.link().callback(|_| Msg::CancelDelete),
                            }
                            Button {
                                label: "Delete",
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

### 9. Toast from Update

```rust
Msg::SaveSuccess => {
    ctx.toast().push(Toast::new("File saved!"));
    Update::none()
}
Msg::SaveError(e) => {
    ctx.toast().push(Toast::new(format!("Error: {e}")).title("Error"));
    Update::none()
}
```

---

### 10. Dynamic Tab Management

```rust
struct State {
    tabs: Vec<String>,
    active: usize,
}

// In update:
Msg::OpenFile(path) => {
    ctx.state.tabs.push(path);
    ctx.state.active = ctx.state.tabs.len() - 1;
    Update::full()
}
Msg::CloseTab(idx) => {
    ctx.state.tabs.remove(idx);
    ctx.state.active = ctx.state.active.min(ctx.state.tabs.len().saturating_sub(1));
    Update::full()
}
Msg::ReorderTabs(e) => {
    ctx.state.tabs.swap(e.from, e.to);
    ctx.state.active = if ctx.state.active == e.from { e.to } else { ctx.state.active };
    Update::full()
}
```

---

### 11. Markdown Editor + Live Preview

Use `TextArea` for editing and `DocumentView` for display transforms. Keep the
source in one state field and feed both widgets from the same value.

```rust
HStack::new()
    .gap(1)
    .child(
        TextArea::new(ctx.state.markdown.clone())
            .language("markdown")
            .on_change(ctx.link().callback(|ev| Msg::SetMarkdown(ev.value))),
    )
    .child(
        DocumentView::new(ctx.state.markdown.clone())
            .markdown()          // requires feature "markdown"
            .line_numbers(true)
            .wrap(true),
    )
```

For scroll-sync, emit source-line metrics from editor/preview and set
`DocumentView::scroll_to_source_line(...)`.

### 11a. TextArea inline sentinels (@mentions, file tokens)

Store app data on `TextAreaSentinel::payload(your_type)` and rely on `SentinelId` + `on_sentinel_event` for deletes instead of mirroring index → metadata maps. Use `TextAreaSnapshot::capture` / `apply` for stash–restore. See [`widgets/input.md`](widgets/input.md#custom-inline-sentinels-extmarks).

---

### 12. Choosing the Right Container

| Need | Use | Don't use |
|------|-----|-----------|
| Stack children vertically | `VStack` | `Frame` (unless you need border/title) |
| Stack children horizontally | `HStack` | `Frame` (unless you need border/title) |
| Border + title + status line | `Frame` | bare VStack with manual border |
| Overlay children on top of each other | `ZStack` | - |
| Center a single child | `Center` | VStack with align + justify hacks |
| Scrollable content | `ScrollView` | - |
| Resizable split panes | `Splitter` | - |

**`Frame` is for visual chrome, not layout.** Use `Frame` only when you need one or
more of: border, title, status text, tab titles, join_frame, clipping, or `FrameDecoration`.
If you just need to group children, use `VStack` or `HStack` directly.

---

## Anti-Patterns (Common Mistakes)

### ❌ Returning `Update::full()` when nothing changed

```rust
// BAD: triggers unnecessary re-render
Msg::SomeEvent => Update::full()

// GOOD: only redraw if state actually changed
Msg::SomeEvent => {
    if self.process(msg) {
        Update::full()
    } else {
        Update::none()
    }
}
```

This is especially expensive for high-frequency widget callbacks. Avoid full
root rebuilds when callbacks only synchronize runtime metadata:

```rust
// BAD: rebuilds the whole component tree on every wheel/drag event
Msg::ViewportChanged(event) => {
    ctx.state.scroll_offset = event.offset;
    Update::full()
}

// GOOD: the ScrollView already moved; just remember the settled offset
Msg::ViewportChanged(event) => {
    ctx.state.scroll_offset = event.offset;
    Update::none()
}
```

---

### ❌ Missing `.key(...)` on list items or dynamic children

```rust
// BAD: reconciliation uses position - breaks on insert/remove
rsx! {
    VStack {
        for item in &items {
            ItemWidget { data: item.clone() }
        }
    }
}

// GOOD: stable key for each child
rsx! {
    VStack {
        for item in &items {
            ItemWidget { key: item.id.to_string(), data: item.clone() }
        }
    }
}
```

---

### ❌ Blocking the main thread in `update()` or `view()`

```rust
// BAD: blocks the render loop
Msg::LoadData => {
    let data = std::fs::read_to_string("data.json").unwrap();  // BLOCKS
    ctx.state.data = data;
    Update::full()
}

// GOOD: use a command
Msg::LoadData => {
    Update::with_command(ctx.link().command(|link| {
        let data = std::fs::read_to_string("data.json").unwrap();
        link.send(Msg::DataLoaded(data));
    }))
}
```

---

### ❌ Using `TaskPolicy::QueueAll` for search/filter

```rust
// BAD: stale results pile up; each keystroke queues a new search
ctx.link().command_keyed("search", TaskPolicy::QueueAll, ...)

// GOOD: drop old pending searches
ctx.link().command_keyed("search", TaskPolicy::LatestOnly, ...)
```

---

### ❌ Forgetting to drain `screen.drain_responses()`

TUI apps like `fzf`, `vim`, and `lazygit` query terminal capabilities. Not forwarding responses causes them to hang or malfunction.

```rust
// BAD: missing response forwarding
screen.process_bytes(&bytes);
let snapshot = screen.render_snapshot();

// GOOD: always drain and forward
screen.process_bytes(&bytes);
if let Some(pty) = &self.pty {
    for response in screen.drain_responses() {
        let _ = pty.write(&response);
    }
}
let snapshot = screen.render_snapshot();
```

---

### ❌ Setting scroll offset before resize in inline mode

```rust
// BAD: corrupts ratatui's internal cursor offset calculation
terminal.set_cursor_position((0, 0));  // before draw on resize
terminal.draw(...);

// GOOD: use backend_mut() for operations that must NOT touch ratatui state
terminal.backend_mut().execute(cursor_op)?;
```

---

### ❌ Missing `PartialEq` on Properties

Properties must implement `Clone + PartialEq` for reconciliation to work. Without `PartialEq`, props changes may not trigger `on_props_changed`.

```rust
// BAD
#[derive(Clone)]
struct Props { value: String }

// GOOD
#[derive(Clone, PartialEq)]
struct Props { value: String }
```

---

### ❌ Accessing image clipboard without `image` feature

```rust
// BAD: will not compile without feature "image"
use tui_lipan::ImageContent;  // Only available with feature "image"

// Check Cargo.toml: features = ["image"]
```

---

### ❌ Using `Color::named()` for precise contrast

```rust
// BAD: Color::Gray varies between terminal palettes
Button::new("Click").focus_style(Style::new().bg(Color::Gray))

// GOOD: use explicit RGB for predictable contrast behavior
Button::new("Click").focus_style(Style::new().bg(Color::rgb(80, 80, 80)))
```

---

### ❌ Setting default values explicitly

Don't set properties that are already the default - it adds noise and signals misunderstanding of the framework. Common offenders:

```rust
// BAD: All of these are no-ops (they set the default value)
Input::new(query.clone())
    .caret_shape(CaretShape::Block)    // Block is the default
    .focusable(true)                   // interactive widgets are focusable by default

VStack::new()
    .width(Length::Flex(1))            // containers default to Flex(1)
    .height(Length::Flex(1))           // containers default to Flex(1)
    .align(Align::Start)              // Start is the default
    .justify(Justify::Start)          // Start is the default

App::new()
    .mouse(true)                      // true in fullscreen mode by default
    .contrast_policy(ContrastPolicy::Wcag)  // Wcag is the default

// GOOD: Only set properties when you want non-default values
Input::new(query.clone())
    .caret_shape(CaretShape::Bar)     // Bar is NOT the default - this IS meaningful

VStack::new()
    .align(Align::Center)             // Center is NOT the default - this IS meaningful
```

---

### ❌ Setting background color on every widget and style variant

Background color inherits from the nearest ancestor. Don't repeat it on every child or sub-style:

```rust
// BAD: Repeating bg on every widget and sub-style
Frame::new()
    .style(Style::new().bg(Color::indexed(235)))
    .child(
        Input::new(query.clone())
            .style(Style::new().fg(Color::White).bg(Color::indexed(235)))
            .focus_style(Style::new().fg(Color::White).bg(Color::indexed(235)).bold())
    )
    .child(
        List::new()
            .items(items)
            .selection_style(Style::new().fg(Color::Cyan).bg(Color::indexed(235)))
    )

// GOOD: Set bg once on the parent; children inherit it.
// Only set bg when you want a DIFFERENT background.
Frame::new()
    .style(Style::new().bg(Color::indexed(235)))
    .child(
        Input::new(query.clone())
            .style(Style::new().fg(Color::White))
            .focus_style(Style::new().fg(Color::White).bold())
    )
    .child(
        List::new()
            .items(items)
            .selection_style(Style::new().fg(Color::Cyan).bold())
    )
```

See [`styling.md`](./styling.md) for the full style inheritance rules.

> **Note**: Only `bg` inherits this way. `fg` does **not** inherit - each widget resolves its own foreground color independently.

---

### ❌ Wrapping containers in `Frame` unnecessarily

`Frame` is for visual chrome (border, title, status text, clipping). If you just need layout, use `VStack`/`HStack` directly:

```rust
// BAD: Frame adds no value here - no border, title, or status used
Frame::new()
    .child(
        VStack::new()
            .child(Text::new("Hello"))
            .child(Button::new("Click"))
    )

// GOOD: VStack alone provides the same layout
VStack::new()
    .child(Text::new("Hello"))
    .child(Button::new("Click"))

// GOOD: Frame is justified when you use its features
Frame::new()
    .title("Settings")
    .border(true)
    .status("v1.2")
    .child(
        VStack::new()
            .gap(1)
            .child(Text::new("Hello"))
            .child(Button::new("Click"))
    )
```
