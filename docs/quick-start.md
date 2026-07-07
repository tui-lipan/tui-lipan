# Quick Start

## Introduction

**tui-lipan** is an opinionated, component-based TUI framework for Rust, inspired by React and Elm.

Key characteristics:
- **Declarative UI** - builder API + `ui!` macro (with full autocomplete), plus optional `rsx!`.
- **Component model** - properties, local state, and message-based updates.
- **Flexbox-like layout** - sensible defaults, no raw coordinate math.
- **Rich widget set** - Frames, Tabs, Lists, Inputs, Tables, Modals, and more.

## Import Map

```rust
// Recommended: start here for typical app-author code
use tui_lipan::prelude::*;
```

The prelude is intentionally curated for app authors. It re-exports the common
component/runtime types, styling primitives, macros, and a broad set of
user-facing widgets and widget event types. For framework internals or unusual
helpers, prefer explicit imports from `tui_lipan`.

Representative `prelude::*` re-exports:

| Symbol | Category |
|--------|----------|
| `App`, `AppRunner`, `ContrastPolicy`, `TextAreaNewlineBinding` | App runner |
| `Component`, `Context`, `Update`, `Command`, `Breakpoint`, `KeyUpdate`, `TaskPolicy` | Component trait |
| `Element`, `IntoElement`, `Key` | Tree primitives |
| `Callback`, `CommandLink`, `KeyHandler`, `Link` | Messaging |
| `KeyCode`, `KeyEvent`, `KeyMods`, `MouseEvent`, `MouseMoveEvent` | Events |
| `KeyBinding`, `KeyBindings` | Common keybinding types |
| `Style`, `Color`, `Length`, `Padding`, `Align`, `Justify`, `BorderStyle`, `BorderEdges`, `CaretShape` | Styling |
| `RichText`, `Span`, `Edge`, `Rect`, `Size`, `ScrollbarConfig`, `ScrollbarVariant` | Layout & text types |
| `Theme`, `ColorGradient`, `GradientDirection`, `GradientRange`, `VisualEffect`, `RippleRadius`, `RetroPreset` | Themes & effects |
| `ClipboardConfig`, `PasteShiftInsertBehavior` | Clipboard config |
| `TextEditor`, `TextInput`, `TextEditEvent`, `TextEditKind` | Text editing |
| `word_forward_start`, `word_end`, `line_start_at`, `first_nonblank_in_line`, … (`text_motion` module) | Vim-style word/line text motion helpers |
| `OverlayId`, `OverlayScope`, `ToastHandle`, `ToastPlacement` | Overlays |
| `App`, `CommandEntry`, `CommandRegistry` | App commands |
| `FrameworkAction`, `FrameworkKeymap`, `KeyDispatchPolicy`, `TerminalKeyPolicy`, `UserKeymapPolicy`, `CommandConflictPolicy`, `ChordMismatchPolicy` | Layered key dispatch |
| `child`, `mockup!`, `rsx!`, `ui!` | Macros & helpers |
| `VStack`, `HStack`, `ZStack`, `Canvas`, `Frame`, `Button`, `Text`, `Input`, `List`, `Tabs`, `Table`, `Modal`, `TextArea`, `Tree`, `DocumentView`, `FileTree`, `Animated`, `AsciiCanvas` | Common and advanced widgets |

The prelude no longer re-exports broad internal modules like `core`, `utils`,
or `widgets::*` wholesale.

Extra imports **not** in `prelude::*`:

```rust
// Clipboard image support (requires feature "image" or "clipboard-images")
use tui_lipan::{ImageContent, ImageFormat, ClipboardProvider, ClipboardError};

// Lower-level framework or specialized APIs
use tui_lipan::NodeId;
```

## Feature Flags

```toml
[dependencies]
tui-lipan = { version = "*", features = ["image", "big-text"] }
```

| Feature | Default | What it enables |
|---------|---------|-----------------|
| `clipboard` | **Yes** | System clipboard via arboard (X11/Wayland/macOS/Windows) |
| `devtools` | No | In-app DevTools overlay (`F12` by default, rebindable) with frame stats and debug log console; controllable from `Context` and configurable via `DevToolsConfig` |
| `ui-snapshot-json` | No | JSON export for `UiSnapshot::to_json()` (markdown export is always available) |
| `ui-snapshot-png` | No | Font-backed PNG export for `UiSnapshot::to_png()` / `to_png_default()` and `CapturedFrame::to_png()` |
| `clipboard-images` | No | Image clipboard read/write (without `Image` rendering widget) |
| `big-text` | No | Large ASCII/pixel text via FIGlet and pixel fonts - `BigText` |
| `diff-view` | No | Side-by-side/unified diff viewer - `DiffView` |
| `image` | No | Protocol-aware image rendering (Kitty, iTerm2, Sixel, halfblocks) with PNG/JPEG/GIF/WebP codecs - includes `clipboard-images` |
| `image-full-formats` | No | Restores the broad `image` crate default codec set for `image`, `clipboard-images`, or `ui-snapshot-png` builds |
| `markdown` | No | Markdown formatter for `DocumentView` + markdown preview example |
| `profiling-tracing` | No | `tracing` spans/events around render loop and `DocumentView` formatting/reconcile hot paths |
| `syntax-syntect` | No | Syntax highlighting in `TextArea`, `DocumentView`, and `DiffView` via syntect |
| `terminal` | No | Embedded PTY / terminal viewport - `Terminal`, `ManagedTerminal` |
| `terminal-serde` | No | Serde derives for terminal snapshot leaf style/mouse types used by external, versioned snapshot transports; includes `terminal` |
| `theme-reload` | No | Live reload of TOML theme files without restarting the app - see [Styling](styling.md) |
| `web` | No | Browser/WASM backend - see [Web / WASM Backend](web-backend.md) |

### Profiling with `tracing`

Enable instrumentation:

```toml
tui-lipan = { version = "*", features = ["markdown", "profiling-tracing"] }
```

Then install any standard `tracing` subscriber in your app binary (for example
`tracing-subscriber`, `tracing-tracy`, or OpenTelemetry exporters). tui-lipan emits
spans/events for frame loop, draw, and `DocumentView` formatting/reconcile hot paths.

To **disable** clipboard (e.g. for minimal no-system-dep builds):

```toml
tui-lipan = { version = "*", default-features = false }
```

For smaller shipping binaries, build app artifacts with the size-optimized profile:

```bash
cargo build --profile release-size --no-default-features
```

Use the normal `release` profile when runtime throughput is more important than artifact size.

Examples requiring specific features:

| Example | Required feature |
|---------|-----------------|
| `big_text`, `figlet_editor` | `big-text` |
| `diff_hub` | `diff-view` |
| `image`, `image_modes`, `messenger` | `image` |
| `markdown_hub` | `markdown` |
| `markdown_editor_sync` | `markdown`, `syntax-syntect` |
| `terminal_filetree_devtools` | `terminal` |
| `devtools` | `devtools` |
| `theme_hot_reload` | `theme-reload` |

With `devtools` enabled, the built-in panel uses fixed default dimensions per tab; use `Context` (`show_devtools`, `hide_devtools`, `toggle_devtools`) for visibility.

### DevTools runtime configuration

When the `devtools` feature is enabled, you can opt out of individual subsystems at app start time:

```rust
use tui_lipan::prelude::*;

App::new()
    .devtools_config(DevToolsConfig {
        logs: true,    // ingest debug_log! lines into the DevTools log panel
        metrics: true, // collect per-frame stats (FPS, reconcile/draw times, node count)
        show_framework_logs: false, // hide tui-lipan's own internal log lines by default
    })
    .mount(MyApp)
    .run()
```

- `logs: false` removes the `debug_log!` → devtools sink path entirely (the macro still respects `TUI_LIPAN_DEBUG=1` env logging).
- `metrics: false` skips frame timing and tree-size collection; the panel will show "No frame metrics yet".
- `show_framework_logs: false` starts the Logs tab with tui-lipan's own framework
  noise (key events, dirty tracking, etc.) hidden, leaving only your app's
  `debug_log!` lines. Toggle it live with the **tui-lipan** button in the Logs tab.
- `logs`, `metrics`, and `show_framework_logs` all default to `true`, so
  `features = ["devtools"]` behaves exactly as before.

In the Logs tab you can also copy the selected row to the clipboard with `Ctrl+C`,
or by activating a row (double-click / `Enter`).

Subsystem cost — what each toggle controls:

| Subsystem | When `true` (default)                                                                                        | When to turn off                                                                |
| --------- | ------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------- |
| `logs`    | Every `debug_log!` allocates a `String` and pushes a `DevLogEntry` onto a bounded ring buffer (small).       | Hot loops calling `debug_log!` thousands of times per frame, or to drop the formatting cost when the panel is never opened. |
| `metrics` | Per-frame timing samples + node-tree size snapshot collected on every render; small fixed-size ring buffer.  | Profiling renders against a `release` build where you don't want sampling overhead, or shipping a build with the feature compiled in but the panel unused. |

Note: `debug_log!` works in `--release` builds when the `devtools` feature is enabled — there is no `debug_assertions` gate. Lines are emitted to the panel regardless of profile, so guard hot paths yourself if you need zero overhead in release.

## Minimal Example

```rust
use tui_lipan::prelude::*;

struct Counter;

#[derive(Default)]
struct State {
    count: i32,
}

#[derive(Clone)]
enum Msg {
    Increment,
    Decrement,
}

impl Component for Counter {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        rsx! {
            VStack {
                gap: 1,
                padding: 2,

                Text { content: format!("Count: {}", ctx.state.count) }

                HStack {
                    gap: 1,
                    Button { label: "-", on_click: ctx.link().callback(|_| Msg::Decrement) }
                    Button { label: "+", on_click: ctx.link().callback(|_| Msg::Increment) }
                }
            }
        }
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Increment => ctx.state.count += 1,
            Msg::Decrement => ctx.state.count -= 1,
        }
        Update::full()  // (needs_redraw, optional_command)
    }
}

fn main() -> tui_lipan::Result<()> {
    App::new()
        .title("Counter")
        .mount(Counter)
        .run()
}
```

## Fast Prototyping with `mockup!`

Skip all `Component` boilerplate for layout previews:

```rust
use tui_lipan::prelude::*;

fn main() -> tui_lipan::Result<()> {
    mockup!("Dashboard Preview", {
        HStack::new()
            .gap(1)
            .child(
                Frame::new()
                    .title("Sidebar")
                    .border(true)
                    .width(Length::Px(30))
                    .child(List::new().items([
                        ListItem::new("Dashboard"),
                        ListItem::new("Settings"),
                        ListItem::new("Logs"),
                    ]).selected(0)),
            )
            .child(
                Frame::new()
                    .title("Content")
                    .border(true)
                    .padding(1)
                    .child(Text::new("Hello from mockup!")),
            )
    })
}
```

**Key behaviors:**
- Press `Esc` or `q` to quit.
- The body expression is auto-wrapped in `.into()` - return any widget builder directly.
- Interactive widgets (List, Tabs, Inputs) still respond to focus and mouse.
- The closure uses `move` capture, so local data is accessible.

**Using `Mockup` adapter directly:**

```rust
App::new()
    .title("My Layout")
    .mount(Mockup::new(|| {
        Frame::new().title("Panel").border(true)
            .child(Text::new("World")).into()  // closure must return Element
    }))
    .run()
```

### Mockup → App Workflow

Extract views as plain functions reusable in both mockups and real components:

```rust
fn sidebar(items: &[&str], selected: usize) -> Element {
    Frame::new().title("Nav").border(true)
        .width(Length::Px(28))
        .child(List::new().items(items.iter().map(|s| ListItem::new(*s))).selected(selected))
        .into()
}

// Step 1: preview with mockup
fn main() -> tui_lipan::Result<()> {
    let items = vec!["Home", "Settings", "Logs"];
    mockup!("Preview", { sidebar(&items, 0) })
}

// Step 2: reuse in real component - zero rewrite
fn view(&self, ctx: &Context<Self>) -> Element {
    sidebar(&ctx.state.nav_items, ctx.state.selected)
}
```

## App Configuration

```rust
App::new()
    .title("My App")           // Optional outer chrome frame
    .theme(Theme::one_dark())  // Optional theme override
    .system_theme()            // Optional: derive theme from host terminal colors
    .inline_ephemeral(8)       // Optional: inline mode (8 terminal rows)
    .mouse(true)               // Mouse capture (default: true in fullscreen, false in inline)
    .scroll_wheel_multiplier(3) // Optional: lines per wheel tick (default: 1)
    .toast_placement(ToastPlacement::BottomEnd)
    .keymap_path("/path/to/keymap.conf")  // see docs/keybindings.md
    .global_quit(None)                   // disable Ctrl-Q quit without a keymap file
    .framework_keymap(
        FrameworkKeymap::default().unbind(FrameworkAction::Quit),
    )
    .user_keymap_policy(UserKeymapPolicy::Disabled) // ignore env/default user keymaps
    .key_dispatch_policy(KeyDispatchPolicy::AppCommandsFirst)
    .terminal_key_policy(TerminalKeyPolicy::AppCommandsThenTerminal)
    .command_conflict_policy(CommandConflictPolicy::HighestPriority)
    .chord_mismatch_policy(ChordMismatchPolicy::ForwardPrefixAndCurrent)
    .clipboard_config(ClipboardConfig { .. })
    .contrast_policy(ContrastPolicy::Wcag)
    .terminal_bg(query_host_colors().map(|c| c.bg))  // enables Opacity through Color::Reset
    .live_host_terminal_colors(true)  // opt-in runner-managed live host palette refresh
    .mount(Root)
    .exit_view(|_component, ctx| {
        Text::new(format!("Final count: {}", ctx.state.count)).into()
    })
    .run()
```

`ScrollView::scroll_wheel_multiplier(...)`,
`TextArea::scroll_wheel_multiplier(...)`, and
`DocumentView::scroll_wheel_multiplier(...)` override the app-wide wheel
multiplier for a specific widget.

> **`terminal_bg` / live host colors**: `ColorTransform::Opacity` blends foreground colors toward the resolved cell background. When the cell background is `Color::Reset` (terminal default) there is no RGB to blend toward, so opacity has no effect. Calling `.terminal_bg(query_host_colors().map(|c| c.bg))` before `run()` provides the terminal's actual default background color and enables correct opacity blending for static apps. Use `.system_theme()` to opt into a framework-wide theme derived from live host colors, or `.live_host_terminal_colors(true)` when app code wants to read `ctx.host_terminal_colors()` and build its own tokens. The runner probes once at startup, refreshes on terminal focus gained, services `ctx.request_host_terminal_color_refresh()`, and never polls continuously. Refreshed host backgrounds update `terminal_bg` automatically. Omitting both leaves opacity unchanged on reset-background cells.

> **`exit_view`**: Attach this on `AppRunner<C>` after `.mount(...)` when you want a final one-shot element rendered to stdout after the TUI exits. The callback runs before unmount, so component state is still available in `ctx.state`. This is useful for persisting a session summary or logo in terminal scrollback.

Layered key dispatch policy builders are always available (no extra feature flag). See [`keybindings.md`](keybindings.md) for precedence rules, command `shortcut(...)` vs `keybinding_hint(...)`, and [`focus.md`](focus.md) / [`widgets/terminal.md`](widgets/terminal.md) for dispatch order tables.

## Development Workflow

1. **Define State** - `struct State { ... }` with `#[derive(Default)]`
2. **Define Messages** - `enum Msg { ... }` with `#[derive(Clone)]`
3. **Implement Component** - `create_state`, `update`, `view`
4. **Run** - `App::new().mount(Root).run()`

## Debugging

### Debug logging

Enable debug output with environment variables:

```sh
TUI_LIPAN_DEBUG=1 cargo run                         # Print to stderr
TUI_LIPAN_DEBUG_FILE=/tmp/tui.log cargo run          # Also append to file
```

Use the `debug_log!` macro in your own code to emit messages through the same channel:

```rust
use tui_lipan::debug_log;

debug_log!("Current state: {:?}", ctx.state);
```

### Layout snapshot diagnostics

When content vanishes in tests or mockups, first check the viewport and sizing:
- `TestBackend` starts at an 80x24 viewport unless you call `set_viewport(...)`; use a fixed viewport for reproducible snapshots.
- `Mockup` renders at the live terminal size, so a layout can change when the terminal is narrow or short.
- `VStack`, `HStack`, and `Frame` default to `Length::Flex(1)` on both axes; fixed headers, footers, and side bars usually need `Length::Px(...)`.
- Capture with `UiSnapshotOptions::diagnostic()` to include zero-area nodes, spacers, and dividers; markdown snapshots flag zero-size widgets as `zero-area`.

### Mouse event diagnostics

The `tui_lipan::debug` module exposes counters for diagnosing mouse event throughput:

```rust
use tui_lipan::debug;

let count = debug::mouse_events_processed();  // Total mouse events since start
debug::reset_mouse_events();                  // Reset counter to zero
```
