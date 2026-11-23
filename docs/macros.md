# UI Macros (`ui!` and `rsx!`)

Both macros build `Element` trees and can be mixed in the same file. Use `ui!`
for standard Rust builder syntax and rust-analyzer autocomplete.

| Macro | Syntax style | Autocomplete | Formatter support | Use |
|-------|-------------|-------------|-------------------|-------------|
| **`ui!`** | Builder chains with `=> { children }` | **Full** (standard Rust) | `ui-fmt` for macro body; `rustfmt` preserves body and formats surrounding Rust | Recommended |
| `rsx!` | Struct-literal DSL (`Widget { prop: val }`) | No (custom DSL) | `rsx-fmt` for macro body; `rustfmt` for surrounding Rust | Optional builder syntax sugar |

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

- Everything before `=>` is a Rust expression.
- `=> { child1, child2, ... }` adds children using the parent type's `UiChildren` implementation.
- Write leaf widgets as expressions without a children block.
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
        },
        if ctx.state.loading {
            Spinner::new().label("Loading..."),
        } else {
            Text::new("Done"),
        }
    }
}
```

### Child dispatch

The expression before `=>` can be a builder chain, a helper that returns a
builder, or a variable. The parent type determines how children are added.
Here, `list()` returns a `List`, so each child goes through `List::item`.

```rust
fn list() -> List {
    List::new().selected(0)
}

ui! {
    list() => {
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
        align: Align::Center,
        justify: Justify::Center,

        Text { content: "Hello World" },

        Button {
            label: "Click Me",
            style: Style::new().fg(Color::Blue),
            on_click: ctx.link().callback(|_| Msg::Clicked),
        }
    }
}
```

### Property Mappings

Properties use the builder method name. For example, `border: true` calls
`.border(true)`. Constructor keys, `key`, and element constraints have their own
rules below. If you used `alignment` or `spacing` for a stack, use `align` or
`gap`.

### Layout Constraints

These properties apply to the resulting `Element`.

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
| `automation_id` | Tree-wide automation identity |

### Constructor Keys

Use the named key below to pass an argument to a widget's `new()` constructor.
Text bodies use `content`, labels use `label`, and edited values use `value`.
For `ListItem`, `Tooltip`, and `Toast`, migrate the old `text` or `message` key
to `content`.

| Widget | Key | Example |
|--------|-----|---------|
| `Text` | `content` | `Text { content: "Hello" }` |
| `Button` | `label` | `Button { label: "OK" }` |
| `Input` | `value` | `Input { value: self.text.clone() }` |
| `TextArea` | `value` | `TextArea { value: self.text.clone() }` |
| `DocumentView` | `value` | `DocumentView { value: text.clone() }` |
| `Tab` | `label` | `Tab { label: "Tab 1" }` |
| `DraggableTab` | `label` | `DraggableTab { label: "file.rs" }` |
| `AccordionItem` | `title` | `AccordionItem { title: "Section", Text { content: "Body" } }` |
| `ListItem` | `content` | `ListItem { content: "Item 1" }` |
| `Checkbox` | `checked` | `Checkbox { checked: true }` |
| `ProgressBar` | `progress` | `ProgressBar { progress: 0.75 }` |
| `Radio` | `options` | `Radio { options: ["A", "B"] }` |
| `Heatmap` | `data` | `Heatmap { data: values.clone() }` |
| `Sparkline` | `data` | `Sparkline { data: [1, 2, 3] }` |
| `Tree` | `root` | `Tree { root: node }` |
| `FileTree` | `root` | `FileTree { root: "/home/user" }` |
| `Flowchart` | `direction` | `Flowchart { direction: FlowDirection::LeftRight }` |
| `Toast` | `content` | `Toast { content: "Saved!" }` |
| `Tooltip` | `content` | `Tooltip { content: "Help text" }` |
| `ThemeProvider` | `theme` | `ThemeProvider { theme: Theme::default() }` |
| `ContextProvider` | `value` | `ContextProvider { value: 42u32 }` |
| `Badge` | `content` | `Badge { content: "New" }` |
| `Slider` | `value` | `Slider { value: 0.5 }` |
| `Image` | `src` | `Image { src: "logo.png" }` |
| `Divider` | `orientation` | `Divider { orientation: Orientation::Horizontal }` |
| `Splitter` | `orientation` | `Splitter { orientation: Orientation::Vertical }` |
| `AsciiCanvas` | `lines` | `AsciiCanvas { lines: vec!["..."] }` |
| `ContextMenu` | `trigger` | `ContextMenu { trigger: btn }` |
| `HexArea` | `bytes` | `HexArea { bytes: vec![0u8, 1] }` |
| `PaginationBar` | `state` | `PaginationBar { state: pg.clone() }` |

### Grid

`Grid::new()` takes no arguments. Use `uniform_columns: 3` for three
auto-sized columns, or pass specific widths through the `columns` builder
property.

### DiffView

`DiffView::new()` takes no arguments. Use the `before` and `after` builder
properties to set the two texts.

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

Declare children inside the block. Both macros use the parent's `UiChildren`
implementation, so aliases and qualified paths work for child dispatch.
The `rsx!` constructor keys above still use the widget name. Use a builder
expression or `ui!` for an alias whose constructor takes arguments.

| Parent | Child method |
|--------|--------------|
| `Flow`, `Grid`, `HStack`, `ScrollView`, `Splitter`, `StatusBar`, `VStack`, `ZStack` | `.child(...)` |
| `Animated`, `Badge`, `Center`, `ContextProvider`, `DragSource`, `DropTarget`, `EffectScope`, `Frame`, `Modal`, `MouseRegion`, `PanView`, `ThemeProvider`, `Tooltip` | `.child(...)` (single child) |
| `List`, `Accordion` | `.item(...)` |
| `Tabs`, `DraggableTabBar` | `.tab(...)` |
| `AccordionItem`, `Popover` | `.content(...)` (single child) |
| `Canvas` | `.child_at(...)` with a `CanvasItem` |
| `Graph` | `.root(...)` (single root) |
| `GraphNode`, `TreeNode` | `.child(...)` (typed node) |

For a single-child builder, the macros reject a second direct child in the
same block. A child added by control flow follows the builder's behavior: if
several calls run, the last child replaces the previous one. Use a `VStack`
or `HStack` to group siblings.

```rust
rsx! {
    Modal {
        title: "Confirm",
        VStack {
            gap: 1,
            Text { content: "Delete this item?" },
            HStack {
                gap: 1,
                Button { label: "Cancel" },
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
        Frame { title: "Panel", border: true },
        HStack {
            gap: 1,
            Text { content: "Left" },
            Text { content: "Right" }
        }
    }
}
```

### CenterPin

`CenterPin` reserves the true center for one child and shares the remaining
vertical space across the top and bottom zones. Set the zones explicitly with
`top`, `center`, and `bottom`.

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
        },

        if let Some(error) = &ctx.state.error {
            Text { content: error.clone(), style: Style::new().fg(Color::Red) }
        }
    }
}
```

### Event Handlers

```rust
rsx! {
    VStack {
        Button {
            label: "Save",
            on_click: ctx.link().callback(|_| Msg::Save),
        },

        Input {
            value: ctx.state.text.clone(),
            on_change: ctx.link().callback(Msg::TextChanged),
        },

        List {
            items: items.clone(),
            selected: ctx.state.selected,
            on_select: ctx.link().callback(|e| Msg::Select(e.index)),
            on_activate: ctx.link().callback(|e| Msg::Activate(e.index)),
        }
    }
}
```

### Key Attribute

Assign stable keys for reconciliation and focus:

```rust
rsx! {
    VStack {
        List { key: "file-list", items: files.clone(), selected: 0 },
        Input { key: "search-input", value: query.clone() }
    }
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

### Custom containers

Implement `tui_lipan::UiChildren` for a custom builder that
accepts macro children. The first trait parameter is the accepted child type.
Use a generic `const ADDITIONAL: bool` to accept multiple siblings or leave
that parameter at its default for a single-child builder. An implementation
can use a bound such as `C: Into<Element>` to accept several child types.

```rust
use tui_lipan::{UiChildren, prelude::*};

struct Rows(List);

impl Rows {
    fn new() -> Self {
        Self(List::new())
    }
}

impl<const ADDITIONAL: bool> UiChildren<ListItem, ADDITIONAL> for Rows {
    fn ui_child(self, child: ListItem) -> Self {
        Self(self.0.item(child))
    }
}

impl From<Rows> for Element {
    fn from(rows: Rows) -> Self {
        rows.0.into()
    }
}

let rows = ui! {
    Rows::new() => {
        ListItem::new("Alpha"),
        ListItem::new("Beta"),
    }
};

let same_rows = rsx! {
    Rows {
        ListItem { content: "Alpha" },
        ListItem { content: "Beta" }
    }
};
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
