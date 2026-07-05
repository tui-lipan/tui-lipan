<div align="center">

<img src="assets/tui-lipan-logo-animated.gif" alt="tui-lipan" width="720" />

# tui-lipan

**Opinionated, component-based TUI framework for Rust.**

Build terminal apps with React/Elm-like components, declarative UI trees,
scoped messages, and modern interaction primitives - mouse, focus, overlays,
async commands - out of the box.

General-purpose: editors and IDEs, dashboards, AI-agent interfaces, games,
and developer tools - anything that runs in a terminal.

[![Crates.io](https://img.shields.io/crates/v/tui-lipan.svg)](https://crates.io/crates/tui-lipan)
[![docs.rs](https://img.shields.io/docsrs/tui-lipan)](https://docs.rs/tui-lipan)
[![Context7](https://img.shields.io/badge/Context7-indexed-8A2BE2)](https://context7.com/websites/tui-lipan_dev)
[![CI](https://github.com/tui-lipan/tui-lipan/actions/workflows/ci.yml/badge.svg)](https://github.com/tui-lipan/tui-lipan/actions/workflows/ci.yml)
[![License: MPL-2.0](https://img.shields.io/badge/license-MPL--2.0-blue.svg)](#license)
[![MSRV](https://img.shields.io/badge/MSRV-1.85-blue)](#)
[![Sponsor](https://img.shields.io/github/sponsors/Razuer?logo=githubsponsors&label=sponsor)](https://github.com/sponsors/Razuer)

[Website](https://tui-lipan.dev) ·
[Documentation](https://docs.tui-lipan.dev) ·
[Examples](examples/) ·
[Changelog](CHANGELOG.md)

</div>

> **Live demos:** interactive WASM showcases (whack-a-mole, lazygit clone,
> ghost canvas, OpenCode clone) run in your browser at
> **[tui-lipan.dev](https://tui-lipan.dev)**.

---

## Quick Start

```toml
[dependencies]
tui-lipan = "0.1"
# Optional feature-gated widgets:
# tui-lipan = { version = "0.1", features = ["image", "big-text", "terminal"] }
```

```rust
use tui_lipan::prelude::*;

struct Counter;

#[derive(Default)]
struct State { value: i32 }

#[derive(Clone, Copy)]
enum Msg { Inc, Quit }

impl Component for Counter {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &Self::Properties) -> Self::State {
        State::default()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        VStack::new()
            .border(true).padding(1).gap(1)
            .child(Text::new(format!("Value: {}", ctx.state.value)))
            .child(Button::new("Increment").on_click(ctx.link().callback(|_| Msg::Inc)))
            .child(Button::new("Quit").on_click(ctx.link().callback(|_| Msg::Quit)))
            .into()
    }

    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Inc  => { ctx.state.value += 1; Update::full() }
            Msg::Quit => { ctx.quit();            Update::full() }
        }
    }
}

fn main() -> tui_lipan::Result<()> {
    App::new().title("Counter").mount(Counter).run()
}
```

For a guided walkthrough, see the [tutorial](docs/tutorial.md).
For zero-boilerplate layout previews, see [`mockup!`](#zero-boilerplate-layout-preview).

---

## Why this exists

Most Rust TUI codebases eventually hit the same friction:

- UI logic and rendering are tightly coupled
- State updates are scattered across event handlers
- Reusable UI blocks are hard to isolate
- Advanced UX (focus chains, overlays, drag, keyboard routing) becomes ad-hoc

tui-lipan provides a **high-DX application framework** for TUIs, not just
rendering primitives. You write components and messages - the runtime handles
tree expansion, reconciliation, layout, event routing, focus, and rendering.

## What you get

- **Component model** - typed `Message`, `Properties`, and `State`
- **Declarative UI** - `view()` returns an `Element` tree; builder API + `ui!` macro (with full autocomplete)
- **Zero-boilerplate previews** - `mockup!` macro for instant layout prototyping without any component code
- **Nested components** - `child::<C, _>(...)` with scoped state/message routing
- **Async side effects** - `Command` + background tasks + typed message return
- **Layout engine** - flexbox-inspired `Auto`/`Px`/`Flex`, stacks, frames, splitters
- **Interaction model** - mouse hit-testing, focus traversal, hover/focus introspection, key bubbling
- **Overlay system** - modals, popovers, toasts, dismissal policies, focus capture
- **Rich widget set** - forms, tables, terminal viewport, file tree, log view, diff view, sparkline, charts, and more
- **Theming** - preset + custom themes, host-derived `system` theme, contrast policies, live hot reload
- **Animation & effects** - easing transitions, animated geometry, and `EffectScope` cell shaders (scanlines, palette quantize, custom per-cell effects)
- **Agent-visible UI** - headless rendering + `UiSnapshot` markdown/JSON/PNG exports, so AI agents (and CI) can *see* the TUI they build - see [Agents can see the TUI](#agents-can-see-the-tui)
- **Inline mode** - render below the shell prompt, no alternate screen
- **Two backends** - native terminal and browser/WASM (`web` feature)
- **Backend boundary** - no `ratatui` types in public API

---

## `ui!` Macro (recommended)

The `ui!` macro gives you readable builder-chain syntax with `=> { children }`
sugar for nesting. Because the code is standard Rust, you get **full
rust-analyzer autocomplete**. `rustfmt` preserves `ui!` macro bodies while
formatting surrounding Rust, and you can optionally run `ui-fmt` (then
`rsx-fmt`) before `rustfmt` for macro-body formatting.

```rust
ui! {
    Frame::new().title("Counter").border(true).padding(1) => {
        VStack::new().gap(1) => {
            Text::new(format!("Value: {}", ctx.state.value)),
            Button::new("Increment")
                .on_click(ctx.link().callback(|_| Msg::Inc)),
        }
    }
}
```

An alternative `rsx!` macro with struct-literal syntax is also available
(`rsx! { Frame { title: "Counter", ... } }`), but it lacks editor
autocomplete. Both macros produce `Element` and can be mixed freely.

See [`docs/macros.md`](docs/macros.md) for full syntax reference, control
flow, key attributes, and editor integration.

### Zero-boilerplate layout preview

Skip all component code to quickly preview a layout:

```rust
use tui_lipan::prelude::*;

fn main() -> tui_lipan::Result<()> {
    mockup!("Dashboard Preview", {
        HStack::new().gap(1)
            .child(
                Frame::new().title("Sidebar").border(true).width(Length::Px(28))
                    .child(List::new()
                        .items(["Dashboard", "Settings", "Logs"].map(ListItem::new))
                        .selected(0))
            )
            .child(
                Frame::new().title("Content").border(true).padding(1)
                    .child(Text::new("Press Esc or q to quit"))
            )
    })
}
```

Press `Esc` or `q` to quit. Interactive widgets (lists, inputs, tabs) still
respond to focus and mouse - you get a fully interactive preview without
writing any `update()` logic.

---

## Optional Features

| Feature | What it adds |
|---------|-------------|
| `clipboard` | System clipboard via arboard - **enabled by default** |
| `devtools` | In-app DevTools overlay (`F12`) with frame stats and debug log console; configurable via `DevToolsConfig` |
| `ui-snapshot-json` | JSON export for agent UI snapshots; markdown export is always available |
| `ui-snapshot-png` | Font-backed PNG export for agent UI snapshots and captured frames |
| `clipboard-images` | Image clipboard read/write (without the `Image` rendering widget) |
| `big-text` | `BigText` widget: large text via FIGlet and pixel fonts |
| `diff-view` | `DiffView` widget: side-by-side and unified diff viewer |
| `image` | `Image` widget: Kitty/iTerm2/Sixel/halfblock rendering with PNG/JPEG/GIF/WebP codecs, including animated GIF/WebP (includes `clipboard-images`) |
| `image-full-formats` | Restores the broad `image` crate default codec set for image-backed features |
| `markdown` | Markdown formatter for `DocumentView` |
| `profiling-tracing` | `tracing` spans/events for render loop and `DocumentView` hot paths |
| `syntax-syntect` | Syntax highlighting in `TextArea`, `DocumentView`, and `DiffView` |
| `terminal` | `Terminal` and `ManagedTerminal`: embedded PTY terminal viewport |
| `terminal-serde` | Serde derives for terminal snapshot leaf style/mouse types used by external, versioned snapshot transports; includes `terminal` |
| `theme-reload` | Live reload of TOML theme files without restarting the app - see [`docs/styling.md`](docs/styling.md) |
| `web` | Browser/WASM backend - see [`docs/web-backend.md`](docs/web-backend.md) |

To disable the system clipboard (no-system-dep builds):

```toml
tui-lipan = { version = "0.1", default-features = false }
```

For smaller shipping binaries, build with the size-optimized profile:

```bash
cargo build --profile release-size --no-default-features
```

---

## Core Runtime Model

```
event → callback → message queue → update()
      → (dirty, command) → command runtime
      → command sends message → queue → reconcile → render
```

Key behaviors:

- Focused widget handles key events first; unhandled events bubble child → parent → root
- `Tab` / `Shift+Tab` traverse focusable elements in DOM order
- Mouse routes to the deepest hit-tested interactive node
- Background work runs in `Command` closures and returns results via message channels
- `mockup!` / `Mockup` adapter: full interactive preview with zero `Component` boilerplate

---

## Agents Can See the TUI

Terminal UIs have always had one development bottleneck: *you have to run the
app and look at it*. tui-lipan removes it. Any component can be rendered
headlessly - no terminal, no PTY - and exported in formats that an AI coding
agent, a snapshot test, or a CI job can actually read:

```rust
use tui_lipan::prelude::*;
use tui_lipan::TestBackend;

let mut backend = TestBackend::new(MyComponent);
backend.set_viewport(Rect { x: 0, y: 0, w: 80, h: 24 });
backend.render();

let snapshot = backend.capture_ui_snapshot();
std::fs::write("ui.md", snapshot.to_markdown())?;       // always available
std::fs::write("ui.png", snapshot.to_png_default())?;   // feature "ui-snapshot-png"
```

- **Markdown snapshots** (always available) - layout tree, widget geometry, text,
  styles, and focus state in an agent-readable report.
- **PNG snapshots** (`ui-snapshot-png`) - font-backed pixel rendering of the real
  frame, so multimodal agents review the actual visual result.
- **JSON snapshots** (`ui-snapshot-json`) - compact structured export for tooling.
- **Drivable** - `TestBackend` can `dispatch()` messages, move focus, and hover,
  so agents and tests exercise real interaction flows headlessly.
- **DevTools** (`devtools`) - in-app `F12` overlay with frame stats and a debug
  log console for live sessions.

This loop - *write view code → render headlessly → look at the PNG → refine* -
is how the apps below were built. See [`examples/ui_snapshot.rs`](examples/ui_snapshot.rs).

### Agent skills included

The repository ships ready-made skills for coding agents (Claude Code,
opencode, Cursor, and compatible) in [`.agents/skills/`](.agents/skills/):

| Skill | For |
|-------|-----|
| `tui-lipan-app-builder` | Structuring full stateful apps: components, messages, props, focus, async |
| `tui-lipan-ui-sketch` | Design-first screen sketching with `mockup!` + PNG inspection |
| `tui-lipan-visual-design` | Snapshot-driven visual review and polish of existing UIs |
| `tui-lipan-layout-debug` | Diagnosing measurement, rect, and caching issues |
| `tui-lipan-widget` | Framework maintainers: authoring primitive widgets |

The app-facing skills are written to work from *your* application repository -
copy the ones you want into your project's skills directory (e.g.
`.claude/skills/` or `.agents/skills/`) or your user-level skills directory,
and your agent gains framework-specific build, sketch, and review workflows.

The documentation is also indexed on
[Context7](https://context7.com/websites/tui-lipan_dev): agents with the
Context7 MCP server can pull current tui-lipan docs and examples on demand
instead of relying on training data.

---

## Extensibility

tui-lipan ships a curated set of framework-maintained primitive widgets. Apps and
external crates extend the UI through composition: reusable `Component`s, helper
functions, or composite widget structs that return built-in primitives as an
`Element` tree. Keeping primitive widgets closed is why the public API stays free
of `ratatui` types and does not expose primitive render hooks.

---

## Widgets Included

**Layout & chrome**
`VStack`, `HStack`, `ZStack`, `Frame`, `Center`, `Grid`, `ScrollView`, `Splitter`, `Spacer`, `Divider`, `MouseRegion`

**Text & media**
`Text`, `BigText`*, `AsciiCanvas`, `Image`*

**Forms & input**
`Input`, `TextArea`, `Button`, `Checkbox`, `Radio`, `Select`, `Slider`, `DatePicker`

**Data & navigation**
`List`, `Table`, `Tree`, `FileTree`, `Tabs`, `DraggableTabBar`, `Breadcrumb`, `SearchPalette`, `Chart`, `Sparkline`, `Heatmap`

**Diagrams & graphs**
`Graph`, `PanView`, `Flowchart`, `SequenceDiagram`, `ClassDiagram`, `StateDiagram`, `ErDiagram`, `GanttDiagram`

**Feedback & overlays**
`ProgressBar`, `Spinner`, `StatusBar`, `Badge`, `Toast`, `Modal`, `Popover`, `Tooltip`, `Accordion`, `ContextMenu`

**Terminal & developer tooling**
`Terminal`*, `ManagedTerminal`*, `LogView`, `DiffView`*, `StatusBar`

*Requires optional feature - see [Optional Features](#optional-features) above.

---

## Examples

```bash
cargo run --example <name>
# Feature-gated examples:
cargo run --example big_text   --features big-text
cargo run --example image      --features image
cargo run --example diff_hub  --features diff-view
cargo run --example terminal_filetree_devtools --features terminal
cargo run --example devtools --features devtools
```

Browse [`examples/`](examples/) for runnable demos covering widgets, layouts,
inline mode, and more - full catalog in [`docs/examples.md`](docs/examples.md).

---

## Built with tui-lipan

- **[opencode-tui](https://github.com/tui-lipan/opencode-tui)** - a full,
  parity-focused rebuild of the OpenCode AI coding TUI: streaming transcripts,
  markdown + syntax + diffs, rich prompt editing, modal workflows, themes, and
  clipboard images - built end-to-end on tui-lipan. Live preview on
  [tui-lipan.dev](https://tui-lipan.dev).
- **[hyprmux](https://github.com/tui-lipan/hyprmux)** - a Hyprland-style tiling
  terminal multiplexer: dwindle/master layouts, floating and fullscreen panes,
  workspaces, animated geometry, themes with hot reload, and real PTY panes via
  tui-lipan's terminal primitives.
- **[emberdeep](https://github.com/tui-lipan/emberdeep)** - a roguelike where
  torchlight is health, clock, and field of view: truecolor `AsciiCanvas`
  rendering, occluded lighting, deterministic particles, and animated title and
  death screens.

Built something with tui-lipan? Open a PR adding it to this list - or drop a
note in [Discussions](https://github.com/tui-lipan/tui-lipan/discussions).

---

## Design Constraints

- No `ratatui` types in public API
- Single-threaded UI tree runtime
- Background work via `Command` + message channel return
- Keyed identity for stable focus/node reuse across reorders

For architecture details, see [`docs/DESIGN.md`](docs/DESIGN.md).

---

## Documentation

The hosted documentation site lives at **[docs.tui-lipan.dev](https://docs.tui-lipan.dev)**
(API reference also on [docs.rs](https://docs.rs/tui-lipan)). The same content
is shipped in this repository:

| Resource | Contents |
|----------|----------|
| [`docs/tutorial.md`](docs/tutorial.md) | End-to-end tutorial: build a complete app |
| [`docs/quick-start.md`](docs/quick-start.md) | Import map, feature flags, first app |
| [`docs/components.md`](docs/components.md) | Component lifecycle, commands, async |
| [`docs/external-programs.md`](docs/external-programs.md) | `$EDITOR`, `terminal_handoff`, `request_full_repaint` |
| [`docs/text-editing.md`](docs/text-editing.md) | `TextEditor`, `TextInput`, undo/redo, widget integration |
| [`docs/events.md`](docs/events.md) | Event/callback types for all widgets |
| [`docs/enums.md`](docs/enums.md) | Enum & type reference (all variants with defaults) |
| [`docs/styling.md`](docs/styling.md) | Style, Color, themes, contrast |
| [`docs/web-backend.md`](docs/web-backend.md) | Browser/WASM backend |
| [`docs/focus.md`](docs/focus.md) | Focus system, key bubbling |
| [`docs/keybindings.md`](docs/keybindings.md) | Keymap, chords, `tui_lipan::input` |
| [`docs/widgets/`](docs/widgets/) | Per-category widget reference with typed prop tables |
| [`docs/patterns.md`](docs/patterns.md) | Common patterns and anti-patterns |
| [`docs/examples.md`](docs/examples.md) | Complete example catalog |
| [`docs/DESIGN.md`](docs/DESIGN.md) | Architecture and runtime internals |
| `examples/` | Runnable demos |
| [`.agents/skills/`](.agents/skills/) | Ready-made skills for coding agents (app building, UI sketch, visual review) |

---

## Status

tui-lipan is **approaching stability**. The widget set, runtime model, and
public API surface are mature and used to build full applications (an
OpenCode TUI clone, among others).

- **Versioning:** semver under `0.x.y` - minor bumps (`0.1` → `0.2`) may
  contain breaking changes, patch bumps (`0.1.0` → `0.1.1`) will not. This
  will tighten to standard semver on `1.0.0`.
- **API stability:** the public surface (component lifecycle, builder API,
  `ui!` / `rsx!` / `mockup!` macros, widget props) is stable in spirit.
  Targeted breaking changes still happen when a clearly better design
  emerges, but each is called out in `CHANGELOG.md`.
- **Compatibility:** all changes - breaking and non-breaking - are tracked
  in [`CHANGELOG.md`](CHANGELOG.md).
- **Docs and examples:** kept in sync with current architecture; out-of-date
  examples are fixed before release.

---

## Contributing

Contributions are very welcome - bug reports, feature ideas, docs, and PRs.
See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the dev loop, MSRV, and the
CHANGELOG policy.

If tui-lipan is useful to you, consider
[sponsoring its development](https://github.com/sponsors/Razuer) ♥

## Security

To report a security vulnerability, please email
[security@tui-lipan.dev](mailto:security@tui-lipan.dev) - **do not open a
public GitHub issue**. See [`SECURITY.md`](SECURITY.md) for the full policy,
expected response times, and scope.

---

## Author

tui-lipan is designed and built by **Adam Mikołajczyk**
([@Razuer](https://github.com/Razuer)), solo.

(The name? *Tulipan* is Polish for tulip: tui-lipan is that word with a TUI
planted inside. Yes, the logo is a tulip. 🌷)

When Omarchy pulled me over to Linux, I fell for TUIs almost immediately - and
then tried to build one of my own. It was hard: clunky, limited, every bit of
polish fighting back. That left me with a question I couldn't put down: why is
there no Elm or React for the terminal? Something where you drop in widgets and
get advanced, good-looking TUIs out of the box - with style, animation, and
personalization - and can rebuild the apps I admired, like lazygit, Neovim,
OpenCode, and Claude Code, in the simplest way possible, while running faster
and doing more.

tui-lipan is my answer to that question: over 230,000 lines of Rust, written by
one person. The OpenCode clone in [Built with tui-lipan](#built-with-tui-lipan)
is the proof it holds up.

---

## License

tui-lipan is licensed under the **Mozilla Public License 2.0**
([LICENSE](LICENSE) or <https://www.mozilla.org/MPL/2.0/>).

MPL-2.0 is **file-level copyleft**: you can build closed-source applications on
top of tui-lipan with no obligation to open your own code. Modifications to
tui-lipan's **own source files** must stay open under MPL-2.0 - so the framework
itself cannot be forked into a closed, competing version.

### Commercial support & services

Priority support, custom development, sponsored features, and integration help
are available. A non-copyleft commercial license also exists for the rare
organization whose policy forbids any copyleft dependency - though most teams
never need it, since MPL-2.0 already permits closed, proprietary, commercial
apps. See [COMMERCIAL.md](COMMERCIAL.md) or contact <contact@tui-lipan.dev>.

### Contribution

Contributions follow **inbound = outbound**: unless you state otherwise, any
contribution you intentionally submit is licensed under the same MPL-2.0 as the
project, with no additional terms. You keep the copyright in your contributions
- there is no CLA. We use a [Developer Certificate of Origin](https://developercertificate.org/)
sign-off (`git commit -s`) instead. See [CONTRIBUTING.md](CONTRIBUTING.md).

### Third-party licenses

Dependencies are predominantly MIT / Apache-2.0 / MPL-2.0 and remain under their
own terms.
