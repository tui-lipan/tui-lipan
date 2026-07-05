# Event Handling Patterns (Current)

Use this reference for interactive widget wiring in current tui-lipan.

## Event Pipeline Overview

- Native event loop integration: `src/app/runner/events.rs`.
- Shared mouse dispatch for native runner and test backend: `src/app/mouse_dispatch.rs`.
- Hit actions: `mouse::gather_hit_actions` (`src/app/input/mouse/gather.rs`).
- Scroll wheel: `mouse::handle_scroll_wheel_n` (`src/app/input/mouse/scroll.rs`).
- Hover: `mouse::should_hover` and related logic (`src/app/input/mouse/hover.rs`).
- Drag helpers: `src/app/input/drag.rs`; `ActiveDrag` lives in
  `src/app/interaction_state.rs`; active drag updates live in `src/app/runner/drag.rs`.
- Keyboard: `src/app/input/keyboard.rs` is a **thin router**; per-widget behavior lives in `src/app/input/handlers/` (see `handlers/mod.rs`).

## Click Handling

1. Extend `gather_hit_actions` for hit → action mapping when needed.
2. Add or extend payload types in `src/app/input/mouse/types.rs`.
3. Handle the action in `src/app/runner/mouse_clicks.rs` and route shared native/test-backend behavior through `src/app/mouse_dispatch.rs` as appropriate.

Prefer this over scattering ad-hoc branches.

## Hover Handling

Default hover uses `node.is_hoverable()`. For partial-hit widgets (e.g. slider track only), specialize in `src/app/input/mouse/hover.rs`.

## Drag Support Pattern

1. Add drag helpers/types in `src/app/input/drag.rs` if needed.
2. Add `ActiveDrag::...` in `src/app/interaction_state.rs` when the runner or test backend must track drag state.
3. Start drag from the left-button path in `mouse_clicks.rs` / `events.rs`.
4. Update on drag move; clear on button-up.
5. Use `tree.is_valid(id)` guards before drag updates.

## Keyboard Pattern

Do **not** put large widget key logic in `keyboard.rs`.

1. Add `src/app/input/handlers/<widget>.rs` with a `handle_key(...)` (and scroll helpers if needed). Follow existing modules such as `handlers/checkbox.rs` or `handlers/input_widget.rs`.
2. Register the widget in `InteractiveTag` and `classify_interactive` in `handlers/mod.rs`.
3. Add a dispatch arm in `keyboard.rs` `dispatch_key` that calls your handler (same pattern as `handlers::checkbox::handle_key`).

For scroll wheel classification, mirror the same idea with `ScrollableTag` / `classify_scrollable` and the scroll dispatch in `src/app/input/mouse/scroll.rs`.

## WidgetNode Hooks That Affect Input

- `is_focusable` — focus eligibility.
- `has_on_click` — click target eligibility.
- `is_hoverable` — hover feedback eligibility.
- `hit_test_refinement` — precise hit regions.
- `scrollbar_zones` — scrollbar hit bands when the widget owns scrollbars.

## Selection Patterns

For text/grid-like widgets: track anchor/cursor, keep selection updates idempotent, gate clipboard on non-empty selection, reuse helpers from existing input/text/document/terminal flows where possible.

## Debounce / No-op Guards

For drag, wheel, repeated keys: compare old/new before emitting callbacks; avoid forcing dirty when nothing changed.

## Terminal Interop

The shared mouse dispatcher forwards to embedded terminals in some cases. Preserve existing terminal-forwarding checks; do not consume events that should reach a terminal widget.

## Common Pitfalls

- Callbacks on the node but no `gather_hit_actions` mapping.
- Drag started but not cleared on mouse-up.
- Missing hover specialization for partial-hit widgets.
- Spurious callbacks every drag tick when the value did not change.
- Forgetting focus updates for click-target nodes.

## Validation

After interactive widget changes: `cargo build`, `cargo clippy`, `cargo fmt`, `cargo test`.
