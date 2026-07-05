//! Theme showcase example - demonstrates ThemeProvider with switchable themes.
//!
//! Run with: cargo run --example theme_showcase
//!
//! Controls:
//! - 1-7: Switch between themes
//! - Tab: Navigate between widgets
//! - Ctrl+Q: Quit

#[cfg(feature = "syntax-syntect")]
use tui_lipan::SyntectStrategy;
use tui_lipan::prelude::*;

/// Available themes in the showcase.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum ThemeChoice {
    #[default]
    Lipan,
    OneDark,
    Dracula,
    Nord,
    Gruvbox,
    Catppuccin,
    Ansi,
}

impl ThemeChoice {
    fn name(self) -> &'static str {
        match self {
            Self::Lipan => "Lipan",
            Self::OneDark => "One Dark",
            Self::Dracula => "Dracula",
            Self::Nord => "Nord",
            Self::Gruvbox => "Gruvbox",
            Self::Catppuccin => "Catppuccin",
            Self::Ansi => "ANSI",
        }
    }

    fn theme(self) -> Theme {
        // Use the built-in theme presets from the library
        match self {
            Self::Lipan => Theme::lipan(),
            Self::OneDark => Theme::one_dark(),
            Self::Dracula => Theme::dracula(),
            Self::Nord => Theme::nord(),
            Self::Gruvbox => Theme::gruvbox(),
            Self::Catppuccin => Theme::catppuccin(),
            Self::Ansi => Theme::ansi(),
        }
    }

    fn all() -> &'static [Self] {
        &[
            Self::Lipan,
            Self::OneDark,
            Self::Dracula,
            Self::Nord,
            Self::Gruvbox,
            Self::Catppuccin,
            Self::Ansi,
        ]
    }
}

#[cfg(feature = "markdown")]
const MARKDOWN_THEME_SAMPLE: &str = r#"# Markdown + Syntax

Theme palettes now drive headings, links, quotes, and code blocks.

> Blockquotes and table borders pick up themed document styles.

- Themed links and emphasis stay in sync with the selected preset.
- Lists and separators should also shift with the theme.
"#;

#[cfg(feature = "syntax-syntect")]
const SYNTAX_THEME_SAMPLE: &str = r#"// Theme-native syntax preview
struct ThemePreview {
    title: String,
    count: usize,
}

fn greet(name: &str) -> String {
    let preview = ThemePreview {
        title: name.to_string(),
        count: 42,
    };

    if preview.count > 10 {
        format!("Hello, {} #{}", preview.title, preview.count)
    } else {
        "small".to_string()
    }
}"#;

#[cfg(feature = "syntax-syntect")]
fn syntax_legend(theme: &Theme) -> Element {
    HStack::new()
        .gap(1)
        .child(Text::new("keyword").style(theme.syntax.keyword))
        .child(Text::new("string").style(theme.syntax.string))
        .child(Text::new("number").style(theme.syntax.number))
        .child(Text::new("function").style(theme.syntax.function))
        .child(Text::new("type").style(theme.syntax.type_name))
        .child(Text::new("comment").style(theme.syntax.comment))
        .into()
}

#[cfg(all(feature = "markdown", feature = "syntax-syntect"))]
fn theme_preview_panel(theme: Theme) -> Element {
    Frame::new()
        .title("Markdown + Syntax")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .height(Length::Flex(1))
        .child(
            VStack::new()
                .gap(1)
                .child(
                    DocumentView::new(MARKDOWN_THEME_SAMPLE)
                        .markdown()
                        .border(true)
                        .height(Length::Px(8))
                        .wrap(true),
                )
                .child(syntax_legend(&theme))
                .child(
                    TextArea::new(SYNTAX_THEME_SAMPLE)
                        .read_only(true)
                        .border(true)
                        .height(Length::Flex(1))
                        .line_numbers(true)
                        .language("rust")
                        .color_strategy(
                            SyntectStrategy::default()
                                .default_theme("One Dark (Atom)")
                                .syntax_palette(theme.syntax),
                        ),
                ),
        )
        .into()
}

#[cfg(all(feature = "markdown", not(feature = "syntax-syntect")))]
fn theme_preview_panel(_theme: Theme) -> Element {
    Frame::new()
        .title("Markdown")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .height(Length::Flex(1))
        .child(
            DocumentView::new(MARKDOWN_THEME_SAMPLE)
                .markdown()
                .border(false)
                .wrap(true),
        )
        .into()
}

#[cfg(all(feature = "syntax-syntect", not(feature = "markdown")))]
fn theme_preview_panel(theme: Theme) -> Element {
    Frame::new()
        .title("Syntax (syntect)")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .height(Length::Flex(1))
        .child(
            VStack::new().gap(1).child(syntax_legend(&theme)).child(
                TextArea::new(SYNTAX_THEME_SAMPLE)
                    .read_only(true)
                    .border(false)
                    .line_numbers(true)
                    .language("rust")
                    .color_strategy(
                        SyntectStrategy::default()
                            .default_theme("One Dark (Atom)")
                            .syntax_palette(theme.syntax),
                    ),
            ),
        )
        .into()
}

#[cfg(not(any(feature = "markdown", feature = "syntax-syntect")))]
fn theme_preview_panel(_theme: Theme) -> Element {
    Frame::new()
        .title("Markdown + Syntax")
        .border(true)
        .border_style(BorderStyle::Rounded)
        .height(Length::Flex(1))
        .padding(1)
        .child(
            Text::new(
                "Enable `markdown` and `syntax-syntect` to preview theme-native document and code styling.",
            ),
        )
        .into()
}

// --- App component ---

struct ThemeShowcase;

struct State {
    current_theme: ThemeChoice,
    input: TextInput,
    list_selected: usize,
    checkbox_checked: bool,
    slider_value: f64,
}

impl Default for State {
    fn default() -> Self {
        Self {
            current_theme: ThemeChoice::default(),
            input: TextInput::new("Sample text"),
            list_selected: 0,
            checkbox_checked: true,
            slider_value: 50.0,
        }
    }
}

#[derive(Clone, Debug)]
enum Msg {
    SetTheme(ThemeChoice),
    InputChanged(InputEvent),
    ListSelected(ListEvent),
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
        // Ctrl+Q to quit
        if key.mods.ctrl && matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q')) {
            ctx.quit();
            return KeyUpdate::handled(Update::full());
        }

        // Number keys to switch themes
        let theme = match key.code {
            KeyCode::Char('1') => Some(ThemeChoice::Lipan),
            KeyCode::Char('2') => Some(ThemeChoice::OneDark),
            KeyCode::Char('3') => Some(ThemeChoice::Dracula),
            KeyCode::Char('4') => Some(ThemeChoice::Nord),
            KeyCode::Char('5') => Some(ThemeChoice::Gruvbox),
            KeyCode::Char('6') => Some(ThemeChoice::Catppuccin),
            KeyCode::Char('7') => Some(ThemeChoice::Ansi),
            _ => None,
        };

        if let Some(choice) = theme {
            ctx.link().send(Msg::SetTheme(choice));
            return KeyUpdate::handled(Update::full());
        }

        KeyUpdate::unhandled(Update::none())
    }

    fn update(&mut self, msg: Self::Message, ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::SetTheme(theme_choice) => {
                ctx.state.current_theme = theme_choice;
                ctx.toast()
                    .push(Toast::new(format!("Switched to {}", theme_choice.name())));
            }
            Msg::InputChanged(ev) => {
                ctx.state.input.set_text(ev.value.to_string());
                ctx.state.input.set_cursor(ev.cursor);
            }
            Msg::ListSelected(ev) => {
                ctx.state.list_selected = ev.index;
            }
            Msg::CheckboxToggled(ev) => {
                ctx.state.checkbox_checked = ev.state == CheckboxState::Checked;
            }
            Msg::SliderChanged(value) => {
                ctx.state.slider_value = value;
            }
        }
        Update::full()
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let current = ctx.state.current_theme;
        let theme = current.theme();

        // Theme selector buttons
        let theme_buttons: Vec<Element> = ThemeChoice::all()
            .iter()
            .enumerate()
            .map(|(i, &choice)| {
                let is_active = choice == current;
                let variant = if is_active {
                    ButtonVariant::Filled
                } else {
                    ButtonVariant::Outlined
                };

                Button::new(format!("[{}] {}", i + 1, choice.name()))
                    .variant(variant)
                    .width(Length::Flex(1))
                    .on_click(ctx.link().callback(move |_| Msg::SetTheme(choice)))
                    .into()
            })
            .collect();

        // Sample list items (more items to demonstrate scrollbar)
        let list_items: Vec<ListItem> = vec![
            ListItem::new("First item"),
            ListItem::new("Second item"),
            ListItem::new("Third item"),
            ListItem::new("Fourth item"),
            ListItem::new("Fifth item"),
            ListItem::new("Sixth item"),
            ListItem::new("Seventh item"),
            ListItem::new("Eighth item"),
        ];

        // Build the theme selector bar
        let theme_bar = HStack::new()
            .gap(1)
            .height(Length::Px(3))
            .children(theme_buttons);

        let left_column = VStack::new()
            .gap(1)
            .child(
                Frame::new()
                    .title("FileTree (themed icons)")
                    .height(Length::Px(12))
                    .border(true)
                    .border_style(BorderStyle::Rounded)
                    .child(
                        FileTree::new(".")
                            .show_hidden(false)
                            .git_status(true)
                            .icon_style(FileIconStyle::NerdFontColored),
                    ),
            )
            .child(theme_preview_panel(theme.clone()));

        // Main content wrapped in ThemeProvider
        let themed_content = ThemeProvider::new(theme.clone()).child(rsx! {
            VStack {
                gap: 1,
                padding: 1,
                Frame {
                    title: format!("Theme Showcase - {}", current.name()),
                    border: true,
                    border_style: BorderStyle::Rounded,
                    padding: 1,
                    height: Length::Auto,
                    VStack {
                        gap: 1,
                        Text {
                            content: "Press 1-7 to switch themes. Tab to navigate. Ctrl+Q to quit.",
                            style: Style::new().dim(),
                        },
                        theme_bar,
                    },
                },
                HStack {
                    gap: 1,
                    left_column,
                    VStack {
                        gap: 1,
                        Frame {
                            title: "Input",
                            border: true,
                            border_style: BorderStyle::Rounded,
                            padding: 1,
                            height: Length::Auto,
                            Input {
                                value: ctx.state.input.text().to_owned(),
                                cursor: ctx.state.input.cursor(),
                                placeholder: "Type something...",
                                border: true,
                                on_change: ctx.link().callback(Msg::InputChanged),
                            },
                        },
                        Frame {
                            title: "List",
                            border: true,
                            border_style: BorderStyle::Rounded,
                            List {
                                items: list_items,
                                selected: ctx.state.list_selected,
                                on_select: ctx.link().callback(Msg::ListSelected),
                            },
                        },
                        Frame {
                            title: "Other Widgets",
                            border: true,
                            border_style: BorderStyle::Rounded,
                            padding: 1,
                            VStack {
                                gap: 1,
                                HStack {
                                    gap: 2,
                                    Checkbox {
                                        label: "Themed checkbox",
                                        checked: ctx.state.checkbox_checked,
                                        on_toggle: ctx.link().callback(Msg::CheckboxToggled),
                                    },
                                    Spinner {},
                                },
                                ProgressBar {
                                    progress: 0.65,
                                    width: Length::Flex(1),
                                },
                                Slider {
                                    value: ctx.state.slider_value,
                                    min: 0.0,
                                    max: 100.0,
                                    label: "Themed slider",
                                    on_change: ctx.link().callback(Msg::SliderChanged),
                                },
                                HStack {
                                    gap: 2,
                                    Text {
                                        content: "Git:",
                                        style: Style::new().bold(),
                                    },
                                    Text {
                                        content: " Modified",
                                        style: Style::new().fg(theme.git_status.modified),
                                    },
                                    Text {
                                        content: " Added",
                                        style: Style::new().fg(theme.git_status.added),
                                    },
                                    Text {
                                        content: " Deleted",
                                        style: Style::new().fg(theme.git_status.deleted),
                                    },
                                    Text {
                                        content: " Untracked",
                                        style: Style::new().fg(theme.git_status.untracked),
                                    },
                                },
                            },
                        },
                    },
                },
            }
        });

        themed_content.into()
    }
}

fn main() -> Result<()> {
    App::new()
        .toast_placement(ToastPlacement::BottomEnd)
        .mount(ThemeShowcase)
        .run()
}
