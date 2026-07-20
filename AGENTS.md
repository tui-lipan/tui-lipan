# AGENTS.md (tui-lipan)

**tui-lipan**: Opinionated, modern, high-DX TUI framework in Rust with React/Elm-like architecture.

## Mission

- Build a **component-based** TUI framework with declarative UI (`Element` tree)
- Provide **builder API first**, with `rsx!` supported as syntax sugar
- Offer **lazygit/lazydocker-style chrome** (frames/panels with titles) out of the box

## Breaking Changes Policy

The crate is approaching stability and is published under semver `0.x.y`
(minor bump = breaking allowed, patch bump = backward-compatible only).

- Breaking changes are still acceptable when a clearly better design emerges,
  but each one **must** be recorded in `CHANGELOG.md` under `[Unreleased]`
  with the suffix "(breaking)"
- Delete deprecated code rather than keeping shims - but log the removal in
  the changelog
- Rename freely if better names exist; record the rename in the changelog
- Update examples/tests/docs to match new APIs in the same PR

## CHANGELOG Policy

Every user-visible change requires a `CHANGELOG.md` entry under `[Unreleased]`
(public API, widget behavior, feature flags, or user-facing docs). See
[`CONTRIBUTING.md`](CONTRIBUTING.md) for the format, sections, and what to skip.

## Hard Constraints

1. **No `ratatui` types in public API** - internal renderer only
2. **Single-threaded UI/runtime state (`Rc`/`RefCell`)** - use `Command`/`CommandLink` message passing for background work
3. **Component DI supported** - `App::mount(component)` accepts instances, not just types
4. **Mouse + focus from day 1** - preserve precise hit-testing, hover-testing, and scrollbar-zone routing
5. **Flexbox-inspired layout** - leaf nodes default `Auto`, containers default `Flex(1)`

## Commands

### Build & Run
- `cargo build` - Compile project
- `cargo run --example <name>` - Run example (e.g., `todo`, `lazygit`, `showcase`)
- `cargo check` - Fast compile check
- `cargo fetch` - Download dependencies

When you edit Rust files that contain `ui!` or `rsx!` - especially examples in `examples/` - run the repo helper first so macro blocks are formatted too:

```bash
./scripts/format-rust-with-macros examples/<name>.rs
```

### Testing
- `cargo test` - Run all tests
- `cargo test --workspace` - Run workspace tests
- `cargo test --examples` - Compile and test examples
- `cargo test --no-run` - Compile tests without running
- `cargo test <test_name>` - Run specific test by name
- `cargo test --package tui-lipan --lib <module>::tests::<test_name> --exact` - Run single test

### Linting & Formatting
- `cargo fmt` - Format code
- `cargo fmt --all -- --check` - Check formatting
- `cargo ui-fmt <file.rs>` - Format `ui!` blocks in Rust files
- `cargo rsx-fmt <file.rs>` - Format `rsx!` blocks in Rust files
- `./scripts/format-rust-with-macros <file.rs>` - Format `ui!` + `rsx!` blocks, then run `rustfmt`
- `cargo clippy` - Clippy (default features only; same as any bare subcommand)
- `cargo lint` or `cargo dev-clippy` - Full strict lint, same as CI (`--all-targets --all-features`, `-D warnings`)
- `cargo doc --open` - Generate docs

### CI Pipeline
```bash
cargo fetch
python3 scripts/check-widget-variant-parity.py
python3 scripts/generate-node-kind-delegate-arms.py
python3 scripts/check-widget-style-slots.py
python3 scripts/check-children-replace.py
find src tests benches examples tui-lipan-macro -name '*.rs' -print0 \
  | xargs -0 -r ./scripts/format-rust-with-macros --check
python3 scripts/check-feature-tables.py
cargo fmt --all -- --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps
cargo test --workspace --all-features
cargo deny --all-features check licenses sources advisories bans
```

If `scripts/generate-node-kind-delegate-arms.py` fails, regenerate the checked-in
delegate block with `python3 scripts/generate-node-kind-delegate-arms.py --write`
and rerun the check.

### CI environment notes

- CI runners have **no tty**. Library and test code paths must never query the
  host terminal. In particular, ratatui's `Terminal::resize` on a
  `Viewport::Fixed` terminal calls `backend.size()`, which asks the real tty
  even when the backend writes to an in-memory buffer - it fails with EAGAIN
  on CI. For in-memory scratch terminals, recreate the terminal with a new
  `Viewport::Fixed` instead of resizing (`Terminal::with_options` never
  touches the backend for fixed viewports). See `new_inline_commit_scratch`
  in `src/app/runner/render_service/inline.rs` and the guardrail test in
  `src/runtime.rs`.
- A test passing locally but failing only in CI usually means it depends on a
  real terminal, the filesystem layout, or wall-clock timing. Fix the
  dependency, don't loosen the assertion.

### Dependencies & advisories

- New dependencies must carry licenses on the `deny.toml` allowlist and pass
  `cargo deny --all-features check licenses sources advisories bans` (the
  exact CI invocation; `cargo install cargo-deny --locked` to run locally).
- The advisories check can start failing **without any code change** when a
  new RUSTSEC advisory is published. Resolution order: upgrade if a patched
  version exists; otherwise add an ignore entry to `deny.toml` **with a
  written reason** explaining why the vulnerable path is not reachable with
  untrusted input through tui-lipan's API (see the existing entries for the
  expected format). Remove ignores that no longer match any crate.

## Development Workflow

## Local tui-lipan RAG MCP

When working on tui-lipan code, examples, docs, widgets, feature flags, or
public APIs, use the `tui-lipan-rag` MCP server before relying on memory or
generic search:

- Use `tui_lipan_lookup_widget` and `tui_lipan_lookup_widget_defaults` before
  writing or changing widget builder code.
- Use `tui_lipan_lookup_example` for runnable example patterns and required
  feature flags.
- Use `tui_lipan_search` for broader framework questions across docs,
  examples, root guides, Cargo metadata, and generated references.
- Use `tui_lipan_read` after search/lookup when you need the full surrounding
  source section.

**After implementing features or making changes, ALWAYS run:**
```bash
./scripts/format-rust-with-macros <changed-rust-files>
cargo build          # Ensure everything compiles
cargo lint           # Full clippy (same as CI)
cargo fmt            # Format code
cargo test           # Run tests to verify nothing broke
```

Use the helper script for changed Rust files before building or running examples so `ui!` and `rsx!` bodies are formatted in addition to normal Rust code.

This catches compilation errors, lint issues, and formatting problems before committing.

## Code Style

### Naming
- **Types/Traits**: `PascalCase` (e.g., `Component`, `ButtonVariant`)
- **Functions/Variables**: `snake_case` (e.g., `create_state`, `on_click`)
- **Enums**: `PascalCase` variants (e.g., `KeyCode::Char('c')`)
- **Properties**: Use `type Properties = ()` when no props are needed; use a typed props struct when they are

### Types
- **Strings**: Use `Arc<str>` for immutable shared strings (labels, titles)
- **Callbacks**: Use `Callback<T>` for events
- **Strong Typing**: Avoid `Any`, use enums for state/messages
- **Option**: Use `Option<T>` for optional properties with semantic meaning

### Imports
```rust
// Group: std first, then external, then crate
use std::sync::Arc;

use crate::callback::Callback;
use crate::style::{Style, Color};

// Examples/tests can use prelude
use crate::prelude::*;
```

### Error Handling
- Use `thiserror` for custom errors
- Use crate-wide `Result<T>` alias (`src/lib.rs`)
- Avoid `unwrap()`/`expect()` in library code

### Formatting
- Standard `rustfmt` (Edition 2024)
- Trailing commas in multi-line structs/enums
- Keep lines within 100 characters

## Component Pattern

```rust
impl Component for MyComponent {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State { 
        State::default() 
    }
    
    fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update { 
        match msg {
            Msg::Increment => ctx.state.count += 1,
        }
        Update::full() // use Update::with_command(...) when update starts async work
    }
    
    fn view(&self, ctx: &Context<Self>) -> Element { 
        ui! {
            Button::new("Click")
                .on_click(ctx.link().callback(|_| Msg::Increment))
        }
    }
}
```

## Where to Work

- Public API: `src/lib.rs`, `src/prelude.rs`
- New public app-author types (widgets, events, callback payloads, enums, helper
  constructors) must be considered for all export surfaces: defining module,
  `src/widgets/mod.rs`, crate root `src/lib.rs`, and curated `src/prelude.rs`
  when commonly used in app code. Verify direct imports like
  `use tui_lipan::NewType;` before finishing.
- Keep `ratatui`/`crossterm` isolated to `src/backend/` and `src/app/`
- Widgets: usually `src/widgets/<name>/`; small composites can live in `src/widgets/<name>.rs`
- Widget variant registry: `src/widget_manifest.rs` drives generated tag, dimensions, and layout-hash dispatch; add manual enum/render/reconcile wiring too
- Input/event behavior: keep dispatch thin; prefer `src/app/input/handlers/`, `src/app/input/mouse/`, `src/app/runner/mouse_clicks.rs`, and `src/app/runner/drag.rs`

## Documentation

The docs are split into focused files. **Update the right file.**

### Doc map

| Change type | File(s) to update |
|-------------|------------------|
| New widget or widget prop change | `docs/widgets/<category>.md` + `docs/widgets/index.md` |
| New feature flag | `docs/quick-start.md` (feature table) + `README.md` (features table) |
| New callback or event type | `docs/events.md` |
| New enum variant or type change | `docs/enums.md` |
| TextEditor / TextInput changes | `docs/text-editing.md` |
| `clipboard` feature behaviour | `docs/clipboard.md` |
| Component lifecycle / Context API | `docs/components.md` |
| Focus / key bubbling | `docs/focus.md` |
| Keybindings / keymap / chords (`tui_lipan::input`) | `docs/keybindings.md` |
| Style / Color / Theme / Length | `docs/styling.md` |
| `ui!` / `rsx!` macros / constructor keys | `docs/macros.md` |
| Inline viewport mode | `docs/inline-mode.md` |
| New pattern or anti-pattern | `docs/patterns.md` |
| Application performance guidance | `docs/perf.md` |
| Runtime architecture or module-level design | `docs/DESIGN.md` |
| Adding a new widget (contributor) | `docs/widget-authoring.md` (checklist + wiring guide) |

### File overview

```
docs/tutorial.md          End-to-end tutorial: build a complete multi-panel app
docs/quick-start.md       Import map, feature flags, minimal example, App config
docs/macros.md            ui! and rsx! macro reference
docs/components.md        Component lifecycle, update(), commands, async
docs/text-editing.md      TextEditor, TextInput, undo/redo, widget integration
docs/events.md            Event/callback types for all widgets (payload structs)
docs/enums.md             Enum & type reference (all variants with defaults)
docs/perf.md              App performance, update scope, DevTools, tracing, benchmarks
docs/styling.md           Style, Color, Length, Padding, themes, contrast
docs/web-backend.md       Browser/WASM backend
docs/focus.md             Focus traversal, programmatic focus, key bubbling
docs/keybindings.md       keymap.conf, chord API, TextArea newline keys
docs/clipboard.md         Clipboard config, image clipboard
docs/inline-mode.md       Inline viewport mode
docs/external-programs.md terminal_handoff, UI-thread Command::new, request_full_repaint
docs/error-handling.md    Error handling policy (panics vs Error variants)
docs/DESIGN.md            Architecture and runtime internals
docs/patterns.md          Common patterns + anti-patterns
docs/examples.md          Complete example catalog
docs/widget-authoring.md  Contributor guide: how to add a new widget end-to-end
docs/widgets/index.md     Widget category listing
docs/widgets/layout.md    VStack, HStack, ZStack, Frame, Grid, ScrollView, …
docs/widgets/display.md   Text, AsciiCanvas, BigText, Image, Sparkline, Chart
docs/widgets/input.md     Button, Input, TextArea, Checkbox, Radio, Slider, …
docs/widgets/data.md      List, Table, Tree, FileTree, LogView
docs/widgets/feedback.md  ProgressBar, Spinner, StatusBar, Badge, Breadcrumb
docs/widgets/overlays.md  Modal, Toast, Popover, Tooltip, Accordion, SearchPalette
docs/widgets/tabs.md      Tabs, DraggableTabBar
docs/widgets/terminal.md  ManagedTerminal, Terminal, TerminalPty, TerminalScreen
```

### Rules

- **Adding a widget?** Add it to `docs/widgets/<category>.md` with a typed props table, then add a row to `docs/widgets/index.md`.
- **Removing or renaming a prop?** Find and update the props table in the relevant `docs/widgets/*.md`. Check `docs/patterns.md` for code examples that use the old name.
- **Changing the feature flag set?** Update `docs/quick-start.md` (feature table) and `README.md` (features table).
- **Adding a new pattern?** Add it to `docs/patterns.md`.

## Commit Practices

Use conventional commits:
- `feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `style:`, `perf:`, `chore:`
- Imperative mood: "feat: Add button widget" not "Added"
- Keep under 72 characters, no period at end

Sign-off and identity:
- Every commit needs a DCO sign-off: `git commit -s` (see
  [`CONTRIBUTING.md`](CONTRIBUTING.md) for what the sign-off means and how to
  fix a missed one).
- Your commit email is public forever - in every clone, mirror, and archive of
  this repository. Use an address you are comfortable exposing; GitHub's
  `<user>@users.noreply.github.com` address works fine and still credits your
  profile. Whatever you pick, use the same identity for authorship and the
  `Signed-off-by` line.
- AI-assisted contributions are welcome - this framework is substantially
  built with agent assistance. You remain the author of record: review what
  you submit, and remember your sign-off attests that you have the right to
  contribute it under MPL-2.0. Extra attribution trailers beyond
  `Signed-off-by` are unnecessary.

## Pull Requests

PRs are **squash-merged**, so the **PR title becomes the squashed commit
subject** on `main`. The title must therefore be a valid Conventional Commit,
following the same rules as [Commit Practices](#commit-practices):

- `<type>(<optional scope>): <imperative summary>`, `<= 72` chars, no trailing period
- Types: `feat`, `fix`, `docs`, `refactor`, `test`, `style`, `perf`, `chore`,
  `ci`, `release`
- Examples: `fix(scroll_view): clip last row on odd content height`,
  `feat(modal): add max_height + reserve_max_height`

Other expectations:
- One logical change per PR - keep unrelated refactors out.
- The PR body follows [`.github/PULL_REQUEST_TEMPLATE.md`](.github/PULL_REQUEST_TEMPLATE.md);
  work through its checklist (which mirrors `CONTRIBUTING.md`) before requesting review.
- Individual commits inside the PR should also be conventional, but the **title**
  is what ships to history - get that one right.

## Releases (maintainers)

Releases are tag-driven: bumping versions + `CHANGELOG.md`, then pushing a
`vX.Y.Z` tag triggers `.github/workflows/release.yml`, which verifies the tag
against both crate versions and the changelog, runs the test suite, and
publishes to crates.io via Trusted Publishing. Contributors never need to
publish - keeping `CHANGELOG.md` accurate is the contribution that makes
releases painless.
