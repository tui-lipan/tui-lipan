# Architecture

This document is the implementation-aware architecture map for tui-lipan. It is
intended for contributors and maintainers who need to understand how the runtime
is assembled across modules.

It deliberately avoids duplicating API reference material. For user-facing APIs,
use the focused docs:

- [Quick start](quick-start.md) for app setup, features, and imports
- [Components](components.md) for lifecycle, state, commands, and nested components
- [Focus](focus.md) for focus traversal and scoped key bubbling
- [Keybindings](keybindings.md) for keymap files and chord APIs
- [Styling](styling.md) for style, color, length, and theme behavior
- [Widget reference](widgets/index.md) for per-widget APIs
- [Widget authoring](widget-authoring.md) for adding or changing widgets

## Design Goals

tui-lipan is a component-based TUI framework with a React/Elm-like runtime model:

- User code defines `Component`s with typed `Message`, `Properties`, and `State`.
- `view()` returns a declarative `Element` tree.
- The runtime expands nested components, reconciles to a realized node tree, routes
  input, and renders.
- Builder APIs are primary; `ui!` and `rsx!` are syntax sugar over the same model.

Core architectural constraints:

- No `ratatui` types in the public API.
- UI tree and component state are single-threaded.
- Background work returns through `Command` / `CommandLink` messages.
- Mouse, focus, hover, overlays, and scrollbars are routed through precise runtime
  hit-testing instead of ad-hoc widget-local coordinates.

## Runtime Flow

```text
terminal event
  -> AppRunner input dispatch
  -> widget callback or scoped key bubbling
  -> scoped message queue
  -> Component::update / Component::on_key
  -> optional Command scheduling
  -> CommandLink::send(...) back to queue
  -> view expansion if dirty
  -> reconcile + layout
  -> render
```

The important boundary is that component state only mutates on the UI/runtime
thread. Commands may perform background work, but they communicate back by
sending typed messages into the same scoped queue.

## Module Map

Primary runtime modules:

| Area | Modules |
|------|---------|
| App orchestration | `src/app/`, `src/app/runner/` |
| Runtime core for app/tests | `src/runtime.rs` |
| Component and element model | `src/core/` |
| Nested component registry | `src/core/nested/` |
| Realized node tree | `src/core/node/` |
| Layout and reconciliation | `src/layout/` |
| Input dispatch | `src/app/input/`, `src/app/runner/` |
| Overlay state and routing | `src/overlay.rs`, app runner overlay handlers |
| Internal terminal backend | `src/backend/ratatui_backend/` |
| Headless test runtime | `src/test_backend.rs` |
| Widgets | `src/widgets/` |
| Styling and themes | `src/style/` |

Keep backend-specific types inside `src/backend/` and app/runner internals. Public
types should remain crate-owned.

## Trees

tui-lipan uses two main tree representations.

`Element` is the declarative tree returned by `view()`:

- keyable for stable identity
- cloneable
- stores layout constraints
- stores widgets, group wrappers, and nested component placeholders

`NodeTree` is the realized runtime tree after expansion and reconciliation:

- stable `NodeId` values from an arena plus generation
- computed rects
- parent/child links
- widget-specific runtime payloads

The `NodeTree` is used by rendering, focus traversal, hit-testing, hover testing,
scrollbar routing, overlay routing, and pointer capture.

## Reconciliation

Reconciliation lives under `src/layout/reconcile/` and is epoch-based:

- Existing nodes are reused when shape, type, and key permit.
- Keyed children preserve identity across reorder.
- Unkeyed children fall back to order-based matching.
- Removed nodes and nested component instances are swept after the epoch.
- Capability flags such as hoverability, mouse-move handlers, and animated nodes
  are tracked incrementally to avoid extra full-tree scans.

Nested components are expanded before layout. Each expanded nested subtree is
wrapped in a layout-transparent group that carries scope metadata for message and
key routing. See [Components](components.md#nested-components) for author-facing
usage.

## Layout

The layout engine lives in `src/layout/`.

Current sizing model:

- Containers default to flex-like fill.
- Leaf widgets generally measure to content.
- Sizing is expressed with crate-owned layout types such as `Length`, `Size`,
  `Rect`, `Padding`, `Align`, and `Justify`.

Key implementation areas:

- measurement in `src/layout/measure.rs`
- stack algorithms for `VStack` and `HStack`
- min/max/focus/collapse constraints via `LayoutConstraints`
- overlay roots laid out relative to viewport bounds

For app-author layout usage, see [Styling](styling.md) and
[Layout widgets](widgets/layout.md). For contributor guidance, see
[Widget authoring](widget-authoring.md).

## Input Routing

Input dispatch is intentionally thin. It should resolve the target and delegate
behavior to widgets, component scopes, or overlay policy rather than embedding
large widget-specific behavior in the runner.

Keyboard flow:

- Crossterm events are converted to crate-owned `KeyEvent` values.
- Focused widgets get first chance.
- Unhandled keys bubble by component scope from child to parent to root.
- Focus traversal is handled centrally, including overlay-local traversal.

Mouse flow:

- Deep hit-testing resolves the target node, including overlay z-order rules.
- Click, drag, scroll, move, and scrollbar thumb drags use explicit routed state.
- Motion and scroll streams may be coalesced for render efficiency.

For behavior-level documentation, see [Focus](focus.md), [Keybindings](keybindings.md),
and [Events](events.md).

## Commands

Commands preserve a single-threaded UI tree while allowing concurrent I/O or
compute work:

- `Command::new(...)` runs a UI-thread action.
- `Command::spawn(...)` runs background work.
- `Command::spawn_keyed(...)` coalesces keyed background work according to
  `TaskPolicy`.

External full-screen subprocesses such as `$EDITOR` must use UI-thread terminal
handoff rather than a background worker. See [External programs](external-programs.md).

## Borrow discipline

The runtime is single-threaded and shares mutable state through `Rc<RefCell<…>>`
(message queue, overlay manager, contexts, theme, caches). `RefCell` enforces
borrow rules at runtime, so a re-entrant borrow panics (`already borrowed`)
instead of being a compile error. Two structural choices plus three conventions
keep this from happening.

Structural guarantees:

- **Component state is owned, not shared.** `RuntimeCore` holds `component: C`
  and the `ComponentRegistry` by value; `update` takes `&mut self` and `view`
  takes `&self`. A component cannot re-enter its own `update`/`view` through a
  `RefCell` — the borrow checker prevents it at compile time.
- **Callbacks are deferred.** A callback enqueues a typed message rather than
  running synchronously, so user code never executes while `view` holds a borrow.

Conventions for the shared `RefCell` cells — **never hold a borrow across
`update`, `view`, a callback, or a recursive layout/expand call**:

- *Pop in a temporary block*, then process outside the borrow:
  ```rust
  while let Some((scope, msg)) = { self.core.queue.borrow_mut().pop_front() } {
      let level = self.core.update_from_boxed(scope, msg)?; // borrow already released
  }
  ```
  This is why `update` can safely enqueue more messages while the queue drains.
- *Drain then `drop`* before re-entrant work: `borrow_mut` → collect into a
  `Vec` → `drop(guard)` → process.
- *Scope each borrow narrowly* — release a `borrow()` (a block or single
  statement) before taking a `borrow_mut()` of the same cell, and write caches
  only after child recursion has returned.

When adding shared state, keep borrows this short rather than introducing a guard
type; the conventions above are the load-bearing invariant.

## Rendering

The terminal backend is internal and lives under `src/backend/ratatui_backend/`.
It maps realized `NodeKind` values to ratatui rendering primitives and owns
terminal guard behavior such as raw mode, alternate screen, mouse capture,
keyboard enhancement negotiation, and panic-safe restoration.

Render invalidation has three broad levels:

- full: rebuild view, expand, reconcile, layout, and draw
- layout-only: reuse expanded view where possible, then reconcile/layout/draw
- paint-only: redraw the existing realized tree

Renderer responsibilities include clipping, overlay clears, chrome frames,
scrollbars, and widget-specific painting.

## Overlays

Overlay primitives are implemented as portal-style roots plus centralized overlay
manager state. The app runner owns routing policy for focus capture, pointer
capture, outside/escape dismissal, toast ticking, and overlay-local tab traversal.

In inline viewport mode, root overlays are intentionally suppressed. See
[Inline mode](inline-mode.md) and [Overlay widgets](widgets/overlays.md).

## Testing

`TestBackend` provides a headless runtime that exercises the same message queue,
scoped routing, nested expansion, reconciliation, and command-drain behavior used
by real apps. Prefer it for deterministic component tests that should not enter
terminal raw mode.

## Architectural Invariants

These invariants should remain true unless the architecture is intentionally
redesigned:

- Public API uses crate-owned types, not backend types.
- Component and UI tree state mutate on the runtime thread only.
- Async/background work returns through typed messages.
- Keyed nodes and nested components preserve identity across reorder.
- Focus, hover, mouse, scrollbar, and overlay routing are centralized over the
  realized node tree.
- Overlay focus/pointer capture and dismiss behavior are explicit runtime policy.
- Primitive widget variants are closed and framework-owned; application-level
  extension happens through composition.

### Closed primitive widgets

`ElementKind` and `NodeKind` are deliberately closed internal enums. Exhaustive
dispatch keeps layout, reconciliation, hit-testing, scrollbar routing, and
backend rendering predictable, while preserving the rule that public APIs use
crate-owned types rather than `ratatui` or backend internals.

Apps and external crates extend tui-lipan by composing existing primitives in
`Component`s, view helpers, or composite widget structs that return `Element`.
Adding a new primitive is a framework-contributor task and should follow the
triage criteria and wiring checklist in [Widget authoring](widget-authoring.md).

This is a fast-moving prototype. Prefer improving the architecture over preserving
obsolete internal shapes.
