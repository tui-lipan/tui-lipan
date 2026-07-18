# Focus System Overhaul — Full Implementation Plan

## Context

Users building apps with tui-lipan repeatedly fight the built-in focus system: ~20 widget types are focusable by default, `restore_focus()` unconditionally auto-focuses the first focusable node (`src/app/input/focus.rs:83`) so "nothing focused" is impossible, the single `focusable` flag conflates tab-ring membership / key routing / click-to-focus / focus visuals, and taming it requires `.focusable(false)` scattered across every screen. This plan delivers a mature, decomposed focus system: an app-level policy, a legal unfocused state, `tab_stop` separation, subtree focus scoping, focus events, a theme-level visuals kill switch, and public blur/focus APIs.

**User decisions (final):**
- **Selective default flip** — keep focus-required widgets focusable by default; flip incidental ones to opt-in.
- **`FocusPolicy::OnDemand` is the new default** — nothing focused at startup; focus arrives via Tab/click/`request_focus`.
- **Full events + observability** — `on_focus`/`on_blur` callbacks, app-level hook, devtools display.
- Breaking changes acceptable (v0.1.0), no deprecated aliases.

**Design decisions:**

| Question | Decision |
|---|---|
| Name collision (`FocusPolicy` exists at `src/widgets/containers/mod.rs:39` = accordion sizing) | Rename container enum → **`FocusSizing`** (small blast radius: 6 src files, 5 docs, lazygit example). App-level enum takes **`FocusPolicy`**. |
| Tab-flag name | **`tab_stop`** (HTML/WinForms mental model). Input's existing `.tab_order(bool)` renamed; trait `in_tab_order()` → `is_tab_stop()`. |
| DocumentView default | Stays focusable — it's primary content (keyboard scroll/search/copy require focus), same class as List/Table. |
| Manual × modals | Capturing overlays **still auto-focus + trap under Manual**, with per-overlay `.auto_focus(false)` opt-out. A Modal without focus is broken software; the trap is a correctness property. With `auto_focus(false)`: trap keys but suspend focus (existing `suspend_focus_for_empty_overlay` precedent, `src/app/runner/overlay.rs:169`). |
| `ctx.focus_next/prev` under Manual | Allowed — Manual disables framework-*initiated* movement only; explicit calls are user initiative. |

## New Public API Surface

```rust
// src/app/context.rs, re-exported in prelude
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusPolicy {
    /// Auto-focus first focusable at startup and whenever focus is lost (old behavior).
    Auto,
    /// Nothing focused until Tab / click / request_focus. On loss: restore by key,
    /// then tag, else None.
    #[default]
    OnDemand,
    /// Framework never moves focus itself (no Tab traversal, no click-to-focus,
    /// no fallback). Only explicit APIs. Exception: capturing overlays w/ auto_focus.
    Manual,
}

// AppBuilder
pub fn focus_policy(mut self, policy: FocusPolicy) -> Self;
pub fn on_focus_changed(mut self, hook: impl Fn(&FocusChanged) + 'static) -> Self;

pub struct FocusChanged { pub old: Option<FocusEntry>, pub new: Option<FocusEntry> }
pub struct FocusEntry { pub key: Option<Key>, pub tag: Tag }

// Ctx (src/core/component.rs, next to request_focus at :1189)
pub fn blur(&mut self);          // NEW: clear focus (focused → None)
pub fn focus_next(&mut self);    // NEW: programmatic ring step
pub fn focus_prev(&mut self);    // NEW

// Per-widget builders (~19 focusable widgets)
pub fn tab_stop(mut self, v: bool) -> Self;   // default true
pub fn on_focus(mut self, cb: ...) -> Self;
pub fn on_blur(mut self, cb: ...) -> Self;

// Containers (VStack/HStack via impl_stack_props! + Frame)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum FocusScope {
    #[default] None,
    /// Subtree removed from tab ring, auto-fallback, and click-to-focus.
    /// Explicit request_focus(key) into the subtree still works (escape hatch).
    Exclude,
    /// While focus is inside, Tab/Shift-Tab cycle within this subtree only.
    Contain,
}
pub fn focus_scope(mut self, scope: FocusScope) -> Self;

// Renamed container sizing enum
pub enum FocusSizing { None, Accordion(FocusAccordion) }   // was FocusPolicy
pub fn focus_sizing(...) -> Self;                          // was .focus_policy()

// Overlays (Modal/Popover builders + OverlayRoot)
pub fn auto_focus(mut self, v: bool) -> Self;  // default true

// Theme (src/style/theme.rs)
pub focus_decoration: bool,                     // default true
pub fn focus_decoration(mut self, v: bool) -> Self;

// TestBackend
pub fn blur(&mut self);   // + focus_policy parity with App
```

Internal: `restore_focus`/`focus_next`/`focus_prev` (`src/app/input/focus.rs`) gain a `policy: FocusPolicy` param (they're called with separately-borrowed `&mut` fields, so a `Copy` param is least invasive). `FocusState` (`src/app/interaction_state.rs:305`) gains `policy` and `last_notified: Option<(NodeId, Option<Key>)>`. `focus_request` channel (`src/runtime.rs:164`) becomes `enum FocusRequest { Key(Key), Clear, Next, Prev }`.

## Policy Semantics (precise)

**`restore_focus` (focus.rs:21) per policy** — steps 1–2 (keep valid focused; restore by key incl. overlay subtrees) run under ALL policies (key restore = continuity, not framework movement). Then:
- **Auto**: step 3 tag-restore + step 4 first-focusable fallback (unchanged).
- **OnDemand**: tag-restore; step 4 → `focused = None`, **keep `focused_key`** (remount restores focus), clear tag.
- **Manual**: skip tag-restore (heuristic that can land on a different same-type widget = framework movement); step 4 → None with key retained (needed so `request_focus` before mount resolves on mount).

**OnDemand lifecycle** (all verified against existing code):
- Initial mount (`src/app/runner/mod.rs:1007`): all-None → no-op → app starts unfocused. Safe: `keyboard::dispatch_key` (`keyboard.rs:390`) already returns false on None; keys fall through to commands/framework/ambient scroll.
- `request_focus` in `init()`: `apply_pending_focus_request` runs before both restore sites (`render_service/mod.rs:71`, drain + pre-restore) → resolves same frame under all policies.
- First Tab: `focus_next` with None already picks `focusables[0]` (focus.rs:111).
- Overlay over None focus: `push_focus_stack_for_overlay` pushes None; dismiss pops None → restore lands on None (falls out naturally once policy is threaded).

**Manual gating sites** (must change in lockstep):
1. Tab traversal — 3 duplicated dispatch paths: `src/app/runner/key_dispatch.rs:550,576`, `src/app/input/runtime_dispatch.rs:317,339`, `src/app/runner/messages.rs:78,95`, plus `src/test_backend.rs:599,613`. Under Manual: skip global `focus_next/prev`, report unhandled so Tab falls to widget/command layers. Overlay trap cycling (`focus_overlay_next/prev`) stays active when a capturing `auto_focus` overlay is up.
2. Click-to-focus — single site `src/app/mouse_dispatch/mod.rs:397` + PanView fallback :400–406: skip under Manual (click handlers still fire; `is_interactive()` hit-testing untouched).
3. Auto-fallback — inside `restore_focus`.
4. `ensure_overlay_focus` (`overlay.rs:114`) — NOT gated by Manual; gated per-overlay by new `OverlayRoot.auto_focus`.

## Implementation Phases (each compiles green; one commit per phase)

### Phase 1 — Rename `FocusPolicy` → `FocusSizing` (mechanical)
`src/widgets/containers/mod.rs` (:39,47,181,202,293), `containers/reconcile.rs` (:16,700), `src/layout/hash.rs` (:10,353–358), `src/layout/stack/compute.rs`, `src/layout/stack/types.rs` (`FocusPolicyContext`→`FocusSizingContext`), `src/widgets/mod.rs:136`, `src/prelude.rs:154`, `examples/lazygit.rs` (4 sites), docs (enums.md, focus.md, widgets/layout.md, widgets/data.md, widget-defaults.md), CHANGELOG.

### Phase 2 — `FocusPolicy` enum + plumbing + OnDemand default (the big one)
- Enum + builder in `src/app/context.rs` (pattern of `key_dispatch_policy` at :527); `FocusState.policy`; policy param through `restore_focus`/`focus_next`/`focus_prev` + per-policy semantics; update focus.rs unit tests (:217–314).
- Call sites: `render_service/mod.rs:761`, `runner/mod.rs:1007`, `runner/overlay.rs:103`, `runner/key_dispatch.rs:294`, `test_backend.rs:155,660,814,1037`.
- Manual gating at the 3 dispatch paths + mouse site (above).
- Test churn: pin `.focus_policy(FocusPolicy::Auto)` on legacy-style examples/tests that assume startup auto-focus (keeps snapshots stable); add OnDemand-specific tests instead of rewriting.

### Phase 3 — `FocusRequest` channel: blur + programmatic next/prev
`enum FocusRequest { Key, Clear, Next, Prev }` replaces `Option<Key>` at `runtime.rs:164`; `Ctx::blur/focus_next/focus_prev` in component.rs; `apply_pending_focus_request` (`render_service/mod.rs:71`) handles all variants; TestBackend pump drain (:641–646) + public `blur()`. Note: under Auto, `blur()` clears and next restore re-focuses first-focusable (document as "reset to default focus").

### Phase 4 — Widget prop pass: `tab_stop` + `on_focus`/`on_blur` + default flips
- Trait: rename `in_tab_order()` → `is_tab_stop()` (`src/core/node/kind.rs:34`), add `on_focus_callback()/on_blur_callback() -> Option<&Callback<()>>` (default None). Regenerate delegate arms: `python3 scripts/generate-node-kind-delegate-arms.py --write`. Update `tree.rs:85` + `collect_focusables` (`tree.rs:766`).
- Per-widget 3-file pattern (mod.rs builder → reconcile.rs copy → node.rs field + override), template = Input's `tab_order` (`input/mod.rs:75,128,437`, `reconcile.rs:47-48`, `node.rs:52-53,61`). One pass per widget covering tab_stop + both callbacks. Widgets: accordion, button, checkbox, document_view, draggable_tab_bar, file_tree, hex_area, hyperlink, input (migrate `tab_order`→`tab_stop`; fix `file_tree/component.rs:269`), list, managed_terminal, pan_view, search_palette, slider, table, tabs, terminal, text_area, tree.
- Default flips to `focusable: false`: `pan_view/mod.rs:245` (keep auto-enable at :325), `hyperlink.rs:59`, `accordion/mod.rs:62`, `draggable_tab_bar/mod.rs:508`, `tabs/mod.rs:126`. DocumentView unchanged.
- Refresh parity tooling: `scripts/generate-widget-defaults.py`, `check-widget-style-slots.py`, `check-widget-variant-parity.py`; regen docs/widget-defaults.md.

### Phase 5 — `FocusScope` (Exclude / Contain)
- Enum in `containers/mod.rs`; `StackProps.focus_scope` via `impl_stack_props!` (:210); Frame builder+node+reconcile; include in `layout/hash.rs` StackProps hash; trait method `WidgetNode::focus_scope()` (default None) + delegate regen.
- Enforcement: `collect_focusables` (tree.rs:764) — on Exclude don't push, don't recurse (fixes tab ring + `focusables_in_subtree` + overlay traversal at once); switch Auto fallback from raw `tree.iter().find(is_focusable)` to scoped DFS helper; `find_first_focusable_descendant` (focus.rs:145) skips Exclude; click gate via `in_excluded_scope(tree, id)` in `focus_for_node` (`events.rs:447`); `request_focus` bypasses Exclude (documented escape hatch).
- Contain: in `focus_next/prev`, walk up from focused node to nearest Contain ancestor; if found cycle within `focusables_in_subtree(scope_id)`. Ordering at dispatch sites: overlay trap check → Contain check → global ring. Escaping Contain is the app's job (pane-switch keys) — document as "pane trap".
- Cache safety: `cached_focusables` is epoch-scoped (cleared in `begin_epoch`, tree.rs:865) — no extra invalidation needed; add no cross-epoch caches.

### Phase 6 — Events wiring + app hook + overlay `auto_focus` + devtools
- `notify_focus_change()` on AppRunner, modeled on `emit_terminal_focus_change` (`terminal_service.rs:96`): diff against `FocusState.last_notified`.
  - Dedup: same focus iff NodeId equal OR both keys `Some` and equal (suppresses spurious pairs on keyed remount).
  - Ordering: `on_blur(old)` → `on_focus(new)` → app `on_focus_changed`. Emit through the normal Callback queue (handlers run on next pump — never re-entrant into reconcile). Skip blur callback if old node gone; app hook still reports old key/tag from stored state.
  - Call sites: end of `finalize_after_reconcile` (after `ensure_overlay_focus`), tail of key dispatch (Tab), tail of mouse dispatch (click), event-loop request drain (`runner/mod.rs:1518`), initial mount. Never mid-reconcile.
- `OverlayRoot.auto_focus` + Modal/Popover builders (`src/overlay.rs`, `runner/overlay.rs:114`): when false, `suspend_focus_for_empty_overlay` instead of focusing.
- Devtools (`src/devtools/component.rs`, `state.rs` — zero focus display today): add Focus line — policy, focused tag/key/NodeId, ring length, focus_stack depth.

### Phase 7 — Theme kill switch + docs + final audit
- `Theme.focus_decoration: bool` (default true). Enforcement: theme-default focus-slot injection (`render/mod.rs:1282–1289` + siblings, add `&& theme.focus_decoration`), `resolve_interactive_style` (`style_resolve.rs:105`) skip theme-sourced focus, per-widget palette focus defaults + scrollbar `thumb_focus`.
- Precedence (document in docs/styling.md): explicit widget `.focus_style()` always wins > `focus_decoration=false` kills theme-sourced focus styling > `theme.focus`/palette roles. Note: `UnfocusedSelection` dimming is selection styling, not focus decoration — unaffected; add doc note that OnDemand lists start in unfocused-selection style.
- Docs: full rewrite of `docs/focus.md` (policies/tab_stop/scopes/events/blur); update enums.md, styling.md, widget-authoring.md (is_tab_stop + callbacks for custom widgets), widget-defaults.md, patterns.md, large-app-shells.md, keybindings.md (Tab under Manual), widgets/display.md, clipboard.md, README, CHANGELOG (reconcile promises at :85–88, 236–245, 357–360).

## Test Plan

- **Update**: `focus.rs` tests :217–314 (parameterize by policy; first-focusable-fallback becomes Auto-only; add OnDemand→None-with-key-kept, Manual→no-tag-restore); keyboard.rs non-focusable copy tests (verify pass after flips); accordion focusable test; run_tests.rs startup-focus assumptions.
- **New (TestBackend-driven)**:
  - Per policy: startup state; Tab-from-None; click-to-focus; focused-widget unmount (key retained, remount restores); request_focus-before-mount; blur() under each policy.
  - Overlays: modal over None under OnDemand (dismiss → None); modal under Manual (auto-focuses + traps); `auto_focus(false)` (traps, no focus); empty-overlay suspend regression.
  - tab_stop: focusable-not-tab-stop reachable by click + request_focus, skipped by Tab; Input migration.
  - Scoping: Exclude out of ring/fallback/click but not request_focus; Contain cycles + wraps within subtree; Contain inside overlay (overlay trap wins).
  - Events: blur-before-focus ordering; keyed-remount dedup (no spurious pair); app hook payload; events fire only post-finalize.
  - Visuals: `focus_decoration(false)` suppresses theme chrome; explicit `.focus_style()` survives.

## Risks / Gotchas

- **Three duplicated Tab dispatch paths** (key_dispatch.rs / runtime_dispatch.rs / messages.rs) + TestBackend must change identically in Phases 2 & 5 — extract a shared helper where borrows permit; else add a test exercising all three routes (real key, runtime dispatch, FrameworkCommandAction).
- **Default-flip blast radius**: OnDemand + focusable flips change startup snapshots of most examples (lazygit, todo, widgets, window_manager, frame_hub, messenger, search_lists…). Pin `Auto` on legacy examples; flagship examples demo OnDemand.
- **`focused_key` retention after None-fallback** means focus "returns" on remount — document prominently; `blur()` is the full clear.
- **Event dedup is key-based**: unkeyed focused widget with a new NodeId across reconcile fires a blur/focus pair — document "key your focusable widgets".
- **Generated code**: `is_tab_stop`/callback accessors/`focus_scope` touch delegate arms — regenerate via scripts, never hand-edit; parity check scripts gate CI.

## Verification

1. `cargo build && cargo test` after every phase (workspace, all features: `cargo test --all-features`).
2. `python3 scripts/check-widget-variant-parity.py` + `check-widget-style-slots.py` after Phases 4–5.
3. End-to-end: run `examples/todo` (or `widgets`) under each policy — confirm OnDemand starts unfocused, Tab brings focus in, click focuses, Esc-dismissed modal returns focus to None; `lazygit` example still behaves with `Auto` pinned + `FocusSizing` rename.
4. Run a Manual-policy scratch example: verify Tab does nothing, clicks fire handlers without moving focus, Modal still auto-focuses and traps, `auto_focus(false)` modal traps without focusing.
5. Devtools overlay shows policy + focused node; toggle `focus_decoration(false)` and confirm no focus chrome while `.focus_style()` overrides still render.
