# Event & Callback Reference

Every widget callback has a specific payload type. This document lists all callback signatures
and event struct fields so you never need to read source code to wire up handlers.

---

## Core Types

### `Callback<T>`

All event handlers use `Callback<T>`. Create them via `ctx.link().callback(...)`:

```rust
// Map event to message:
ctx.link().callback(|e: ListEvent| Msg::Select(e.index))

// Ignore event payload:
ctx.link().callback(|_| Msg::DoSomething)

// Direct message constructor (when Msg variant takes the same type):
ctx.link().callback(Msg::TextChanged)  // Msg::TextChanged(String)
```

### `KeyHandler`

Used for `on_key` props. Created via `ctx.link().key_handler(...)`:

```rust
ctx.link().key_handler(|key: KeyEvent| {
    match key.code {
        KeyCode::Enter => Some(Msg::Submit),
        _ => None,  // None = unhandled, key bubbles
    }
})
```

---

## Event Structs

### `ListEvent`

Emitted by: `List::on_select`, `List::on_activate`, `List::on_item_click`

```rust
pub struct ListEvent {
    pub index: usize,   // Selected/activated item index
}
```

### `CheckboxEvent`

Emitted by: `Checkbox::on_toggle`

```rust
pub struct CheckboxEvent {
    pub state: CheckboxState,  // New state after toggle
}
```

### `TabsEvent`

Emitted by: `Tabs::on_change`, `VStack::on_tab_change`

```rust
pub struct TabsEvent {
    pub index: usize,  // Newly active tab index
}
```

### `ScrollEvent`

Emitted by: `ScrollView::on_scroll` (including reconcile-time sync when the controlled offset differs), `DocumentView::on_scroll`, `TextArea::on_scroll`

```rust
pub struct ScrollEvent {
    pub offset: usize,           // New scroll offset (row index)
    pub metrics: ScrollMetrics,   // Viewport metrics
}

pub struct ScrollMetrics {
    pub len: usize,       // Total scrollable rows
    pub visible: usize,   // Number of visible rows
    pub max_offset: usize, // Maximum scroll offset
}
```

### `ScrollViewportEvent`

Emitted by: `ScrollView::on_viewport_change` after layout/reconcile when the
visible immediate-child snapshot changes. This includes scrolling, resize,
wrapping, content changes, and child insertion/removal.

```rust
pub struct ScrollViewportEvent {
    pub offset: usize,
    pub metrics: ScrollMetrics,
    pub viewport_width: u16,
    pub children_len: usize,
    pub first_visible_index: Option<usize>,
    pub last_visible_index: Option<usize>,
    pub visible: Vec<ScrollVisibleChild>,
    pub entered: Vec<ScrollVisibleChild>,
    pub exited: Vec<ScrollExitedChild>,
    pub top_indicator: bool,
    pub bottom_indicator: bool,
    pub bottom_count: usize,
}

pub struct ScrollVisibleChild {
    pub index: usize,
    pub key: Option<Key>,
    pub content_rect: Rect,
    pub viewport_rect: Rect,
    pub visible_rect: Rect,
    pub visible_height: u16,
    pub clipped_above: u16,
    pub clipped_below: u16,
    pub visibility: ScrollChildVisibility,
}

pub struct ScrollExitedChild {
    pub child: ScrollVisibleChild, // Last visible snapshot
    pub direction: ScrollChildExitDirection,
}

pub enum ScrollChildVisibility {
    FullyVisible,
    PartiallyVisible,
}

pub enum ScrollChildExitDirection {
    Above,
    Below,
    Removed,
}
```

The API tracks immediate `ScrollView` children, not arbitrary descendants. Put
stable keys on immediate row children when you need reliable `entered` / `exited`
diffs across insertion and removal. `entered` and `exited` are identity-based:
children that stay visible but change from full to partial visibility, or vice
versa, remain in `visible` rather than appearing in either diff list.
`content_rect` is relative to scroll content before offset; `viewport_rect` is
relative to the effective child viewport after offset and indicator rows;
`visible_rect` is the clipped visible portion. These rectangles use tui-lipan's
framework `Rect`, not ratatui types.

### `PanEvent`

```rust
pub struct PanEvent {
    pub x: i32,
    pub y: i32,
    pub metrics: PanMetrics,
}

pub struct PanMetrics {
    pub content_w: u16,
    pub content_h: u16,
    pub viewport_w: u16,
    pub viewport_h: u16,
    pub max_x: i32,
    pub max_y: i32,
}
```

### `DiffScrollEvent` *(feature `diff-view`)*

Emitted by: `DiffView::on_scroll`

```rust
pub struct DiffScrollEvent {
    pub pane: DiffPane,      // Which pane emitted the event
    pub scroll: ScrollEvent, // Underlying scroll payload
}

pub enum DiffPane {
    Left,
    Right,
    Unified,
}
```

### `DiffContextSeparatorEvent` *(feature `diff-view`)*

Emitted by: `DiffView::on_context_separator_click`

```rust
pub struct DiffContextSeparatorEvent {
    pub pane: DiffPane,
    pub range: DiffContextRange,
    pub hidden_lines: usize,
    pub direction: DiffContextSeparatorDirection,
    pub expand_lines: usize,
}

impl DiffContextSeparatorEvent {
    pub fn next_expansion(&self, current: Option<&DiffContextExpansion>) -> DiffContextExpansion;
    pub fn next_expansion_by(
        &self,
        current: Option<&DiffContextExpansion>,
        step: usize,
    ) -> DiffContextExpansion;
}

pub struct DiffContextExpansion {
    pub range: DiffContextRange,
    pub lines_revealed: usize,
}

pub struct DiffContextRange {
    pub old_start: Option<usize>, // 1-based, inclusive
    pub old_end: Option<usize>,   // 1-based, inclusive
    pub new_start: Option<usize>, // 1-based, inclusive
    pub new_end: Option<usize>,   // 1-based, inclusive
}
```

### `MouseEvent`

Emitted by: `Button::on_click`, `List::on_click`, `MouseRegion::on_click`

```rust
pub struct MouseEvent {
    pub x: u16,            // Terminal-space X
    pub y: u16,            // Terminal-space Y
    pub kind: MouseKind,   // Down, Up, Drag, ScrollUp, ScrollDown, ...
    pub mods: KeyMods,     // Modifier keys held
}
```

### `GraphNodeEvent`

Emitted by: `Graph::on_node_click`, `Graph::on_node_hover`,
`Graph::on_node_focus`, `Graph::on_node_activate`

```rust
pub struct GraphNodeEvent {
    pub path: GraphNodePath, // Child-index path from root to node
    pub label: Arc<str>,     // Node label
}

pub struct GraphNodePath(...);

impl GraphNodePath {
    pub fn root() -> Self;
    pub fn from_segments(segments: impl IntoIterator<Item = usize>) -> Self;
    pub fn segments(&self) -> &[usize];
}
```

### `SequenceItemEvent`

Emitted by: `SequenceDiagram::on_item_click`, `SequenceDiagram::on_item_hover`

```rust
pub struct SequenceItemEvent {
    pub path: SequenceItemPath, // Which rendered diagram item was targeted
    pub label: Arc<str>,        // Display label for the targeted item
}

pub enum SequenceItemPath {
    Message(usize),     // Index into the rendered message list
    SelfMessage(usize), // Index into the same rendered message list
    Participant(usize), // Index into rendered participants
    Note(usize),        // Index into rendered notes
    Fragment(usize),    // Index into rendered fragments
    Divider(usize),     // Index into rendered dividers
}
```

Use `SequenceItemPath` when you need stable item-type dispatch in your update
logic. `Message` and `SelfMessage` share the rendered message-list index;
other variants index into their corresponding rendered item lists. Parser- or
app-owned IDs should be tracked separately when diagrams are rebuilt from
external data.

### Flowchart events

Emitted by: `Flowchart::on_node_click`, `Flowchart::on_edge_click`,
`Flowchart::on_subgraph_click` and their `*_hover` siblings.

```rust
pub struct FlowchartNodeEvent {
    pub id: NodeId,
    pub label: Arc<str>,
}

pub struct FlowchartEdgeEvent {
    pub from: NodeId,
    pub to: NodeId,
    pub label: Option<Arc<str>>,
}

pub struct FlowchartSubgraphEvent {
    pub id: NodeId,
    pub label: Arc<str>,
}

pub enum FlowchartItemPath {
    Node(NodeId),
    Edge(usize),
    Subgraph(NodeId),
}
```

### Drag And Drop Events

Emitted by: `DragSource`, `DropTarget`

```rust
pub struct DragOverEvent {
    pub x: u16,            // Terminal-space X
    pub y: u16,            // Terminal-space Y
    pub local_y: u16,      // Y relative to the drop target top edge
    pub local_height: u16, // Drop target height in cells
    pub payload: Arc<dyn DragPayload>,
}

pub struct DropEvent {
    pub x: u16,            // Terminal-space X
    pub y: u16,            // Terminal-space Y
    pub local_y: u16,      // Y relative to the drop target top edge
    pub local_height: u16, // Drop target height in cells
    pub payload: Arc<dyn DragPayload>,
}
```

### `HyperlinkEvent`

Emitted by: `Hyperlink::on_activate`

```rust
pub struct HyperlinkEvent {
    pub label: Arc<str>,         // Link label
    pub href: Option<Arc<str>>,  // Optional destination URL
}
```

### `MouseMoveEvent`

Emitted by: `MouseRegion::on_mouse_move`

```rust
pub struct MouseMoveEvent {
    pub x: u16,          // Terminal-space X
    pub y: u16,          // Terminal-space Y
    pub local_x: u16,    // Relative to MouseRegion rect
    pub local_y: u16,    // Relative to MouseRegion rect
    pub target_w: u16,   // MouseRegion width
    pub target_h: u16,   // MouseRegion height
    pub mods: KeyMods,   // Modifier keys held
}
```

### `MouseDragEvent`

Emitted by: `MouseRegion::on_drag_start`, `MouseRegion::on_drag`,
`MouseRegion::on_drag_end`, `MouseRegion::on_right_drag_start`,
`MouseRegion::on_right_drag`, `MouseRegion::on_right_drag_end`

```rust
pub struct MouseDragEvent {
    pub from_x: u16,       // Terminal-space drag origin X
    pub from_y: u16,       // Terminal-space drag origin Y
    pub from_local_x: u16, // Origin X relative to MouseRegion rect
    pub from_local_y: u16, // Origin Y relative to MouseRegion rect
    pub x: u16,            // Current terminal-space X
    pub y: u16,            // Current terminal-space Y
    pub local_x: u16,      // Current X relative to MouseRegion rect
    pub local_y: u16,      // Current Y relative to MouseRegion rect
    pub delta_x: i16,      // X delta since previous drag tick
    pub delta_y: i16,      // Y delta since previous drag tick
    pub target_w: u16,     // MouseRegion width
    pub target_h: u16,     // MouseRegion height
    pub mods: KeyMods,     // Modifier keys held
}
```

### `KeyEvent`

Emitted by: `Component::on_key`, `KeyHandler`

```rust
pub struct KeyEvent {
    pub code: KeyCode,  // Char('a'), Enter, Esc, Tab, F(1), Up, Down, ...
    pub mods: KeyMods,  // Modifier flags
}

pub struct KeyMods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}
```

Common pitfall: matching only `key.code` ignores modifiers. For example, `Ctrl+S` and `S`
share the same `code`, so prefer key helpers that check both code and modifier state.

```rust
ctx.link().key_handler(|key: KeyEvent| {
    if key.is(KeyCode::Enter) {
        Some(Msg::Submit)
    } else if key.is_with(KeyCode::Char('s'), KeyMods::CTRL) {
        Some(Msg::Save)
    } else {
        None
    }
})
```

### `TextAreaEvent`

Emitted by: `TextArea::on_change`

```rust
pub struct TextAreaEvent {
    pub value: Arc<str>,
    pub cursor: usize,
    pub anchor: Option<usize>,
}
```

Use `TextArea::bound(&TextEditor)` (recommended) or `TextArea::new("").bind(&TextEditor)` and
`TextAreaEvent::apply_to(&mut TextEditor)` to keep value, cursor, and selection in sync across
rerenders. `bound` is a shorthand that creates and binds in one call.
`cursor` and `anchor` are byte offsets; use `LineIndex` when a callback needs
line/column coordinates for the same text snapshot.

### `TextAreaVimMode`

Emitted by: `TextArea::on_vim_mode_change`

```rust
pub enum TextAreaVimMode {
    Insert,
    Normal,
    Visual,
    VisualLine,
}
```

Opt-in Vim-enabled TextAreas start in Normal mode. The callback fires when the
TextArea changes between Insert, Normal, Visual, and VisualLine modes. Use it for
app-owned status labels, frame titles, or focus styling; it is not an edit event
and does not replace `TextArea::on_change`. Commands such as `i`, `a`, `I`, `A`,
`o`, `O`, `c{motion}`, Visual `c`, `Esc`, and Visual operators can all trigger
mode-change callbacks. Mouse-created selections in Vim-enabled TextAreas
(double/triple click or drag) also enter `Visual` mode and can emit this callback.

### `InputEvent`

Emitted by: `Input::on_change`

```rust
pub struct InputEvent {
    pub value: Arc<str>,
    pub cursor: usize,
    pub anchor: Option<usize>,
}
```

Use `Input::bound(&TextInput)` (recommended) or `Input::new("").bind(&TextInput)` and
`InputEvent::apply_to(&mut TextInput)` to keep value, cursor, and selection in sync across
rerenders. `bound` is a shorthand that creates and binds in one call.

### `SentinelEvent`

Emitted by: `TextArea::on_sentinel_event` (vector batch)

```rust
pub enum SentinelEvent {
    Deleted {
        id: SentinelId,
        sentinel: TextAreaSentinel,
    },
}
```

### `TextAreaSentinelClickEvent`

Emitted by: `TextArea::on_sentinel_click`

```rust
pub struct TextAreaSentinelClickEvent {
    pub kind: TextAreaSentinelClickKind,
    pub byte_range: (usize, usize),
    pub mouse: MouseEvent,
}

pub enum TextAreaSentinelClickKind {
    Image { index: usize, image: ImageContent },
    Custom { index: usize, id: SentinelId, sentinel: TextAreaSentinel },
}
```

The callback fires for inline image placeholders and custom sentinel labels. Custom sentinel payloads are available through `sentinel.get_payload::<T>()`.

### `TextEditEvent`

Emitted by: `Input::on_edit`, `TextArea::on_edit`

```rust
pub struct TextEditEvent {
    pub value: String,         // Full text value after edit
    pub kind: TextEditKind,    // Type of edit
}

pub enum TextEditKind {
    Insert,
    Delete,
    Replace,
    Paste,
    Cut,
    Undo,
    Redo,
}
```

Text-widget clear actions use the same callback path as other edits. For
`TextArea`, clear emits `TextAreaEvent` via `on_change` and a `TextEditEvent`
with `TextEditKind::Replace` via `on_edit`.

### `ComboBoxCommitEvent`

Emitted by: `ComboBox::on_commit`

```rust
pub struct ComboBoxCommitEvent {
    pub index: Option<usize>,   // Source index (None if custom value)
    pub value: Arc<str>,         // Committed text
    pub from_custom_value: bool, // true if free-form input
}
```

### `MultiSelectToggleEvent`

Emitted by: `MultiSelect::on_toggle`

```rust
pub struct MultiSelectToggleEvent {
    pub index: usize,     // Toggled row index
    pub selected: bool,   // New selection state
}
```

### `MultiSelectChangeEvent`

Emitted by: `MultiSelect::on_change`

```rust
pub struct MultiSelectChangeEvent {
    pub selected_indices: Vec<usize>,
}
```

### `MultiSelectCommitEvent`

Emitted by: `MultiSelect::on_commit`

```rust
pub struct MultiSelectCommitEvent {
    pub selected_indices: Vec<usize>,
}
```

### `HexAreaCursorEvent`

Emitted by: `HexArea::on_cursor_change`

```rust
pub struct HexAreaCursorEvent {
    pub cursor: usize,
    pub anchor: Option<usize>,
}
```

### `HexAreaChangeEvent`

Emitted by: `HexArea::on_change`

```rust
pub struct HexAreaChangeEvent {
    pub bytes: Arc<[u8]>,
}
```

### `HexAreaEditEvent`

Emitted by: `HexArea::on_edit`

```rust
pub struct HexAreaEditEvent {
    pub index: usize,
    pub before: Option<u8>,
    pub after: Option<u8>,
    pub kind: HexAreaEditKind,
}
```

### `DraggableTabCloseEvent`

Emitted by: `DraggableTabBar::on_close`

```rust
pub struct DraggableTabCloseEvent {
    pub index: usize,  // Closed tab index
}
```

### `DraggableTabActionEvent`

Emitted by: `DraggableTabBar::on_action`

```rust
pub struct DraggableTabActionEvent {
    pub index: usize,  // Action tab index
}
```

### `DraggableTabReorderEvent`

Emitted by: `DraggableTabBar::on_reorder`

```rust
pub struct DraggableTabReorderEvent {
    pub from: usize,  // Source tab index
    pub to: usize,    // Destination tab index
}
```

### `DraggableTabTransferEvent`

Emitted by: `DraggableTabBar::on_transfer`

```rust
pub struct DraggableTabTransferEvent {
    pub from_bar: Arc<str>,
    pub to_bar: Arc<str>,
    pub from: usize,
    pub to: usize,
}
```

---

## Widget Callback Summary

Quick lookup - which callbacks does each widget support?

### Button

| Callback | Payload | When |
|----------|---------|------|
| `on_click` | `MouseEvent` | Mouse click, or plain `Enter` / `Space` on a focused button |
| `on_key` | `KeyHandler` | Any key while focused; runs before default activation and can consume the key |

### Hyperlink

| Callback | Payload | When |
|----------|---------|------|
| `on_activate` | `HyperlinkEvent` | Click, Enter, or Space |
| `on_key` | `KeyHandler` | Non-activation keys while focused |

### Input

| Callback | Payload | When |
|----------|---------|------|
| `on_change` | `String` | Every keystroke (new full value) |
| `on_edit` | `TextEditEvent` | Structured edit events |

### TextArea

| Callback | Payload | When |
|----------|---------|------|
| `on_change` | `TextAreaEvent` | Every edit (value, cursor, anchor) |
| `on_edit` | `TextEditEvent` | Structured edit events |
| `on_vim_mode_change` | `TextAreaVimMode` | Insert/Normal/Visual/VisualLine transition when `vim_motions(true)` is enabled |
| `on_scroll` | `ScrollEvent` | Scroll offset changes |
| `on_scroll_to` | `usize` | Scrollbar interaction |
| `on_sentinels_change` | `Vec<TextAreaSentinel>` | Custom sentinel list after prune (e.g. token deleted) |
| `on_sentinel_event` | `Vec<SentinelEvent>` | Lifecycle events (e.g. `Deleted` with stable id + payload) |
| `on_sentinel_click` | `TextAreaSentinelClickEvent` | Inline image or custom sentinel placeholder clicked |
| `on_images_change` | `Vec<ImageContent>` | Image list updated |
| `on_image_paste` | `ImageContent` | Legacy: single image pasted |

### List

| Callback | Payload | When |
|----------|---------|------|
| `on_select` | `ListEvent` | Selection changes (keyboard or mouse) |
| `on_activate` | `ListEvent` | Enter key (or click if `activate_on_click` is true) |
| `on_item_click` | `ListEvent` | Row clicked with mouse |
| `on_click` | `MouseEvent` | Raw mouse click on list area |
| `on_scroll_to` | `usize` | Scrollbar interaction |
| `on_key` | `KeyHandler` | Key while focused |

### Checkbox

| Callback | Payload | When |
|----------|---------|------|
| `on_toggle` | `CheckboxEvent` | State toggled (keyboard or mouse) |
| `on_click` | `MouseEvent` | Raw mouse click |
| `on_key` | `KeyHandler` | Key while focused |

### Radio

| Callback | Payload | When |
|----------|---------|------|
| `on_change` | `usize` | Selection changed (new index) |

### Select

| Callback | Payload | When |
|----------|---------|------|
| `on_select` | `usize` | Item selected |
| `on_change` | `usize` | Selection changed |
| `on_toggle` | `bool` | Dropdown opened/closed |

### ComboBox

| Callback | Payload | When |
|----------|---------|------|
| `on_query_change` | `Arc<str>` | Input text changed |
| `on_open_change` | `bool` | Dropdown open/close requested |
| `on_active_index_change` | `Option<usize>` | Active row changed |
| `on_commit` | `ComboBoxCommitEvent` | Enter/activate commit |

### MultiSelect

| Callback | Payload | When |
|----------|---------|------|
| `on_active_index_change` | `usize` | Active row changed |
| `on_toggle` | `MultiSelectToggleEvent` | Row toggled |
| `on_change` | `MultiSelectChangeEvent` | Selected set changed |
| `on_commit` | `MultiSelectCommitEvent` | Enter pressed |

### Slider

| Callback | Payload | When |
|----------|---------|------|
| `on_change` | `f64` | Value changed (drag or keyboard) |
| `on_click` | `f64` | Click or Enter |

### DatePicker

| Callback | Payload | When |
|----------|---------|------|
| `on_select` | `(i32, u32, u32)` | Day selected (year, month, day) |
| `on_prev_month` | `()` | Navigate to previous month |
| `on_next_month` | `()` | Navigate to next month |

### Tabs

| Callback | Payload | When |
|----------|---------|------|
| `on_change` | `TabsEvent` | Active tab changed |

### DraggableTabBar

| Callback | Payload | When |
|----------|---------|------|
| `on_change` | `TabsEvent` | Active tab changed |
| `on_action` | `DraggableTabActionEvent` | Action tab clicked |
| `on_close` | `DraggableTabCloseEvent` | Tab close button clicked |
| `on_reorder` | `DraggableTabReorderEvent` | Tab dragged to new position |
| `on_transfer` | `DraggableTabTransferEvent` | Tab transferred to another bar |
| `on_click` | `MouseEvent` | Raw mouse click on tab bar |

### ScrollView

| Callback | Payload | When |
|----------|---------|------|
| `on_scroll` | `ScrollEvent` | User input / scrollbar; also after reconcile when a controlled `offset` differs from the laid-out offset (sync) |
| `on_scroll_to` | `usize` | Scrollbar interaction |
| `on_viewport_change` | `ScrollViewportEvent` | Visible immediate-child snapshot changes after layout/reconcile |

### DiffView *(feature `diff-view`)*

| Callback | Payload | When |
|----------|---------|------|
| `on_scroll` | `DiffScrollEvent` | A rendered diff pane scrolls |
| `on_context_separator_click` | `DiffContextSeparatorEvent` | A visible collapsed-context separator line is clicked |

### Graph

| Callback | Payload | When |
|----------|---------|------|
| `on_node_click` | `GraphNodeEvent` | Node box clicked with the mouse |
| `on_node_hover` | `GraphNodeEvent` | Hover moves to a different node |
| `on_node_focus` | `GraphNodeEvent` | Keyboard navigation moves internal node focus |
| `on_node_activate` | `GraphNodeEvent` | Enter or Space activates the internally focused node |

`on_node_focus` and `on_node_activate` imply keyboard focusability for the graph;
`on_node_click` and `on_node_hover` are pointer-only and do not.

### MouseRegion

| Callback | Payload | When |
|----------|---------|------|
| `on_click` | `MouseEvent` | Left-button click |
| `on_mouse_move` | `MouseMoveEvent` | Pointer movement |
| `on_drag_start` | `MouseDragEvent` | First left-button drag tick after threshold |
| `on_drag` | `MouseDragEvent` | Each left-button drag tick after drag start |
| `on_drag_end` | `MouseDragEvent` | Left-button release after drag start |
| `on_right_drag_start` | `MouseDragEvent` | First right-button drag tick after threshold |
| `on_right_drag` | `MouseDragEvent` | Each right-button drag tick after drag start |
| `on_right_drag_end` | `MouseDragEvent` | Right-button release after drag start |

Use `drag_requires_mods(...)` and `right_drag_requires_mods(...)` to require
modifiers before drag callbacks can start. Use `bubble_mouse_down(true)` when an
ancestor should receive `on_mouse_down` for descendant presses without consuming
the descendant's click. Use `capture_requires_mods(...)` when modifier-owned
gestures must be fully consumed before a terminal/input descendant can select text
or receive a terminal mouse report.

### HexArea

| Callback | Payload | When |
|----------|---------|------|
| `on_cursor_change` | `HexAreaCursorEvent` | Cursor movement |
| `on_change` | `HexAreaChangeEvent` | Bytes modified |
| `on_edit` | `HexAreaEditEvent` | Per-edit metadata |
| `on_scroll` | `ScrollEvent` | Navigation/wheel scroll |
| `on_key` | `KeyHandler` | Custom key handler |

---

## `Element::empty()`

Use in conditional branches where no widget should be rendered:

```rust
if condition {
    some_widget.into()
} else {
    Element::empty()
}
```
## TextArea editor state changes

`TextArea::on_editor_state_change` emits `TextAreaStateChangeEvent` with a
single primary reason (`Edit`, `SelectionChange`, `CursorMove`,
`VimModeChange`, or `Scroll`). The payload carries the canonical byte cursor and
anchor, the current value, an optional `TextEditEvent` when the existing edit
path produced one, and an optional Vim mode when a mode transition occurred.
