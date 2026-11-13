# Clipboard

## Feature Flag

The clipboard is **enabled by default** via the `clipboard` feature (backed by `arboard`):

```toml
# Default: clipboard enabled (no extra config needed)
tui-lipan = { version = "*" }

# Opt out for minimal builds with no system clipboard dependency
tui-lipan = { version = "*", default-features = false }

# Re-enable clipboard alongside other features
tui-lipan = { version = "*", default-features = false, features = ["clipboard", "image"] }
```

When the `clipboard` feature is disabled, all clipboard operations silently return `ClipboardError::Unsupported` - the API surface is identical.

## ClipboardConfig

Configure clipboard behavior via `App::clipboard_config(...)`:

```rust
use tui_lipan::prelude::*;

App::new()
    .clipboard_config(ClipboardConfig {
        enable_performable_ctrl_c_copy: true,  // Bind Ctrl+C to copy when selection exists
        enable_primary_selection: true,         // Linux primary selection
        paste_shift_insert_behavior: PasteShiftInsertBehavior::PrimarySelection,
        copy_on_mouse_select: CopyOnSelect::PrimarySelection,
        middle_click_paste: PasteSource::PrimarySelection,
        right_click_action: RightClickAction::Disabled,
        paste_max_bytes: 1_000_000,            // Clamp large text pastes
        enable_osc52: true,                    // OSC52 for SSH clipboard
        paste_max_image_bytes: 10_000_000,     // Clamp large image pastes (default 10MB)
        copy_feedback: CopyFeedback::new(CopyFeedbackEffect::Flash), // Highlight after a copy
    })
    .mount(Root)
    .run()
```

For preferences that can change while an app is running, update the complete policy from a
component message:

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    if let Msg::ClipboardConfigChanged(config) = msg {
        ctx.set_clipboard_config(config);
    }
    Update::none()
}
```

`Context::set_clipboard_config` normalizes the requested policy against the active clipboard
provider, publishes the effective value to every component context, and updates mouse behavior and
clipboard-derived key bindings at the end of the current update. It preserves the app's keymap
path, user-keymap policy, and `FrameworkKeymap` overrides. Use `ctx.clipboard_config()` to read the
effective normalized policy; for example, a request for primary selection may read back as regular
clipboard or disabled when the provider has no primary-selection support. The setter requires a
mutable context, so call it from lifecycle methods such as `init()` or `update()`, not `view()`.

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `enable_performable_ctrl_c_copy` | `bool` | `true` | Bind `Ctrl+C` to copy when selection exists; otherwise it falls through |
| `enable_primary_selection` | `bool` | platform | Enable the Linux primary-selection clipboard |
| `paste_shift_insert_behavior` | `PasteShiftInsertBehavior` | platform | `PrimarySelection` or `Clipboard` |
| `copy_on_mouse_select` | `CopyOnSelect` | platform | Clipboard target updated when a mouse text selection completes |
| `middle_click_paste` | `PasteSource` | platform | Clipboard source pasted by middle click |
| `right_click_action` | `RightClickAction` | `Disabled` | Fallback clipboard action for an otherwise-unhandled right click |
| `paste_max_bytes` | `usize` | unbounded | Clamp large text pastes to avoid stalls |
| `enable_osc52` | `bool` | `true` | Emit OSC52 escape on copy/cut (useful over SSH) |
| `paste_max_image_bytes` | `usize` | 10MB | Clamp large image pastes |
| `copy_feedback` | `CopyFeedback` | 150 ms flash | How a successful selection copy is highlighted, and whether the selection is cleared afterwards. See [Copy feedback](#copy-feedback) |

On local Linux desktops, `copy_on_mouse_select` and `middle_click_paste` default to
`PrimarySelection`. This restores the traditional Unix workflow that terminal mouse reporting
otherwise intercepts:

```text
left-drag selection -> PRIMARY
middle click        -> paste PRIMARY
Ctrl+C / Ctrl+V     -> regular CLIPBOARD
```

The regular clipboard is deliberately left unchanged by the default selection gesture. Apps that
want selection followed by `Ctrl+V` can choose `CopyOnSelect::Clipboard` or
`CopyOnSelect::Both`. On macOS, Windows, web, and Linux sessions without a detected display,
mouse copy and middle-click paste default to disabled.

`RightClickAction::PasteClipboard`, `RightClickAction::CopyOrPaste`, and
`RightClickAction::CopyOnly` are opt-in. Existing overlay, nested-terminal, `TextArea::on_click`,
and right-drag handlers take precedence; the configured action is only a fallback for an
otherwise-unhandled right-button press.

The copy highlight follows intent, not input device. A deliberate copy, whether a copy shortcut, a
`CopyOrPaste` right click, or a `CopyOnly` right click, highlights the copied selection once the
write succeeds, so users can tell a right-click copy from a right-click paste. Copy-on-select stays
silent by default because it fires on every completed selection; set `CopyFeedback::on_select` to
highlight it too. A failed right-click copy reports the clipboard error and does not fall through
to a paste. `CopyOnly` never pastes, whether or not the copy succeeds.

### Copy feedback

`ClipboardConfig::copy_feedback` decides what a successful selection copy looks like:

```rust
use std::time::Duration;
use tui_lipan::prelude::*;

let feedback = CopyFeedback::new(CopyFeedbackEffect::Scan) // default duration for the effect
    .duration(Duration::from_millis(400))
    .style(Style::new().lighten_by(0.4))
    .on_select(true)        // also highlight copy-on-select
    .clear_selection(true); // drop the selection once the highlight ends
```

| Effect | What the copied selection does |
|--------|--------------------------------|
| `Off` | Nothing |
| `Flash` | Lights up at once and goes back when the time is up (150 ms) |
| `Fade` | Lights up at once, then eases back to the plain selection (300 ms) |
| `Scan` | A soft vertical line sweeps across it from left to right (450 ms) |
| `Pulse` | Lights up twice (420 ms) |

`CopyFeedbackEffect::default_duration` gives the times in brackets, which `CopyFeedback::new`
uses. A zero `duration` turns the highlight off. `CopyFeedbackEffect` parses from and displays as
its lowercase name, so a config file can name it directly.

`style` is the strongest highlight, merged onto the widget's selection style; the default lightens
it by `0.35`. `Fade`, `Scan`, and `Pulse` scale its color transforms (`lighten_by`, `dim_by`,
`tint_by`, opacity) toward nothing. Absolute colors and text modifiers cannot be partly applied, so
they stay on for the brighter half of the effect. Animated effects repaint on their own, at about
60 frames per second, only while they run.

`Scan` sweeps across the full width of a `Terminal` at one speed, lighting the copied cells as it
passes, so a short selection lights briefly rather than slowing the line down. Widgets that wrap
or reflow text, such as `Input`,
`TextArea`, and `DocumentView`, fade instead. Toasts show the full `style` for `duration` with every
effect except `Off`.

With `clear_selection`, the copied selection is cleared once the highlight ends, or straight away
when there is no highlight. Clearing goes through the widget's change callback, as a click
elsewhere does. A selection the user has changed since the copy is left alone. A `DocumentView`
that scrolls out of a `ScrollView` before the highlight ends is cleared all the same, so it does
not come back selected. Copies made with
`flash_copy_feedback_range` never clear anything, because that range is not the widget's selection.

Mouse clipboard gestures act on the widget under the pointer, never on whatever happens to hold
focus:

- Middle-click paste and right-click paste target the editable `Input`, `TextArea`, or `Terminal`
  under the pointer. If that widget is not focused, it takes focus first, so the paste lands where
  the user clicked, the way a terminal emulator or tmux pastes into the pane under the mouse. A
  widget that cannot take pointer focus, for example because `FocusPolicy::Manual` is set or it is
  not focusable, receives nothing. The caret is not repositioned.
- A `CopyOrPaste` right click copies only the selection owned by the widget under the pointer. For a
  `DocumentView` with a `shared_selection_id`, that is the selection of its own group in its scroll
  view, including rows scrolled off screen; other groups in the same scroll view are left alone. If
  the pointer is not on a selection, the click falls through to a paste at the pointer.
- A `CopyOnly` right click copies that same selection to the regular clipboard. When the widget
  under the pointer has no selection, the click does nothing: it does not paste, and it does not
  move focus. A selection owned by some other widget stays where it is.
- A press over anything else, such as a status bar, tab strip, frame border, or empty space, does
  nothing.

Apps that track focus themselves can follow a paste-driven focus change through
`Component::on_focus_changed`, just like a left click.

Copy-on-select runs once when a non-empty mouse selection is completed, including word and line
selections, rather than on every drag update.

Primary-selection mouse behavior currently uses the local clipboard provider. Regular clipboard
writes can still use OSC 52, but remote PRIMARY reads and OSC 52 `p` writes are not performed.

All clipboard shortcuts are performable by default: copy/cut only consume when the action can run on
a selection, and paste only consumes when the focused widget can accept pasted content. Copy
shortcuts such as `Ctrl+C` and `Ctrl+Insert` also copy any active mouse selection from `Input`,
`TextArea`, `DocumentView`, or `Terminal`, even when those widgets are not focusable. Editable
`Input` and `TextArea` selections can also be cut with cut shortcuts such as `Ctrl+X`.
Otherwise the key falls through to app-level handlers.

Focus decides which selection a copy or cut shortcut takes:

- The focused widget's own selection always comes first.
- A focused `Terminal` owns selection lookup. Selections in other widgets, such as another
  terminal pane, are ignored, so a leftover selection elsewhere never turns `Ctrl+C` into a copy.
  If the terminal's own selection does not handle the shortcut, the key continues through the
  configured [`TerminalKeyPolicy`](widgets/terminal.md): under `AppCommandsThenTerminal` an app
  command bound to the key runs first, and otherwise the terminal forwards it to its child.
- Any other focus, or no focus at all, can still copy a selection made elsewhere. An app can keep
  focus in an input while the user selects and copies text from a log.

This behavior is independent of app focus policy. Under the default unfocused
`FocusPolicy::OnDemand` state, an existing mouse selection can still be copied. `Manual` prevents
click-to-focus but does not disable mouse selection or performable copy. Setting
`Theme::focus_decoration(false)` changes only visuals and does not affect clipboard routing.

Native terminal bracketed-paste events are also routed through the same focused-widget paste path.
That means dropping files or pasting large/quoted text directly into a terminal running tui-lipan
reaches `Input`, `TextArea`, or `Terminal` widgets as a paste instead of raw keystrokes.

Terminal-host applications can set
`Terminal::paste_shortcut_behavior(TerminalPasteShortcutBehavior::Performable)` to make direct
`Ctrl+V` paste text locally while forwarding the key for file lists, images, or unknown non-text
clipboard content. Enable `clipboard-images` when image data may also advertise a text fallback and
must still be recognized as rich content. Wayland classification checks the advertised MIME types
without reading image bytes. Arboard does not expose the equivalent presence query on X11, macOS,
or Windows, so those backends currently decode an advertised image during classification.

For `DocumentView`, when siblings inside the same `ScrollView` share
`shared_selection_id`, copy shortcuts copy a single concatenated selection for
that shared group (in visual order), including selections temporarily virtualized
out of the live tree by parent `ScrollView` scrolling. Groups with different ids
are copied independently.

## Programmatic Access

Use `ctx.clipboard()` from any component to copy or read text programmatically:

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::CopyClicked => {
            if let Err(e) = ctx.clipboard().copy("copied text") {
                ctx.toast().error(format!("Copy failed: {e}"));
            }
        }
        Msg::Paste => {
            match ctx.clipboard().read() {
                Ok(text) => { /* use text */ }
                Err(e) => { /* handle error */ }
            }
        }
    }
    Update::default()
}
```

`ClipboardHandle` returned by `ctx.clipboard()` respects the app-level `ClipboardConfig` - it automatically emits OSC 52 when enabled and writes to the primary selection on supported platforms.
When OSC 52 is enabled, a native clipboard-provider failure does not fail `copy()`: the outer
terminal still received the copy request. Terminal hosts that already applied their own policy to a
parsed child request can call `relay_osc52()` to emit only the outer-terminal sequence.
`accept_osc52_store()` applies a parsed child request only when `enable_osc52` is enabled, returning
`Ok(false)` without touching the native clipboard otherwise. `ManagedTerminal` uses this policy.

### Showing framework clipboard failures

Copies and pastes the framework performs itself, such as the keymap's paste action,
copy-on-select and right-click copy, report failures to the
`App::clipboard_reporter`, which only logs by default. To tell the user, install a
handler. It runs after the reporter and receives the toast API:

```rust
App::new().on_clipboard_error(|error, toasts| {
    toasts.push(Toast::new(error.user_message()));
});
```

`ClipboardError::user_message()` names what the user tried, for example "Reading the
clipboard is not supported on this system" or "Could not copy to the clipboard:
...". `ClipboardError::operation()` returns a `ClipboardOperation`, and
`ClipboardOperation::is_read()` separates pastes from copies.

### OSC 52 under a multiplexer

A multiplexer sits between the app and the real terminal emulator, so the escape needs one extra
hop and the framing is chosen from the environment:

| Environment | Framing written |
| --- | --- |
| `$TMUX` set | the bare escape **and** the tmux DCS passthrough copy |
| `$STY` set (GNU screen) | the DCS passthrough copy only |
| neither | the bare escape |

tmux gets both because its two forwarding mechanisms consume different bytes: `set-clipboard`
(default `external`) forwards a bare OSC 52 outward, while `allow-passthrough` (added in tmux 3.3,
default **off**) forwards the wrapped copy verbatim. Writing both means whichever one the user has
enabled performs the copy; if both are on, the clipboard is set twice to the same text. GNU screen
has no `set-clipboard` equivalent, so the passthrough is the only framing it forwards.

A multiplexer that hosts panes with tui-lipan's own terminal widget does not need any of this: it
parses a child's bare OSC 52 into a `TerminalClipboardEvent` directly. Such a host should avoid
leaking an outer `$TMUX` into its panes' environment, since that would make children emit the tmux
framing for a multiplexer that is not tmux.
Call `ctx.flash_copy_feedback(node_id)` after a successful programmatic copy to reuse the
configured copy highlight on that exact widget. For the focused widget, obtain `node_id` with
`ctx.focused_node_id()` before requesting the flash.

`flash_copy_feedback` paints whatever the widget currently has selected. If your app copies and
then immediately leaves its selection mode, use
`ctx.flash_copy_feedback_range(node_id, range)` instead: it captures the copied range and paints
that for the flash duration, so the selection can be cleared straight away rather than being held
alive purely to give the flash something to draw. Columns are display columns, matching the
renderer.

## File Clipboard

`copy_files` puts real files on the clipboard rather than their paths as text. Pasting into a file
manager, a file dialog, or a browser upload target yields the files themselves:

```rust
match ctx.clipboard().copy_files(&["src/main.rs", "Cargo.toml"]) {
    Ok(()) => ctx.toast().push(Toast::new("Copied - paste into a file manager")),
    Err(e) => ctx.toast().push(Toast::new(format!("Copy failed: {e}"))),
}

// Read a file list someone else put on the clipboard.
let paths: Vec<PathBuf> = ctx.clipboard().read_files()?;  // empty vec = no file list
```

| Method | Purpose |
|--------|---------|
| `copy_files(&[impl AsRef<Path>])` | Place files on the clipboard |
| `read_files()` | Read a file list; empty `Vec` means the clipboard holds none |
| `supports_files()` | Whether the provider can exchange file lists at all |

Paths are resolved to absolute form, so relative paths resolve against the current working
directory. A path that does not exist fails the **whole** call with
`ClipboardError::InvalidInput` naming it - the platform clipboards drop unresolvable entries
silently, which would otherwise copy a shorter list than you asked for without telling you.

Unlike `copy`, this never emits OSC 52: that escape carries plain text only and cannot express a
file list, so emitting it would silently downgrade the copy to a path string. Over SSH, copy the
path with `copy` and accept that it lands as text.

Gate any "copy file" affordance on `supports_files()` - it is `false` on the web backend and in
builds without the `clipboard` feature, so you can hide the option instead of surfacing an error
after the user asks for it.

### Why there is no drag-and-drop out of the terminal

A common follow-up is whether a file can be *dragged* out of a TUI into another application. It
cannot. The OS drag protocols - XDND on X11, `wl_data_device` on Wayland, `NSDraggingSession` on
macOS, OLE on Windows - are driven by the window that owns the pointer grab, and that window
belongs to the terminal emulator, not to the process drawing inside it. A TUI receives mouse input
as escape sequences carrying **cell** coordinates, and reporting stops entirely once the pointer
leaves the terminal. No terminal protocol exposes a "begin a native drag" request.

Dragging *in* works because the emulator acts as the drop target and pastes the path for you; the
source side has no equivalent. `copy_files` is the closest portable substitute, and it also works
where a GUI helper cannot, such as over SSH to a machine with no display.

If you specifically need the drag *gesture*, spawn a small GUI helper that owns its own window and
can act as the drag source - `ripdrag` or `dragon-drop`, which is what `ranger`, `lf`, and `nnn`
do. That is application-level glue rather than framework API; see `examples/lazygit.rs` for a
working version behind the `D` key.

## Image Clipboard *(requires feature `image` or `clipboard-images`)*

```rust
use tui_lipan::{ImageContent, ImageFormat};
```

**Reading images from clipboard** is handled automatically by `TextArea` when image callbacks are set. When the user pastes and the clipboard contains an image, the framework invokes the `on_images_change` or `on_image_paste` callback with the decoded `ImageContent`.

**TextArea image integration (recommended pattern):**

```rust
// Inline mode: sentinel chars in text value
TextArea::new(self.input.clone())
    .image_mode(TextAreaImageMode::Inline)
    .images(self.images.clone())
    .on_images_change(ctx.link().callback(Msg::ImagesChanged))
    .image_placeholder("[Img]")
    .image_placeholder_style(Style::new().fg(Color::Magenta).bold())
```

**ImageContent API:**

```rust
let content: ImageContent = ...;
content.mime      // e.g. "image/png"
content.data      // base64-encoded string

// Decode to raw bytes
let bytes = content.to_bytes()?;
let arc_bytes: Arc<[u8]> = Arc::from(bytes.as_slice());

// Use with Image widget
Image::from_bytes(arc_bytes)
```

**ImageFormat:** `ImageFormat::Png`, `ImageFormat::Jpeg`

Images are automatically converted to/from RGBA format for clipboard compatibility. Supported on Linux (X11/Wayland), macOS, and Windows.
The default image-backed feature set enables PNG, JPEG, GIF, and WebP codecs;
add `image-full-formats` when decoding or encoding less common formats through
the `image` crate.

## TextArea Image Modes

See [`docs/widgets/input.md`](widgets/input.md#textarea) for complete `TextAreaImageMode` documentation.

| Mode | Behavior |
|------|----------|
| `TextAreaImageMode::Inline` | Images embedded as Unicode PUA sentinels in text value |
| `TextAreaImageMode::Attachment` | Images appended to separate list; text value unchanged |

Image pasting is **opt-in**: only active when `on_images_change` or `on_image_paste` is set on `TextArea`.

## Read content for a terminal host

With the `terminal` feature, `ctx.clipboard().read_content()` returns
`Result<Option<ClipboardContent>, ClipboardError>`. It prefers native file lists,
then image data, then text. `None` means no supported content. Images require the
`clipboard-images` feature and obey `paste_max_image_bytes`.
`ClipboardProvider::read_clipboard_content` defines the typed read operation. Its
default tries `read_clipboard_files`, `read_clipboard_image`, then
`read_clipboard_text`, even if the provider leaves `read_terminal_paste` at its
text-only default. Optional readers returning `Unsupported` are skipped; genuine
provider failures propagate without text fallback. Providers with native format
availability can override the typed operation and return `None` for an empty
clipboard. The system provider reads image data once, without a classification
probe followed by a second payload read.

Use [`Terminal::on_paste`](widgets/terminal.md#receive-paste-content-before-pty-encoding)
to receive the same content through terminal paste shortcuts. File lists contain
local paths and do not transfer file bytes by themselves.

## Context-menu precedence

An accepting `on_context_menu` callback runs before right-click clipboard
fallback and terminal mouse forwarding. This applies to editor/terminal callbacks,
item-aware widgets, and an enclosing `MouseRegion`. It consumes the full
right-button gesture and preserves selection. With no accepting handler,
`right_click_action` and PTY mouse reporting keep their configured behavior.
