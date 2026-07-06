# Changelog

All notable changes to **tui-lipan** are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

While the crate is on `0.x.y`:
- A **minor** bump (`0.1` → `0.2`) signals breaking changes.
- A **patch** bump (`0.1.0` → `0.1.1`) is backward-compatible only.

## [Unreleased]

### Added

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

### Changed

- **(breaking)** The `height` field of `SurfaceMode::InlineEphemeral` and
  `SurfaceMode::InlineTranscript` is now `InlineHeight` instead of `u16`.
  Code constructing these variants directly must wrap the row count
  (`height: InlineHeight::Fixed(8)` or `height: 8.into()`); the `App` builder
  methods are unaffected thanks to `From<u16> for InlineHeight`.

### Deprecated

- `Splitter::join_frame(bool)`: use
  `Splitter::handle_mode(SplitterHandleMode::Border)` instead. Frame border
  merging (`Frame::join_frame`) is unchanged and remains current API.

### Fixed

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
