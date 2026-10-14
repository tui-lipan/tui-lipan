//! Host-visible regression scene for native Kitty image layers.
//!
//! Run with `cargo run --example terminal_patch_layers --features terminal-images`.
//! `t` shows opaque text, `b` a filled frame, `m` a transparent modal, `n` removes the
//! overlay, `p` hosts the terminal in a portal, `u` replaces the live source, and `q` quits.
//! Compare with `TUI_LIPAN_TERMINAL_IMAGES=composed`; resize the host in both modes.
use std::cell::RefCell;
use std::rc::Rc;

use base64::Engine as _;
use tui_lipan::prelude::*;

struct PatchLayers;
struct State {
    screen: Rc<RefCell<TerminalScreen>>,
    blue: bool,
    cell: TerminalCellSize,
    overlay: char,
}

impl Component for PatchLayers {
    type Message = char;
    type Properties = ();
    type State = State;

    fn create_state(&self, _: &()) -> State {
        let cell = host_cell_size();
        State {
            screen: Rc::new(RefCell::new(image_screen(cell))),
            blue: false,
            cell,
            overlay: std::env::var("TUI_LIPAN_PATCH_LAYER")
                .ok()
                .and_then(|value| value.chars().next())
                .unwrap_or('n'),
        }
    }

    fn init(&mut self, ctx: &mut Context<Self>) -> Option<Command> {
        ctx.link().send('r');
        None
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let KeyCode::Char(key @ ('t' | 'b' | 'm' | 'n' | 'p' | 'u' | 'q')) = key.code else {
            return KeyUpdate::unhandled(Update::none());
        };
        ctx.link().send(key);
        KeyUpdate::handled(Update::none())
    }

    fn update(&mut self, key: char, ctx: &mut Context<Self>) -> Update {
        if key == 'q' {
            ctx.quit();
            return Update::none();
        }
        if key == 'u' {
            ctx.state.blue = !ctx.state.blue;
            replace_image(
                &mut ctx.state.screen.borrow_mut(),
                ctx.state.cell,
                ctx.state.blue,
            );
            return Update::terminal_graphics();
        }
        if key == 'r' {
            let cell = host_cell_size();
            if cell == ctx.state.cell {
                return Update::none();
            }
            ctx.state.cell = cell;
            ctx.state.screen.borrow_mut().set_cell_size(cell);
            replace_image(&mut ctx.state.screen.borrow_mut(), cell, ctx.state.blue);
        } else {
            ctx.state.overlay = key;
        }
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
                .screen(Rc::clone(&ctx.state.screen))
                .focusable(false)
                .scrollbar(false)
                .on_resize(ctx.link().callback(|_: TerminalViewport| 'r')),
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
        // Multiplexers keep an empty popup host mounted above their panes.
        canvas = canvas.child_at(
            Rect {
                x: 0,
                y: 0,
                w: 40,
                h: 12,
            },
            Animated::new(Canvas::new()),
        );
        if ctx.state.overlay == 'p' {
            return VStack::new()
                .child(Text::new("p: terminal in portal | n: image | q: quit"))
                .child(
                    Modal::new()
                        .width(Length::Px(40))
                        .height(Length::Px(12))
                        .border(false)
                        .padding(0)
                        .frame_style(Style::default().bg(Color::Transparent))
                        .child(canvas),
                )
                .into();
        }
        let scene: Element = VStack::new()
            .child(Text::new(
                "t: text | b: block | m: modal | p: portal | n: image | u: replace | q: quit",
            ))
            .child(canvas.height(Length::Px(12)))
            .into();
        if ctx.state.overlay == 'm' {
            ZStack::new()
                .child(scene)
                .child(
                    Modal::new()
                        .width(Length::Percent(100))
                        .height(Length::Percent(100))
                        .border(false)
                        .padding(0)
                        .frame_style(Style::default().bg(Color::Transparent))
                        .child(Canvas::new().child_at(
                            Rect {
                                x: 5,
                                y: 3,
                                w: 16,
                                h: 1,
                            },
                            Text::new("TRANSPARENT TEXT").style(Style::default().fg(Color::White)),
                        )),
                )
                .into()
        } else {
            scene
        }
    }
}

// State is created before host probing. The first viewport callback refreshes this scene with
// measured cells; subsequent font zooms do the same without changing the selected overlay.
fn image_screen(cell: TerminalCellSize) -> TerminalScreen {
    let mut screen = TerminalScreen::new(12, 40, 0);
    screen.set_cell_size(cell);
    screen.process_bytes(b"\x1b[?25l\x1b[H");
    replace_image(&mut screen, cell, false);
    screen
}

fn replace_image(screen: &mut TerminalScreen, cell: TerminalCellSize, blue: bool) {
    let (width, height) = (u32::from(cell.width) * 24, u32::from(cell.height) * 6);
    let color = if blue {
        [40, 90, 190, 255]
    } else {
        [40, 190, 90, 255]
    };
    let pixels = color.repeat((width * height) as usize);
    let payload = base64::engine::general_purpose::STANDARD.encode(pixels);
    screen.process_bytes(
        format!("\x1b_Ga=T,f=32,s={width},v={height},i=1,p=9,z=2,C=1,q=2;{payload}\x1b\\")
            .as_bytes(),
    );
}

fn main() -> Result<()> {
    App::new()
        .title("Native patch layers")
        .mount(PatchLayers)
        .run()
}
