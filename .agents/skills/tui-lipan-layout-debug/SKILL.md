---
name: tui-lipan-layout-debug
description: >
  Guide for debugging layout measurement, rect allocation, and caching issues in
  the tui-lipan TUI framework. Use this skill whenever investigating clipped
  widgets, stale heights after resize, split-wrap sync desync between DiffView
  panes, or any mismatch between measured size and reconciled rect. Also use when
  touching measurement caches, the dual-pass split-wrap protocol, or
  ScrollView/DocumentView auto-height geometry. Trigger on terms like "clipped",
  "wrong height", "stale size", "split-wrap", "DiffView pane mismatch",
  "measurement cache", "layout_hstack", "measure_stack", "reconcile height".
---

# tui-lipan Layout Debugging Guide

This skill encodes hard-won knowledge about how tui-lipan measures, caches, and
reconciles widget geometry. It exists because the measurement and reconcile
pipelines are independent code paths that must agree on sizes — and when they
don't, the symptom is subtle clipping or stale heights that only appear on
resize or under specific caching conditions.

## How layout works (the 30-second version)

Every frame, tui-lipan runs two phases:

1. **Measurement** — walks the element tree bottom-up, asking each widget "how
   big are you?" given parent constraints. Entry point: `min_size_constrained`
   in `src/layout/measure.rs`. Results feed into parent containers so they can
   divide space among children.

2. **Reconciliation** — walks the element tree top-down with concrete `Rect`s,
   telling each widget "you live here". Entry point per container:
   `reconcile_hstack` / `reconcile_vstack` in
   `src/widgets/containers/reconcile.rs`. Child rects are computed by layout
   helpers like `layout_hstack` in `src/layout/stack/mod.rs`.

The critical invariant: **measurement and reconcile must produce the same
heights.** If measurement says a widget is 8 lines tall but reconcile allocates
a 7-line rect, the bottom line gets clipped.

## First-pass app-author checks

Before changing framework layout code, rule out common usage traps:

- Set an explicit `TestBackend` viewport; it defaults to 80x24, while `Mockup`
  previews use the live terminal size.
- Capture `UiSnapshotOptions::diagnostic()` and inspect markdown for `zero-area`
  widgets, spacers, and dividers.
- Remember `VStack`, `HStack`, and `Frame` default to `Length::Flex(1)`; fixed
  headers, footers, sidebars, and status bars usually need `Length::Px(...)`.
- If a widget is merely clipped by design inside `ScrollView`/`Frame`, verify the
  scroll/clip container before suspecting measurement drift.

## The three measurement cache layers

Measurement is expensive, so tui-lipan caches aggressively. When debugging stale
sizes, you need to know which cache is lying:

### Layer 1: Element-local cache
- **Where**: `el.measure_cache` — `Cell<[Option<MeasureCacheEntry>; 2]>` (2-slot LRU)
- **Key**: `(max_w, max_h)` — the constraints the parent offered
- **Checked**: early in `min_size_constrained` (`src/layout/measure.rs`)
- **Bypass**: `element_skips_shared_measure_cache(el)` returns true

### Layer 2: Global shared cache
- **Where**: `GLOBAL_MEASURE_CACHE` thread-local `HashMap` in `measure.rs`
- **Key**: `(element_layout_hash, max_w, max_h)` — lets structurally identical
  elements (same props, same children) share one measurement
- **Bypass**: same `element_skips_shared_measure_cache` flag

### Layer 3: Widget-specific caches
- `DocumentView::measure_cache` — 2-slot, keyed on `document_measure_cache_key`
  which hashes all geometry-affecting props including `split_wrap_lp` (layout
  pass number), pane widths, and scrollbar columns
- `TextArea` visual cache entries
- `ScrollView` content/layout hashes (`scroll_content_hashes` in
  `scroll_view/reconcile.rs`)

### When caches get bypassed

`element_skips_shared_measure_cache` (in `measure.rs`) returns true when
`element_subtree_has_split_wrap_sync(el)` is true (defined in
`diff_view/wrap_sync.rs`). This is because split-wrap sync uses mutable shared
state (`layout_pass`) that changes between measurement passes — the same element
with the same props needs to produce different results at pass 1 vs pass 2.

**Debugging stale cache**: if you suspect a cache is returning stale data, make
`element_skips_shared_measure_cache` return true for the element type in
question and see if the bug goes away. If it does, the real fix is to include
the missing state in the cache key.

## Split-wrap sync (DiffView split mode)

This is the most complex part of the layout system. In split mode, two DiffView
panes sit side-by-side in an HStack. When wrap is enabled, a source line that
wraps to 3 visual lines on the right pane needs 2 padding lines on the left pane
so both panes stay aligned. This is the "split-wrap sync" protocol.

### The shared state

`SplitWrapSyncState` (in `diff_view/wrap_sync.rs`) is an `Rc<RefCell<...>>`
shared by both panes. It tracks:
- `left_pane_width` / `right_pane_width` — inner widths from layout
- `left_scrollbar_cols` / `right_scrollbar_cols` — scrollbar gutter widths
- `layout_pass` — 0 (simulation), 1 (record), or 2 (apply)
- `pass1_left_heights` / `pass1_right_heights` — actual visual line counts
  per source line, recorded during pass 1

### The three layout passes

- **Pass 0** (default/simulation): `compute_split_wrap_padding` wraps raw peer
  source text at a simulated content width. This is a rough estimate — it
  doesn't account for formatter modifications (prefixes, indent changes, code
  block decorations). Used as a fallback when the dual-pass hasn't run.

- **Pass 1** (record): Each pane runs its full measurement/reconcile pipeline
  normally, producing actual visual lines from the formatter. Then it calls
  `record_pass1_source_heights` to store the exact visual-line count per source
  line. No padding is inserted — the heights are raw.

- **Pass 2** (apply): Each pane calls `peer_pass1_source_heights` to get the
  other pane's exact pass-1 heights, then computes padding via
  `compute_split_wrap_padding_from_heights`. This is the accurate path.

### The three dual-pass sites

The dual-pass protocol must run consistently in **all three** of these places:

1. **`measure_stack`** (`src/widgets/containers/layout.rs`)
   During measurement of the HStack containing the two panes. After
   `compute_stack_layout` determines pane widths, it:
   - Updates pane width hints
   - Sets pass 1, measures all children (recording heights)
   - Sets pass 2, then the main measurement loop runs (computing padded heights)
   - Resets sync state

2. **`layout_hstack`** (`src/layout/stack/mod.rs`)
   During reconcile-time rect computation. After `compute_stack_layout`
   determines widths, it must ALSO:
   - Update pane width hints
   - Set pass 1, measure all children (recording heights)
   - Set pass 2, so `measured_cross_for_layout` computes correct padded heights
   - Reset sync state after building rects

   **This site was historically missing**, causing the most common clipping bug.
   Without it, `measured_cross_for_layout` runs at pass 0 (simulation), which
   can disagree with the exact dual-pass used during measurement. The symptom:
   measurement returns 8 lines (correct), but reconcile allocates 7 (from
   simulation), clipping one pane.

3. **`reconcile_hstack`** (`src/widgets/containers/reconcile.rs`)
   Reconciles children twice with the same rects: pass 1 first (so each
   DocumentView records its source heights in the node), then pass 2 (so each
   DocumentView inserts the correct padding into its visual cache).

### Why pass 0 can disagree with passes 1+2

Pass 0 wraps raw peer source text: `compute_split_wrap_padding(&own_heights,
&peer_lines, peer_sim_w)`. But the actual visual lines come from the formatter
(e.g. `DiffDocumentFormatter`), which may:
- Add diff prefixes (+, -, space) that change line widths
- Modify indentation for code blocks
- Add separator lines or decorations

So wrapping raw text produces different line counts than wrapping formatted text.
The fix is always to use the exact dual-pass (passes 1+2) rather than the
simulation (pass 0).

## Common bug patterns

### Pattern 1: Pane clipped by N lines after resize

**Symptom**: Content padding is correct (visible in both panes) but one pane's
bottom N lines are cut off. Both panes should have the same height.

**Root cause**: One of the three dual-pass sites is missing or inconsistent.
`layout_hstack` is the most likely culprit — it was the last site to get
dual-pass logic.

**Diagnosis**:
1. Note that HStack defaults to `Align::Center`, NOT `Align::Stretch`. This
   means each child gets its own measured height from `measured_cross_for_layout`,
   not the full `bounds.h`. Per-child height accuracy is critical.
2. Check `split_wrap_layout_pass` value when `measured_cross_for_layout` runs.
   If it's 0, the dual-pass isn't set up in `layout_hstack`.
3. Compare `measured_cross_for_layout` output vs `measure_stack` cross result
   for the same child at the same width.

**Fix**: Ensure `layout_hstack` runs passes 1+2 before the rect-building loop,
mirroring what `measure_stack` does.

### Pattern 2: Height doesn't update on resize (stale cache)

**Symptom**: Widget keeps its old height even after the window width changes and
content wraps differently.

**Root cause**: A measurement cache is returning stale data because the cache key
doesn't include all relevant state.

**Diagnosis**:
1. Check `element_skips_shared_measure_cache` — does it cover this element
   type? If the element participates in split-wrap sync, it must be skipped.
2. Check `document_measure_cache_key` — does it hash `split_wrap_lp`, pane
   widths, and scrollbar columns?
3. Check `scroll_content_hashes` — does it include `has_split_wrap_sync`?
4. Check `element_layout_hash` in `src/layout/hash.rs` — this feeds the global
   cache. If it's missing state, structurally similar elements will share
   incorrect measurements.

**Quick test**: Make `element_skips_shared_measure_cache` return true for the
element type and see if the bug resolves. If it does, find what's missing from
the cache key.

### Pattern 3: ScrollView auto-height lags behind by one frame

**Symptom**: After resize, the first frame shows the old height, then the next
frame corrects it. The "lag" is visible as a brief flash of clipping.

**Root cause**: ScrollView's content hash didn't change on the first frame, so
it reused the old layout. On the second frame, the element tree is rebuilt
(due to state change or re-render) and the hash changes.

**Diagnosis**: Check if `scroll_content_hashes` includes all state that changes
on resize — especially split-wrap sync state which can change even when the
element tree looks structurally identical.

## Key files quick reference

| File | What it does |
|------|-------------|
| `src/layout/measure.rs` | Top-level measurement dispatch + cache layers 1 & 2 |
| `src/layout/stack/mod.rs` | `layout_hstack` / `layout_vstack` (reconcile-time rects) |
| `src/layout/stack/compute.rs` | `compute_stack_layout` (flex distribution) |
| `src/layout/hash.rs` | `element_layout_hash` for global cache keys |
| `src/widgets/containers/layout.rs` | `measure_stack` (measurement-time sizing) |
| `src/widgets/containers/reconcile.rs` | `reconcile_hstack` / `reconcile_vstack` |
| `src/widgets/diff_view/wrap_sync.rs` | Split-wrap shared state + padding math |
| `src/widgets/diff_view/mod.rs` | DiffView element tree construction |
| `src/widgets/document_view/planner.rs` | Visual plan with split-wrap padding insertion |
| `src/widgets/document_view/layout.rs` | DocumentView measurement + widget cache |
| `src/widgets/scroll_view/reconcile.rs` | ScrollView caching + virtualization |

## Debugging checklist

When you encounter a layout/clipping bug:

1. **Identify the symptom** — clipping, stale height, pane mismatch, or lag?
2. **Find the widget** — which widget's rect is wrong? Use the node tree to
   compare `rect.h` vs actual visual line count.
3. **Check the pipeline stage** — is measurement wrong, or is measurement right
   but reconcile disagrees? Add temporary logging to both paths.
4. **Check cache freshness** — is a cache returning a stale value? Temporarily
   bypass caches for the affected element to isolate.
5. **For split-wrap bugs** — verify all three dual-pass sites are consistent.
   Check `layout_pass` value at the point of measurement. The pass-0 simulation
   is the most common source of disagreement.
6. **Write a regression test** — the test suite already has several split-wrap
   and resize tests. Add one that exercises the exact width transition that
   triggered the bug.
