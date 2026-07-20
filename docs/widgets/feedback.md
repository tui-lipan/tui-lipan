# Feedback & Status Widgets

## ProgressBar

Visual progress indicator with multiple styles and optional drag interaction.

| Prop | Type | Description |
|------|------|-------------|
| `progress` | `f64` | **Constructor** - progress 0.0–1.0 |
| `progress_style` | `ProgressStyle` | Visual style variant |
| `show_percentage` | `bool` | Show percentage text |
| `percentage_position` | `ProgressTextPosition` | Where to show percentage |
| `label` | `String` | Custom label text shown in addition to percentage |
| `label_position` | `ProgressTextPosition` | Where to show label text |
| `label_style` | `Style` | Label style |
| `filled_style` | `Style` | Filled portion style (default: green) |
| `filled_gradient` | `ColorGradient` | Filled portion gradient |
| `empty_style` | `Style` | Empty portion style (default: dark gray) |
| `block_empty_bg_dim` | `f32` | How far the empty block track recedes toward the surface behind the bar (`0.0` = full fill color, `1.0` = indistinguishable from the background) |
| `style` | `Style` | Base style |
| `hover_style` | `Style` | Hover style (when draggable) |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `focus_style` | `Style` | Focus style |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `padding` | `impl Into<Padding>` | Padding |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `draggable` | `bool` | Allow drag to change value |
| `step` | `f64` | Drag step increment |
| `inverted` | `bool` | Flip fill direction |
| `focusable` | `bool` | Accept focus |
| `target` | `Option<f64>` | Optional marker position |
| `target_style` | `Style` | Marker style |
| `target_symbol` | `char` | Marker character |
| `zones` | `Vec<ProgressZone>` | Threshold zones with custom styles |
| `on_change` | `Callback<f64>` | Value changed (when draggable) |
| `on_click` | `Callback<f64>` | Clicked position |

**Progress styles**: `Block` (default), `Line`, `LineDotted`, `Dots`, `Arrow`, `Rect`, `Braille`, `Custom { filled: char, empty: char }`.

**Text positions**: `Left`, `Right` (default), `Above`, `Below`, `Middle` (`Middle` only works with `ProgressStyle::Block`). Percentage and label text share one line when both use the same `Above`, `Below`, or `Middle` position.

```rust
ProgressBar::new(0.67)
    .progress_style(ProgressStyle::Block)
    .show_percentage(true)
    .filled_style(Style::new().fg(Color::Green))
    .target(0.8)
    .target_style(Style::new().fg(Color::Yellow))
    .zones(vec![
        ProgressZone::new(0.75).style(Style::new().fg(Color::Yellow)),
        ProgressZone::new(0.90).style(Style::new().fg(Color::Red)),
    ])
```

---

## Spinner

Animated loading indicator.

| Prop | Type | Description |
|------|------|-------------|
| `spinner_style` | `SpinnerStyle` | Animation style variant |
| `speed` | `SpinnerSpeed` | Animation speed |
| `frame` | `Option<usize>` | Manual frame index (controlled mode) |
| `label` | `String` | Label text next to spinner |
| `gap` | `u16` | Space between spinner and label |
| `style` | `Style` | Spinner style |
| `label_style` | `Style` | Label style |
| `width` | `Length` | Width |
| `height` | `Length` | Height |

**Animation styles**: `Dots` (default), `Line`, `Circle`, `Arc`, `Braille`, `Moon`, `Box`, `Bar`, `Arrow`, `Fade`, `Trail`, `Earth`, `Claude`, `OpenCode`, `ThreeDot`, `ThreeDotFade`, `SquareFade`, `Lightsaber`.

**Speeds**: `Slow`, `Normal` (default), `Fast`, `Custom { frame_ms }`.

```rust
Spinner::new()
    .spinner_style(SpinnerStyle::Dots)
    .speed(SpinnerSpeed::Normal)
    .label("Loading...")
    .style(Style::new().fg(Color::Cyan))

// Controlled mode (advance manually via .tick())
Spinner::new().frame(Some(ctx.state.spinner_frame))
```

---

## StatusBar

Application status line with left/center/right slots.

| Prop / Method | Type | Description |
|---------------|------|-------------|
| `.left(Element)` | method | Left slot content |
| `.center(Element)` | method | Center slot content |
| `.right(Element)` | method | Right slot content |
| `style` | `Style` | Bar container style (default for all slots) |
| `left_style` | `Style` | Left slot style patch |
| `center_style` | `Style` | Center slot style patch |
| `right_style` | `Style` | Right slot style patch |
| `padding` | `impl Into<Padding>` | Bar padding |
| `gap` | `u16` | Slot gap |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `reserve_center_space` | `bool` | Keep an empty center slot in the spacing model |
| `loading` | `bool` | Show loading spinner |
| `loading_label` | `String` | Loading label text |
| `loading_style` | `Style` | Loading indicator style |
| `loading_spinner_style` | `SpinnerStyle` | Loading spinner variant |
| `loading_spinner_speed` | `SpinnerSpeed` | Loading spinner speed |

**Layout behavior**: StatusBar is a convenience widget with content-aware
left/center/right slots. When center content exists, the center slot is pinned to
the center; side slots are allocated from the actual center width, keep the
configured gap around the center, and clip at that boundary. Without center
content, the default remains `left + spacer + right`; in this default path,
single-sided bars omit the empty side lane. Set `reserve_center_space(true)` to
keep an empty center slot in the spacing model for compatibility; it does not
imply equal-third lanes.

```rust
StatusBar::new()
    .style(Style::new().bg(Color::DarkGray))
    .left_style(Style::new().fg(Color::Green))
    .left(Text::new("MODE: NORMAL").into())
    .right(Text::new("ln 42, col 8").into())

// RSX
rsx! {
    StatusBar {
        style: Style::new().bg(Color::DarkGray),
        left: Text { content: "Mode: Normal" }
        right: Badge { content: "v1.0" }
    }
}
```

---

## PaginationBar

Composable pagination controls for `PaginationState` with button-style personalization.

| Prop / Method | Type | Description |
|---------------|------|-------------|
| `PaginationBar::new(state)` | constructor | Controlled state source |
| `labels` | `PaginationLabels` | Set all nav labels (`first/prev/next/last`) |
| `first_label` / `prev_label` / `next_label` / `last_label` | `String` | Override individual nav labels |
| `show_first_last` | `bool` | Show/hide first/last buttons |
| `show_range_info` | `bool` | Show/hide `(rows x-y of total)` in center label |
| `gap` | `u16` | Horizontal spacing between controls |
| `button_variant` | `ButtonVariant` | Shared variant for all nav buttons |
| `button_border_style` | `BorderStyle` | Outlined button border style |
| `button_style` | `Style` | Base nav button style |
| `button_hover_style` | `Style` | Hover nav button style |
| `button_focus_style` | `Style` | Focus nav button style |
| `button_disabled_style` | `Style` | Disabled nav button style |
| `button_overrides_for` | `PaginationAction + PaginationButtonOverrides` | Per-button style override |
| `first_button_overrides` / `prev_button_overrides` / `next_button_overrides` / `last_button_overrides` | `PaginationButtonOverrides` | Per-button style override helpers |
| `info_style` | `Style` | Center text style |
| `info_formatter` | `Fn(PaginationInfo) -> Arc<str>` | Custom center text formatter |
| `on_action` | `Callback<PaginationAction>` | Emits `First/Prev/Next/Last` |

```rust
PaginationBar::new(self.pagination)
    .button_variant(ButtonVariant::Outlined)
    .button_border_style(BorderStyle::Rounded)
    .button_style(Style::new().fg(Color::Cyan))
    .next_button_overrides(
        PaginationButtonOverrides::new().style(Style::new().fg(Color::Green)),
    )
    .info_formatter(|info| {
        Arc::from(format!(
            "Pg {} / {}  ·  {}..{} / {}",
            info.page_number,
            info.total_pages,
            if info.total_items == 0 { 0 } else { info.start + 1 },
            info.end,
            info.total_items,
        ))
    })
    .button_hover_style(Style::new().fg(Color::White).bg(Color::Blue))
    .button_focus_style(Style::new().fg(Color::Black).bg(Color::Yellow).bold())
    .on_action(ctx.link().callback(Msg::Navigate))
```

Use `PaginationState` for data windowing (`range()`, `next_page()`, `set_per_page()`, etc.) and pair it with `PaginationBar` for UI controls.

---

## Breadcrumb

Navigation trail.

| Prop | Type | Description |
|------|------|-------------|
| `segments` | `Vec<Arc<str>>` | Path segments |
| `separator` | `char` | Separator character (default: `/`) |
| `gap` | `u16` | Gap between segments |
| `active` | `Option<usize>` | Currently active segment index |
| `style` | `Style` | Base style |
| `active_style` | `Style` | Active segment style |
| `inactive_style` | `Style` | Inactive segment style |
| `hover_style` | `Style` | Hover style |
| `separator_style` | `Style` | Separator style |
| `align` | `Align` | Alignment |
| `justify` | `Justify` | Justification |
| `padding` | `impl Into<Padding>` | Padding |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `on_select` | `Callback<usize>` | Segment selected |

---

## Badge

Small status indicator overlaid on an element.

| Prop | Type | Description |
|------|------|-------------|
| `content` | `impl Into<String>` | **Constructor** - badge text |
| `child` | `Element` | Element to attach badge to |
| `style` | `Style` | Badge background/text style |
| `text_style` | `Style` | Badge text style |
| `border` | `bool` | Badge border |
| `border_style` | `BorderStyle` | Badge border style |
| `padding` | `impl Into<Padding>` | Badge padding |
| `offset` | `(i16, i16)` | Position offset from corner |
| `position` | `BadgePosition` | Corner to attach to |
| `width` | `Length` | Badge width |
| `height` | `Length` | Badge height |

```rust
Badge::new("New")
    .style(Style::new().fg(Color::White).bg(Color::Red))
    .position(BadgePosition::TopRight)
    .child(button_element)
```
