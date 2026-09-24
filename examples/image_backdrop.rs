//! Images dim with the cells around them under a modal backdrop, an `EffectScope`, or an `Animated`
//! fade.
//!
//! Run with:
//!   cargo run --example image_backdrop --features terminal-images
//!
//! The left pane is a terminal whose child drew a gradient through Kitty graphics escapes, under a
//! row of truecolor cells painted in the same colors. The right pane is an `Image` widget with the
//! same gradient and the same row. Open the modal and every picture should dim to exactly the color
//! of the cells beside it; close it and they return to full brightness at once, from the cached
//! undimmed encode. The layer key swaps the root modal for a `Local`-scope one, for an
//! `EffectScope` around both panes, or for an `Animated` fade of both panes toward a dark color.
//!
//! Try it in a host that draws real pixels (Kitty, WezTerm, Ghostty, iTerm2, a sixel terminal).
//! In a plain xterm images fall back to half blocks, which are cells and always dimmed.
//!
//! The app queries the host's color palette, so named colors such as the hint row's `Cyan` dim
//! from the RGB your terminal theme gives them rather than the standard ANSI values.
//!
//! Keys: `m` opens and closes the layer, `l` cycles the layer, `s` cycles the backdrop style,
//! `q` quits.

use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use tui_lipan::prelude::*;

const ROWS: u16 = 16;
const COLS: u16 = 60;
const IMAGE_COLS: u32 = 32;
const IMAGE_ROWS: u32 = 8;

/// The backdrops to compare, with the label shown for each.
fn backdrops() -> [(&'static str, Style); 4] {
    [
        (
            "tint_by(rgb(0, 0, 40), 0.5)",
            Style::new().tint_by(Color::Rgb(0, 0, 40), 0.5),
        ),
        ("dim_by(0.6)", Style::new().dim_by(0.6)),
        (
            "transform_bg(Elevate(0.5))",
            Style::new().transform_bg(ColorTransform::Elevate(0.5)),
        ),
        ("no backdrop", Style::default()),
    ]
}

/// What dims the panes when open.
#[derive(Clone, Copy)]
enum Layer {
    RootModal,
    LocalModal,
    Scope,
    Fade,
}

impl Layer {
    const ALL: [Self; 4] = [Self::RootModal, Self::LocalModal, Self::Scope, Self::Fade];

    fn label(self) -> &'static str {
        match self {
            Self::RootModal => "root modal",
            Self::LocalModal => "local modal",
            Self::Scope => "EffectScope",
            Self::Fade => "Animated fade to 0.4",
        }
    }
}

/// The fade's target, the dark surface a dialog recedes toward.
const FADE_TARGET: Color = Color::Rgb(0, 0, 40);

struct ImageBackdrop;

struct State {
    snapshot: TerminalRenderSnapshot,
    picture: Arc<[u8]>,
    open: bool,
    style: usize,
    layer: usize,
}

#[derive(Clone)]
enum Msg {
    Toggle,
    Close,
    NextStyle,
    NextLayer,
    Quit,
}

impl Component for ImageBackdrop {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let (snapshot, picture) = panes(host_cell_size());
        Self::State {
            snapshot,
            picture,
            open: false,
            style: 0,
            layer: 0,
        }
    }

    /// Draw the pictures again at the host's real cell size.
    ///
    /// The root's state is created before the app enters the terminal, where `host_cell_size` can
    /// only guess. Pictures drawn at a guessed cell cover a box of a different shape than the cells
    /// the panes lay out, and the host has to fit them into it.
    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        (ctx.state.snapshot, ctx.state.picture) = panes(host_cell_size());
        None
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let msg = match key.code {
            KeyCode::Char('m') => Msg::Toggle,
            KeyCode::Char('s') => Msg::NextStyle,
            KeyCode::Char('l') => Msg::NextLayer,
            KeyCode::Char('q') => Msg::Quit,
            _ => return KeyUpdate::unhandled(Update::none()),
        };
        ctx.link().send(msg);
        KeyUpdate::handled(Update::none())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Toggle => ctx.state.open = !ctx.state.open,
            Msg::Close => ctx.state.open = false,
            Msg::NextStyle => ctx.state.style = (ctx.state.style + 1) % backdrops().len(),
            Msg::NextLayer => ctx.state.layer = (ctx.state.layer + 1) % Layer::ALL.len(),
            Msg::Quit => {
                ctx.quit();
                return Update::none();
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let (label, backdrop) = backdrops()[ctx.state.style];
        let layer = Layer::ALL[ctx.state.layer];
        let open = ctx.state.open;
        let footer = Text::from_spans([
            Span::new("m").fg(Color::Yellow).bold(),
            Span::new(if open { " close   " } else { " open   " }).fg(Color::DarkGray),
            Span::new("l").fg(Color::Yellow).bold(),
            Span::new(" layer: ").fg(Color::DarkGray),
            Span::new(layer.label()).fg(Color::Cyan),
            Span::new("   s").fg(Color::Yellow).bold(),
            Span::new(" style: ").fg(Color::DarkGray),
            Span::new(label).fg(Color::Cyan),
            Span::new("   q").fg(Color::Yellow).bold(),
            Span::new(" quit").fg(Color::DarkGray),
        ]);

        let terminal = Frame::new()
            .header(BorderLabels::new().center(FrameLabel::new(" terminal pane ")))
            .child(
                Terminal::new()
                    .snapshot(ctx.state.snapshot.clone())
                    .focusable(false)
                    .scrollbar(false)
                    .width(Length::Flex(1))
                    .height(Length::Flex(1)),
            )
            .width(Length::Flex(1))
            .height(Length::Flex(1));

        let widget = Frame::new()
            .header(BorderLabels::new().center(FrameLabel::new(" Image widget ")))
            .child(
                VStack::new()
                    .child(
                        Text::new("truecolor cells in the image's colors:")
                            .style(Style::new().dim()),
                    )
                    .child(Text::from_spans(swatch_spans()))
                    .child(Text::new(" "))
                    .child(
                        Image::from_bytes(Arc::clone(&ctx.state.picture))
                            .fit(ImageFit::Contain)
                            .height(Length::Px(IMAGE_ROWS as u16)),
                    ),
            )
            .width(Length::Flex(1))
            .height(Length::Flex(1));

        let mut scope = EffectScope::new();
        if open && matches!(layer, Layer::Scope) {
            if let Some((color, alpha)) = backdrop.tint {
                scope = scope.tint_by(color, alpha);
            }
            if let Some(amount) = backdrop.dim_amount {
                scope = scope.dim_by(amount);
            }
            if let Some(transform) = backdrop.bg_transform {
                scope = scope.transform_bg(transform);
            }
        }
        let fading = open && matches!(layer, Layer::Fade);
        let panes = Animated::new(scope.child(HStack::new().child(terminal).child(widget)))
            .opacity(if fading { 0.4 } else { 1.0 })
            .opacity_target(FADE_TARGET);

        let mut root = ZStack::new().child(VStack::new().child(panes).child(footer));
        let scope = match layer {
            Layer::RootModal => Some(OverlayScope::RootPortal),
            Layer::LocalModal => Some(OverlayScope::Local),
            Layer::Scope | Layer::Fade => None,
        };
        if let Some(scope) = scope.filter(|_| open) {
            root = root.child(
                Modal::new()
                    .scope(scope)
                    .title("Backdrop")
                    .width(Length::Px(34))
                    .height(Length::Auto)
                    .backdrop_style(backdrop)
                    .on_close(ctx.link().callback(|_| Msg::Close))
                    .child(
                        VStack::new()
                            .child(Text::new(format!(
                                "{label}\n\nThe pictures behind should dim\nlike the cells above them.\n"
                            )))
                            .child(
                                HStack::new()
                                    .gap(1)
                                    .child(
                                        Button::new("Style (s)")
                                            .on_click(ctx.link().callback(|_| Msg::NextStyle)),
                                    )
                                    .child(
                                        Button::new("Close (m)")
                                            .on_click(ctx.link().callback(|_| Msg::Close)),
                                    ),
                            ),
                    ),
            );
        }
        root.into()
    }
}

/// The terminal pane's screen and the `Image` widget's picture, drawn for `cell`.
fn panes(cell: TerminalCellSize) -> (TerminalRenderSnapshot, Arc<[u8]>) {
    let mut screen = TerminalScreen::new(ROWS, COLS, 100);
    screen.set_cell_size(cell);
    screen.process_bytes(b"\x1b[2mtruecolor cells in the image's colors:\x1b[0m\r\n");
    screen.process_bytes(&swatch_row());
    screen.process_bytes(b"\r\n\x1b[1;36m$ icat gradient.png\x1b[0m\r\n");
    screen.process_bytes(&transmit_and_display(1, &gradient(cell)));
    (screen.render_snapshot(), png_bytes(cell))
}

/// The gradient's color at a fraction of its width.
fn gradient_color(t: f32) -> [u8; 3] {
    let t = t.clamp(0.0, 1.0);
    [(255.0 * t) as u8, (60.0 + 140.0 * (1.0 - t)) as u8, 170]
}

/// A horizontal gradient, `IMAGE_COLS` x `IMAGE_ROWS` cells, as raw RGB.
fn gradient(cell: TerminalCellSize) -> (u32, u32, Vec<u8>) {
    let width = IMAGE_COLS * u32::from(cell.width);
    let height = IMAGE_ROWS * u32::from(cell.height);
    let mut data = Vec::with_capacity((width * height * 3) as usize);
    for _ in 0..height {
        for x in 0..width {
            data.extend_from_slice(&gradient_color(x as f32 / width.max(1) as f32));
        }
    }
    (width, height, data)
}

/// The same gradient, encoded as a PNG for the `Image` widget.
fn png_bytes(cell: TerminalCellSize) -> Arc<[u8]> {
    let (width, height, data) = gradient(cell);
    let image = image::RgbImage::from_raw(width, height, data).expect("pixel buffer size");
    let mut bytes = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("PNG encoding");
    bytes.into_inner().into()
}

/// The color of each image column's middle, one cell per column.
fn swatch_colors() -> impl Iterator<Item = [u8; 3]> {
    (0..IMAGE_COLS).map(|col| gradient_color((col as f32 + 0.5) / IMAGE_COLS as f32))
}

/// A row of truecolor cells, as SGR escapes for the terminal pane.
fn swatch_row() -> Vec<u8> {
    let mut out = String::new();
    for [r, g, b] in swatch_colors() {
        out.push_str(&format!("\x1b[48;2;{r};{g};{b}m "));
    }
    out.push_str("\x1b[0m");
    out.into_bytes()
}

/// The same row as styled spans for the widget pane.
fn swatch_spans() -> Vec<Span> {
    swatch_colors()
        .map(|[r, g, b]| Span::new(" ").bg(Color::Rgb(r, g, b)))
        .collect()
}

/// Wrap raw pixels in a chunked transmit-and-display command, the way `icat` does.
fn transmit_and_display(id: u32, (width, height, data): &(u32, u32, Vec<u8>)) -> Vec<u8> {
    const CHUNK: usize = 4096;

    let payload = BASE64.encode(data);
    let chunks: Vec<&[u8]> = payload.as_bytes().chunks(CHUNK).collect();
    let mut out = Vec::new();
    for (index, chunk) in chunks.iter().enumerate() {
        let more = u8::from(index + 1 < chunks.len());
        let keys = if index == 0 {
            format!("a=T,f=24,s={width},v={height},t=d,i={id},")
        } else {
            String::new()
        };
        out.extend_from_slice(format!("\x1b_G{keys}m={more};").as_bytes());
        out.extend_from_slice(chunk);
        out.extend_from_slice(b"\x1b\\");
    }
    out
}

fn main() -> Result<()> {
    App::new()
        .title("Image backdrop")
        .live_host_terminal_colors(true)
        .mount(ImageBackdrop)
        .run()
}
