//! Color contrast showcase - demonstrates automatic text readability.
//!
//! Run with: cargo run --example color_contrast
//!
//! Controls:
//! - Ctrl+Q: Quit
//!
//! Shows the same text (Black foreground) on various backgrounds,
//! side-by-side: raw colors on the left, auto-adjusted on the right.

use tui_lipan::prelude::*;
use tui_lipan::utils::color_contrast::{black_or_white, contrast_ratio, readable_text_color};

struct ContrastDemo;

#[derive(Clone, Debug)]
enum Msg {}

/// Background swatches to demonstrate.
const SWATCHES: &[(&str, Color)] = &[
    ("Black", Color::Black),
    ("Red", Color::Red),
    ("Green", Color::Green),
    ("Yellow", Color::Yellow),
    ("Blue", Color::Blue),
    ("Magenta", Color::Magenta),
    ("Cyan", Color::Cyan),
    ("White", Color::White),
    ("DarkGray", Color::DarkGray),
    ("LightBlue", Color::LightBlue),
    ("Navy", Color::Rgb(0, 0, 128)),
    ("Teal", Color::Rgb(0, 128, 128)),
];

/// A single swatch cell.
fn swatch(label: String, bg: Color, fg: Color) -> Element {
    let ratio = contrast_ratio(fg, bg);
    let ratio_label = format!("{ratio:.1}:1");

    Frame::new()
        .border(true)
        .border_style(BorderStyle::Rounded)
        .width(Length::Flex(1))
        .height(Length::Px(4))
        .style(Style::new().bg(bg).fg(fg))
        .child(
            VStack::new()
                .padding((1, 0))
                .child(Text::new(label).style(Style::new().fg(fg).bg(bg).bold()))
                .child(Text::new(ratio_label).style(Style::new().fg(fg).bg(bg).dim())),
        )
        .into()
}

/// Build a row of swatches from a chunk.
fn swatch_row(chunk: &[(&str, Color)], apply_contrast: bool) -> Element {
    let preferred = Color::Black;
    let mut row = HStack::new().gap(1);
    for &(label, bg) in chunk {
        let fg = if apply_contrast {
            readable_text_color(Some(preferred), bg)
        } else {
            preferred
        };
        row = row.child(swatch(label.to_owned(), bg, fg));
    }
    row.into()
}

impl Component for ContrastDemo {
    type Message = Msg;
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.mods.ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }
        KeyUpdate::unhandled(Update::none())
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        // --- Section 1: Side-by-side comparison ---
        let raw_rows: Vec<Element> = SWATCHES
            .chunks(4)
            .map(|chunk| swatch_row(chunk, false))
            .collect();

        let adjusted_rows: Vec<Element> = SWATCHES
            .chunks(4)
            .map(|chunk| swatch_row(chunk, true))
            .collect();

        let raw_panel = {
            let mut stack = VStack::new().gap(1);
            for row in raw_rows {
                stack = stack.child(row);
            }
            Frame::new()
                .title("Raw: Black fg on every background")
                .border(true)
                .border_style(BorderStyle::Rounded)
                .padding(1)
                .width(Length::Flex(1))
                .child(stack)
        };

        let adjusted_panel = {
            let mut stack = VStack::new().gap(1);
            for row in adjusted_rows {
                stack = stack.child(row);
            }
            Frame::new()
                .title("Auto-adjusted for WCAG AA (4.5:1)")
                .border(true)
                .border_style(BorderStyle::Rounded)
                .padding(1)
                .width(Length::Flex(1))
                .child(stack)
        };

        let comparison = HStack::new().gap(1).child(raw_panel).child(adjusted_panel);

        // --- Section 2: The "black on blue" regression ---
        let blues: Vec<(&str, Color)> = vec![
            ("ANSI Blue", Color::Blue),
            ("RGB(0,0,238)", Color::Rgb(0, 0, 238)),
            ("RGB(0,0,128)", Color::Rgb(0, 0, 128)),
            ("RGB(0,0,60)", Color::Rgb(0, 0, 60)),
        ];

        let mut blue_row = HStack::new().gap(1);
        for (label, bg) in &blues {
            let raw_fg = Color::Black;
            let adj_fg = readable_text_color(Some(raw_fg), *bg);
            let auto_label = if let Color::Rgb(r, g, b) = adj_fg {
                format!("adjusted → rgb({r},{g},{b})")
            } else {
                format!("adjusted → {adj_fg:?}")
            };

            let cell: Element = Frame::new()
                .border(true)
                .border_style(BorderStyle::Rounded)
                .width(Length::Flex(1))
                .height(Length::Px(5))
                .style(Style::new().bg(*bg))
                .child(
                    VStack::new()
                        .padding((1, 0))
                        .child(
                            Text::new(label.to_string())
                                .style(Style::new().fg(adj_fg).bg(*bg).bold()),
                        )
                        .child(Text::new(auto_label).style(Style::new().fg(adj_fg).bg(*bg))),
                )
                .into();
            blue_row = blue_row.child(cell);
        }

        let blue_section = Frame::new()
            .title("Regression: Black text on blue backgrounds")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(blue_row);

        // --- Section 3: black_or_white() showcase ---
        let spectrum: Vec<(String, Color)> = (0..12)
            .map(|i| {
                let v = (i * 255 / 11) as u8;
                (format!("gray({v})"), Color::Rgb(v, v, v))
            })
            .collect();

        let mut bw_row = HStack::new().gap(0);
        for (label, bg) in &spectrum {
            let fg = black_or_white(*bg);
            let cell: Element = Frame::new()
                .width(Length::Flex(1))
                .height(Length::Px(3))
                .style(Style::new().bg(*bg))
                .child(
                    Center::new()
                        .child(Text::new(label.clone()).style(Style::new().fg(fg).bg(*bg))),
                )
                .into();
            bw_row = bw_row.child(cell);
        }

        let bw_section = Frame::new()
            .title("black_or_white() on grayscale spectrum")
            .border(true)
            .border_style(BorderStyle::Rounded)
            .padding(1)
            .child(bw_row);

        // --- Assemble ---
        VStack::new()
            .padding(1)
            .gap(1)
            .child(Text::new("Color Contrast Showcase").style(Style::new().bold()))
            .child(Text::new(
                "Demonstrates WCAG 2.1 contrast adjustment: readable_text_color() \
                 and black_or_white(). Ctrl+Q to quit.",
            ))
            .child(comparison)
            .child(blue_section)
            .child(bw_section)
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Color Contrast")
        .mount(ContrastDemo)
        .run()
}
