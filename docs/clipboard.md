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
| `enable_osc52` | `bool` | `false` | Emit OSC52 escape on copy/cut (useful over SSH) |
| `paste_max_image_bytes` | `usize` | 10MB | Clamp large image pastes |
| `copy_feedback_duration_ms` | `u16` | `150` | Brief paint-only selection flash after successful copy (`0` disables) |
| `copy_feedback_style` | `Style` | lighten | Style merged onto the selection during the flash |

All clipboard shortcuts are performable by default: copy/cut only consume when the action can run on
a selection, and paste only consumes when the focused widget can accept pasted content. Copy
shortcuts such as `Ctrl+C` and `Ctrl+Insert` also copy any active mouse selection from `Input`,
`TextArea`, `DocumentView`, or `Terminal`, even when those widgets are not focusable. Editable
`Input` and `TextArea` selections can also be cut with cut shortcuts such as `Ctrl+X`.
Otherwise the key falls through to app-level handlers.

Native terminal bracketed-paste events are also routed through the same focused-widget paste path.
That means dropping files or pasting large/quoted text directly into a terminal running tui-lipan
reaches `Input`, `TextArea`, or `Terminal` widgets as a paste instead of raw keystrokes.

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
