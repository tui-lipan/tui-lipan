# Snapshot / Visual Testing

`TestBackend` supports headless snapshot testing via `capture_frame()`. After a `render()` (or `dispatch()` / `send_key()` which implicitly re-render), call `capture_frame()` to get a `CapturedFrame` containing the full rendered buffer as crate-owned types - no ratatui types leak.

### Plain-text snapshot with `insta`

```rust
use tui_lipan::prelude::*;

struct MyWidget;

impl Component for MyWidget {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _: &()) -> () { () }
    fn update(&mut self, _: (), _: &mut Context<Self>) -> Update { Update::none() }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        Frame::new()
            .header_left("Panel")
            .child(Text::new("hello"))
            .into()
    }
}

#[test]
fn snapshot_my_widget() {
    let mut backend = TestBackend::new(MyWidget);
    backend.set_viewport(Rect { x: 0, y: 0, w: 30, h: 5 });
    backend.render();

    let frame = backend.capture_frame();
    insta::assert_snapshot!(frame.plain_text());
}
```

`plain_text()` returns newline-joined rows with trailing spaces trimmed - the output is stable and deterministic across runs.

### Per-cell style assertions

```rust
let frame = backend.capture_frame();
let cell = frame.cell(0, 0);

assert_eq!(cell.symbol, "A");
assert_eq!(cell.fg, Color::Rgb(12, 34, 56));
assert_eq!(cell.bg, Color::Rgb(90, 80, 70));
assert!(cell.modifiers.bold);
```

### Styled runs

`styled_lines()` groups each row into `Vec<(String, Style)>` runs by identical style, useful for asserting that specific text is rendered with a certain color:

```rust
let runs = &frame.styled_lines()[0];
assert_eq!(runs[0].0, "error:");
assert_eq!(runs[0].1.fg, Some(Color::Red));
```

### Cursor capture

When a focused input widget requests cursor placement, `frame.cursor` is populated:

```rust
backend.focus_next();
backend.render();
let frame = backend.capture_frame();

let cursor = frame.cursor.expect("input should place cursor");
assert!(cursor.visible);
assert_eq!(cursor.y, 0);
```

### Animations

`render()` recomputes the tree but does not advance time, so anything time-based
renders at its starting value. `advance(dt)` ticks every animation and refreshes
layout or the rendered tree as needed, which lets a test assert that an animation
actually moves rather than that it merely exists. It covers `Animated` transitions,
smooth scrolls, and the property transitions behind `Context::transition`.

```rust
backend.render();
let start = width_of(&backend, "panel");

backend.advance(Duration::from_millis(25));
assert!(width_of(&backend, "panel") < start, "the panel should be shrinking");

backend.advance(Duration::from_millis(200));
assert_eq!(width_of(&backend, "panel"), 0);
```

Each call is clamped to one frame's worth of time (50 ms), the same clamp the
runner applies, so a single large `dt` behaves like one long frame instead of
jumping to the end of the animation. Step through with repeated calls when you
need intermediate states.

### Viewport resize

```rust
backend.set_viewport(Rect { x: 0, y: 0, w: 40, h: 10 });
backend.render();
let frame = backend.capture_frame();
assert_eq!(frame.width, 40);
assert_eq!(frame.height, 10);
```

### `CapturedFrame` API summary

| Method | Returns | Description |
|--------|---------|-------------|
| `plain_text()` | `String` | Full frame as trimmed plain text, `\n`-separated |
| `to_lines()` | `Vec<String>` | Same as `plain_text()` but per-row |
| `row(y)` | `&[CapturedCell]` | All cells for row `y` |
| `cell(x, y)` | `&CapturedCell` | Single cell at `(x, y)` |
| `styled_lines()` | `Vec<Vec<(String, Style)>>` | Rows grouped into style runs |
| `to_fixed_grid()` | `String` | Full-width rows without trailing trim (layout-faithful) |
| `to_ansi()` | `String` | ANSI styled frame (full terminal repaint prelude) |
| `to_ansi_diff(prev)` | `String` | Incremental ANSI update from a previous frame |
| `to_png(&PngOptions)` | `Result<Vec<u8>>` | PNG bytes with font-backed or bitmap rendering (`ui-snapshot-png`) |

`CapturedCell` fields: `symbol`, `fg`, `bg`, `underline_color`, `modifiers` (`CellModifiers` with bool fields `bold`, `dim`, `italic`, `underline`, `reverse`, `strikethrough`).

### Agent / design-review snapshots

`TestBackend::capture_ui_snapshot()` returns a `UiSnapshot`: rendered
`CapturedFrame` plus semantic `UiWidgetDesc` entries (widget kind, keys, rects,
focus/hover, selection, values). Use `to_markdown()` for agent-readable reports.
Enable the `ui-snapshot-json` feature for `to_json()` / `to_json_pretty()`.
Enable `ui-snapshot-png` for `to_png()` / `to_png_default()` when layout, color,
focus chrome, and visual hierarchy matter; PNG complements markdown/JSON rather
than replacing them. Both return `Result`, so an encoder failure surfaces at the
call rather than as a zero-byte file.

The PNG renderer uses antialiased real-font text by default when a system font is
available, with font8x8 bitmap rendering as the fallback. `PngOptions` is a
crate-root import (not prelude) and can select `PngTextRenderer::Auto`, `Font`,
or `Bitmap`; `font_family` / `font_path` let captures use system or Nerd Fonts.
Force `Bitmap` for deterministic coarse cell output and fallback-style reviews.

```rust
let mut backend = TestBackend::new(MyApp);
backend.set_viewport(Rect { x: 0, y: 0, w: 80, h: 24 });
backend.render();

let snapshot = backend.capture_ui_snapshot();
println!("{}", snapshot.to_markdown());

#[cfg(feature = "ui-snapshot-png")]
std::fs::write("/tmp/ui-snapshot.png", snapshot.to_png_default()?)?;
```

For design review captures, prefer fit-to-content margin helpers so flex space is visible without hand-tuning a viewport. The recommended default margin is `(20, 8)`:

```rust
let snapshot = backend.capture_ui_snapshot_with_margin(
    20,
    8,
    &UiSnapshotOptions::default(),
);
```

`capture_frame_with_margin(20, 8)` provides the same fit-to-content viewport behavior when you only need the rendered `CapturedFrame`.

**Live apps:** snapshot export is **queued until after the next paint** (not synchronous from `update()`). Each request replaces any earlier pending one. Requests schedule a repaint so idle apps still deliver. File routing follows the path extension: `.md` writes markdown, `.json` writes JSON with `ui-snapshot-json`, and `.png` writes the current viewport as PNG with `ui-snapshot-png`.

```rust
// Store the slot in component state:
struct State {
    slot: UiSnapshotSlot,
}

// In update():
ctx.request_ui_snapshot_to("ui-snapshot.md");
ctx.request_ui_snapshot_to_slot(&ctx.state.slot);

// Later (next tick / handler):
if let Some(snap) = ctx.state.slot.take() {
    // use snap
}
```

See `examples/ui_snapshot.rs`.

### Headless snapshots from the environment

Set `TUI_LIPAN_SNAPSHOT` and `AppRunner::run()` renders one frame off-screen,
writes the artifact, and returns - without entering raw mode or opening a
terminal. This captures an existing app or example without editing its source,
and works where there is no tty (CI runners, agent sessions).

```sh
TUI_LIPAN_SNAPSHOT=/tmp/app.png cargo run --example todo --features ui-snapshot-png
```

| Variable | Default | Effect |
|----------|---------|--------|
| `TUI_LIPAN_SNAPSHOT` | unset | Output path; setting it enables headless mode. Format routed by extension |
| `TUI_LIPAN_SNAPSHOT_VIEWPORT` | `100x30` | Layout viewport, `WIDTHxHEIGHT` |
| `TUI_LIPAN_SNAPSHOT_FRAMES` | `1` | Render/message passes before capture; raise when `init()` starts work |
| `TUI_LIPAN_SNAPSHOT_FOCUS` | `0` | Focus advances before capture, for visible focus chrome |
| `TUI_LIPAN_SNAPSHOT_KEYS` | unset | Comma-separated key script dispatched before capture, e.g. `tab,tab,enter` |
| `TUI_LIPAN_SNAPSHOT_SCRIPT` | unset | Full action script (see below); takes precedence over `_KEYS` |
| `TUI_LIPAN_SNAPSHOT_DIAGNOSTIC` | unset | `1` captures with `UiSnapshotOptions::diagnostic()` |

`TUI_LIPAN_SNAPSHOT_KEYS` uses ordinary keybinding syntax (`ctrl+n`, `esc`,
`f12`), the same spelling as keymaps. Each key is dispatched, its messages
drained, and the tree re-rendered before the next one - the same sequence as the
event loop - so typed text accumulates instead of collapsing to its last
character. This is how states behind a keystroke are captured without a harness:

```sh
TUI_LIPAN_SNAPSHOT=/tmp/modal.png TUI_LIPAN_SNAPSHOT_KEYS="tab,enter" cargo snap myapp
```

An unparseable script fails the run rather than being skipped, because a dropped
keystroke silently captures the wrong state.

Format follows the path extension, matching `request_ui_snapshot_to`: `.json`
with `ui-snapshot-json`, `.png` with `ui-snapshot-png`, markdown otherwise.

### Terminal recordings

A recording is text, not video: an asciinema cast v2 file is a JSON header plus
one `[time, "o", data]` line per output chunk, and `CapturedFrame::to_ansi_diff`
already produces that `data`. A few seconds of a real app is typically smaller
than a single PNG frame of it, and the result scrubs, selects as text, and plays
in a browser.

Recording needs **no feature flag and no extra dependency** - the cast JSON is
written directly, so it works in any build.

```sh
TUI_LIPAN_RECORD=/tmp/demo.cast TUI_LIPAN_RECORD_KEYS="tab,enter" cargo run --example todo
```

| Variable | Default | Effect |
|----------|---------|--------|
| `TUI_LIPAN_RECORD` | unset | Output path; setting it enables headless recording |
| `TUI_LIPAN_RECORD_VIEWPORT` | `100x30` | Recorded terminal size, `WIDTHxHEIGHT` |
| `TUI_LIPAN_RECORD_FPS` | `30` | Capture rate |
| `TUI_LIPAN_RECORD_KEYS` | unset | Key script to play, e.g. `tab,tab,enter` |
| `TUI_LIPAN_RECORD_KEY_DELAY_MS` | `400` | Pause after each key, so a viewer can follow |
| `TUI_LIPAN_RECORD_SETTLE_MS` | `1200` | Hold on the final frame |
| `TUI_LIPAN_RECORD_FRAMES` | unset | Directory for truecolor PNG frames (needs `ui-snapshot-png`) |
| `TUI_LIPAN_RECORD_SCRIPT` | unset | Full action script (see below); takes precedence over `_KEYS` |

In code, `Recording` mirrors `Sketch`:

```rust
use tui_lipan::Recording;

Recording::view("demo", login_screen)   // or Recording::component(...)
    .viewport(100, 30)
    .keys("tab,enter")
    .fps(30)
    .write("docs/demo.cast")?;
```

| Method | Effect |
|--------|--------|
| `Recording::view(title, fn)` | Record a plain `Fn() -> Element` |
| `Recording::component(title, c)` | Record a `Component` with default properties |
| `viewport(w, h)` | Recorded terminal size |
| `fps(n)` | Capture rate |
| `keys(script)` | Key script to play |
| `key_delay(duration)` | Pause after each key |
| `settle(duration)` | Hold on the final frame |
| `png_options(opts)` | Rendering options for frame export |
| `quiet(b)` | Suppress the written-path line |
| `record()` | Return a `CastRecording` without writing |
| `write(path)` | Write the cast; returns the path |
| `write_frames(dir)` | Write one truecolor PNG per frame; returns the paths |

**Timing is a synthetic fixed step.** The recorder advances a clock in `1/fps`
increments and ticks animations by the same amount, so the same view and script
always produce identical bytes - a committed recording stays diffable, and
animations are captured at full rate. The trade is that work depending on real
elapsed time (a PTY child's output, a network response) does not arrive on a
synthetic clock: recordings capture an app's own rendering, not a live session.

Identical frames are dropped, so a still stretch costs nothing. Because that
would otherwise end the file at the last visible change, the recorder writes a
final zero-length event to hold the closing frame for the intended duration
(`CastRecording::mark_time`).

### Choosing an output format

| Format | Size (7s demo) | Best for | Cost |
|--------|----------------|----------|------|
| `.cast` | **9 KB** | Docs sites, PR links, committing to the repo | Needs a player |
| `.mp4` via GIF | 26 KB | Slack, quick shares | 256-colour quantisation |
| `.gif` | 44 KB | READMEs - auto-plays inline with no player | 256 colours, largest |
| `.mp4` truecolor | 55 KB | Marketing, high-DPI, real colour fidelity | Needs frame export + ffmpeg |

Sizes are from the same recording of `examples/todo`; a busier UI widens the gap
in the cast's favour, since it transmits only changed cells while video re-encodes
whole frames.

Start with `.cast`. Reach for video only when the destination cannot play one.

#### GIF

[`agg`](https://github.com/asciinema/agg) converts a cast:

```sh
agg --theme dracula --idle-time-limit 1 --speed 1.5 demo.cast demo.gif
```

`--idle-time-limit` is the biggest size win - it caps dead air between keystrokes.
Themes: `asciinema`, `dracula`, `github-dark`, `github-light`, `kanagawa`,
`monokai`, `nord`, `solarized-dark`, `solarized-light`, `gruvbox-dark`.

#### MP4, the quick way

```sh
ffmpeg -i demo.gif -pix_fmt yuv420p -movflags +faststart \
       -vf "scale=trunc(iw/2)*2:trunc(ih/2)*2" demo.mp4
```

Both filters earn their place: `yuv420p` is what makes the file play in browsers
and chat clients, and the `scale` filter forces even dimensions, which H.264
requires - without it an odd-sized terminal fails to encode.

This route inherits GIF's 256-colour palette. For a flat theme that is invisible;
for gradients it bands.

#### MP4, truecolor

Export PNG frames instead, skipping GIF entirely:

```sh
TUI_LIPAN_RECORD=demo.cast TUI_LIPAN_RECORD_FRAMES=frames \
  cargo run --features ui-snapshot-png --example todo

ffmpeg -framerate 30 -i frames/frame_%05d.png \
       -pix_fmt yuv420p -movflags +faststart demo.mp4
```

`write_frames` and `TUI_LIPAN_RECORD_FRAMES` print that exact `ffmpeg` line with
the right paths and frame rate filled in, so it can be pasted straight back.

Frames are written at a **constant rate**, one per `1/fps` tick including
unchanged ones, because an encoder reconstructs timing from a numbered sequence.
That costs disk: a 7-second capture at 30fps is ~200 files and ~19 MB. They are
an intermediate - delete them after encoding. Unchanged frames reuse the previous
encode rather than paying for it twice.

Raise `PngOptions::scale` (via `Recording::png_options`) for a higher-resolution
video; the default 2x gives 16x32 pixels per cell.

```rust
Recording::view("demo", view)
    .viewport(90, 26)
    .keys("tab,enter")
    .write_frames("target/recordings/frames")?;
```

### Action scripts

A key script can only type. An action script can also click, hover, focus,
scroll, drag, and wait - enough to reach a modal behind a button or a row behind
a scroll.

```sh
TUI_LIPAN_SNAPSHOT=/tmp/after.png \
TUI_LIPAN_SNAPSHOT_SCRIPT="focus:#draft; type:buy milk; click:#add; wait:200" \
  cargo run --example todo --features ui-snapshot-png
```

Steps are separated by `;` or newlines.

| Step | Effect |
|------|--------|
| `key:ctrl+n` | One key event, in keybinding syntax |
| `type:hello world` | Literal text, one key event per character |
| `click:#submit` | Left click the centre of the widget keyed `submit` |
| `click:12,7` | Left click a cell |
| `rclick:` / `mclick:` | Right / middle click |
| `hover:#sidebar` | Move the pointer over a widget |
| `focus:#email` | Focus a widget directly |
| `focus:next` / `focus:prev` | Move focus one step |
| `scroll:#list,down` | Scroll over a widget (`up` / `down`) |
| `scroll:down` | Scroll at the current pointer position |
| `drag:#card>#column` | Press, move, release |
| `wait:500` | Advance the clock 500ms, ticking animations |

**Target widgets by key, not by coordinate.** `#submit` resolves through the
current tree to that widget's rect and clicks its centre, so it survives the
widget moving and **fails loudly** when the key is absent:

```
Error: no widget with key `does-not-exist` is currently rendered
```

A coordinate cannot do that - a layout change silently turns `click:42,7` into a
click on empty space while the script still reports success. Coordinates remain
available for what keys cannot express.

Give widgets stable keys (`.key("add")`) to make them scriptable; the keys a
running app exposes are listed in any markdown snapshot.

In code, `Recording::script(...)` takes the same syntax, and `Recording::keys(...)`
remains the shorthand for the typing-only case.

### Live control channel

`TUI_LIPAN_CONTROL=<path>` makes a running app listen on a Unix socket, so an
agent can inspect and drive a live TUI the way a browser tool drives a page:
snapshot, pick a widget by key, act, look again.

```sh
TUI_LIPAN_CONTROL=/tmp/app.sock cargo run --example todo
```

Requests are single `\n`-terminated lines:

| Command | Reply |
|---------|-------|
| `ping` | `pong` |
| `keys` | Newline-separated reconciliation keys currently rendered |
| `snapshot` | Markdown snapshot |
| `snapshot json` | JSON snapshot (needs `ui-snapshot-json`) |
| `snapshot png <path>` | Writes a PNG, replies with the path (needs `ui-snapshot-png`) |
| `act <script>` | Runs an action script; empty payload on success |
| `highlight <key>` | Outlines a widget; replies with the resolved rect |
| `highlight <col>,<row>` | Outlines the smallest widget covering a cell |
| `highlight clear` | Removes the outline |
| `quit` | Asks the app to exit |

Replies are a status line plus exactly that many bytes:

```text
ok <byte-length>\n<payload>
err <byte-length>\n<message>
```

Length prefixing keeps payloads newline- and binary-safe without escaping, so a
client is a few lines in any language. `keys` is the index of what `act` can
target - the equivalent of a browser tool's element refs.

`highlight` is an inspector marker, drawn over the finished frame in magenta. It
does not depend on the widget styling itself for hover or focus, so it marks
anything - including widgets with no interactive styling at all. Large rects are
outlined so the content underneath stays readable; rects one or two cells thick
are filled, having no interior to preserve. The outline reaches captures as well
as the live paint, so `snapshot png` shows what the operator sees.

Cell targeting resolves to the **smallest** widget covering that cell rather than
the topmost, which lands on the leaf a user would say they are pointing at
instead of the panel containing it. That is how unkeyed widgets stay
inspectable.

**Notes:**

- Unix only. The socket is created `0600`, because anything that can reach it can
  type into your application. Do not place it on a shared filesystem.
- `AF_UNIX` paths are limited to about 100 bytes; a long path fails to bind.
- Connections are served one at a time - the UI is a single shared surface.
- Runtime state stays single-threaded: the listener thread queues requests and
  the event loop answers them, the same pattern the terminal reader uses.

### Design sketches

`Sketch` renders a view at one or more viewports and writes every artifact in a
single call, so a design capture is small enough to keep in the repository
instead of being written and deleted:

```rust
use tui_lipan::{Result, Sketch};

Sketch::view("login", login_screen)   // any Fn() -> Element
    .viewport(80, 24)
    .fit(20, 8)                       // content minimum + margin
    .focus_next(1)                    // visible focus chrome
    .write()?;
```

| Method | Effect |
|--------|--------|
| `Sketch::view(name, fn)` | Sketch a plain `Fn() -> Element` (mounted through `Mockup`) |
| `Sketch::component(name, c)` | Sketch a `Component` with default properties |
| `viewport(w, h)` | Capture at an exact size; repeat for breakpoints |
| `fit(margin_w, margin_h)` | Capture at content minimum size plus margin |
| `focus_next(n)` | Advance focus `n` times before capturing |
| `options(opts)` | Describe options, e.g. `UiSnapshotOptions::diagnostic()` |
| `keys(script)` | Dispatch a key script before capturing, e.g. `"tab,enter"` |
| `markdown(b)` / `png(b)` / `json(b)` | Toggle formats; markdown and PNG default on |
| `dir(path)` | Output directory override |
| `baseline(dir)` | Compare each capture against a stored baseline image |
| `tolerance(ratio)` | Max fraction of differing pixels still counted as a match (default `0.0`) |
| `quiet(b)` | Suppress printing written paths |
| `write()` | Run every pass; returns `Result<Vec<PathBuf>>` |
| `check()` | Run and return `Vec<BaselineComparison>` |
| `assert_baseline()` | Run and fail if any capture regressed |

With no explicit viewport, `Sketch` captures `80x24` plus a fit-to-content pass -
the pairing that exposes flex-distribution bugs a single viewport hides. Output
defaults to `target/ui-sketches/` (override with `TUI_LIPAN_SKETCH_DIR`), so
sketches need no `.gitignore` entry.

Keep sketches in `examples/sketches/`; see that directory's `main.rs` for how new
ones register without a `Cargo.toml` change.

### Visual regression baselines

A kept sketch only protects against regressions if something notices when the
picture changes. `baseline(dir)` stores one PNG per capture, compares the next
render against it pixel by pixel, and writes a highlighted `*.diff.png` beside
any baseline that changed - unchanged pixels dimmed for context, changed pixels
in magenta.

```rust
#[test]
fn login_screen_has_not_drifted() -> Result<()> {
    Sketch::view("login", login_screen)
        .viewport(80, 24)
        .baseline("tests/ui-baselines")
        .assert_baseline()
}
```

The first run records baselines and passes. Later runs fail with every changed
capture listed at once, each naming its diff image. Accept new output with:

```sh
TUI_LIPAN_UPDATE_BASELINES=1 cargo test
```

**Baseline captures force `PngTextRenderer::Bitmap`.** The default `Auto`
renderer picks whichever system font it discovers, so the same UI produces
different pixels on CI than on a laptop and comparison becomes meaningless. The
built-in bitmap font ships with the crate, so it renders identically everywhere.
Font-rendered artifacts are still written for human review - they are simply not
what gets compared.

`BaselineOutcome` distinguishes `Created` (first run, not a failure), `Match`,
`Updated`, `Changed` (with pixel counts, ratio, and diff path), and `SizeChanged`
(dimensions differ, so pixels cannot be compared). `is_regression()` is the
single check for whether an outcome should fail a build.

Prefer removing nondeterminism over raising `tolerance`; a tolerance that hides
a real change is worse than no baseline.

---
