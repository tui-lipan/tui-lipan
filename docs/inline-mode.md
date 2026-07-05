# Inline Viewport Mode

tui-lipan supports rendering in a fixed-height inline viewport instead of taking over the terminal with an alternate screen. The terminal history remains intact because inline mode does not enter the alternate screen.

## Surface Modes

The app surface mode determines how the viewport occupies terminal space.

| Mode | Builder | Behavior |
|------|---------|----------|
| Fullscreen | `.fullscreen()` (default) | Takes over the full terminal using the alternate screen. |
| Ephemeral | `.inline_ephemeral(height)` | Inline viewport intended for short-lived sessions (e.g., a list picker). |
| Transcript | `.inline_transcript(height)` | Inline viewport intended for transcript-friendly sessions (e.g., a chat CLI). |

## Basic Setup

### Ephemeral Mode

Best for tools that provide a temporary UI and then exit, leaving only the `exit_view` in scrollback.

```rust
App::new()
    .inline_ephemeral(8)
    .mount(MyPicker)
    .exit_view(|_component, ctx| {
        Text::new(format!("Selected: {}", ctx.state.selection)).into()
    })
    .run()
```

### Transcript Mode

Best for chat-style apps that append messages to terminal history while running.

```rust
App::new()
    .inline_transcript(12)
    // .inline_transcript_with_startup(12, InlineStartupPolicy::ClearHost)
    .mount(ChatApp)
    .run()
```

## Mouse Behavior

| Mode | Mouse default | Behavior |
|------|--------------|----------|
| Fullscreen | Enabled | Wheel events delivered to app; terminal scrollback inactive |
| Inline | **Disabled** | Native terminal scrolling preserved by default |

Enable mouse in inline mode when your app needs wheel events:

```rust
App::new().inline_ephemeral(8).mouse(true).mount(Root).run()
```

Runtime mouse control:

```rust
ctx.mouse_capture_enabled()      // Current state
ctx.set_mouse_capture(true)      // Enable at runtime
ctx.set_mouse_capture(false)     // Disable at runtime
ctx.toggle_mouse_capture()       // Toggle, returns new state
```

## Context Methods for Inline Mode

```rust
ctx.is_inline()                    // true if running in inline mode
ctx.append_transcript_lines(lines) // Append styled lines above the viewport
ctx.append_transcript_element(el)  // Append a rendered Element to history
```

## Transcript / Native Scroll Pattern

Inline transcript mode allows modeling a Claude Code or Gemini CLI style interaction:

- Keep a small live viewport for the composer and any in-progress response.
- Call `ctx.append_transcript_element(element)` when a user/assistant message is complete.
- The appended element is rendered once and inserted into terminal history above the viewport.
- `append_transcript_element` is for already-expanded widget trees. Do not pass `Component` elements or subtrees that still contain them.
- Use `Text::overflow(Overflow::Wrap)` (or `Overflow::Auto` with a width constraint) inside appended subtrees so lines wrap to the current terminal width instead of clipping.

```rust
fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::UserSubmitted(prompt) => {
            ctx.append_transcript_element(message_card("You", &prompt));
            Update::full()
        }
        Msg::AssistantFinished(text) => {
            ctx.append_transcript_element(message_card("Assistant", &text));
            Update::full()
        }
    }
}
```

## Resize Behavior

Inline resize behavior is defined by **surface mode semantics**, not by user-facing terminal wrap controls.

- **Ephemeral mode** keeps a fixed inline viewport, disables terminal autowrap, and preserves the current live session during resize.
- **Transcript mode** resets to a full-height inline surface on resize, clears the visible terminal, and redraws the app from scratch while leaving prior output in native scrollback.

The last terminal column is reserved only for ephemeral inline mode, where autowrap stays disabled during resize.

## Limits in Inline Mode

- **Root-level overlays suppressed**: Modal, Popover, and Toast portals are disabled to avoid terminal history corruption. Use local overlays or inline widgets instead.
- **Image rendering disabled**: Falls back to text placeholders.
- **Last column reserved**: Avoids width-change soft-wrap artifacts.
- **Mouse coordinates**: Translated from terminal-space to viewport-space before hit testing.

## Examples

- `examples/inline.rs` - Basic inline ephemeral session
- `examples/native_scroll_chat.rs` - Transcript-style native scrollback chat
- `examples/inline_list_picker.rs` - Inline lists, filtering, and activation
- `examples/inline_choices.rs` - Choice selection in inline mode
