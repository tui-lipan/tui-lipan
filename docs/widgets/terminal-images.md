# Terminal image passthrough (design)

Status: **roadmap / design only** — not implemented.
Scope: Kitty graphics protocol first; sixel later.
Primary owner: `tui-lipan` (`TerminalScreen` + renderer). `hyprmux` follows with
pane clipping, multi-client attach, and memory budgets.

## Goals

- Allow programs that emit Kitty graphics (`APC _G …`) to show images inside a
  `Terminal` / `TerminalScreen` pane when the **host** terminal supports it.
- Preserve the cell-diff snapshot pipeline for text; images are a parallel layer
  that must not force full-frame text redraws on every scroll.
- Keep the same-bytes client model (server owns PTY bytes; each client parses
  locally), matching how semantic marks work today.

## Non-goals (v1)

- Sixel (follow-up once Kitty path is stable).
- Editing / screenshotting image contents from the app.
- Guaranteeing images survive resurrect / export_replay (may accept loss).
- Perfect multi-client fidelity when clients have different host capabilities.

## Proposed architecture

```text
PTY bytes
  │
  ├─► existing VTE / alacritty grid (cells)
  └─► Kitty APC `_G` parser ──► ImageStore on TerminalScreen
                                    │
                                    ▼
                            placements (id, z, x, y, w, h, pixel refs)
                                    │
Render snapshot (cells) ────────────┤
                                    ▼
Host renderer: capability detect → passthrough / remap ids → clip to pane
```

### Parsing & storage (`TerminalScreen`)

1. Extend the byte ingest path to recognize APC `_G` (Kitty) without disturbing
   the cell grid when the image is a floating placement.
2. `ImageStore`:
   - keyed by Kitty image id (and optionally placement id);
   - holds compressed/raw pixel payloads with a hard memory budget;
   - tracks which absolute lines / cells a placement occupies for eviction.
3. Cell placeholders: when a placement uses Unicode placeholders, mark those
   cells so scroll/clear/reflow can evict or move placements coherently.
4. Eviction triggers: scroll into history past retention, `ED`/`EL` clears,
   RIS/alt-screen swap, explicit Kitty delete actions, memory pressure.

### Renderer

1. **Capability detection** once per host backend (Kitty / iTerm2 / none).
2. **Passthrough**: re-emit Kitty APC sequences for still-visible placements,
   remapping ids per pane so overlapping panes do not collide.
3. **Damage / z-order**: image layer composites above/below cells according to
   Kitty z; must not invalidate the entire cell-diff cache when only an
   off-screen image changes.
4. **Fallback**: if the host cannot display graphics, leave placeholders (or
   blank cells) and skip APC emission — never corrupt the text stream.

### hyprmux concerns

- Clip image emission to each pane’s on-screen rect (and under overlays).
- Multi-client attach: each client’s host capability may differ; same-bytes
  parsing still runs, but passthrough is local.
- Attach replay: either extend `export_replay_bytes` to re-emit image payloads
  (expensive) or **accept loss** of pre-attach images (documented).
- Resurrect snapshots: image payloads can dominate size — default omit, with an
  explicit opt-in later if needed.
- Memory budgets: global + per-pane caps; drop oldest / largest first.

## Open questions (must answer before coding)

| Topic | Options / notes |
| --- | --- |
| Host support matrix | Kitty, Ghostty, WezTerm, iTerm2, Contour, Windows Terminal — which ship Kitty `_G`? Fallback UX? |
| Compositing vs cell-diff | Can we emit images only for damaged placements, or does Kitty require full re-place after scroll? |
| Scrollback semantics | Do images scroll with history lines, freeze at the live edge, or drop when leaving the viewport? |
| Resurrect size | Omit images by default vs size-capped inclusion. |
| Mixed-capability multi-client | Controller with graphics + follower without — acceptable divergence? |
| Payload limits | Max bytes / max dimension / max placements per pane. |
| Sixel | Separate parser + different host capability; do not block Kitty MVP. |

## Suggested first implementation slices

1. **Design spike (this doc)** — lock answers to the open questions above.
2. **tui-lipan**: APC `_G` parse → `ImageStore` → no renderer yet (tests on
   placement/eviction only).
3. **tui-lipan**: host capability + passthrough in the ratatui/crossterm backend
   behind a feature flag.
4. **hyprmux**: pane clip + id remapping + memory budget config.
5. **Docs / examples**: knobs, limitations, attach/resurrect behavior.
6. **Sixel** as a separate design + implementation track.

## Risks

- Highest: interaction with the cell-diff snapshot pipeline and scroll/reflow.
- Memory: unbounded image retention will OOM agent-heavy sessions.
- Attach/replay: without payload re-emission, images vanish on reattach — must
  be documented as accepted if chosen.
- Two-repo lockstep: framework feature lands and is published before hyprmux
  depends on it in CI.

## References

- Kitty graphics protocol: https://sw.kovidgoyal.net/kitty/graphics-protocol/
- Existing `TerminalScreen` replay / semantic mark patterns in
  `src/widgets/terminal/screen.rs` (same-bytes, client-local derived state).
