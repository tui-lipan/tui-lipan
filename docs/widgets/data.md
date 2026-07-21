# Data Widgets

## List

Selectable list of items with optional headers, spacers, prefixes, left gutters, and scrollbar.

State-style setters use [StyleSlot semantics](../styling.md#state-style-slots):
`selection_style` / `active_style` replace theme roles, while
`extend_selection_style` / `extend_active_style` patch over them and
`inherit_selection_style` / `inherit_active_style` delegate to the theme.

| Prop | Type | Description |
|------|------|-------------|
| `items` | `impl Iterator<Item = ListItem>` | List items |
| `selected` | `Option<usize>` | Selected index; pass `None` for no current row (no selection highlight). Bare integers still work (`list.selected(0)`) |
| `scroll_keys` | `bool` | Enable keyboard scroll keys |
| `scroll_wheel` | `bool` | Enable mouse wheel |
| `scrollbar` | `bool` | Show scrollbar |
| `scrollbar_config` | `ScrollbarConfig` | Full scrollbar configuration (variant, gap, thumb, thumb styles) |
| `show_scroll_indicators` | `bool` | Show top/bottom overflow indicators |
| `scroll_indicator_style` | `Style` | Overflow indicator style |
| `border` | `bool` | Draw border |
| `title` | `String` | Border title |
| `padding` | `impl Into<Padding>` | Inner padding |
| `empty_text` | `String` | Text when list is empty |
| `empty_text_style` | `Style` | Empty text style |
| `active_style` | `Style` | Style for rows where `ListItem::active(true)` |
| `extend_active_style` / `inherit_active_style` | `Style` / `()` | Extend or inherit the active-row theme role instead of replacing it |
| `active_symbol` | `Option<impl Into<Arc<str>>>` | Prefix symbol for active rows |
| `active_symbol_position` | `ListSymbolPosition` | Render active symbol on the left or immediately after the label |
| `active_symbol_style` | `Style` | Style for active symbol |
| `selection_symbol` | `Option<String>` | Prefix for selected item (e.g. `"> "`) |
| `selection_symbol_right` | `Option<String>` | Trailing symbol after the selected item's label; pair with `selection_symbol` for "pill" caps. Shares `selection_symbol_style` |
| `symbol_column` | `bool` | Enable reservation/rendering for the built-in selection/status symbol column |
| `selection_style` | `Style` | Selected item style |
| `extend_selection_style` / `inherit_selection_style` | `Style` / `()` | Extend or inherit the selection theme role instead of replacing it |
| `unfocused_selection_style` | `Style` | Selected item style while list is not focused; defaults to `selection_style` |
| `extend_unfocused_selection_style` / `inherit_unfocused_selection_style` | `Style` / `()` | Extend or inherit the unfocused selection theme role instead of replacing it |
| `selection_full_width` | `bool` | Extend selection to full width |
| `unfocused_selection_symbol_style` | `Style` | Selection symbol style while list is not focused; defaults to `selection_symbol_style` |
| `gutter_gap` | `u16` | Cells between row-local gutters and labels; default `0`, opt into spacing with `.gutter_gap(n)` |
| `gutter_for_non_selectable` | `bool` | Include non-selectable rows (headers/spacers) in gutter reservation/alignment |
| `item_horizontal_padding` | `impl Into<Padding>` | Left/right padding for normal rows (top/bottom ignored) |
| `header_horizontal_padding` | `impl Into<Padding>` | Left/right padding for header rows (top/bottom ignored) |
| `focusable` | `bool` | Accept focus |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `on_select` | `Callback<ListEvent>` | Selection changed |
| `on_item_click` | `Callback<ListEvent>` | Row clicked with mouse |
| `on_activate` | `Callback<ListEvent>` | Item activated (Enter/double-click) |
| `on_scroll_to` | `Callback<usize>` | Scroll position changed |

### ListItem Types

```rust
ListItem::new("Normal item")          // Selectable row
ListItem::header("Section Title")      // Non-selectable header row
ListItem::spacer()                     // Non-selectable blank row
ListItem::new("Service").active(true)  // Marks row as active
ListItem::role(ListItemRole::Header)   // Explicit role

// Multi-line rows
ListItem::new("build")
    .line(ListItemLine::new("target/debug/build.log").selection_left(false))

// Prefix helpers
ListItem::new("Item").numbered(1)
ListItem::new("Bullet").bulleted('•')
ListItem::new("Label")
    .prefix("> ")
    .prefix_style(Style::new().fg(Color::Cyan))

// Left gutter helpers. Spinner gutters animate with the app's spinner ticker.
// Set List::gutter_gap(1) when the framework should provide label spacing.
ListItem::new("Building").gutter(Spinner::new())
ListItem::new("Changed").gutter(ListItemGutter::text("~ "))

// Status helpers render inside the existing selection/unselected symbol column.
ListItem::new("Working").status_spinner(Spinner::new())
ListItem::new("Dirty").status_symbol(" ~ ")

// Symbol/gutter can be rendered on a non-primary line (useful for "description above")
ListItem::new("description")
    .line(ListItemLine::new("label"))
    .symbol_line(1)
    .gutter_line(1)
```

Keyboard/mouse selection and activation skip non-selectable rows (`Header`/`Spacer`).

`ListItem::line(...)` adds extra visual lines under the primary line. Selection,
activation, and callbacks still use the item index (not visual line index).

`ListItem::prefix(...)` renders a prefix before the primary label and automatically
indents extra lines to match the label start. Use `.extra_line_indent(...)` to override
that alignment when needed.

`ListItem::gutter(...)` is the canonical row-local leading adornment: use it for
per-row markers, icons, spinners, or badges that need their own column before the
label. Gutters reserve a consistent left gutter column across participating rows
so labels stay aligned even when only some rows have gutter content. The default
gap between the gutter and label is `0`; opt into spacing with `.gutter_gap(n)`.
By default, only selectable item rows participate, keeping headers left-aligned;
use `.gutter_for_non_selectable(true)` when headers/spacers should reserve the
same gutter width.

`ListItem::status(...)`, `.status_symbol(...)`, and `.status_spinner(...)` render
inside the existing list symbol column. Status is symbol-column content, not a
separate row gutter. Active symbols keep priority, then row status, then selected
symbols, then the unselected symbol/spaces. Use this for one-column row state
such as a busy spinner.

`List::symbol_column(true)` enables the built-in status/selection symbol column;
the reserved width still comes from configured selection/unselected/active
symbols or row status content. Use `List::symbol_column(false)` when row-local
gutters provide the leading markers and the built-in column should not measure
or render.

### ListConfig

Composite widgets such as `Select`, `ComboBox`, `MultiSelect`, and
`SearchPalette` forward shared list chrome through `ListConfig`. In addition to
the convenience setters on those widgets, the struct carries these fields for
builder-style or typed configuration:

| Field | Type | Description |
|-------|------|-------------|
| `border` | `bool` | Whether to draw a border around the inner list |
| `border_style` | `BorderStyle` | Inner list border style |
| `padding` | `Padding` | Inner list padding |
| `style` | `Style` | Inner list base style |
| `selection_style` | `StyleSlot` | Selected/active row style |
| `unfocused_selection_style` | `StyleSlot` | Selected row style while the list is unfocused |
| `item_hover_style` | `Option<StyleSlot>` | Per-row hover style; `None` lets the host widget apply its own default (several fall back to `selection_style`) |
| `selection_full_width` | `bool` | Extend selection across the row width |
| `selection_symbol` | `Option<Arc<str>>` | Selected-row symbol content |
| `selection_symbol_right` | `Option<Arc<str>>` | Trailing selected-row symbol (right "pill" cap); shares `selection_symbol_style` |
| `selection_symbol_style` | `Option<Style>` | Selected-row symbol style |
| `unfocused_selection_symbol_style` | `Option<Style>` | Selected-row symbol style while unfocused |
| `symbol_column` | `bool` | Reserve the list symbol/status column |
| `gutter_gap` | `u16` | Gap between row-local gutters and labels; default `0` |
| `gutter_for_non_selectable` | `bool` | Whether headers/spacers participate in gutter reservation |
| `item_horizontal_padding` | `Padding` | Left/right padding for normal rows (interior to the highlight) |
| `header_horizontal_padding` | `Padding` | Left/right padding for header rows |
| `empty_text_style` | `Style` | Style for the empty-state placeholder text |
| `scrollbar` | `bool` | Show a vertical scrollbar when content overflows |
| `scrollbar_config` | `ScrollbarConfig` | Full scrollbar configuration |

Prefer `ListConfig::new()`/`Default::default()` with builder methods over struct
literals so newly added fields do not break app code.

> `selection_symbol` (e.g. `"> "`) is prepended to the selected item. Unselected items are padded with spaces to maintain alignment. Include a trailing space in the symbol if needed.

> **Pill / capsule selection:** pair `selection_symbol` (left cap) with `selection_symbol_right` (right cap). Color both caps via `selection_symbol_style` with foreground equal to the selection background and background equal to the row/terminal background, e.g. `selection_symbol(Some(""))`, `selection_symbol_right(Some(""))`, `selection_symbol_style(Style::new().fg(sel_bg).bg(row_bg))`. The trailing cap renders only on the selected row and always closes the **right edge of the highlighted region**. The highlight spans the full row width only when you opt in with `selection_full_width(true)` (or when right-aligned content must be pushed to the edge); otherwise it hugs the content — `item_horizontal_padding` stays interior to the highlight and does **not** force a full-width bar. So `selection_full_width(false)` gives a tight capsule around the label (plus any padding), and `selection_full_width(true)` gives a full-width capsule with the cap at the row's right edge. A right-positioned `active_symbol` shares this slot and takes priority when both apply.

> Active row rendering is independent from selection. Use `active_symbol_position(ListSymbolPosition::Right)` to render the active marker after the label instead of in the left symbol column. When using the right position, include any desired separator in the symbol itself (for example `" ✓"`).

> `item_horizontal_padding` and `header_horizontal_padding` accept `Padding`, but only `left`/`right` are applied in `List`.

> **File icons in a plain list:** the Nerd Font icon resolvers `FileTree` uses are exposed via `tui_lipan::utils::{file_icon, file_icon_span, directory_icon, directory_icon_span}`, so you can prefix list rows with themed file/folder icons without reimplementing the mapping. Pass a `FileIconPalette` (e.g. `theme.file_icons`): `ListItem::from_spans([file_icon_span(name, &palette), Span::new(format!(" {name}"))])` for files, or `directory_icon_span(expanded, &palette)` for folders (bare glyph only; no disclosure arrow).

> **Pointer vs keyboard row hover:** For `List` and `Table`, when `item_hover_style` is non-empty, changing `selected` from the keyboard or from component logic (not a row click) stops using the mouse position for per-row hover until the pointer moves. Widget-level hover and row clicks behave as usual. **`Tree` uses an inner `List`** with the same `item_hover_style` prop, so it follows the same rules automatically.

```rust
List::new()
    .items(self.files.iter().map(|f| ListItem::new(f.name.clone())))
    .selected(self.selected)
    .scrollbar(true)
    .selection_symbol(Some("> ".to_string()))
    .selection_style(Style::new().fg(Color::Cyan).bold())
    .on_select(ctx.link().callback(|e| Msg::FileSelected(e.index)))
    .on_activate(ctx.link().callback(|e| Msg::FileOpened(e.index)))
```

---

## Table

Structured data with rows, columns, and optional scrollbar.

| Prop | Type | Description |
|------|------|-------------|
| `header` | `TableRow` | Column header row |
| `rows` | `Arc<[TableRow]>` / `rows_arc` | Data rows (`rows(...)` collects into `Arc`; prefer `rows_arc` when sharing) |
| `widths` | `Vec<ColumnWidth>` | Column widths |
| `selected` | `Option<usize>` | Selected row index |
| `column_spacing` | `u16` | Space between columns |
| `row_gap` | `u16` | Blank terminal rows between rendered table rows |
| `scroll_keys` | `bool` | Keyboard scroll |
| `scroll_wheel` | `bool` | Mouse wheel |
| `scrollbar` | `bool` | Scrollbar |
| `scrollbar_config` | `ScrollbarConfig` | Full scrollbar configuration (variant, gap, thumb, thumb styles) |
| `show_scroll_indicators` | `bool` | Overflow indicators |
| `scroll_indicator_style` | `Style` | Indicator style |
| `selection_symbol` | `Option<String>` | Selected row prefix |
| `selection_style` | `Style` | Selected row style |
| `extend_selection_style` / `inherit_selection_style` | `Style` / `()` | Extend or inherit the selection theme role instead of replacing it |
| `header_style` | `Style` | Header row style |
| `row_style` | `Style` | Default row style |
| `alternating_row_style` | `Style` | Style applied to odd rows for zebra striping |
| `column_style` | `(usize, Style)` | Style a zero-based column; applies to header and data cells |
| `column_styles` | `impl IntoIterator<Item = Style>` | Set zero-based per-column styles in order |
| `row_style_at` | `(usize, Style)` | Style a zero-based absolute data row; does not affect the header |
| `row_styles` | `impl IntoIterator<Item = Style>` | Set zero-based per-data-row styles in order |
| `row_style_full_width` | `bool` | Extend row hover/selection/zebra style across full row width |
| `focusable` | `bool` | Accept focus |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `on_select` | `Callback<TableEvent>` | Row selection changed |
| `on_activate` | `Callback<TableEvent>` | Row activated |
| `on_scroll_to` | `Callback<usize>` | Scroll position |

Table style precedence for body cells is: alternating row style, then
`TableRow::style`, indexed row style (`row_style_at` / `row_styles`), column
style (`column_style` / `column_styles`), `TableCell::style`, and finally
state styles such as hover/selection/disabled. Header cells use header styling
plus column and cell styles; indexed row styles only target data rows. Full-row
backgrounds from `row_style_full_width(true)` use row-level styles, not column
or cell styles.

### Column Widths

```rust
ColumnWidth::Fixed(10)   // Fixed cell width
ColumnWidth::Fill(1)     // Proportional fill
ColumnWidth::Min(5)      // Minimum width, fills remaining
```

### Row Sizing

```rust
TableRow::new(vec!["col1", "col2"])
    .height(2)           // Fixed height
    .auto_height()       // Height = max line count of cells
    .bottom_margin(1)    // Spacing below row

Table::new()
    .row_gap(1)          // Global blank rows between rendered rows
```

`Table::row_gap(gap)` inserts a global number of blank terminal rows between
rendered table rows. It applies between the header and first data row only when
data rows exist, and between data rows, but never after the final data row. This
global gap is additive with per-row `TableRow::bottom_margin`: use `row_gap` for
consistent table-wide spacing, and `bottom_margin` for row-specific extra space.

### Heatmap Cells

```rust
TableCell::heat_fg(value, &gradient, GradientRange::new(0.0, 100.0))  // Color fg by value
TableCell::heat_bg(value, &gradient, GradientRange::new(0.0, 100.0))  // Color bg by value
```

### Inspector Patterns

```rust
Table::inspector(true)   // Enable inspector presets

// Row helpers
TableRow::key_value("Name", "Alice")
TableRow::section("Personal Info")
TableRow::separator()

// Hierarchy
TableRow::new(cells)
    .depth(2)
    .disclosure(TableDisclosureState::Expanded)
```

Inspector styling hooks: `inspector_key_style`, `inspector_value_style`, `inspector_section_style`, `inspector_separator_style`, `inspector_indent_size`, `inspector_disclosure_symbols`, `inspector_separator_char`.

Row semantics: `TableRowRole::{Normal, Section, Separator}`.

```rust
Table::new()
    .header(TableRow::new(vec!["ID", "Name", "Status"]))
    .rows(self.data.iter().map(|d| {
        TableRow::new(vec![d.id.to_string(), d.name.clone(), d.status.clone()])
    }).collect())
    .widths(vec![ColumnWidth::Fixed(5), ColumnWidth::Fill(1), ColumnWidth::Fixed(10)])
    .alternating_row_style(Style::new().bg(Color::indexed(236)))
    .row_style_full_width(true)
    .selected(Some(self.selected))
    .selection_style(Style::new().fg(Color::Cyan))
    .on_select(ctx.link().callback(|e| Msg::RowSelected(e.index)))
```

---

## Tree

Hierarchical tree view with expand/collapse.

| Prop | Type | Description |
|------|------|-------------|
| `root` | `TreeNode` | **Constructor** - root node |
| `selected` | `usize` | Controlled selected visible row index |
| `clear_selection` | `bool` | When `true`, suppress the selection highlight (authoritative over `selected` and internal state) |
| `force_scroll_to_selected` | `bool` | Force the internal list to reveal the selected row on next render |
| `gap` | `u16` | Vertical gap between items |
| `icon_gap` | `u16` | Gap between icon and label |
| `show_icons` | `bool` | Show expand/collapse icons |
| `expanded_icon` | `String` | Icon for expanded nodes |
| `collapsed_icon` | `String` | Icon for collapsed nodes |
| `leaf_icon` | `String` | Icon for leaf nodes |
| `icon_style` | `Style` | Icon style |
| `indent_style` | `IndentStyle` | Indent guide glyph variant: `None`, `Line`, `Short`, `Long`, `ShortRounded`, or `LongRounded` |
| `indent_guide_style` | `Style` | Vertical indent guide |
| `indent_gradient` | `ColorGradient` | Gradient for indent depth |
| `style` | `Style` | Base style |
| `hover_style` | `Style` | Hover style |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `item_hover_style` | `Style` | Individual item hover |
| `extend_item_hover_style` / `inherit_item_hover_style` | `Style` / `()` | Extend or inherit the item hover theme role instead of replacing it |
| `selection_style` | `Style` | Selected item style |
| `extend_selection_style` / `inherit_selection_style` | `Style` / `()` | Extend or inherit the selection theme role instead of replacing it |
| `unfocused_selection_style` | `Style` | Selected item style while tree is not focused; defaults to `selection_style` |
| `extend_unfocused_selection_style` / `inherit_unfocused_selection_style` | `Style` / `()` | Extend or inherit the unfocused selection theme role instead of replacing it |
| `selection_symbol` | `String` | Selected item prefix |
| `selection_symbol_style` | `Style` | Prefix style |
| `unfocused_selection_symbol_style` | `Style` | Prefix style while tree is not focused; defaults to `selection_symbol_style` |
| `scrollbar` | `bool` | Scrollbar |
| `scrollbar_config` | `ScrollbarConfig` | Full scrollbar configuration (variant, gap, thumb, thumb styles) |
| `scroll_keys` | `bool` | Keyboard scroll |
| `scroll_wheel` | `bool` | Mouse wheel |
| `empty_text` | `String` | Text when tree is empty |
| `empty_text_style` | `Style` | Empty text style |
| `focusable` | `bool` | Accept focus |
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
| `activate_on_click` | `bool` | Single-click activates |
| `keymap` | `TreeKeymap` | Keyboard expand/collapse mapping |
| `focus_policy` | `FocusAccordion` | Tree accordion behavior |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `on_select` | `Callback<TreeEvent>` | Node selected |
| `on_toggle` | `Callback<TreeToggleEvent>` | Node expanded/collapsed |

`Tree` is implemented with an inner `List` (flattened visible rows). Per-row `item_hover_style` and **pointer vs keyboard row hover** behavior match `List` - see the callout under [List](#list).

`IndentStyle` controls connector glyphs: `None` disables guides, `Line` uses
`│`, `Short` uses `├`/`└`, `Long` uses `├─`/`└─`, `ShortRounded` uses
`├`/`╰`, and `LongRounded` uses `├─`/`╰─`.

### Building Nodes

```rust
TreeNode::new("parent")
    .expanded(true)
    .child(
        TreeNode::new("child-1")
    )
    .child(
        TreeNode::new("child-2")
            .child(TreeNode::new("grandchild"))
    )

// With styled ListItem
TreeNode::new(ListItem::from_spans(vec![
    Span::new("file.rs").fg(Color::Cyan),
    Span::new(" [modified]").fg(Color::Yellow),
]))
```

### Events

```rust
// TreeEvent { index: usize, path: Vec<usize> }
// TreeToggleEvent { index: usize, path: Vec<usize>, expanded: bool }

.on_select(ctx.link().callback(|e: TreeEvent| Msg::NodeSelected(e.path)))
.on_toggle(ctx.link().callback(|e: TreeToggleEvent| Msg::NodeToggled(e.path, e.expanded)))
```

**Keymap**: `TreeKeymap` supports expand/collapse via `Left`/`Right`, `h`/`l`, `Space` (toggle).

---

## FileTree

Lazy-loading filesystem explorer built on `Tree`, with git-backed or
application-provided change projections.

| Prop | Type | Description |
|------|------|-------------|
| `root` | `impl Into<Arc<str>>` | **Constructor** - root directory path |
| `show_hidden` | `bool` | Show hidden files (`.` prefix) |
| `max_entries_per_dir` | `usize` | Cap entries per directory |
| `directory_label_style` | `Style` | Style applied to directory names |
| `file_label_style` | `Style` | Style applied to regular file names |
| `path_style` | `(path, FileTreeItemStyle)` | Apply row/icon/label/suffix styles to one exact path |
| `path_styles` | `impl IntoIterator<Item = (path, FileTreeItemStyle)>` | Apply exact path-specific item styles in bulk |
| `git_status` | `bool` | Show change status badges for git/provided changes (default: true) |
| `highlight_changed_labels` | `bool` | Also apply change status colors to file/directory labels (default: false) |
| `change_suffix_style` | `Style` | Style only the right-side change metadata suffix, such as status markers and diff stats |
| `change_suffix_priority` | `FileTreeSuffixPriority` | Whether labels or right-side change metadata are preserved first when rows are narrow |
| `change_source` | `FileTreeChangeSource` | Change metadata source; defaults to local git status/diff data |
| `change_view` | `FileTreeChangeView` | `AllFiles` (default) or `ChangedOnly` source-agnostic view |
| `show_diff_stats` | `bool` | Show `+N -M` diff stats next to change markers |
| `git_suffix_style` | `Style` | Compatibility setter for styling only right-side git metadata |
| `git_suffix_priority` | `FileTreeSuffixPriority` | Compatibility setter for right-side git metadata truncation priority |
| `git_view` | `FileTreeGitView` | `AllFiles` (default) or `ChangedOnly` git-focused view |
| `git_changed_only` | `bool` | Compatibility convenience setter for changed-only mode |
| `git_diff_stats` | `bool` | Compatibility setter for `+N -M` diff stats next to change markers |
| `git_refresh_token` | `u64` | Token to trigger deterministic git refresh |
| `selected` | `usize` | Controlled selected visible row index |
| `clear_selection` | `bool` | When `true`, suppress the selection highlight (authoritative over `selected` and internal state) |
| `selected_path` | `impl Into<Arc<str>>` | Controlled selection by absolute path under the root or path relative to the root, when the row is visible |
| `reveal_path` | `impl Into<Arc<str>>` | Expand/load ancestors for an absolute or root-relative path when possible |
| `select_path` | `impl Into<Arc<str>>` | Reveal and select a path, forcing the tree to scroll to the row when visible |
| `force_scroll_to_selected` | `bool` | Force the tree to reveal the selected row on next render |
| `expanded_paths` | `impl IntoIterator<Item = impl Into<Arc<str>>>` | Controlled expanded directory paths; the root path is kept expanded automatically |
| `directory_icon` | `String` | Directory icon |
| `file_icon` | `String` | File icon |
| `symlink_icon` | `String` | Symlink icon |
| `other_icon` | `String` | Other entry icon |
| `explorer` | `bool` | Show fuzzy search input |
| `explorer_placeholder` | `String` | Search input placeholder |
| `explorer_prefix` | `String` | Search input prefix |
| `explorer_input_border` | `bool` | Search input border |
| `explorer_match_style` | `Style` | Fuzzy match highlight |
| `explorer_divider` | `bool` | Show divider between search and tree |
| `focusable` | `bool` | Accept focus |
| `tab_stop` | `bool` | Include the tree target in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Tree focus gained / lost |
| `on_select` | `Callback<FileTreeEvent>` | File/dir selected |
| `on_toggle` | `Callback<FileTreeToggleEvent>` | Directory expanded/collapsed |

Plus all `Tree` styling/scrolling props, including `indent_style` and `scrollbar_config`.

**Behavior:**
- Directories load on demand in a background command on first expand.
- Git is the default change source; use `FileTreeChangeSource::Provided(...)` to display backend-provided change data without requiring a local git repository.
- `directory_label_style(...)` and `file_label_style(...)` style names independently from icons and right-aligned change indicators.
- `path_style(...)` / `path_styles(...)` match exact paths and can override row, icon, label, and suffix styling for reviewed, pinned, or otherwise annotated files.
- `change_suffix_style(...)` and `git_suffix_style(...)` style only right-side metadata such as `M +30 -21`, leaving icons and labels unchanged.
- `change_suffix_priority(FileTreeSuffixPriority::Suffix)` keeps right-side metadata visible first on narrow rows, truncating labels before suffixes.
- Change colors apply to the right-aligned indicators by default; use `highlight_changed_labels(true)` to also color dirty file and directory names, layered over their label styles.
- `FileTreeChangeView::ChangedOnly` shows changed files only, grouped by ancestor directories. With provided change data this projection is virtual/source-agnostic and can include nonexistent or deleted paths supplied by the backend.
- `FileTreeGitView` is a compatibility alias for `FileTreeChangeView`; `git_view`, `git_changed_only`, and `git_diff_stats` remain available for git-focused call sites.
- Changed-only mode respects `show_hidden`; paths under hidden components such as `.github/` appear only when hidden entries are enabled.
- `show_diff_stats(true)` displays numeric diff stats when the selected change source provides them; untracked and binary files may show a status marker without `+N -M` counts.
- Filesystem fuzzy matching respects `.gitignore`/`.ignore` rules.
- In changed-only mode, fuzzy matching is scoped to the changed-path projection instead of the whole filesystem.
- Auto-expands ancestor directories to reveal search matches.
- `selected_path`, `reveal_path`, and `select_path` normalize absolute paths under the root or paths relative to the root. They are no-ops for paths outside the root, paths hidden by `show_hidden(false)`, absent/unreadable/capped entries, or rows filtered out by the current all-files/changed-only projection. `selected_path` only selects an already-visible row; `reveal_path` expands/loads ancestors when possible; `select_path` combines reveal + selection and scrolls to the selected row. With controlled `expanded_paths`, app-provided expansion remains authoritative, so reveal/select can only display rows made available by the controlled expansion set plus the reveal request during rendering.
- Restores pre-search expansion state when query clears.
- Queries containing file extensions (e.g. `layout.rs`) prioritize filename matches.

```rust
let changes = vec![
    FileTreeChange::new("src/main.rs", FileTreeChangeStatus::Modified)
        .kind(FileKind::File)
        .diff_stat(12, 3)
        .staged(true),
    FileTreeChange::new("docs/removed.md", FileTreeChangeStatus::Deleted)
        .kind(FileKind::File),
];

FileTree::new(project_root)
    .change_source(FileTreeChangeSource::Provided(changes))
    .change_view(FileTreeChangeView::ChangedOnly)
    .show_diff_stats(true)
    .path_style(
        "src/main.rs",
        FileTreeItemStyle::new()
            .row(Style::new().fg(Color::DarkGray))
            .icon(Style::new().fg(Color::DarkGray))
            .label(Style::new().fg(Color::DarkGray)),
    )
    .change_suffix_style(Style::new().fg(Color::Yellow))
    .change_suffix_priority(FileTreeSuffixPriority::Suffix)
```

```rust
FileTree::new("/home/user/projects")
    .git_status(true)
    .change_view(FileTreeChangeView::ChangedOnly)
    .show_diff_stats(true)
    .show_hidden(false)
    .explorer(true)
    .explorer_placeholder("Filter files...")
    .on_select(ctx.link().callback(|e: FileTreeEvent| Msg::FileSelected(e.path)))
```

### Events

```rust
// FileTreeEvent { path: Arc<str>, kind: FileKind }
// FileTreeToggleEvent { path: Arc<str>, kind: FileKind, expanded: bool }
```

---

## LogView

High-throughput log list with level highlighting and fuzzy filtering.

| Prop | Type | Description |
|------|------|-------------|
| `buffer` | `Arc<LogBuffer>` | Bounded ring buffer of log entries |
| `filter_mode` | `MatchMode` | `Fuzzy`, `Substring`, `Exact` |
| `case_sensitive` | `bool` | Case-sensitive filtering |
| `auto_follow` | `bool` | Auto-scroll to newest entry |
| `paused` | `bool` | Pause log streaming display |
| `trace_style` | `Style` | TRACE level style |
| `debug_style` | `Style` | DEBUG level style |
| `info_style` | `Style` | INFO level style |
| `warn_style` | `Style` | WARN level style |
| `error_style` | `Style` | ERROR level style |
| `unfocused_selection_style` | `Style` | Selected row style while log view is not focused; defaults to `selection_style` |
| `extend_unfocused_selection_style` / `inherit_unfocused_selection_style` | `Style` / `()` | Extend or inherit the unfocused selection theme role instead of replacing it |
| `on_select` | `Callback<LogViewEvent>` | Entry selected |
| `on_activate` | `Callback<LogViewEvent>` | Entry activated |

Plus standard list/scroll styling props, including `scrollbar_config`.

```rust
let buffer = Arc::new(LogBuffer::new(10_000)); // 10k entry ring buffer

// In a background thread: buffer.push(LogEntry { level, message });

LogView::new(buffer.clone())
    .filter_mode(MatchMode::Fuzzy)
    .auto_follow(true)
    .info_style(Style::new().fg(Color::Green))
    .error_style(Style::new().fg(Color::Red).bold())
```

### Events

```rust
// LogViewEvent { visible_index: usize, source_index: usize, entry: LogEntry }
```
