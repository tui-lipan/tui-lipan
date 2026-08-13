# Changelog

All notable changes to **tui-lipan** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While the crate is on `0.x.y`:
- A **minor** bump (`0.1` → `0.2`) signals breaking changes.
- A **patch** bump (`0.1.0` → `0.1.1`) is backward-compatible only.

## [Unreleased]

### Added

- `TerminalScreen::drain_clipboard_events()` exposes decoded OSC 52 clipboard-store requests so
  terminal hosts can apply or relay child clipboard copies while retaining control of clipboard
  policy. `ClipboardHandle::accept_osc52_store()` applies that policy for managed hosts, and
  `relay_osc52()` relays an already-approved request without requiring a native clipboard provider.
  OSC 52 clipboard loads remain disabled.

### Changed

- Docs site: navbar version reads `Cargo.toml`, headings/links/outline match the
  structural fixes from the rozi site, and markdown links that leave `docs/`
  rewrite to GitHub so they work both in the repo and on docs.tui-lipan.dev.

## [0.2.0] - 2026-08-13

### Added

- `Context::last_mouse()` — last pointer in terminal content coordinates, or `None` until a mouse
  event has been seen. Updated on motion even when the event is forwarded to a tracking terminal, so
  a key binding can place UI at the pointer without a move listener.
- `TUI_LIPAN_SNAPSHOT_SETTLE_MS`, the script action `sleep:500`, and `TestBackend::settle(dt)` — real
  time, pumped, for capturing an app whose content arrives from somewhere a clock cannot reach: a
  spawned process, a socket, a background task. A virtual advance cannot make another thread finish,
  so such an app previously captured as empty chrome with nothing in the widget tree to explain it.
  The env var settles before the action script so it acts on an app that has finished starting;
  `sleep:` waits between actions, which the settle deliberately leaves alone so a mid-animation
  capture stays possible. All default to zero, so no existing capture slows down. Adds a variant to
  the public `ui_snapshot::Action` enum, so an exhaustive `match` on it now needs another arm.
  (breaking)

### Fixed

- The caret no longer shows through whatever is drawn on top of the focused widget. The caret is the
  host terminal's own cursor rather than a cell in the frame buffer, so a floating pane, a popover,
  or any later sibling painted over a focused `Terminal`, `Input`, or `TextArea` still had it
  blinking on top of them. Placement now asks the tree what sits topmost at the caret cell and
  withholds the cursor when that is a layer outside the focused widget's own chain — including on
  the incremental-scroll fast path, which places the caret without running the renderers.
  Interactivity is the limit of what this can see: a purely decorative layer is not a hit-test
  target, so it still lets the caret through, which keeps the failure safe in the direction of a
  caret too many rather than one missing.

- Advancing the virtual clock now also fires `Command::after` timers, so a headless capture or a
  `TestBackend` settles work the framework itself deferred. Previously the clock moved but its own
  timers did not, leaving a gap no caller could close: the deferred command is framework-owned, so an
  application could not settle it, and the harness had no wall clock for it to wait on. Anything
  revealed on a timer — a spawn animation's opacity gate, a debounce, a delayed affordance — stayed in
  its pre-timer state for the whole capture. Timers run inline on the advancing thread, so their
  messages are queued before `advance` returns; chains resolve within one call, bounded so a
  self-rearming timer cannot spin. The timer thread still ignores the virtual offset, so skipping
  virtual time never makes later real timers fire early.

- `FileTree::initial_expanded_paths`, seeding expansion the tree then owns, so an app can restore
  what the user had open after the tree unmounts or re-roots — a file panel following the focused
  document, a sidebar whose tab was switched away and back. Paths may be absolute under the root or
  relative to it, apply on mount and on every root change, and never disturb expansion the user has
  since changed. Ancestors are deliberately not expanded along with a seeded path, so a directory
  the user collapsed keeps its contents' expansion without being reopened by it. Pair with
  `on_toggle` to record the set; ignored when controlled `expanded_paths` is set.
- `Easing::EaseOutBack { overshoot_permille }` and `animation::ease_out_back`, a back ease-out with
  a **tunable** single overshoot. It crosses 1.0 once and settles, which is what makes it usable for
  springy *position* and rectangle animation, where the repeated crossings of `EaseOutElastic` read
  as a 1-cell tremor on a character grid. `Easing::EASE_OUT_BACK` is the standard easings.net
  `easeOutBack` (100‰, peaking at ~1.10 near t = 0.58).

  Because the overshoot is a fraction of the animated *distance*, a fixed amplitude flings a
  long-distance animation proportionally far; the amplitude lets a caller ask for a bounded nudge
  instead (`permille = 1000 * wanted_units / distance_units`). The requested amplitude is what you
  get up to the documented `MAX_BACK_OVERSHOOT_PERMILLE` ceiling of `500`: the curve Newton-solves
  the tension for it, since pinning both endpoints of the cubic family leaves no closed-form
  inverse. Larger requests saturate at the ceiling, and `0` degenerates to a plain cubic ease-out.

  Adds a variant to the public `Easing` enum, so an exhaustive `match` on it now needs another arm.
  (breaking)
- `FileTree::empty_text_padding`, insetting the empty-state placeholder independently of the tree.
  Rows are flush with the surface because the root row starts the hierarchy there, but a placeholder
  is prose rather than a row, and applications usually align it with their other empty states.
  Defaults to none, which is the previous rendering.
- `TUI_LIPAN_SNAPSHOT_ADVANCE_MS`, `Sketch::advance(Duration)`, and
  `TestBackend::advance(Duration)`, so a headless capture can settle time-gated UI
  without sleeping: a which-key panel behind `App::command_chord_reveal_delay`, a finished
  transition. The virtual clock is honoured by chord reveal and the animation ticker, which
  runs in frame-sized steps for the requested duration. Action-script `wait:` uses the same
  path. Default remains zero, so existing captures are unchanged. Virtual advancement
  affects tui-lipan-managed time and animation state; application-owned wall-clock timers
  and `Instant::now()` are not advanced. Headless capture runs the live runner ticker;
  `TestBackend` / `Sketch` tick tree animations, transitions, overlays, and chord reveal,
  not blink or spinner frames.
- `TestBackend::baseline(dir)` and `UiSnapshot::baseline(dir)`, the Sketch baseline
  affordance for apps that need real state. `.name()`, `.tolerance()`, `.check()`, and
  `.assert_baseline()` match Sketch; `TUI_LIPAN_UPDATE_BASELINES=1` accepts the current
  render. Baseline captures still force bitmap rendering.
- `TUI_LIPAN_SNAPSHOT_VIEWPORTS=80x24,120x30,160x40`, capturing a breakpoint matrix in one
  run and writing suffixed files (`app-80x24.png`, …). `TUI_LIPAN_SNAPSHOT_VIEWPORT` still
  writes to the exact path when `_VIEWPORTS` is unset. A malformed entry fails the run
  rather than being skipped.
- `App::command_chord_reveal_delay(Duration)` and `Context::command_chord_revealed()`, for chord
  affordances that should appear only when the user hesitates — a which-key panel, a hint bar. The
  runtime schedules the frame at which the delay elapses, so a view reading `command_chord_revealed`
  needs no timer of its own; a chord completed or cancelled before then never reveals.
  `Context::command_chord_pending()` is unchanged and still flips the instant the chord starts,
  which is what instant chrome (a mode badge, suppressing a caret) should keep using.
  `Context::command_chord_pending_since()` exposes the underlying instant for apps that want their
  own policy, and `Context::set_command_chord_reveal_delay()` retimes it at runtime for an app whose
  delay comes from a config file it reloads while running. The default delay is zero, so behavior is
  unchanged unless the delay is set.
- Root components can implement `Component::on_window_focus_changed` to observe host
  terminal/window focus transitions, with deterministic `TestBackend::set_window_focused` support.
- `Tab::capped(bool)`, opting an inactive, unhovered tab into `Tabs::caps` end caps. A tab that
  carries its own background for an app-specific reason — an unsaved marker, an error state — is
  emphasized in a way the widget cannot infer, and previously read as a flat colored block beside
  shaped peers. The remaining cap conditions are unchanged: untruncated, a background distinct from
  the strip, and caps that fit the padding cells they replace.
- `Style::elevate_by(f32)`, the `Style` form of `ColorTransform::Elevate`. Prefer it over
  `Style::lighten_by` on hover/focus/active state styles: elevation is luminance-aware, so it lifts
  a dark surface and dims a light one instead of washing both toward white, and being a transform it
  composes with a color the element already carries rather than replacing it the way an explicit
  `bg(..)` does. Previously reachable only as `transform_bg(ColorTransform::Elevate(..))`.
- `Color::elevate()`, elevating by the default `0.08` step, completing the
  `dim`/`dim_by`, `lighten`/`lighten_by` pairing. The default is deliberately much smaller than the
  `0.35` those use: one surface step, not a wash.

### Fixed

- `FileTree` no longer strands an empty directory. Expandability came from the loaded children, so
  a directory turned out to be empty, and closing it again left a row with no arrow and no way to
  reopen it — a state only reachable by opening it once, since an unread directory was expandable.
  A directory is now expandable because it is a directory. Collapsed directories also stop emitting
  the placeholder child that used to stand in for this.
- `FileTree::empty_text` now renders for a `ChangedOnly` view with nothing changed, instead of a
  lone root row over an empty list. The root row is the one row a tree always has, so the
  placeholder was unreachable there — but a changed-only root is a heading over a projection rather
  than a directory being browsed, and a heading over nothing says less than "No changes". Browsing
  keeps its root row. A `git status` still in flight holds the heading rather than claiming a clean
  tree it is about to contradict; application-provided change data is taken as final, so an app that
  fetches its own changes should vary `empty_text` while it loads.
- `Esc` cancelling a pending command chord is no longer also delivered to the focused widget. One
  press did two things, and for a terminal that was not a harmless extra byte: terminals read `ESC`
  followed by a key as `Alt+<key>`, so cancelling a leader chord silently turned the next keystroke
  into a meta chord. With no chord pending, `Esc` is untouched and still reaches whatever has focus.

### Changed

- `TestBackend::advance(Duration)` now consumes the full duration in 50 ms steps rather than
  clamping to one frame. **(breaking)** The previous one-frame clamp is
  `TestBackend::advance_frame(Duration)`. `backend.advance(400ms)` no longer silently means
  50 ms. `Sketch::advance` is unchanged (it already consumed the full duration).
- `RuntimeEnv::command_chord_pending: Rc<Cell<bool>>` becomes
  `command_chord_pending_since: Rc<Cell<Option<Instant>>>`, so the pending chord carries its start
  time rather than duplicating it in a second cell that could drift. **(breaking)** Migration: read
  `Context::command_chord_pending()` as before; code touching the field directly replaces
  `.get()` with `.get().is_some()`.
- `Color::elevate(f32)` is renamed `Color::elevate_by(f32)`, so every amount-taking color modifier
  ends in `_by` and the bare name is free for the default-amount form. **(breaking)** Migration:
  rename `.elevate(x)` to `.elevate_by(x)`; `.elevate()` now means the default step.
- `DatePicker` and `Radio` use roving focus (one tab stop) instead of making every
  day cell / option a tab stop. Only the selected day / active option is
  focusable; arrows move selection and focus follows via `focus_key` (derived by
  default from `DatePicker` title / `Radio` option labels). `DatePicker::on_focus`
  / `on_blur` are now `Callback<DateEvent>` and `Radio::on_focus` / `on_blur` are
  `Callback<usize>`. Debug builds assert unique focusable keys after reconcile.
  **(breaking)** Update handlers that expected `Callback<()>`.
- `Hyperlink` `on_key` now runs **caller-first**: returning `true` consumes the key
  before built-in Enter/Space activation. Previously activation ran first and the
  custom handler was a fallback. **(breaking)** Migration: if your `on_key` should
  not block activation, return `false` for Enter/Space (or omit those keys).
- `ClipboardError` gained an `InvalidInput { operation, message }` variant for arguments the
  clipboard cannot represent, distinct from `Provider` (the platform clipboard failed) and
  `Unsupported` (the operation does not exist here). Retrying `InvalidInput` without changing the
  input cannot succeed. **(breaking)** Only exhaustive `match` over `ClipboardError` needs
  updating - add an `InvalidInput` arm, or match `{ .. }` as the existing call sites do.

### Fixed

- Handing the terminal to another program - a `ctrl+z` suspend, `$EDITOR`, a pager - now restores
  the cursor. Ratatui hides it on every frame drawn without one and shows it again only when
  `Terminal` drops, which a handoff never does, so the shell prompt came back with no visible
  cursor until something else turned it on.
- A `SIGTSTP` from outside the app (`kill -TSTP`, a parent shell) no longer stops the process with
  the terminal still in raw mode, on the alternate screen and with mouse tracking on - which left
  the shell prompt drawing over the frozen UI while mouse motion printed escape sequences into it.
  The runner now takes the signal, releases the terminal at a frame boundary, and stops for real
  with the default disposition back in place, so the shell still sees the job stop.
- Mouse all-motion tracking (`1003`) is re-armed after the terminal comes back from an external
  program or a suspend. Only basic capture is part of the resume sequence, so hover stayed dead
  after returning from `$EDITOR` until something else toggled motion tracking.
- Wheel events forwarded to a `Terminal` whose child has mouse tracking on no longer lose ticks.
  The runner coalesces a burst of same-direction wheel events into one dispatch, but the terminal
  forwarding path emitted a single mouse report and dropped the count, so the child saw one tick
  where the host sent several. Hosts that emit several wheel events per physical notch made this
  permanent rather than merely bursty: a TUI running inside a `Terminal` scrolled a fraction of
  what the same TUI scrolled outside it. Forwarding stays a passthrough - the app-level
  `scroll_wheel_multiplier` applies to local scrolling only, never to reports sent to a child.
- Scripted `ui_snapshot` wheel actions apply `scroll_wheel_multiplier` once instead of squaring it.
- `SearchPalette` can preserve the caller's matched-item order and can prioritize right-aligned
  metadata over long labels through `preserve_item_order` and
  `primary_truncate_description_first(false)`.
- Terminal-native paste now cancels any pending app-command chord and repaints the cleared prefix
  indicator, rather than leaving stale chord state visible.
- Tabs hit-testing now follows actual rendered widths through Unicode truncation and ellipsis;
  divider cells are inert for selection and per-tab hover, with symmetric repainting in either
  direction.
- `Splitter` hands each leftover column to the pane with the largest dropped fraction instead of to
  the leftmost panes in index order. Sizes are floored from weights, so the spare columns were
  landing on pane 0 regardless of which pane earned them - `[0.09, 0.45, 0.46]` across 10 columns
  gave `[1, 5, 4]` rather than `[1, 4, 5]`. A drag round trips sizes through weights every frame,
  so the misplacement could shift a pane the drag never touched. The weights-to-sizes round trip is
  now exact.
- `yazi` example no longer bumps `weights_nonce` on every splitter resize. The nonce means
  "override whatever the splitter currently holds", so echoing it back each drag tick forced the
  splitter to re-derive its exact columns from rounded weights instead of keeping its own.
- Table headers no longer underline the selection-gutter spaces, so labels like `PID` do not pick
  up a long underscore from `unselected_symbol` padding.
- Measurement honors explicit `.width(Length::Px/Percent)` / `.height(...)` for every widget.
  Auto/`Center` parents previously shrink-wrapped to content-only measure, so a child
  `Input::…width(Length::Px(40))` (and the same pattern on `Button`, `Text`, `Checkbox`,
  `Spacer`, …) was clamped away at reconcile. `Flex` still shrink-wraps so it does not
  inflate Auto parents; `Percent` still falls back to content when the parent size is unknown.

### Added

- `Context::suspend_to_shell` stops the app to the shell the way `ctrl+z` does in an ordinary
  program. Raw mode clears the tty's `ISIG` flag, so the terminal driver never generates `SIGTSTP`
  while a TUI runs and an app that wants that keybinding has to ask for it. At the next frame
  boundary the runner hands the terminal back (raw mode, alternate screen, mouse tracking), stops
  the process group, and restores the terminal with a full repaint once the job is foregrounded.
  No-op on targets without POSIX job control (Windows, wasm).
- `SearchItem::priority` (and `SearchEntry::priority`) pins matched rows ahead of the rest of a
  `SearchPalette` result list: higher values lead, ties keep the order matching produced (score
  order, or source order under `preserve_item_order`). It applies to the unfiltered list and to
  search results, and is skipped under `preserve_groups`, where result order is the visual order
  navigation walks. Callers that build `SearchItem` with a struct literal instead of
  `SearchItem::new` need the new field. **(breaking)**
- `SearchPalette::preserve_item_order` keeps prefiltered results in source order across synchronous
  and asynchronous matching paths.
- Opt-in compact keybinding display via `KeyBinding::compact_display()` and
  `KeyBindings::compact_display()`, plus `format_binding_compact` and
  `format_bindings_compact` helpers. Shift-only letters and US-layout punctuation render as
  produced glyphs, equivalent alternatives are stable-deduplicated, and executable binding
  identity is unchanged.
- Interaction API parity for `Slider`, `DatePicker`, and `Radio`: `disabled` /
  `disabled_style`, focus props (`focusable`, `tab_stop`, `on_focus`, `on_blur`),
  and `on_key` where applicable. `Slider` also gains track `hover_style` (with
  `extend_*` / `inherit_*` / `*_style_slot`) and a keyboard handler (arrows /
  Home / End). `Radio` stores `hover_style` / `focus_style` as `StyleSlot` so it
  participates in theme slot inheritance; existing `hover_style(Style)` /
  `focus_style(Style)` call sites keep working via `StyleSlot::Replace`.
  Compile-time guardrail: `tests/interaction_parity.rs`.
- `Select`, `ComboBox`, and `MultiSelect` surface whole-control `focusable`,
  `tab_stop`, `on_focus`, `on_blur`, and `on_key`, forwarded to their inner
  trigger/input/list. Caller `on_key` runs before built-in navigation
  (same rule as `Hyperlink`). Documented as the shared interaction contract in
  `docs/widgets/input.md`.
- Visual regression baselines for Modal, Tabs, TextArea (line numbers + caret),
  Table (header + selection gutter), expanded Select dropdown, and Splitter
  handle chrome in `tests/visual_baseline.rs` / `tests/ui-baselines/`.
- `CheckboxVariant::Switch` (`●` / `○` / `◐`) for switch-style checkbox glyphs
  without a separate Switch widget.
- Extracted snapshot / visual testing docs into `docs/testing.md` (linked from
  the docs index and VitePress sidebar); fixed `docs/examples.md` drift
  (removed duplicate scroll-view rows, catalogued missing examples).
- File clipboard support: `ctx.clipboard().copy_files(&[path])` places real files on the system
  clipboard, so pasting into a file manager, file dialog, or browser upload target yields the files
  rather than their paths as text. `read_files()` reads a file list back (empty `Vec` when the
  clipboard holds none) and `supports_files()` reports whether the provider can exchange them at
  all, for hiding the affordance on the web backend or in builds without the `clipboard` feature.
  A path that does not exist fails the whole call with `ClipboardError::InvalidInput` naming it,
  rather than silently copying a shorter list the way the platform clipboards do on their own.
  `copy_files` deliberately does not emit OSC 52, which is text-only and would downgrade a file
  copy to a path string. `ClipboardProvider` gained `read_clipboard_files`, `write_clipboard_files`,
  and `supports_file_clipboard`, all defaulted to unsupported so existing providers keep compiling.
  This is also the answer to dragging files out of a terminal, which is not possible: the OS drag
  protocols belong to the terminal emulator's window, not to the process drawing inside it. See
  `docs/clipboard.md` for the full explanation and the GUI-helper workaround.
- `yazi` example yanks the selected file or directory onto the clipboard with `y`, and demonstrates
  the app-side drag-source helper (`ripdrag`/`dragon-drop`) behind `D`.
- `ChartSeriesMode::Braille` renders dense chart traces on a 2x4 subcell grid instead of limiting
  connected samples to one glyph position per terminal cell.
- `ChartAxis::tick_labels(...)` labels an axis with domain values instead of sample indices, so a
  time series reads `22:40:00 … 22:40:59` rather than `0 … 59`. Labels spread evenly across the
  axis with the ends anchored flush; any label that would collide with its neighbour is skipped, so
  a dense set thins out in a narrow plot instead of overprinting. Axes without tick labels keep the
  existing numeric endpoints.
- `process_monitor` example recreates a compact desktop process inspector with live metric cards,
  a selectable telemetry table, memory graph, event ledger, and keyboard command bar.
- `Graph::focus_offset_for(path, viewport_w, viewport_h)` for controlled `PanView` focusing: same
  coordinate space as `center_offset_for`, but clamps to content bounds so edge nodes only move far
  enough to stay visible instead of scrolling empty space past the diagram.
- Live control channel: `TUI_LIPAN_CONTROL=<path>` makes a running app listen on a Unix socket, so
  an agent can inspect and drive a live TUI - `snapshot` for the widget tree, `keys` for what can be
  targeted, `act <script>` to click or type, `quit` to exit. Replies are a status line plus a
  length-prefixed payload, which keeps markdown and JSON newline-safe without escaping and makes a
  client a few lines in any language. Unix only; the socket is created `0600`. Runtime state stays
  single-threaded: the listener thread queues requests and the event loop answers them, matching how
  the terminal reader already feeds the loop.
- `highlight <key>`, `highlight <col>,<row>`, and `highlight clear` on the control channel outline a
  widget for inspection, the way a browser's element inspector does. Drawn over the finished frame,
  so it marks a widget whether or not that widget styles itself for hover or focus - the gap hover
  alone left. Large rects are outlined and thin ones filled; the outline reaches captures as well as
  the live paint, so a PNG shows what the operator sees. Cell targeting resolves to the smallest
  covering widget, keeping unkeyed widgets inspectable.
- `hover:` actions now update hover state. Hover tracking and `on_mouse_move` handlers are separate
  paths, and a bare move only reached the latter - which returns early when a widget has no move
  handler, so hovering silently did nothing while a click (which moves, presses, releases) worked.
- `AppRunner` records the bounds of its latest layout pass, so a capture taken between frames uses
  the real viewport instead of a zero-sized one.

- Action scripts drive a UI beyond typing: `click`, `rclick`, `mclick`, `hover`, `focus`, `scroll`,
  `drag`, `type`, `key`, and `wait`, separated by `;` or newlines. Available as
  `TUI_LIPAN_SNAPSHOT_SCRIPT`, `TUI_LIPAN_RECORD_SCRIPT`, and `Recording::script`, so a modal
  behind a button or a row behind a scroll is reachable without writing a harness. Widgets are
  targeted by reconciliation key (`click:#submit`), which resolves through the current tree and
  **fails loudly** when the key is absent - a coordinate would silently click empty space after a
  layout change. Raw `col,row` targets remain available. `TestBackend::rect_of_key` and
  `TestBackend::focus_key` are public, and one executor backs the headless backend and the live
  runner so a script means the same thing everywhere.

- `Recording::write_frames(dir)` and `TUI_LIPAN_RECORD_FRAMES=<dir>` export one truecolor PNG per
  frame (feature `ui-snapshot-png`), for encoding to MP4 without GIF's 256-colour quantisation.
  Frames are written at a constant rate - one per `1/fps` tick including unchanged ones - because
  an encoder reconstructs timing from a numbered sequence; unchanged frames reuse the previous
  encode rather than paying for it twice. Both print a ready-to-run `ffmpeg` command with paths and
  frame rate filled in. `Recording::png_options` controls rendering (raise `scale` for a
  higher-resolution video). `docs/components.md` gains a format comparison with example commands
  for `.cast`, GIF, and both MP4 routes.

- Terminal recording: `TUI_LIPAN_RECORD=<path>` plays a key script against an app off-screen and
  writes an asciinema cast v2 file, so any existing app or example becomes a recordable demo with
  no source change. `Recording` is the in-code equivalent and mirrors `Sketch`. Companion
  variables: `TUI_LIPAN_RECORD_VIEWPORT`, `_FPS`, `_KEYS`, `_KEY_DELAY_MS`, `_SETTLE_MS`.
  A recording is text rather than video - a few seconds of a real app is typically smaller than a
  single PNG frame of it - and needs **no feature flag and no new dependency**, because the cast
  JSON is written directly rather than through `serde_json`.
- `CastRecording` builds asciinema casts frame by frame, encoding each frame as an ANSI diff
  against the previous one via the existing `CapturedFrame::to_ansi_diff`. Identical frames are
  dropped so a still stretch costs nothing; `mark_time` then holds the closing frame, without which
  a recording would end at its last visible change and players would cut the ending short.
  Timestamps use a synthetic fixed step and no wall-clock header field, so the same script always
  produces identical bytes and a committed recording stays diffable.
- `tests/visual_baseline.rs` guards core widget chrome (frame borders and headers, focus chrome,
  input placeholders and masking, list selection) against unintended pixel changes, with reference
  images committed in `tests/ui-baselines/`. Intended rendering changes are re-recorded with
  `TUI_LIPAN_UPDATE_BASELINES=1 cargo test --all-features --test visual_baseline` and committed in
  the same PR; see `CONTRIBUTING.md`.
- Visual regression baselines: `Sketch::baseline(dir)` stores one image per capture, compares the
  next render against it pixel by pixel, and writes a highlighted `*.diff.png` beside any baseline
  that changed (unchanged pixels dimmed, changed pixels magenta). `check()` returns
  `Vec<BaselineComparison>`; `assert_baseline()` fails listing every regression at once.
  `tolerance(ratio)` sets the maximum differing-pixel fraction still counted as a match (default
  `0.0`). `TUI_LIPAN_UPDATE_BASELINES=1` accepts current output as the new baseline. Baseline
  captures force `PngTextRenderer::Bitmap`, because default font discovery resolves differently per
  machine and makes pixel comparison meaningless across CI and local checkouts.
- `TUI_LIPAN_SNAPSHOT_KEYS` and `Sketch::keys` dispatch a key script (`"tab,tab,enter"`) before
  capturing, so states behind a keystroke - an open modal, a submitted form, an error - are
  reachable without writing a harness. Entries use ordinary keybinding syntax. Each key is
  dispatched, its messages drained, and the tree re-rendered before the next, matching the event
  loop; batching them instead collapses typed text to its last character. An unparseable script
  fails the capture rather than being skipped.
- `TUI_LIPAN_SNAPSHOT=<path>` makes `AppRunner::run()` render one frame off-screen, write a
  snapshot artifact, and return without entering raw mode or opening a terminal. This captures an
  existing app or example without editing its source, and works with no tty (CI runners, agent
  sessions). Format follows the path extension, matching `Context::request_ui_snapshot_to`.
  Companion variables: `TUI_LIPAN_SNAPSHOT_VIEWPORT` (`WIDTHxHEIGHT`, default `100x30`),
  `TUI_LIPAN_SNAPSHOT_FRAMES`, `TUI_LIPAN_SNAPSHOT_FOCUS`, and `TUI_LIPAN_SNAPSHOT_DIAGNOSTIC=1`.
- `Sketch` renders a view function or `Component` at one or more viewports and writes markdown,
  PNG, and JSON artifacts in a single call, so a design capture is small enough to keep in the
  repository rather than be written and deleted. `Sketch::view` mounts any `Fn() -> Element`
  through `Mockup`; `viewport`, `fit`, `focus_next`, `options`, `dir`, and the per-format toggles
  configure it. Defaults to `80x24` plus a fit-to-content pass, written to `target/ui-sketches/`
  (override with `TUI_LIPAN_SKETCH_DIR`).
- `UiSnapshotFileFormat::from_path` routes a snapshot path to a format by extension. Previously
  this logic was private to `Context::request_ui_snapshot_to`.
- `examples/sketches/` holds kept design sketches, registered in that directory's `main.rs` so a
  new sketch needs no `Cargo.toml` change. Run with `cargo snap sketches`.
- `cargo snap <example>` alias runs an example with `ui-snapshot-png,ui-snapshot-json` enabled, so
  capture runs and ordinary `cargo check`/`cargo test` stop invalidating each other's build
  artifacts.
- `QrCode` (feature `qr-code`) renders a scannable QR symbol as terminal cells. Defaults to
  half-block mapping so the symbol stays square despite the 1:2 terminal cell aspect ratio, a
  spec-mandated 4-module quiet zone, and explicit black-on-white so a dark terminal palette cannot
  invert it out of scanning range. `QrRender::Wide` trades columns for physically larger modules.
  Because a QR symbol cannot reflow, `size()` and `module_count()` report the fixed footprint up
  front so callers can swap in a fallback rather than render a clipped, unscannable symbol;
  `fallback` covers payloads past QR capacity.
- `DraggableTabBar::empty_text` / `empty_text_style` show a left-aligned placeholder when the bar
  has no tabs. The placeholder truncates with an ellipsis to the available width, patches onto the
  resolved bar style, and stays non-interactive. Defaults keep the empty bar blank.
- `ColorTransform::Elevate(f32)` is the relative form of `Color::elevate`: luminance-aware, so it
  lightens a dark color and dims a light one, and hue- and chroma-preserving rather than washing
  toward white. Use it where a transform has to land on the same color an absolute `Color::elevate`
  step produces elsewhere in the UI, which `ColorTransform::Lighten` no longer does.

- `Context::set_devtools_metrics` lazily replaces ordered host-application label/value rows in a
  content-sized, viewport-capped DevTools App tab. Publication is render-neutral: host-view rows
  are consumed by the panel later in the same frame, and no frame is scheduled by replacing them.
  The factory is not invoked without the `devtools` feature. `Context::devtools_visible` reads the
  runner-synchronized panel state and returns `false` when the feature is disabled.
- `TerminalScreen::try_for_each_text_line` streams clamped absolute line ranges through one reused
  scratch buffer with immediate early exit. Existing plain-text exports and terminal snapshots now
  append cell text directly instead of allocating a `String` per cell.
- `DraggableTabBar::overflow_left_label` and `DraggableTabBar::overflow_right_label` take a
  formatter over the hidden tab count, so apps can replace the Nerd Font overflow indicators.
  Control widths and hit targets are measured from the custom label; defaults are unchanged.
- `Toast::min_width` sets a minimum width constraint on toast overlays (alongside existing
  `max_width`).
- `Tree::indent_width` / `FileTree::indent_width` set indentation cells per hierarchy level
  (default `2`), and `Tree::indent_guide_start_depth` / `FileTree::indent_guide_start_depth` set the
  first non-root depth that renders guides (default `1`). Width `1` with short guides produces
  compact `├item` rows.
- `TreeNode::expandable` keeps expand/collapse behavior on a node whose child list is currently
  empty, for a directory whose contents load asynchronously. Toggling a node that is neither
  expandable nor populated is now a no-op rather than emitting `on_toggle`.
- Add `FileTree::on_explorer_focus`, `FileTree::on_explorer_blur`, and
  `FileTree::on_explorer_escape` for routing explorer focus by its pointer/tree origin. Explorer
  focus entered from the tree with `/` still returns to the tree on Escape.
- Add configurable `FileTree::tree_focus_key` and `FileTree::explorer_focus_key` targets for
  composite focus routing and multiple simultaneously mounted trees.
- Add `Tree::focus_key` for assigning a focus key directly to the tree's focusable list node.
- `DraggableTabBar` now keeps successive close buttons under the pointer by temporarily giving a
  deleted tab's width to its replacement until the mouse leaves the bar. Action tabs (for example a
  trailing `+`) are excluded from that width lock.
- `KeyBinding` parsing accepts the plus key as a bare `+` chord step or the name `plus`
  (including `alt-plus` / `alt-+`), while `+` inside a mixed step remains a modifier separator
  (`ctrl+c`). Canonical display maps crokey's `Hyphen`/`minus` labels to `-`.

### Fixed

- `TerminalPty` no longer loses the output of a command that writes and exits immediately.
  `TerminalPtyEvent::Exited` came from a thread blocked on `child.wait()`, which returns the instant
  the child dies — usually before the reader thread has been scheduled to pick up the bytes still
  buffered on the master. Consumers reasonably treat `Exited` as "this PTY is finished" and drop the
  handle, and dropping kills the reader, so those bytes were discarded rather than delivered late: a
  fast command could show its exit status and none of its output. The exit event is now emitted by
  whichever side can prove the stream is drained — the reader on end-of-stream or on an idle master
  after the child is gone, with the wait thread stepping in only if the reader cannot get there — so
  `Output` always precedes `Exited`. A killed PTY still reports its exit immediately.

- `SearchPalette` no longer swallows its navigation keys when nothing matches. The internal input
  interceptor claimed `Enter` (and the arrows, `PageUp`/`PageDown`, `Home`/`End`) unconditionally,
  so an empty result list still sent an activation for a row that does not exist and reported the
  key handled — `input_key_interceptor` never saw it. Those keys now fall through to the caller
  where the palette has no row to act on, which is what lets `Enter` mean "create what was typed"
  or "start something new" in an empty list. Navigation still outranks the caller everywhere the
  palette can act on it.
- Clearing or entering an app-command chord while a terminal has focus now claims a frame for chord
  chrome (`command_chord_pending`), even when the key itself is forwarded with `DirtyLevel::None`.
  A mismatch or Esc cancel no longer leaves a stale PREFIX indicator painted over a busy child TUI.
- Clicking a `FileTree` explorer input now focuses it even when the tree is inside an excluded focus
  scope.
- Cross-bar `DraggableTabBar` drops now select the transferred tab on the destination bar by
  emitting its `on_change` callback after `on_transfer`, restoring the expected active style.
- Divider junctions now keep every same-axis segment that shares a cell (two titled horizontals
  meeting a vertical) and pick the glyph from the union of arms, so a descending vertical tees as
  `┬` instead of cornering as `┌`.
- Fuzzy/Exact `BorderMergeMode` no longer wipes a neighbor's border-title on a shared seam that
  already carries box-drawing: non-box glyphs stay put, and spaces that sit next to that text (the
  `icon  title` gap) are preserved. Borders still replace ordinary underlay content (e.g. a modal
  over text). Plain backdrop spaces still accept a border so parent fills with a foreground color do
  not suppress frames. `Replace` still overwrites so occluding frames win.

### Changed

- MSRV raised to Rust 1.90 (breaking). The manifest declared `1.88` while the `image` feature family
  resolves `ratatui-image → icy_sixel → quantette`, and `quantette 0.5.1` requires `1.90`;
  `icy_sixel 0.5.0` pins that exact version, so no older resolution avoids it. `README.md` and
  `CONTRIBUTING.md` had also drifted to a stale `1.85`, so all three now agree. The crate's own code
  and its default features still build on 1.88 - update your toolchain (`rustup update stable`) if
  you enable `image`, `terminal-images`, `image-full-formats`, or `ui-snapshot-png`.
- The `todo` / `todo_ui` examples drop the outer "Todo App" bordered frame, give the New Task input
  row `height: Length::Auto` (input stays `Flex` width), and nest Tips in a flex `VStack` so tip
  text yields space instead of collapsing the bordered input in short terminals.
- Floating `DragPreview::SourceSnapshot` previews are no longer forced to stay fully inside the
  terminal. They stay anchored to the grab point under the cursor and cells that leave the
  viewport are clipped, so large cards can slide partially off-screen (as in
  `examples/drag_drop_kanban.rs`).
- `DragSource` `preview_max_width` / `preview_max_height` of `None` no longer apply the old
  60×20 defaults (breaking). Unset means paint the full source snapshot; pass
  `Some(DEFAULT_PREVIEW_MAX_WIDTH)` / `Some(DEFAULT_PREVIEW_MAX_HEIGHT)` to keep the previous caps.

- `CapturedFrame::to_png` and `UiSnapshot::to_png` / `to_png_default` now return
  `Result<Vec<u8>>`, and `try_to_png` / `try_to_png_default` are removed (breaking). The infallible
  forms returned an **empty buffer** when encoding failed, which wrote a zero-byte file that only
  failed later, when something tried to read it. Migration: add `?` (or `.expect(...)`) at the call
  site, and rename any `try_to_png*` call to `to_png*`.
- The `tui-lipan-ui-sketch` and `tui-lipan-visual-design` agent skills are merged into a single
  `tui-lipan-visual` skill (breaking, for anyone who copied them into their own skills directory).
  They overlapped and gave contradictory advice: one told agents to delete their snapshot harness
  when finished, the other told them to keep it. The merged skill ranks captures by cost - env-var
  capture of a running app, then a kept `Sketch`, then a real test - and drops the throwaway
  harness pattern entirely.
- `Color::elevate` now applies its endpoint-blended lightness in OKLab with stable, gamut-mapped
  relative chroma, preserving authored casts without weakening elevation or amplifying one-step
  near-black tints.
- Divider `label_padding` no longer clears blank cells around the label. Inset cells keep the
  divider character (so a left inset of `1` reads as `─title`, matching Frame border titles), and
  only the label's own cells interrupt the line. The value is now independent left/right insets
  (`label_padding(n)` still sets both; `label_padding_axes(left, right)` sets them separately).

### Added

- Add `MouseRegion::drag_threshold(columns, rows)`, overriding how far the pointer must travel
  from the press before drag callbacks start. The default is unchanged and now named
  `DEFAULT_DRAG_THRESHOLD` (3 columns or 1 row) — loose on columns because a cell is about twice
  as tall as it is wide, so the same tremor covers more of them. That asymmetry is wrong for a
  region whose only gesture is dragging: a horizontal drag on a resize handle or split divider
  ignored the first two columns and then arrived three cells out in a single jump. Such regions
  can now ask for `(1, 1)` and be tracked from the pointer's first step. Clearing a lowered
  threshold still marks the gesture as a drag, so the release is not also delivered as a click.
- Anchor terminal selections to absolute retained scrollback lines, preserving selection and copy
  behavior while scrolling, receiving output, and edge-autoscrolling a mouse drag. Replace the
  viewport-based `TerminalSelection` alias with terminal-specific `TerminalPos`,
  `TerminalSelection`, and `TerminalSelectionEvent` types (breaking).
- Add the `terminal-images` feature: programs running inside a `Terminal` pane can draw pictures
  with the Kitty graphics protocol. `TerminalScreen` reads `APC _G` out of the PTY stream and
  decodes it rather than forwarding it, so the host terminal does not have to speak Kitty — the
  pixels are re-encoded through the same path the `Image` widget uses, down to half-blocks. That is
  also what lets two panes pick the same image id, and a half-scrolled pane crop its pixels instead
  of squashing them. Both ways of placing an image are read: at the cursor, as `icat` does, and
  through Unicode placeholder cells (`U=1`), as terminal UI toolkits including `ratatui-image` do.
  Cursor placements are anchored to absolute scrollback lines, so images scroll, scroll back, and
  evict with the text they were drawn against; placeholder placements are read off the grid and so
  follow the cells holding them. Decoded pixels are held to a per-screen budget
  (`TerminalScreen::set_image_budget`, 96 MiB by default). New public types `TerminalImage`,
  `TerminalImagePlacement`, `TerminalImageCrop`, and the snapshot field
  `TerminalRenderSnapshot::images` (breaking: the struct gained a field). Unsupported requests —
  file/shared-memory transmission, the protocol's own animation frames — are answered with the
  protocol's own `ENOTSUPP` report. See [`docs/widgets/terminal-images.md`](docs/widgets/terminal-images.md).
- Keep a pane's images across a width change unless the resize actually rewraps text. Treating
  every column change as a rewrap cost a pane every image in it on each resize, which in a tiling
  multiplexer is every time a neighbour opens.
- Give `TerminalImagePlacement` the `image_id` its transmission used, and key the renderer's
  encoding cache on it. Two placements holding identical pixels shared one encoding and so one
  Kitty image id, which a host reads as a single placement — it drew one and dropped the other, so
  repeated copies of a picture vanished as new ones arrived.
- Accept a graphics transmission sent as one oversized escape rather than the chunks the protocol
  asks for. Raw pixels in a single escape clear 64 KiB with a picture only a few hundred cells
  wide, and the old per-escape bound dropped those silently — indistinguishable, from the sender's
  side, from a terminal with no graphics support at all.
- Add `TerminalCellSize`, `host_cell_size()`, `TerminalScreen::set_cell_size`, and
  `TerminalPtyConfig::cell_size` / `TerminalPty::resize_with_cell_size`. The PTY now reports pixel
  dimensions in `TIOCGWINSZ` instead of zeroes, and `CSI 14 t` (text-area size in pixels) is
  answered instead of ignored, so a child that measures itself before drawing no longer waits out
  its own timeout. `ManagedTerminal` wires the host's real cell size to both ends automatically.
- Add `Context::animated_color` and `Paint::Animated` for colours that only feed a style. The returned
  paint *names* its transition instead of carrying the current colour, so the element tree holds still
  for the whole fade and the runtime advances it with a repaint: a 160 ms focus fade at 60 fps costs
  ten repaints where `Context::transition` costs ten full rebuilds of the window. The renderer
  resolves the slot while drawing. Reach for `transition` when the interpolated value has to inform
  layout, text, or a decision — that is the trade that makes skipping `view()` sound.
- Add `Terminal::screen` and `TerminalScreenHandle` so a terminal can read a `TerminalScreen` the app
  owns instead of being handed a `TerminalRenderSnapshot`. This takes the screen's contents out of the
  element tree, so an app can answer new output with `Update::paint()` rather than a full rebuild; the
  runtime pulls the current snapshot before each draw. `Terminal::decorations` overlays search hits or
  hint labels on a live screen, so those survive a paint-only frame.
- Make `UpdateLevel` and `Update::level` public, and add `TestBackend::update_level` to report the
  refresh level a message's `update()` asked for. Worth asserting on for messages that arrive per
  keystroke or per chunk of streamed output.
- Add `TestBackend::refresh_live_terminals` to model a paint-only frame in tests: it pulls live screen
  contents into the node tree without running `view()`.
- Pause a toast's auto-dismiss countdown while it is hovered, and revive an automatically fading
  toast with a short post-hover grace period when the pointer catches it during fade-out.
- Add `ToastHandle::renew` to restart an active toast's dismissal countdown without changing its
  stack order or replaying its enter transition.
- Add `Animated::auto_exit` and `ExitAnimation` for automatically retaining and
  animating removed keyed children of `VStack`, `HStack`, `Canvas`, and
  `ZStack`. Retained subtrees are inert, suppress descendant transition-end
  callbacks, preserve simultaneous removal order and Z depth, and expire after
  the exit completes or its retention deadline. Stacks collapse on their main
  axis; positioned containers retain their geometry unless collapse is requested.
- Add keyed `ExitQueue` state for application-owned exit animations.
  `ExitQueue::with_exit_timeout` bounds retention when a host stops rendering,
  while `transfer_out` and `adopt` move an `ExitTransfer` between collections
  without restarting its exit.
- Add `auto_exit` and `exit_animation` examples for framework-retained and
  application-owned exit lifecycles.
- Add `TestBackend::advance(dt)`, which ticks every time-based animation and
  refreshes the rendered tree as needed. Tests can now deterministically step
  `Animated` transitions, smooth scrolls, and `Context::transition` property
  animations using the runner's ordering and 50 ms per-frame clamp.
- Add sentinel resolution helpers on `Color`, `Style`, and `Theme`, plus
  `TerminalColorPalette::from_theme`; host-derived themes retain their exact
  `HostTerminalColors` as a typed theme extension.
- Add `Badge` segment caps for powerline-style chains: `CapStyle`, `CapSides`,
  and the `cap`, `cap_sides`, `cap_behind`, and `cap_same_color` builders. The
  `powerline_bar` example demonstrates cap styles, equal-color seams, and Nerd
  Font fallback.
- Add display-column span editing, untrusted display-text sanitization, and
  dependency-free URL/path/Git hint scanning under `utils`; the optional
  `hints-regex` feature adds string-configured custom scanners.
- Add terminal snapshot decorations, display-column selection extraction,
  `TerminalCopyMode`, cell-cursor text motions, and targeted
  `Context::flash_copy_feedback` requests.
- Add `Context::flash_copy_feedback_range` for copy-and-exit flows. The existing
  `flash_copy_feedback` paints the widget's live selection, forcing callers that
  leave their selection mode on copy to keep the selection alive purely so the
  flash has something to draw; the range variant carries the copied range and
  paints it independently of what the node has selected.
- Add terminal search, hint, and copy-mode examples.
- Add `Terminal::caret_color` for setting the focused hardware caret color through OSC 12.
- Add `TerminalPasteShortcutBehavior::Performable` for terminal hosts that bind direct `Ctrl+V`:
  plain text is emitted through terminal paste input, while file lists, images, and unknown
  non-text clipboard formats forward the original key to clipboard-aware child applications.
- Add `FileTreeEntrySource::Provided` for asynchronously supplied directory listings. Missing
  expanded paths render the existing loading row and emit `FileTreeEntryRequest`; completed
  `FileTreeDirectoryListing` values carry child type, symlink, Git status, ignore, and error data
  without enumerating the local filesystem. `FileTreeEntrySource::Local` remains the default. The
  `provided_file_tree` example demonstrates the complete request/command/delivery cycle.
- Add the opt-in `syntax-extra` feature with bat-curated syntax definitions for
  broad language coverage, including TOML, TypeScript/TSX, Dockerfile, Vue, Zig,
  and Terraform. The `yazi` example now runs with `--features syntax-extra`.
- Add the `yazi` example, a compact Yazi-inspired file browser with resizable
  borderless panes, keyboard/mouse navigation, full-row selection, directory
  previews, Nerd Font icons, pill selection, and syntax-highlighted file
  previews.
- `List::selected` and `Table::selected` now take `impl Into<Option<usize>>`, so
  apps can pass `None` for no current row. Defaults remain `Some(0)`. Bare
  integers still work via `From<T> for Option<T>`. With no selection, the first
  navigation key (arrows, vim keys, `Home`/`End`, `PageUp`/`PageDown`)
  establishes a cursor at the first or last selectable row instead of doing
  nothing; `Enter` stays inert until a row is current.
- `Tree::clear_selection` / `FileTree::clear_selection` suppress the selection
  highlight authoritatively over both the controlled `selected` prop and
  internal state.
- `tui_lipan::utils::{directory_icon, directory_icon_span}` expose the shared
  Nerd Font folder glyph (`U+E5FE` open / `U+E5FF` closed) used by `FileTree`.
- Add `Popover::capture_focus` for passive root-portal overlays that must leave keyboard focus on their trigger.

- `ScrollView::reveal_horizontal_range(start, end)` minimally pans a horizontal
  viewport to reveal a half-open content-column range. The request reapplies
  when the range or viewport width changes while leaving subsequent user
  scrolling authoritative when both remain stable.
- `Command::after` and `CommandLink::send_after` schedule delayed work on a
  shared timer thread instead of sleeping inside a pool worker, so a pending
  timer no longer occupies one of the 2-8 task workers. Timers fire through the
  executor, so a slow callback cannot delay later timers.
- `CommandLink<Msg>` now implements `Clone` unconditionally. The derived impl
  bounded on `Msg: Clone`, so `link.clone()` silently resolved to cloning the
  reference, which cannot escape into a task.
- `PanView::wheel_to_pan(bool)` toggles mouse-wheel panning, mirroring the
  existing `drag_to_pan`. Defaults to `true`.

### Changed

- `SearchPalette` Hybrid matching prefers visible labels over hidden aliases:
  any label hit outranks a synonym-only alias hit on another row, while
  aliases still surface rows the label alone would miss. Fuzzy applies the
  same label-over-alias preference. See `docs/widgets/overlays.md`.
- Compose perpendicular `Divider` intersections into directional box-drawing junctions.
- Promote alpha-aware `Paint::rgba` backgrounds in the styling guide with practical translucent
  surface and toast examples.
- Remove the unused public `Transition::is_exit` field (breaking).
- `ManagedTerminal` now coalesces PTY resize bursts over a configurable 16 ms
  trailing window, preventing transient width changes from repeatedly clearing
  terminal semantic marks.
- Display-column helpers and terminal selection rendering preserve joined
  grapheme clusters instead of measuring or slicing their Unicode scalars
  independently.
- `TerminalCopyMode` returns `Ignored` for motions already at a boundary and
  prompt jumps no longer wrap from one end of the prompt list to the other.
- Text and virtual-text rendering now drops control characters consistently;
  virtual text also strips terminal control sequences.
- Add theme-level `CaretPalette` defaults for `Input`, `TextArea`, and embedded
  `SearchPalette` query inputs; explicit widget caret settings still override the
  theme (breaking)
- Replace the breaking `Frame` border title/status API with positional `BorderLabels` header and
  footer groups. Labels now support left, center, and right placement, independent group styles,
  focused group styles, per-label overrides, and group padding. The old `title`, `status`, and
  related Frame style methods and fields were removed (breaking).

- `FileTree` no longer rebuilds the subtree of a collapsed directory on every
  render. A collapsed directory projects a single placeholder child, so
  per-frame work now scales with the rows a user can actually see instead of
  with everything the tree has loaded. Explorer search keeps the full walk.
  This matters most for `FileTreeEntrySource::Provided`, which hydrates every
  supplied listing rather than only the expanded ones.
- `ListNode.selected` and `TableNode.selected` are now `Option<usize>`
  (breaking). Defaults and builder call sites that pass bare integers are
  unchanged; only code that reads the public node fields as `usize` needs an
  update.
- `List` and `Table` keep reporting overflow scroll indicators when
  `show_scroll_indicators` is on and no row is selected. The no-selection
  scroll branch reported no indicators and a zero overflow count, so a
  read-only list lost its "N more below" affordance until the user scrolled.
- Theme reload files now accept TOML 1.1 syntax, including multiline inline
  tables and trailing commas. The dependency stacks behind optional image,
  diff, filesystem-watcher, and terminal-emulation features were also updated
  to their latest stable release lines, along with the procedural macro stack.
- `PanView` now responds to the mouse wheel. It supported drag and keyboard
  panning but was never routed to the wheel dispatcher at all, so spinning the
  wheel over a pan surface did nothing. The wheel pans vertically and
  shift+wheel horizontally, each tick moving one `key_step` in that axis so
  wheel and keyboard panning share one step size. A clamped view at its edge
  leaves the tick unhandled so it bubbles to an ancestor; an unclamped free
  canvas has no edge and keeps consuming. Opt out with `wheel_to_pan(false)`.

- `children(...)` doc comments on `ScrollView`, the stack containers, `Flow`,
  `Splitter`, `TreeNode`, and `GraphNode` now state that the call discards
  anything already added with `child(...)`. Semantics are unchanged: plural
  setters replace, singular setters append. A new CI guard
  (`scripts/check-children-replace.py`) fails the build when a `children(...)`
  call would silently drop earlier `child(...)` entries.
- A plain mouse wheel tick over a `ScrollView` with `ScrollAxis::Horizontal`
  now pans that view horizontally instead of being discarded. Shift+wheel is
  still the explicit horizontal override, and stays required when both axes are
  enabled; with vertical scrolling off there was nothing to disambiguate, so the
  modifier only made the wheel do nothing. When the view has no horizontal
  travel left the tick is reported unhandled and bubbles to an ancestor, so a
  horizontal strip nested inside a vertical `ScrollView` does not trap the
  wheel.

- Renamed `Theme::catppuccin` to `Theme::catppuccin_mocha` and `Theme::gruvbox`
  to `Theme::gruvbox_dark`, so every preset in a multi-variant family names its
  variant explicitly. Both bare names were ambiguous next to the variants added
  this release: Catppuccin has four equally-named flavors and no default, and
  `gruvbox` sat asymmetrically beside `gruvbox_light` and
  `solarized_dark`/`solarized_light`. Presets whose bare name is upstream's own
  variant name (`tokyo_night`, `rose_pine`) are unchanged. `preset_by_name`
  still resolves `"catppuccin"` and `"gruvbox"`, so theme TOML and config files
  keep working. (breaking)
- The nine original theme presets now share the same internal color-table
  construction as the new ones. Every preset's rendered colors are unchanged;
  this is an internal cleanup only.
- Redesigned the DevTools stats panel for readability under animation: a fixed
  13-row layout (no more lines flickering in and out per frame), a bold label
  gutter with an aligned value column, all values aggregated over the last 60
  recorded frames instead of the latest one, a frame-time chart with
  microsecond resolution, an explicit scale caption and square-root height
  compression so sub-millisecond frames stay visible next to spikes, a compact
  untruncated focus line, and a 48x15 panel.
- Restyled the DevTools Logs tab: Follow/Pause/Framework toggle chips that show
  their on/off state (filled accent dot when on, hollow dimmed dot when off)
  with hover and focus styles, a Clear action with a destructive hover color,
  colored log-level tags in the list, and a dimmed line counter.

### Fixed

- Apply list selection/hover style overrides and contrast finalization to spinner gutters and
  status spinners (text markers already did). Narrow spinner gutters also lead-pad inside the
  reserved column so a 1-cell frame lines up with text markers like `" ●"`. Add
  `ListItemGutter::leading(n)` for an explicit inset when the spinner is the widest gutter
  (otherwise a lone 1-cell spinner stays flush left).
- Gate `AnimatedNode::auto_exit_warned` and `supports_auto_exit` behind `#[cfg(debug_assertions)]` to
  suppress dead-code warnings in release builds where the only call site is compiled out.
- A `Toast` with a translucent background no longer renders a darker patch behind its own text. The
  frame's background paint was copied onto the message style, so text cells composited that alpha a
  second time on top of the surface the frame had already produced. An alpha paint is now left for
  the frame alone to paint, and the text keeps the surface underneath it.
- A translucent overlay background now blends against the content the overlay covers, per cell,
  instead of against one flat backdrop. Overlays draw onto a cleared region, so the alpha flattening
  that ran while the subtree rendered had nothing to blend with and fell back to the terminal
  background - a toast with an `rgba` surface came out a single opaque colour and discarded whatever
  variation was underneath, which is the entire point of making it translucent. The pre-clear
  snapshot the restore pass already keeps is now reused to redo the blend properly.
- `Style::tint_by` no longer does nothing on an ordinary widget style. It set only the compositor
  hook, which nothing outside a backdrop path (`EffectScope`, overlay backdrops) reads, so tinting a
  plain widget silently changed no colour at all. It now also transforms that style's own `fg`/`bg`,
  mirroring `dim_by`; the compositor already skipped a transform matching its own hook, so backdrop
  paths are unaffected and nothing double-applies.
- Document that `Style::contrast_policy` is resolved before anything is drawn, against this style's
  `bg`, else the containing style's, else the terminal's. An alpha background is flattened against
  that same assumed backdrop, so a translucent surface floating over unrelated content — a toast
  above live output — gets approved on a pairing that renders unreadable. `EffectScope::contrast_policy`
  judges the composited cells instead and is the fix for that case.
- A tab strip with a `tab_hover_style` no longer reports a repaint on every pointer-motion event that
  crosses it. Hover now resolves which tab the pointer is over and reports dirt only when that
  changes, so a strip spanning the top of a window stops repainting the whole tree ~60 times a second
  while the pointer moves along it.
- `DraggableTabBar`, `TextArea` sentinel placeholders, and diff context separators in `TextArea` and
  `DocumentView` now likewise report hover dirt only when the painted target changes. Draggable tabs
  distinguish the tab body, close affordance, and overflow controls, while text widgets track the
  sentinel byte and separator source row.
- A hover change is no longer priced as a full rebuild just because *some* view reads hover. The
  runtime records which hover questions each view pass asked — `has_hover_within`,
  `has_hover_within_key`, `hovered_node_id` — along with the scope that asked, and answers a pointer
  movement with the smallest refresh that fits: a repaint when no recorded answer changes, and a
  layout pass over just the scopes whose answers did. One keyed `has_hover_within_key` call used to
  promote every pointer crossing anywhere in the window to a full `view()` + layout pass, so an app
  with a hover-revealed affordance in a sidebar paid a rebuild for merely sweeping across an
  unrelated tab bar.
- The layout pass a hover crossing asks for now actually lands. Two halves were missing:
  `refresh_cached_scopes` re-ran the marked views without first pointing the hover chain at the new
  position, so a view re-run *because* hover moved was built against where the pointer used to be;
  and reconciling the refreshed tree then cleared the recorded hover questions, which priced the next
  crossing as a repaint and left that tree on screen. Together they made a hover-revealed affordance
  read as broken — a sidebar row whose hover background dropped the moment the pointer reached the
  close button nested inside it, then lifted twice on the way back out, until an unrelated full
  render happened to repair it.
- `TestBackend` now fires `MouseRegion::on_hover_change` on enter and leave, matching the live
  runner. Synthetic pointer movement updated the hovered node and its hover visuals but never told
  the component, so an app whose *state* follows hover — anything revealing an affordance on a
  hovered row — could only be tested by seeding that state by hand, which skips the very transition
  under test.
- A key forwarded to a focused `Terminal` no longer claims a frame of its own. Forwarding writes bytes
  for the child program and changes nothing on screen, so the frame was speculative and doubled the
  render cost of every keystroke; whatever the child draws in response arrives as output and asks for
  its own frame. A key that drops a live selection still repaints, since that *is* visible.
- `dirty_override: Some(DirtyLevel::None)` from a key handler no longer also reports the key as dirty,
  matching its documented meaning of "handled, and nothing needs redrawing".
- Toast fade transitions now dim foreground-only frame decorations when they are drawn directly
  over the terminal's default background instead of leaving those glyphs at full intensity.
- Scrolling a `ScrollView` no longer punches holes of the host terminal background through an
  `App::fill_background` surface. The incremental scroll fast path assumed the terminal's
  scroll-region command left the exposed rows filled with the configured screen background, but
  `CSI S`/`CSI T` carry no SGR and background-color erase fills them with the terminal default, so
  the follow-up diff judged every blank exposed cell already-correct and never repainted it. Most
  visible when dragging the scrollbar, which jumps many rows per frame.
- Make constrained `DocumentView` auto width use its intrinsic content and scrollbar chrome instead
  of expanding to the full available width and wrapping content under the scrollbar.
- Tooltips now open when their trigger is hovered and remain passive, so showing one no longer
  captures focus or blocks input to the rest of the application. The controlled `open` value now
  overrides automatic hover/focus behavior, allowing a focused trigger to close its tooltip.
  `Tooltip::show_on_focus(false)` supports hover-only triggers while focus display remains enabled
  by default for keyboard accessibility.
- Blend Toast enter and exit transitions against the rendered cells underneath the toast instead
  of the terminal's default background.

- Make `TestBackend::advance(dt)` available to web examples by sharing animation stepping with
  both native and WASM runtimes.
- Terminal selection copying now uses the same display-column convention as
  rendering, so CJK, emoji, and other wide cells copy the text that was
  highlighted rather than a range shifted by the width of each wide cell.
- Controlled terminal selections no longer disappear after mouse clicks or drags.
- Control characters no longer consume a zero-column measurement budget while
  painting visible escape payload bytes into neighboring layout.
- URL hints are now detected when other text precedes them on the same line.
  Scheme detection anchored on the first letter of the line, so any prose ahead
  of a URL consumed its `:` and dropped every match on that line.
- `assign_labels` no longer returns duplicate labels for a single-character key
  alphabet, and ignores repeated characters in `keys`. Both cases produced
  colliding labels that `filter_labels` could never resolve.
- Mouse release now clears pending command chords and repaints the indicator immediately, preventing a prior key prefix from leaking into subsequent input.
- `TerminalScreen::export_replay_bytes` now preserves effective Kitty keyboard flags, so attached
  terminal clients keep modified-key input such as `Shift+Enter` after reconnecting.
- DevTools borders no longer merge their rounded corners with app-layer borders beneath the panel.
- `Frame` border headers now preserve tab titles alongside grouped labels, and
  label padding glyphs use the resolved border style instead of the label style.
- Border-tab mouse hit-testing now accounts for grouped header prefixes and
  padding before resolving the clicked tab.
- `TestBackend::new_with_app` now honors custom `App::clipboard_provider` and
  `App::clipboard_reporter` values, matching the native runner and allowing deterministic
  clipboard-routing tests.
- Changing the root prop of a mounted local `FileTree` now rebuilds the tree and reloads Git status
  instead of continuing to display the previous root.
- `syntax-syntect` no longer pulls C Oniguruma into `wasm32` builds; browser
  builds use Syntect's pure-Rust `fancy-regex` backend.
- The `paint` example's toolbar now renders its Pencil, Eraser, and Clear
  buttons. They were added with `child(...)` and then discarded by a following
  `children([...])`, which replaces the child list rather than extending it.
- Corrected doc comments on `ContextMenu::items` and `ListItem::description_spans`
  that read "Add ..." while both methods replace the whole collection.
- `ScrollView::scroll_wheel(false)` now also suppresses horizontal wheel
  panning. Shift+wheel previously kept panning a view that had opted out of
  wheel scrolling entirely.
- `TestBackend` now forwards mouse events to a terminal in a mouse-tracking
  mode, matching the runner. It left `forward_terminal_mouse` at the trait
  default of `false`, so a full-screen TUI that consumes clicks before ordinary
  `MouseRegion` dispatch behaved differently under test than in the real app,
  and that path could not be regression-tested at all. The runner's logic now
  lives in one shared `terminal_mouse_forward_plan` so the two cannot drift
  again.

- `ProgressBar` in `ProgressStyle::Block` no longer paints a black trough on
  light themes. The empty half of the track dimmed the fill color toward black
  regardless of the theme, which is invisible on a dark background but draws a
  black bar across a light one. It now recedes toward the surface behind the
  bar, so `block_empty_bg_dim` means "how far toward the background" in both
  polarities. Themes whose background is not a concrete color (`Theme::ansi`)
  keep the previous dimming.

- Hover transitions no longer force a repaint when neither the node being left
  nor the one being entered paints differently while hovered. A `MouseRegion`
  that is hoverable only because it accepts clicks — with a masked `hit_test`,
  flapping hover on every gap the pointer crosses — repainted the whole tree on
  each motion event; now its `on_hover_change` callback's `Update` decides
  (breaking). Widgets can override the new `WidgetNode::hover_affects_paint`
  to declare hover-dependent visuals.

- Keyboard focus traversal (Tab/Shift-Tab) now repaints. Focus chrome drawn from
  the focused widget - focus styles, carets, `Input` prefix/suffix decorations -
  previously stayed on the old widget until some later key happened to dirty the
  tree, so focus appeared to lag one action behind.

- DevTools memo hit rate no longer counts plain (non-`memo_key`) component
  renders as misses; it previously pinned at 0% in apps that memoize few
  components. A retained component whose child refresh falls back to a full
  re-render also no longer double-counts as both hit and miss.

### Added

- 20 new built-in `Theme` presets, all resolvable through `preset_by_name`.
  Light: `solarized_light`, `gruvbox_light`, `tokyo_night_day`,
  `catppuccin_latte`, `rose_pine_dawn`, `ayu_light` — the first light presets
  the crate ships. Dark: `catppuccin_frappe`, `catppuccin_macchiato`,
  `rose_pine`, `rose_pine_moon`, `kanagawa`, `everforest`, `ayu_dark`,
  `ayu_mirage`, `nightfox`, `nordfox`, `night_owl`, `material_palenight`,
  `oxocarbon`, `zenburn`. `preset_by_name` also accepts `"catppuccin_mocha"`
  as an alias for the existing `catppuccin` preset.

- `SearchPalette::input_key(key)` keys the query input directly, so
  `ctx.request_focus(key)` targets it instead of relying on the palette
  container's first-focusable-descendant fallback. Uncontrolled mode only; a
  controlled palette renders no input of its own.

- DevTools frame metrics now attribute dirty updates to components and input
  sources (`input:key` / `input:mouse` / `input:drag` / `input:scroll`), coalesced
  across deferred-full skipped iterations into the next recorded frame and shown
  under the Dirty line in the stats panel.
- Collection widgets now expose paired `Arc<[T]>` bulk setters alongside the
  existing iterator setters: `Table::rows_arc`, `Tabs::tabs_arc`,
  `Chart::series_arc`, `ChartSeries::data_arc`, `Sparkline::data_arc`,
  `MultiSelect::items_arc`, and `SearchPalette::{items_arc, entries_arc}`.
  Prefer these when component state already holds a shared slice so frames avoid
  reallocating identical collections.
- Internal component registry now stores trimmed display names and full type
  names at mount for DevTools diagnostics and tracing identity.
- DevTools reports memo miss reasons (key/dirty/deps/in-view Memo taxonomy) and
  counts in-view `Memo` hits toward the panel hit rate. Reason collection is
  gated on the stats panel being visible with metrics enabled, so hidden-panel
  builds pay only the plain hit/miss counters.
- DevTools records exclusive per-component `view()` timings (top slow views) and
  emits `component.view` / `component.refresh` spans with component identity when
  `profiling-tracing` is enabled.
- DevTools shows a passive input-pressure line when recent Full frames are both
  input-sourced and over the 16ms budget (overlay only; no log warnings).
- Documented production performance patterns for update scope, widget-owned
  scrolling, subtree memoization, stable shared props, bounded rendering, and
  coalesced background work, distilled from opencode-tui.
- Added plain-text export over absolute terminal line ranges: `TerminalScreen::total_text_lines`,
  `text_lines`, `export_text`, `absolute_line_to_viewport`, and `absolute_line_to_offset`.
  Absolute indices count from the oldest retained history line and never mutate the display
  offset or run the render pipeline, so exporting does not disturb what the user is looking at.
- Added OSC 133 semantic marks anchored to those absolute lines: public `SemanticMark` and
  `SemanticMarkKind`, plus `TerminalScreen::semantic_marks`, `last_command_output_range`, and
  `export_last_command_output`. Marks are bounded, dropped once their line falls out of
  scrollback, and ignored while the alt screen is up.
- Added `KeyBinding::key_events`, expanding a parsed binding into one `KeyEvent` per chord step
  for send-keys style callers, with a dedicated `KeyEventExpansionError` for bindings that cannot
  be expressed as discrete events.

- Added `Theme::focus_decoration(bool)` and public `Theme::focus_decoration`, defaulting to `true`.
  Disabling it suppresses theme-sourced focus chrome, focused-content palette defaults, and focused
  scrollbar thumbs while preserving explicit widget focus styles and all selection styling.
  (breaking)
- Added widget `on_focus`/`on_blur` delivery and `App::on_focus_changed`, with public
  `FocusEntry`/`FocusChanged` payloads, keyed-remount deduplication, post-reconcile delivery, and
  focus diagnostics in the `devtools` panel. `Modal` and root `Popover` auto-focus by default;
  `.auto_focus(false)` retains their existing focus trap while suspending focus.
- `TestBackend` now drives the full generic `DragSource`/`DropTarget` pipeline: `send_mouse`
  with `Down`/`Drag`/`Up` activates drags past the movement threshold and emits `on_drag_start`,
  `on_drag_over`, `on_drag_leave`, `on_drop`, and `on_drag_cancel`, enabling headless integration
  tests of composed drag-and-drop UIs (previously these drags were silently discarded). The
  axis-neutral activation and target-compatibility logic is shared with the terminal runner.
- Added `FocusScope::{None, Exclude, Contain}` and `.focus_scope(...)` to `VStack`, `HStack`,
  and `Frame`. Excluded subtrees are skipped by traversal, fallback, descendant, and pointer
  focus while explicit keyed requests can enter them; contained subtrees cycle focus internally.
  A `Contain` pane is **opaque to the enclosing tab ring**: Tab from outside never enters it,
  because a ring that could Tab *in* but not back *out* traps focus. Focus enters a pane by
  click, `request_focus`, or an app-level pane-switch key. A focusable (`.focusable(true)`)
  `Contain` pane is itself a tab stop in the enclosing ring, so the pane stays keyboard-reachable
  even though its contents are opaque; the boundary node is never part of its own pane's ring.
  As a safety valve, when every tab stop in the tree lives inside a pane, Tab from an unfocused
  app descends into panes so traversal is never dead; the same valve applies inside capturing
  overlays, whose ring descends through panes when it would otherwise be empty.

- New `sidebar_tabs` example: rich vertical sidebar tabs composed from primitives — status
  icon or live spinner, label plus description line per item, click/keyboard selection, and
  drag-to-reorder with a flicker-free insertion indicator built on per-item `DropTarget`s.
  The per-item top/bottom-half drop mapping is documented in `docs/widgets/input.md`.

- `Flow::justify(Justify)` distributes each wrapped row's leftover width along the main axis.
  All `Justify` variants are supported and applied per row (`SpaceBetween` pins every row's first
  item to the left edge and last item to the right edge). Because Flow items are always measured
  at their natural size, the space variants need no explicit child sizing, unlike stacks.

- `RowStylePolicy` controls how row-level selection/hover/active styling interacts with a rich-text
  span: `Full` (row styling overrides the span, the default), `PreserveForeground` (row background
  and modifiers apply but the span keeps its explicit foreground — useful for search matches that
  must stay distinguishable inside a selected row), and `Disabled` (row styling never touches the
  span). The new `Span::row_style_policy` field and setter replace the `allow_row_style` bool
  field and setter: `allow_row_style(true)` becomes `RowStylePolicy::Full` and
  `allow_row_style(false)` becomes `RowStylePolicy::Disabled`. (breaking)
- `ToastHandle::dismiss_immediately(id)` removes a toast synchronously without an exit transition,
  allowing state notifications to be replaced without briefly stacking the fading old toast beside
  its replacement. See `docs/widgets/overlays.md`.
- `Update::layout_with_command(command)` combines a component-scoped layout
  refresh with background work, avoiding a root-level full update for controlled
  editors and other high-frequency widgets that launch async tasks.
- `TerminalScreen::semantic_state()`, `drain_semantic_events()`, and
  `restore_semantic_state()` expose working-directory and command-lifecycle
  metadata parsed from `OSC 7` (`file://host/path`), `OSC 9;9` (Windows-style
  CWD reports), and `OSC 133 A/B/C/D` (prompt/input/execution/completion
  boundaries), plus a minimal `hyprmux_exe=` key/value extension and Fish/Kitty's
  `cmdline_url=` for foreground-executable identity. Parsing runs through a
  second, independent `vte::Perform` observer fed the same raw bytes as the
  primary Alacritty grid parser, so it cannot affect rendering. New types:
  `TerminalSemanticState`, `TerminalSemanticEvent`, `TerminalWorkingDirectory`,
  `TerminalWorkingDirectorySource`, `TerminalCommandPhase`. This state is
  deliberately kept out of `TerminalRenderSnapshot` - it is runtime metadata,
  not something the renderer paints. See `docs/widgets/terminal.md`.
- `TerminalPty::foreground_process_group_id()` (Unix-only) reports the PTY's
  foreground process-group id (`tcgetpgrp(3)`) without exposing the underlying
  master file descriptor, for host apps that need a native foreground-process
  fallback when no shell integration is available.
- `TerminalScreen::bell_count()` exposes a monotonic count of BEL events parsed
  from child output, allowing hosts to trigger visual or audible notifications.
- `SearchPalette::match_mode(SearchMatchMode)` adds a `Hybrid` matching
  strategy alongside the existing (and still default) `Fuzzy` mode.
  `Hybrid` evaluates exact, prefix, word-prefix, substring, and fuzzy
  matching together and ranks results by that priority order first, so a
  real substring or prefix match always outranks a fuzzy one. Fuzzy
  candidates are additionally quality-gated on match density, span, start
  position, and whether the matched characters stay mostly within one word,
  rejecting weak scattered matches (e.g. `layo` against "Enable pane
  synchronization") while keeping useful abbreviations (e.g. `prd` against
  "production"). Fields (label/aliases, description, and the right-hand
  hint) allow separate whitespace-delimited terms to match different fields,
  while characters within one term never combine across fields. All terms
  must match. Contiguous queries may omit separators within one field, so
  `switchmodel` matches `Switch model`. Labels/aliases are weighted highest,
  descriptions lower, and the right-hand hint is restricted to
  exact/substring matching. See `docs/widgets/overlays.md` and `docs/enums.md`.
- `rank_search_palette_indices_with_mode(items, query, match_mode, score_fn)`
  ranks items with the standalone helper under an explicit `SearchMatchMode`
  (e.g. `Hybrid`), for callers that own the query/selection but want the same
  ordering as a `SearchPalette` configured with that mode.
  `rank_search_palette_indices_with_score` remains and now delegates to it with
  `SearchMatchMode::Fuzzy`. See `docs/widgets/overlays.md`.
- `Modal::focus_style(Style)`, `extend_focus_style(Style)`, and
  `inherit_focus_style()` configure the dialog frame while the modal or one of
  its descendants holds focus, allowing focused root-portal dialogs to retain
  intentional frame accents or compose with the theme focus style. See
  `docs/widgets/overlays.md`.
- `Tabs::caps(Option<(char, char)>)` draws `(left, right)` end-cap glyphs around
  the active and hovered tabs. Each cap replaces one of the tab's two padding
  cells, so the tab keeps its measured width and hit region, and is painted in
  the tab's own background over the strip background so the tab reads as a
  rounded or pointed pill (pass powerline separators for that look). A tab falls
  back to flat padding when it is truncated by the overflow policy, when its
  background matches the strip's, or when either cap is not single-width.
  Defaults to `None` (flat padding). See `docs/widgets/tabs.md`.
- `TerminalKeyModes` describes the input-affecting modes a child program has
  enabled: `app_cursor` (DECCKM), `bracketed_paste` (mode 2004), and
  `kitty_keyboard` (a `KittyKeyboardFlags` capturing the Kitty keyboard protocol
  flags pushed with `CSI > <flags> u`). It rides on `TerminalRenderSnapshot`, is
  applied automatically by `Terminal::snapshot`, and is exposed by
  `TerminalScreen::key_modes()` and `Terminal::key_modes()` for hosts that wire a
  `TerminalPty` by hand. This is the keyboard counterpart to the existing
  `MouseModeState`. See `docs/widgets/terminal.md`.
- `TerminalRenderSnapshot` now carries `cursor_shape` (`CaretShape`) and
  `cursor_blinking` (`bool`) captured from the child program's `DECSCUSR`
  (`CSI Ps SP q`) sequences, plus matching `Terminal::cursor_shape()` /
  `Terminal::cursor_blinking()` builders. The `Terminal` widget now renders the
  child's requested cursor shape and honors its steady/blinking preference
  instead of forcing a blinking block. See `docs/widgets/terminal.md`.
- `Context::command_chord_pending` method to query whether an app command chord is currently pending completion (e.g., after a leader prefix key has been matched).
- Reference documentation for `BorderMergeMode` and `SplitterHandleMode` enums in `docs/enums.md` and `docs/styling.md`.
- `Modal::max_height(Length)` caps a modal's height, and
  `Modal::reserve_height(Length)` keeps a `RootPortal` modal's top edge fixed as
  its content grows and shrinks: the overlay is centered as if it were
  `reserve_height` tall, then the content is top-aligned within that reserved
  band, pinning the top edge at `(viewport - reserve_height) / 2`. Together with
  `height(Length::Auto)` this lets a content-hugging modal — e.g. a
  `SearchPalette` filtered as the user types — shrink to its visible rows
  without drifting toward the vertical center.

  `reserve_height` positions and `max_height` bounds, independently: content
  taller than the band keeps the same top edge and extends past the band's
  bottom, so a modal can be anchored a quarter of the way down the viewport
  (`reserve_height(Percent(50))`) while being free to grow to 75% of it. See
  `docs/widgets/overlays.md`.

- Layered keyboard dispatch: `FrameworkAction`, `FrameworkKeymap`, `UserKeymapPolicy`,
  `KeyDispatchPolicy`, `TerminalKeyPolicy`, `CommandConflictPolicy`, and
  `ChordMismatchPolicy` for explicit app-side input routing control.
- `App::framework_keymap`, `App::global_quit`, `App::user_keymap_policy`,
  `App::key_dispatch_policy`, `App::terminal_key_policy`,
  `App::command_conflict_policy`, and `App::chord_mismatch_policy` builders.
- Executable app command shortcuts via `CommandBuilder::shortcut` /
  `CommandBuilder::shortcuts` with deterministic conflict resolution and chord
  runtime support.
- `SplitterHandleMode` (`Splitter::handle_mode`): `Gutter` (default) keeps the
  classic reserved handle gutter; `Border` drops the gutter and rides the pane
  border seam, deriving handle thickness from the borders actually present
  (merged borders share a 1-cell wall, separate borders are grabbed together
  as a 2-cell handle, borderless panes get a synthetic 1-cell handle).
- Corner drag for splitters: when a vertical and a horizontal handle meet,
  clicking on or next to the junction grabs both handles and dragging resizes
  both splitters simultaneously; release emits `on_resize` for both.
- Public `text_motion` module (also re-exported through the prelude) exposing
  the byte-offset vim word/WORD/line motion algorithms
  (`word_forward_start`/`word_backward_start`/`word_end`,
  `big_word_forward_start`/`big_word_backward_start`/`big_word_end`,
  `line_start_at`/`line_end_at`/`first_nonblank_in_line`) that back
  `TextArea`'s vim mode, so host apps that render their own text grids (for
  example a terminal emulator's scrollback copy mode) can reuse the same
  `w`/`b`/`e`/`W`/`B`/`E`/`0`/`^`/`$` motions instead of reimplementing them.
  See `docs/text-editing.md`.
- `InlineHeight` height policy for inline viewports: `InlineHeight::Fixed(rows)`
  keeps the classic fixed height, `InlineHeight::auto()` sizes the viewport to
  the content's measured height every frame (growing and shrinking as the view
  changes), and `InlineHeight::auto_capped(rows)` adds an upper bound. The
  inline builders (`App::inline_ephemeral`, `App::inline_transcript`,
  `App::inline_transcript_with_startup`) now take `impl Into<InlineHeight>`,
  so existing calls with a plain row count keep compiling. When auto-sized
  content is taller than the terminal (or the cap), the layout keeps its
  natural height and the viewport shows its top rows, clipping the bottom.
  See `docs/inline-mode.md` and `examples/inline_auto_height.rs`.
- Unix-only `TerminalPty::handoff()` and `TerminalPtyHandoff` for advanced
  terminal hosts that need to transfer a live PTY master to another process
  without restarting the child. See `docs/widgets/terminal.md`.
- `TerminalScreen::export_replay_bytes()` serializes the current screen state
  (scrollback, primary/alternate contents, cursor, title, and common modes) as
  a VT byte stream that a fresh same-sized `TerminalScreen` reproduces by
  replaying it through the normal parser. Useful for seeding a newly attached
  client from a server-owned terminal. See `docs/widgets/terminal.md`.

### Changed

- `Sparkline.data` is now `Arc<[u64]>` instead of `Vec<u64>` (breaking). Call
  sites that assigned a `Vec` directly should use `Sparkline::new` / `.data(...)`
  or `.data_arc(...)`.
- `MultiSelect` and `SearchPalette` now store item/entry collections as
  `Arc<[T]>` instead of `Vec<T>` (breaking for any code that depended on the
  previous private storage shape via struct updates or reflection). Builder
  iterator setters still accept `IntoIterator` and collect into `Arc`.

- Added app-level `FocusPolicy::{Auto, OnDemand, Manual}` and `App::focus_policy(...)`;
  `OnDemand` is now the default, so apps start unfocused until Tab, pointer interaction, or an
  explicit focus request establishes focus. `Manual` disables framework Tab and pointer focus
  movement while preserving explicit focus APIs and capturing-overlay focus traps. (breaking)
- Added `Context::blur()`, `Context::focus_next()`, `Context::focus_prev()`, and
  `TestBackend::blur()` for explicit focus control.
- Added `tab_stop`, `on_focus`, and `on_blur` to focusable widgets. Renamed
  `Input::tab_order` to `Input::tab_stop` and TextArea's literal-tab width setter from
  `tab_stop` to `tab_display_width`. (breaking)
- Accordion, DraggableTabBar, Hyperlink, PanView, and Tabs are no longer focusable by default;
  opt in with `.focusable(true)`. (breaking)
- Renamed stack containers' `FocusPolicy` accordion-sizing enum to `FocusSizing` and
  `.focus_policy(...)` builder to `.focus_sizing(...)`. Tree's distinct
  `.focus_policy(FocusAccordion)` API is unchanged. (breaking)

### Fixed

- Terminal semantic marks no longer drift onto unrelated lines once scrollback fills up.
  Eviction cannot be recovered from the grid after the fact: at the scrollback limit
  `history_size()` and `topmost_line()` are pinned while content keeps shifting, so a remap
  derived by comparing grid state always computed a zero delta. `export_last_command_output`
  could then silently return a later command's output instead of the marked one. Evictions are
  now counted as they happen, while the VTE parser is driving the terminal, and marks whose line
  is gone are dropped rather than left pointing at recycled lines.
- OSC 133 sequences emitted by alt-screen programs no longer produce bogus main-screen marks.
  Recording was skipped while the alt screen was up, but the pending events were left queued and
  replayed against main-screen coordinates once the alt screen was torn down.
- Tab no longer resets to the first widget when the focused widget is not in the tab ring.
  Focus is granted on focusability while the ring is built from tab stops, so a widget reached
  by click or `request_focus` (`.tab_stop(false)`, or an `Exclude`/`Contain` escape hatch) was
  routinely absent from the ring; traversal now steps from where it would sit.
- `FocusPolicy::Auto` startup focus now agrees with the first Tab target. The fallback walked
  children while the ring sorts by node id, so the two diverged whenever children were ordered
  differently from allocation.
- Dismissing a capturing overlay no longer restores a *different* overlay's saved focus. Saved
  entries are keyed by overlay identity and only the matching entry is consumed, so a skipped
  save (focus already inside the overlay, or nothing focused under it) can no longer desynchronise
  the focus stack. Declarative overlays whose node identity changes across a remount are handled
  too: a save whose overlay no longer exists is rebound to the live overlay on the next frame,
  and consumed as the fallback on dismissal - entries belonging to other still-open overlays are
  never stolen.
- `on_blur` is no longer delivered to an unrelated widget when the blurred node's arena slot is
  recycled during reconcile. The callback is captured when the transition is recorded rather than
  re-resolved from a stale node id.
- **(breaking)** Raised the declared MSRV from Rust 1.85 to 1.88 (matches the
  locked Ratatui requirement), as part of laying groundwork for native
  macOS/Windows support.
- **(breaking)** `key_event_to_bytes` takes a second argument,
  `modes: TerminalKeyModes`, carrying the DEC private modes the child has
  enabled. Pass `TerminalKeyModes::default()` to keep the previous encoding, or
  `TerminalScreen::key_modes()` to honor the child's requests. `TerminalPty::send_key`
  gains the same argument.
- **(breaking)** Renamed `wrap_bracketed_paste(text)` to
  `encode_paste(text, modes)`. The old name always wrapped, which is wrong for a
  child that has not enabled bracketed paste; the new one wraps only when
  `modes.bracketed_paste` is set. `paste_sequences()` is unchanged.
- **(breaking)** `TerminalRenderSnapshot` gains a `key_modes: TerminalKeyModes`
  field, and `TerminalRenderSnapshot::from_parts` takes it as a final argument.
  Callers constructing snapshots from an external transport must carry the
  child's input modes across the wire, or pass `TerminalKeyModes::default()`.
- **(breaking)** `TerminalRenderSnapshot::from_parts` takes two additional
  arguments (`cursor_shape: CaretShape`, `cursor_blinking: bool`) after
  `cursor_visible`. Callers constructing snapshots from an external transport
  must supply the child's cursor shape and blink state.
- **(breaking)** Renamed `CommandBuilder::keybinding(...)` to
  `CommandBuilder::keybinding_hint(...)` for display-only palette hints;
  executable bindings use `shortcut(...)` / `shortcuts(...)`.

- **(breaking)** The `height` field of `SurfaceMode::InlineEphemeral` and
  `SurfaceMode::InlineTranscript` is now `InlineHeight` instead of `u16`.
  Code constructing these variants directly must wrap the row count
  (`height: InlineHeight::Fixed(8)` or `height: 8.into()`); the `App` builder
  methods are unaffected thanks to `From<u16> for InlineHeight`.
### Removed

- (breaking) Removed `Splitter::join_frame(bool)` method (use `Splitter::handle_mode(SplitterHandleMode::Border)` instead).
- (breaking) Removed unused `TextAreaDecorationKind::VirtualText` enum variant.

### Fixed

- Fixed initially-open root popovers resolving placement before the root node has a valid rect.
- `TextArea` word wrapping reuses the previous word break when a separator overflows, including
  while that separator is still trailing, so typing the next character cannot jump ahead of it.
  When a trailing separator itself fills the row, it stays there and the caret moves to the
  continuation row until the next input arrives.
- `TextArea` wrap boundaries now keep downstream caret affinity, including after visible path and
  identifier punctuation, while Up/Down navigation no longer skips continuation rows. Wrapped
  editors use the full content width for non-caret rows and reserve the final cell only on an
  exactly full row ending at the caret, instead of rendering the caret on a synthetic empty row.
- Fixed wrapped `TextArea` rows moving back when the cursor leaves a caret-adjusted row after an
  edit; cursor-only movement now preserves the continuation row and word break, and auto height
  reserves the same row so the preserved wrap is not clipped.
- `Flow` no longer subtracts its padding and border twice while measuring constrained widths,
  preventing rows that fit from reserving an extra wrapped line.
- Centered and stacked overlays (`Modal`, toasts) now measure their auto height against the width
  they are clamped to when a fixed- or percent-width overlay is wider than the viewport, so
  width-dependent content (a wrapping `Flow` footer, wrapped `Text`) grows the overlay to fit
  instead of being sized for its unwrapped width and clipped.
- `Terminal` now discards stale mouse-scroll state when a new snapshot changes the scrollback
  offset, keeping the rendered viewport, scrollbar thumb, and subsequent wheel input synchronized.
- `SearchPalette` query matches with an explicit foreground now remain visually distinct inside
  selected and hovered rows instead of being flattened to the row foreground.
- `SearchPalette` hybrid matching now averages per-term scores for multi-word queries instead of
  summing them, so an exact phrase match can no longer be outranked by several weaker distributed
  term matches solely because the query contains spaces.
- Toast exit transitions now fade from the toast's current opacity and use per-toast timing, so
  settled, clicked, and timed-out toasts no longer disappear in a single frame.
- Opted-in Unix fullscreen apps (`App::system_theme()` or
  `App::live_host_terminal_colors(true)`) now subscribe to compatible terminals'
  DEC private mode 2031 palette-change notifications and immediately refresh
  foreground/background colors through one Termina input worker. Runtime
  refreshes preserve the startup probe's resolved RGB ANSI palette rather than
  degrading app-owned syntax and derived colors to unresolved ANSI indices,
  preserve queued key, mouse, focus, resize, and paste input, and suspend/restore
  the notification mode around external terminal handoff. Complete repaints now
  invalidate Ratatui's previous frame in memory instead of flushing a standalone
  terminal clear, preventing a visible blank frame after focus or handoff.
  Handoff resume now replaces the Termina reader wake handle, reports legacy
  crossterm reader failures with their original error, and never leaves a failed
  Termina resume parked as a live but permanently paused input worker. Terminal
  response cleanup uses a DA ordering sentinel instead of a timing sleep.
  Inline, non-live, non-Unix, and unsupported terminals keep the existing
  startup, focus-gained, and manual refresh behavior.
- `Context::text_area_metrics()` and `text_area_scrollbars()` dependencies are
  now tracked by component scope, widget key, and metric kind. TextArea edits no
  longer invalidate unrelated memoized views or promote unrelated layout-only
  updates to full renders.
- `TerminalPty` now satisfies portable-pty 0.9's initial Windows ConPTY cursor-position handshake
  before child creation, preventing `PSEUDOCONSOLE_INHERIT_CURSOR` from stalling later requests.
- `TerminalPty::clone()` no longer kills the shared child process when just one
  of several outstanding clones is dropped. Previously every `TerminalPty` drop
  unconditionally killed the PTY, so dropping any handle (not just the last
  one) could terminate a still-referenced child out from under other holders.
- The generic `TerminalPtyConfig::default()` shell fallback now resolves
  `%COMSPEC%` (falling back to `cmd.exe`) on Windows instead of always trying
  `$SHELL`/`/bin/sh`, which does not exist there.
- The keyboard-enhancement probe now consumes its terminating DA1 reply instead
  of relying on a later input flush, closing a race that could still leak
  `^[[?…c` into the shell prompt on exit.
- Terminal teardown now discards delayed capability-probe responses before raw
  mode is disabled, preventing stray DA1 sequences such as `^[[?…c` from being
  echoed into the shell prompt on slower terminals and multiplexers.
- `key_event_to_bytes` now encodes `Ctrl+Backspace` as `ESC DEL` (readline's
  `backward-kill-word`, identical to `Alt+Backspace`) instead of collapsing it to
  a bare `Backspace`, so it deletes the previous word out of the box in shells and
  line editors rather than a single character. `Backspace` on its own still sends
  `DEL`. See `docs/widgets/terminal.md`.
- `key_event_to_bytes` now honors the Kitty keyboard protocol when the child has
  negotiated it (`CSI > <flags> u`), encoding chords that have no legacy terminal
  sequence — `Ctrl+1`…`Ctrl+9`, `Ctrl+Enter`, `Shift+Enter`, `Ctrl+Tab`,
  `Ctrl+Backspace`, and a disambiguated `Esc` — as `CSI <codepoint>;<mod> u`.
  Because tui-lipan's own backend pushes `DISAMBIGUATE_ESCAPE_CODES` on startup,
  a tui-lipan app running inside a `Terminal` widget gets these for free; before,
  `Ctrl+1` (a common tab-switch binding) reached the child as nothing at all.
  Children that have not negotiated the protocol keep the legacy bytes, since a
  crossterm reader discards an unsolicited `CSI u` sequence. `TerminalScreen`
  now enables alacritty's `kitty_keyboard` config so these pushes are tracked.
- `Ctrl` chords on punctuation that has a C0 control code are no longer dropped:
  `Ctrl+/` and `Ctrl+_` send `0x1f` (readline's `undo`), `Ctrl+?` sends `0x7f`,
  `Ctrl+@` sends `0x00`, and xterm's digit aliases `Ctrl+2`…`Ctrl+8` send their
  control codes. Previously `key_event_to_bytes` returned `None` for these and
  the key never reached a legacy child. (Under the Kitty protocol these carry
  their real codepoint instead.) Chords with no control code and no protocol
  (`Ctrl+1` in a plain shell) still return `None` and stay available to the app.
- `key_event_to_bytes` now returns `None` for `Super`-modified keys instead of
  sending the unmodified key, so `Super+C` no longer types a literal `c` into the
  child. The chord bubbles to the app.
- Function keys `F13`–`F20` now encode (`CSI 25~`…`CSI 34~`) instead of being
  dropped.
- Pasted text is only wrapped in the bracketed-paste sequences when the child has
  actually enabled the mode (`CSI ? 2004 h`). A child that never asked for
  bracketed paste does not strip the wrapper, so it previously received the
  literal bytes `ESC [ 200 ~` around every paste.
- Unmodified cursor keys now honor DECCKM (`CSI ? 1 h`): when the child has
  entered application-cursor mode they are introduced by `SS3` (`ESC O A`)
  instead of `CSI` (`ESC [ A`). ncurses emits `smkx` on startup and then matches
  arrows against terminfo's `kcuu1=\EOA`, so children in application mode were
  seeing a sequence they do not have a binding for. Modified cursor keys stay on
  the `CSI` parameterized form, as xterm does.
- `key_event_to_bytes` now encodes `Ctrl` and `Shift` on cursor, navigation, and
  function keys instead of dropping them, so `Ctrl+Left` reaches the child as
  `CSI 1;5D` rather than collapsing to a bare `Left` and losing word-wise motion
  in readline, editors, and other TUIs. Arrows and `Home`/`End` use `CSI 1;<mod>
  <letter>`; `Insert`, `Delete`, `PageUp`, `PageDown`, and the function keys use
  `CSI <num>;<mod>~`, with the xterm modifier parameter `1 + shift + 2·alt +
  4·ctrl`. Plain `Alt` keeps its historical ESC-prefix encoding, and `Shift`
  alone on `Insert`/`PageUp`/`PageDown` keeps the unmodified bytes because those
  are emulator-reserved bindings the `Terminal` widget forwards rather than
  consumes. See `docs/widgets/terminal.md`.
- `TerminalPty` no longer reports a spurious `TerminalPtyEvent::Error`
  ("Input/output error (os error 5)") when a child exits on Linux. A PTY master
  read returns `EIO` once the slave side is fully closed, which is the normal
  end-of-stream signal for a master rather than a fault; the reader now treats
  `EIO` like EOF and lets the wait thread deliver the real exit code. Hosts that
  surfaced this event as an error toast (e.g. on `exit`/`:q`) no longer see it.
- Focused `Terminal` panes no longer force every cursor into a blinking block.
  TUIs that set a steady or differently shaped cursor (for example Neovim's
  steady block in normal mode and steady bar in insert mode) now render as
  requested; a child that never issues `DECSCUSR` still defaults to a blinking
  block.
- `Context::command_chord_pending()` now schedules a repaint when its value
  changes, so apps can show or hide leader-prefix indicators immediately.
- Splitter corner-drag junction hit-testing (`find_junction_splitter`) no
  longer casts a handle rect's `w`/`h` (`u16`) to `i16` before computing
  bounds, which could wrap to a negative number and break hit-testing for
  very long splitter handles; the bounds math now runs entirely in `i32`,
  matching `Rect::contains`.
- Document `DocumentView` syntax highlighting support in the `syntax-syntect`
  feature tables in `README.md` and `docs/quick-start.md`.
- Clarify that `theme-reload` supports live TOML theme customization for app
  users as well as theme authors, not just development workflows
  (`README.md`, `docs/quick-start.md`, `docs/styling.md`).
- `SearchPalette` treats `initial_selected_item_index` as a selection seed and
  externally changed driver rather than a continuously controlled value.
  Unchanged seeds no longer override navigation when items refresh or matches
  rerank; synchronous and asynchronous results preserve the selected source
  item, including navigation that happens while an async search is queued.
  Changing the seed still reseeds selection, and synchronized callbacks no
  longer repeat solely because the same source item moved to another result row.

## [0.1.0] - 2026-07-05

Initial public release, after six months of private development.

Highlights of what ships in 0.1.0:

- **Component model**: typed `Message` / `Properties` / `State`, Elm-style
  `create_state → view → update` lifecycle, nested components with scoped
  routing, async side effects via `Command`.
- **Declarative UI**: builder API plus the `ui!`, `rsx!`, and `mockup!` macros.
- **Layout engine**: flexbox-inspired `Auto` / `Px` / `Flex` sizing, stacks,
  frames, grid, splitters, absolute-positioned `Canvas`, reconciliation with
  keyed identity.
- **Interaction**: mouse hit-testing, drag, hover/focus introspection, focus
  traversal, key bubbling, configurable keymaps and chords.
- **Overlays**: modals, popovers, toasts, tooltips, context menus, dismissal
  policies, focus capture.
- **65+ widgets**: forms, tables, trees, tabs, charts, diagrams, diff viewer,
  markdown document view, embedded PTY terminal, and more.
- **Theming**: presets, custom themes, host-derived `system` theme, contrast
  policies, live hot reload.
- **Animation & effects**: easing transitions, animated geometry, `EffectScope`
  cell shaders.
- **Agent-visible UI**: headless `TestBackend` + `UiSnapshot` with markdown /
  JSON / PNG exports.
- **Two backends**: native terminal (ratatui/crossterm) and browser/WASM.

See the [README](README.md) for the full feature set and
[docs.tui-lipan.dev](https://docs.tui-lipan.dev) for documentation.

[Unreleased]: https://github.com/tui-lipan/tui-lipan/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/tui-lipan/tui-lipan/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/tui-lipan/tui-lipan/releases/tag/v0.1.0
