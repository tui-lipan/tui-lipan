# Display Widgets (Read-Only)

Structural and Mermaid-style diagram widgets (`Graph`, `Flowchart`,
`SequenceDiagram`, `ClassDiagram`, `StateDiagram`, `ErDiagram`, `GanttDiagram`)
live on their own page: [Diagrams](diagrams.md).

## Text

Renders styled text.

| Prop | Type | Description |
|------|------|-------------|
| `content` | `impl Into<String>` | **Constructor** - text content |
| `spans` | `impl IntoIterator<Item = Span>` | Construct from styled spans |
| `from_ansi` | `&str` | Construct from ANSI-escaped string (SGR sequences → styled spans) |
| `style` | `Style` | Text style |
| `overflow` | `Overflow` | `Clip`, `Ellipsis`, `Wrap` |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

```rust
Text::new("Hello, World!")
    .style(Style::new().fg(Color::Cyan).bold())
    .overflow(Overflow::Ellipsis)
```

```rust
// Render ANSI-styled output (ls --color, compiler errors, git diff, etc.)
Text::from_ansi("\x1b[31merror\x1b[0m: file not found")
```

---

## DocumentView

Read-only rich document renderer with pluggable formatting.

Use it for markdown previews, formatted logs, and custom read-only views where
display content differs from source text.

| Prop | Type | Description |
|------|------|-------------|
| `value` | `impl Into<Arc<str>>` | **Constructor** - source text |
| `content_type` | `Arc<str>` | Optional formatter hint (`"markdown"`, etc.) |
| `formatter` | `impl ContentFormatter` | Custom formatting strategy |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `wrap` | `bool` | Word-wrap long lines (default: `true`) |
| `border` | `bool` | Show/hide outer border (default: `true`) |
| `border_style` | `BorderStyle` | Border glyph variant |
| `hover_border_style` | `BorderStyle` | Border variant when hovered |
| `padding` | `impl Into<Padding>` | Inner padding |
| `table_wrap` | `bool` | Wrap table cell text within column width |
| `table_width_mode` | `DocumentTableWidthMode` | `Content` (natural) or `Fill` (stretch to viewport) |
| `table_outer_frame` | `bool` | Show/hide outer table frame |
| `table_inner_frame` | `bool` | Show/hide inner table separators |
| `table_cell_padding` | `u16` | Horizontal padding inside each table cell |
| `table_border_variant` | `BorderStyle` | Table border glyph variant (`Plain`, `Rounded`, `Double`, etc.) |
| `table_border_style` | `Style` | Style for table borders (color/emphasis) |
| `line_numbers` | `bool` | Source-mapped line-number gutter |
| `line_number_mode` | `DocumentLineNumberMode` | `Visual` (visual row index) or `Source` (source line mapping) |
| `min_line_number_width` | `u8` | Minimum line-number gutter digit width |
| `line_number_separator` | `bool` | Show/hide built-in line-number separator (`" │ "`, default: `true`) |
| `line_number_content_gap` | `u16` | Empty cells between built-in line numbers and content |
| `line_number_style` | `Style` | Style override for built-in line-number gutter text |
| `gutter_inset` | `u16` | Empty cells before the gutter / line numbers |
| `style` | `Style` | Base style |
| `hover_style` | `Style` | Hover style |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `focus_style` | `Style` | Focus chrome style |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `focus_content_style` | `Style` | Text content style when focused |
| `selection_style` | `Style` | Text selection style |
| `extend_selection_style` / `inherit_selection_style` | `Style` / `()` | Extend or inherit the text-selection theme role instead of replacing it |
| `highlight_full_width` | `bool` | Extend per-line background highlights across full content width |
| `doc_styles` | `DocumentStyles` | Element styles (heading/code/link/table/hr/diagram/etc.) |
| `code_block_style` | `Style` | Shortcut - sets `doc_styles.code_block_style` |
| `scroll_offset` | `usize` | Controlled vertical scroll offset |
| `scroll_to_source_line` | `usize` | Scroll-sync target source line |
| `scroll_behavior` | `ScrollBehavior` | `Instant` by default; opt into smooth `scroll_to_source_line` movement |
| `scroll_transition` | `TransitionConfig` | Shortcut for smooth source-line target movement |
| `scrollbar` | `bool` | Show/hide vertical scrollbar |
| `scrollbar_config` | `ScrollbarConfig` | Full scrollbar configuration (variant, gap, thumb, thumb styles) |
| `h_scrollbar` | `bool` | Show/hide horizontal scrollbar (only when `wrap` is `false`) |
| `scroll_wheel` | `bool` | Enable/disable mouse wheel scrolling (default: `true`) |
| `scroll_wheel_multiplier` | `u16` | Override the app-wide wheel line multiplier for this DocumentView |
| `focusable` | `bool` | Participate in focus traversal; mouse selection and copy shortcuts still work when false |
| `on_scroll` | `Callback<ScrollEvent>` | Scroll callback |
| `on_click` | `Callback<DocumentClickEvent>` | Click callback with source-line mapping |
| `on_select` | `Callback<DocumentSelectEvent>` | Text selection callback |
| `on_key` | `KeyHandler` | Focused keyboard handler |
| `shared_selection_id` | `Arc<str>` | Group id for cross-DocumentView selection/copy within the same `ScrollView` |
| `triple_click_mode` | `TripleClickSelectionMode` | Triple-click selects a visual line or paragraph |

`DocumentView` ships with `PlainFormatter` by default.

For inline/message-style read-only blocks, wrapped `DocumentView` now
implicitly behaves like `height: Length::Auto` when all of these are true:
default `height: Flex(1)`, `wrap: true`, `scrollbar: false`,
`h_scrollbar: false`, and `focusable: false`. Set `.height(...)`
explicitly when you want viewport-style behavior instead.

When using `DocumentView::markdown()`, default heading/link/code/table styling is
derived from `Theme::document`. Use `.doc_styles(...)` or a custom
`MarkdownFormatter::styles(...)` only when you want local overrides.

With feature `markdown`, you can use `MarkdownFormatter`:

```rust
DocumentView::new("# Hello\n\n| A | B |\n|---|---|\n| 1 | 2 |")
    .markdown()
    .line_numbers(true)
    .wrap(true)

DocumentView::new(text)
    .markdown_compact(true)
```

`.markdown_compact(true)` drops blank source lines between blocks so the
rendered output sits flush against itself. Useful for tight previews such as
chat bubbles, list cells, or narrow side panels where vertical space matters.
`.markdown()` preserves blank lines and adds a spacer before adjacent fenced
code/diagram blocks for readable paragraph spacing. The toggle is also
available directly on the formatter as
`MarkdownFormatter::compact_blocks(bool)`.

Mermaid fenced blocks render as diagrams for the supported subset: flowchart,
sequence, class, state, ER, pie, and gantt diagrams. To build any of these as a
widget directly (rather than from a markdown fence), see the
[Diagrams](diagrams.md) page. Flowcharts tolerate `subgraph`
grouping and standalone node declarations. Sequence diagrams support
participants, actors, arrow messages, self messages, `loop` grouping, and
single-line `Note over`, `Note left of`, or `Note right of` notes. State
diagrams tolerate composite `state ... { ... }` grouping. Gantt diagrams support
`title`, `dateFormat YYYY-MM-DD`, `section`, task ids, explicit `YYYY-MM-DD`
starts, `after <id>` dependencies, day durations, and `crit` / `active` /
`done` / `milestone` tags. Diagram cells inherit `code_block_style`, then apply
the diagram-specific `DocumentStyles` slots
(`diagram_node_fill_style`, `diagram_node_border_style`,
`diagram_node_label_style`, and `diagram_edge_style`) for flowchart, sequence,
class, state, and ER diagrams. Gantt diagrams use label, primary/border, edge,
and muted diagram slots, then derive foreground-only status task-bar shades from
the primary/border color with the shared gradient utility so `pending`,
`active`, `done`, `critical`, and `milestone` rows remain distinguishable inside
themed markdown previews without painting a task-area background. Pie diagrams
are text-only and inherit only the code-block style.
Flowcharts support Mermaid `style` directives for opaque hex
`fill`, `stroke`, and `color` node colors; those explicit directives override
the diagram theme slots. Alpha hex in Mermaid style directives is rejected for
now rather than silently ignored.

To render mermaid fences as plain code blocks instead of diagrams, call
`.render_diagrams(false)` on a `DocumentView` already configured with
`.markdown()` / `.markdown_compact(...)`. The setting also exists directly on
`MarkdownFormatter::render_diagrams(bool)` for callers wiring a custom
formatter. Default is `true`.

```rust
DocumentView::new(markdown_source)
    .markdown()
    .render_diagrams(false)
```

Keyboard scrolling (when focused): arrows, `j/k`, `PageUp/PageDown`, `Home/End`.

`scroll_to_source_line(...)` maps the zero-based source line to the first matching
wrapped visual row. Add `.scroll_behavior(ScrollBehavior::smooth_default())`,
`.scroll_behavior(ScrollBehavior::smooth_adaptive())`, or `.scroll_transition(config)`
to animate that programmatic target; adaptive timing derives duration from row
distance and caps long jumps. Controlled `scroll_offset`, mouse wheel/key scrolling,
and scrollbar drag remain immediate and cancel any active smooth target.

Mouse text selection is independent of focusability: `.focusable(false)` removes
the widget from focus traversal while still allowing drag selection. Wheel events
bubble to ancestor scroll containers when `.scroll_wheel(false)` is set or when
the `DocumentView` content is not clipped.

Use `.scroll_wheel_multiplier(lines)` when one `DocumentView` should scroll a
different number of lines per wheel tick than the app-wide
`App::scroll_wheel_multiplier(...)` setting.

Table drag-selection supports rectangular selection by row/column and copies as TSV.

When sibling `DocumentView` widgets under the same `ScrollView` share
`shared_selection_id`, linear drag selection can continue across widget
boundaries and shared copy concatenates text in visual order with newline
separators between document boundaries.

### Custom markdown styles

`DocumentStyles` controls per-element colors. Default values come from `Theme::document`
automatically - only set this when you want widget-local overrides.

```rust
// Via DocumentView::doc_styles - applies to any formatter
DocumentView::new(text)
    .markdown()
    .doc_styles(DocumentStyles {
        heading_styles: [
            Style::new().bold().fg(Color::Cyan),   // h1
            Style::new().bold().fg(Color::Blue),   // h2
            Style::new().bold().fg(Color::Green),  // h3
            Style::new().bold(),                   // h4
            Style::new().bold(),                   // h5
            Style::new().bold().dim(),             // h6
        ],
        link_style: Style::new().fg(Color::Blue).underline(),
        code_inline_style: Style::new().fg(Color::Green),
        code_block_style: Style::new().bg(Color::rgb(0x1E, 0x1E, 0x1E)),
        emphasis_style: Style::new().italic(),
        strong_style: Style::new().bold(),
        strikethrough_style: Style::new().strikethrough(),
        blockquote_bar_style: Style::new().fg(Color::DarkGray),
        table_border_style: Style::new().fg(Color::DarkGray),
        table_header_style: Style::new().bold(),
        hr_style: Style::new().fg(Color::DarkGray).dim(),
        list_item_style: Style::new().fg(Color::Blue).bold(),
        list_enumeration_style: Style::new().fg(Color::Blue).bold(),
        diagram_node_fill_style: Style::new().bg(Color::rgb(0x12, 0x18, 0x22)),
        diagram_node_border_style: Style::new().fg(Color::Cyan),
        diagram_node_label_style: Style::new().fg(Color::White),
        diagram_edge_style: Style::new().fg(Color::LightBlue),
    });

// Via MarkdownFormatter::styles - same effect, explicit formatter path
DocumentView::new(text)
    .formatter(MarkdownFormatter::default().styles(DocumentStyles {
        strong_style: Style::new().bold().fg(Color::Yellow),
        ..DocumentStyles::default()  // theme fills in the rest automatically
    }));
```

### Syntax highlighting in code blocks *(requires feature `syntax-syntect`)*

```rust
DocumentView::new(text)
    .markdown()
    .code_syntax_strategy(SyntectStrategy::default().default_theme("One Dark (Atom)"))
// .markdown() already sets this default theme; override here to use a different one
```

---

## AsciiCanvas

Renders ASCII art as text lines, cell grids, or multi-frame sprite sheets.

### Constructors

```rust
// Line-based content
AsciiCanvas::new(["Line 1", "Line 2"])

// Cell grid
AsciiCanvas::from_cells(width, height, cells)

// Blank grid for programmatic fill
AsciiCanvas::blank(width, height)

// Generated grid
AsciiCanvas::with_cell_fn(width, height, |x, y| AsciiCanvasCell { ch, fg, bg })

// Multi-frame sprite sheet
AsciiCanvas::from_sequence(Arc::new(frame_sequence))
```

### Frame Sequence API

```rust
let seq = Arc::new(FrameSequence::from_json(&json_str).unwrap());

let canvas = AsciiCanvas::from_sequence(seq.clone())
    .frame(0)                            // Select frame by index
    // or: .frame_by_tag("key", "value") // Select frame by tag
```

`FrameSequence::from_json()` parses ASCII Motion export format. Supports `foreground` and `background` color maps.

### Color Remapping

```rust
// Unified map (both fg and bg)
let colors = seq.collect_colors();         // All unique colors, fg+bg merged
let canvas = AsciiCanvas::from_sequence(seq)
    .color_map(vec![
        (colors[0], theme.highlight.fg.unwrap_or(Color::White)),
        (colors[1], Color::hex("#3A3A3A")),
    ]);

// Per-channel maps (when same hex appears in both fg and bg)
let fg_colors = seq.collect_fg_colors();   // Unique foreground colors
let bg_colors = seq.collect_bg_colors();   // Unique background colors
let canvas = AsciiCanvas::from_sequence(seq)
    .fg_color_map(vec![(fg_colors[0], Color::White)])
    .bg_color_map(vec![(bg_colors[0], Color::Black)]);
```

Per-channel maps take precedence over the unified map for their respective channel.

### Props

| Prop | Type | Description |
|------|------|-------------|
| `lines` | `Vec<String>` | **Constructor** - line-based content |
| `style` | `Style` | Base style |
| `background` | `Style` | Background-only style |
| `color_map` | `Vec<(Color, Color)>` | Unified fg+bg remap |
| `fg_color_map` | `Vec<(Color, Color)>` | Foreground-only remap |
| `bg_color_map` | `Vec<(Color, Color)>` | Background-only remap |
| `grid_size` | `(u16, u16)` | Grid dimensions (must match cells count) |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

---

## BigText *(requires feature `big-text`)*

Large text rendered with ASCII or pixel fonts.

| Prop | Type | Description |
|------|------|-------------|
| `text` | `impl Into<RichText>` | Text content, set with `.text(...)`; accepts `Vec<Span>` for multicolor |
| `font` | `BigFont` | Font choice (see below) |
| `style` | `Style` | Base style (per-span styles override this) |
| `shadow` | `impl Into<Option<Shadow>>` | Shadow configuration |
| `with_shadow` | `Style` | Quick shadow with given style |
| `custom_figlet` | `impl Into<Arc<str>>` | Custom `.flf` FIGlet font content |
| `custom_figlet_from_file` | `impl AsRef<Path>` | Load custom `.flf` FIGlet font content from a file |
| `gradient` | `ColorGradient`, `GradientDirection` | Render-time color gradient |

`BigText::new()` creates an empty auto-sized widget; set content with `.text(...)`.
It does not expose `width` or `height` builder setters.

**FIGlet fonts**: `Standard`, `Slant`, `Bloody`, `Colossal`, `Roman`, `SubZero`, `Poison`, `Nancyj`, `SmallPoison`, `DosRebel`, `AnsiShadow`, `Small`, `CustomFiglet`

**Pixel fonts**: `Pixel` (8x8 half blocks), `PixelBold`, `Quadrant` (2×2 block mapping)

```rust
BigText::new()
    .text("Hello")
    .font(BigFont::AnsiShadow)
    .style(Style::new().fg(Color::Cyan))

// Multicolor via spans
BigText::new()
    .text(vec![
        Span::new("open").fg(Color::Cyan),
        Span::new("code").fg(Color::White),
    ])
    .font(BigFont::Standard)

// Render-time gradients
BigText::new()
    .text("FIRE")
    .font(BigFont::SubZero)
    .gradient(
        ColorGradient::new(Color::Yellow, Color::rgb(200, 0, 0)),
        GradientDirection::Vertical,
    )
```

> FIGlet smushing does not cross span boundaries in multicolor mode.

---

## Image *(requires feature `image`)*

Protocol-aware image rendering.

The default `image` feature enables PNG, JPEG, GIF, and WebP codecs to keep app
binary size modest. Add `image-full-formats` if your app needs the broader
`image` crate default codec set, such as AVIF, BMP, DDS, EXR, HDR, ICO, PNM,
QOI, TGA, or TIFF.

| Prop | Type | Description |
|------|------|-------------|
| `src` | `impl Into<String>` | **Constructor** - image file path |
| `bytes` | `Arc<[u8]>` | In-memory image (use `Image::from_bytes(...)`) |
| `fit` | `ImageFit` | `Contain` (default), `Crop`, `Scale` |
| `protocol` | `ImageProtocol` | `Auto`, `Kitty`, `Iterm2`, `Sixel`, `Halfblocks` |
| `style` | `Style` | Container style |
| `alt` | `String` | Alt text shown when protocol fails |
| `playback` | `ImagePlayback` | `Playing`, `Paused` |
| `repeat` | `ImageRepeat` | `Loop`, `Once` |
| `speed_percent` | `u32` | Animation speed percentage |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

```rust
Image::new("logo.png")
    .fit(ImageFit::Contain)
    .alt("Company Logo")

// From memory
let bytes: Arc<[u8]> = load_image_bytes();
Image::from_bytes(bytes)
    .protocol(ImageProtocol::Auto)
```

**Animation** (GIF, animated WebP, APNG): advance automatically.
- Small GIFs are preloaded; large GIFs use a bounded worker channel.
- Controls via `playback`, `repeat`, `speed_percent`.

**Environment knobs:**

| Variable | Default | Description |
|----------|---------|-------------|
| `TUI_LIPAN_IMAGE_MAX_FPS` | `30` | Frame rate cap |
| `TUI_LIPAN_IMAGE_MAX_CATCHUP_MS` | `100` | Frame catch-up window |
| `TUI_LIPAN_IMAGE_AUTO_ANIM_HALF_BLOCKS` | `false` | Allow halfblocks for animations |
| `TUI_LIPAN_IMAGE_ENCODE_WORKERS` | `1` | Async encoding workers (1–2) |
| `TUI_LIPAN_IMAGE_GIF_PRELOAD` | `true` | Eager GIF preloading |
| `TUI_LIPAN_IMAGE_GIF_PRELOAD_MAX_BYTES` | `262144` | Preload size cap |
| `TUI_LIPAN_IMAGE_GIF_PRELOAD_MAX_FRAMES` | `24` | Preload frame cap |
| `TUI_LIPAN_IMAGE_GIF_PRELOAD_BUDGET_MS` | `16` | Preload time cap |
| `TUI_LIPAN_IMAGE_GIF_WORKER_QUEUE` | `2` | Worker queue size (1–4) |
| `TUI_LIPAN_IMAGE_RESIZE_PAUSE_MS` | `180` | Pause rendering during resize |
| `TUI_LIPAN_IMAGE_LAYOUT_STABILIZE_MS` | `120` | Pause while layout changes |

---

## Sparkline

Minimal inline chart for time-series data.

| Prop | Type | Description |
|------|------|-------------|
| `data` | `Vec<u64>` | **Constructor** - data points |
| `variant` | `SparklineVariant` | `Bars` (default), `Braille`, `Line` |
| `min` | `Option<u64>` | Data minimum (auto if None) |
| `max` | `Option<u64>` | Data maximum (auto if None) |
| `chart_height` | `u16` | Multi-row height for Bars/Braille/Line |
| `mirror_x` | `bool` | Reverse sample order (time axis) |
| `mirror_y` | `bool` | Flip vertical direction |
| `max_points` | `Option<usize>` | Cap rendered width (enables downsampling) |
| `aggregation` | `SparklineAggregation` | `Average`, `Min`, `Max`, `First`, `Last` |
| `zero_policy` | `SparklineZeroPolicy` | `MinGlyph` for baseline on zeros |
| `gradient` | `ColorGradient` | Map values → RGB colors |
| `height_gradient` | `ColorGradient` | Map row position → RGB colors |
| `gradient_range` | `GradientRange` | Normalize gradient range |
| `style` | `Style` | Base style |
| `rising_style` | `Style` | Style for rising values |
| `falling_style` | `Style` | Style for falling values |
| `overflow` | `Overflow` | Default: `Ellipsis`; use `ClipStart` for live charts |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

```rust
Sparkline::new(metrics.clone())
    .variant(SparklineVariant::Braille)
    .chart_height(4)
    .overflow(Overflow::ClipStart)   // Keep newest data visible
    .gradient(ColorGradient::new(vec![
        (0.0, Color::Green),
        (1.0, Color::Red),
    ]))
```

---

## Chart

Multi-series chart with axes, legend, thresholds, and viewport windowing.

| Prop | Type | Description |
|------|------|-------------|
| `series` | `Vec<ChartSeries>` | Data series |
| `x_axis` | `ChartAxis` | X axis configuration |
| `y_axis` | `ChartAxis` | Y axis configuration |
| `thresholds` | `Vec<ChartThreshold>` | Horizontal threshold lines |
| `viewport_start` | `Option<usize>` | Start sample index for zoom |
| `viewport_len` | `Option<usize>` | Number of samples to show |
| `show_legend` | `bool` | Show series legend |
| `show_grid` | `bool` | Show background grid |
| `legend_style` | `Style` | Legend style |
| `grid_style` | `Style` | Grid style |
| `legend_separator` | `char` | Legend entry separator |
| `border` | `bool` | Draw border |
| `border_style` | `BorderStyle` | Border appearance |
| `padding` | `impl Into<Padding>` | Inner padding |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

```rust
let series = ChartSeries::new("CPU", cpu_data)
    .mode(ChartSeriesMode::Line)
    .style(Style::new().fg(Color::Cyan));

Chart::new()
    .series(vec![series])
    .show_legend(true)
    .show_grid(true)
    .thresholds(vec![ChartThreshold::new(80.0).style(Style::new().fg(Color::Red))])
```

---

## Heatmap

2D matrix visualization with gradient-colored cells or glyphs.

| Prop | Type | Description |
|------|------|-------------|
| `data` | `Vec<Vec<f64>>` | **Constructor** - 2D matrix of values |
| `row_labels` | `Vec<impl Into<Arc<str>>>` | Labels on the left side |
| `column_labels` | `Vec<impl Into<Arc<str>>>` | Labels on top |
| `gradient` | `ColorGradient` | Color gradient for value mapping |
| `range` | `(f64, f64)` | Explicit min/max for gradient normalization |
| `cell_mode` | `HeatmapCellMode` | `Background`, `Glyph(Arc<str>)`, or `GlyphForeground(Arc<str>)` |
| `cell_width` | `u16` | Character width per cell (default: 4) |
| `gap_x` | `u16` | Horizontal gap between cells in characters |
| `gap_y` | `u16` | Vertical gap between heatmap rows in lines |
| `legend_gap` | `u16` | Horizontal gap between legend markers/swatches |
| `legend_spacing` | `u16` | Vertical gap between the heatmap grid and legend |
| `legend_width` | `HeatmapLegendWidth` | Legend alignment: grid width or full inner width |
| `show_values` | `bool` | Display numeric values in cells |
| `show_legend` | `bool` | Show gradient legend below |
| `style` | `Style` | Base style |
| `label_style` | `Style` | Row/column label style |
| `legend_style` | `Style` | Legend style |
| `padding` | `impl Into<Padding>` | Inner padding |
| `border` | `bool` | Draw border |
| `border_style` | `BorderStyle` | Border appearance |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

Use `HeatmapCellMode::Glyph(...)` when you want a repeated glyph texture with colored tile
backgrounds, or `HeatmapCellMode::GlyphForeground(...)` when you want only the glyph colored and
the background left untouched. Both glyph modes accept strings like `" "` as well as single
characters. `gap_x` and `gap_y` work in all modes and make sparse glyph layouts much easier to
read. `legend_gap` separates legend markers from each other, `legend_spacing` adds space between
the heatmap grid and the legend itself, and `legend_width(HeatmapLegendWidth::Full)` lets the
legend ignore the row-label gutter and stretch across the full inner width.

```rust
let data = vec![
    vec![10.0, 25.0, 40.0, 55.0],
    vec![20.0, 35.0, 50.0, 65.0],
    vec![30.0, 45.0, 60.0, 75.0],
];

Heatmap::new(data)
    .row_labels(["Low", "Med", "High"])
    .column_labels(["Q1", "Q2", "Q3", "Q4"])
    .gradient(ColorGradient::new(Color::Rgb(60, 179, 113), Color::Rgb(226, 82, 87)))
    .range(0.0, 100.0)
    .cell_mode(HeatmapCellMode::GlyphForeground(" ".into()))
    .gap_x(1)
    .gap_y(1)
    .legend_gap(1)
    .legend_spacing(1)
    .legend_width(HeatmapLegendWidth::Full)
    .show_legend(true)
    .border(true)
```
