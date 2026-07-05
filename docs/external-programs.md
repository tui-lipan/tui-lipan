# External programs (editors, pagers, subprocesses)

When your app spawns an interactive program that needs the **real TTY** (Neovim, `less`, a password prompt, etc.), the framework must temporarily give up:

- raw mode and the alternate screen (fullscreen apps),
- mouse and focus-tracking sequences,
- and, in **fullscreen** mode, the background thread that reads stdin for crossterm events.

Otherwise the subprocess and the TUI will **fight over stdin and the display** (garbled input, cursor blink on top of the editor, incomplete redraw after exit).

This page describes the supported APIs and the follow-up repaint behavior for **nested** components.

For **non-interactive** commands where the TUI should keep running and consume
stdout/stderr as data, use the native-only `process` helpers instead of terminal
handoff.

---

## `terminal_handoff` (crate root)

```rust
use std::io;
use tui_lipan::terminal_handoff::{
    resume_after_external_process,
    suspend_for_external_process,
};
```

| Function | Role |
|----------|------|
| `suspend_for_external_process(surface_mode)` | Pause the fullscreen stdin reader (when applicable), leave interactive terminal state so the child can use the TTY. |
| `resume_after_external_process(surface_mode, mouse_enabled)` | Restore raw mode, alternate screen (if not inline), mouse capture if it was enabled, resume the reader, and **request a host redraw** on the next frame (ratatui buffer clear + full draw). |

The framework consumes that request on the next tick: it promotes the frame to **full** render, runs [`Terminal::clear`](https://docs.rs/ratatui/latest/ratatui/terminal/struct.Terminal.html#method.clear) to reset the back buffer, and drops incremental scroll snapshots so the UI matches the TTY again. You can still call [`Context::request_full_repaint()`](components.md#context-methods) for other cases where the host display may be stale.

**Stale stdin:** Before the event reader is unpaused, `resume_after_external_process` drains the crossterm event queue and, on Unix, `tcflush(TCIFLUSH)` on stdin so CSI/OSC/DA tails and other mode-switch bytes are not delivered as fake key input to the focused widget.

**Parameters must match the running app:**

- `surface_mode` - the `SurfaceMode` configured on the `App` (Fullscreen, InlineEphemeral, or InlineTranscript).
- `mouse_enabled` - same as [`Context::mouse_capture_enabled`](components.md#context-methods) **at the time you suspend** (pass through to `resume_after_external_process` so mouse state is restored correctly).

**Keyboard enhancement (Kitty protocol):** suspend/resume does **not** push or pop keyboard-enhancement flags; the long-lived `TerminalGuard` still owns that. Only terminal modes needed for a typical full-screen subprocess are toggled.

Always pair suspend and resume. Prefer an RAII guard in your own code so resume runs on panic or early return:

```rust
struct Handoff {
    surface_mode: SurfaceMode,
    mouse_enabled: bool,
}

impl Drop for Handoff {
    fn drop(&mut self) {
        let _ = resume_after_external_process(
            self.surface_mode,
            self.mouse_enabled,
        );
    }
}

fn run_editor(
    surface_mode: SurfaceMode,
    mouse_enabled: bool,
) -> io::Result<()> {
    suspend_for_external_process(surface_mode)?;
    let _guard = Handoff {
        surface_mode,
        mouse_enabled,
    };
    // spawn / wait on editor...
    Ok(())
}
```

---

## Streaming non-interactive processes (`process`)

`tui_lipan::process` is available only on native targets
(`#[cfg(not(target_arch = "wasm32"))]`). It does not expose crossterm, ratatui,
or PTY types, and it is separate from the `terminal` feature/LSP integrations.

Use it for commands such as `rg`, formatters, compilers, or small shell helpers
whose stdout/stderr should become component messages:

```rust
use tui_lipan::prelude::*;

enum Msg {
    Proc(ProcessEvent),
}

// Inside update():
let command = ProcessSpec::new("sh")
    .args(["-c", "printf out; printf err >&2"])
    .command(Msg::Proc);

Update::command_only(command)
```

`ProcessEvent::Stdout(Vec<u8>)` and `ProcessEvent::Stderr(Vec<u8>)` may arrive
in chunks. `ProcessEvent::Exited(ProcessExitStatus)` is sent after both output
streams are drained. `ProcessEvent::Error(Arc<str>)` reports spawn/pipe/wait
errors without exposing `std::process::ExitStatus` in the public API.

Stdout and stderr are drained concurrently, so a child that writes heavily to
both streams will not deadlock on a full pipe. The helper is for streaming data,
not for programs that need terminal control; use `terminal_handoff` for editors,
pagers, shells, password prompts, and other interactive programs.

**Cancellation:** unkeyed `.command(...)` tasks normally run to completion. Use
`.command_keyed(key, TaskPolicy::LatestOnly, ...)` or `process_command_keyed`
when newer work should cancel a running process. Keyed process commands observe
the background cancellation token; on cancellation they kill the child, drain
stdout/stderr, and suppress stale component messages. For manual use,
`stream_process_until` accepts a cancellation predicate with the same
kill-and-drain behavior.

---

## Run blocking work on the **UI thread**

[`Command::spawn`](components.md#commands-async--background-work) and `Link::command(...)` run closures on a **worker thread**. That is wrong for `terminal_handoff`: the main thread still holds the ratatui terminal and keeps drawing.

Use **`Command::new`** so suspend → subprocess → resume runs on the **same thread** as the event loop:

```rust
use tui_lipan::prelude::*;

// Inside update():
let link = ctx.link().clone();
let surface_mode = ctx.surface_mode();
let mouse_enabled = ctx.mouse_capture_enabled();
let initial = ctx.state.draft.clone();

Update {
    dirty: false,
    command: Some(Command::new(move || {
        match run_my_editor(&initial, surface_mode, mouse_enabled) {
            Ok(text) => link.send(Msg::EditorDone(text)),
            Err(e) => link.send(Msg::EditorFailed(e)),
        }
    })),
}
```

Use `link.send(...)` inside the closure to push follow-up messages; they are processed in the same message drain as other updates.

---

## Force a **full** redraw after handoff

When a nested child returns [`Update::full()`](components.md#the-update-return-type), the runtime may schedule a **layout-only** reconcile for that scope. After the host terminal was repainted by another process, that is often **not enough** to refresh the entire frame.

Call **`Context::request_full_repaint()`** from the message handler that runs **after** the external program exits (success or failure), before or alongside your usual state updates:

```rust
Msg::EditorDone(text) => {
    ctx.request_full_repaint();
    ctx.state.draft = text;
    Update::full()
}
```

On the next loop iteration the runner promotes the frame to a **full** render (full reconcile + draw), not only a nested layout pass.

---

## Summary checklist

1. Use `suspend_for_external_process` / `resume_after_external_process` with correct `surface_mode` and `mouse_enabled`.
2. Run that sequence on the UI thread via `Command::new`, not `Command::spawn` / `link.command`.
3. After returning to the TUI, call `ctx.request_full_repaint()` when a full frame repaint is required (especially for nested components).
