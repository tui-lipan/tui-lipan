# Focus system

Keyboard **routing** in the component tree (bubbling, `on_key`) lives here. **Configurable bindings**, `keymap.conf`, chord parsing, and `TextArea` newline keys are documented in [`keybindings.md`](keybindings.md).

## Focus basics

Widgets receive focus if they have `focusable: true` (default for interactive widgets like `Input`, `List`, `Button`, etc.). Non-interactive widgets (`Text`, `Frame`) are not focusable by default.

## Tab traversal

| Key | Action |
|-----|--------|
| `Tab` | Move to next focusable element |
| `Shift+Tab` | Move to previous focusable element |
| Click | Focus clicked element |

Default keys follow the built-in keymap; you can remap `focus_next` / `focus_prev` in `keymap.conf` (see [`keybindings.md`](keybindings.md)).

## Programmatic focus

Control focus from `update()`:

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::EditItem(id) => {
            ctx.state.editing = Some(id);
            ctx.request_focus(format!("input-{}", id));  // Move focus to keyed widget
            Update::full()
        }
        Msg::Save => {
            ctx.state.editing = None;
            ctx.request_focus("list");  // Return focus to list
            Update::full()
        }
    }
}

fn view(&self, ctx: &Context<Self>) -> Element {
    rsx! {
        List { key: "list", items: ctx.state.items.clone(), selected: ctx.state.selected }
        if let Some(id) = ctx.state.editing {
            Input { key: format!("input-{}", id), value: ctx.state.edit_value.clone() }
        }
    }
}
```

## Composite widget focus

Some widgets expose a single outer focus target while managing an internal active
item. `Graph` uses this pattern for node navigation: tab traversal enters and
leaves the graph as one focusable element, then arrow keys move a roving focused
node inside the rendered tree.

Static graphs remain unfocusable by default. Set `.focusable(true)` to opt in
explicitly, or attach `.on_node_focus(...)` / `.on_node_activate(...)` to opt in
implicitly. Pointer-only graph callbacks such as `.on_node_click(...)` do not
make the graph keyboard-focusable.

Graph keyboard bindings are direction-aware:

| Direction | Parent / first child | Siblings |
|-----------|----------------------|----------|
| `TopDown` | `Up` / `Down` | `Left` / `Right` |
| `LeftRight` | `Left` / `Right` | `Up` / `Down` |

`Enter` and `Space` activate the focused node. `Home` and `End` move to the
first and last rendered node.

## Focus state queries

In headless tests, use `TestBackend::focus_next()` / `focus_prev()` to drive the
same traversal and `TestBackend::focused_key()` to assert the focused keyed widget.

```rust
fn view(&self, ctx: &Context<Self>) -> Element {
    let sidebar_active = ctx.has_focus_within_key("sidebar");
    let editor_active = ctx.has_focus_within_key("editor");

    rsx! {
        HStack {
            Frame {
                title: "Sidebar",
                border: true,
                key: "sidebar",
                border_style: if sidebar_active { BorderStyle::Thick } else { BorderStyle::Rounded },
            }
            Frame {
                title: "Editor",
                border: true,
                key: "editor",
                border_style: if editor_active { BorderStyle::Thick } else { BorderStyle::Rounded },
            }
        }
    }
}
```

## Key event bubbling

Keyboard events bubble up the tree if unhandled by the focused widget. If no widget is focused, bubbling starts at the deepest mounted component scope and continues toward root:

1. **Focused widget** (e.g., `Input` handles typing)
2. **Parent components** up the tree (via `on_key`)
3. **Root component** `on_key`

`ScrollView` also has an explicit fallback for page navigation: if exactly one mounted scroll view sets `.ambient_page_scroll(true)`, `PageUp` / `PageDown` can target it even when that scroll view is not focused. This ambient fallback runs only after the normal focused-widget path, ancestor `ScrollView` bubbling, and component `on_key` bubbling all leave the page key unhandled.

```rust
impl Component for MyApp {
    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') if key.mods.ctrl => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            KeyCode::F(1) => {
                ctx.state.show_help = !ctx.state.show_help;
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none())
        }
    }
}
```

**`KeyEvent` fields:**
- `key.code: KeyCode` - `Char('a')`, `Enter`, `Esc`, `Tab`, `F(1)`, `Up`, `Down`, etc.
- `key.mods: KeyMods` - `ctrl`, `alt`, `shift`, `super_key` boolean flags

**`KeyUpdate`:**
- `KeyUpdate::handled(update)` - stop bubbling
- `KeyUpdate::unhandled(update)` - continue bubbling

## Focus policy (accordion)

`VStack` supports lazygit-style accordion sizing based on focus:

```rust
use tui_lipan::prelude::*;

VStack::new()
    .focus_policy(FocusPolicy::Accordion(FocusAccordion {
        focused_min: 10,
        collapsed: 1,
        ..Default::default()
    }))
    .child(Panel::new().key("panel-a"))
    .child(Panel::new().key("panel-b"))
```

Keyed children are required for focus protection (prevents focused panel from collapsing).

### Sticky accordion (remembering layout across focus changes)

By default, when focus moves outside the stack entirely (e.g. to a sibling column), the accordion deactivates and all panels revert to equal sizes. The `sticky` flag (default `true`) makes the VStack automatically remember the last focused child and keep it expanded even when the stack has no real focus - with zero boilerplate:

```rust
VStack::new()
    .focus_policy(FocusPolicy::Accordion(FocusAccordion {
        focused_min: 7,
        ..FocusAccordion::default()  // sticky: true by default
    }))
    .child(frame_a.key("panel-a"))
    .child(frame_b.key("panel-b"))
```

The VStack node persists the last focused child's key across frames. When focus leaves the stack, the accordion behaves as if the previously focused child still had focus - expanding it and collapsing others in squash/tiny modes. When real focus returns to any child the sticky state is updated and normal accordion rules apply.

**Requirements**: children must have unique keys (via `.key("...")`) for the sticky tracking to work.

To opt out of sticky behavior, set `sticky: false` explicitly:

```rust
FocusAccordion { sticky: false, ..FocusAccordion::default() }
```
