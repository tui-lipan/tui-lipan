# Large App Shells

Large tui-lipan apps are easiest to diagnose when the root `Component` stays thin and app behavior lives in named operation modules.

## Recommended shape

- Keep the root app type, `Message` enum, `Component` implementation, and bootstrap code together.
- Put the `Message` match in a dispatcher module when it grows beyond a short screen of code.
- Put app-owned behavior in operation modules named after behavior: `actions`, `key_routing`, `search`, `theme`, `focus`, or domain-specific names.
- Keep view code as the widget/callback boundary. It should build elements and emit messages, not mutate app policy directly.
- Keep reusable chrome as helper functions or composite widgets. Use nested `Component`s only when the child owns state, lifecycle, keyboard handling, or async work.

## Message dispatch

The root component can delegate update logic without hiding the tui-lipan lifecycle:

```rust
impl Component for App {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        update::handle_msg(self, msg, ctx)
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        view::render(self, ctx)
    }
}
```

The dispatcher should route messages to narrow functions. Long behavior bodies belong in app modules, not in the root component.

## Input routing

Apps with global shortcuts and focusable children should make the input sources explicit:

- `Component::on_key` receives keys that bubble to the root.
- Widget callbacks receive keys consumed by focused widgets.
- Both paths can call the same app-owned routing function when they share policy.

Choose `App::focus_policy(...)` at bootstrap rather than scattering local workarounds. `OnDemand`
is the default for shells that should start neutral, `Auto` suits applications requiring an
immediate keyboard target, and `Manual` suits shells with fully app-owned pane routing. Under
`Manual`, global Tab and click-to-focus are disabled, but explicit context APIs and capturing
overlay traps remain active.

Use `ctx.request_focus(...)` and stable element keys for focus handoff. A retained `OnDemand` key
restores focus when its widget remounts; `ctx.blur()` clears that identity. Use `tab_stop(false)`
for command-only targets, `FocusScope::Contain` for pane-local rings, and
`FocusScope::Exclude` for non-navigable subtrees. Return `KeyUpdate::handled(...)` only when the app
consumed the key.

Route widget `on_focus`/`on_blur` callbacks into messages when focus changes affect state. Use
`App::on_focus_changed` for diagnostics or cross-cutting observation, and key dynamic focusables so
remount deduplication has stable identity.

## Terminal apps

Terminal widgets expose low-level control. A terminal-heavy app should keep these concerns visible:

- PTY readiness and output event handling.
- Terminal input forwarding.
- Terminal resize side effects.
- Scrollback synchronization.
- App theme to terminal palette mapping.

These policies are usually app-owned. Move them to focused app modules before considering a framework abstraction.

## Diagnostics checklist

When debugging a large app, locate the bug by boundary:

| Symptom | First place to inspect |
| --- | --- |
| Message has no effect | Message dispatcher and operation module |
| Shortcut works only in some widgets | Root `on_key`, widget callback, and focus bubbling |
| Focus jumps to the wrong element | Stable keys and `ctx.request_focus(...)` call site |
| Focus returns after a conditional panel remount | Retained `OnDemand` key; call `ctx.blur()` when closing permanently |
| Tab cannot leave a pane | Nearest `FocusScope::Contain` and pane-switch command |
| Focus callbacks duplicate after rerender | Missing stable key on a remounted focusable widget |
| Widget renders correctly but app state is stale | Callback-to-message wiring in the view boundary |
| Terminal receives wrong bytes | App terminal input forwarding |
| Terminal layout or resize is wrong | App geometry policy before framework layout primitives |
| Async result applies out of order | Command key and `TaskPolicy` choice |

## Reference example

`examples/window_manager.rs` demonstrates a large root app with canvas composition, focus routing, drag/resize behavior, workspace switching, and terminal composition. Treat it as a reference for boundaries and event flow rather than as a generic framework abstraction.
