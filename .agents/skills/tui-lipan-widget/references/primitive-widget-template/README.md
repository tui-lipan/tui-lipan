# Primitive Widget Template

Starter stubs for a new **primitive** widget. Align field names and patterns with a real widget in-tree (e.g. `button`, `checkbox`, `spacer`) before shipping.

## Files in This Folder

| File | Copy to |
|------|---------|
| `mod.rs`, `node.rs`, `layout.rs`, `reconcile.rs` | `src/widgets/<name>/` |
| `renderer.rs` | `src/backend/ratatui_backend/renderers/<name>.rs` (then register in `renderers/mod.rs` and `render.rs`) |

The renderer stub imports `crate::widgets::<name>::<Name>Node`; keep that import style when the file lives under `src/backend/ratatui_backend/renderers/`.

## Placeholders

Replace in all copied files:

| Placeholder | Example |
|-------------|---------|
| `#Name#` | `Toggle` |
| `#NAME_SNAKE#` | `toggle` |

## After Copying

Follow **Mandatory Wiring Checklist** in `SKILL.md` (`internal.rs`, `element.rs`, `node/kind.rs`, `layout/tag.rs`, `measure.rs`, `reconcile/element.rs`, `layout/hash.rs`, theme provider, docs).

Normal sizing goes through `ElementKind::dimensions()` in `element.rs`; only use `layout/axis.rs` for wrapper-style widgets.

## Checklist

- [ ] Properties and node fields match between `mod.rs` and `node.rs`
- [ ] `measure_<name>` respects `Length::Auto` / `Length::Px` and clamps safely
- [ ] Reconcile reuses node kind, returns fast when unchanged
- [ ] Renderer clips to `clip_rect`, patches styles correctly
- [ ] `LayoutHash` if the widget participates in layout caching
- [ ] Theme arm in `theme_provider.rs` if the widget is style-bearing
- [ ] Interactive? → handlers, gather, tags per `event-handling-patterns.md`

Then: `cargo build && cargo clippy && cargo fmt && cargo test`
