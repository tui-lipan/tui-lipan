# Enum & Type Reference

Quick reference for all public enums and types used in widget props and API calls.

> **Convention**: When a prop table shows a type like `ButtonVariant`, look it up here.

---

## Text Coordinates

### `TextPosition`

Zero-based logical text coordinate:

| Field | Type | Notes |
|-------|------|-------|
| `line` | `usize` | Logical line index |
| `column` | `usize` | Unicode-scalar column by default |

### `TextRange`

Half-open text range with `start: TextPosition` and `end: TextPosition`.

### `TextEncoding`

Column encoding for `LineIndex` conversions:

| Variant | Meaning |
|---------|---------|
| `Utf8` | Column is a byte offset from the line start |
| `Utf16` | Column is a UTF-16 code-unit offset from the line start |
| `UnicodeScalar` | Column is a Unicode scalar count from the line start (default) |

### `LineIndex`

Snapshot helper for converting between canonical byte offsets and
`TextPosition` / `TextRange`. Rebuild it when the underlying text changes.

---

## UI Snapshots (agent / design review)

### `UiWidgetKind`

Typed widget tag on each `UiWidgetDesc` entry (`Frame`, `List`, `Input`, …). Implements `Display`.

### `UiWidgetDesc`

| Field | Type | Notes |
|-------|------|-------|
| `kind` | `UiWidgetKind` | Widget type |
| `key` | `Option<Key>` | Reconciliation key |
| `rect` | `Rect` | Layout bounds |
| `focused` / `hovered` | `bool` | Interaction state |
| `title` / `label` / `value` | `Option<String>` | Semantic text |
| `placeholder` | `Option<String>` | Input placeholder (distinct from `label`) |
| `value_masked` | `bool` | When true, `value` is intentionally omitted |
| `checkbox_state` | `Option<CheckboxState>` | Tri-state checkbox value |
| `selected_index` / `scroll_offset` | `Option<usize>` | List/tab selection and scroll |
| `item_labels` / `total_items` | `Option<…>` | List/table preview; `total_items` set when labels truncated |
| `child_count` | `Option<usize>` | Structural containers |

### `UiSnapshot`

Combined `CapturedFrame` + `widgets` + `focus_key` / `hover_key`. Methods: `to_markdown()`; with `ui-snapshot-json` feature: `to_json()`, `to_json_pretty()`; with `ui-snapshot-png` feature: `to_png(&PngOptions)`, `to_png_default()`, `try_to_png(&PngOptions)`, `try_to_png_default()`.

Headless: `TestBackend::capture_ui_snapshot()` after `render()`. Live: `Context::request_ui_snapshot_to(path)` and `request_ui_snapshot_to_slot(&UiSnapshotSlot)` — delivered **after the next paint**.

### `PngOptions` (`ui-snapshot-png`)

Options for `CapturedFrame::to_png(&PngOptions)` / `try_to_png(&PngOptions)` and `UiSnapshot::to_png(&PngOptions)` / `try_to_png(&PngOptions)`.

`PngOptions` and `PngTextRenderer` are exported from the crate root, not the prelude:

```rust
#[cfg(feature = "ui-snapshot-png")]
use tui_lipan::{PngOptions, PngTextRenderer};
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `cell_width` | `u16` | `8` | Cell width in pixels before scaling |
| `cell_height` | `u16` | `16` | Cell height in pixels before scaling |
| `scale` | `u16` | `2` | Output cell scale multiplier |
| `default_fg` | `Color` | `Color::White` | Fallback when a cell foreground resolves to reset/transparent |
| `default_bg` | `Color` | `Color::Black` | Fallback when a cell background resolves to reset/transparent/backdrop |
| `render_cursor` | `bool` | `true` | Draw the captured cursor outline when visible |
| `text_renderer` | `PngTextRenderer` | `Auto` | `Auto` uses fonts when found and falls back to bitmap; `Font` tries font rendering first with the same fallback; `Bitmap` forces coarse cell glyphs |
| `font_family` | `Option<Arc<str>>` | `None` | Preferred system font family, e.g. a Nerd Font |
| `font_path` | `Option<PathBuf>` | `None` | Explicit font file path; takes precedence over family lookup |

### `PngTextRenderer` (`ui-snapshot-png`)

Controls PNG text rasterization: `Auto` (default) uses fontdue/fontdb to render
antialiased real-font text when a system font is found, then falls back to the
built-in font8x8 bitmap renderer; `Font` requests font rendering with the same
family/path selection; `Bitmap` forces deterministic coarse cell rendering.
Use `font_family` or `font_path` for system/Nerd Font captures, and force
`Bitmap` when stable fallback-style screenshots matter more than glyph fidelity.

---

## Input Keymaps

### `PanKeymap`

Bitflag-style key set for `PanView`: `NONE`, `ARROWS`, `VIM`, and `DEFAULT` (`ARROWS | VIM`). Combine sets with `|` and test with `.contains(...)`. `VIM` includes `h/j/k/l` cardinal panning.

### `FrameworkAction`

Framework-owned actions configurable from Rust via `FrameworkKeymap` and `App::framework_keymap(...)`. Maps to internal keymap actions after file/env/user bindings are applied.

| Variant | Default binding (typical) | `FrameworkKeymap` use |
|---------|---------------------------|------------------------|
| `Quit` | `ctrl-q` | `.unbind(FrameworkAction::Quit)` or rebind |
| `DismissOverlay` | `esc` | Overlay dismissal |
| `FocusNext` | `tab` | Tab traversal |
| `FocusPrev` | `shift-tab` | Reverse tab traversal |
| `ToggleDevTools` | `f12` | DevTools panel (requires `devtools` feature at runtime) |

Sugar: `App::global_quit(None)` unbinds quit without touching other framework actions.

### `FrameworkKeymap`

Builder for Rust-side framework binding overrides. Applied **after** user keymap files and built-in defaults. Methods: `.bind(action, KeyBindings)`, `.unbind(action)`.

### `UserKeymapPolicy`

| Variant | Behavior |
|---------|----------|
| `Enabled` **(default)** | Load `App::keymap_path`, `TUI_LIPAN_KEYMAP`, or default user keymap |
| `Disabled` | Ignore user keymap files; built-in defaults and Rust `FrameworkKeymap` still apply |

### `KeyDispatchPolicy`

Non-terminal focus ordering between widgets and app command shortcuts.

| Variant | Behavior |
|---------|----------|
| `WidgetFirst` **(default)** | Focused widget and bubble run before app command shortcuts |
| `AppCommandsFirst` | App command shortcuts run before focused widget handlers (command chords still first) |

### `TerminalKeyPolicy`

Terminal-focused key ordering. See [`widgets/terminal.md`](widgets/terminal.md).

| Variant | Summary |
|---------|---------|
| `FrameworkFirst` **(default)** | Framework shortcuts before terminal passthrough |
| `AppCommandsThenTerminal` | Mux-style: terminal copy/paste preflight, then app commands, then PTY |
| `TerminalFirst` | Terminal forwarding before app commands |
| `TerminalOnly` | No app command or framework fallback while terminal is focused |

### `CommandConflictPolicy`

Resolves duplicate executable shortcuts on `CommandEntry`.

| Variant | Behavior |
|---------|----------|
| `FirstRegistered` **(default)** | Stable registration order among equal priorities |
| `HighestPriority` | Highest `CommandEntry::priority(i32)`, then first registered |

### `ChordMismatchPolicy`

Behavior when a key fails to complete a pending app command chord.

| Variant | Behavior |
|---------|----------|
| `SwallowPrefixReplayCurrent` **(default)** | Swallow the prefix; retry the mismatching key as a fresh dispatch |
| `ForwardPrefixAndCurrent` | Forward both prefix and mismatching key to lower-priority sinks |
| `CancelOnly` | Cancel pending command state; treat mismatch as unhandled by commands |

---

## Layout & Sizing

### `Length`

| Variant | Meaning | Default for |
|---------|---------|-------------|
| `Length::Auto` | Size to content | Leaf widgets (Text, Button, Input, etc.) |
| `Length::Px(u16)` | Fixed cell count | - |
| `Length::Percent(u16)` | Percentage of available space (clamped to `0..=100`) | - |
| `Length::Flex(u16)` | Proportional share of remaining space | Containers (VStack, HStack, Frame) |

### `ShrinkPriority`

Controls stack shrink order for widgets that opt into custom layout constraints.

| Variant | Effect |
|---------|--------|
| `ShrinkPriority::Normal` | Default shrink order |
| `ShrinkPriority::First` | Yield space before normal siblings, for lower-priority reflowing groups |

### `Align` (cross-axis)

| Variant | Effect |
|---------|--------|
| `Align::Start` | Top/left **(default)** |
| `Align::Center` | Centered |
| `Align::End` | Bottom/right |
| `Align::Stretch` | Fill available space |

### `Justify` (main-axis)

| Variant | Effect |
|---------|--------|
| `Justify::Start` | Pack toward start **(default)** |
| `Justify::Center` | Center in available space |
| `Justify::End` | Pack toward end |
| `Justify::SpaceBetween` | Even space between children (none at edges) |
| `Justify::SpaceAround` | Even space around each child |
| `Justify::SpaceEvenly` | Equal space between and around children |

### `Orientation`

| Variant | Usage |
|---------|-------|
| `Orientation::Horizontal` | Horizontal divider, horizontal splitter |
| `Orientation::Vertical` | Vertical divider, vertical splitter |

### `SplitterHandleMode`

Where a `Splitter` places its drag handles relative to pane borders. This is independent of whether neighboring frames merge their borders.

| Variant | Description |
|---------|-------------|
| `SplitterHandleMode::Gutter` | Reserve a gutter between panes and draw the handle glyph there **(default)** |
| `SplitterHandleMode::Border` | Drop the gutter and ride the pane border seam (thickness adapts to borders actually present) |

### `Padding`

Create via conversion:

```rust
Padding::from(1u16)               // uniform: all sides = 1
Padding::from((2u16, 1u16))       // (vertical, horizontal)
Padding::from((1u16, 2u16, 1u16, 2u16))  // (top, right, bottom, left)
```

Or use the `.padding(...)` builder which accepts `impl Into<Padding>`:

```rust
.padding(1)           // uniform
.padding((2, 1))      // (vertical, horizontal)
.padding((1, 2, 1, 2))  // (top, right, bottom, left)
```

### `FloatRect`

Fractional terminal-cell rectangle with `x`, `y`, `w`, and `h` fields as `f32`.
`Transition<FloatRect>` interpolates geometry field-by-field; use `.to_rect()`
to round and clamp the current value before passing it to widgets such as
`Canvas`.

---

## Visual Style

### `BorderStyle`

| Variant | Appearance |
|---------|------------|
| `BorderStyle::Plain` | `─ │ ┌ ┐ └ ┘` **(default)** |
| `BorderStyle::Rounded` | `─ │ ╭ ╮ ╰ ╯` |
| `BorderStyle::Double` | `═ ║ ╔ ╗ ╚ ╝` |
| `BorderStyle::Thick` | `━ ┃ ┏ ┓ ┗ ┛` |
| `BorderStyle::LightDoubleDashed` | Dashed light border |
| `BorderStyle::HeavyDoubleDashed` | Dashed heavy border |
| `BorderStyle::LightTripleDashed` | Triple-dashed light |
| `BorderStyle::HeavyTripleDashed` | Triple-dashed heavy |
| `BorderStyle::LightQuadrupleDashed` | Quadruple-dashed light |
| `BorderStyle::HeavyQuadrupleDashed` | Quadruple-dashed heavy |
| `BorderStyle::Custom { glyphs }` | Custom glyph set via `BorderGlyphs` |

### `BorderEdges`

| Variant | Effect |
|---------|--------|
| `BorderEdges::All` | Reserve and render all four border edges **(default)** |
| `BorderEdges::HorizontalCaps` | Reserve only top/bottom rows and render corner caps; left/right content columns are not consumed |

### `BorderMergeMode`

Strategy used when frame border symbols overlap (e.g. adjacent or overlapping frames).

| Variant | Description |
|---------|-------------|
| `BorderMergeMode::Replace` | Last write wins; no symbol merging (clean overlap override) |
| `BorderMergeMode::Exact` | Merge only when an exact box-drawing intersection symbol exists **(default)** |
| `BorderMergeMode::Fuzzy` | Merge using the closest matching symbol when an exact merge symbol is unavailable |

### `Overflow`

| Variant | Effect |
|---------|--------|
| `Overflow::Auto` | Widget-specific default overflow behavior |
| `Overflow::Clip` | Clip overflowing content at the end |
| `Overflow::ClipStart` | Clip from the start, keeping the tail visible |
| `Overflow::Ellipsis` | Truncate overflowing content with `…` |
| `Overflow::Wrap` | Soft-wrap content to the available width |

### `CaretShape`

| Variant | Description |
|---------|-------------|
| `CaretShape::Block` | Block cursor (█) **(default - do not set explicitly)** |
| `CaretShape::Bar` | Vertical bar cursor (│) |
| `CaretShape::Underline` | Underline cursor (_) |

### `ScrollbarVariant`

| Variant | Description |
|---------|-------------|
| `ScrollbarVariant::Standalone` | Separate column consuming content width **(default)** |
| `ScrollbarVariant::Integrated` | Integrate into right border (lazygit-style) |

### `ScrollBehavior`

Controls programmatic target scrolling for `ScrollView::scroll_to`,
`ScrollView::scroll_to_key`,
`DocumentView::scroll_to_source_line`, and `TextArea::scroll_to_line`.

| Variant | Description |
|---------|-------------|
| `ScrollBehavior::Instant` | Snap directly to the resolved target row **(default)** |
| `ScrollBehavior::Smooth(TransitionConfig)` | Animate to the target row using the provided transition timing |
| `ScrollBehavior::SmoothDistance(ScrollDistanceConfig)` | Animate to the target row with duration derived from row distance |

Use `ScrollBehavior::smooth_default()` for a fixed default transition, or
`ScrollBehavior::smooth_adaptive()` for distance-based timing. Overshooting easing
curves are clamped to the start/end row range so terminal scroll offsets do not
jitter past their target.

### `ScrollWheelBehavior`

Controls user mouse-wheel scrolling for `ScrollView`.

| Variant | Description |
|---------|-------------|
| `ScrollWheelBehavior::Immediate` | Apply wheel steps as discrete line jumps **(default)** |
| `ScrollWheelBehavior::Smooth(ScrollWheelConfig)` | Add wheel steps to an inertial velocity and decay it over animation ticks |

Use `ScrollView::smooth_wheel_scroll(true)` for the default smooth physics, or
`ScrollWheelBehavior::smooth(config)` when passing physics as data.

### `ScrollTarget`

Semantic target for framework-owned `ScrollView` navigation.

| Variant | Description |
|---------|-------------|
| `ScrollTarget::Top` | Resolve to the current top edge |
| `ScrollTarget::Bottom` | Resolve to the current bottom extent |
| `ScrollTarget::Key(Key)` | Resolve to the first child subtree containing the key |
| `ScrollTarget::KeyOffset { key, offset }` | Resolve to the first child subtree containing the key, then add `offset` rows |

Use `ScrollView::scroll_to_bottom()` / `scroll_to_top()` for edge targets, or
`ScrollView::scroll_to(ScrollTarget::Bottom)` when passing the target as data.

### `ScrollChildVisibility`

Used by `ScrollViewportEvent` for immediate `ScrollView` children.

| Variant | Description |
|---------|-------------|
| `ScrollChildVisibility::FullyVisible` | The child rect is fully inside the effective viewport |
| `ScrollChildVisibility::PartiallyVisible` | The child is clipped by the effective viewport |

### `ScrollChildExitDirection`

Used by `ScrollExitedChild` when a previously visible immediate `ScrollView`
child leaves the viewport.

| Variant | Description |
|---------|-------------|
| `ScrollChildExitDirection::Above` | The child is now fully above the viewport |
| `ScrollChildExitDirection::Below` | The child is now fully below the viewport |
| `ScrollChildExitDirection::Removed` | The child identity is gone or no longer has measurable geometry |

### `ScrollDistanceConfig`

Distance-based timing for smooth target scrolling. Duration is computed as
`min_duration + duration_per_row * distance_rows`, then capped by
`max_duration`. Defaults: `120ms` minimum, `700ms` maximum, `8ms` per row,
`Easing::EaseOutQuad`.

| Field | Description |
|-------|-------------|
| `min_duration` | Baseline duration for non-zero jumps |
| `max_duration` | Cap for long jumps |
| `duration_per_row` | Added duration per resolved target row |
| `easing` | Easing curve for the generated transition |

### `ScrollWheelConfig`

Physics parameters for opt-in smooth wheel scrolling. Defaults: `40.0`
acceleration, `12.0` deceleration, `320.0` max velocity, and `0.05` stop
velocity, all in content rows/second terms.

| Field | Description |
|-------|-------------|
| `acceleration` | Velocity impulse added per wheel line |
| `deceleration` | Exponential velocity decay per second; higher values stop sooner |
| `max_velocity` | Absolute velocity clamp |
| `stop_velocity` | Velocity threshold below which inertial scrolling settles |

### `ColorTransform`

Used with `Style::transform_fg(...)` and `Style::transform_bg(...)`.

| Variant | Description |
|---------|-------------|
| `ColorTransform::Dim(f32)` | Dim the resolved color toward black by `0.0..=1.0` |
| `ColorTransform::Lighten(f32)` | Lighten the resolved color toward white by `0.0..=1.0` |
| `ColorTransform::Opacity(f32)` | Compose the resolved paint alpha with the factor; `1.0` keeps the paint, `0.0` resolves to the backdrop for that channel |
| `ColorTransform::OpacityToward { factor, target }` | Same factor semantics as `Opacity`, but blend toward `target` instead of the backdrop |
| `ColorTransform::Tint(Color, f32)` | Blend the resolved color toward a target color by alpha |

### `Paint`

Alpha-aware style-channel color. `Style::fg`, `Style::bg`, and
`Style::underline_color` store `Option<Paint>` and accept `Color` directly via
`From<Color>`.

| Variant | Meaning |
|---------|---------|
| `Paint::Solid(Color)` | Opaque terminal color or semantic sentinel |
| `Paint::Alpha { color, alpha }` | Source pigment with `0..=255` alpha, composited before terminal output |

Construct with `Paint::solid(Color)`, `Paint::rgb(r,g,b)`,
`Paint::rgba(r,g,b,a)`, or `Paint::hex("#RRGGBBAA")`. `Color::Transparent` and
`Color::Backdrop` keep their sentinel meanings only when used as solid paint;
`Paint::Alpha { alpha: 0, .. }` is an alpha paint that preserves the backdrop for
that channel, not the transparent sentinel.

### `ThemeRole`

Semantic roles resolved by `Theme::role(...)` and `StyleSlot` state overlays.

| Variant | Resolves from / purpose |
|---------|-------------------------|
| `ThemeRole::Base` | Default widget text/surface style (`theme.primary`) |
| `ThemeRole::Accent` | Interactive accent/emphasis, falling back to primary foreground |
| `ThemeRole::Selection` | Selected/current item style (`theme.selection`) |
| `ThemeRole::TextSelection` | Text/range selection style (`theme.text_selection`) |
| `ThemeRole::UnfocusedSelection` | Unfocused selection style; currently follows `Selection` |
| `ThemeRole::Hover` | Genuine pointer-hover state (`theme.hover`) |
| `ThemeRole::DragSource` | Drag-source active overlay; currently follows `Hover` for compatibility |
| `ThemeRole::DropTarget` | Future inactive drop-zone affordance; currently follows `Hover` |
| `ThemeRole::DropTargetActive` | Compatible-drag-over-target highlight; currently follows `Hover` |
| `ThemeRole::Focus` | Focused widget chrome (`theme.focus`) |
| `ThemeRole::Active` | Active/current state; currently follows `Selection` |
| `ThemeRole::ItemHover` | Per-row/per-item hover; currently follows `Hover` |
| `ThemeRole::Border` | Frame/divider border role (`primary.patch(border)`) |
| `ThemeRole::Disabled` | Disabled widget content (`primary.patch(muted)`) |
| `ThemeRole::Muted` | Secondary content (`primary.patch(muted)`) |
| `ThemeRole::Error` | Error/status color |
| `ThemeRole::InputFocusContent` | Focused text content for `Input` |
| `ThemeRole::TextAreaFocusContent` | Focused content for `TextArea` |
| `ThemeRole::DocumentViewFocusContent` | Focused content for `DocumentView` |
| `ThemeRole::HexAreaFocusContent` | Focused content for `HexArea` |
| `ThemeRole::HexAreaCursor` | Hex-area cursor style |
| `ThemeRole::TerminalFocusContent` | Focused terminal content |
| `ThemeRole::ScrollbarThumb` | Scrollbar thumb color |
| `ThemeRole::ScrollbarThumbFocus` | Focused scrollbar thumb color |
| `ThemeRole::ScrollbarTrack` | Scrollbar track color |
| `ThemeRole::SplitterHover` | Splitter hover handle color |
| `ThemeRole::SplitterActive` | Splitter active handle color |

### `VisualEffect`

Used with `EffectScope::effect(...)`, `EffectScope::effects(...)`, and `MouseRegion::hover_effect(...)`.

| Variant | Description |
|---------|-------------|
| `VisualEffect::Monochrome { strength }` | Desaturate fg/bg colors toward grayscale |
| `VisualEffect::PaletteQuantize { palette }` | Quantize fg/bg colors to an effect palette |
| `VisualEffect::Scanlines { strength, spacing }` | Dim every `spacing` rows |
| `VisualEffect::RainbowWave { blend, frequency, speed, axis }` | Animated color wave sampled in scope-local coordinates |
| `VisualEffect::Gradient { gradient, blend, frequency, speed, axis }` | Mirrored `ColorGradient` wash sampled in scope-local coordinates |
| `VisualEffect::RetroCrt { preset, flicker, scanline_strength }` | Retro CRT preset with palette, scanlines, and optional flicker |
| `VisualEffect::Ripple { origin, radius, ring_width, tint, strength }` | Aspect-correct radial tint ring from an explicit or aligned `EffectOrigin`; `radius` is a `RippleRadius` |
| `VisualEffect::Clipped { bounds, mask, inner }` | Restrict another effect to a scope-local rect and/or mask |
| `VisualEffect::ColorTransform { fg, bg }` | Apply relative `ColorTransform`s to fg and/or bg |
| `VisualEffect::ContrastPolicy(policy)` | Apply readable-foreground contrast adjustment |
| `VisualEffect::Custom(Arc<dyn CellEffect>)` | User-defined per-cell effect; can optionally prepare frame-constant state with `CellEffect::prepare` |

### `EffectOrigin`

Used by positional effects such as `VisualEffect::Ripple`.

| Variant | Description |
|---------|-------------|
| `EffectOrigin::Cell { x, y }` | Explicit scope-local cell coordinates; use `EffectOrigin::cell(x, y)` |
| `EffectOrigin::Aligned(EffectAlignment)` | Resolve from the current effect-scope bounds at render time |

### `EffectAlignment`

Two-dimensional alignment for `EffectOrigin::Aligned`. Common constants: `TOP_LEFT`, `TOP_CENTER`, `TOP_RIGHT`, `CENTER_LEFT`, `CENTER`, `CENTER_RIGHT`, `BOTTOM_LEFT`, `BOTTOM_CENTER`, `BOTTOM_RIGHT`.

### `RippleRadius`

Used by `VisualEffect::Ripple`.

| Variant | Description |
|---------|-------------|
| `RippleRadius::Fixed(f32)` | Static ring radius in character columns |
| `RippleRadius::Loop { max_radius, period_ticks }` | Repeating ease-out shockwave with implicit linear strength fade |
| `RippleRadius::Once { max_radius, duration_ticks, start_tick }` | One-shot ease-out shockwave; capture `start_tick` from `ctx.effect_phase()` when the burst starts |

### `Color` (selected variants)

Full listing: named ANSI colors, `Indexed(u8)`, `Rgb`, `hex` / `hex_u24` helpers. Highlights:

| Variant / form | Description |
|----------------|-------------|
| `Color::Reset` | Terminal default for that attribute (ANSI reset) |
| `Color::Backdrop` | Background-only surface semantic: blank areas clear foreground content but preserve the background color already beneath them |
| `Color::Transparent` | Omit fg/bg when rendering so cells keep the color underneath; in `Style::patch`, does not override the resolved base for that channel |

### `TripleClickSelectionMode`

Used by `TextArea::triple_click_mode(...)` and `DocumentView::triple_click_mode(...)`.

| Variant | Description |
|---------|-------------|
| `TripleClickSelectionMode::Line` | Select the current logical/rendered line **(default)** |
| `TripleClickSelectionMode::Paragraph` | Select the current paragraph bounded by blank lines |

### `HeatmapCellMode`

| Variant | Description |
|---------|-------------|
| `HeatmapCellMode::Background` | Fill each cell with a background color and optional numeric text **(default)** |
| `HeatmapCellMode::Glyph(Arc<str>)` | Draw a centered glyph string over a colored background tile |
| `HeatmapCellMode::GlyphForeground(Arc<str>)` | Draw only the glyph string in the mapped color, leaving the background untouched |

### `HeatmapLegendWidth`

| Variant | Description |
|---------|-------------|
| `HeatmapLegendWidth::Grid` | Align the legend with the heatmap grid start **(default)** |
| `HeatmapLegendWidth::Full` | Let the legend span the full inner width, including the row-label gutter |

### `ActorKind` *(SequenceDiagram)*

| Variant | Description |
|---------|-------------|
| `ActorKind::Participant` | Render as a participant box **(default)** |
| `ActorKind::Actor` | Render as a Mermaid-style stick-figure actor with a label |

### `SequenceDiagramVariant` *(SequenceDiagram)*

| Variant | Description |
|---------|-------------|
| `SequenceDiagramVariant::Boxed` | Render participant headers/footers with boxes **(default)** |
| `SequenceDiagramVariant::Minimal` | Render compact unboxed participant labels; actor labels use `actor_glyph` or the `"○ "` fallback |

### `SequenceDiagramTheme` *(SequenceDiagram)*

`SequenceDiagramTheme` is a public struct for diagram-local styles and glyphs.
Use `SequenceDiagramTheme::classic()` for the default look,
`SequenceDiagramTheme::minimal()` for the compact preset, and
`SequenceDiagramTheme::ascii()` when output must avoid Unicode box-drawing
characters. The theme contains public sub-structs for the customizable glyph and
style groups: `MessageGlyphs`, `FragmentGlyphs`, `LifelineTheme`,
`ActivationTheme`, and `AutonumberTheme`, plus participant/note border slots for
diagram-local chrome.

`MessageStyle` and `FragmentKind` index the per-kind style slots used by
`SequenceDiagram::message_kind_style(...)` and
`SequenceDiagram::fragment_kind_style(...)`.

### Flowchart enums

| Type | Variants |
|------|----------|
| `FlowDirection` | `TopDown` **(default)**, `BottomUp`, `LeftRight`, `RightLeft` |
| `NodeShape` | `Rect` **(default)**, `Round`, `Stadium`, `Subroutine`, `Cylinder`, `Circle`, `Asymmetric`, `Diamond`, `Hexagon`, `Parallelogram`, `ParallelogramAlt`, `Trapezoid`, `TrapezoidAlt`, `DoubleCircle` |
| `EdgeStyle` | `Solid` **(default)**, `Dashed`, `Thick`, `Invisible` |
| `EdgeArrow` | `None`, `Open`, `Filled` **(default)**, `Cross`, `Circle` |

`FlowchartTheme` is a diagram-local glyph/style bundle with `classic()`,
`minimal()`, and `ascii()` presets.

### Gantt diagram enums

| Type | Variants |
|------|----------|
| `GanttTaskStatus` | `Pending` **(default)**, `Active`, `Done`, `Critical` |
| `GanttTaskStart` | `Date(GanttDate)`, `After(Arc<str>)` |

`GanttDuration::days(n)` stores day-based task lengths. `GanttTask::milestone()`
marks a zero-duration task rendered as a milestone glyph.

### `MessageStyle` *(SequenceDiagram)*

| Variant | Mermaid form | Description |
|---------|--------------|-------------|
| `MessageStyle::Sync` | `->` / `->>` | Solid request/call arrow |
| `MessageStyle::Async` | `-)` / async arrow | Solid asynchronous/open-head arrow |
| `MessageStyle::SyncReply` | `-->` | Dashed reply arrow with filled head |
| `MessageStyle::AsyncReply` | `-->>` | Dashed reply arrow with open head |
| `MessageStyle::Lost` | `-x` | Message ending in a lost/error marker |
| `MessageStyle::Open` | `-)` | Message ending in an open circle marker |

Prefer constructor helpers such as `SequenceMessage::sync(...)`,
`SequenceMessage::async_(...)`, and `SequenceMessage::reply(...)` unless you need to set a
style directly.

### `FragmentKind` *(SequenceDiagram)*

| Variant | Description |
|---------|-------------|
| `FragmentKind::Loop` | Repeated block (`loop`) |
| `FragmentKind::Alt` | Conditional block with `else` branches (`alt`) |
| `FragmentKind::Opt` | Optional block (`opt`) |
| `FragmentKind::Par` | Parallel block with `and` branches (`par`) |
| `FragmentKind::Critical` | Critical section (`critical`) |
| `FragmentKind::Break` | Break/abort block (`break`) |
| `FragmentKind::Rect` | Background rectangle region (`rect`) |

### `NotePlacement` *(SequenceDiagram)*

| Variant | Description |
|---------|-------------|
| `NotePlacement::LeftOf` | Note box to the left of one actor |
| `NotePlacement::RightOf` | Note box to the right of one actor |
| `NotePlacement::Over` | Note box spanning one or more actors |

---

## Widget Variants

### `SpinnerStyle`

| Variant | Frames |
|---------|--------|
| `SpinnerStyle::Dots` **(default)** | `⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏` |
| `SpinnerStyle::Line` | Four-frame line spinner |
| `SpinnerStyle::Circle` | `◐◓◑◒` |
| `SpinnerStyle::Arc` | `◜◠◝◞◡◟` |
| `SpinnerStyle::Braille` | `⣾⣽⣻⢿⡿⣟⣯⣷` |
| `SpinnerStyle::Moon` | `🌑🌒🌓🌔🌕🌖🌗🌘` |
| `SpinnerStyle::Box` | `▖▘▝▗` |
| `SpinnerStyle::Bar` | ` ▂▃▄▅▆▇█▇▆▅▄▃▂ ` |
| `SpinnerStyle::Arrow` | `←↖↑↗→↘↓↙` |
| `SpinnerStyle::Fade` | `█▓▒░▒▓` |
| `SpinnerStyle::Trail` | Moving shaded trail |
| `SpinnerStyle::Earth` | `🌍🌎🌏` |
| `SpinnerStyle::Claude` | `·✢✳✶✻*✻✶✳✢` |
| `SpinnerStyle::OpenCode` | Custom OpenCode-style glowing track |
| `SpinnerStyle::ThreeDot` | Three-dot chase |
| `SpinnerStyle::ThreeDotFade` | Three-dot chase with trail |
| `SpinnerStyle::SquareFade` | Square fill/fade |
| `SpinnerStyle::Lightsaber` | Custom lightsaber ignition/retraction |

### `SpinnerSpeed`

| Variant | Description |
|---------|-------------|
| `SpinnerSpeed::Slow` | Approx. 200 ms per frame |
| `SpinnerSpeed::Normal` **(default)** | Approx. 100 ms per frame |
| `SpinnerSpeed::Fast` | Approx. 50 ms per frame |
| `SpinnerSpeed::Custom { frame_ms }` | Custom milliseconds per frame, quantized to the runtime spinner tick |

### `DraggableTabKind`

| Variant | Description |
|---------|-------------|
| `DraggableTabKind::Tab` | Regular selectable, draggable tab **(default)** |
| `DraggableTabKind::Action` | Pinned action item, such as a `+` new-tab button, that emits `on_action` instead of selecting or reordering |

### `DraggableTabBarVariant`

| Variant | Description |
|---------|-------------|
| `DraggableTabBarVariant::Bordered` | Segmented tabs with dividers **(default)** |
| `DraggableTabBarVariant::FrameLine` | One-line frame-like tabs with accent markers |

### `DraggableTabBarOverflow`

| Variant | Description |
|---------|-------------|
| `DraggableTabBarOverflow::Scroll` | Keep natural tab widths and scroll horizontally when tabs overflow **(default)** |
| `DraggableTabBarOverflow::ShrinkThenScroll { min_tab_width }` | Shrink tab labels down to the configured minimum tab width before scrolling |

### `DragReorderMode`

| Variant | Description |
|---------|-------------|
| `DragReorderMode::Live` | Emit reorder events as the drag crosses tab boundaries **(default)** |
| `DragReorderMode::OnDrop` | Emit one reorder event when the mouse is released |

### `ButtonVariant`

| Variant | Rendered as | Constructor shortcut |
|---------|-------------|---------------------|
| `ButtonVariant::Bracket` | `[ Label ]` **(default)** | `Button::new("Label")` |
| `ButtonVariant::Filled` | Background-filled (no brackets) | `Button::filled("Label")` |
| `ButtonVariant::Outlined` | Border-only (no background) | `Button::outlined("Label")` |

### `CheckboxVariant`

| Variant | Checked | Unchecked | Indeterminate |
|---------|---------|-----------|---------------|
| `CheckboxVariant::Bracket` **(default)** | `[x]` | `[ ]` | `[-]` |
| `CheckboxVariant::Circle` | `◉` | `○` | `◍` |
| `CheckboxVariant::Box` | `✓` | `☐` | `▣` |
| `CheckboxVariant::Custom { checked, unchecked, indeterminate }` | Custom strings | | |

### `CheckboxState`

| Variant | Description |
|---------|-------------|
| `CheckboxState::Unchecked` | Not checked |
| `CheckboxState::Checked` | Checked |
| `CheckboxState::Indeterminate` | Partial/unknown state |

### `RadioLayout`

| Variant | Description |
|---------|-------------|
| `RadioLayout::Vertical` | Stack options vertically **(default)** |
| `RadioLayout::Horizontal` | Stack options horizontally |

> **Note**: `Radio` uses `CheckboxVariant::Circle` by default (not `Bracket`).

### `ListItemRole`

| Variant | Description |
|---------|-------------|
| `ListItemRole::Normal` | Regular selectable row **(default)** |
| `ListItemRole::Header` | Non-selectable section header |
| `ListItemRole::Spacer` | Non-selectable blank row |

### `ListSymbolPosition`

| Variant | Description |
|---------|-------------|
| `ListSymbolPosition::Left` | Render the symbol in the left symbol column **(default)** |
| `ListSymbolPosition::Right` | Render the symbol immediately after the label content |

### `DescriptionPlacement` *(SearchPalette)*

| Variant | Description |
|---------|-------------|
| `DescriptionPlacement::Inline` | `label - description` on primary line **(default)** |
| `DescriptionPlacement::Right` | Description in right-aligned slot on primary line |
| `DescriptionPlacement::Above` | Description line above label |
| `DescriptionPlacement::Below` | Description line below label |

### `DescriptionOverflow` *(SearchPalette)*

| Variant | Description |
|---------|-------------|
| `DescriptionOverflow::Truncate` | Keep descriptions on one visual line and truncate with ellipsis **(default)** |
| `DescriptionOverflow::Wrap` | Wrap descriptions across multiple lines for `DescriptionPlacement::Above` and `DescriptionPlacement::Below` |

### `SearchMatchMode` *(SearchPalette)*

| Variant | Description |
|---------|-------------|
| `SearchMatchMode::Fuzzy` | Plain `nucleo` fuzzy matching across label, aliases, and description **(default)** |
| `SearchMatchMode::Hybrid` | Exact/prefix/word-prefix/substring/fuzzy tiers evaluated independently per field (label+aliases, description, right-hand hint) and ranked in that priority order; weak scattered fuzzy matches are quality-gated and rejected. See `docs/widgets/overlays.md` (Matching config). |

### `MultiSelectDescriptionPlacement`

| Variant | Description |
|---------|-------------|
| `MultiSelectDescriptionPlacement::Inline` | `label - description` on primary line **(default)** |
| `MultiSelectDescriptionPlacement::Right` | Description in right-aligned slot on primary line |
| `MultiSelectDescriptionPlacement::Above` | Description line above label |
| `MultiSelectDescriptionPlacement::Below` | Description line below label |

### `MultiSelectDescriptionOverflow`

| Variant | Description |
|---------|-------------|
| `MultiSelectDescriptionOverflow::Truncate` | Keep descriptions on one visual line and truncate with ellipsis **(default)** |
| `MultiSelectDescriptionOverflow::Wrap` | Wrap descriptions across multiple lines for `MultiSelectDescriptionPlacement::Above` and `MultiSelectDescriptionPlacement::Below` |

---

## Focus & Input

### `FocusPolicy`

| Variant | Usage |
|---------|-------|
| `FocusPolicy::None` | No focus-aware sizing **(default)** |
| `FocusPolicy::Accordion(FocusAccordion { ... })` | Lazygit-style panel resizing |

### `Grid` track `Length` (`.rows` / `.columns`)

| Value | Effect |
|-------|--------|
| `Length::Auto` | Track sizes from content (subject to spanning rules) **(default)** |
| `Length::Px(n)` | Fixed track size |
| `Length::Percent(n)` | Resolves against the grid's inner width or height |
| `Length::Flex(n)` | Shares remaining space along that axis with other flex tracks |

Implicit rows are added as `Auto` when auto-flow runs out of space.

### `FocusAccordion`

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `focused_min` | `u16` | `7` | Minimum height for focused child |
| `collapsed` | `u16` | `3` | Height for non-focused children |
| `tiny_collapsed` | `u16` | `1` | Height in tight-space mode |
| `expanded_weight` | `u16` | `2` | Flex weight multiplier for focused child |
| `squash_threshold` | `u16` | `28` | Viewport height to enter squashed mode |
| `tiny_threshold` | `u16` | `21` | Viewport height to enter tiny mode |

### `TextAreaNewlineBinding`

| Variant | Enter key behavior |
|---------|-------------------|
| `TextAreaNewlineBinding::Enter` | Enter inserts newline **(default)** |
| `TextAreaNewlineBinding::ShiftEnter` | Shift+Enter inserts newline |
| `TextAreaNewlineBinding::EnterOrShiftEnter` | Both insert newline |

### `TextAreaLineNumberMode`

Controls how built-in `TextArea` line numbers are displayed when
`TextArea::line_numbers(true)` is enabled.

| Variant | Description |
|---------|-------------|
| `TextAreaLineNumberMode::Absolute` | Show one-based logical line numbers **(default)** |
| `TextAreaLineNumberMode::Relative` | Show Vim-style relative numbers: the cursor line stays absolute and other lines show their distance from it |

### `TextAreaVimMode`

Emitted by `TextArea::on_vim_mode_change` when `TextArea::vim_motions(true)` is
enabled.

| Variant | Description |
|---------|-------------|
| `TextAreaVimMode::Insert` | Plain text insertion mode |
| `TextAreaVimMode::Normal` | Vim-style normal/motion mode; enabled TextAreas start here **(default)** |
| `TextAreaVimMode::Visual` | Vim-style visual selection mode; motions extend the cursor/anchor selection |
| `TextAreaVimMode::VisualLine` | Vim-style linewise visual selection mode; motions extend a whole-logical-line selection |

### `TextAreaVimKeymap` / `TextAreaVimKeyBinding`

Widget-local Vim key remaps for `TextArea::vim_motions(true)`. A
`TextAreaVimKeymap` contains `TextAreaVimKeyBinding` entries that translate
single-key `KeyBindings` into canonical Vim command characters before TextArea
Vim dispatch. These remaps are separate from `keymap.conf` and only run while the
TextArea is not in Insert mode.

### `TextAreaVimConfig` / `TextAreaVimCurrentLineHighlight`

`TextAreaVimConfig` groups Vim-only rendering options used by
`TextArea::vim_config(...)`:

| Field | Description |
|-------|-------------|
| `search_bar_style` | `StyleSlot` for the bottom Vim search/status bar |
| `search_bar_prefix_style` | `StyleSlot` overlay for the Vim search bar prefix icons; unset fields fall through to `search_bar_style` |
| `search_bar_count_style` | `StyleSlot` overlay for Vim search count labels like `[2/5]`; unset fields fall through to `search_bar_style` |
| `search_match_style` | `StyleSlot` patched over visible matches while Vim search feedback is shown |
| `current_search_match_style` | `StyleSlot` patched over the current Vim search match; during pending search this is the match `Enter` would jump to, and after `Enter` it follows `n` / `N` |
| `current_line_highlight` | Optional current-line highlight mode |
| `current_line_style` | `StyleSlot` for current-line highlighting |
| `current_line_number_style` | `StyleSlot` overlay for the current line number/custom gutter row; unset fields fall through to `current_line_style` |

| Variant | Description |
|---------|-------------|
| `TextAreaVimCurrentLineHighlight::Off` | Disable current-line highlighting **(default)** |
| `TextAreaVimCurrentLineHighlight::Content` | Highlight only text content rows for the cursor's logical line |
| `TextAreaVimCurrentLineHighlight::Full` | Highlight the full inner row, including line numbers or custom gutter |

### `KeyBinding` / `KeyBindings`

Public shortcut binding types from `tui_lipan::input`. Parsing: **whitespace** = chord steps, **comma** = alternatives. See [`keybindings.md`](keybindings.md).

| Type | Description |
|------|-------------|
| `KeyBinding` | One shortcut or chord (`FromStr`, `Display`, `matches_sequence`, `is_chord`, `step_count`, `canonical_lowercase`) |
| `KeyBindings` | Comma-separated alternatives (`FromStr`, `Display`, `canonical_lowercase`, `iter`, `primary`, `is_empty`, `len`) |
| `ChordMatcher<T>` | Stateful incremental matcher for chords (`feed`, `reset`, `is_pending`) |
| `ChordResult<T>` | `None` / `Pending` / `Matched` from `ChordMatcher::feed` |
| `KeyBindingParseError` | Parse error type for invalid binding strings |

Lowercase helpers are also available:

- `KeyBinding::canonical_lowercase()`
- `KeyBindings::canonical_lowercase()`
- `format_binding_lowercase(...)`
- `format_bindings_lowercase(...)`

### `SentinelId`

Opaque `Copy` id for a custom inline sentinel. `SentinelId::UNKNOWN` is `0` when no id was set on a removed token. New ids are assigned by `insert_sentinel` (via internal `SentinelId::next()`).

### `SentinelEvent`

| Variant | Payload | When |
|---------|---------|------|
| `SentinelEvent::Deleted { id, sentinel }` | Stable id (or `UNKNOWN`), full `TextAreaSentinel` including payload | User edit removed the sentinel char from the buffer |

Emitted by: `TextArea::on_sentinel_event` (batched).

### `TextAreaSentinel`

Builder-style struct (not an enum): `new(label)`, `style`, `focus_style`, `hover_style`, `payload<T>(data)`, `id(SentinelId)`, `get_payload`, `sentinel_id`. Equality compares label, styles, and id (payload is ignored).

### `TextAreaSentinelClickKind`

| Variant | Payload | When |
|---------|---------|------|
| `TextAreaSentinelClickKind::Image { index, image }` | Inline image index and `ImageContent` | User clicked an inline image placeholder |
| `TextAreaSentinelClickKind::Custom { index, id, sentinel }` | Custom sentinel index, stable id, and metadata including payload | User clicked a custom sentinel label |

### `TextAreaSnapshot`

| Field | Description |
|-------|-------------|
| `value`, `cursor`, `anchor` | Buffer and caret state |
| `sentinels` | Parallel custom sentinel metadata |
| `images`, `image_mode` | Same fields as on `TextArea` |

Methods: `TextAreaSnapshot::capture(&TextArea)`, `apply(self, TextArea) -> TextArea`, `diff(&self, &Self) -> Vec<SentinelEvent>` (stable ids removed between snapshots).

### `TextAreaImageMode` *(requires feature `image`)*

| Variant | Description |
|---------|-------------|
| `TextAreaImageMode::Inline` | Unicode PUA sentinels embedded in text value |
| `TextAreaImageMode::Attachment` | Images in separate list; text value unchanged |

---

## Overlay & Toast

### `DismissPolicy`

| Variant | Dismissed by |
|---------|-------------|
| `DismissPolicy::None` | Nothing (manual only) |
| `DismissPolicy::ClickOutside` | Click outside overlay **(default)** |
| `DismissPolicy::ClickInside` | Click inside overlay |
| `DismissPolicy::ClickOutsideOrEscape` | Click outside or Escape |

### `ToastPlacement`

| Variant | Position |
|---------|----------|
| `ToastPlacement::TopStart` | Top-left |
| `ToastPlacement::TopCenter` | Top-center |
| `ToastPlacement::TopEnd` | Top-right |
| `ToastPlacement::BottomStart` | Bottom-left |
| `ToastPlacement::BottomCenter` | Bottom-center |
| `ToastPlacement::BottomEnd` | Bottom-right **(default)** |

### `ToastCopyAffordance`

| Variant | Behavior |
|---------|----------|
| `ToastCopyAffordance::None` | No visual copy control; copyable toasts still copy on right-click |
| `ToastCopyAffordance::BorderGlyph` | Show a copy glyph in the top border when the toast has a border **(default)** |

---

## App Configuration

### `ContrastPolicy`

Used by `App::contrast_policy(...)`, widget-level `.contrast_policy(...)` builders, and `Style::contrast_policy(...)` for per-style overrides.

| Variant | Behavior |
|---------|----------|
| `ContrastPolicy::Wcag` | Auto-adjust low-contrast text using WCAG 2.1 contrast **(default)** |
| `ContrastPolicy::BlackOrWhite` | Keep the current foreground if it already passes WCAG; otherwise snap to black or white |
| `ContrastPolicy::Apca` | Auto-adjust using APCA perceptual contrast |
| `ContrastPolicy::Off` | Preserve explicit colors exactly |

### `TaskPolicy`

| Variant | Behavior |
|---------|----------|
| `TaskPolicy::QueueAll` | Enqueue every task; native workers may run same-key tasks concurrently |
| `TaskPolicy::DropIfRunning` | Ignore new task while one with same key is running without cancelling the active task |
| `TaskPolicy::LatestOnly` | Keep only newest pending task, cancel the active token, and cancel replaced pending tokens |

`LatestOnly` cancellation is cooperative. Background work must poll its
`CommandLink` / `CancellationToken` and use `send_if_not_cancelled` to avoid
delivering stale messages.

---

## Data Widgets

### `IndentStyle`

Used by `Tree::indent_style(...)` and inherited by `FileTree` for hierarchy
guide glyphs.

| Variant | Glyphs |
|---------|--------|
| `IndentStyle::None` | No guides |
| `IndentStyle::Line` | `│` |
| `IndentStyle::Short` | `├`, `└` |
| `IndentStyle::Long` | `├─`, `└─` |
| `IndentStyle::ShortRounded` | `├`, `╰` |
| `IndentStyle::LongRounded` | `├─`, `╰─` |

### `FileTreeChangeView`

| Variant | Description |
|---------|-------------|
| `FileTreeChangeView::AllFiles` | Browse all files under the configured root **(default)** |
| `FileTreeChangeView::ChangedOnly` | Show only changed paths and ancestor directories from the configured change source |

`FileTreeGitView` is a compatibility alias for `FileTreeChangeView`.

### `FileTreeChangeSource`

| Variant | Description |
|---------|-------------|
| `FileTreeChangeSource::Git` | Read change data from the local git repository **(default)** |
| `FileTreeChangeSource::Provided(Vec<FileTreeChange>)` | Use application/backend-provided change rows; does not require local git and may include virtual, nonexistent, or deleted paths |

### `FileTreeChangeStatus`

| Variant | Description |
|---------|-------------|
| `FileTreeChangeStatus::Modified` | Existing path has modifications |
| `FileTreeChangeStatus::Added` | Path is newly added |
| `FileTreeChangeStatus::Deleted` | Path was deleted and may not exist on disk |
| `FileTreeChangeStatus::Renamed` | Path was renamed |
| `FileTreeChangeStatus::Untracked` | Path is untracked by the source |
| `FileTreeChangeStatus::Conflicted` | Path has a conflict |

### `FileTreeChange`

`FileTreeChange::new(path, status)` creates a provided change row. Builder
methods include `.kind(FileKind)`, `.diff_stat(additions, deletions)`,
`.additions(...)`, `.deletions(...)`, and `.staged(...)`.

### `FileTreeItemStyle`

Path-specific FileTree decoration style used by `FileTree::path_style(...)` and
`FileTree::path_styles(...)`. `FileTreeItemStyle::new()` starts empty; builder
methods `.row(...)`, `.icon(...)`, `.label(...)`, and `.suffix(...)` set optional
styles for the whole row, leading icon, name label, and right-side metadata
suffix independently.

### `FileTreeSuffixPriority`

Controls what wins when a FileTree row is too narrow for both the label and
right-aligned change metadata.

| Variant | Description |
|---------|-------------|
| `FileTreeSuffixPriority::Label` | Preserve the label and truncate suffix metadata first **(default)** |
| `FileTreeSuffixPriority::Suffix` | Preserve suffix metadata such as `M +30 -21` and truncate the label first |

---

## Diff View *(feature `diff-view`)*

### `DiffViewMode`

| Variant | Description |
|---------|-------------|
| `DiffViewMode::Split` | Side-by-side view **(default)** |
| `DiffViewMode::Unified` | Unified view |

### `DiffViewBackend`

| Variant | Description |
|---------|-------------|
| `DiffViewBackend::TextArea` | TextArea-backed rendering (default, editable supported) |
| `DiffViewBackend::DocumentView` | DocumentView-backed rendering (read-only optimized) |

### `DiffPane`

| Variant | Description |
|---------|-------------|
| `DiffPane::Left` | Left pane in split mode |
| `DiffPane::Right` | Right pane in split mode |
| `DiffPane::Unified` | Unified pane |

### `DiffContextSeparatorDirection`

| Variant | Description |
|---------|-------------|
| `DiffContextSeparatorDirection::Above` | Hidden context appears above the visible hunk |
| `DiffContextSeparatorDirection::Below` | Hidden context appears below the visible hunk |
| `DiffContextSeparatorDirection::Between` | Hidden context appears between two visible hunks |

### `DiffContextRange`

Stable identifier for a collapsed unchanged range. Line numbers are git-style,
1-based, and inclusive when present.

```rust
pub struct DiffContextRange {
    pub old_start: Option<usize>,
    pub old_end: Option<usize>,
    pub new_start: Option<usize>,
    pub new_end: Option<usize>,
}
```

### `DiffHunkAnchor`

Logical navigation anchor for one parsed unified-patch hunk. `logical_line` is a
zero-based rendered source row before soft wrapping; `DiffView::scroll_to_hunk`
uses this row and lets the active backend resolve the final visual row.

```rust
pub struct DiffHunkAnchor {
    pub pane: DiffPane,
    pub index: usize,
    pub old_start: Option<usize>,
    pub new_start: Option<usize>,
    pub logical_line: usize,
}
```

---

## Utility Types

### `GridPos`

A position in a 2D grid, used for mouse-driven selection in grid-like UIs.

```rust
pub struct GridPos {
    pub row: usize,  // Zero-based row index
    pub col: usize,  // Zero-based column index
}
```

### `GridSelection`

A 2D range selection with anchor (start) and cursor (current) positions.

| Method | Returns | Description |
|--------|---------|-------------|
| `new(pos)` | `GridSelection` | Create a new single-point selection |
| `extend_to(pos)` | - | Extend selection to a new cursor position |
| `normalized()` | `(GridPos, GridPos)` | Get ordered `(start, end)` where start <= end |
| `is_empty()` | `bool` | Check if anchor equals cursor |
| `contains(row, col)` | `bool` | Check if a cell is within the selection |
| `extract_text(lines)` | `String` | Extract selected text from a slice of line strings |
| `columns_for_row(row, line_width)` | `Option<(usize, usize)>` | Get selected column range for a row (for rendering) |

### `GridSelectionEvent`

```rust
pub struct GridSelectionEvent {
    pub selection: Option<GridSelection>,
    pub text: Option<String>,
}
```

---

## Element Helpers

| Expression | Description |
|-----------|-------------|
| `Element::empty()` | Empty placeholder (use in `if/else` branches) |
| `widget.into()` | Convert any widget into `Element` |
| `widget.key("my-key")` | Assign stable identity for reconciliation/focus |
## TextArea editor primitive enums

- `TextAreaDecorationKind`: `Range`, `WholeLine`, `Underline`.
  Byte offsets remain canonical. `Underline` applies the supplied style and
  enables underline automatically.
- `VirtualTextPlacement`: `Inline`, `Eol`. Inline virtual text shifts visual
  columns before the anchor byte; EOL virtual text appends after a logical line's
  final visual row without affecting wrapping.
- `TextAreaStateChangeReason`: `Edit`, `SelectionChange`, `CursorMove`,
  `Scroll`, `VimModeChange`.
