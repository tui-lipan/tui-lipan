# Terminal Widgets *(require feature `terminal`)*

```toml
tui-lipan = { version = "*", features = ["terminal"] }
```

---

## ManagedTerminal

The recommended starting point: a complete PTY terminal with automatic lifecycle management. No manual wiring needed.

| Prop | Type | Description |
|------|------|-------------|
| `config` | `TerminalPtyConfig` | Shell/cwd/env configuration |
| `scrollback` | `usize` | Scrollback buffer size in lines (default: 2000) |
| `initial_cols` | `u16` | Initial columns (default: 120) |
| `initial_rows` | `u16` | Initial rows (default: 24) |
| `auto_start` | `bool` | Start PTY on init (default: true) |
| `placeholder` | `Option<Arc<str>>` | Text shown before PTY is ready |
| `forward_mouse` | `bool` | Forward mouse events to PTY (default: true) |
| `scroll_wheel` | `bool` | Mouse wheel for scrollback (default: true) |
| `style` | `Style` | Terminal content style |
| `focusable` | `bool` | Accept focus (default: true) |
| `width` | `Length` | Width (default: `Flex(1)`) |
| `height` | `Length` | Height (default: `Flex(1)`) |
| `on_status` | `Callback<ManagedTerminalStatus>` | Status change callback |

```rust
use tui_lipan::prelude::*;

// Simple usage - starts shell in current directory
ManagedTerminal::new()
    .on_status(ctx.link().callback(Msg::TerminalStatus))

// Custom shell and working directory
ManagedTerminal::new()
    .config(
        TerminalPtyConfig::new("/bin/bash")
            .cwd("/home/user/projects")
            .env("MY_VAR", "value")
    )
    .scrollback(5000)
    .initial_size(120, 40)
    .on_status(ctx.link().callback(Msg::TerminalStatus))
```

**Status events (`ManagedTerminalStatus`):**

| Variant | Meaning |
|---------|---------|
| `Starting` | PTY is being initialized |
| `Ready` | PTY is ready and accepting input |
| `Exited(i32)` | Shell exited with status code |
| `Error(Arc<str>)` | Error occurred |

```rust
match status {
    ManagedTerminalStatus::Ready => ctx.state.terminal_ready = true,
    ManagedTerminalStatus::Exited(code) => { /* handle exit */ }
    ManagedTerminalStatus::Error(msg) => { /* handle error */ }
    _ => {}
}
```

---

## Terminal (Low-Level)

The low-level terminal viewport widget. Use when you need custom PTY handling, multiple terminals, or specialized input routing.

| Prop | Type | Description |
|------|------|-------------|
| `snapshot` | `TerminalRenderSnapshot` | Current screen snapshot |
| `style` | `Style` | Container style |
| `hover_style` | `Style` | Hover style |
| `extend_hover_style` / `inherit_hover_style` | `Style` / `()` | Extend or inherit the hover theme role instead of replacing it |
| `focus_style` | `Style` | Focus chrome style |
| `extend_focus_style` / `inherit_focus_style` | `Style` / `()` | Extend or inherit the focus theme role instead of replacing it |
| `focusable` | `bool` | Accept focus |
| `scroll_wheel` | `bool` | Mouse wheel scrollback |
| `selection_style` | `Style` | Text selection style |
| `extend_selection_style` / `inherit_selection_style` | `Style` / `()` | Extend or inherit the text-selection theme role instead of replacing it |
| `selection` | `Option<TerminalSelection>` | Controlled selection |
| `border` | `bool` | Show border |
| `border_style` | `BorderStyle` | Border style |
| `padding` | `impl Into<Padding>` | Padding |
| `width` | `Length` | Width |
| `height` | `Length` | Height |
| `on_input` | `Callback<TerminalInputEvent>` | Keyboard/paste input from user |
| `on_resize` | `Callback<TerminalViewport>` | Viewport size changed |
| `on_scroll_to` | `Callback<usize>` | Scrollback offset changed |
| `on_mouse_forward` | `Callback<Vec<u8>>` | Mouse event bytes for PTY |
| `on_selection` | `Callback<TerminalSelection>` | Selection changed |
| `on_key` | `KeyHandler` | Low-level key handler |

Use `scrollbar_config` to configure layout variant, gap, and thumb for the vertical scrollbar.

---

## Terminal key routing policy

Configure how keys are ordered when a terminal pane has focus:

```rust
App::new()
    .terminal_key_policy(TerminalKeyPolicy::AppCommandsThenTerminal)
    .mount(MyMuxApp)
    .run()
```

| Policy | Typical use | Order (summary) |
|--------|-------------|-----------------|
| `FrameworkFirst` **(default)** | Simple apps, backward-compatible behavior | Framework shortcuts → terminal clipboard preflight → terminal `on_key` / `on_input` → bubble |
| `AppCommandsThenTerminal` | Multiplexer / terminal-host apps | Terminal performable copy/paste preflight → app command chords/shortcuts → terminal forwarding → bubble → framework fallback |
| `TerminalFirst` | PTY-first apps that still want optional app shortcuts | Terminal preflight → terminal forwarding → app commands (if terminal did not consume) → bubble → framework fallback |
| `TerminalOnly` | Full passthrough; no app command or framework fallback while terminal is focused | Terminal preflight → terminal forwarding only |

**Performable preflight** runs before app commands under mux-style policies so terminal copy/paste is never stolen:

- `Ctrl+C` with a non-empty terminal selection copies to the clipboard instead of running an app shortcut on the same key.
- `Ctrl+Shift+C` / `Ctrl+Shift+V` paste paths run when the terminal can accept input.

Register mux lifecycle shortcuts with executable command bindings (`CommandEntry::shortcut(...)`) rather than framework keymap entries. Pair `AppCommandsThenTerminal` with `KeyDispatchPolicy::AppCommandsFirst` when app shortcuts should win over non-terminal widgets too.

See [`focus.md`](../focus.md) for non-terminal dispatch order and [`keybindings.md`](../keybindings.md) for Rust/file keymap precedence.

---

## TerminalPty

PTY spawner and I/O bridge. Used internally by `ManagedTerminal`.

```rust
use tui_lipan::prelude::*;

let config = TerminalPtyConfig::new("/bin/zsh")
    .cwd("/home/user")
    .term("xterm-256color");

// Spawn the PTY with an event callback
let pty = TerminalPty::spawn(config, move |event| {
    match event {
        TerminalPtyEvent::Output(bytes) => link.send(Msg::Output(bytes)),
        TerminalPtyEvent::Exited(code) => link.send(Msg::Exited(code)),
        TerminalPtyEvent::Error(msg) => link.send(Msg::Error(msg)),
    }
})?;

// Send input to the PTY
pty.write(b"ls -la\r")?;

// Read the spawned child pid when available
let pid = pty.pid();

// Resize the PTY
pty.resize(cols, rows)?;
```

**PTY env defaults**: `TERM=xterm-256color`, `COLORTERM=truecolor` (overridable via `.env(...)`).

`TerminalPty::pid()` returns the OS process id reported by the platform at spawn time, or `None` when unavailable.

---

## TerminalScreen

VT100/VT220 screen emulator (wraps `alacritty_terminal`). Maintains scrollback buffer.

```rust
// Create a screen with given dimensions and scrollback
let mut screen = TerminalScreen::new(rows, cols, scrollback_lines);

// Process PTY output
screen.process_bytes(&bytes);

// Drain terminal responses (e.g., device queries from TUI apps like fzf)
for response in screen.drain_responses() {
    pty.write(&response)?;
}

// Get a render snapshot for the Terminal widget
let snapshot = screen.render_snapshot();

// Optional: resolve default/ANSI colors through an app-owned palette
screen.set_palette(TerminalColorPalette::new(fg, bg, ansi_16));

// Or preserve the host terminal's exact ANSI palette while choosing the pane background
let host = query_host_colors();
if let Some(colors) = host {
    screen.set_palette(TerminalColorPalette::from_host_colors(colors, pane_bg));
}

// Scrollback control
screen.set_scrollback(offset);    // 0 = live view, >0 = history
screen.scrollback_offset()        // Current offset
screen.total_scrollback_rows()    // Total scrollback rows available
screen.resize(new_rows, new_cols) // Resize the terminal
```

**`TerminalRenderSnapshot` fields:**

| Field | Type | Description |
|-------|------|-------------|
| `text` | `Arc<str>` | Plain visible contents |
| `color_lines` | `Arc<[Vec<Span>]>` | Styled visible lines matching `text` logical lines |
| `cursor_row` / `cursor_col` | `u16` | Cursor position in the visible viewport |
| `cursor_visible` | `bool` | Whether cursor is shown |
| `sequence` | `u64` | Cache invalidation sequence |
| `scrollback_offset` | `usize` | Current scrollback offset |
| `total_scrollback_rows` | `usize` | Total history rows |
| `mouse_mode` | `MouseModeState` | PTY mouse/focus reporting mode |

`TerminalRenderSnapshot::from_parts(...)` rebuilds a snapshot from owned parts. It is intended for
applications that define their own versioned external snapshot transport. `TerminalRenderSnapshot`
itself is not a stable wire protocol.

### Serializable terminal snapshot leaf types

The optional `terminal-serde` feature enables `serde` derives for the terminal snapshot leaf
style/mouse types that external transports commonly need to name: `Style`, `Paint`, `Color`,
`ColorTransform`, `ContrastPolicy`, `MouseModeState`, `MouseMode`, and `MouseEncoding`. It includes
the `terminal` feature.

`terminal-serde` intentionally does **not** serialize `TerminalScreen`, `TerminalPty`, `Span`, or
`TerminalRenderSnapshot`. Types containing `Arc` should be mirrored by the application in an owned,
versioned wire format and converted back with `Span::new(...).style(...)` plus
`TerminalRenderSnapshot::from_parts(...)`.

`TerminalColorPalette` resolves SGR default foreground/background plus ANSI slots 0..15 for
render snapshots and OSC 4/10/11 color-query responses. Use
`TerminalColorPalette::from_host_colors(colors, pane_bg)` when embedding terminals that should
keep the user's real ANSI palette but still paint on an app-controlled pane background.

---

## Manual Wiring Pattern

For advanced use cases (multiple terminals, custom input routing):

```rust
pub struct MyState {
    screen: TerminalScreen,
    snapshot: TerminalRenderSnapshot,
    pty: Option<TerminalPty>,
    cols: u16,
    rows: u16,
}

// In update():
Msg::PtyOutput(bytes) => {
    ctx.state.screen.process_bytes(&bytes);
    // Forward device query responses back to PTY
    if let Some(pty) = &ctx.state.pty {
        for response in ctx.state.screen.drain_responses() {
            let _ = pty.write(&response);
        }
    }
    ctx.state.snapshot = ctx.state.screen.render_snapshot();
    Update::full()
}
Msg::TerminalInput(input) => {
    if let Some(pty) = &ctx.state.pty {
        let _ = pty.write(&input.bytes);
        // Snap to live view when user types
        if ctx.state.screen.scrollback_offset() > 0 {
            ctx.state.screen.set_scrollback(0);
            ctx.state.snapshot = ctx.state.screen.render_snapshot();
            return Update::full();
        }
    }
    Update::none()
}
Msg::Resize { cols, rows } => {
    ctx.state.cols = cols;
    ctx.state.rows = rows;
    if let Some(pty) = &ctx.state.pty {
        let _ = pty.resize(cols, rows);
    }
    ctx.state.screen.resize(rows, cols);
    ctx.state.snapshot = ctx.state.screen.render_snapshot();
    Update::full()
}

// In view():
Terminal::new()
    .snapshot(ctx.state.snapshot.clone())
    .focusable(true)
    .scroll_wheel(true)
    .on_input(ctx.link().callback(Msg::TerminalInput))
    .on_resize(ctx.link().callback(|v: TerminalViewport| Msg::Resize {
        cols: v.cols, rows: v.rows
    }))
    .on_scroll_to(ctx.link().callback(Msg::ScrollTo))
    .on_mouse_forward(ctx.link().callback(Msg::MouseForward))
    .into()
```

---

## Scrollback

Mouse wheel scrolls through scrollback history when `scroll_wheel(true)` (default in `ManagedTerminal`).

Use `on_scroll_to` to receive the new offset and call `screen.set_scrollback(offset)`.

```rust
Msg::ScrollTo(offset) => {
    ctx.state.screen.set_scrollback(offset);
    ctx.state.snapshot = ctx.state.screen.render_snapshot();
    Update::full()
}
```

The cursor is hidden while scrolled into history. Typing input snaps back to live view (set scrollback to 0).

`TerminalScreen::process_bytes()` automatically preserves the user's scrollback position when new output arrives while scrolled up - the offset is adjusted for newly added rows.
