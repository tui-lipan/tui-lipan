# Styling

## Style Struct

`Style` defines visual appearance. All fields are optional. Rendered color channels (`fg`, `bg`, and `underline_color`) store `Paint`, so they accept both opaque `Color` values and alpha-aware paints. Background paint (`bg`) inherits from parent containers when unset; other fields use widget or theme defaults.

```rust
Style::new()
    .fg(Color::Blue)
    .bg(Color::indexed(235))
    .bold()
    .italic()
    .underline()
    .dim()
    .reverse()
```

| Method | Effect |
|--------|--------|
| `.fg(impl Into<Paint>)` | Foreground paint (`Color` works directly) |
| `.bg(impl Into<Paint>)` | Background paint (`Color` works directly) |
| `.fg_alpha(Color, f32)` | Foreground paint with normalized alpha |
| `.bg_alpha(Color, f32)` | Background paint with normalized alpha |
| `.bold()` | Bold text |
| `.not_bold()` | Explicitly disable bold (suppresses renderer fallbacks) |
| `.dim()` | Dimmed/faint text |
| `.dim_by(f32)` | Dim resolved `fg`/`bg` and cell backdrop |
| `.tint_by(Color, f32)` | Tint existing rendered cells toward a color |
| `.lighten_by(f32)` | Lighten resolved `fg`/`bg` colors |
| `.transform_fg(ColorTransform)` | Transform the resolved foreground color |
| `.transform_bg(ColorTransform)` | Transform the resolved background color |
| `.contrast_policy(ContrastPolicy)` | Override contrast adjustment for this style |
| `.italic()` | Italic text |
| `.underline()` | Underline |
| `.reverse()` | Swap fg/bg |

`Style` also exposes a `tint` field (`Option<(Color, f32)>`) for advanced/manual construction, but normal usage should prefer `.tint_by(color, alpha)`.

Relative transforms are resolved after style patching, so they work well with theme-provided or inherited style values:

```rust
let disabled = Style::new().transform_fg(ColorTransform::Dim(0.5));
let warning_surface = Style::new().transform_bg(ColorTransform::Tint(Color::Yellow, 0.25));
let washed_out = Style::new().transform_fg(ColorTransform::Opacity(0.6));
let forced_readable = Style::new().contrast_policy(ContrastPolicy::Apca);
```

> Note: `Style` has `.lighten_by(...)` but no `.lighten()`, and `.tint_by(...)` but no `.tint()` convenience method.

## Style Inheritance

**Only background color (`bg`) automatically inherits** from parent containers. Foreground color (`fg`) and text modifiers (bold, italic, etc.) do **not** inherit - each widget resolves its own `fg` independently.

### How Background Inheritance Works

When a container (VStack, HStack, Frame, etc.) has `bg` set, the framework fills its entire rectangular area with that background color before rendering children. Children that don't set their own `bg` naturally show the parent's background through the terminal buffer.

```rust
// ✅ GOOD: Set bg once on the parent container - children see it automatically
VStack::new()
    .style(Style::new().bg(Color::indexed(235)))
    .child(Text::new("Shows bg from VStack"))
    .child(Text::new("Also shows bg from VStack"))
    .child(Button::new("Also shows bg from VStack"))

// ❌ BAD: Setting bg on every single widget - all of these are redundant
VStack::new()
    .style(Style::new().bg(Color::indexed(235)))
    .child(Text::new("A").style(Style::new().bg(Color::indexed(235))))     // redundant!
    .child(Text::new("B").style(Style::new().bg(Color::indexed(235))))     // redundant!
    .child(Button::new("C").style(Style::new().bg(Color::indexed(235))))   // redundant!
```

> **Note**: `fg` does NOT work this way. Each widget must set its own `fg` if you want a specific text color. Setting `.fg(Color::White)` on a parent VStack does **not** make children's text white.

### Sub-Style Inheritance

Don't repeat the parent's bg on every sub-style variant either - only set bg when you want a **different** background for that state:

```rust
// ❌ BAD: Repeating bg on every style variant
Input::new(query.clone())
    .style(Style::new().fg(Color::White).bg(Color::indexed(235)))
    .focus_style(Style::new().fg(Color::White).bg(Color::indexed(235)).bold())

// ✅ GOOD: bg is inherited from the parent container; only set fg and modifiers
Input::new(query.clone())
    .style(Style::new().fg(Color::White))
    .focus_style(Style::new().fg(Color::White).bold())
```

### Style Precedence

| Priority | Source |
|----------|--------|
| Highest | Explicit widget style (set directly on the widget) |
| | ThemeProvider (applied to the subtree) |
| | Parent container bg (painted in the terminal buffer) |
| Lowest | App-level theme default |

`ThemeProvider` is a scoped provider, not a tree-rewrite pass. Widgets keep
their explicit `Style` / `StyleSlot` values and combine them with the active
theme while rendering. That means changing or nesting themes updates the next
frame without permanently baking the old theme into widget fields.

Base widget `.style(...)` fields remain partial overlays: foregrounds and text
modifiers fall through to the active theme, while theme backgrounds are not
inherited automatically. State slots use the explicit Replace/Extend/Inherit
model below.

### State Style Slots

State-overlay setters (`selection_style`, `hover_style`, `focus_style`,
`active_style`, and prefixed variants such as `list_selection_style`) use
`StyleSlot` semantics. This makes the slot's relationship to the active theme
role explicit:

| Mode | Builder pattern | Behavior |
|------|-----------------|----------|
| Replace | `selection_style(style)` | Use `style` as the complete state overlay and ignore the theme role. This is the default for state-style setters. |
| Extend | `extend_selection_style(style)` | Start from the scoped theme role, then patch `style` over it. Use this when you want to keep theme backgrounds/fg/modifiers and customize a few fields. |
| Inherit | `inherit_selection_style()` | Use the scoped theme role directly. Use this to undo an explicit override or delegate the slot entirely to the theme. |

The same naming applies to other state slots: `extend_hover_style`,
`inherit_focus_style`, `extend_active_style`, and prefixed forwarding setters
such as `extend_list_selection_style` / `inherit_list_selection_style` where a
composite exposes an inner widget's slot.

Selection slots resolve against the role that matches the widget semantics:
row/current-item selections use `theme.selection`, while text/range selections
in `Input`, `TextArea`, `DocumentView`, `Terminal`, and `HexArea` use
`theme.text_selection`.

Active row/tab slots inherit the selection theme role by default. This keeps
active-row and active-tab overlays visually aligned with selected items unless
you replace or extend the active slot.

When multiple state overlays are active at once, concrete fields keep the
durable-state precedence used by the renderer: hover is lower priority than
focus/selection/active, so a focused or selected background still beats a hover
background. Hover color transforms and compositor effects (`transform_fg`,
`transform_bg`, `dim_by`, `tint_by`) are treated as transient effects and apply
after durable concrete colors. This lets patterns such as “lighten on hover”
remain visible on focused or selected rows without making concrete hover colors
override focused or selected colors. Text modifier flags (`bold`, `italic`,
`underline`, `reverse`, `strikethrough`) follow durable-state precedence.
Widgets with specialized interaction semantics may still suppress hover for a
more specific state; for example, draggable tab bars keep active-tab hover
suppression during drag/reorder interactions.

```rust
// Replace: exact selected-row overlay, independent of theme.selection.
List::new().selection_style(Style::new().fg(Color::Black).bg(Color::Cyan))

// Extend: keep theme.selection and add bold text.
List::new().extend_selection_style(Style::new().bold())

// Inherit: selected rows follow the scoped ThemeProvider/App theme.
List::new().inherit_selection_style()
```

## Colors and Paint

`Color` is an opaque terminal color. It models ANSI/named colors, indexed
colors, RGB colors, and semantic sentinels. `Color::hex` accepts only opaque
`RGB` and `RRGGBB` hex forms; alpha hex belongs to `Paint`.

```rust
Color::Red           // Named ANSI color
Color::indexed(235)  // 256-color palette (u8)
Color::rgb(30, 40, 50)  // True color
Color::hex("#1E2832")   // Opaque hex string; invalid input falls back to Color::Reset
Color::Backdrop         // Clear fg but preserve the background already underneath
Color::Transparent      // Skip painting fg/bg - show whatever is already in the buffer / parent
```

`Paint` is the style-channel value. Use it when a foreground/background needs an
alpha channel:

```rust
Paint::solid(Color::Blue)
Paint::rgb(30, 40, 50)
Paint::rgba(30, 40, 50, 192)
Paint::hex("#1E2832CC")

Style::new().bg(Paint::hex("#101015CC"))
Style::new().fg_alpha(Color::White, 0.75)
Style::new().bg_alpha(Color::rgb(16, 16, 21), 0.8)
```

Alpha paint is source-over composited before terminal output. Background paint
blends over the existing cell background (or `App::terminal_bg()` when the cell
uses terminal reset and a terminal background is configured). Foreground paint
blends over the resolved cell background; when no RGB backdrop is available the
renderer falls back to the source pigment so text remains visible. `alpha = 0`
preserves the backdrop for that channel but is not the same as
`Color::Transparent`: widgets may still draw symbols or clear cells according to
their normal rendering behavior.

`Color::Transparent` is not a pigment: it tells the renderer **not** to set that style channel on ratatui cells, so lower layers stay visible. It differs from `Color::Reset`, which selects the terminal’s default palette for that attribute. In `Style::patch`, a transparent overlay leaves the resolved base color for that channel unchanged.

`Color::Backdrop` is intended for surface/background fills. It preserves the background color already in the buffer while still allowing the surface to clear text/foreground content above it. This matches the old modal behavior where the dialog body blanked underlying text without painting a new solid background.

Named colors: `Black`, `Red`, `Green`, `Yellow`, `Blue`, `Magenta`, `Cyan`, `White`, `Gray`, `DarkGray`, `LightRed`, `LightGreen`, `LightYellow`, `LightBlue`, `LightMagenta`, `LightCyan`.

> **Tip**: Prefer `Color::rgb(...)` for interactive/selection styles when exact contrast matters. Named ANSI colors vary by terminal palette.

### Palette (Tailwind-style colors)

`tui_lipan::style::palette` provides a comprehensive color palette based on Tailwind CSS. Use it for consistent, designer-friendly colors across your app.

**Top-level 500-series constants** (quick access):

`SLATE`, `GRAY`, `ZINC`, `NEUTRAL`, `STONE`, `RED`, `ORANGE`, `AMBER`, `YELLOW`, `LIME`, `GREEN`, `EMERALD`, `TEAL`, `CYAN`, `SKY`, `BLUE`, `INDIGO`, `VIOLET`, `PURPLE`, `FUCHSIA`, `PINK`, `ROSE`

**Color family modules** with shades `B50`–`B950` (light to dark):

`slate`, `gray`, `zinc`, `neutral`, `stone`, `red`, `orange`, `amber`, `yellow`, `lime`, `green`, `emerald`, `teal`, `cyan`, `sky`, `blue`, `indigo`, `violet`, `purple`, `fuchsia`, `pink`, `rose`

```rust
use tui_lipan::style::{palette, Style};

// Top-level 500-series
Style::new().fg(palette::BLUE)

// Shades (e.g. red::B500, slate::B200)
Style::new().fg(palette::red::B500).bg(palette::slate::B900)
```

### Color Transform Helpers

Use these for direct color manipulation:

| Method | Effect |
|--------|--------|
| `Color::dim()` | Dim by default amount (`0.35`) |
| `Color::dim_by(f32)` | Dim by explicit amount `0.0..=1.0` |
| `Color::lighten()` | Lighten by default amount (`0.35`) |
| `Color::lighten_by(f32)` | Lighten by explicit amount `0.0..=1.0` |
| `Color::blend_toward(Color, f32)` | Blend toward target color by alpha |

```rust
let dialog_backdrop = Style::new().tint_by(Color::rgb(10, 20, 60), 0.55);
let boosted_text = Style::new().fg(Color::Blue.lighten());
let softer_text = Style::new().fg(Color::Blue.lighten_by(0.20));
let inherited_dim = Style::new().transform_fg(ColorTransform::Dim(0.5));
let inherited_opacity = Style::new().transform_fg(ColorTransform::Opacity(0.6));
```

> **Note**: `ColorTransform::Opacity` composes with paint alpha. For opaque colors it behaves like an alpha paint over the resolved cell background. To make opacity work predictably through terminal-default/reset backgrounds, supply the terminal's default background color with `App::terminal_bg(...)` for static apps, or enable `App::system_theme()` / `App::live_host_terminal_colors(true)` so the runner updates the resolved background after startup, focus-gained, or manual host color refreshes.

## Visual Effects

`VisualEffect` is a value-based post-processing model for `EffectScope`. Unlike `Style`, these effects do not describe widget-local text styling; they mutate the already-rendered cells inside an `EffectScope` rect.

```rust
EffectScope::new()
    .effect(VisualEffect::PaletteQuantize {
        palette: EffectPalette::Gameboy,
    })
    .effect(VisualEffect::Scanlines {
        strength: 0.18,
        spacing: 2,
    })
    .child(content)
```

Common variants:

| Type | Purpose |
|------|---------|
| `VisualEffect::ColorTransform` | Apply relative color transforms (Dim, Lighten, Opacity, Tint) to fg/bg of each cell. Constructors: `dim`, `lighten`, `tint`, `transform_fg`, `transform_bg` |
| `VisualEffect::ContrastPolicy` | Apply `ContrastPolicy` to ensure text legibility |
| `VisualEffect::Monochrome` | Desaturation / grayscale conversion |
| `VisualEffect::PaletteQuantize` | Reduce colors to a preset or custom palette |
| `VisualEffect::Scanlines` | Static row-based dimming mask |
| `VisualEffect::RainbowWave` | Animated color cycling by position and frame phase, blended back into the subtree |
| `VisualEffect::Ripple` | Aspect-correct radial tint ring from an `EffectOrigin`; `RippleRadius::Fixed` is static, while `Loop` / `Once` animate from the renderer phase |
| `VisualEffect::Gradient` | Sine-eased mirrored `ColorGradient` wash sampled in scope-local coordinates; optional animation via `speed` / frame `phase` |
| `VisualEffect::RetroCrt` | Retro preset built from palette, scanline, and flicker primitives |
| `VisualEffect::Clipped` | Bounds and/or `CellMask` to restrict another effect - see [widgets/effects.md](widgets/effects.md) |

Supporting enums:

| Enum | Variants |
|------|----------|
| `EffectAxis` | `Horizontal`, `Vertical`, `Diagonal` |
| `EffectPalette` | `Cga`, `Gameboy`, `Amber`, `Green`, `Custom(Vec<Color>)` |
| `RetroPreset` | `Amber`, `Green`, `Cga`, `Gameboy`, `VaultTec` |
| `RippleRadius` | `Fixed(f32)`, `Loop { max_radius, period_ticks }`, `Once { max_radius, duration_ticks, start_tick }` |

Use `Style` when you need inherited colors, focus/hover patches, or per-widget presentation. Use `VisualEffect` when you want to transform the final composed output of an entire subtree.

For widgets like `MouseRegion`, these form two distinct layers:
- `hover_style(...)` paints the hovered region before child content is rendered. It is best for hover backgrounds and modifiers; child text commonly paints its own foreground afterward, so `hover_style(Style::new().fg(...))` may not recolor that text.
- `hover_effect(...)` applies a visual post-processing transformation to the rendered child content. Use it when you need to change colors that children already painted, such as text foreground.
- `hover_tint(color, alpha)` is a symmetric tint shorthand: it blends both foreground and background toward `color`. At `alpha = 1.0`, both channels become `color`; use `hover_effect(VisualEffect::transform_fg(ColorTransform::Tint(color, 1.0)))` when you only want to recolor text.

## Layout Primitives

### Length

| Value | Meaning |
|-------|---------|
| `Length::Auto` | Size to content |
| `Length::Px(u16)` | Fixed cell count |
| `Length::Percent(u16)` | Percentage of available space (clamped to `0..=100`) |
| `Length::Flex(u16)` | Proportional share of remaining space |

Containers (`VStack`, `HStack`) default to `Flex(1)` for both axes.

### Layout Constraints

`LayoutConstraints` and `Element::{min_width,min_height,max_width,max_height}` use `Length`:

- `Px(n)` is absolute.
- `Percent(p)` resolves against the parent-allocated size.
- `Auto` / `Flex(_)` mean no minimum (min) or no cap (max).
- Percent constraints are ignored when the parent size is unknown during measurement.
- Hard `min_*`/`max_*` constraints are separate from intrinsic min/max-content sizing; widgets that wrap can mark `LayoutConstraints::reflows(true)` / `Element::reflows(true)` and use `ShrinkPriority::First` to yield before normal siblings.

### Padding

```rust
// Uniform (all sides)
.padding(1)                 // 1 cell on all sides
// or: Padding::from(1u16)

// Vertical + Horizontal
.padding((2, 1))            // top/bottom=2, left/right=1
// or: Padding::from((2u16, 1u16))

// Full control (top, right, bottom, left)
.padding((1, 2, 1, 2))
// or: Padding::from((1u16, 2u16, 1u16, 2u16))
```

`Padding` methods: `.horizontal()` → left+right sum, `.vertical()` → top+bottom sum.

### Align

Cross-axis alignment for stacks and containers:

| Value | Effect |
|-------|--------|
| `Align::Start` | Top/left **(default)** |
| `Align::Center` | Centered |
| `Align::End` | Bottom/right |
| `Align::Stretch` | Fill available space |

### Justify

Main-axis alignment for stacks:

| Value | Effect |
|-------|--------|
| `Justify::Start` | Pack children toward start **(default)** |
| `Justify::Center` | Center in available space |
| `Justify::End` | Pack toward end |
| `Justify::SpaceBetween` | Even space between children (none at edges) |
| `Justify::SpaceAround` | Even space around each child |
| `Justify::SpaceEvenly` | Equal space between and around children |

### BorderStyle

| Value | Appearance |
|-------|-----------|
| `BorderStyle::Plain` | `─ │ ┌ ┐ └ ┘` |
| `BorderStyle::Rounded` | `─ │ ╭ ╮ ╰ ╯` |
| `BorderStyle::Double` | `═ ║ ╔ ╗ ╚ ╝` |
| `BorderStyle::Thick` | `━ ┃ ┏ ┓ ┗ ┛` |
| `BorderStyle::LightDoubleDashed` | Dashed light border |
| `BorderStyle::HeavyDoubleDashed` | Dashed heavy border |
| `BorderStyle::LightTripleDashed` | Triple-dashed light |
| `BorderStyle::HeavyTripleDashed` | Triple-dashed heavy |
| `BorderStyle::LightQuadrupleDashed` | Quadruple-dashed light |
| `BorderStyle::HeavyQuadrupleDashed` | Quadruple-dashed heavy |

### BorderEdges

`BorderEdges` controls frame border geometry separately from `BorderStyle` glyphs.

| Value | Effect |
|-------|--------|
| `BorderEdges::All` | Full box border (default) |
| `BorderEdges::HorizontalCaps` | Top/bottom rows with corner caps; no left/right content inset |

### BorderMergeMode

`BorderMergeMode` controls how adjacent or overlapping frame border symbols are merged at their seams.

| Value | Effect |
|-------|--------|
| `BorderMergeMode::Replace` | Last write wins; no symbol merging (clean overlap override) |
| `BorderMergeMode::Exact` | Merge only when an exact box-drawing intersection symbol exists (default) |
| `BorderMergeMode::Fuzzy` | Merge using the closest matching symbol when an exact merge symbol is unavailable |

## Theme System

### App-Wide Theme

```rust
App::new()
    .theme(Theme::one_dark())
    .mount(Root)
    .run()
```

If omitted, `Theme::default()` applies automatically.

### ThemeProvider Widget

Scopes a theme to a subtree:

```rust
ThemeProvider::new(Theme::dracula())
    .child(my_sidebar_element)
```

Style precedence: **explicit widget style/slot > ThemeProvider theme > widget defaults**.

> Note: state slots are resolved at render/reconcile time by widgets that read
> theme roles, so partial state overrides retain their slot-level intent. Base
> style defaults and formatter palettes are still populated during expansion, and
> base-style background is not auto-injected into every descendant.

### Named Presets

```rust
Theme::default()
Theme::one_dark()
Theme::dracula()
Theme::nord()
Theme::gruvbox()
Theme::catppuccin()
Theme::tokyo_night()
Theme::solarized_dark()
Theme::monokai()
```

### System Theme

`App::system_theme()` opts the whole app into a theme derived from the host
terminal palette. For app-owned variants, build one explicitly with
`Theme::from_host_colors(HostTerminalColors)` after reading live host colors.

`preset_by_name("system")` is intentionally unsupported: preset lookup stays
pure and non-blocking, while host color probing remains an opt-in app/runner
operation.

### Builder API for Custom Themes

```rust
// Fast path: define a theme from foreground, background, and accent
let my_theme = Theme::custom(
    Color::rgb(0xE0, 0xE0, 0xE0),
    Color::rgb(0x10, 0x10, 0x15),
    Color::rgb(0xFF, 0x80, 0x00),
);

// Start from a preset, override only what you need
let my_theme = Theme::one_dark()
    .focus_decoration(false)
    .primary(Style::new().fg(Color::rgb(0xE0, 0xE0, 0xE0)).bg(Color::rgb(0x10, 0x10, 0x15)))
    .accent(Style::new().fg(Color::rgb(0xFF, 0x80, 0x00)))
    .selection(Style::new().bg(Color::rgb(0x24, 0x1A, 0x0C)))
    .text_selection(Style::new().fg(Color::White).bg(Color::rgb(0x3A, 0x2A, 0x12)))
    .hover(Style::new().bg(Color::rgb(0x18, 0x18, 0x22)));

// Minimal palette path: override item and text selection colors separately
let palette_theme = ThemePalette::new(
    Color::rgb(0xE0, 0xE0, 0xE0),
    Color::rgb(0x10, 0x10, 0x15),
    Color::rgb(0xFF, 0x80, 0x00),
)
.selection(Color::rgb(0xFF, 0x80, 0x00))
.text_selection(Color::rgb(0x66, 0x99, 0xFF))
.into_theme();

// Opt in to focused text recoloring on specific text surfaces
let my_theme = Theme::one_dark()
    .input(InputPalette {
        focus: Style::new().fg(Color::rgb(0xFF, 0xC0, 0x66)).bold(),
    })
    .text_area(TextAreaPalette {
        focus: Style::new().fg(Color::rgb(0xC3, 0xE8, 0x8D)),
    })
    .document_view(DocumentViewPalette {
        focus: Style::new().fg(Color::rgb(0x8B, 0xD5, 0xFF)),
    });

// Full control over all sub-palettes
let full_custom = Theme::default()
    .primary(Style::new().fg(Color::White).bg(Color::Black))
    .accent(Style::new().fg(Color::Cyan))
    .selection(Style::new().fg(Color::Black).bg(Color::Cyan))
    .text_selection(Style::new().fg(Color::White).bg(Color::Blue))
    .hover(Style::new().bg(Color::indexed(236)))
    .scrollbar(ScrollbarPalette {
        track: None,
        thumb: Color::DarkGray,
        thumb_focus: Some(Color::White),
    })
    .splitter(SplitterPalette { hover: Color::Blue, active: Color::Cyan })
    .file_icons(FileIconPalette { /* ... */ })
    .git_status(GitStatusPalette { /* ... */ });
```

### Theme Hot Reload (feature: `theme-reload`)

Watch TOML theme files on disk and reload them at runtime so theme authors
and app users can tweak colors without restarting the app.

Enable the feature and run the example:

```bash
cargo run --example theme_hot_reload --features theme-reload
```

Example TOML theme file with `extends` plus style/color overrides:

```toml
extends = "one_dark"
focus_decoration = false

[primary]
fg = "#E0E0E0"
bg = "#101015"

[accent]
fg = "#FF8000"
```

Style fields (`fg`, `bg`, and `underline_color` on style tables such as
`[primary]`, `[selection]`, `[text_selection]`, `[document.heading_style]`, etc.) are
paint-capable. They accept opaque color formats plus alpha hex and `rgba(...)`:

```toml
[primary]
bg = "#101015CC"

[selection]
fg = "rgba(250, 240, 230, 0.5)" # float alpha 0.0..=1.0
bg = "rgba(30, 40, 50, 192)"    # integer alpha 0..=255

[text_selection]
fg = "#FFFFFF"
bg = "#334155CC"
```

Bare palette fields such as `[status]`, `[git_status]`, `[scrollbar]`,
`[splitter]`, and `[surface]` are still color-only and reject alpha. They keep
using opaque hex, ANSI names, `indexed(n)`, or `rgb(r,g,b)` until those render
paths are intentionally migrated to paint.

Watcher wiring in the example is intentionally simple:

- `ThemeWatcher` monitors the theme file for on-disk updates.
- `load_theme_from_toml` rebuilds a `Theme` from the current TOML file.
- A periodic app message drives polling; when a change is detected, the app reloads and applies the new theme.

Note: watcher path matching includes a filename fallback for editor save-via-rename flows. If multiple watchers
target sibling files with the same basename, events may cross-trigger.

Limitation: `Theme::extensions` (typed extension data from `with_extension`) is not TOML-reloadable and remains programmatic.

### Typed Theme Extensions

When your app has semantic theme tokens that do not fit the framework's core palettes, store them inside `Theme` rather than a parallel global cache.

```rust
use tui_lipan::prelude::*;

#[derive(Clone, Debug, PartialEq)]
struct BrandTheme {
    shell_badge: Style,
}

let theme = Theme::one_dark().with_extension(BrandTheme {
    shell_badge: Style::new().fg(Color::rgb(0x7D, 0xCF, 0xFF)),
});
```

Read them from components with `ctx.theme_extension::<T>()`:

```rust
let brand = ctx.theme_extension::<BrandTheme>().expect("brand theme installed");
Text::new("shell").style(brand.shell_badge)
```

This keeps app-specific tokens inside the same `ThemeProvider` tree as the framework palettes, so theme switching and invalidation remain centralized.

### Theme Fields

| Field | Type | Purpose |
|-------|------|---------|
| `primary` | `Style` | Base text and background |
| `accent` | `Style` | Interactive emphasis for hover/cursors/controls |
| `selection` | `Style` | Selected/current state |
| `focus` | `Style` | Focused widget chrome and focus affordances |
| `focus_decoration` | `bool` | Enable theme-sourced focus roles, focused-content palettes, automatic frame focus chrome, and focused scrollbar thumbs (default: `true`) |
| `hover` | `Style` | Optional row/surface hover state |
| `border` | `Style` | Frame and divider color |
| `muted` | `Style` | Placeholders, disabled text, indicators |
| `diff` | `DiffPalette` | DiffView line/word/marker/separator/patch-header styles |
| `document` | `DocumentPalette` | DocumentView/markdown heading/link/code/table/diagram styles |
| `syntax` | `SyntaxPalette` | Theme-aware syntect token recoloring |
| `input` | `InputPalette` | Explicit focused-content styling for `Input` and input-backed composites |
| `text_area` | `TextAreaPalette` | Explicit focused-content styling for `TextArea` |
| `document_view` | `DocumentViewPalette` | Explicit focused-content styling for `DocumentView` |
| `hex_area` | `HexAreaPalette` | Explicit focused-content/cursor styling for `HexArea` |
| `terminal` | `TerminalPalette` | Explicit focused-content styling for `Terminal` |
| `scrollbar` | `ScrollbarPalette` | Scrollbar track/thumb colors |
| `splitter` | `SplitterPalette` | Splitter handle colors |
| `file_icons` | `FileIconPalette` | File icon colors |
| `git_status` | `GitStatusPalette` | Git status badge colors |

Notes:

- `Theme::custom(fg, bg, accent)` derives `accent`, `selection`, `focus`, `border`, `muted`, `diff`, `document`, `syntax`, `scrollbar`, and `splitter` defaults from those three colors.
- Generic `hover` is disabled by default. Opt in with `Theme::hover(...)` when you want row/surface hover feedback.
- `Theme::focus_decoration(false)` is the complete theme-level focus-decoration kill switch. It suppresses inherited and extended `theme.focus`, per-widget focus palettes, automatic frame focus chrome, and `scrollbar.thumb_focus`. Explicit widget focus styles still render.
- `Theme::focus(Style::default())` only empties the generic focus role; use it when per-widget focus palettes should remain active.
- Buttons and other control-emphasis states use `accent`, not `selection`, so selection styling stays independent from interactive styling.
- Text-oriented widgets keep their normal text color on focus by default. Theme `focus` applies to focus chrome (borders, focus affordances), while `input.focus`, `text_area.focus`, `document_view.focus`, `hex_area.focus`, and `terminal.focus` opt into focused content styling.
- Widget APIs follow the same split: use `.focus_style(...)` for focus chrome and `.focus_content_style(...)` when you want focused text/content to change.
- Focus precedence is explicit widget focus style > `focus_decoration(false)` suppressing theme sources > `theme.focus` and per-widget palette defaults. Selection and `UnfocusedSelection` are selection styling, not focus decoration, and remain active.
- `DiffView` now uses `theme.diff` by default unless you explicitly override `diff_style(...)`.
- `DocumentView::markdown()` now uses `theme.document` by default unless you explicitly override formatter/document styles. Mermaid diagram blocks use `diagram_node_fill_style`, `diagram_node_border_style`, `diagram_node_label_style`, and `diagram_edge_style` on top of `code_block`; Gantt task bars derive foreground-only status shades from the diagram border/primary color, and explicit Mermaid flowchart `style` directives still win for node fill/border/label colors.
- `SyntectStrategy` now accepts a theme-native `syntax` palette as a hybrid recoloring layer on top of the selected syntect theme.
- `SyntaxPalette` includes separate `constant`, `builtin`, and `parameter` styles so syntect can distinguish booleans/null-like values, stdlib names, and function parameters from numbers or regular identifiers.

`DocumentPalette`, `SyntaxPalette`, and `DiffPalette` are role-keyed style tables
rather than per-widget `StyleSlot`s. Document rendering maps semantic roles
(headings, links, inline code, syntax tokens, diff additions/removals, line
numbers) across generated spans and formatter caches before the normal widget
renderer sees them, so these palettes act as theme tokens for content roles
instead of state slots such as hover/focus/selected. As a result,
`MarkdownView`, `DiffView`, and syntax-highlighted document content are currently
theme-only unless the specific widget or formatter exposes an explicit override
such as `diff_style(...)`, document styles, or syntax/document palette overrides.

## Color Contrast

Available in `tui_lipan::utils::color_contrast`:

```rust
use tui_lipan::utils::color_contrast;

// Pick a readable foreground for a given background.
// Tries: preferred → lightness-adjusted preferred → black or white.
let fg = color_contrast::readable_text_color(preferred_fg, bg);

// Simply pick black or white (Material Design / Apple HIG approach)
let fg = color_contrast::black_or_white(bg);

// Adjust a color's lightness to meet a contrast target (preserves hue)
let fg = color_contrast::adjust_for_contrast(fg, bg, 4.5);

// WCAG 2.1 metrics
let ratio = color_contrast::contrast_ratio(fg, bg);
let lum = color_contrast::relative_luminance(color);

// Color transforms (general-purpose, not used in readability logic)
let comp = color_contrast::complementary_color(color);
let inv = color_contrast::inverse_color(color);
```

**App-level contrast policy:**

```rust
App::new()
    .contrast_policy(ContrastPolicy::Wcag)          // default: WCAG 2.1 auto-adjust
    // or:
    .contrast_policy(ContrastPolicy::BlackOrWhite)  // keep readable fg, else snap to black/white
    // or:
    .contrast_policy(ContrastPolicy::Apca)          // APCA perceptual contrast
    // or:
    .contrast_policy(ContrastPolicy::Off)           // preserve explicit colors exactly
```

Per-widget override via `.contrast_policy(...)` on: `Button`, `Checkbox`, `Input`, `TextArea`, `List`, `Table`, `Tabs`, `DraggableTabBar`, `ProgressBar`.

You can also force contrast on a specific style after patching/theme resolution:

```rust
let label_style = Style::new()
    .transform_fg(ColorTransform::Dim(0.35))
    .contrast_policy(ContrastPolicy::BlackOrWhite);
```

## Color Gradients

```rust
use tui_lipan::prelude::*; // re-exports ColorGradient, GradientDirection, GradientRange

let gradient = ColorGradient::new(vec![
    (0.0, Color::rgb(0, 128, 255)),
    (0.5, Color::rgb(128, 0, 255)),
    (1.0, Color::rgb(255, 0, 128)),
]);

// Use in Sparkline, ProgressBar, Table heatmaps, etc.
ProgressBar::new(0.7).filled_gradient(gradient)
```
