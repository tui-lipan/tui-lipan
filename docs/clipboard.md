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
use tui_lipan::style::Style;

App::new()
    .clipboard_config(ClipboardConfig {
        enable_performable_ctrl_c_copy: true,  // Bind Ctrl+C to copy when selection exists
        enable_primary_selection: true,         // X11 primary selection
        paste_shift_insert_behavior: PasteShiftInsertBehavior::PrimarySelection,
        paste_max_bytes: 1_000_000,            // Clamp large text pastes
        enable_osc52: true,                    // OSC52 for SSH clipboard
        paste_max_image_bytes: 10_000_000,     // Clamp large image pastes (default 10MB)
        copy_feedback_duration_ms: 150,          // Selection flash after copy (0 disables)
        copy_feedback_style: Style::new().lighten_by(0.35),
    })
    .mount(Root)
    .run()
```

| Field | Type | Default | Purpose |
|-------|------|---------|---------|
| `enable_performable_ctrl_c_copy` | `bool` | `true` | Bind `Ctrl+C` to copy when selection exists; otherwise it falls through |
| `enable_primary_selection` | `bool` | platform | Enable X11 primary (middle-click) clipboard |
| `paste_shift_insert_behavior` | `PasteShiftInsertBehavior` | platform | `PrimarySelection` or `Clipboard` |
| `paste_max_bytes` | `usize` | unbounded | Clamp large text pastes to avoid stalls |
| `enable_osc52` | `bool` | `true` | Emit OSC52 escape on copy/cut (useful over SSH) |
| `paste_max_image_bytes` | `usize` | 10MB | Clamp large image pastes |
| `copy_feedback_duration_ms` | `u16` | `150` | Brief paint-only selection flash after successful copy (`0` disables) |
| `copy_feedback_style` | `Style` | lighten | Style merged onto the selection during the flash |

All clipboard shortcuts are performable by default: copy/cut only consume when the action can run on
a selection, and paste only consumes when the focused widget can accept pasted content. Copy
shortcuts such as `Ctrl+C` and `Ctrl+Insert` also copy any active mouse selection from `Input`,
`TextArea`, `DocumentView`, or `Terminal`, even when those widgets are not focusable. Editable
`Input` and `TextArea` selections can also be cut with cut shortcuts such as `Ctrl+X`.
Otherwise the key falls through to app-level handlers.

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
configured selection flash on that exact widget. For the focused widget, obtain `node_id` with
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
