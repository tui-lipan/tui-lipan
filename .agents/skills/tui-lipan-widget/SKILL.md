---
name: tui-lipan-widget
description: Create or update widgets for tui-lipan, including primitive widgets (node/layout/reconcile/renderer), composite widgets, and full framework wiring across src/widgets/, src/core/, src/layout/, and src/backend/ratatui_backend/. Use when adding a new widget, changing widget behavior, or integrating widget styling with theme provider and event handling.
---

# TUI-lipan Widget Creation

Build widgets with the current tui-lipan architecture, not generic VDOM patterns.

## Choose Widget Type

- Use a **primitive widget** when you need custom measurement, custom rendering, internal node state, or custom input behavior.
- Use a **composite widget** when you can compose existing widgets and return one `Element` tree.

## Primitive Widget Workflow

1. Create `src/widgets/<widget_name>/` with:
   - `mod.rs` (public builder API)
   - `node.rs` (internal node)
   - `layout.rs` (measurement)
   - `reconcile.rs` (node update logic)
   - optional helper modules when the widget needs extra formatting, animation, caching, or internal component logic
2. Create backend renderer: `src/backend/ratatui_backend/renderers/<widget_name>.rs`.
3. Wire every integration point in the checklist below.

### Implement Public API (`mod.rs`)

- Define `#[derive(Clone)] pub struct WidgetName { ... }`.
- Expose builder-style setters.
- Implement conversion with `impl From<WidgetName> for Element`.
  - Do **not** use a `Widget` trait (there is no public widget trait in current architecture).
- Implement `LayoutHash` here when the widget should participate in layout caching.
- Keep public API free of `ratatui` types.
- Use crate conventions (`Arc<str>` for shared immutable strings, `Callback<T>` / `KeyHandler` for events).

### Implement Node (`node.rs`)

- Define `WidgetNameNode` for internal runtime fields.
- Implement `WidgetNode` methods when needed:
  - `is_focusable`
  - `has_on_click`
  - `is_hoverable`
  - `hit_test_refinement`
  - `scrollbar_zones`
- Implement `From<WidgetName> for WidgetNameNode`.
- Keep node fields aligned with renderer/reconcile needs; do not assume `Length` must be removed from nodes.

### Implement Measurement (`layout.rs`)

- Add `measure_<widget_name>(...) -> (u16, u16)`.
- Respect `Length::Auto` vs `Length::Px(...)` behavior.
- Clamp safely and handle empty content.

### Implement Reconcile (`reconcile.rs`)

- For simple leaf widgets, use `reconcile_simple_leaf` from
  `crate::layout::reconcile` — pass `constraints: &LayoutConstraints` so that
  min/max clamping is applied.  See `src/widgets/button/reconcile.rs` for the
  canonical pattern.
- Reuse existing node when kind matches; update only changed fields.
- Return fast when unchanged.
- For expensive rendering/transforms, add explicit cache keys and output cache (see sparkline/big_text patterns).

### Implement Renderer (`renderers/<widget>.rs`)

- Receive frame + node + rect + clip bounds.
- Intersect with clip rect before drawing.
- Patch base style with span/state style in renderer (`base.patch(detail)`) rather than replacing.
- Keep overflow/clipping behavior explicit and consistent.

## Mandatory Wiring Checklist

When adding a new primitive widget, update all relevant files:

1. `src/widgets/mod.rs`
   - Add `mod <widget_name>;`
   - Re-export public widget type(s).
2. `src/widgets/internal.rs`
   - Re-export node/measure/reconcile for internal pipeline.
3. `src/widget_manifest.rs`
   - Add the widget to the correct category so generated dispatch stays in sync.
   - This drives `Tag` matching, standard `ElementKind::dimensions()` arms, and
     `LayoutHash for ElementKind` dispatch.
   - Pick a feature-gated or no-hash category when appropriate; do not manually
     patch generated arms in `layout/tag.rs`, `ElementKind::dimensions()`, or
     `layout/hash.rs`.
4. `src/core/element.rs`
   - Add `ElementKind::<WidgetName>(...)` variant.
   - Update imports, `children()` / `children_mut()` only if the widget owns child elements, and
     `From<WidgetName> for Element`.
5. `src/core/node/kind.rs`
   - Add `NodeKind::<WidgetName>(...)` variant.
   - Wire delegation in `WidgetNode for NodeKind` match arms.
   - Add `impl From<WidgetName> for NodeKind`.
6. `src/layout/measure.rs`
   - Call your `measure_<widget_name>` in min-size dispatch.
7. `src/layout/reconcile/element.rs`
   - Add element reconcile branch passing `&el.layout` as constraints:
     `ElementKind::Foo(foo) => reconcile_foo(tree, id, foo, rect, &el.layout),`
8. `src/backend/ratatui_backend/renderers/mod.rs`
   - Register renderer module.
9. `src/backend/ratatui_backend/render.rs`
    - Add `NodeKind::<WidgetName>` render dispatch.
10. `src/widgets/theme_provider.rs`
    - Apply theme defaults when the widget has themeable styles or themed children.
11. `docs/widgets/<category>.md` + `docs/widgets/index.md`
    - Update the relevant widget docs file and widget index entry.

Touch `src/layout/axis.rs` only for wrapper/per-axis special cases like `Frame`, `Group`,
`MouseRegion`, or `Popover`. Normal widgets should size through the
manifest-generated `ElementKind::dimensions()` arms.

Touch `src/lib.rs` only when a type should also be promoted as a crate-root export.
`src/prelude.rs` already re-exports `crate::widgets::*`.

### Optional but Important

- Add widget-specific helper modules freely when one file becomes noisy; directory-based composites
  and primitives are both normal in the current codebase.

## Input/Event Integration Checklist

Use this only for interactive widgets.

- **Keyboard behavior**: add or extend a handler in `src/app/input/handlers/`, then register the
  widget in `InteractiveTag` and classifier helpers in `src/app/input/handlers/mod.rs`
- **Wheel scrolling**: add `handle_scroll` in a handler module, then register the widget in
  `ScrollableTag` and scroll classifiers in `src/app/input/handlers/mod.rs`; wheel dispatch lives
  in `src/app/input/mouse/scroll.rs`
- **Mouse click payload gathering**: update `src/app/input/mouse/types.rs` and
  `src/app/input/mouse/gather.rs`
- **Per-widget click execution**: add most non-trivial widget click behavior in
  `src/app/runner/mouse_clicks.rs`; shared native/test-backend routing lives in
  `src/app/mouse_dispatch.rs`
- **Drag state and math**: update `src/app/input/drag.rs`
- **Drag lifecycle**: add `ActiveDrag` state in `src/app/interaction_state.rs`, start it from
  `src/app/runner/mouse_clicks.rs`, and handle move/release in `src/app/mouse_dispatch.rs` and
  `src/app/runner/drag.rs`
- **Hover / mouse move**: update `src/app/input/mouse/hover.rs` and/or
  `src/app/input/mouse/move.rs`

## Composite Widget Pattern

- Small composites can live in one file `src/widgets/<widget_name>.rs`.
- Use a directory when the widget needs helper modules, caches, internal components, or
  non-trivial state.
- Store props on a simple builder struct.
- Compose primitives in a `build(self) -> Element` method.
- Implement `From<CompositeWidget> for Element` and return composed tree.
- Do not add `NodeKind`/renderer for pure composites.

## Theme Integration Rules (Crucial)

- Always integrate new style-bearing widgets with `ThemeProvider`.
- Use helper behavior consistently:
  - `apply_theme_style` for base text/surface style defaults.
  - `apply_theme_accent_style` for interactive emphasis (hover/focus/rising/falling-like accents).
  - `apply_theme_style_force` when a style should strongly follow theme primary defaults.
- Patch only unset fields where intended; avoid overriding explicit user styling.

## Validation

Run after widget changes:

1. `cargo build`
2. `cargo clippy`
3. `cargo fmt`
4. `cargo test`

## Common Pitfalls

- Forgetting `theme_provider` integration (widget looks unthemed).
- Wiring renderer but missing layout reconcile branch (widget exists but never updates correctly).
- Missing `Tag` mapping (node reuse/reconcile anomalies).
- Missing `ElementKind::dimensions()` or measure/reconcile wiring (bad sizing in stacks).
- Using public `ratatui` types in widget API.
- Rebuilding heavy render output every reconcile instead of key-based cache reuse.

## References

- `references/primitive-widget-template/` for starter structure.
- `references/architecture.md` for deeper pipeline context.
- `references/event-handling-patterns.md` for detailed input patterns.

If any reference content conflicts with current source code, follow current source code.
