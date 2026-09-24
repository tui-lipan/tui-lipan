# Keybindings & keymap

This page covers the **`keymap.conf` file** (text widgets and global actions), **`TextArea` newline** configuration, and the public **`tui_lipan::input`** helpers for parsing, formatting, and matching shortcuts-including **multi-key chords**.

For how keyboard events flow through the component tree (`on_key`, bubbling), see [`focus.md`](focus.md).

---

## `keymap.conf` (text input & app actions)

Text widgets and related handlers use **crokey**-style bindings loaded from a config file:

```rust
App::new()
    .keymap_path("/path/to/keymap.conf")  // Explicit path (highest priority)
```

Environment fallback: `TUI_LIPAN_KEYMAP=/path/to/keymap.conf`

Default path: `$XDG_CONFIG_HOME/tui-lipan/keymap.conf` (or `~/.config/tui-lipan/keymap.conf`)

**Format** - one action per line, `action = key1, key2`:

```
# Comments with #
copy = ctrl-c, super-c, ctrl-insert
paste = super-v, ctrl-shift-v
paste_selection = shift-insert
cut = ctrl-x, super-x, shift-delete
undo = ctrl-z, super-z
redo = ctrl-shift-z, ctrl-y
clear = ctrl-u
select_all = ctrl-a, super-a
move_left = left
select_word_right = shift-ctrl-right
delete_word_left = ctrl-backspace
insert_newline = enter
dismiss_overlay = esc
focus_next = tab
focus_prev = shift-tab
quit = ctrl-q
toggle_devtools = f12
```

**Available actions:** `copy`, `paste`, `paste_selection`, `cut`, `undo`, `redo`, `clear`, `select_all`, `move_left`, `move_right`, `move_up`, `move_down`, `move_word_left`, `move_word_right`, `select_word_left`, `select_word_right`, `delete_word_left`, `delete_word_right`, `move_home`, `move_end`, `select_home`, `select_end`, `insert_newline`, `copy_image`, `paste_image`, `quit`, `dismiss_overlay`, `focus_next`, `focus_prev`, `toggle_devtools`.

The text-widget `clear` action also accepts `clear-text` and `clear-input`
aliases. It has no default binding; users or apps opt in, for example
`clear = ctrl-u` or `clear = ctrl-c`. Clear performs an internal replace edit
and emits the normal text edit/change callbacks.

`toggle_devtools` is available when the `devtools` feature is enabled. `F12` is the default binding, but you can remap or unbind it in `keymap.conf`. App code can also control the panel directly with `ctx.show_devtools()`, `ctx.hide_devtools()`, and `ctx.toggle_devtools()`.

Clipboard actions are performable: copy/cut only consume when the action can run on a selection, and paste only consumes when the focused widget can accept it. Copy shortcuts such as `Ctrl+C` and `Ctrl+Insert` also copy active mouse selections from Input, TextArea, DocumentView, and Terminal even when those widgets are not focusable. The focused widget's own selection comes first, and a focused Terminal ignores selections in other widgets; if its own selection does not handle the shortcut, the key continues through the configured `TerminalKeyPolicy`. See [Clipboard](clipboard.md). Cut shortcuts such as `Ctrl+X` cut editable Input/TextArea selections.

For `TextArea`, a matching widget-level single-key clear binding takes
precedence over keymap clipboard bindings for the same key. `key_interceptor`
still runs first and can consume the key before clear handling.

`TextArea::vim_motions(true)` is not loaded from `keymap.conf`. It is a
per-widget modal editing option that starts in Normal mode and has its own Vim
grammar for motions, WORD motions (`W`, `B`, `E`) over non-whitespace runs,
operators (`d`, `y`, `c`), registers, search, marks, Visual/VisualLine
selections, and repeat (`.`). Use `TextArea::vim_keymap(...)`
with `TextAreaVimKeymap` when a widget needs aliases to canonical Vim command
characters. Vim undo/redo uses Normal `u` and `ctrl+r`; `Ctrl+Z` and `Ctrl+Y`
are not the Vim-mode undo/redo path and do not mutate Vim-enabled TextAreas. In
Visual modes, supported Vim motions update the cursor while preserving the visual
anchor so selection remains widget-owned and is not a keymap action. `V` enters
linewise Visual selection, which selects whole logical lines rather than wrapped
visual rows; the emitted cursor/anchor still span full lines while the terminal
caret stays on the active selected line. Mouse-created TextArea selections
(double/triple click or drag) enter Visual mode automatically. The existing
precedence is preserved: clipboard handling still runs before the Vim layer,
`key_interceptor` can consume keys before built-in TextArea handling, and matching
TextArea clear bindings continue to win before motion/default editing dispatch.
Mutating clipboard operations and clear bindings exit Visual or VisualLine mode
after they update the selection/text state.
Pending `/` and `?` searches render a bottom search bar inside the focused
TextArea, move the cursor into that bar while typing, right-align the current
match count, underline visible matches, and give the active target a distinct
background highlight. After `Enter`, the bottom bar disappears and the
`[current/total]` count is mirrored after the text on the visible row containing
the current match. The stored query stays highlighted and repeats with `n` / `N`.
Normal `Esc` hides the visible search feedback without forgetting the stored
query, so `n` / `N` can repeat and show it again.

For `DocumentView`, shared selections (`shared_selection_id`) copy as one concatenated payload per shared group within the same `ScrollView`.

Use `none` to unbind a key:

```
quit = none
focus_next = none
focus_prev = none
```

To remap focus traversal instead of disabling it:

```
focus_next = ctrl-j
focus_prev = ctrl-k
```

Shift+Tab is normalized to the terminal's reverse-Tab event automatically.

Under `FocusPolicy::Manual`, framework `focus_next` / `focus_prev` traversal is skipped and the
key remains available to widget, command, or component handling. Capturing overlays still cycle
their trapped focus, and explicit `ctx.focus_next()` / `ctx.focus_prev()` calls still work.

**Modifier names:** `ctrl`, `alt`, `shift`, `super` (aliases: `cmd`, `command`, `meta`, `win`, `windows`). Use `-` between parts: `ctrl-shift-z`, `super-c`.

### Built-in keymap matching

Entries are parsed with the same **`KeyBinding` rules** as the public API, including chord syntax. The bundled keymap runtime drives built-in actions from both single-step bindings and multi-step chords such as `ctrl+x b`.

When a key is a pending chord prefix, the runner consumes that prefix before focused-widget dispatch. If the following key completes the chord, the mapped built-in action runs. If it does not complete the chord, the matcher resets and tries that key as a fresh keypress, so it can still trigger a single-step keymap action or fall through to normal widget handling. There is no chord timeout.

### Layered dispatch (Rust API)

Explicit `App` configuration wins over file and environment keymaps:

1. `App::framework_keymap(...)` / `App::global_quit(None)` (Rust)
2. `App::keymap_path(...)` (app file)
3. `TUI_LIPAN_KEYMAP` / default user keymap (when `UserKeymapPolicy::Enabled`)
4. Built-in defaults

Policy builders on `App`:

```rust
App::new()
    .framework_keymap(FrameworkKeymap::default().unbind(FrameworkAction::Quit))
    .global_quit(None) // sugar for unbinding quit
    .user_keymap_policy(UserKeymapPolicy::Disabled)
    .key_dispatch_policy(KeyDispatchPolicy::AppCommandsFirst)
    .terminal_key_policy(TerminalKeyPolicy::AppCommandsThenTerminal)
    .command_conflict_policy(CommandConflictPolicy::HighestPriority)
    .chord_mismatch_policy(ChordMismatchPolicy::ForwardPrefixAndCurrent)
```

Command palette entries distinguish **display hints** (`keybinding_hint`) from **executable shortcuts** (`shortcut` / `shortcuts`). Shortcut conflicts resolve with `CommandConflictPolicy::FirstRegistered` (default) or `HighestPriority`.

See [`focus.md`](focus.md) for the full keyboard dispatch order and [`widgets/terminal.md`](widgets/terminal.md) for terminal-focused policies.

---

## `KeyBinding` / `KeyBindings` parsing

- **`KeyBinding`**: one shortcut, optionally a **chord** (sequence of key steps).
  - **Whitespace** separates steps: `ctrl+x b` → Ctrl+X, then `b`.
  - Each step is a single **combination** (`ctrl-shift-up`, `super-c`, …).
- **`KeyBindings`**: **alternatives** for the same logical shortcut.
  - **Comma** separates alternatives: `ctrl+d, ctrl+q` → either binding.

So `ctrl+x b, ctrl+q` means: *(Ctrl+X then B)* **or** *Ctrl+Q*.

Within a step, `-` and `+` are interchangeable modifier separators (`ctrl+c` ≡ `ctrl-c`).
A step that is only the plus key is special: bare `+`, the name `plus`, or a trailing
`…-plus` / `…-+` bind `Char('+')`. Crokey's `hyphen` / `minus` names both mean `-`;
formatted output shows `-` rather than `Hyphen`.

### Matching

- **`KeyBinding::matches_sequence(&[KeyEvent])`** - true when the slice length equals the binding’s step count and each event matches the corresponding step (same normalization as the keymap: legacy raw ctrl characters, BackTab, etc.).
- **`KeyBinding::is_chord()`** / **`step_count()`** - inspect parsed chords.
- There is **no** `KeyBinding::matches(&KeyEvent)` on a single event; use `matches_sequence(&[key])` for a one-step binding, or **`ChordMatcher`** (below) when several keys must be accumulated.

### `ChordMatcher` (stateful chords)

`ChordMatcher<T>` holds a list of `(KeyBinding, T)` and implements incremental matching across key events: `feed(&KeyEvent) -> ChordResult<&T>`.

- **`ChordResult::Matched`** - a full binding matched.
- **`ChordResult::Pending`** - prefix of at least one chord; more keys needed.
- **`ChordResult::None`** - no match (after reset behavior for failed continuations).

If one key is both a full single-step binding and a prefix of a longer chord, the matcher stays **pending** until the next key disambiguates.

Re-exported from the crate root and **`prelude`** (`ChordMatcher`, `ChordResult`).

### Three string forms

A `KeyBinding` has three string forms, each with one job. None of them affects parsing, equality,
or matching.

| Form | Method | Example (`super-shift-p`) | Use it for |
| --- | --- | --- | --- |
| Label | `label()`, `Display` | `Super+Shift+P` | Text shown to people |
| Source | `to_source()` | `super-shift-p` | Text written back to a config file |
| Canonical | `canonical()` | `Super+Shift+P` | Identity and debugging |

The modifier is named `Super` in every form; `cmd`, `command`, `meta`, `win`, and `windows` are
accepted as input aliases.

```rust
use std::str::FromStr;
use tui_lipan::input::{
    KeyBinding,
    KeyBindings,
    format_binding,
    format_binding_lowercase,
    format_bindings,
    format_bindings_lowercase,
};

let one = KeyBinding::from_str("cmd+p")?;
assert_eq!(one.to_string(), "Super+P");
assert_eq!(one.to_source(), "super-p");

let many = KeyBindings::from_str("ctrl+d, ctrl+q")?;
assert_eq!(many.to_string(), "Ctrl+D / Ctrl+Q");
assert_eq!(many.to_source(), "ctrl-d, ctrl-q");

let chord = KeyBinding::from_str("ctrl+x b")?;
assert!(chord.is_chord());
assert_eq!(chord.to_string(), "Ctrl+X b");
assert_eq!(chord.canonical(), "Ctrl+X B");

assert_eq!(format_binding("control-shift-up")?, "Ctrl+Shift+Up");
assert_eq!(format_binding("shift-m")?, "M");
assert_eq!(format_bindings("?, shift-/, ctrl-shift-3")?, "? / Ctrl+Shift+3");
assert_eq!(format_binding_lowercase("Esc")?, "esc");
assert_eq!(format_bindings_lowercase("ctrl+d, super+q")?, "ctrl+d / super+q");
```

#### Labels

`label()` writes keycap notation:

- A printable key with `Ctrl`, `Alt`, or `Super` is a keycap chord. Every modifier is written out,
  letters are uppercase, and case carries no meaning: `Ctrl+A`, `Ctrl+Shift+A`, `Ctrl+Shift+/`.
- A printable key on its own shows the character it types: `s`, `S`, `?`. Shifted punctuation uses
  the US layout (`shift-/` → `?`).
- A named key always writes its modifiers out: `Tab`, `Shift+Tab`, `Ctrl+Shift+Left`.

Chord steps are separated by spaces (`Ctrl+X b`). `KeyBindings::label()` joins alternatives with
` / ` and drops repeats, so `?, shift-/` shows one `?`. The command palette shows shortcut hints in
this notation.

`KeyMods::label()` writes held modifiers in the same order (`Ctrl+Shift`), so a shortcut recorder
can show `Ctrl+Shift+` while keys are held and `Ctrl+Shift+A` once one is pressed, without the
modifiers moving.

#### Source spelling

`to_source()` is a compatibility promise: it always parses back to an equal binding, and its
format does not follow display changes. Modifiers are `ctrl`, `alt`, `super`, `shift` in that
order, joined to the key with `-`. Keys are lowercase names (`enter`, `pageup`, `f5`, `space`),
and the `-`, `+`, and `,` keys are `minus`, `plus`, and `comma`. Chord steps are separated by
spaces. `KeyBindings::to_source()` joins alternatives with `, `, so a comma key inside a list of
alternatives must be written `comma`.

### Recording and checking bindings

`KeyBinding::from_key_event(event)` builds the one-step binding an event stands for. Modifiers come
only from what the event reports: a character's case never implies Shift, because Caps Lock
produces capitals without it and legacy terminal encodings cannot report Shift alongside Ctrl.
`Char('A')` with Ctrl alone records as `ctrl-a`. Raw control characters become their Ctrl chord,
and `BackTab` becomes `shift-tab`, exactly as `matches_sequence` treats the same events.

`KeyBinding::conflicts_with(&other)` is true when one binding would fire before the other could
finish: they are equal, or one's steps start the other's (`ctrl+x` and `ctrl+x b`). Run it before
assigning a binding to a second command.

```rust
use std::str::FromStr;
use tui_lipan::input::KeyBinding;
use tui_lipan::prelude::{KeyCode, KeyEvent, KeyMods};

let recorded = KeyBinding::from_key_event(KeyEvent {
    code: KeyCode::Char('A'),
    mods: KeyMods::CTRL,
});
assert_eq!(recorded.to_source(), "ctrl-a");
assert!(recorded.conflicts_with(&KeyBinding::from_str("ctrl-a x")?));
```

### Expanding a binding into key events

`KeyBinding::key_events()` turns a parsed binding into one `KeyEvent` per chord step,
for callers that need to *replay* a binding rather than match it - forwarding a shortcut
into an embedded terminal, driving a headless test, or scripting input.

```rust
use std::str::FromStr;
use tui_lipan::input::KeyBinding;

let events = KeyBinding::from_str("ctrl-x b")?.key_events()?;
assert_eq!(events.len(), 2); // one per chord step
```

`shift-tab` expands to `KeyCode::BackTab` with the shift modifier cleared, since
`BackTab` already encodes it.

Bindings that cannot be expressed as discrete events fail with `KeyEventExpansionError`:
`MultiKeyCombination` for steps pressing several key codes at once, and
`UnsupportedKeyCode` for keys with no `KeyCode` equivalent. This is separate from
`KeyBindingParseError` - the binding parsed fine, it just cannot be replayed.

---

## TextArea newline and Tab keys

Configure Enter behavior for `TextArea` only (does not affect single-line `Input`):

```rust
App::new()
    .text_area_newline_binding(TextAreaNewlineBinding::Enter)        // default
    // or:
    .text_area_newline_binding(TextAreaNewlineBinding::ShiftEnter)
    .text_area_newline_binding(TextAreaNewlineBinding::EnterOrShiftEnter)
```

Per-widget override (takes priority over app setting):

```rust
TextArea::new(value).newline_binding(TextAreaNewlineBinding::ShiftEnter)
```

`TextArea::tab_display_width(width)` controls the visual width of inserted literal tab characters.
It was renamed from `tab_stop`; `.tab_stop(bool)` now consistently means focus-ring membership on
focusable widgets.
