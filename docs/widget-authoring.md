# Widget Authoring Guide

How to add a new widget to tui-lipan, from design decision through full integration.

---

## 1. Primitive vs Composite

**Primitive widget** - needs its own node type, measurement, reconciliation, and renderer.
Primitive authoring is for tui-lipan contributors extending the built-in widget
set. Choose this when you need:
- Custom rendering (drawing cells directly)
- Internal mutable state in the node tree (e.g., cursor position, cached render output)
- Custom hit-testing, hover zones, or scrollbar regions
- Custom measurement logic

**Composite widget** - composes existing primitives and returns an `Element` tree.
This is the public extension model for applications and external crates. Choose
this when you can express the widget entirely in terms of existing widgets
(`VStack`, `HStack`, `Frame`, `Text`, `Button`, etc.).

> Most new widgets start as composites. Only promote to a primitive when you hit
> a limitation that requires custom node/renderer behavior.

tui-lipan does not expose or promise a public primitive-widget plugin API, custom
`ElementKind`/`NodeKind` variants, or public render-canvas hooks. The public
integration point is returning `Element` from components, helpers, and composite
widget structs.

### Primitive acceptance criteria

Before adding a framework primitive, verify that it:

- cannot be cleanly expressed as a composite of existing primitives;
- needs custom measurement, node state, rendering, hit testing, or scrollbar
  regions;
- fits the curated built-in widget set rather than one app's private UI;
- includes user-facing docs, examples or tests when appropriate, and a changelog
  entry when the behavior or API is user-visible.

---

## 2. End-to-End Checklist (Primitive Widget)

Every file you need to create or update when adding a primitive widget named `Foo`:

### Files to create

| File | Purpose |
|------|---------|
| `src/widgets/foo/mod.rs` | Public builder API, `From<Foo> for Element`, `LayoutHash` impl |
| `src/widgets/foo/node.rs` | `FooNode` runtime state, `WidgetNode` impl, `From<Foo> for FooNode` |
| `src/widgets/foo/layout.rs` | `measure_foo()` - intrinsic min-size measurement |
| `src/widgets/foo/reconcile.rs` | `reconcile_foo()` - update node from element |
| `src/backend/ratatui_backend/renderers/foo.rs` | Renderer - draw node to terminal frame |

### Files to update (mandatory wiring)

| # | File | What to add |
|---|------|-------------|
| 1 | `src/widgets/mod.rs` | `mod foo;` + `pub use foo::Foo;` |
| 2 | `src/widgets/internal.rs` | `pub(crate) use super::foo::{FooNode, measure_foo, reconcile_foo};` |
| 3 | `src/widget_manifest.rs` | Add `Foo` to the exact category that matches its sizing/hash/node behavior |
| 4 | `src/core/element.rs` | `ElementKind::Foo(Foo)` variant; `dimensions()` is generated from the manifest category |
| 5 | `src/core/node/kind.rs` | `NodeKind::Foo(FooNode)` variant + any needed `From` impl; `WidgetNode` delegation arms are generated |
| 6 | `src/layout/measure.rs` | Call `measure_foo` in the min-size dispatch |
| 7 | `src/layout/reconcile/element.rs` | Reconcile branch for `ElementKind::Foo` |
| 8 | `src/backend/ratatui_backend/renderers/mod.rs` | `pub(crate) mod foo;` |
| 9 | `src/backend/ratatui_backend/render.rs` | `NodeKind::Foo` render dispatch arm |
| 10 | `docs/widgets/<category>.md` + `docs/widgets/index.md` | User-facing widget docs |
| 11 | `docs/events.md` | Event structs and callback tables (if interactive) |

For every new public app-author type, including event structs, callback payloads,
enums, and helper constructors, also check the export surfaces:

| Surface | When to export |
|---------|----------------|
| Defining module | Always, for the canonical module path |
| `src/widgets/mod.rs` | Widget-owned public types and helpers |
| `src/lib.rs` | Types expected to work as `use tui_lipan::TypeName;` |
| `src/prelude.rs` | Common app-author types used directly in component code |

Before finishing, verify direct imports for promoted types, for example
`use tui_lipan::FooEvent;`, not just `use tui_lipan::prelude::*;`.

**Note**: Touch `src/layout/axis.rs` only for wrappers with per-axis special cases
(Frame, Group, MouseRegion, Popover). Normal widgets should size through
`ElementKind::dimensions()`, which is generated from `src/widget_manifest.rs`.

`src/layout/tag.rs` (`Tag`, `tag_of_element`, `tag_of_node`) and
`src/layout/hash.rs` dispatch are also generated from `src/widget_manifest.rs`.
Do not add one-off arms there; choose the correct manifest category instead.

Before submitting, run the lightweight plumbing guard:

```bash
python3 scripts/generate-node-kind-delegate-arms.py --write
python3 scripts/check-widget-variant-parity.py
python3 scripts/generate-node-kind-delegate-arms.py
```

The generator updates the checked-in `node_kind_delegate_match!` arms from the
manifest. The parity check compares the manifest against `ElementKind`,
`NodeKind`, `node_kind_delegate_match!`, and the `render_node` dispatch,
including feature-gated variants textually.

---

## 3. Public API and Builder Conventions

The public widget struct lives in `src/widgets/foo/mod.rs`. Follow these conventions:

```rust
use std::sync::Arc;

use crate::callback::Callback;
use crate::core::element::{Element, ElementKind};
use crate::style::{Length, Padding, Style};

/// A foo widget that does X.
#[derive(Clone)]
pub struct Foo {
    pub label: Option<Arc<str>>,     // Arc<str> for shared immutable strings
    pub value: f64,
    pub style: Style,
    pub width: Length,
    pub height: Length,
    pub on_change: Option<Callback<f64>>,
    pub focusable: bool,
}
```

### Builder methods

Use consuming `self` builders. Each setter takes `mut self` and returns `Self`:

```rust
impl Foo {
    pub fn new(value: f64) -> Self {
        Self {
            label: None,
            value,
            style: Style::default(),
            width: Length::Auto,     // leaf widgets default to Auto
            height: Length::Auto,
            on_change: None,
            focusable: true,
        }
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    pub fn width(mut self, width: Length) -> Self {
        self.width = width;
        self
    }

    pub fn on_change(mut self, cb: Callback<f64>) -> Self {
        self.on_change = Some(cb);
        self
    }
}
```

### Conversion to Element

```rust
impl From<Foo> for Element {
    fn from(value: Foo) -> Self {
        Element::new(ElementKind::Foo(value))
    }
}
```

There is no `Widget` trait. Conversion to `Element` is the only integration point.

### Hard rules

- **No `ratatui` types in the public API.** Use `Style`, `Length`, `Color`, `Callback`, etc. from `tui_lipan`.
- **Use `Arc<str>`** for immutable shared strings (labels, titles, placeholder text).
- **Use `Callback<T>`** for event callbacks and `KeyHandler` for `on_key` props.
- **Leaf widgets default to `Length::Auto`**; containers default to `Length::Flex(1)`.

---

## 4. Node, Reconcile, and Runtime State

### Node (`node.rs`)

The node is the runtime representation that lives in the `NodeTree`. It holds all
fields the renderer and input system need:

```rust
use crate::callback::Callback;
use crate::core::node::{NodeKind, WidgetNode};
use crate::style::Style;
use super::Foo;

#[derive(Clone)]
pub struct FooNode {
    pub value: f64,
    pub label: Option<Arc<str>>,
    pub style: Style,
    pub on_change: Option<Callback<f64>>,
    pub focusable: bool,
}

impl WidgetNode for FooNode {
    fn is_focusable(&self) -> bool { self.focusable }
    fn has_on_click(&self) -> bool { self.on_change.is_some() }
    fn is_hoverable(&self) -> bool { self.has_on_click() }
}

impl From<Foo> for FooNode {
    fn from(foo: Foo) -> Self {
        Self {
            value: foo.value,
            label: foo.label,
            style: foo.style,
            on_change: foo.on_change,
            focusable: foo.focusable,
        }
    }
}

impl From<FooNode> for NodeKind {
    fn from(node: FooNode) -> Self {
        NodeKind::Foo(node)
    }
}
```

The `WidgetNode` trait hooks control input behavior:

| Method | Purpose |
|--------|---------|
| `is_focusable()` | Can this node receive keyboard focus? |
| `has_on_click()` | Is this node a click target? |
| `is_hoverable()` | Should hover state be tracked? |
| `hit_test_refinement(x, y, rect)` | Override hit-test for partial-area widgets |
| `scrollbar_zones(...)` | Expose scrollbar hit zones |

### Reconcile (`reconcile.rs`)

Reconciliation updates an existing node from a new element. For simple widgets,
use the `reconcile_simple_leaf` helper - it handles auto-sizing and constraint
clamping in one call:

```rust
use crate::core::node::{NodeId, NodeKind, NodeTree};
use crate::layout::reconcile::reconcile_simple_leaf;
use crate::style::{LayoutConstraints, Rect};
use super::{Foo, FooNode, measure_foo};

pub fn reconcile_foo(
    tree: &mut NodeTree,
    id: NodeId,
    foo: &Foo,
    rect: Rect,
    constraints: &LayoutConstraints,
) -> NodeId {
    reconcile_simple_leaf(
        tree,
        id,
        rect,
        constraints,
        foo.width,
        foo.height,
        measure_foo(foo),
        || NodeKind::Foo(FooNode::from(foo.clone())),
    )
}
```

The helper performs: measure → `resolve_rect_with_auto` (honours `Length::Auto`
and applies `LayoutConstraints` min/max clamping) → clear children → set node
kind → return id. Do **not** skip the `constraints` parameter - omitting it
silently ignores any `min_width`/`max_width` constraints set by the parent.

In `element.rs` dispatch, call it as:

```rust
ElementKind::Foo(foo) => reconcile_foo(tree, id, foo, rect, &el.layout),
```

For widgets with expensive rendering, cache the output with hash keys (see
`src/widgets/sparkline/reconcile.rs` for the pattern - compare a cache key before
regenerating render output).

---

## 5. Manifest, Layout, and Axis Sizing

Most widgets participate in layout through `ElementKind::dimensions()` in
`src/core/element.rs`, but those arms are generated from
`src/widget_manifest.rs`. Add `Foo` to the category that matches its sizing and
layout-hash behavior:

| Manifest category | Dimensions | Layout hash | In `NodeKind` | Use when |
|---|---|---|---|---|
| `direct` | `w.width, w.height` | delegate | yes | Standard width/height-backed widget |
| `direct_gated` | `w.width, w.height` | delegate | yes | Standard widget behind a feature flag |
| `direct_no_hash` | `w.width, w.height` | `None` | yes | Width/height widget that intentionally skips layout caching |
| `direct_no_hash_gated` | `w.width, w.height` | `None` | yes | Feature-gated no-hash widget |
| `props_dims` | `w.props.width, w.props.height` | delegate | yes | Container whose dimensions live in `props` |
| `const_auto_hash` | `(Auto, Auto)` | delegate | yes | Node-backed widget with fixed auto sizing |
| `const_auto_hash_gated` | `(Auto, Auto)` | delegate | yes | Feature-gated fixed-auto widget |
| `const_flex` | `(Flex(1), Flex(1))` | delegate | yes | Node-backed flex container/helper |
| `const_flex_no_hash` | `(Flex(1), Flex(1))` | `None` | yes | Flex helper that intentionally skips layout caching |
| `no_dims` | `None` | delegate | yes | Wrapper/special-case widget handled by recursive/per-axis logic |
| `element_only_const_auto` | `(Auto, Auto)` | `None` | no | Element-only wrappers consumed before node reconciliation |

The layout engine uses these dimensions to compute flexbox-style sizing within stacks.

**Only touch `src/layout/axis.rs`** if your widget wraps children and needs
per-axis delegation (like Frame, Group, MouseRegion). Normal widgets should not
need this; put them in the correct manifest category instead.

Manual variant sync checklist:

- `src/widget_manifest.rs`: add the variant to exactly one category, preserving
  feature annotations such as `Foo => "foo-feature"` when applicable.
- `src/core/element.rs`: add `ElementKind::Foo(Foo)` with matching `#[cfg]` if gated.
- `src/core/node/kind.rs`: add `NodeKind::Foo(FooNode)` and any needed
  `From<Foo> for NodeKind` impl, then run
  `python3 scripts/generate-node-kind-delegate-arms.py --write` to refresh the
  generated `node_kind_delegate_match!` arms.
- `src/backend/ratatui_backend/render.rs`: add the `NodeKind::Foo` arm in
  `render_node`.
- Run `python3 scripts/check-widget-variant-parity.py` and
  `python3 scripts/generate-node-kind-delegate-arms.py` to catch drift.
- `src/ui_snapshot/kind.rs`: add the variant to `UiWidgetKind`, `as_str`, and
  `from_node_kind` (agent snapshot describe tags).

---

## 6. Measurement and LayoutHash

### Measurement (`layout.rs`)

The measure function returns the intrinsic `(width, height)` in terminal cells:

```rust
use crate::style::Length;
use super::Foo;

pub fn measure_foo(foo: &Foo) -> (u16, u16) {
    let w = if let Length::Px(px) = foo.width { px } else { 10 }; // sensible default
    let h = if let Length::Px(px) = foo.height { px } else { 1 };
    (w, h)
}
```

Respect the `Length::Auto` vs `Length::Px(n)` distinction. When `Auto`, compute
the natural size from content. When `Px`, use the explicit value.

Wire this function into `src/layout/measure.rs` in the min-size dispatch match.

### LayoutHash

Implement `LayoutHash` on your widget struct so the layout cache can detect when
re-layout is needed:

```rust
impl crate::layout::hash::LayoutHash for Foo {
    fn layout_hash(
        &self,
        hasher: &mut impl std::hash::Hasher,
        _recurse: &dyn Fn(&crate::core::element::Element) -> Option<u64>,
    ) -> Option<()> {
        use std::hash::Hash;
        self.width.hash(hasher);
        self.height.hash(hasher);
        self.label.hash(hasher);
        // Hash all fields that affect measurement/positioning.
        // Do NOT hash purely visual fields (styles, callbacks).
        Some(())
    }
}
```

**If you skip LayoutHash**, the layout cache will always miss for your widget,
causing unnecessary re-layout on every render cycle. After implementing
`LayoutHash`, put the widget in a manifest category whose `layout_hash` behavior
is `delegate`; `src/layout/hash.rs` dispatch is generated from that category.

---

## 7. Renderer and Clipping

Create `src/backend/ratatui_backend/renderers/foo.rs`. The renderer receives the
ratatui frame, node data, layout rect, and clip bounds:

```rust
use ratatui::Frame;
use crate::core::node::kind::FooNode;
use crate::style::Rect;

pub(crate) fn render_foo(
    frame: &mut Frame,
    node: &FooNode,
    rect: Rect,
    clip: Rect,
    // ... other params as needed (hover state, focus state, etc.)
) {
    // Always intersect with clip rect before drawing
    let visible = rect.intersect(clip);
    if visible.is_empty() {
        return;
    }

    // Draw using ratatui APIs within the visible area
    // Use base.patch(detail) for style layering, not replacement
}
```

Key rules:
- **Always clip.** Intersect with the clip rect before any drawing.
- **Patch styles, don't replace.** Use `base_style.patch(detail_style)` to layer styles
  so parent/theme styles are preserved.
- **Keep `ratatui` types confined to this file.** The node and public API must not
  expose `ratatui` types.

Register the module in `src/backend/ratatui_backend/renderers/mod.rs` and add the
dispatch arm in `src/backend/ratatui_backend/render.rs`.

---

## 8. Keyboard Integration

For interactive widgets that handle keyboard input when focused, add a handler in
`src/app/input/handlers/`.

The pattern:

```rust
// In src/app/input/handlers/foo.rs (or extend an existing handler file)
NodeKind::Foo(node) => {
    let mut handled = false;

    match key.code {
        KeyCode::Left => {
            if let Some(cb) = &node.on_change {
                cb.emit(node.value - node.step);
                handled = true;
            }
        }
        KeyCode::Right => {
            if let Some(cb) = &node.on_change {
                cb.emit(node.value + node.step);
                handled = true;
            }
        }
        _ => {}
    }

    if !handled {
        handled = node.on_key.as_ref().map(&handle_key).unwrap_or(false);
    }

    handled
}
```

Then register the widget in `InteractiveTag` and the classifier helpers in
`src/app/input/handlers/mod.rs`.

---

## 9. Mouse Click Integration

Mouse clicks flow through two stages:

1. **Gather** (`src/app/input/mouse/gather.rs`): Map the hit node to an action payload.
   Add your `NodeKind::Foo` match here to produce an action describing what was clicked.

2. **Execute** (`src/app/runner/mouse_clicks.rs`): Consume the action and fire callbacks.
   Add the actual click execution logic here, called from `src/app/runner/events.rs`.

If you need a new action type, define it in `src/app/input/mouse/types.rs`.

**Important**: Keep gather lightweight (just identify what was hit). Put the actual
side effects (callback emission, state changes) in the runner's mouse_clicks module.

---

## 10. Drag Integration

For drag-capable widgets (sliders, splitters, scrollbar thumbs):

1. **Define drag state** in `src/app/input/drag.rs` - a struct holding the initial
   click position and any reference values needed during the drag.

2. **Start the drag** from `src/app/runner/mouse_clicks.rs` on left-button-down
   when the hit target matches your widget.

3. **Update during drag** in `src/app/runner/drag.rs` - compute new values from
   mouse position and emit callbacks.

4. **End the drag** on left-button-up in `src/app/runner/events.rs` - clear the
   active drag state.

Guard every drag update with `tree.is_valid(id)` - the node may have been removed
between ticks.

**Debounce**: Compare old and new values before emitting callbacks. Avoid forcing
dirty re-renders when nothing actually changed.

---

## 11. Hover, Focus, Hit-Testing, and Scrollbar Zones

### Hover

Default hover behavior comes from `node.is_hoverable()`. If hover should only
apply to part of the widget (e.g., the track of a slider, not the label), add
shape-specific logic in `src/app/input/mouse/hover.rs`.

### Focus

If `is_focusable()` returns `true`, the widget participates in tab-order traversal.
Users can also route focus to it via `ctx.request_focus("key")`.

### Hit-testing

By default, any node with `has_on_click() == true` or `is_focusable() == true`
is a click target within its full rect. Override `hit_test_refinement()` to restrict
the clickable area:

```rust
fn hit_test_refinement(&self, x: i16, y: i16, rect: Rect) -> Option<bool> {
    // Only the left half is clickable
    Some(x < rect.x + rect.w as i16 / 2)
}
```

### Scrollbar zones

If your widget has scrollbars, implement `scrollbar_zones()` to expose them as
hit zones so the mouse system can route scrollbar interactions correctly.

---

## 12. Theme Integration

Widgets should resolve theme roles at the point where they compute their effective
styles. Do not add `ThemeProvider` arms that bake theme defaults into descendant
elements; `ThemeProvider` is a scoped provider.

For state overlays (selection, hover, focus, active), expose `StyleSlot`-style
builder triples where applicable:

| Builder | Slot mode | Behavior |
|---------|-----------|----------|
| `foo_style(style)` | Replace | Explicit user style replaces the theme role |
| `extend_foo_style(style)` | Extend | Theme role patched with user style |
| `inherit_foo_style()` | Inherit | Use scoped theme role directly |

Base styles remain partial `Style` overlays; state slots use the explicit
Replace/Extend/Inherit contract documented in `docs/styling.md`.

CI enforces this convention with:

```bash
python3 scripts/check-widget-style-slots.py
```

The guard scans `src/widgets/**/*.rs` for struct fields named `*_style` (other
than the base field `style`) typed as bare `Style` or `Option<Style>`. New
hover/focus/active/selection-style fields should use `StyleSlot`; only exact
non-state or documented legacy exceptions may be added to the script allowlist,
and each allowlist entry must include a reason.

If a state slot or intrinsic affordance makes the node hoverable, implement
`WidgetNode::is_hoverable_for_theme(&self, theme)` on the node and resolve the
slot against the active theme:

```rust
fn is_hoverable_for_theme(&self, theme: &Theme) -> bool {
    self.has_on_click()
        || self.hover_style.resolves_non_empty(theme, ThemeRole::Hover)
}
```

Use `StyleSlot::resolves_non_empty(theme, role)` instead of checking
`StyleSlot::is_empty()` for hover/focus bookkeeping; inherited slots depend on
the active `ThemeProvider` scope.

---

## 13. Documentation Updates

After implementing the widget, update these docs:

1. **`docs/widgets/<category>.md`** - Add a props table for your widget with all
   builder methods, their types, defaults, and descriptions.

2. **`docs/widgets/index.md`** - Add a row in the appropriate category table.

3. **`docs/events.md`** - If your widget emits events, add:
   - The event struct definition with field descriptions
   - A callback summary table for the widget

---

## 14. Validation Checklist

Run these after every change:

```bash
cargo build          # Compiles?
python3 scripts/check-widget-style-slots.py  # State styles use StyleSlot?
cargo clippy         # No lint warnings?
cargo fmt            # Formatted?
cargo test           # Tests pass?
```

Add tests for:
- Measurement edge cases (empty content, zero-width)
- Reconciliation (unchanged input reuses node, changed input updates it)
- Any custom logic (clamping, validation)

---

## 15. Common Failure Modes

These are the mistakes that trip up most contributors. Check each one before
submitting your widget.

### Missing manifest entry or wrong manifest category

**Symptom**: Widget gets zero or wrong size in stacks.

Your widget needs an entry in the correct `src/widget_manifest.rs` category so
generated `ElementKind::dimensions()` arms match its sizing model. Without it,
the layout engine can't determine your widget's requested size.

### Missing LayoutHash

**Symptom**: Layout recalculates on every frame even when nothing changed.

Implement `LayoutHash` on your widget struct and put the variant in a manifest
category whose `layout_hash` behavior delegates. Hash all layout-affecting fields
(width, height, padding, label, content length) but skip purely visual fields
(styles, callbacks).

### Putting click execution into gather instead of runner

**Symptom**: Side effects happen during hit-testing, causing state corruption or
double-firing.

`gather_hit_actions` in `src/app/input/mouse/gather.rs` should only identify
what was clicked and produce an action. The actual callback emission and state
mutation must happen in `src/app/runner/mouse_clicks.rs`.

### Starting drags without matching release handling

**Symptom**: Widget gets stuck in drag state; ghost drags after mouse-up.

If you start a drag on mouse-down, you must clear it on mouse-up in
`src/app/runner/events.rs`. Always pair start/end.

### Forgetting theme role resolution

**Symptom**: Widget looks unstyled when the user applies a theme; explicit user
styles work but theme defaults don't.

Resolve the relevant theme role when computing effective styles. For state
overlays, store a `StyleSlot` and expose replace/extend/inherit builder methods
instead of hard-coding a one-way patch over the theme.

For hoverable state styles, also update the node's
`is_hoverable_for_theme(&Theme)` implementation so hit-testing and hover-index
tracking agree with the renderer.

### Forgetting doc updates

New widgets need entries in:
- `docs/widgets/<category>.md` (props table)
- `docs/widgets/index.md` (catalog row)
- `docs/events.md` (if interactive)

### Missing variant plumbing

**Symptom**: Reconciliation anomalies - widget flickers or loses state between
renders.

Add `Foo` to `src/widget_manifest.rs`, then manually sync `ElementKind`,
`NodeKind`, and `render_node`. `Tag` mappings and the
`node_kind_delegate_match!` arms are generated from the manifest. Run
`python3 scripts/generate-node-kind-delegate-arms.py --write` and
`python3 scripts/check-widget-variant-parity.py`.

### Using ratatui types in public API

All `ratatui` and `crossterm` types must stay confined to `src/backend/` and
`src/app/`. The widget struct and its builder methods must use only `tui_lipan`
types (`Style`, `Color`, `Length`, `Callback`, etc.).

---

## Composite Widget Quick Reference

Composites are simpler - no node, no renderer, no wiring checklist.

```rust
// src/widgets/my_composite.rs
use crate::core::element::Element;
use crate::widgets::{Frame, Text, VStack};
use crate::style::Style;

#[derive(Clone)]
pub struct MyComposite {
    pub title: String,
    pub items: Vec<String>,
}

impl MyComposite {
    pub fn new(title: impl Into<String>) -> Self {
        Self { title: title.into(), items: Vec::new() }
    }

    pub fn items(mut self, items: Vec<String>) -> Self {
        self.items = items;
        self
    }
}

impl From<MyComposite> for Element {
    fn from(val: MyComposite) -> Self {
        Frame::new()
            .title(val.title)
            .border(true)
            .child(
                VStack::new()
                    .children(val.items.into_iter().map(|s| Text::new(s).into()))
            )
            .into()
    }
}
```

Add `mod my_composite; pub use my_composite::MyComposite;` to `src/widgets/mod.rs`.
No changes to element.rs, node/kind.rs, tag.rs, or renderers needed.

---

## Further Reading

- [`docs/components.md`](components.md) - Component lifecycle and Context API
- [`docs/events.md`](events.md) - All event/callback payload types
- [`docs/styling.md`](styling.md) - Style, Color, Length, themes
- [`docs/patterns.md`](patterns.md) - Usage patterns and anti-patterns
- [`docs/widgets/index.md`](widgets/index.md) - Full widget catalog
