# Example Catalog

All examples live in `examples/` and can be run with:

```bash
cargo run --example <name>
```

Web/WASM examples live under `examples/web/` and are standalone crates:

- `examples/web/hello`
- `examples/web/search_palette`

Feature-gated examples require `--features`:

```bash
cargo run --example image --features image
cargo run --example markdown_editor_sync --features markdown,syntax-syntect
```

---

## Getting Started

| Example | Description |
|---------|-------------|
| `todo` | Classic todo app: text input, add, toggle, scroll, delete confirmation |
| `dashboard` | Multi-panel dashboard: `Grid`, `Sparkline`, `StatusBar`, `Badge` |
| `forms` | Form patterns: `Radio`, `Select`, `ComboBox`, `Slider`, `DatePicker` |
| `form_validation` | Login-style validation: `Validator`, `Input::error(...)`, Enter-to-submit |
| `mockup` | Dashboard-style layout prototyping with `mockup!` (no `Component` code) |
| `ui_snapshot` | Agent-oriented UI snapshot export via `TestBackend::capture_ui_snapshot()` |
| `network_client_sketch` | Design-first `Mockup` sketch for a TUI Postman/Insomnia-style HTTP + GraphQL client; writes markdown plus font-backed PNG variants with `ui-snapshot-png` |
| `lazygit` | Lazygit-style multi-panel layout with focus hints and number-key switching |
| `showcase` | Broad demo: tabs, accordion, fuzzy palette, context menu, tree, toast, tooltip |
| `whack_a_mole` | Whack-a-mole arcade (terminal); same layout and rules as the WASM showcase tab |

---

## Layout & Containers

| Example | Description |
|---------|-------------|
| `length_percent` | `Length::Percent` for horizontal and vertical sizing |
| `frame_hub` | Frame demos hub: border merge, decorations, and divider/tab features |
| `splitter` | `Splitter` classic vs frame-join modes with resizable panes |
| `window_manager` | Hyprland-style tiling/floating window manager showcase with `Canvas`, `Transition<FloatRect>` geometry, workspaces, hover/framework focus integration, animated focus chrome, Alt-only keybindings, fullscreen, configurable title/focus/animation policy, smart local split-axis toggles, animated tiled drag/drop reflow, remembered floating geometry, position-aware float-to-tile toggles, persistent dwindle-tree target splitting for tiled windows, and corner-aware resize |
| `modal_auto_height` | `Modal` + `List` with `Auto` vs `max_height` constraints |
| `modal_percent_repro` | Modal height percent and `OverlayScope` behavior |
| `mouse_region_click` | `MouseRegion`: passthrough vs capturing button/region clicks |

---

## Input Widgets

| Example | Description |
|---------|-------------|
| `inputs` | Single-line and multi-line inputs: username, password, search, bio |
| `drag_drop_kanban` | Generic `DragSource` / `DropTarget` kanban board with preview and cancel |
| `sidebar_tabs` | Rich vertical sidebar tabs composed from primitives: label + description + status spinner per item, drag to reorder with insertion indicator |
| `text_area` | `TextArea` editor paired with read-only viewer: scroll, selection, line numbers, Vim modal editing |
| `text_area_sentinels` | `TextArea` sentinels: `@` file picker tokens, stash/restore snapshots |
| `text_area_virtual_text` | `TextArea` virtual text: inline inlay hints and end-of-line diagnostics absent from the buffer |
| `multi_select` | `MultiSelect`: space toggle, enter commit |
| `hex_area` | `HexArea`: binary editing, cursor, selection, scroll |

---

## Data Widgets

| Example | Description |
|---------|-------------|
| `table` | `Table`: header, rows, selection, scroll + inspector preset (tabs, sections, key/value, disclosure) |
| `list_headers` | `List` with section headers and spacers |
| `log_viewer` | `LogView` over a sample file: filter, follow, pause, level coloring |
| `provided_file_tree` | Async application-provided `FileTree` listings with loading, Git, and ignore metadata |

---

## Tabs & Navigation

| Example | Description |
|---------|-------------|
| `tabs_hub` | Tabs demos hub: `TabVariant` styles + draggable tab bar behaviors |
| `pagination_composed` | Composed pagination controls with page-size dropdown |

---

## Search & Overlays

| Example | Description |
|---------|-------------|
| `search_palette_hub` | `SearchPalette` hub: uncontrolled, controlled, delete-confirm, description placement, popover overlays |
| `command_palette` | `CommandPalette` + command registry demo: open with `p`, app and child commands |
| `search_lists` | Global `/` search plus per-panel filters over files, branches, tasks |

---

## Display & Visualization

| Example | Description |
|---------|-------------|
| `ascii_canvas_ghost` | ASCII ghost animation using `AsciiCanvas` with frame sequences |
| `ascii_canvas_scroll` | `AsciiCanvas` inside `ScrollView`: row-skip clipping and gradient rendering |
| `animated_showcase` | `Animated` opacity, height, and keyed position transitions with easing/duration controls |
| `animated_sequential_swap` | Sequential fade between subtrees via `on_opacity_transition_end` (fg+bg, fg-only, framed) |
| `chart_showcase` | `Chart` with multiple series, thresholds, axes, legend, grid |
| `diagram_showcase` | Tabbed diagram hub covering `SequenceDiagram`, `ClassDiagram`, `ErDiagram`, `StateDiagram`, and `GanttDiagram` examples |
| `flowchart_showcase` | `Flowchart` with branching, cycles, dashed/thick edges, nested subgraphs, class styles, and item callbacks |
| `graph_showcase` | Scrollable `Graph` layouts with compact and rounded node/edge variants, clickable nodes, hover styling, and path status |
| `pan_view` | `PanView` with a centered clickable `Graph`: bounded free panning, arrow/hjkl controls, and animated reset-to-center |
| `sparkline_showcase` | Animated `Sparkline` samples (CPU-like usage, download/upload) |
| `heatmap` | Heatmap-style grid with color gradients |
| `progress_zones` | Several `ProgressBar` styles/zones (CPU, memory, disk, network) |
| `visual_effects` | `EffectScope` post-processing: monochrome, palettes, scanlines, rainbow, CRT presets |
| `effects_ripple` | Phase-owned ripple effects: looping background wave plus one-shot burst |
| `effect_scope_dot_field` | Tabbed full-screen custom `EffectScope` shaders: neon bloom, veiled lights, ember drift, hearth glow |

---

## Styling & Themes

| Example | Description |
|---------|-------------|
| `theme_showcase` | `ThemeProvider` and switchable built-in themes |
| `live_host_colors` | `App::system_theme()` plus runner-managed host terminal color refresh for app-owned tokens |
| `alpha_foreground_backdrop` | Repro for alpha foreground blending against parent backgrounds vs terminal fallback |
| `color_contrast` | Automatic text readability on varied backgrounds |
| `gradient_widgets` | Gradient-themed `ProgressBar`, `Slider`, `Table`, `SearchPalette` |
| `hover` | Hover styles across interactive widgets |

---

## Inline Mode

| Example | Description |
|---------|-------------|
| `inline` | Inline viewport: draft input and `ctx.append_transcript_lines` rich log lines |
| `native_scroll_chat` | Claude Code / Gemini CLI style transcript using `ctx.append_transcript_element` + native scrollback |
| `inline_choices` | Inline theme/target pickers and apply-to-insert summary |
| `inline_list_picker` | Inline list picker over command list |

---

## ScrollView

| Example | Description |
|---------|-------------|
| `scroll_view_scroll_to_key` | `ScrollView` + smooth `scroll_to_key`: search jumps to matching keyed message |
| `scroll_view_both_axes` | `ScrollView` with `ScrollAxis::Both`: vertical + horizontal pan and scrollbars |
| `smooth_scroll_targets` | Distance-adaptive smooth target scrolling across `ScrollView`, `DocumentView`, and `TextArea` with user-cancel behavior |
| `scroll_view_stress` | Stress: many `Frame` + `DocumentView` children (FPS) |
| `scroll_view_opencode_repro` | Opencode-like session: sidebar, timeline, search/theme overlays |

---

## Diagnostics & Stress Tests

| Example | Description |
|---------|-------------|
| `diagnostics` | Render/mouse diagnostics hub: active, idle, and tabbed behavior metrics |
| `stress_test` | Large list + text field + optional FPS overlay |
| `widget_gallery` | Built-in widgets: checkboxes, progress bars, spinners, sliders |
| `widgets` | Filterable demo: tabs, list, table, scroll, border toggles |
| `auto_height_test` | `TextArea` + `DocumentView` with `Length::Auto` height behavior |

---

## Feature-Gated Examples

### `big-text`

| Example | Description |
|---------|-------------|
| `big_text` | `BigText`: FIGlet fonts, horizontal/vertical/rainbow gradients |
| `figlet_editor` | FIGlet editor: per-character glyphs, font selection, import/export |

```bash
cargo run --example big_text --features big-text
cargo run --example figlet_editor --features big-text
```

### `diff-view`

| Example | Description |
|---------|-------------|
| `diff_hub` | `DiffView` hub: before/after compare and patch-based unified/split modes |
| `diff_hunk_navigation` | Global hunk navigation across one auto-height patch-backed `DiffView` per file inside a keyed `ScrollView` |

```bash
cargo run --example diff_hub --features diff-view
cargo run --example diff_hunk_navigation --features diff-view
```

### `image`

| Example | Description |
|---------|-------------|
| `image` | `Image` widget: procedural PNG, GIF, playback controls |
| `image_modes` | `TextArea` inline image sentinels vs attachment mode |
| `messenger` | Chat UI with plain/rich messages and optional images |

```bash
cargo run --example image --features image
cargo run --example image_modes --features image
cargo run --example messenger --features image
```

### `markdown`

| Example | Description |
|---------|-------------|
| `markdown_hub` | Markdown hub: preview, table rendering options, and hyperlink handling |
| `document_view_mermaid` | Markdown `DocumentView` rendering Mermaid fenced blocks, including Gantt schedules, for supported diagram types |
| `scroll_view_stress` | Stress: `Frame` + `DocumentView` children in `ScrollView` |
| `scroll_view_opencode_repro` | Opencode-like session with sidebar and overlays |

```bash
cargo run --example markdown_hub --features markdown
cargo run --example document_view_mermaid --features markdown
```

### `markdown` + `syntax-syntect`

| Example | Description |
|---------|-------------|
| `markdown_editor_sync` | Live markdown editor + preview with bidirectional scroll sync |

```bash
cargo run --example markdown_editor_sync --features markdown,syntax-syntect
```

### `syntax-syntect` and `syntax-extra`

| Example | Description |
|---------|-------------|
| `syntax_theme_compare` | Side-by-side syntax highlighting themes |
| `yazi` | Compact Yazi-inspired file browser: resizable borderless panes, keyboard/mouse navigation, full-row selection, directory previews, Nerd Font icons, and syntax-highlighted file previews |

```bash
cargo run --example syntax_theme_compare --features syntax-syntect
cargo run --example yazi --features syntax-extra
```

`yazi` requires `syntax-extra` because its shared file-path detection is used to
demonstrate the extended grammar set.

### `terminal`

| Example | Description |
|---------|-------------|
| `terminal_filetree_devtools` | `FileTree` + `ManagedTerminal` devtools split |

```bash
cargo run --example terminal_filetree_devtools --features terminal
```

### `devtools`

| Example | Description |
|---------|-------------|
| `devtools` | Minimal app that emits periodic `debug_log!` lines for the DevTools logs tab |

```bash
cargo run --example devtools --features devtools
```
