//! Host-visible regression scene for native Kitty image layers.
//!
//! Run with `cargo run --example terminal_patch_layers --features terminal-images`.
//! `t` shows opaque text, `b` a filled frame, `n` removes the overlay, and `q` quits.
//! Compare with `TUI_LIPAN_TERMINAL_IMAGES=composed`; resize the host in both modes.
use base64::Engine as _;
use tui_lipan::prelude::*;

struct PatchLayers;
struct State {
    snapshot: TerminalRenderSnapshot,
    overlay: char,
}

impl Component for PatchLayers {
    type Message = char;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> State {
        let cell = host_cell_size();
        let mut screen = TerminalScreen::new(12, 40, 0);
        screen.set_cell_size(cell);
        screen.process_bytes(b"\x1b[?25l\x1b[H");
        let (width, height) = (u32::from(cell.width) * 24, u32::from(cell.height) * 8);
        let pixels = [40, 190, 90, 255].repeat((width * height) as usize);
        let payload = base64::engine::general_purpose::STANDARD.encode(pixels);
        screen.process_bytes(
            format!("\x1b_Ga=T,f=32,s={width},v={height},i=1,z=1;{payload}\x1b\\").as_bytes(),
        );
        State {
            snapshot: screen.render_snapshot(),
            overlay: std::env::var("TUI_LIPAN_PATCH_LAYER")
                .ok()
                .and_then(|value| value.chars().next())
                .unwrap_or('n'),
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let KeyCode::Char(key @ ('t' | 'b' | 'n' | 'q')) = key.code else {
            return KeyUpdate::unhandled(Update::none());
        };
        ctx.link().send(key);
        KeyUpdate::handled(Update::full())
    }

    fn update(&mut self, key: char, ctx: &mut Context<Self>) -> Update {
        if key == 'q' {
            ctx.quit();
            return Update::none();
        }
        ctx.state.overlay = key;
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let mut canvas = Canvas::new().child_at(
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            Terminal::new()
                .snapshot(ctx.state.snapshot.clone())
                .focusable(false)
                .scrollbar(false),
        );
        let style = Style::default().fg(Color::White).bg(Color::Red);
        let overlay: Option<Element> = match ctx.state.overlay {
            't' => Some(Text::new("OPAQUE TEXT").style(style).into()),
            'b' => Some(
                Frame::new()
                    .border(false)
                    .padding(0)
                    .style(style)
                    .child(Text::new("OPAQUE BLOCK"))
                    .into(),
            ),
            _ => None,
        };
        if let Some(overlay) = overlay {
            canvas = canvas.child_at(
                Rect {
                    x: 5,
                    y: 2,
                    w: 14,
                    h: 3,
                },
                overlay,
            );
        }
        VStack::new()
            .child(Text::new("t: text | b: block | n: image only | q: quit"))
            .child(canvas.height(Length::Px(12)))
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("Native patch layers")
        .mount(PatchLayers)
        .run()
}
