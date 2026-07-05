# Architecture Reference (Current)

Use this file for **how the pipeline fits together**. For the ordered wiring checklist, use `SKILL.md` → *Mandatory Wiring Checklist*.

## Core Model

- `Element` is the declarative input tree (`src/core/element.rs`).
- `NodeKind` is the mutable runtime tree (`src/core/node/kind.rs`).
- `src/widget_manifest.rs` is the central registry for widget variants. It keeps
  tag reuse, standard dimension dispatch, and layout-hash dispatch in sync.
- Reconciliation updates/reuses nodes from elements (`src/layout/reconcile/element.rs`).
- Rendering dispatches by `NodeKind` (`src/backend/ratatui_backend/render.rs`).

## Primitive Widget Layout

For a primitive widget named `Foo`:

- Public module: `src/widgets/foo/mod.rs`
- Node: `src/widgets/foo/node.rs`
- Measure: `src/widgets/foo/layout.rs`
- Reconcile helpers: `src/widgets/foo/reconcile.rs`
- Renderer: `src/backend/ratatui_backend/renderers/foo.rs` (not under `src/widgets/`)

Public conversion pattern:

```rust
impl From<Foo> for Element {
    fn from(value: Foo) -> Self {
        Element::new(crate::core::element::ElementKind::Foo(value))
    }
}
```

There is no public `Widget` trait; use `From<Widget> for Element` (and `LayoutHash` on the builder type when the widget participates in layout caching).

**Sizing:** normal widgets expose width/height through the category chosen in
`src/widget_manifest.rs`, which generates the standard `ElementKind::dimensions()`
arms. Touch `src/layout/axis.rs` only for wrapper-style special cases (`Frame`,
`Group`, `MouseRegion`, `Popover`, etc.).

**Layout cache:** choose a manifest category that delegates layout hashing when the
widget should participate in shared measurement, and implement `LayoutHash` on the
builder. No-hash manifest categories intentionally force cache misses.

## Reconcile Patterns

- **Simple widgets**: assign `node.kind = NodeKind::Foo(FooNode::from(foo.clone()))` or use `reconcile_simple_leaf` where appropriate.
- **Incremental widgets**: if existing kind matches, update fields in place and early-return when unchanged.
- **Heavy render widgets**: cache expensive derived output with hash keys (see sparkline and big_text).

## Renderer Rules

- Always honor clipping (intersect with clip rect before draw).
- Keep rendering-only state in node/output; avoid public `ratatui` types in the widget API.
- Patch style layers intentionally (`base.patch(detail)`), do not accidentally drop parent/base style.

## Theme Provider

If the widget has visual styles, add an `ElementKind::Foo(foo)` arm in `src/widgets/theme_provider.rs`.

- `apply_theme_style`: fill unset primary/base style fields.
- `apply_theme_accent_style`: interactive/emphasis states.
- `apply_theme_style_force`: stronger theme-default participation.

Do not overwrite explicit user style fields unless behavior requires it.

## Input (Interactive Widgets Only)

- `src/app/input/mouse/gather.rs` — hit → actions.
- `src/app/input/mouse/hover.rs` — custom hover zones.
- `src/app/input/mouse/scroll.rs` — scroll-wheel classification and dispatch.
- `src/app/mouse_dispatch.rs` — shared mouse routing for native runner and test backend.
- `src/app/runner/events.rs` — native event loop integration and terminal forwarding.
- `src/app/runner/mouse_clicks.rs` — most per-widget click and drag-start execution.
- `src/app/input/drag.rs` — drag helpers/types.
- `src/app/interaction_state.rs` — `ActiveDrag` and runner/test-backend interaction state.
- `src/app/runner/drag.rs` — drag updates while active.
- `src/app/input/keyboard.rs` — thin `dispatch_key` router.
- `src/app/input/handlers/` — per-widget `handle_key` / scroll implementations (see `handlers/mod.rs` for `InteractiveTag` / `ScrollableTag`).

## Composite Widgets

- Single-file `src/widgets/<name>.rs` when the composition is small.
- Compose primitives and return `Element`; `From<Composite> for Element`.
- No `NodeKind` or backend renderer for pure composites.

## Validation

After wiring: `cargo build`, `cargo clippy`, `cargo fmt`, `cargo test`.
