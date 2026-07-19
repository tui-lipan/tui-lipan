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
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
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
| `tab_stop` | `bool` | Include in sequential Tab traversal (default: `true`) |
| `on_focus` / `on_blur` | `Callback<()>` | Focus gained / lost |
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

## Key encoding

Once a key is routed to the terminal, `key_event_to_bytes(key, modes)` turns it into the bytes written to the child's stdin. `modes` is a `TerminalKeyModes`, which carries the modes the child has negotiated — see [Input modes](#input-modes) below.

Cursor, navigation, and function keys carry `Ctrl`/`Shift` as an xterm modifier parameter of `1 + shift + 2·alt + 4·ctrl` — so `Shift` is `2`, `Ctrl` is `5`, `Ctrl+Shift` is `6`, `Ctrl+Alt` is `7`.

| Key | Unmodified | Modified |
|-----|-----------|----------|
| `Up` / `Down` / `Right` / `Left` | `CSI A` / `B` / `C` / `D` | `CSI 1;<mod>A` … `D` |
| `Home` / `End` | `CSI H` / `CSI F` | `CSI 1;<mod>H` / `CSI 1;<mod>F` |
| `Insert` / `Delete` | `CSI 2~` / `CSI 3~` | `CSI 2;<mod>~` / `CSI 3;<mod>~` |
| `PageUp` / `PageDown` | `CSI 5~` / `CSI 6~` | `CSI 5;<mod>~` / `CSI 6;<mod>~` |
| `F1`–`F20` | `CSI 11~` … `CSI 34~` | `CSI 11;<mod>~` … `CSI 34;<mod>~` |
| `Backspace` | `DEL` | `ESC DEL` (with `Ctrl`) |
| `Enter` / `Tab` / `Esc` | `CR` / `HT` / `ESC` | unchanged (legacy) |

Three deliberate exceptions:

- **`Alt` on its own** keeps the historical ESC-prefix encoding (`Alt+Left` is `ESC` followed by `CSI D`), matching xterm's `metaSendsEscape`. Combined with `Ctrl` or `Shift` it folds into the modifier parameter instead.
- **`Shift+Insert`, `Shift+PageUp`, and `Shift+PageDown`** keep their unmodified bytes. A terminal emulator conventionally consumes these for paste and scrollback, but the `Terminal` widget forwards them (its scrollback runs on the wheel and `on_scroll_to`), and children do not recognize the parameterized form. Sending `CSI 5;2~` would make `Shift+PageUp` a no-op rather than paging the child. Adding `Ctrl` lifts the exception.
- **`Super`-modified keys** produce no bytes at all. `Super` has no representation in any of these encodings, and forwarding the unmodified key would type a character the user never pressed. `key_event_to_bytes` returns `None` and the chord bubbles to the app.

`Ctrl+Backspace` sends `ESC DEL` — readline's `backward-kill-word`, the same bytes as `Alt+Backspace` — so it deletes the previous word out of the box.

### Char keys

`Ctrl+<letter>` maps to its C0 control code (`Ctrl+A` → `0x01`); everything else is sent as UTF-8. `Ctrl` also maps the punctuation that has a control code:

| Chord | Byte | | Chord | Byte |
|-------|------|-|-------|------|
| `Ctrl+Space`, `Ctrl+@`, `Ctrl+2` | `0x00` | | `Ctrl+^`, `Ctrl+6` | `0x1e` |
| `Ctrl+[`, `Ctrl+3` | `0x1b` | | `Ctrl+_`, `Ctrl+/`, `Ctrl+7` | `0x1f` |
| `Ctrl+\`, `Ctrl+4` | `0x1c` | | `Ctrl+?`, `Ctrl+8` | `0x7f` |
| `Ctrl+]`, `Ctrl+5` | `0x1d` | | | |

A `Ctrl+<char>` chord with no control code (`Ctrl+1`, `Ctrl+;`) has no legacy encoding: it returns `None` and stays available to the app **unless** the child has negotiated the Kitty keyboard protocol (below), which can carry it.

Note that in the legacy encoding `Ctrl+Shift+C` produces `0x03`, identical to `Ctrl+C` — a control code has no shift bit. The `Terminal` widget never reaches that path, because the clipboard preflight consumes the chord first. Code calling `key_event_to_bytes` directly must do the same. Under the Kitty protocol the two are distinct (`CSI 99;6u` versus `CSI 99;5u`).

### Kitty keyboard protocol

Chords like `Ctrl+1`, `Ctrl+Enter`, and `Shift+Enter` have **no** legacy terminal encoding. There is no byte sequence to send a shell for `Ctrl+1`, which is why pressing it in a plain shell does nothing anywhere. They become expressible only under the [Kitty keyboard protocol], which a child turns on by pushing flags with `CSI > <flags> u`.

tui-lipan's own backend pushes `DISAMBIGUATE_ESCAPE_CODES | REPORT_EVENT_TYPES` on startup (when the host terminal supports it), so **a tui-lipan app running inside a `Terminal` widget negotiates the protocol automatically.** When `modes.kitty_keyboard.disambiguate_escape_codes` is set, keys with no unambiguous legacy encoding switch to `CSI <codepoint>;<mod> u`:

| Chord | Bytes |
|-------|-------|
| `Ctrl+1` … `Ctrl+9` | `CSI 49;5u` … `CSI 57;5u` |
| `Ctrl+Enter` / `Shift+Enter` | `CSI 13;5u` / `CSI 13;2u` |
| `Ctrl+Tab` | `CSI 9;5u` |
| `Ctrl+Backspace` | `CSI 127;5u` |
| `Ctrl+C` / `Ctrl+Shift+C` | `CSI 99;5u` / `CSI 99;6u` |
| `Esc` | `CSI 27u` |

The codepoint is the key **as engraved** (lowercase, no shift applied); Shift lands in the modifier parameter. Text keys still arrive as text — `Shift` alone never promotes a key to the escape form — and the arrows, tilde keys, and function keys keep their unambiguous legacy sequences. Only `Ctrl`/`Alt` (or a chord that has no other encoding, like `Esc`) triggers `CSI u`.

Sending these to a child that has **not** negotiated the protocol would be wrong: a crossterm-based reader rejects the unknown sequence and discards it. So the encoder falls back to the legacy bytes above until the child asks, and a chord with no legacy form is simply dropped (`None`).

[Kitty keyboard protocol]: https://sw.kovidgoyal.net/kitty/keyboard-protocol/

### Input modes

`TerminalKeyModes` carries the modes that change what a key press or a paste must produce. `TerminalRenderSnapshot` publishes it, `Terminal::snapshot` picks it up automatically, and `TerminalScreen::key_modes()` exposes it for manual wiring.

| Field | Mode | Effect |
|-------|------|--------|
| `app_cursor` | DECCKM (`CSI ? 1 h`) | Unmodified cursor keys are introduced by `SS3` (`ESC O A`) instead of `CSI` (`ESC [ A`). Modified cursor keys and the tilde keys are unaffected. |
| `bracketed_paste` | `CSI ? 2004 h` | `encode_paste` wraps pasted text in `CSI 200~` / `CSI 201~`. |
| `kitty_keyboard` | `CSI > <flags> u` | The negotiated [Kitty keyboard protocol](#kitty-keyboard-protocol) flags (a `KittyKeyboardFlags`). |

These matter for correctness rather than polish. ncurses emits `smkx` (`ESC [ ? 1 h ESC =`) on startup and then matches arrows against terminfo's `kcuu1=\EOA`, so a child in application-cursor mode is waiting for `ESC O A`. A child that never enabled bracketed paste does not strip the wrapper, so pasting into it would insert the literal bytes `ESC [ 200 ~`. And a chord like `Ctrl+1` cannot reach the child at all until it negotiates the Kitty protocol.

```rust
// Manual wiring: take the modes from the screen, hand them to the encoder.
Msg::Key(key) => {
    let modes = ctx.state.screen.key_modes();
    ctx.state.pty.send_key(key, modes).ok();
    Update::none()
}
```

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

`TerminalPty::foreground_process_group_id()` (Unix-only) returns the PTY's foreground process-group
id (`tcgetpgrp(3)`) without exposing the underlying master file descriptor - a building block for a
native foreground-process fallback when no shell integration/OSC 133 metadata is available.

Cloning a `TerminalPty` shares the same child process; dropping one clone only kills the child once
every clone has been dropped, not on the first one.

### Unix PTY handoff

On Unix, advanced terminal-host apps that move a live PTY between processes can call `TerminalPty::handoff()` before transferring the master fd over their own IPC channel:

```rust
#[cfg(unix)]
let handoff = pty.handoff()?;
```

`TerminalPtyHandoff` contains the raw master fd and optional child pid. The token keeps the original PTY master alive until it is dropped; pass or duplicate the fd before dropping it. After handoff, the original `TerminalPty` stops forwarding output, rejects writes/resizes, and will not kill the child on drop. The receiving process is responsible for reading, writing, resizing, and terminating the adopted PTY.

`handoff()` waits for the local reader thread to stop before returning, so the receiving process can start reading without racing the old owner. Calling `kill()` after handoff is a no-op because ownership has moved to the receiver.

This is intentionally a low-level Unix-only escape hatch for multiplexers and session managers. Ordinary apps should keep using `ManagedTerminal` or `TerminalPty::spawn`.

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

// Serialize the full state so a fresh same-sized screen can reproduce it
let replay = screen.export_replay_bytes();
```

**`TerminalRenderSnapshot` fields:**

| Field | Type | Description |
|-------|------|-------------|
| `text` | `Arc<str>` | Plain visible contents |
| `color_lines` | `Arc<[Vec<Span>]>` | Styled visible lines matching `text` logical lines |
| `cursor_row` / `cursor_col` | `u16` | Cursor position in the visible viewport |
| `cursor_visible` | `bool` | Whether cursor is shown |
| `cursor_shape` | `CaretShape` | Shape requested by the child via `DECSCUSR` |
| `cursor_blinking` | `bool` | Whether the child requested a blinking cursor |
| `sequence` | `u64` | Cache invalidation sequence |
| `scrollback_offset` | `usize` | Current scrollback offset |
| `total_scrollback_rows` | `usize` | Total history rows |
| `mouse_mode` | `MouseModeState` | PTY mouse/focus reporting mode |
| `key_modes` | `TerminalKeyModes` | PTY input modes (DECCKM, bracketed paste) |

`TerminalRenderSnapshot::from_parts(...)` rebuilds a snapshot from owned parts. It is intended for
applications that define their own versioned external snapshot transport. `TerminalRenderSnapshot`
itself is not a stable wire protocol.

### Cursor shape and blinking

The focused `Terminal` renders the real hardware cursor. It follows the cursor shape the child
program requests through `DECSCUSR` (`CSI Ps SP q`): a block (`1`/`2`), underline (`3`/`4`), or bar
(`5`/`6`), mapped to `CaretShape`. Odd codes request blinking and even codes request a steady
cursor; the widget honors that preference (blinking is driven by the framework blink timer, so a
steady cursor stays lit). A child that never issues `DECSCUSR` falls back to a blinking block. These
fields flow through `TerminalRenderSnapshot` and can be overridden directly with
`Terminal::cursor_shape()` / `Terminal::cursor_blinking()`.

### Semantic state (working directory & command lifecycle)

`TerminalScreen` runs a second, independent OSC observer beside the grid parser, fed the same raw
bytes `process_bytes()` receives. It never touches cell/cursor state and is not part of
`TerminalRenderSnapshot` - it is runtime metadata a host app polls separately:

```rust
screen.process_bytes(&bytes);

// Current accumulated state (cwd, command phase, foreground executable).
let state = screen.semantic_state();

// Or react only to what changed since the last drain.
for event in screen.drain_semantic_events() {
    match event {
        TerminalSemanticEvent::WorkingDirectoryChanged(cwd) => { /* cwd.path, cwd.host */ }
        TerminalSemanticEvent::CommandPhaseChanged(phase) => { /* ... */ }
        TerminalSemanticEvent::ExecutableChanged(exe) => { /* ... */ }
    }
}

// Reapply previously captured state (e.g. after recreating a screen for session
// resurrection) without replaying the escape sequences that originally produced it.
screen.restore_semantic_state(state);
```

Recognized sequences:

- **`OSC 7 ; file://host/path`** (percent-encoded) reports the child's working directory.
  `TerminalWorkingDirectory::path` holds the percent-decoded path as raw bytes rather than a lossy
  UTF-8 string, since arbitrary filenames are valid `OsStr` data but not necessarily valid UTF-8;
  use `path_str()` for the common UTF-8 case. `host` is `Some(..)` when the child reported a
  non-empty host component (e.g. over SSH) - a caller must not treat that path as locally
  spawnable without first checking the host.
- **`OSC 9 ; 9 ; path`** reports a Windows-style working directory (ConEmu/Windows Terminal
  convention); the path is taken as-is (no percent-decoding, no host).
- **`OSC 133 ; A/B/C/D`** reports command lifecycle boundaries: `A` = prompt start, `B` = prompt
  end / input start, `C` = execution start, `D[;exit_code]` = command finished, surfaced as
  `TerminalCommandPhase::{Prompt, Input, Executing, Completed { exit_status }}`.
- Two `OSC 133` key/value extensions report foreground-executable identity - never a full command
  line - as a normalized basename: hyprmux's own `hyprmux_exe=<percent-encoded name>`, and Fish/
  Kitty's `cmdline_url=<percent-encoded command line>` (only the first token's basename is kept).

`TerminalScreen::reset()` clears in-flight parser state but preserves accumulated semantic state -
a child hard reset (RIS) does not imply its last-known working directory or command lifecycle
became invalid.

### Exporting replay bytes

`TerminalScreen::export_replay_bytes()` serializes the current screen state as a VT byte stream.
Feeding that stream into a fresh, same-sized `TerminalScreen` (via `process_bytes`) reproduces the
state, because replay goes through the normal VTE parser rather than a parallel snapshot format —
so future parser fixes apply to exported state automatically.

```rust
// On the source (e.g. a server-owned terminal):
let replay = source.export_replay_bytes();

// On a fresh receiver of the same size (`scrollback_lines` is the app's own capacity):
let mut screen = TerminalScreen::new(source.rows, source.cols, scrollback_lines);
screen.set_palette(source.palette());
screen.process_bytes(&replay);
let _ = screen.drain_responses(); // the source already answered device queries
```

The stream captures scrollback, primary/alternate screen contents, the cursor position and pen
template, the title, and common terminal modes. It is a replay stream, not a stable data format:
tab stops, custom scrolling regions, cursor style, the kitty keyboard stack, hyperlinks, and the
display offset are intentional non-goals — the receiver lands on the live view. This is the seeding
mechanism a terminal host uses to bring a newly attached client up to date with a live terminal it
does not own directly.

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

---

## Plain-text export

`TerminalScreen` can read out its contents as plain text without touching the display
offset or running the render pipeline, so exporting never disturbs what the user is
looking at.

Lines are addressed by **absolute index**, counting from the oldest line still retained
(`0`) through the live bottom (`total_text_lines() - 1`):

| Method | Returns |
|--------|---------|
| `total_text_lines()` | Retained line count (scrollback history + visible screen) |
| `text_lines(start, end)` | `Vec<String>` for the half-open range `[start, end)` |
| `export_text(start, end)` | The same range joined with `\n` |
| `absolute_line_to_viewport(abs)` | `Option<(scrollback_offset, viewport_row)>` |
| `absolute_line_to_offset(abs)` | `Option<usize>` scrollback offset alone |

Out-of-range bounds are clamped and empty ranges yield empty output. Trailing blanks
are trimmed per line, and wide-character spacer cells are skipped, so exported text
matches what is on screen rather than the raw cell grid.

```rust
// Copy the whole buffer.
let all = screen.export_text(0, screen.total_text_lines());

// Copy just the last 20 lines.
let total = screen.total_text_lines();
let tail = screen.export_text(total.saturating_sub(20), total);
```

> **Absolute indices are not stable across output.** They are relative to the oldest
> *retained* line, so once scrollback is full every new line shifts existing indices
> down by one. Resolve a range and use it promptly rather than storing indices across
> reads.

## Semantic marks (OSC 133)

When the shell emits [OSC 133](https://gitlab.freedesktop.org/Per_Bothner/specifications/blob/master/proposals/prompts-data-model.md)
prompt markers, `TerminalScreen` records them against the same absolute line space:

```rust
pub struct SemanticMark {
    pub kind: SemanticMarkKind,   // Prompt | OutputStart | OutputEnd
    pub absolute_line: usize,
    pub exit_status: Option<i32>, // from OSC 133;D
}
```

| Method | Returns |
|--------|---------|
| `semantic_marks()` | Retained marks, oldest first |
| `last_command_output_range()` | `Option<(start, end)>` for the last command's output |
| `export_last_command_output()` | That range as text |

While a command is still running (an `OutputStart` with no matching `OutputEnd`), the
range extends to the live bottom.

```rust
if let Some(output) = screen.export_last_command_output() {
    ctx.copy_to_clipboard(&output);
}
```

Behaviour worth knowing:

- **Marks are bounded** (256 retained); the oldest are discarded first.
- **Evicted marks are dropped.** When a marked line falls out of scrollback the mark
  goes with it, so `last_command_output_range()` returns `None` rather than a range
  pointing at whatever text now occupies those indices. A command whose output is
  longer than the scrollback depth is therefore not recoverable - size scrollback for
  the output you intend to export.
- **Alt-screen marks are ignored.** Full-screen programs (`vim`, `less`) emitting
  OSC 133 do not contribute marks, since the alt screen has no scrollback of its own.
- `reset()` clears all marks.

---

## Image passthrough (roadmap)

Kitty graphics / sixel image display is **not implemented**. See the design doc:
[terminal-images.md](terminal-images.md).
