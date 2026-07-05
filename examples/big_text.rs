use tui_lipan::prelude::*;

struct BigTextDemo;

impl Component for BigTextDemo {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn view(&self, _ctx: &Context<Self>) -> Element {
        let custom_font = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/fonts/custom.flf"),
        )
        .ok();
        let custom_figlet_example = if let Some(content) = custom_font {
            BigText::new()
                .text("Custom FIGlet")
                .custom_figlet(content)
                .style(Style::new().fg(Color::LightYellow))
        } else {
            BigText::new()
                .text("Custom FIGlet (missing examples/fonts/custom.flf)")
                .font(BigFont::Standard)
                .style(Style::new().fg(Color::DarkGray).dim())
        };

        // Gradient examples - built with the builder API and spliced into RSX.
        let grad_horizontal = BigText::new()
            .text("GRADIENT")
            .font(BigFont::PixelBold)
            .gradient(
                ColorGradient::new(Color::rgb(255, 30, 120), Color::rgb(30, 200, 255)),
                GradientDirection::Horizontal,
            );

        let grad_vertical = BigText::new()
            .text("VERTICAL")
            .font(BigFont::Colossal)
            .gradient(
                ColorGradient::new(Color::Yellow, Color::Red),
                GradientDirection::Vertical,
            );

        let grad_three_stop = BigText::new()
            .text("RAINBOW")
            .font(BigFont::AnsiShadow)
            .gradient(
                ColorGradient::new(Color::Red, Color::Blue).with_center(Color::Green),
                GradientDirection::Horizontal,
            );

        let grad_with_shadow = BigText::new().text("FIRE").font(BigFont::SubZero).gradient(
            ColorGradient::new(Color::Yellow, Color::rgb(200, 0, 0)),
            GradientDirection::Vertical,
        );

        rsx! {
            Frame {
                title: "Big Text Demo",
                ScrollView {
                    gap: 1,
                    padding: 2,
                    scrollbar: true,
                    scroll_keys: ScrollKeymap::DEFAULT,
                    BigText {
                        text: vec![
                            Span::new("TUI-").fg(Color::Cyan).bold(),
                            Span::new("LIPAN").fg(Color::Blue).bold(),
                        ],
                        font: BigFont::PixelBold,
                    },
                    BigText {
                        text: "TUI-LIPAN",
                        style: Style::new().fg(Color::Cyan).bold(),
                        shadow: Some(Shadow {
                            style: Style::new().fg(Color::DarkGray),
                            offset_x: 1,
                            offset_y: 1,
                        }),
                    },
                    BigText {
                        text: "ASCII ART",
                        style: Style::new().fg(Color::Green),
                        with_shadow: Style::new().fg(Color::Black),
                    },
                    BigText {
                        text: "PIXEL",
                        font: BigFont::Pixel,
                        style: Style::new().fg(Color::Yellow),
                        with_shadow: Style::new().fg(Color::DarkGray),
                    },
                    BigText {
                        text: "BOLD",
                        font: BigFont::PixelBold,
                        style: Style::new().fg(Color::LightRed),
                        with_shadow: Style::new().fg(Color::DarkGray),
                    },
                    BigText {
                        text: "QUADRANT",
                        font: BigFont::Quadrant,
                        style: Style::new().fg(Color::LightBlue),
                    },
                    Text {
                        content: "--- FIGlet Fonts ---",
                        style: Style::new().fg(Color::White).dim(),
                    },
                    custom_figlet_example,
                    BigText {
                        text: "Slant",
                        font: BigFont::Slant,
                        style: Style::new().fg(Color::Cyan),
                    },
                    BigText {
                        text: "Bloody",
                        font: BigFont::Bloody,
                        style: Style::new().fg(Color::Red),
                    },
                    BigText {
                        text: "COLOSSAL",
                        font: BigFont::Colossal,
                        style: Style::new().fg(Color::Yellow),
                    },
                    BigText {
                        text: "Roman",
                        font: BigFont::Roman,
                        style: Style::new().fg(Color::Magenta),
                    },
                    BigText {
                        text: "SUB-ZERO",
                        font: BigFont::SubZero,
                        style: Style::new().fg(Color::LightCyan),
                    },
                    BigText {
                        text: "Poison",
                        font: BigFont::Poison,
                        style: Style::new().fg(Color::Green),
                    },
                    BigText {
                        text: "Nancyj",
                        font: BigFont::Nancyj,
                        style: Style::new().fg(Color::LightMagenta),
                    },
                    BigText {
                        text: "Small Poison",
                        font: BigFont::SmallPoison,
                        style: Style::new().fg(Color::LightGreen),
                    },
                    BigText {
                        text: "DOS Rebel",
                        font: BigFont::DosRebel,
                        style: Style::new().fg(Color::LightRed),
                    },
                    BigText {
                        text: "ANSI Shadow",
                        font: BigFont::AnsiShadow,
                        style: Style::new().fg(Color::LightBlue),
                    },
                    BigText {
                        text: "Small Font",
                        font: BigFont::Small,
                        style: Style::new().fg(Color::White),
                    },
                    BigText {
                        text: "Shadows!",
                        style: Style::new().fg(Color::Magenta),
                        shadow: Some(Shadow {
                            style: Style::new().fg(Color::White).dim(),
                            offset_x: 2,
                            offset_y: 2,
                        }),
                    },
                    Text {
                        content: "--- Gradients ---",
                        style: Style::new().fg(Color::White).dim(),
                    },
                    grad_horizontal,
                    grad_vertical,
                    grad_three_stop,
                    grad_with_shadow,
                },
            }
        }
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }
}

fn main() -> Result<()> {
    App::new().mount(BigTextDemo).run()
}
