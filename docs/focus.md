# Focus

Focus controls keyboard routing, traversal, pointer focus, focus scopes, and focus-change
notifications. Configurable bindings and chords are documented in
[`keybindings.md`](keybindings.md).

## Focus Policy

Configure framework-initiated focus movement on `App`:

```rust
App::new()
    .focus_policy(FocusPolicy::OnDemand)
    .mount(Root)
```

| Policy | Startup and fallback | Tab and pointer focus | Explicit APIs |
|--------|----------------------|-----------------------|---------------|
| `OnDemand` | Starts unfocused; restores stable keyed targets but has no first-widget fallback. This is the default. | Enabled | Enabled |
| `Auto` | Focuses the first eligible widget at startup and when no prior target can be restored. | Enabled | Enabled |
| `Manual` | Starts unfocused; restores only an exact keyed target and never uses tag or first-widget fallback. | Global traversal and click-to-focus disabled | Enabled |

`Manual` disables framework-initiated movement, not focus itself. Calls to
`ctx.request_focus`, `ctx.focus_next`, and `ctx.focus_prev` still work. Capturing overlays are
the other deliberate exception: they auto-focus and trap by default even under `Manual`.

## Focusable Versus Tab Stop

These properties answer different questions:

| Property | Meaning |
|----------|---------|
| `focusable` | The widget may own focus and receive focused keyboard input. |
| `tab_stop` | The widget participates in next/previous traversal. Default: `true`. |

A widget with `.focusable(true).tab_stop(false)` is omitted from Tab traversal but remains
reachable by pointer focus and `ctx.request_focus(key)`. A non-focusable widget cannot become a
focus target. Incidental controls such as `Accordion`, `DraggableTabBar`, `Hyperlink`, `PanView`,
and `Tabs` are not focusable by default; opt in when their keyboard behavior belongs in the app's
focus ring. Input, editor, and primary data surfaces remain focusable by default.

```rust
Input::new(value)
    .key("search")
    .tab_stop(false)
```

## Traversal

The default framework bindings are:

| Input | Action |
|-------|--------|
| `Tab` | Move to the next tab stop |
| `Shift+Tab` | Move to the previous tab stop |
| Pointer press | Focus the eligible target before dispatching its interaction |

With no current focus, next traversal selects the first tab stop and previous traversal selects
the last. `Manual` leaves global Tab/Shift+Tab unhandled so widget, command, or component layers
can use those keys. A `TextArea` may consume Tab for insertion before framework traversal. Its
literal-tab display width is configured with `.tab_display_width(...)`, not `.tab_stop(...)`.

Remap or unbind `focus_next` and `focus_prev` in `keymap.conf`; see
[`keybindings.md`](keybindings.md).

## Programmatic Focus

Focus APIs are available from `update()` and `on_key()`:

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::Edit(id) => {
            ctx.state.editing = Some(id);
            ctx.request_focus(format!("editor-{id}"));
        }
        Msg::NextPane => ctx.focus_next(),
        Msg::PreviousPane => ctx.focus_prev(),
        Msg::CloseEditor => {
            ctx.state.editing = None;
            ctx.blur();
        }
    }
    Update::full()
}
```

`request_focus(key)` may be issued before the keyed widget mounts; the pending key resolves after
reconciliation. It is also the explicit escape hatch into `FocusScope::Exclude`.

Keying a **composite** widget keys its container, which is usually not focusable. `request_focus`
on such a key falls back to the container's first focusable descendant — which works until the
composite grows another focusable widget, at which point focus silently lands somewhere else.
Prefer a setter that keys the inner widget directly when one exists, such as
`SearchPalette::input_key`:

```rust
SearchPalette::<T>::new().input_key("command-query")
// ...
ctx.request_focus("command-query");
```

`blur()` clears both the current focus and retained focus identity. Under `OnDemand` and `Manual`,
the app remains unfocused until focus is established again. Under `Auto`, the next reconciliation
restores the default eligible target, so blur acts as a reset to automatic focus.

`TestBackend::focus_next()`, `focus_prev()`, `blur()`, and `focused_key()` expose the same behavior
for headless tests.

## OnDemand Retention And Remounts

`OnDemand` permits no current focus while retaining the key of a focused widget that temporarily
unmounts. If a widget with that key remounts, focus returns to it. This is continuity, not a
first-widget fallback. Use `ctx.blur()` when the old target must be forgotten permanently.

Stable keys are therefore important for dynamic focus targets:

```rust
if let Some(id) = ctx.state.editing {
    Input::new(ctx.state.value.clone()).key(format!("editor-{id}"))
} else {
    Element::empty()
}
```

When no keyed target can be restored, `OnDemand` may restore a surviving target of the same tag
within the reconciled tree, then otherwise remains unfocused. `Manual` skips that tag heuristic so
it cannot move focus to a different same-type widget.

## Focus Scopes

`VStack`, `HStack`, and `Frame` accept `.focus_scope(...)`:

| Scope | Behavior |
|-------|----------|
| `FocusScope::None` | Normal inherited behavior. Default. |
| `FocusScope::Exclude` | Removes the subtree from traversal, automatic fallback, descendant focus, and click-to-focus. Explicit keyed requests may enter it. |
| `FocusScope::Contain` | While focus is inside, next/previous traversal wraps within the nearest containing ancestor. The pane is also opaque from outside: Tab never enters it. |

```rust
HStack::new()
    .child(
        Frame::new()
            .focus_scope(FocusScope::Contain)
            .child(editor_pane),
    )
    .child(
        Frame::new()
            .focus_scope(FocusScope::Exclude)
            .child(read_only_preview),
    )
```

`Contain` is a pane trap, and the trap works in both directions: Tab from outside the pane will
not enter it, just as Tab from inside will not leave. A ring that could Tab *in* but not back
*out* would strand focus, so entering is deliberate — click the pane, `request_focus` a widget
in it, or bind an app-owned pane-switch key:

```rust
// Tab cycles within whichever pane holds focus; F6 moves between panes.
Frame::new().focus_scope(FocusScope::Contain).child(sidebar)
```

The same rule applies to nested panes: an inner `Contain` is not part of its parent's ring.

The pane's *boundary node* is the exception: a `Contain` frame that is itself focusable
(`.focusable(true)`) is a tab stop in the enclosing ring, so keyboard users can still reach the
pane. Tab lands on the frame and the next Tab continues the outer ring; it still never descends
into the contents. The boundary is never a member of its own pane's ring, so cycling inside the
pane cannot leak out over it.

One exception keeps traversal from ever being dead. If *every* tab stop in the tree lives inside
a pane — an app that is a single `Contain` frame, say — Tab from an unfocused app descends into
panes to establish focus. Once focus is inside, that pane's own ring takes over.

Capturing overlay traps take priority over contained scopes: while a capturing overlay is on
top, Tab cycles the overlay's own ring, and panes inside the overlay are opaque to it in the
usual way. The safety valve applies there too - if every tab stop in the overlay lives inside a
pane, the overlay ring descends through panes so the overlay is never a keyboard dead end.

> **Note:** the tab ring is built from tab stops, but focus is granted to anything focusable. A
> widget with `.tab_stop(false)`, or one reached through an `Exclude`/`Contain` escape hatch, is
> focusable without being in the ring. Tab from such a widget moves to the neighbour it *would*
> have had, rather than restarting the ring.

## Capturing Overlays

Root `Modal` and `Popover` overlays capture and trap focus. Their default `.auto_focus(true)`
focuses the first eligible descendant under every policy, including `Manual`. Dismissal restores
the prior focus entry; opening over an unfocused `OnDemand` app and dismissing returns to no focus.

Set `.auto_focus(false)` to retain keyboard capture and trapping while suspending focus inside the
overlay. Empty capturing overlays use the same suspension behavior. Local overlays do not provide
root capture semantics.

Set `Popover::capture_focus(false)` for a passive root-portal overlay that must remain above the
normal tree without taking focus from its trigger, such as autocomplete suggestions. In this mode,
`auto_focus` has no effect and keyboard input continues to route to the existing focused widget.

## Widget Focus Controls

`Accordion`, `Button`, `Checkbox`, `DocumentView`, `DraggableTabBar`, `FileTree`, `HexArea`,
`Hyperlink`, `Input`, `List`, `ManagedTerminal`, `PanView`, `SearchPalette`, `Slider`, `Table`,
`Tabs`, `Terminal`, `TextArea`, and `Tree` expose `tab_stop`, `on_focus`, and `on_blur`.

## Focus Events

The focusable widgets listed in [Widget Focus Controls](#widget-focus-controls) expose
`.on_focus(Callback<()>)` and `.on_blur(Callback<()>)`. Observe all changes with
`App::on_focus_changed`:

```rust
App::new().on_focus_changed(|change: &FocusChanged| {
    tracing::debug!(?change.old, ?change.new, "focus changed");
})
```

`FocusChanged` contains optional `old` and `new` `FocusEntry` values. Each entry carries the
widget's optional `Key` and public `Tag`.

The runtime emits `on_blur(old)` before `on_focus(new)`, then invokes the app hook. Link-backed
widget callbacks enter the normal callback queue in that order and run on the next pump, so the
synchronous app hook runs before those queued component handlers. Delivery is never re-entrant
into reconciliation.
If the old node has already unmounted, its widget callback cannot run, but the app hook still
receives the retained old entry.

Notifications are deduplicated when the runtime node is unchanged or both old and new nodes have
the same non-empty key. An unkeyed focusable widget that is replaced during reconciliation may
emit a blur/focus pair. Key dynamic focusable widgets when stable callback identity matters.

## Focus Decoration

Focus ownership and focus visuals are independent. Disable all theme-sourced focus decoration
without changing traversal or keyboard routing:

```rust
let theme = Theme::one_dark().focus_decoration(false);
```

Focus style precedence is:

1. Explicit widget `.focus_style(...)`, focused-content style, or scrollbar focus style.
2. `Theme::focus_decoration(false)`, which suppresses inherited/extended theme focus roles,
   per-widget focus palettes, automatic frame focus decoration, and `scrollbar.thumb_focus`.
3. `theme.focus` and widget focus palettes when decoration is enabled.

An explicit style always survives the theme switch. `Theme::focus(Style::default())` only empties
the generic focus role; `focus_decoration(false)` is the complete theme-level kill switch.

Selection styling is not focus decoration. `UnfocusedSelection` and text selections remain
visible when focus decoration is disabled. Because `OnDemand` starts unfocused, a list with a
selected row initially renders its unfocused-selection styling.

## Focus Queries

Use subtree queries to drive app-owned chrome:

```rust
let sidebar_active = ctx.has_focus_within_key("sidebar");

Frame::new()
    .key("sidebar")
    .border_style(if sidebar_active {
        BorderStyle::Thick
    } else {
        BorderStyle::Rounded
    })
    .child(sidebar)
```

## Keyboard Bubbling

Keyboard input normally starts at the focused widget, then bubbles through parent component
scopes to the root `Component::on_key`. With no focused widget, bubbling starts at the deepest
mounted component scope and continues toward the root. Unhandled keys then continue through app
commands, framework actions, and ambient page-scroll fallback according to `KeyDispatchPolicy`.

Under the default `KeyDispatchPolicy::WidgetFirst`, non-terminal dispatch is:

1. Pending app command chord.
2. `TextArea` Tab/Shift+Tab insertion opportunity.
3. Focused widget.
4. Bubbling component `on_key` handlers.
5. App command shortcuts.
6. Framework actions such as overlay dismissal and focus traversal.
7. A single opted-in ambient `ScrollView` for PageUp/PageDown.

`KeyDispatchPolicy::AppCommandsFirst` moves command and framework actions before widget and
bubbling dispatch. Embedded terminals use `TerminalKeyPolicy`; see
[`widgets/terminal.md`](widgets/terminal.md).

## Focus Sizing

`FocusSizing` is stack layout policy, not app focus policy. A `VStack` can expand its focused
keyed child in lazygit-style layouts:

```rust
VStack::new()
    .focus_sizing(FocusSizing::Accordion(FocusAccordion {
        focused_min: 10,
        collapsed: 1,
        ..FocusAccordion::default()
    }))
    .child(panel_a.key("panel-a"))
    .child(panel_b.key("panel-b"))
```

`FocusAccordion::sticky` defaults to `true`, retaining the last focused child for sizing when real
focus leaves the stack. Set it to `false` to return all panels to normal sizing. Children need
unique keys for sticky tracking.
