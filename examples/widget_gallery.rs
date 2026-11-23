//! A comprehensive showcase of Lipan's built-in widgets.
//!
//! This example demonstrates Checkboxes, ProgressBars, Spinners, and Sliders.
//!
//! Controls:
//! - Tab/Shift+Tab: Navigate between focusable widgets
//! - Space/Enter: Toggle checkboxes or interact with sliders
//! - Click: Direct interaction with widgets
//! - Drag: Adjust progress bars and sliders
//! - Spinners: Animate automatically
//! - 'q'/Esc: Quit

use tui_lipan::prelude::*;
use tui_lipan::style::palette;

struct App {
    checkboxes: [bool; 4],
    progress: f64,
    slider_val: f64,
}

#[derive(Clone, Debug)]
enum Msg {
    Toggle(usize, bool),
    SetProgress(f64),
    SetSlider(f64),
}

impl Component for App {
    type Message = Msg;
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn update(&mut self, msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::Toggle(idx, checked) => {
                self.checkboxes[idx] = checked;
            }
            Msg::SetProgress(progress) => {
                self.progress = progress;
            }
            Msg::SetSlider(val) => {
                self.slider_val = val;
            }
        }
        Update::layout()
    }

    fn on_key(&mut self, key: KeyEvent, ctx: &mut Context<Self>) -> KeyUpdate {
        match key.code {
            KeyCode::Char('q') => {
                ctx.quit();
                KeyUpdate::handled(Update::full())
            }
            _ => KeyUpdate::unhandled(Update::none()),
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Element {
        let link = ctx.link();

        rsx! {
            VStack {
                gap: 0,
                padding: 0,
                Frame {
                    title: "Checkboxes (Tab to navigate, Space/Enter/Click to toggle)",
                    border: true,
                    border_style: BorderStyle::Rounded,
                    height: Length::Auto,
                    VStack {
                        gap: 1,
                        padding: 1,
                        Checkbox {
                            checked: self.checkboxes[0],
                            label: "Bracket style - classic terminal look",
                            variant: CheckboxVariant::Bracket,
                            on_toggle: link.callback(move |e: CheckboxEvent| { Msg::Toggle(0, e.state.is_checked()) }),
                            focusable: false,
                        },
                        Checkbox {
                            checked: self.checkboxes[1],
                            label: "Circle style - radio-like appearance",
                            variant: CheckboxVariant::Circle,
                            on_toggle: link.callback(move |e: CheckboxEvent| { Msg::Toggle(1, e.state.is_checked()) }),
                        },
                        Checkbox {
                            checked: self.checkboxes[2],
                            label: "Box style - modern with checkmark",
                            variant: CheckboxVariant::Box,
                            on_toggle: link.callback(move |e: CheckboxEvent| { Msg::Toggle(2, e.state.is_checked()) }),
                        },
                        Checkbox {
                            checked: self.checkboxes[3],
                            label: "Custom style - (Y)/(N)",
                            variant: CheckboxVariant::Custom {
                                checked: "(Y)",
                                unchecked: "(N)",
                                indeterminate: "(?)",
                            },
                            on_toggle: link.callback(move |e: CheckboxEvent| { Msg::Toggle(3, e.state.is_checked()) }),
                        },
                    },
                },
                Frame {
                    title: "Sliders (drag to change value)",
                    border: true,
                    border_style: BorderStyle::Rounded,
                    height: Length::Auto,
                    VStack {
                        gap: 1,
                        padding: 1,
                        Slider {
                            value: self.slider_val,
                            min: 0.0,
                            max: 100.0,
                            step: 1.0,
                            label: "Standard Slider",
                            focusable: true,
                            focus_style: Style::new().fg(palette::BLUE),
                            on_change: link.callback(Msg::SetSlider),
                        },
                        Slider {
                            value: self.slider_val,
                            min: 0.0,
                            max: 100.0,
                            step: 5.0,
                            label: "Interactive (Hover/Focus)",
                            style: Style::new().fg(palette::SLATE),
                            thumb_style: Style::new().fg(palette::BLUE),
                            focusable: true,
                            focus_style: Style::new().fg(palette::CYAN),
                            focus_thumb_style: Style::new().fg(palette::CYAN).bold(),
                            hover_thumb_style: Style::new().fg(palette::RED),
                            thumb_symbol: "●",
                            hover_thumb_symbol: "⬤",
                            on_change: link.callback(Msg::SetSlider),
                        },
                    },
                },
                Frame {
                    title: "Progress Bars (drag to change value)",
                    border: true,
                    border_style: BorderStyle::Rounded,
                    height: Length::Flex(1),
                    VStack {
                        gap: 0,
                        padding: (0, 1),
                        HStack {
                            gap: 1,
                            Text { content: "Block:  " },
                            ProgressBar {
                                progress: self.progress,
                                progress_style: ProgressStyle::Block,
                                show_percentage: true,
                                filled_style: Style::new().fg(palette::EMERALD),
                                draggable: true,
                                on_change: link.callback(|e: ProgressEvent| Msg::SetProgress(e.progress)),
                            },
                        },
                        HStack {
                            gap: 1,
                            Text { content: "Rect:   " },
                            ProgressBar {
                                progress: self.progress,
                                progress_style: ProgressStyle::Rect,
                                show_percentage: true,
                                filled_style: Style::new().fg(palette::ROSE),
                                draggable: true,
                                on_change: link.callback(|e: ProgressEvent| Msg::SetProgress(e.progress)),
                            },
                        },
                        HStack {
                            gap: 1,
                            Text { content: "Custom: " },
                            ProgressBar {
                                progress: self.progress,
                                progress_style: ProgressStyle::Custom {
                                    filled: '#',
                                    empty: '-',
                                },
                                show_percentage: true,
                                filled_style: Style::new().fg(palette::LIME),
                                draggable: true,
                                on_change: link.callback(|e: ProgressEvent| Msg::SetProgress(e.progress)),
                            },
                        },
                        HStack {
                            gap: 1,
                            Text { content: "Line:   " },
                            ProgressBar {
                                progress: self.progress,
                                progress_style: ProgressStyle::Line,
                                show_percentage: true,
                                filled_style: Style::new().fg(palette::SKY),
                                draggable: true,
                                on_change: link.callback(|e: ProgressEvent| Msg::SetProgress(e.progress)),
                            },
                        },
                        HStack {
                            gap: 1,
                            Text { content: "Dots:   " },
                            ProgressBar {
                                progress: self.progress,
                                progress_style: ProgressStyle::Dots,
                                show_percentage: true,
                                filled_style: Style::new().fg(palette::FUCHSIA),
                                draggable: true,
                                on_change: link.callback(|e: ProgressEvent| Msg::SetProgress(e.progress)),
                            },
                        },
                        HStack {
                            gap: 1,
                            Text { content: "Dotted: " },
                            ProgressBar {
                                progress: self.progress,
                                progress_style: ProgressStyle::LineDotted,
                                show_percentage: true,
                                filled_style: Style::new().fg(palette::AMBER),
                                draggable: true,
                                on_change: link.callback(|e: ProgressEvent| Msg::SetProgress(e.progress)),
                            },
                        },
                        HStack {
                            gap: 1,
                            Text { content: "Braille:" },
                            ProgressBar {
                                progress: self.progress,
                                progress_style: ProgressStyle::Braille,
                                show_percentage: true,
                                filled_style: Style::new().fg(palette::TEAL),
                                draggable: true,
                                on_change: link.callback(|e: ProgressEvent| Msg::SetProgress(e.progress)),
                            },
                        },
                        HStack {
                            gap: 1,
                            Text { content: "Stepped: " },
                            ProgressBar {
                                progress: self.progress,
                                progress_style: ProgressStyle::Block,
                                show_percentage: true,
                                filled_style: Style::new().fg(palette::VIOLET),
                                draggable: true,
                                step: 0.1,
                                focusable: true,
                                focus_style: Style::new().fg(palette::YELLOW),
                                on_change: link.callback(|e: ProgressEvent| Msg::SetProgress(e.progress)),
                            },
                        },
                    },
                },
                Frame {
                    title: "Spinners (auto-animated)",
                    border: true,
                    border_style: BorderStyle::Rounded,
                    height: Length::Flex(1),
                    VStack {
                        gap: 0,
                        padding: 0,
                        align: Align::Center,
                        HStack {
                            gap: 0,
                            padding: 0,
                            align: Align::Center,
                            justify: Justify::SpaceBetween,
                            Spinner {
                                spinner_style: SpinnerStyle::Dots,
                                label: "Dots",
                                style: Style::new().fg(Color::Cyan),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Line,
                                label: "Line",
                                style: Style::new().fg(Color::Yellow),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Circle,
                                label: "Circle",
                                style: Style::new().fg(Color::Green),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Braille,
                                label: "Braille",
                                style: Style::new().fg(Color::Magenta),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Arc,
                                label: "Arc",
                                style: Style::new().fg(Color::Red),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Bar,
                                label: "Bar",
                                style: Style::new().fg(Color::Green),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Arrow,
                                label: "Arrow",
                                style: Style::new().fg(Color::Yellow),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Fade,
                                label: "Fade",
                                style: Style::new().fg(Color::Magenta),
                            },
                        },
                        HStack {
                            gap: 0,
                            padding: 0,
                            align: Align::Center,
                            justify: Justify::SpaceBetween,
                            Spinner {
                                spinner_style: SpinnerStyle::Trail,
                                label: "Trail",
                                style: Style::new().fg(Color::Cyan),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Earth,
                                label: "Earth",
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Moon,
                                label: "Moon",
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Box,
                                label: "Box",
                                style: Style::new().fg(Color::LightBlue),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::ThreeDot,
                                label: "ThreeDot",
                                style: Style::new().fg(Color::Cyan),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::ThreeDotFade,
                                label: "ThreeDotFade",
                                style: Style::new().fg(Color::Blue),
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::SquareFade,
                                label: "SquareFade",
                                style: Style::new().fg(Color::Magenta),
                            },
                        },
                        HStack {
                            gap: 0,
                            padding: 0,
                            align: Align::Center,
                            justify: Justify::SpaceBetween,
                            Spinner {
                                spinner_style: SpinnerStyle::OpenCode,
                                label: "OpenCode",
                                speed: SpinnerSpeed::Fast,
                            },
                            Spinner {
                                spinner_style: SpinnerStyle::Claude,
                                label: "Claude",
                                style: Style::new().fg(Color::Rgb(217, 119, 87)),
                                speed: SpinnerSpeed::Custom {
                                    frame_ms: 150,
                                },
                            },
                        },
                        HStack {
                            gap: 0,
                            padding: 0,
                            align: Align::Center,
                            justify: Justify::SpaceBetween,
                            VStack {
                                width: Length::Flex(1),
                                height: Length::Auto,
                                align: Align::Center,
                                Spinner {
                                    spinner_style: SpinnerStyle::Lightsaber,
                                    label: "Lightsaber (Cyan)",
                                    style: Style::new().fg(Color::Rgb(0, 200, 255)),
                                    speed: SpinnerSpeed::Fast,
                                    width: Length::Auto,
                                },
                            },
                            VStack {
                                width: Length::Flex(1),
                                height: Length::Auto,
                                align: Align::Center,
                                Spinner {
                                    spinner_style: SpinnerStyle::Lightsaber,
                                    label: "Lightsaber (Red)",
                                    style: Style::new().fg(Color::Rgb(220, 0, 0)),
                                    speed: SpinnerSpeed::Fast,
                                    width: Length::Auto,
                                },
                            },
                        },
                    },
                },
                Text {
                    content: "Press 'q' or Esc to quit",
                    style: Style::new().dim(),
                },
            }
        }
    }
}

fn main() -> tui_lipan::Result<()> {
    let app = App {
        checkboxes: [false, true, false, true],
        progress: 0.35,
        slider_val: 50.0,
    };

    tui_lipan::App::new().mount(app).run()
}
