# Text Editing

`TextEditor` and `TextInput` are the text buffer models that power `TextArea` and `Input` widgets respectively. They manage cursor position, selection, undo/redo history, and keystroke handling. Use them in component state to maintain editing context across renders.

```rust
use tui_lipan::prelude::*;
```

## TextEditor (multi-line)

A multi-line text editor with selection support. The cursor is stored as a byte index into the UTF-8 string and is always kept on a character boundary. Selection is represented by an optional anchor position.

```rust
let mut editor = TextEditor::new("Hello\nWorld");
// Cursor starts at position 0 (beginning of text)

// Also available via Default
let editor = TextEditor::default(); // empty text, cursor at 0
```

### Accessors

| Method | Returns | Description |
|--------|---------|-------------|
| `text()` | `&str` | Current text content |
| `cursor()` | `usize` | Cursor byte position |
| `anchor()` | `Option<usize>` | Selection anchor (if active) |
| `selection()` | `Option<(usize, usize)>` | Ordered `(start, end)` range if selection exists |
| `selected_text()` | `Option<&str>` | Text within the selection |

### Editing

| Method | Description |
|--------|-------------|
| `insert_char(ch)` | Insert a character at cursor (replaces selection if active) |
| `insert_str(s)` | Insert a string at cursor (replaces selection if active) |
| `backspace()` | Delete character before cursor or delete selection |
| `delete()` | Delete character after cursor or delete selection |
| `delete_word_left()` | Delete word before cursor |
| `delete_word_right()` | Delete word after cursor |
| `clear()` | Clear the full buffer as an undoable replace edit; returns `true` when text, cursor, or selection changed |

### Cursor movement

| Method | Description |
|--------|-------------|
| `move_left()` / `move_right()` | Move cursor by one character, clears selection |
| `move_up()` / `move_down()` | Move cursor by one line (grapheme-column aware) |
| `move_word_left()` / `move_word_right()` | Move cursor by one word |
| `move_home()` / `move_end()` | Move to start/end of **current line** |

### Selection

| Method | Description |
|--------|-------------|
| `select_left()` / `select_right()` | Extend selection by one character |
| `select_up()` / `select_down()` | Extend selection by one line |
| `select_word_left()` / `select_word_right()` | Extend selection by one word |
| `select_home()` / `select_end()` | Extend selection to start/end of current line |
| `select_all()` | Select all text |
| `clear_selection()` | Remove selection (keep cursor position) |

### Sync and setters

| Method | Description |
|--------|-------------|
| `set_text(s)` | Replace entire text content (clears history) |
| `set_cursor(pos)` | Set cursor position (clears selection) |
| `set_cursor_keep_anchor(pos)` | Set cursor position (preserves anchor for selection) |
| `set_anchor(pos)` | Set anchor position directly |

### Undo / Redo

`TextEditor` maintains an undo history (up to 1000 entries by default). Consecutive edits of the same kind are merged into logical groups for natural undo behavior.
`clear()` is recorded as a replace edit, so `undo()` restores the previous text,
cursor, and selection.

| Method | Description |
|--------|-------------|
| `can_undo()` | Whether undo is available |
| `can_redo()` | Whether redo is available |
| `undo()` | Undo last edit group |
| `redo()` | Redo last undone edit group |
| `clear_history()` | Clear all undo/redo history |

### Keystroke handling

`handle_key` processes a `KeyEvent` using the default keymap and returns whether the editor state changed:

```rust
let changed = editor.handle_key(key_event);
if changed {
    // editor state was modified
}
```

Supported keys include character insertion, arrow keys, word movement (`Ctrl+Left/Right`), `Home`/`End`, `Backspace`/`Delete`, word deletion (`Ctrl+Backspace`/`Ctrl+Delete`), `Enter` (newline), and undo/redo (`Ctrl+Z`/`Ctrl+Shift+Z`).

Clipboard operations (copy/cut/paste) are handled at the widget layer, not by `handle_key`.
`TextEditor` itself remains plain and keymap-driven; opt-in Vim motions are a
`TextArea` widget-layer behavior enabled with `TextArea::vim_motions(true)`,
including Visual-mode cursor/anchor updates for selections.

---

## TextInput (single-line)

A single-line text input model. Same selection and undo/redo model as `TextEditor`, but constrains input to a single line.

```rust
let mut input = TextInput::new("initial value");
// Cursor starts at end of text (unlike TextEditor which starts at 0)

let input = TextInput::default(); // empty text, cursor at 0
```

### Differences from TextEditor

| Behavior | TextInput | TextEditor |
|----------|-----------|------------|
| Initial cursor | End of text | Start of text |
| Newlines | Replaced with space on insert | Preserved |
| `Home` / `End` | Move to start/end of **entire string** | Move to start/end of **current line** |
| Vertical movement | Not supported | `move_up` / `move_down` |

### Additional methods

| Method | Description |
|--------|-------------|
| `clear()` | Clear all text content |
| `delete_to_start()` | Delete from cursor to start of text (or delete selection) |
| `delete_to_end()` | Delete from cursor to end of text (or delete selection) |

All methods from `TextEditor` (accessors, cursor movement, selection, undo/redo) are available on `TextInput` as well, except for vertical movement methods.

---

## Integration with widgets

`TextEditor` and `TextInput` are typically stored in component state and passed to `TextArea` and `Input` widgets using `bound` constructors that preserve cursor position and selection across rerenders:

### TextArea + TextEditor

```rust
struct State {
    editor: TextEditor,
}

fn create_state(&self, _props: &Self::Properties) -> Self::State {
    State {
        editor: TextEditor::new(""),
    }
}

fn view(&self, ctx: &Context<Self>) -> Element {
    TextArea::bound(&ctx.state.editor)
        .on_change(ctx.link().callback(|ev: TextAreaEvent| Msg::EditorChanged(ev)))
}

fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::EditorChanged(ev) => {
            ctx.state.editor.set_text(ev.value.to_string());
            ctx.state.editor.set_cursor(ev.cursor);
            ctx.state.editor.set_anchor(ev.anchor);
            Update::layout()
        }
    }
}
```

Selections remain visible by default when a `TextArea` loses focus through
keyboard or programmatic focus changes, matching `DocumentView`. Use
`show_selection_when_unfocused(false)` to opt out, and set
`unfocused_selection_style(...)` or `inherit_unfocused_selection_style()` to tune
that inactive highlight. Inherited/default `TextArea` selection styles resolve
against `theme.text_selection`, not the list/item `theme.selection` role.

### Input + TextInput

```rust
struct State {
    input: TextInput,
}

fn create_state(&self, _props: &Self::Properties) -> Self::State {
    State {
        input: TextInput::new(""),
    }
}

fn view(&self, ctx: &Context<Self>) -> Element {
    Input::bound(&ctx.state.input)
        .placeholder("Type here...")
        .on_change(ctx.link().callback(|ev: InputEvent| Msg::InputChanged(ev)))
}

fn update(&mut self, msg: Msg, ctx: &mut Context<Self>) -> Update {
    match msg {
        Msg::InputChanged(ev) => {
            ev.apply_to(&mut ctx.state.input);
            Update::layout()
        }
    }
}
```

`Input::bound(&state)` reads the value, cursor, and anchor from the `TextInput` state bundle, so cursor position and text selection are preserved across rerenders. The `InputEvent::apply_to` helper writes all three fields back in one call.

> **Avoid `Input::new(value)` for editable inputs** — it always places the cursor at the end, so left/right arrow keys and selection will appear broken after each keystroke.

`TextArea` clear shortcuts use the widget's internal editor and emit the normal
change/edit callbacks. Prefer `TextArea::clear_bindings(bindings)` or the
keymap `clear` action over replacing the controlled value externally when you
want undo to restore the pre-clear buffer.

`TextArea::vim_motions(true)` adds TextArea-only Normal/Insert/Visual/VisualLine
modal editing on top of the widget's internal editor state. Vim-enabled TextAreas
start in Normal mode. Visual-mode motions update the cursor and anchor through
normal `TextAreaEvent` state sync; VisualLine mode keeps selections expanded to
whole logical lines while drawing the caret on the active selected line. Mouse
double/triple-click and drag selections enter Visual mode automatically. Vim
undo/redo uses Normal `u` and `ctrl+r`. The Vim layer also handles delete/yank/change
operators, lowercase word motions, uppercase WORD motions (`W` / `B` / `E`) over
non-whitespace runs, `x`/`X`, `o`/`O`, `p`/`P`, registers, search, marks, text
objects, and repeat (`.`) through the same `TextAreaEvent`/`TextEditEvent`
emission path.
Searches render a bottom search bar on the focused TextArea, move the cursor into
that bar while typing, right-align the current match count, underline visible
matches, and give the active target a distinct background highlight as `Enter`,
`n`, and `N` navigate results. After `Enter`, the bottom bar disappears and the
same `[current/total]` count is mirrored after the text on the visible row
containing the current match. Normal `Esc` hides the visible search feedback
without forgetting the stored query.
Observe mode changes with `TextArea::on_vim_mode_change`; store the emitted
`TextAreaVimMode` only when your app wants a status indicator or mode-aware
styling.

### TextEditEvent

Both `Input` and `TextArea` emit `TextEditEvent` through their `on_edit` callback for structured edit tracking:

```rust
pub struct TextEditEvent {
    pub start: usize,                // Byte offset where the edit began
    pub deleted: Arc<str>,           // Text that was removed
    pub inserted: Arc<str>,          // Text that was inserted
    pub cursor_before: usize,        // Cursor position before the edit
    pub anchor_before: Option<usize>,// Anchor position before the edit
    pub cursor_after: usize,         // Cursor position after the edit
    pub anchor_after: Option<usize>, // Anchor position after the edit
    pub kind: TextEditKind,          // Type of edit
}

pub enum TextEditKind {
    Insert,
    DeleteBackspace,
    DeleteForward,
    Replace,
}
```

Clear operations are reported as `TextEditKind::Replace` through the same
`TextAreaEvent` / `TextEditEvent` path as other text edits.

Wire it up via `on_edit`:

```rust
TextArea {
    editor: ctx.state.editor.clone(),
    on_edit: ctx.link().callback(|ev: TextEditEvent| Msg::OnEdit(ev)),
}
```

### LineIndex and text coordinates

`LineIndex` converts canonical UTF-8 byte offsets into snapshot line/column
coordinates for editor integrations. Build it from the current text and rebuild
it whenever the text changes:

```rust
let index = LineIndex::new(editor.text());
let pos = index.byte_to_position(editor.text(), editor.cursor());
let byte = index.position_to_byte(editor.text(), pos);
```

Columns are Unicode scalar counts by default. Use
`byte_to_position_with_encoding` / `position_to_byte_with_encoding` with
`TextEncoding::Utf8` or `TextEncoding::Utf16` when talking to protocols that use
byte or UTF-16 columns. Empty text has one logical line, trailing `\n` creates a
final empty line, and only `\n` is treated as a line break.

---

## Text motion helpers (`text_motion`)

`TextArea`'s vim mode word/WORD/line motions (`w`/`b`/`e`, `W`/`B`/`E`, `0`/`^`/`$`) are pure,
byte-offset functions with no dependency on `TextEditor`/`TextArea` state. They're exposed
publicly through `tui_lipan::text_motion` (and re-exported from the prelude) so host apps that
render their own text grids — for example a terminal emulator's scrollback copy mode — can reuse
the same algorithms instead of reimplementing vim motion from scratch.

| Function | Motion | Description |
|----------|--------|-------------|
| `word_forward_start(text, cursor)` | `w` | Start of the next word |
| `word_backward_start(text, cursor)` | `b` | Start of the previous word |
| `word_end(text, cursor)` | `e` | End of the current/next word |
| `big_word_forward_start(text, cursor)` | `W` | Start of the next WORD (whitespace-delimited, punctuation included) |
| `big_word_backward_start(text, cursor)` | `B` | Start of the previous WORD |
| `big_word_end(text, cursor)` | `E` | End of the current/next WORD |
| `line_start_at(text, cursor)` | `0` | Start of the line containing `cursor` |
| `line_end_at(text, cursor)` | `$` | One past the end of the line (exclusive of the newline) |
| `first_nonblank_in_line(text, line_start, line_end)` | `^` | First non-blank byte in `text[line_start..line_end]`, or `line_end` if the line is blank |

```rust
use tui_lipan::text_motion::{word_forward_start, word_end};

let text = "cat dog";
assert_eq!(word_forward_start(text, 0), 4); // -> start of "dog"
assert_eq!(word_end(text, 0), 3);           // -> one past "cat"
```

**Cursor convention:** offsets are "insertion points", matching `TextEditor`/`TextInput` — a
cursor value `N` sits *between* the bytes at `N - 1` and `N`, not "on" the character at `N`. This
matters most for `word_end`/`big_word_end`, which land one byte **past** the word's last
character (the result can equal `text.len()` and isn't always a valid index to read a character
from). If your own cursor model tracks a selected *cell* instead (as in a terminal grid), convert
to an insertion point — add the byte width of the character under the cursor — before calling
`word_end`/`big_word_end`, then map the result back down to the cell at `offset - 1`. Feeding a
cell's own start byte directly in breaks the case where the cursor already sits on a word's last
character: since that byte still belongs to the current word, the motion re-finds the same word's
end instead of advancing to the next word.

---

## Examples

- `examples/text_area.rs` - Two `TextEditor` instances driving `TextArea` widgets
- `examples/text_area_sentinels.rs` - `TextEditor` with inline sentinels and snapshots
- `examples/todo.rs` - `TextInput` for new item entry
- `examples/inline.rs` - `TextInput` with insert mode
- `examples/opencode_home.rs` - `TextEditor` in a multi-panel layout
- `examples/search_lists.rs` - `TextInput` for filter-as-you-type

## Imports

`TextEditor`, `TextEditEvent`, `TextEditKind`, `LineIndex`, `TextPosition`,
`TextRange`, and `TextEncoding` are available from both the prelude and the
crate root:

```rust
use tui_lipan::TextEditor;
use tui_lipan::TextEditEvent;
use tui_lipan::{LineIndex, TextEncoding, TextPosition, TextRange};
```

`TextInput` is available from the prelude only:

```rust
use tui_lipan::prelude::TextInput;
// or
use tui_lipan::prelude::*;
```

The `text_motion` module functions (`word_forward_start`, `word_backward_start`, `word_end`,
`big_word_forward_start`, `big_word_backward_start`, `big_word_end`, `line_start_at`,
`line_end_at`, `first_nonblank_in_line`) are available from both the crate root module path and
the prelude:

```rust
use tui_lipan::text_motion::{word_forward_start, word_end};
// or
use tui_lipan::prelude::*; // brings the same functions into scope directly
```

## TextArea metrics, decorations, and state callbacks

For editor integrations, keep byte offsets as the source of truth and use
`LineIndex`/`TextPosition` only as projections. A keyed `TextArea` can be read on
the next frame with `Context::text_area_metrics(key)`, which includes viewport,
gutter, scrollbar, logical/visual line ranges, and cursor rectangles without
backend-specific types.

External highlights can be supplied as byte-range `TextAreaDecoration`s.
`Range`, `WholeLine`, and `Underline` styles render before selection so the
selected text style remains highest priority. `Underline` enables the underline
modifier automatically. `TextAreaDecorationKind::VirtualText` is deprecated as a
reserved no-op; virtual content uses `TextAreaVirtualText` instead.

`TextAreaVirtualText::inline(anchor, spans)` renders styled inlay text before the
anchor byte. It occupies terminal columns, affects wrapping/cursor rectangles and
mouse hit-testing, but never enters `value`, edits, undo/redo, or selection byte
ranges. A cursor at the anchor is drawn after the virtual text. Use
`TextAreaVirtualText::eol(anchor, spans)` for end-of-line diagnostics; EOL text
is clipped at the viewport edge and does not reflow the line.

Use `on_editor_state_change` when one coherent cursor/selection/edit callback is
preferable to multiple widget-specific callbacks. Existing `on_change`,
`on_edit`, `on_scroll`, and `on_vim_mode_change` behavior is unchanged.
