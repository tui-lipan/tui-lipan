use tui_lipan::prelude::*;

struct FormsDemo {
    theme: usize,
    country_idx: Option<usize>,
    country_expanded: bool,
    year: i32,
    month: u32,
    day: u32,
    volume: f64,
    combo_query: String,
    combo_open: bool,
    combo_active: Option<usize>,
    combo_selected: Option<usize>,
    combo_status: String,
}

#[derive(Clone, Debug)]
enum Msg {
    ThemeChanged(usize),
    CountryToggle(bool),
    CountrySelect(usize),
    CountryChange(usize),
    VolumeChanged(f64),
    PrevMonth,
    NextMonth,
    DateSelected(DateEvent),
    ComboQueryChanged(std::sync::Arc<str>),
    ComboOpenChanged(bool),
    ComboHighlightChanged(Option<usize>),
    ComboCommit(ComboBoxCommitEvent),
}

impl Component for FormsDemo {
    type Message = Msg;
    type Properties = ();
    type State = ();

    fn create_state(&self, _props: &Self::Properties) -> Self::State {}

    fn view(&self, ctx: &Context<Self>) -> Element {
        let countries = vec!["USA", "Canada", "UK", "Germany", "France", "Japan"];

        let radio_section = Frame::new()
            .title("Theme Selection")
            .border(true)
            .height(Length::Flex(1))
            .child_align(Align::Center)
            .child(
                Radio::new(vec!["Light", "Dark", "System"])
                    .selected(Some(self.theme))
                    .layout(RadioLayout::Horizontal)
                    .gap(2)
                    .checked_style(Style::new().fg(Color::Green))
                    .unchecked_style(Style::new().fg(Color::DarkGray))
                    .hover_style(Style::new().fg(Color::LightBlue))
                    .on_change(ctx.link().callback(Msg::ThemeChanged)),
            );

        let select_section = Frame::new()
            .title("Country")
            .border(true)
            .height(Length::Flex(1))
            .child(
                Select::new()
                    .options(countries)
                    .selected(self.country_idx)
                    .placeholder("Select a country...")
                    .expanded(self.country_expanded)
                    .on_toggle(ctx.link().callback(Msg::CountryToggle))
                    .on_change(ctx.link().callback(Msg::CountryChange))
                    .on_select(ctx.link().callback(Msg::CountrySelect))
                    .width(Length::Flex(1)),
            );

        let date_section = Frame::new()
            .title("Date Picker")
            .border(true)
            .height(Length::Flex(1))
            .child(
                Center::new().child(
                    DatePicker::new()
                        .title(None::<&str>)
                        .border(true)
                        .year(self.year)
                        .month(self.month)
                        .day(self.day)
                        .show_outside_days(true)
                        .header_style(Style::new().bold().fg(Color::Yellow))
                        .weekday_style(Style::new().fg(Color::DarkGray))
                        .selected_style(Style::new().bg(Color::LightBlue).fg(Color::Black).bold())
                        .nav_style(Style::new().fg(Color::LightBlue))
                        .nav_disabled_style(Style::new().dim())
                        .on_prev_month(ctx.link().callback(|_| Msg::PrevMonth))
                        .on_next_month(ctx.link().callback(|_| Msg::NextMonth))
                        .on_select(ctx.link().callback(Msg::DateSelected)),
                ),
            );

        let slider_section = Frame::new()
            .title("Volume")
            .border(true)
            .height(Length::Flex(1))
            .child(
                Slider::new(self.volume)
                    .min(0.0)
                    .max(100.0)
                    .step(5.0)
                    .label("Vol")
                    .on_change(ctx.link().callback(Msg::VolumeChanged)),
            );

        const ITEMS: &[&str] = &[
            "Rust",
            "Go",
            "Zig",
            "TypeScript",
            "Python",
            "Kotlin",
            "Swift",
            "Elixir",
            "OCaml",
            "Clojure",
        ];

        let combo_section = Frame::new()
            .title("Language")
            .border(true)
            .height(Length::Flex(1))
            .child(
                VStack::new()
                    .gap(1)
                    .child(Text::new("Search languages:").style(Style::new().bold()))
                    .child(
                        ComboBox::new()
                            .items(ITEMS.iter().copied())
                            .query(self.combo_query.clone())
                            .open(self.combo_open)
                            .active_index(self.combo_active)
                            .selected(self.combo_selected)
                            .allow_custom_value(true)
                            .placeholder("Start typing...")
                            .match_input_width(true)
                            .list_height(Length::Px(6))
                            .on_query_change(ctx.link().callback(Msg::ComboQueryChanged))
                            .on_open_change(ctx.link().callback(Msg::ComboOpenChanged))
                            .on_active_index_change(ctx.link().callback(Msg::ComboHighlightChanged))
                            .on_commit(ctx.link().callback(Msg::ComboCommit)),
                    )
                    .child(Text::new(format!("Status: {}", self.combo_status))),
            );

        Grid::new()
            .uniform_columns(2)
            .gap(1)
            .child(radio_section)
            .child(select_section)
            .child(date_section)
            .child(slider_section)
            .child(combo_section)
            .width(Length::Flex(1))
            .height(Length::Flex(1))
            .into()
    }

    fn update(&mut self, msg: Self::Message, _ctx: &mut Context<Self>) -> Update {
        match msg {
            Msg::ThemeChanged(idx) => {
                self.theme = idx;
                Update::full()
            }
            Msg::CountryToggle(expanded) => {
                self.country_expanded = expanded;
                Update::full()
            }
            Msg::CountrySelect(idx) => {
                self.country_idx = Some(idx);
                Update::full()
            }
            Msg::CountryChange(idx) => {
                self.country_idx = Some(idx);
                Update::full()
            }
            Msg::VolumeChanged(val) => {
                self.volume = val;
                Update::full()
            }
            Msg::PrevMonth => {
                let (year, month) = prev_month(self.year, self.month);
                self.year = year;
                self.month = month;
                let max_day = days_in_month(year, month);
                if self.day > max_day {
                    self.day = max_day;
                }
                Update::full()
            }
            Msg::NextMonth => {
                let (year, month) = next_month(self.year, self.month);
                self.year = year;
                self.month = month;
                let max_day = days_in_month(year, month);
                if self.day > max_day {
                    self.day = max_day;
                }
                Update::full()
            }
            Msg::DateSelected(ev) => {
                self.year = ev.year;
                self.month = ev.month;
                self.day = ev.day;
                Update::full()
            }
            Msg::ComboQueryChanged(query) => {
                self.combo_query = query.to_string();
                Update::full()
            }
            Msg::ComboOpenChanged(open) => {
                self.combo_open = open;
                Update::full()
            }
            Msg::ComboHighlightChanged(index) => {
                self.combo_active = index;
                Update::full()
            }
            Msg::ComboCommit(event) => {
                self.combo_selected = event.index;
                self.combo_query = event.value.to_string();
                self.combo_open = false;
                self.combo_status = if event.from_custom_value {
                    format!("Committed custom value: {}", event.value)
                } else {
                    format!(
                        "Committed item #{}: {}",
                        event.index.unwrap_or(0),
                        event.value
                    )
                };
                Update::full()
            }
        }
    }
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn prev_month(year: i32, month: u32) -> (i32, u32) {
    if month == 1 {
        (year - 1, 12)
    } else {
        (year, month - 1)
    }
}

fn next_month(year: i32, month: u32) -> (i32, u32) {
    if month == 12 {
        (year + 1, 1)
    } else {
        (year, month + 1)
    }
}

fn main() -> Result<()> {
    App::new()
        .title("tui-lipan - Forms Demo")
        .mount(FormsDemo {
            theme: 1,
            country_idx: None,
            country_expanded: false,
            year: 2026,
            month: 2,
            day: 5,
            volume: 50.0,
            combo_query: String::new(),
            combo_open: false,
            combo_active: None,
            combo_selected: None,
            combo_status: "Type to filter, Enter to commit".to_string(),
        })
        .run()
}
