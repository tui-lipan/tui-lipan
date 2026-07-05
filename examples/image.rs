use std::io::Cursor;
use std::sync::Arc;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
use tui_lipan::prelude::*;

struct ImageDemo;

struct State {
    sample_png: Arc<[u8]>,
    gandalf_path: Arc<str>,
    paused: bool,
    looped: bool,
    speed_percent: u16,
}

fn make_sample_png(width: u32, height: u32) -> Arc<[u8]> {
    let mut rgba = RgbaImage::new(width, height);

    for y in 0..height {
        for x in 0..width {
            let fx = x as f32 / width.max(1) as f32;
            let fy = y as f32 / height.max(1) as f32;
            let checker = ((x / 8 + y / 8) % 2) as u8;

            let r = (40.0 + 180.0 * fx) as u8;
            let g = (30.0 + 190.0 * fy) as u8;
            let b = if checker == 0 { 70 } else { 150 };

            rgba.put_pixel(x, y, Rgba([r, g, b, 255]));
        }
    }

    let image = DynamicImage::ImageRgba8(rgba);
    let mut out = Cursor::new(Vec::new());
    image
        .write_to(&mut out, ImageFormat::Png)
        .expect("sample image PNG encoding should succeed");
    out.into_inner().into()
}

impl Component for ImageDemo {
    type Message = ();
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        let gandalf_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("examples/assets/gandalf.gif")
            .to_string_lossy()
            .into_owned()
            .into();

        State {
            sample_png: make_sample_png(96, 56),
            gandalf_path,
            paused: false,
            looped: true,
            speed_percent: 100,
        }
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.mods.ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            ctx.quit();
            return KeyUpdate::handled(Update::none());
        }

        match key.code {
            KeyCode::Char(' ') => {
                ctx.state.paused = !ctx.state.paused;
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('l') | KeyCode::Char('L') => {
                ctx.state.looped = !ctx.state.looped;
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                ctx.state.speed_percent = ctx.state.speed_percent.saturating_add(25).min(400);
                return KeyUpdate::handled(Update::full());
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                ctx.state.speed_percent = ctx.state.speed_percent.saturating_sub(25).max(25);
                return KeyUpdate::handled(Update::full());
            }
            _ => {}
        }

        KeyUpdate::unhandled(Update::none())
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let sample = Arc::clone(&ctx.state.sample_png);
        let playback = if ctx.state.paused {
            ImagePlayback::Paused
        } else {
            ImagePlayback::Playing
        };
        let repeat = if ctx.state.looped {
            ImageRepeat::Loop
        } else {
            ImageRepeat::Once
        };

        Frame::new()
            .title("Image Widget")
            .status("Ctrl+Q quit | Space play/pause | L loop | +/- speed")
            .padding(1)
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new(format!(
                        "GIF controls: playback={:?} repeat={:?} speed={}%",
                        playback, repeat, ctx.state.speed_percent
                    )))
                    .child(
                        HStack::new()
                            .gap(1)
                            .height(Length::Flex(1))
                            .child(
                                Frame::new()
                                    .title("Static PNG")
                                    .width(Length::Flex(1))
                                    .padding(1)
                                    .child(
                                        Image::from_bytes(sample)
                                            .fit(ImageFit::Contain)
                                            .protocol(ImageProtocol::Auto)
                                            .width(Length::Flex(1))
                                            .height(Length::Flex(1)),
                                    ),
                            )
                            .child(
                                Frame::new()
                                    .title("Gandalf GIF")
                                    .width(Length::Flex(1))
                                    .padding(1)
                                    .child(
                                        Image::new(Arc::clone(&ctx.state.gandalf_path))
                                            .fit(ImageFit::Scale)
                                            .protocol(ImageProtocol::Auto)
                                            .playback(playback)
                                            .repeat(repeat)
                                            .speed_percent(ctx.state.speed_percent)
                                            .width(Length::Flex(1))
                                            .height(Length::Flex(1)),
                                    ),
                            )
                            .child(
                                Frame::new()
                                    .title("Decode Fallback")
                                    .width(Length::Flex(1))
                                    .padding(1)
                                    .child(
                                        Image::from_bytes(vec![1, 2, 3, 4])
                                            .alt("Could not decode image bytes")
                                            .style(Style::new().fg(Color::LightRed))
                                            .width(Length::Flex(1))
                                            .height(Length::Flex(1)),
                                    ),
                            ),
                    ),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new().mount(ImageDemo).run()
}
