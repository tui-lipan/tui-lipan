//! Images inside a terminal pane, drawn by Kitty graphics escapes in the child's output.
//!
//! Run with:
//!   cargo run --example terminal_images --features terminal-images
//!
//! This feeds a [`TerminalScreen`] the same `APC _G` escapes a real program emits, so no external
//! tool is needed to see the path work end to end. A pane driven by a PTY behaves identically:
//! `kitty +kitten icat`, `timg -pk`, and `chafa -f kitty` all probe for graphics support, get an
//! answer, and draw.
//!
//! What the *host* terminal speaks does not matter. The pane decodes the child's escapes into
//! pixels and re-encodes them for whatever the host does support, down to half-blocks - so this
//! renders in a plain xterm as well as in Kitty.
//!
//! Keys: `g` draws a gradient, `p` draws a second one beside it, `c` clears, `q` quits.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tui_lipan::prelude::*;

/// Starting size only. The screen is resized to the widget as soon as it reports a viewport, the
/// way a PTY-backed pane tracks its own geometry - without that the screen stays this tall no
/// matter how big the window is, and images stop appearing once these rows are used up.
const ROWS: u16 = 20;
const COLS: u16 = 92;

struct TerminalImages;

struct State {
    screen: TerminalScreen,
    snapshot: TerminalRenderSnapshot,
    cell: TerminalCellSize,
    cols: u16,
    rows: u16,
    drawn: usize,
}

#[derive(Clone)]
enum Msg {
    Gradient,
    Plot,
    Clear,
    Quit,
    Resize(u16, u16),
}

impl Component for TerminalImages {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let cell = host_cell_size();
        let mut screen = TerminalScreen::new(ROWS, COLS, 500);
        // Size placements against the host's real cell. A pane driven by a PTY passes the same
        // value to `TerminalPtyConfig::cell_size`, so the child's own arithmetic agrees with this.
        screen.set_cell_size(cell);
        screen.process_bytes(
            b"\x1b[1;36m$ demo --draw\x1b[0m\r\n\
              press g for a gradient, p for a plot, c to clear\r\n",
        );

        let snapshot = screen.render_snapshot();
        Self::State {
            screen,
            snapshot,
            cell,
            cols: COLS,
            rows: ROWS,
            drawn: 0,
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let msg = match key.code {
            KeyCode::Char('g') => Msg::Gradient,
            KeyCode::Char('p') => Msg::Plot,
            KeyCode::Char('c') => Msg::Clear,
            KeyCode::Char('q') | KeyCode::Esc => Msg::Quit,
            _ => return KeyUpdate::unhandled(Update::none()),
        };
        ctx.link().send(msg);
        KeyUpdate::handled(Update::full())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        let cell = ctx.state.cell;
        match msg {
            Msg::Gradient => {
                let id = 1 + ctx.state.drawn as u32;
                ctx.state.drawn += 1;
                let image = gradient(28, 8, cell);
                ctx.state
                    .screen
                    .process_bytes(&transmit_and_display(id, &image));
                ctx.state.screen.process_bytes(b"\r\n");
            }
            Msg::Plot => {
                let id = 1 + ctx.state.drawn as u32;
                ctx.state.drawn += 1;
                let image = plot(28, 8, cell);
                ctx.state
                    .screen
                    .process_bytes(&transmit_and_display(id, &image));
                ctx.state.screen.process_bytes(b"\r\n");
            }
            // `a=d,d=A` deletes every placement and frees the pixels behind them.
            Msg::Clear => {
                ctx.state
                    .screen
                    .process_bytes(b"\x1b_Ga=d,d=A;\x1b\\\x1b[2J\x1b[H");
                ctx.state.drawn = 0;
            }
            Msg::Resize(cols, rows) => {
                if (cols, rows) == (ctx.state.cols, ctx.state.rows) {
                    return Update::none();
                }
                ctx.state.cols = cols;
                ctx.state.rows = rows;
                ctx.state.screen.resize(rows, cols);
            }
            Msg::Quit => {
                ctx.quit();
                return Update::none();
            }
        }
        ctx.state.snapshot = ctx.state.screen.render_snapshot();
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let footer = Text::from_spans([
            Span::new("g").fg(Color::Yellow).bold(),
            Span::new(" gradient   ").fg(Color::DarkGray),
            Span::new("p").fg(Color::Yellow).bold(),
            Span::new(" plot   ").fg(Color::DarkGray),
            Span::new("c").fg(Color::Yellow).bold(),
            Span::new(" clear   ").fg(Color::DarkGray),
            Span::new("q").fg(Color::Yellow).bold(),
            Span::new(" quit").fg(Color::DarkGray),
        ]);

        VStack::new()
            .child(
                Frame::new()
                    .header(BorderLabels::new().center(FrameLabel::new(" kitty graphics ")))
                    .child(
                        Terminal::new()
                            .snapshot(ctx.state.snapshot.clone())
                            .focusable(false)
                            .on_resize(ctx.link().callback(|viewport: TerminalViewport| {
                                Msg::Resize(viewport.cols, viewport.rows)
                            }))
                            .width(Length::Flex(1))
                            .height(Length::Flex(1)),
                    )
                    .width(Length::Flex(1))
                    .height(Length::Flex(1)),
            )
            .child(footer)
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .into()
    }
}

/// Raw RGB pixels plus their dimensions.
struct Rgb {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// Wrap raw pixels in a transmit-and-display command, the way `icat` does.
///
/// Chunked at the 4096 base64 bytes the protocol asks for: only the first chunk carries the keys,
/// every one but the last sets `m=1`, and the terminal reassembles them before decoding. A picture
/// this size is several chunks, which is exactly what a real sender would produce.
fn transmit_and_display(id: u32, image: &Rgb) -> Vec<u8> {
    const CHUNK: usize = 4096;

    let Rgb {
        width,
        height,
        data,
    } = image;
    let payload = BASE64.encode(data);
    let mut out = Vec::new();

    let chunks: Vec<&str> = payload
        .as_bytes()
        .chunks(CHUNK)
        .map(|chunk| std::str::from_utf8(chunk).expect("base64 is ascii"))
        .collect();

    for (index, chunk) in chunks.iter().enumerate() {
        let more = u8::from(index + 1 < chunks.len());
        let keys = if index == 0 {
            format!("a=T,f=24,s={width},v={height},t=d,i={id},")
        } else {
            String::new()
        };
        out.extend_from_slice(format!("\x1b_G{keys}m={more};{chunk}\x1b\\").as_bytes());
    }
    out
}

fn canvas(cols: u32, rows: u32, cell: TerminalCellSize) -> Rgb {
    let width = cols * u32::from(cell.width);
    let height = rows * u32::from(cell.height);
    Rgb {
        width,
        height,
        data: vec![0; (width * height * 3) as usize],
    }
}

impl Rgb {
    fn set(&mut self, x: u32, y: u32, rgb: [u8; 3]) {
        if x >= self.width || y >= self.height {
            return;
        }
        let at = ((y * self.width + x) * 3) as usize;
        self.data[at..at + 3].copy_from_slice(&rgb);
    }
}

fn gradient(cols: u32, rows: u32, cell: TerminalCellSize) -> Rgb {
    let mut image = canvas(cols, rows, cell);
    for y in 0..image.height {
        for x in 0..image.width {
            let r = (x * 255 / image.width.max(1)) as u8;
            let g = (y * 255 / image.height.max(1)) as u8;
            image.set(x, y, [r, g, 170]);
        }
    }
    image
}

fn plot(cols: u32, rows: u32, cell: TerminalCellSize) -> Rgb {
    let mut image = canvas(cols, rows, cell);
    let (w, h) = (image.width, image.height);
    for x in 0..w {
        let phase = x as f32 / w.max(1) as f32 * std::f32::consts::TAU * 2.0;
        let y = ((phase.sin() * 0.4 + 0.5) * h as f32) as u32;
        for thickness in 0..3 {
            image.set(x, y + thickness, [90, 220, 160]);
        }
    }
    for x in (0..w).step_by(20) {
        for y in 0..h {
            image.set(x, y, [40, 40, 55]);
        }
    }
    image
}

fn main() -> Result<()> {
    App::new()
        .title("Terminal images")
        .mount(TerminalImages)
        .run()
}
