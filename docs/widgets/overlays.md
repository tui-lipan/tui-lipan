# Overlays & Navigation Widgets

## Overlay Z-Ordering

Overlays are rendered above the main content at the root level:

1. **Modal** - lowest (backdrop covers entire screen)
2. **Popover** - middle
3. **Toast** - highest (always visible)

---

## Modal

Centered dialog overlay. Portals to root level regardless of declaration location.
Root-portal modals capture keyboard focus and backdrop pointer routing; if the
modal content has no focusable descendants, focus is suspended until the modal is
dismissed. Use `OverlayScope::Local` only for inline composition without full
modal focus/backdrop semantics.

| Prop | Type | Description |
|------|------|-------------|
| `title` | `impl Into<String>` | **Constructor** - dialog title |
| `child` | `Element` | Dialog content |
| `scope` | `OverlayScope` | `RootPortal` (default) or `Local` |
| `on_close` | `Callback<()>` | Close callback (Esc/backdrop click) |
| `width` | `Length` | Dialog width |
| `height` | `Length` | Dialog height |
| `max_height` | `Length` | Cap the modal height; pair with `height(Length::Auto)` so the modal hugs its content but never exceeds the cap (inner content scrolls past it) |
| `reserve_height` | `Length` | For `RootPortal` modals: center as if the modal were this tall, then top-align it in that band, so the top edge stays fixed as content grows and shrinks. Positions only — content taller than the band extends past its bottom |
| `backdrop_style` | `Style` | Backdrop overlay style |
| `frame_style` | `Style` | Dialog container style |
| `focus_style` | `Style` | Dialog frame style while the modal or a descendant holds focus |
| `extend_focus_style` | `Style` | Extend the themed dialog frame focus style |
| `inherit_focus_style` | — | Restore the themed dialog frame focus style |
| `border_style` | `BorderStyle` | Dialog border |
| `padding` | `impl Into<Padding>` | Dialog inner padding |
| `title_style` | `Style` | Title style |

```rust
if ctx.state.show_confirm {
    Modal::new("Confirm Delete")
        .child(
            VStack::new()
                .gap(1)
                .child(Text::new("This action cannot be undone."))
                .child(
                    HStack::new().gap(1)
                        .child(Button::new("Cancel")
                            .on_click(ctx.link().callback(|_| Msg::CancelDelete)))
                        .child(Button::new("Delete")
                            .style(Style::new().fg(Color::White).bg(Color::Red))
                            .on_click(ctx.link().callback(|_| Msg::ConfirmDelete)))
                )
        )
        .on_close(ctx.link().callback(|_| Msg::CancelDelete))
        .into()
}
```

**Content-hugging modals with a stable top** - a modal whose content grows and
shrinks (for example a `SearchPalette` filtered as the user types) can hug its
content while staying capped, without drifting toward the vertical center as it
shrinks:

```rust
Modal::new("Commands")
    .height(Length::Auto)              // hug the visible rows
    .reserve_height(Length::Percent(50)) // center a 50%-tall band; top-align in it
    .max_height(Length::Percent(75))   // ...but never grow past 75% of the viewport
    .padding(0)
    .child(palette)
```

Without `reserve_height`, the modal re-centers by its actual height, so a
shrinking palette drifts upward toward the middle each keystroke. With it, the
overlay is centered as if it were `reserve_height` tall, then the content is
top-aligned within that reserved band, fixing the top edge at
`(viewport - reserve_height) / 2`. Has no effect in `OverlayScope::Local`.

`reserve_height` **positions**; `max_height` **bounds**. They are independent, so
content taller than the band keeps the same top edge and extends past the band's
bottom — as above, a modal anchored a quarter of the way down the viewport that
may grow to 75% of it. Give a modal `reserve_height` without a `max_height` and
it can run off the bottom of the screen.

---

## Toast Notifications

Toasts use `ctx.toast()` - no view tree setup required.

**App configuration:**

```rust
App::new()
    .toast_placement(ToastPlacement::BottomEnd)  // default
    .toast_gap(1)
    .mount(Root)
    .run()
```

**Showing toasts:**

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::SaveSuccess => {
            ctx.toast().push(Toast::new("Saved successfully!"));
            Update::full()
        }
        Msg::SaveError(e) => {
            ctx.toast().push(
                Toast::new(format!("Save failed: {e}"))
                    .title("Error")
                    .border(true)
            );
            Update::full()
        }
        Msg::ShowCustom => {
            let id = ctx.toast().push(
                Toast::new("Custom message")
                    .duration(10.0)
                    .border(true)
            );
            ctx.state.toast_id = Some(id);  // Store to dismiss later
            Update::full()
        }
        Msg::DismissToast => {
            if let Some(id) = ctx.state.toast_id.take() {
                ctx.toast().dismiss(id);
            }
            Update::none()
        }
    }
}
```

**ToastHandle methods:**

| Method | Description |
|--------|-------------|
| `.push(Toast)` | Show toast, returns `OverlayId` |
| `.dismiss(id)` | Dismiss a specific toast |
| `.clear()` | Clear all toasts |

**ToastPlacement:** `TopStart`, `TopCenter`, `TopEnd`, `BottomStart`, `BottomCenter`, `BottomEnd` (default).

**Toast props:**

| Prop | Type | Description |
|------|------|-------------|
| `message` | `impl Into<String>` | **Constructor** - toast text |
| `duration` | `f32` | Auto-dismiss seconds (0 = permanent) |
| `copyable` | `bool` | Allow copying `message` by right-clicking the toast; bordered toasts also show a copy glyph by default |
| `copy_affordance` | `ToastCopyAffordance` | Optional visual copy control (`BorderGlyph` or `None`) |
| `title` | `String` | Optional title |
| `title_prefix` | `String` | Title prefix symbol |
| `title_suffix` | `String` | Title suffix |
| `title_alignment` | `Align` | Title alignment |
| `title_style` | `Style` | Title style |
| `message_style` | `Style` | Message style |
| `frame_style` | `Style` | Container style |
| `border` | `bool` | Show border |
| `border_style` | `BorderStyle` | Border style |
| `padding` | `impl Into<Padding>` | Padding |
| `width` | `Length` | Width |
| `max_width` | `Length` | Maximum width |
| `wrap` | `bool` | Wrap long messages |
| `decoration` / `decorations` | `FrameDecoration` | Edge decorations |

Use `Toast::new("...").copyable(true)` for error strings, paths, command output, or IDs that
users may need to paste elsewhere. The copied text is the toast message only. Right-click anywhere
inside a copyable toast to copy it; left-click keeps the existing dismiss behavior. Bordered toasts
also render a copy glyph by default. Use `.copy_affordance(ToastCopyAffordance::None)` to keep the
right-click copy behavior without showing the glyph. Successful toast copies briefly apply
`ClipboardConfig::copy_feedback_style` for `copy_feedback_duration_ms`, matching selection-copy
feedback.

> Toasts are suppressed in inline mode to avoid terminal history corruption.

---

## Popover

Floating content panel triggered by an element.

| Prop | Type | Description |
|------|------|-------------|
| `trigger` | `Element` | The trigger element |
| `content` | `Element` | Popover content |
| `open` | `bool` | Controlled open state |
| `scope` | `OverlayScope` | `RootPortal` (default) or `Local` |
| `on_close` | `Callback<()>` | Close callback |
| `placement` | `PopoverPlacement` | `Above`, `Below`, `Left`, `Right` + start/center/end variants |
| `offset` | `u16` | Distance from trigger |
| `clamp` | `bool` | Keep within screen bounds |
| `auto_flip` | `bool` | Flip placement when out of bounds |
| `min_trigger_width` | `bool` | Keep popover at least as wide as the trigger; content may still make it wider |
| `fit_trigger_width` | `bool` | Force popover width to exactly match the trigger, unless capped by `max_width` |
| `max_width` | `Length` | Cap the resolved popover width; percent resolves against overlay bounds |
| `anchor` | `Option<(u16, u16)>` | Absolute content-coordinate anchor instead of trigger rect |

`Popover` renders through the root overlay pipeline by default, so it appears above normal in-tree content. Use `.scope(OverlayScope::Local)` when it should stay inside parent stacking order, such as an autocomplete attached to content that can be covered by an inline sidebar layer.

By default, `Popover` uses `.min_trigger_width(true)`: the overlay is at least as wide as its trigger but can grow wider for long content. Use `.fit_trigger_width(true)` for exact trigger width, or `.max_width(...)` to cap content-driven growth.

---

## Tooltip

Help text on hover or focus.

| Prop | Type | Description |
|------|------|-------------|
| `text` | `impl Into<String>` | **Constructor** - tooltip text |
| `child` | `Element` | The element to add tooltip to |
| `open` | `bool` | Controlled open state |
| `auto` | `bool` | Auto-show on hover/focus |
| `text_style` | `Style` | Tooltip text style |
| `container_style` | `Style` | Tooltip container style |
| `border` | `bool` | Show border |
| `border_style` | `BorderStyle` | Border style |
| `padding` | `impl Into<Padding>` | Inner padding |
| `placement` | `PopoverPlacement` | Tooltip placement |
| `offset` | `u16` | Distance from element |
| `clamp` | `bool` | Keep within screen bounds |
| `auto_flip` | `bool` | Flip when out of bounds |

```rust
Tooltip::new("This button saves your work")
    .auto(true)
    .placement(PopoverPlacement::Top)
    .child(
        Button::new("Save")
            .on_click(ctx.link().callback(|_| Msg::Save))
            .into()
    )
```

---

## Accordion

Collapsible content sections.

| Prop | Type | Description |
|------|------|-------------|
| `exclusive` | `bool` | Only one section open at a time |
| `gap` | `u16` | Gap between sections |
| `padding` | `impl Into<Padding>` | Outer padding |
| `border` | `bool` | Section border |
| `border_style` | `BorderStyle` | Section border style |
| `style` | `Style` | Container style |
| `header_style` | `Style` | Header idle style |
| `header_hover_style` | `Style` | Header hover style |
| `header_focus_style` | `Style` | Header focus style |
| `header_padding` | `impl Into<Padding>` | Header padding |
| `content_padding` | `impl Into<Padding>` | Content area padding |
| `content_border` | `bool` | Content area border |
| `content_style` | `Style` | Content area style |
| `expanded_icon` | `char` | Expanded section icon |
| `collapsed_icon` | `char` | Collapsed section icon |
| `disabled_style` | `Style` | Style when disabled |
| `focusable` | `bool` | Whether headers participate in focus traversal |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `on_toggle` | `Callback<AccordionEvent>` | Section toggle event |

```rust
Accordion::new()
    .exclusive(true)
    .item(AccordionItem::new(
        "Section 1",
        Text::new("Content for section 1").into()
    ))
    .item(AccordionItem::new(
        "Section 2",
        Text::new("Content for section 2").into()
    ))
```

---

## SearchPalette

Fuzzy search widget powered by `nucleo`. Composes an `Input` and `List` into a filterable, keyboard-navigable search panel.

SearchPalette is **not** an overlay by itself - wrap it in `Modal` for the classic command-palette experience, or embed it inline in a `Frame`, sidebar, or any other container.

---

## CommandPalette

`CommandPalette` is a composite overlay widget that reads commands from `ctx.command_registry()` and renders them through `SearchPalette<CommandId>` inside a `Modal`.

Use `ctx.register_command(...)` inside a component to register component-scoped commands, or `ctx.command_registry().register(...)` for app-wide commands.

| Prop | Type | Description |
|------|------|-------------|
| `on_close` | `Callback<()>` | Fired when the modal closes or a command executes |
| `show_disabled` | `bool` | Include disabled commands in results (muted and non-activating) |
| `title` | `impl Into<RichText>` | Modal title |
| `width` | `Length` | Modal width |
| `height` | `Length` | Modal height |
| `scope` | `OverlayScope` | `RootPortal` (default) or `Local` |
| `backdrop_style` | `Style` | Backdrop overlay style |
| `frame_style` | `Style` | Modal frame style |
| `border` | `bool` | Show modal border |
| `border_style` | `BorderStyle` | Modal border style |
| `padding` | `impl Into<Padding>` | Modal content padding |
| `title_style` | `Style` | Title style |
| `title_alignment` | `Align` | Title alignment |

```rust
fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
    let link = ctx.link().clone();
    ctx.register_command(
        CommandEntry::builder("app.toggle-wrap")
            .label("Toggle word wrap")
            .description("Enable or disable editor wrapping")
            .category("Application")
            .keybinding_hint("p")
            .handler(Callback::new(move |_| link.send(Msg::ToggleWrap)))
            .build(),
    );
    None
}

fn view(&self, ctx: &Context<Self>) -> Element {
    if ctx.state.show_palette {
        CommandPalette::new()
            .title("Commands")
            .show_disabled(true)
            .on_close(ctx.link().callback(|_| Msg::ClosePalette))
            .into()
    } else {
        Element::empty()
    }
}
```

Command ids are open-ended (`CommandId`) and can be grouped with optional categories and right-aligned keybinding hints.

Items can be provided flat via `.items()` or grouped via `.entries()` using `SearchEntry::item(...)`, `SearchEntry::header(...)`, and `SearchEntry::spacer()`.

Headers and spacers are **display-only** rows (not searchable/selectable). Group rendering rules:

- With an empty query, all item rows are shown and headers/spacers render in entry order.
- With a non-empty query, grouped chrome is hidden and matches render as a flat ranked list.

Item text is typically built from `SearchItem::new(label, value)` and optional
`.description("...")`. By default, description renders inline as
`label - description` and uses `description_style`.

You can also add hidden aliases with `SearchItem::alias(...)` or
`SearchItem::aliases(...)`. Aliases are searched and scored like the label,
but they are never rendered, which makes them useful for abbreviations,
legacy names, or alternate command titles.

You can also mark rows active with `.active(true)` on `SearchItem` or `SearchEntry::item(...)`.

### Core props

| Prop | Type | Description |
|------|------|-------------|
| `items` | `impl IntoIterator<Item = SearchItem<T>>` | Flat searchable items (clears entries) |
| `entries` | `impl IntoIterator<Item = SearchEntry<T>>` | Grouped entries via item/header/spacer rows |
| `sync_match_limit` | `usize` | Max item count that still matches synchronously (default: `100`) |
| `sync_selection` | `bool` | Keep `on_select` synced with the current visible row |
| `initial_query` | `impl Into<Arc<str>>` | Pre-populate search field |
| `initial_selected_item_index` | `Option<usize>` | Start selection on this `items` index when it appears in results (else first row) |
| `placeholder` | `impl Into<Arc<str>>` | Input placeholder (default: `"Search..."`) |
| `width` | `Length` | Requested palette width (default: `Flex(1)`) |
| `height` | `Length` | Requested palette height (default: `Flex(1)`) |
| `max_width` | `Length` | Maximum palette width constraint |
| `max_height` | `Length` | Maximum palette height constraint |

### Callbacks

| Prop | Type | Description |
|------|------|-------------|
| `on_query_change` | `Callback<Arc<str>>` | Fired when the query text changes |
| `on_select` | `Callback<SearchEvent<T>>` | Fired when selection moves; with `sync_selection(true)` also fires for initial/result-driven selection |
| `on_activate` | `Callback<SearchEvent<T>>` | Fired on Enter or double-click |

### Input forwarding

| Prop | Type | Description |
|------|------|-------------|
| `input_prefix` | `impl Into<Arc<str>>` | Prefix before query text (default: `" "`) |
| `input_suffix` | `impl Into<Arc<str>>` | Suffix after query text (default: `"{matches}/{total}"`) |
| `input_border` | `bool` | Show input border |
| `input_divider` | `bool` | Render divider below input (uncontrolled mode, default: `true`) |
| `input_divider_style` | `Style` | Divider style below input |
| `input_divider_join_frame` | `bool` | Join divider with surrounding frame border (default: `true`) |
| `input_caret_shape` | `CaretShape` | Input caret shape (`Block`, `Bar`, `Underline`) |
| `input_caret_color` | `Color` | Input caret color (OSC 12 cursor color, terminal support required) |
| `input_border_style` | `BorderStyle` | Input border style |
| `input_padding` | `impl Into<Padding>` | Input padding |
| `input_style` | `Style` | Input base style |
| `input_hover_style` | `Style` | Input hover style |
| `input_focus_style` | `Style` | Input focus style |
| `input_placeholder_style` | `Style` | Placeholder style |
| `input_focus_placeholder_style` | `Style` | Placeholder style when focused |
| `input_prefix_style` | `Style` | Prefix style |
| `input_focus_prefix_style` | `Style` | Prefix style when focused |
| `input_suffix_style` | `Style` | Suffix style |
| `input_focus_suffix_style` | `Style` | Suffix style when focused |

### List forwarding

Forwarded list state-style setters use [StyleSlot semantics](../styling.md#state-style-slots):
`list_selection_style`, `list_item_hover_style`, and `list_active_style` replace
theme roles; use matching `extend_list_*_style` / `inherit_list_*_style` methods
to extend or inherit the scoped theme roles.

| Prop | Type | Description |
|------|------|-------------|
| `list_border` | `bool` | Show list border |
| `list_border_style` | `BorderStyle` | List border style |
| `list_padding` | `impl Into<Padding>` | List padding |
| `list_style` | `Style` | List base style |
| `list_hover_style` | `Style` | List hover style |
| `list_selection_style` | `Style` | Selected item style |
| `extend_list_selection_style` / `inherit_list_selection_style` | `Style` / `()` | Extend or inherit the selection theme role instead of replacing it |
| `list_unfocused_selection_style` | `Style` | Selected item style while list is not focused; defaults to `list_selection_style` |
| `extend_list_unfocused_selection_style` / `inherit_list_unfocused_selection_style` | `Style` / `()` | Extend or inherit the unfocused selection theme role instead of replacing it |
| `list_selection_symbol` | `impl Into<Arc<str>>` | Selection indicator (default: `"> "`) |
| `list_selection_symbol_style` | `Style` | Selection indicator style |
| `list_unfocused_selection_symbol_style` | `Style` | Selection indicator style while list is not focused; defaults to `list_selection_symbol_style` |
| `list_symbol_column` | `bool` | Control whether the internal list reserves and renders its symbol/status column |
| `list_unselected_symbol` | `impl Into<Arc<str>>` | Non-selected item indent |
| `list_selection_full_width` | `bool` | Extend selection to full width |
| `list_item_hover_style` | `Style` | Individual item hover style |
| `extend_list_item_hover_style` / `inherit_list_item_hover_style` | `Style` / `()` | Extend or inherit the item hover theme role instead of replacing it |
| `list_active_style` | `Style` | Active item style |
| `extend_list_active_style` / `inherit_list_active_style` | `Style` / `()` | Extend or inherit the active item theme role instead of replacing it |
| `list_active_symbol` | `impl Into<Arc<str>>` | Active item symbol |
| `list_active_symbol_style` | `Style` | Active symbol style |
| `list_item_horizontal_padding` | `impl Into<Padding>` | Normal row padding (left/right used) |
| `list_header_horizontal_padding` | `impl Into<Padding>` | Header row padding (left/right used) |
| `list_focusable` | `bool` | Allow list keyboard focus (default: `true`) |
| `list_scrollbar` | `bool` | Show scrollbar |
| `list_scrollbar_config` | `ScrollbarConfig` | Full scrollbar configuration (variant, gap, thumb, thumb styles) |
| `empty_text` | `impl Into<Arc<str>>` | Empty state text (default: `"No matches"`) |
| `empty_text_style` | `Style` | Empty state text style |

> `list_item_horizontal_padding` and `list_header_horizontal_padding` accept `Padding`, but only `left` and `right` are applied by `List`.

> SearchPalette forwards the same `ListConfig` symbol/gutter fields as `List`:
> use `list_symbol_column(bool)` to control the status/selection symbol column,
> and configure `gutter_gap` / non-selectable gutter participation with
> `list_config(ListConfig::new().gutter_gap(n).gutter_for_non_selectable(true))`.
> Gutters reserve across participating rows; the default gap is `0`.

### Item rendering

| Prop | Type | Description |
|------|------|-------------|
| `item_style` | `Style` | Item label base style |
| `header_style` | `Style` | Group header row style for entries created with `SearchEntry::header(...)` |
| `description_style` | `Style` | Item description style |
| `description_placement` | `DescriptionPlacement` | Description placement: `Inline`, `Right`, `Above`, `Below` |
| `description_selection` | `bool` | Whether selection highlight applies to description text |
| `description_overflow` | `DescriptionOverflow` | Description overflow policy: `Truncate` or `Wrap` (`Wrap` applies to `Above`/`Below`) |
| `match_style` | `Style` | Matched character style |
| `show_scores` | `bool` | Show numeric match scores |
| `score_gradient` | `ColorGradient` | Gradient for score coloring |
| `score_range` | `(u64, u64)` | Explicit score range for gradient |
| `render_item` | `SearchRenderer<T>` | Custom item renderer (`Arc<dyn Fn>`) |
| `item_status` | `SearchStatusRenderer<T>` | Add per-item content in the existing list symbol column |
| `item_gutter` | `SearchGutterRenderer<T>` | Add a per-item left gutter while keeping default row rendering |

> `description_selection(false)` has no effect with `DescriptionPlacement::Inline` and `DescriptionPlacement::Right`, because selection/hover styling applies to the whole primary row in those modes.

> `description_selection` also controls description hover styling when `list_item_hover_style` is set.

> `description_overflow(DescriptionOverflow::Wrap)` affects only `DescriptionPlacement::Above` and `DescriptionPlacement::Below`; inline and right placement keep single-row truncation behavior.

> `header_style` applies only to generated group headers. Use `list_header_horizontal_padding` for header row padding.

> `ItemDescription::right(...)` is trailing metadata: it is bounded and truncated before the primary label, so long right-side hints do not displace item labels in narrow palettes.

Use `item_status(...)` for symbol-column status indicators without replacing the
default renderer:

```rust
SearchPalette::new()
    .items(items)
    .item_status(Arc::new(|item, _highlight| {
        is_working(&item.value).then(|| Spinner::new().into())
    }))
```

Use `item_gutter(...)` when a row-local leading adornment needs its own extra
column instead of the selection/status symbol column:

```rust
SearchPalette::new()
    .items(items)
    .item_gutter(Arc::new(|item, _highlight| {
        is_working(&item.value).then(|| Spinner::new().into())
    }))
```

### Matching config

| Prop | Type | Description |
|------|------|-------------|
| `case_matching` | `CaseMatching` | Case sensitivity (default: `Smart`) |
| `normalization` | `Normalization` | Unicode normalization (default: `Smart`) |

> Matching uses synchronous updates for lists up to `sync_match_limit`
> items and off-thread `nucleo` searches for larger lists; items do not need
> `Send`/`Sync`.

**Standalone ranking** - `rank_search_palette_indices(&[SearchItem<T>], query)` returns each item’s index in the source slice ordered like the palette’s fuzzy results (smart case matching and normalization). Use `rank_search_palette_indices_with_score(..., |index, item, score| ...)` when another signal should boost or demote matched items before final ordering; `NaN` adjusted scores rank after finite scores. Use these helpers when another widget owns the query/focus but you need the same ordering for keyboard selection.

**As a modal overlay** - wrap in `Modal` and set `on_close`/size there:

```rust
if ctx.state.show_palette {
    let palette = SearchPalette::<Arc<str>>::new()
        .entries(vec![
            SearchEntry::header("Sources"),
            SearchEntry::item("src/lib.rs", Arc::from("src/lib.rs"))
                .description("Crate root"),
            SearchEntry::header("Examples"),
            SearchEntry::item("examples/demo.rs", Arc::from("examples/demo.rs")),
        ])
        .list_scrollbar(true)
        .list_selection_full_width(true)
        .list_item_hover_style(Style::new().bg(Color::DarkGray))
        .on_activate(ctx.link().callback(Msg::Activated));

    Modal::new("Open File")
        .child(palette)
        .width(Length::Px(60))
        .height(Length::Px(20))
        .border_style(BorderStyle::Rounded)
        .padding(0)
        .on_close(ctx.link().callback(|_| Msg::ClosePalette))
}
```

**Inline** - embed directly without Modal:

```rust
Frame::new()
    .title("Search")
    .border(true)
    .child(SearchPalette::<Arc<str>>::new().items(my_items))
```

---

## ContextMenu

A popup menu backed by `Popover` + `List`. Typically triggered by right-click or a keyboard shortcut.

| Prop | Type | Default | Description |
|------|------|---------|-------------|
| `trigger` | `impl IntoElement` | **Constructor** | The element that owns the menu |
| `items` | `impl IntoIterator<Item = impl Into<ListItem>>` | `[]` | Menu items |
| `open` | `bool` | `false` | Controlled open state |
| `on_select` | `Callback<usize>` | - | Fires with selected item index |
| `on_close` | `Callback<()>` | - | Fires when menu should close |
| `anchor` | `Option<(u16, u16)>` | `None` | Absolute anchor position (content coordinates) |
| `placement` | `PopoverPlacement` | `BelowStart` | Menu placement relative to trigger |
| `offset` | `impl Into<PopoverOffset>` | `0` | Distance from trigger |
| `clamp` | `bool` | `true` | Keep within screen bounds |
| `auto_flip` | `bool` | `true` | Flip placement when out of bounds |
| `width` | `Length` | `Px(20)` | Menu width |
| `height` | `Length` | `Auto` | Menu height |
| `border` | `bool` | `true` | Show border |
| `border_style` | `BorderStyle` | `Plain` | Border style |
| `padding` | `impl Into<Padding>` | default | Inner padding |
| `style` | `Style` | default | Base style |
| `selection_style` | `Style` | default | Selected item style |
| `extend_selection_style` / `inherit_selection_style` | `Style` / `()` | - | Extend or inherit the selection theme role instead of replacing it |
| `unfocused_selection_style` | `Style` | - | Selected item style while menu list is not focused; defaults to `selection_style` |
| `extend_unfocused_selection_style` / `inherit_unfocused_selection_style` | `Style` / `()` | - | Extend or inherit the unfocused selection theme role instead of replacing it |
| `item_hover_style` | `Style` | - | Hovered item style |
| `extend_item_hover_style` / `inherit_item_hover_style` | `Style` / `()` | - | Extend or inherit the hover theme role instead of replacing it |
| `selection_symbol` | `Option<impl Into<Arc<str>>>` | `"> "` | Selection indicator |
| `selection_symbol_style` | `Style` | - | Selection indicator style |
| `unfocused_selection_symbol_style` | `Style` | - | Selection indicator style while menu list is not focused; defaults to `selection_symbol_style` |
| `scrollbar` | `bool` | `false` | Show scrollbar |
| `scrollbar_config` | `ScrollbarConfig` | default | Scrollbar configuration |

```rust
ContextMenu::new(
    Button::new("Options").on_click(ctx.link().callback(|_| Msg::ToggleMenu))
)
    .items(vec!["Cut", "Copy", "Paste", "Delete"])
    .open(ctx.state.menu_open)
    .on_select(ctx.link().callback(Msg::MenuAction))
    .on_close(ctx.link().callback(|_| Msg::CloseMenu))
    .selection_style(Style::new().bg(Color::DarkGray))
```
