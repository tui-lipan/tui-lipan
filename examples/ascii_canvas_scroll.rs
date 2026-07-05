//! AsciiCanvas widgets inside a ScrollView - tests row-skip clipping and
//! demonstrates color gradient rendering on both cell-grid and text-line canvases.
//!
//! Controls: ↑/↓ or j/k to scroll, mouse wheel, q to quit.
//! Run with: cargo run --example ascii_canvas_scroll

use tui_lipan::prelude::*;

struct AsciiCanvasScroll;

impl Component for AsciiCanvasScroll {
    type Message = ();
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                KeyUpdate::handled(Update::none())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Element {
        // Canvas 1: checkerboard - cell grid mode (56×18)
        let checkerboard = AsciiCanvas::with_cell_fn(56, 18, |x, y| {
            if (x + y) % 2 == 0 {
                AsciiCell::new('█').style(Style::new().fg(Color::Cyan))
            } else {
                AsciiCell::new('░').style(Style::new().fg(Color::Blue).dim())
            }
        });

        // Canvas 2: row-number labels - cell grid mode (56×22).
        // Each row displays its own index. When you scroll this canvas
        // partially out of view the visible row numbers must still match
        // the rows actually drawn, confirming that row-skip is working.
        let row_grid = AsciiCanvas::with_cell_fn(56, 22, |x, y| {
            let row_label = format!("row {:02}: ", y); // "row 00: " = 8 chars
            let ch = if (x as usize) < row_label.len() {
                row_label.chars().nth(x as usize).unwrap_or(' ')
            } else {
                // Repeating fill after the label
                let fill_x = x as usize - row_label.len();
                if fill_x % 4 == 0 { '|' } else { '-' }
            };
            let g = 80u8.saturating_add((y as u8).wrapping_mul(7));
            AsciiCell::new(ch).style(Style::new().fg(Color::rgb(g, 200, 255 - g)))
        });

        // Canvas 3: density gradient with horizontal ColorGradient overlay (56×18)
        const SHADES: &[char] = &[' ', '.', ':', '+', 'o', 'O', '#', '@'];
        let shade_canvas = AsciiCanvas::with_cell_fn(56, 18, |x, y| {
            let idx = ((x as usize * 2 + y as usize * 5) / 4) % SHADES.len();
            AsciiCell::new(SHADES[idx])
        })
        .gradient(
            ColorGradient::new(Color::rgb(255, 80, 0), Color::rgb(0, 200, 255)),
            GradientDirection::Horizontal,
        );

        // Canvas 4: diagonal stripes with vertical ColorGradient (text lines mode, 56×18)
        let stripe_lines: Vec<String> = (0u16..18)
            .map(|y| {
                (0u16..56)
                    .map(|x| if (x + y * 2) % 8 < 4 { '/' } else { '\\' })
                    .collect()
            })
            .collect();
        let diagonal = AsciiCanvas::new(stripe_lines.iter().map(|s| s.as_str())).gradient(
            ColorGradient::new(Color::Magenta, Color::Yellow).with_center(Color::rgb(255, 100, 0)),
            GradientDirection::Vertical,
        );

        Frame::new()
            .title("AsciiCanvas in ScrollView  (↑↓ / j k to scroll · q to quit)")
            .border_style(BorderStyle::Rounded)
            .child(
                ScrollView::new()
                    .padding(1)
                    .gap(1)
                    .scrollbar(true)
                    .scroll_keys(ScrollKeymap::DEFAULT)
                    .child(
                        Frame::new()
                            .title("Canvas 1 - checkerboard, per-cell colors (56×18)")
                            .border(true)
                            .child(checkerboard),
                    )
                    .child(
                        Frame::new()
                            .title("Canvas 2 - row labels, per-cell colors (scroll to verify row-skip, 56×22)")
                            .border(true)
                            .child(row_grid),
                    )
                    .child(
                        Frame::new()
                            .title("Canvas 3 - shading + horizontal gradient orange→cyan (56×18)")
                            .border(true)
                            .child(shade_canvas),
                    )
                    .child(
                        Frame::new()
                            .title("Canvas 4 - diagonal stripes + vertical gradient magenta→yellow (56×18)")
                            .border(true)
                            .child(diagonal),
                    ),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new().mount(AsciiCanvasScroll).run()
}
