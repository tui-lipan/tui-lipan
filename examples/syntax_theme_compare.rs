//! Explicit syntax theme comparison.
//!
//! Run with:
//! `cargo run --example syntax_theme_compare --features "syntax-syntect"`

use tui_lipan::SyntectStrategy;
use tui_lipan::prelude::*;

const SAMPLE: &str = r#"// Explicit syntax palette comparison
struct ThemePreview {
    title: String,
    count: usize,
}

fn greet(name: &str) -> String {
    const HAS_PREFIX: bool = true;
    let preview = ThemePreview {
        title: name.to_string(),
        count: 42,
    };

    if preview.count > 10 {
        let label = if HAS_PREFIX { format!("Hello, {} #{}", preview.title, preview.count) } else { name.to_string() };
        println!("{}", label);
        label
    } else {
        "small".to_string()
    }
}
"#;

struct SyntaxThemeCompare;

impl Component for SyntaxThemeCompare {
    type Message = ();
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
        let panels = [
            syntax_panel("One Dark", Theme::one_dark()),
            syntax_panel("Dracula", Theme::dracula()),
            syntax_panel("Nord", Theme::nord()),
            syntax_panel("ANSI", Theme::ansi()),
        ];

        VStack::new()
            .gap(1)
            .child(
                Frame::new()
                    .title("Syntax Theme Compare")
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .padding(1)
                    .height(Length::Auto)
                    .child(
                        Text::new(
                            "Each panel hardcodes its own non-default Theme and passes theme.syntax directly into SyntectStrategy. If these panels differ, syntax theming itself works. Ctrl+Q quits.",
                        ),
                    ),
            )
            .child(
                HStack::new()
                    .gap(1)
                    .child(panels[0].clone())
                    .child(panels[1].clone()),
            )
            .child(HStack::new().gap(1).child(panels[2].clone()).child(panels[3].clone()))
            .into()
    }
}

fn syntax_panel(title: &str, theme: Theme) -> Element {
    let title = title.to_string();
    let legend = HStack::new()
        .gap(1)
        .height(Length::Auto)
        .child(Text::new("keyword").style(flatten_style(theme.syntax.keyword)))
        .child(Text::new("string").style(flatten_style(theme.syntax.string)))
        .child(Text::new("number").style(flatten_style(theme.syntax.number)))
        .child(Text::new("constant").style(flatten_style(theme.syntax.constant)))
        .child(Text::new("function").style(flatten_style(theme.syntax.function)))
        .child(Text::new("builtin").style(flatten_style(theme.syntax.builtin)))
        .child(Text::new("type").style(flatten_style(theme.syntax.type_name)))
        .child(Text::new("comment").style(flatten_style(theme.syntax.comment)))
        .child(Text::new("variable").style(flatten_style(theme.syntax.variable)))
        .child(Text::new("parameter").style(flatten_style(theme.syntax.parameter)))
        .child(Text::new("operator").style(flatten_style(theme.syntax.operator)));

    ThemeProvider::new(theme.clone())
        .child(
            Frame::new()
                .title(title)
                .border(true)
                .border_style(BorderStyle::Rounded)
                .width(Length::Flex(1))
                .height(Length::Auto)
                .padding(1)
                .child(
                    VStack::new().gap(1).child(legend).child(
                        TextArea::new(SAMPLE)
                            .read_only(true)
                            .border(true)
                            .line_numbers(true)
                            .height(Length::Flex(1))
                            .language("rust")
                            .color_strategy(
                                SyntectStrategy::default()
                                    .default_theme("One Dark (Atom)")
                                    .syntax_palette(theme.syntax),
                            ),
                    ),
                ),
        )
        .into()
}

fn flatten_style(style: Style) -> Style {
    let mut out = style;
    out.fg = style
        .fg
        .and_then(|paint| paint.color().to_rgb())
        .map(|(r, g, b)| Paint::from(Color::rgb(r, g, b)));
    out.bg = style
        .bg
        .and_then(|paint| paint.color().to_rgb())
        .map(|(r, g, b)| Paint::from(Color::rgb(r, g, b)));
    out
}

fn main() -> Result<()> {
    App::new().mount(SyntaxThemeCompare).run()
}
