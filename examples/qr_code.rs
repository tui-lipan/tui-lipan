//! QR code widget across render modes, error correction levels, and payload sizes.
//!
//! Point a phone at the symbol to open <https://tui-lipan.dev>. Resize the
//! terminal to watch the fit check swap in the fallback: a clipped QR code
//! still looks like one but will not scan, so the widget's size is checked
//! against the viewport before it is rendered.
//!
//! ```sh
//! cargo run --example qr_code --features qr-code
//! ```

use tui_lipan::prelude::*;

/// Rows and columns spent on the surrounding chrome (title, help, status,
/// frame border, padding) rather than the symbol itself.
const CHROME_W: u16 = 6;
const CHROME_H: u16 = 12;

const PAYLOADS: [(&str, &str); 4] = [
    ("Site", "https://tui-lipan.dev"),
    (
        "Deep link",
        "https://tui-lipan.dev/widgets/display.html#qrcode",
    ),
    (
        "Long URL",
        "https://tui-lipan.dev/widgets/display.html?utm_source=terminal&utm_medium=qr&utm_campaign=example&ref=tui-lipan-examples",
    ),
    // Past the capacity of any QR version, so `.fallback(...)` takes over.
    ("Too long", TOO_LONG),
];

const TOO_LONG: &str = "https://tui-lipan.dev/?payload=this-string-is-deliberately-far-longer-than-any-qr-version-can-hold-so-the-encoder-gives-up-and-the-widget-falls-back-to-plain-text-instead-of-drawing-a-symbol-that-could-never-be-scanned-no-matter-how-large-the-terminal-happens-to-be-right-now-and-we-keep-going-well-past-the-2953-byte-ceiling-of-version-40-with-low-error-correction-so-there-is-no-ambiguity-about-what-is-being-tested-here-at-all";

const ECC_LEVELS: [(&str, QrEcc); 4] = [
    ("Low ~7%", QrEcc::Low),
    ("Medium ~15%", QrEcc::Medium),
    ("Quartile ~25%", QrEcc::Quartile),
    ("High ~30%", QrEcc::High),
];

const QUIET_ZONES: [u16; 4] = [4, 2, 0, 8];

struct QrCodeExample;

#[derive(Default)]
struct State {
    payload: usize,
    ecc: usize,
    quiet: usize,
    wide: bool,
    inverted: bool,
}

#[derive(Clone, Debug)]
enum Msg {}

impl Component for QrCodeExample {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn update(&mut self, _msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        Update::none()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        let state = &mut ctx.state;
        match key.code {
            KeyCode::Char('p') => state.payload = (state.payload + 1) % PAYLOADS.len(),
            KeyCode::Char('e') => state.ecc = (state.ecc + 1) % ECC_LEVELS.len(),
            KeyCode::Char('z') => state.quiet = (state.quiet + 1) % QUIET_ZONES.len(),
            KeyCode::Char('m') => state.wide = !state.wide,
            KeyCode::Char('i') => state.inverted = !state.inverted,
            KeyCode::Char('q') | KeyCode::Esc => {
                ctx.quit();
                return KeyUpdate::handled(Update::full());
            }
            _ => return KeyUpdate::unhandled(Update::none()),
        }
        KeyUpdate::handled(Update::full())
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let state = &ctx.state;
        let (payload_label, payload) = PAYLOADS[state.payload];
        let (ecc_label, ecc) = ECC_LEVELS[state.ecc];
        let quiet = QUIET_ZONES[state.quiet];
        let render = if state.wide {
            QrRender::Wide
        } else {
            QrRender::HalfBlock
        };

        let mut qr = QrCode::new(payload)
            .ecc(ecc)
            .render(render)
            .quiet_zone(quiet)
            .fallback(
                Text::new("Payload exceeds QR capacity")
                    .style(Style::new().fg(Color::Rgb(255, 140, 140)).bold()),
            );
        if state.inverted {
            qr = qr.invert();
        }

        // A QR symbol cannot reflow, so compare its fixed footprint against the
        // space actually available and degrade to something readable instead of
        // rendering a clipped symbol that no scanner will decode.
        let viewport = ctx.viewport();
        let budget = (
            viewport.w.saturating_sub(CHROME_W),
            viewport.h.saturating_sub(CHROME_H),
        );
        let size = qr.size();
        let fits = size.is_some_and(|(w, h)| w <= budget.0 && h <= budget.1);

        let symbol: Element = if fits {
            qr.into()
        } else if let Some((w, h)) = size {
            VStack::new()
                .gap(1)
                .align(Align::Center)
                .child(
                    Text::new(format!(
                        "Needs {w}x{h} cells, only {}x{} available",
                        budget.0, budget.1
                    ))
                    .style(Style::new().fg(Color::Rgb(255, 190, 100))),
                )
                .child(Text::new(payload).style(Style::new().dim()))
                .into()
        } else {
            // Encoding failed outright; let the widget's own fallback speak.
            qr.into()
        };

        let status = match size {
            Some((w, h)) => format!(
                "{w}x{h} cells   {} modules   viewport {}x{}",
                qr_modules(payload, ecc),
                viewport.w,
                viewport.h
            ),
            None => format!("unencodable   viewport {}x{}", viewport.w, viewport.h),
        };

        let settings = format!(
            "payload: {payload_label}   ecc: {ecc_label}   mode: {}   quiet zone: {quiet}{}",
            if state.wide { "Wide" } else { "HalfBlock" },
            if state.inverted { "   inverted" } else { "" },
        );

        rsx! {
            VStack {
                padding: 1,
                spacing: 1,
                Text {
                    content: "QR Code Widget",
                    style: Style::new().bold().fg(Color::Rgb(126, 190, 255)),
                },
                Text {
                    content: "p payload   e error correction   m mode   z quiet zone   i invert   q quit",
                    style: Style::new().dim(),
                },
                Text {
                    content: settings,
                    style: Style::new().fg(Color::Rgb(170, 210, 160)),
                },
                Text {
                    content: status,
                    style: Style::new().dim(),
                },
                Frame {
                    header_left: "Scan me",
                    border: true,
                    border_style: BorderStyle::Rounded,
                    padding: 1,
                    Center {
                        symbol,
                    },
                },
            }
        }
    }
}

/// Module count for the status line, independent of quiet zone and render mode.
fn qr_modules(payload: &str, ecc: QrEcc) -> String {
    match QrCode::new(payload).ecc(ecc).module_count() {
        Some(modules) => format!("{modules}x{modules}"),
        None => "-".to_string(),
    }
}

fn main() -> Result<()> {
    App::new().mount(QrCodeExample).run()
}
