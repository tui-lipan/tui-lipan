# Changelog

All notable changes to **tui-lipan** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While the crate is on `0.x.y`:
- A **minor** bump (`0.1` → `0.2`) signals breaking changes.
- A **patch** bump (`0.1.0` → `0.1.1`) is backward-compatible only.

## [Unreleased]

### Added

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
- `TextArea` word wrapping no longer moves a completed full-width word to the next row when a
  trailing space is typed; the separator now occupies the existing caret continuation row.
- `TextArea` wrap boundaries now keep downstream caret affinity, including after visible path and
  identifier punctuation, while Up/Down navigation no longer skips continuation rows. Wrapped
  editors also use the full content width instead of reserving a trailing caret column.
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
- `SearchPalette` now honors a change to the controlled
  `initial_selected_item_index` even when the `items`/`entries` set changes in
  the same render. Previously a simultaneous items change took an early refresh
  path that reset the selection only on query changes, so the palette's internal
  highlight stayed pinned to the old numeric row while the caller moved the
  controlled index elsewhere — leaving the palette highlight and the caller's
  selection on two different rows (visible, for example, in a session/command
  list that gains rows from a background fetch while open).

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
