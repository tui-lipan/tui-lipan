# Terminal image passthrough

*(feature: `terminal-images`)*

Programs running inside a `Terminal` pane can draw pictures. `TerminalScreen` reads the
[Kitty graphics protocol](https://sw.kovidgoyal.net/kitty/graphics-protocol/) out of the PTY byte
stream, decodes it, and the renderer paints the result over the pane's text.

```toml
tui-lipan = { version = "0.2", features = ["terminal-images"] }
```

```bash
cargo run --example terminal_images --features terminal-images
```

## It does not depend on the host terminal

The child's escapes are never forwarded to the host. They are decoded into pixels and re-encoded
through the same path the [`Image`](display.md#image-requires-feature-image) widget uses, so the host renders them with whatever
*it* supports — Kitty, iTerm2, sixel, or half-blocks. A pane in a plain `xterm` shows pictures.

Decoding rather than forwarding is also what makes the rest work: image ids from two panes cannot
collide, a pane that is half scrolled off gets its pixels cropped instead of squashed, and the
cell-diff render pipeline is untouched.

## Setting the cell size

A terminal deals in cells; a program drawing a picture needs pixels. It learns the conversion from
the PTY's `TIOCGWINSZ` pixel fields or by asking with `CSI 14 t`. Give both ends the same answer:

```rust
use tui_lipan::prelude::*;

let cell = host_cell_size();

let mut screen = TerminalScreen::new(rows, cols, scrollback);
screen.set_cell_size(cell);

let config = TerminalPtyConfig::default().cell_size(cell);
```

[`ManagedTerminal`](terminal.md) does this for you. A raw `TerminalScreen` defaults to a 10x20
cell, which is a guess: a mismatch shows up as images that overlap the text below them or leave a
gap, because the child reserved a different number of rows than the pane drew.

Pass the cell size to later resizes too, with `TerminalPty::resize_with_cell_size`; plain `resize`
keeps the last one it was given.

## Where images live

### Two ways an image gets placed

**At the cursor** (`a=T` without `U=1`) is what `icat` and friends do: the image lands where the
cursor is, the cursor moves past it, and the placement is anchored to that scrollback line.

**Through placeholder cells** (`a=T,U=1`) is what terminal UI toolkits do, `ratatui-image` among
them: the transmission draws nothing, and the program then writes the placeholder character
`U+10EEEE` into the cells the image should cover, tagging them with the image id (in the cell's
foreground colour) and the position inside the image (in combining marks). Those placements are
read back off the grid on every snapshot rather than stored, so they scroll, clear, and reflow with
the cells holding them, for free. A cell may leave any of that out and inherit it from its
left-hand neighbour — including the high byte of the image id — which is what keeps a row of
placeholders down to a single escape sequence.

### Cursor placements

A cursor placement is anchored to an absolute scrollback line, in the same space as `OSC 133`
semantic marks. That is what makes it behave like the text it was drawn against:

| Event | What happens |
| --- | --- |
| Output scrolls | The image scrolls with it, cropped row by row as it leaves the viewport |
| Scrolling back | It reappears at the line it was drawn on |
| A line falls out of scrollback | The image's remaining rows stay; it goes once its last row is evicted |
| Alternate screen | Placements made there are dropped when the child leaves it |
| Column resize | Kept, unless the change actually rewraps text — then the anchor stops naming what it named, and placements are dropped |
| `RIS` / `TerminalScreen::reset` | Everything is cleared |

A placeholder placement needs none of that bookkeeping: it *is* the text, so it does whatever the
cells do, and it is gone the moment they are.

Each placement carries the `image_id` its transmission used. A renderer must key its encoding on
that and not on the pixels alone: a host drawing through Kitty identifies a placement by the id of
its encoding, so two placements sharing one encoding are a single placement to it — and two copies
of one picture would collapse, the second silently not drawn.

`TerminalRenderSnapshot::images` carries the placements overlapping the visible rows, back to front
by Kitty z-index. Rows and columns are viewport-relative, and a cursor placement's may be negative
when the image starts above or to the left of the pane.

A transmission may name the cell box it is meant to fill (`c=` columns, `r=` rows), and a virtual
placement is then mapped through that box rather than through the cell grid: each placeholder cell
takes the slice of the image its position in the box calls for. This is what lets a child draw at
whatever resolution it likes — twice the cell size on a HiDPI screen, say — and still have the
picture scaled into the cells it asked for instead of cropped to its top-left corner. The box belongs
to the image id, so a sender animating by re-transmitting under the same id declares it once.

## Memory

Decoded pixels are capped per screen, 96 MiB by default, and evicted least-recently-used once the
cap is passed — placed images included, since leaving old plots on screen must not pin memory. One
image larger than the whole budget is kept anyway; that is a budget set too low, not a picture that
should silently fail to appear.

```rust
screen.set_image_budget(16 * 1024 * 1024);
```

Server-side terminal mirrors that forward the original PTY stream to another rendering client can
avoid decoding every image twice:

```rust
screen.set_image_storage_enabled(false);
```

That mode still validates command metadata, answers protocol queries, and applies image-implied
cursor movement. It retains dimensions only; render snapshots contain no images. Raw RGB/RGBA
payloads also skip base64 decoding, while PNG payloads are still decoded far enough to obtain their
dimensions.

`screen.has_images()` is whether any of those pixels are still retained: true after a child has
transmitted an image that has not been deleted or evicted, including when it has scrolled out of
view. Visible placements this frame are `snapshot.images`. A host that needs the class of pane
whose image layer does not follow a widget shrink or fade should look at the flag, not at the
process in front of the PTY.

Payloads are bounded before decoding as well: 32 MiB per transmission, 16384 pixels per axis.

## Out-of-band payloads

A child redrawing a full window every frame can leave its pixels somewhere and *name* them in the
escape sequence instead of base64-encoding several megabytes through the PTY: `t=f` a file it leaves
in place, `t=t` a temporary file, `t=s` a POSIX shared-memory object. This is the difference between
a hundred bytes on the wire and a compress/encode/decode/inflate round trip per frame, and it is what
lets a program in a pane animate at the host's own frame rate.

The last two are consumed by reading - the file is deleted, the object unlinked - so exactly one
reader may ever claim them. Anything that fans one PTY stream out to several readers (a multiplexer
with several attached clients, a server-side mirror) must therefore decline them and keep only `t=f`,
which stays readable:

```rust
screen.set_image_media_policy(GraphicsMediaPolicy::SHARED);
```

`GraphicsMediaPolicy::NONE` declines all three, so every picture arrives inline. Refusals are the
protocol's own `EBADF`/`ENOTSUPP` reports, which is what makes a child fall back to `t=d` rather than
lose the image. A `t=t` path must be temporary or say in its own name what it is for, `t=s` names are
resolved as shared-memory objects rather than paths, and both reads are capped by the same
transmission budget as an inline payload.

A path only means something on the machine that wrote it, so a pane attached from elsewhere should
decline: a remote client reading `/tmp/...` from its own filesystem is the one failure mode here that
is not a clean error.

### Out the other side, to the host

The same reasoning applies to what this framework writes *to* the terminal it is running in, and the
cost is the same one twice over: a pane whose child sends a full window of pixels every frame would
otherwise have them deflated, base64-encoded, chunked and written down stdout, and the host would
undo all of it.

So a frame goes into a POSIX shared-memory object and the escape sequence carries its name. Nothing
configures this. At startup the host is asked - with a `t=s` query it can only answer by reading a
real object - and only a terminal that answers `OK` is handed frames that way; everything else keeps
the inline path, including a terminal reached through `tmux`, whose reader is not the terminal, and
one on another machine, which cannot resolve a name in this machine's memory and says so. Objects are
unlinked by the host as it reads them, and by this process for any frame the host was never told
about.

On Linux the objects are pooled, which is why `/dev/shm` holds a handful of `tui-lipan-pool-<pid>-*`
names for as long as a pane is drawing. A fresh object per frame is a fresh *allocation* per frame -
every byte written lands on a page that does not exist yet - so each slot is created once and written
again every frame, and what the host is handed is a fresh hard link to it. That link is the part the
host unlinks, and its disappearance is the only signal that the host is finished: a slot whose link is
still there is never reused, and a frame that finds every slot busy allocates its own object exactly
as it used to. Pool objects left behind by a run that was killed are removed by the next run, which
can tell because the name carries the process that made it.

Windows has no POSIX shared-memory namespace, so frames there are always inline.

## What is supported

| Key | Support |
| --- | --- |
| `a=t`, `a=T`, `a=p`, `a=d`, `a=q` | Transmit, transmit-and-display, display, delete, query |
| `t=d` | Direct transmission, chunked with `m=1` |
| `t=f`, `t=t`, `t=s` | Out-of-band transmission: a file left in place, a temporary file, a POSIX shared-memory object |
| `f=24`, `f=32`, `f=100` | RGB, RGBA, PNG |
| `o=z` | zlib-compressed payloads |
| `s`, `v` | Pixel dimensions for the raw formats |
| `i`, `I`, `p` | Image id, image number, placement id |
| `x`, `y`, `w`, `h` | Source rectangle to display |
| `c`, `r` | Explicit placement size in cells |
| `z` | Stacking order against the text layer |
| `C=1` | Leave the cursor where it is |
| `q=1`, `q=2` | Suppress success reports / all reports |
| `d=` | `a`/`A`, `i`/`I`, `n`/`N`, `c`/`C`, `z`/`Z`, `p`/`P`, `x`/`X`, `y`/`Y` |
| `U=1` | Virtual placements shown through Unicode placeholder cells |

Not supported, and answered with the protocol's own `ENOTSUPP` report so a child that probes first
gets a clean answer rather than silence:

- **The protocol's animation frames** (`a=a`, `a=f`, `a=c`). A sender that animates by
  re-transmitting under the same image id — which is what `ratatui-image` does for GIFs — works
  regardless: the new pixels replace the old and the placeholders keep pointing at them.
- **Relative placements** (parent references).

Sixel input from the child is a separate protocol and is not read.

## Known limits

- **Reattaching to a session loses images drawn before the attach.** `export_replay_bytes` is a
  text replay stream and does not re-emit image payloads.
- **A partly visible image is cropped, not scaled.** That is what makes scrolling look right, but
  it also means an image wider than its pane shows its left part rather than shrinking to fit.
- **Encoding is asynchronous.** The first image has no pixels until its encode lands. Later updates
  keep the displayed frame in place while the replacement is encoded. Its transmission is emitted
  before its native placeholders in one paint, so the host switches only after it has the pixels.

## Testing

The half-block encoder is the fallback path, which makes images assertable without a real terminal:
render through `TestBackend` and read the colors back out of `capture_frame()`. See
`tests/terminal_images_render.rs`.
