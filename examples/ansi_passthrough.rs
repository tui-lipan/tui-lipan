//! ANSI escape passthrough demo - renders styled text from raw ANSI strings.
//!
//! Run with: cargo run --example ansi_passthrough
//!
//! Controls:
//! - Ctrl+Q: Quit
//!
//! Demonstrates `Text::from_ansi()` and `RichText::from_ansi()` rendering
//! ANSI SGR sequences (16-color, 256-color, truecolor, bold, italic, etc.)
//! without embedding a full terminal emulator.

use tui_lipan::prelude::*;

struct AnsiDemo;

#[derive(Clone, Debug)]
enum Msg {}

impl Component for AnsiDemo {
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
        // --- 16-color palette ---
        // ANSI 37 ("white") maps to Color::Gray (light gray) - use 97 for true white.
        let ansi_16_fg = "\x1b[30mBlack\x1b[31m Red\x1b[32m Grn\x1b[33m Yel\x1b[34m Blu\x1b[35m Mag\x1b[36m Cyn\x1b[37m Gry\x1b[0m";
        let ansi_16_bright = "\x1b[90mDrk\x1b[91m LtR\x1b[92m LtG\x1b[93m LtY\x1b[94m LtB\x1b[95m LtM\x1b[96m LtC\x1b[97m Wht\x1b[0m";

        // --- Bold / Italic / Underline / Strikethrough / Reverse ---
        let ansi_attrs = "\x1b[1mbold\x1b[0m \x1b[3mitalic\x1b[0m \x1b[4munderline\x1b[0m \x1b[9mstrike\x1b[0m \x1b[7mreverse\x1b[0m";

        // --- 256-color (indexed) ---
        let ansi_256 = "\x1b[38;5;196mR196\x1b[0m \x1b[38;5;82mG82\x1b[0m \x1b[38;5;27mB27\x1b[0m \x1b[38;5;220mY220\x1b[0m";

        // --- Truecolor (RGB) ---
        let ansi_rgb = "\x1b[38;2;255;128;0mOrange\x1b[0m \x1b[38;2;0;200;180mTeal\x1b[0m \x1b[38;2;180;80;220mPurple\x1b[0m";

        // --- Combined: bold + underline + RGB ---
        let ansi_combo = "\x1b[1;4;38;2;255;200;50mBold Underline Gold\x1b[0m";

        // --- Background colors ---
        let ansi_bg = "\x1b[41mRed BG\x1b[0m \x1b[42mGrn BG\x1b[0m \x1b[44mBlu BG\x1b[0m \x1b[48;2;60;60;80mRGB BG\x1b[0m";

        // --- Simulated ls --color output ---
        let ansi_ls = "\x1b[01;34mdir\x1b[0m  \x1b[01;32mexec\x1b[0m  \x1b[33mlink\x1b[0m  readme.txt  \x1b[31merror.log\x1b[0m";

        // --- Simulated compiler output ---
        let ansi_compiler = "\x1b[1msrc/main.rs\x1b[0m:\x1b[1;34m4\x1b[0m:\x1b[1;34m12\x1b[0m: \x1b[1;31merror\x1b[0m: mismatched types\n  \x1b[2m= note\x1b[0m: expected `i32`, found `String`";

        let section = |title: &str, content: &str| -> Element {
            Frame::new()
                .title(title.to_string())
                .border(true)
                .border_style(BorderStyle::Rounded)
                .padding(1)
                .width(Length::Flex(1))
                .child(Text::from_ansi(content).overflow(Overflow::Wrap))
                .into()
        };

        VStack::new()
            .padding(1)
            .gap(1)
            .child(Text::new("ANSI Escape Passthrough").style(Style::new().bold()))
            .child(Text::new(
                "Text::from_ansi() renders SGR escape sequences as styled spans. Ctrl+Q to quit.",
            ))
            .child(
                HStack::new()
                    .gap(1)
                    .child(section("16-Color FG", ansi_16_fg))
                    .child(section("Bright FG", ansi_16_bright)),
            )
            .child(section("Attributes", ansi_attrs))
            .child(
                HStack::new()
                    .gap(1)
                    .child(section("256-Color", ansi_256))
                    .child(section("Truecolor RGB", ansi_rgb)),
            )
            .child(section("Combined", ansi_combo))
            .child(section("Backgrounds", ansi_bg))
            .child(section("Simulated ls --color", ansi_ls))
            .child(section("Simulated Compiler Output", ansi_compiler))
            .into()
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - ANSI Passthrough")
        .mount(AnsiDemo)
        .run()
}
