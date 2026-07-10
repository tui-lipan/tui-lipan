# Changelog

All notable changes to **tui-lipan** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While the crate is on `0.x.y`:
- A **minor** bump (`0.1` → `0.2`) signals breaking changes.
- A **patch** bump (`0.1.0` → `0.1.1`) is backward-compatible only.

## [Unreleased]

### Added

- `Modal::focus_style(Style)` sets the dialog frame style while the modal or one
  of its descendants holds focus, allowing focused root-portal dialogs to retain
  intentional frame accents instead of inheriting the theme focus style. See
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

[Unreleased]: https://github.com/tui-lipan/tui-lipan/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/tui-lipan/tui-lipan/releases/tag/v0.1.0
