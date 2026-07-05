# Web / WASM Backend

tui-lipan can compile to **wasm32** and run in a browser, rendering through
[xterm.js](https://xtermjs.org/). The same component, widget, and layout code
works unchanged - only the entry point and build toolchain differ.

## How it works

The web backend reuses `TestBackend` (the headless render engine used for
snapshot tests) as its runtime. After each key press or viewport change the
framework renders a `CapturedFrame` (a grid of styled cells), converts it to
ANSI escape sequences, and writes them to the xterm.js `Terminal` object via
`wasm-bindgen`. From xterm's perspective it's receiving normal VT100/ANSI
input from a program.

```
Component state change
      │
      ▼
TestBackend::send_key / set_viewport
      │
      ▼
TestBackend::render  →  CapturedFrame (cell grid)
      │
      ▼
captured_frame_to_ansi  →  ANSI string
      │
      ▼
xterm.js Terminal.write(ansi)
```

## Feature gating

The web backend is behind the `web` Cargo feature and only compiles for the
`wasm32-unknown-unknown` target. A `compile_error!` fires if `--features web`
is passed on a native host so the wrong combination is caught at compile time.

```toml
# wasm32 build
tui-lipan = { path = "…", default-features = false, features = ["web"] }
```

The following features are **incompatible** with `web` and trigger a compile
error on wasm32: `image`, `terminal`.

Native process streaming is not part of the wasm API. The `tui_lipan::process`
module and prelude exports such as `ProcessSpec` and `ProcessEvent` are gated
with `#[cfg(not(target_arch = "wasm32"))]`; browser builds should use web APIs
or a server bridge for subprocess-like work.

## Public API

```rust
// src/app/web_runner.rs  (wasm32 + `web` feature only)

pub fn mount_web<C: Component>(
    component: C,
    props: C::Properties,
    term: JsValue,    // xterm.js Terminal object
    cols: u16,
    rows: u16,
) -> Result<WebTerminal<C>>

impl<C: Component> WebTerminal<C> {
    pub fn dispatch_key_event(&mut self, ev: &web_sys::KeyboardEvent) -> Result<()>;
    pub fn dispatch_mouse_event(&mut self, event: MouseEvent) -> Result<()>;
    pub fn set_viewport(&mut self, cols: u16, rows: u16) -> Result<()>;
}
```

`WebTerminal` is not `#[wasm_bindgen]` itself - wrap it in your own
`#[wasm_bindgen]` struct (see the web examples) so you control the JS API
surface.

## Writing a web app

### 1. Create a sub-crate

Web examples must be separate workspace members with `crate-type = ["cdylib"]`.
They cannot be inlined as `[[example]]` entries in the root workspace.

```
my-app/
├── Cargo.toml        [workspace] entry, cdylib crate-type
├── src/lib.rs        component + #[wasm_bindgen] handle
├── index.html
├── serve.py          MIME-aware dev server
└── package.json      @xterm/xterm + @xterm/addon-fit
```

`Cargo.toml`:
```toml
[package]
name    = "my-tui-web-app"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
tui-lipan   = { path = "…", default-features = false, features = ["web"] }
wasm-bindgen = "0.2"
web-sys = { version = "0.3", features = ["KeyboardEvent"] }

[workspace]   # isolates this crate from the root workspace
```

### 2. Implement a component

Components are identical to native ones - no web-specific code inside the
component itself.

```rust
use tui_lipan::prelude::*;

struct Counter;

impl Component for Counter {
    type Message = ();
    type Properties = ();
    type State = i32;

    fn create_state(&self, _: &()) -> i32 { 0 }

    fn view(&self, ctx: &Context<Self>) -> Element {
        Frame::new()
            .border(true)
            .child(Text::new(format!("count: {}", ctx.state)))
            .into()
    }

    fn update(&mut self, (): (), _: &mut Context<Self>) -> Update { Update::none() }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('+') => { ctx.state += 1; KeyUpdate::handled(Update::full()) }
            KeyCode::Char('-') => { ctx.state -= 1; KeyUpdate::handled(Update::full()) }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }
}
```

### 3. Expose to JavaScript

```rust
use wasm_bindgen::prelude::*;
use tui_lipan::{WebTerminal, mount_web};

#[wasm_bindgen]
pub struct AppHandle {
    app: std::cell::RefCell<WebTerminal<Counter>>,
}

#[wasm_bindgen]
impl AppHandle {
    #[wasm_bindgen(constructor)]
    pub fn new(term: JsValue, cols: u16, rows: u16) -> Result<AppHandle, JsValue> {
        let app = mount_web(Counter, (), term, cols, rows)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;
        Ok(Self { app: std::cell::RefCell::new(app) })
    }

    pub fn on_key_down(&self, ev: web_sys::KeyboardEvent) -> Result<(), JsValue> {
        ev.prevent_default();
        self.app.borrow_mut().dispatch_key_event(&ev)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    pub fn resize(&self, cols: u16, rows: u16) -> Result<(), JsValue> {
        self.app.borrow_mut().set_viewport(cols, rows)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    // Canonical browser mouse bridge used by web examples.
    pub fn on_mouse(
        &self,
        x: i32,
        y: i32,
        button: u8,
        phase: u8,
        is_wheel: bool,
        shift: bool,
        alt: bool,
        ctrl: bool,
    ) -> Result<(), JsValue> {
        let x = x.clamp(0, i32::from(u16::MAX)) as u16;
        let y = y.clamp(0, i32::from(u16::MAX)) as u16;
        let mods = KeyMods { ctrl, alt, shift, super_key: false };

        // map (button, phase, is_wheel) -> MouseKind.
        // unsupported wheel/button codes should be ignored.
        let event = MouseEvent { x, y, kind: /* ... */, mods };

        self.app.borrow_mut().dispatch_mouse_event(event)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }
}
```

`WebTerminal::dispatch_mouse_event(MouseEvent)` stays the runtime API. The wasm
handle method above is an example-facing adapter layer.

### 4. HTML + JS glue

```html
<link rel="stylesheet" href="./node_modules/@xterm/xterm/css/xterm.css" />
<div id="term" style="height:100%;width:100%"></div>
<script type="module">
  import { Terminal }  from "./node_modules/@xterm/xterm/lib/xterm.mjs";
  import { FitAddon }  from "./node_modules/@xterm/addon-fit/lib/addon-fit.mjs";
  import { parseSgrMouse } from "../shared/sgr_mouse.js";
  import init, { AppHandle } from "./pkg/my_tui_web_app.js";

  await init();
  const term = new Terminal({ fontFamily: "monospace" });
  const fit  = new FitAddon();
  term.loadAddon(fit);
  term.open(document.getElementById("term"));
  fit.fit();

  const app = new AppHandle(term, term.cols, term.rows);

  term.onKey(({ domEvent }) => app.on_key_down(domEvent));

  term.onData((data) => {
    const mouse = parseSgrMouse(data);
    if (!mouse) return;
    app.on_mouse(
      mouse.x,
      mouse.y,
      mouse.button,
      mouse.phase,
      mouse.isWheel,
      mouse.shift,
      mouse.alt,
      mouse.ctrl,
    );
  });

  window.addEventListener("resize", () => {
    fit.fit();
    app.resize(term.cols, term.rows);
  });
</script>
```

The shared `parseSgrMouse` helper preserves xterm SGR behavior (`m` release,
wheel bit, drag/down/up phase, modifier bits). Keep both examples on the same
parser + `on_mouse(x, y, button, phase, is_wheel, shift, alt, ctrl)` bridge so
mouse semantics stay consistent.

Load xterm from `node_modules/` (or pinned CDN in demos) and use a MIME-aware
dev server. In this repo, example servers run from `examples/web/` so shared
assets under `examples/web/shared/` resolve from both example pages.

### 5. Build and run

```bash
npm install           # installs @xterm/xterm and @xterm/addon-fit
wasm-pack build --target web
python3 serve.py      # serves with correct .mjs / .wasm MIME types
# open http://localhost:8080
```

`cargo check --target wasm32-unknown-unknown` is the fast iteration loop
before a full `wasm-pack build`.

## Web examples in this repo

- `examples/web/hello` - minimal counter + keyboard input
- `examples/web/search_palette` - richer overlay/fuzzy-search demo with mouse input

Both crates are standalone wasm `cdylib`s with their own `[workspace]` table.

From repo root you can use:

```bash
make -C examples/web hello
make -C examples/web search-palette
```

## Known limitations

| Limitation | Notes |
|---|---|
| Mouse capture model differs from DOM | xterm DECSET mouse reporting (`1000/1002/1006`) owns drag/select input |
| Async task timing differs from native | wasm `web` builds use `spawn_local` on the browser event loop (single-threaded); minimal no-`web` wasm builds run command closures synchronously |
| Command cancellation is cooperative | `LatestOnly` marks tokens cancelled, but synchronous CPU-bound wasm closures cannot observe cancellation until they yield or return |
| Clipboard read is best-effort | `navigator.clipboard.readText()` is async/user-gesture gated; sync bridge returns cached value when available |
| No image protocols | kitty/sixel/iterm not supported by xterm.js by default |
| No terminal embedding | `portable-pty` is native-only |
| Full repaint on first frame / resize | stable viewport paints use incremental frame diffs |

## Architecture notes

- **`Instant`** - all `std::time::Instant` uses are replaced with
  `web_time::Instant` (backed by `performance.now()` on wasm). Native builds
  are unaffected.
- **`libc` / `crossterm` / `open` / `ignore`** - gated behind
  `[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`; not linked into
  wasm builds.
- **Scrollback** - the paint prefix emits `\x1b[3J` (erase saved lines) before
  each frame so old content does not accumulate in the xterm.js scrollback
  buffer when the viewport is resized.
- **Symbol encoding** - `CapturedCell.symbol` is already a valid Unicode
  grapheme cluster; no additional encoding step is needed.
