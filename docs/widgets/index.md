# Widget Reference

All widgets are available via `use tui_lipan::prelude::*;`

## Layout & Containers → [layout.md](layout.md) · [effects.md](effects.md) (`VisualEffect`)

| Widget | Description |
|--------|-------------|
| `VStack` | Vertical stack container |
| `HStack` | Horizontal stack container |
| `ZStack` | Overlay container (children stacked) |
| `Canvas` | Absolute-positioned child container with local `Rect` placement |
| `Frame` | Container with border, title, status line |
| `Grid` | 2D grid layout |
| `Flow` | Wrapping row container for chip/tag-like content |
| `ScrollView` | Scrollable container |
| `PanView` | Single-child 2D panning viewport for diagrams and oversized content |
| `Center` | Centers a single child |
| `CenterPin` | Absolutely positions a child relative to a center point |
| `EffectScope` | Applies style and `VisualEffect` post-processing to a rendered subtree |
| `Animated` | Opacity / fg/bg color lerp / height / keyed position transition wrapper (`layout_height`, `position_transition`, transition-end callbacks; see [layout.md](layout.md#animated)) |
| `Spacer` | Flexible empty space |
| `Divider` | Visual separator line |
| `MouseRegion` | Pointer click/move/drag wrapper with optional hover style |
| `Splitter` | Resizable panes with draggable handles |
| `ThemeProvider` | Applies a `Theme` to a subtree (see [styling.md](../styling.md)) |
| `ContextProvider` | Provides typed context to a subtree (`ctx.use_context::<T>()`) |

### Animated

For fading between structurally different subtrees, tween opacity 1.0 → 0.0 on the outgoing content, swap the child inside `on_opacity_transition_end`, then tween 0.0 → 1.0. A single `Animated` lane is sufficient - no ZStack needed.

For layout reorders, wrap the moving subtree in `Animated::new(...)`, enable `.position_transition(true)`, and give that wrapper a stable `.key(...)`. Position transitions are visual-only FLIP movement: layout, focus, and pointer hit testing move to the final rect immediately while paint animates from the previous screen position.

## Display (Read-Only) → [display.md](display.md)

| Widget | Feature | Description |
|--------|---------|-------------|
| `Text` | - | Styled text |
| `DocumentView` | `markdown` *(optional formatter)* | Read-only rich document view with tables/code/blockquote rendering |
| `AsciiCanvas` | - | ASCII art, cell grids, sprite sheets |
| `BigText` | `big-text` | Large text via FIGlet/pixel fonts |
| `Image` | `image` | Protocol-aware image (Kitty, iTerm2, Sixel, halfblocks) |
| `Sparkline` | - | Inline time-series chart |
| `Chart` | - | Multi-series chart with axes and legend |
| `Heatmap` | - | 2D matrix with gradient-colored cells |

## Diagrams → [diagrams.md](diagrams.md)

| Widget | Description |
|--------|-------------|
| `Graph` | Node-edge structural visualization with clickable, focusable tree nodes |
| `Flowchart` | Mermaid-style directed flowcharts with shapes, edge labels, subgraphs, classes, and item callbacks |
| `SequenceDiagram` | Mermaid-style participant/message timelines with fragments, notes, and item callbacks |
| `ClassDiagram` | Static UML class diagrams with compartments and relation glyphs |
| `StateDiagram` | Static UML state diagrams with transitions and pseudo-state glyphs |
| `ErDiagram` | Static entity-relationship diagrams with crow's-foot cardinality glyphs |
| `GanttDiagram` | Static timeline schedules with sections, task bars, dependencies, and milestones |

## Input → [input.md](input.md)

| Widget | Description |
|--------|-------------|
| `Button` | Interactive button with icon/shortcut support |
| `DragSource` | Wrapper that initiates generic drag-and-drop |
| `DropTarget` | Wrapper that receives generic drag-and-drop payloads |
| `Hyperlink` | Clickable text link with keyboard activation |
| `Input` | Single-line text field |
| `TextArea` | Multi-line editor; Vim modal editing, syntax highlighting, images, custom inline sentinels (payloads, ids, snapshots) |
| `DiffView` | Read-only diff viewer (feature: `diff-view`) |
| `Checkbox` | Toggle with indeterminate state |
| `Radio` | Radio button group |
| `Select` | Dropdown selector |
| `ComboBox` | Input + filtered dropdown selector |
| `MultiSelect` | List with multi-row selection via space toggle |
| `HexArea` | Hex/ASCII binary data viewer with cursor navigation |
| `Slider` | Numeric slider with gradient support |
| `DatePicker` | Calendar date picker |

## Data → [data.md](data.md)

| Widget | Description |
|--------|-------------|
| `List` | Selectable list with headers, spacers, prefixes, scrollbar |
| `Table` | Structured table with heatmaps, inspector presets |
| `Tree` | Hierarchical tree with expand/collapse |
| `FileTree` | Lazy-loading filesystem explorer with git/provided changes, changed-only view, and diff stats |
| `LogView` | High-throughput log stream with fuzzy filtering |

## Feedback & Status → [feedback.md](feedback.md)

| Widget | Description |
|--------|-------------|
| `ProgressBar` | Progress indicator with zones, gradients, drag |
| `Spinner` | Animated loading indicator |
| `StatusBar` | Application status line (left/center/right slots) |
| `PaginationBar` | Composed pagination controls with styled nav buttons |
| `Breadcrumb` | Navigation trail |
| `Badge` | Status indicator overlaid on an element |

## Overlays & Navigation → [overlays.md](overlays.md)

| Widget | Description |
|--------|-------------|
| `Modal` | Centered dialog (portals to root level) |
| `Toast` | Transient notifications via `ctx.toast()` |
| `Popover` | Floating content panel |
| `Tooltip` | Help text on hover/focus |
| `Accordion` | Collapsible sections |
| `CommandPalette` | Modal command palette backed by the runtime command registry |
| `SearchPalette` | Fuzzy search overlay with optional grouped entries and hidden aliases |
| `ContextMenu` | Right-click menu |

## Tabs → [tabs.md](tabs.md)

| Widget | Description |
|--------|-------------|
| `Tabs` | Simple tab bar |
| `DraggableTabBar` | Editor-style tabs with drag, close, file icons, and shrink-then-scroll overflow |

## Terminal → [terminal.md](terminal.md) *(feature: `terminal`)*

| Widget | Description |
|--------|-------------|
| `ManagedTerminal` | Full PTY terminal (recommended) |
| `Terminal` | Low-level terminal viewport widget |
| `TerminalPty` | PTY spawner and I/O bridge |
| `TerminalScreen` | VT100/VT220 screen emulator |

Design (not implemented): [terminal-images.md](terminal-images.md) — Kitty/sixel image passthrough.
