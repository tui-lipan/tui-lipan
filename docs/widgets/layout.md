# Layout & Container Widgets

## Scrolling Model

Several widgets support scrolling. Understand the two modes before choosing:

### Uncontrolled (default)

Do **not** set an explicit scroll offset property. The runtime manages internal scroll state on mouse wheel, key scrolling, and scrollbar drag.

```rust
ScrollView::new()
    .scrollbar(true)   // Draggable scrollbar out of the box
    .child(long_content)
```

### Controlled

Set an explicit scroll offset property. The parent is the source of truth:

```rust
ScrollView::new()
    .offset(self.scroll)               // Controlled by parent state
    .on_scroll_to(ctx.link().callback(Msg::Scrolled))  // Update parent state
    .child(content)
```

### Scroll Callbacks

| Callback | Emits | Used by |
|----------|-------|---------|
| `on_scroll` | `ScrollEvent { offset, metrics }` | `ScrollView`, `DocumentView`, `TextArea` |
| `on_scroll_to` | `usize` (target offset) | `ScrollView`, `TextArea`, `List`, `Table` |
| `on_viewport_change` | `ScrollViewportEvent` | `ScrollView` |

---

## VStack / HStack

Vertical/Horizontal stack containers. Default sizing: `width: Flex(1)`, `height: Flex(1)`.

Layout pitfall checklist:
- Stacks consume remaining space by default. Use `Length::Px(...)` for fixed bars before giving the main content flexible space.
- A child can end up with a zero-width or zero-height rect when the parent viewport is too small or fixed siblings consume the available space.
- For headless debugging, set an explicit `TestBackend` viewport and capture with `UiSnapshotOptions::diagnostic()`; markdown flags zero-area widgets as `zero-area`.

| Prop | Type | Description |
|------|------|-------------|
| `gap` | `u16` | Space between children |
| `padding` | `impl Into<Padding>` | Inner padding |
| `align` | `Align` | Cross-axis alignment |
| `justify` | `Justify` | Main-axis packing |
| `style` | `Style` | Container style |
| `border` | `bool` | Draw border |
| `border_style` | `BorderStyle` | Border appearance |
| `focus_sizing` | `FocusSizing` | Accordion sizing behavior (includes `sticky: bool`, default `true`) |
| `focus_scope` | `FocusScope` | Subtree traversal behavior (`None`, `Exclude`, or `Contain`) |
| `tab_titles` | `Vec<String>` | Border-embedded tab titles |
| `active_tab` | `usize` | Active border tab index |
| `active_tab_style` | `Style` | Active border tab style |
| `extend_active_tab_style` / `inherit_active_tab_style` | `Style` / `()` | Extend or inherit the selection theme role for the active border tab |
| `width` | `Length` | Width override |
| `height` | `Length` | Height override |

**Accordion focus sizing:**

```rust
VStack::new()
    .focus_sizing(FocusSizing::Accordion(FocusAccordion {
        focused_min: 10,
        collapsed: 1,
        ..FocusAccordion::default()
    }))
    .child(frame_a.key("a"))
    .child(frame_b.key("b"))
```

The accordion automatically remembers the last focused child and keeps it expanded when focus moves outside the stack (`sticky: true` by default) - see [`focus.md`](../focus.md#sticky-accordion-remembering-layout-across-focus-changes).

### Pointer hit testing

`NodeTree` depth-first hit testing visits **`VStack` / `HStack` children in reverse document order** (see `depth_first_test` in `src/core/node/tree.rs`). That means the **last** child in the source list is considered **before** earlier siblings when deciding which subtree to search first.

For normal stacked layout, each child receives a **non-overlapping** rectangle on the main axis, so a given `(x, y)` lies inside **at most one** child’s `rect` and traversal order does not change the resolved target.

If you build **overlapping** siblings inside the same stack (uncommon) or need **visual layers** that all share the parent’s full rectangle (foreground decoration over content, dock under a logo, etc.), use a [`ZStack`](#zstack). Set `passthrough` when misses on non-interactive foreground layers should reach the layer below. For shape-specific pointer regions instead of layered passthrough, use `MouseRegion` hit-test refinement, as shown in `examples/burst_effects.rs`.

---

## Flow

Wrapping layout container for chip/tag-like content. `Flow` packs children left-to-right and automatically continues on the next row when a child does not fit the remaining width.

| Prop | Type | Description |
|------|------|-------------|
| `gap` | `u16` | Space between children on both axes |
| `row_gap` | `u16` | Vertical gap between wrapped rows, independent of item `gap` |
| `align` | `Align` | Cross-axis alignment for items inside each wrapped row |
| `justify` | `Justify` | Main-axis distribution of items within each wrapped row |
| `padding` | `Padding` | Inner padding around the content area |
| `border` | `bool` | Draw a border around the container |
| `border_style` | `BorderStyle` | Border style variant |
| `children` | `Vec<Element>` | Child elements to place in flow order |
| `style` | `Style` | Container style |
| `width` | `Length` | Width override |
| `height` | `Length` | Height override |
| `shrinkable` | `bool` | Yield width before normal siblings, allowing item truncation under pressure |

```rust
Flow::new()
    .gap(1)
    .align(Align::Start)
    .children(vec![
        Text::new("rust").style(Style::new().bg(Color::Blue).fg(Color::Black).bold()).into(),
        Text::new("tui-lipan").style(Style::new().bg(Color::Cyan).fg(Color::Black).bold()).into(),
        Text::new("layout").style(Style::new().bg(Color::Magenta).fg(Color::Black).bold()).into(),
    ])
```

Use `Flow` for mixed-width chips, badges, and quick filters where the number of items is dynamic and row breaks must adapt to container resizing.

`justify` distributes each row's leftover width independently: `SpaceBetween` pins the first item of every row to the left edge and the last to the right edge, `Center`/`End` shift whole rows, and `SpaceAround`/`SpaceEvenly` pad the edges too. Unlike `HStack`/`VStack`, Flow items are always measured at their natural size, so the space variants work without giving children explicit non-flex sizing:

```rust
Flow::new()
    .gap(1)
    .justify(Justify::SpaceBetween)
    .children(tags)
```

---

## ZStack

Overlay container - children stack on top of each other. Each child is laid out with the **same** bounds as the `ZStack`. The **last** child is painted on top.

| Prop | Type | Description |
|------|------|-------------|
| `style` | `Style` | Container style |
| `passthrough` | `bool` | When `true`, pointer routing can fall through non-interactive layers to lower children that share the same bounds (foreground + background pattern). When `false`, only the topmost matching layer is traversed. |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

---

## Canvas

Absolute-positioned child container. Each child is placed at a `Rect` in
Canvas-local coordinates, then clipped to the Canvas bounds. Children are painted
in declaration order, so later children appear visually on top.

| Prop | Type | Description |
|------|------|-------------|
| `child_at` | `(Rect, impl Into<Element>)` | Add a child at a Canvas-local rectangle |
| `items` | `IntoIterator<Item = CanvasItem>` | Replace all positioned children |
| `style` | `Style` | Canvas background/effect style |
| `passthrough` | `bool` | Let pointer routing pass through non-interactive top layers |
| `width` | `Length` | Requested width (default `Flex(1)`) |
| `height` | `Length` | Requested height (default `Flex(1)`) |

```rust
Canvas::new()
    .child_at(
        Rect { x: 2, y: 1, w: 26, h: 6 },
        Frame::new().title("Logs").child(log_panel),
    )
    .child_at(
        Rect { x: 18, y: 4, w: 24, h: 7 },
        Frame::new().title("Inspector").child(inspector),
    )
```

Use `Canvas` when the application owns geometry directly: floating panes,
drag previews, custom compositors, or demos that intentionally overlap children.
For normal responsive layout, prefer `VStack`, `HStack`, `Grid`, `Flow`, or
`ZStack`. For animated app-owned geometry, use `ctx.transition(...)` or a
state-owned `Transition<FloatRect>`, then pass the current value’s `.to_rect()`
to `child_at(...)`.

Pointer routing follows painter order. With the default `passthrough(false)`, a
Canvas only descends into the topmost child whose rect contains the pointer, so
overlapped regions block lower windows while exposed regions of lower windows
remain clickable. Use `passthrough(true)` for decorative overlays that should let
misses fall through to lower children.

`Canvas` is documented here because it is a layout primitive; see
`examples/window_manager.rs` for a complete tiling/floating compositor-style
example.

---

## Frame

Container with border, title, optional status line, and tab affordances.

| Prop | Type | Description |
|------|------|-------------|
| `title` | `impl Into<String>` | Frame title |
| `title_style` | `Style` | Title style |
| `focus_title_style` | `Style` | Title style when focused |
| `focus_style` | `Style` | Frame style when focused |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `hover_style` | `Style` | Frame style when hovered |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `title_align` | `Align` | Title alignment |
| `status` | `impl Into<String>` | Right-side status text |
| `status_style` | `Style` | Status text style |
| `focus_status_style` | `Style` | Status style when focused |
| `border` | `bool` | Draw border |
| `border_style` | `BorderStyle` | Border appearance |
| `border_edges` | `BorderEdges` | Border geometry (`All` or `HorizontalCaps`) |
| `border_merge_mode` | `BorderMergeMode` | `Replace` \| `Exact` \| `Fuzzy` (default `Exact`) |
| `join_frame` | `bool` | Draw junction caps when adjacent to another bordered Frame |
| `active_tab` | `usize` | Active border tab index |
| `tab_titles` | `Vec<String>` | Border-embedded tab titles |
| `active_tab_style` | `Style` | Active tab style |
| `inactive_tab_style` | `Style` | Inactive tab style |
| `focus_active_tab_style` | `Style` | Active tab when frame focused |
| `focus_inactive_tab_style` | `Style` | Inactive tab when frame focused |
| `compact` | `bool` | Single-line mode |
| `decoration` | `FrameDecoration` | Single edge overlay |
| `decorations` | `Vec<FrameDecoration>` | Multiple edge overlays |
| `padding` | `impl Into<Padding>` | Inner padding |
| `style` | `Style` | Container style |
| `width` | `Length` | Width (default `Flex(1)`) |
| `height` | `Length` | Height (default `Flex(1)`) |
| `focus_scope` | `FocusScope` | Subtree traversal behavior (`None`, `Exclude`, or `Contain`) |

**Clipping**: Children are automatically clipped to the Frame's inner content area (inside borders and padding).

**Horizontal caps**: `Frame::border_edges(BorderEdges::HorizontalCaps)` draws the top and bottom border rows with corner caps, but does not reserve left/right columns for content. Use it for lighter panel chrome when full vertical borders feel too heavy.

**Edge decorations**: With `border: false`, `DecorationPlacement::Border` still draws on the frame body edge; the layout engine reserves those cells so children (and full-width list selection) do not paint over the decoration band.

Integrated scrollbars (`ScrollbarVariant::Integrated`) treat those bands like a drawn border: a **right** or **left** `Border` decoration is the vertical track; **bottom** or **top** is the horizontal track (e.g. `TextArea` with an integrated horizontal scrollbar inside a borderless framed panel).

---

## Grid

Explicit row/column tracks with `Length` (`Auto` / `Px` / `Percent` / `Flex`), independent
horizontal and vertical gaps, row-major auto-flow for `.child(…)`, and `.cell` /
`.cell_span` for explicit placement. `Auto` tracks size to their contents; use
`Flex` tracks when you want columns or rows to absorb remaining parent space.

| Prop | Type | Description |
|------|------|-------------|
| `columns` | `[Length]` | Column track list (default one `Auto` column if omitted) |
| `rows` | `[Length]` | Row track list (default one `Auto` row if omitted) |
| `gap` | `u16` | Sets both `gap_x` and `gap_y` |
| `gap_x` / `column_gap` | `u16` | Horizontal gap between columns |
| `gap_y` / `row_gap` | `u16` | Vertical gap between rows |
| `uniform_columns(n)` | `usize` | Shorthand for `n`× `Length::Auto` columns |
| `padding` | `Padding` | Inner padding |
| `align` / `justify` | `Align` / `Justify` | Child alignment within each cell |
| `width` / `height` | `Length` | Requested size |
| `border` / `border_style` | … | Optional border |

```rust
Grid::new()
    .columns([Length::Px(20), Length::Flex(1), Length::Auto])
    .rows([Length::Auto, Length::Flex(1)])
    .gap_x(1)
    .gap_y(0)
    .child(Text::new("auto-placed"))
    .cell(1, 2, Text::new("explicit"))
    .cell_span(0, 0, 1, 3, Text::new("span"))

// Builder order for span on the last auto-placed child:
Grid::new()
    .child(Text::new("wide"))
    .span(2, 1)
```

See `examples/grid_basic.rs`.

---

## ScrollView

Scrollable container with optional scrollbar.

| Prop | Type | Description |
|------|------|-------------|
| `offset` | `Option<usize>` | Controlled scroll offset |
| `scroll_request` | `Option<ScrollRequest>` | One-shot relative scroll request (`lines`, page fractions, top, bottom) |
| `scroll_to` | `Option<ScrollTarget>` | Semantic target (`Top`, `Bottom`, `Key`, or `KeyOffset`) resolved each layout |
| `scroll_to_key` | `Option<Key>` | Convenience wrapper for `ScrollTarget::Key` |
| `scroll_to_key_offset` | `(Key, usize)` | Convenience wrapper for `ScrollTarget::KeyOffset`, useful for landing inside large keyed rows |
| `scroll_to_top` / `scroll_to_bottom` | - | Convenience wrappers for edge targets that do not need sentinel children |
| `scroll_behavior` | `ScrollBehavior` | `Instant` by default; opt into smooth target movement |
| `scroll_transition` | `TransitionConfig` | Shortcut for smooth target movement with a transition config |
| `scroll_keys` | `ScrollKeymap` | Configure keyboard scroll keys |
| `scroll_wheel` | `bool` | Enable mouse wheel scrolling |
| `scroll_wheel_multiplier` | `u16` | Override the app-wide wheel line multiplier for this ScrollView |
| `scroll_wheel_behavior` | `ScrollWheelBehavior` | `Immediate` by default; opt into inertial wheel movement |
| `smooth_wheel_scroll` | `bool` | Convenience toggle for default inertial wheel physics |
| `scroll_acceleration` | `f32` | Convenience setter that enables smooth wheel scrolling and changes the wheel impulse |
| `ambient_page_scroll` | `bool` | Opt this ScrollView into PageUp/PageDown fallback routing when no focused handler or `on_key` scope handles the key |
| `focusable` | `bool` | Whether ScrollView is focusable |
| `scrollbar` | `bool` | Show vertical scrollbar |
| `scrollbar_config` | `ScrollbarConfig` | Full vertical scrollbar configuration (variant, gap, thumb, thumb styles) |
| `axis` | `ScrollAxis` | Scroll axes: `Vertical` (default), `Horizontal`, or `Both` |
| `h_scrollbar` | `bool` | Show horizontal scrollbar when axis includes horizontal scrolling |
| `h_scrollbar_config` | `ScrollbarConfig` | Horizontal scrollbar styling (same type as `scrollbar_config`) |
| `h_scroll_wheel_multiplier` | `u16` | Horizontal wheel step (columns); falls back to `scroll_wheel_multiplier`, then the app default |
| `show_scroll_indicators` | `bool` | Show top/bottom overflow indicators |
| `scroll_indicator_style` | `Style` | Overflow indicator style |
| `estimated_child_height` | `u16` | Cold-start fallback height for unmeasured off-screen children (default `3`) |
| `on_scroll` | `Callback<ScrollEvent>` | Scroll event (includes metrics) |
| `on_scroll_to` | `Callback<usize>` | Target offset |
| `on_viewport_change` | `Callback<ScrollViewportEvent>` | Fires when the visible immediate-child snapshot changes after layout/reconcile |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

**`scrollbar_config`**: Use `ScrollbarConfig` to configure scrollbar appearance beyond the on/off toggle. It has its own builder methods:

```rust
ScrollView::new()
    .scrollbar_config(
        ScrollbarConfig::new()
            .enabled(true)
            .variant(ScrollbarVariant::Integrated)
            .thumb('▐')
            .thumb_style(Style::new().fg(Color::DarkGray))
            .thumb_focus_style(Style::new().fg(Color::Cyan))
            .gap(1)
    )
```

The same `ScrollbarConfig` type is used on all scrollable widgets (`List`, `Table`, `TextArea`, `DocumentView`, etc.).

**Clipping**: Children are automatically clipped to ScrollView's inner viewport.

**Horizontal scrolling**: Use `.axis(ScrollAxis::Both)` (or `ScrollAxis::Horizontal`) with
`.h_scrollbar(true)` when content is wider than the viewport. Children measure to their
natural width instead of stretching to the viewport width. Arrow Left/Right and
shift+mouse wheel pan horizontally; Up/Down and the vertical wheel remain on the vertical
axis when both are enabled.

With `ScrollAxis::Horizontal` the plain mouse wheel also pans horizontally, since there is
no vertical axis to disambiguate against. Once the view reaches the relevant edge the tick
is left unhandled and bubbles to an ancestor, so a horizontal strip nested inside a
vertical `ScrollView` does not trap the wheel. Shift+wheel keeps working as the explicit
horizontal override on every axis configuration.

```rust
ScrollView::new()
    .axis(ScrollAxis::Both)
    .scrollbar(true)
    .h_scrollbar(true)
    .child(wide_and_tall_content)
```

See `examples/scroll_view_both_axes.rs` for a combined vertical + horizontal demo.

**Viewport visibility**: `.on_viewport_change(...)` reports which immediate
`ScrollView` children are visible, entered, or exited. It fires after
layout/reconcile when that snapshot changes, including resize, wrapping, content
changes, and insertion/removal - not only user scroll. Put stable keys on the
immediate row children for reliable diffs across insertion/removal; descendants
are not tracked individually. Reported `content_rect`, `viewport_rect`, and
`visible_rect` values are framework `Rect`s: content-relative before offset,
effective-viewport-relative after offset/indicator rows, and clipped visible
portion respectively.

Performance note: `on_viewport_change` can fire during fast wheel scrolling or
scrollbar dragging. If the callback only mirrors `event.offset` into parent
state, return `Update::none()` from your component update handler. Use
`Update::layout()` when visible-child metadata changes the emitting component's
view or mounted subtree, such as a sticky header. Use `Update::full()` only when
the change affects other scopes or root-level composition.

**One-shot scroll requests**: Use `.scroll_request(...)` for command-driven moves without permanently controlling the settled offset.

```rust
ScrollView::new()
    .scroll_request(ScrollRequest::half_page_down())
```

For custom fractions, use `ScrollRequest::viewport_fraction(numerator, denominator)`. Positive values move down; negative values move up.

**Priority**: target scrolling (`scroll_to(...)`, `scroll_to_key(...)`,
`scroll_to_key_offset(...)`, `scroll_to_top()`, `scroll_to_bottom()`) is
framework-owned and persistent, but fresh one-shot `scroll_request(...)` values
or explicit controlled `offset(...)` changes can interrupt and suppress the
current target until the target changes.

**Smooth target scrolling**: `.scroll_behavior(ScrollBehavior::smooth_default())`,
`.scroll_behavior(ScrollBehavior::smooth_adaptive())`, or `.scroll_transition(config)`
animates semantic targets from `scroll_to(...)`, `scroll_to_key(...)`,
`scroll_to_key_offset(...)`, `scroll_to_top()`, and `scroll_to_bottom()`.
Adaptive timing derives the transition duration from the resolved row distance
and caps long jumps.
Controlled `.offset(...)`, `scroll_request(...)`, key scrolling, scrollbar drag,
and default mouse wheel scrolling remain immediate. User input cancels any active
smooth target animation.

**Smooth wheel scrolling**: `.smooth_wheel_scroll(true)` opts mouse wheel input
into inertial row movement. The default remains immediate for compatibility.
Use `.scroll_wheel_behavior(ScrollWheelBehavior::smooth(config))` for full
physics control or `.scroll_acceleration(value)` for the common acceleration
tweak.

```rust
ScrollView::new()
    .smooth_wheel_scroll(true)
    .scroll_acceleration(56.0)
```

Terminal backends expose discrete wheel up/down events rather than pixel-level
trackpad deltas. Smooth wheel scrolling treats those line events as velocity
impulses, so it can move farther than immediate mode for the same wheel input,
especially when events are repeated or coalesced. Use the default immediate mode
when each wheel step must map to an exact row delta.

Use `.scroll_wheel_multiplier(lines)` when one `ScrollView` should scroll a
different number of lines per wheel tick than the app-wide
`App::scroll_wheel_multiplier(...)` setting.

For scroll-to-edge flows, prefer built-in edge targets over invisible sentinel
children:

```rust
ScrollView::new()
    .scroll_to_bottom()
    .scroll_behavior(ScrollBehavior::smooth_adaptive())
```

Edge targets resolve against the actual content extent on each layout pass, so
they continue following top/bottom as content height changes and do not affect
layout or `gap(...)` spacing.

For precise message-list jumps, place the `Key` on the immediate `ScrollView`
child representing the message. Nested keys still use the existing behavior:
the view scrolls to the containing top-level child, not the exact descendant row.
Use `scroll_to_key_offset(key, rows)` when a large keyed row contains an
auto-height child and navigation should land inside that row. For example, a
timeline can render one auto-height `DiffView` per file, key each file card, and
scroll to `file_card_top + hunk_logical_row` for global hunk navigation.

**Ambient page scroll fallback**: Use `.ambient_page_scroll(true)` when you want `PageUp` / `PageDown` to target one explicit `ScrollView` even if it is not focused. This fallback runs only after normal focused-widget dispatch, ancestor scroll bubbling, and component `on_key` bubbling all decline the key. To avoid ambiguity, ambient page scroll activates only when exactly one mounted `ScrollView` has the flag set.

**Controlled tail alignment**: If you keep passing a tail-style controlled offset (for example `usize::MAX` or another value that stays at/beyond the current max offset), `ScrollView` stays bottom-pinned even when content grows from fully fitting the viewport to becoming scrollable on a later layout pass. If you want growth to keep the viewport at the top instead, do not pass a tail-aligned offset for those frames.

**Stable `key` + tail**: When the same logical timeline may be reparented (e.g. full-width vs `HStack` + sidebar), give the `ScrollView` a stable key as the **last** builder step (`.key("…")` on the `IntoElement` chain). The runtime records whether that key was at the scroll bottom last frame and restores tail-pinch after node-id churn, without width probes.

---

## PanView

Single-child two-dimensional viewport for wide/tall content such as diagrams.

| Prop | Type | Description |
|------|------|-------------|
| `child` | `impl Into<Element>` | Content rendered at its natural size |
| `offset` | `(i32, i32)` | Controlled `(x, y)` pan offset; negative offsets are allowed when unclamped |
| `on_pan` | `Callback<PanEvent>` | Fired after drag or keyboard panning changes the offset |
| `clamp` | `bool` | Clamp offsets to content bounds (default `true`) |
| `center_content` | `bool` | Start uncontrolled views centered until input or remembered state takes over (default `false`) |
| `free_pan_margin` | `u16` | With `clamp(false)`, limit movement so at least this many cells of content remain reachable |
| `free_pan_margins` | `(u16, u16)` | Independent horizontal/vertical free-pan margins |
| `drag_to_pan` | `bool` | Enable left-button drag panning (default `true`) |
| `keymap` / `pan_keys` | `PanKeymap` | Keyboard pan keys (`ARROWS`, `VIM`, default both) |
| `key_step` | `(u16, u16)` | Keyboard pan step as `(horizontal, vertical)` cells (default `(4, 2)`) |
| `focusable` | `bool` | Whether PanView can receive focus (default `false`; interaction callbacks can opt it in) |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `pan_state_key` | `impl Into<Key>` | Stable key for uncontrolled offset persistence |
| `width` | `Length` | Viewport width |
| `height` | `Length` | Viewport height |

```rust
PanView::new()
    .child(diagram)
    .width(Length::Flex(1))
    .height(Length::Px(20))
    .clamp(false)
    .center_content(true)
    .free_pan_margin(2)
    .key_step((4, 2))
    .pan_state_key("diagram-preview")
```

Dragging right/down decreases the offset; dragging left/up increases it. Keyboard panning defaults to a wider horizontal step than vertical step because terminal cells are taller than they are wide. With the default `clamp(true)`, offsets stay within `0..=max` for each axis. Use `.clamp(false)` for free-canvas previews where the child can be pulled past the viewport edges; add `.free_pan_margin(...)` when you want that free movement bounded instead of infinite. The child rect is translated by the negative offset and clipped to the viewport, so child hit-testing stays aligned with visible content.

When the `image` feature is enabled, input-driven PanView movement temporarily renders image children as lightweight placeholder frames while movement stabilizes. This mirrors image handling during layout changes and keeps pan interaction responsive for expensive image protocols.

---

## Center

Centers a single child both horizontally and vertically within the available area. The child is sized to its natural (minimum) dimensions; it does not expand to fill the container.

> **Note**: `Center` remains useful because it is more semantic and concise than the equivalent `VStack::new().align(Align::Center).justify(Justify::Center)` pattern, and it guarantees the child is sized naturally rather than proportionally.

```rust
Center::new().child(my_widget)
```

| Prop | Type | Description |
|------|------|-------------|
| `style` | `Style` | Container style |
| `width` | `Size` | Override centered width (`Auto`, `Fixed`, `Percent`) |
| `height` | `Size` | Override centered height |

---

## CenterPin

Pins one child to the true center of the container. The remaining space is split equally above and below, and given to `top` and `bottom` children respectively. Those zones are **collision-aware**: they never overlap the pinned child regardless of how their content changes.

This is the right widget when you need one element always at the exact middle of the screen while other content (headers, status bars, navigation, etc.) can be added or removed dynamically.

```rust
CenterPin::new()
    .top(VStack::new().child(header).child(nav))
    .center(dialog_or_textarea)
    .bottom(status_bar)
```

| Prop | Type | Description |
|------|------|-------------|
| `top` | `impl Into<Element>` | Element placed in the zone above the center child |
| `center` | `impl Into<Element>` | Element always pinned to the true center |
| `bottom` | `impl Into<Element>` | Element placed in the zone below the center child |
| `style` | `Style` | Container style (e.g. background) |

**Sizing**: defaults to `Flex(1)` on both axes - it fills its parent.

**Layout algorithm**:
1. Measure the `center` child to determine its height.
2. Place the center child at `(total_h − center_h) / 2` from the top.
3. Give everything above that position to `top`, everything below to `bottom`.

The `top` and `bottom` zones receive only what remains, so a taller center child naturally compresses both zones symmetrically.

---

## MouseRegion

Wraps any subtree to handle pointer movement, clicks, and hover visuals.

| Prop | Type | Description |
|------|------|-------------|
| `on_click` | `Callback<MouseEvent>` | Emits on left-button click (`MouseKind::Down(Left)`) |
| `on_mouse_move` | `Callback<MouseMoveEvent>` | Emits on pointer movement |
| `on_drag_start` / `on_drag` / `on_drag_end` | `Callback<MouseDragEvent>` | Left-button drag lifecycle after threshold |
| `drag_requires_mods` | `KeyMods` | Require modifiers before left-button drag callbacks can start |
| `on_right_drag_start` / `on_right_drag` / `on_right_drag_end` | `Callback<MouseDragEvent>` | Right-button drag lifecycle after threshold |
| `right_drag_requires_mods` | `KeyMods` | Require modifiers before right-button drag callbacks can start |
| `bubble_mouse_down` | `bool` | Also emit `on_mouse_down` for descendant presses without consuming them |
| `capture_click` | `bool` | If `true`, captures left-clicks before interactive children |
| `capture_requires_mods` | `KeyMods` | Capture pointer handling over descendants while modifiers are held |
| `hover_style` | `Style` | Pre-paint style applied while hovered; best for backgrounds and modifiers |
| `hover_effect` / `hover_effects` | `VisualEffect` / iterator | Post-process rendered child content while hovered |
| `hover_dim` / `hover_lighten` / `hover_tint` | `f32` / `Color, f32` | Convenience post-processing effects; `hover_tint` affects both fg and bg |
| `enabled` | `bool` | Toggle move/click handling and hover behavior |

```rust
MouseRegion::new()
    .on_click(ctx.link().callback(|e: MouseEvent| Msg::Click(e.x, e.y)))
    .capture_click(true)
    .on_mouse_move(ctx.link().callback(|e: MouseMoveEvent| {
        Msg::Hover { x: e.local_x, y: e.local_y }
    }))
    .hover_style(Style::new().bg(Color::AnsiValue(236)))
    .child(my_widget)
```

Use `bubble_mouse_down(true)` for container focus policies where the container
must learn about descendant presses but the child should still receive its click.
Use `drag_requires_mods(KeyMods::ALT)` or
`right_drag_requires_mods(KeyMods::ALT)` for compositor-style gestures that
should only start while Alt is held.
Pair those with `capture_requires_mods(KeyMods::ALT)` when the wrapped child is a
terminal or text widget, so Alt-click/Alt-drag is fully consumed by the wrapper.

`hover_style` and `hover_effect` are intentionally different layers. `hover_style` paints before the wrapped child subtree renders, so it works well for hover backgrounds and modifiers but may not recolor child text foregrounds that the child paints afterward. To recolor rendered text, use `hover_effect` with a foreground-only transform:

```rust
MouseRegion::new()
    .hover_effect(VisualEffect::transform_fg(ColorTransform::Tint(theme.text, 1.0)))
    .child(my_widget)
```

`hover_tint(color, alpha)` is a symmetric tint shortcut and blends both foreground and background toward `color`. At `alpha = 1.0`, both channels become that color.

`MouseMoveEvent` fields: `x`, `y` (terminal-space), `local_x`, `local_y` (relative to MouseRegion rect), `target_w`, `target_h`, `mods`.

> Mouse motion processing is only active when at least one move listener is present in the tree.

> `capture_click(true)` only reroutes left-button handling when this region has a left-click,
> left-down/up, or left-drag callback. Right-drag callbacks are resolved through the
> target's ancestor chain and do not require `capture_click`.
> `capture_requires_mods(...)` applies the same rerouting only while the required
> modifiers are held, and prevents terminal mouse forwarding for those events.

---

## EffectScope

Wraps any subtree and post-processes the rendered cells inside its bounds.

Use it when you want to dim an inactive pane, tint a whole section, quantize a subtree to a retro palette, or animate a composed `ZStack` after it has already rendered.

| Prop | Type | Description |
|------|------|-------------|
| `style` | `Style` | Effect style; use render-time effects like `dim_by`, `lighten_by`, `tint_by`, `transform_fg`, `transform_bg`, or `contrast_policy` |
| `effect` | `VisualEffect` | Append one declarative post-processing effect |
| `effects` | `IntoIterator<Item = VisualEffect>` | Append multiple effects in declaration order |

```rust
EffectScope::new()
    .dim_by(0.35)
    .child(sidebar)

EffectScope::new()
    .effect(VisualEffect::Monochrome { strength: 0.8 })
    .effect(VisualEffect::Scanlines {
        strength: 0.25,
        spacing: 2,
    })
    .effect(VisualEffect::RainbowWave {
        blend: 0.5,
        frequency: 1.3,
        speed: 1.0,
        axis: EffectAxis::Diagonal,
    })
    .child(content)
```

Effects are applied in insertion order. Nested `EffectScope`s compose naturally: the inner scope post-processes first, then the outer scope applies its own pass over the already-composed result.

Root-portal descendants, including default `Modal` overlays, inherit ancestor `EffectScope`s.
You can wrap the portal element itself or a container that contains it; the overlay content is
post-processed at its rendered portal bounds. If the wrapped child is just a component shell around
portal content, only the rendered portal content is affected, not the declaration-site backdrop area.

`EffectScope` affects the final rendered subtree, so explicit child colors are still transformed. Direct replacement colors like `.fg(...)` and `.bg(...)` are not used to repaint the subtree.

Built-in `VisualEffect` variants:

| Effect | Description |
|--------|-------------|
| `Dim { amount }` | Dim fg/bg colors after render |
| `Tint { color, alpha }` | Blend subtree colors toward a tint |
| `Monochrome { strength }` | Desaturate toward grayscale |
| `PaletteQuantize { palette }` | Snap colors to a small palette |
| `Scanlines { strength, spacing }` | Dim every Nth row |
| `RainbowWave { blend, frequency, speed, axis }` | Animated per-cell color wave |
| `Ripple { origin, radius, ring_width, tint, strength }` | Aspect-correct radial ring; `radius: RippleRadius` can be `Fixed`, `Loop`, or `Once`, and `origin` can be explicit cells (`EffectOrigin::cell`) or aligned from current scope bounds (`EffectOrigin::aligned`) |
| `Gradient { gradient, blend, frequency, speed, axis }` | Sine-eased mirrored `ColorGradient` sampled along `axis`; nested scopes remap independently |
| `RetroCrt { preset, flicker, scanline_strength }` | Preset built from simpler primitives |
| `Clipped { bounds, mask, inner }` | Clip / mask an inner effect (see [effects.md](effects.md)) |

`EffectPalette` presets: `Cga`, `Gameboy`, `Amber`, `Green`, `Custom(Vec<Color>)`.

`RetroPreset` presets: `Amber`, `Green`, `Cga`, `Gameboy`, `VaultTec`.

---

## Spacer

Flexible empty space. Expands to fill available space in a stack.

```rust
HStack::new()
    .child(left_content)
    .child(Spacer::new())   // Pushes right_content to the end
    .child(right_content)
```

---

## Divider

Visual separator line.

| Prop | Type | Description |
|------|------|-------------|
| `orientation` | `Orientation` | **Constructor** - `Horizontal` or `Vertical` |
| `style` | `Style` | Divider style |
| `ch` | `char` | Line glyph character |
| `label` | `Element` | Label (horizontal only) |
| `label_alignment` | `Align` | Label position along divider |
| `label_padding` | `u16` | Padding around label |
| `join_frame` | `bool` | Draw junction caps when inside a bordered Frame |

---

## Splitter

Resizable container with draggable handles between panes.

| Prop | Type | Description |
|------|------|-------------|
| `orientation` | - | Use `Splitter::horizontal()` (top/bottom) or `Splitter::vertical()` (left/right) |
| `weights` | `Vec<u16>` | Initial weight for each pane |
| `min_size` | `Vec<u16>` | Minimum size for each pane in cells |
| `handle_size` | `u16` | Handle gutter width/height |
| `handle_symbol` | `char` | Handle character |
| `handle_style` | `Style` | Handle idle style |
| `handle_hover_style` | `Style` | Handle hover style |
| `handle_active_style` | `Style` | Handle drag style |
| `handle_mode` | `SplitterHandleMode` | `Gutter` (default) or `Border` (ride the pane border seam) |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

```rust
Splitter::vertical()   // Left/Right split
    .weights(vec![30, 70])
    .min_size(vec![10, 20])
    .child(sidebar)
    .child(main_content)
```

**Handle mode**: `handle_mode(SplitterHandleMode::Border)` drops the gutter and rides the pane
border seam instead of drawing its own handle glyph. Thickness follows the borders actually
present, so this is **independent** of whether the neighboring `Frame`s merge their borders:

- panes whose `Frame::join_frame(true)` borders merge share one wall → a 1-cell handle on it,
- panes that keep **separate** borders expose two adjacent walls → a 2-cell handle grabbing both,
- borderless panes fall back to a synthetic 1-cell handle on the seam.

Note `Frame::join_frame` (border merging) is a separate, still-current visual choice owned by the frames.

**Corner drag**: when a vertical and a horizontal splitter handle meet (nested splitters),
clicking on or next to the junction grabs both handles at once - dragging moves the seam on
both axes simultaneously, like a tiling window manager corner. This is automatic; no opt-in.

---

## Animated

Wrapper for opacity, fg/bg color, height, and opt-in x/y position transitions. `height` sets the **animation target**; stacks measure that value for layout and gap math.

| Prop | Type | Description |
|------|------|-------------|
| `opacity` | `f32` | Target opacity (`0.0`…`1.0`) |
| `opacity_target` | `Option<Color>` | When `Some`, opacity blends fg (and bg unless `opacity_fg_only`) toward this color instead of the terminal/theme backdrop; target changes snap (not animated) |
| `opacity_fg_only` | `bool` | When `true`, opacity post-pass affects **foreground** only (backgrounds stay solid; use behind fixed panel fills) |
| `fg` | `Option<Color>` | Target foreground color; lerps to the target using `transition` timing |
| `bg` | `Option<Color>` | Target background color; lerps to the target using `transition` timing |
| `height` | `Length` | Target height (`Auto`, `Px`, …) |
| `layout_height` | `Option<Length>` | When `Some`, used for **stack measurement** instead of `height` (keep `Some(Length::Auto)` while collapsing so `gap` stays stable, then `None` after `on_height_transition_end`) |
| `position_transition` | `bool` | Enables FLIP-style visual x/y movement when the same keyed `Animated` node receives a new final layout rect |
| `on_opacity_transition_end` | `Callback<()>` | Fires once when an opacity transition reaches its target (including zero-duration jumps) |
| `on_height_transition_end` | `Callback<()>` | Fires once when a height transition reaches its target (including zero-duration jumps) |
| `on_position_transition_end` | `Callback<()>` | Fires once when a position transition reaches its final layout position (including zero-duration jumps) |
| `transition` | `TransitionConfig` | Duration and easing |

`opacity` applies a post-pass alpha transform that blends rendered fg/bg toward the terminal background by default (unless `opacity_fg_only` or `opacity_target` is set). At `opacity(0.0)`, the wrapper restores the cells that were already rendered underneath it, so fully faded content does not leave invisible glyphs or blank cells blocking lower `ZStack`/overlay layers. With `opacity_target`, fades go to a chosen color (fade-to-black, flash-to-accent) instead of the host backdrop. `fg` and `bg` are explicit color targets that lerp with the same transition timing. You can combine them (for example, fade + tint) in one `Animated` wrapper.

For correct opacity blending when backgrounds use `Color::Reset`, set `App::terminal_bg(query_host_colors().map(|c| c.bg))` before `run()` - see **quick-start.md** (`terminal_bg` / `query_host_colors`).

### Position transitions

Enable `.position_transition(true)` to animate a wrapper from its previous screen position to its new layout position when layout changes:

```rust
Animated::new(card)
    .position_transition(true)
    .transition(transition)
    .key("card-42")
```

Position transitions use FLIP semantics: reconciliation computes the new final layout immediately, then rendering applies a temporary visual offset that eases back to zero. This is **visual-only** and paint-only; it does not progressively mutate layout, measurement, focus order, scroll math, or pointer geometry.

The `Animated` wrapper must keep a stable `.key(...)` across reorders or reparenting within the same reconciled branch. Without a stable key, the runtime treats the moved item as a new node and the first mount snaps to its final position instead of animating. This is not a global shared-element system: it animates the same keyed `Animated` node's own rect changes, not unmount/remount matches across unrelated branches.

Hit testing, hover, drag/drop local coordinates, scrollbar zones, and focus traversal use the final `node.rect` for the whole transition. A card may be visibly between two positions, but clicks are recognized at the destination layout position, not at the temporary visual offset.

Clipping remains anchored in parent layout space. A moving child inside a `ScrollView` or `Frame` is still clipped to that parent viewport, while the `Animated` wrapper's own child clip follows the moving visual box. Opacity and color post-passes use the same visual rect as the moved subtree, so fades/tints continue to cover the rendered cells during motion.
