//! Theme showcase - every built-in preset, applied live through `ThemeProvider`.
//!
//! Run with: cargo run --example theme_showcase --features markdown,syntax-syntect
//! (both features are optional; without them the preview panel explains what it would show)
//!
//! Controls:
//! - Up/Down, j/k, [ / ], or click: switch theme
//! - Tab: navigate between widgets
//! - Ctrl+Q: quit

#[cfg(feature = "syntax-syntect")]
use tui_lipan::SyntectStrategy;
use tui_lipan::prelude::*;

/// A preset's display name and constructor.
type Preset = (&'static str, fn() -> Theme);

/// Every built-in preset, dark themes first.
const PRESETS: &[Preset] = &[
    ("Lipan", Theme::lipan),
    ("One Dark", Theme::one_dark),
    ("Dracula", Theme::dracula),
    ("Nord", Theme::nord),
    ("Gruvbox Dark", Theme::gruvbox_dark),
    ("Catppuccin Mocha", Theme::catppuccin_mocha),
    ("Catppuccin Macchiato", Theme::catppuccin_macchiato),
    ("Catppuccin Frappe", Theme::catppuccin_frappe),
    ("Tokyo Night", Theme::tokyo_night),
    ("Solarized Dark", Theme::solarized_dark),
    ("Monokai", Theme::monokai),
    ("Rose Pine", Theme::rose_pine),
    ("Rose Pine Moon", Theme::rose_pine_moon),
    ("Kanagawa", Theme::kanagawa),
    ("Everforest", Theme::everforest),
    ("Ayu Dark", Theme::ayu_dark),
    ("Ayu Mirage", Theme::ayu_mirage),
    ("Nightfox", Theme::nightfox),
    ("Nordfox", Theme::nordfox),
    ("Night Owl", Theme::night_owl),
    ("Material Palenight", Theme::material_palenight),
    ("Oxocarbon", Theme::oxocarbon),
    ("Zenburn", Theme::zenburn),
    ("ANSI", Theme::ansi),
    ("Solarized Light", Theme::solarized_light),
    ("Gruvbox Light", Theme::gruvbox_light),
    ("Tokyo Night Day", Theme::tokyo_night_day),
    ("Catppuccin Latte", Theme::catppuccin_latte),
    ("Rose Pine Dawn", Theme::rose_pine_dawn),
    ("Ayu Light", Theme::ayu_light),
];

#[cfg(feature = "markdown")]
const MARKDOWN_THEME_SAMPLE: &str = r#"# Markdown + Syntax

Theme palettes drive **headings**, [links](https://tui-lipan.dev), `code`, and quotes.

> Blockquotes and table borders pick up themed document styles.

- Lists and separators shift with the theme.
"#;

#[cfg(feature = "syntax-syntect")]
const SYNTAX_THEME_SAMPLE: &str = r#"// Theme-native syntax preview
struct Preview {
    title: String,
    count: usize,
}

fn greet(name: &str) -> String {
    let preview = Preview { title: name.into(), count: 42 };
    if preview.count > 10 {
        format!("Hello, {} #{}", preview.title, preview.count)
    } else {
        "small".to_string()
    }
}"#;

#[cfg(feature = "syntax-syntect")]
fn syntax_text_area(theme: &Theme) -> TextArea {
    TextArea::new(SYNTAX_THEME_SAMPLE)
        .read_only(true)
        .border(false)
        .line_numbers(true)
        .language("rust")
        .height(Length::Flex(1))
        .color_strategy(
            SyntectStrategy::default()
                .default_theme("One Dark (Atom)")
                .syntax_palette(theme.syntax),
        )
}

#[cfg(all(feature = "markdown", feature = "syntax-syntect"))]
fn preview_body(theme: &Theme) -> Element {
    VStack::new()
        .gap(1)
        .child(
            DocumentView::new(MARKDOWN_THEME_SAMPLE)
                .markdown()
                .border(false)
                .height(Length::Px(9))
                .wrap(true),
        )
        .child(syntax_text_area(theme))
        .into()
}

#[cfg(all(feature = "markdown", not(feature = "syntax-syntect")))]
fn preview_body(_theme: &Theme) -> Element {
    DocumentView::new(MARKDOWN_THEME_SAMPLE)
        .markdown()
        .border(false)
        .wrap(true)
        .into()
}

#[cfg(all(feature = "syntax-syntect", not(feature = "markdown")))]
fn preview_body(theme: &Theme) -> Element {
    syntax_text_area(theme).into()
}

#[cfg(not(any(feature = "markdown", feature = "syntax-syntect")))]
fn preview_body(_theme: &Theme) -> Element {
    Text::new(
        "Enable `markdown` and `syntax-syntect` to preview theme-native document and code styling.",
    )
    .into()
}

/// A bordered panel painted with the theme's panel surface.
fn panel(theme: &Theme, title: &str) -> Frame {
    Frame::new()
        .header_left(title)
        .border(true)
        .border_style(BorderStyle::Rounded)
        .style(Style::new().bg(theme.surface.panel))
}

fn hex(color: Color) -> String {
    color
        .to_rgb()
        .map(|(r, g, b)| format!("{r:02X}{g:02X}{b:02X}"))
        .unwrap_or_else(|| "ansi".to_string())
}

/// One colour block with its role and hex value underneath.
fn swatch(theme: &Theme, label: &str, color: Color) -> Element {
    VStack::new()
        .width(Length::Flex(1))
        .child(
            HStack::new()
                .style(Style::new().bg(color))
                .height(Length::Px(2)),
        )
        .child(Text::new(label).overflow(Overflow::Ellipsis))
        .child(
            Text::new(hex(color))
                .style(theme.muted)
                .overflow(Overflow::Ellipsis),
        )
        .into()
}

fn palette_strip(theme: &Theme) -> Element {
    let fg = |style: Style| style.fg.map_or(Color::Reset, Paint::color);
    let swatches = [
        ("bg", theme.surface.backdrop),
        ("panel", theme.surface.panel),
        ("menu", theme.surface.menu),
        ("border", fg(theme.border)),
        ("text", fg(theme.primary)),
        ("muted", fg(theme.muted)),
        ("accent", fg(theme.accent)),
        (
            "select",
            theme.selection.bg.map_or(Color::Reset, Paint::color),
        ),
        ("ok", theme.status.success),
        ("warn", theme.status.warning),
        ("error", theme.status.error),
        ("info", theme.status.info),
    ];
    HStack::new()
        .gap(1)
        .children(
            swatches
                .into_iter()
                .map(|(label, color)| swatch(theme, label, color)),
        )
        .into()
}

/// A status word on its own colour, like a statusline segment.
fn pill(label: &str, bg: Color, fg: Color) -> Element {
    Text::new(format!(" {label} "))
        .style(Style::new().bg(bg).fg(fg).bold())
        .into()
}

// --- App component ---

struct ThemeShowcase;

struct State {
    current: usize,
    input: TextInput,
    checkbox_checked: bool,
    slider_value: f64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            current: 0,
            input: TextInput::new("Sample text"),
            checkbox_checked: true,
            slider_value: 50.0,
        }
    }
}

#[derive(Clone, Debug)]
enum Msg {
    SetTheme(usize),
    InputChanged(InputEvent),
    CheckboxToggled(CheckboxEvent),
    SliderChanged(f64),
}

impl Component for ThemeShowcase {
    type Message = Msg;
    type Properties = ();
    type State = State;

    fn create_state(&self, _props: &Self::Properties) -> Self::State {
        State::default()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        if key.mods.ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }

        let count = PRESETS.len();
        let next = match key.code {
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Char(']') => {
                Some((ctx.state.current + 1) % count)
            }
            KeyCode::Up | KeyCode::Char('k') | KeyCode::Char('[') => {
                Some((ctx.state.current + count - 1) % count)
            }
            _ => None,
        };
        match next {
            Some(index) => {
                ctx.link().send(Msg::SetTheme(index));
                KeyUpdate::handled(Update::full())
            }
            None => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::SetTheme(index) => ctx.state.current = index.min(PRESETS.len() - 1),
            Msg::InputChanged(ev) => {
                ctx.state.input.set_text(ev.value.to_string());
                ctx.state.input.set_cursor(ev.cursor);
            }
            Msg::CheckboxToggled(ev) => {
                ctx.state.checkbox_checked = ev.state == CheckboxState::Checked;
            }
            Msg::SliderChanged(value) => ctx.state.slider_value = value,
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let (name, make_theme) = PRESETS[ctx.state.current];
        let theme = make_theme();
        let backdrop = theme.surface.backdrop;

        let header = HStack::new()
            .gap(1)
            .height(Length::Px(1))
            .child(
                Text::new(" tui-lipan themes ").style(
                    Style::new()
                        .bg(theme.accent.fg.map_or(Color::Reset, Paint::color))
                        .fg(backdrop)
                        .bold(),
                ),
            )
            .child(Text::new(name).style(theme.accent.bold()))
            .child(
                Text::new(format!("{}/{}", ctx.state.current + 1, PRESETS.len()))
                    .style(theme.muted),
            )
            .child(Spacer::new())
            .child(
                Text::new("↑/↓ or click to switch · Tab to navigate · Ctrl+Q quit")
                    .style(theme.muted),
            );

        let picker = panel(&theme, "Presets").width(Length::Px(24)).child(
            List::new()
                .items(PRESETS.iter().map(|(name, _)| ListItem::new(*name)))
                .selected(ctx.state.current)
                .on_select(ctx.link().callback(|ev: ListEvent| Msg::SetTheme(ev.index))),
        );

        let buttons = HStack::new()
            .gap(1)
            .height(Length::Px(3))
            .child(Button::new("Filled").variant(ButtonVariant::Filled))
            .child(Button::new("Outlined").variant(ButtonVariant::Outlined))
            .child(Button::new("Bracket").variant(ButtonVariant::Bracket));

        let status = HStack::new()
            .gap(1)
            .height(Length::Px(1))
            .child(pill("ok", theme.status.success, backdrop))
            .child(pill("warn", theme.status.warning, backdrop))
            .child(pill("error", theme.status.error, backdrop))
            .child(pill("info", theme.status.info, backdrop))
            .child(Text::new("M").style(Style::new().fg(theme.git_status.modified)))
            .child(Text::new("A").style(Style::new().fg(theme.git_status.added)))
            .child(Text::new("D").style(Style::new().fg(theme.git_status.deleted)))
            .child(Text::new("?").style(Style::new().fg(theme.git_status.untracked)));

        let controls = panel(&theme, "Controls").padding((0, 1)).child(
            VStack::new()
                .gap(1)
                .child(
                    Input::new(ctx.state.input.text().to_owned())
                        .cursor(ctx.state.input.cursor())
                        .placeholder("Type something...")
                        .border(true)
                        .on_change(ctx.link().callback(Msg::InputChanged)),
                )
                .child(
                    HStack::new()
                        .gap(2)
                        .height(Length::Px(1))
                        .child(
                            Checkbox::new(ctx.state.checkbox_checked)
                                .label("Themed checkbox")
                                .on_toggle(ctx.link().callback(Msg::CheckboxToggled)),
                        )
                        .child(Spinner::new()),
                )
                .child(ProgressBar::new(0.65).width(Length::Flex(1)))
                .child(
                    Slider::new(ctx.state.slider_value)
                        .min(0.0)
                        .max(100.0)
                        .label("Slider")
                        .on_change(ctx.link().callback(Msg::SliderChanged)),
                )
                .child(buttons)
                .child(status),
        );

        let files = panel(&theme, "Files").height(Length::Px(9)).child(
            FileTree::new(".")
                .show_hidden(false)
                .git_status(true)
                .icon_style(FileIconStyle::NerdFontColored),
        );

        let preview = panel(&theme, "Document + code")
            .width(Length::Flex(1))
            .padding((0, 1))
            .child(preview_body(&theme));

        let body = HStack::new().gap(1).child(picker).child(
            VStack::new()
                .gap(1)
                .child(
                    panel(&theme, "Palette")
                        .height(Length::Auto)
                        .padding((0, 1))
                        .child(palette_strip(&theme)),
                )
                .child(
                    HStack::new()
                        .gap(1)
                        .child(
                            VStack::new()
                                .gap(1)
                                .width(Length::Flex(1))
                                .child(files)
                                .child(controls),
                        )
                        .child(preview),
                ),
        );

        let root_style = theme.primary.bg(backdrop);
        ThemeProvider::new(theme)
            .child(
                VStack::new()
                    .style(root_style)
                    .padding((0, 1))
                    .gap(1)
                    .child(header)
                    .child(body),
            )
            .into()
    }
}

fn main() -> Result<()> {
    App::new().mount(ThemeShowcase).run()
}
