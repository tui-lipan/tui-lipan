# UI Macros (`ui!` and `rsx!`)

Two macros are available for building `Element` trees. **`ui!` is recommended** - it provides full rust-analyzer autocomplete with standard Rust builder syntax.

| Macro | Syntax style | Autocomplete | Formatter support | Recommended |
|-------|-------------|-------------|-------------------|-------------|
| **`ui!`** | Builder chains with `=> { children }` | **Full** (standard Rust) | `ui-fmt` for macro body; `rustfmt` preserves body and formats surrounding Rust | **Yes** |
| `rsx!` | Struct-literal DSL (`Widget { prop: val }`) | No (custom DSL) | `rsx-fmt` for macro body; `rustfmt` for surrounding Rust | For reading/reviewing |

Both produce `Element` and are fully interchangeable - mix freely in the same file.

---

## The `ui!` Macro (autocomplete-friendly)

`ui!` uses standard Rust builder method chains. Because the code before `=>` is
ordinary Rust, rust-analyzer provides full autocomplete on constructors, methods,
and argument types.

### Basic Syntax

```rust
ui! {
    VStack::new().gap(1).padding(1) => {
        Text::new("Hello World"),
        Button::new("Click Me")
            .style(Style::new().fg(Color::Blue))
            .on_click(ctx.link().callback(|_| Msg::Clicked)),
    }
}
```

- Everything **before** `=>` is a normal Rust expression (builder chain).
- `=> { child1, child2, ... }` desugars to `.child(child1).child(child2)...`.
- Leaf widgets (no children) need no `=> { }` - just write the expression.
- Items are separated by `,` or `;`.

### Nesting

Nest `=> { }` blocks for deep trees:

```rust
ui! {
    Frame::new().title("App").border(true).padding(1) => {
        VStack::new().gap(1) => {
            Text::new("Header").style(Style::new().bold()),
            HStack::new().gap(1) => {
                Button::new("Save").on_click(save_handler),
                Button::new("Cancel").on_click(cancel_handler),
            }
        }
    }
}
```

### Control Flow

`for` and `if`/`else` work inside children blocks:

```rust
ui! {
    VStack::new() => {
        for item in &items {
            Text::new(item.to_string()),
        }
        if ctx.state.loading {
            Spinner::new().label("Loading..."),
        } else {
            Text::new("Done"),
        }
    }
}
```

### Add Method Inference

The macro infers the correct child-add method from the builder chain root:

| Root type | Inferred method |
|-----------|----------------|
| `List` | `.item(...)` |
| `Tabs` | `.tab(...)` |
| `Accordion` | `.item(...)` |
| `DraggableTabBar` | `.tab(...)` |
| `AccordionItem` | `.content(...)` |
| Everything else | `.child(...)` |

For example, `List::new() => { ... }` automatically uses `.item()`:

```rust
ui! {
    List::new().selected(0).on_select(handler) => {
        ListItem::new("Alpha"),
        ListItem::new("Beta"),
    }
}
```

### Key Attribute

Use `@ key_expr` after the builder chain to assign a stable reconciliation/focus key:

```rust
ui! {
    Frame::new().title("Sidebar").border(true) @"sidebar" => {
        Text::new("Hello"),
    }
}
```

Leaf widgets can also be keyed:

```rust
ui! {
    VStack::new() => {
        Input::new(value).placeholder("Search...") @"search-input",
        List::new().selected(0) @"file-list" => {
            ListItem::new("Item 1"),
        }
    }
}
```

Dynamic keys work with any expression: `@format!("item-{}", id)`.

### Mixing `ui!`, `rsx!`, and Builder API

All three are interchangeable - they all produce `Element`:

```rust
fn view(&self, ctx: &Context<Self>) -> Element {
    ui! {
        Frame::new().title("App").border(true) => {
            self.sidebar(ctx),                          // builder API helper
            rsx! { Text { content: "Hello" } },         // rsx! inside ui!
            Text::new("World"),                         // leaf expression
        }
    }
}
```

---

## The `rsx!` Macro

`rsx!` is optional syntax sugar for the builder API. It uses struct-literal-like syntax for defining widgets, properties, and children. The builder API and `rsx!` are fully interchangeable.

> **Note:** rust-analyzer cannot autocomplete property names or widget names
> inside `rsx!` because it uses a custom DSL. If autocomplete matters, use
> `ui!` instead.

### Basic Syntax

```rust
rsx! {
    VStack {
        gap: 1,
        alignment: Align::Center,
        justify: Justify::Center,

        Text { content: "Hello World" }

        Button {
            label: "Click Me",
            style: Style::new().fg(Color::Blue),
            on_click: ctx.link().callback(|_| Msg::Clicked),
        }
    }
}
```

### Property Mappings

Most property names map directly to their builder method (e.g., `border: true` → `.border(true)`). Two names are remapped:

| RSX key | Builder method |
|---------|---------------|
| `alignment` | `.align(...)` |
| `spacing` | `.gap(...)` |

All other keys (including `justify`, `style`, `on_click`, etc.) pass through unchanged.

### Layout Constraints

Four special props apply to the resulting `Element` rather than the widget builder:

```rust
rsx! {
    Text {
        content: "Hello",
        min_width: 20,
        max_height: 5,
    }
}
```

| Prop | Effect |
|------|--------|
| `min_width` | Minimum column width |
| `max_width` | Maximum column width |
| `min_height` | Minimum row height |
| `max_height` | Maximum row height |

### Constructor Keys

Some widgets require positional arguments in their `new()` constructor. In `rsx!`, pass them using named keys.

#### Single-argument constructors

| Widget | Key | Example |
|--------|-----|---------|
| `Text` | `content` | `Text { content: "Hello" }` |
| `Button` | `label` | `Button { label: "OK" }` |
| `Input` | `value` | `Input { value: self.text.clone() }` |
| `TextArea` | `value` | `TextArea { value: self.text.clone() }` |
| `DocumentView` | `value` | `DocumentView { value: text.clone() }` |
| `Tab` | `label` | `Tab { label: "Tab 1" }` |
| `DraggableTab` | `label` | `DraggableTab { label: "file.rs" }` |
| `AccordionItem` | `title` | `AccordionItem { title: "Section" }` |
| `ListItem` | `text` | `ListItem { text: "Item 1" }` |
| `Checkbox` | `checked` | `Checkbox { checked: true }` |
| `ProgressBar` | `progress` | `ProgressBar { progress: 0.75 }` |
| `Radio` | `options` | `Radio { options: vec!["A".into(), "B".into()] }` |
| `Grid` | `columns` | `Grid { columns: 3 }` |
| `Heatmap` | `data` | `Heatmap { data: values.clone() }` |
| `Sparkline` | `data` | `Sparkline::new([1, 2, 3])` (stores `Arc<[u64]>`; use `data_arc` to share) |
| `Tree` | `root` | `Tree { root: node }` |
| `FileTree` | `root` | `FileTree { root: "/home/user" }` |
| `Toast` | `message` | `Toast { message: "Saved!" }` |
| `Tooltip` | `text` | `Tooltip { text: "Help text" }` |
| `ThemeProvider` | `theme` | `ThemeProvider { theme: Theme::nord() }` |
| `ContextProvider` | `value` | `ContextProvider { value: 42u32 }` |
| `Badge` | `content` | `Badge { content: "New" }` |
| `Slider` | `value` | `Slider { value: 0.5 }` |
| `Image` | `src` | `Image { src: "logo.png" }` |
| `Divider` | `orientation` | `Divider { orientation: Orientation::Horizontal }` |
| `Splitter` | `orientation` | `Splitter { orientation: Orientation::Vertical }` |
| `AsciiCanvas` | `lines` | `AsciiCanvas { lines: vec!["..."] }` |
| `ContextMenu` | `trigger` | `ContextMenu { trigger: btn }` |
| `HexArea` | `bytes` | `HexArea { bytes: data.into() }` |
| `PaginationBar` | `state` | `PaginationBar { state: pg.clone() }` |

#### Multi-argument constructors

Some widgets take more than one positional argument. All required keys must be provided:

| Widget | Keys (in order) | Example |
|--------|----------------|---------|
| `DiffView` | `before`, `after` | `DiffView { before: old.clone(), after: new.clone() }` |

```rust
rsx! {
    DiffView {
        before: original_text.clone(),
        after: modified_text.clone(),
        mode: DiffViewMode::Split,
        word_diff: true,
    }
}
```

### Children & Nesting

Children are declared inside the block, after properties. The macro picks the correct add method based on the parent widget type:

| Widget | Child method |
|--------|-------------|
| `VStack`, `HStack`, `ZStack`, `Frame`, `Group`, `Portal`, `Modal`, `MouseRegion`, `ThemeProvider`, `ContextProvider`, `Tooltip`, `ScrollView`, `Grid`, `StatusBar`, `Center` | `.child(...)` |
| `List` | `.item(...)` |
| `Tabs` | `.tab(...)` |
| `Accordion` | `.item(...)` |
| `DraggableTabBar` | `.tab(...)` |
| `AccordionItem` | `.content(...)` |

Single-child widgets (`Frame`, `Center`, `Group`, `Portal`, `Modal`, `MouseRegion`, `ThemeProvider`, `ContextProvider`, `Tooltip`, `AccordionItem`, `Badge`) accept exactly one child. Use `VStack` or `HStack` to group multiple elements:

```rust
rsx! {
    Modal {
        title: "Confirm",
        VStack {
            gap: 1,
            Text { content: "Delete this item?" }
            HStack {
                gap: 1,
                Button { label: "Cancel" }
                Button { label: "Delete" }
            }
        }
    }
}
```

```rust
rsx! {
    VStack {
        gap: 1,
        Frame { title: "Panel", border: true }
        HStack {
            gap: 1,
            Text { content: "Left" }
            Text { content: "Right" }
        }
    }
}
```

### CenterPin

`CenterPin` is a layout container that pins one child to the true center of the available area, giving the remaining vertical space above and below to `top` and `bottom` zones. It does **not** accept children in `rsx!`; instead, use the `top`, `center`, and `bottom` props:

```rust
rsx! {
    CenterPin {
        top: rsx! { VStack { gap: 1, Text { content: "Header" } } },
        center: rsx! { Modal { title: "Dialog", Text { content: "Body" } } },
        bottom: rsx! { StatusBar { Text { content: "Ready" } } },
    }
}
```

All three props are optional.

### Control Flow

Standard Rust control flow works inside `rsx!`:

**Loops:**
```rust
rsx! {
    VStack {
        for item in &items {
            Text { content: item.to_string() }
        }
    }
}
```

**Conditionals:**
```rust
rsx! {
    VStack {
        if ctx.state.loading {
            Spinner { label: "Loading..." }
        } else {
            List { items: ctx.state.rows.clone(), selected: 0 }
        }

        if let Some(error) = &ctx.state.error {
            Text { content: error.clone(), style: Style::new().fg(Color::Red) }
        }
    }
}
```

### Event Handlers

```rust
rsx! {
    Button {
        label: "Save",
        on_click: ctx.link().callback(|_| Msg::Save),
    }

    Input {
        value: ctx.state.text.clone(),
        on_change: ctx.link().callback(Msg::TextChanged),
    }

    List {
        items: items.clone(),
        selected: ctx.state.selected,
        on_select: ctx.link().callback(|e| Msg::Select(e.index)),
        on_activate: ctx.link().callback(|e| Msg::Activate(e.index)),
    }
}
```

### Key Attribute

Assign stable keys for reconciliation and focus:

```rust
rsx! {
    List { key: "file-list", items: files.clone(), selected: 0 }
    Input { key: "search-input", value: query.clone() }
}
```

`key:` is not supported on `Tab` or `ListItem`.

### Mixing Builder API and `rsx!`

They are interchangeable - use whichever is clearer:

```rust
// Builder API
fn sidebar(items: &[&str]) -> Element {
    Frame::new()
        .title("Nav")
        .border(true)
        .child(List::new().items(items.iter().map(|s| ListItem::new(*s))))
        .into()
}

// rsx! - call builder-API functions by embedding expressions as children
fn view(&self, ctx: &Context<Self>) -> Element {
    rsx! {
        HStack {
            gap: 1,
            sidebar(&self.nav_items),
            VStack {
                Text { content: "Main content" }
            }
        }
    }
}
```

### Editor Snippets

The canonical VS Code/Cursor snippet pack lives at
`tui-lipan-macro/snippets/vscode.code-snippets`.

This format is compatible with:

- VS Code
- Cursor
- VSCodium and other VS Code-compatible editors

To use it in a project, copy or symlink it into your workspace as
`.vscode/tui-lipan.code-snippets`.

Example:

```bash
mkdir -p .vscode
ln -s ../tui-lipan-macro/snippets/vscode.code-snippets .vscode/tui-lipan.code-snippets
```

Other editors do not consume `.code-snippets` directly, but this file is still
the canonical source for snippet content and can be adapted for Neovim, Helix,
Zed, or JetBrains snippet systems.

### Formatting

`cargo fmt`/`rustfmt` formats Rust around macro calls, but does not reflow `ui!` or `rsx!` macro bodies. Use the macro formatters first, then run `rustfmt`.

Recommended order:

```bash
cargo ui-fmt src/main.rs && cargo rsx-fmt src/main.rs && rustfmt --edition 2024 src/main.rs
```

Check mode:

```bash
cargo ui-fmt --check src/main.rs && cargo rsx-fmt --check src/main.rs && rustfmt --check --edition 2024 src/main.rs
```

Or use the repo helper scripts:

```bash
./scripts/format-rust-with-macros src/main.rs
./scripts/format-rust-with-macros --check src/main.rs
./scripts/format-rust-with-rsx src/main.rs
./scripts/format-rust-with-rsx --check src/main.rs
```

#### Editor integration

These examples run the full chain: `ui-fmt` → `rsx-fmt` → `rustfmt`.

**VS Code (`settings.json`, using the Run On Save extension):**

```json
{
  "emeraldwalk.runonsave": {
    "commands": [
      {
        "match": "\\.rs$",
        "cmd": "bash -c 'cargo ui-fmt \"${file}\" && cargo rsx-fmt \"${file}\" && rustfmt --edition 2024 \"${file}\"'"
      }
    ]
  }
}
```

**Neovim (`conform.nvim`):**

```lua
require("conform").setup({
  formatters = {
    ui_fmt = {
      command = "cargo",
      args = { "ui-fmt", "--stdin" },
      stdin = true,
    },
    rsx_fmt = {
      command = "cargo",
      args = { "rsx-fmt", "--stdin" },
      stdin = true,
    },
  },
  formatters_by_ft = {
    rust = { "ui_fmt", "rsx_fmt", "rustfmt" },
  },
})
```

**Helix (`languages.toml`):**

```toml
[[language]]
name = "rust"
formatter = { command = "bash", args = ["-lc", "cargo ui-fmt --stdin | cargo rsx-fmt --stdin | rustfmt --emit stdout --edition 2024"] }
auto-format = true
```

Current limitations:

- `ui-fmt` and `rsx-fmt` rewrite whole Rust files and only touch macro invocations they understand.
- It is intentionally conservative and uses a fixed style.
- `rsx!` blocks that contain comments are left unchanged so comments stay intact.
