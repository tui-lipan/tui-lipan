# Tab Widgets

## Tabs

Horizontal tab bar with clickable tabs.

State-style setters use [StyleSlot semantics](../styling.md#state-style-slots):
`hover_style`, `focus_style`, `tab_hover_style`, and `active_style` replace theme
roles; use matching `extend_*_style` / `inherit_*_style` methods to extend or
inherit scoped theme roles.

| Prop | Type | Description |
|------|------|-------------|
| `tabs` | `Vec<Tab>` | Tab items |
| `tab(Tab)` | method | Add a single tab |
| `active` | `usize` | Active tab index |
| `divider` | `char` | Separator between tabs |
| `border` | `bool` | Show border |
| `border_style` | `BorderStyle` | Border style |
| `padding` | `impl Into<Padding>` | Padding |
| `style` | `Style` | Bar idle style |
| `focus_style` | `Style` | Bar focus style |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `hover_style` | `Style` | Bar hover style |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `tab_hover_style` | `Style` | Individual tab hover style |
| `extend_tab_hover_style` / `inherit_tab_hover_style` | `Style` / `()` | Extend or inherit the tab hover theme role instead of replacing it |
| `active_style` | `Style` | Active tab style |
| `extend_active_style` / `inherit_active_style` | `Style` / `()` | Extend or inherit the active-tab theme role instead of replacing it |
| `disabled` | `bool` | Disable all tabs |
| `disabled_style` | `Style` | Style when disabled |
| `focusable` | `bool` | Accept focus |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `on_change` | `Callback<TabsEvent>` | Active tab changed |
| `on_click` | `Callback<TabsEvent>` | Tab clicked |
| `on_key` | `KeyHandler` | Key handler |

```rust
Tabs::new()
    .tabs(vec![
        Tab::new("Editor"),
        Tab::new("Terminal"),
        Tab::new("Log"),
    ])
    .active(self.active_tab)
    .border(true)
    .active_style(Style::new().fg(Color::Cyan).bold())
    .on_change(ctx.link().callback(|e: TabsEvent| Msg::TabChanged(e.index)))
```

**Tab construction:**

```rust
Tab::new("Label")
    .style(Style::new().fg(Color::White))
    .disabled(false)
```

**`TabsEvent` fields:** `index: usize`

---

## DraggableTabBar

Editor-style tab bar with drag reordering, per-tab close buttons, file icons, and cross-bar transfer.

| Prop | Type | Description |
|------|------|-------------|
| `tabs` | `Vec<DraggableTab>` | Tab items |
| `tab(DraggableTab)` | method | Add a single tab |
| `active` | `usize` | Active tab index |
| `on_action` | `Callback<DraggableTabActionEvent>` | Action tab clicked |
| `variant` | `DraggableTabBarVariant` | `Bordered` or `FrameLine` |
| `draggable` | `bool` | Enable drag reordering |
| `drag_preview` | `bool` | Floating tab label near the pointer while dragging (default: `true`) |
| `reorder_mode` | `DragReorderMode` | `Live` or `OnDrop` |
| `drag_threshold` | `u16` | Pixels before drag starts |
| `show_close_buttons` | `bool` | Show close buttons |
| `close_symbol` | `&str` | Close button symbol |
| `close_on_hover_only` | `bool` | Show close only on hover |
| `tab_max_width` | `Option<u16>` | Maximum tab label width in cells |
| `overflow` | `DraggableTabBarOverflow` | `Scroll` or opt-in `ShrinkThenScroll { min_tab_width }` |
| `scroll_wheel` | `bool` | Mouse wheel to scroll tabs |
| `show_overflow_controls` | `bool` | Show `<` `>` overflow buttons |
| `overflow_style` | `Style` | Overflow button style |
| `overflow_hover_style` | `Style` | Overflow button hover style |
| `scroll_offset` | `usize` | Controlled scroll offset |
| `show_file_icons` | `bool` | Show file type icons |
| `file_icon_style` | `FileIconStyle` | Icon style (`Nerd`, `NerdColored`, `Emoji`) |
| `file_icon_palette` | `FileIconPalette` | Custom icon colors |
| `file_icon_override` | `HashMap<Arc<str>, FileIconOverride>` | Per-extension overrides |
| `bar_id` | `&str` | Bar identifier for drag groups |
| `drag_group` | `&str` | Group name for cross-bar transfer |
| `style` | `Style` | Bar idle style |
| `focus_style` | `Style` | Bar focus style |
| `hover_style` | `Style` | Bar hover style |
| `tab_hover_style` | `Style` | Hover style for inactive tabs (active tab keeps `active_style`) |
| `active_style` | `Style` | Active tab style (takes priority over hover) |
| `close_style` | `Style` | Close button style |
| `close_hover_style` | `Style` | Close button hover style |
| `divider` | `char` | Tab separator character |
| `accent_symbol` | `char` | Left/right accent character |
| `active_accent_symbol` | `char` | Active tab accent |
| `accent_style` | `Style` | Accent style |
| `active_accent_style` | `Style` | Active tab accent style |
| `border` | `bool` | Show border |
| `border_style` | `BorderStyle` | Border style |
| `padding` | `impl Into<Padding>` | Padding |
| `disabled` | `bool` | Disable all tabs |
| `disabled_style` | `Style` | Style when disabled |
| `focusable` | `bool` | Accept focus |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `on_change` | `Callback<TabsEvent>` | Active tab changed |
| `on_close` | `Callback<DraggableTabCloseEvent>` | Close button clicked |
| `on_reorder` | `Callback<DraggableTabReorderEvent>` | Tab dragged to new position |
| `on_transfer` | `Callback<DraggableTabTransferEvent>` | Tab transferred to another bar |
| `on_click` | `Callback<MouseEvent>` | Raw tab bar click |
| `on_key` | `KeyHandler` | Key handler |

### Tab Construction

```rust
DraggableTab::new("main.rs")
    .closeable(true)
    .style(Style::new())
    .active_style(Style::new().fg(Color::Black).bg(Color::Cyan).bold())
    .hover_style(Style::new().bg(Color::indexed(238)))
    .accent_style(Style::new().fg(Color::indexed(244)))
    .active_accent_style(Style::new().fg(Color::Cyan).bold())
    .path("src/main.rs")         // For file icon auto-lookup
    .icon(Span::new("󰙱 ").fg(Color::Red))  // Manual icon override
    .right_badge(Span::new("M").fg(Color::Yellow))  // Git status marker
```

`DraggableTab::active_style(...)` patches over the bar's `active_style` for
that tab, so global active defaults can still provide fields such as
background while the tab overrides foreground or modifiers. `hover_style(...)`
patches over `DraggableTabBar::tab_hover_style(...)` for inactive tabs. Active
tabs keep active styling and do not receive hover styling.
For the `FrameLine` variant, `accent_style(...)` and
`active_accent_style(...)` patch over the bar accent styles for this tab's
left marker.

Create pinned action tabs for tab-strip buttons such as `+`:

```rust
DraggableTabBar::new()
    .tabs(files.iter().map(to_file_tab))
    .tab(DraggableTab::action("+").style(Style::new().fg(Color::LightGreen).bold()))
    .active(active_file_index)
    .on_change(ctx.link().callback(|e| Msg::SelectTab(e.index)))
    .on_action(ctx.link().callback(|_| Msg::CreateTab));
```

Action tabs render in the strip but do not emit `on_change`, become active,
show close buttons, or participate as same-bar reorder targets. A trailing
action tab stays pinned while normal tabs reorder before it.

To show a spinner in the icon position (replaces icon and file-icon when set):

```rust
DraggableTab::new("Session 1")
    .leading(
        Spinner::new()
            .spinner_style(SpinnerStyle::Dots)
            .speed(SpinnerSpeed::Normal)
            .style(Style::new().fg(Color::Cyan))
            .into()
    )
```

`Spinner` label and layout properties are ignored in tabs because the tab owns
its label and sizing.

The same slot also accepts `Text` elements for static fallbacks:

```rust
DraggableTab::new("Session 1")
    .leading(Text::new("⋯").style(Style::new().fg(Color::Cyan)).into())
```

The spinner glyph occupies the same slot as the icon - its width is determined
by `SpinnerStyle::width()`. Tab spinners animate automatically with the app
runtime. Spinner/text content takes priority over `icon` and file icons.

For attention-grabbing tabs, compose these same pieces on the tab item so the
state travels with reorder and cross-bar transfer:

```rust
DraggableTab::new("build")
    .leading(
        Spinner::new()
            .spinner_style(SpinnerStyle::Claude)
            .speed(SpinnerSpeed::Fast)
            .style(Style::new().fg(Color::Yellow).bold())
            .into()
    )
    .right_badge(Span::new("!").fg(Color::Yellow).bold())
    .active_style(Style::new().fg(Color::Black).bg(Color::Yellow).bold())
    .active_accent_style(Style::new().fg(Color::Yellow).bold())
```

If a tab needs custom blinking or pulsing, drive that from component state with
a timer command and toggle the tab's `style(...)` or `active_style(...)` during
`view()`. `DraggableTabBar` intentionally keeps the tab item as the owner of
attention state rather than adding index-based alert styling.

### Events

```rust
// TabsEvent { index: usize }
// DraggableTabActionEvent { index: usize }
// DraggableTabCloseEvent { index: usize }
// DraggableTabReorderEvent { from: usize, to: usize }
// DraggableTabTransferEvent { from_bar: String, to_bar: String, from: usize, to: usize }
```

### File Icons

```rust
DraggableTabBar::new()
    .show_file_icons(true)
    .file_icon_style(FileIconStyle::NerdFontColored)
    .file_icon_palette(FileIconPalette { /* custom colors */ })
```

`FileIconStyle` variants: `NerdFont`, `NerdFontColored`, `Emoji`.

File icon is auto-resolved from `DraggableTab::path(...)`. Use `file_icon_override` for per-extension customization.

### Overflow

By default, `DraggableTabBar` keeps natural tab widths and scrolls horizontally
when the tab strip overflows. To keep more tabs visible, opt into shrinking
labels before scrolling:

```rust
DraggableTabBar::new()
    .overflow(DraggableTabBarOverflow::ShrinkThenScroll { min_tab_width: 8 })
```

Tabs shrink only after their natural widths no longer fit the bar. If all tabs
still cannot fit at `min_tab_width`, horizontal scrolling and overflow controls
work as usual. Fixed tab affordances such as icons, badges, and close buttons
are preserved even when they exceed the configured minimum.

### Cross-Bar Drag (Drag Groups)

```rust
// Left editor bar
DraggableTabBar::new()
    .bar_id("left-editor")
    .drag_group("editors")
    .tabs(self.left_tabs.clone())
    .on_transfer(ctx.link().callback(Msg::TransferFromLeft))

// Right editor bar
DraggableTabBar::new()
    .bar_id("right-editor")
    .drag_group("editors")
    .tabs(self.right_tabs.clone())
    .on_transfer(ctx.link().callback(Msg::TransferFromRight))
```

Tabs can be dragged between bars that share the same `drag_group`. The `on_transfer` callback fires on the **source** bar and reports `from_bar`, `to_bar`, `from` index, and `to` index.

### Full Example

```rust
rsx! {
    DraggableTabBar {
        tabs: vec![
            DraggableTab::new("main.rs").closeable(true).path("src/main.rs"),
            DraggableTab::new("lib.rs").closeable(true).path("src/lib.rs"),
            DraggableTab::new("README.md").path("README.md"),
        ],
        bar_id: "editor",
        drag_group: "editors",
        active: self.active_tab,
        variant: DraggableTabBarVariant::FrameLine,
        tab_max_width: Some(20),
        overflow: DraggableTabBarOverflow::ShrinkThenScroll { min_tab_width: 8 },
        show_file_icons: true,
        file_icon_style: FileIconStyle::NerdFontColored,
        show_close_buttons: true,
        show_overflow_controls: true,
        active_style: Style::new().fg(Color::Cyan).bold(),
        on_change: ctx.link().callback(|e| Msg::SetActive(e.index)),
        on_close: ctx.link().callback(Msg::CloseTab),
        on_reorder: ctx.link().callback(Msg::ReorderTabs),
    }
}
```

### Image Chip Bar Pattern

Use `DraggableTabBar` as an image attachment chip bar above `TextArea`:

```rust
DraggableTabBar::new()
    .tabs(images.iter().enumerate().map(|(i, _)| {
        DraggableTab::new(format!("Image {}", i + 1))
            .closeable(true)
    }))
    .active(usize::MAX)       // No active tab (no highlight)
    .close_symbol("x")
    .draggable(false)
    .focusable(false)
    .on_close(ctx.link().callback(Msg::RemoveImage))
    .height(Length::Auto)
```
