# Input Widgets

State-style setters in this file use [StyleSlot semantics](../styling.md#state-style-slots):
`hover_style`, `focus_style`, `selection_style`, and prefixed variants replace
theme roles by default; matching `extend_*_style` setters patch over the scoped
theme role and `inherit_*_style` setters delegate to it.

## Button

Interactive button.

| Prop | Type | Description |
|------|------|-------------|
| `label` | `impl Into<String>` | **Constructor** - button text |
| `variant` | `ButtonVariant` | Visual variant |
| `style` | `Style` | Idle style |
| `hover_style` | `Style` | Hover style |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `focus_style` | `Style` | Focus style |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `border_style` | `BorderStyle` | Idle border |
| `hover_border_style` | `BorderStyle` | Hover border |
| `focus_border_style` | `BorderStyle` | Focus border |
| `align` | `Align` | Label alignment |
| `padding` | `impl Into<Padding>` | Inner padding |
| `icon` | `Span` | Icon span (prepended to label) |
| `icon_style` | `Style` | Icon style |
| `icon_gap` | `u16` | Space between icon and label |
| `shortcut` | `String` | Keyboard shortcut label |
| `shortcut_bindings` | `KeyBindings` | Keyboard shortcut alternatives (displayed canonically) |
| `shortcut_style` | `Style` | Shortcut style |
| `shortcut_gap` | `u16` | Space between label and shortcut |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `focusable` | `bool` | Whether button accepts focus |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `disabled` | `bool` | Disable button |
| `disabled_style` | `Style` | Style when disabled |
| `on_click` | `Callback<MouseEvent>` | Mouse click, plain `Enter`, or plain `Space` activation callback |
| `on_key` | `KeyHandler` | Key handler that can intercept focused keys before activation |

Focused buttons invoke `on_click` for mouse activation and for unmodified
`Enter` / `Space`. A custom `on_key` handler runs first; return `true` there to
consume the key without also firing `on_click`.

```rust
Button::new("Save")
    .style(Style::new().fg(Color::White).bg(Color::Blue))
    .shortcut_bindings("ctrl+s, super+s".parse().unwrap())
    .focus_style(Style::new().fg(Color::White).bg(Color::DarkBlue).bold())
    .on_click(ctx.link().callback(|_| Msg::Save))
```

---

## DragSource

Wrapper that turns a single child into a generic drag source.

| Prop | Type | Description |
|------|------|-------------|
| `child` | `impl Into<Element>` | Wrapped draggable content |
| `on_drag_start` | `Fn(DragStartEvent) -> Option<Box<dyn DragPayload>>` | Drag-start handler that returns payload; `None` aborts drag |
| `on_drag_cancel` | `Callback<DragCancelEvent>` | Fired when drag is canceled or dropped on invalid target |
| `on_drag_started` | `Callback<DragStartedEvent>` | Fired once when the drag activates (after the movement threshold); includes `payload` |
| `drag_group` | `impl Into<Arc<str>>` | Optional compatibility group |
| `clear_drag_group` | `()` | Remove group restriction |
| `preview` | `DragPreview` | `Label` text near pointer, `SourceSnapshot` (layout slot collapses per `drag_slot`; float preview copies cells), or `None` |
| `preview_label` | `impl Into<Arc<str>>` | Convenience for `DragPreview::Label` |
| `preview_snapshot` | `()` | Convenience for `DragPreview::SourceSnapshot` |
| `no_preview` | `()` | Convenience for `DragPreview::None` |
| `drag_slot` | `DragSlot` | `Collapse` (0 main-axis cells) or `Specified(Length)` using the same `Length` rules as stack children (`Auto`, `Px`, `Percent`, `Flex`). In `VStack`/`HStack` the stack axis applies; elsewhere use `drag_slot_axis`. |
| `drag_slot_collapse` | `()` | Same as `drag_slot(DragSlot::Collapse)` |
| `drag_slot_length` | `Length` | Same as `drag_slot(DragSlot::Specified(len))` |
| `drag_slot_axis` | `DragSlotAxis` | `Vertical` or `Horizontal`: which axis `Fixed`/`Collapse` apply to when measured outside a stack (default: vertical) |
| `dragging_style` | `Style` | Overlay while dragging: first-frame tint, reserved slot fill when using `SourceSnapshot`, and label/none preview modes |
| `extend_dragging_style` / `inherit_dragging_style` | `Style` / `()` | Extend or inherit the drag-source theme role for the dragging overlay |
| `preview_max_width` | `Option<u16>` | Max width of the floating `SourceSnapshot` preview (`None` → `DEFAULT_PREVIEW_MAX_WIDTH`) |
| `preview_max_height` | `Option<u16>` | Max height of the floating preview (`None` → `DEFAULT_PREVIEW_MAX_HEIGHT`) |
| `preview_max_size` | `(Option<u16>, Option<u16>)` | Set both max dimensions |
| `threshold` | `u16` | Pointer movement threshold before drag starts (default: `3`) |
| `enabled` | `bool` | Enable/disable drag behavior |

```rust
DragSource::new()
    .child(Text::new("main.rs"))
    .on_drag_start(|ev| {
        let _ = (ev.x, ev.y);
        Some(Box::new(String::from("main.rs")) as Box<dyn DragPayload>)
    })
    .preview_label("main.rs")
    .threshold(3)
```

---

## DropTarget

Wrapper that marks a single child as a generic drop zone.

| Prop | Type | Description |
|------|------|-------------|
| `child` | `impl Into<Element>` | Wrapped drop-zone content |
| `on_drag_over` | `Callback<DragOverEvent>` | Fired when compatible payload hovers target |
| `on_drag_leave` | `Callback<DragLeaveEvent>` | Fired when active drag leaves target |
| `on_drop` | `Callback<DropEvent>` | Fired on successful drop |
| `accept_group` | `impl Into<Arc<str>>` | Restrict accepted source group |
| `clear_accept_group` | `()` | Accept all groups (`None`) |
| `can_accept` | `PayloadAcceptFn` | Optional payload predicate |
| `can_accept_with` | `Fn(&dyn DragPayload) -> bool` | Closure form of payload predicate |
| `clear_can_accept` | `()` | Remove payload predicate |
| `highlight` | `DropHighlight` | `None`, `Fill`, `Placeholder` (bordered frame), or `Overlay` (tint after children) |
| `highlight_style` | `Style` | Style for `Fill` / `Placeholder` / `Overlay` |
| `extend_highlight_style` / `inherit_highlight_style` | `Style` / `()` | Extend or inherit the active drop-target theme role for the drop highlight |
| `highlight_fill` | `Style` | `highlight(DropHighlight::Fill)` + `highlight_style` |
| `highlight_placeholder` | `Style` | `highlight(DropHighlight::Placeholder)` + `highlight_style` |
| `highlight_overlay` | `Style` | `highlight(DropHighlight::Overlay)` + `highlight_style` |
| `drop_slot` | `DropSlot` | `Child` (render child normally, default) or `SourcePreview` (replace child with the dragged card's snapshot; cursor float is suppressed) |
| `drop_slot_source_preview` | `()` | Shorthand for `drop_slot(DropSlot::SourcePreview)` |
| `enabled` | `bool` | Enable/disable drop behavior |

`on_drag_over` is emitted on **every** pointer move while a compatible drag hovers this target (not only on enter). Use `DragOverEvent::local_y` (offset from the drop target’s top edge) with a row-height estimate to place a single insertion line. `DropEvent::local_y` uses the same convention.

```rust
DropTarget::new()
    .child(Text::new("Drop here"))
    .accept_group("files")
    .can_accept_with(|payload| payload.downcast_ref::<String>().is_some())
    .on_drop(ctx.link().callback(|ev| Msg::Dropped(ev.payload)))
```

**Tip — per-item targets instead of stride math:** for sortable lists, wrap **each item** in its own `DropTarget` and map the pointer's top/bottom half (`local_y * 2 < local_height`) to insert-before/insert-after. Combined with a constant-height indicator row per item (restyled, never inserted), the layout never shifts while hovering, so there is no hover flicker. See `examples/sidebar_tabs.rs` for the full pattern; `examples/drag_drop_kanban.rs` shows the single-target-per-column alternative.

**Examples:** `examples/sidebar_tabs.rs`, `examples/drag_drop_kanban.rs`

---

## MouseRegion

Wrapper that adds pointer callbacks to an arbitrary child subtree.

| Prop | Type | Description |
|------|------|-------------|
| `child` | `impl Into<Element>` | Wrapped content |
| `on_click` | `Callback<MouseEvent>` | Left-button click after press and release on the same region |
| `on_mouse_down` | `Callback<MouseEvent>` | Left-button press |
| `on_mouse_up` | `Callback<MouseEvent>` | Left-button release over the region |
| `on_mouse_move` | `Callback<MouseMoveEvent>` | Pointer movement over the region |
| `on_drag_start` | `Callback<MouseDragEvent>` | First left-button drag tick after the click threshold is exceeded |
| `on_drag` | `Callback<MouseDragEvent>` | Every left-button drag tick after drag start |
| `on_drag_end` | `Callback<MouseDragEvent>` | Left-button release after a drag started |
| `drag_requires_mods` | `KeyMods` | Require modifiers before left-button drag callbacks can start |
| `on_right_drag_start` | `Callback<MouseDragEvent>` | First right-button drag tick after the click threshold is exceeded |
| `on_right_drag` | `Callback<MouseDragEvent>` | Every right-button drag tick after drag start |
| `on_right_drag_end` | `Callback<MouseDragEvent>` | Right-button release after a drag started |
| `right_drag_requires_mods` | `KeyMods` | Require modifiers before right-button drag callbacks can start |
| `bubble_mouse_down` | `bool` | Also emit `on_mouse_down` for descendant presses without consuming them |
| `capture_click` | `bool` | Capture clicks over interactive descendants |
| `capture_requires_mods` | `KeyMods` | Capture pointer handling over descendants while modifiers are held |
| `hover_style` | `Style` | Style underlay while hovered |
| `enabled` | `bool` | Enable/disable pointer behavior |

Drag callbacks use the same click-cancel threshold as built-in draggable widgets,
so a single click does not emit drag-start, drag, or drag-end callbacks.
`MouseDragEvent` includes the global and local drag origin, current global and
local coordinates, delta since the previous drag tick, target size, and
modifiers. Modifier requirements are subset checks: `KeyMods::ALT` means Alt must
be held, while unrelated extra modifiers are allowed.
Use `capture_requires_mods(KeyMods::ALT)` with modifier-gated wrapper gestures
around terminals or text widgets so Alt-click/Alt-drag does not start child
selection or forward a terminal mouse report.

```rust
MouseRegion::new()
    .on_drag_start(ctx.link().callback(|ev: MouseDragEvent| Msg::Begin(ev.local_x, ev.local_y)))
    .on_drag(ctx.link().callback(|ev: MouseDragEvent| Msg::Draw(ev.local_x, ev.local_y)))
    .on_drag_end(ctx.link().callback(|_| Msg::End))
    .child(AsciiCanvas::blank(40, 12))
```

---

## Hyperlink

Clickable text link built on top of `Button` with link-style defaults.

| Prop | Type | Description |
|------|------|-------------|
| `label` | `impl Into<Arc<str>>` | **Constructor** - visible link text |
| `href` | `Arc<str>` | Optional URL metadata emitted in callbacks |
| `style` | `Style` | Idle style |
| `hover_style` | `Style` | Hover style |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `focus_style` | `Style` | Focus style |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `disabled_style` | `Style` | Style when disabled |
| `visited_style` | `Style` | Style overlay when `visited: true` |
| `align` | `Align` | Label alignment |
| `padding` | `impl Into<Padding>` | Inner padding |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `focusable` | `bool` | Whether link accepts focus |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `disabled` | `bool` | Disable interaction |
| `visited` | `bool` | Mark link as visited |
| `on_activate` | `Callback<HyperlinkEvent>` | Emits on click, `Enter`, and `Space` |
| `on_key` | `KeyHandler` | Fallback key handler |

```rust
Hyperlink::new("Open docs")
    .href("https://example.com/docs")
    .visited(self.docs_opened)
    .visited_style(Style::new().fg(Color::Magenta).underline())
    .on_activate(ctx.link().callback(Msg::OpenLink))
```

`HyperlinkEvent` contains:
- `label: Arc<str>`
- `href: Option<Arc<str>>`

Open the destination explicitly from component logic:

```rust
if let Some(url) = ev.href.as_deref() {
    let _ = tui_lipan::utils::open_url(url);
}
```

---

## Input

Single-line text input field.

> **State binding:** Use `Input::bound(&state)` to create an `Input` that reads value, cursor, and anchor from a `TextInput` state bundle, and `InputEvent::apply_to(&mut state)` in your `update` handler to write changes back. This preserves cursor position and selection across rerenders. Using `Input::new(value)` resets the cursor to the end on every render.

| Prop | Type | Description |
|------|------|-------------|
| *(constructor)* | `impl Into<Arc<str>>` | `Input::new(value)` — current text (cursor at end) |
| *(constructor)* | `&TextInput` | `Input::bound(state)` — bind value, cursor, and anchor from state |
| `.bind(state)` | `&TextInput` | Apply cursor, anchor, and value from state onto an existing `Input` |
| `value` | `impl Into<Arc<str>>` | Current text value (use `bound`/`bind` instead for cursor preservation) |
| `cursor` | `usize` | Byte cursor position |
| `anchor` | `Option<usize>` | Selection anchor (for text selection) |
| `caret_shape` | `CaretShape` | Cursor shape - **default: `Block`** (only set when you want `Bar` or `Underline`) |
| `caret_color` | `Option<Color>` | OSC 12 cursor color (terminal support required) |
| `style` | `Style` | Idle style |
| `hover_style` | `Style` | Hover style |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `focus_style` | `Style` | Focused style |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `selection_style` | `Style` | Text selection style |
| `extend_selection_style` / `inherit_selection_style` | `Style` / `()` | Extend or inherit the text-selection theme role instead of replacing it |
| `placeholder` | `String` | Placeholder when empty |
| `mask` | `Option<char>` | Masking character (e.g., `'*'` for passwords) |
| `read_only` | `bool` | Allow selection but block keyboard input |
| `focusable` | `bool` | Whether input accepts focus; mouse selection and copy shortcuts still work when false |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `on_change` | `Callback<InputEvent>` | Emits value, cursor, and anchor on each edit |
| `on_edit` | `Callback<TextEditEvent>` | Emits structured edit events |
| `key_interceptor` | `KeyHandler` | Runs before text insertion |

**Recommended pattern** — store a `TextInput` in component state and use `Input::bound`:

```rust
struct State {
    query: TextInput,
}

fn create_state(&self, _props: &Self::Properties) -> Self::State {
    State { query: TextInput::new("") }
}

fn view(&self, ctx: &Context<Self>) -> Element {
    Input::bound(&ctx.state.query)
        .placeholder("Search...")
        .style(Style::new().fg(Color::White))
        .focus_style(Style::new().fg(Color::White).bg(Color::indexed(237)))
        .on_change(ctx.link().callback(Msg::QueryChanged))
}

fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::QueryChanged(ev) => {
            ev.apply_to(&mut ctx.state.query);
            Update::layout()
        }
    }
}
```

**Legacy pattern** (cursor resets to end on each render — avoid for editable inputs):

```rust
Input::new(self.query.clone())
    .placeholder("Search...")
    .on_change(ctx.link().callback(Msg::QueryChanged))
```

> `caret_color` uses OSC 12. Set `TUI_LIPAN_OSC12=0` to disable if your terminal doesn't support it.

**Undo/Redo:** `Ctrl+Z`, `Ctrl+Shift+Z`, `Ctrl+Y` (also handles raw control codes).

---

## TextArea

Multi-line text editor.

> **State binding:** Use `TextArea::bound(&state)` to create a `TextArea` that reads value, cursor, and anchor from a `TextEditor` state bundle, and `TextAreaEvent::apply_to(&mut state)` in your `update` handler to write changes back. This preserves cursor position and selection across rerenders. Using `TextArea::new(value)` resets the cursor on every render.

| Prop | Type | Description |
|------|------|-------------|
| *(constructor)* | `impl Into<Arc<str>>` | `TextArea::new(value)` — current text |
| *(constructor)* | `&TextEditor` | `TextArea::bound(state)` — bind value, cursor, and anchor from state |
| `.bind(state)` | `&TextEditor` | Apply cursor, anchor, and value from state onto an existing `TextArea` |
| `value` | `impl Into<Arc<str>>` | Current text value (use `bound`/`bind` instead for cursor preservation) |
| `cursor` | `usize` | Byte cursor position |
| `anchor` | `Option<usize>` | Selection anchor |
| `caret_shape` | `CaretShape` | Cursor shape - **default: `Block`**; Vim-enabled TextAreas use mode-aware defaults unless overridden |
| `caret_color` | `Option<Color>` | OSC 12 cursor color |
| `line_numbers` | `bool` | Show line number gutter |
| `line_number_mode` | `TextAreaLineNumberMode` | `Absolute` by default; `Relative` shows Vim-style distances from the cursor line while keeping the cursor line absolute |
| `gutter_inset` | `u16` | Empty cells before the gutter / line numbers |
| `wrap` | `bool` | Word wrap (default: true) |
| `max_width` | `Option<u16>` | Max line width before forced wrap |
| `scroll_offset` | `Option<usize>` | Controlled vertical scroll |
| `scroll_to_line` | `Option<usize>` | Zero-based logical/source line target; resolves through wrapped visual rows |
| `scroll_behavior` | `ScrollBehavior` | `Instant` by default; opt into smooth `scroll_to_line` movement |
| `scroll_transition` | `TransitionConfig` | Shortcut for smooth line-target movement |
| `scroll_wheel` | `bool` | Mouse wheel scrolling |
| `scroll_wheel_multiplier` | `u16` | Override the app-wide wheel line multiplier for this TextArea |
| `scrollbar` | `bool` | Vertical scrollbar |
| `scrollbar_config` | `ScrollbarConfig` | Full scrollbar configuration (variant, gap, thumb, thumb styles) |
| `h_scrollbar` | `bool` | Horizontal scrollbar (only when `wrap: false`) |
| `style` | `Style` | Idle style |
| `focus_style` | `Style` | Focused style |
| `selection_style` | `Style` | Text selection style |
| `extend_selection_style` / `inherit_selection_style` | `Style` / `()` | Extend or inherit the text-selection theme role instead of replacing it |
| `show_selection_when_unfocused` | `bool` | Keep the active anchor/cursor selection visible after focus leaves the `TextArea` (`true` by default; pass `false` to opt out) |
| `unfocused_selection_style` / `inherit_unfocused_selection_style` / `unfocused_selection_style_slot` | `Style` / `()` / `StyleSlot` | Style visible selections while unfocused; inherited/default slots resolve against the text-selection theme role; slot form is for composite forwarding |
| `placeholder` | `String` | Placeholder when empty |
| `placeholder_style` | `Style` | Placeholder style |
| `focus_placeholder_style` | `Style` | Placeholder when focused |
| `read_only` | `bool` | Allow selection but block keyboard input |
| `triple_click_mode` | `TripleClickSelectionMode` | Triple-click selects a line or paragraph |
| `newline_binding` | `TextAreaNewlineBinding` | Enter key behavior |
| `clear_bindings` | `KeyBindings` | Per-widget clear shortcuts; clear is an internal undoable replace edit |
| `vim_motions` | `bool` | Enable TextArea-only Vim-style Insert/Normal/Visual/VisualLine motion mode (default: `false`) |
| `vim_keymap` | `TextAreaVimKeymap` | Widget-local Vim key remaps to canonical Vim command characters |
| `vim_config` | `TextAreaVimConfig` | Vim-only rendering options: search bar, search match/current-match styles, current-line highlighting, and current line-number/gutter styling |
| `on_change` | `Callback<TextAreaEvent>` | Emits value, cursor, anchor on each edit |
| `on_edit` | `Callback<TextEditEvent>` | Structured edit events |
| `on_vim_mode_change` | `Callback<TextAreaVimMode>` | Emits `Insert`/`Normal`/`Visual`/`VisualLine` transitions when `vim_motions(true)` is enabled |
| `sentinels` | `Vec<TextAreaSentinel>` | Custom inline PUA tokens (`SENTINEL_BASE` + index in value) |
| `on_sentinels_change` | `Callback<Vec<TextAreaSentinel>>` | Fires when the list is pruned (e.g. user deleted a token) |
| `on_sentinel_event` | `Callback<Vec<SentinelEvent>>` | Lifecycle events with stable ids (e.g. `Deleted`) |
| `on_sentinel_click` | `Callback<TextAreaSentinelClickEvent>` | Fires when an inline image or custom sentinel placeholder is clicked |
| `decorations` | `Vec<TextAreaDecoration>` | Byte-range overlays for ranges, whole lines, and underline markers |
| `virtual_texts` | `Vec<TextAreaVirtualText>` | Non-editable inline inlay hints and EOL diagnostics |
| `gutter` | `TextAreaGutter` | Compose line numbers, signs, and custom gutter columns |
| `key_interceptor` | `KeyHandler` | Runs before text editing |
| `on_scroll` | `Callback<ScrollEvent>` | Scroll event with metrics |
| `on_scroll_to` | `Callback<usize>` | Target scroll offset |
| `color_strategy` | `Box<dyn TextAreaColorStrategy>` | Syntax highlighting strategy |
| `language` | `String` | Language hint for syntax highlighting; use `language_from_path(path)` to resolve from a file path |
| `theme` | `String` | Syntax theme name |
| `images` | `Vec<ImageContent>` | Attached images |
| `on_images_change` | `Callback<Vec<ImageContent>>` | Images list updated |
| `image_mode` | `TextAreaImageMode` | `Inline` or `Attachment` |
| `image_placeholder` | `String` | Placeholder text for inline images |
| `image_placeholder_style` | `Style` | Inline image placeholder style |
| `image_placeholder_hover_style` | `Style` | Style patched over hovered inline image placeholders |
| `on_image_paste` | `Callback<ImageContent>` | Legacy: image pasted via Ctrl+V |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `focusable` | `bool` | Accept keyboard focus; mouse selection and copy shortcuts still work when false |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `tab_width` | `u8` | Insert spaces to the next column multiple when Tab is pressed; `0` disables |
| `insert_tab` | `bool` | Insert a literal tab character instead of moving focus |
| `tab_display_width` | `u8` | Display width of literal tab characters (default: `8`) |

Use `ctx.text_area_scrollbars(key)` to read the resolved scrollbar visibility
for a keyed `TextArea` from the previous frame. Unkeyed `TextArea` widgets are
not tracked, and the first frame returns `ScrollbarVisibility::default()`.

`scroll_to_line(line)` is a declarative one-shot target for a zero-based logical
line. With wrapping enabled, it scrolls to that line's first visual row and
clamps out-of-range targets to the available content. Smooth behavior applies
only to this explicit target; use `ScrollBehavior::smooth_adaptive()` when you
want duration to scale with the resolved row distance. Controlled `scroll_offset`
and cursor auto-scroll stay immediate, and user scrolling/editing cancels an
active target animation.

`clear_bindings(bindings)` adds per-widget single-key shortcuts for clearing all
text. The clear runs inside the `TextArea` editor instead of requiring the app
to replace the controlled value, preserving the widget's undo history; undo
restores the previous text, cursor, and selection. `key_interceptor` runs before
clear, and a matching clear binding wins over keymap clipboard bindings for
that key. Clear emits the normal `on_change` and `on_edit` callbacks with a
replace edit.

```rust
// Recommended: use TextArea::bound to preserve cursor & selection
TextArea::bound(&ctx.state.editor)
    .line_numbers(true)
    .wrap(false)
    .h_scrollbar(true)
    .on_change(ctx.link().callback(Msg::TextChanged))
```

### TextArea editor primitives

Keyed `TextArea`s expose previous-frame geometry with
`ctx.text_area_metrics(key) -> Option<TextAreaMetrics>`. Metrics use
`tui_lipan::Rect` and byte offsets plus `TextPosition` projections; the first
frame, missing keys, and unkeyed text areas return `None`.

`TextArea::decoration` / `decorations` accept byte-range
`TextAreaDecoration`s. `Range`, `WholeLine`, and `Underline` styles render
through the normal overlay path before selection; `Underline` also enables the
underline modifier automatically. The old `TextAreaDecorationKind::VirtualText`
variant is a deprecated no-op; use dedicated virtual text entries instead.

`TextArea::virtual_text` / `virtual_texts` accept `TextAreaVirtualText` for
non-editable inlay hints and diagnostics. `VirtualTextPlacement::Inline` inserts
styled columns before the anchor byte and shifts later visual columns without
changing the buffer. A cursor at the anchor renders after the inline virtual
text, while mouse clicks inside the virtual columns clamp to the anchor byte.
`VirtualTextPlacement::Eol` appends styled text after the logical line's final
visual row and does not participate in wrapping. `TextAreaMetrics.position`
stays buffer-based for LSP round-tripping; cursor `rect` values include inline
virtual width.

Use `TextAreaGutter::new().line_numbers(...).signs(...)` with
`TextArea::gutter` to compose line numbers with sign/custom columns. Existing
`line_numbers`, `line_number_mode`, `gutter_lines`, and `gutter_inset` builders
remain supported.

### Vim motions

`TextArea::vim_motions(true)` enables an opt-in, widget-layer modal motion mode
for that `TextArea` only. Existing apps keep plain TextArea behavior unless they
enable it. An enabled TextArea starts in `TextAreaVimMode::Normal`; `i`, `a`,
`I`, or `A` enter Insert, `v` toggles characterwise Visual selection mode from
Normal/Visual, and `V` enters linewise Visual selection mode from Normal. Use
`on_vim_mode_change` to update app-owned status bars, frame titles, or other
mode-aware chrome.

When `caret_shape` is not overridden, Vim mode uses a steady block cursor for
Normal, Visual, and VisualLine, and a steady vertical bar for Insert. Non-Vim
TextAreas keep the regular caret behavior, including selection-driven cursor
hiding for non-Vim selections.

| Mode | Key | Behavior |
|------|-----|----------|
| Insert | `Esc` | Switch to `TextAreaVimMode::Normal` |
| Insert | other keys | Existing TextArea editing behavior |
| Normal | `Esc` | Clear pending count/command or hide visible search feedback, stay Normal |
| Normal | `1..9`, then `0..9` | Build a count prefix for motions |
| Normal | `0` | Move to current logical line start when no count is pending |
| Normal | `h` / `l` | Move left/right by character |
| Normal | `j` / `k` | Move down/up; wrapped TextAreas use visual-line navigation where available |
| Normal | `w` / `b` / `e` | Move to next word start, previous word start, or word end; punctuation runs and word characters are separate |
| Normal | `W` / `B` / `E` | Move by Vim WORDs: contiguous non-whitespace runs such as `open-code` or `path/to/file` |
| Normal | `$` | Move to current logical line end |
| Normal | `gg` / `G` | Move to first/last line; counts target one-based line numbers (`Ngg`, `NG`) |
| Normal | `v` | Enter `TextAreaVimMode::Visual` and anchor the selection at the cursor |
| Normal | `V` | Enter `TextAreaVimMode::VisualLine` and select whole logical lines |
| Normal | `i` / `a` | Enter Insert at the cursor / after moving right once if possible |
| Normal | `I` / `A` | Enter Insert at first non-blank / end of the current logical line |
| Normal | `u` | Undo the previous edit group |
| Normal | `ctrl+r` | Redo the next edit group |
| Normal | `yy` | Yank the current logical line (`y` enters operator-pending mode) |
| Normal | `d{motion}` / `dd` | Delete a motion range / whole logical lines, yanking before delete |
| Normal | `c{motion}` / `cc` | Delete a motion range / whole logical lines, yank it, and enter Insert |
| Normal | `dw` / `cw` | Delete/change by Vim word motion; `cw` changes the current word |
| Normal | `x` / `X` | Delete characters after / before the cursor, yanking before delete |
| Normal | `o` / `O` | Open a new indented line below / above and enter Insert |
| Normal | `p` / `P` | Paste after / before the cursor or current logical line |
| Normal | `.` | Repeat the last Vim change command supported by TextArea |
| Normal | `/text` `Enter` / `?text` `Enter` | Search forward / backward; `Esc` cancels pending search input |
| Normal | `n` / `N` | Repeat the last search in the same / opposite direction |
| Normal | `m{a-z}` | Set a mark at the current byte cursor |
| Normal | `'a` / `` `a `` | Jump to mark line first non-blank / exact mark cursor |
| Normal | `"{reg}` | Select a register for the next yank/delete/change/paste (`+`, `_`, `0..9`, `a..z`) |
| Normal operator | `iw` / `aw`, `iW` / `aW` | Inner / around word or WORD text object |
| Normal operator | `ip` / `ap` | Inner / around paragraph text object |
| Normal operator | quotes/brackets | Text objects for `'`, `"`, `` ` ``, `()`, `[]`, `{}`, and `<>` |
| Visual | supported motion keys | Move the cursor and extend the cursor/anchor selection |
| Visual | `y` | Yank the active selection and return to `TextAreaVimMode::Normal` |
| Visual | `d` / `x` | Delete the active selection, yank it, and return to `TextAreaVimMode::Normal` |
| Visual | `c` | Delete the active selection, yank it, and enter Insert |
| Visual | `p` / `P` | Replace the active selection with pasted text and return to `TextAreaVimMode::Normal` |
| Visual | `v` | Exit Visual mode and return to `TextAreaVimMode::Normal` |
| Visual | `V` | Exit Visual mode and return to `TextAreaVimMode::Normal` |
| Visual | `Esc` | Exit Visual mode, clear pending input, and return to `TextAreaVimMode::Normal` |
| VisualLine | supported motion keys | Move by supported motions and extend the selection to whole logical lines |
| VisualLine | `y` | Yank the whole-logical-line selection and return to `TextAreaVimMode::Normal` |
| VisualLine | `d` / `x` | Delete the whole-logical-line selection, yank it, and return to `TextAreaVimMode::Normal` |
| VisualLine | `c` | Delete the whole-logical-line selection, yank it, and enter Insert |
| VisualLine | `p` / `P` | Replace the whole-logical-line selection with pasted text and return to `TextAreaVimMode::Normal` |
| VisualLine | `v` / `V` | Exit linewise Visual mode and return to `TextAreaVimMode::Normal` |
| VisualLine | `Esc` | Exit linewise Visual mode, clear pending input, and return to `TextAreaVimMode::Normal` |

Normal mode blocks unsupported printable and text-editing keys so accidental
typing does not mutate the buffer. Characterwise Visual mode uses the same
supported motions as Normal mode, but moves extend the active selection instead
of clearing it. Linewise Visual mode selects whole logical lines, not soft-wrapped
visual rows; each selected line includes its trailing newline except the final
line when the buffer has no trailing newline.
Vim mode uses Normal `u` and `ctrl+r` for undo/redo; `Ctrl+Z` and `Ctrl+Y` are
not the Vim undo/redo path. Registers are TextArea-local except the unnamed and
`+` registers, which also write/read the runtime clipboard. The black-hole
register (`_`) discards yanks/deletes, `0` stores the latest yank, and `1..9`
track recent deletes/changes.

Pending `/` and `?` searches render a dedicated bottom search bar on the focused
TextArea. The bar owns the full inner row, including line-number/custom gutter
space, uses `  ` for forward search and `  ` for backward search, moves the
terminal cursor into the query bar while typing, and right-aligns the current
match count (`[2/5]`). Visible matches stay underlined after `Enter`, and the
current match gets the configured current-match background highlight as `n` / `N`
navigate through results. After `Enter`, the bottom search bar disappears and the
`[current/total]` count is mirrored after the text on the visible row containing
the current match. Normal `Esc` hides the visible search highlights/count without
forgetting the stored query, so `n` / `N` can show and repeat it again.

Use `TextArea::vim_config(...)` for Vim-only visual affordances such as the
search bar style, search-match/current-match styles, current-line highlighting,
or the current line number/custom gutter style:

```rust
TextArea::bound(&ctx.state.editor)
    .vim_motions(true)
    .vim_config(
        TextAreaVimConfig::new()
            .search_bar_prefix_style(Style::new().fg(Color::Cyan))
            .search_bar_count_style(Style::new().fg(Color::Yellow))
            .current_line_highlight(TextAreaVimCurrentLineHighlight::Full)
            .current_line_number_style(Style::new().fg(Color::Yellow).bold()),
    )
```

Clipboard shortcuts, configured clear bindings, `key_interceptor`, non-mutating
navigation actions, and app/global actions keep their existing precedence. Vim
bindings are not loaded from `keymap.conf`; use `TextArea::vim_keymap(...)` for
widget-local aliases to canonical Vim command characters:

```rust
let vim_keymap = TextAreaVimKeymap::new()
    .bind(KeyBindings::from_str("ctrl+n")?, 'j')
    .bind(KeyBindings::from_str("ctrl+p")?, 'k');

TextArea::bound(&ctx.state.editor)
    .vim_motions(true)
    .vim_keymap(vim_keymap)
```

Mutating clipboard operations and clear bindings leave Visual or VisualLine mode
and clear the visual anchor after they run. Vim support is still TextArea-only:
it does not affect `Input` or `TextEditor` directly and does not render a built-in
mode label.

Mouse-created selections participate in Vim mode: double-click, triple-click, and
drag ranges enter `TextAreaVimMode::Visual` automatically. Current-line row
highlighting is suppressed while Visual/VisualLine selections are active. In
VisualLine mode (`V`), the emitted cursor/anchor still cover whole logical lines,
but the terminal caret is drawn on the active line at the original column clamped
to that line's length.

### Custom inline sentinels (extmarks)

Styled atomic tokens in the buffer use a **separate PUA range** from inline images: `SENTINEL_BASE` (`U+F000`). Entry `i` in `sentinels` maps to the single character `U+F000 + i` in `value`. One backspace/delete removes the whole codepoint; the framework prunes the parallel `sentinels` list and can emit `SentinelEvent` batches (see [`docs/enums.md`](../enums.md) and [`docs/events.md`](../events.md)).

| Type / API | Role |
|------------|------|
| `TextAreaSentinel` | Label, normal/focus/hover styles, optional type-erased `payload`, optional `SentinelId` (or assign via `insert_sentinel`) |
| `insert_sentinel(value, cursor, sentinels, sentinel)` | Insert at cursor; assigns `SentinelId::next()` when `id` is unset |
| `TextAreaSnapshot` | `capture` / `apply` / `diff` for in-memory stash–restore |

Prefer `on_sentinel_event` for cleanup keyed by stable id; keep `on_sentinels_change` if you only need the pruned list. Use `on_sentinel_click` when a sentinel should open or expand app-owned content stored in its payload. Use `TextAreaSentinel::hover_style` for hover affordances such as `ColorTransform::Lighten`.

**Example:** `examples/text_area_sentinels.rs`

### Syntax Highlighting *(requires feature `syntax-syntect`)*

```rust
TextArea::new(code.clone())
    .with_syntax("rust", "base16-ocean.dark")

// With background colors
    .with_syntax_bg("rust", "one-dark")

// Auto-detect language from file path (extension/filename matching, no I/O)
TextArea::new(code.clone())
    .language_from_path("src/main.rs")   // resolves to "Rust"
    .with_syntax_strategy(SyntectStrategy::default(), "Rust", "base16-ocean.dark")

// Or use the free function to get the language string yourself
if let Some(lang) = tui_lipan::language_from_path(&file_path) {
    area = area.language(lang);
}

// Custom theme from file
    .with_syntax_custom_theme_from_file("rust", "MyTheme", "/path/to/theme.tmTheme")
```

Built-in themes: `Catppuccin Frappe`, `Catppuccin Latte`, `Catppuccin Macchiato`, `Catppuccin Mocha`, `Dracula`, `InspiredGitHub`, `Monokai Extended`, `One Dark (Atom)`, `Solarized (dark)`, `Solarized (light)`, `base16-eighties.dark`, `base16-mocha.dark`, `base16-ocean.dark`, `base16-ocean.light`.

The bundled Syntect defaults do not include TypeScript/TSX grammars. When
`.language_from_path(...)` sees `.ts` or `.tsx` and no exact grammar is present,
it falls back to JavaScript highlighting instead of leaving the content plain.

`SyntectStrategy::use_background(true)` - apply syntax theme backgrounds (default: false).

When a `TextArea`, `DocumentView`, or `DiffView` uses `SyntectStrategy`, the app
theme can now gently recolor token categories via `Theme::syntax(...)` while
still using the selected syntect theme for tokenization and base styling.

### Image Modes *(requires feature `image`)*

**Inline mode** (images embedded as Unicode PUA sentinels in text value):

```rust
TextArea::new(self.input.clone())
    .image_mode(TextAreaImageMode::Inline)
    .images(self.images.clone())
    .on_images_change(ctx.link().callback(Msg::ImagesChanged))
    .image_placeholder("[Img]")
    .image_placeholder_style(Style::new().fg(Color::Magenta).bold())
```

Sentinel characters (U+E000…) represent images in the text value. Cursor movement and deletion work naturally. Use `IMAGE_SENTINEL_BASE` to construct sentinel chars manually if needed.

Inline image placeholders can be made interactive with `on_sentinel_click`; the event reports the image index and `ImageContent` for the clicked placeholder. Use `image_placeholder_hover_style` to add hover affordance to these image labels.

**Attachment mode** (images in separate list, text value unchanged):

```rust
VStack::new()
    .gap(0)
    .child(
        if !self.images.is_empty() {
            DraggableTabBar::new()
                .tabs(self.images.iter().enumerate().map(|(i, _)| {
                    DraggableTab::new(format!("Image {}", i + 1)).closeable(true)
                }))
                .active(usize::MAX)
                .close_symbol("x")
                .draggable(false)
                .focusable(false)
                .on_close(ctx.link().callback(Msg::RemoveImage))
                .into()
        } else { Element::empty() }
    )
    .child(
        TextArea::new(self.input.clone())
            .image_mode(TextAreaImageMode::Attachment)
            .images(self.images.clone())
            .on_images_change(ctx.link().callback(Msg::ImagesChanged))
    )
```

**Image pasting is opt-in**: only active when `on_images_change` or `on_image_paste` is set. Without these, `Ctrl+V` pastes text only.

---

## DiffView *(requires feature `diff-view`)*

Diff viewer with pluggable backends:

- `DiffViewBackend::TextArea` (default) - supports editable mode
- `DiffViewBackend::DocumentView` - read-only review + selection optimized

Backend selection rules:

- Explicit `.backend(...)` always wins.
- If backend is not set explicitly, calling `.document_view(...)` switches to `DocumentView`.
- If backend is not set explicitly, calling `.text_area(...)` switches to `TextArea`.
- Outer `DiffView` sizing inherits the active backend's `width`/`height` unless you override it with `.width(...)` / `.height(...)`.
- Pane borders are rendered by internal `Frame` wrappers, not by inner `TextArea`/`DocumentView` borders.

Line numbers in `DiffView` are source-mapped (git-style), not visual-row counters:

- Split left pane shows original (`before`) line numbers.
- Split right pane shows modified (`after`) line numbers.
- Unified mode uses original numbers for removed lines, and modified numbers for added/context lines.

| Prop | Type | Description |
|------|------|-------------|
| `before` | `String` | **Constructor** (first arg) - original text |
| `after` | `String` | **Constructor** (second arg) - modified text |
| `mode` | `DiffViewMode` | `Split` (default) or `Unified` |
| `backend` | `DiffViewBackend` | `TextArea` (default) or `DocumentView` |
| `editable` | `bool` | Enable editing (TextArea backend only) |
| `width` | `Length` | Override outer diff view width (otherwise inherit active backend width) |
| `height` | `Length` | Override outer diff view height (otherwise inherit active backend height) |
| `wrap` | `bool` | Apply wrapping to both backends |
| `line_numbers` | `bool` | Toggle line numbers in both backends |
| `min_line_number_width` | `u8` | Minimum gutter digits in both backends |
| `gutter_inset` | `u16` | Empty cells before the gutter / line numbers in both backends |
| `border` | `bool` | Toggle outer border around the whole DiffView |
| `panels_border` | `bool` | Toggle per-pane wrapper borders |
| `scrollbar` | `bool` | Toggle vertical scrollbar in both backends |
| `h_scrollbar` | `bool` | Toggle horizontal scrollbar in both backends |
| `focusable` | `bool` | Toggle focusability in both backends |
| `single_scrollbar` | `bool` | In split mode, show vertical scrollbar only on right pane |
| `join_frame` | `bool` | Join split-pane wrapper frames (`Frame::join_frame`) |
| `vertical_separator` | `bool` | Insert vertical divider between split panes |
| `vertical_separator_char` | `char` | Character used by split divider |
| `vertical_separator_style` | `Style` | Style for split divider |
| `highlight_full_width` | `bool` | Extend changed-line background highlight to full row width |
| `word_diff` | `bool` | Word-level diff highlighting |
| `trim_common_indent` | `bool` | Trim the smallest shared leading indent from visible diff lines (default: `true`) |
| `show_prefixes` | `bool` | Show +/- prefix symbols |
| `diff_style` | `DiffPalette` | Added/removed/context-separator/patch-header styles |
| `neutral_bg` | `Color` | Convenience setter for context/unchanged line background |
| `base_color_strategy` | `Box<dyn TextAreaColorStrategy>` | Syntax highlighting base strategy |
| `language` | `String` | Language hint; use `language_from_path(path)` to resolve from a file path |
| `theme` | `String` | Syntax theme name |
| `text_area` | `TextArea` | Pre-configured TextArea for scroll/border/wrap settings |
| `document_view` | `DocumentView` | Pre-configured DocumentView for scroll/border/wrap settings |
| `scroll_offset` | `usize` | Controlled scroll offset (applies to rendered pane(s)) |
| `scroll_to_hunk` | `usize` | Scroll rendered pane(s) to a zero-based parsed patch hunk index; resolved after trim/context collapse and before backend wrap layout |
| `context_lines` | `usize` | Collapse unchanged regions farther than `n` lines from any change into a separator line (default: show all) |
| `show_context_separator` | `bool` | Show/hide the context separator placeholder when collapsing context (default: `true`) |
| `context_separator_text` | `String` | Template for context separator text; supports `{count}`, `{line_word}`, `{direction}`, `{arrow}` |
| `context_separator_hover_style` | `Style` | Style patched over a context separator while the pointer hovers it |
| `context_separator_min_lines` | `usize` | Minimum hidden lines before a separator is shown; shorter runs render as normal context (default: `2`) |
| `context_expand_lines` | `usize` | Per-click reveal size used by `DiffContextSeparatorEvent::next_expansion` (default: `20`) |
| `expanded_contexts` | `IntoIterator<Item = DiffContextRange>` | Controlled set of collapsed context ranges that should render fully expanded |
| `expanded_context_expansions` | `IntoIterator<Item = DiffContextExpansion>` | Controlled partial or full expansions |
| `expanded_context` | `DiffContextRange` | Convenience setter to fully expand one collapsed context range |
| `expanded_context_lines` | `(DiffContextRange, usize)` | Expand one collapsed range by a specific number of lines |
| `on_context_separator_click` | `Callback<DiffContextSeparatorEvent>` | Fires when a visible context separator line is clicked |
| `shared_selection_id` | `Arc<str>` | Cross-widget selection group id (unified: as-is; split: auto-suffixed `:left`/`:right`, while plain `DocumentView`s using the unsuffixed base id can still drag into split panes) |
| `on_scroll` | `Callback<DiffScrollEvent>` | Pane-aware scroll callback (`pane` + `ScrollEvent`) |

```rust
let diff = DiffView::new(before, after)
    .mode(DiffViewMode::Split)
    .document_view(DocumentView::new("")) // backend inferred
    .height(Length::Auto) // useful for inline/message-style diff blocks
    .border(true)
    .panels_border(true)
    .wrap(true)
    .line_numbers(true)
    .min_line_number_width(4)
    .single_scrollbar(true)
    .join_frame(true)
    .vertical_separator(true)
    .vertical_separator_style(Style::new().dim())
    .highlight_full_width(true)
    .neutral_bg(Color::rgb(24, 24, 24))
    .word_diff(true)
    .show_prefixes(true);

By default, `DiffView` derives its added/removed/marker styling from `Theme::diff`.
Use `.diff_style(...)` only when you want per-widget overrides.

`DiffView` also trims shared leading indentation by default so deeply indented code is easier to read inline. Use `.trim_common_indent(false)` when you need the original left margin preserved exactly.

With the `DocumentView` backend in split mode, dragging selection across the
divider selects both panes row-by-row. Copy shortcuts copy that cross-pane
selection as tab-separated logical diff rows (`left\tright`), collapsing any
soft-wrapped visual rows back into their original diff line.

// Efficient: build DiffData once, reuse across renders
let data = DiffData::with_config(before, after, config);
let diff = DiffView::new(before, after).with_diff(data);
// or: DiffView::new(before, after).with_shared_diff(Arc<DiffData>)

// Patch navigation: anchors are logical rendered rows, not final wrapped rows.
// Use scroll_to_hunk(index) so the backend resolves wrapping during layout.
let patch_data = DiffData::from_patch(patch);
let hunk_count = patch_data.hunk_anchors(DiffViewMode::Unified).len();
let diff = DiffView::from_patch(patch)
    .mode(DiffViewMode::Unified)
    .scroll_to_hunk(selected_hunk.min(hunk_count.saturating_sub(1)));

// DiffData::hunk_anchors_for_pane(DiffPane::Right) is also available when an
// app needs pane-specific logical anchors. Before/after content diffs have no
// patch hunk anchors unless built from a unified patch.

// DiffDataConfig fields:
// - word_diff: bool        - precompute word-level diff tokens
// - context_lines: Option<usize> - collapse unchanged regions (same as the prop)
// - show_context_separator: bool   - insert separator placeholder (default: true)
// - context_separator_text: Arc<str> - template for separator text

// Split scroll sync pattern (controlled):
let mut offset: Option<usize> = None;
let mut diff = DiffView::new(before, after)
    .mode(DiffViewMode::Split)
    .on_scroll(ctx.link().callback(|ev: DiffScrollEvent| Msg::DiffScrolled(ev)));
if let Some(v) = offset {
    diff = diff.scroll_offset(v);
}

// Custom diff colors (lines + markers + line numbers):
let style = DiffPalette {
    added: Style::new().bg(Color::rgb(0x14, 0x2F, 0x20)),
    removed: Style::new().bg(Color::rgb(0x3B, 0x1E, 0x24)),
    context_line_number: Style::new().fg(Color::DarkGray), // fg/bg for unchanged line numbers in gutter
    added_marker: Style::new().fg(Color::Green),
    removed_marker: Style::new().fg(Color::Red),
    added_line_number: Style::new().fg(Color::DarkGray),   // fg/bg for added line numbers in gutter
    removed_line_number: Style::new().fg(Color::DarkGray), // fg/bg for removed line numbers in gutter
    context_separator_style: Style::new().fg(Color::DarkGray).dim(), // style for context-collapse separator lines
    patch_header: Style::new().fg(Color::Cyan).bold(), // style for in-band `diff --git ...` metadata lines
    ..DiffPalette::default()
};

// Context lines - collapse unchanged regions far from changes:
DiffView::new(before, after)
    .context_lines(3) // show 3 lines of context around each change
    .context_separator_text("{arrow} {count} {line_word} omitted {direction}")
    .show_context_separator(false) // omit separator placeholders entirely
    .mode(DiffViewMode::Unified);
// Separator lines are excluded from copy operations.
// Both Split and Unified modes support context_lines.
// Default separator text is arrow-based and direction-aware, e.g. "↑ 9 hidden lines above ↑".
// The separator style is themed by default (dimmed muted text); override via DiffPalette::context_separator_style.
// Raw patch metadata lines like `diff --git a/... b/...` stay in the scrollable diff and use DiffPalette::patch_header.

// Click-to-expand pattern (controlled by your component state):
DiffView::new(before, after)
    .context_lines(3)
    .context_expand_lines(20)
    .expanded_context_expansions(expansions.clone())
    .context_separator_text("+ show {count} {line_word} {direction}")
    .context_separator_hover_style(Style::new().underline())
    .on_context_separator_click(ctx.link().callback(Msg::ExpandDiffContext));
// In update(), find the current expansion by ev.range, then store ev.next_expansion(current).
// Scroll position is preserved automatically when context lines are revealed.

// Auto-detect language from file path (requires feature `syntax-syntect`):
DiffView::new(before, after)
    .language_from_path("src/main.rs")  // resolves to "Rust", no-op if unknown
    .theme("One Dark (Atom)");          // set theme separately; with_syntax() would override the detected language
```

For `.ts` and `.tsx` paths, `DiffView::language_from_path(...)` uses the same
JavaScript fallback as `TextArea` when the active Syntect syntax set has no
TypeScript/TSX grammar.

---

## Checkbox

Toggle widget for boolean values.

| Prop | Type | Description |
|------|------|-------------|
| `checked` | `bool` | **Constructor** - checked state |
| `state` | `CheckboxState` | Full state (overrides `checked`) |
| `indeterminate` | `bool` | Show indeterminate state |
| `label` | `String` | Label text |
| `variant` | `CheckboxVariant` | Visual variant |
| `gap` | `u16` | Space between box and label |
| `style` | `Style` | Idle style |
| `hover_style` | `Style` | Hover style |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `focus_style` | `Style` | Focus style |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `checked_style` | `Style` | Style when checked |
| `unchecked_style` | `Style` | Style when unchecked |
| `indeterminate_style` | `Style` | Style when indeterminate |
| `label_style` | `Style` | Label style |
| `padding` | `impl Into<Padding>` | Padding |
| `disabled` | `bool` | Disable interaction |
| `disabled_style` | `Style` | Style when disabled |
| `focusable` | `bool` | Accept focus |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `on_toggle` | `Callback<bool>` | Toggle callback |
| `on_click` | `Callback<()>` | Click callback |

---

## Radio

Radio button group.

| Prop | Type | Description |
|------|------|-------------|
| `options` | `Vec<Arc<str>>` | **Constructor** - option labels |
| `selected` | `Option<usize>` | Selected index |
| `layout` | `RadioLayout` | `Vertical` or `Horizontal` |
| `variant` | `RadioVariant` | Visual variant |
| `gap` | `u16` | Space between options |
| `style` | `Style` | Base style |
| `checked_style` | `Style` | Selected item style |
| `unchecked_style` | `Style` | Unselected item style |
| `hover_style` | `Style` | Hover style |
| `focus_style` | `Style` | Focus style |
| `label_style` | `Style` | Label style |
| `disabled` | `bool` | Disable interaction |
| `disabled_style` | `Style` | Style when disabled |
| `on_change` | `Callback<usize>` | Selection changed callback |

```rust
Radio::new(vec!["Option A".into(), "Option B".into(), "Option C".into()])
    .selected(Some(self.choice))
    .layout(RadioLayout::Horizontal)
    .on_change(ctx.link().callback(Msg::ChoiceChanged))
```

---

## Select

Dropdown select widget.

| Prop | Type | Description |
|------|------|-------------|
| `options` | `Vec<String>` | Available options |
| `selected` | `Option<usize>` | Selected index |
| `placeholder` | `String` | Placeholder when nothing selected |
| `expanded` | `bool` | Controlled expanded state |
| `width` | `Length` | Width |
| `disabled` | `bool` | Disable interaction |
| `on_select` | `Callback<usize>` | Item selected |
| `on_change` | `Callback<usize>` | Selection changed |
| `on_toggle` | `Callback<bool>` | Dropdown opened/closed |
| `button_variant` | `ButtonVariant` | Trigger button variant |
| `button_style` | `Style` | Button idle style |
| `button_hover_style` | `Style` | Button hover style |
| `extend_button_hover_style` / `inherit_button_hover_style` | `Style` / `()` | Extend or inherit the button hover theme role |
| `button_focus_style` | `Style` | Button focus style |
| `extend_button_focus_style` / `inherit_button_focus_style` | `Style` / `()` | Extend or inherit the button focus theme role |
| `button_disabled_style` | `Style` | Button disabled style |
| `button_border_style` | `BorderStyle` | Trigger border style (outlined variant) |
| `button_hover_border_style` | `BorderStyle` | Trigger border style when hovered |
| `button_focus_border_style` | `BorderStyle` | Trigger border style when focused |
| `button_open_suffix` | `String` | Trigger suffix while expanded |
| `button_closed_suffix` | `String` | Trigger suffix while collapsed |
| `button_suffix_style` | `Style` | Trigger suffix style |
| `list_title` | `String` | Dropdown title (bordered list) |
| `list_title_style` | `Style` | Dropdown title style |
| `list_border` | `bool` | Dropdown list border |
| `list_border_style` | `BorderStyle` | Dropdown border style |
| `list_padding` | `impl Into<Padding>` | Dropdown padding |
| `list_style` | `Style` | Dropdown list style |
| `list_selection_style` | `Style` | Selected item style |
| `extend_list_selection_style` / `inherit_list_selection_style` | `Style` / `()` | Extend or inherit the selection theme role instead of replacing it |
| `list_unfocused_selection_style` | `Style` | Selected item style while dropdown list is not focused; defaults to `list_selection_style` |
| `extend_list_unfocused_selection_style` / `inherit_list_unfocused_selection_style` | `Style` / `()` | Extend or inherit the unfocused selection theme role instead of replacing it |
| `list_selection_full_width` | `bool` | Extend selection style across full row |
| `list_selection_symbol` | `Option<String>` | Selection symbol for selected row |
| `list_selection_symbol_style` | `Style` | Selection symbol style |
| `list_unfocused_selection_symbol_style` | `Style` | Selection symbol style while dropdown list is not focused; defaults to `list_selection_symbol_style` |
| `list_hover_style` | `Style` | Hover style in list |
| `extend_list_hover_style` / `inherit_list_hover_style` | `Style` / `()` | Extend or inherit the list hover theme role |
| `list_width` | `Length` | Dropdown width override |
| `list_height` | `Length` | Dropdown height override |
| `match_button_width` | `bool` | Force dropdown width to trigger width |
| `list_scrollbar` | `bool` | Scrollbar in list |
| `list_scrollbar_config` | `ScrollbarConfig` | Full scrollbar configuration (variant, gap, thumb, thumb styles) |
| `list_empty_text` | `String` | Dropdown empty text |
| `list_empty_text_style` | `Style` | Dropdown empty text style |
| `list_disabled_style` | `Style` | Dropdown disabled style |

Select forwards shared dropdown-list chrome through `ListConfig`, including
`symbol_column`, `gutter_gap`, and `gutter_for_non_selectable` for row-local
leading adornment alignment.

---

## ComboBox

Controlled input + dropdown list for searchable selection.

| Prop | Type | Description |
|------|------|-------------|
| `items` | `Vec<Arc<str>>` | Source options |
| `query` | `Arc<str>` | Controlled input value |
| `placeholder` | `String` | Input placeholder |
| `open` | `bool` | Controlled dropdown state |
| `active_index` | `Option<usize>` | Active source index |
| `selected` | `Option<usize>` | Selected source index fallback |
| `allow_custom_value` | `bool` | Allow Enter to commit free-form query |
| `width` | `Length` | Input width |
| `list_width` | `Length` | Dropdown width override |
| `list_height` | `Length` | Dropdown height |
| `list_selection_style` | `Style` | Active dropdown item style |
| `extend_list_selection_style` / `inherit_list_selection_style` | `Style` / `()` | Extend or inherit the selection theme role instead of replacing it |
| `list_unfocused_selection_style` | `Style` | Active dropdown item style while dropdown list is not focused; defaults to `list_selection_style` |
| `extend_list_unfocused_selection_style` / `inherit_list_unfocused_selection_style` | `Style` / `()` | Extend or inherit the unfocused selection theme role instead of replacing it |
| `list_unfocused_selection_symbol_style` | `Style` | Active dropdown item symbol style while dropdown list is not focused; defaults to `list_selection_symbol_style` |
| `match_input_width` | `bool` | Force dropdown width to match input width |
| `disabled` | `bool` | Disable interaction |
| `input_hover_style` | `Style` | Input hover style |
| `extend_input_hover_style` / `inherit_input_hover_style` | `Style` / `()` | Extend or inherit the input hover theme role |
| `input_focus_style` | `Style` | Input focus style |
| `extend_input_focus_style` / `inherit_input_focus_style` | `Style` / `()` | Extend or inherit the input focus theme role |
| `input_disabled_style` | `Style` | Input disabled style |
| `input_hover_border_style` | `BorderStyle` | Input border style while hovered |
| `input_open_suffix` | `String` | Suffix when dropdown is open |
| `input_closed_suffix` | `String` | Suffix when dropdown is closed |
| `input_suffix_style` | `Style` | Input suffix style |
| `input_focus_suffix_style` | `Style` | Input suffix style when focused |
| `on_query_change` | `Callback<Arc<str>>` | Input query changed |
| `on_open_change` | `Callback<bool>` | Request open/close change |
| `on_active_index_change` | `Callback<Option<usize>>` | Active source index changed |
| `on_commit` | `Callback<ComboBoxCommitEvent>` | Enter/activate commit event |

`ComboBoxCommitEvent` contains:
- `index: Option<usize>`
- `value: Arc<str>`
- `from_custom_value: bool`

Interaction notes:
- With `match_input_width(true)`, dropdown width follows the rendered trigger/input width.
- ComboBox forwards dropdown-list chrome through `ListConfig`, including
  `symbol_column`, `gutter_gap`, and `gutter_for_non_selectable`.

---

## MultiSelect

Controlled list for selecting multiple items with `Space` toggle and `Enter` commit.

| Prop | Type | Description |
|------|------|-------------|
| `items` | `impl Iterator<Item = impl Into<MultiSelectItem>>` | Source rows |
| `active_index` | `usize` | Active source index |
| `selected_indices` | `Vec<usize>` | Controlled selected source indices |
| `max_selected` | `usize` | Optional maximum selected rows |
| `selected_prefix` | `String` | Prefix for selected rows (default: `[x]`) |
| `unselected_prefix` | `String` | Prefix for unselected rows (default: `[ ]`) |
| `description_style` | `Style` | Style used for item descriptions |
| `description_placement` | `MultiSelectDescriptionPlacement` | Description placement: `Inline`, `Right`, `Above`, `Below` |
| `description_overflow` | `MultiSelectDescriptionOverflow` | Description overflow policy: `Truncate` or `Wrap` (`Wrap` applies to `Above`/`Below`) |
| `description_selection` | `bool` | Whether selection highlight applies to descriptions |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `title` | `String` | List title (requires border) |
| `title_style` | `Style` | List title style |
| `selection_style` | `Style` | Active row style |
| `extend_selection_style` / `inherit_selection_style` | `Style` / `()` | Extend or inherit the selection theme role instead of replacing it |
| `unfocused_selection_style` | `Style` | Active row style while list is not focused; defaults to `selection_style` |
| `extend_unfocused_selection_style` / `inherit_unfocused_selection_style` | `Style` / `()` | Extend or inherit the unfocused selection theme role instead of replacing it |
| `unfocused_selection_symbol_style` | `Style` | Active row symbol style while list is not focused; defaults to `selection_symbol_style` |
| `selection_full_width` | `bool` | Expand selection style across row width |
| `disabled` | `bool` | Disable interaction |
| `disabled_style` | `Style` | Style when disabled |
| `empty_text` | `String` | Text when list is empty |
| `empty_text_style` | `Style` | Empty-text style |
| `on_active_index_change` | `Callback<usize>` | Active row changed |
| `on_toggle` | `Callback<MultiSelectToggleEvent>` | Current row toggled |
| `on_change` | `Callback<MultiSelectChangeEvent>` | Selected set changed |
| `on_commit` | `Callback<MultiSelectCommitEvent>` | Enter commit with selected set |

Event payloads:
- `MultiSelectToggleEvent { index, selected }`
- `MultiSelectChangeEvent { selected_indices }`
- `MultiSelectCommitEvent { selected_indices }`

Interaction notes:
- Space toggles the active_index row.
- Mouse click on a row toggles that row and updates the active row.
- Enter emits `on_commit` (mouse click does not commit).

`MultiSelectItem` supports optional descriptions:

```rust
MultiSelectItem::new("Cargo.toml")
    .description("Workspace manifest")
```

`description_selection(false)` only affects `Above`/`Below` description lines.
For `Inline` and `Right`, selection/hover styling still applies to the full row.

`description_overflow(MultiSelectDescriptionOverflow::Wrap)` affects only `Above`/`Below` placement.
`Inline` and `Right` keep single-row truncation behavior.

MultiSelect forwards its inner `List` chrome through `ListConfig`, including
`symbol_column`, `gutter_gap`, and `gutter_for_non_selectable` for consistent
marker-column alignment.

---

## HexArea

Hex/ASCII binary data viewer with keyboard cursor navigation.

| Prop | Type | Description |
|------|------|-------------|
| `bytes` | `Arc<[u8]>` | **Constructor** - byte buffer to render |
| `cursor` | `usize` | Controlled cursor byte index |
| `anchor` | `Option<usize>` | Optional selection anchor byte index |
| `read_only` | `bool` | Read-only mode flag |
| `bytes_per_row` | `u16` | Number of bytes rendered per row |
| `show_ascii` | `bool` | Show ASCII preview column |
| `show_offsets` | `bool` | Show hexadecimal offset gutter |
| `uppercase_hex` | `bool` | Uppercase (`AA`) or lowercase (`aa`) hex output |
| `scroll_offset` | `Option<usize>` | Controlled row scroll offset |
| `style` | `Style` | Base style |
| `hover_style` | `Style` | Hover style |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `focus_style` | `Style` | Focus style |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `selection_style` | `Style` | Byte-range selection style |
| `extend_selection_style` / `inherit_selection_style` | `Style` / `()` | Extend or inherit the text-selection theme role instead of replacing it |
| `cursor_style` | `Style` | Cursor byte style |
| `pending_edit_style` | `Style` | Background/style for half-entered nibble edits |
| `border` | `bool` | Draw border |
| `border_style` | `BorderStyle` | Border style |
| `padding` | `impl Into<Padding>` | Inner padding |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `focusable` | `bool` | Accept keyboard focus |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `disabled` | `bool` | Disable interaction |
| `on_cursor_change` | `Callback<HexAreaCursorEvent>` | Emits on cursor movement (keyboard/mouse) |
| `on_change` | `Callback<HexAreaChangeEvent>` | Emits updated bytes after edits |
| `on_edit` | `Callback<HexAreaEditEvent>` | Emits per-edit metadata (replace/insert/delete) |
| `on_scroll` | `Callback<ScrollEvent>` | Emits desired row offset during navigation/wheel scrolling |
| `on_key` | `KeyHandler` | Custom key handler fallback |

`HexAreaCursorEvent` contains:
- `cursor: usize`
- `anchor: Option<usize>`

Interaction notes:
- Hex and ASCII columns are both clickable and map to the same byte index.
- Click-and-drag creates/extends a byte-range selection.
- First typed hex digit clears the cell to `<digit> ` and enters pending-nibble mode.
- Pending-nibble mode highlights the edited cell with `pending_edit_style`.
- `Esc` cancels pending-nibble mode and restores the original byte.

`HexAreaEditEvent` contains:
- `index: usize`
- `before: Option<u8>`
- `after: Option<u8>`
- `kind: HexAreaEditKind`

Edit keys (when `read_only(false)` and `on_change` is set):
- Hex digits (`0-9`, `a-f`) replace bytes in two-keystroke nibble mode
- `Insert` inserts `0x00` at cursor
- `Delete` removes byte at cursor
- `Backspace` removes byte before cursor
- `Ctrl+Z` undo, `Ctrl+Shift+Z`/`Ctrl+Y` redo

---

## Slider

Numeric selection slider.

| Prop | Type | Description |
|------|------|-------------|
| `value` | `f64` | **Constructor** - current value |
| `min` | `f64` | Minimum value |
| `max` | `f64` | Maximum value |
| `step` | `f64` | Step increment |
| `label` | `String` | Label text |
| `show_value` | `bool` | Show current value |
| `thumb_symbol` | `char` | Thumb character |
| `track_symbol` | `char` | Empty track character |
| `filled_track_symbol` | `char` | Filled track character |
| `hover_thumb_symbol` | `char` | Thumb on hover |
| `style` | `Style` | Base style |
| `filled_track_style` | `Style` | Filled portion style |
| `filled_track_gradient` | `ColorGradient` | Filled portion gradient |
| `thumb_style` | `Style` | Thumb style |
| `thumb_gradient` | `ColorGradient` | Thumb gradient |
| `focus_style` | `Style` | Focus style |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `focus_thumb_style` | `Style` | Thumb when focused |
| `extend_focus_thumb_style` / `inherit_focus_thumb_style` | `Style` / `()` | Extend or inherit the focus theme role for the thumb |
| `hover_thumb_style` | `Style` | Thumb on hover |
| `extend_hover_thumb_style` / `inherit_hover_thumb_style` | `Style` / `()` | Extend or inherit the hover theme role for the thumb |
| `label_style` | `Style` | Label style |
| `padding` | `impl Into<Padding>` | Padding |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `focusable` | `bool` | Accept focus |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `on_change` | `Callback<f64>` | Value changed |
| `on_click` | `Callback<f64>` | Click / Enter |

---

## DatePicker

Calendar-based date selection.

| Prop | Type | Description |
|------|------|-------------|
| `year` | `i32` | Current year |
| `month` | `u32` | Current month (1–12) |
| `day` | `Option<u32>` | Selected day |
| `title` | `String` | Calendar title |
| `show_outside_days` | `bool` | Show days from adjacent months |
| `border` | `bool` | Draw border |
| `border_style` | `BorderStyle` | Border appearance |
| `padding` | `impl Into<Padding>` | Padding |
| `style` | `Style` | Base style |
| `header_style` | `Style` | Month/year header style |
| `weekday_style` | `Style` | Weekday name row style |
| `day_style` | `Style` | Regular day style |
| `day_hover_style` | `Style` | Day hover style |
| `extend_day_hover_style` / `inherit_day_hover_style` | `Style` / `()` | Extend or inherit the hover theme role for day cells |
| `selected_style` | `Style` | Selected day style |
| `outside_month_style` | `Style` | Adjacent-month day style |
| `nav_style` | `Style` | Navigation button style |
| `nav_hover_style` | `Style` | Navigation button hover |
| `extend_nav_hover_style` / `inherit_nav_hover_style` | `Style` / `()` | Extend or inherit the hover theme role for navigation buttons |
| `nav_disabled_style` | `Style` | Disabled navigation style |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `on_select` | `Callback<(i32, u32, u32)>` | Day selected (year, month, day) |
| `on_prev_month` | `Callback<()>` | Previous month navigation |
| `on_next_month` | `Callback<()>` | Next month navigation |
